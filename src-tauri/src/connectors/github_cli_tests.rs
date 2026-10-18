//! GitHub via the GitHub CLI: a fake `gh` on a login PATH and a fake HTTP
//! server in place of `api.github.com`. Nothing here touches the real `gh`,
//! the network, or the token store.

use super::*;
use crate::connectors::{policy, store, ConnectorAccountStatus, ConnectorProvider};
use crate::llm::shell_env::LoginEnv;
use sqlx_sqlite::SqlitePoolOptions;
use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const TOKEN: &str = "gho_fake_cli_token_0123456789";
const USER_ID: &str = "4242";

/// A fake `gh`: logged in while `<dir>/token` exists (its content is the
/// token), and every invocation is appended to `<dir>/calls`.
struct FakeGh {
    dir: tempfile::TempDir,
}

impl FakeGh {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("gh");
        std::fs::write(
            &script,
            r#"#!/bin/sh
dir="${0%/*}"
echo "$*" >> "$dir/calls"
case "$1 $2 $3 $4" in
  "auth status --hostname github.com")
    if [ -f "$dir/token" ]; then echo "Logged in to github.com account octo" >&2; exit 0; fi
    echo "You are not logged into any GitHub hosts." >&2; exit 1 ;;
  "auth token --hostname github.com")
    if [ -f "$dir/token" ]; then IFS= read -r token < "$dir/token"; printf '%s\n' "$token"; exit 0; fi
    echo "no oauth token found for github.com" >&2; exit 1 ;;
esac
exit 2
"#,
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        Self { dir }
    }

    fn log_in(&self) {
        std::fs::write(self.dir.path().join("token"), TOKEN).unwrap();
    }

    fn log_out(&self) {
        let _ = std::fs::remove_file(self.dir.path().join("token"));
    }

    fn calls(&self, subcommand: &str) -> usize {
        std::fs::read_to_string(self.dir.path().join("calls"))
            .unwrap_or_default()
            .lines()
            .filter(|line| line.starts_with(subcommand))
            .count()
    }

    fn env(&self) -> LoginEnv {
        login_env_with_path(self.dir.path())
    }
}

/// The login PATH is only `dir`, so the box's own `gh` is never found; the
/// fake script uses shell builtins alone.
fn login_env_with_path(dir: &Path) -> LoginEnv {
    let mut vars = BTreeMap::new();
    vars.insert("PATH".to_string(), dir.display().to_string());
    vars.insert("HOME".to_string(), dir.display().to_string());
    LoginEnv::from_vars(vars)
}

/// A fake `api.github.com`: answers `/user` and `/user/repos` only for the
/// fake token, and records the paths it was asked for.
struct FakeGithub {
    paths: Arc<Mutex<Vec<String>>>,
    _server: tokio::task::JoinHandle<()>,
}

impl FakeGithub {
    async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        github::set_test_api_base(Some(base));
        let paths = Arc::new(Mutex::new(Vec::new()));
        let recorded = paths.clone();
        let server = tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let recorded = recorded.clone();
                tokio::spawn(async move {
                    let mut buffer = vec![0u8; 16 * 1024];
                    let read = stream.read(&mut buffer).await.unwrap_or(0);
                    let request = String::from_utf8_lossy(&buffer[..read]).to_string();
                    let path = request
                        .lines()
                        .next()
                        .and_then(|line| line.split_whitespace().nth(1))
                        .unwrap_or_default()
                        .to_string();
                    let authorized = request.lines().any(|line| {
                        line.eq_ignore_ascii_case(&format!("authorization: Bearer {TOKEN}"))
                    });
                    recorded.lock().unwrap().push(path.clone());
                    let (status, body) = if !authorized {
                        (
                            "401 Unauthorized",
                            r#"{"message":"Bad credentials"}"#.to_string(),
                        )
                    } else if path == "/user" {
                        (
                            "200 OK",
                            format!(r#"{{"id":{USER_ID},"login":"octo","name":"Octo Cat"}}"#),
                        )
                    } else if path.starts_with("/user/repos") {
                        (
                            "200 OK",
                            r#"[{"id":1,"name":"hello","full_name":"octo/hello","private":false,"html_url":"https://github.com/octo/hello","description":null,"default_branch":"main","owner":{"login":"octo"}}]"#
                                .to_string(),
                        )
                    } else {
                        ("404 Not Found", r#"{"message":"Not Found"}"#.to_string())
                    };
                    let response = format!(
                        "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = stream.write_all(response.as_bytes()).await;
                    let _ = stream.shutdown().await;
                });
            }
        });
        Self {
            paths,
            _server: server,
        }
    }

    fn paths(&self) -> Vec<String> {
        self.paths.lock().unwrap().clone()
    }
}

impl Drop for FakeGithub {
    fn drop(&mut self) {
        github::set_test_api_base(None);
    }
}

async fn repositories() -> crate::db::repositories::Repositories {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("sqlite memory");
    crate::db::migrations::run_migrations(&pool)
        .await
        .expect("migrations");
    crate::db::repositories::Repositories::new(pool)
}

fn prefs_dir() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().to_path_buf();
    (dir, path)
}

async fn app_account(repos: &crate::db::repositories::Repositories) {
    repos
        .upsert_connector_account(
            "999",
            ConnectorProvider::Github.as_str(),
            "app-user",
            &[policy::GITHUB_READ.to_string()],
            ConnectorAccountStatus::Connected.as_str(),
            r#"{"actorLogin":"app-user"}"#,
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn connects_on_its_own_with_read_only_when_gh_is_logged_in_and_no_account_exists() {
    let gh = FakeGh::new();
    gh.log_in();
    let api = FakeGithub::start().await;
    let repos = repositories().await;
    let (_keep, prefs) = prefs_dir();
    store::reset_test_store_writes();

    let account = detect_and_connect(&repos, &gh.env(), &prefs, ConnectMode::Automatic)
        .await
        .unwrap()
        .expect("connected via gh");

    assert_eq!(account.account_id, USER_ID);
    assert_eq!(account.provider, ConnectorProvider::Github);
    assert_eq!(account.email, "octo");
    assert!(account.via_gh);
    assert_eq!(account.status, ConnectorAccountStatus::Connected);
    assert_eq!(account.scopes, vec![policy::GITHUB_READ.to_string()]);
    assert_eq!(api.paths(), vec!["/user".to_string()]);

    let record = repos.get_connector_account(USER_ID).await.unwrap().unwrap();
    assert!(is_gh_account(&record.metadata));
    assert!(!record.metadata.contains(TOKEN), "no token in the metadata");
    assert_eq!(
        store::test_store_writes(),
        0,
        "nothing goes to the token store"
    );
}

#[tokio::test]
async fn does_not_connect_when_a_github_app_account_exists() {
    let gh = FakeGh::new();
    gh.log_in();
    let _api = FakeGithub::start().await;
    let repos = repositories().await;
    app_account(&repos).await;
    let (_keep, prefs) = prefs_dir();

    let outcome = detect_and_connect(&repos, &gh.env(), &prefs, ConnectMode::Automatic)
        .await
        .unwrap();

    assert!(outcome.is_none());
    assert_eq!(repos.list_connector_accounts().await.unwrap().len(), 1);
    assert_eq!(gh.calls("auth token"), 0);
}

#[tokio::test]
async fn does_nothing_without_gh_or_when_gh_is_logged_out() {
    let _api = FakeGithub::start().await;
    let repos = repositories().await;
    let (_keep, prefs) = prefs_dir();
    let empty = tempfile::tempdir().unwrap();

    let missing = detect_and_connect(
        &repos,
        &login_env_with_path(empty.path()),
        &prefs,
        ConnectMode::Automatic,
    )
    .await
    .unwrap();
    assert!(missing.is_none());

    let gh = FakeGh::new();
    let logged_out = detect_and_connect(&repos, &gh.env(), &prefs, ConnectMode::Automatic)
        .await
        .unwrap();
    assert!(logged_out.is_none());
    assert!(repos.list_connector_accounts().await.unwrap().is_empty());
}

#[tokio::test]
async fn reads_the_token_from_gh_on_every_call_and_never_stores_it() {
    let gh = FakeGh::new();
    gh.log_in();
    let _api = FakeGithub::start().await;
    let repos = repositories().await;
    let (_keep, prefs) = prefs_dir();
    detect_and_connect(&repos, &gh.env(), &prefs, ConnectMode::Automatic)
        .await
        .unwrap()
        .unwrap();
    store::reset_test_store_writes();
    let before = gh.calls("auth token");

    let first = access_token(&repos, &gh.env(), USER_ID).await.unwrap();
    let second = access_token(&repos, &gh.env(), USER_ID).await.unwrap();

    assert_eq!(first, TOKEN);
    assert_eq!(second, TOKEN);
    assert_eq!(gh.calls("auth token") - before, 2);
    assert_eq!(store::test_store_writes(), 0);
}

#[tokio::test]
async fn gh_logout_turns_the_account_into_reconnect_required() {
    let gh = FakeGh::new();
    gh.log_in();
    let _api = FakeGithub::start().await;
    let repos = repositories().await;
    let (_keep, prefs) = prefs_dir();
    detect_and_connect(&repos, &gh.env(), &prefs, ConnectMode::Automatic)
        .await
        .unwrap()
        .unwrap();

    gh.log_out();
    let error = access_token(&repos, &gh.env(), USER_ID).await.unwrap_err();

    assert_eq!(error.code, "connector_reconnect_required");
    assert!(!error.message.contains(TOKEN));
    let record = repos.get_connector_account(USER_ID).await.unwrap().unwrap();
    assert_eq!(
        record.status,
        ConnectorAccountStatus::ReconnectRequired.as_str()
    );

    // Logging back in heals it on the next detection.
    gh.log_in();
    detect_and_connect(&repos, &gh.env(), &prefs, ConnectMode::Automatic)
        .await
        .unwrap();
    let record = repos.get_connector_account(USER_ID).await.unwrap().unwrap();
    assert_eq!(record.status, ConnectorAccountStatus::Connected.as_str());
}

#[tokio::test]
async fn disconnecting_stops_automatic_reconnection_until_an_explicit_connect() {
    let gh = FakeGh::new();
    gh.log_in();
    let _api = FakeGithub::start().await;
    let repos = repositories().await;
    let (_keep, prefs) = prefs_dir();
    assert!(auto_connect_enabled(&prefs));
    detect_and_connect(&repos, &gh.env(), &prefs, ConnectMode::Automatic)
        .await
        .unwrap()
        .unwrap();

    disconnect(&repos, &prefs, USER_ID).await.unwrap();
    assert!(!auto_connect_enabled(&prefs));
    assert!(repos
        .get_connector_account(USER_ID)
        .await
        .unwrap()
        .is_none());

    let automatic = detect_and_connect(&repos, &gh.env(), &prefs, ConnectMode::Automatic)
        .await
        .unwrap();
    assert!(automatic.is_none());

    let explicit = detect_and_connect(&repos, &gh.env(), &prefs, ConnectMode::Explicit)
        .await
        .unwrap();
    assert!(explicit.is_some_and(|account| account.via_gh));
    assert!(auto_connect_enabled(&prefs));
}

#[tokio::test]
async fn explicit_connect_explains_a_missing_or_logged_out_gh() {
    let _api = FakeGithub::start().await;
    let repos = repositories().await;
    let (_keep, prefs) = prefs_dir();
    let empty = tempfile::tempdir().unwrap();

    let missing = detect_and_connect(
        &repos,
        &login_env_with_path(empty.path()),
        &prefs,
        ConnectMode::Explicit,
    )
    .await
    .unwrap_err();
    assert_eq!(missing.code, "connector_github_cli_missing");

    let gh = FakeGh::new();
    let logged_out = detect_and_connect(&repos, &gh.env(), &prefs, ConnectMode::Explicit)
        .await
        .unwrap_err();
    assert_eq!(logged_out.code, "connector_github_cli_logged_out");
}

#[tokio::test]
async fn write_needs_the_write_bundle_like_an_app_account() {
    let gh = FakeGh::new();
    gh.log_in();
    let _api = FakeGithub::start().await;
    let repos = repositories().await;
    let (_keep, prefs) = prefs_dir();
    detect_and_connect(&repos, &gh.env(), &prefs, ConnectMode::Automatic)
        .await
        .unwrap()
        .unwrap();

    let accounts = vec![account_from_record(
        repos.get_connector_account(USER_ID).await.unwrap().unwrap(),
    )];
    let refused = crate::connectors::check_account_grant(
        &accounts,
        USER_ID,
        ConnectorProvider::Github,
        policy::GITHUB_WRITE,
        false,
    )
    .unwrap_err();
    assert_eq!(refused.code, "connector_scope_missing");

    let write = policy::ScopeBundle::from_name("github_write").unwrap();
    let granted = grant_bundles(&repos, USER_ID, &[write]).await.unwrap();
    assert!(granted.via_gh);
    assert!(granted.scopes.contains(&policy::GITHUB_WRITE.to_string()));
    let accounts = vec![account_from_record(
        repos.get_connector_account(USER_ID).await.unwrap().unwrap(),
    )];
    crate::connectors::check_account_grant(
        &accounts,
        USER_ID,
        ConnectorProvider::Github,
        policy::GITHUB_WRITE,
        false,
    )
    .unwrap();
}

#[tokio::test]
async fn lists_repositories_of_a_gh_account_from_the_user_repos_endpoint() {
    let _api = FakeGithub::start().await;

    let list = github::list_user_repositories(TOKEN).await.unwrap();

    assert_eq!(list.repositories.len(), 1);
    assert_eq!(list.repositories[0].full_name, "octo/hello");
    assert!(!list.truncated);
    assert!(_api
        .paths()
        .iter()
        .all(|path| path.starts_with("/user/repos")));
}
