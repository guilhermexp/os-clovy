//! fal video generation and image animation over the shared queue transport
//! in [`crate::fal_image`].
//!
//! Video is an asynchronous job. `queue`/`queue_animation` submit one run and
//! hand back fal's request id as the job's provider handle; `retrieve` is one
//! status poll that, once fal reports `COMPLETED`, fetches the result, downloads
//! the video from fal's CDN (credential-free, no redirects, capped at
//! `max_response_bytes`, declared `file_size` checked first) and checks it really
//! is MP4 or `WebM` before returning the bytes. A run that completed with an
//! error, or without a usable video, is a terminal failure, never a completion.
//! Billing (quote, credits, authorize, charge once) lives in `VideoService`.
//!
//! A job runs under one fal account for its whole life. `queue` uses the user's
//! own fal key when the request carries one (BYOK) and Clovy's key otherwise;
//! `retrieve` polls with exactly the account the service names for that job and
//! never falls back to the other one. The user's key is never stored with the
//! job: a BYOK poll brings it again.
//!
//! fal has no quote endpoint and its pricing API returns one unit price per
//! endpoint, which cannot express per-resolution tiers, so `quote` computes the
//! documented list price locally: the model's per-second rate at the requested
//! resolution times the billed seconds. Anything the model cannot price or run
//! (unknown model, `Auto` duration, unsupported resolution, aspect ratio, audio
//! or negative prompt) is rejected there, before the wallet is touched.
//!
//! Curated models (model pages fetched 2026-10-08):
//! - `fal-ai/wan/v2.2-a14b/text-to-video` and `.../image-to-video`: 1 to 10 s
//!   rendered as `16 * s + 1` frames at 16 fps; 480p/580p/720p at
//!   $0.04/$0.06/$0.08 per video second, "calculated at 16 frames per second";
//!   negative prompt supported, no audio
//!   (<https://fal.ai/models/fal-ai/wan/v2.2-a14b/text-to-video/llms.txt>).
//! - `alibaba/wan-3.0/text-to-video` and `.../image-to-video`: 2 to 30 s;
//!   480p/720p/1080p at $0.05/$0.10/$0.20 per second; optional generated
//!   audio, no negative prompt
//!   (<https://fal.ai/models/alibaba/wan-3.0/text-to-video/llms.txt>).
//!
//! A text-to-video model only serves `queue` and an image-to-video model only
//! serves `queue_animation`, so an animation is never a text-to-video run in
//! disguise.

use crate::fal_image::{
    DataUri, FalError, FalKey, FalQueue, MAX_QUEUE_JSON_BYTES, QueueState, QueuedRequest,
    Submission, image_source_mime, invalid_input, is_valid_request_id,
};
use async_trait::async_trait;
use clovy_config::UpstreamConfig;
use clovy_domain::{
    DomainError, VideoAnimationRequest, VideoGenerationRequest, VideoProvider, VideoQueued,
    VideoQuoteRequest, VideoRetrieved,
};
use serde::{Deserialize, Serialize};
use std::{ops::RangeInclusive, time::Duration};

/// fal will not start a run that waited longer than the video job budget in
/// its queue: past it the hold may expire, so the run could never be charged.
const VIDEO_START_TIMEOUT: Duration = Duration::from_secs(clovy_config::DEFAULT_VIDEO_JOB_MAX_SECS);
/// Wan 2.2 renders and bills at 16 frames per second.
const WAN22_FRAMES_PER_SECOND: u32 = 16;

static VIDEO_MODELS: [VideoModel; 4] = [
    VideoModel {
        endpoint: "fal-ai/wan/v2.2-a14b/text-to-video",
        action: VideoAction::Generate,
        family: Family::Wan22,
    },
    VideoModel {
        endpoint: "fal-ai/wan/v2.2-a14b/image-to-video",
        action: VideoAction::Animate,
        family: Family::Wan22,
    },
    VideoModel {
        endpoint: "alibaba/wan-3.0/text-to-video",
        action: VideoAction::Generate,
        family: Family::Wan30,
    },
    VideoModel {
        endpoint: "alibaba/wan-3.0/image-to-video",
        action: VideoAction::Animate,
        family: Family::Wan30,
    },
];

/// USD per billed second by resolution tier.
static WAN22_PRICES: [(&str, f64); 3] = [("480p", 0.04), ("580p", 0.06), ("720p", 0.08)];
static WAN30_PRICES: [(&str, f64); 3] = [("480p", 0.05), ("720p", 0.10), ("1080p", 0.20)];
static WAN22_GENERATE_RATIOS: [&str; 3] = ["16:9", "9:16", "1:1"];
static WAN22_ANIMATE_RATIOS: [&str; 4] = ["auto", "16:9", "9:16", "1:1"];
static WAN30_RATIOS: [&str; 6] = ["adaptive", "16:9", "4:3", "1:1", "3:4", "9:16"];

/// `VideoProvider` over fal's queue for the curated video endpoints.
pub struct FalVideoProvider {
    queue: FalQueue,
    /// Budget for one call: a submit, or one poll with its result and
    /// download. The whole job is bounded by the video hold TTL in
    /// clovy-config, not here.
    timeout: Duration,
    max_response_bytes: u64,
}

impl FalVideoProvider {
    pub fn from_config(
        http: reqwest::Client,
        config: &UpstreamConfig,
        timeout: Duration,
        max_response_bytes: u64,
    ) -> Self {
        Self {
            queue: FalQueue::from_config(http, config, timeout),
            timeout,
            max_response_bytes,
        }
    }

    /// One submit, never retried: a retry could start a second paid run.
    async fn submit(
        &self,
        model: &VideoModel,
        input: &VideoInput<'_>,
        key: FalKey<'_>,
    ) -> Result<VideoQueued, DomainError> {
        let submission = Submission {
            endpoint: model.endpoint,
            input,
            key,
            start_timeout: VIDEO_START_TIMEOUT,
        };
        match tokio::time::timeout(self.timeout, self.queue.submit(&submission)).await {
            Ok(Ok(provider_request_id)) => Ok(VideoQueued {
                provider_request_id,
                download_url: None,
            }),
            Ok(Err(error)) => Err(queue_error(error)),
            Err(_elapsed) => {
                tracing::error!(endpoint = model.endpoint, "fal video: submit timed out");
                Err(DomainError::UpstreamProvider)
            }
        }
    }

    async fn retrieve_once(&self, request: &QueuedRequest<'_>) -> Result<VideoRetrieved, FalError> {
        let QueueState::Completed(completion) = self.queue.poll(request).await? else {
            // fal reports a queue position, not timing estimates: zero means
            // unknown, the same convention the job state already uses.
            return Ok(VideoRetrieved::Processing {
                average_execution_ms: 0,
                execution_ms: 0,
            });
        };
        let output: VideoOutput = self
            .queue
            .completed_output(request, &completion, MAX_QUEUE_JSON_BYTES)
            .await?;
        let Some(file) = output.video else {
            return Err(FalError::MediaUnavailable);
        };
        if file
            .file_size
            .is_some_and(|size| size > self.max_response_bytes)
        {
            return Err(FalError::TooLarge);
        }
        let bytes = self
            .queue
            .download(&file.url, self.max_response_bytes)
            .await?;
        let mime_type = sniff_video(&bytes).ok_or(FalError::MediaUnavailable)?;
        Ok(VideoRetrieved::CompletedBytes {
            bytes,
            mime_type: mime_type.to_string(),
        })
    }
}

#[async_trait]
impl VideoProvider for FalVideoProvider {
    async fn quote(&self, request: VideoQuoteRequest) -> Result<f64, DomainError> {
        let Some(model) = video_model(&request.model.0) else {
            return Err(invalid_input("video_model_unsupported"));
        };
        let plan = model.plan(&VideoOptions {
            duration: &request.duration,
            resolution: request.resolution.as_deref(),
            aspect_ratio: request.aspect_ratio.as_deref(),
            audio: request.audio,
            negative_prompt: None,
        })?;
        Ok(plan.quote_usd())
    }

    async fn queue(&self, request: VideoGenerationRequest) -> Result<VideoQueued, DomainError> {
        let prompt = non_empty_prompt(&request.prompt)?;
        let model = model_for(&request.model.0, VideoAction::Generate)?;
        let plan = model.plan(&VideoOptions {
            duration: &request.duration,
            resolution: request.resolution.as_deref(),
            aspect_ratio: request.aspect_ratio.as_deref(),
            audio: request.audio,
            negative_prompt: request.negative_prompt.as_deref(),
        })?;
        let key = self.queue.key_for(&request.provider_credentials);
        self.submit(model, &plan.input(prompt, None), key).await
    }

    async fn queue_animation(
        &self,
        request: VideoAnimationRequest,
    ) -> Result<VideoQueued, DomainError> {
        let prompt = non_empty_prompt(&request.prompt)?;
        let image = request.image_base64.trim();
        if image.is_empty() {
            return Err(invalid_input("video_source_empty"));
        }
        let Some(mime_type) = image_source_mime(&request.mime_type) else {
            return Err(invalid_input("video_source_unsupported"));
        };
        let model = model_for(&request.model.0, VideoAction::Animate)?;
        let plan = model.plan(&VideoOptions {
            duration: &request.duration,
            resolution: request.resolution.as_deref(),
            aspect_ratio: request.aspect_ratio.as_deref(),
            audio: request.audio,
            negative_prompt: request.negative_prompt.as_deref(),
        })?;
        let source = DataUri {
            mime_type,
            base64: image,
        };
        let key = self.queue.key_for(&request.provider_credentials);
        self.submit(model, &plan.input(prompt, Some(source)), key)
            .await
    }

    /// `user_fal_key` names the job's account: `None` for a job Clovy queued
    /// with its own key, `Some` for a BYOK job, polled with that key only.
    async fn retrieve(
        &self,
        model: &str,
        provider_request_id: &str,
        user_fal_key: Option<&str>,
    ) -> Result<VideoRetrieved, DomainError> {
        let Some(video_model) = video_model(model) else {
            return Err(invalid_input("video_model_unsupported"));
        };
        if !is_valid_request_id(provider_request_id) {
            tracing::error!(model, "fal video: stored handle is not a fal request id");
            return Err(invalid_input("video_media_unavailable"));
        }
        let key = match user_fal_key {
            None => self.queue.service_key(),
            Some(raw) => FalKey::user(raw).ok_or_else(|| invalid_input("fal_api_key_required"))?,
        };
        let request = QueuedRequest {
            endpoint: video_model.endpoint,
            request_id: provider_request_id,
            key,
        };
        match tokio::time::timeout(self.timeout, self.retrieve_once(&request)).await {
            Ok(result) => result.map_err(retrieve_error),
            Err(_elapsed) => {
                tracing::error!(model, "fal video: retrieve timed out");
                Err(DomainError::UpstreamProvider)
            }
        }
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum VideoAction {
    Generate,
    Animate,
}

#[derive(Clone, Copy)]
enum Family {
    Wan22,
    Wan30,
}

impl Family {
    fn seconds(self) -> RangeInclusive<u32> {
        match self {
            Self::Wan22 => 1..=10,
            Self::Wan30 => 2..=30,
        }
    }

    fn price_tiers(self) -> &'static [(&'static str, f64)] {
        match self {
            Self::Wan22 => &WAN22_PRICES,
            Self::Wan30 => &WAN30_PRICES,
        }
    }

    fn default_resolution(self) -> &'static str {
        match self {
            Self::Wan22 => "720p",
            Self::Wan30 => "1080p",
        }
    }

    fn aspect_ratios(self, action: VideoAction) -> &'static [&'static str] {
        match (self, action) {
            (Self::Wan22, VideoAction::Generate) => &WAN22_GENERATE_RATIOS,
            (Self::Wan22, VideoAction::Animate) => &WAN22_ANIMATE_RATIOS,
            (Self::Wan30, _) => &WAN30_RATIOS,
        }
    }

    fn has_audio(self) -> bool {
        matches!(self, Self::Wan30)
    }

    fn has_negative_prompt(self) -> bool {
        matches!(self, Self::Wan22)
    }
}

struct VideoModel {
    endpoint: &'static str,
    action: VideoAction,
    family: Family,
}

impl VideoModel {
    /// Validates the request against what this model can run and price.
    fn plan<'a>(&self, options: &VideoOptions<'a>) -> Result<VideoPlan<'a>, DomainError> {
        let family = self.family;
        let seconds = parse_seconds(options.duration)
            .filter(|seconds| family.seconds().contains(seconds))
            .ok_or_else(|| invalid_input("video_duration_unsupported"))?;
        let wanted = non_blank(options.resolution).unwrap_or(family.default_resolution());
        let &(resolution, usd_per_second) = family
            .price_tiers()
            .iter()
            .find(|(tier, _)| tier.eq_ignore_ascii_case(wanted))
            .ok_or_else(|| invalid_input("video_resolution_unsupported"))?;
        let aspect_ratio = non_blank(options.aspect_ratio)
            .map(|wanted| {
                family
                    .aspect_ratios(self.action)
                    .iter()
                    .copied()
                    .find(|ratio| *ratio == wanted)
                    .ok_or_else(|| invalid_input("video_aspect_ratio_unsupported"))
            })
            .transpose()?;
        if options.audio == Some(true) && !family.has_audio() {
            return Err(invalid_input("video_audio_unsupported"));
        }
        let negative_prompt = non_blank(options.negative_prompt);
        if negative_prompt.is_some() && !family.has_negative_prompt() {
            return Err(invalid_input("video_negative_prompt_unsupported"));
        }
        Ok(VideoPlan {
            family,
            seconds,
            resolution,
            usd_per_second,
            aspect_ratio,
            audio: options.audio.filter(|_| family.has_audio()),
            negative_prompt,
        })
    }
}

struct VideoOptions<'a> {
    duration: &'a str,
    resolution: Option<&'a str>,
    aspect_ratio: Option<&'a str>,
    audio: Option<bool>,
    negative_prompt: Option<&'a str>,
}

/// A validated run: everything the submit body and the quote need.
struct VideoPlan<'a> {
    family: Family,
    seconds: u32,
    resolution: &'static str,
    usd_per_second: f64,
    aspect_ratio: Option<&'static str>,
    audio: Option<bool>,
    negative_prompt: Option<&'a str>,
}

impl<'a> VideoPlan<'a> {
    /// Wan 2.2 frame count for the requested seconds (schema range 17..=161).
    fn num_frames(&self) -> u32 {
        self.seconds * WAN22_FRAMES_PER_SECOND + 1
    }

    /// List price in USD for this run.
    fn quote_usd(&self) -> f64 {
        let billed_seconds = match self.family {
            Family::Wan22 => f64::from(self.num_frames()) / f64::from(WAN22_FRAMES_PER_SECOND),
            Family::Wan30 => f64::from(self.seconds),
        };
        billed_seconds * self.usd_per_second
    }

    fn input(&self, prompt: &'a str, source: Option<DataUri<'a>>) -> VideoInput<'a> {
        match self.family {
            Family::Wan22 => VideoInput::Wan22(Wan22Input {
                prompt,
                image_url: source,
                negative_prompt: self.negative_prompt,
                num_frames: self.num_frames(),
                resolution: self.resolution,
                aspect_ratio: self.aspect_ratio,
            }),
            Family::Wan30 => VideoInput::Wan30(Wan30Input {
                prompt,
                start_image_url: source,
                duration: self.seconds,
                resolution: self.resolution,
                aspect_ratio: self.aspect_ratio,
                audio: self.audio,
            }),
        }
    }
}

#[derive(Serialize)]
#[serde(untagged)]
enum VideoInput<'a> {
    Wan22(Wan22Input<'a>),
    Wan30(Wan30Input<'a>),
}

#[derive(Serialize)]
struct Wan22Input<'a> {
    prompt: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    image_url: Option<DataUri<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    negative_prompt: Option<&'a str>,
    num_frames: u32,
    resolution: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    aspect_ratio: Option<&'static str>,
}

#[derive(Serialize)]
struct Wan30Input<'a> {
    prompt: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    start_image_url: Option<DataUri<'a>>,
    duration: u32,
    resolution: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    aspect_ratio: Option<&'static str>,
    /// `None` keeps the model default (audio on).
    #[serde(skip_serializing_if = "Option::is_none")]
    audio: Option<bool>,
}

#[derive(Deserialize)]
struct VideoOutput {
    #[serde(default)]
    video: Option<crate::fal_image::FalFile>,
}

fn video_model(id: &str) -> Option<&'static VideoModel> {
    VIDEO_MODELS.iter().find(|model| model.endpoint == id)
}

fn model_for(id: &str, action: VideoAction) -> Result<&'static VideoModel, DomainError> {
    video_model(id)
        .filter(|model| model.action == action)
        .ok_or_else(|| invalid_input("video_model_unsupported"))
}

fn non_empty_prompt(raw: &str) -> Result<&str, DomainError> {
    let prompt = raw.trim();
    if prompt.is_empty() {
        Err(invalid_input("video_prompt_empty"))
    } else {
        Ok(prompt)
    }
}

fn non_blank(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

/// Whole seconds from `"5s"` or `"5"`. `Auto` and other enum words have no
/// price and no fixed length, so they do not parse.
fn parse_seconds(raw: &str) -> Option<u32> {
    let raw = raw.trim();
    raw.strip_suffix(['s', 'S'])
        .unwrap_or(raw)
        .trim_end()
        .parse()
        .ok()
}

/// The real container from its magic bytes: an ISO media `ftyp` box (MP4) or
/// an EBML header (`WebM`).
fn sniff_video(bytes: &[u8]) -> Option<&'static str> {
    if bytes.get(4..8) == Some(&b"ftyp"[..]) {
        Some("video/mp4")
    } else if bytes.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
        Some("video/webm")
    } else {
        None
    }
}

/// Submit errors keep the reasons `VideoService` already translates; a
/// 429/503 stays `MeteringProvider`, the variant the API maps to a retryable
/// 503.
fn queue_error(error: FalError) -> DomainError {
    match error {
        FalError::Rejected => invalid_input("video_generation_rejected"),
        FalError::ContentPolicy => invalid_input("video_content_violation"),
        FalError::PayloadTooLarge => invalid_input("video_payload_too_large"),
        FalError::UserKeyRejected => invalid_input("fal_api_key_rejected"),
        FalError::Busy => DomainError::MeteringProvider,
        FalError::NotFound
        | FalError::RequestInaccessible
        | FalError::TooLarge
        | FalError::MediaUnavailable
        | FalError::GenerationFailed
        | FalError::Upstream { .. } => DomainError::UpstreamProvider,
    }
}

/// Retrieve errors: content, expiry, size and a failed or empty run are
/// terminal reasons; anything transient stays `UpstreamProvider`, which the
/// service treats as "poll again". With a user's key, a refused key
/// (`fal_api_key_rejected`) or a queue read answering 404/410
/// (`fal_request_inaccessible`) is not terminal: the job keeps its state, is
/// never polled with Clovy's key instead, and its own expiry still applies.
/// A 404/410 on the public video download after a valid result is expiry.
fn retrieve_error(error: FalError) -> DomainError {
    match error {
        FalError::ContentPolicy => invalid_input("video_content_violation"),
        FalError::NotFound => invalid_input("video_media_expired"),
        FalError::RequestInaccessible => invalid_input("fal_request_inaccessible"),
        FalError::TooLarge => invalid_input("video_too_large"),
        FalError::MediaUnavailable => invalid_input("video_media_unavailable"),
        FalError::Rejected | FalError::PayloadTooLarge | FalError::GenerationFailed => {
            invalid_input("video_generation_failed")
        }
        FalError::UserKeyRejected => invalid_input("fal_api_key_rejected"),
        FalError::Busy | FalError::Upstream { .. } => DomainError::UpstreamProvider,
    }
}

#[cfg(test)]
mod tests {
    use super::FalVideoProvider;
    use crate::{
        fal_image::{FalQueue, queue_server},
        http,
    };
    use clovy_config::UpstreamConfig;
    use clovy_domain::{
        DomainError, ModelId, ProviderCredentials, VideoAnimationRequest, VideoGenerationRequest,
        VideoProvider, VideoQueued, VideoQuoteRequest, VideoRetrieved,
    };
    use pretty_assertions::assert_eq;
    use serde_json::json;
    use std::time::Duration;
    use wiremock::{
        Mock, MockServer, Request, ResponseTemplate,
        matchers::{body_partial_json, header, method, path},
    };

    const WAN22_T2V: &str = "fal-ai/wan/v2.2-a14b/text-to-video";
    const WAN30_I2V: &str = "alibaba/wan-3.0/image-to-video";
    const STATUS: &str = "/fal-ai/wan/requests/req-1/status";
    const RESULT: &str = "/fal-ai/wan/requests/req-1";
    const MP4: &[u8] = b"\0\0\0\x18ftypisom\0\0\x02\0isomiso2";

    fn provider(server: &MockServer, max_response_bytes: u64) -> FalVideoProvider {
        let config = UpstreamConfig {
            api_key: "service_key".to_string(),
            base_url: server.uri(),
        };
        let timeout = Duration::from_secs(5);
        FalVideoProvider {
            queue: FalQueue::from_config(http::default_client(), &config, timeout)
                .with_media_origin(&server.uri()),
            timeout,
            max_response_bytes,
        }
    }

    fn invalid(reason: &str) -> DomainError {
        DomainError::InvalidInput {
            reason: reason.to_string(),
        }
    }

    fn quote(model: &str, duration: &str, resolution: Option<&str>) -> VideoQuoteRequest {
        VideoQuoteRequest {
            model: ModelId(model.to_string()),
            duration: duration.to_string(),
            resolution: resolution.map(str::to_string),
            aspect_ratio: Some("16:9".to_string()),
            audio: None,
        }
    }

    fn generation(model: &str) -> VideoGenerationRequest {
        VideoGenerationRequest {
            prompt: "a robot dancing".to_string(),
            model: ModelId(model.to_string()),
            duration: "5s".to_string(),
            resolution: Some("720p".to_string()),
            aspect_ratio: Some("16:9".to_string()),
            audio: None,
            negative_prompt: None,
            provider_credentials: ProviderCredentials::default(),
        }
    }

    fn animation(model: &str) -> VideoAnimationRequest {
        VideoAnimationRequest {
            image_base64: "aGVsbG8=".to_string(),
            mime_type: "image/png".to_string(),
            prompt: "the cat waves".to_string(),
            model: ModelId(model.to_string()),
            duration: "5s".to_string(),
            resolution: Some("720p".to_string()),
            aspect_ratio: None,
            audio: Some(false),
            negative_prompt: None,
            provider_credentials: ProviderCredentials::default(),
        }
    }

    fn assert_usd(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < 1e-9,
            "quoted {actual}, expected {expected}"
        );
    }

    async fn mount_completed(
        server: &MockServer,
        status: serde_json::Value,
        result: ResponseTemplate,
    ) {
        Mock::given(method("GET"))
            .and(path(STATUS))
            .respond_with(ResponseTemplate::new(200).set_body_json(status))
            .mount(server)
            .await;
        Mock::given(method("GET"))
            .and(path(RESULT))
            .respond_with(result)
            .mount(server)
            .await;
    }

    #[tokio::test]
    async fn quotes_the_documented_list_price() {
        let server = MockServer::start().await;
        let provider = provider(&server, 1024);

        let wan22_5s = provider.quote(quote(WAN22_T2V, "5s", Some("720p"))).await;
        let wan22_10s = provider.quote(quote(WAN22_T2V, "10", Some("480p"))).await;
        let wan30_5s = provider
            .quote(quote("alibaba/wan-3.0/text-to-video", "5s", Some("720P")))
            .await;
        let wan30_default = provider.quote(quote(WAN30_I2V, "5s", None)).await;

        // 81 frames / 16 fps = 5.0625 billed seconds at $0.08.
        assert_usd(wan22_5s.expect("priced"), 0.405);
        assert_usd(wan22_10s.expect("priced"), 161.0 / 16.0 * 0.04);
        assert_usd(wan30_5s.expect("priced"), 0.5);
        // No resolution means the model default, 1080p at $0.20.
        assert_usd(wan30_default.expect("priced"), 1.0);
    }

    #[tokio::test]
    async fn the_quote_refuses_what_the_model_cannot_price_or_run() {
        let server = MockServer::start().await;
        let provider = provider(&server, 1024);
        let mut with_audio = quote(WAN22_T2V, "5s", Some("720p"));
        with_audio.audio = Some(true);
        let mut square_ish = quote(WAN22_T2V, "5s", Some("720p"));
        square_ish.aspect_ratio = Some("4:3".to_string());

        let cases = [
            (quote(WAN22_T2V, "Auto", None), "video_duration_unsupported"),
            (quote(WAN22_T2V, "11s", None), "video_duration_unsupported"),
            (
                quote(WAN22_T2V, "5s", Some("1080p")),
                "video_resolution_unsupported",
            ),
            (with_audio, "video_audio_unsupported"),
            (square_ish, "video_aspect_ratio_unsupported"),
            (
                quote("wan-2.2-a14b-text-to-video", "5s", None),
                "video_model_unsupported",
            ),
        ];
        for (request, reason) in cases {
            assert_eq!(provider.quote(request).await, Err(invalid(reason)));
        }
    }

    #[tokio::test]
    async fn generation_submits_the_frames_for_the_duration_and_keeps_fals_id() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/fal-ai/wan/v2.2-a14b/text-to-video"))
            .and(header("authorization", "Key service_key"))
            .and(body_partial_json(json!({
                "prompt": "a robot dancing",
                "num_frames": 81,
                "resolution": "720p",
                "aspect_ratio": "16:9",
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "request_id": "req-1",
                "status_url": "https://queue.fal.run/elsewhere",
            })))
            .mount(&server)
            .await;

        let queued = provider(&server, 1024)
            .queue(generation(WAN22_T2V))
            .await
            .expect("queued");

        assert_eq!(
            queued,
            VideoQueued {
                provider_request_id: "req-1".to_string(),
                download_url: None,
            }
        );
    }

    #[tokio::test]
    async fn animation_sends_the_source_frame_inline_to_an_image_to_video_model() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/alibaba/wan-3.0/image-to-video"))
            .and(body_partial_json(json!({
                "prompt": "the cat waves",
                "start_image_url": "data:image/png;base64,aGVsbG8=",
                "duration": 5,
                "resolution": "720p",
                "audio": false,
            })))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({ "request_id": "req-2" })),
            )
            .mount(&server)
            .await;

        let queued = provider(&server, 1024)
            .queue_animation(animation(WAN30_I2V))
            .await
            .expect("queued");

        assert_eq!(queued.provider_request_id, "req-2");
    }

    #[tokio::test]
    async fn each_model_serves_only_its_own_action() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&server)
            .await;
        let provider = provider(&server, 1024);

        let text_on_animate_model = provider.queue(generation(WAN30_I2V)).await;
        let animate_on_text_model = provider.queue_animation(animation(WAN22_T2V)).await;

        assert_eq!(
            text_on_animate_model,
            Err(invalid("video_model_unsupported"))
        );
        assert_eq!(
            animate_on_text_model,
            Err(invalid("video_model_unsupported"))
        );
    }

    #[tokio::test]
    async fn submit_errors_keep_their_meaning() {
        let cases = [
            (
                422,
                json!({ "detail": [{ "loc": ["body", "prompt"], "msg": "flagged", "type": "content_policy_violation" }] }),
                invalid("video_content_violation"),
            ),
            (
                422,
                json!({ "detail": [{ "loc": ["body", "num_frames"], "msg": "too many", "type": "less_than_equal" }] }),
                invalid("video_generation_rejected"),
            ),
            (
                429,
                json!({ "detail": "slow down" }),
                DomainError::MeteringProvider,
            ),
            (
                401,
                json!({ "detail": "bad key" }),
                DomainError::UpstreamProvider,
            ),
        ];
        for (status, body, expected) in cases {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .respond_with(ResponseTemplate::new(status).set_body_json(body))
                .expect(1)
                .mount(&server)
                .await;

            let error = provider(&server, 1024).queue(generation(WAN22_T2V)).await;

            assert_eq!(error, Err(expected));
        }
    }

    #[tokio::test]
    async fn a_queued_run_reports_processing() {
        let server = queue_server().await;
        Mock::given(method("GET"))
            .and(path(STATUS))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({ "status": "IN_QUEUE", "queue_position": 3 })),
            )
            .mount(&server)
            .await;

        let retrieved = provider(&server, 1024)
            .retrieve(WAN22_T2V, "req-1", None)
            .await
            .expect("still running");

        assert_eq!(
            retrieved,
            VideoRetrieved::Processing {
                average_execution_ms: 0,
                execution_ms: 0,
            }
        );
    }

    #[tokio::test]
    async fn a_completed_run_returns_the_downloaded_video() {
        let server = queue_server().await;
        mount_completed(
            &server,
            json!({ "status": "COMPLETED", "metrics": { "inference_time": 42.0 } }),
            ResponseTemplate::new(200).set_body_json(json!({
                "video": { "url": format!("{}/media/out.mp4", server.uri()), "file_size": MP4.len() }
            })),
        )
        .await;
        Mock::given(method("GET"))
            .and(path("/media/out.mp4"))
            .and(|request: &Request| !request.headers.contains_key("authorization"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(MP4.to_vec(), "video/mp4"))
            .mount(&server)
            .await;

        let retrieved = provider(&server, 1024)
            .retrieve(WAN22_T2V, "req-1", None)
            .await
            .expect("completed");

        assert_eq!(
            retrieved,
            VideoRetrieved::CompletedBytes {
                bytes: MP4.to_vec(),
                mime_type: "video/mp4".to_string(),
            }
        );
    }

    #[tokio::test]
    async fn a_run_completed_with_an_error_is_a_failure_not_a_video() {
        let cases = [
            (
                ResponseTemplate::new(500)
                    .set_body_json(json!({ "detail": "boom", "error_type": "runner_server_error" })),
                invalid("video_generation_failed"),
            ),
            (
                ResponseTemplate::new(422).set_body_json(json!({
                    "detail": [{ "loc": ["body"], "msg": "flagged", "type": "content_policy_violation" }]
                })),
                invalid("video_content_violation"),
            ),
            (
                ResponseTemplate::new(200).set_body_json(json!({ "video": { "url": "x" } })),
                invalid("video_generation_failed"),
            ),
        ];
        for (result, expected) in cases {
            let server = queue_server().await;
            mount_completed(
                &server,
                json!({ "status": "COMPLETED", "error": "Run failed", "error_type": "runner_server_error" }),
                result,
            )
            .await;

            let error = provider(&server, 1024)
                .retrieve(WAN22_T2V, "req-1", None)
                .await;

            assert_eq!(error, Err(expected));
        }
    }

    #[tokio::test]
    async fn a_completion_without_a_usable_video_fails() {
        for case in ["foreign host", "no video", "not a video"] {
            let server = queue_server().await;
            let output = match case {
                "foreign host" => json!({ "video": { "url": "https://example.com/out.mp4" } }),
                "no video" => json!({ "seed": 7 }),
                _ => json!({ "video": { "url": format!("{}/media/page.html", server.uri()) } }),
            };
            mount_completed(
                &server,
                json!({ "status": "COMPLETED" }),
                ResponseTemplate::new(200).set_body_json(output),
            )
            .await;
            Mock::given(method("GET"))
                .and(path("/media/page.html"))
                .respond_with(ResponseTemplate::new(200).set_body_string("<html></html>"))
                .mount(&server)
                .await;

            let error = provider(&server, 1024)
                .retrieve(WAN22_T2V, "req-1", None)
                .await;

            assert_eq!(error, Err(invalid("video_media_unavailable")), "{case}");
        }
    }

    #[tokio::test]
    async fn an_oversized_video_is_refused() {
        let declared = queue_server().await;
        mount_completed(
            &declared,
            json!({ "status": "COMPLETED" }),
            ResponseTemplate::new(200).set_body_json(json!({
                "video": { "url": format!("{}/media/out.mp4", declared.uri()), "file_size": 10 }
            })),
        )
        .await;
        Mock::given(method("GET"))
            .and(path("/media/out.mp4"))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&declared)
            .await;
        let streamed = queue_server().await;
        mount_completed(
            &streamed,
            json!({ "status": "COMPLETED" }),
            ResponseTemplate::new(200).set_body_json(json!({
                "video": { "url": format!("{}/media/out.mp4", streamed.uri()) }
            })),
        )
        .await;
        Mock::given(method("GET"))
            .and(path("/media/out.mp4"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(MP4.to_vec(), "video/mp4"))
            .mount(&streamed)
            .await;

        let declared_error = provider(&declared, 4)
            .retrieve(WAN22_T2V, "req-1", None)
            .await;
        let streamed_error = provider(&streamed, 4)
            .retrieve(WAN22_T2V, "req-1", None)
            .await;

        assert_eq!(declared_error, Err(invalid("video_too_large")));
        assert_eq!(streamed_error, Err(invalid("video_too_large")));
    }

    #[tokio::test]
    async fn an_unknown_request_is_expired_media() {
        let server = queue_server().await;
        Mock::given(method("GET"))
            .and(path(STATUS))
            .respond_with(
                ResponseTemplate::new(404).set_body_json(json!({ "detail": "Not found" })),
            )
            .mount(&server)
            .await;

        let error = provider(&server, 1024)
            .retrieve(WAN22_T2V, "req-1", None)
            .await;

        assert_eq!(error, Err(invalid("video_media_expired")));
    }

    #[tokio::test]
    async fn a_stored_handle_that_is_not_a_fal_id_is_never_requested() {
        let server = MockServer::start().await;

        let error = provider(&server, 1024)
            .retrieve(WAN22_T2V, "../../admin", None)
            .await;

        assert_eq!(error, Err(invalid("video_media_unavailable")));
        let received = server.received_requests().await.expect("recording is on");
        assert!(received.is_empty());
    }

    #[tokio::test]
    async fn a_byok_job_is_queued_and_polled_with_the_users_key_only() {
        let server = queue_server().await;
        Mock::given(method("POST"))
            .and(path("/fal-ai/wan/v2.2-a14b/text-to-video"))
            .and(header("authorization", "Key user_fal_key"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({ "request_id": "req-1" })),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(STATUS))
            .and(header("authorization", "Key user_fal_key"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({ "status": "IN_PROGRESS" })),
            )
            .mount(&server)
            .await;
        let provider = provider(&server, 1024);
        let mut request = generation(WAN22_T2V);
        request.provider_credentials = ProviderCredentials {
            fal_api_key: Some("user_fal_key".to_string()),
            ..ProviderCredentials::default()
        };

        let queued = provider
            .queue(request)
            .await
            .expect("queued on the user's account");
        let retrieved = provider
            .retrieve(WAN22_T2V, &queued.provider_request_id, Some("user_fal_key"))
            .await
            .expect("polled on the user's account");

        assert_eq!(
            retrieved,
            VideoRetrieved::Processing {
                average_execution_ms: 0,
                execution_ms: 0,
            }
        );
    }

    #[tokio::test]
    async fn a_byok_poll_never_falls_back_to_clovys_account() {
        let server = queue_server().await;
        Mock::given(method("GET"))
            .and(path(STATUS))
            .respond_with(ResponseTemplate::new(401).set_body_json(json!({ "detail": "bad key" })))
            .mount(&server)
            .await;
        let provider = provider(&server, 1024);

        let blank = provider.retrieve(WAN22_T2V, "req-1", Some("  ")).await;
        let calls_after_blank = server
            .received_requests()
            .await
            .expect("recording is on")
            .len();
        let refused = provider
            .retrieve(WAN22_T2V, "req-1", Some("revoked_key"))
            .await;

        assert_eq!(blank, Err(invalid("fal_api_key_required")));
        assert_eq!(calls_after_blank, 0);
        assert_eq!(refused, Err(invalid("fal_api_key_rejected")));
    }

    #[tokio::test]
    async fn a_byok_request_fal_will_not_show_is_not_treated_as_expired() {
        for status in [404, 410] {
            let server = queue_server().await;
            Mock::given(method("GET"))
                .and(path(STATUS))
                .and(header("authorization", "Key other_account_key"))
                .respond_with(
                    ResponseTemplate::new(status).set_body_json(json!({ "detail": "Not Found" })),
                )
                .mount(&server)
                .await;

            let error = provider(&server, 1024)
                .retrieve(WAN22_T2V, "req-1", Some("other_account_key"))
                .await;

            assert_eq!(error, Err(invalid("fal_request_inaccessible")), "{status}");
        }
    }

    #[tokio::test]
    async fn a_byok_video_gone_from_the_cdn_is_expired_media() {
        let server = queue_server().await;
        mount_completed(
            &server,
            json!({ "status": "COMPLETED" }),
            ResponseTemplate::new(200).set_body_json(json!({
                "video": { "url": format!("{}/media/out.mp4", server.uri()) }
            })),
        )
        .await;
        Mock::given(method("GET"))
            .and(path("/media/out.mp4"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;

        let error = provider(&server, 1024)
            .retrieve(WAN22_T2V, "req-1", Some("user_fal_key"))
            .await;

        assert_eq!(error, Err(invalid("video_media_expired")));
    }
}
