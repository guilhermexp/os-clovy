//! Cleanup of one-shot secrets created by the retired native chat engine.
//! The SQL queue contains opaque references only, never credential values.
use crate::domain::types::AppError;
use sqlx::row::Row;
use sqlx_sqlite::SqlitePool;

fn services(identifier: &str, debug: bool) -> Vec<String> {
    let mut services = vec![crate::app_identity::credential_service_for(
        identifier,
        debug,
        "agent-secrets",
    )];
    // A proof identifier must never reach the owner's credentials or a legacy service.
    if identifier == crate::app_identity::RELEASE_APP_IDENTIFIER {
        for legacy in ["co.opensoftware.clovy", "co.opensoftware.june"] {
            if debug {
                services.push(format!("{legacy}.dev.agent-secrets"));
            } else {
                services.push(format!("{legacy}.agent-secrets"));
            }
        }
    }
    services
}

pub(crate) async fn cleanup_secrets(pool: &SqlitePool) -> Result<(), AppError> {
    let refs = sqlx::query::query(
        "SELECT secret_ref FROM agent_secret_retirements WHERE completed_at IS NULL",
    )
    .fetch_all(pool)
    .await?;
    let services = services(crate::app_identity::APP_IDENTIFIER, cfg!(debug_assertions));
    for row in refs {
        let secret_ref: String = row.get("secret_ref");
        let owned_ref = secret_ref.clone();
        let services = services.clone();
        let removed = tokio::task::spawn_blocking(move || {
            services
                .iter()
                .all(|service| crate::credentials::delete_password(service, &owned_ref).is_ok())
        })
        .await
        .unwrap_or(false);
        if removed {
            sqlx::query::query(
                "UPDATE agent_secret_retirements SET completed_at = ? WHERE secret_ref = ?",
            )
            .bind(crate::db::repositories::timestamp())
            .bind(secret_ref)
            .execute(pool)
            .await?;
        } else {
            tracing::warn!("Retired agent secret cleanup will retry on the next launch");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn proof_cleanup_never_opens_owner_or_legacy_services() {
        assert_eq!(
            services("dev.guilhermexp.clovy.testproof", true),
            vec!["dev.guilhermexp.clovy.testproof-dev.agent-secrets"]
        );
        assert!(services(crate::app_identity::RELEASE_APP_IDENTIFIER, true)
            .iter()
            .all(|name| name.contains("-dev.") || name.contains(".dev.")));
        assert!(services(crate::app_identity::RELEASE_APP_IDENTIFIER, false)
            .contains(&"co.opensoftware.clovy.agent-secrets".to_string()));
    }
}
