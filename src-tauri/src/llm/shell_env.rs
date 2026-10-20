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

/// Variables Clovy itself reads (its own configuration and credentials, such
/// as the local development bearer token loaded from `.env`). The login
/// environment is merged over Clovy's process environment, so without this
/// filter they would reach every CLI, its tools, and its MCP servers.
const CLOVY_OWNED_PREFIXES: [&str; 5] =
    ["OS_CLOVY_", "OS_JUNE_", "OS_ACCOUNTS_", "CLOVY_", "JUNE_"];
const CLOVY_OWNED_NAMES: [&str; 2] = ["GOOGLE_OAUTH_CLIENT_ID", "GOOGLE_OAUTH_CLIENT_SECRET"];

/// The environment of a process Clovy starts for the user (a CLI, its
/// `--version`, the login shell): every variable unchanged (HOME, PATH,
/// `PI_CODING_AGENT_DIR`, the user's own keys) except Clovy's own. The one
/// filter for `llm` one-shot calls, CLI detection, the login-shell capture,
/// and the CLI chat engine.
pub fn cli_environment(vars: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    vars.iter()
        .filter(|(name, _)| {
            !CLOVY_OWNED_NAMES.contains(&name.as_str())
                && !CLOVY_OWNED_PREFIXES
                    .iter()
                    .any(|prefix| name.starts_with(prefix))
        })
        .map(|(name, value)| (name.clone(), value.clone()))
        .collect()
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
        // The user's profile scripts run here too: keep Clovy's own
        // variables out of them.
        env: cli_environment(process_env),
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

    #[cfg(windows)]
    fn touch(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "").unwrap();
    }

    #[cfg(windows)]
    fn windows_vars(entries: &[(&str, &Path)]) -> BTreeMap<String, String> {
        entries
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string_lossy().into_owned()))
            .collect()
    }

    /// npm writes `codex.cmd` next to a POSIX `codex` script with no
    /// extension; only the `.cmd` can run on Windows.
    #[test]
    #[cfg(windows)]
    fn llm_windows_path_key_and_pathext_pick_the_npm_cmd_shim() {
        let npm = tempfile::tempdir().unwrap();
        touch(&npm.path().join("codex"));
        touch(&npm.path().join("codex.cmd"));
        let mut vars = windows_vars(&[("Path", npm.path())]);
        vars.insert("PATHEXT".into(), ".COM;.EXE;.BAT;.CMD".into());
        let env = LoginEnv::from_vars(vars);
        assert_eq!(env.which("codex"), Some(npm.path().join("codex.cmd")));
    }

    #[test]
    #[cfg(windows)]
    fn llm_windows_without_pathext_uses_the_default_extensions() {
        let bin = tempfile::tempdir().unwrap();
        touch(&bin.path().join("claude.exe"));
        touch(&bin.path().join("pi"));
        let env = LoginEnv::from_vars(windows_vars(&[("Path", bin.path())]));
        assert_eq!(env.which("claude"), Some(bin.path().join("claude.exe")));
        assert_eq!(env.which("pi"), None);
    }

    /// The installers' directories are found even when the app's `Path`
    /// lacks them, and the result stays in the app's own `Path` key so the
    /// child never gets two PATH variables.
    #[tokio::test]
    #[cfg(windows)]
    async fn llm_windows_fallback_dirs_come_from_the_user_profile() {
        let profile = tempfile::tempdir().unwrap();
        let roaming = profile.path().join("AppData").join("Roaming");
        let local = profile.path().join("AppData").join("Local");
        let system = profile.path().join("system32");
        std::fs::create_dir_all(&system).unwrap();
        touch(&profile.path().join(".local").join("bin").join("claude.exe"));
        touch(&roaming.join("npm").join("codex.cmd"));
        touch(&local.join("cursor-agent").join("cursor-agent.cmd"));
        let process_env = windows_vars(&[
            ("Path", &system),
            ("USERPROFILE", profile.path()),
            ("APPDATA", &roaming),
            ("LOCALAPPDATA", &local),
        ]);

        let env = resolve_login_env(Path::new("unused-on-windows"), &process_env).await;

        assert_eq!(
            env.which("claude"),
            Some(profile.path().join(".local").join("bin").join("claude.exe"))
        );
        assert_eq!(
            env.which("codex"),
            Some(roaming.join("npm").join("codex.cmd"))
        );
        assert_eq!(
            env.which("cursor-agent"),
            Some(local.join("cursor-agent").join("cursor-agent.cmd"))
        );
        assert!(!env.vars().contains_key("PATH"), "{:?}", env.vars().keys());
        assert_eq!(env.path_dirs().first(), Some(&system));
    }
}
