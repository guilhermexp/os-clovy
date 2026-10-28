//! Managed bundled skills and the local MCP subprocess sandbox.
use super::skill_identity::reconcile_managed_obsidian_skill;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};
pub fn seed_bundled_skills(app: &AppHandle) {
    let Some(managed_root) = crate::app_paths::app_data_dir(app)
        .ok()
        .map(|path| path.join("agents").join("skills"))
    else {
        return;
    };
    let resource_root = app
        .path()
        .resource_dir()
        .ok()
        .map(|dir| dir.join("native").join("agent-skills"));
    let debug_root = std::env::var("CARGO_MANIFEST_DIR").ok().map(|manifest| {
        PathBuf::from(manifest)
            .join("resources")
            .join("agent-skills")
    });
    let source_root = resource_root.filter(|path| path.is_dir()).or(debug_root);
    let Some(source_root) = source_root else {
        tracing::warn!("could not resolve bundled agent-skills resource directory");
        return;
    };
    if let Err(error) = reconcile_managed_obsidian_skill(&managed_root) {
        tracing::warn!(?error, "could not promote the legacy Obsidian skill");
    }
    let Ok(entries) = std::fs::read_dir(&source_root) else {
        return;
    };
    for entry in entries.flatten() {
        let skill_file = entry.path().join("SKILL.md");
        if !skill_file.is_file() {
            continue;
        }
        let skill_id = entry.file_name().to_string_lossy().into_owned();
        let dest_dir = managed_root.join(&skill_id);
        let dest_file = dest_dir.join("SKILL.md");
        if dest_file.is_file() {
            continue;
        }
        if let Err(error) = std::fs::create_dir_all(&dest_dir) {
            tracing::warn!(?error, skill = %skill_id, "could not create managed skill directory");
            continue;
        }
        let temp = dest_dir.join(".SKILL.md.tmp");
        if let Err(error) = std::fs::copy(&skill_file, &temp) {
            tracing::warn!(?error, skill = %skill_id, "could not copy bundled skill");
            continue;
        }
        if let Err(error) = std::fs::rename(&temp, &dest_file) {
            tracing::warn!(?error, skill = %skill_id, "could not finalize bundled skill");
            let _ = std::fs::remove_file(&temp);
        }
    }
    if let Err(error) = reconcile_managed_obsidian_skill(&managed_root) {
        tracing::warn!(?error, "could not publish the legacy Obsidian skill alias");
    }
}

#[cfg(any(target_os = "macos", test))]
pub(crate) fn sandbox_profile(workspace: &std::path::Path) -> String {
    let escaped = workspace
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    format!("(version 1) (allow default) (deny file-write*) (allow file-write* (subpath \"{escaped}\")) (allow file-write* (subpath \"/private/tmp\"))")
}

#[cfg(test)]
mod tests {
    use super::sandbox_profile;
    #[test]
    fn escape_regression_sandbox_profile_preserves_workspace_paths() {
        for path in [
            "/tmp/plain",
            "/tmp/quoted\"name",
            "/tmp/slash\\\"quote",
            "/tmp/trailing\\",
        ] {
            let profile = sandbox_profile(std::path::Path::new(path));
            let quoted = serde_json::to_string(path).unwrap();
            assert!(
                profile.contains(&format!("(subpath {quoted})")),
                "{profile}"
            );
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn escape_regression_sandbox_exec_accepts_quoted_and_trailing_backslash_paths() {
        let directory = tempfile::tempdir().unwrap();
        for name in ["plain", "quoted\"name", "slash\\\"quote", "trailing\\"] {
            let workspace = directory.path().join(name);
            std::fs::create_dir(&workspace).unwrap();
            let output = std::process::Command::new("/usr/bin/sandbox-exec")
                .args(["-p", &sandbox_profile(&workspace), "/usr/bin/true"])
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }

    #[test]
    fn sandbox_profile_only_grants_workspace_and_tmp_writes() {
        let profile = sandbox_profile(std::path::Path::new("/Users/example/Clovy Workspace"));
        assert!(profile.contains("(deny file-write*)"));
        assert!(profile.contains("/Users/example/Clovy Workspace"));
        assert!(!profile.contains("(allow file-write*)"));
    }
}
