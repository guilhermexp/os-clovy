//! fal image generation, plus the fal queue transport shared with
//! [`crate::fal_image_edit`] and [`crate::fal_video`].
//!
//! Every fal model runs behind one queue protocol
//! (<https://fal.ai/docs/documentation/model-apis/inference/queue>):
//! `POST {base}/{endpoint}` submits a job and returns its `request_id`. The
//! run is then read under its app, `{owner}/{alias}`, without the endpoint's
//! subpath, as fal's own client does; the queue answers 405 to a read under
//! the full endpoint. `GET {base}/{app}/requests/{id}/status` reports
//! `IN_QUEUE`, `IN_PROGRESS` or `COMPLETED` (with `error`/`error_type` when the
//! run failed), and `GET {base}/{app}/requests/{id}` returns the model output,
//! or the model's error body with its HTTP status. The endpoint id is the
//! submit path (checked against each model's queue `OpenAPI` document). Each
//! model has its own input and output schema, so every adapter keeps a curated
//! table of the endpoints it maps faithfully and rejects any other model id
//! before calling fal: a request never chooses an arbitrary queue path.
//!
//! Generation runs the whole queue lifecycle inside one leg budget, the bound
//! the image hold arithmetic assumes (config `lib.rs`). fal returns the image
//! inline as a data URI (`sync_mode`), or as a link on its CDN that is
//! downloaded without credentials. Either way the bytes are capped, their real
//! format is sniffed, and an output fal's checker flagged (`has_nsfw_concepts`,
//! which fal answers with a black image) is a rejection, never a success.
//! Billing lives in `ImageService`; this provider only picks the user's own
//! fal key or Clovy's service key.

use crate::retry;
use async_trait::async_trait;
use base64::Engine as _;
use clovy_config::UpstreamConfig;
use clovy_domain::{
    DomainError, GeneratedImage, ImageGenerationRequest, ImageGenerator, ProviderCredentials,
};
use reqwest::{StatusCode, header::AUTHORIZATION};
use serde::{Deserialize, Serialize, Serializer, de::DeserializeOwned};
use std::time::Duration;

/// Provider label written into fal results.
pub const PROVIDER_NAME: &str = "fal";

/// Curated text-to-image endpoints. All three accept the input fields this
/// adapter sends (`image_size`, `sync_mode`, `output_format`,
/// `enable_safety_checker`) and report `has_nsfw_concepts`. Prices per
/// megapixel from each model page (fetched 2026-10-08): `fal-ai/flux/dev`
/// $0.025, `fal-ai/flux/schnell` $0.003, `fal-ai/qwen-image` $0.02.
const GENERATION_ENDPOINTS: [&str; 3] = [
    "fal-ai/flux/dev",
    "fal-ai/flux/schnell",
    "fal-ai/qwen-image",
];

/// The container requested from fal for every image, so the inline result is
/// a PNG and the mime the frontend wraps into a data URL is predictable.
pub(crate) const IMAGE_OUTPUT_FORMAT: &str = "png";
/// Largest image accepted from fal: the cap Clovy applies to an edit source,
/// so every generated image can be edited again.
const MAX_IMAGE_BYTES: u64 = clovy_config::IMAGE_EDIT_SOURCE_MAX_BYTES as u64;
const MAX_IMAGE_BASE64_LEN: usize = clovy_config::IMAGE_EDIT_SOURCE_MAX_BYTES.div_ceil(3) * 4;
/// Image result JSON cap: the inline base64 image plus room for the envelope.
const MAX_IMAGE_RESULT_BYTES: u64 = MAX_IMAGE_BASE64_LEN as u64 + 1024 * 1024;
/// Cap for queue bookkeeping bodies: submit, status, error and video result.
pub(crate) const MAX_QUEUE_JSON_BYTES: u64 = 1024 * 1024;
const IMAGE_POLL_INTERVAL: Duration = Duration::from_millis(500);
/// fal's model API credential scheme: `Authorization: Key <key>`.
const AUTH_SCHEME: &str = "Key";
/// Server-side deadline for a run to start (fal platform headers).
const START_TIMEOUT_HEADER: &str = "X-Fal-Request-Start-Timeout";
/// fal may otherwise reroute a failing run to an "equivalent" endpoint with
/// its own price; Clovy prices per model, so the run must stay on the model
/// that was priced (<https://fal.ai/docs/documentation/model-apis/inference/reliability>).
const DISABLE_FALLBACK_HEADER: &str = "x-app-fal-disable-fallback";
const CONTENT_POLICY_ERROR: &str = "content_policy_violation";
const FILE_TOO_LARGE_ERROR: &str = "file_too_large";
const DATA_URI_PREFIX: &str = "data:";
const MAX_REQUEST_ID_LEN: usize = 64;

/// What went wrong with one fal call, classified so each adapter maps it to
/// its own consumer-visible reason.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FalError {
    /// fal refused this input (400/422 validation, unreadable image, no media).
    Rejected,
    /// fal's (or a partner's) content checker flagged the input or the output.
    ContentPolicy,
    /// An input file is over fal's limit (413 or `file_too_large`).
    PayloadTooLarge,
    /// fal refused the user's own key.
    UserKeyRejected,
    /// With Clovy's key: the request or its result does not exist (404/410
    /// on a queue read). On a public media download: the file is gone.
    NotFound,
    /// With a user's key: a queue read answered 404/410. fal documents a 404
    /// only as "No request exists with that ID" (cancel) and no 410 at all, so
    /// a key of another account cannot be told apart from an expired request:
    /// this is not a terminal expiry.
    RequestInaccessible,
    /// Rate limit or capacity (429/503).
    Busy,
    /// Transport, 5xx, service-key auth, malformed body or exhausted budget.
    Upstream { retryable: bool },
    /// The asset is larger than the cap.
    TooLarge,
    /// The run completed without a usable asset.
    MediaUnavailable,
    /// The run completed with an error that is neither the input's fault nor
    /// a content verdict.
    GenerationFailed,
}

impl FalError {
    /// Worth another attempt later: rate limits, capacity, transient transport.
    pub(crate) fn is_transient(self) -> bool {
        matches!(self, Self::Busy | Self::Upstream { retryable: true })
    }
}

/// The credential one fal job runs under. A job is polled with the key that
/// submitted it, so the choice is made once per job.
#[derive(Clone, Copy)]
pub(crate) struct FalKey<'a> {
    value: &'a str,
    user_supplied: bool,
}

impl<'a> FalKey<'a> {
    /// A user's own key, or `None` when it is blank.
    pub(crate) fn user(value: &'a str) -> Option<Self> {
        let value = value.trim();
        (!value.is_empty()).then_some(Self {
            value,
            user_supplied: true,
        })
    }

    fn header_value(self) -> String {
        format!("{AUTH_SCHEME} {}", self.value)
    }
}

/// Where generated media may be downloaded from. fal serves outputs from its
/// public CDN (`https://v3.fal.media/...`, queue docs); any other location is
/// refused rather than fetched from inside Clovy API.
enum MediaPolicy {
    FalCdn,
    #[cfg(test)]
    Origin(String),
}

impl MediaPolicy {
    fn allows(&self, url: &reqwest::Url) -> bool {
        match self {
            Self::FalCdn => is_fal_cdn_url(url),
            #[cfg(test)]
            Self::Origin(origin) => url.as_str().starts_with(origin.as_str()),
        }
    }
}

fn is_fal_cdn_url(url: &reqwest::Url) -> bool {
    url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && url.port().is_none()
        && url
            .host_str()
            .is_some_and(|host| host == "fal.media" || host.ends_with(".fal.media"))
}

/// HTTP transport for fal's queue API, shared by the image, edit and video
/// adapters.
pub(crate) struct FalQueue {
    http: reqwest::Client,
    /// Media downloads use a client that never follows redirects, so a
    /// redirect out of fal's CDN cannot become a request Clovy API makes for
    /// fal. `None` only if that client could not be built; downloads then
    /// fail closed.
    media_http: Option<reqwest::Client>,
    api_key: String,
    base_url: String,
    media_policy: MediaPolicy,
}

impl FalQueue {
    pub(crate) fn from_config(
        http: reqwest::Client,
        config: &UpstreamConfig,
        media_timeout: Duration,
    ) -> Self {
        let media_http = match reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(media_timeout)
            .build()
        {
            Ok(client) => Some(client),
            Err(error) => {
                // A client-builder error carries configuration state only:
                // no request, body or credential exists yet.
                tracing::error!(%error, "fal: media download client could not be built");
                None
            }
        };
        Self {
            http,
            media_http,
            api_key: config.api_key.clone(),
            base_url: config.base_url.trim().trim_end_matches('/').to_string(),
            media_policy: MediaPolicy::FalCdn,
        }
    }

    /// Lets tests serve media from the mock server instead of fal's CDN.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn with_media_origin(mut self, origin: &str) -> Self {
        self.media_policy = MediaPolicy::Origin(origin.to_string());
        self
    }

    /// Clovy's own fal key.
    pub(crate) fn service_key(&self) -> FalKey<'_> {
        FalKey {
            value: &self.api_key,
            user_supplied: false,
        }
    }

    /// The user's own fal key when they supplied one, else Clovy's key. No
    /// other provider's credential is ever sent to fal.
    pub(crate) fn key_for<'a>(&'a self, credentials: &'a ProviderCredentials) -> FalKey<'a> {
        credentials
            .fal_api_key
            .as_deref()
            .and_then(FalKey::user)
            .unwrap_or_else(|| self.service_key())
    }

    /// Submits one run and returns fal's request id, checked to be a plain
    /// identifier before it is ever put into a URL. Never repeated: fal
    /// documents no idempotency for a new submit (its automatic retries act
    /// inside one `request_id`), so a second POST after a lost answer could
    /// start a second paid run.
    pub(crate) async fn submit<B>(&self, submission: &Submission<'_, B>) -> Result<String, FalError>
    where
        B: Serialize + Sync + ?Sized,
    {
        let url = format!("{}/{}", self.base_url, submission.endpoint);
        let response = self
            .http
            .post(&url)
            .header(AUTHORIZATION, submission.key.header_value())
            .header(
                START_TIMEOUT_HEADER,
                submission.start_timeout.as_secs().max(1).to_string(),
            )
            .header(DISABLE_FALLBACK_HEADER, "true")
            .json(submission.input)
            .send()
            .await
            .map_err(|error| transport_error(&error, &url))?;
        ensure_not_redirected(&response, &url)?;
        let response = ensure_success(response, submission.key, &url).await?;
        let wire: SubmitWire = read_json(response, &url).await?;
        let request_id = wire.request_id.trim();
        if !is_valid_request_id(request_id) {
            tracing::error!(%url, "fal: submit returned an unusable request id");
            return Err(FalError::Upstream { retryable: false });
        }
        Ok(request_id.to_string())
    }

    /// One status poll.
    pub(crate) async fn poll(&self, request: &QueuedRequest<'_>) -> Result<QueueState, FalError> {
        let url = request.status_url(&self.base_url);
        let response = self.get(&url, request.key).await?;
        let response = ensure_success(response, request.key, &url).await?;
        let wire: StatusWire = read_json(response, &url).await?;
        wire.into_state(&url)
    }

    /// Fetches a finished run's output. A run fal reports as failed never
    /// yields output, even when the result endpoint answers 200; its error
    /// body only refines why it failed.
    pub(crate) async fn completed_output<T: DeserializeOwned>(
        &self,
        request: &QueuedRequest<'_>,
        completion: &Completion,
        max_bytes: u64,
    ) -> Result<T, FalError> {
        if let Completion::Failed {
            error_type: Some(error_type),
        } = completion
            && error_type == CONTENT_POLICY_ERROR
        {
            return Err(FalError::ContentPolicy);
        }
        let url = request.result_url(&self.base_url);
        let response = self
            .get(&url, request.key)
            .await
            .map_err(|error| completion.settle(error))?;
        let response = ensure_success(response, request.key, &url)
            .await
            .map_err(|error| completion.settle(error))?;
        if let Completion::Failed { error_type } = completion {
            tracing::warn!(
                %url,
                error_type = error_type.as_deref().unwrap_or("none"),
                "fal: run completed with an error"
            );
            return Err(FalError::GenerationFailed);
        }
        let body = read_capped(response, max_bytes).await?;
        serde_json::from_slice(&body).map_err(|error| {
            // Category only: serde's message can quote values from the body.
            tracing::error!(
                %url,
                category = ?error.classify(),
                "fal: completed output does not match the model schema"
            );
            FalError::MediaUnavailable
        })
    }

    /// Downloads a generated file from fal's CDN: never with credentials,
    /// never following a redirect, never past `max_bytes`. The URL itself is
    /// a public capability link, so it is not logged.
    pub(crate) async fn download(&self, url: &str, max_bytes: u64) -> Result<Vec<u8>, FalError> {
        let Ok(parsed) = reqwest::Url::parse(url) else {
            tracing::error!("fal: media location is not a URL");
            return Err(FalError::MediaUnavailable);
        };
        if !self.media_policy.allows(&parsed) {
            tracing::error!(
                host = parsed.host_str().unwrap_or("none"),
                "fal: media is not on fal's CDN"
            );
            return Err(FalError::MediaUnavailable);
        }
        let Some(client) = &self.media_http else {
            return Err(FalError::Upstream { retryable: false });
        };
        let response = client.get(parsed.as_str()).send().await.map_err(|error| {
            let retryable = retry::is_retryable_transport_error(&error);
            // Transport kind only: the capability URL is stripped, no body exists.
            tracing::error!(error = %error.without_url(), retryable, "fal: media download failed");
            FalError::Upstream { retryable }
        })?;
        let status = response.status();
        if status == StatusCode::NOT_FOUND || status == StatusCode::GONE {
            return Err(FalError::NotFound);
        }
        if !status.is_success() {
            tracing::error!(
                %status,
                host = parsed.host_str().unwrap_or("none"),
                "fal: media download refused"
            );
            return Err(if status.is_redirection() {
                FalError::MediaUnavailable
            } else {
                FalError::Upstream {
                    retryable: retry::is_retryable_status(status),
                }
            });
        }
        read_capped(response, max_bytes).await
    }

    /// One synchronous image leg inside `job.budget`: a single submit, then
    /// status polls and result fetches (idempotent reads of the same
    /// `request_id`, polled again after a transient failure) until the image
    /// is resolved or the budget runs out.
    pub(crate) async fn run_image_job<B>(
        &self,
        job: &ImageJob<'_, B>,
    ) -> Result<ImageAsset, FalError>
    where
        B: Serialize + Sync + ?Sized,
    {
        match tokio::time::timeout(job.budget, self.image_leg(job)).await {
            Ok(result) => result,
            Err(_elapsed) => {
                tracing::error!(endpoint = job.endpoint, "fal image: leg budget exhausted");
                Err(FalError::Upstream { retryable: false })
            }
        }
    }

    async fn image_leg<B>(&self, job: &ImageJob<'_, B>) -> Result<ImageAsset, FalError>
    where
        B: Serialize + Sync + ?Sized,
    {
        let submission = Submission {
            endpoint: job.endpoint,
            input: job.input,
            key: job.key,
            start_timeout: job.budget,
        };
        let request_id = self.submit(&submission).await?;
        let request = QueuedRequest {
            endpoint: job.endpoint,
            request_id: &request_id,
            key: job.key,
        };
        loop {
            let state = match self.poll(&request).await {
                Ok(state) => Some(state),
                Err(error) if error.is_transient() => None,
                Err(error) => return Err(error),
            };
            if let Some(QueueState::Completed(completion)) = state {
                match self
                    .completed_output::<ImagesOutput>(&request, &completion, MAX_IMAGE_RESULT_BYTES)
                    .await
                {
                    Ok(output) => return self.image_asset(output).await,
                    Err(error) if !error.is_transient() => return Err(error),
                    Err(_) => {}
                }
            }
            tokio::time::sleep(IMAGE_POLL_INTERVAL).await;
        }
    }

    async fn image_asset(&self, output: ImagesOutput) -> Result<ImageAsset, FalError> {
        if output
            .has_nsfw_concepts
            .as_deref()
            .and_then(<[bool]>::first)
            == Some(&true)
        {
            return Err(FalError::ContentPolicy);
        }
        let Some(file) = output.images.into_iter().next() else {
            return Err(FalError::MediaUnavailable);
        };
        if file.url.starts_with(DATA_URI_PREFIX) {
            return image_from_data_uri(&file.url);
        }
        let bytes = self.download(&file.url, MAX_IMAGE_BYTES).await?;
        let mime_type = sniff_image(&bytes).ok_or(FalError::MediaUnavailable)?;
        Ok(ImageAsset { bytes, mime_type })
    }

    async fn get(&self, url: &str, key: FalKey<'_>) -> Result<reqwest::Response, FalError> {
        let response = self
            .http
            .get(url)
            .header(AUTHORIZATION, key.header_value())
            .send()
            .await
            .map_err(|error| transport_error(&error, url))?;
        ensure_not_redirected(&response, url)?;
        Ok(response)
    }
}

/// One queue submission.
pub(crate) struct Submission<'a, B: ?Sized> {
    pub(crate) endpoint: &'static str,
    pub(crate) input: &'a B,
    pub(crate) key: FalKey<'a>,
    /// Sent as `X-Fal-Request-Start-Timeout`: fal does not start the run once
    /// this has elapsed in its queue, so no paid run begins after Clovy can
    /// no longer use or charge for it.
    pub(crate) start_timeout: Duration,
}

/// A submitted run, addressed by its endpoint and fal request id.
pub(crate) struct QueuedRequest<'a> {
    pub(crate) endpoint: &'static str,
    pub(crate) request_id: &'a str,
    pub(crate) key: FalKey<'a>,
}

impl QueuedRequest<'_> {
    /// The app a run is read back under: the endpoint without its subpath,
    /// `{owner}/{alias}` (`fal-ai/wan/v2.2-a14b/text-to-video` is read at
    /// `fal-ai/wan`). fal's client also keeps a leading `workflows/` or
    /// `comfy/` namespace; no curated endpoint has one.
    fn app(&self) -> &'static str {
        let endpoint = self.endpoint;
        endpoint
            .match_indices('/')
            .nth(1)
            .map_or(endpoint, |(app_end, _)| &endpoint[..app_end])
    }

    fn status_url(&self, base_url: &str) -> String {
        format!(
            "{base_url}/{}/requests/{}/status",
            self.app(),
            self.request_id
        )
    }

    fn result_url(&self, base_url: &str) -> String {
        format!("{base_url}/{}/requests/{}", self.app(), self.request_id)
    }
}

/// The lifecycle state one status poll reports.
pub(crate) enum QueueState {
    /// `IN_QUEUE` or `IN_PROGRESS`.
    Pending,
    Completed(Completion),
}

/// How a `COMPLETED` run ended.
pub(crate) enum Completion {
    Succeeded,
    Failed { error_type: Option<String> },
}

impl Completion {
    /// Once fal says a run failed, a transient error while reading why does
    /// not make it retryable: the run is over.
    fn settle(&self, error: FalError) -> FalError {
        if matches!(self, Self::Failed { .. }) && error.is_transient() {
            FalError::GenerationFailed
        } else {
            error
        }
    }
}

/// One synchronous image leg.
pub(crate) struct ImageJob<'a, B: ?Sized> {
    pub(crate) endpoint: &'static str,
    pub(crate) input: &'a B,
    pub(crate) key: FalKey<'a>,
    pub(crate) budget: Duration,
}

/// A validated image: its decoded bytes and sniffed mime. Callers encode it
/// once, when they hand it back.
pub(crate) struct ImageAsset {
    pub(crate) bytes: Vec<u8>,
    pub(crate) mime_type: &'static str,
}

/// An inline file input, `data:<mime>;base64,<payload>`. fal accepts data
/// URIs for file inputs (<https://fal.ai/docs/documentation/model-apis/fal-cdn>),
/// which keeps the user's image off fal's public CDN. It is written straight
/// into the request JSON without first building the (up to 67 MB) string.
pub(crate) struct DataUri<'a> {
    pub(crate) mime_type: &'a str,
    pub(crate) base64: &'a str,
}

impl Serialize for DataUri<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&format_args!(
            "data:{};base64,{}",
            self.mime_type, self.base64
        ))
    }
}

/// The mime of a user-supplied source image when it can be embedded in a data
/// URI: an `image/*` type made only of RFC 6838 name characters. An empty value
/// is the HTTP boundary's default, `image/png`.
pub(crate) fn image_source_mime(raw: &str) -> Option<&str> {
    let mime = raw.trim();
    if mime.is_empty() {
        return Some("image/png");
    }
    let (kind, subtype) = mime.split_once('/')?;
    let is_name = !subtype.is_empty()
        && subtype.len() <= 127
        && subtype
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"!#$&^_.+-".contains(&byte));
    (kind.eq_ignore_ascii_case("image") && is_name).then_some(mime)
}

/// fal request ids are UUIDs; anything else never reaches a URL path.
pub(crate) fn is_valid_request_id(request_id: &str) -> bool {
    !request_id.is_empty()
        && request_id.len() <= MAX_REQUEST_ID_LEN
        && request_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

pub(crate) fn invalid_input(reason: &str) -> DomainError {
    DomainError::InvalidInput {
        reason: reason.to_string(),
    }
}

/// Reads a body without ever holding more than `max_bytes` of it.
async fn read_capped(mut response: reqwest::Response, max_bytes: u64) -> Result<Vec<u8>, FalError> {
    if response
        .content_length()
        .is_some_and(|length| length > max_bytes)
    {
        return Err(FalError::TooLarge);
    }
    let capacity = response
        .content_length()
        .and_then(|length| usize::try_from(length).ok())
        .unwrap_or(0);
    let mut bytes = Vec::with_capacity(capacity);
    while let Some(chunk) = response.chunk().await.map_err(|error| {
        // Transport kind only (connection or framing); bytes already read are not rendered.
        tracing::error!(error = %error.without_url(), "fal: reading a response body failed");
        FalError::Upstream { retryable: true }
    })? {
        if bytes.len() as u64 + chunk.len() as u64 > max_bytes {
            return Err(FalError::TooLarge);
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

async fn read_json<T: DeserializeOwned>(
    response: reqwest::Response,
    url: &str,
) -> Result<T, FalError> {
    let body = read_capped(response, MAX_QUEUE_JSON_BYTES)
        .await
        .map_err(|error| match error {
            FalError::TooLarge => FalError::Upstream { retryable: false },
            other => other,
        })?;
    serde_json::from_slice(&body).map_err(|error| {
        // Category only: serde's message can quote values from the body.
        tracing::error!(
            %url,
            category = ?error.classify(),
            "fal: response does not match the queue schema"
        );
        FalError::Upstream { retryable: false }
    })
}

async fn ensure_success(
    response: reqwest::Response,
    key: FalKey<'_>,
    url: &str,
) -> Result<reqwest::Response, FalError> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let body = read_capped(response, MAX_QUEUE_JSON_BYTES)
        .await
        .unwrap_or_default();
    let error_types = error_types(&body);
    tracing::warn!(
        %status,
        %url,
        error_types = %error_types.join(","),
        "fal: non-success response"
    );
    Err(classify(status, &error_types, key.user_supplied))
}

/// A send error from reqwest renders its kind and the queue URL (endpoint
/// and request id; the key travels in a header), never a response body.
fn transport_error(error: &reqwest::Error, url: &str) -> FalError {
    let retryable = retry::is_retryable_transport_error(error);
    tracing::error!(%error, %url, retryable, "fal: transport error");
    FalError::Upstream { retryable }
}

/// A queue call must be answered by the exact URL Clovy built from its
/// configured base, the curated endpoint and a checked request id. A redirect
/// anywhere else is refused; reqwest already drops `Authorization` when a
/// redirect leaves the origin, so the key never follows it.
fn ensure_not_redirected(response: &reqwest::Response, url: &str) -> Result<(), FalError> {
    if reqwest::Url::parse(url).is_ok_and(|expected| &expected == response.url()) {
        Ok(())
    } else {
        tracing::error!(%url, "fal: queue call was redirected");
        Err(FalError::Upstream { retryable: false })
    }
}

/// Machine-readable identifiers in a fal error body: `detail[].type` for model
/// errors (<https://fal.ai/docs/documentation/model-apis/errors>) and
/// `error_type` for request errors
/// (<https://fal.ai/docs/documentation/model-apis/request-errors>). They are
/// fixed API codes and safe to log; the free-form `msg`, `input` and `detail`
/// text, which can echo the prompt, is never kept.
fn error_types(body: &[u8]) -> Vec<String> {
    let Ok(json) = serde_json::from_slice::<serde_json::Value>(body) else {
        return Vec::new();
    };
    let mut types: Vec<String> = json
        .get("detail")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|issue| issue.get("type").and_then(serde_json::Value::as_str))
        .map(str::to_string)
        .collect();
    if let Some(error_type) = json.get("error_type").and_then(serde_json::Value::as_str) {
        types.push(error_type.to_string());
    }
    types
}

fn classify(status: StatusCode, error_types: &[String], user_supplied_key: bool) -> FalError {
    let has = |wanted: &str| error_types.iter().any(|error_type| error_type == wanted);
    if has(CONTENT_POLICY_ERROR) {
        return FalError::ContentPolicy;
    }
    if status == StatusCode::PAYLOAD_TOO_LARGE || has(FILE_TOO_LARGE_ERROR) {
        return FalError::PayloadTooLarge;
    }
    match status {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN if user_supplied_key => {
            FalError::UserKeyRejected
        }
        StatusCode::NOT_FOUND | StatusCode::GONE if user_supplied_key => {
            FalError::RequestInaccessible
        }
        StatusCode::NOT_FOUND | StatusCode::GONE => FalError::NotFound,
        StatusCode::TOO_MANY_REQUESTS | StatusCode::SERVICE_UNAVAILABLE => FalError::Busy,
        StatusCode::BAD_REQUEST | StatusCode::UNPROCESSABLE_ENTITY => FalError::Rejected,
        other => FalError::Upstream {
            retryable: retry::is_retryable_status(other),
        },
    }
}

/// Decodes an inline `data:<mime>;base64,<payload>` image and checks its real
/// format; the decoded bytes are the result.
fn image_from_data_uri(uri: &str) -> Result<ImageAsset, FalError> {
    let Some((header, payload)) = uri.split_once(',') else {
        return Err(FalError::MediaUnavailable);
    };
    if !header.ends_with(";base64") {
        return Err(FalError::MediaUnavailable);
    }
    if payload.len() > MAX_IMAGE_BASE64_LEN {
        return Err(FalError::TooLarge);
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(payload)
        .map_err(|_| FalError::MediaUnavailable)?;
    let mime_type = sniff_image(&bytes).ok_or(FalError::MediaUnavailable)?;
    Ok(ImageAsset { bytes, mime_type })
}

/// The real image container from its magic bytes: PNG, JPEG or `WebP`.
fn sniff_image(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if bytes.get(..4) == Some(&b"RIFF"[..]) && bytes.get(8..12) == Some(&b"WEBP"[..]) {
        Some("image/webp")
    } else {
        None
    }
}

fn generation_error(error: FalError) -> DomainError {
    match error {
        FalError::Rejected | FalError::PayloadTooLarge => {
            invalid_input("image_generation_rejected")
        }
        FalError::ContentPolicy => invalid_input("image_content_violation"),
        FalError::UserKeyRejected => invalid_input("fal_api_key_rejected"),
        FalError::TooLarge => invalid_input("image_too_large"),
        FalError::NotFound
        | FalError::RequestInaccessible
        | FalError::Busy
        | FalError::Upstream { .. }
        | FalError::MediaUnavailable
        | FalError::GenerationFailed => DomainError::UpstreamProvider,
    }
}

/// `ImageGenerator` over fal's queue for the curated text-to-image endpoints.
pub struct FalImageGenerator {
    queue: FalQueue,
    /// End-to-end budget for the whole generate leg (submit, polls, result,
    /// download); the hold TTL and route timeout arithmetic assume this bound
    /// (config `lib.rs`).
    leg_budget: Duration,
}

impl FalImageGenerator {
    pub fn from_config(
        http: reqwest::Client,
        config: &UpstreamConfig,
        leg_budget: Duration,
    ) -> Self {
        Self {
            queue: FalQueue::from_config(http, config, leg_budget),
            leg_budget,
        }
    }
}

#[async_trait]
impl ImageGenerator for FalImageGenerator {
    async fn generate(
        &self,
        request: ImageGenerationRequest,
    ) -> Result<GeneratedImage, DomainError> {
        let prompt = request.prompt.trim();
        if prompt.is_empty() {
            return Err(invalid_input("image_prompt_empty"));
        }
        let Some(endpoint) = GENERATION_ENDPOINTS
            .into_iter()
            .find(|endpoint| *endpoint == request.model.0)
        else {
            return Err(invalid_input("image_model_unsupported"));
        };
        let image_size = match (request.width, request.height) {
            (Some(width), Some(height)) => Some(ImageSize { width, height }),
            (None, None) => None,
            _ => return Err(invalid_input("image_dimensions_incomplete")),
        };
        let input = TextToImageInput {
            prompt,
            image_size,
            num_images: 1,
            output_format: IMAGE_OUTPUT_FORMAT,
            sync_mode: true,
            enable_safety_checker: request.safe_mode,
        };
        let asset = self
            .queue
            .run_image_job(&ImageJob {
                endpoint,
                input: &input,
                key: self.queue.key_for(&request.provider_credentials),
                budget: self.leg_budget,
            })
            .await
            .map_err(generation_error)?;
        Ok(GeneratedImage {
            image_base64: base64::engine::general_purpose::STANDARD.encode(&asset.bytes),
            mime_type: asset.mime_type.to_string(),
            model: request.model.0,
            provider: PROVIDER_NAME.to_string(),
        })
    }
}

#[derive(Serialize)]
struct TextToImageInput<'a> {
    prompt: &'a str,
    /// `None` keeps the model default (`landscape_4_3`, 1024x768).
    #[serde(skip_serializing_if = "Option::is_none")]
    image_size: Option<ImageSize>,
    num_images: u8,
    output_format: &'static str,
    /// The image comes back inline as a data URI instead of a CDN link.
    sync_mode: bool,
    /// `None` keeps the model default (on). fal honors `false` only for
    /// accounts authorized to disable the checker; others are always checked.
    #[serde(skip_serializing_if = "Option::is_none")]
    enable_safety_checker: Option<bool>,
}

/// fal's `ImageSize` object: explicit output pixels.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct ImageSize {
    pub(crate) width: u32,
    pub(crate) height: u32,
}

#[derive(Deserialize)]
struct SubmitWire {
    request_id: String,
}

#[derive(Deserialize)]
struct StatusWire {
    status: String,
    #[serde(default)]
    error: Option<serde_json::Value>,
    #[serde(default)]
    error_type: Option<String>,
}

impl StatusWire {
    fn into_state(self, url: &str) -> Result<QueueState, FalError> {
        match self.status.as_str() {
            "IN_QUEUE" | "IN_PROGRESS" => Ok(QueueState::Pending),
            "COMPLETED" => {
                let failed = self.error.as_ref().is_some_and(|error| !error.is_null())
                    || self.error_type.is_some();
                Ok(QueueState::Completed(if failed {
                    Completion::Failed {
                        error_type: self.error_type,
                    }
                } else {
                    Completion::Succeeded
                }))
            }
            _ => {
                // The unknown value itself comes from the body; not logged.
                tracing::error!(%url, "fal: unknown queue status");
                Err(FalError::Upstream { retryable: false })
            }
        }
    }
}

/// The output shape shared by every curated image endpoint.
#[derive(Deserialize)]
struct ImagesOutput {
    #[serde(default)]
    images: Vec<FalFile>,
    #[serde(default)]
    has_nsfw_concepts: Option<Vec<bool>>,
}

/// fal's `File` object.
#[derive(Deserialize)]
pub(crate) struct FalFile {
    pub(crate) url: String,
    #[serde(default)]
    pub(crate) file_size: Option<u64>,
}

/// A stand-in for fal's queue host. Like the real queue, it answers 405 to a
/// read addressed under an endpoint's subpath (`{owner}/{alias}/{path}/requests/...`);
/// tests mount the routes they exercise on top.
#[cfg(test)]
pub(crate) async fn queue_server() -> wiremock::MockServer {
    use wiremock::{Mock, MockServer, Request, ResponseTemplate, matchers::method};

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(|request: &Request| {
            let path = request.url.path();
            path.find("/requests/")
                .is_some_and(|app_end| path[..app_end].matches('/').count() > 2)
        })
        .respond_with(ResponseTemplate::new(405))
        .mount(&server)
        .await;
    server
}

#[cfg(test)]
mod tests {
    use super::{FalImageGenerator, FalQueue, is_fal_cdn_url, queue_server};
    use crate::http;
    use base64::Engine as _;
    use clovy_config::UpstreamConfig;
    use clovy_domain::{
        DomainError, ImageGenerationRequest, ImageGenerator, ModelId, ProviderCredentials,
    };
    use pretty_assertions::assert_eq;
    use serde_json::json;
    use std::time::Duration;
    use wiremock::{
        Mock, MockServer, Request, ResponseTemplate,
        matchers::{body_partial_json, header, method, path},
    };

    const SUBMIT: &str = "/fal-ai/flux/dev";
    const STATUS: &str = "/fal-ai/flux/requests/req-1/status";
    const RESULT: &str = "/fal-ai/flux/requests/req-1";
    const PNG: &[u8] = b"\x89PNG\r\n\x1a\nimage-bytes";

    fn generator(server: &MockServer, leg_budget: Duration) -> FalImageGenerator {
        let config = UpstreamConfig {
            api_key: "service_key".to_string(),
            base_url: server.uri(),
        };
        FalImageGenerator {
            queue: FalQueue::from_config(http::default_client(), &config, leg_budget)
                .with_media_origin(&server.uri()),
            leg_budget,
        }
    }

    fn request(model: &str) -> ImageGenerationRequest {
        ImageGenerationRequest {
            prompt: "a red bicycle".to_string(),
            model: ModelId(model.to_string()),
            width: None,
            height: None,
            safe_mode: None,
            provider_credentials: ProviderCredentials::default(),
        }
    }

    fn data_uri(bytes: &[u8]) -> String {
        format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(bytes)
        )
    }

    async fn mount_queue(server: &MockServer, output: serde_json::Value) {
        Mock::given(method("POST"))
            .and(path(SUBMIT))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({ "request_id": "req-1" })),
            )
            .mount(server)
            .await;
        Mock::given(method("GET"))
            .and(path(STATUS))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({ "status": "COMPLETED" })),
            )
            .mount(server)
            .await;
        Mock::given(method("GET"))
            .and(path(RESULT))
            .respond_with(ResponseTemplate::new(200).set_body_json(output))
            .mount(server)
            .await;
    }

    #[tokio::test]
    async fn generates_through_the_queue_and_returns_the_inline_image() {
        let server = queue_server().await;
        Mock::given(method("POST"))
            .and(path(SUBMIT))
            .and(header("authorization", "Key service_key"))
            .and(body_partial_json(json!({
                "prompt": "a red bicycle",
                "image_size": { "width": 1024, "height": 768 },
                "enable_safety_checker": true,
                "sync_mode": true,
                "output_format": "png",
            })))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({ "request_id": "req-1" })),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(STATUS))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "status": "IN_QUEUE" })))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(STATUS))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({ "status": "COMPLETED" })),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(RESULT))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "images": [{ "url": data_uri(PNG), "width": 1024, "height": 768 }],
                "has_nsfw_concepts": [false],
            })))
            .mount(&server)
            .await;
        let mut request = request("fal-ai/flux/dev");
        request.width = Some(1024);
        request.height = Some(768);
        request.safe_mode = Some(true);

        let image = generator(&server, Duration::from_secs(5))
            .generate(request)
            .await
            .expect("generation succeeds");

        assert_eq!(
            image.image_base64,
            base64::engine::general_purpose::STANDARD.encode(PNG)
        );
        assert_eq!(image.mime_type, "image/png");
        assert_eq!(image.model, "fal-ai/flux/dev");
        assert_eq!(image.provider, "fal");
    }

    #[tokio::test]
    async fn a_user_key_runs_the_whole_job() {
        let server = queue_server().await;
        for (verb, route) in [("POST", SUBMIT), ("GET", STATUS), ("GET", RESULT)] {
            let body = match route {
                SUBMIT => json!({ "request_id": "req-1" }),
                STATUS => json!({ "status": "COMPLETED" }),
                _ => json!({ "images": [{ "url": data_uri(PNG) }] }),
            };
            Mock::given(method(verb))
                .and(path(route))
                .and(header("authorization", "Key user_fal_key"))
                .respond_with(ResponseTemplate::new(200).set_body_json(body))
                .mount(&server)
                .await;
        }
        let mut request = request("fal-ai/flux/dev");
        request.provider_credentials = ProviderCredentials {
            fal_api_key: Some("user_fal_key".to_string()),
            ..ProviderCredentials::default()
        };

        let image = generator(&server, Duration::from_secs(5))
            .generate(request)
            .await
            .expect("every queue call carries the user's key");

        assert_eq!(image.mime_type, "image/png");
    }

    #[tokio::test]
    async fn a_flagged_output_is_a_content_rejection_not_a_black_image() {
        let server = queue_server().await;
        mount_queue(
            &server,
            json!({ "images": [{ "url": data_uri(PNG) }], "has_nsfw_concepts": [true] }),
        )
        .await;

        let error = generator(&server, Duration::from_secs(5))
            .generate(request("fal-ai/flux/dev"))
            .await
            .expect_err("a flagged image is not a result");

        assert_eq!(
            error,
            DomainError::InvalidInput {
                reason: "image_content_violation".to_string()
            }
        );
    }

    #[tokio::test]
    async fn a_failed_run_flagged_by_the_content_checker_is_a_content_rejection() {
        let server = queue_server().await;
        Mock::given(method("POST"))
            .and(path(SUBMIT))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({ "request_id": "req-1" })),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(STATUS))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "status": "COMPLETED",
                "error": "flagged",
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(RESULT))
            .respond_with(ResponseTemplate::new(422).set_body_json(json!({
                "detail": [{
                    "loc": ["body", "prompt"],
                    "msg": "flagged",
                    "type": "content_policy_violation",
                    "input": "a red bicycle",
                }]
            })))
            .mount(&server)
            .await;

        let error = generator(&server, Duration::from_secs(5))
            .generate(request("fal-ai/flux/dev"))
            .await
            .expect_err("a flagged run fails");

        assert_eq!(
            error,
            DomainError::InvalidInput {
                reason: "image_content_violation".to_string()
            }
        );
    }

    #[tokio::test]
    async fn an_input_fal_refuses_is_a_usable_bad_request() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(SUBMIT))
            .respond_with(ResponseTemplate::new(422).set_body_json(json!({
                "detail": [{ "loc": ["body", "image_size"], "msg": "too small", "type": "greater_than_equal" }]
            })))
            .mount(&server)
            .await;

        let error = generator(&server, Duration::from_secs(5))
            .generate(request("fal-ai/flux/dev"))
            .await
            .expect_err("a refused input fails");

        assert_eq!(
            error,
            DomainError::InvalidInput {
                reason: "image_generation_rejected".to_string()
            }
        );
    }

    #[tokio::test]
    async fn only_a_refused_user_key_is_reported_as_the_users_key() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(SUBMIT))
            .respond_with(ResponseTemplate::new(401).set_body_json(json!({ "detail": "bad key" })))
            .mount(&server)
            .await;
        let generator = generator(&server, Duration::from_secs(5));
        let mut user_request = request("fal-ai/flux/dev");
        user_request.provider_credentials = ProviderCredentials {
            fal_api_key: Some("user_fal_key".to_string()),
            ..ProviderCredentials::default()
        };

        let user_error = generator
            .generate(user_request)
            .await
            .expect_err("the user's key is refused");
        let service_error = generator
            .generate(request("fal-ai/flux/dev"))
            .await
            .expect_err("Clovy's key is refused");

        assert_eq!(
            user_error,
            DomainError::InvalidInput {
                reason: "fal_api_key_rejected".to_string()
            }
        );
        assert_eq!(service_error, DomainError::UpstreamProvider);
    }

    #[tokio::test]
    async fn a_cdn_image_is_downloaded_without_credentials() {
        let server = queue_server().await;
        mount_queue(
            &server,
            json!({ "images": [{ "url": format!("{}/media/out.png", server.uri()) }] }),
        )
        .await;
        Mock::given(method("GET"))
            .and(path("/media/out.png"))
            .and(|request: &Request| !request.headers.contains_key("authorization"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(PNG.to_vec(), "image/png"))
            .mount(&server)
            .await;

        let image = generator(&server, Duration::from_secs(5))
            .generate(request("fal-ai/flux/dev"))
            .await
            .expect("the CDN image is fetched");

        assert_eq!(
            image.image_base64,
            base64::engine::general_purpose::STANDARD.encode(PNG)
        );
    }

    #[tokio::test]
    async fn media_outside_fals_cdn_or_not_an_image_fails_closed() {
        for output in [
            json!({ "images": [{ "url": "https://example.com/out.png" }] }),
            json!({ "images": [{ "url": data_uri(b"<html>not an image</html>") }] }),
            json!({ "images": [] }),
        ] {
            let server = queue_server().await;
            mount_queue(&server, output).await;

            let error = generator(&server, Duration::from_secs(5))
                .generate(request("fal-ai/flux/dev"))
                .await
                .expect_err("no usable image");

            assert_eq!(error, DomainError::UpstreamProvider);
        }
    }

    #[test]
    fn only_https_fal_cdn_hosts_are_media_sources() {
        for allowed in [
            "https://v3.fal.media/files/rabbit/abc.png",
            "https://fal.media/files/abc.mp4",
            "https://V3B.FAL.MEDIA/files/abc.mp4",
        ] {
            let url = reqwest::Url::parse(allowed).expect("valid URL");
            assert!(is_fal_cdn_url(&url), "{allowed} should be allowed");
        }
        for refused in [
            "http://v3.fal.media/files/abc.png",
            "https://fal.media.example.com/abc.png",
            "https://example.com/v3.fal.media/abc.png",
            "https://user@v3.fal.media/abc.png",
            "https://v3.fal.media:8443/abc.png",
            "https://notfal.media/abc.png",
        ] {
            let url = reqwest::Url::parse(refused).expect("valid URL");
            assert!(!is_fal_cdn_url(&url), "{refused} should be refused");
        }
    }

    #[tokio::test]
    async fn requests_are_checked_before_calling_fal() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&server)
            .await;
        let generator = generator(&server, Duration::from_secs(5));
        let mut one_side = request("fal-ai/flux/dev");
        one_side.width = Some(512);

        let unsupported = generator
            .generate(request("venice-sd35"))
            .await
            .expect_err("not a curated fal model");
        let incomplete = generator
            .generate(one_side)
            .await
            .expect_err("a single dimension is not a size");

        assert_eq!(
            unsupported,
            DomainError::InvalidInput {
                reason: "image_model_unsupported".to_string()
            }
        );
        assert_eq!(
            incomplete,
            DomainError::InvalidInput {
                reason: "image_dimensions_incomplete".to_string()
            }
        );
    }

    #[tokio::test]
    async fn the_leg_budget_bounds_a_run_that_never_finishes() {
        let server = queue_server().await;
        Mock::given(method("POST"))
            .and(path(SUBMIT))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({ "request_id": "req-1" })),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(STATUS))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({ "status": "IN_PROGRESS" })),
            )
            .mount(&server)
            .await;

        let error = generator(&server, Duration::from_secs(1))
            .generate(request("fal-ai/flux/dev"))
            .await
            .expect_err("the leg gives up at its budget");

        assert_eq!(error, DomainError::UpstreamProvider);
    }

    #[tokio::test]
    async fn a_request_id_that_is_not_an_id_never_reaches_a_url() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(SUBMIT))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({ "request_id": "../../admin" })),
            )
            .mount(&server)
            .await;

        let error = generator(&server, Duration::from_secs(5))
            .generate(request("fal-ai/flux/dev"))
            .await
            .expect_err("an unusable id fails the leg");

        assert_eq!(error, DomainError::UpstreamProvider);
        let received = server.received_requests().await.expect("recording is on");
        assert_eq!(received.len(), 1);
    }

    #[tokio::test]
    async fn a_failed_submit_is_never_repeated() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(SUBMIT))
            .respond_with(ResponseTemplate::new(503).set_body_json(json!({
                "detail": "Runner unavailable",
                "error_type": "runner_connection_error",
            })))
            .expect(1)
            .mount(&server)
            .await;

        let error = generator(&server, Duration::from_secs(5))
            .generate(request("fal-ai/flux/dev"))
            .await
            .expect_err("the submit failed");

        assert_eq!(error, DomainError::UpstreamProvider);
    }

    /// Generates through the client the app builds for its upstream and
    /// metered-inference calls (`upstream_client`).
    async fn generate_with_app_client(server: &MockServer) -> DomainError {
        let config = UpstreamConfig {
            api_key: "service_key".to_string(),
            base_url: server.uri(),
        };
        let budget = Duration::from_secs(5);
        let client = http::upstream_client(budget).expect("the upstream client builds");
        FalImageGenerator::from_config(client, &config, budget)
            .generate(request("fal-ai/flux/dev"))
            .await
            .expect_err("a redirect is not fal's answer")
    }

    async fn redirected_submit(server: &MockServer, status: u16, location: &str) -> DomainError {
        Mock::given(method("POST"))
            .and(path(SUBMIT))
            .respond_with(ResponseTemplate::new(status).insert_header("location", location))
            .mount(server)
            .await;
        generate_with_app_client(server).await
    }

    async fn calls(server: &MockServer) -> Vec<String> {
        server
            .received_requests()
            .await
            .expect("recording is on")
            .iter()
            .map(|request| format!("{} {}", request.method, request.url.path()))
            .collect()
    }

    #[tokio::test]
    async fn a_redirect_to_another_host_is_never_followed() {
        for status in [307, 308] {
            let elsewhere = MockServer::start().await;
            let server = MockServer::start().await;
            let location = format!("{}/stolen", elsewhere.uri());

            let error = redirected_submit(&server, status, &location).await;

            assert_eq!(error, DomainError::UpstreamProvider, "{status}");
            assert_eq!(calls(&server).await, [format!("POST {SUBMIT}")], "{status}");
            assert_eq!(calls(&elsewhere).await, Vec::<String>::new(), "{status}");
        }
    }

    #[tokio::test]
    async fn a_redirect_on_the_same_host_never_resends_the_submit() {
        for status in [307, 308] {
            let server = MockServer::start().await;

            let error = redirected_submit(&server, status, "/stolen").await;

            assert_eq!(error, DomainError::UpstreamProvider, "{status}");
            assert_eq!(calls(&server).await, [format!("POST {SUBMIT}")], "{status}");
        }
    }

    #[tokio::test]
    async fn a_redirected_status_poll_is_never_followed() {
        for status in [307, 308] {
            let elsewhere = MockServer::start().await;
            let server = queue_server().await;
            Mock::given(method("POST"))
                .and(path(SUBMIT))
                .respond_with(
                    ResponseTemplate::new(200).set_body_json(json!({ "request_id": "req-1" })),
                )
                .mount(&server)
                .await;
            let location = format!("{}/stolen", elsewhere.uri());
            Mock::given(method("GET"))
                .and(path(STATUS))
                .respond_with(
                    ResponseTemplate::new(status).insert_header("location", location.as_str()),
                )
                .mount(&server)
                .await;

            let error = generate_with_app_client(&server).await;

            assert_eq!(error, DomainError::UpstreamProvider, "{status}");
            assert_eq!(calls(&elsewhere).await, Vec::<String>::new(), "{status}");
        }
    }
}
