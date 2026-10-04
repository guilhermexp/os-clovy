//! Codex CLI and app: `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`. The
//! working directory comes from `session_meta` / `turn_context`. Turns are
//! read from either generation of the rollout format:
//!
//! - current: `event_msg` `item_completed` with `item.type` `UserMessage` /
//!   `AgentMessage` (tool items such as `CommandExecution` become tool lines);
//! - earlier: `event_msg` `user_message` / `agent_message`.
//!
//! `response_item` messages are skipped: they include the injected
//! instructions and environment context, not what the user typed. When a file
//! has current-format turns, earlier-format turns are ignored so a transitional
//! file never counts a prompt twice.

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::{file_stem, list_dir, read_json_lines, str_at, SourceRoots};
use crate::coding_agents::record::{cap, parse_time, Record, RecordKind, Session, TOOL_INPUT_CAP};
use crate::coding_agents::SourceId;

pub fn root(roots: &SourceRoots) -> PathBuf {
    roots.home.join(".codex").join("sessions")
}

pub fn discover(roots: &SourceRoots) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for year in list_dir(&root(roots)).into_iter().filter(|p| p.is_dir()) {
        for month in list_dir(&year).into_iter().filter(|p| p.is_dir()) {
            for day in list_dir(&month).into_iter().filter(|p| p.is_dir()) {
                files.extend(
                    list_dir(&day)
                        .into_iter()
                        .filter(|file| file.extension().is_some_and(|ext| ext == "jsonl")),
                );
            }
        }
    }
    files
}

pub fn load(path: &Path) -> Option<Session> {
    let values = read_json_lines(path);
    Some(normalize(file_stem(path)?, &values))
}

pub fn normalize(fallback_id: String, values: &[Value]) -> Session {
    let current_format = values.iter().any(|value| {
        item_type(value).is_some_and(|kind| kind == "UserMessage" || kind == "AgentMessage")
    });
    let id = values
        .iter()
        .find(|value| str_at(value, &["type"]) == Some("session_meta"))
        .and_then(|value| str_at(value, &["payload", "id"]))
        .map(str::to_string)
        .unwrap_or(fallback_id);
    let records = values
        .iter()
        .map(|value| record(value, current_format))
        .collect();
    Session {
        source: SourceId::Codex,
        id,
        title: None,
        cwd: None,
        records,
    }
}

fn item_type(value: &Value) -> Option<&str> {
    if str_at(value, &["type"]) != Some("event_msg")
        || str_at(value, &["payload", "type"]) != Some("item_completed")
    {
        return None;
    }
    str_at(value, &["payload", "item", "type"])
}

fn record(value: &Value, current_format: bool) -> Record {
    let at = str_at(value, &["timestamp"]).and_then(parse_time);
    let cwd = match str_at(value, &["type"]) {
        Some("session_meta" | "turn_context") => {
            str_at(value, &["payload", "cwd"]).map(str::to_string)
        }
        _ => None,
    };
    let event = || Record::event(at).with_cwd(cwd.clone());
    if str_at(value, &["type"]) != Some("event_msg") {
        return event();
    }
    let payload = &value["payload"];
    let (kind, body) = match str_at(payload, &["type"]) {
        Some("item_completed") => match item(&payload["item"]) {
            Some(turn) => turn,
            None => return event(),
        },
        Some("user_message") if !current_format => {
            (RecordKind::Prompt, message_text(&payload["message"]))
        }
        Some("agent_message") if !current_format => {
            (RecordKind::Reply, message_text(&payload["message"]))
        }
        _ => return event(),
    };
    if body.trim().is_empty() {
        return event();
    }
    Record::new(at, kind, body).with_cwd(cwd)
}

fn item(item: &Value) -> Option<(RecordKind, String)> {
    match str_at(item, &["type"])? {
        "UserMessage" => Some((RecordKind::Prompt, content_text(&item["content"]))),
        "AgentMessage" => Some((RecordKind::Reply, content_text(&item["content"]))),
        "Reasoning" => None,
        other => {
            let name = ["tool", "name", "command", "query"]
                .iter()
                .find_map(|key| match item.get(key) {
                    Some(Value::String(text)) => Some(text.clone()),
                    Some(Value::Array(parts)) => Some(
                        parts
                            .iter()
                            .filter_map(Value::as_str)
                            .collect::<Vec<_>>()
                            .join(" "),
                    ),
                    _ => None,
                })
                .unwrap_or_default();
            Some((
                RecordKind::Tool,
                format!("[tool: {other} {}]", cap(&name, TOOL_INPUT_CAP)),
            ))
        }
    }
}

fn content_text(content: &Value) -> String {
    match content {
        Value::String(text) => text.trim().to_string(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|part| str_at(part, &["text"]))
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn message_text(message: &Value) -> String {
    match message {
        Value::Array(parts) => parts
            .iter()
            .filter_map(|part| match part {
                Value::String(text) => Some(text.as_str()),
                other => str_at(other, &["text"]).or_else(|| str_at(other, &["content"])),
            })
            .collect::<Vec<_>>()
            .join("\n"),
        other => content_text(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn current_format_turns_and_cwd() {
        let session = normalize(
            "rollout-x".into(),
            &[
                json!({"timestamp": "2026-10-04T10:00:00Z", "type": "session_meta",
                       "payload": {"id": "abc", "cwd": "/repo"}}),
                json!({"timestamp": "2026-10-04T10:00:01Z", "type": "response_item",
                       "payload": {"type": "message", "role": "user",
                                   "content": [{"type": "input_text", "text": "<environment_context>"}]}}),
                json!({"timestamp": "2026-10-04T10:00:02Z", "type": "event_msg",
                       "payload": {"type": "item_completed",
                                   "item": {"type": "UserMessage", "content": [{"type": "text", "text": "add tests"}]}}}),
                json!({"timestamp": "2026-10-04T10:00:03Z", "type": "event_msg",
                       "payload": {"type": "item_completed",
                                   "item": {"type": "CommandExecution", "command": "cargo test"}}}),
                json!({"timestamp": "2026-10-04T10:00:04Z", "type": "event_msg",
                       "payload": {"type": "item_completed",
                                   "item": {"type": "AgentMessage", "content": [{"type": "Text", "text": "Done."}]}}}),
                json!({"timestamp": "2026-10-04T10:00:05Z", "type": "event_msg",
                       "payload": {"type": "user_message", "message": "duplicate"}}),
            ],
        );
        assert_eq!(session.id, "abc");
        let turns: Vec<(RecordKind, &str)> = session
            .records
            .iter()
            .filter(|r| r.is_turn())
            .map(|r| (r.kind, r.body.as_str()))
            .collect();
        assert_eq!(
            turns,
            vec![
                (RecordKind::Prompt, "add tests"),
                (RecordKind::Tool, "[tool: CommandExecution cargo test]"),
                (RecordKind::Reply, "Done."),
            ]
        );
        assert_eq!(session.records[0].cwd.as_deref(), Some("/repo"));
    }

    #[test]
    fn earlier_format_turns() {
        let session = normalize(
            "rollout-y".into(),
            &[
                json!({"timestamp": "2026-10-04T10:00:00Z", "type": "turn_context",
                       "payload": {"cwd": "/repo"}}),
                json!({"timestamp": "2026-10-04T10:00:02Z", "type": "event_msg",
                       "payload": {"type": "user_message", "message": "explain"}}),
                json!({"timestamp": "2026-10-04T10:00:04Z", "type": "event_msg",
                       "payload": {"type": "agent_message", "message": "It works."}}),
            ],
        );
        assert_eq!(session.id, "rollout-y");
        let kinds: Vec<RecordKind> = session.records.iter().map(|r| r.kind).collect();
        assert_eq!(
            kinds,
            vec![RecordKind::Event, RecordKind::Prompt, RecordKind::Reply]
        );
    }
}
