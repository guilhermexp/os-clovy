//! Cursor (the editor): every conversation lives in
//! `~/Library/Application Support/Cursor/User/globalStorage/state.vscdb`,
//! table `cursorDiskKV`. `composerData:<composer>` holds the conversation
//! (`fullConversationHeadersOnly` is the authoritative turn order; key order
//! is not), and `bubbleId:<composer>:<bubble>` each message (`type` 1 = user,
//! 2 = assistant, `createdAt` ISO time). The store keeps no working directory.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::query::query;

use super::sqlite::{text_column, value_for_key, Snapshot};
use super::{str_at, SourceRoots};
use crate::coding_agents::record::{
    cap, from_epoch_millis, parse_time, Record, RecordKind, Session, TOOL_INPUT_CAP, TOOL_TEXT_CAP,
};
use crate::coding_agents::SourceId;

const BUBBLE_SQL: &str = "SELECT value FROM cursorDiskKV WHERE key = ?";

pub fn database(roots: &SourceRoots) -> PathBuf {
    roots
        .app_support("Cursor")
        .join("User")
        .join("globalStorage")
        .join("state.vscdb")
}

/// Conversations updated at or after `since`.
pub async fn load(path: &Path, since: DateTime<Utc>) -> Result<Vec<Session>, String> {
    let snapshot = Snapshot::open(path).await?;
    let result = read(&snapshot, since).await;
    snapshot.close().await;
    result
}

async fn read(snapshot: &Snapshot, since: DateTime<Utc>) -> Result<Vec<Session>, String> {
    let pool = snapshot.pool();
    let composers = query("SELECT key, value FROM cursorDiskKV WHERE key LIKE 'composerData:%'")
        .fetch_all(pool)
        .await
        .map_err(|error| error.to_string())?;
    let mut sessions = Vec::new();
    for row in composers {
        let (Some(key), Some(raw)) = (text_column(&row, 0), text_column(&row, 1)) else {
            continue;
        };
        let Ok(composer) = serde_json::from_str::<Value>(&raw) else {
            continue;
        };
        let updated = ["lastUpdatedAt", "createdAt"]
            .iter()
            .find_map(|field| composer.get(field).and_then(Value::as_f64))
            .and_then(from_epoch_millis);
        if !updated.is_some_and(|updated| updated >= since) {
            continue;
        }
        let id = key.trim_start_matches("composerData:").to_string();
        let mut bubbles = Vec::new();
        for header in composer
            .get("fullConversationHeadersOnly")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(bubble_id) = str_at(header, &["bubbleId"]) else {
                continue;
            };
            let bubble = value_for_key(pool, BUBBLE_SQL, &format!("bubbleId:{id}:{bubble_id}"))
                .await
                .and_then(|raw| serde_json::from_str::<Value>(&raw).ok());
            if let Some(bubble) = bubble {
                bubbles.push(bubble);
            }
        }
        if bubbles.is_empty() {
            continue;
        }
        sessions.push(normalize(id, &composer, &bubbles));
    }
    Ok(sessions)
}

pub fn normalize(id: String, composer: &Value, bubbles: &[Value]) -> Session {
    Session {
        source: SourceId::Cursor,
        id,
        title: str_at(composer, &["name"])
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_string),
        cwd: None,
        records: bubbles.iter().map(record).collect(),
    }
}

fn record(bubble: &Value) -> Record {
    let at = str_at(bubble, &["createdAt"]).and_then(parse_time);
    let text = str_at(bubble, &["text"]).unwrap_or_default().trim();
    match bubble.get("type").and_then(Value::as_i64) {
        Some(1) if !text.is_empty() => Record::new(at, RecordKind::Prompt, text),
        Some(2) => {
            let mut parts = Vec::new();
            if let Some(tool) = bubble.get("toolFormerData") {
                let name = str_at(tool, &["name"]).unwrap_or("?");
                let args = str_at(tool, &["rawArgs"]).unwrap_or_default();
                parts.push(format!("[tool_use: {name} {}]", cap(args, TOOL_INPUT_CAP)));
                if let Some(result) = str_at(tool, &["result"]).filter(|r| !r.trim().is_empty()) {
                    parts.push(format!("[tool_result: {}]", cap(result, TOOL_TEXT_CAP)));
                }
            }
            if !text.is_empty() {
                parts.push(text.to_string());
            }
            if parts.is_empty() {
                Record::event(at)
            } else {
                Record::new(at, RecordKind::Reply, parts.join("\n"))
            }
        }
        _ => Record::event(at),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx_sqlite::{SqliteConnectOptions, SqlitePoolOptions};

    async fn fixture(path: &Path, rows: &[(&str, &str)]) {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(
                SqliteConnectOptions::new()
                    .filename(path)
                    .create_if_missing(true),
            )
            .await
            .unwrap();
        query("CREATE TABLE cursorDiskKV (key TEXT UNIQUE ON CONFLICT REPLACE, value BLOB)")
            .execute(&pool)
            .await
            .unwrap();
        for (key, value) in rows {
            query("INSERT INTO cursorDiskKV (key, value) VALUES (?, ?)")
                .bind(*key)
                .bind(value.as_bytes())
                .execute(&pool)
                .await
                .unwrap();
        }
        pool.close().await;
    }

    #[tokio::test]
    async fn reads_conversations_in_header_order_and_skips_old_ones() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.vscdb");
        fixture(
            &path,
            &[
                ("composerData:c1", r#"{"name":"fix the bug","createdAt":1791100800000,"lastUpdatedAt":1791100830000,"fullConversationHeadersOnly":[{"bubbleId":"b1","type":1},{"bubbleId":"b2","type":2}]}"#),
                ("bubbleId:c1:b2", r#"{"type":2,"createdAt":"2026-10-04T08:00:10Z","text":"Fixed.","toolFormerData":{"name":"read_file","rawArgs":"{\"path\":\"auth.ts\"}","result":"{\"lines\":42}"}}"#),
                ("bubbleId:c1:b1", r#"{"type":1,"createdAt":"2026-10-04T08:00:00Z","text":"fix the login bug"}"#),
                ("composerData:old", r#"{"createdAt":1000,"fullConversationHeadersOnly":[{"bubbleId":"x","type":1}]}"#),
                ("bubbleId:old:x", r#"{"type":1,"createdAt":"1970-01-01T00:00:01Z","text":"old"}"#),
            ],
        )
        .await;
        let since = parse_time("2026-10-01T00:00:00Z").unwrap();
        let sessions = load(&path, since).await.unwrap();
        assert_eq!(sessions.len(), 1);
        let session = &sessions[0];
        assert_eq!(session.id, "c1");
        assert_eq!(session.title.as_deref(), Some("fix the bug"));
        assert_eq!(session.records[0].kind, RecordKind::Prompt);
        assert_eq!(
            session.records[1].body,
            "[tool_use: read_file {\"path\":\"auth.ts\"}]\n[tool_result: {\"lines\":42}]\nFixed."
        );
    }
}
