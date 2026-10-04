//! The user's login-shell environment.
//!
//! A GUI app launched from Finder or the Dock does not inherit the shell's
//! PATH, so CLIs installed in `~/.local/bin` or by version managers are
//! invisible to it. Clovy captures the environment of `$SHELL -l` once and
//! uses it, merged over its own environment, both to resolve CLI executables
//! and as the base environment of every CLI it runs. HOME and profile
//! variables such as `PI_CODING_AGENT_DIR` therefore reach the CLI exactly as
//! the user's shell sets them; Clovy never rewrites them.
//!
//! `-i` is deliberately not used: interactive startup files can prompt, print
//! banners, or wait on a TTY.

use super::process::{self, ProcessSpec};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::sync::OnceCell;

const CAPTURE_TIMEOUT: Duration = Duration::from_secs(6);
const ENV_START_MARKER: &str = "__CLOVY_LOGIN_ENV_START__";
const ENV_END_MARKER: &str = "__CLOVY_LOGIN_ENV_END__";
const DEFAULT_SHELL: &str = "/bin/zsh";

/// Resolved environment for running user CLIs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LoginEnv {
    vars: BTreeMap<String, String>,
}

impl LoginEnv {
    pub fn from_vars(vars: BTreeMap<String, String>) -> Self {
        Self { vars }
    }

    pub fn vars(&self) -> &BTreeMap<String, String> {
        &self.vars
    }

    pub fn home(&self) -> Option<PathBuf> {
        self.vars
            .get("HOME")
            .filter(|home| !home.is_empty())
            .map(PathBuf::from)
    }

    pub fn path_dirs(&self) -> Vec<PathBuf> {
        self.vars
            .get("PATH")
            .map(|path| std::env::split_paths(&OsString::from(path)).collect())
            .unwrap_or_default()
    }

    /// First executable named `binary` on this environment's PATH.
    pub fn which(&self, binary: &str) -> Option<PathBuf> {
        self.path_dirs()
            .into_iter()
            .map(|dir| dir.join(binary))
            .find(|candidate| is_executable(candidate))
    }
}

/// The login environment, captured once per app run.
pub async fn login_env() -> Arc<LoginEnv> {
    #[cfg(test)]
    if let Some(env) = TEST_LOGIN_ENV
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
    {
        return env;
    }
    static LOGIN_ENV: OnceCell<Arc<LoginEnv>> = OnceCell::const_new();
    LOGIN_ENV
        .get_or_init(|| async {
            let process_env: BTreeMap<String, String> = std::env::vars().collect();
            let shell = process_env
                .get("SHELL")
                .filter(|shell| !shell.trim().is_empty())
                .cloned()
                .unwrap_or_else(|| DEFAULT_SHELL.to_string());
            Arc::new(resolve_login_env(Path::new(&shell), &process_env).await)
        })
        .await
        .clone()
}

#[cfg(test)]
static TEST_LOGIN_ENV: std::sync::Mutex<Option<Arc<LoginEnv>>> = std::sync::Mutex::new(None);

/// Test-only: replaces the login environment so integration tests can run
/// fake CLIs. `None` restores real capture.
#[cfg(test)]
pub(crate) fn set_login_env_for_tests(env: Option<LoginEnv>) {
    *TEST_LOGIN_ENV
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = env.map(Arc::new);
}

/// Captures `shell -l`'s environment and merges it over `process_env`. Falls
/// back to `process_env` plus the usual user bin directories when the shell
/// cannot be run or times out.
pub async fn resolve_login_env(shell: &Path, process_env: &BTreeMap<String, String>) -> LoginEnv {
    let captured = capture_login_env(shell, process_env).await;
    let mut vars = process_env.clone();
    if let Some(captured) = captured {
        vars.extend(captured);
    }
    append_fallback_dirs(&mut vars);
    LoginEnv { vars }
}

async fn capture_login_env(
    shell: &Path,
    process_env: &BTreeMap<String, String>,
) -> Option<BTreeMap<String, String>> {
    let cwd = process_env
        .get("HOME")
        .map(PathBuf::from)
        .filter(|home| home.is_dir())
        .unwrap_or_else(std::env::temp_dir);
    let script = format!("printf '%s' {ENV_START_MARKER}; env -0; printf '%s' {ENV_END_MARKER}");
    let output = process::run(ProcessSpec {
        program: shell.to_path_buf(),
        args: vec!["-l".to_string(), "-c".to_string(), script],
        env: process_env.clone(),
        cwd,
        stdin: None,
        timeout: CAPTURE_TIMEOUT,
    })
    .await
    .ok()?;
    if !output.success {
        tracing::warn!(code = ?output.code, "login shell environment capture failed");
        return None;
    }
    parse_env_block(&output.stdout)
}

/// Parses the NUL-separated `env -0` block between the markers. Profile
/// scripts may print anything before or after; only the marked block counts.
fn parse_env_block(stdout: &str) -> Option<BTreeMap<String, String>> {
    let start = stdout.find(ENV_START_MARKER)? + ENV_START_MARKER.len();
    let end = start + stdout[start..].find(ENV_END_MARKER)?;
    let vars: BTreeMap<String, String> = stdout[start..end]
        .split('\0')
        .filter_map(|entry| {
            let (key, value) = entry.split_once('=')?;
            (!key.is_empty()).then(|| (key.to_string(), value.to_string()))
        })
        .collect();
    (!vars.is_empty()).then_some(vars)
}

/// Appends common user bin directories that are missing from PATH, so a CLI
/// installed by a standard installer is still found when the profile does
/// not export its directory.
fn append_fallback_dirs(vars: &mut BTreeMap<String, String>) {
    let home = vars.get("HOME").cloned().unwrap_or_default();
    let mut dirs: Vec<PathBuf> = vars
        .get("PATH")
        .map(|path| std::env::split_paths(&OsString::from(path)).collect())
        .unwrap_or_default();
    let mut candidates = Vec::new();
    if !home.is_empty() {
        for relative in [".local/bin", ".npm-global/bin", ".bun/bin", ".volta/bin"] {
            candidates.push(Path::new(&home).join(relative));
        }
    }
    candidates.push(PathBuf::from("/opt/homebrew/bin"));
    candidates.push(PathBuf::from("/usr/local/bin"));
    for candidate in candidates {
        if !dirs.contains(&candidate) {
            dirs.push(candidate);
        }
    }
    if let Ok(path) = std::env::join_paths(dirs) {
        vars.insert("PATH".to_string(), path.to_string_lossy().into_owned());
    }
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .map(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn llm_env_block_ignores_profile_noise_and_keeps_multiline_values() {
        let stdout = format!(
            "Welcome!\n{ENV_START_MARKER}PATH=/a:/b\0MULTI=line1\nline2\0EMPTY=\0{ENV_END_MARKER}bye"
        );
        let vars = parse_env_block(&stdout).unwrap();
        assert_eq!(vars["PATH"], "/a:/b");
        assert_eq!(vars["MULTI"], "line1\nline2");
        assert_eq!(vars["EMPTY"], "");
    }

    #[test]
    fn llm_env_block_requires_markers() {
        assert!(parse_env_block("PATH=/a\0").is_none());
    }
}
