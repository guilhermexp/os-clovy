//! GitHub through the GitHub CLI's own login (ADR-0068). Not implemented yet.

use super::{github, policy, store, ConnectorAccount};
use crate::db::repositories::{ConnectorAccountRecord, Repositories};
use crate::domain::types::AppError;
use crate::llm::shell_env::LoginEnv;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectMode {
    Automatic,
    Explicit,
}

fn not_implemented() -> AppError {
    AppError::new("not_implemented", "not implemented")
}

pub fn is_gh_account(_metadata: &str) -> bool {
    false
}

pub fn auto_connect_enabled(_prefs_dir: &Path) -> bool {
    false
}

pub async fn detect_and_connect(
    _repos: &Repositories,
    _env: &LoginEnv,
    _prefs_dir: &Path,
    _mode: ConnectMode,
) -> Result<Option<ConnectorAccount>, AppError> {
    Err(not_implemented())
}

pub async fn access_token(
    _repos: &Repositories,
    _env: &LoginEnv,
    _account_id: &str,
) -> Result<String, AppError> {
    Err(not_implemented())
}

pub async fn disconnect(
    _repos: &Repositories,
    _prefs_dir: &Path,
    _account_id: &str,
) -> Result<(), AppError> {
    Err(not_implemented())
}

pub async fn grant_bundles(
    _repos: &Repositories,
    _account_id: &str,
    _bundles: &[policy::ScopeBundle],
) -> Result<ConnectorAccount, AppError> {
    Err(not_implemented())
}

pub fn account_from_record(_record: ConnectorAccountRecord) -> ConnectorAccount {
    unimplemented!()
}

#[allow(dead_code)]
fn _uses() -> (fn(&str) -> (), usize) {
    (|_| (), store::test_store_writes())
}

#[cfg(test)]
#[path = "github_cli_tests.rs"]
mod tests;
