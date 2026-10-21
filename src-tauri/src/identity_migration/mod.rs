//! One-time copy of the June-era identity into Clovy's own (ADR-0068).
//!
//! The first launch under the new identifier copies the old data and config
//! directories, the WebView store and the credential items into the new
//! names, before anything opens a database. The copy is all or nothing and
//! never touches the source.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};

#[cfg(test)]
mod tests;

/// Whether the first launch should copy a source directory into a target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Migrate,
    TargetInUse,
    NoSource,
}

pub fn decide(source: &Path, target: &Path) -> Decision {
    if !source.is_dir() {
        return Decision::NoSource;
    }
    match std::fs::read_dir(target) {
        Ok(mut entries) => {
            if entries.next().is_some() {
                Decision::TargetInUse
            } else {
                Decision::Migrate
            }
        }
        Err(error) if error.kind() == ErrorKind::NotFound => Decision::Migrate,
        // Anything at the target we cannot list is someone's data.
        Err(_) => Decision::TargetInUse,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryCopy {
    pub source: PathBuf,
    pub target: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CredentialCopy {
    pub source_service: String,
    /// The June-era twin that the old credential bridge kept in sync with
    /// `source_service`, resolved the way that bridge read it.
    pub legacy_service: Option<String>,
    pub target_service: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationPlan {
    /// The data directory: its decision gates the whole migration.
    pub data: DirectoryCopy,
    /// The other directories (config when it differs, WebView store).
    pub extra_directories: Vec<DirectoryCopy>,
    pub credentials: Vec<CredentialCopy>,
    /// Executable names of the source identity's app: nothing is copied while
    /// one of them runs.
    pub source_executables: Vec<String>,
    /// How long to wait for another launch that is migrating the same target.
    pub lock_wait: std::time::Duration,
}

/// The platform base directories the app identifier is appended to.
#[derive(Debug, Clone)]
pub struct PlatformDirs {
    pub data: PathBuf,
    pub config: PathBuf,
    /// Windows: `%LOCALAPPDATA%`, where WebView2 keeps `EBWebView`.
    pub local_data: Option<PathBuf>,
    /// macOS: `~/Library/WebKit`, keyed by the bundle identifier.
    pub webkit: Option<PathBuf>,
}

/// The identity a migration reads from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceIdentity {
    pub identifier: String,
    pub credential_prefix: String,
    pub legacy_credential_prefix: Option<String>,
    /// The source app's executable names (`os-june`, `os-june.exe`).
    pub executables: Vec<String>,
}

/// The June-era names: the only place the app still spells them.
const JUNE_IDENTIFIER: &str = "co.opensoftware.june";
const JUNE_CLOVY_CREDENTIAL_PREFIX: &str = "co.opensoftware.clovy";
/// Every build before the rename ran as `os-june`, June and Clovy alike.
const JUNE_EXECUTABLES: &[&str] = &["os-june", "os-june.exe"];

/// Each credential service, and whether the old bridge kept a June-era twin.
const CREDENTIAL_SERVICES: &[(&str, bool)] = &[
    ("activity-db", false),
    ("llm-providers", false),
    ("agent-mcp", true),
    ("agent-secrets", true),
    ("notion-hosted-mcp", true),
    ("google", true),
    ("linear", true),
    ("notion", true),
    ("github", true),
];

/// The June-era identity, migrated only into the release identifier (and its
/// `-dev` profile). A development identity of its own never reads it.
pub fn default_source(target_identifier: &str) -> Option<SourceIdentity> {
    (target_identifier == crate::app_identity::RELEASE_APP_IDENTIFIER).then(|| SourceIdentity {
        identifier: JUNE_IDENTIFIER.into(),
        credential_prefix: JUNE_CLOVY_CREDENTIAL_PREFIX.into(),
        legacy_credential_prefix: Some(JUNE_IDENTIFIER.into()),
        executables: JUNE_EXECUTABLES
            .iter()
            .map(|name| name.to_string())
            .collect(),
    })
}

/// A source named by `CLOVY_DEV_IDENTITY_MIGRATION_SOURCE` in a development
/// build, so a native proof migrates a synthetic profile instead.
pub fn source_override(identifier: &str) -> Option<SourceIdentity> {
    let valid = identifier.contains('.')
        && identifier.split('.').all(|part| {
            !part.is_empty()
                && part
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
        });
    valid.then(|| SourceIdentity {
        identifier: identifier.into(),
        credential_prefix: identifier.into(),
        legacy_credential_prefix: None,
        executables: Vec::new(),
    })
}

fn profile_dir_name(identifier: &str, debug: bool) -> String {
    if debug {
        format!("{identifier}-dev")
    } else {
        identifier.to_string()
    }
}

/// The development app bundle is `<identifier>.dev` (ADR-0060).
fn webkit_dir_name(identifier: &str, debug: bool) -> String {
    if debug {
        format!("{identifier}.dev")
    } else {
        identifier.to_string()
    }
}

/// The June-era service names: `-dev` suffix, except agent secrets (`.dev`).
fn source_service(prefix: &str, debug: bool, name: &str) -> String {
    match (debug, name) {
        (false, _) => format!("{prefix}.{name}"),
        (true, "agent-secrets") => format!("{prefix}.dev.{name}"),
        (true, _) => format!("{prefix}-dev.{name}"),
    }
}

pub fn plan(
    dirs: &PlatformDirs,
    target_identifier: &str,
    debug: bool,
    source: &SourceIdentity,
) -> MigrationPlan {
    let from = &source.identifier;
    let profile = |base: &Path| DirectoryCopy {
        source: base.join(profile_dir_name(from, debug)),
        target: base.join(profile_dir_name(target_identifier, debug)),
    };
    let mut extra_directories = Vec::new();
    if dirs.config != dirs.data {
        extra_directories.push(profile(&dirs.config));
    }
    if let Some(webkit) = &dirs.webkit {
        extra_directories.push(DirectoryCopy {
            source: webkit.join(webkit_dir_name(from, debug)),
            target: webkit.join(webkit_dir_name(target_identifier, debug)),
        });
    }
    // Tauri gives WebView2 `<local data>/<identifier>` in every build.
    if let Some(local) = &dirs.local_data {
        extra_directories.push(DirectoryCopy {
            source: local.join(from),
            target: local.join(target_identifier),
        });
    }
    let credentials = CREDENTIAL_SERVICES
        .iter()
        .map(|(name, bridged)| CredentialCopy {
            source_service: source_service(&source.credential_prefix, debug, name),
            legacy_service: source
                .legacy_credential_prefix
                .as_deref()
                .filter(|_| *bridged)
                .map(|prefix| source_service(prefix, debug, name)),
            target_service: crate::app_identity::credential_service_for(
                target_identifier,
                debug,
                name,
            ),
        })
        .collect();
    MigrationPlan {
        data: profile(&dirs.data),
        extra_directories,
        credentials,
        source_executables: source.executables.clone(),
        lock_wait: std::time::Duration::from_secs(600),
    }
}

/// Credential storage the migration reads from and writes to.
pub trait CredentialStore {
    fn accounts(&self, service: &str) -> Result<Vec<String>, String>;
    fn get(&self, service: &str, account: &str) -> Result<Option<String>, String>;
    fn set(&self, service: &str, account: &str, value: &str) -> Result<(), String>;
    fn delete(&self, service: &str, account: &str) -> Result<(), String>;
}

/// Copies one regular file; tests inject failures through it.
pub trait FileCopier {
    fn copy_file(&mut self, from: &Path, to: &Path) -> std::io::Result<()>;

    /// Publishes a staged folder; tests stand in a crash here.
    fn rename(&mut self, from: &Path, to: &Path) -> std::io::Result<()> {
        std::fs::rename(from, to)
    }
}

pub struct StdCopier;

impl FileCopier for StdCopier {
    fn copy_file(&mut self, from: &Path, to: &Path) -> std::io::Result<()> {
        std::fs::copy(from, to).map(|_| ())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Skipped(Decision),
    Migrated {
        files: u64,
        bytes: u64,
        credentials: usize,
    },
}

#[derive(Debug, thiserror::Error)]
#[error("identity migration failed: {message}")]
pub struct MigrationError {
    pub message: String,
    /// Nothing was copied because something outside the app must change
    /// first (the old app is running, another launch is migrating): the
    /// launch must stop instead of starting on an empty profile.
    pub blocked: bool,
}

impl MigrationError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            blocked: false,
        }
    }
}

fn fail(context: impl std::fmt::Display, error: impl std::fmt::Display) -> MigrationError {
    MigrationError::new(format!("{context}: {error}"))
}

/// Regular files, their bytes, and symlinks under a tree. Sockets and other
/// special files (a live MCP socket) are neither copied nor counted.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct TreeTotals {
    files: u64,
    bytes: u64,
    links: u64,
}

fn copy_tree(
    source: &Path,
    target: &Path,
    copier: &mut dyn FileCopier,
) -> Result<TreeTotals, MigrationError> {
    let mut totals = TreeTotals::default();
    std::fs::create_dir(target).map_err(|e| fail(format!("creating {}", target.display()), e))?;
    let entries =
        std::fs::read_dir(source).map_err(|e| fail(format!("reading {}", source.display()), e))?;
    for entry in entries {
        let entry = entry.map_err(|e| fail(format!("reading {}", source.display()), e))?;
        let from = entry.path();
        let to = target.join(entry.file_name());
        let kind = std::fs::symlink_metadata(&from)
            .map_err(|e| fail(format!("reading {}", from.display()), e))?;
        if kind.is_dir() {
            let nested = copy_tree(&from, &to, copier)?;
            totals.files += nested.files;
            totals.bytes += nested.bytes;
            totals.links += nested.links;
        } else if kind.file_type().is_symlink() {
            copy_link(&from, &to)?;
            totals.links += 1;
        } else if kind.is_file() {
            copier
                .copy_file(&from, &to)
                .map_err(|e| fail(format!("copying {}", from.display()), e))?;
            totals.files += 1;
            totals.bytes += kind.len();
        }
    }
    Ok(totals)
}

fn copy_link(from: &Path, to: &Path) -> Result<(), MigrationError> {
    let link =
        std::fs::read_link(from).map_err(|e| fail(format!("reading {}", from.display()), e))?;
    #[cfg(unix)]
    let created = std::os::unix::fs::symlink(&link, to);
    #[cfg(windows)]
    let created = if from.is_dir() {
        std::os::windows::fs::symlink_dir(&link, to)
    } else {
        std::os::windows::fs::symlink_file(&link, to)
    };
    created.map_err(|e| fail(format!("linking {}", to.display()), e))
}

fn measure(root: &Path) -> Result<TreeTotals, MigrationError> {
    let mut totals = TreeTotals::default();
    let entries =
        std::fs::read_dir(root).map_err(|e| fail(format!("reading {}", root.display()), e))?;
    for entry in entries {
        let path = entry
            .map_err(|e| fail(format!("reading {}", root.display()), e))?
            .path();
        let kind = std::fs::symlink_metadata(&path)
            .map_err(|e| fail(format!("reading {}", path.display()), e))?;
        if kind.is_dir() {
            let nested = measure(&path)?;
            totals.files += nested.files;
            totals.bytes += nested.bytes;
            totals.links += nested.links;
        } else if kind.file_type().is_symlink() {
            totals.links += 1;
        } else if kind.is_file() {
            totals.files += 1;
            totals.bytes += kind.len();
        }
    }
    Ok(totals)
}

/// `<target name>.identity-migration-<pid>` beside the target, so the final
/// rename stays on one volume.
fn staging_path(target: &Path) -> Result<PathBuf, MigrationError> {
    let name = target
        .file_name()
        .ok_or_else(|| MigrationError::new(format!("{} has no name", target.display())))?;
    let mut staged = name.to_os_string();
    staged.push(format!(".identity-migration-{}", std::process::id()));
    Ok(target.with_file_name(staged))
}

/// Staging folders an interrupted earlier launch left behind.
fn remove_stale_staging(target: &Path) {
    let (Some(parent), Some(name)) = (target.parent(), target.file_name()) else {
        return;
    };
    let prefix = format!("{}.identity-migration-", name.to_string_lossy());
    let Ok(entries) = std::fs::read_dir(parent) else {
        return;
    };
    for entry in entries.flatten() {
        if entry.file_name().to_string_lossy().starts_with(&prefix) {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

const SYNC_MARKER_SUFFIX: &str = ".compat-sync";
const DELETE_MARKER_PREFIX: &str = "deleted:";

fn fingerprint(value: &str) -> String {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
    use sha2::{Digest, Sha256};
    URL_SAFE_NO_PAD.encode(Sha256::digest(value.as_bytes()))
}

fn read(
    store: &dyn CredentialStore,
    service: &str,
    account: &str,
) -> Result<Option<String>, MigrationError> {
    store
        .get(service, account)
        .map_err(|e| fail(format!("reading credential {service} ({account})"), e))
}

/// The value the old bridge returned for `account`: the June-era twin is the
/// rollback-readable side, a deletion marker hides what it tombstoned, and a
/// Clovy-only value was a rollback sign-out.
fn resolve(
    store: &dyn CredentialStore,
    copy: &CredentialCopy,
    account: &str,
) -> Result<Option<String>, MigrationError> {
    let current = read(store, &copy.source_service, account)?;
    let Some(legacy_service) = copy.legacy_service.as_deref() else {
        return Ok(current);
    };
    let legacy = read(store, legacy_service, account)?;
    let marker_service = format!("{}{SYNC_MARKER_SUFFIX}", copy.source_service);
    let marker = read(store, &marker_service, account)?;
    if let Some(deleted) = marker
        .as_deref()
        .and_then(|marker| marker.strip_prefix(DELETE_MARKER_PREFIX))
    {
        return Ok(legacy.filter(|value| {
            let value = fingerprint(value);
            !deleted.split(',').any(|item| item == value)
        }));
    }
    Ok(match (current, legacy) {
        (Some(current), Some(legacy)) if current == legacy => Some(current),
        (Some(current), Some(legacy)) => {
            let changed_after_sync = marker.as_deref() == Some(fingerprint(&legacy).as_str())
                && marker.as_deref() != Some(fingerprint(&current).as_str());
            Some(if changed_after_sync { current } else { legacy })
        }
        (None, Some(legacy)) => Some(legacy),
        (Some(_), None) | (None, None) => None,
    })
}

struct ResolvedCredential {
    service: String,
    account: String,
    value: zeroize::Zeroizing<String>,
}

fn read_credentials(
    plan: &MigrationPlan,
    store: &dyn CredentialStore,
) -> Result<Vec<ResolvedCredential>, MigrationError> {
    let mut resolved = Vec::new();
    for copy in &plan.credentials {
        let mut accounts = std::collections::BTreeSet::new();
        for service in std::iter::once(&copy.source_service).chain(copy.legacy_service.as_ref()) {
            accounts.extend(
                store
                    .accounts(service)
                    .map_err(|e| fail(format!("listing credentials of {service}"), e))?,
            );
        }
        for account in accounts {
            if let Some(value) = resolve(store, copy, &account)? {
                resolved.push(ResolvedCredential {
                    service: copy.target_service.clone(),
                    account,
                    value: zeroize::Zeroizing::new(value),
                });
            }
        }
    }
    Ok(resolved)
}

/// What a failed run must undo: staging folders, renamed targets and the
/// credentials it created. Nothing here was there before the run.
#[derive(Default)]
struct Undo {
    staged: Vec<PathBuf>,
    published: Vec<PathBuf>,
    credentials: Vec<(String, String)>,
}

impl Undo {
    fn run(self, store: &dyn CredentialStore) {
        for (service, account) in &self.credentials {
            let _ = store.delete(service, account);
        }
        for path in self.staged.iter().chain(&self.published) {
            let _ = std::fs::remove_dir_all(path);
        }
    }
}

pub fn migrate(
    plan: &MigrationPlan,
    store: &dyn CredentialStore,
    copier: &mut dyn FileCopier,
) -> Result<Outcome, MigrationError> {
    match decide(&plan.data.source, &plan.data.target) {
        Decision::Migrate => {}
        other => return Ok(Outcome::Skipped(other)),
    }
    let mut undo = Undo::default();
    match migrate_into(plan, store, copier, &mut undo) {
        Ok(outcome) => Ok(outcome),
        Err(error) => {
            undo.run(store);
            Err(error)
        }
    }
}

fn migrate_into(
    plan: &MigrationPlan,
    store: &dyn CredentialStore,
    copier: &mut dyn FileCopier,
    undo: &mut Undo,
) -> Result<Outcome, MigrationError> {
    let mut moves = Vec::new();
    let mut files = 0;
    let mut bytes = 0;
    let directories = std::iter::once(&plan.data).chain(&plan.extra_directories);
    for copy in directories {
        if decide(&copy.source, &copy.target) != Decision::Migrate {
            continue;
        }
        remove_stale_staging(&copy.target);
        let staging = staging_path(&copy.target)?;
        undo.staged.push(staging.clone());
        let copied = copy_tree(&copy.source, &staging, copier)?;
        let expected = measure(&copy.source)?;
        if copied != expected || measure(&staging)? != expected {
            return Err(MigrationError::new(format!(
                "{} changed or copied incompletely ({copied:?} of {expected:?})",
                copy.source.display()
            )));
        }
        files += copied.files;
        bytes += copied.bytes;
        moves.push((staging, copy.target.clone()));
    }

    // The activity key must arrive before anything opens activity.sqlite3,
    // or the app would create a new database under a new key.
    let credentials = read_credentials(plan, store)?;
    for credential in &credentials {
        if read(store, &credential.service, &credential.account)?.is_some() {
            continue;
        }
        store
            .set(&credential.service, &credential.account, &credential.value)
            .map_err(|e| {
                fail(
                    format!(
                        "writing credential {} ({})",
                        credential.service, credential.account
                    ),
                    e,
                )
            })?;
        undo.credentials
            .push((credential.service.clone(), credential.account.clone()));
    }

    for (staging, target) in moves {
        // Only an empty folder can be here (decided above).
        let _ = std::fs::remove_dir(&target);
        std::fs::rename(&staging, &target)
            .map_err(|e| fail(format!("publishing {}", target.display()), e))?;
        undo.staged.retain(|path| path != &staging);
        undo.published.push(target);
    }
    Ok(Outcome::Migrated {
        files,
        bytes,
        credentials: credentials.len(),
    })
}

/// Names a synthetic source profile for a development build's proof run.
#[cfg(debug_assertions)]
const DEV_SOURCE_ENV: &str = "CLOVY_DEV_IDENTITY_MIGRATION_SOURCE";

fn startup_source(target_identifier: &str) -> Option<SourceIdentity> {
    #[cfg(debug_assertions)]
    if let Some(value) = std::env::var(DEV_SOURCE_ENV)
        .ok()
        .filter(|value| !value.trim().is_empty())
    {
        // An explicit proof source never falls back to the June profile.
        return source_override(value.trim());
    }
    default_source(target_identifier)
}

/// Runs once per launch, before Tauri creates a window or opens a database.
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub fn run_at_startup() {
    let target = crate::app_identity::APP_IDENTIFIER;
    let debug = cfg!(debug_assertions);
    // A development build pointed at release data must not migrate into it.
    if debug && crate::app_paths::use_prod_data_dir() {
        return;
    }
    let Some(source) = startup_source(target) else {
        return;
    };
    let (Some(data), Some(config)) = (dirs::data_dir(), dirs::config_dir()) else {
        return;
    };
    let dirs = PlatformDirs {
        data,
        config,
        local_data: if cfg!(windows) {
            dirs::data_local_dir()
        } else {
            None
        },
        webkit: if cfg!(target_os = "macos") {
            dirs::home_dir().map(|home| home.join("Library").join("WebKit"))
        } else {
            None
        },
    };
    let plan = plan(&dirs, target, debug, &source);
    #[cfg(target_os = "macos")]
    let plan = match macos_bundle_identifier() {
        Some(bundle) => retarget_webkit(plan, &dirs, target, &source.identifier, &bundle),
        None => plan,
    };
    let result = migrate(&plan, &SystemStore, &mut StdCopier);
    let line = match &result {
        Ok(Outcome::Skipped(_)) => return,
        Ok(outcome) => format!(
            "migrated {} -> {}: {outcome:?}",
            plan.data.source.display(),
            plan.data.target.display()
        ),
        Err(error) => format!(
            "{} -> {}: {error}",
            plan.data.source.display(),
            plan.data.target.display()
        ),
    };
    eprintln!("[identity-migration] {line}");
    // Beside, not inside, the new folder: a failed run leaves that empty.
    let mut log = plan.data.target.clone().into_os_string();
    log.push(".identity-migration.log");
    let _ = std::fs::write(PathBuf::from(log), format!("{line}\n"));
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn run_at_startup() {}

/// The WebKit store follows the running bundle's identifier, which a debug
/// proof bundle sets to the bare identifier instead of `<identifier>.dev`.
#[cfg(target_os = "macos")]
fn retarget_webkit(
    mut plan: MigrationPlan,
    dirs: &PlatformDirs,
    target: &str,
    source: &str,
    bundle: &str,
) -> MigrationPlan {
    let (Some(webkit), Some(suffix)) = (&dirs.webkit, bundle.strip_prefix(target)) else {
        return plan;
    };
    for copy in &mut plan.extra_directories {
        if copy.target.parent() == Some(webkit.as_path()) {
            copy.target = webkit.join(bundle);
            copy.source = webkit.join(format!("{source}{suffix}"));
        }
    }
    plan
}

#[cfg(target_os = "macos")]
fn macos_bundle_identifier() -> Option<String> {
    use core_foundation::bundle::CFBundle;
    use core_foundation::string::CFString;

    let info = CFBundle::main_bundle().info_dictionary();
    let value = info.find(CFString::from_static_string("CFBundleIdentifier"))?;
    value.downcast::<CFString>().map(|value| value.to_string())
}

/// The system credential store: the login Keychain on macOS, the Credential
/// Manager on Windows, both as the `keyring` crate names their items.
#[cfg(any(target_os = "macos", target_os = "windows"))]
struct SystemStore;

#[cfg(any(target_os = "macos", target_os = "windows"))]
impl CredentialStore for SystemStore {
    fn accounts(&self, service: &str) -> Result<Vec<String>, String> {
        platform_accounts(service)
    }

    fn get(&self, service: &str, account: &str) -> Result<Option<String>, String> {
        match keyring::Entry::new(service, account).and_then(|entry| entry.get_password()) {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(error.to_string()),
        }
    }

    fn set(&self, service: &str, account: &str, value: &str) -> Result<(), String> {
        keyring::Entry::new(service, account)
            .and_then(|entry| entry.set_password(value))
            .map_err(|error| error.to_string())
    }

    fn delete(&self, service: &str, account: &str) -> Result<(), String> {
        match keyring::Entry::new(service, account).and_then(|entry| entry.delete_credential()) {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(error.to_string()),
        }
    }
}

/// Account names of the generic passwords under `service`. Only attributes
/// are read, so listing never asks the user for access.
#[cfg(target_os = "macos")]
fn platform_accounts(service: &str) -> Result<Vec<String>, String> {
    use security_framework::item::{ItemClass, ItemSearchOptions, Limit};

    const ERR_SEC_ITEM_NOT_FOUND: i32 = -25300;
    match ItemSearchOptions::new()
        .class(ItemClass::generic_password())
        .service(service)
        .load_attributes(true)
        .limit(Limit::All)
        .search()
    {
        Ok(results) => Ok(results
            .iter()
            .filter_map(|result| result.simplify_dict())
            .filter_map(|attributes| attributes.get("acct").cloned())
            .collect()),
        Err(error) if error.code() == ERR_SEC_ITEM_NOT_FOUND => Ok(Vec::new()),
        Err(error) => Err(error.to_string()),
    }
}

/// `keyring` names a Windows credential `<account>.<service>` and keeps the
/// account in `UserName`.
#[cfg(target_os = "windows")]
fn platform_accounts(service: &str) -> Result<Vec<String>, String> {
    use windows::Win32::Foundation::ERROR_NOT_FOUND;
    use windows::Win32::Security::Credentials::{
        CredEnumerateW, CredFree, CREDENTIALW, CRED_TYPE_GENERIC,
    };

    let mut count = 0_u32;
    let mut credentials: *mut *mut CREDENTIALW = std::ptr::null_mut();
    // SAFETY: CredEnumerateW fills `credentials` with `count` pointers that
    // stay valid until the single CredFree below.
    let listed = unsafe {
        CredEnumerateW(
            windows::core::PCWSTR::null(),
            None,
            &mut count,
            &mut credentials,
        )
    };
    if let Err(error) = listed {
        if error.code() == ERROR_NOT_FOUND.to_hresult() {
            return Ok(Vec::new());
        }
        return Err(error.to_string());
    }
    let suffix = format!(".{service}");
    let mut accounts = Vec::new();
    // SAFETY: see above; each pointer is read before the free.
    unsafe {
        for index in 0..count as usize {
            let credential = &**credentials.add(index);
            if credential.Type != CRED_TYPE_GENERIC
                || credential.TargetName.is_null()
                || credential.UserName.is_null()
            {
                continue;
            }
            let (Ok(target), Ok(user)) = (
                credential.TargetName.to_string(),
                credential.UserName.to_string(),
            ) else {
                continue;
            };
            if target.strip_suffix(&suffix) == Some(user.as_str()) {
                accounts.push(user);
            }
        }
        CredFree(credentials as *const std::ffi::c_void);
    }
    Ok(accounts)
}
