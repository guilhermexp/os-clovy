//! Resolves transcription before reading credentials. There is no provider fallback.
use super::{ProviderModelSettings, DEFAULT_TRANSCRIPTION_MODEL};
use crate::domain::types::AppError;
use std::path::PathBuf;

pub const LOCAL_MODEL: &str = "parakeet-ultra";
pub const OPENROUTER_MODELS: [&str; 3] = [
    "openai/whisper-large-v3-turbo",
    "openai/whisper-large-v3",
    "openai/whisper-1",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LocalModelId {
    WhisperTurbo,
    ParakeetUltra,
    FluidSpeechPro,
    Phonon25,
}

impl LocalModelId {
    pub const ALL: [Self; 4] = [
        Self::WhisperTurbo,
        Self::ParakeetUltra,
        Self::FluidSpeechPro,
        Self::Phonon25,
    ];

    pub fn parse(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|model| model.as_str() == id)
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::WhisperTurbo => "whisper-large-v3-turbo-local",
            Self::ParakeetUltra => LOCAL_MODEL,
            Self::FluidSpeechPro => "fluid-speech-pro",
            Self::Phonon25 => "phonon-2.5",
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::WhisperTurbo => "Whisper large-v3-turbo",
            Self::ParakeetUltra => "Parakeet Ultra",
            Self::FluidSpeechPro => "Fluid Speech Pro",
            Self::Phonon25 => "Phonon-2.5",
        }
    }

    pub const fn minimum_macos(self) -> u32 {
        match self {
            Self::FluidSpeechPro | Self::Phonon25 => 15,
            Self::WhisperTurbo | Self::ParakeetUltra => 14,
        }
    }

    pub fn compatible(self, macos_major: Option<u32>) -> bool {
        macos_major.is_some_and(|major| major >= self.minimum_macos())
    }
}

pub fn macos_major() -> Option<u32> {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        static VERSION: std::sync::OnceLock<Option<u32>> = std::sync::OnceLock::new();
        *VERSION.get_or_init(|| {
            let output = crate::hide_console::command("/usr/bin/sw_vers")
                .arg("-productVersion")
                .output()
                .ok()?;
            if !output.status.success() {
                return None;
            }
            std::str::from_utf8(&output.stdout)
                .ok()?
                .trim()
                .split('.')
                .next()?
                .parse()
                .ok()
        })
    }
    #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
    None
}

pub fn openrouter_model_name(id: &str) -> &str {
    match id {
        "openai/whisper-large-v3-turbo" => "Whisper large-v3-turbo",
        "openai/whisper-large-v3" => "Whisper large-v3",
        "openai/whisper-1" => "Whisper-1",
        _ => id,
    }
}

#[derive(Clone)]
pub struct LocalSelection {
    pub id: LocalModelId,
    pub directory: PathBuf,
    #[cfg(target_os = "macos")]
    _lease: Option<std::sync::Arc<crate::transcription_model::ModelUse>>,
}

impl LocalSelection {
    #[cfg(test)]
    pub(crate) fn fixture(directory: impl Into<PathBuf>, id: LocalModelId) -> Self {
        Self {
            id,
            directory: directory.into(),
            #[cfg(target_os = "macos")]
            _lease: None,
        }
    }
}

#[derive(Clone)]
pub enum TranscriptionBackend {
    Groq { key: String, model: String },
    OpenRouter { key: String, model: String },
    Local(LocalSelection),
}

impl TranscriptionBackend {
    pub fn model_id(&self) -> &str {
        match self {
            Self::Groq { model, .. } | Self::OpenRouter { model, .. } => model,
            Self::Local(model) => model.id.as_str(),
        }
    }

    pub fn provider(&self) -> &'static str {
        match self {
            Self::Groq { .. } => super::PROVIDER_GROQ,
            Self::OpenRouter { .. } => super::PROVIDER_OPENROUTER,
            Self::Local(_) => super::PROVIDER_LOCAL,
        }
    }
}

impl std::fmt::Debug for TranscriptionBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TranscriptionBackend")
            .field("provider", &self.provider())
            .field("model", &self.model_id())
            .finish_non_exhaustive()
    }
}

fn resolve_backend(
    settings: &ProviderModelSettings,
    supported: bool,
    local: impl FnOnce(LocalModelId) -> Result<LocalSelection, AppError>,
    read_key: impl FnOnce(super::BuiltinProvider) -> Result<Option<String>, AppError>,
) -> Result<TranscriptionBackend, AppError> {
    let model = &settings.transcription_model;
    match settings.transcription_provider.as_str() {
        super::PROVIDER_LOCAL if supported => {
            let id = LocalModelId::parse(model).unwrap_or(LocalModelId::ParakeetUltra);
            local(id).map(TranscriptionBackend::Local)
        }
        super::PROVIDER_OPENROUTER => {
            let key = read_key(super::BuiltinProvider::OpenRouter)
                .map_err(super::openrouter::transcription_key_error)?;
            Ok(TranscriptionBackend::OpenRouter {
                key: super::openrouter::require_audio_key(key.as_deref())?.into(),
                model: model.clone(),
            })
        }
        _ => {
            let key = read_key(super::BuiltinProvider::Groq)?;
            Ok(TranscriptionBackend::Groq {
                key: super::groq::require_groq_key(key.as_deref())?.into(),
                model: if settings.transcription_provider == super::PROVIDER_LOCAL {
                    DEFAULT_TRANSCRIPTION_MODEL.into()
                } else {
                    model.clone()
                },
            })
        }
    }
}

#[cfg(test)]
pub fn effective_transcription_backend(
    settings: &ProviderModelSettings,
    model_dir: Option<PathBuf>,
    supported: bool,
    read_key: impl FnOnce() -> Result<Option<String>, AppError>,
) -> Result<TranscriptionBackend, AppError> {
    resolve_backend(
        settings,
        supported,
        |id| {
            model_dir
                .map(|directory| LocalSelection::fixture(directory, id))
                .ok_or_else(|| missing_local(id))
        },
        |_| read_key(),
    )
}

#[cfg(test)]
tokio::task_local! { pub(crate) static TEST_BACKEND: TranscriptionBackend; }

pub fn current_backend() -> Result<TranscriptionBackend, AppError> {
    backend_for_settings(&super::effective_current_settings())
}

pub fn backend_for_settings(
    settings: &ProviderModelSettings,
) -> Result<TranscriptionBackend, AppError> {
    #[cfg(test)]
    if let Ok(backend) = TEST_BACKEND.try_with(Clone::clone) {
        return Ok(backend);
    }
    resolve_backend(
        settings,
        super::local_transcription_supported(),
        acquire_local,
        super::provider_api_key,
    )
}

pub(crate) fn acquire_local(id: LocalModelId) -> Result<LocalSelection, AppError> {
    require_compatible(id)?;
    #[cfg(target_os = "macos")]
    if let Some((directory, lease)) = crate::transcription_model::acquire(id) {
        return Ok(LocalSelection {
            id,
            directory,
            _lease: Some(lease),
        });
    }
    Err(missing_local(id))
}

pub fn require_local_ready_for(id: LocalModelId) -> Result<(), AppError> {
    acquire_local(id).map(|_| ())
}

pub(crate) fn unsupported_local_error() -> AppError {
    AppError::new(
        "local_transcription_unsupported",
        "Local transcription requires macOS 14 or later on Apple Silicon.",
    )
}

pub(crate) fn require_compatible(id: LocalModelId) -> Result<(), AppError> {
    let version = macos_major();
    if id.compatible(version) {
        Ok(())
    } else if LocalModelId::ParakeetUltra.compatible(version) {
        let mut error = AppError::new(
            "local_model_requires_macos",
            format!("{} requires macOS {}.", id.name(), id.minimum_macos()),
        );
        error.details =
            Some(serde_json::json!({ "model": id.name(), "version": id.minimum_macos() }));
        Err(error)
    } else {
        Err(unsupported_local_error())
    }
}

fn missing_local(id: LocalModelId) -> AppError {
    let mut error = AppError::new(
        "local_model_not_installed",
        format!("Install {} in Settings > Models.", id.name()),
    );
    error.details = Some(serde_json::json!({ "model": id.name() }));
    error
}

pub fn warm_dictation_route_if_idle() {
    match super::configured_transcription_provider().as_str() {
        super::PROVIDER_LOCAL => super::local_transcriber::selection_changed(),
        super::PROVIDER_GROQ => super::groq::warm_dictation_route_if_idle(),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_requirements_keep_ultra_available_on_macos_14() {
        assert!(LocalModelId::ParakeetUltra.compatible(Some(14)));
        assert!(LocalModelId::WhisperTurbo.compatible(Some(14)));
        assert!(!LocalModelId::FluidSpeechPro.compatible(Some(14)));
        assert!(!LocalModelId::Phonon25.compatible(Some(14)));
        assert!(LocalModelId::ALL
            .into_iter()
            .all(|id| id.compatible(Some(15))));
        assert!(LocalModelId::ALL.into_iter().all(|id| !id.compatible(None)));
    }

    #[test]
    fn stt_reads_only_the_openrouter_key_and_never_loads_local_weights() {
        let mut settings = super::super::default_settings_for_tests();
        settings.transcription_provider = "openrouter".into();
        settings.transcription_model = "openai/whisper-1".into();
        let error = resolve_backend(
            &settings,
            true,
            |_| panic!("OpenRouter attempted to load local weights"),
            |provider| {
                assert_eq!(provider, super::super::BuiltinProvider::OpenRouter);
                Ok(None)
            },
        )
        .unwrap_err();
        assert_eq!(error.code, "openrouter_audio_key_missing");
    }
    #[test]
    fn local_transcription_ready_never_reads_groq_credentials() {
        let mut settings = super::super::default_settings_for_tests();
        settings.transcription_provider = "local".into();
        let backend = effective_transcription_backend(
            &settings,
            Some(PathBuf::from("/fixture/model")),
            true,
            || panic!("Local read a Groq key"),
        )
        .unwrap();
        assert!(matches!(backend, TranscriptionBackend::Local(_)));
    }
    #[test]
    fn local_transcription_missing_model_fails_without_groq_fallback() {
        let mut settings = super::super::default_settings_for_tests();
        settings.transcription_provider = "local".into();
        let error = effective_transcription_backend(&settings, None, true, || {
            panic!("Missing Local model read Groq credentials")
        })
        .unwrap_err();
        assert_eq!(error.code, "local_model_not_installed");
    }
    #[test]
    fn local_transcription_unsupported_platform_uses_a_groq_model() {
        let mut settings = super::super::default_settings_for_tests();
        settings.transcription_provider = "local".into();
        settings.transcription_model = LOCAL_MODEL.into();
        let backend = effective_transcription_backend(
            &settings,
            Some(PathBuf::from("/fixture/model")),
            false,
            || Ok(Some("fixture-key".into())),
        )
        .unwrap();
        assert!(
            matches!(backend, TranscriptionBackend::Groq { model, .. } if model == DEFAULT_TRANSCRIPTION_MODEL)
        );
    }
    #[test]
    fn local_transcription_resolver_preserves_groq_model_and_key_errors() {
        let mut settings = super::super::default_settings_for_tests();
        settings.transcription_model = "whisper-large-v3".into();
        let backend = effective_transcription_backend(&settings, None, true, || {
            Ok(Some("fixture-key".into()))
        })
        .unwrap();
        assert!(
            matches!(backend, TranscriptionBackend::Groq { model, .. } if model == "whisper-large-v3")
        );
        let error = effective_transcription_backend(&settings, None, true, || {
            Err(AppError::new("key_unreadable", "fixture"))
        })
        .unwrap_err();
        assert_eq!(error.code, "key_unreadable");
    }
}
