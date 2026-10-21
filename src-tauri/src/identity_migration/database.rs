//! Databases travel as single-file snapshots, and every path into the old
//! profile is rewritten to the new one in the staged copy.

use std::path::{Path, PathBuf};

use sqlx::connection::{ConnectOptions, Connection};
use sqlx::row::Row;
use sqlx_sqlite::{SqliteConnectOptions, SqliteConnection};

use super::{fail, FileCopier, MigrationError};

const SQLITE_HEADER: &[u8; 16] = b"SQLite format 3\0";
/// The one encrypted database; its key comes from the migrated credentials.
pub(super) const ENCRYPTED_DATABASE: &str = "activity.sqlite3";
const COMPANION_SUFFIXES: [&str; 3] = ["-wal", "-shm", "-journal"];
/// Larger files are recordings or models, never path-bearing text.
const MAX_TEXT_FILE_BYTES: u64 = 16 * 1024 * 1024;

pub(super) fn is_plain_sqlite(path: &Path) -> bool {
    use std::io::Read;
    let mut header = [0_u8; 16];
    std::fs::File::open(path)
        .and_then(|mut file| file.read_exact(&mut header))
        .is_ok_and(|()| &header == SQLITE_HEADER)
}

/// `notes.sqlite3-wal` -> `notes.sqlite3`.
pub(super) fn companion_of(name: &str) -> Option<&str> {
    COMPANION_SUFFIXES
        .iter()
        .find_map(|suffix| name.strip_suffix(suffix))
}

/// Runs database work on its own thread and runtime: the migration runs
/// before Tauri's runtime exists, and tests call it from inside one.
fn block_on<T: Send + 'static>(
    work: impl std::future::Future<Output = Result<T, MigrationError>> + Send + 'static,
) -> Result<T, MigrationError> {
    std::thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| fail("starting the database runtime", e))?
            .block_on(work)
    })
    .join()
    .map_err(|_| MigrationError::new("the database snapshot thread panicked"))?
}

fn options(path: &Path, key: Option<&str>) -> SqliteConnectOptions {
    let options = SqliteConnectOptions::new().filename(path);
    match key {
        // The raw-key form ActivityStore uses.
        Some(key) => options.pragma("key", format!("\"x'{key}'\"")),
        None => options,
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Snapshot {
    Written,
    /// The key does not open it: copied byte for byte like any other file.
    NotADatabase,
}

/// Writes a consistent single-file snapshot of `from` (and its WAL) to `to`.
///
/// The source is never opened: SQLite would create or checkpoint its WAL and
/// change the old profile. The database and its companions are copied to a
/// private scratch folder, the copy is checked against a source that did not
/// change meanwhile, and `VACUUM INTO` writes the snapshot from there.
pub(super) fn snapshot(
    from: &Path,
    to: &Path,
    key: Option<&str>,
    scratch: &Path,
    copier: &mut dyn FileCopier,
) -> Result<Snapshot, MigrationError> {
    let name = from
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let members: Vec<(PathBuf, PathBuf)> = std::iter::once(name.clone())
        .chain(
            COMPANION_SUFFIXES
                .iter()
                .map(|suffix| format!("{name}{suffix}")),
        )
        .map(|member| (from.with_file_name(&member), scratch.join(&member)))
        .filter(|(source, _)| source.is_file())
        .collect();
    std::fs::create_dir_all(scratch)
        .map_err(|e| fail(format!("creating {}", scratch.display()), e))?;
    let before = stamps(&members)?;
    for (source, copy) in &members {
        copier
            .copy_file(source, copy)
            .map_err(|e| fail(format!("copying {}", source.display()), e))?;
    }
    if stamps(&members)? != before {
        return Err(MigrationError::new(format!(
            "{} changed while it was being copied; quit every app that uses it and reopen Clovy",
            from.display()
        )));
    }
    let scratch_db = scratch.join(&name);
    let target = to.to_path_buf();
    let key = key.map(str::to_owned);
    let written = block_on(async move {
        let mut connection = match options(&scratch_db, key.as_deref()).connect().await {
            Ok(connection) => connection,
            Err(error) if key.is_some() && not_a_database(&error) => {
                return Ok(Snapshot::NotADatabase)
            }
            Err(error) => return Err(fail(format!("opening {}", scratch_db.display()), error)),
        };
        if let Err(error) = sqlx::query::query("SELECT count(*) FROM sqlite_master")
            .fetch_one(&mut connection)
            .await
        {
            if key.is_some() && not_a_database(&error) {
                return Ok(Snapshot::NotADatabase);
            }
            return Err(fail(format!("reading {}", scratch_db.display()), error));
        }
        sqlx::query::query("VACUUM INTO ?")
            .bind(target.to_string_lossy().into_owned())
            .execute(&mut connection)
            .await
            .map_err(|e| fail(format!("snapshotting into {}", target.display()), e))?;
        connection
            .close()
            .await
            .map_err(|e| fail(format!("closing {}", scratch_db.display()), e))?;
        // The snapshot must be whole, and an encrypted one must stay so.
        if key.is_some() && is_plain_sqlite(&target) {
            return Err(MigrationError::new(format!(
                "the snapshot of {} was written without encryption",
                target.display()
            )));
        }
        let mut copy = options(&target, key.as_deref())
            .connect()
            .await
            .map_err(|e| fail(format!("opening {}", target.display()), e))?;
        let check: String = sqlx::query::query("PRAGMA quick_check")
            .fetch_one(&mut copy)
            .await
            .map_err(|e| fail(format!("checking {}", target.display()), e))?
            .get(0);
        copy.close()
            .await
            .map_err(|e| fail(format!("closing {}", target.display()), e))?;
        if check != "ok" {
            return Err(MigrationError::new(format!(
                "the snapshot of {} failed its integrity check: {check}",
                target.display()
            )));
        }
        Ok(Snapshot::Written)
    });
    let _ = std::fs::remove_dir_all(scratch);
    let written = written?;
    if written == Snapshot::NotADatabase {
        let _ = std::fs::remove_file(to);
    }
    Ok(written)
}

fn not_a_database(error: &sqlx::Error) -> bool {
    error.to_string().contains("file is not a database")
}

type Stamp = (PathBuf, u64, Option<std::time::SystemTime>);

fn stamps(members: &[(PathBuf, PathBuf)]) -> Result<Vec<Stamp>, MigrationError> {
    members
        .iter()
        .map(|(source, _)| {
            let metadata = std::fs::metadata(source)
                .map_err(|e| fail(format!("reading {}", source.display()), e))?;
            Ok((source.clone(), metadata.len(), metadata.modified().ok()))
        })
        .collect()
}

/// Every spelling of "inside the old profile" and its new equivalent.
///
/// A root is rewritten only where a path starts with it: at the start of a
/// value, or after whitespace, a quote or punctuation, and only when the path
/// goes on with a separator or ends there. A path that merely contains the old
/// folder (`/Volumes/Backup/Users/...`) or a look-alike sibling
/// (`...-dev-backup`) stays as it is.
#[derive(Debug, Clone, Default)]
pub(super) struct PathRewrite {
    /// `(old root, new root)`, longest first.
    roots: Vec<(String, String)>,
}

impl PathRewrite {
    pub(super) fn new(folders: &[(PathBuf, PathBuf)]) -> Self {
        let mut roots = Vec::new();
        for (old, new) in folders {
            let mut forms = vec![(
                old.to_string_lossy().into_owned(),
                new.to_string_lossy().into_owned(),
            )];
            // The app stores canonical paths (`/private/var` for `/var`).
            if let (Ok(old_canonical), Some(parent), Some(name)) =
                (old.canonicalize(), new.parent(), new.file_name())
            {
                if let Ok(parent) = parent.canonicalize() {
                    forms.push((
                        old_canonical.to_string_lossy().into_owned(),
                        parent.join(name).to_string_lossy().into_owned(),
                    ));
                }
            }
            for (old, new) in forms {
                // Text that is not JSON may still carry JSON-escaped paths.
                if old.contains('\\') {
                    let escape = |value: &str| value.replace('\\', "\\\\");
                    roots.push((escape(&old), escape(&new)));
                }
                roots.push((old, new));
            }
        }
        roots.sort_by_key(|(old, _): &(String, String)| std::cmp::Reverse(old.len()));
        roots.dedup();
        Self { roots }
    }

    /// The rewritten value, or `None` when nothing in it points inside. JSON
    /// is rewritten string by string, so a value equal to the root is too.
    pub(super) fn apply(&self, text: &str) -> Option<String> {
        let trimmed = text.trim_start();
        if trimmed.starts_with('{') || trimmed.starts_with('[') {
            if let Ok(mut value) = serde_json::from_str::<serde_json::Value>(text) {
                return self.rewrite_json(&mut value).then(|| value.to_string());
            }
        }
        self.apply_plain(text)
    }

    fn rewrite_json(&self, value: &mut serde_json::Value) -> bool {
        match value {
            serde_json::Value::String(text) => match self.apply_plain(text) {
                Some(rewritten) => {
                    *text = rewritten;
                    true
                }
                None => false,
            },
            serde_json::Value::Array(items) => items
                .iter_mut()
                .fold(false, |changed, item| self.rewrite_json(item) | changed),
            serde_json::Value::Object(fields) => fields
                .values_mut()
                .fold(false, |changed, item| self.rewrite_json(item) | changed),
            _ => false,
        }
    }

    /// Rewrites each root that starts a path inside free text.
    fn apply_plain(&self, text: &str) -> Option<String> {
        let mut output = String::with_capacity(text.len());
        let mut changed = false;
        let mut index = 0;
        let mut previous: Option<char> = None;
        while index < text.len() {
            let rest = &text[index..];
            if starts_a_path(previous) {
                if let Some((old, new)) = self.roots.iter().find(|(old, _)| {
                    rest.starts_with(old.as_str()) && ends_a_root(&rest[old.len()..])
                }) {
                    output.push_str(new);
                    index += old.len();
                    previous = old.chars().last();
                    changed = true;
                    continue;
                }
            }
            let character = rest.chars().next().unwrap_or_default();
            output.push(character);
            index += character.len_utf8();
            previous = Some(character);
        }
        changed.then_some(output)
    }

    /// The strings any rewritten value must contain, to narrow the scans.
    fn needles(&self) -> Vec<String> {
        self.roots.iter().map(|(old, _)| old.clone()).collect()
    }

    pub(super) fn is_empty(&self) -> bool {
        self.roots.is_empty()
    }

    /// Rewrites every text value (and WebKit's UTF-16 localStorage values)
    /// of every ordinary table. Virtual tables and their shadow tables are
    /// left to their own triggers.
    pub(super) fn apply_to_database(
        &self,
        path: &Path,
        key: Option<&str>,
    ) -> Result<u64, MigrationError> {
        let rewrite = self.clone();
        let path = path.to_path_buf();
        let key = key.map(str::to_owned);
        block_on(async move {
            let mut connection = options(&path, key.as_deref())
                .connect()
                .await
                .map_err(|e| fail(format!("opening {}", path.display()), e))?;
            let changed = rewrite
                .rewrite_tables(&mut connection)
                .await
                .map_err(|e| fail(format!("rewriting paths in {}", path.display()), e))?;
            connection
                .close()
                .await
                .map_err(|e| fail(format!("closing {}", path.display()), e))?;
            Ok(changed)
        })
    }

    async fn rewrite_tables(&self, connection: &mut SqliteConnection) -> Result<u64, sqlx::Error> {
        let tables: Vec<(String, String)> = sqlx::query::query(
            "SELECT name, coalesce(sql, '') FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
        )
        .fetch_all(&mut *connection)
        .await?
        .iter()
        .map(|row| (row.get(0), row.get(1)))
        .collect();
        let virtual_tables: Vec<String> = tables
            .iter()
            .filter(|(_, sql)| sql.to_ascii_uppercase().starts_with("CREATE VIRTUAL"))
            .map(|(name, _)| name.clone())
            .collect();
        let needles = self.needles();
        let mut changed = 0;
        for (table, sql) in &tables {
            let shadow = virtual_tables
                .iter()
                .any(|virtual_table| table.starts_with(&format!("{virtual_table}_")));
            if sql.to_ascii_uppercase().starts_with("CREATE VIRTUAL") || shadow {
                continue;
            }
            let columns: Vec<String> = sqlx::query::query("SELECT name FROM pragma_table_info(?)")
                .bind(table)
                .fetch_all(&mut *connection)
                .await?
                .iter()
                .map(|row| row.get(0))
                .collect();
            for column in &columns {
                let quoted_table = quote(table);
                let quoted_column = quote(column);
                let filter = needles
                    .iter()
                    .map(|_| format!("instr({quoted_column}, ?) > 0"))
                    .collect::<Vec<_>>()
                    .join(" OR ");
                let select = format!(
                    "SELECT DISTINCT {quoted_column} FROM {quoted_table} WHERE typeof({quoted_column}) = 'text' AND ({filter})"
                );
                let mut statement = sqlx::query::query(&select);
                for needle in &needles {
                    statement = statement.bind(needle.clone());
                }
                let values: Vec<String> = statement
                    .fetch_all(&mut *connection)
                    .await?
                    .iter()
                    .map(|row| row.get(0))
                    .collect();
                for value in values {
                    if let Some(rewritten) = self.apply(&value) {
                        changed += sqlx::query::query(&format!(
                            "UPDATE {quoted_table} SET {quoted_column} = ? WHERE {quoted_column} = ?"
                        ))
                        .bind(rewritten)
                        .bind(value)
                        .execute(&mut *connection)
                        .await?
                        .rows_affected();
                    }
                }
                if table == "ItemTable" && column == "value" {
                    changed += self
                        .rewrite_utf16_values(connection, &quoted_table, &quoted_column)
                        .await?;
                }
            }
        }
        Ok(changed)
    }

    /// WebKit stores localStorage values as UTF-16 little-endian blobs.
    async fn rewrite_utf16_values(
        &self,
        connection: &mut SqliteConnection,
        table: &str,
        column: &str,
    ) -> Result<u64, sqlx::Error> {
        let values: Vec<Vec<u8>> = sqlx::query::query(&format!(
            "SELECT DISTINCT {column} FROM {table} WHERE typeof({column}) = 'blob'"
        ))
        .fetch_all(&mut *connection)
        .await?
        .iter()
        .map(|row| row.get(0))
        .collect();
        let mut changed = 0;
        for value in values {
            if value.len() % 2 != 0 {
                continue;
            }
            let units: Vec<u16> = value
                .chunks_exact(2)
                .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                .collect();
            let Ok(text) = String::from_utf16(&units) else {
                continue;
            };
            if let Some(rewritten) = self.apply(&text) {
                let encoded: Vec<u8> = rewritten
                    .encode_utf16()
                    .flat_map(u16::to_le_bytes)
                    .collect();
                changed += sqlx::query::query(&format!(
                    "UPDATE {table} SET {column} = ? WHERE {column} = ?"
                ))
                .bind(encoded)
                .bind(value)
                .execute(&mut *connection)
                .await?
                .rows_affected();
            }
        }
        Ok(changed)
    }

    /// Rewrites a small UTF-8 file in place; binary and large files are left
    /// as they are.
    pub(super) fn apply_to_text_file(&self, path: &Path) -> Result<bool, MigrationError> {
        let metadata =
            std::fs::metadata(path).map_err(|e| fail(format!("reading {}", path.display()), e))?;
        if metadata.len() > MAX_TEXT_FILE_BYTES || is_plain_sqlite(path) {
            return Ok(false);
        }
        let bytes =
            std::fs::read(path).map_err(|e| fail(format!("reading {}", path.display()), e))?;
        let Ok(text) = std::str::from_utf8(&bytes) else {
            return Ok(false);
        };
        let Some(rewritten) = self.apply(text) else {
            return Ok(false);
        };
        std::fs::write(path, rewritten)
            .map_err(|e| fail(format!("writing {}", path.display()), e))?;
        Ok(true)
    }
}

fn quote(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

/// A path can start here: at the beginning, or after whitespace, a quote or
/// punctuation, never in the middle of another path or word.
fn starts_a_path(previous: Option<char>) -> bool {
    match previous {
        None => true,
        Some(character) => character.is_whitespace() || "\"'`([{<=:,;|>".contains(character),
    }
}

/// The root ends the path or continues into it with a separator.
fn ends_a_root(rest: &str) -> bool {
    match rest.chars().next() {
        None => true,
        Some('/') => true,
        Some('\\') => cfg!(windows),
        Some(character) => !(character.is_alphanumeric() || "._-~".contains(character)),
    }
}
