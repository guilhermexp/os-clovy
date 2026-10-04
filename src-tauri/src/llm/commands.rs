//! Tauri commands for the provider registry (Settings, Models).

use super::{
    cli::CliKind,
    detect::{self, CliStatus},
    registry::{self, LlmProvidersDto, LlmUsage, ProviderRef, SaveEndpointRequest},
    secrets, shell_env, test_provider,
};
use crate::{domain::types::AppError, providers::ProviderSettingsState};
use serde::Serialize;
use tauri::State;

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LlmProviderTestResultDto {
    pub latency_ms: u64,
    pub structured_output: super::StructuredOutputLevel,
}

#[tauri::command]
pub fn llm_providers(state: State<'_, ProviderSettingsState>) -> Result<LlmProvidersDto, AppError> {
    let settings = crate::providers::settings_snapshot(&state)?;
    Ok(LlmProvidersDto::from(&settings))
}

#[tauri::command]
pub async fn llm_detect_clis() -> Result<Vec<CliStatus>, AppError> {
    let env = shell_env::login_env().await;
    Ok(detect::detect_all(env.as_ref()).await)
}

#[tauri::command]
pub fn llm_save_endpoint(
    state: State<'_, ProviderSettingsState>,
    request: SaveEndpointRequest,
) -> Result<LlmProvidersDto, AppError> {
    let settings = crate::providers::update_llm_registry(&state, |settings| {
        registry::save_endpoint(settings, request, secrets::store()).map(|_| ())
    })?;
    Ok(LlmProvidersDto::from(&settings))
}

#[tauri::command]
pub fn llm_delete_endpoint(
    state: State<'_, ProviderSettingsState>,
    id: String,
) -> Result<LlmProvidersDto, AppError> {
    let settings = crate::providers::update_llm_registry(&state, |settings| {
        registry::delete_endpoint(settings, &id, secrets::store())
    })?;
    Ok(LlmProvidersDto::from(&settings))
}

#[tauri::command]
pub async fn llm_set_usage(
    state: State<'_, ProviderSettingsState>,
    usage: LlmUsage,
    provider: ProviderRef,
) -> Result<LlmProvidersDto, AppError> {
    let env = shell_env::login_env().await;
    let installed = |kind: CliKind| env.which(kind.id()).is_some();
    let settings = crate::providers::update_llm_registry(&state, |settings| {
        registry::set_usage(settings, usage, provider, installed)
    })?;
    Ok(LlmProvidersDto::from(&settings))
}

/// Runs the connection test and stores the measured structured-output level.
#[tauri::command]
pub async fn llm_test_provider(
    state: State<'_, ProviderSettingsState>,
    provider: ProviderRef,
) -> Result<LlmProviderTestResultDto, AppError> {
    let test = test_provider(&provider).await?;
    crate::providers::update_llm_registry(&state, |settings| {
        registry::record_level(
            settings,
            &provider,
            test.level,
            test.tested_endpoint.as_ref(),
        );
        Ok(())
    })?;
    Ok(LlmProviderTestResultDto {
        latency_ms: u64::try_from(test.latency.as_millis()).unwrap_or(u64::MAX),
        structured_output: test.level,
    })
}
