//! One-time copy of the June-era identity into Clovy's own (ADR-0068).
//!
//! The first launch under the new identifier copies the old data and config
//! directories, the WebView store and the credential items into the new
//! names, before anything opens a database. The copy is all or nothing and
//! never touches the source.

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

pub fn decide(_source: &Path, _target: &Path) -> Decision {
    Decision::NoSource
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
}

/// The June-era identity, migrated only into the release identifier (and its
/// `-dev` profile). A development identity of its own never reads it.
pub fn default_source(_target_identifier: &str) -> Option<SourceIdentity> {
    None
}

/// A source named by `CLOVY_DEV_IDENTITY_MIGRATION_SOURCE` in a development
/// build, so a native proof migrates a synthetic profile instead.
pub fn source_override(_identifier: &str) -> Option<SourceIdentity> {
    None
}

pub fn plan(
    dirs: &PlatformDirs,
    _target_identifier: &str,
    _debug: bool,
    _source: &SourceIdentity,
) -> MigrationPlan {
    MigrationPlan {
        data: DirectoryCopy {
            source: dirs.data.clone(),
            target: dirs.data.clone(),
        },
        extra_directories: Vec::new(),
        credentials: Vec::new(),
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
#[error("identity migration failed: {0}")]
pub struct MigrationError(pub String);

pub fn migrate(
    _plan: &MigrationPlan,
    _store: &dyn CredentialStore,
    _copier: &mut dyn FileCopier,
) -> Result<Outcome, MigrationError> {
    Err(MigrationError("not implemented".into()))
}
