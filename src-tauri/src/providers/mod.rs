//! Model-picker state. The Tauri side persists which transcription /
//! text generation models the user selected. Groq and OpenRouter
//! requests use the user's provider keys directly.
//! The user's own providers (named OpenAI-compatible endpoints, agent CLIs, and
//! the provider chosen per use) are the `llm` registry, also persisted here.
//! Keys never live in `provider-settings.json`: endpoint keys and a user's own
//! Groq / OpenRouter / Tavily keys live in the Keychain store
//! (`crate::llm::secrets`), and settings only record that one is saved.

mod catalog;
mod direct;
#[cfg(test)]
pub(crate) use direct::TEST_ORIGIN;
pub mod groq;
pub mod local_transcriber;
pub mod notes;
pub mod openrouter;
pub mod transcription;

use crate::domain::types::AppError;
use crate::llm::{
    cli::CliKind,
    registry::{LlmEndpointRecord, LlmRegistry, LlmUsageSelection, ProviderRef},
    StructuredOutputLevel,
};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};
use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    sync::{LazyLock, Mutex, OnceLock},
    time::Duration,
};
use tauri::{AppHandle, Manager, State};

pub const PROVIDER_GROQ: &str = "groq";
pub const PROVIDER_OPENROUTER: &str = "openrouter";
pub const PROVIDER_LOCAL: &str = "local";
/// Whisper model for a fresh or missing Groq transcription selection.
pub const DEFAULT_TRANSCRIPTION_MODEL: &str = "whisper-large-v3";
/// The Groq Whisper models Clovy offers for dictation and notes (Groq
/// speech-to-text docs). A persisted transcription model outside this set,
/// such as an id saved before the Groq cutover, loads as the default so the
/// next recording always uses a supported transcription model.
pub const KNOWN_TRANSCRIPTION_MODELS: &[&str] = &["whisper-large-v3-turbo", "whisper-large-v3"];
pub const DEFAULT_GENERATION_MODEL: &str = "z-ai/glm-5.2";
pub const LEGACY_AUTO_GENERATION_MODEL: &str = "open-software/auto";
/// Text ids saved before the OpenRouter cutover with matching OpenRouter ids. Settings migrate through this table once (see
/// `remote_models_migrated`); any other pre-cutover id moves to the default.
const LEGACY_TEXT_MODEL_EQUIVALENTS: &[(&str, &str)] = &[
    ("zai-org-glm-5-2", "z-ai/glm-5.2"),
    ("zai-org-glm-5-1", "z-ai/glm-5.1"),
    ("zai-org-glm-5", "z-ai/glm-5"),
    ("kimi-k2-6", "moonshotai/kimi-k2.6"),
    ("kimi-k3", "moonshotai/kimi-k3"),
    (
        "nvidia-nemotron-3-nano-30b-a3b",
        "nvidia/nemotron-3-nano-30b-a3b",
    ),
];
const PROVIDER_API_KEY_CHECK_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_PROVIDER_API_KEY_CHARS: usize = 4_096;

static MODEL_SETTINGS: OnceLock<Mutex<ProviderModelSettings>> = OnceLock::new();
static PROVIDER_KEY_CHECK_HTTP_CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    crate::http_client::build_client(
        crate::http_client::base_client_builder()
            .timeout(PROVIDER_API_KEY_CHECK_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none()),
        "provider_key_check",
    )
});

/// Providers whose user-owned keys are stored locally. Groq transcribes,
/// OpenRouter serves audio and remote text, and Tavily credentials remain
/// available for explicit connector configuration. Keys are sent only to
/// their owning provider.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum BuiltinProvider {
    Groq,
    OpenRouter,
    Tavily,
}

impl BuiltinProvider {
    pub fn id(self) -> &'static str {
        match self {
            Self::Groq => "groq",
            Self::OpenRouter => "openrouter",
            Self::Tavily => "tavily",
        }
    }

    /// Keychain account of the user's key. Endpoint accounts are `ep-<uuid>`
    /// or `local`, so the `builtin:` prefix never collides with one.
    pub fn secret_account(self) -> &'static str {
        match self {
            Self::Groq => "builtin:groq",
            Self::OpenRouter => "builtin:openrouter",
            Self::Tavily => "builtin:tavily",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Groq => "Groq",
            Self::OpenRouter => "OpenRouter",
            Self::Tavily => "Tavily",
        }
    }

    /// A documented endpoint that needs a valid key and runs no inference, so
    /// saving a key costs nothing: Groq lists models, OpenRouter describes the
    /// current key, and Tavily reports the key's usage.
    fn key_check_url(self) -> String {
        match self {
            Self::Groq => "https://api.groq.com/openai/v1/models".to_string(),
            Self::OpenRouter => "https://openrouter.ai/api/v1/key".to_string(),
            Self::Tavily => "https://api.tavily.com/usage".to_string(),
        }
    }
}

/// Which providers have a user key saved in the Keychain store. Never the keys
/// themselves; a saved key passed the provider's check when it was saved, and
/// a later rejection surfaces on the request that uses it.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct ProviderApiKeysConfigured {
    pub groq: bool,
    pub openrouter: bool,
    pub tavily: bool,
}

impl ProviderApiKeysConfigured {
    pub fn get(&self, provider: BuiltinProvider) -> bool {
        match provider {
            BuiltinProvider::Groq => self.groq,
            BuiltinProvider::OpenRouter => self.openrouter,
            BuiltinProvider::Tavily => self.tavily,
        }
    }

    fn set(&mut self, provider: BuiltinProvider, configured: bool) {
        let slot = match provider {
            BuiltinProvider::Groq => &mut self.groq,
            BuiltinProvider::OpenRouter => &mut self.openrouter,
            BuiltinProvider::Tavily => &mut self.tavily,
        };
        *slot = configured;
    }
}

pub struct ProviderSettingsState {
    path: PathBuf,
    settings: Mutex<ProviderModelSettings>,
    transcription_intent: Mutex<TranscriptionIntent>,
    retired_media: RetiredMediaMetadata,
}

/// Historical preferences that are carried through settings saves and never
/// included in the settings used by providers or returned over IPC.
#[derive(Default)]
struct RetiredMediaMetadata {
    models_and_safety: Map<String, Value>,
    fal_configured: Option<bool>,
    profile_overrides: BTreeMap<String, Map<String, Value>>,
}

impl RetiredMediaMetadata {
    fn from_document(document: &Value) -> Self {
        let models_and_safety = Self::fields(
            document,
            &[
                "imageModel",
                "videoModel",
                "imageSafeMode",
                "imageSafeModePromptDismissed",
                "imageSafeModeSetByUser",
            ],
        );
        let fal_configured = document["providerApiKeysConfigured"]["fal"].as_bool();
        let profile_overrides = document["profileOverrides"]
            .as_object()
            .into_iter()
            .flatten()
            .filter_map(|(profile, overrides)| {
                let fields = Self::fields(overrides, &["imageModel", "videoModel"]);
                (!fields.is_empty()).then(|| (profile.clone(), fields))
            })
            .collect();
        Self {
            models_and_safety,
            fal_configured,
            profile_overrides,
        }
    }

    fn fields(document: &Value, keys: &[&str]) -> Map<String, Value> {
        keys.iter()
            .filter_map(|&key| {
                document
                    .get(key)
                    .map(|value| (key.to_string(), value.clone()))
            })
            .collect()
    }

    fn restore(&self, document: &mut Value) {
        for (key, value) in &self.models_and_safety {
            document[key] = value.clone();
        }
        if let Some(configured) = self.fal_configured {
            document["providerApiKeysConfigured"]["fal"] = Value::Bool(configured);
        }
        for (profile, fields) in &self.profile_overrides {
            for (key, value) in fields {
                document["profileOverrides"][profile][key] = value.clone();
            }
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderModelSettings {
    #[serde(default = "default_transcription_provider")]
    pub transcription_provider: String,
    #[serde(default = "default_generation_provider")]
    pub generation_provider: String,
    #[serde(default = "default_transcription_model")]
    pub transcription_model: String,
    #[serde(default = "default_generation_model")]
    pub generation_model: String,
    #[serde(default = "default_generation_model")]
    pub remote_generation_model: String,
    /// Which providers have a user key in the Keychain store (never the keys).
    #[serde(default)]
    pub provider_api_keys_configured: ProviderApiKeysConfigured,
    /// False on files saved before the Groq and OpenRouter cutover: their
    /// text model ids are migrated once on load (see `migrate_legacy_text_model`)
    /// and the file is rewritten, which also drops the retired plaintext
    /// `veniceApiKey` field without copying it anywhere.
    #[serde(default)]
    pub remote_models_migrated: bool,
    /// The pre-registry single endpoint. Only read to migrate it into
    /// `llm_endpoints`; kept on disk solely while the Keychain refuses its key.
    #[serde(default, skip_serializing_if = "LocalGenerationSettings::is_empty")]
    pub local_generation: LocalGenerationSettings,
    /// Named OpenAI-compatible endpoints (keys live in the Keychain).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub llm_endpoints: Vec<LlmEndpointRecord>,
    /// The provider chosen for each use.
    #[serde(default)]
    pub llm_usage: LlmUsageSelection,
    /// Structured-output level measured by the last CLI connection test.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub llm_cli_levels: BTreeMap<CliKind, StructuredOutputLevel>,
    #[serde(default)]
    pub llm_registry_migrated: bool,
    /// Chat was moved to the default CLI engine once, or the user chose a
    /// chat engine (`crate::llm::registry::apply_cli_default`).
    #[serde(default)]
    pub llm_cli_default_applied: bool,
    /// When true (the default), Clovy streams a live transcript preview while
    /// recording. On builds that carry this setting the preview is billed as
    /// extra usage (ADR-0002 addendum, JUN-375); turning it off stops the
    /// preview lanes entirely, so nothing is sent or billed.
    #[serde(default = "default_live_transcription")]
    pub live_transcription: bool,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub profile_overrides: BTreeMap<String, ProfileModelOverrides>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LocalGenerationSettings {
    pub base_url: String,
    pub model_id: String,
    #[serde(default)]
    pub api_key: String,
}

impl LocalGenerationSettings {
    pub fn is_empty(&self) -> bool {
        self.base_url.trim().is_empty()
            && self.model_id.trim().is_empty()
            && self.api_key.trim().is_empty()
    }
}

impl ProviderModelSettings {
    pub fn endpoint(&self, id: &str) -> Option<&LlmEndpointRecord> {
        self.llm_endpoints.iter().find(|endpoint| endpoint.id == id)
    }
}

/// The endpoint serving chat, as the settings UI sees it. Never carries the
/// API key.
#[derive(Clone, Debug, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ChatEndpointDto {
    pub base_url: String,
    pub model_id: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProfileModelOverrides {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transcription_provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transcription_model: Option<String>,
}

/// The client-facing view of provider settings. API keys are never
/// serialized back, only which providers have a saved user key.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderModelSettingsDto {
    pub local_transcription_supported: bool,
    pub transcription_provider: String,
    pub generation_provider: String,
    pub transcription_model: String,
    pub generation_model: String,
    pub remote_generation_model: String,
    pub provider_api_keys_configured: ProviderApiKeysConfigured,
    pub local_generation: ChatEndpointDto,
    pub live_transcription: bool,
}

impl From<&ProviderModelSettings> for ProviderModelSettingsDto {
    fn from(settings: &ProviderModelSettings) -> Self {
        Self {
            local_transcription_supported: local_transcription_supported(),
            transcription_provider: settings.transcription_provider.clone(),
            generation_provider: settings.generation_provider.clone(),
            transcription_model: settings.transcription_model.clone(),
            generation_model: settings.generation_model.clone(),
            remote_generation_model: settings.remote_generation_model.clone(),
            provider_api_keys_configured: settings.provider_api_keys_configured,
            local_generation: chat_endpoint_dto(settings),
            live_transcription: settings.live_transcription,
        }
    }
}

fn chat_endpoint_dto(settings: &ProviderModelSettings) -> ChatEndpointDto {
    match &settings.llm_usage.chat {
        ProviderRef::Endpoint { id } => settings
            .endpoint(id)
            .map(|endpoint| ChatEndpointDto {
                base_url: endpoint.base_url.clone(),
                model_id: endpoint.model_id.clone(),
            })
            .unwrap_or_default(),
        _ => ChatEndpointDto::default(),
    }
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderModelSettingsResponse {
    pub settings: ProviderModelSettingsDto,
    pub effective_settings: ProviderModelSettingsDto,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProfileModelOverridesDto {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transcription_provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transcription_model: Option<String>,
}

impl From<&ProfileModelOverrides> for ProfileModelOverridesDto {
    fn from(overrides: &ProfileModelOverrides) -> Self {
        Self {
            transcription_provider: overrides.transcription_provider.clone(),
            transcription_model: overrides.transcription_model.clone(),
        }
    }
}

impl From<ProfileModelOverridesDto> for ProfileModelOverrides {
    fn from(overrides: ProfileModelOverridesDto) -> Self {
        Self {
            transcription_provider: normalize_optional_model_field(
                overrides.transcription_provider,
            ),
            transcription_model: normalize_optional_model_field(overrides.transcription_model),
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SetProviderModelRequest {
    pub mode: ModelMode,
    pub model_id: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProbeLocalGenerationEndpointRequest {
    pub base_url: String,
    /// A key typed in the form; blank falls back to the key saved for
    /// `endpoint_id`.
    #[serde(default)]
    pub api_key: String,
    /// The registered endpoint being edited, whose Keychain key is used when
    /// no key is typed.
    #[serde(default)]
    pub endpoint_id: Option<String>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LocalEndpointProbe {
    pub models: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderModelsRequest {
    pub mode: ModelMode,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SetProviderApiKeyRequest {
    pub provider: BuiltinProvider,
    pub api_key: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ClearProviderApiKeyRequest {
    pub provider: BuiltinProvider,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderModelsResponse {
    pub mode: ModelMode,
    pub model_type: String,
    pub selected_model: String,
    pub models: Vec<ProviderModelDto>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderModelDto {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub available: Option<bool>,
    pub provider: String,
    pub id: String,
    pub name: String,
    pub model_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub privacy: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pricing: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_tokens: Option<i64>,
    pub traits: Vec<String>,
    pub capabilities: Vec<String>,
    pub price_unit: String,
    pub price_description: String,
}

pub fn configured_transcription_provider() -> String {
    effective_current_settings().transcription_provider
}

pub fn transcription_model() -> String {
    effective_current_settings().transcription_model
}

pub fn generation_model() -> String {
    current_settings().generation_model
}

/// The saved service-managed generation model, independent of an explicitly
/// enabled local provider. Unattended Routines use this accessor so they stay
/// on Clovy's metered remote route.
pub fn remote_generation_model() -> String {
    current_settings().remote_generation_model
}

pub fn generation_provider() -> String {
    current_settings().generation_provider
}

/// The endpoint serving chat (the agent route), key included; empty when
/// chat is not on an endpoint.
pub fn local_generation_settings() -> LocalGenerationSettings {
    let registry = llm_registry();
    let connection = match &registry.usage.chat {
        ProviderRef::Endpoint { id } => registry.connection(id, crate::llm::secrets::store()).ok(),
        _ => None,
    };
    crate::llm::registry::legacy_settings(connection)
}

/// The endpoint an agent request for `model_id` should use: the chat
/// endpoint when it serves that model, else any registered endpoint that does.
pub fn local_generation_settings_for_model(model_id: &str) -> LocalGenerationSettings {
    let registry = llm_registry();
    let connection = registry.endpoint_for_model(model_id).and_then(|endpoint| {
        registry
            .connection(&endpoint.id, crate::llm::secrets::store())
            .ok()
    });
    crate::llm::registry::legacy_settings(connection)
}

/// The registered endpoint `endpoint_id`, key included; empty when it no
/// longer exists (the agent route then refuses instead of picking another).
pub fn local_generation_settings_for_endpoint(endpoint_id: &str) -> LocalGenerationSettings {
    let connection = llm_registry()
        .connection(endpoint_id, crate::llm::secrets::store())
        .ok();
    crate::llm::registry::legacy_settings(connection)
}

/// Snapshot of the provider registry for routing one generation call.
pub fn llm_registry() -> LlmRegistry {
    LlmRegistry::from(&current_settings())
}

/// The user's saved key for `provider`, or `None` when none is saved.
/// The credential store is read only when settings record a saved key.
/// A recorded key the store cannot return is an error; there is no fallback.
pub fn provider_api_key(provider: BuiltinProvider) -> Result<Option<String>, AppError> {
    saved_provider_api_key(&current_settings(), crate::llm::secrets::store(), provider)
}

fn saved_provider_api_key(
    settings: &ProviderModelSettings,
    store: &dyn crate::llm::secrets::SecretStore,
    provider: BuiltinProvider,
) -> Result<Option<String>, AppError> {
    if !settings.provider_api_keys_configured.get(provider) {
        return Ok(None);
    }
    match store.get(provider.secret_account()) {
        Ok(Some(api_key)) => match normalize_api_key(&api_key) {
            Some(api_key) => Ok(Some(api_key)),
            None => Err(provider_api_key_unavailable(provider)),
        },
        Ok(None) => Err(provider_api_key_unavailable(provider)),
        Err(error) => {
            tracing::warn!(provider = provider.id(), %error, "saved provider key unavailable");
            Err(provider_api_key_unavailable(provider))
        }
    }
}

fn provider_api_key_unavailable(provider: BuiltinProvider) -> AppError {
    AppError::new(
        "provider_api_key_unavailable",
        format!(
            "Clovy could not read your saved {} key. Open Settings and save it again.",
            provider.display_name()
        ),
    )
}

pub fn live_transcription() -> bool {
    current_settings().live_transcription
}

#[tauri::command]
pub fn provider_model_settings(
    state: State<'_, ProviderSettingsState>,
) -> Result<ProviderModelSettingsResponse, AppError> {
    let settings = state
        .settings
        .lock()
        .map_err(|_| AppError::new("provider_settings_unavailable", "Settings lock failed."))?;
    let effective_settings = effective_settings_for_profile(&settings, "default");
    Ok(ProviderModelSettingsResponse {
        settings: ProviderModelSettingsDto::from(&*settings),
        effective_settings: ProviderModelSettingsDto::from(&effective_settings),
    })
}

#[tauri::command]
pub fn profile_model_overrides(
    state: State<'_, ProviderSettingsState>,
    profile: String,
) -> Result<Option<ProfileModelOverridesDto>, AppError> {
    let profile = validate_profile_override_name(&profile)?;
    let settings = state
        .settings
        .lock()
        .map_err(|_| AppError::new("provider_settings_unavailable", "Settings lock failed."))?;
    Ok(settings
        .profile_overrides
        .get(profile)
        .map(ProfileModelOverridesDto::from))
}

#[tauri::command]
pub fn set_profile_model_overrides(
    state: State<'_, ProviderSettingsState>,
    profile: String,
    overrides: ProfileModelOverridesDto,
) -> Result<(), AppError> {
    set_profile_model_overrides_impl(&state, &profile, overrides)
}

fn set_profile_model_overrides_impl(
    state: &ProviderSettingsState,
    profile: &str,
    overrides: ProfileModelOverridesDto,
) -> Result<(), AppError> {
    let profile = validate_profile_override_name(profile)?.to_string();
    update_settings_result(state, |settings| {
        let overrides = ProfileModelOverrides::from(overrides);
        if overrides.transcription_provider.is_some() || overrides.transcription_model.is_some() {
            if let Some(id) = local_model_for_override(settings, &overrides) {
                transcription::require_local_ready_for(id)?;
            }
        }
        if profile_overrides_empty(&overrides) {
            settings.profile_overrides.remove(&profile);
        } else {
            settings.profile_overrides.insert(profile, overrides);
        }
        Ok(())
    })?;
    Ok(())
}

#[tauri::command]
pub fn delete_profile_model_overrides(
    state: State<'_, ProviderSettingsState>,
    profile: String,
) -> Result<(), AppError> {
    delete_profile_model_overrides_impl(&state, &profile)
}

fn delete_profile_model_overrides_impl(
    state: &ProviderSettingsState,
    profile: &str,
) -> Result<(), AppError> {
    let profile = validate_profile_override_name(profile)?.to_string();
    update_settings(state, |settings| {
        settings.profile_overrides.remove(&profile);
    })?;
    Ok(())
}

#[tauri::command]
pub fn set_provider_model(
    app: tauri::AppHandle,
    state: State<'_, ProviderSettingsState>,
    request: SetProviderModelRequest,
) -> Result<ProviderModelSettingsDto, AppError> {
    use tauri::Emitter;
    let transcription = request.mode == ModelMode::Transcription;
    let settings = set_provider_model_impl(&state, request)?;
    if transcription {
        let _ = app.emit(TRANSCRIPTION_SELECTION_CHANGED, ());
    }
    Ok(settings)
}

pub(crate) const TRANSCRIPTION_SELECTION_CHANGED: &str = "transcription-selection-changed";

/// Transcription model identities are unique across the three providers.
fn set_provider_model_impl(
    state: &ProviderSettingsState,
    request: SetProviderModelRequest,
) -> Result<ProviderModelSettingsDto, AppError> {
    let model_id = match request.model_id.trim() {
        LEGACY_AUTO_GENERATION_MODEL if request.mode == ModelMode::Generation => {
            DEFAULT_GENERATION_MODEL
        }
        value => value,
    }
    .to_string();
    if model_id.is_empty() {
        return Err(AppError::new("provider_model_required", "Select a model."));
    }
    if request.mode == ModelMode::Transcription {
        let mut intent = state
            .transcription_intent
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let saved =
            save_transcription_selection(state, &model_id, transcription::require_local_ready_for)?;
        intent.replace(None);
        return Ok(saved);
    }
    update_settings(state, |settings| {
        // Text selection is independent from the chosen CLI and Notes engine.
        settings.generation_provider = PROVIDER_OPENROUTER.to_string();
        settings.generation_model = model_id.clone();
        settings.remote_generation_model = model_id;
    })
}

pub fn local_transcription_supported() -> bool {
    transcription::LocalModelId::ParakeetUltra.compatible(transcription::macos_major())
}

fn save_transcription_selection(
    state: &ProviderSettingsState,
    model_id: &str,
    check_local: impl FnOnce(transcription::LocalModelId) -> Result<(), AppError>,
) -> Result<ProviderModelSettingsDto, AppError> {
    let local = transcription::LocalModelId::parse(model_id);
    let provider = if local.is_some() {
        PROVIDER_LOCAL
    } else if transcription::OPENROUTER_MODELS.contains(&model_id) {
        PROVIDER_OPENROUTER
    } else if KNOWN_TRANSCRIPTION_MODELS.contains(&model_id) {
        PROVIDER_GROQ
    } else {
        return Err(AppError::new(
            "provider_model_unavailable",
            "This model is not available. Choose another model.",
        ));
    };
    update_settings_result(state, |settings| {
        if let Some(local) = local {
            check_local(local)?;
        }
        settings.transcription_provider = provider.into();
        settings.transcription_model = model_id.into();
        Ok(())
    })
}

#[derive(Default)]
struct TranscriptionIntent {
    revision: u64,
    pending: Option<transcription::LocalModelId>,
}

impl TranscriptionIntent {
    fn replace(&mut self, model: Option<transcription::LocalModelId>) -> u64 {
        self.revision = self.revision.wrapping_add(1);
        self.pending = model;
        self.revision
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn start_transcription_install(
    state: &ProviderSettingsState,
    model: transcription::LocalModelId,
) -> u64 {
    state
        .transcription_intent
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .replace(Some(model))
}

#[cfg(target_os = "macos")]
pub(crate) fn finish_transcription_install(
    state: &ProviderSettingsState,
    model: transcription::LocalModelId,
    revision: u64,
    ready: bool,
) -> Result<Option<ProviderModelSettingsDto>, AppError> {
    complete_transcription_install(
        state,
        model,
        revision,
        ready,
        transcription::require_local_ready_for,
    )
}

#[cfg(any(target_os = "macos", test))]
fn complete_transcription_install(
    state: &ProviderSettingsState,
    model: transcription::LocalModelId,
    revision: u64,
    ready: bool,
    check_local: impl FnOnce(transcription::LocalModelId) -> Result<(), AppError>,
) -> Result<Option<ProviderModelSettingsDto>, AppError> {
    let mut intent = state
        .transcription_intent
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if intent.revision != revision || intent.pending != Some(model) {
        return Ok(None);
    }
    intent.replace(None);
    if ready {
        save_transcription_selection(state, model.as_str(), check_local).map(Some)
    } else {
        Ok(None)
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn cancel_transcription_install(
    state: &ProviderSettingsState,
    model: transcription::LocalModelId,
) {
    let mut intent = state
        .transcription_intent
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if intent.pending == Some(model) {
        intent.replace(None);
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn prepare_local_model_removal<'a>(
    state: &ProviderSettingsState,
    model_id: transcription::LocalModelId,
    model: &'a crate::transcription_model::TranscriptionModel,
    confirm_active: bool,
) -> Result<(crate::transcription_model::RemovalGuard<'a>, bool), AppError> {
    let mut intent = state
        .transcription_intent
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let active = settings_snapshot(state).is_ok_and(|settings| {
        settings.transcription_provider == PROVIDER_LOCAL
            && settings.transcription_model == model_id.as_str()
    });
    if active && !confirm_active {
        return Err(AppError::new(
            "local_model_confirm_removal",
            "Confirm switching transcription to Groq before removing this model.",
        ));
    }
    let removal = model.begin_removal()?;
    let switched = remove_local_transcription_selection(state, model_id)?;
    if intent.pending == Some(model_id) {
        intent.replace(None);
    }
    Ok((removal, switched))
}

#[cfg(any(target_os = "macos", test))]
fn remove_local_transcription_selection(
    state: &ProviderSettingsState,
    model: transcription::LocalModelId,
) -> Result<bool, AppError> {
    let is_selected = |settings: &ProviderModelSettings| {
        settings.transcription_provider == PROVIDER_LOCAL
            && settings.transcription_model == model.as_str()
    };
    let mut settings = state
        .settings
        .lock()
        .map_err(|_| AppError::new("provider_settings_unavailable", "Settings lock failed."))?;
    let switched = is_selected(&settings);
    let mut candidate = settings.clone();
    let mut cleared_override = false;
    for overrides in candidate.profile_overrides.values_mut() {
        if (overrides.transcription_provider.is_some() || overrides.transcription_model.is_some())
            && local_model_for_override(&settings, overrides) == Some(model)
        {
            overrides.transcription_provider = Some(PROVIDER_GROQ.into());
            overrides.transcription_model = Some(DEFAULT_TRANSCRIPTION_MODEL.into());
            cleared_override = true;
        }
    }
    if switched {
        candidate.transcription_provider = PROVIDER_GROQ.into();
        candidate.transcription_model = DEFAULT_TRANSCRIPTION_MODEL.into();
    }
    if switched || cleared_override {
        persist_settings_candidate(state, &mut settings, candidate)?;
        drop(settings);
        local_transcriber::selection_changed();
    }
    Ok(switched)
}

fn local_model_for_override(
    settings: &ProviderModelSettings,
    overrides: &ProfileModelOverrides,
) -> Option<transcription::LocalModelId> {
    let provider = overrides
        .transcription_provider
        .as_deref()
        .unwrap_or(&settings.transcription_provider);
    if provider != PROVIDER_LOCAL {
        return None;
    }
    let model = overrides
        .transcription_model
        .as_deref()
        .unwrap_or(&settings.transcription_model);
    Some(
        transcription::LocalModelId::parse(model)
            .unwrap_or(transcription::LocalModelId::ParakeetUltra),
    )
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SetLiveTranscriptionRequest {
    pub enabled: bool,
}

#[tauri::command]
pub fn set_live_transcription(
    state: State<'_, ProviderSettingsState>,
    request: SetLiveTranscriptionRequest,
) -> Result<ProviderModelSettingsDto, AppError> {
    set_live_transcription_impl(&state, request)
}

fn set_live_transcription_impl(
    state: &ProviderSettingsState,
    request: SetLiveTranscriptionRequest,
) -> Result<ProviderModelSettingsDto, AppError> {
    update_settings(state, |settings| {
        settings.live_transcription = request.enabled;
    })
}

/// Saves the user's own key for one provider. The key is checked against the
/// provider's free documented endpoint first (see
/// [`BuiltinProvider::key_check_url`]); only an accepted key reaches the
/// Keychain store, and settings record just that one is saved.
#[tauri::command]
pub async fn set_provider_api_key(
    state: State<'_, ProviderSettingsState>,
    request: SetProviderApiKeyRequest,
) -> Result<ProviderModelSettingsDto, AppError> {
    let check_url = request.provider.key_check_url();
    set_provider_api_key_impl(&state, crate::llm::secrets::store(), request, &check_url).await
}

#[tauri::command]
pub fn clear_provider_api_key(
    state: State<'_, ProviderSettingsState>,
    request: ClearProviderApiKeyRequest,
) -> Result<ProviderModelSettingsDto, AppError> {
    clear_provider_api_key_impl(&state, crate::llm::secrets::store(), request.provider)
}

/// Validates, checks against `check_url`, and only then writes the key and
/// records it; a rejected or unchecked key changes nothing.
async fn set_provider_api_key_impl(
    state: &ProviderSettingsState,
    store: &dyn crate::llm::secrets::SecretStore,
    request: SetProviderApiKeyRequest,
    check_url: &str,
) -> Result<ProviderModelSettingsDto, AppError> {
    let provider = request.provider;
    let api_key = validate_provider_api_key(provider, &request.api_key)?;
    verify_provider_api_key(provider, &api_key, check_url).await?;
    update_provider_credential(state, store, provider, Some(&api_key))
}

fn clear_provider_api_key_impl(
    state: &ProviderSettingsState,
    store: &dyn crate::llm::secrets::SecretStore,
    provider: BuiltinProvider,
) -> Result<ProviderModelSettingsDto, AppError> {
    update_provider_credential(state, store, provider, None)
}

/// Serialize secret changes with settings writes and publication. Neither
/// settings view changes until the atomic file replacement succeeds. A failed
/// persistence restores the previous secret (also restoring its store cache).
fn update_provider_credential(
    state: &ProviderSettingsState,
    store: &dyn crate::llm::secrets::SecretStore,
    provider: BuiltinProvider,
    next_key: Option<&str>,
) -> Result<ProviderModelSettingsDto, AppError> {
    let mut settings = state
        .settings
        .lock()
        .map_err(|_| AppError::new("provider_settings_unavailable", "Settings lock failed."))?;
    let mut published = settings_store()
        .lock()
        .map_err(|_| AppError::new("provider_settings_unavailable", "Settings lock failed."))?;
    let account = provider.secret_account();
    let prior_key = store
        .get(account)
        .map_err(|_| provider_api_key_store_failed(provider))?;
    let mut candidate = settings.clone();
    candidate
        .provider_api_keys_configured
        .set(provider, next_key.is_some());
    let write_secret = |key: Option<&str>| match key {
        Some(key) => store.set(account, key),
        None => store.delete(account),
    };
    write_secret(next_key).map_err(|_| provider_api_key_store_failed(provider))?;
    if let Err(error) = save_settings(state, &candidate) {
        if write_secret(prior_key.as_deref()).is_err() {
            // Never hide a failed compensation or expose the store's error
            // text (which could include a credential). Both settings views
            // and the old file remain intact; the user must restore the key.
            return Err(AppError::new(
                "provider_api_key_rollback_failed",
                format!("Settings could not be saved and your previous {} key could not be restored. Save that key again before retrying.", provider.display_name()),
            ));
        }
        return Err(error);
    }
    *settings = candidate.clone();
    *published = candidate;
    if provider == BuiltinProvider::OpenRouter {
        catalog::invalidate();
    }
    Ok(ProviderModelSettingsDto::from(&*settings))
}

fn provider_api_key_store_failed(provider: BuiltinProvider) -> AppError {
    AppError::new(
        "provider_api_key_store_failed",
        format!(
            "Clovy could not update your {} key in the Keychain. Try again.",
            provider.display_name()
        ),
    )
}

/// Lists the models an OpenAI-compatible endpoint advertises, so the settings
/// UI can confirm the endpoint is reachable and offer real model ids. Uses a
/// short timeout because this runs interactively while the user types.
#[tauri::command]
pub async fn probe_local_generation_endpoint(
    request: ProbeLocalGenerationEndpointRequest,
) -> Result<LocalEndpointProbe, AppError> {
    let base_url = normalize_local_base_url(&request.base_url)?;
    let api_key = crate::llm::registry::listing_key(
        &llm_registry(),
        request.endpoint_id.as_deref(),
        &request.api_key,
        crate::llm::secrets::store(),
    )
    .unwrap_or_default();
    let url = format!("{base_url}/models");

    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|error| AppError::new("local_endpoint_unreachable", error.to_string()))?;

    let mut request = client.get(&url);
    if !api_key.is_empty() {
        request = request.bearer_auth(&api_key);
    }

    let response = request.send().await.map_err(|_| {
        AppError::new(
            "local_endpoint_unreachable",
            "Could not reach the endpoint. Check the URL and that the server is running.",
        )
    })?;

    let status = response.status();
    if !status.is_success() {
        return Err(AppError::new(
            "local_endpoint_failed",
            format!("The endpoint returned status {}.", status.as_u16()),
        ));
    }

    let body = response.bytes().await.map_err(|_| {
        AppError::new(
            "local_endpoint_unreachable",
            "Could not read the response from the endpoint.",
        )
    })?;
    parse_local_models_response(&body)
}

/// Parses the OpenAI models-list shape `{"data":[{"id":"..."}]}`. Extracted so
/// it can be tested without a network round-trip. Tolerates extra fields.
fn parse_local_models_response(body: &[u8]) -> Result<LocalEndpointProbe, AppError> {
    let value: serde_json::Value = serde_json::from_slice(body).map_err(|_| {
        AppError::new(
            "local_endpoint_invalid_response",
            "The endpoint returned a response we could not read.",
        )
    })?;
    let models = value
        .get("data")
        .and_then(|data| data.as_array())
        .ok_or_else(|| {
            AppError::new(
                "local_endpoint_invalid_response",
                "The endpoint did not return a model list.",
            )
        })?
        .iter()
        .filter_map(|item| item.get("id").and_then(|id| id.as_str()))
        .map(|id| id.to_string())
        .collect();
    Ok(LocalEndpointProbe { models })
}

#[tauri::command]
pub async fn list_provider_models(
    state: State<'_, ProviderSettingsState>,
    request: ProviderModelsRequest,
) -> Result<ProviderModelsResponse, AppError> {
    list_provider_models_impl(&state, request).await
}

async fn list_provider_models_impl(
    state: &ProviderSettingsState,
    request: ProviderModelsRequest,
) -> Result<ProviderModelsResponse, AppError> {
    let model_type = request.mode.api_type();
    let selected_model = selected_model_for_mode(state, request.mode)?;
    let mut models = catalog::models(request.mode).await?;
    models.sort_by(|left, right| {
        left.name
            .to_ascii_lowercase()
            .cmp(&right.name.to_ascii_lowercase())
            .then_with(|| left.id.cmp(&right.id))
    });
    Ok(ProviderModelsResponse {
        mode: request.mode,
        model_type: model_type.to_string(),
        selected_model,
        models,
    })
}

pub fn setup(app: &mut tauri::App) {
    // Resolve the real app identity once, before any Agy child gets a private
    // HOME. Authentication and every generation route share these paths.
    let agy_paths = crate::app_paths::app_data_dir(app.handle())
        .map_err(|error| error.to_string())
        .and_then(|data| {
            let host_env = std::env::vars().collect();
            crate::llm::agy::HostContext::new(data, &host_env)
                .and_then(crate::llm::agy::HostContext::configure)
                .map_err(|error| error.to_string())
        });
    if let Err(error) = agy_paths {
        tracing::warn!(%error, "could not resolve the Clovy Antigravity profile");
    }
    let path = provider_settings_path(app.handle())
        .unwrap_or_else(|| PathBuf::from("provider-settings.json"));
    let (settings, migrated_now, retired_media) = load_settings_from_disk(app.handle());
    replace_current_settings(settings.clone());
    let state = ProviderSettingsState {
        transcription_intent: Default::default(),
        path,
        settings: Mutex::new(settings.clone()),
        retired_media,
    };
    if migrated_now {
        // Rewrite the file so a migrated endpoint key, the pre-cutover model
        // ids, and the retired plaintext Venice key no longer sit in it.
        if let Err(error) = save_settings(&state, &settings) {
            tracing::warn!(code = %error.code, "could not persist the migrated provider settings");
        }
    }
    app.manage(state);
    let handle = app.handle().clone();
    tauri::async_runtime::spawn(async move {
        ensure_cli_default(&handle.state::<ProviderSettingsState>()).await;
    });
}

/// Moves chat to the default CLI engine once (ADR-0062,
/// `crate::llm::registry::apply_cli_default`). Runs at launch and before the
/// default chat engine is read, so a session created right after launch
/// already gets it. Nothing is written when nothing changes.
pub async fn ensure_cli_default(state: &ProviderSettingsState) {
    let Ok(snapshot) = settings_snapshot(state) else {
        return;
    };
    if snapshot.llm_cli_default_applied
        && matches!(snapshot.llm_usage.chat, ProviderRef::Cli { .. })
    {
        return;
    }
    let env = crate::llm::shell_env::login_env().await;
    let installed = |kind: CliKind| env.which(kind.id()).is_some();
    let mut candidate = snapshot.clone();
    crate::llm::registry::apply_cli_default(&mut candidate, installed);
    if candidate == snapshot {
        return;
    }
    let mut moved = None;
    match update_llm_registry(state, |settings| {
        moved = crate::llm::registry::apply_cli_default(settings, installed);
        Ok(())
    }) {
        Ok(_) => match moved {
            Some(kind) => {
                tracing::info!(cli = kind.id(), "chat moved to the default CLI engine")
            }
            None => tracing::info!("chat engine already chosen; default CLI migration recorded"),
        },
        Err(error) => {
            tracing::warn!(code = %error.code, "could not record the default CLI chat engine")
        }
    }
}

/// The selected chat engine, including an explicitly chosen unavailable CLI.
pub fn selected_chat_engine_model() -> String {
    crate::chat_engine::default_engine_of(&current_settings()).model_id
}

pub fn load_local_env() {
    crate::local_env::load_local_env();
}

fn current_settings() -> ProviderModelSettings {
    settings_store()
        .lock()
        .map(|settings| settings.clone())
        .unwrap_or_else(|_| default_settings())
}

pub(crate) fn effective_current_settings() -> ProviderModelSettings {
    let settings = current_settings();
    effective_settings_for_profile(&settings, "default")
}

fn settings_store() -> &'static Mutex<ProviderModelSettings> {
    MODEL_SETTINGS.get_or_init(|| Mutex::new(default_settings()))
}

fn replace_current_settings(settings: ProviderModelSettings) {
    if let Ok(mut current) = settings_store().lock() {
        if current.live_transcription != settings.live_transcription {
            crate::meeting_assist::invalidate_preview(true);
        }
        *current = settings;
    }
}

/// Test-only hook: installs settings into the process-wide store that
/// `current_settings()` reads, so live integration tests (see
/// `providers::notes::live_local_tests`) can activate the local provider without a
/// running Tauri app.
#[cfg(test)]
pub(crate) fn replace_current_settings_for_tests(settings: ProviderModelSettings) {
    replace_current_settings(settings);
}

/// Test-only: serializes tests that install process-wide settings, so they
/// never observe each other's provider selection.
#[cfg(test)]
pub(crate) static GLOBAL_SETTINGS_TEST_LOCK: tokio::sync::Mutex<()> =
    tokio::sync::Mutex::const_new(());

/// Test-only companion to [`replace_current_settings_for_tests`]: the default
/// (remote) settings, for restoring the store after a live test.
#[cfg(test)]
pub(crate) fn default_settings_for_tests() -> ProviderModelSettings {
    default_settings()
}

/// Test-only: parse, migrate (with `store`), and sanitize a settings file.
#[cfg(test)]
pub(crate) fn settings_from_json_for_tests(
    raw: &str,
    store: &dyn crate::llm::secrets::SecretStore,
) -> Option<ProviderModelSettings> {
    settings_from_json(raw, store).map(|(settings, _, _)| settings)
}

fn default_settings() -> ProviderModelSettings {
    ProviderModelSettings {
        transcription_provider: PROVIDER_GROQ.to_string(),
        generation_provider: PROVIDER_OPENROUTER.to_string(),
        transcription_model: DEFAULT_TRANSCRIPTION_MODEL.to_string(),
        generation_model: default_generation_model(),
        remote_generation_model: default_generation_model(),
        provider_api_keys_configured: ProviderApiKeysConfigured::default(),
        remote_models_migrated: true,
        local_generation: LocalGenerationSettings::default(),
        llm_endpoints: Vec::new(),
        llm_usage: LlmUsageSelection::default(),
        llm_cli_levels: BTreeMap::new(),
        llm_registry_migrated: true,
        llm_cli_default_applied: false,
        live_transcription: true,
        profile_overrides: BTreeMap::new(),
    }
}

fn default_live_transcription() -> bool {
    true
}

fn default_transcription_provider() -> String {
    PROVIDER_GROQ.to_string()
}

fn default_generation_provider() -> String {
    PROVIDER_OPENROUTER.to_string()
}

fn default_transcription_model() -> String {
    DEFAULT_TRANSCRIPTION_MODEL.to_string()
}

fn default_generation_model() -> String {
    DEFAULT_GENERATION_MODEL.to_string()
}

fn provider_settings_path(app: &AppHandle) -> Option<PathBuf> {
    crate::app_paths::app_config_dir(app)
        .ok()
        .map(provider_settings_path_from_config_dir)
}

fn provider_settings_path_from_config_dir(directory: PathBuf) -> PathBuf {
    directory.join("provider-settings.json")
}

/// Loads the saved settings; the flag is true when this load migrated the
/// legacy endpoint or pre-cutover model ids and the file should be rewritten.
fn load_settings_from_disk(app: &AppHandle) -> (ProviderModelSettings, bool, RetiredMediaMetadata) {
    provider_settings_path(app)
        .and_then(|path| fs::read_to_string(path).ok())
        .and_then(|raw| settings_from_json(&raw, crate::llm::secrets::store()))
        .unwrap_or_else(|| (default_settings(), false, RetiredMediaMetadata::default()))
}

fn settings_from_json(
    raw: &str,
    store: &dyn crate::llm::secrets::SecretStore,
) -> Option<(ProviderModelSettings, bool, RetiredMediaMetadata)> {
    let document: serde_json::Value = serde_json::from_str(raw).ok()?;
    let retired_usage = document["llmUsage"]["notes"]["kind"] == "clovy"
        || document["llmUsage"].get("dictationCleanup").is_some();
    let retired_cost_quality = document.get("costQuality").is_some();
    let retired_auto = document["generationModel"] == LEGACY_AUTO_GENERATION_MODEL
        || document["remoteGenerationModel"] == LEGACY_AUTO_GENERATION_MODEL;
    let missing_chat = document["llmUsage"].get("chat").is_none();
    let retired_media = RetiredMediaMetadata::from_document(&document);
    let parsed = serde_json::from_value::<ProviderModelSettings>(document).ok()?;
    let retired_chat = missing_chat || !matches!(parsed.llm_usage.chat, ProviderRef::Cli { .. });
    let was_migrated = parsed.llm_registry_migrated;
    let remote_models_migrated = parsed.remote_models_migrated;
    let migrated = crate::llm::registry::migrate_legacy_local_generation(parsed, store);
    let migrated_now = retired_chat
        || retired_usage
        || retired_auto
        || retired_cost_quality
        || migrated.llm_registry_migrated != was_migrated
        || !remote_models_migrated;
    Some((
        sanitize_settings(migrated, &default_settings()),
        migrated_now,
        retired_media,
    ))
}

fn normalize_transcription(provider: &str, model: String) -> (String, String) {
    if provider == PROVIDER_LOCAL && local_transcription_supported() {
        let local = transcription::LocalModelId::parse(model.trim())
            .unwrap_or(transcription::LocalModelId::ParakeetUltra);
        (PROVIDER_LOCAL.into(), local.as_str().into())
    } else if provider == PROVIDER_OPENROUTER {
        (
            PROVIDER_OPENROUTER.into(),
            curated_or_default(
                model,
                &transcription::OPENROUTER_MODELS,
                transcription::OPENROUTER_MODELS[0],
            ),
        )
    } else {
        (
            PROVIDER_GROQ.into(),
            curated_or_default(
                model,
                KNOWN_TRANSCRIPTION_MODELS,
                DEFAULT_TRANSCRIPTION_MODEL,
            ),
        )
    }
}

fn sanitize_settings(
    settings: ProviderModelSettings,
    defaults: &ProviderModelSettings,
) -> ProviderModelSettings {
    // Text ids are migrated once (the OpenRouter catalog is live, so there is
    // no fixed allowlist); transcription ids are checked against the curated
    // sets on every load.
    let migrate_text = !settings.remote_models_migrated;
    let migrate = |model: String| {
        if model == LEGACY_AUTO_GENERATION_MODEL {
            DEFAULT_GENERATION_MODEL.to_string()
        } else if migrate_text {
            migrate_legacy_text_model(model)
        } else {
            model
        }
    };
    let (transcription_provider, transcription_model) = normalize_transcription(
        &settings.transcription_provider,
        settings.transcription_model,
    );
    let mut remote_generation_model = migrate(non_empty_or(
        settings.remote_generation_model,
        &defaults.remote_generation_model,
    ));
    let local_generation = sanitize_local_generation(settings.local_generation);
    // A historical chat endpoint must not leak its stale local model id into
    // the independent OpenRouter model. The endpoint itself remains for notes.
    let generation_model = if settings.generation_provider == PROVIDER_LOCAL {
        remote_generation_model.clone()
    } else {
        // Remote or missing provider: honor the saved generation model and
        // back-fill remote_generation_model from it.
        let configured = match normalize_optional_model_field(Some(settings.generation_model)) {
            Some(model) => migrate(model),
            None => remote_generation_model.clone(),
        };
        remote_generation_model = configured.clone();
        configured
    };
    let llm_endpoints = settings
        .llm_endpoints
        .into_iter()
        .filter_map(|mut endpoint| {
            endpoint.base_url = normalize_local_base_url(&endpoint.base_url).ok()?;
            endpoint.model_id = endpoint.model_id.trim().to_string();
            (!endpoint.id.trim().is_empty() && !endpoint.model_id.is_empty()).then_some(endpoint)
        })
        .collect();

    let mut sanitized = ProviderModelSettings {
        transcription_provider,
        generation_provider: PROVIDER_OPENROUTER.to_string(),
        transcription_model,
        generation_model,
        remote_generation_model,
        provider_api_keys_configured: settings.provider_api_keys_configured,
        remote_models_migrated: true,
        local_generation,
        llm_endpoints,
        llm_usage: settings.llm_usage,
        llm_cli_levels: settings.llm_cli_levels,
        llm_registry_migrated: settings.llm_registry_migrated,
        llm_cli_default_applied: settings.llm_cli_default_applied,
        live_transcription: settings.live_transcription,
        profile_overrides: sanitize_profile_overrides(settings.profile_overrides),
    };
    crate::llm::registry::normalize(&mut sanitized);
    sanitized
}

fn sanitize_profile_overrides(
    overrides: BTreeMap<String, ProfileModelOverrides>,
) -> BTreeMap<String, ProfileModelOverrides> {
    overrides
        .into_iter()
        .filter_map(|(profile, overrides)| {
            let profile = validate_profile_override_name(&profile).ok()?.to_string();
            let provider = normalize_optional_model_field(overrides.transcription_provider);
            let model = normalize_optional_model_field(overrides.transcription_model);
            let (transcription_provider, transcription_model) = match provider.as_deref() {
                Some(provider) => {
                    let has_model = model.is_some() || provider == PROVIDER_LOCAL;
                    let (provider, model) =
                        normalize_transcription(provider, model.unwrap_or_default());
                    (Some(provider), has_model.then_some(model))
                }
                None => (
                    None,
                    model.filter(|id| {
                        KNOWN_TRANSCRIPTION_MODELS.contains(&id.as_str())
                            || transcription::OPENROUTER_MODELS.contains(&id.as_str())
                            || transcription::LocalModelId::parse(id).is_some()
                    }),
                ),
            };
            let overrides = ProfileModelOverrides {
                transcription_provider,
                transcription_model,
            };
            (!profile_overrides_empty(&overrides)).then_some((profile, overrides))
        })
        .collect()
}

/// Keeps a supported curated model id and loads anything else as the default.
/// This includes empty, delisted, and pre-cutover model ids.
fn curated_or_default(persisted: String, curated: &[&str], default: &str) -> String {
    let candidate = non_empty_or(persisted, default);
    if curated.contains(&candidate.as_str()) {
        candidate
    } else {
        default.to_string()
    }
}

/// The OpenRouter id for a text model id saved before the
/// OpenRouter cutover (`kimi-k2-6` → `moonshotai/kimi-k2.6`); any other id,
/// Auto included, is returned unchanged. Agent sessions and stored runs keep
/// the id they recorded, so the remote boundary resolves it here instead of
/// relying on a server-side alias.
pub fn current_text_model_id(model: &str) -> &str {
    let model = model.trim();
    LEGACY_TEXT_MODEL_EQUIVALENTS
        .iter()
        .find(|(legacy, _)| *legacy == model)
        .map_or(model, |&(_, current)| current)
}

/// Maps a text model id from a settings file saved before the OpenRouter
/// cutover. Auto and unknown legacy ids become the explicit default; known
/// equivalents retain their OpenRouter model identity.
fn migrate_legacy_text_model(model: String) -> String {
    if model == LEGACY_AUTO_GENERATION_MODEL {
        return DEFAULT_GENERATION_MODEL.to_string();
    }
    let current = current_text_model_id(&model);
    if current == model.trim() {
        DEFAULT_GENERATION_MODEL.to_string()
    } else {
        current.to_string()
    }
}

fn normalize_api_key(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

/// Keys are opaque: any non-empty value without control characters and within
/// the size cap is sent to the provider's check.
fn validate_provider_api_key(provider: BuiltinProvider, value: &str) -> Result<String, AppError> {
    let name = provider.display_name();
    let api_key = normalize_api_key(value).ok_or_else(|| {
        AppError::new(
            "provider_api_key_required",
            format!("Enter your {name} API key before saving."),
        )
    })?;
    if api_key.chars().count() > MAX_PROVIDER_API_KEY_CHARS || api_key.chars().any(char::is_control)
    {
        return Err(AppError::new(
            "provider_api_key_invalid",
            format!("Enter a valid {name} API key."),
        ));
    }
    Ok(api_key)
}

/// Asks `url` (the provider's key check) whether it accepts `api_key`. 401/403
/// is a rejection; anything else that is not a success is reported as a failed
/// check, never as a valid key.
async fn verify_provider_api_key(
    provider: BuiltinProvider,
    api_key: &str,
    url: &str,
) -> Result<(), AppError> {
    let name = provider.display_name();
    let response = PROVIDER_KEY_CHECK_HTTP_CLIENT
        .get(url)
        .bearer_auth(api_key)
        .send()
        .await
        .map_err(|_| {
            AppError::new(
                "provider_api_key_verification_failed",
                format!("Could not check the {name} API key. Check your connection and try again."),
            )
        })?;
    let status = response.status();
    if status.is_success() {
        return Ok(());
    }
    if matches!(
        status,
        reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN
    ) {
        return Err(AppError::new(
            "provider_api_key_rejected",
            format!("{name} rejected this API key. Check the key and try again."),
        ));
    }
    Err(AppError::new(
        "provider_api_key_verification_failed",
        format!("Could not check the {name} API key. Try again later."),
    ))
}

fn non_empty_or(value: String, fallback: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        fallback.to_string()
    } else {
        value.to_string()
    }
}

fn normalize_optional_model_field(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn profile_overrides_empty(overrides: &ProfileModelOverrides) -> bool {
    overrides.transcription_provider.is_none() && overrides.transcription_model.is_none()
}

fn effective_settings_for_profile(
    settings: &ProviderModelSettings,
    profile: &str,
) -> ProviderModelSettings {
    let mut effective = settings.clone();
    let profile = profile.trim();
    if profile.is_empty() || profile == "default" {
        return effective;
    }
    let Some(overrides) = settings.profile_overrides.get(profile) else {
        return effective;
    };
    if let Some(provider) = overrides.transcription_provider.as_deref() {
        effective.transcription_provider = provider.to_string();
    }
    if let Some(model) = overrides.transcription_model.as_deref() {
        effective.transcription_model = model.to_string();
    }
    (
        effective.transcription_provider,
        effective.transcription_model,
    ) = normalize_transcription(
        &effective.transcription_provider,
        effective.transcription_model,
    );
    effective
}

fn validate_profile_override_name(profile: &str) -> Result<&str, AppError> {
    let profile = profile.trim();
    if profile == "default" {
        return Err(AppError::new(
            "profile_model_overrides_default_profile",
            "The default data partition uses global model settings.",
        ));
    }
    if !is_safe_profile_name(profile) {
        return Err(AppError::new(
            "profile_model_overrides_invalid_profile",
            "Invalid data partition.",
        ));
    }
    Ok(profile)
}

fn is_safe_profile_name(name: &str) -> bool {
    if name.is_empty() || name.len() > 64 {
        return false;
    }
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_alphanumeric() {
        return false;
    }
    name.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-')
}

fn update_settings(
    state: &ProviderSettingsState,
    update: impl FnOnce(&mut ProviderModelSettings),
) -> Result<ProviderModelSettingsDto, AppError> {
    update_settings_result(state, |settings| {
        update(settings);
        Ok(())
    })
}

/// Apply a candidate atomically; errors never mutate the managed or published
/// view. Secret operations need `update_provider_credential` for compensation.
fn update_settings_result(
    state: &ProviderSettingsState,
    update: impl FnOnce(&mut ProviderModelSettings) -> Result<(), AppError>,
) -> Result<ProviderModelSettingsDto, AppError> {
    let settings = update_llm_registry(state, update)?;
    local_transcriber::selection_changed();
    Ok(ProviderModelSettingsDto::from(&settings))
}

/// Applies a registry change atomically: the closure works on a copy, and
/// only a successful change is saved and published.
pub(crate) fn update_llm_registry(
    state: &ProviderSettingsState,
    update: impl FnOnce(&mut ProviderModelSettings) -> Result<(), AppError>,
) -> Result<ProviderModelSettings, AppError> {
    let mut settings = state
        .settings
        .lock()
        .map_err(|_| AppError::new("provider_settings_unavailable", "Settings lock failed."))?;
    let mut candidate = settings.clone();
    update(&mut candidate)?;
    persist_settings_candidate(state, &mut settings, candidate.clone())?;
    Ok(candidate)
}

fn persist_settings_candidate(
    state: &ProviderSettingsState,
    settings: &mut ProviderModelSettings,
    candidate: ProviderModelSettings,
) -> Result<(), AppError> {
    save_settings(state, &candidate)?;
    *settings = candidate.clone();
    replace_current_settings(candidate);
    Ok(())
}

pub(crate) fn settings_snapshot(
    state: &ProviderSettingsState,
) -> Result<ProviderModelSettings, AppError> {
    state
        .settings
        .lock()
        .map(|settings| settings.clone())
        .map_err(|_| AppError::new("provider_settings_unavailable", "Settings lock failed."))
}

fn save_settings(
    state: &ProviderSettingsState,
    settings: &ProviderModelSettings,
) -> Result<(), AppError> {
    if let Some(parent) = state.path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| AppError::new("provider_settings_save_failed", error.to_string()))?;
    }
    let serialization_error = |error: serde_json::Error| {
        AppError::new("provider_settings_save_failed", error.to_string())
    };
    let mut document = serde_json::to_value(settings).map_err(serialization_error)?;
    state.retired_media.restore(&mut document);
    let serialized = serde_json::to_string_pretty(&document).map_err(serialization_error)?;
    // A failed write must not truncate the saved settings. Stage on the same
    // filesystem, then atomically replace; tempfile cleans up on any error.
    use std::io::Write;
    let parent = state.path.parent().ok_or_else(|| {
        AppError::new(
            "provider_settings_save_failed",
            "Settings directory is missing.",
        )
    })?;
    let save_error =
        |error: std::io::Error| AppError::new("provider_settings_save_failed", error.to_string());
    let mut temporary = tempfile::NamedTempFile::new_in(parent).map_err(save_error)?;
    temporary
        .write_all(serialized.as_bytes())
        .map_err(save_error)?;
    temporary.as_file().sync_all().map_err(save_error)?;
    temporary
        .persist(&state.path)
        .map_err(|error| save_error(error.error))?;
    Ok(())
}

fn sanitize_local_generation(settings: LocalGenerationSettings) -> LocalGenerationSettings {
    // Only genuinely unparseable / wrong-scheme values sanitize to empty. A
    // valid http(s) URL is preserved regardless of host (bring your own
    // inference: LAN Ollama, vLLM, etc.).
    let base_url = normalize_local_base_url(&settings.base_url).unwrap_or_default();
    LocalGenerationSettings {
        base_url,
        model_id: settings.model_id.trim().to_string(),
        api_key: settings.api_key.trim().to_string(),
    }
}

/// Validates an OpenAI-compatible endpoint base URL. Accepts any http/https
/// URL that has a host (trailing slashes trimmed). The loopback-only
/// restriction was removed so LAN endpoints work; the frontend surfaces the
/// "requests leave your device" warning.
pub(crate) fn normalize_local_base_url(value: &str) -> Result<String, AppError> {
    let trimmed = value.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return Err(AppError::new(
            "local_model_base_url_required",
            "Enter a local model endpoint.",
        ));
    }
    let parsed = reqwest::Url::parse(trimmed).map_err(|_| {
        AppError::new(
            "local_model_base_url_invalid",
            "Enter a valid local model endpoint.",
        )
    })?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(AppError::new(
            "local_model_base_url_invalid",
            "Use an http or https local model endpoint.",
        ));
    }
    if parsed.host_str().is_none() {
        return Err(AppError::new(
            "local_model_base_url_invalid",
            "Enter a local model endpoint with a host.",
        ));
    }
    Ok(trimmed.to_string())
}

fn selected_model_for_mode(
    state: &ProviderSettingsState,
    mode: ModelMode,
) -> Result<String, AppError> {
    let settings = state
        .settings
        .lock()
        .map_err(|_| AppError::new("provider_settings_unavailable", "Settings lock failed."))?;
    Ok(match mode {
        ModelMode::Transcription => settings.transcription_model.clone(),
        ModelMode::Generation => settings.generation_model.clone(),
    })
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ModelMode {
    Transcription,
    Generation,
}

impl<'de> Deserialize<'de> for ModelMode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).ok_or_else(|| serde::de::Error::custom("unknown provider model mode"))
    }
}

impl ModelMode {
    fn api_type(self) -> &'static str {
        match self {
            Self::Transcription => "asr",
            Self::Generation => "text",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value.trim() {
            "transcription" | "dictation" | "asr" => Some(Self::Transcription),
            "generation" | "notes" | "text" => Some(Self::Generation),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::secrets::SecretStore;

    #[test]
    fn provider_settings_path_derives_from_the_isolated_config_dir() {
        let isolated_config_dir = PathBuf::from("/tmp/dev.guilhermexp.clovy-dev");

        assert_eq!(
            provider_settings_path_from_config_dir(isolated_config_dir),
            PathBuf::from("/tmp/dev.guilhermexp.clovy-dev/provider-settings.json")
        );
    }

    #[test]
    fn groq_settings_default_to_large_v3_for_fresh_and_missing_model() {
        assert_eq!(default_settings().transcription_model, "whisper-large-v3");
        let store = crate::llm::secrets::MemorySecretStore::default();
        let (loaded, _, _) = settings_from_json(r#"{"transcriptionProvider":"groq"}"#, &store)
            .expect("load settings without a model");
        assert_eq!(loaded.transcription_model, "whisper-large-v3");
    }

    #[test]
    fn saved_groq_models_survive_global_and_profile_round_trips() {
        let store = crate::llm::secrets::MemorySecretStore::default();
        for model in ["whisper-large-v3", "whisper-large-v3-turbo"] {
            let raw = serde_json::json!({
                "transcriptionProvider": "groq",
                "transcriptionModel": model,
                "profileOverrides": {
                    "explicit": { "transcriptionProvider": "groq", "transcriptionModel": model },
                    "inherited": { "transcriptionModel": model },
                },
            })
            .to_string();
            let (loaded, _, _) = settings_from_json(&raw, &store).expect("load saved settings");
            let saved = serde_json::to_string(&loaded).expect("serialize loaded settings");
            let (reloaded, _, _) =
                settings_from_json(&saved, &store).expect("reload saved settings");
            assert_eq!(reloaded.transcription_model, model);
            for profile in ["explicit", "inherited"] {
                assert_eq!(
                    reloaded.profile_overrides[profile]
                        .transcription_model
                        .as_deref(),
                    Some(model)
                );
                assert_eq!(
                    effective_settings_for_profile(&reloaded, profile).transcription_model,
                    model
                );
            }
        }
    }

    #[test]
    fn retired_cost_quality_is_not_persisted_or_exposed_to_the_renderer() {
        let settings: ProviderModelSettings =
            serde_json::from_value(serde_json::json!({"costQuality": 42})).unwrap();
        let stored = serde_json::to_value(&settings).unwrap();
        let dto = serde_json::to_value(ProviderModelSettingsDto::from(&settings)).unwrap();
        assert!(stored.get("costQuality").is_none());
        assert!(dto.get("costQuality").is_none());
    }

    #[test]
    fn model_mode_deserializes_canonical_values() {
        assert_eq!(
            serde_json::from_value::<ModelMode>(serde_json::json!("transcription")).unwrap(),
            ModelMode::Transcription
        );
        assert_eq!(
            serde_json::from_value::<ModelMode>(serde_json::json!("generation")).unwrap(),
            ModelMode::Generation
        );
    }

    #[test]
    fn model_mode_deserializes_legacy_aliases() {
        assert_eq!(
            serde_json::from_value::<ModelMode>(serde_json::json!("asr")).unwrap(),
            ModelMode::Transcription
        );
        assert_eq!(
            serde_json::from_value::<ModelMode>(serde_json::json!("notes")).unwrap(),
            ModelMode::Generation
        );
    }

    #[test]
    fn provider_commands_reject_retired_model_modes_and_credentials_at_decode() {
        use serde_json::{from_value, json};

        for mode in ["image", "images", "video", "videos"] {
            assert!(
                from_value::<ProviderModelsRequest>(json!({"mode": mode})).is_err(),
                "provider catalog accepted retired mode {mode}"
            );
            assert!(from_value::<SetProviderModelRequest>(json!({
                "mode": mode, "modelId": "historical-model"
            }))
            .is_err());
        }
        assert!(from_value::<SetProviderApiKeyRequest>(json!({
            "provider": "fal", "apiKey": "synthetic-input"
        }))
        .is_err());
        assert!(from_value::<ClearProviderApiKeyRequest>(json!({"provider": "fal"})).is_err());
    }

    #[test]
    fn provider_settings_deserialize_pre_profile_overrides_fixture() {
        let fixture = r#"{
          "transcriptionProvider": "venice",
          "generationProvider": "venice",
          "transcriptionModel": "nvidia/parakeet-tdt-0.6b-v3",
          "generationModel": "zai-org-glm-5-2",
          "remoteGenerationModel": "zai-org-glm-5-2",
          "imageModel": "venice-sd35",
          "localGeneration": {
            "baseUrl": "",
            "modelId": "",
            "apiKey": ""
          },
          "imageSafeMode": true,
          "imageSafeModePromptDismissed": false,
          "imageSafeModeSetByUser": true
        }"#;

        let settings: ProviderModelSettings = serde_json::from_str(fixture).unwrap();
        assert!(settings.profile_overrides.is_empty());

        let round_tripped = serde_json::to_string(&settings).unwrap();
        let reparsed: ProviderModelSettings = serde_json::from_str(&round_tripped).unwrap();
        assert_eq!(reparsed, settings);
        assert!(!round_tripped.contains("profileOverrides"));
    }

    #[test]
    fn default_settings_enable_live_transcription() {
        assert!(default_settings().live_transcription);
    }

    #[test]
    fn provider_settings_deserialize_defaults_missing_live_transcription_field() {
        // Files written by builds before JUN-375 carry no `liveTranscription`
        // key; the serde default keeps the preview on for them.
        let settings = serde_json::from_value::<ProviderModelSettings>(serde_json::json!({
            "transcriptionProvider": "venice",
            "transcriptionModel": "nvidia/parakeet-tdt-0.6b-v3",
            "generationModel": "zai-org-glm-5-2",
            "imageModel": "venice-sd35"
        }))
        .unwrap();

        assert!(settings.live_transcription);
    }

    #[test]
    fn set_live_transcription_persists_the_toggle() {
        let state = test_state();

        let updated =
            set_live_transcription_impl(&state, SetLiveTranscriptionRequest { enabled: false })
                .unwrap();
        assert!(!updated.live_transcription);
        let saved: ProviderModelSettings =
            serde_json::from_str(&fs::read_to_string(&state.path).unwrap()).unwrap();
        assert!(!saved.live_transcription);

        let updated =
            set_live_transcription_impl(&state, SetLiveTranscriptionRequest { enabled: true })
                .unwrap();
        assert!(updated.live_transcription);
        let saved: ProviderModelSettings =
            serde_json::from_str(&fs::read_to_string(&state.path).unwrap()).unwrap();
        assert!(saved.live_transcription);
    }

    #[test]
    fn provider_models_response_serializes_canonical_mode() {
        let response = ProviderModelsResponse {
            mode: ModelMode::Transcription,
            model_type: "asr".to_string(),
            selected_model: DEFAULT_TRANSCRIPTION_MODEL.to_string(),
            models: Vec::new(),
        };

        assert_eq!(
            serde_json::to_value(response).unwrap()["mode"],
            serde_json::json!("transcription")
        );
    }

    /// A file written by a pre-cutover build, shaped like a real profile:
    /// Venice transcription, text, and media, a plaintext Venice key, Claude CLI
    /// for chat and activity, Clovy for notes and cleanup, an endpoint whose key
    /// is in the Keychain, a measured CLI level, and a profile override.
    const LEGACY_VENICE_SETTINGS: &str = r#"{
      "transcriptionProvider": "venice",
      "generationProvider": "venice",
      "transcriptionModel": "nvidia/parakeet-tdt-0.6b-v3",
      "generationModel": "zai-org-glm-5-2",
      "remoteGenerationModel": "zai-org-glm-5-2",
      "costQuality": 40,
      "imageModel": "venice-sd35",
      "videoModel": "wan-2.2-a14b-text-to-video",
      "veniceApiKey": "legacy-venice-secret",
      "llmRegistryMigrated": true,
      "llmEndpoints": [{
        "id": "ep-lan", "name": "LAN", "baseUrl": "http://192.168.1.5:11434/v1",
        "modelId": "llama3.1:8b", "hasApiKey": true
      }],
      "llmUsage": {
        "chat": { "kind": "cli", "id": "claude" },
        "notes": { "kind": "clovy" },
        "dictationCleanup": { "kind": "clovy" },
        "activity": { "kind": "cli", "id": "claude" }
      },
      "llmCliLevels": { "claude": "json_schema" },
      "imageSafeMode": false,
      "imageSafeModeSetByUser": true,
      "liveTranscription": false,
      "profileOverrides": {
        "work": {
          "transcriptionProvider": "openai",
          "transcriptionModel": "gpt-4o-transcribe",
          "imageModel": "lustify-v8"
        }
      }
    }"#;

    #[test]
    fn legacy_settings_migrate_to_the_new_providers_and_keep_every_other_choice() {
        let store = crate::llm::secrets::MemorySecretStore::default();
        let (migrated, migrated_now, retired_media) =
            settings_from_json(LEGACY_VENICE_SETTINGS, &store).unwrap();

        assert!(migrated_now, "a pre-cutover file must be rewritten");
        assert_eq!(migrated.transcription_provider, PROVIDER_GROQ);
        assert_eq!(migrated.transcription_model, DEFAULT_TRANSCRIPTION_MODEL);
        assert_eq!(migrated.generation_provider, PROVIDER_OPENROUTER);
        assert_eq!(migrated.generation_model, "z-ai/glm-5.2");
        assert_eq!(migrated.remote_generation_model, "z-ai/glm-5.2");
        assert_eq!(
            migrated.provider_api_keys_configured,
            ProviderApiKeysConfigured::default(),
            "a Venice key never becomes a key for another provider"
        );

        // Everything that is not a Venice selection survives untouched.
        assert_eq!(
            migrated.llm_usage.chat,
            ProviderRef::Cli {
                id: CliKind::Claude
            }
        );
        assert_eq!(
            migrated.llm_usage.activity,
            ProviderRef::Cli {
                id: CliKind::Claude
            }
        );
        assert_eq!(migrated.llm_usage.notes, ProviderRef::OpenRouter);
        assert_eq!(
            migrated.llm_usage.dictation_cleanup,
            ProviderRef::OpenRouter
        );
        assert_eq!(migrated.llm_endpoints.len(), 1);
        assert_eq!(migrated.llm_endpoints[0].id, "ep-lan");
        assert!(migrated.llm_endpoints[0].has_api_key);
        assert_eq!(
            migrated.llm_cli_levels.get(&CliKind::Claude),
            Some(&StructuredOutputLevel::JsonSchema)
        );
        assert!(!migrated.live_transcription);
        let work = migrated
            .profile_overrides
            .get("work")
            .expect("override kept");
        assert_eq!(work.transcription_provider.as_deref(), Some(PROVIDER_GROQ));
        assert_eq!(
            work.transcription_model.as_deref(),
            Some(DEFAULT_TRANSCRIPTION_MODEL)
        );

        // No secret moved into the Keychain store, and the rewritten file no
        // longer carries the plaintext Venice key.
        for provider in [
            BuiltinProvider::Groq,
            BuiltinProvider::OpenRouter,
            BuiltinProvider::Tavily,
        ] {
            assert_eq!(store.get(provider.secret_account()).unwrap(), None);
        }
        let directory = tempfile::tempdir().unwrap();
        let state = ProviderSettingsState {
            path: directory.path().join("provider-settings.json"),
            settings: Mutex::new(migrated.clone()),
            transcription_intent: Default::default(),
            retired_media,
        };
        save_settings(&state, &migrated).unwrap();
        let rewritten = fs::read_to_string(&state.path).unwrap();
        assert!(!rewritten.contains("veniceApiKey"));
        assert!(!rewritten.contains("legacy-venice-secret"));

        // Restart: the rewritten file loads to the same settings and is not
        // migrated again.
        let (reloaded, migrated_again, _) = settings_from_json(&rewritten, &store).unwrap();
        assert!(!migrated_again);
        assert_eq!(reloaded, migrated);
    }

    #[test]
    fn retired_media_metadata_stays_on_disk_across_a_settings_save() {
        use serde_json::{json, to_value};

        let mut legacy = to_value(default_settings()).unwrap();
        legacy["transcriptionProvider"] = json!("openrouter");
        legacy["transcriptionModel"] = json!("openai/whisper-large-v3");
        legacy["llmUsage"] = json!({
            "chat": { "kind": "cli", "id": "codex" },
            "notes": { "kind": "cli", "id": "claude" },
            "activity": { "kind": "cli", "id": "claude" }
        });
        legacy["llmCliLevels"] = json!({"claude": "json_schema"});
        legacy["providerApiKeysConfigured"] = json!({"openrouter": true, "fal": true});
        legacy["imageModel"] = json!("historical-image");
        legacy["videoModel"] = json!("historical-video");
        legacy["imageSafeMode"] = json!(false);
        legacy["imageSafeModePromptDismissed"] = json!(true);
        legacy["imageSafeModeSetByUser"] = json!(true);
        legacy["profileOverrides"] = json!({
            "work": {
                "transcriptionProvider": "groq",
                "transcriptionModel": "whisper-large-v3",
                "imageModel": "historical-work-image"
            },
            "media-only": { "videoModel": "historical-profile-video" }
        });
        let raw = legacy.to_string();
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("provider-settings.json");
        fs::write(&path, &raw).unwrap();
        let store = crate::llm::secrets::MemorySecretStore::default();
        let (settings, migrated, retired_media) = settings_from_json(&raw, &store).unwrap();
        assert!(
            !migrated,
            "retired media alone must not rewrite settings at startup"
        );
        assert!(!settings.profile_overrides.contains_key("media-only"));
        assert_eq!(settings.transcription_model, "openai/whisper-large-v3");
        assert_eq!(
            effective_settings_for_profile(&settings, "work").transcription_model,
            "whisper-large-v3"
        );
        assert!(settings.provider_api_keys_configured.openrouter);
        let work = ProfileModelOverridesDto::from(&settings.profile_overrides["work"]);
        assert!(to_value(work).unwrap().get("imageModel").is_none());

        let state = ProviderSettingsState {
            path,
            settings: Mutex::new(settings.clone()),
            transcription_intent: Default::default(),
            retired_media,
        };
        let _guard = GLOBAL_SETTINGS_TEST_LOCK.blocking_lock();
        let _restore = RestoreGlobals;
        let updated =
            set_live_transcription_impl(&state, SetLiveTranscriptionRequest { enabled: false })
                .unwrap();
        let dto = to_value(updated).unwrap();
        assert!(dto["providerApiKeysConfigured"].get("fal").is_none());
        let written = fs::read_to_string(&state.path).unwrap();
        let saved: Value = serde_json::from_str(&written).unwrap();
        for key in [
            "imageModel",
            "videoModel",
            "imageSafeMode",
            "imageSafeModePromptDismissed",
            "imageSafeModeSetByUser",
        ] {
            assert!(dto.get(key).is_none());
            assert_eq!(saved[key], legacy[key]);
        }
        assert_eq!(saved["providerApiKeysConfigured"]["fal"], true);
        assert_eq!(saved["profileOverrides"], legacy["profileOverrides"]);

        let (reloaded, migrated, _) = settings_from_json(&written, &store).unwrap();
        assert!(!migrated);
        assert_eq!(
            reloaded,
            ProviderModelSettings {
                live_transcription: false,
                ..settings
            }
        );
    }

    #[test]
    fn legacy_text_models_retire_auto_and_keep_same_model_equivalents() {
        let store = crate::llm::secrets::MemorySecretStore::default();
        for (legacy, expected) in [
            (LEGACY_AUTO_GENERATION_MODEL, DEFAULT_GENERATION_MODEL),
            ("kimi-k2-6", "moonshotai/kimi-k2.6"),
            ("venice-uncensored", DEFAULT_GENERATION_MODEL),
        ] {
            let raw = serde_json::json!({
                "generationProvider": "venice",
                "generationModel": legacy,
                "remoteGenerationModel": legacy
            })
            .to_string();
            let (migrated, _, _) = settings_from_json(&raw, &store).unwrap();
            assert_eq!(migrated.generation_model, expected, "{legacy}");
            assert_eq!(migrated.remote_generation_model, expected, "{legacy}");
        }
    }

    #[test]
    fn retired_provider_usage_and_auto_migrate_once_without_changing_cli_chat() {
        let store = crate::llm::secrets::MemorySecretStore::default();
        let raw = serde_json::json!({
            "generationProvider": "openrouter",
            "generationModel": "open-software/auto",
            "remoteGenerationModel": "open-software/auto",
            "remoteModelsMigrated": true,
            "llmRegistryMigrated": true,
            "llmCliDefaultApplied": true,
            "llmUsage": {
                "chat": { "kind": "cli", "id": "claude" },
                "notes": { "kind": "clovy" },
                "dictationCleanup": { "kind": "clovy" },
                "activity": { "kind": "none" }
            }
        })
        .to_string();
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("provider-settings.json");
        fs::write(&path, raw).unwrap();
        let (settings, migrated, retired_media) =
            settings_from_json(&fs::read_to_string(&path).unwrap(), &store).unwrap();
        let state = ProviderSettingsState {
            transcription_intent: Default::default(),
            path: path.clone(),
            settings: Mutex::new(settings.clone()),
            retired_media,
        };
        save_settings(&state, &settings).unwrap();
        let rewritten = fs::read_to_string(&path).unwrap();
        let written: serde_json::Value = serde_json::from_str(&rewritten).unwrap();
        assert_eq!(written["llmUsage"]["notes"]["kind"], "openrouter");
        assert!(written["llmUsage"].get("dictationCleanup").is_none());
        assert_eq!(settings.generation_model, DEFAULT_GENERATION_MODEL);
        assert_eq!(settings.remote_generation_model, DEFAULT_GENERATION_MODEL);
        assert_eq!(
            settings.llm_usage.chat,
            ProviderRef::Cli {
                id: CliKind::Claude
            }
        );
        assert_eq!(settings.llm_usage.activity, ProviderRef::None);
        assert!(migrated);
        let (reloaded, migrated_again, _) = settings_from_json(&rewritten, &store).unwrap();
        assert!(!migrated_again);
        assert_eq!(reloaded, settings);
    }

    #[test]
    fn retired_dictation_cleanup_choice_is_fixed_openrouter_and_not_persisted() {
        let store = crate::llm::secrets::MemorySecretStore::default();
        let raw = serde_json::json!({
            "remoteModelsMigrated": true,
            "llmRegistryMigrated": true,
            "llmUsage": {
                "chat": { "kind": "cli", "id": "claude" },
                "notes": { "kind": "cli", "id": "codex" },
                "dictationCleanup": { "kind": "endpoint", "id": "old" }
            }
        })
        .to_string();
        let (settings, migrated, _) = settings_from_json(&raw, &store).unwrap();
        assert_eq!(
            settings
                .llm_usage
                .get(crate::llm::registry::LlmUsage::DictationCleanup)
                .key(),
            "openrouter"
        );
        assert_eq!(
            settings.llm_usage.notes,
            ProviderRef::Cli { id: CliKind::Codex }
        );
        assert!(serde_json::to_value(&settings).unwrap()["llmUsage"]
            .get("dictationCleanup")
            .is_none());
        assert!(migrated);
    }

    #[test]
    fn migrated_settings_keep_the_chosen_models_on_every_load() {
        let store = crate::llm::secrets::MemorySecretStore::default();
        let raw = serde_json::json!({
            "transcriptionProvider": PROVIDER_GROQ,
            "transcriptionModel": "whisper-large-v3",
            "generationProvider": PROVIDER_OPENROUTER,
            "generationModel": "moonshotai/kimi-k3",
            "remoteGenerationModel": "moonshotai/kimi-k3",
            "providerApiKeysConfigured": { "groq": true },
            "remoteModelsMigrated": true,
            "llmRegistryMigrated": true
        })
        .to_string();

        let (loaded, migrated_now, _) = settings_from_json(&raw, &store).unwrap();

        // The pre-cut file still needs its missing/native chat choice retired.
        assert!(migrated_now);
        let rewritten = serde_json::to_string(&loaded).unwrap();
        let (reloaded, migrated_again, _) = settings_from_json(&rewritten, &store).unwrap();
        assert!(!migrated_again);
        assert_eq!(reloaded, loaded);
        assert_eq!(loaded.transcription_model, "whisper-large-v3");
        assert_eq!(loaded.generation_model, "moonshotai/kimi-k3");
        assert_eq!(loaded.remote_generation_model, "moonshotai/kimi-k3");
        assert!(loaded.provider_api_keys_configured.groq);
        assert!(!loaded.provider_api_keys_configured.openrouter);
    }

    #[tokio::test]
    async fn text_model_selection_preserves_chat_and_note_engines() {
        let _guard = GLOBAL_SETTINGS_TEST_LOCK.lock().await;
        let previous = current_settings();
        let directory = tempfile::tempdir().unwrap();
        let state = ProviderSettingsState {
            transcription_intent: Default::default(),
            path: directory.path().join("provider-settings.json"),
            settings: Mutex::new(default_settings()),
            retired_media: RetiredMediaMetadata::default(),
        };
        {
            let mut settings = state.settings.lock().unwrap();
            settings.llm_usage.chat = ProviderRef::Cli {
                id: CliKind::Claude,
            };
            settings.llm_usage.notes = ProviderRef::Cli { id: CliKind::Codex };
        }
        set_provider_model_impl(
            &state,
            SetProviderModelRequest {
                mode: ModelMode::Generation,
                model_id: "example/text-only".to_string(),
            },
        )
        .unwrap();
        let settings = state.settings.lock().unwrap();
        assert_eq!(
            settings.llm_usage.chat,
            ProviderRef::Cli {
                id: CliKind::Claude
            }
        );
        assert_eq!(
            settings.llm_usage.notes,
            ProviderRef::Cli { id: CliKind::Codex }
        );
        assert_eq!(settings.remote_generation_model, "example/text-only");
        let saved = fs::read_to_string(&state.path).unwrap();
        let (reloaded, _, _) =
            settings_from_json(&saved, &crate::llm::secrets::MemorySecretStore::default()).unwrap();
        assert_eq!(reloaded.remote_generation_model, "example/text-only");
        assert_eq!(reloaded.llm_usage.chat, settings.llm_usage.chat);
        assert_eq!(reloaded.llm_usage.notes, settings.llm_usage.notes);
        replace_current_settings(previous);
    }

    #[test]
    fn local_transcription_selection_without_model_is_rejected_without_mutation() {
        let state = test_state();
        let error = set_provider_model_impl(
            &state,
            SetProviderModelRequest {
                mode: ModelMode::Transcription,
                model_id: "parakeet-ultra".into(),
            },
        )
        .unwrap_err();
        assert_eq!(
            error.code,
            if local_transcription_supported() {
                "local_model_not_installed"
            } else {
                "local_transcription_unsupported"
            }
        );
        assert_eq!(
            state.settings.lock().unwrap().transcription_provider,
            "groq"
        );
        assert!(!state.path.exists());
    }

    #[test]
    fn local_transcription_saved_choice_and_override_survive_supported_platform_load() {
        let mut settings = default_settings();
        settings.transcription_provider = "local".into();
        settings.transcription_model = "old-model".into();
        settings.profile_overrides.insert(
            "work".into(),
            ProfileModelOverrides {
                transcription_provider: Some("local".into()),
                transcription_model: Some("old-model".into()),
            },
        );
        let settings = sanitize_settings(settings, &default_settings());
        let expected_provider = if local_transcription_supported() {
            "local"
        } else {
            "groq"
        };
        let expected_model = if local_transcription_supported() {
            "parakeet-ultra"
        } else {
            DEFAULT_TRANSCRIPTION_MODEL
        };
        assert_eq!(settings.transcription_provider, expected_provider);
        assert_eq!(settings.transcription_model, expected_model);
        let profile = effective_settings_for_profile(&settings, "work");
        assert_eq!(profile.transcription_provider, expected_provider);
        assert_eq!(profile.transcription_model, expected_model);
    }

    #[test]
    fn openrouter_transcription_selection_round_trips_without_changing_note_model() {
        let _guard = GLOBAL_SETTINGS_TEST_LOCK.blocking_lock();
        let _restore = RestoreGlobals;
        let state = test_state();
        let note_model = state.settings.lock().unwrap().generation_model.clone();
        set_provider_model_impl(
            &state,
            SetProviderModelRequest {
                mode: ModelMode::Transcription,
                model_id: "openai/whisper-large-v3".into(),
            },
        )
        .unwrap();
        let persisted = fs::read_to_string(&state.path).unwrap();
        let (reloaded, _, _) = settings_from_json(
            &persisted,
            &crate::llm::secrets::MemorySecretStore::default(),
        )
        .unwrap();
        assert_eq!(reloaded.transcription_provider, "openrouter");
        assert_eq!(reloaded.transcription_model, "openai/whisper-large-v3");
        assert_eq!(reloaded.generation_model, note_model);
    }

    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    #[test]
    fn installed_local_model_identity_survives_preference_reload() {
        let mut settings = default_settings();
        settings.transcription_provider = "local".into();
        settings.transcription_model = "phonon-2.5".into();
        settings.profile_overrides.insert(
            "work".into(),
            ProfileModelOverrides {
                transcription_provider: Some("local".into()),
                transcription_model: Some("fluid-speech-pro".into()),
            },
        );
        let settings = sanitize_settings(settings, &default_settings());
        assert_eq!(settings.transcription_model, "phonon-2.5");
        assert_eq!(
            effective_settings_for_profile(&settings, "work").transcription_model,
            "fluid-speech-pro"
        );
    }

    #[test]
    fn rejected_transcription_selection_does_not_cancel_the_pending_install() {
        let _guard = GLOBAL_SETTINGS_TEST_LOCK.blocking_lock();
        let _restore = RestoreGlobals;
        let state = test_state();
        let model = transcription::LocalModelId::Phonon25;
        let revision = state
            .transcription_intent
            .lock()
            .unwrap()
            .replace(Some(model));
        let rejected = set_provider_model_impl(
            &state,
            SetProviderModelRequest {
                mode: ModelMode::Transcription,
                model_id: "not-in-the-catalog".into(),
            },
        )
        .unwrap_err();
        assert_eq!(rejected.code, "provider_model_unavailable");
        assert_eq!(
            settings_snapshot(&state).unwrap().transcription_provider,
            PROVIDER_GROQ
        );
        let activated = complete_transcription_install(&state, model, revision, true, |_| Ok(()))
            .unwrap()
            .expect("the pending install can still activate");
        assert_eq!(activated.transcription_model, model.as_str());
        let persisted: ProviderModelSettings =
            serde_json::from_slice(&fs::read(&state.path).unwrap()).unwrap();
        assert_eq!(persisted.transcription_provider, PROVIDER_LOCAL);
        assert_eq!(persisted.transcription_model, model.as_str());
    }

    #[test]
    fn late_download_cannot_overwrite_a_more_recent_openrouter_selection() {
        let _guard = GLOBAL_SETTINGS_TEST_LOCK.blocking_lock();
        let _restore = RestoreGlobals;
        let state = test_state();
        let model = transcription::LocalModelId::Phonon25;
        let revision = state
            .transcription_intent
            .lock()
            .unwrap()
            .replace(Some(model));
        set_provider_model_impl(
            &state,
            SetProviderModelRequest {
                mode: ModelMode::Transcription,
                model_id: "openai/whisper-1".into(),
            },
        )
        .unwrap();
        assert!(
            complete_transcription_install(&state, model, revision, true, |_| {
                panic!("A superseded download attempted activation")
            })
            .unwrap()
            .is_none()
        );
        assert_eq!(
            settings_snapshot(&state).unwrap().transcription_model,
            "openai/whisper-1",
        );
    }

    #[cfg(unix)]
    #[test]
    fn an_install_with_failed_persistence_keeps_the_previous_model_on_disk_and_in_memory() {
        use std::os::unix::fs::PermissionsExt;
        let _guard = GLOBAL_SETTINGS_TEST_LOCK.blocking_lock();
        let _restore = RestoreGlobals;
        let state = test_state();
        let model = transcription::LocalModelId::Phonon25;
        let snapshot = settings_snapshot(&state).unwrap();
        save_settings(&state, &snapshot).unwrap();
        let parent = state.path.parent().unwrap();
        let previous = fs::read(&state.path).unwrap();
        let permissions = fs::metadata(parent).unwrap().permissions();
        let revision = state
            .transcription_intent
            .lock()
            .unwrap()
            .replace(Some(model));
        fs::set_permissions(parent, fs::Permissions::from_mode(0o500)).unwrap();
        let result = complete_transcription_install(&state, model, revision, true, |_| Ok(()));
        fs::set_permissions(parent, permissions).unwrap();
        assert_eq!(result.unwrap_err().code, "provider_settings_save_failed");
        assert_eq!(settings_snapshot(&state).unwrap(), snapshot);
        assert_eq!(fs::read(&state.path).unwrap(), previous);
    }

    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    #[tokio::test]
    async fn local_profile_override_without_model_fails_before_groq_credentials_or_network() {
        let mut settings = default_settings();
        settings.profile_overrides.insert(
            "work".into(),
            ProfileModelOverrides {
                transcription_provider: Some(PROVIDER_LOCAL.into()),
                transcription_model: Some("parakeet-ultra".into()),
            },
        );
        let profile = effective_settings_for_profile(&settings, "work");
        assert_eq!(settings.transcription_provider, PROVIDER_GROQ);
        assert_eq!(profile.transcription_provider, PROVIDER_LOCAL);
        let sentinel = local_transcriber::NetworkSentinel::start().await;
        TEST_ORIGIN
            .scope(sentinel.origin.clone(), async {
                let error =
                    transcription::effective_transcription_backend(&profile, None, true, || {
                        panic!("Missing Local profile model must not read Groq credentials")
                    })
                    .unwrap_err();
                assert_eq!(error.code, "local_model_not_installed");
            })
            .await;
        sentinel.assert_unused();
    }

    #[test]
    fn local_model_removal_persists_groq_and_clears_local_profile_overrides() {
        let _guard = GLOBAL_SETTINGS_TEST_LOCK.blocking_lock();
        let _restore = RestoreGlobals;
        let state = test_state();
        {
            let mut settings = state.settings.lock().unwrap();
            settings.transcription_provider = PROVIDER_LOCAL.into();
            settings.transcription_model = "parakeet-ultra".into();
            settings.profile_overrides.insert(
                "work".into(),
                ProfileModelOverrides {
                    transcription_provider: Some(PROVIDER_LOCAL.into()),
                    transcription_model: Some("parakeet-ultra".into()),
                },
            );
        }
        assert!(remove_local_transcription_selection(
            &state,
            transcription::LocalModelId::ParakeetUltra
        )
        .unwrap());
        let saved: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&state.path).unwrap()).unwrap();
        assert_eq!(saved["transcriptionProvider"], "groq");
        assert_eq!(saved["transcriptionModel"], DEFAULT_TRANSCRIPTION_MODEL);
        assert_eq!(
            saved["profileOverrides"]["work"]["transcriptionProvider"],
            "groq"
        );
        assert!(!remove_local_transcription_selection(
            &state,
            transcription::LocalModelId::ParakeetUltra
        )
        .unwrap());
    }

    #[test]
    fn set_provider_model_accepts_curated_ids_and_rejects_others() {
        let state = test_state();

        let error = set_provider_model_impl(
            &state,
            SetProviderModelRequest {
                mode: ModelMode::Transcription,
                model_id: "nvidia/parakeet-tdt-0.6b-v3".to_string(),
            },
        )
        .unwrap_err();
        assert_eq!(error.code, "provider_model_unavailable");
        assert_eq!(
            state.settings.lock().unwrap().transcription_model,
            DEFAULT_TRANSCRIPTION_MODEL
        );

        let updated = set_provider_model_impl(
            &state,
            SetProviderModelRequest {
                mode: ModelMode::Transcription,
                model_id: " whisper-large-v3 ".to_string(),
            },
        )
        .unwrap();
        assert_eq!(updated.transcription_provider, PROVIDER_GROQ);
        assert_eq!(updated.transcription_model, "whisper-large-v3");

        let updated = set_provider_model_impl(
            &state,
            SetProviderModelRequest {
                mode: ModelMode::Generation,
                model_id: "moonshotai/kimi-k3".to_string(),
            },
        )
        .unwrap();
        assert_eq!(updated.generation_provider, PROVIDER_OPENROUTER);
        assert_eq!(updated.generation_model, "moonshotai/kimi-k3");
        assert_eq!(updated.remote_generation_model, "moonshotai/kimi-k3");
    }

    #[test]
    fn provider_api_key_format_treats_credentials_as_opaque() {
        for key in [
            "gsk_example",
            "sk-or-v1-example",
            "legacy-or-future-key-format",
        ] {
            assert_eq!(
                validate_provider_api_key(BuiltinProvider::Groq, &format!("  {key}  ")).unwrap(),
                key
            );
        }
        assert_eq!(
            validate_provider_api_key(BuiltinProvider::OpenRouter, "   ")
                .unwrap_err()
                .code,
            "provider_api_key_required"
        );
        assert_eq!(
            validate_provider_api_key(BuiltinProvider::Tavily, "invalid\tkey")
                .unwrap_err()
                .code,
            "provider_api_key_invalid"
        );
        assert_eq!(
            validate_provider_api_key(
                BuiltinProvider::OpenRouter,
                &"x".repeat(MAX_PROVIDER_API_KEY_CHARS + 1)
            )
            .unwrap_err()
            .code,
            "provider_api_key_invalid"
        );
    }

    #[tokio::test]
    async fn provider_api_key_save_checks_the_provider_and_stores_only_that_key() {
        use crate::llm::endpoint::test_server;
        let server = test_server::start(std::sync::Arc::new(|_: &serde_json::Value| {
            (200, serde_json::json!({ "data": [] }))
        }))
        .await;
        let state = test_state();
        let store = crate::llm::secrets::MemorySecretStore::default();

        set_provider_api_key_impl(
            &state,
            &store,
            SetProviderApiKeyRequest {
                provider: BuiltinProvider::OpenRouter,
                api_key: " sk-or-user-key ".to_string(),
            },
            &format!("{}/key", server.base_url),
        )
        .await
        .unwrap();
        let dto = set_provider_api_key_impl(
            &state,
            &store,
            SetProviderApiKeyRequest {
                provider: BuiltinProvider::Tavily,
                api_key: "fixture-tavily-value".to_string(),
            },
            &format!("{}/usage", server.base_url),
        )
        .await
        .unwrap();

        // Each verification request carried only its provider's own key.
        assert_eq!(
            server.authorizations.lock().unwrap().as_slice(),
            [
                Some("Bearer sk-or-user-key".to_string()),
                Some("Bearer fixture-tavily-value".to_string()),
            ]
        );
        assert_eq!(
            dto.provider_api_keys_configured,
            ProviderApiKeysConfigured {
                openrouter: true,
                tavily: true,
                ..ProviderApiKeysConfigured::default()
            }
        );
        assert_eq!(
            store.get("builtin:openrouter").unwrap().as_deref(),
            Some("sk-or-user-key")
        );
        assert_eq!(
            store.get("builtin:tavily").unwrap().as_deref(),
            Some("fixture-tavily-value")
        );
        assert_eq!(store.get("builtin:groq").unwrap(), None);

        // The settings file records only that keys exist.
        let on_disk = fs::read_to_string(&state.path).unwrap();
        assert!(!on_disk.contains("sk-or-user-key"));
        assert!(!on_disk.contains("fixture-tavily-value"));
        let reloaded: ProviderModelSettings = serde_json::from_str(&on_disk).unwrap();
        assert_eq!(
            reloaded.provider_api_keys_configured,
            dto.provider_api_keys_configured
        );
        let settings = state.settings.lock().unwrap().clone();
        assert_eq!(
            saved_provider_api_key(&settings, &store, BuiltinProvider::Tavily).unwrap(),
            Some("fixture-tavily-value".to_string())
        );
        assert_eq!(
            saved_provider_api_key(&settings, &store, BuiltinProvider::Groq).unwrap(),
            None
        );
    }

    #[tokio::test]
    async fn provider_api_key_rejected_or_unchecked_changes_nothing() {
        use crate::llm::endpoint::test_server;
        for (status, expected) in [
            (401, "provider_api_key_rejected"),
            (403, "provider_api_key_rejected"),
            (500, "provider_api_key_verification_failed"),
        ] {
            let server = test_server::start(std::sync::Arc::new(move |_: &serde_json::Value| {
                (status, serde_json::json!({ "error": "no" }))
            }))
            .await;
            let state = test_state();
            let store = crate::llm::secrets::MemorySecretStore::default();

            let error = set_provider_api_key_impl(
                &state,
                &store,
                SetProviderApiKeyRequest {
                    provider: BuiltinProvider::Groq,
                    api_key: "gsk-bad".to_string(),
                },
                &format!("{}/models", server.base_url),
            )
            .await
            .unwrap_err();

            assert_eq!(error.code, expected, "status {status}");
            assert!(!error.message.contains("gsk-bad"));
            assert_eq!(store.get("builtin:groq").unwrap(), None);
            assert!(
                !state
                    .settings
                    .lock()
                    .unwrap()
                    .provider_api_keys_configured
                    .groq
            );
            assert!(!state.path.exists(), "nothing is saved on a failed check");
        }
    }

    #[test]
    fn provider_api_key_clear_removes_only_that_provider() {
        let state = test_state();
        let store = crate::llm::secrets::MemorySecretStore::default();
        store.set("builtin:groq", "gsk-user").unwrap();
        store.set("builtin:tavily", "tvly-user").unwrap();
        {
            let mut settings = state.settings.lock().unwrap();
            settings.provider_api_keys_configured.groq = true;
            settings.provider_api_keys_configured.tavily = true;
        }

        let dto = clear_provider_api_key_impl(&state, &store, BuiltinProvider::Groq).unwrap();

        assert!(!dto.provider_api_keys_configured.groq);
        assert!(dto.provider_api_keys_configured.tavily);
        assert_eq!(store.get("builtin:groq").unwrap(), None);
        assert_eq!(
            store.get("builtin:tavily").unwrap().as_deref(),
            Some("tvly-user")
        );
    }

    #[test]
    fn recorded_provider_key_missing_from_the_store_is_an_error_without_fallback() {
        let store = crate::llm::secrets::MemorySecretStore::default();
        let mut settings = default_settings();
        // A key left in the store without a record is never sent.
        store.set("builtin:groq", "gsk-orphan").unwrap();
        assert_eq!(
            saved_provider_api_key(&settings, &store, BuiltinProvider::Groq).unwrap(),
            None
        );

        settings.provider_api_keys_configured.openrouter = true;
        let error =
            saved_provider_api_key(&settings, &store, BuiltinProvider::OpenRouter).unwrap_err();
        assert_eq!(error.code, "provider_api_key_unavailable");
    }

    #[test]
    fn sanitize_settings_keeps_every_curated_groq_model() {
        // A curated non-default id must survive load unchanged; otherwise an
        // allowlist would silently collapse every pick to the default.
        for &model in KNOWN_TRANSCRIPTION_MODELS {
            let settings = ProviderModelSettings {
                transcription_model: model.to_string(),
                ..default_settings()
            };
            let sanitized = sanitize_settings(settings, &default_settings());
            assert_eq!(sanitized.transcription_model, model);
        }
    }

    #[test]
    fn local_base_url_accepts_any_http_host() {
        assert_eq!(
            normalize_local_base_url("http://localhost:11434/v1").unwrap(),
            "http://localhost:11434/v1"
        );
        // Trailing slashes are trimmed.
        assert_eq!(
            normalize_local_base_url("http://127.0.0.1:1234/v1/").unwrap(),
            "http://127.0.0.1:1234/v1"
        );
        // LAN and public hosts are now allowed (bring your own inference).
        assert_eq!(
            normalize_local_base_url("http://192.168.1.5:11434/v1").unwrap(),
            "http://192.168.1.5:11434/v1"
        );
        assert_eq!(
            normalize_local_base_url("https://example.com/v1").unwrap(),
            "https://example.com/v1"
        );
    }

    #[test]
    fn local_base_url_rejects_empty_and_wrong_scheme() {
        assert_eq!(
            normalize_local_base_url("   ").unwrap_err().code,
            "local_model_base_url_required"
        );
        assert_eq!(
            normalize_local_base_url("ftp://localhost/v1")
                .unwrap_err()
                .code,
            "local_model_base_url_invalid"
        );
        assert_eq!(
            normalize_local_base_url("not a url").unwrap_err().code,
            "local_model_base_url_invalid"
        );
    }

    #[test]
    fn llm_invalid_legacy_local_endpoint_does_not_activate() {
        // An unparseable legacy base_url cannot activate an endpoint, and the
        // stale local model id must not leak into the remote fallback.
        let raw = r#"{
            "generationProvider": "local",
            "generationModel": "llama3.1:8b",
            "remoteGenerationModel": "remote-model",
            "remoteModelsMigrated": true,
            "localGeneration": { "baseUrl": "not a url", "modelId": "llama3.1:8b" }
        }"#;
        let store = crate::llm::secrets::MemorySecretStore::default();
        let sanitized = settings_from_json_for_tests(raw, &store).unwrap();
        assert_eq!(sanitized.generation_provider, PROVIDER_OPENROUTER);
        assert!(sanitized.llm_endpoints.is_empty());
        assert_eq!(sanitized.generation_model, "remote-model");
        assert_eq!(sanitized.remote_generation_model, "remote-model");
    }

    #[test]
    fn llm_lan_endpoint_chat_migrates_without_changing_the_text_model() {
        let raw = r#"{
            "generationProvider": "openrouter",
            "generationModel": "remote-model",
            "remoteGenerationModel": "remote-model",
            "remoteModelsMigrated": true,
            "llmRegistryMigrated": true,
            "llmEndpoints": [{
                "id": "lan", "name": "LAN", "baseUrl": "http://192.168.1.5:11434/v1/",
                "modelId": "llama3.1:8b", "hasApiKey": false
            }],
            "llmUsage": { "chat": { "kind": "endpoint", "id": "lan" } }
        }"#;
        let store = crate::llm::secrets::MemorySecretStore::default();
        let sanitized = settings_from_json_for_tests(raw, &store).unwrap();
        assert_eq!(sanitized.generation_provider, PROVIDER_OPENROUTER);
        assert_eq!(sanitized.generation_model, "remote-model");
        assert_eq!(
            sanitized.llm_usage.chat,
            ProviderRef::Cli {
                id: CliKind::Claude
            }
        );
        assert_eq!(sanitized.remote_generation_model, "remote-model");
        assert_eq!(
            sanitized.llm_endpoints[0].base_url,
            "http://192.168.1.5:11434/v1"
        );
        assert_eq!(sanitized.llm_usage.notes, ProviderRef::OpenRouter);
        assert_eq!(sanitized.llm_usage.activity, ProviderRef::None);
        let dto = ProviderModelSettingsDto::from(&sanitized);
        assert!(dto.local_generation.model_id.is_empty());
    }

    #[test]
    fn parse_local_models_response_reads_openai_shape() {
        let body = serde_json::to_vec(&serde_json::json!({
            "object": "list",
            "data": [
                { "id": "llama3.1:8b", "object": "model", "owned_by": "meta" },
                { "id": "qwen2.5:14b" }
            ]
        }))
        .unwrap();
        let probe = parse_local_models_response(&body).unwrap();
        assert_eq!(probe.models, vec!["llama3.1:8b", "qwen2.5:14b"]);
    }

    #[test]
    fn parse_local_models_response_tolerates_missing_ids() {
        let body = serde_json::to_vec(&serde_json::json!({
            "data": [ { "id": "keep" }, { "object": "model" } ]
        }))
        .unwrap();
        let probe = parse_local_models_response(&body).unwrap();
        assert_eq!(probe.models, vec!["keep"]);
    }

    #[test]
    fn parse_local_models_response_rejects_unexpected_shape() {
        assert_eq!(
            parse_local_models_response(b"not json").unwrap_err().code,
            "local_endpoint_invalid_response"
        );
        let body = serde_json::to_vec(&serde_json::json!({ "models": [] })).unwrap();
        assert_eq!(
            parse_local_models_response(&body).unwrap_err().code,
            "local_endpoint_invalid_response"
        );
    }

    fn test_state() -> ProviderSettingsState {
        let dir = std::env::temp_dir().join(format!(
            "clovy-provider-test-{}-{}",
            std::process::id(),
            NEXT_TEST_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        ProviderSettingsState {
            transcription_intent: Default::default(),
            path: dir.join("provider-settings.json"),
            settings: Mutex::new(default_settings()),
            retired_media: RetiredMediaMetadata::default(),
        }
    }

    static NEXT_TEST_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    #[test]
    fn profile_overrides_resolve_fallbacks_and_partial_overrides() {
        let mut settings = default_settings();
        settings.transcription_provider = PROVIDER_GROQ.to_string();
        settings.transcription_model = "global-asr".to_string();
        settings.profile_overrides.insert(
            "writing".to_string(),
            ProfileModelOverrides {
                transcription_provider: Some(PROVIDER_GROQ.to_string()),
                transcription_model: Some("whisper-large-v3".to_string()),
            },
        );

        let effective = effective_settings_for_profile(&settings, "writing");
        assert_eq!(effective.transcription_provider, PROVIDER_GROQ);
        assert_eq!(effective.transcription_model, "whisper-large-v3");

        let effective = effective_settings_for_profile(&settings, "unknown");
        assert_eq!(effective.transcription_provider, PROVIDER_GROQ);
        assert_eq!(effective.transcription_model, "global-asr");

        let effective = effective_settings_for_profile(&settings, "default");
        assert_eq!(effective.transcription_model, "global-asr");
    }

    #[test]
    fn profile_overrides_skip_default_profile() {
        let mut settings = default_settings();
        settings.transcription_model = "global-asr".to_string();
        settings.profile_overrides.insert(
            "default".to_string(),
            ProfileModelOverrides {
                transcription_provider: Some(PROVIDER_GROQ.to_string()),
                transcription_model: Some("profile-asr".to_string()),
            },
        );

        let effective = effective_settings_for_profile(&settings, "default");
        assert_eq!(effective.transcription_provider, PROVIDER_GROQ);
        assert_eq!(effective.transcription_model, "global-asr");
    }

    #[test]
    fn profile_override_commands_write_validate_and_delete() {
        let state = test_state();

        set_profile_model_overrides_impl(
            &state,
            "research",
            ProfileModelOverridesDto {
                transcription_provider: Some(format!(" {PROVIDER_GROQ} ")),
                transcription_model: Some(" whisper-large-v3 ".to_string()),
            },
        )
        .unwrap();

        let saved: ProviderModelSettings =
            serde_json::from_str(&fs::read_to_string(&state.path).unwrap()).unwrap();
        assert_eq!(
            saved
                .profile_overrides
                .get("research")
                .and_then(|overrides| overrides.transcription_provider.as_deref()),
            Some(PROVIDER_GROQ)
        );
        assert_eq!(
            saved
                .profile_overrides
                .get("research")
                .and_then(|overrides| overrides.transcription_model.as_deref()),
            Some("whisper-large-v3")
        );

        set_profile_model_overrides_impl(
            &state,
            "research",
            ProfileModelOverridesDto {
                transcription_provider: None,
                transcription_model: Some(DEFAULT_TRANSCRIPTION_MODEL.to_string()),
            },
        )
        .unwrap();
        assert_eq!(
            state
                .settings
                .lock()
                .unwrap()
                .profile_overrides
                .get("research")
                .and_then(|overrides| overrides.transcription_provider.as_deref()),
            None
        );
        assert_eq!(
            state
                .settings
                .lock()
                .unwrap()
                .profile_overrides
                .get("research")
                .and_then(|overrides| overrides.transcription_model.as_deref()),
            Some(DEFAULT_TRANSCRIPTION_MODEL)
        );

        delete_profile_model_overrides_impl(&state, "research").unwrap();
        assert!(!state
            .settings
            .lock()
            .unwrap()
            .profile_overrides
            .contains_key("research"));

        let error =
            set_profile_model_overrides_impl(&state, "--flag", ProfileModelOverridesDto::default())
                .unwrap_err();
        assert_eq!(error.code, "profile_model_overrides_invalid_profile");
    }

    #[test]
    fn profile_override_commands_reject_default() {
        let state = test_state();

        let error = set_profile_model_overrides_impl(
            &state,
            "default",
            ProfileModelOverridesDto {
                transcription_provider: Some(PROVIDER_GROQ.to_string()),
                transcription_model: None,
            },
        )
        .unwrap_err();

        assert_eq!(error.code, "profile_model_overrides_default_profile");
    }

    #[test]
    fn llm_registry_update_persists_and_rejected_change_leaves_state_untouched() {
        let state = test_state();
        let store = crate::llm::secrets::MemorySecretStore::default();
        let saved = update_llm_registry(&state, |settings| {
            crate::llm::registry::save_endpoint(
                settings,
                crate::llm::registry::SaveEndpointRequest {
                    id: None,
                    name: "Ollama".to_string(),
                    base_url: "http://localhost:11434/v1/".to_string(),
                    model_id: "llama3.1:8b".to_string(),
                    api_key: None,
                    clear_api_key: false,
                },
                &store,
            )
            .map(|_| ())
        })
        .unwrap();
        let id = saved.llm_endpoints[0].id.clone();
        let on_disk: ProviderModelSettings =
            serde_json::from_str(&fs::read_to_string(&state.path).unwrap()).unwrap();
        assert_eq!(
            on_disk.llm_endpoints[0].base_url,
            "http://localhost:11434/v1"
        );

        let error = update_llm_registry(&state, |settings| {
            crate::llm::registry::save_endpoint(
                settings,
                crate::llm::registry::SaveEndpointRequest {
                    id: Some(id.clone()),
                    name: "Ollama".to_string(),
                    base_url: "not a url".to_string(),
                    model_id: "llama3.1:8b".to_string(),
                    api_key: None,
                    clear_api_key: false,
                },
                &store,
            )
            .map(|_| ())
        })
        .unwrap_err();
        assert_eq!(error.code, "local_model_base_url_invalid");
        let settings = state.settings.lock().unwrap();
        assert_eq!(
            settings.llm_endpoints[0].base_url,
            "http://localhost:11434/v1"
        );
    }

    #[tokio::test]
    async fn llm_listing_models_of_a_saved_endpoint_sends_its_keychain_key() {
        use crate::llm::endpoint::test_server;
        let _lock = GLOBAL_SETTINGS_TEST_LOCK.lock().await;
        let server = test_server::start(std::sync::Arc::new(|_: &serde_json::Value| {
            (
                200,
                serde_json::json!({ "data": [{ "id": "private-model" }] }),
            )
        }))
        .await;
        let id = format!("ep-list-{}", uuid::Uuid::new_v4().simple());
        crate::llm::secrets::store()
            .set(&id, "sk-saved-key")
            .unwrap();
        let mut settings = default_settings();
        settings.llm_endpoints.push(LlmEndpointRecord {
            id: id.clone(),
            name: "Private".to_string(),
            base_url: server.base_url.clone(),
            model_id: "private-model".to_string(),
            has_api_key: true,
            structured_output: None,
            rpm_limit: None,
        });
        replace_current_settings_for_tests(settings);

        let probe = probe_local_generation_endpoint(ProbeLocalGenerationEndpointRequest {
            base_url: server.base_url.clone(),
            api_key: String::new(),
            endpoint_id: Some(id.clone()),
        })
        .await;
        replace_current_settings_for_tests(default_settings());
        assert_eq!(probe.unwrap().models, vec!["private-model"]);
        assert_eq!(
            server.authorizations.lock().unwrap().as_slice(),
            [Some("Bearer sk-saved-key".to_string())]
        );
    }

    /// Restores the process-wide settings and login environment on drop.
    struct RestoreGlobals;

    impl Drop for RestoreGlobals {
        fn drop(&mut self) {
            replace_current_settings(default_settings());
            crate::llm::shell_env::set_login_env_for_tests(None);
        }
    }

    fn login_path_with(clis: &[&str]) -> tempfile::TempDir {
        #[cfg(unix)]
        use std::os::unix::fs::PermissionsExt;
        let bin = tempfile::tempdir().unwrap();
        for cli in clis {
            let path = bin.path().join(cli);
            fs::write(&path, "#!/bin/sh\n").unwrap();
            #[cfg(unix)]
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        }
        crate::llm::shell_env::set_login_env_for_tests(Some(
            crate::llm::shell_env::LoginEnv::from_vars(BTreeMap::from([(
                "PATH".to_string(),
                bin.path().display().to_string(),
            )])),
        ));
        bin
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn selection_launch_resolves_only_the_installed_cli_without_invoking_any_cli() {
        let _lock = GLOBAL_SETTINGS_TEST_LOCK.lock().await;
        let _restore = RestoreGlobals;
        for only in [CliKind::Claude, CliKind::Codex, CliKind::Agy] {
            let bin = login_path_with(&[only.id()]);
            let calls = bin.path().join("calls");
            fs::write(
                bin.path().join(only.id()),
                format!(
                    "#!/bin/sh\necho invoked >> '{}'\nexit 91\n",
                    calls.display()
                ),
            )
            .unwrap();
            let state = test_state();
            ensure_cli_default(&state).await;
            let saved: ProviderModelSettings =
                serde_json::from_slice(&fs::read(&state.path).unwrap()).unwrap();
            assert_eq!(crate::chat_engine::default_engine_of(&saved).cli, only);
            assert_eq!(saved.llm_usage.notes, ProviderRef::OpenRouter);
            assert_eq!(saved.generation_model, default_settings().generation_model);
            assert!(!calls.exists(), "selection must never invoke a CLI");
            fs::remove_dir_all(state.path.parent().unwrap()).unwrap();
        }
    }

    #[tokio::test]
    async fn launch_moves_chat_to_the_installed_default_cli_and_saves_it() {
        let _lock = GLOBAL_SETTINGS_TEST_LOCK.lock().await;
        let _restore = RestoreGlobals;
        let _bin = login_path_with(&["codex"]);
        let state = test_state();

        ensure_cli_default(&state).await;

        let settings = settings_snapshot(&state).unwrap();
        assert_eq!(
            settings.llm_usage.chat,
            ProviderRef::Cli { id: CliKind::Codex }
        );
        assert!(settings.llm_cli_default_applied);
        let saved: ProviderModelSettings =
            serde_json::from_str(&fs::read_to_string(&state.path).unwrap()).unwrap();
        assert_eq!(
            saved.llm_usage.chat,
            ProviderRef::Cli { id: CliKind::Codex }
        );
        assert!(saved.llm_cli_default_applied);
        // Chat starts on the CLI; the text picker keeps its independent model.
        assert_eq!(
            crate::chat_engine::default_engine_of(&settings).model_id,
            "__clovy_cli_engine__:codex"
        );
        assert_eq!(
            selected_model_for_mode(&state, ModelMode::Generation).unwrap(),
            settings.generation_model
        );
        let _ = fs::remove_dir_all(state.path.parent().unwrap());
    }

    #[tokio::test]
    async fn launch_without_claude_or_codex_selects_claude_without_remote_fallback() {
        let _lock = GLOBAL_SETTINGS_TEST_LOCK.lock().await;
        let _restore = RestoreGlobals;
        let _bin = login_path_with(&["pi"]);
        let state = test_state();

        ensure_cli_default(&state).await;

        let settings = settings_snapshot(&state).unwrap();
        assert_eq!(
            settings.llm_usage.chat,
            ProviderRef::Cli {
                id: CliKind::Claude
            }
        );
        assert!(settings.llm_cli_default_applied);
        assert!(state.path.exists());
        let engine = crate::chat_engine::default_engine_of(&settings);
        assert_eq!(engine.model_id, "__clovy_cli_engine__:claude");
        assert!(!crate::llm::registry::LlmProvidersDto::from(&settings).cli_default_pending);
    }
}

#[cfg(all(test, unix))]
mod transaction_regressions;
