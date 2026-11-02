//! The host snapshots the data partition in create_run's INSERT ... SELECT,
//! together with approvalMode. Neither a caller nor the visible app view can
//! choose it. Missing/invalid snapshots are never upgraded to current authority.

use crate::domain::types::AppError;
use sqlx::{query::query, row::Row};
use sqlx_sqlite::SqlitePool;

pub(crate) async fn resolve(
    pool: &SqlitePool,
    session_id: &str,
    run_id: &str,
) -> Result<String, AppError> {
    let row = query(
        "SELECT r.run_config_json, COALESCE(p.profile, 'default') AS profile
         FROM agent_runs r JOIN agent_sessions s ON s.id = r.session_id
         LEFT JOIN session_profiles p ON p.session_id = s.id
         WHERE r.id = ? AND s.id = ?
           AND r.status IN ('queued', 'running', 'waiting_for_user')",
    )
    .bind(run_id)
    .bind(session_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(expired)?;
    let config = row
        .get::<Option<String>, _>("run_config_json")
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .ok_or_else(expired)?;
    let partition = config
        .get("dataPartition")
        .and_then(serde_json::Value::as_str)
        .filter(|s| !s.is_empty() && s.trim() == *s)
        .ok_or_else(expired)?;
    if row.get::<String, _>("profile") != partition {
        return Err(expired());
    }
    Ok(partition.to_owned())
}

pub(crate) async fn revalidate(
    pool: &SqlitePool,
    session_id: &str,
    run_id: &str,
    partition: &str,
) -> Result<(), AppError> {
    if resolve(pool, session_id, run_id).await? != partition {
        return Err(expired());
    }
    Ok(())
}

fn expired() -> AppError {
    AppError::new("turn_expired", "The bound run or data partition changed.")
}

#[cfg(test)]
#[path = "run_partition_tests.rs"]
mod tests;
