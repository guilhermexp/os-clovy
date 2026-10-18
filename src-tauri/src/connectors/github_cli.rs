//! GitHub through the GitHub CLI's own login (ADR-0068).
//!
//! When the user is signed in to `gh` on `github.com` and has no GitHub
//! account in Clovy, Clovy connects one marked "via gh". Its credential is
//! whatever `gh auth token --hostname github.com` prints at the moment of each
//! call: Clovy never copies it into the Keychain, SQLite, a file or a log, and
//! never hands it to an agent CLI. The account row only carries
//! `credentialSource: "gh"` next to the usual non-secret identity. Clovy's
//! `github_read`/`github_write` bundles and write approvals still bound what
//! the tools may do, whatever scopes the `gh` token holds.
//!
//! Disconnecting such an account writes a local preference that stops the
//! automatic connection until the user connects through `gh` again.

use super::{github, policy, ConnectorAccount, ConnectorAccountStatus, ConnectorProvider};
use crate::db::repositories::{ConnectorAccountRecord, Repositories};
use crate::domain::types::AppError;
use crate::llm::shell_env::LoginEnv;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;
use zeroize::{Zeroize, Zeroizing};

/// Each `gh` invocation is a short local process; detection must never hold
/// up app start or the Plugins page.
const GH_TIMEOUT: Duration = Duration::from_secs(2);
const GH_HOSTNAME: &str = "github.com";
const CREDENTIAL_SOURCE_KEY: &str = "credentialSource";
const CREDENTIAL_SOURCE_GH: &str = "gh";
const PREFERENCES_FILE: &str = "github-cli-connector.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectMode {
    /// App start and opening Plugins: honors the disconnect preference and
    /// stays silent when `gh` is missing or signed out.
    Automatic,
    /// The "Connect with GitHub CLI" button: turns the preference back on
    /// and explains what is missing.
    Explicit,
}

// ----- gh process ---------------------------------------------------------------

/// The `gh` binary on the login shell's PATH, run with that shell's
/// environment so it reads the same config the user's terminal does.
struct Gh {
    path: PathBuf,
    vars: BTreeMap<String, String>,
}

impl Gh {
    fn find(env: &LoginEnv) -> Option<Self> {
        Some(Self {
            path: env.which("gh")?,
            vars: env.vars().clone(),
        })
    }

    /// Stdout of one `gh` call when it exits successfully. Stderr is
    /// discarded so no `gh` diagnostic can carry a credential into an error.
    async fn run(&self, args: &[&str]) -> Option<Zeroizing<Vec<u8>>> {
        let mut command = Command::new(&self.path);
        command
            .args(args)
            .env_clear()
            .envs(&self.vars)
            .env("GH_PROMPT_DISABLED", "1")
            .env("NO_COLOR", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        let output = tokio::time::timeout(GH_TIMEOUT, command.output())
            .await
            .ok()?
            .ok()?;
        let stdout = Zeroizing::new(output.stdout);
        output.status.success().then_some(stdout)
    }

    async fn signed_in(&self) -> bool {
        self.run(&["auth", "status", "--hostname", GH_HOSTNAME])
            .await
            .is_some()
    }

    async fn token(&self) -> Option<Zeroizing<String>> {
        let stdout = self
            .run(&["auth", "token", "--hostname", GH_HOSTNAME])
            .await?;
        let mut text = String::from_utf8(stdout.to_vec()).ok()?;
        let token = Zeroizing::new(text.trim().to_string());
        text.zeroize();
        (!token.is_empty()).then_some(token)
    }
}

// ----- preference -----------------------------------------------------------------

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Preferences {
    github_cli_auto_connect: bool,
}

/// `githubCliAutoConnect`, on unless the user disconnected a "via gh"
/// account. An unreadable file reads as the default.
pub fn auto_connect_enabled(prefs_dir: &Path) -> bool {
    std::fs::read(prefs_dir.join(PREFERENCES_FILE))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Preferences>(&bytes).ok())
        .map(|prefs| prefs.github_cli_auto_connect)
        .unwrap_or(true)
}

fn set_auto_connect(prefs_dir: &Path, enabled: bool) -> Result<(), AppError> {
    let failed = |error: std::io::Error| {
        AppError::new("connector_preferences_write_failed", error.to_string())
    };
    std::fs::create_dir_all(prefs_dir).map_err(failed)?;
    let json = serde_json::to_vec(&Preferences {
        github_cli_auto_connect: enabled,
    })
    .map_err(|error| AppError::new("connector_preferences_write_failed", error.to_string()))?;
    std::fs::write(prefs_dir.join(PREFERENCES_FILE), json).map_err(failed)
}

// ----- accounts ---------------------------------------------------------------------

pub fn is_gh_account(metadata: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(metadata)
        .ok()
        .and_then(|value| {
            value
                .get(CREDENTIAL_SOURCE_KEY)
                .and_then(serde_json::Value::as_str)
                .map(|source| source == CREDENTIAL_SOURCE_GH)
        })
        .unwrap_or(false)
}

fn metadata_json(identity: &github::GithubIdentity) -> String {
    let mut map = serde_json::Map::new();
    for (key, value) in [
        ("actorLogin", &identity.login),
        ("actorName", &identity.name),
    ] {
        if !value.is_empty() {
            map.insert(key.to_string(), serde_json::Value::String(value.clone()));
        }
    }
    map.insert(
        CREDENTIAL_SOURCE_KEY.to_string(),
        serde_json::Value::String(CREDENTIAL_SOURCE_GH.to_string()),
    );
    serde_json::Value::Object(map).to_string()
}

pub fn account_from_record(record: ConnectorAccountRecord) -> ConnectorAccount {
    ConnectorAccount {
        via_gh: is_gh_account(&record.metadata),
        provider: ConnectorProvider::from_db(&record.provider),
        status: ConnectorAccountStatus::from_db(&record.status),
        account_id: record.account_id,
        email: record.email,
        scopes: record.scopes,
        workspace_name: None,
        workspace_url_key: None,
        selected_teams: Vec::new(),
    }
}

fn cli_missing_error() -> AppError {
    AppError::new(
        "connector_github_cli_missing",
        "GitHub CLI (gh) was not found on this Mac. Install it, run gh auth login, then try again.",
    )
}

fn cli_signed_out_error() -> AppError {
    AppError::new(
        "connector_github_cli_logged_out",
        "GitHub CLI is not signed in to github.com. Run gh auth login, then try again.",
    )
}

fn reconnect_required_error() -> AppError {
    AppError::new(
        "connector_reconnect_required",
        "GitHub CLI is no longer signed in. Run gh auth login, then reconnect GitHub in settings.",
    )
}

async fn stored(repos: &Repositories, account_id: &str) -> Result<ConnectorAccount, AppError> {
    repos
        .get_connector_account(account_id)
        .await?
        .map(account_from_record)
        .ok_or_else(|| {
            AppError::new(
                "connector_not_connected",
                "This GitHub account is not connected.",
            )
        })
}

/// Connect "GitHub (via gh)" when there is no GitHub account yet, and heal a
/// "via gh" account that went `reconnect_required` once `gh` is signed in
/// again as the same user. An account connected through the GitHub App is
/// left alone. Only the read bundle is granted.
pub async fn detect_and_connect(
    repos: &Repositories,
    env: &LoginEnv,
    prefs_dir: &Path,
    mode: ConnectMode,
) -> Result<Option<ConnectorAccount>, AppError> {
    let explicit = mode == ConnectMode::Explicit;
    if !explicit && !auto_connect_enabled(prefs_dir) {
        return Ok(None);
    }
    let existing = repos
        .list_connector_accounts()
        .await?
        .into_iter()
        .find(|record| record.provider == ConnectorProvider::Github.as_str());
    if let Some(record) = existing.as_ref() {
        if !is_gh_account(&record.metadata) {
            if explicit {
                return Err(AppError::new(
                    "connector_single_account_only",
                    format!(
                        "Clovy local mode uses one GitHub account at a time. Disconnect {} before connecting another.",
                        record.email
                    ),
                ));
            }
            return Ok(None);
        }
    }

    let Some(gh) = Gh::find(env) else {
        return if explicit {
            Err(cli_missing_error())
        } else {
            Ok(None)
        };
    };
    let token = if gh.signed_in().await {
        gh.token().await
    } else {
        None
    };
    let Some(token) = token else {
        return if explicit {
            Err(cli_signed_out_error())
        } else {
            Ok(existing.map(account_from_record))
        };
    };
    let identity = match github::fetch_identity(&token).await {
        Ok(identity) => identity,
        Err(error) if explicit => return Err(error),
        Err(error) => {
            tracing::warn!(error_code = %error.code, "github cli identity check failed");
            return Ok(existing.map(account_from_record));
        }
    };
    drop(token);

    if let Some(record) = existing {
        // Heal only the same GitHub user; a different gh login needs the
        // user to disconnect first, like any account switch.
        if !record.account_id.eq_ignore_ascii_case(&identity.user_id) {
            return Ok(Some(account_from_record(record)));
        }
        repos
            .set_connector_account_status(
                &record.account_id,
                ConnectorAccountStatus::Connected.as_str(),
            )
            .await?;
    } else {
        let scopes = vec![policy::GITHUB_READ.to_string()];
        repos
            .upsert_connector_account(
                &identity.user_id,
                ConnectorProvider::Github.as_str(),
                &identity.login,
                &scopes,
                ConnectorAccountStatus::Connected.as_str(),
                &metadata_json(&identity),
            )
            .await?;
    }
    if explicit {
        set_auto_connect(prefs_dir, true)?;
    }
    stored(repos, &identity.user_id).await.map(Some)
}

/// The `gh` token for one call. When `gh` no longer yields one the account
/// turns `reconnect_required` and the call fails with that code.
pub async fn access_token(
    repos: &Repositories,
    env: &LoginEnv,
    account_id: &str,
) -> Result<String, AppError> {
    if let Some(gh) = Gh::find(env) {
        if let Some(token) = gh.token().await {
            return Ok(token.as_str().to_string());
        }
    }
    repos
        .set_connector_account_status(
            account_id,
            ConnectorAccountStatus::ReconnectRequired.as_str(),
        )
        .await?;
    Err(reconnect_required_error())
}

/// Disconnect a "via gh" account: drop the row (there is no Clovy-held
/// credential to revoke or delete) and stop the automatic connection.
pub async fn disconnect(
    repos: &Repositories,
    prefs_dir: &Path,
    account_id: &str,
) -> Result<(), AppError> {
    set_auto_connect(prefs_dir, false)?;
    repos.delete_connector_account(account_id).await?;
    Ok(())
}

/// "Add access" for a "via gh" account: the `gh` token already reaches
/// GitHub, so adding a bundle is a Clovy-side grant with no device flow.
pub async fn grant_bundles(
    repos: &Repositories,
    account_id: &str,
    bundles: &[policy::ScopeBundle],
) -> Result<ConnectorAccount, AppError> {
    let record = repos
        .get_connector_account(account_id)
        .await?
        .filter(|record| is_gh_account(&record.metadata))
        .ok_or_else(|| {
            AppError::new(
                "connector_not_connected",
                "This GitHub account is not connected.",
            )
        })?;
    let requested = policy::requested_github_scopes(bundles);
    let scopes = policy::resolve_granted_scopes(None, &requested, Some(&record.scopes));
    repos
        .upsert_connector_account(
            &record.account_id,
            &record.provider,
            &record.email,
            &scopes,
            &record.status,
            &record.metadata,
        )
        .await?;
    stored(repos, account_id).await
}

#[cfg(test)]
#[path = "github_cli_tests.rs"]
mod tests;
