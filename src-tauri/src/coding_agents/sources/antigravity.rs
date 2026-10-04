//! Antigravity (CLI `agy` and the editor): each conversation keeps a JSONL
//! transcript at `~/.gemini/antigravity-cli/brain/<conversation>/
//! .system_generated/logs/transcript.jsonl` (the editor uses
//! `~/.gemini/antigravity/brain/`). Steps carry `created_at` (ISO), `source`
//! (`USER_EXPLICIT`, `MODEL`, `SYSTEM`), `type`, and `content`. A user step's
//! text is wrapped in `<USER_REQUEST>`, followed by `<ADDITIONAL_METADATA>`.
//! The transcript keeps no working directory.

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::{list_dir, read_json_lines, str_at, SourceRoots};
use crate::coding_agents::record::{
    cap, inner_tag, parse_time, Record, RecordKind, Session, TOOL_TEXT_CAP,
};
use crate::coding_agents::SourceId;

pub fn cli_root(roots: &SourceRoots) -> PathBuf {
    roots.home.join(".gemini").join("antigravity-cli")
}

pub fn ide_root(roots: &SourceRoots) -> PathBuf {
    roots.home.join(".gemini").join("antigravity")
}

pub fn discover(roots: &SourceRoots) -> Vec<PathBuf> {
    [cli_root(roots), ide_root(roots)]
        .iter()
        .flat_map(|root| list_dir(&root.join("brain")))
        .map(|conversation| {
            conversation
                .join(".system_generated")
                .join("logs")
                .join("transcript.jsonl")
        })
        .filter(|file| file.is_file())
        .collect()
}

pub fn load(path: &Path) -> Option<Session> {
    let id = path
        .ancestors()
        .nth(3)?
        .file_name()?
        .to_string_lossy()
        .into_owned();
    Some(normalize(id, &read_json_lines(path)))
}

pub fn normalize(id: String, values: &[Value]) -> Session {
    Session {
        source: SourceId::Antigravity,
        id,
        title: None,
        cwd: None,
        records: values.iter().map(record).collect(),
    }
}

fn record(value: &Value) -> Record {
    let at = str_at(value, &["created_at"]).and_then(parse_time);
    let content = str_at(value, &["content"]).unwrap_or_default().trim();
    match (str_at(value, &["source"]), str_at(value, &["type"])) {
        (Some("USER_EXPLICIT"), Some("USER_INPUT")) => {
            let prompt = inner_tag(content, "USER_REQUEST").unwrap_or(content);
            if prompt.is_empty() {
                Record::event(at)
            } else {
                Record::new(at, RecordKind::Prompt, prompt)
            }
        }
        (Some("MODEL"), Some("PLANNER_RESPONSE")) => {
            let mut parts: Vec<String> = value
                .get("tool_calls")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|call| str_at(call, &["name"]))
                .map(|name| format!("[tool_use: {name}]"))
                .collect();
            if !content.is_empty() {
                parts.push(content.to_string());
            }
            if parts.is_empty() {
                Record::event(at)
            } else {
                Record::new(at, RecordKind::Reply, parts.join("\n"))
            }
        }
        (Some("MODEL"), Some(step)) if !content.is_empty() => Record::new(
            at,
            RecordKind::Tool,
            format!("[{step}: {}]", cap(content, TOOL_TEXT_CAP)),
        ),
        _ => Record::event(at),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn unwraps_user_requests_and_classifies_steps() {
        let session = normalize(
            "conv".into(),
            &[
                json!({"step_index": 0, "source": "USER_EXPLICIT", "type": "USER_INPUT",
                       "created_at": "2026-10-04T12:27:18Z",
                       "content": "<USER_REQUEST>\nAdd a health check\n</USER_REQUEST>\n<ADDITIONAL_METADATA>\nlocal time\n</ADDITIONAL_METADATA>"}),
                json!({"step_index": 1, "source": "SYSTEM", "type": "CONVERSATION_HISTORY"}),
                json!({"step_index": 2, "source": "MODEL", "type": "PLANNER_RESPONSE",
                       "created_at": "2026-10-04T12:27:20Z", "tool_calls": [{"name": "run_command"}]}),
                json!({"step_index": 3, "source": "MODEL", "type": "RUN_COMMAND",
                       "created_at": "2026-10-04T12:27:25Z", "content": "ok"}),
                json!({"step_index": 4, "source": "MODEL", "type": "PLANNER_RESPONSE",
                       "created_at": "2026-10-04T12:27:30Z", "content": "Added /health."}),
            ],
        );
        let turns: Vec<(RecordKind, &str)> = session
            .records
            .iter()
            .map(|r| (r.kind, r.body.as_str()))
            .collect();
        assert_eq!(
            turns,
            vec![
                (RecordKind::Prompt, "Add a health check"),
                (RecordKind::Event, ""),
                (RecordKind::Reply, "[tool_use: run_command]"),
                (RecordKind::Tool, "[RUN_COMMAND: ok]"),
                (RecordKind::Reply, "Added /health."),
            ]
        );
    }
}
