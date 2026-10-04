//! Read-only readers for the seven agents' local stores. Every reader opens
//! files for reading only (SQLite stores with `mode=ro`), never creates,
//! renames, or touches anything under the agent's directories, and turns one
//! store entry into a normalized [`Session`].
//!
//! Discovery is cheap (directory listing and metadata); a candidate is loaded
//! only when its fingerprint (size and mtime of the file and its WAL) changed
//! since the last scan.

pub mod antigravity;
pub mod claude;
pub mod codex;
pub mod copilot_cli;
pub mod copilot_vscode;
pub mod cursor;
pub mod cursor_cli;
pub mod sqlite;

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use chrono::{DateTime, Utc};
use serde_json::Value;

use super::record::Session;
use super::SourceId;

/// Where the agents keep their data. Production uses the user's home;
/// tests point it at a temporary directory.
#[derive(Clone, Debug)]
pub struct SourceRoots {
    pub home: PathBuf,
}

impl SourceRoots {
    pub fn from_home(home: impl Into<PathBuf>) -> Self {
        Self { home: home.into() }
    }

    pub fn current() -> Option<Self> {
        std::env::var_os("HOME")
            .filter(|home| !home.is_empty())
            .map(|home| Self::from_home(PathBuf::from(home)))
    }

    pub fn app_support(&self, app: &str) -> PathBuf {
        self.home
            .join("Library")
            .join("Application Support")
            .join(app)
    }

    /// The directory whose existence means the agent was used on this Mac.
    pub fn presence_path(&self, source: SourceId) -> PathBuf {
        match source {
            SourceId::ClaudeCode => claude::root(self),
            SourceId::Codex => codex::root(self),
            SourceId::CopilotCli => copilot_cli::root(self),
            SourceId::CopilotVscode => copilot_vscode::user_dir(self),
            SourceId::Cursor => cursor::database(self),
            SourceId::CursorCli => cursor_cli::root(self),
            SourceId::Antigravity => antigravity::cli_root(self),
        }
    }

    pub fn is_present(&self, source: SourceId) -> bool {
        let present = self.presence_path(source).exists();
        if source == SourceId::Antigravity {
            return present || antigravity::ide_root(self).exists();
        }
        present
    }
}

/// Size and mtime of a store file plus its SQLite WAL, if any.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fingerprint(Vec<(u64, Option<SystemTime>)>);

/// One store entry that may hold new turns.
#[derive(Clone, Debug)]
pub struct Candidate {
    pub source: SourceId,
    pub path: PathBuf,
    pub modified: SystemTime,
    pub fingerprint: Fingerprint,
}

fn metadata_pair(path: &Path) -> Option<(u64, Option<SystemTime>)> {
    let metadata = fs::metadata(path).ok()?;
    Some((metadata.len(), metadata.modified().ok()))
}

/// A candidate for `path` when it (or its WAL) changed at or after `since`.
pub fn candidate(source: SourceId, path: PathBuf, since: SystemTime) -> Option<Candidate> {
    let main = metadata_pair(&path)?;
    let wal = metadata_pair(&PathBuf::from(format!("{}-wal", path.display())));
    let modified = [main.1, wal.and_then(|wal| wal.1)]
        .into_iter()
        .flatten()
        .max()?;
    if modified < since {
        return None;
    }
    let mut parts = vec![main];
    parts.extend(wal);
    Some(Candidate {
        source,
        path,
        modified,
        fingerprint: Fingerprint(parts),
    })
}

/// Entries of `dir` (sorted for determinism); empty when it cannot be read.
pub fn list_dir(dir: &Path) -> Vec<PathBuf> {
    let mut entries: Vec<PathBuf> = fs::read_dir(dir)
        .map(|entries| entries.flatten().map(|entry| entry.path()).collect())
        .unwrap_or_default();
    entries.sort();
    entries
}

/// Store entries of `source` changed at or after `since`.
pub fn discover(source: SourceId, roots: &SourceRoots, since: SystemTime) -> Vec<Candidate> {
    let paths = match source {
        SourceId::ClaudeCode => claude::discover(roots),
        SourceId::Codex => codex::discover(roots),
        SourceId::CopilotCli => copilot_cli::discover(roots),
        SourceId::CopilotVscode => copilot_vscode::discover(roots),
        SourceId::Cursor => vec![cursor::database(roots)],
        SourceId::CursorCli => cursor_cli::discover(roots),
        SourceId::Antigravity => antigravity::discover(roots),
    };
    paths
        .into_iter()
        .filter_map(|path| candidate(source, path, since))
        .collect()
}

/// Loads the sessions of one candidate. `since` bounds multi-session stores
/// (Cursor keeps every conversation in one database). An error (a store
/// caught mid-write) leaves the candidate to be retried on the next scan.
pub async fn load(candidate: &Candidate, since: DateTime<Utc>) -> Result<Vec<Session>, String> {
    match candidate.source {
        SourceId::Cursor => cursor::load(&candidate.path, since).await,
        SourceId::CursorCli => cursor_cli::load(&candidate.path, candidate.modified)
            .await
            .map(|session| session.into_iter().collect()),
        source => {
            let path = candidate.path.clone();
            tokio::task::spawn_blocking(move || load_file(source, &path))
                .await
                .map(|session| session.into_iter().collect())
                .map_err(|error| error.to_string())
        }
    }
}

fn load_file(source: SourceId, path: &Path) -> Option<Session> {
    match source {
        SourceId::ClaudeCode => claude::load(path),
        SourceId::Codex => codex::load(path),
        SourceId::CopilotCli => copilot_cli::load(path),
        SourceId::CopilotVscode => copilot_vscode::load(path),
        SourceId::Antigravity => antigravity::load(path),
        SourceId::Cursor | SourceId::CursorCli => None,
    }
}

/// Every well-formed JSON object line of a file. Partial last lines (the
/// agent is mid-write), invalid UTF-8, and malformed lines are skipped; the
/// next scan sees the completed line.
pub fn read_json_lines(path: &Path) -> Vec<Value> {
    let Ok(file) = fs::File::open(path) else {
        return Vec::new();
    };
    let mut reader = BufReader::new(file);
    let mut out = Vec::new();
    let mut line = Vec::new();
    loop {
        line.clear();
        match reader.read_until(b'\n', &mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {
                let text = String::from_utf8_lossy(&line);
                let text = text.trim();
                if text.is_empty() {
                    continue;
                }
                if let Ok(value @ Value::Object(_)) = serde_json::from_str::<Value>(text) {
                    out.push(value);
                }
            }
        }
    }
    out
}

pub fn str_at<'a>(value: &'a Value, path: &[&str]) -> Option<&'a str> {
    let mut current = value;
    for key in path {
        current = current.get(key)?;
    }
    current.as_str()
}

pub fn file_stem(path: &Path) -> Option<String> {
    path.file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .filter(|stem| !stem.is_empty())
}

/// `file:///Users/me/My%20Repo` → `/Users/me/My Repo`.
pub fn path_from_file_uri(uri: &str) -> Option<String> {
    let rest = uri.strip_prefix("file://")?;
    let path = rest.strip_prefix("localhost").unwrap_or(rest);
    urlencoding::decode(path)
        .ok()
        .map(|decoded| decoded.into_owned())
        .filter(|decoded| decoded.starts_with('/'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_uris_decode_to_paths() {
        assert_eq!(
            path_from_file_uri("file:///Users/me/My%20Repo").as_deref(),
            Some("/Users/me/My Repo")
        );
        assert_eq!(path_from_file_uri("vscode-remote://x/y"), None);
    }

    #[test]
    fn json_lines_skip_partial_and_malformed_lines() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.jsonl");
        fs::write(
            &path,
            "{\"a\":1}\nnot json\n\n[1]\n{\"b\":2}\n{\"partial\":",
        )
        .unwrap();
        let values = read_json_lines(&path);
        assert_eq!(values.len(), 2);
    }
}
