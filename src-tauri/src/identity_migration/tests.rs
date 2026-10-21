//! Behavior of the one-time identity migration: when it copies, that the copy
//! is complete or absent, and that the source never changes.

use super::*;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;

const RELEASE: &str = "dev.guilhermexp.clovy";

#[derive(Default)]
struct MemoryStore {
    items: Mutex<BTreeMap<(String, String), String>>,
    denied: Mutex<BTreeSet<(String, String)>>,
}

impl MemoryStore {
    fn with(items: &[(&str, &str, &str)]) -> Self {
        let store = Self::default();
        for (service, account, value) in items {
            store.put(service, account, value);
        }
        store
    }

    fn put(&self, service: &str, account: &str, value: &str) {
        self.items
            .lock()
            .unwrap()
            .insert((service.into(), account.into()), value.into());
    }

    fn deny(&self, service: &str, account: &str) {
        self.denied
            .lock()
            .unwrap()
            .insert((service.into(), account.into()));
    }

    fn value(&self, service: &str, account: &str) -> Option<String> {
        self.items
            .lock()
            .unwrap()
            .get(&(service.to_string(), account.to_string()))
            .cloned()
    }

    fn snapshot(&self) -> BTreeMap<(String, String), String> {
        self.items.lock().unwrap().clone()
    }
}

impl CredentialStore for MemoryStore {
    fn accounts(&self, service: &str) -> Result<Vec<String>, String> {
        Ok(self
            .items
            .lock()
            .unwrap()
            .keys()
            .filter(|(item_service, _)| item_service == service)
            .map(|(_, account)| account.clone())
            .collect())
    }

    fn get(&self, service: &str, account: &str) -> Result<Option<String>, String> {
        if self
            .denied
            .lock()
            .unwrap()
            .contains(&(service.to_string(), account.to_string()))
        {
            return Err("user denied access".into());
        }
        Ok(self.value(service, account))
    }

    fn set(&self, service: &str, account: &str, value: &str) -> Result<(), String> {
        self.put(service, account, value);
        Ok(())
    }

    fn delete(&self, service: &str, account: &str) -> Result<(), String> {
        self.items
            .lock()
            .unwrap()
            .remove(&(service.to_string(), account.to_string()));
        Ok(())
    }
}

/// Fails the copy of the `fail_at`-th file, after copying the earlier ones.
struct FailingCopier {
    copied: usize,
    fail_at: usize,
}

impl FileCopier for FailingCopier {
    fn copy_file(&mut self, from: &Path, to: &Path) -> std::io::Result<()> {
        self.copied += 1;
        if self.copied == self.fail_at {
            return Err(std::io::Error::other("injected copy failure"));
        }
        std::fs::copy(from, to).map(|_| ())
    }
}

/// Every path under `root` with the SHA-256 of each file's contents.
fn tree_hash(root: &Path) -> BTreeMap<PathBuf, String> {
    let mut out = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            let relative = path.strip_prefix(root).unwrap().to_path_buf();
            let meta = std::fs::symlink_metadata(&path).unwrap();
            if meta.is_dir() {
                out.insert(relative, "dir".into());
                stack.push(path);
            } else if meta.file_type().is_symlink() {
                out.insert(
                    relative,
                    format!("link:{}", std::fs::read_link(&path).unwrap().display()),
                );
            } else {
                let digest = Sha256::digest(std::fs::read(&path).unwrap());
                out.insert(relative, format!("{digest:x}"));
            }
        }
    }
    out
}

fn write(path: &Path, contents: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}

/// An old profile: notes, an activity database, a recording and settings.
fn old_profile(root: &Path) {
    write(&root.join("notes.sqlite3"), b"notes database bytes");
    write(&root.join("activity.sqlite3"), b"encrypted activity bytes");
    write(
        &root
            .join("recordings")
            .join("note-1")
            .join("s-1")
            .join("mic.wav"),
        &vec![7_u8; 64 * 1024],
    );
    write(&root.join("provider-settings.json"), br#"{"engine":"cli"}"#);
}

fn dir_plan(base: &Path) -> MigrationPlan {
    MigrationPlan {
        data: DirectoryCopy {
            source: base.join("old"),
            target: base.join("new"),
        },
        extra_directories: Vec::new(),
        credentials: Vec::new(),
    }
}

fn is_empty_or_absent(path: &Path) -> bool {
    !path.exists() || std::fs::read_dir(path).unwrap().next().is_none()
}

fn leftover_staging(base: &Path) -> Vec<String> {
    std::fs::read_dir(base)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name != "old" && name != "new")
        .collect()
}

#[test]
fn decides_to_migrate_only_into_an_absent_or_empty_target() {
    let base = tempfile::tempdir().unwrap();
    let source = base.path().join("old");
    let target = base.path().join("new");

    assert_eq!(decide(&source, &target), Decision::NoSource);

    old_profile(&source);
    assert_eq!(decide(&source, &target), Decision::Migrate);

    std::fs::create_dir_all(&target).unwrap();
    assert_eq!(decide(&source, &target), Decision::Migrate);

    write(&target.join("notes.sqlite3"), b"already in use");
    assert_eq!(decide(&source, &target), Decision::TargetInUse);
}

#[test]
fn copies_the_whole_profile_and_leaves_the_source_unchanged() {
    let base = tempfile::tempdir().unwrap();
    let plan = dir_plan(base.path());
    old_profile(&plan.data.source);
    #[cfg(unix)]
    std::os::unix::fs::symlink("notes.sqlite3", plan.data.source.join("notes-link")).unwrap();
    let before = tree_hash(&plan.data.source);

    let outcome = migrate(&plan, &MemoryStore::default(), &mut StdCopier).expect("migrate");

    assert!(
        matches!(outcome, Outcome::Migrated { files: 4, .. }),
        "{outcome:?}"
    );
    assert_eq!(tree_hash(&plan.data.target), before);
    assert_eq!(tree_hash(&plan.data.source), before);
    assert!(leftover_staging(base.path()).is_empty());
}

#[test]
fn a_target_in_use_is_left_alone() {
    let base = tempfile::tempdir().unwrap();
    let mut plan = dir_plan(base.path());
    old_profile(&plan.data.source);
    write(
        &plan.data.target.join("notes.sqlite3"),
        b"new identity notes",
    );
    plan.credentials.push(CredentialCopy {
        source_service: "old.activity-db".into(),
        legacy_service: None,
        target_service: "new.activity-db".into(),
    });
    let store = MemoryStore::with(&[("old.activity-db", "sqlcipher-key", "k")]);
    let target_before = tree_hash(&plan.data.target);

    let outcome = migrate(&plan, &store, &mut StdCopier).expect("migrate");

    assert_eq!(outcome, Outcome::Skipped(Decision::TargetInUse));
    assert_eq!(tree_hash(&plan.data.target), target_before);
    assert_eq!(store.value("new.activity-db", "sqlcipher-key"), None);
}

#[test]
fn a_copy_failure_leaves_the_target_empty_and_the_source_unchanged() {
    let base = tempfile::tempdir().unwrap();
    let mut plan = dir_plan(base.path());
    old_profile(&plan.data.source);
    plan.extra_directories.push(DirectoryCopy {
        source: base.path().join("old-webkit"),
        target: base.path().join("new-webkit"),
    });
    write(
        &base.path().join("old-webkit").join("LocalStorage"),
        b"ui state",
    );
    plan.credentials.push(CredentialCopy {
        source_service: "old.activity-db".into(),
        legacy_service: None,
        target_service: "new.activity-db".into(),
    });
    let store = MemoryStore::with(&[("old.activity-db", "sqlcipher-key", "k")]);
    let before = tree_hash(&plan.data.source);

    // The third file fails after two were already copied.
    let error = migrate(
        &plan,
        &store,
        &mut FailingCopier {
            copied: 0,
            fail_at: 3,
        },
    )
    .expect_err("injected failure");

    assert!(
        error.to_string().contains("injected copy failure"),
        "{error}"
    );
    assert!(is_empty_or_absent(&plan.data.target));
    assert!(is_empty_or_absent(&base.path().join("new-webkit")));
    assert_eq!(tree_hash(&plan.data.source), before);
    assert_eq!(store.value("new.activity-db", "sqlcipher-key"), None);
    let leftovers: Vec<_> = leftover_staging(base.path())
        .into_iter()
        .filter(|name| name != "old-webkit")
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

#[test]
fn copies_credentials_including_the_activity_key_without_removing_them() {
    let base = tempfile::tempdir().unwrap();
    let mut plan = dir_plan(base.path());
    old_profile(&plan.data.source);
    plan.credentials = vec![
        CredentialCopy {
            source_service: "old.activity-db".into(),
            legacy_service: None,
            target_service: "new.activity-db".into(),
        },
        CredentialCopy {
            source_service: "old.llm-providers".into(),
            legacy_service: None,
            target_service: "new.llm-providers".into(),
        },
    ];
    let store = MemoryStore::with(&[
        ("old.activity-db", "sqlcipher-key", "ab".repeat(32).as_str()),
        ("old.llm-providers", "endpoint:a", "sk-a"),
        ("old.llm-providers", "endpoint:b", "sk-b"),
        ("unrelated", "x", "y"),
    ]);
    let before = store.snapshot();

    let outcome = migrate(&plan, &store, &mut StdCopier).expect("migrate");

    assert!(
        matches!(outcome, Outcome::Migrated { credentials: 3, .. }),
        "{outcome:?}"
    );
    assert_eq!(
        store.value("new.activity-db", "sqlcipher-key"),
        Some("ab".repeat(32))
    );
    assert_eq!(
        store.value("new.llm-providers", "endpoint:a").as_deref(),
        Some("sk-a")
    );
    assert_eq!(
        store.value("new.llm-providers", "endpoint:b").as_deref(),
        Some("sk-b")
    );
    for (key, value) in before {
        assert_eq!(store.value(&key.0, &key.1), Some(value), "{key:?} changed");
    }
}

#[test]
fn a_denied_credential_fails_the_whole_migration() {
    let base = tempfile::tempdir().unwrap();
    let mut plan = dir_plan(base.path());
    old_profile(&plan.data.source);
    plan.credentials = vec![
        CredentialCopy {
            source_service: "old.llm-providers".into(),
            legacy_service: None,
            target_service: "new.llm-providers".into(),
        },
        CredentialCopy {
            source_service: "old.activity-db".into(),
            legacy_service: None,
            target_service: "new.activity-db".into(),
        },
    ];
    let store = MemoryStore::with(&[
        ("old.llm-providers", "endpoint:a", "sk-a"),
        ("old.activity-db", "sqlcipher-key", "k"),
    ]);
    store.deny("old.activity-db", "sqlcipher-key");
    let before = tree_hash(&plan.data.source);

    let error = migrate(&plan, &store, &mut StdCopier).expect_err("denied");

    assert!(error.to_string().contains("denied"), "{error}");
    assert!(is_empty_or_absent(&plan.data.target));
    assert_eq!(store.value("new.llm-providers", "endpoint:a"), None);
    assert_eq!(store.value("new.activity-db", "sqlcipher-key"), None);
    assert_eq!(tree_hash(&plan.data.source), before);
    assert!(leftover_staging(base.path()).is_empty());
}

#[test]
fn bridged_credentials_migrate_the_value_the_old_bridge_would_read() {
    let base = tempfile::tempdir().unwrap();
    let mut plan = dir_plan(base.path());
    old_profile(&plan.data.source);
    plan.credentials = vec![CredentialCopy {
        source_service: "clovy.google".into(),
        legacy_service: Some("june.google".into()),
        target_service: "new.google".into(),
    }];
    let store = MemoryStore::with(&[
        // Only the June-era service has it: still signed in.
        ("june.google", "legacy-only", "token-1"),
        // Both agree.
        ("clovy.google", "both", "token-2"),
        ("june.google", "both", "token-2"),
        // Only the Clovy service has it: the old bridge read it as signed out.
        ("clovy.google", "signed-out", "stale"),
    ]);

    migrate(&plan, &store, &mut StdCopier).expect("migrate");

    assert_eq!(
        store.value("new.google", "legacy-only").as_deref(),
        Some("token-1")
    );
    assert_eq!(
        store.value("new.google", "both").as_deref(),
        Some("token-2")
    );
    assert_eq!(store.value("new.google", "signed-out"), None);
}

#[tokio::test]
async fn the_migrated_activity_database_opens_with_the_copied_key() {
    use crate::activity::key::{ActivityKeyStore, MemoryKeyStore};
    use crate::activity::store::{ActivityStore, NewFrame, TextSource, ACTIVITY_DB_FILE};
    use chrono::{TimeZone, Utc};

    let base = tempfile::tempdir().unwrap();
    let mut plan = dir_plan(base.path());
    std::fs::create_dir_all(&plan.data.source).unwrap();
    let old_keys = MemoryKeyStore::default();
    let old_db = ActivityStore::open(&plan.data.source.join(ACTIVITY_DB_FILE), &old_keys)
        .await
        .expect("open old");
    old_db
        .insert_frame(&NewFrame {
            captured_at: Utc.with_ymd_and_hms(2026, 10, 1, 9, 0, 0).unwrap(),
            app_name: "Zed".into(),
            bundle_id: None,
            window_title: Some("notes.md".into()),
            browser_url: None,
            text_source: TextSource::Accessibility,
            text: Some("migrated activity".into()),
            input_idle_ms: None,
        })
        .await
        .expect("insert");
    old_db.close().await;
    let old_key = old_keys.current().expect("old key");
    plan.credentials.push(CredentialCopy {
        source_service: "old.activity-db".into(),
        legacy_service: None,
        target_service: "new.activity-db".into(),
    });
    let store = MemoryStore::with(&[("old.activity-db", "sqlcipher-key", &old_key)]);

    migrate(&plan, &store, &mut StdCopier).expect("migrate");

    let new_keys = MemoryKeyStore::default();
    new_keys
        .store(
            &store
                .value("new.activity-db", "sqlcipher-key")
                .expect("copied key"),
        )
        .unwrap();
    let new_db = ActivityStore::open(&plan.data.target.join(ACTIVITY_DB_FILE), &new_keys)
        .await
        .expect("open migrated");
    assert_eq!(new_db.frames_after(0, 10).await.unwrap().len(), 1);
}

#[test]
fn the_june_profile_migrates_only_into_the_release_identifier() {
    let source = default_source(RELEASE).expect("June source for the release identifier");
    assert_eq!(source.identifier, "co.opensoftware.june");
    assert!(default_source("dev.guilhermexp.clovy.proof").is_none());
}

#[test]
fn the_dev_plan_maps_every_june_name_to_the_new_dev_names() {
    let dirs = PlatformDirs {
        data: PathBuf::from("/home/Library/Application Support"),
        config: PathBuf::from("/home/Library/Application Support"),
        local_data: None,
        webkit: Some(PathBuf::from("/home/Library/WebKit")),
    };
    let source = default_source(RELEASE).unwrap();

    let plan = plan(&dirs, RELEASE, true, &source);

    assert_eq!(
        plan.data,
        DirectoryCopy {
            source: dirs.data.join("co.opensoftware.june-dev"),
            target: dirs.data.join("dev.guilhermexp.clovy-dev"),
        }
    );
    assert_eq!(
        plan.extra_directories,
        vec![DirectoryCopy {
            source: PathBuf::from("/home/Library/WebKit/co.opensoftware.june.dev"),
            target: PathBuf::from("/home/Library/WebKit/dev.guilhermexp.clovy.dev"),
        }]
    );
    let activity = plan
        .credentials
        .iter()
        .find(|copy| copy.target_service == "dev.guilhermexp.clovy-dev.activity-db")
        .expect("activity key");
    assert_eq!(
        activity.source_service,
        "co.opensoftware.clovy-dev.activity-db"
    );
    assert_eq!(activity.legacy_service, None);
    let secrets = plan
        .credentials
        .iter()
        .find(|copy| copy.target_service == "dev.guilhermexp.clovy-dev.agent-secrets")
        .expect("agent secrets");
    assert_eq!(
        secrets.source_service,
        "co.opensoftware.clovy.dev.agent-secrets"
    );
    assert_eq!(
        secrets.legacy_service.as_deref(),
        Some("co.opensoftware.june.dev.agent-secrets")
    );
    let google = plan
        .credentials
        .iter()
        .find(|copy| copy.target_service == "dev.guilhermexp.clovy-dev.google")
        .expect("google connector");
    assert_eq!(google.source_service, "co.opensoftware.clovy-dev.google");
    assert_eq!(
        google.legacy_service.as_deref(),
        Some("co.opensoftware.june-dev.google")
    );
}

#[test]
fn the_release_plan_on_windows_copies_the_webview_store_folder() {
    let dirs = PlatformDirs {
        data: PathBuf::from(r"C:\Users\u\AppData\Roaming"),
        config: PathBuf::from(r"C:\Users\u\AppData\Roaming"),
        local_data: Some(PathBuf::from(r"C:\Users\u\AppData\Local")),
        webkit: None,
    };
    let source = default_source(RELEASE).unwrap();

    let plan = plan(&dirs, RELEASE, false, &source);

    assert_eq!(plan.data.source, dirs.data.join("co.opensoftware.june"));
    assert_eq!(plan.data.target, dirs.data.join(RELEASE));
    assert_eq!(
        plan.extra_directories,
        vec![DirectoryCopy {
            source: PathBuf::from(r"C:\Users\u\AppData\Local").join("co.opensoftware.june"),
            target: PathBuf::from(r"C:\Users\u\AppData\Local").join(RELEASE),
        }]
    );
    assert!(plan.credentials.iter().any(|copy| copy.source_service
        == "co.opensoftware.clovy.activity-db"
        && copy.target_service == "dev.guilhermexp.clovy.activity-db"));
}

#[test]
fn a_proof_source_reads_only_its_own_names() {
    let source = source_override("dev.guilhermexp.clovy.proofold").expect("override");
    let dirs = PlatformDirs {
        data: PathBuf::from("/s"),
        config: PathBuf::from("/s"),
        local_data: None,
        webkit: None,
    };

    let plan = plan(&dirs, "dev.guilhermexp.clovy.proofnew", true, &source);

    assert_eq!(
        plan.data.source,
        PathBuf::from("/s/dev.guilhermexp.clovy.proofold-dev")
    );
    assert_eq!(
        plan.data.target,
        PathBuf::from("/s/dev.guilhermexp.clovy.proofnew-dev")
    );
    for copy in &plan.credentials {
        assert!(
            copy.source_service
                .starts_with("dev.guilhermexp.clovy.proofold"),
            "{copy:?}"
        );
        assert!(copy.legacy_service.is_none(), "{copy:?}");
        assert!(
            copy.target_service
                .starts_with("dev.guilhermexp.clovy.proofnew-dev."),
            "{copy:?}"
        );
    }
    assert!(source_override("not an identifier").is_none());
}
