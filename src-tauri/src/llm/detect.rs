//! Detection of the six agent CLIs on the user's login-shell PATH, with the
//! resolved path and the version each CLI reports.

use super::{
    cli::CliKind,
    process::{self, ProcessSpec},
    shell_env::LoginEnv,
    StructuredOutputLevel,
};
use serde::Serialize;
use std::time::Duration;

const VERSION_TIMEOUT: Duration = Duration::from_secs(8);
const MAX_VERSION_CHARS: usize = 80;

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CliStatus {
    pub id: CliKind,
    pub name: String,
    pub installed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub structured_output: StructuredOutputLevel,
}

/// Detects every CLI concurrently, in catalog order.
pub async fn detect_all(env: &LoginEnv) -> Vec<CliStatus> {
    futures_util::future::join_all(CliKind::ALL.into_iter().map(|kind| detect(kind, env))).await
}

pub async fn detect(kind: CliKind, env: &LoginEnv) -> CliStatus {
    let Some(path) = env.which(kind.id()) else {
        return CliStatus {
            id: kind,
            name: kind.display_name().to_string(),
            installed: false,
            path: None,
            version: None,
            reason: Some(format!(
                "`{}` was not found in your login shell PATH.",
                kind.id()
            )),
            structured_output: kind.structured_output(),
        };
    };
    let version = read_version(&path, env).await;
    CliStatus {
        id: kind,
        name: kind.display_name().to_string(),
        installed: true,
        path: Some(path.to_string_lossy().into_owned()),
        version,
        reason: None,
        structured_output: kind.structured_output(),
    }
}

async fn read_version(path: &std::path::Path, env: &LoginEnv) -> Option<String> {
    let scratch = tempfile::tempdir().ok()?;
    let output = process::run(ProcessSpec {
        program: path.to_path_buf(),
        args: vec!["--version".to_string()],
        env: env.vars().clone(),
        cwd: scratch.path().to_path_buf(),
        stdin: None,
        timeout: VERSION_TIMEOUT,
    })
    .await
    .ok()?;
    output
        .stdout
        .lines()
        .chain(output.stderr.lines())
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(|line| line.chars().take(MAX_VERSION_CHARS).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::shell_env::resolve_login_env;
    use std::{collections::BTreeMap, os::unix::fs::PermissionsExt, path::Path};

    fn write_script(path: &Path, body: &str) {
        std::fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// A fake login shell: like a profile, it prepends `extra_dir` to PATH
    /// and exports a profile variable, then runs the `-c` script.
    fn fake_login_shell(dir: &Path, extra_dir: &Path) -> std::path::PathBuf {
        let shell = dir.join("fake-shell");
        write_script(
            &shell,
            &format!(
                "export PATH=\"{}:$PATH\"\nexport PI_CODING_AGENT_DIR=\"$HOME/.pi/agent\"\nshift\nexec /bin/sh -c \"$2\"",
                extra_dir.display()
            ),
        );
        shell
    }

    fn gui_app_env(home: &Path) -> BTreeMap<String, String> {
        BTreeMap::from([
            ("PATH".to_string(), "/usr/bin:/bin".to_string()),
            ("HOME".to_string(), home.to_string_lossy().into_owned()),
        ])
    }

    #[tokio::test]
    async fn llm_cli_found_only_through_the_login_shell_path() {
        let root = tempfile::tempdir().unwrap();
        let bin = root.path().join("local-bin");
        std::fs::create_dir(&bin).unwrap();
        write_script(&bin.join("claude"), "echo '2.1.0 (Claude Code)'");
        let shell = fake_login_shell(root.path(), &bin);
        let process_env = gui_app_env(root.path());

        // The GUI app's own PATH does not see the CLI.
        assert!(LoginEnv::from_vars(process_env.clone())
            .which("claude")
            .is_none());

        let env = resolve_login_env(&shell, &process_env).await;
        let status = detect(CliKind::Claude, &env).await;
        assert!(status.installed);
        assert_eq!(
            status.path.as_deref(),
            Some(bin.join("claude").to_string_lossy().as_ref())
        );
        assert_eq!(status.version.as_deref(), Some("2.1.0 (Claude Code)"));
        // The profile's environment is preserved for the CLIs Clovy runs.
        assert_eq!(
            env.vars().get("PI_CODING_AGENT_DIR").map(String::as_str),
            Some(format!("{}/.pi/agent", root.path().display()).as_str())
        );
        assert_eq!(
            env.vars().get("HOME").map(String::as_str),
            Some(root.path().to_string_lossy().as_ref())
        );
    }

    #[tokio::test]
    async fn llm_missing_cli_is_reported_with_a_reason() {
        let root = tempfile::tempdir().unwrap();
        let empty = root.path().join("empty-bin");
        std::fs::create_dir(&empty).unwrap();
        let env = LoginEnv::from_vars(BTreeMap::from([(
            "PATH".to_string(),
            empty.to_string_lossy().into_owned(),
        )]));
        let status = detect(CliKind::CursorAgent, &env).await;
        assert!(!status.installed);
        assert!(status.path.is_none());
        assert!(status.reason.unwrap().contains("cursor-agent"));
    }

    #[tokio::test]
    async fn llm_non_executable_file_is_not_a_cli() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("codex"), "not a program").unwrap();
        let env = LoginEnv::from_vars(BTreeMap::from([(
            "PATH".to_string(),
            root.path().to_string_lossy().into_owned(),
        )]));
        assert!(!detect(CliKind::Codex, &env).await.installed);
    }

    #[tokio::test]
    async fn llm_broken_login_shell_falls_back_to_the_app_environment() {
        let root = tempfile::tempdir().unwrap();
        let shell = root.path().join("broken-shell");
        write_script(&shell, "exit 3");
        let env = resolve_login_env(&shell, &gui_app_env(root.path())).await;
        let path = env.vars().get("PATH").unwrap();
        assert!(path.starts_with("/usr/bin:/bin"));
        assert!(path.contains(".local/bin"), "{path}");
    }
}
