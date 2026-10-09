//! Metered video generation (ADR 0015). Video is an asynchronous,
//! dynamically priced surface, so it departs from image in three ways:
//!
//! 1. **Quote-derived pricing.** Before authorizing or queueing, the service
//!    resolves the model against a config allowlist (markup per model; a
//!    retired or unknown id is served by the default model, ADR 0021), then
//!    calls the free quote and converts to credits
//!    (`credits = ceil(quote_usd * markup_millis)`), rejecting a quote above the
//!    config ceiling as `price_overflow`.
//! 2. **Async billing.** `generate`/`animate` authorize a hold, quote, queue the
//!    job, and return a Clovy `JobId`. The client polls `status`, which
//!    forwards a retrieve; the charge settles once on the completing poll
//!    inside a spawned, cancellation-safe task.
//! 3. **Handles, not bytes.** The registry stores the provider queue handle plus
//!    the settlement scope, never the mp4 bytes; bytes are fetched on the
//!    completing poll (and any re-poll) and streamed straight to the response.
//!
//! Billing correctness carries over from image wholesale: an unpriced default
//! model is rejected before the wallet or provider is touched; a same-`request_id` retry
//! dedupes to one queue; the settlement key is attempt-unique and NEVER
//! derived from the client `request_id` (the round-3 replay-funded-free-work
//! rule); a completed job charges exactly once and re-serves without recharging;
//! a post-provider charge failure re-charges the SAME settlement key on re-poll.

use crate::{
    charge_flow::{
        AuthorizeParams, ChargeParams, ReleaseHoldParams, authorize_or_deny, charge, clamp_to_cap,
        log_settled, release_hold,
    },
    error::ServiceError,
    util::{served_model, sha256_hex},
};
use clovy_domain::{
    ActionSlug, Credits, DomainError, GeneratedVideo, OsAccountsClient, ProviderCredentials,
    Receipt, UpstreamProvider, UserId, VideoAnimationRequest, VideoGenerationRequest,
    VideoProvider, VideoQueued, VideoQuoteRequest, VideoRetrieved,
};
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex as StdMutex, MutexGuard, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::sync::{Notify, futures::Notified};
use uuid::Uuid;

const VIDEO_PROVIDER_NAME: &str = "fal";
const VIDEO_DEFAULT_MIME: &str = "video/mp4";

/// Clovy's own opaque handle onto a video job. A uuidv7, never the upstream's
/// raw request id.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct JobId(pub String);

/// Markup applied to the live quote for one video model. Mirrors
/// `ImageModelPrice`, but carries a fixed-point-thousandths markup instead of a
/// flat credit figure, since video is quote-priced.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VideoModelPrice {
    /// Markup in thousandths: `2000` = 2.0x.
    pub markup_millis: u32,
}

impl VideoModelPrice {
    pub fn new(markup_millis: u32) -> Self {
        Self { markup_millis }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum VideoFundingMode {
    Clovy,
    UserKey,
}

pub struct VideoServiceDeps {
    pub os_accounts: Arc<dyn OsAccountsClient>,
    pub provider: Arc<dyn VideoProvider>,
    /// Markup and upstream provider per text-to-video model. A model absent here
    /// is a retired or unknown id and falls back to `default_model` (ADR 0021).
    pub pricing: BTreeMap<String, VideoModelPrice>,
    /// Text-to-video model used for a retired or unknown model id. Must be a
    /// key in `pricing`, or such requests fail `model_not_priced`.
    pub default_model: String,
    /// Markup per image-to-video (animate) model, a separate catalog.
    pub animate_pricing: BTreeMap<String, VideoModelPrice>,
    /// Animate model used when a request names none or names a retired one.
    /// Must be a key in `animate_pricing`.
    pub default_animate_model: String,
    /// Defensive per-request credit ceiling. A computed price above this is
    /// rejected `price_overflow` before authorize.
    pub max_credits_per_request: u64,
    pub hold_ttl_seconds: u64,
}

pub struct VideoService {
    os_accounts: Arc<dyn OsAccountsClient>,
    provider: Arc<dyn VideoProvider>,
    pricing: BTreeMap<String, VideoModelPrice>,
    default_model: String,
    animate_pricing: BTreeMap<String, VideoModelPrice>,
    default_animate_model: String,
    max_credits_per_request: u64,
    hold_ttl_seconds: u64,
    registry: VideoJobRegistry,
}

impl VideoService {
    pub fn new(deps: VideoServiceDeps) -> Self {
        Self {
            os_accounts: deps.os_accounts,
            provider: deps.provider,
            pricing: deps.pricing,
            default_model: deps.default_model,
            animate_pricing: deps.animate_pricing,
            default_animate_model: deps.default_animate_model,
            max_credits_per_request: deps.max_credits_per_request,
            hold_ttl_seconds: deps.hold_ttl_seconds,
            registry: VideoJobRegistry::new(),
        }
    }

    /// Queues a text-to-video job: resolve the served model, dedupe, quote,
    /// authorize, queue, and return a `JobId`. See the module docs for the
    /// invariants.
    pub async fn generate(
        &self,
        mut params: VideoGenerateParams,
    ) -> Result<VideoJobHandle, ServiceError> {
        params.model = served_model(&self.pricing, Some(&params.model), &self.default_model);
        let price = self
            .pricing
            .get(&params.model)
            .copied()
            .ok_or(ServiceError::ModelNotPriced)?;
        let funding_mode = if params
            .provider_credentials
            .has_user_key(UpstreamProvider::Fal)
        {
            VideoFundingMode::UserKey
        } else {
            VideoFundingMode::Clovy
        };
        let shape = generate_shape(&params, funding_mode);
        let shape_hash = sha256_hex(shape.as_bytes());
        let job_key = params.request_id.as_ref().map(|request_id| {
            format!(
                "video_generate:{}:{}:{}",
                params.user_id.0, request_id, shape_hash
            )
        });
        let queue_credentials = match funding_mode {
            VideoFundingMode::UserKey => {
                let fal_key = params
                    .provider_credentials
                    .user_key(UpstreamProvider::Fal)
                    .unwrap_or_default();
                ProviderCredentials::for_provider(UpstreamProvider::Fal, fal_key.to_string())
            }
            VideoFundingMode::Clovy => ProviderCredentials::default(),
        };
        let request = VideoGenerationRequest {
            prompt: params.prompt.clone(),
            model: clovy_domain::ModelId(params.model.clone()),
            duration: params.duration.clone(),
            resolution: params.resolution.clone(),
            aspect_ratio: params.aspect_ratio.clone(),
            audio: params.audio,
            negative_prompt: params.negative_prompt.clone(),
            provider_credentials: queue_credentials,
        };
        self.create_job(CreateJobInputs {
            action: ActionSlug::VideoGenerate,
            user_id: params.user_id,
            model: params.model,
            shape_hash,
            job_key,
            price,
            funding_mode,
            queue: QueueRequest::Generate(request),
        })
        .await
    }

    /// Queues an image-to-video (animate) job. Mirrors `generate`, resolving the
    /// default animate model when the request names none (the MCP never does)
    /// or names a retired one.
    pub async fn animate(
        &self,
        params: VideoAnimateParams,
    ) -> Result<VideoJobHandle, ServiceError> {
        let model = served_model(
            &self.animate_pricing,
            params.model.as_deref(),
            &self.default_animate_model,
        );
        let price = self
            .animate_pricing
            .get(&model)
            .copied()
            .ok_or(ServiceError::ModelNotPriced)?;
        let funding_mode = if params
            .provider_credentials
            .has_user_key(UpstreamProvider::Fal)
        {
            VideoFundingMode::UserKey
        } else {
            VideoFundingMode::Clovy
        };
        let shape = animate_shape(&params, &model, funding_mode);
        let shape_hash = sha256_hex(shape.as_bytes());
        let job_key = params.request_id.as_ref().map(|request_id| {
            format!(
                "video_animate:{}:{}:{}",
                params.user_id.0, request_id, shape_hash
            )
        });
        let queue_credentials = match funding_mode {
            VideoFundingMode::UserKey => {
                let fal_key = params
                    .provider_credentials
                    .user_key(UpstreamProvider::Fal)
                    .unwrap_or_default();
                ProviderCredentials::for_provider(UpstreamProvider::Fal, fal_key.to_string())
            }
            VideoFundingMode::Clovy => ProviderCredentials::default(),
        };
        let request = VideoAnimationRequest {
            image_base64: params.image_base64.clone(),
            mime_type: params.mime_type.clone(),
            prompt: params.prompt.clone(),
            model: clovy_domain::ModelId(model.clone()),
            duration: params.duration.clone(),
            resolution: params.resolution.clone(),
            aspect_ratio: params.aspect_ratio.clone(),
            audio: params.audio,
            negative_prompt: params.negative_prompt.clone(),
            provider_credentials: queue_credentials,
        };
        self.create_job(CreateJobInputs {
            action: ActionSlug::VideoAnimate,
            user_id: params.user_id,
            model,
            shape_hash,
            job_key,
            price,
            funding_mode,
            queue: QueueRequest::Animate(request),
        })
        .await
    }

    async fn create_job(&self, inputs: CreateJobInputs) -> Result<VideoJobHandle, ServiceError> {
        if let Some(key) = inputs.job_key.clone() {
            match self.registry.claim_create(key).await {
                CreateClaim::Existing(job_id) => return Ok(VideoJobHandle { job_id }),
                CreateClaim::Failed(error) => return Err(error),
                CreateClaim::Run { guard } => {
                    return match self.run_create(&inputs).await {
                        Ok(job) => {
                            let job_id = job.job_id.clone();
                            guard.commit(job);
                            Ok(VideoJobHandle { job_id })
                        }
                        Err(error) => {
                            guard.fail(&error);
                            Err(error)
                        }
                    };
                }
            }
        }
        let job = self.run_create(&inputs).await?;
        let job_id = job.job_id.clone();
        self.registry.insert_job(job);
        Ok(VideoJobHandle { job_id })
    }

    /// The paid work behind a create: quote -> ceiling -> authorize -> mint the
    /// settlement key -> queue. On a queue failure the hold is released (no
    async fn run_create(&self, inputs: &CreateJobInputs) -> Result<VideoJob, ServiceError> {
        match inputs.funding_mode {
            VideoFundingMode::UserKey => {
                // User-funded create: still calls quote so the provider can
                // validate parameters locally, but skips credit ceiling,
                // authorize, and charge.
                self.provider
                    .quote(quote_request(&inputs.queue))
                    .await
                    .map_err(translate_upstream_error)?;
                let queued = self
                    .queue(&inputs.queue)
                    .await
                    .map_err(translate_upstream_error)?;
                Ok(Self::build_job(inputs, queued, None))
            }
            VideoFundingMode::Clovy => {
                let estimate = self.quote_to_credits(inputs).await?;
                let authorization = authorize_or_deny(AuthorizeParams {
                    os_accounts: self.os_accounts.as_ref(),
                    user_id: inputs.user_id.clone(),
                    action: inputs.action,
                    estimate,
                    hold_ttl_seconds: self.hold_ttl_seconds,
                })
                .await?;
                let settlement_key = format!(
                    "{}:{}:attempt:{}:{}",
                    inputs.action.as_str(),
                    inputs.user_id.0,
                    Uuid::now_v7(),
                    inputs.shape_hash,
                );
                let queued = match self.queue(&inputs.queue).await {
                    Ok(queued) => queued,
                    Err(error) => {
                        release_hold(ReleaseHoldParams {
                            os_accounts: self.os_accounts.as_ref(),
                            action: inputs.action,
                            action_token: authorization.action_token,
                        })
                        .await;
                        return Err(translate_upstream_error(error));
                    }
                };
                let charge_credits = clamp_to_cap(estimate, authorization.cap_credits);
                Ok(Self::build_job(
                    inputs,
                    queued,
                    Some(VideoCharge {
                        action_token: authorization.action_token,
                        credits: charge_credits,
                        settlement_key,
                    }),
                ))
            }
        }
    }

    /// Quote via provider, convert to credits, and enforce the per-request
    /// ceiling — all BEFORE authorize, so a catalog change cannot authorize
    /// an unbounded hold.
    async fn quote_to_credits(&self, inputs: &CreateJobInputs) -> Result<Credits, ServiceError> {
        let quote_usd = self
            .provider
            .quote(quote_request(&inputs.queue))
            .await
            .map_err(translate_upstream_error)?;
        let credits = credits_from_quote(quote_usd, inputs.price.markup_millis)?;
        if credits > self.max_credits_per_request {
            tracing::warn!(
                model = %inputs.model,
                credits,
                ceiling = self.max_credits_per_request,
                "video quote exceeds the per-request credit ceiling"
            );
            return Err(ServiceError::PriceOverflow);
        }
        Ok(Credits(credits))
    }

    async fn queue(&self, request: &QueueRequest) -> Result<VideoQueued, DomainError> {
        match request {
            QueueRequest::Generate(request) => self.provider.queue(request.clone()).await,
            QueueRequest::Animate(request) => self.provider.queue_animation(request.clone()).await,
        }
    }

    fn build_job(
        inputs: &CreateJobInputs,
        queued: VideoQueued,
        charge: Option<VideoCharge>,
    ) -> VideoJob {
        VideoJob {
            job_id: JobId(Uuid::now_v7().to_string()),
            key: inputs.job_key.clone(),
            user_id: inputs.user_id.clone(),
            action: inputs.action,
            model: inputs.model.clone(),
            provider_request_id: queued.provider_request_id,
            download_url: queued.download_url,
            charge,
            funding_mode: inputs.funding_mode,
            created_at: Instant::now(),
            state: VideoJobState::Queued,
        }
    }

    /// Polls a job: forwards a retrieve, and on completion charges exactly
    /// once (on a spawned, cancellation-safe task) then re-serves without
    /// recharging. See the module docs for the charge-once concurrency guard.
    pub async fn status(
        &self,
        user_id: UserId,
        job_id: JobId,
        provider_credentials: ProviderCredentials,
    ) -> Result<VideoStatusOutput, ServiceError> {
        let funding_mode = self.registry.job_funding_mode(&user_id, &job_id)?;
        let user_api_key = match funding_mode {
            VideoFundingMode::UserKey => {
                let Some(fal_key) = provider_credentials.user_key(UpstreamProvider::Fal) else {
                    return Err(ServiceError::InvalidInput {
                        reason: "fal_api_key_required".to_string(),
                    });
                };
                Some(fal_key)
            }
            VideoFundingMode::Clovy => None,
        };
        loop {
            match self.registry.next_status_action(&user_id, &job_id)? {
                StatusAction::Failed { reason } => {
                    return Ok(VideoStatusOutput::Failed { reason });
                }
                StatusAction::Wait { notify } => {
                    notify.notified().await;
                }
                StatusAction::Replay { job } => {
                    // Already Delivered: re-fetch the bytes and serve them WITHOUT
                    // charging again.
                    return self.reserve_completed(&job, user_api_key).await;
                }
                StatusAction::Retrieve { job } => {
                    match self
                        .provider
                        .retrieve(&job.model, &job.provider_request_id, user_api_key)
                        .await
                    {
                        Ok(VideoRetrieved::Processing {
                            average_execution_ms,
                            execution_ms,
                        }) => {
                            self.registry.mark_processing(
                                &job_id,
                                average_execution_ms,
                                execution_ms,
                            );
                            return Ok(VideoStatusOutput::Processing {
                                average_execution_ms,
                                execution_ms,
                            });
                        }
                        Ok(completed) => {
                            // Build/validate the deliverable media BEFORE claiming the charge, so a
                            // completed-but-unusable result (empty url + no bytes) cannot strand the
                            // job in Charging with no settlement task to release waiters.
                            let Ok(video) = generated_video(&job, completed) else {
                                // Upstream reported COMPLETED but returned no usable media:
                                // terminal, no charge (we never entered Charging). Retrying
                                // will not help.
                                let released =
                                    self.registry.mark_failed(&job_id, "media_unavailable");
                                self.release_failed_job_hold(released).await;
                                return Ok(VideoStatusOutput::Failed {
                                    reason: "media_unavailable".to_string(),
                                });
                            };
                            // Retrieve says COMPLETED. Race to own the single charge; a loser loops
                            // and replays the winner's result rather than charging again.
                            match self.registry.claim_charge(&job_id) {
                                ChargeClaim::Won(ticket) => {
                                    return self.settle_completed(ticket, &job, video).await;
                                }
                                ChargeClaim::Retry => {}
                            }
                        }
                        Err(error) => {
                            return self.handle_retrieve_error(&job_id, &error).await;
                        }
                    }
                }
            }
        }
    }

    /// The completing poll won the charge. Spawn the charge on a task that owns
    /// the ledger update so a route-timeout cancel between upstream success and
    /// the charge cannot skip it.
    async fn settle_completed(
        &self,
        ticket: ChargeTicket,
        job: &VideoJobSnapshot,
        video: GeneratedVideo,
    ) -> Result<VideoStatusOutput, ServiceError> {
        let Some(charge) = job.charge.clone() else {
            self.registry.mark_delivered(&ticket);
            return Ok(VideoStatusOutput::Completed(video));
        };
        let handle = spawn_video_charge_settlement(
            self.os_accounts.clone(),
            self.registry.clone(),
            ticket,
            job.user_id.clone(),
            job.action,
            job.model.clone(),
            charge,
        );
        match handle.await {
            Ok(Ok(_receipt)) => Ok(VideoStatusOutput::Completed(video)),
            Ok(Err(error)) => Err(error),
            Err(error) => {
                tracing::warn!(
                    panicked = error.is_panic(),
                    cancelled = error.is_cancelled(),
                    "video settlement task failed"
                );
                Err(ServiceError::MeteringProvider)
            }
        }
    }

    /// A re-poll of a Delivered job: re-fetch the bytes and serve them, never
    /// charging again. If the provider has since reaped the media, surface 404.
    async fn reserve_completed(
        &self,
        job: &VideoJobSnapshot,
        user_api_key: Option<&str>,
    ) -> Result<VideoStatusOutput, ServiceError> {
        match self
            .provider
            .retrieve(&job.model, &job.provider_request_id, user_api_key)
            .await
        {
            Ok(retrieved) => Ok(VideoStatusOutput::Completed(generated_video(
                job, retrieved,
            )?)),
            Err(DomainError::InvalidInput { reason }) if user_key_cannot_reach(&reason) => {
                Err(ServiceError::InvalidInput { reason })
            }
            Err(DomainError::InvalidInput { reason }) if reason == "video_media_expired" => {
                Err(ServiceError::JobNotFound)
            }
            Err(error) => Err(translate_upstream_error(error)),
        }
    }

    async fn handle_retrieve_error(
        &self,
        job_id: &JobId,
        error: &DomainError,
    ) -> Result<VideoStatusOutput, ServiceError> {
        match classify_retrieve_error(error) {
            // A refused user key, or a queue read the key's fal account cannot see
            // (fal answers 404 for a request created under another account, so a
            // rotated key cannot prove the job failed): report it and leave the
            // job alone. It keeps its state, still ages out at its existing
            // deadline, and is never polled with another credential.
            RetrieveOutcome::UserKeyCannotReach { reason } => {
                Err(ServiceError::InvalidInput { reason })
            }
            RetrieveOutcome::ContentRejected { reason } => {
                // Content policy discovered mid-job: mark Failed, no charge,
                // and release the hold.
                let released = self.registry.mark_failed(job_id, &reason);
                self.release_failed_job_hold(released).await;
                Ok(VideoStatusOutput::Failed { reason })
            }
            RetrieveOutcome::MediaExpired => {
                let released = self.registry.mark_failed(job_id, "media_expired");
                self.release_failed_job_hold(released).await;
                Err(ServiceError::JobNotFound)
            }
            RetrieveOutcome::Transient => {
                // Leave the state untouched; report progress so the client keeps
                // polling.
                Ok(self.registry.processing_snapshot(job_id))
            }
            RetrieveOutcome::Fatal => Err(ServiceError::UpstreamProvider),
        }
    }

    /// Releases the hold handed back by `mark_failed`: a Failed job can never
    /// charge, so its unused grant must not keep pinning the user's wallet.
    async fn release_failed_job_hold(&self, released: Option<(ActionSlug, VideoCharge)>) {
        if let Some((action, charge)) = released {
            release_hold(ReleaseHoldParams {
                os_accounts: self.os_accounts.as_ref(),
                action,
                action_token: charge.action_token,
            })
            .await;
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn spawn_video_charge_settlement(
    os_accounts: Arc<dyn OsAccountsClient>,
    registry: VideoJobRegistry,
    ticket: ChargeTicket,
    user_id: UserId,
    action: ActionSlug,
    model: String,
    charge_details: VideoCharge,
) -> tokio::task::JoinHandle<Result<Receipt, ServiceError>> {
    tokio::spawn(async move {
        let result = charge(ChargeParams {
            os_accounts: os_accounts.as_ref(),
            action_token: charge_details.action_token,
            credits: charge_details.credits,
            idempotency_key: charge_details.settlement_key,
        })
        .await;
        match &result {
            Ok(receipt) => {
                log_settled(action, &user_id, &model, receipt);
                registry.mark_delivered(&ticket);
            }
            Err(_) => {
                // Park as retryable: a re-poll re-charges the SAME settlement key
                // (stored on the job), never re-keys under a fresh scope.
                registry.mark_charge_failed(&ticket);
            }
        }
        result
    })
}

/// `credits = ceil(quote_usd * (markup_millis / 1000) * 1000) = ceil(quote_usd *
/// markup_millis)`. Overflow or a non-finite/negative product is a `price_overflow`.
///
/// The product is snapped to 6 decimals before `ceil`: an upstream quote like
/// `$0.11` is not exactly representable in f64, so `0.11 * 2000` computes as
/// `220.00000000000003`, which a naive `ceil` would round up to 221 — a 1-credit
/// overcharge on the money path. Snapping absorbs representation error below
/// 1e-6 while still rounding a genuine fractional credit up.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn credits_from_quote(quote_usd: f64, markup_millis: u32) -> Result<u64, ServiceError> {
    if !quote_usd.is_finite() || quote_usd < 0.0 {
        return Err(ServiceError::UpstreamProvider);
    }
    let product = quote_usd * f64::from(markup_millis);
    let snapped = (product * 1_000_000.0).round() / 1_000_000.0;
    let ceiled = snapped.ceil();
    // Reject anything outside `[0, 2^64)` before the cast; the bounds check makes
    // the `as u64` exact (a non-negative integer strictly below 2^64).
    if !ceiled.is_finite() || !(0.0..18_446_744_073_709_551_616.0).contains(&ceiled) {
        return Err(ServiceError::PriceOverflow);
    }
    Ok(ceiled as u64)
}

fn quote_request(request: &QueueRequest) -> VideoQuoteRequest {
    match request {
        QueueRequest::Generate(request) => VideoQuoteRequest {
            model: request.model.clone(),
            duration: request.duration.clone(),
            resolution: request.resolution.clone(),
            aspect_ratio: request.aspect_ratio.clone(),
            audio: request.audio,
        },
        QueueRequest::Animate(request) => VideoQuoteRequest {
            model: request.model.clone(),
            duration: request.duration.clone(),
            resolution: request.resolution.clone(),
            aspect_ratio: request.aspect_ratio.clone(),
            audio: request.audio,
        },
    }
}

/// Builds the response-facing `GeneratedVideo` from a retrieve outcome, falling
/// back to the queue-time `download_url` for the VPS-backed case.
fn generated_video(
    job: &VideoJobSnapshot,
    retrieved: VideoRetrieved,
) -> Result<GeneratedVideo, ServiceError> {
    match retrieved {
        VideoRetrieved::CompletedBytes { bytes, mime_type } => {
            let size_bytes = Some(bytes.len() as u64);
            Ok(GeneratedVideo {
                bytes: Some(bytes),
                download_url: job.download_url.clone(),
                mime_type,
                model: job.model.clone(),
                provider: VIDEO_PROVIDER_NAME.to_string(),
                size_bytes,
            })
        }
        VideoRetrieved::CompletedUrl { download_url } => {
            let url = if download_url.trim().is_empty() {
                job.download_url.clone()
            } else {
                Some(download_url)
            };
            let url = url.ok_or(ServiceError::UpstreamProvider)?;
            Ok(GeneratedVideo {
                bytes: None,
                download_url: Some(url),
                mime_type: VIDEO_DEFAULT_MIME.to_string(),
                model: job.model.clone(),
                provider: VIDEO_PROVIDER_NAME.to_string(),
                size_bytes: None,
            })
        }
        // A Delivered replay whose retrieve unexpectedly reports PROCESSING:
        // fall back to a stored VPS url if we have one, else fail.
        VideoRetrieved::Processing { .. } => {
            let url = job
                .download_url
                .clone()
                .ok_or(ServiceError::UpstreamProvider)?;
            Ok(GeneratedVideo {
                bytes: None,
                download_url: Some(url),
                mime_type: VIDEO_DEFAULT_MIME.to_string(),
                model: job.model.clone(),
                provider: VIDEO_PROVIDER_NAME.to_string(),
                size_bytes: None,
            })
        }
    }
}

/// Translates a queue/quote `DomainError` into a `ServiceError`, mapping the
/// reasoned content-policy codes the provider emits.
fn translate_upstream_error(error: DomainError) -> ServiceError {
    match error {
        DomainError::InvalidInput { reason } => match reason.as_str() {
            "video_generation_failed" => ServiceError::ContentRejected {
                reason: "generation_failed".to_string(),
            },
            "video_media_unavailable" => ServiceError::ContentRejected {
                reason: "media_unavailable".to_string(),
            },
            "video_content_violation" => ServiceError::ContentRejected {
                reason: "content_violation".to_string(),
            },
            "video_needs_consent" => ServiceError::ContentRejected {
                reason: "needs_consent".to_string(),
            },
            "video_model_region_blocked" => ServiceError::ContentRejected {
                reason: "model_region_blocked".to_string(),
            },
            _ => ServiceError::InvalidInput { reason },
        },
        other => other.into(),
    }
}

enum RetrieveOutcome {
    UserKeyCannotReach { reason: String },
    ContentRejected { reason: String },
    MediaExpired,
    Transient,
    Fatal,
}

/// Retrieve reasons that only say the user's own fal key cannot reach the job
/// right now; neither is a verdict on the job itself.
fn user_key_cannot_reach(reason: &str) -> bool {
    matches!(reason, "fal_api_key_rejected" | "fal_request_inaccessible")
}

fn classify_retrieve_error(error: &DomainError) -> RetrieveOutcome {
    match error {
        DomainError::InvalidInput { reason } => match reason.as_str() {
            reason if user_key_cannot_reach(reason) => RetrieveOutcome::UserKeyCannotReach {
                reason: reason.to_string(),
            },
            "video_generation_failed" => RetrieveOutcome::ContentRejected {
                reason: "generation_failed".to_string(),
            },
            "video_media_unavailable" => RetrieveOutcome::ContentRejected {
                reason: "media_unavailable".to_string(),
            },
            "video_content_violation" => RetrieveOutcome::ContentRejected {
                reason: "content_violation".to_string(),
            },
            "video_needs_consent" => RetrieveOutcome::ContentRejected {
                reason: "needs_consent".to_string(),
            },
            "video_model_region_blocked" => RetrieveOutcome::ContentRejected {
                reason: "model_region_blocked".to_string(),
            },
            "video_too_large" => RetrieveOutcome::ContentRejected {
                reason: "video_too_large".to_string(),
            },
            "video_media_expired" => RetrieveOutcome::MediaExpired,
            _ => RetrieveOutcome::Fatal,
        },
        DomainError::MeteringProvider | DomainError::UpstreamProvider => RetrieveOutcome::Transient,
        _ => RetrieveOutcome::Fatal,
    }
}

fn generate_shape(params: &VideoGenerateParams, funding_mode: VideoFundingMode) -> String {
    let mode = match funding_mode {
        VideoFundingMode::Clovy => "clovy",
        VideoFundingMode::UserKey => "user_key",
    };
    serde_json::json!({
        "kind": "generate",
        "funding_mode": mode,
        "prompt": params.prompt,
        "model": params.model,
        "duration": params.duration,
        "resolution": params.resolution,
        "aspect_ratio": params.aspect_ratio,
        "audio": params.audio,
        "negative_prompt": params.negative_prompt,
    })
    .to_string()
}

fn animate_shape(
    params: &VideoAnimateParams,
    model: &str,
    funding_mode: VideoFundingMode,
) -> String {
    let mode = match funding_mode {
        VideoFundingMode::Clovy => "clovy",
        VideoFundingMode::UserKey => "user_key",
    };
    serde_json::json!({
        "kind": "animate",
        "funding_mode": mode,
        "prompt": params.prompt,
        "model": model,
        "image": sha256_hex(params.image_base64.as_bytes()),
        "mime_type": params.mime_type,
        "duration": params.duration,
        "resolution": params.resolution,
        "aspect_ratio": params.aspect_ratio,
        "audio": params.audio,
        "negative_prompt": params.negative_prompt,
    })
    .to_string()
}

struct CreateJobInputs {
    action: ActionSlug,
    user_id: UserId,
    model: String,
    shape_hash: String,
    job_key: Option<String>,
    price: VideoModelPrice,
    funding_mode: VideoFundingMode,
    queue: QueueRequest,
}

enum QueueRequest {
    Generate(VideoGenerationRequest),
    Animate(VideoAnimationRequest),
}

#[derive(Clone)]
struct VideoCharge {
    action_token: String,
    credits: Credits,
    settlement_key: String,
}

#[derive(Clone, Debug)]
pub struct VideoGenerateParams {
    pub user_id: UserId,
    /// Stable per-call id used to dedupe a retried create to one provider queue.
    pub request_id: Option<String>,
    pub prompt: String,
    pub model: String,
    pub duration: String,
    pub resolution: Option<String>,
    pub aspect_ratio: Option<String>,
    pub audio: Option<bool>,
    pub negative_prompt: Option<String>,
    pub provider_credentials: ProviderCredentials,
}

#[derive(Clone, Debug)]
pub struct VideoAnimateParams {
    pub user_id: UserId,
    pub request_id: Option<String>,
    /// Source image as raw base64 (no `data:` prefix).
    pub image_base64: String,
    pub mime_type: String,
    pub prompt: String,
    /// `None` uses the service's default animate model.
    pub model: Option<String>,
    pub duration: String,
    pub resolution: Option<String>,
    pub aspect_ratio: Option<String>,
    pub audio: Option<bool>,
    pub negative_prompt: Option<String>,
    pub provider_credentials: ProviderCredentials,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VideoJobHandle {
    pub job_id: JobId,
}

/// What a status poll reports. On `Completed`, the handler streams the bytes as
/// `video/mp4` (or returns the VPS url) rather than base64-through-JSON.
#[derive(Clone, Debug, PartialEq)]
pub enum VideoStatusOutput {
    Processing {
        average_execution_ms: u64,
        execution_ms: u64,
    },
    Completed(GeneratedVideo),
    Failed {
        reason: String,
    },
}

// --- Registry ----------------------------------------------------------------
//
// KNOWN BOUNDARY (ADR 0015 Decision 3): this registry is per-process memory. A
// Clovy API restart mid-job orphans the upstream job (the poll loop dies, the hold
// expires, the user is not charged, and the provider may still bill Clovy). Accepted
// for the first cut and tied to durable-request-state follow-up #613. The fix is
// NOT to derive the settlement key from the client request id (that reopens the
// round-3 replay-funded-free-work hole) — it is durable job state.
//
// Because jobs store handles, not bytes (ADR 0015 Decision 4), the caps bound
// job count, not gigabytes.
const VIDEO_LEDGER_JOB_TTL: Duration = Duration::from_mins(30);
const VIDEO_LEDGER_CREATING_TTL: Duration = Duration::from_mins(15);
const VIDEO_LEDGER_MAX_JOBS_PER_USER: usize = 32;
const VIDEO_LEDGER_MAX_JOBS_GLOBAL: usize = VIDEO_LEDGER_MAX_JOBS_PER_USER * 4;

#[derive(Clone)]
struct VideoJobRegistry {
    inner: Arc<VideoJobRegistryInner>,
}

#[derive(Default)]
struct VideoJobRegistryInner {
    state: StdMutex<RegistryState>,
    next_owner: AtomicU64,
}

#[derive(Default)]
struct RegistryState {
    jobs: BTreeMap<JobId, VideoJob>,
    /// Dedupe index: request-id-derived job key -> its outcome.
    keys: BTreeMap<String, KeyEntry>,
}

enum KeyEntry {
    Creating {
        notify: Arc<AttemptNotify>,
        owner: u64,
        created_at: Instant,
    },
    Created {
        job_id: JobId,
    },
}

/// What the waiters of a same-request create share: the wakeup and, for a
/// create that failed or was interrupted, its outcome. A waiter attached to
/// such a create returns that outcome; it never takes the request over and
/// queues a second paid job behind the client's back.
#[derive(Default)]
struct AttemptNotify {
    waiters: Notify,
    failure: OnceLock<ServiceError>,
}

impl AttemptNotify {
    fn notified(&self) -> Notified<'_> {
        self.waiters.notified()
    }

    fn notify_waiters(&self) {
        self.waiters.notify_waiters();
    }

    fn fail(&self, error: ServiceError) {
        // A create ends once; its first outcome is the one waiters see.
        let _ = self.failure.set(error);
    }

    fn failure(&self) -> Option<&ServiceError> {
        self.failure.get()
    }
}

struct VideoJob {
    job_id: JobId,
    key: Option<String>,
    user_id: UserId,
    action: ActionSlug,
    model: String,
    provider_request_id: String,
    download_url: Option<String>,
    charge: Option<VideoCharge>,
    funding_mode: VideoFundingMode,
    created_at: Instant,
    state: VideoJobState,
}

enum VideoJobState {
    Queued,
    Processing {
        average_execution_ms: u64,
        execution_ms: u64,
    },
    Charging {
        notify: Arc<Notify>,
        owner: u64,
    },
    /// Completed on the provider but the charge failed; a re-poll re-charges the SAME
    /// settlement key.
    ChargePending,
    /// Charged. A re-poll re-fetches the bytes and re-serves without charging
    /// again.
    Delivered,
    Failed {
        reason: String,
    },
}

/// A lock-free snapshot of the fields a poll needs after releasing the mutex.
struct VideoJobSnapshot {
    user_id: UserId,
    action: ActionSlug,
    model: String,
    provider_request_id: String,
    download_url: Option<String>,
    charge: Option<VideoCharge>,
}

enum CreateClaim {
    Existing(JobId),
    /// The same-request create this claim waited on failed or was interrupted.
    Failed(ServiceError),
    Run {
        guard: VideoCreateGuard,
    },
}

enum StatusAction {
    Failed { reason: String },
    Wait { notify: Arc<Notify> },
    Replay { job: VideoJobSnapshot },
    Retrieve { job: VideoJobSnapshot },
}

enum ChargeClaim {
    Won(ChargeTicket),
    Retry,
}

/// Identifies the single poll that owns a job's charge, so a stale settlement
/// task cannot clobber a newer attempt's state.
#[derive(Clone)]
struct ChargeTicket {
    job_id: JobId,
    owner: u64,
}

struct VideoCreateGuard {
    registry: VideoJobRegistry,
    key: Option<String>,
    owner: u64,
}

impl VideoCreateGuard {
    fn commit(mut self, job: VideoJob) {
        if let Some(key) = self.key.take() {
            self.registry.commit_create(key, self.owner, job);
        }
    }

    fn fail(mut self, error: &ServiceError) {
        if let Some(key) = self.key.take() {
            self.registry.abort_create(&key, self.owner, error.clone());
        }
    }
}

impl Drop for VideoCreateGuard {
    fn drop(&mut self) {
        if let Some(key) = self.key.take() {
            // Dropped mid-create (the request was cancelled): whether the
            // provider queued the job is unknown, so its waiters get the
            // ambiguous upstream error.
            self.registry
                .abort_create(&key, self.owner, ServiceError::UpstreamProvider);
        }
    }
}

impl VideoJobRegistry {
    fn new() -> Self {
        Self {
            inner: Arc::new(VideoJobRegistryInner::default()),
        }
    }

    async fn claim_create(&self, key: String) -> CreateClaim {
        loop {
            let attempt;
            let notified = {
                let mut state = self.state();
                prune_expired(&mut state, Instant::now());
                match state.keys.get(&key) {
                    Some(KeyEntry::Created { job_id }) if state.jobs.contains_key(job_id) => {
                        return CreateClaim::Existing(job_id.clone());
                    }
                    Some(KeyEntry::Created { .. }) => {
                        // The job was evicted/expired; re-create under this key.
                        state.keys.remove(&key);
                        return CreateClaim::Run {
                            guard: self.begin_create(&mut state, key),
                        };
                    }
                    Some(KeyEntry::Creating { notify, .. }) => {
                        attempt = Arc::clone(notify);
                        attempt.notified()
                    }
                    None => {
                        return CreateClaim::Run {
                            guard: self.begin_create(&mut state, key),
                        };
                    }
                }
            };
            notified.await;
            if let Some(error) = attempt.failure() {
                return CreateClaim::Failed(error.clone());
            }
        }
    }

    fn begin_create(&self, state: &mut RegistryState, key: String) -> VideoCreateGuard {
        let owner = self.inner.next_owner.fetch_add(1, Ordering::Relaxed);
        state.keys.insert(
            key.clone(),
            KeyEntry::Creating {
                notify: Arc::new(AttemptNotify::default()),
                owner,
                created_at: Instant::now(),
            },
        );
        VideoCreateGuard {
            registry: self.clone(),
            key: Some(key),
            owner,
        }
    }

    fn commit_create(&self, key: String, owner: u64, job: VideoJob) {
        let notify = {
            let mut state = self.state();
            let owns = matches!(
                state.keys.get(&key),
                Some(KeyEntry::Creating { owner: current, .. }) if *current == owner
            );
            if !owns {
                return;
            }
            let notify = creating_notify(&state, &key);
            let job_id = job.job_id.clone();
            state.jobs.insert(job_id.clone(), job);
            state.keys.insert(key, KeyEntry::Created { job_id });
            evict_over_cap(&mut state);
            notify
        };
        if let Some(notify) = notify {
            notify.notify_waiters();
        }
    }

    /// Ends a create that did not register a job: its waiters get `outcome`.
    fn abort_create(&self, key: &str, owner: u64, outcome: ServiceError) {
        let notify = {
            let mut state = self.state();
            match state.keys.get(key) {
                Some(KeyEntry::Creating { owner: current, .. }) if *current == owner => {
                    let notify = creating_notify(&state, key);
                    state.keys.remove(key);
                    notify
                }
                _ => None,
            }
        };
        if let Some(notify) = notify {
            notify.fail(outcome);
            notify.notify_waiters();
        }
    }

    fn insert_job(&self, job: VideoJob) {
        let mut state = self.state();
        prune_expired(&mut state, Instant::now());
        state.jobs.insert(job.job_id.clone(), job);
        evict_over_cap(&mut state);
    }

    fn next_status_action(
        &self,
        user_id: &UserId,
        job_id: &JobId,
    ) -> Result<StatusAction, ServiceError> {
        let mut state = self.state();
        prune_expired(&mut state, Instant::now());
        let Some(job) = state.jobs.get(job_id) else {
            return Err(ServiceError::JobNotFound);
        };
        // Scope every job to its creator: a guessed/leaked JobId from another
        // user must not retrieve or settle someone else's video.
        if &job.user_id != user_id {
            return Err(ServiceError::JobNotFound);
        }
        Ok(match &job.state {
            VideoJobState::Failed { reason } => StatusAction::Failed {
                reason: reason.clone(),
            },
            VideoJobState::Charging { notify, .. } => StatusAction::Wait {
                notify: notify.clone(),
            },
            VideoJobState::Delivered => StatusAction::Replay { job: snapshot(job) },
            VideoJobState::Queued
            | VideoJobState::Processing { .. }
            | VideoJobState::ChargePending => StatusAction::Retrieve { job: snapshot(job) },
        })
    }

    fn claim_charge(&self, job_id: &JobId) -> ChargeClaim {
        let mut state = self.state();
        let Some(job) = state.jobs.get_mut(job_id) else {
            return ChargeClaim::Retry;
        };
        match job.state {
            VideoJobState::Queued
            | VideoJobState::Processing { .. }
            | VideoJobState::ChargePending => {
                let owner = self.inner.next_owner.fetch_add(1, Ordering::Relaxed);
                job.state = VideoJobState::Charging {
                    notify: Arc::new(Notify::new()),
                    owner,
                };
                ChargeClaim::Won(ChargeTicket {
                    job_id: job_id.clone(),
                    owner,
                })
            }
            VideoJobState::Charging { .. }
            | VideoJobState::Delivered
            | VideoJobState::Failed { .. } => ChargeClaim::Retry,
        }
    }

    fn mark_processing(&self, job_id: &JobId, average_execution_ms: u64, execution_ms: u64) {
        let mut state = self.state();
        if let Some(job) = state.jobs.get_mut(job_id)
            && matches!(
                job.state,
                VideoJobState::Queued | VideoJobState::Processing { .. }
            )
        {
            job.state = VideoJobState::Processing {
                average_execution_ms,
                execution_ms,
            };
        }
    }

    fn mark_delivered(&self, ticket: &ChargeTicket) {
        let notify = {
            let mut state = self.state();
            let Some(job) = state.jobs.get_mut(&ticket.job_id) else {
                return;
            };
            match &job.state {
                VideoJobState::Charging {
                    notify,
                    owner: current,
                } if *current == ticket.owner => {
                    let notify = notify.clone();
                    job.state = VideoJobState::Delivered;
                    Some(notify)
                }
                _ => None,
            }
        };
        if let Some(notify) = notify {
            notify.notify_waiters();
        }
    }

    fn mark_charge_failed(&self, ticket: &ChargeTicket) {
        let notify = {
            let mut state = self.state();
            let Some(job) = state.jobs.get_mut(&ticket.job_id) else {
                return;
            };
            match &job.state {
                VideoJobState::Charging {
                    notify,
                    owner: current,
                } if *current == ticket.owner => {
                    let notify = notify.clone();
                    job.state = VideoJobState::ChargePending;
                    Some(notify)
                }
                _ => None,
            }
        };
        if let Some(notify) = notify {
            notify.notify_waiters();
        }
    }

    /// Fails a non-terminal job and hands back its unused charge (if any) so
    /// the caller can release the hold — a Failed job never enters Charging
    /// (`next_charge_claim` treats Failed as terminal), so the token is dead
    /// weight that would otherwise pin the user's wallet until TTL.
    fn mark_failed(&self, job_id: &JobId, reason: &str) -> Option<(ActionSlug, VideoCharge)> {
        let mut state = self.state();
        let job = state.jobs.get_mut(job_id)?;
        // A job already Charging/Delivered must not be flipped to Failed by a
        // stray retrieve error; only a non-terminal job fails.
        match &job.state {
            VideoJobState::Charging { .. }
            | VideoJobState::Delivered
            | VideoJobState::Failed { .. } => return None,
            VideoJobState::Queued
            | VideoJobState::Processing { .. }
            | VideoJobState::ChargePending => {}
        }
        job.state = VideoJobState::Failed {
            reason: reason.to_string(),
        };
        job.charge.take().map(|charge| (job.action, charge))
    }

    fn processing_snapshot(&self, job_id: &JobId) -> VideoStatusOutput {
        let state = self.state();
        match state.jobs.get(job_id).map(|job| &job.state) {
            Some(VideoJobState::Processing {
                average_execution_ms,
                execution_ms,
            }) => VideoStatusOutput::Processing {
                average_execution_ms: *average_execution_ms,
                execution_ms: *execution_ms,
            },
            _ => VideoStatusOutput::Processing {
                average_execution_ms: 0,
                execution_ms: 0,
            },
        }
    }

    fn state(&self) -> MutexGuard<'_, RegistryState> {
        match self.inner.state.lock() {
            Ok(state) => state,
            Err(poisoned) => {
                tracing::warn!("video job registry mutex was poisoned");
                poisoned.into_inner()
            }
        }
    }

    fn job_funding_mode(
        &self,
        user_id: &UserId,
        job_id: &JobId,
    ) -> Result<VideoFundingMode, ServiceError> {
        let mut state = self.state();
        prune_expired(&mut state, Instant::now());
        let Some(job) = state.jobs.get(job_id) else {
            return Err(ServiceError::JobNotFound);
        };
        if &job.user_id != user_id {
            return Err(ServiceError::JobNotFound);
        }
        Ok(job.funding_mode)
    }
}

fn snapshot(job: &VideoJob) -> VideoJobSnapshot {
    VideoJobSnapshot {
        user_id: job.user_id.clone(),
        action: job.action,
        model: job.model.clone(),
        provider_request_id: job.provider_request_id.clone(),
        download_url: job.download_url.clone(),
        charge: job.charge.clone(),
    }
}

fn creating_notify(state: &RegistryState, key: &str) -> Option<Arc<AttemptNotify>> {
    match state.keys.get(key) {
        Some(KeyEntry::Creating { notify, .. }) => Some(notify.clone()),
        _ => None,
    }
}

fn prune_expired(state: &mut RegistryState, now: Instant) {
    let mut stale_notifies = Vec::new();
    let mut stale_creates = Vec::new();
    state.jobs.retain(|_, job| {
        let keep = now.saturating_duration_since(job.created_at) < VIDEO_LEDGER_JOB_TTL;
        if !keep && let VideoJobState::Charging { notify, .. } = &job.state {
            stale_notifies.push(notify.clone());
        }
        keep
    });
    state.keys.retain(|_, entry| match entry {
        KeyEntry::Creating {
            notify, created_at, ..
        } => {
            let keep = now.saturating_duration_since(*created_at) < VIDEO_LEDGER_CREATING_TTL;
            if !keep {
                stale_creates.push(notify.clone());
            }
            keep
        }
        KeyEntry::Created { job_id } => state.jobs.contains_key(job_id),
    });
    for notify in stale_notifies {
        notify.notify_waiters();
    }
    for notify in stale_creates {
        notify.notify_waiters();
    }
}

fn evict_over_cap(state: &mut RegistryState) {
    // Per-user pressure first, then a global hard backstop. Jobs hold handles,
    // not bytes, so this bounds job count. Eviction wakes a waiter on the
    // removed job (if Charging) so it re-reads JobNotFound rather than hanging.
    loop {
        let mut per_user: std::collections::HashMap<&UserId, usize> =
            std::collections::HashMap::new();
        for job in state.jobs.values() {
            *per_user.entry(&job.user_id).or_default() += 1;
        }
        let Some(over_user) = per_user
            .into_iter()
            .find(|(_, count)| *count > VIDEO_LEDGER_MAX_JOBS_PER_USER)
            .map(|(user, _)| user.clone())
        else {
            break;
        };
        let Some(oldest) = oldest_job_for_user(state, &over_user) else {
            break;
        };
        remove_job(state, &oldest);
    }
    while state.jobs.len() > VIDEO_LEDGER_MAX_JOBS_GLOBAL {
        let Some(oldest) = oldest_job(state) else {
            break;
        };
        remove_job(state, &oldest);
    }
}

fn oldest_job(state: &RegistryState) -> Option<JobId> {
    state
        .jobs
        .values()
        .min_by_key(|job| job.created_at)
        .map(|job| job.job_id.clone())
}

fn oldest_job_for_user(state: &RegistryState, user_id: &UserId) -> Option<JobId> {
    state
        .jobs
        .values()
        .filter(|job| &job.user_id == user_id)
        .min_by_key(|job| job.created_at)
        .map(|job| job.job_id.clone())
}

fn remove_job(state: &mut RegistryState, job_id: &JobId) {
    let Some(job) = state.jobs.remove(job_id) else {
        return;
    };
    if let Some(key) = &job.key
        && matches!(state.keys.get(key), Some(KeyEntry::Created { job_id: id }) if id == job_id)
    {
        state.keys.remove(key);
    }
    if let VideoJobState::Charging { notify, .. } = &job.state {
        notify.notify_waiters();
    }
}

#[cfg(test)]
mod tests;
