//! One-time copy of the June-era identity into Clovy's own (ADR-0068).
//!
//! The first launch under the new identifier copies the old data and config
//! directories, the WebView store and the credential items into the new
//! names, before anything opens a database. The copy is all or nothing and
//! never touches the source.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};

mod database;
mod guard;
#[cfg(test)]
mod tests;

use database::PathRewrite;
use std::collections::BTreeSet;

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
    /// An earlier launch's interrupted publication was finished.
    Resumed,
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
/// special files (a live MCP socket) are neither copied nor counted, and a
/// database counts as one file whatever the size of its snapshot.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct TreeTotals {
    files: u64,
    bytes: u64,
    links: u64,
}

impl TreeTotals {
    fn add(&mut self, other: TreeTotals) {
        self.files += other.files;
        self.bytes += other.bytes;
        self.links += other.links;
    }
}

/// What one directory copy needs besides the copier.
struct CopyContext<'a> {
    key: Option<&'a str>,
    rewrite: &'a PathRewrite,
    scratch: PathBuf,
    /// Source databases that were snapshotted, and their staged copies.
    snapshotted: BTreeSet<PathBuf>,
    databases: Vec<(PathBuf, Option<String>)>,
}

fn sorted_entries(source: &Path) -> Result<Vec<std::fs::DirEntry>, MigrationError> {
    let mut entries = std::fs::read_dir(source)
        .map_err(|e| fail(format!("reading {}", source.display()), e))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| fail(format!("reading {}", source.display()), e))?;
    // A database before its WAL: whether the WAL is copied depends on it.
    entries.sort_by_key(|entry| {
        let name = entry.file_name().to_string_lossy().into_owned();
        (database::companion_of(&name).is_some(), name)
    });
    Ok(entries)
}

fn copy_tree(
    source: &Path,
    target: &Path,
    copier: &mut dyn FileCopier,
    context: &mut CopyContext,
) -> Result<TreeTotals, MigrationError> {
    let mut totals = TreeTotals::default();
    std::fs::create_dir(target).map_err(|e| fail(format!("creating {}", target.display()), e))?;
    for entry in sorted_entries(source)? {
        let from = entry.path();
        let to = target.join(entry.file_name());
        let name = entry.file_name().to_string_lossy().into_owned();
        let kind = std::fs::symlink_metadata(&from)
            .map_err(|e| fail(format!("reading {}", from.display()), e))?;
        if kind.is_dir() {
            totals.add(copy_tree(&from, &to, copier, context)?);
        } else if kind.file_type().is_symlink() {
            copy_link(&from, &to, context.rewrite)?;
            totals.links += 1;
        } else if kind.is_file() {
            if database::companion_of(&name).is_some_and(|database| {
                context.snapshotted.contains(&from.with_file_name(database))
            }) {
                continue;
            }
            let key = if database::is_plain_sqlite(&from) {
                Some(None)
            } else if name == database::ENCRYPTED_DATABASE && context.key.is_some() {
                Some(context.key)
            } else {
                None
            };
            if let Some(key) = key {
                let written = database::snapshot(&from, &to, key, &context.scratch, copier)?;
                if written == database::Snapshot::Written {
                    context.snapshotted.insert(from.clone());
                    context.databases.push((to, key.map(str::to_owned)));
                    totals.files += 1;
                    continue;
                }
            }
            copier
                .copy_file(&from, &to)
                .map_err(|e| fail(format!("copying {}", from.display()), e))?;
            totals.files += 1;
            totals.bytes += kind.len();
        }
    }
    Ok(totals)
}

fn copy_link(from: &Path, to: &Path, rewrite: &PathRewrite) -> Result<(), MigrationError> {
    let link =
        std::fs::read_link(from).map_err(|e| fail(format!("reading {}", from.display()), e))?;
    let link = rewrite
        .apply(&link.to_string_lossy())
        .map(PathBuf::from)
        .unwrap_or(link);
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

/// The totals a copy of `root` must reach, counting each of `databases` as
/// one file and skipping its companions.
fn measure(root: &Path, databases: &BTreeSet<PathBuf>) -> Result<TreeTotals, MigrationError> {
    let mut totals = TreeTotals::default();
    for entry in sorted_entries(root)? {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        let kind = std::fs::symlink_metadata(&path)
            .map_err(|e| fail(format!("reading {}", path.display()), e))?;
        if kind.is_dir() {
            totals.add(measure(&path, databases)?);
        } else if kind.file_type().is_symlink() {
            totals.links += 1;
        } else if kind.is_file() {
            if databases.contains(&path) {
                totals.files += 1;
            } else if !database::companion_of(&name)
                .is_some_and(|database| databases.contains(&path.with_file_name(database)))
            {
                totals.files += 1;
                totals.bytes += kind.len();
            }
        }
    }
    Ok(totals)
}

fn regular_files(root: &Path, files: &mut Vec<PathBuf>) -> Result<(), MigrationError> {
    for entry in sorted_entries(root)? {
        let path = entry.path();
        let kind = std::fs::symlink_metadata(&path)
            .map_err(|e| fail(format!("reading {}", path.display()), e))?;
        if kind.is_dir() {
            regular_files(&path, files)?;
        } else if kind.is_file() {
            files.push(path);
        }
    }
    Ok(())
}

/// `<target name>.identity-migration-<pid>` beside the target, so the final
/// rename stays on one volume.
fn staging_path(target: &Path) -> Result<PathBuf, MigrationError> {
    if target.file_name().is_none() {
        return Err(MigrationError::new(format!(
            "{} has no name",
            target.display()
        )));
    }
    Ok(guard::sibling(
        target,
        &format!(".identity-migration-{}", std::process::id()),
    ))
}

/// Staging folders an earlier launch left behind, removed only when their
/// owner is gone (or is this process).
fn remove_stale_staging(target: &Path) {
    let (Some(parent), Some(name)) = (target.parent(), target.file_name()) else {
        return;
    };
    let prefix = format!("{}.identity-migration-", name.to_string_lossy());
    let Ok(entries) = std::fs::read_dir(parent) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(owner) = guard::staging_owner(&name, &prefix) else {
            continue;
        };
        if owner == std::process::id() || !guard::is_alive(owner) {
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

/// The publication in progress, persisted before the first rename so a
/// launch that dies halfway is finished (or undone) by the next one.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct Journal {
    moves: Vec<Move>,
    credentials: Vec<(String, String)>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct Move {
    staging: PathBuf,
    target: PathBuf,
}

fn journal_path(plan: &MigrationPlan) -> PathBuf {
    guard::sibling(&plan.data.target, ".identity-migration.journal")
}

fn write_journal(path: &Path, journal: &Journal) -> Result<(), MigrationError> {
    let temporary = path.with_extension("journal.tmp");
    let bytes = serde_json::to_vec(journal).map_err(|e| fail("encoding the journal", e))?;
    std::fs::write(&temporary, bytes)
        .and_then(|()| std::fs::rename(&temporary, path))
        .map_err(|e| fail(format!("writing {}", path.display()), e))
}

fn is_populated(path: &Path) -> bool {
    std::fs::read_dir(path).is_ok_and(|mut entries| entries.next().is_some())
}

/// Finishes a publication an earlier launch left halfway. When a staged
/// folder is missing it cannot be finished: what it published and the
/// credentials it wrote are removed, and the migration starts over.
fn resume(
    plan: &MigrationPlan,
    store: &dyn CredentialStore,
    copier: &mut dyn FileCopier,
) -> Result<Option<Outcome>, MigrationError> {
    let path = journal_path(plan);
    let Ok(raw) = std::fs::read(&path) else {
        return Ok(None);
    };
    let Ok(journal) = serde_json::from_slice::<Journal>(&raw) else {
        // Written whole or not at all; an unreadable one published nothing.
        let _ = std::fs::remove_file(&path);
        return Ok(None);
    };
    let finishable = journal
        .moves
        .iter()
        .all(|item| is_populated(&item.target) || item.staging.is_dir());
    if finishable {
        for item in &journal.moves {
            if is_populated(&item.target) {
                continue;
            }
            let _ = std::fs::remove_dir(&item.target);
            copier
                .rename(&item.staging, &item.target)
                .map_err(|e| fail(format!("publishing {}", item.target.display()), e))?;
        }
        let _ = std::fs::remove_file(&path);
        return Ok(Some(Outcome::Resumed));
    }
    for item in &journal.moves {
        if item.staging.is_dir() {
            let _ = std::fs::remove_dir_all(&item.staging);
        } else {
            let _ = std::fs::remove_dir_all(&item.target);
        }
    }
    for (service, account) in &journal.credentials {
        let _ = store.delete(service, account);
    }
    let _ = std::fs::remove_file(&path);
    Ok(None)
}

pub fn migrate(
    plan: &MigrationPlan,
    store: &dyn CredentialStore,
    copier: &mut dyn FileCopier,
) -> Result<Outcome, MigrationError> {
    let _lock = guard::MigrationLock::acquire(&plan.data.target, plan.lock_wait)?;
    if let Some(outcome) = resume(plan, store, copier)? {
        return Ok(outcome);
    }
    match decide(&plan.data.source, &plan.data.target) {
        Decision::Migrate => {}
        other => return Ok(Outcome::Skipped(other)),
    }
    let running = guard::running_executables(&plan.source_executables)?;
    if !running.is_empty() {
        return Err(MigrationError {
            message: format!(
                "{} from the previous identity is still running; quit it and reopen Clovy",
                running.join(", ")
            ),
            blocked: true,
        });
    }
    let mut undo = Undo::default();
    match migrate_into(plan, store, copier, &mut undo) {
        Ok(outcome) => Ok(outcome),
        Err(error) => {
            undo.run(store);
            let _ = std::fs::remove_file(journal_path(plan));
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
    // Read first: the activity key opens the database being snapshotted, and
    // a denied credential fails before anything is copied.
    let credentials = read_credentials(plan, store)?;
    let key = credentials
        .iter()
        .find(|credential| {
            credential.service.ends_with(".activity-db") && credential.account == "sqlcipher-key"
        })
        .map(|credential| credential.value.to_string());
    let directories: Vec<&DirectoryCopy> = std::iter::once(&plan.data)
        .chain(&plan.extra_directories)
        .collect();
    let rewrite = PathRewrite::new(
        &directories
            .iter()
            .map(|copy| (copy.source.clone(), copy.target.clone()))
            .collect::<Vec<_>>(),
    );

    let mut moves = Vec::new();
    let mut files = 0;
    let mut bytes = 0;
    for copy in &directories {
        if decide(&copy.source, &copy.target) != Decision::Migrate {
            continue;
        }
        remove_stale_staging(&copy.target);
        let staging = staging_path(&copy.target)?;
        let scratch = guard::sibling(&staging, ".scratch");
        undo.staged.push(staging.clone());
        undo.staged.push(scratch.clone());
        let mut context = CopyContext {
            key: key.as_deref(),
            rewrite: &rewrite,
            scratch,
            snapshotted: BTreeSet::new(),
            databases: Vec::new(),
        };
        let copied = copy_tree(&copy.source, &staging, copier, &mut context)?;
        let expected = measure(&copy.source, &context.snapshotted)?;
        let staged: BTreeSet<PathBuf> = context
            .databases
            .iter()
            .map(|(path, _)| path.clone())
            .collect();
        if copied != expected || measure(&staging, &staged)? != expected {
            return Err(MigrationError::new(format!(
                "{} changed or copied incompletely ({copied:?} of {expected:?})",
                copy.source.display()
            )));
        }
        // Paths into the old profile now point into the new one.
        if !rewrite.is_empty() {
            for (database, database_key) in &context.databases {
                rewrite.apply_to_database(database, database_key.as_deref())?;
            }
            let mut texts = Vec::new();
            regular_files(&staging, &mut texts)?;
            for path in texts.iter().filter(|path| !staged.contains(*path)) {
                rewrite.apply_to_text_file(path)?;
            }
        }
        files += copied.files;
        bytes += copied.bytes;
        moves.push(Move {
            staging,
            target: copy.target.clone(),
        });
    }

    // The activity key must arrive before anything opens activity.sqlite3,
    // and a key minted by an earlier failed launch would make the copied
    // database unreadable: an existing different value stops everything.
    for credential in &credentials {
        match read(store, &credential.service, &credential.account)? {
            Some(existing) if existing == *credential.value => continue,
            Some(_) => {
                return Err(MigrationError::new(format!(
                    "credential {} ({}) already exists with a different value; remove it and reopen Clovy",
                    credential.service, credential.account
                )))
            }
            None => {}
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

    // The data folder is what the next launch's decision looks at, so it is
    // published last; the journal lets a launch that dies meanwhile resume.
    moves.sort_by_key(|item| item.target == plan.data.target);
    let journal = journal_path(plan);
    write_journal(
        &journal,
        &Journal {
            moves,
            credentials: undo.credentials.clone(),
        },
    )?;
    let moves = serde_json::from_slice::<Journal>(
        &std::fs::read(&journal).map_err(|e| fail(format!("reading {}", journal.display()), e))?,
    )
    .map_err(|e| fail("reading the journal", e))?
    .moves;
    for item in moves {
        // Only an empty folder can be here (decided above).
        let _ = std::fs::remove_dir(&item.target);
        copier
            .rename(&item.staging, &item.target)
            .map_err(|e| fail(format!("publishing {}", item.target.display()), e))?;
        undo.staged.retain(|path| path != &item.staging);
        undo.published.push(item.target);
    }
    for leftover in &undo.staged {
        let _ = std::fs::remove_dir_all(leftover);
    }
    let _ = std::fs::remove_file(&journal);
    Ok(Outcome::Migrated {
        files,
        bytes,
        credentials: credentials.len(),
    })
}

/// Names a synthetic source profile for a development build's proof run.
#[cfg(debug_assertions)]
const DEV_SOURCE_ENV: &str = "CLOVY_DEV_IDENTITY_MIGRATION_SOURCE";
/// The executable name of that synthetic profile's "old app".
#[cfg(debug_assertions)]
const DEV_SOURCE_APP_ENV: &str = "CLOVY_DEV_IDENTITY_MIGRATION_SOURCE_APP";

fn startup_source(target_identifier: &str) -> Option<SourceIdentity> {
    #[cfg(debug_assertions)]
    if let Some(value) = std::env::var(DEV_SOURCE_ENV)
        .ok()
        .filter(|value| !value.trim().is_empty())
    {
        // An explicit proof source never falls back to the June profile.
        return source_override(value.trim()).map(|mut source| {
            if let Ok(app) = std::env::var(DEV_SOURCE_APP_ENV) {
                source.executables = vec![app.trim().to_string()];
            }
            source
        });
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
    // Starting now would create the new profile and the next launch would
    // never migrate: tell the owner what to do and stop.
    if let Err(error) = &result {
        if error.blocked {
            alert(&error.message);
            std::process::exit(1);
        }
    }
}

/// A native alert before any window exists.
#[cfg(any(target_os = "macos", target_os = "windows"))]
fn alert(message: &str) {
    let message = format!("Clovy could not move your data to its new identity yet.\n\n{message}");
    // Shown by Clovy itself on the main thread: an alert from a helper
    // process can be dismissed without ever reaching the owner.
    #[cfg(target_os = "macos")]
    if let Some(mtm) = objc2::MainThreadMarker::new() {
        use objc2_app_kit::{NSAlert, NSAlertStyle, NSApplication, NSApplicationActivationPolicy};
        use objc2_foundation::NSString;

        let app = NSApplication::sharedApplication(mtm);
        app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
        #[allow(deprecated)]
        app.activateIgnoringOtherApps(true);
        let alert = NSAlert::new(mtm);
        alert.setAlertStyle(NSAlertStyle::Critical);
        alert.setMessageText(&NSString::from_str("Clovy"));
        alert.setInformativeText(&NSString::from_str(&message));
        alert.runModal();
    }
    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            "Add-Type -AssemblyName PresentationFramework; [System.Windows.MessageBox]::Show($env:CLOVY_ALERT, 'Clovy') | Out-Null",
        ])
        .env("CLOVY_ALERT", &message)
        .status();
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
