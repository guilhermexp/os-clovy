//! Claude Code: `~/.claude/projects/<project>/<session-uuid>.jsonl`, one JSON
//! record per line. Tool results are logged as `type: "user"` records whose
//! content has only `tool_result` blocks; they are not prompts. Sidechain
//! (subagent) and meta records carry only their timestamp.

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::{file_stem, list_dir, read_json_lines, str_at, SourceRoots};
use crate::coding_agents::record::{
    cap, parse_time, Record, RecordKind, Session, TOOL_INPUT_CAP, TOOL_TEXT_CAP,
};
use crate::coding_agents::SourceId;

pub fn root(roots: &SourceRoots) -> PathBuf {
    roots.home.join(".claude").join("projects")
}

/// Top-level session files of every project (subagent files live one level
/// deeper and are part of their parent session's work).
pub fn discover(roots: &SourceRoots) -> Vec<PathBuf> {
    list_dir(&root(roots))
        .into_iter()
        .filter(|project| project.is_dir())
        .flat_map(|project| list_dir(&project))
        .filter(|file| file.extension().is_some_and(|ext| ext == "jsonl"))
        .collect()
}

pub fn load(path: &Path) -> Option<Session> {
    let values = read_json_lines(path);
    Some(normalize(file_stem(path)?, &values))
}

pub fn normalize(id: String, values: &[Value]) -> Session {
    let records = values.iter().map(record).collect();
    Session {
        source: SourceId::ClaudeCode,
        id,
        title: title(values),
        cwd: None,
        records,
    }
}

/// The last AI title, else the last custom title, else the last summary.
fn title(values: &[Value]) -> Option<String> {
    let pick = |kind: &str, field: &str| {
        values
            .iter()
            .rev()
            .find(|value| str_at(value, &["type"]) == Some(kind))
            .and_then(|value| str_at(value, &[field]))
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_string)
    };
    pick("ai-title", "aiTitle")
        .or_else(|| pick("custom-title", "customTitle"))
        .or_else(|| pick("summary", "summary"))
}

fn record(value: &Value) -> Record {
    let at = str_at(value, &["timestamp"]).and_then(parse_time);
    let cwd = str_at(value, &["cwd"]).map(str::to_string);
    let kind = str_at(value, &["type"]);
    let flag = |name: &str| value.get(name).and_then(Value::as_bool).unwrap_or(false);
    if flag("isSidechain") || flag("isMeta") || !matches!(kind, Some("user" | "assistant")) {
        return Record::event(at).with_cwd(cwd);
    }
    let content = value
        .get("message")
        .and_then(|message| message.get("content"))
        .unwrap_or(&Value::Null);
    let body = render_content(content);
    let kind = match kind {
        Some("user") if has_prompt_text(content) => RecordKind::Prompt,
        Some("user") => RecordKind::Tool,
        _ => RecordKind::Reply,
    };
    if body.trim().is_empty() {
        return Record::event(at).with_cwd(cwd);
    }
    Record::new(at, kind, body).with_cwd(cwd)
}

fn has_prompt_text(content: &Value) -> bool {
    match content {
        Value::String(text) => !text.trim().is_empty(),
        Value::Array(blocks) => blocks.iter().any(|block| {
            str_at(block, &["type"]) == Some("text")
                && str_at(block, &["text"]).is_some_and(|text| !text.trim().is_empty())
        }),
        _ => false,
    }
}

fn render_content(content: &Value) -> String {
    match content {
        Value::String(text) => text.trim().to_string(),
        Value::Array(blocks) => blocks
            .iter()
            .filter_map(render_block)
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn render_block(block: &Value) -> Option<String> {
    match str_at(block, &["type"])? {
        "text" => str_at(block, &["text"])
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_string),
        "tool_use" => {
            let name = str_at(block, &["name"]).unwrap_or("?");
            let input = block
                .get("input")
                .map(|input| cap(&input.to_string(), TOOL_INPUT_CAP))
                .unwrap_or_default();
            Some(format!("[tool_use: {name} {input}]"))
        }
        "tool_result" => {
            let text = match block.get("content") {
                Some(Value::String(text)) => text.clone(),
                Some(Value::Array(parts)) => parts
                    .iter()
                    .filter_map(|part| str_at(part, &["text"]))
                    .collect::<Vec<_>>()
                    .join("\n"),
                _ => String::new(),
            };
            Some(format!("[tool_result: {}]", cap(&text, TOOL_TEXT_CAP)))
        }
        // Thinking and images are not part of the work log.
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tool_results_logged_as_user_are_not_prompts() {
        let session = normalize(
            "s1".into(),
            &[
                json!({"type": "user", "timestamp": "2026-10-04T10:00:00Z", "cwd": "/repo",
                       "message": {"role": "user", "content": "fix the login bug"}}),
                json!({"type": "assistant", "timestamp": "2026-10-04T10:00:05Z",
                       "message": {"role": "assistant", "content": [
                           {"type": "thinking", "thinking": "hmm"},
                           {"type": "text", "text": "Reading the file."},
                           {"type": "tool_use", "name": "Read", "input": {"path": "a.rs"}}]}}),
                json!({"type": "user", "timestamp": "2026-10-04T10:00:06Z",
                       "message": {"role": "user", "content": [
                           {"type": "tool_result", "content": "fn main() {}"}]}}),
                json!({"type": "user", "isSidechain": true, "timestamp": "2026-10-04T10:00:07Z",
                       "message": {"role": "user", "content": "subagent prompt"}}),
                json!({"type": "ai-title", "aiTitle": "Fix login"}),
            ],
        );
        let kinds: Vec<RecordKind> = session.records.iter().map(|r| r.kind).collect();
        assert_eq!(
            kinds,
            vec![
                RecordKind::Prompt,
                RecordKind::Reply,
                RecordKind::Tool,
                RecordKind::Event,
                RecordKind::Event
            ]
        );
        assert_eq!(
            session.records[1].body,
            "Reading the file.\n[tool_use: Read {\"path\":\"a.rs\"}]"
        );
        assert_eq!(session.records[2].body, "[tool_result: fn main() {}]");
        assert_eq!(session.records[0].cwd.as_deref(), Some("/repo"));
        assert_eq!(session.title.as_deref(), Some("Fix login"));
    }
}
