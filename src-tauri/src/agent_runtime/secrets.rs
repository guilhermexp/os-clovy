//! One-shot secret delivery for agent interruptions.
//!
//! SQLite stores only an opaque reference. The value lives in the operating
//! system keychain until the resumed tool consumes it, then it is deleted.

use crate::domain::types::AppError;
use zeroize::Zeroize;

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn service() -> String {
    crate::app_identity::credential_service("agent-secrets")
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub async fn put(secret_ref: &str, value: String) -> Result<(), AppError> {
    let service = service();
    let secret_ref = secret_ref.to_string();
    tokio::task::spawn_blocking(move || {
        let mut value = value;
        let result =
            crate::credentials::set_password(&service, &secret_ref, &value).map_err(|_| {
                AppError::new(
                    "agent_secret_store_failed",
                    "Clovy could not save the secret securely.",
                )
            });
        value.zeroize();
        result
    })
    .await
    .map_err(|_| {
        AppError::new(
            "agent_secret_store_failed",
            "Clovy could not save the secret securely.",
        )
    })?
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub async fn put(_secret_ref: &str, mut value: String) -> Result<(), AppError> {
    value.zeroize();
    Err(AppError::new(
        "agent_secret_store_unavailable",
        "Secure secret delivery is unavailable on this platform.",
    ))
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub async fn take(secret_ref: &str) -> Result<Option<String>, AppError> {
    let service = service();
    let secret_ref = secret_ref.to_string();
    tokio::task::spawn_blocking(move || {
        crate::credentials::take_password(&service, &secret_ref).map_err(|_| {
            AppError::new(
                "agent_secret_read_failed",
                "Clovy could not read the saved secret.",
            )
        })
    })
    .await
    .map_err(|_| {
        AppError::new(
            "agent_secret_read_failed",
            "Clovy could not read the saved secret.",
        )
    })?
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub async fn take(_secret_ref: &str) -> Result<Option<String>, AppError> {
    Ok(None)
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub async fn delete(secret_ref: &str) -> Result<(), AppError> {
    let service = service();
    let secret_ref = secret_ref.to_string();
    tokio::task::spawn_blocking(move || {
        crate::credentials::delete_password(&service, &secret_ref).map_err(|_| {
            AppError::new(
                "agent_secret_delete_failed",
                "Clovy could not remove the saved secret.",
            )
        })
    })
    .await
    .map_err(|_| {
        AppError::new(
            "agent_secret_delete_failed",
            "Clovy could not remove the saved secret.",
        )
    })?
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub async fn delete(_secret_ref: &str) -> Result<(), AppError> {
    Ok(())
}
