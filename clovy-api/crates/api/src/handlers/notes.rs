use crate::{
    audio::validate_audio,
    auth::{authenticated_user, provider_credentials},
    envelope::{self, ApiResponse},
    error::ApiError,
    multipart::MultipartFields,
    state::ApiState,
    validation,
};
use axum::{
    Json,
    body::{Body, Bytes},
    extract::{Multipart, State},
    http::{HeaderMap, StatusCode, header::CONTENT_TYPE},
    response::{IntoResponse, Response},
};
use clovy_domain::{ModelId, ModelKind, ProviderCredentials, UpstreamProvider};
use clovy_services::{
    NoteGenerateOutput, NoteGenerateParams, NoteTranscribeOutput, NoteTranscribeParams,
    PricingError, PricingTable,
};
use serde::{Deserialize, Serialize};
use std::{convert::Infallible, time::Duration};
use tokio::{sync::mpsc, time::MissedTickBehavior};
use tokio_stream::wrappers::UnboundedReceiverStream;

pub(crate) async fn transcribe(
    State(state): State<ApiState>,
    headers: HeaderMap,
    multipart: Multipart,
) -> Result<Json<ApiResponse<TranscribeResponse>>, ApiError> {
    let user_id = authenticated_user(&state, &headers).await?;
    let provider_credentials = provider_credentials(&headers, UpstreamProvider::Groq)?;
    let limits = state.limits();
    let mut form = MultipartFields::collect(multipart, limits.max_audio_bytes).await?;
    let audio = form.required_audio()?;
    validate_audio(&audio)?;
    let requested_model_id = form.required_text("model")?;
    validation::validate_text_len("model", &requested_model_id, validation::MAX_MODEL_CHARS)?;
    let model_id = resolve_priced_asr_model(&state, &requested_model_id)?;
    state.ensure_provider_configured(UpstreamProvider::Groq, &provider_credentials)?;
    // Accepted and validated for wire compatibility, but deliberately not
    // carried into the transcription pipeline: the note title is user data
    // an ASR provider has no business seeing.
    let title = form.required_text("title")?;
    validation::validate_text_len("title", &title, validation::MAX_TITLE_CHARS)?;
    drop(title);
    let context = form.optional_text("context");
    validation::validate_optional_text_len(
        "context",
        context.as_deref(),
        validation::MAX_TRANSCRIPTION_CONTEXT_CHARS,
    )?;
    let language = form.optional_text("language");
    validation::validate_optional_text_len(
        "language",
        language.as_deref(),
        validation::MAX_LANGUAGE_CHARS,
    )?;
    // This client field is not an authorization decision. It only requests
    // live-preview semantics; NoteTranscribeService still probes duration,
    // enforces the preview cap, and gates ASR through OS Accounts.
    let preview = parse_preview_flag(form.optional_text("preview").as_deref());
    // Sent only by builds whose Live transcription setting discloses preview
    // cost; absence keeps the legacy zero-credit preview settlement. Not an
    // authorization decision - billing still flows through OS Accounts.
    let preview_opted_in =
        preview && parse_preview_flag(form.optional_text("previewOptedIn").as_deref());
    let note_id = form.required_text("noteId")?;
    validation::validate_text_len("note_id", &note_id, validation::MAX_ID_CHARS)?;
    let filename = form
        .take_filename()
        .unwrap_or_else(|| "recording.wav".to_string());
    let output = state
        .note_transcribe()
        .transcribe(NoteTranscribeParams {
            user_id,
            note_id,
            audio,
            filename,
            context,
            language,
            model_id: ModelId(model_id),
            preview,
            preview_opted_in,
            provider_credentials,
        })
        .await?;
    Ok(Json(ApiResponse::ok(TranscribeResponse::from(output))))
}

pub(crate) async fn generate(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(request): Json<GenerateRequest>,
) -> Result<Response, ApiError> {
    let user_id = authenticated_user(&state, &headers).await?;
    let provider_credentials = provider_credentials(&headers, UpstreamProvider::Openrouter)?;
    request.validate()?;
    let requested_model_id = required(request.model, "model_required")?;
    validation::validate_text_len("model", &requested_model_id, validation::MAX_MODEL_CHARS)?;
    let (model_id, provider_credentials) = resolve_text_model_and_credentials(
        &state,
        &requested_model_id,
        AutoSelection {
            cost_quality: request.cost_quality,
            requirements: None,
        },
        provider_credentials,
    )?;
    state.ensure_provider_configured(UpstreamProvider::Openrouter, &provider_credentials)?;
    let stream = request.stream;
    let params = NoteGenerateParams {
        user_id,
        note_id: required(request.note_id, "note_id_required")?,
        prompt_version: required(request.prompt_version, "prompt_version_required")?,
        title: request.title,
        transcript: request.transcript,
        transcript_source_labels: request.transcript_source_labels,
        manual_notes: request.manual_notes,
        language: request.language,
        existing_generated_note: request.existing_generated_note,
        model_id: ModelId(model_id),
        provider_credentials,
    };

    if stream {
        return Ok(stream_generate(state, params));
    }

    let output = state.note_generate().generate(params).await?;
    Ok(Json(ApiResponse::ok(GenerateResponse::from(output))).into_response())
}

fn stream_generate(state: ApiState, params: NoteGenerateParams) -> Response {
    // Unbounded + non-blocking sends: a client that stops reading must not
    // suspend this loop mid-send, or the generation timeout below would stop
    // being polled and holds/upstream work could outlive request_timeout_secs.
    // Keep-alives are tiny and the terminal event is a single frame, so the
    // queue cannot grow meaningfully.
    let (tx, rx) = mpsc::unbounded_channel::<Result<Bytes, Infallible>>();
    // Full-route backstop only. `generate()` spans authorize + upstream +
    // charge settlement; a tighter bound would cancel `charge` after Clovy
    // already paid the upstream. The hold-TTL guarantee comes from the layers
    // below: the upstream leg is cut at the metered-inference client timeout
    // (route minus the authorize + settlement budgets), which leaves the
    // settlement budget inside the hold (validate_long_inference_hold_ttl).
    let generation_backstop = Duration::from_secs(state.limits().request_timeout_secs);
    tokio::spawn(async move {
        let generation =
            tokio::time::timeout(generation_backstop, state.note_generate().generate(params));
        tokio::pin!(generation);
        let mut keep_alive = tokio::time::interval(Duration::from_secs(10));
        keep_alive.set_missed_tick_behavior(MissedTickBehavior::Skip);
        keep_alive.tick().await;

        loop {
            // Biased so a disconnect that is already visible wins over a
            // generation that became ready in the same wake: polling the
            // generation first could drive it across its charge call after
            // the client is known gone. (Once the charge request is on the
            // wire nothing here can recall it — bias narrows the race to
            // that unavoidable in-flight window.)
            tokio::select! {
                biased;
                // Cancel the moment the client is gone — not at the next
                // keep-alive tick. Generation is all-or-nothing delivery:
                // dropping the future cancels the upstream call and no charge
                // settles, the same semantics a buffered request has when the
                // connection drops (and deliberately different from agent
                // chat's drain-and-settle, where content was already
                // streamed). Waiting for a tick left a window where a
                // generation completing after the disconnect billed for a
                // result nobody received.
                () = tx.closed() => {
                    break;
                }
                result = &mut generation => {
                    let event = match result {
                        Ok(Ok(output)) => result_event(output).unwrap_or_else(|_| {
                            error_event(ApiError::Internal.response_parts(), None)
                        }),
                        Ok(Err(error)) => {
                            let error = ApiError::from(error);
                            let retry_after = error.retry_after_secs();
                            error_event(error.response_parts(), retry_after)
                        }
                        Err(_elapsed) => error_event(envelope::timeout_response_parts(), None),
                    };
                    let _ = tx.send(Ok(Bytes::from(event)));
                    break;
                }
                _ = keep_alive.tick() => {
                    // Unbounded sends never fail on backpressure; a failure
                    // means the client vanished between closed() polls.
                    // Comment line only — no blank line (same rule as the
                    // chat pump: a blank line dispatches buffered SSE fields).
                    if tx.send(Ok(Bytes::from_static(b": keep-alive\n"))).is_err() {
                        break;
                    }
                }
            }
        }
    });

    (
        StatusCode::OK,
        [(CONTENT_TYPE, "text/event-stream")],
        Body::from_stream(UnboundedReceiverStream::new(rx)),
    )
        .into_response()
}

fn result_event(output: NoteGenerateOutput) -> Result<String, serde_json::Error> {
    let body = ApiResponse::ok(GenerateResponse::from(output));
    Ok(format!(
        "event: result\ndata: {}\n\n",
        serde_json::to_string(&body)?
    ))
}

fn error_event(
    (status, body): (StatusCode, serde_json::Value),
    retry_after_secs: Option<u64>,
) -> String {
    // retry_after_secs carries the buffered path's Retry-After header — an
    // SSE body cannot set response headers, so the hint rides the payload.
    let mut data = serde_json::json!({
        "status": status.as_u16(),
        "body": body,
    });
    if let (Some(secs), Some(object)) = (retry_after_secs, data.as_object_mut()) {
        object.insert("retry_after_secs".to_string(), serde_json::json!(secs));
    }
    format!("event: error\ndata: {data}\n\n")
}

impl GenerateRequest {
    fn validate(&self) -> Result<(), ApiError> {
        if self
            .cost_quality
            .is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value))
        {
            return Err(ApiError::unprocessable("cost_quality_invalid"));
        }
        validation::validate_optional_text_len(
            "note_id",
            self.note_id.as_deref(),
            validation::MAX_ID_CHARS,
        )?;
        validation::validate_optional_text_len(
            "prompt_version",
            self.prompt_version.as_deref(),
            validation::MAX_ID_CHARS,
        )?;
        validation::validate_text_len("title", &self.title, validation::MAX_TITLE_CHARS)?;
        validation::validate_text_len(
            "transcript",
            &self.transcript,
            validation::MAX_NOTE_TRANSCRIPT_CHARS,
        )?;
        validation::validate_optional_text_len(
            "manual_notes",
            self.manual_notes.as_deref(),
            validation::MAX_NOTE_MANUAL_NOTES_CHARS,
        )?;
        validation::validate_optional_text_len(
            "language",
            self.language.as_deref(),
            validation::MAX_LANGUAGE_CHARS,
        )?;
        validation::validate_optional_text_len(
            "existing_generated_note",
            self.existing_generated_note.as_deref(),
            validation::MAX_EXISTING_NOTE_CHARS,
        )?;
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateRequest {
    pub note_id: Option<String>,
    pub prompt_version: Option<String>,
    pub title: String,
    pub transcript: String,
    #[serde(default)]
    pub transcript_source_labels: bool,
    pub manual_notes: Option<String>,
    pub language: Option<String>,
    pub existing_generated_note: Option<String>,
    pub model: Option<String>,
    pub cost_quality: Option<f64>,
    #[serde(default)]
    pub stream: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscribeResponse {
    pub text: String,
    pub language: Option<String>,
    pub provider: String,
    pub credits_charged: u64,
    pub idempotent_replay: bool,
}

impl From<NoteTranscribeOutput> for TranscribeResponse {
    fn from(output: NoteTranscribeOutput) -> Self {
        Self {
            text: output.transcript.text,
            language: output.transcript.language,
            provider: output.transcript.provider,
            credits_charged: output.receipt.credits_charged.0,
            idempotent_replay: output.receipt.idempotent_replay,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateResponse {
    pub content: String,
    pub title_suggestion: Option<String>,
    pub provider: String,
    pub upstream_provider: Option<String>,
    pub privacy_level: Option<String>,
    pub upstream_endpoint: Option<String>,
    pub prompt_version: String,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub credits_charged: u64,
    pub idempotent_replay: bool,
}

impl From<NoteGenerateOutput> for GenerateResponse {
    fn from(output: NoteGenerateOutput) -> Self {
        Self {
            content: output.generated.content,
            title_suggestion: output.generated.title_suggestion,
            provider: output.generated.provider,
            upstream_provider: output.generated.route.provider,
            privacy_level: output.generated.route.privacy_level,
            upstream_endpoint: output.generated.route.endpoint,
            prompt_version: output.prompt_version,
            prompt_tokens: output.generated.usage.prompt_tokens,
            completion_tokens: output.generated.usage.completion_tokens,
            credits_charged: output.receipt.credits_charged.0,
            idempotent_replay: output.receipt.idempotent_replay,
        }
    }
}

pub(crate) fn required(value: Option<String>, message: &str) -> Result<String, ApiError> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ApiError::bad_request(message))
}

pub(crate) const AUTO_TEXT_MODEL: &str = "open-software/auto";
pub(crate) const DEFAULT_ASR_MODEL: &str = "whisper-large-v3-turbo";
pub(crate) const TIER_ECONOMY_MODEL: &str = "nvidia/nemotron-3-nano-30b-a3b";
pub(crate) const TIER_BALANCED_MODEL: &str = "moonshotai/kimi-k2.6";
pub(crate) const TIER_QUALITY_MODEL: &str = "z-ai/glm-5.2";
pub(crate) const PREFERRED_VISION_TEXT_MODEL: &str = "moonshotai/kimi-k2.6";

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct AgentModelRequirements {
    pub(crate) vision: bool,
    pub(crate) tools: bool,
}

impl AgentModelRequirements {
    pub(crate) fn is_empty(self) -> bool {
        !self.vision && !self.tools
    }
}

/// How an Auto request picks its concrete model: the requested quality tier
/// and, for agent chat, the capabilities the request body needs.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct AutoSelection {
    pub(crate) cost_quality: Option<f64>,
    pub(crate) requirements: Option<AgentModelRequirements>,
}

pub(crate) fn model_supports_requirements(
    model: &clovy_config::ModelPriceConfig,
    requirements: AgentModelRequirements,
) -> bool {
    (!requirements.vision || has_capability(&model.capabilities, "supportsvision"))
        && (!requirements.tools
            || has_capability(&model.capabilities, "functioncalling")
            || has_capability(&model.capabilities, "toolcalling"))
}

pub(crate) fn has_capability(capabilities: &[String], expected: &str) -> bool {
    capabilities.iter().any(|capability| {
        capability
            .chars()
            .filter(char::is_ascii_alphabetic)
            .map(|character| character.to_ascii_lowercase())
            .collect::<String>()
            .contains(expected)
    })
}

pub(crate) fn resolve_auto_model(
    pricing: &PricingTable,
    cost_quality: Option<f64>,
    requirements: Option<AgentModelRequirements>,
) -> Result<String, ApiError> {
    let tier_model_id = match cost_quality {
        Some(cq) if cq < 0.34 => TIER_ECONOMY_MODEL,
        Some(cq) if cq <= 0.66 => TIER_BALANCED_MODEL,
        _ => TIER_QUALITY_MODEL,
    };

    let reqs = requirements.unwrap_or_default();
    if !reqs.is_empty() {
        if pricing
            .priced_model(tier_model_id, ModelKind::Text)
            .is_ok_and(|model| model_supports_requirements(&model, reqs))
        {
            return Ok(tier_model_id.to_string());
        }

        if pricing
            .priced_model(PREFERRED_VISION_TEXT_MODEL, ModelKind::Text)
            .is_ok_and(|model| model_supports_requirements(&model, reqs))
        {
            return Ok(PREFERRED_VISION_TEXT_MODEL.to_string());
        }

        if let Some((compatible_id, _)) = pricing.find_priced_model(ModelKind::Text, |id, model| {
            id != AUTO_TEXT_MODEL && model_supports_requirements(model, reqs)
        }) {
            return Ok(compatible_id);
        }

        return Err(ApiError::unprocessable("model_capability_unavailable"));
    }

    if pricing
        .ensure_model_kind(tier_model_id, ModelKind::Text)
        .is_ok()
    {
        return Ok(tier_model_id.to_string());
    }

    if pricing
        .ensure_model_kind(TIER_BALANCED_MODEL, ModelKind::Text)
        .is_ok()
    {
        return Ok(TIER_BALANCED_MODEL.to_string());
    }

    pricing
        .find_priced_model(ModelKind::Text, |id, _| id != AUTO_TEXT_MODEL)
        .map(|(model_id, _)| model_id)
        .ok_or_else(|| ApiError::unprocessable("model_capability_unavailable"))
}

pub(crate) fn resolve_text_model_and_credentials(
    state: &ApiState,
    requested_model_id: &str,
    auto: AutoSelection,
    mut credentials: ProviderCredentials,
) -> Result<(String, ProviderCredentials), ApiError> {
    if requested_model_id == AUTO_TEXT_MODEL {
        credentials.clear(UpstreamProvider::Openrouter);
        let concrete_model =
            resolve_auto_model(state.pricing(), auto.cost_quality, auto.requirements)?;
        return Ok((concrete_model, credentials));
    }

    match state
        .pricing()
        .ensure_model_kind(requested_model_id, ModelKind::Text)
    {
        Ok(()) => Ok((requested_model_id.to_string(), credentials)),
        Err(PricingError::NotPriced | PricingError::MissingRate) => {
            if credentials.has_user_key(UpstreamProvider::Openrouter) {
                return Err(ApiError::unprocessable(
                    "openrouter_api_key_model_unavailable",
                ));
            }
            let concrete_model =
                resolve_auto_model(state.pricing(), auto.cost_quality, auto.requirements)?;
            Ok((concrete_model, credentials))
        }
        Err(PricingError::WrongUnit) => Err(ApiError::unprocessable("model_type_invalid")),
        Err(PricingError::Overflow) => Err(ApiError::unprocessable("price_overflow")),
    }
}

pub(crate) fn resolve_priced_asr_model(
    state: &ApiState,
    requested_model_id: &str,
) -> Result<String, ApiError> {
    match state
        .pricing()
        .ensure_model_kind(requested_model_id, ModelKind::Asr)
    {
        Ok(()) => Ok(requested_model_id.to_string()),
        Err(PricingError::NotPriced | PricingError::MissingRate) => {
            if state
                .pricing()
                .ensure_model_kind(DEFAULT_ASR_MODEL, ModelKind::Asr)
                .is_ok()
            {
                return Ok(DEFAULT_ASR_MODEL.to_string());
            }
            state
                .pricing()
                .find_priced_model(ModelKind::Asr, |_, _| true)
                .map(|(model_id, _)| model_id)
                .ok_or_else(|| ApiError::unprocessable("model_not_priced"))
        }
        Err(PricingError::WrongUnit) => Err(ApiError::unprocessable("model_type_invalid")),
        Err(PricingError::Overflow) => Err(ApiError::unprocessable("price_overflow")),
    }
}

fn parse_preview_flag(value: Option<&str>) -> bool {
    value.is_some_and(|value| matches!(value, "true" | "1"))
}

#[cfg(test)]
mod tests {
    use super::{
        AgentModelRequirements, DEFAULT_ASR_MODEL, TIER_BALANCED_MODEL, TIER_ECONOMY_MODEL,
        TIER_QUALITY_MODEL, parse_preview_flag, resolve_auto_model,
    };
    use clovy_config::{ModelPriceConfig, ModelProvider, ModelType, PriceUnit};
    use clovy_services::PricingTable;
    use std::{collections::BTreeMap, error::Error};

    fn pricing_table() -> PricingTable {
        let mut models = BTreeMap::new();
        models.insert(
            DEFAULT_ASR_MODEL.to_string(),
            ModelPriceConfig {
                unit: PriceUnit::Seconds,
                credits_per_million_seconds: Some(11_112),
                input_credits_per_million_tokens: None,
                output_credits_per_million_tokens: None,
                provider: ModelProvider::Groq,
                model_type: ModelType::Asr,
                display_name: "Whisper Large v3 Turbo".to_string(),
                description: None,
                privacy: None,
                pricing: None,
                context_tokens: None,
                traits: Vec::new(),
                capabilities: Vec::new(),
            },
        );
        models.insert(
            "asr-model".to_string(),
            ModelPriceConfig {
                unit: PriceUnit::Seconds,
                credits_per_million_seconds: Some(1),
                input_credits_per_million_tokens: None,
                output_credits_per_million_tokens: None,
                provider: ModelProvider::Groq,
                model_type: ModelType::Asr,
                display_name: "ASR".to_string(),
                description: None,
                privacy: None,
                pricing: None,
                context_tokens: None,
                traits: Vec::new(),
                capabilities: Vec::new(),
            },
        );
        models.insert(
            TIER_ECONOMY_MODEL.to_string(),
            ModelPriceConfig {
                unit: PriceUnit::Tokens,
                credits_per_million_seconds: None,
                input_credits_per_million_tokens: Some(60),
                output_credits_per_million_tokens: Some(240),
                provider: ModelProvider::Openrouter,
                model_type: ModelType::Text,
                display_name: "Nemotron 3".to_string(),
                description: None,
                privacy: None,
                pricing: None,
                context_tokens: None,
                traits: Vec::new(),
                capabilities: vec!["supportsFunctionCalling".to_string()],
            },
        );
        models.insert(
            TIER_BALANCED_MODEL.to_string(),
            ModelPriceConfig {
                unit: PriceUnit::Tokens,
                credits_per_million_seconds: None,
                input_credits_per_million_tokens: Some(438),
                output_credits_per_million_tokens: Some(2_450),
                provider: ModelProvider::Openrouter,
                model_type: ModelType::Text,
                display_name: "Kimi K2.6".to_string(),
                description: None,
                privacy: None,
                pricing: None,
                context_tokens: None,
                traits: Vec::new(),
                capabilities: vec![
                    "supportsVision".to_string(),
                    "supportsFunctionCalling".to_string(),
                ],
            },
        );
        models.insert(
            TIER_QUALITY_MODEL.to_string(),
            ModelPriceConfig {
                unit: PriceUnit::Tokens,
                credits_per_million_seconds: None,
                input_credits_per_million_tokens: Some(30),
                output_credits_per_million_tokens: Some(10_000),
                provider: ModelProvider::Openrouter,
                model_type: ModelType::Text,
                display_name: "GLM 5.2".to_string(),
                description: None,
                privacy: None,
                pricing: None,
                context_tokens: None,
                traits: Vec::new(),
                capabilities: vec!["supportsFunctionCalling".to_string()],
            },
        );
        PricingTable::new(models)
    }

    #[test]
    fn auto_resolves_economy_tier() -> Result<(), Box<dyn Error>> {
        let pricing = pricing_table();
        let resolved = resolve_auto_model(&pricing, Some(0.2), None)?;
        assert_eq!(resolved, TIER_ECONOMY_MODEL);
        Ok(())
    }

    #[test]
    fn auto_resolves_balanced_tier() -> Result<(), Box<dyn Error>> {
        let pricing = pricing_table();
        let resolved = resolve_auto_model(&pricing, Some(0.5), None)?;
        assert_eq!(resolved, TIER_BALANCED_MODEL);
        Ok(())
    }

    #[test]
    fn auto_resolves_quality_tier() -> Result<(), Box<dyn Error>> {
        let pricing = pricing_table();
        let resolved_none = resolve_auto_model(&pricing, None, None)?;
        assert_eq!(resolved_none, TIER_QUALITY_MODEL);
        let resolved_high = resolve_auto_model(&pricing, Some(0.9), None)?;
        assert_eq!(resolved_high, TIER_QUALITY_MODEL);
        Ok(())
    }

    #[test]
    fn auto_falls_back_to_vision_model_when_required() -> Result<(), Box<dyn Error>> {
        let pricing = pricing_table();
        let resolved = resolve_auto_model(
            &pricing,
            Some(0.2),
            Some(AgentModelRequirements {
                vision: true,
                tools: false,
            }),
        )?;
        assert_eq!(resolved, TIER_BALANCED_MODEL);
        Ok(())
    }

    #[test]
    fn parse_preview_flag_accepts_only_wire_true_values() {
        assert!(parse_preview_flag(Some("true")));
        assert!(parse_preview_flag(Some("1")));
        assert!(!parse_preview_flag(None));
        assert!(!parse_preview_flag(Some("false")));
        assert!(!parse_preview_flag(Some("yes")));
    }
}
