//! Copilot CLI: `~/.copilot/session-state/<session-uuid>/events.jsonl`, an
//! append-only event log. `user.message` `data.content` is what the user
//! typed (`transformedContent` adds Copilot's own framing and is ignored);
//! `session.shutdown` / `session.end` mark an exit.

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::{list_dir, read_json_lines, str_at, SourceRoots};
use crate::coding_agents::record::{cap, parse_time, Record, RecordKind, Session, TOOL_INPUT_CAP};
use crate::coding_agents::SourceId;

pub fn root(roots: &SourceRoots) -> PathBuf {
    roots.home.join(".copilot").join("session-state")
}

pub fn discover(roots: &SourceRoots) -> Vec<PathBuf> {
    list_dir(&root(roots))
        .into_iter()
        .map(|dir| dir.join("events.jsonl"))
        .filter(|file| file.is_file())
        .collect()
}

pub fn load(path: &Path) -> Option<Session> {
    let id = path.parent()?.file_name()?.to_string_lossy().into_owned();
    Some(normalize(id, &read_json_lines(path)))
}

pub fn normalize(id: String, values: &[Value]) -> Session {
    Session {
        source: SourceId::CopilotCli,
        id,
        title: None,
        cwd: None,
        records: values.iter().map(record).collect(),
    }
}

fn record(value: &Value) -> Record {
    let at = str_at(value, &["timestamp"]).and_then(parse_time);
    let text = || {
        str_at(value, &["data", "content"])
            .map(str::trim)
            .unwrap_or_default()
            .to_string()
    };
    match str_at(value, &["type"]) {
        Some("session.start") => Record::event(at)
            .with_cwd(str_at(value, &["data", "context", "cwd"]).map(str::to_string)),
        Some("user.message") if !text().is_empty() => Record::new(at, RecordKind::Prompt, text()),
        Some("assistant.message") if !text().is_empty() => {
            Record::new(at, RecordKind::Reply, text())
        }
        Some("tool.execution_start") => {
            let name = str_at(value, &["data", "toolName"]).unwrap_or("?");
            let arguments = value["data"]
                .get("arguments")
                .map(|args| cap(&args.to_string(), TOOL_INPUT_CAP))
                .unwrap_or_default();
            Record::new(
                at,
                RecordKind::Tool,
                format!("[tool_use: {name} {arguments}]"),
            )
        }
        Some("session.shutdown" | "session.end") => Record::new(at, RecordKind::End, ""),
        _ => Record::event(at),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn uses_typed_content_and_marks_shutdown() {
        let session = normalize(
            "u1".into(),
            &[
                json!({"type": "session.start", "timestamp": "2026-10-04T06:14:06.353Z",
                       "data": {"sessionId": "u1", "context": {"cwd": "/Users/dev/repo"}}}),
                json!({"type": "user.message", "timestamp": "2026-10-04T06:14:15.323Z",
                       "data": {"content": "what is an editorial?",
                                "transformedContent": "<current_datetime>x</current_datetime> what is an editorial?"}}),
                json!({"type": "assistant.turn_start", "timestamp": "2026-10-04T06:14:15.500Z", "data": {}}),
                json!({"type": "assistant.message", "timestamp": "2026-10-04T06:14:20Z",
                       "data": {"content": "An opinion piece."}}),
                json!({"type": "session.shutdown", "timestamp": "2026-10-04T06:15:00Z", "data": {}}),
            ],
        );
        assert_eq!(session.records[0].cwd.as_deref(), Some("/Users/dev/repo"));
        assert_eq!(session.records[1].kind, RecordKind::Prompt);
        assert_eq!(session.records[1].body, "what is an editorial?");
        assert_eq!(session.records[3].kind, RecordKind::Reply);
        assert_eq!(session.records[4].kind, RecordKind::End);
    }
}
