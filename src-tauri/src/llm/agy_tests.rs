use super::*;

fn private_root() -> tempfile::TempDir {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tempfile::Builder::new()
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap()
    }
    #[cfg(windows)]
    {
        tempfile::tempdir().unwrap()
    }
}

fn host(root: &Path) -> HostContext {
    HostContext::new(
        root.canonicalize().unwrap(),
        &BTreeMap::from([
            (
                "HOME".into(),
                root.join("synthetic-host-home").display().to_string(),
            ),
            (
                "APPDATA".into(),
                root.join("synthetic-host-appdata").display().to_string(),
            ),
        ]),
    )
    .unwrap()
}
fn launch(root: &Path, token: &str) -> McpLaunch {
    McpLaunch {
        command: std::env::current_exe().unwrap(),
        args: vec![
            "--dir".into(),
            root.join("mcp").display().to_string(),
            "--turn".into(),
            token.into(),
        ],
    }
}

#[test]
fn managed_profile_transport_cleanup_preserves_native_state_and_session_identity() {
    let root = private_root();
    let host = host(root.path());
    let mut execution = host.chat("session-a").unwrap();
    let auth = execution.home().join("synthetic-native-state");
    std::fs::write(&auth, b"native SDK state must survive").unwrap();
    let cwd = execution.cwd().to_path_buf();
    let origin = execution.profile_id().to_string();
    execution
        .install_mcp(&launch(root.path(), "synthetic-turn-a"))
        .unwrap();
    assert!(cwd.join(".agents/mcp_config.json").exists());
    execution.finish().unwrap();
    drop(execution);
    assert!(!cwd.join(".agents/mcp_config.json").exists());
    assert!(!cwd.join(".agents").join(RECEIPT).exists());
    assert_eq!(
        std::fs::read(&auth).unwrap(),
        b"native SDK state must survive"
    );
    let reopened = host.chat("session-a").unwrap();
    assert_eq!(reopened.cwd(), cwd);
    assert_eq!(reopened.profile_id(), origin);
    let other = host.chat("session-b").unwrap();
    assert_ne!(other.cwd(), reopened.cwd());
    assert_eq!(other.home(), reopened.home());
    assert!(
        host.chat("session-a").is_err(),
        "one session cannot have two transport owners"
    );
}

#[test]
fn managed_profile_concurrent_sessions_have_separate_authority_and_leave_other_files() {
    let root = private_root();
    let host = host(root.path());
    let mut a = host.chat("a").unwrap();
    let mut b = host.chat("b").unwrap();
    a.install_mcp(&launch(root.path(), "synthetic-a")).unwrap();
    b.install_mcp(&launch(root.path(), "synthetic-b")).unwrap();
    let bconfig = b.cwd().join(".agents/mcp_config.json");
    let before = std::fs::read(&bconfig).unwrap();
    assert_ne!(a.server(), b.server());
    std::fs::write(a.cwd().join(".agents/keep.txt"), b"unrelated state").unwrap();
    let kept = a.cwd().join(".agents/keep.txt");
    a.finish().unwrap();
    assert_eq!(std::fs::read(&bconfig).unwrap(), before);
    assert_eq!(std::fs::read(kept).unwrap(), b"unrelated state");
    b.finish().unwrap();
}

#[cfg(unix)]
#[test]
fn managed_profile_rejects_symlinks_and_insecure_permissions_without_repair() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    for symlinked in [true, false] {
        let root = private_root();
        let foreign = tempfile::tempdir().unwrap();
        let target = root.path().join("runtime-profiles");
        if symlinked {
            symlink(foreign.path(), &target).unwrap();
        } else {
            std::fs::create_dir(&target).unwrap();
            std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let marker = if symlinked {
            foreign.path().join("keep")
        } else {
            target.join("keep")
        };
        std::fs::write(&marker, b"foreign state").unwrap();
        let metadata = std::fs::metadata(marker.parent().unwrap()).unwrap();
        assert!(host(root.path()).chat("session").is_err());
        assert_eq!(std::fs::read(&marker).unwrap(), b"foreign state");
        assert_eq!(
            std::fs::metadata(marker.parent().unwrap())
                .unwrap()
                .permissions()
                .mode(),
            metadata.permissions().mode()
        );
        assert!(!marker.parent().unwrap().join("agy").exists());
    }
}

#[test]
fn managed_profile_modified_transport_is_retained_without_touching_authentication() {
    let root = private_root();
    let host = host(root.path());
    let mut execution = host.chat("a").unwrap();
    let auth = execution.home().join("synthetic-auth");
    std::fs::write(&auth, b"keep authentication").unwrap();
    execution
        .install_mcp(&launch(root.path(), "synthetic-turn"))
        .unwrap();
    let config = execution.cwd().join(".agents/mcp_config.json");
    std::fs::write(&config, b"foreign replacement").unwrap();
    assert!(execution.finish().is_err());
    drop(execution);
    assert_eq!(std::fs::read(config).unwrap(), b"foreign replacement");
    assert_eq!(std::fs::read(auth).unwrap(), b"keep authentication");
    assert!(
        host.chat("a").is_err(),
        "unknown data must not be replaced by a new grant"
    );
}

#[test]
fn managed_profile_crash_child() {
    let Some(root) = std::env::var_os("CLOVY_AGY_TEST_CRASH_ROOT") else {
        return;
    };
    let root = PathBuf::from(root);
    let mut execution = host(&root).chat("crash").unwrap();
    execution
        .install_mcp(&launch(&root, "dead-synthetic-turn"))
        .unwrap();
    std::process::exit(0);
}

#[test]
fn managed_profile_recovers_owned_transport_after_crash_and_keeps_native_state() {
    let root = private_root();
    let host = host(root.path());
    let execution = host.chat("crash").unwrap();
    let auth = execution.home().join("synthetic-auth");
    std::fs::write(&auth, b"durable authentication").unwrap();
    let cwd = execution.cwd().to_path_buf();
    drop(execution);
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "llm::agy::tests::managed_profile_crash_child",
            "--nocapture",
        ])
        .env("CLOVY_AGY_TEST_CRASH_ROOT", root.path())
        .output()
        .unwrap();
    assert!(
        child.status.success(),
        "{}",
        String::from_utf8_lossy(&child.stderr)
    );
    assert!(cwd.join(".agents/mcp_config.json").exists());
    let recovered = host.chat("crash").unwrap();
    assert!(!cwd.join(".agents/mcp_config.json").exists());
    assert_eq!(std::fs::read(auth).unwrap(), b"durable authentication");
    drop(recovered);
}

#[cfg(target_os = "macos")]
#[test]
fn managed_profile_darwin_extended_acl_is_refused_before_authority_and_preserved() {
    let root = private_root();
    let host = host(root.path());
    let execution = host.chat("a").unwrap();
    let path = execution.cwd().to_path_buf();
    drop(execution);
    assert!(std::process::Command::new("/bin/chmod")
        .args(["+a", "everyone allow read,readattr,readsecurity"])
        .arg(&path)
        .status()
        .unwrap()
        .success());
    let listing = || {
        std::process::Command::new("/bin/ls")
            .arg("-lde")
            .arg(&path)
            .output()
            .unwrap()
            .stdout
    };
    let before = listing();
    assert!(host.chat("a").is_err());
    assert_eq!(listing(), before);
    assert!(!path.join(".agents/mcp_config.json").exists());
}

#[cfg(unix)]
#[test]
fn managed_profile_relay_guard_child() {
    let Some(expected) = std::env::var_os("CLOVY_SYNTHETIC_RELAY_ROOT") else {
        return;
    };
    let production = PathBuf::from(expected);
    assert_eq!(
        crate::mcp_server::stdio::production_data_refusal(&production.join("mcp")),
        Some(crate::mcp_server::stdio::PRODUCTION_DATA_MESSAGE)
    );
    assert_eq!(
        crate::mcp_server::stdio::production_data_refusal(
            &production.with_extension("dev").join("mcp")
        ),
        None
    );
}

#[cfg(unix)]
#[test]
fn managed_profile_relay_retains_host_production_refusal_while_sdk_home_is_private() {
    let root = private_root();
    let home = root.path().join("synthetic-host-home");
    std::fs::create_dir(&home).unwrap();
    #[cfg(windows)]
    let production = root
        .path()
        .join("synthetic-host-appdata")
        .join(crate::app_identity::RELEASE_APP_IDENTIFIER);
    #[cfg(not(windows))]
    let production = home
        .join("Library/Application Support")
        .join(crate::app_identity::RELEASE_APP_IDENTIFIER);
    std::fs::create_dir_all(production.join("mcp")).unwrap();
    let host = host(root.path());
    let mut execution = host.chat("relay-guard").unwrap();
    execution
        .install_mcp(&launch(root.path(), "synthetic-relay-turn"))
        .unwrap();
    let bytes = std::fs::read(execution.cwd().join(".agents/mcp_config.json")).unwrap();
    let config: Value = serde_json::from_slice(&bytes).unwrap();
    let env = config["mcpServers"]
        .as_object()
        .unwrap()
        .values()
        .next()
        .unwrap()["env"]
        .as_object()
        .unwrap();
    assert_ne!(execution.home(), home);
    let mut child = std::process::Command::new(std::env::current_exe().unwrap());
    child
        .args([
            "--exact",
            "llm::agy::tests::managed_profile_relay_guard_child",
            "--nocapture",
        ])
        .env_clear();
    for (key, value) in env {
        child.env(key, value.as_str().unwrap());
    }
    if cfg!(windows) {
        if let Some(value) = std::env::var_os("SystemRoot") {
            child.env("SystemRoot", value);
        }
    }
    let output = child
        .env("CLOVY_SYNTHETIC_RELAY_ROOT", &production)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(windows)]
#[test]
fn managed_profile_windows_aliases_are_private_and_relay_keeps_host_locations() {
    let root = private_root();
    let host_values = BTreeMap::from([
        ("Home".into(), "C:\\personal".into()),
        ("userprofile".into(), "C:\\personal".into()),
        ("AppData".into(), "C:\\personal\\roaming".into()),
        ("LocalAppData".into(), "C:\\personal\\local".into()),
        ("homedrive".into(), "C:".into()),
        ("homepath".into(), "\\personal".into()),
        ("GEMINI_API_KEY".into(), "synthetic-forbidden-key".into()),
    ]);
    let host = HostContext::new(root.path().canonicalize().unwrap(), &host_values).unwrap();
    let mut execution = host.chat("windows-aliases").unwrap();
    let env = execution.environment(&host_values);
    for key in [
        "HOME",
        "USERPROFILE",
        "APPDATA",
        "LOCALAPPDATA",
        "HOMEDRIVE",
        "HOMEPATH",
    ] {
        assert_eq!(
            env.keys()
                .filter(|name| name.eq_ignore_ascii_case(key))
                .count(),
            1
        );
        assert!(!env[key].contains("personal"));
    }
    assert_eq!(env["HOME"], env["USERPROFILE"]);
    let combined = PathBuf::from(format!("{}{}", env["HOMEDRIVE"], env["HOMEPATH"]))
        .canonicalize()
        .unwrap();
    assert_eq!(combined, execution.home());
    assert!(!env.contains_key("GEMINI_API_KEY"));
    execution
        .install_mcp(&launch(root.path(), "synthetic-windows-turn"))
        .unwrap();
    let config: Value = serde_json::from_slice(
        &std::fs::read(execution.cwd().join(".agents/mcp_config.json")).unwrap(),
    )
    .unwrap();
    let relay = &config["mcpServers"][execution.server().unwrap()]["env"];
    assert_eq!(relay["HOME"], "C:\\personal");
    assert_eq!(relay["APPDATA"], "C:\\personal\\roaming");
    assert!(relay.get("GEMINI_API_KEY").is_none());
}
