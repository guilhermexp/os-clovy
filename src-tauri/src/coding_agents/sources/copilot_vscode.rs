//! Copilot chat in VS Code: `~/Library/Application Support/Code/User/
//! workspaceStorage/<hash>/chatSessions/<session>.jsonl` (and
//! `globalStorage/emptyWindowChatSessions/` for windows without a folder).
//!
//! The `.jsonl` file is an operation log replayed in order: `kind 0` is a full
//! snapshot, `kind 1` sets the value at key path `k`, `kind 2` appends to the
//! array at `k`. Older builds wrote the snapshot alone as `<session>.json`.
//! The workspace folder comes from the sibling `workspace.json`.

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::{file_stem, list_dir, path_from_file_uri, read_json_lines, str_at, SourceRoots};
use crate::coding_agents::record::{
    cap, from_epoch_millis, Record, RecordKind, Session, TOOL_INPUT_CAP,
};
use crate::coding_agents::SourceId;

pub fn user_dir(roots: &SourceRoots) -> PathBuf {
    roots.app_support("Code").join("User")
}

pub fn discover(roots: &SourceRoots) -> Vec<PathBuf> {
    let user = user_dir(roots);
    let mut dirs: Vec<PathBuf> = list_dir(&user.join("workspaceStorage"))
        .into_iter()
        .map(|workspace| workspace.join("chatSessions"))
        .collect();
    dirs.push(user.join("globalStorage").join("emptyWindowChatSessions"));
    dirs.iter()
        .flat_map(|dir| list_dir(dir))
        .filter(|file| {
            file.extension()
                .is_some_and(|ext| ext == "jsonl" || ext == "json")
        })
        .collect()
}

pub fn load(path: &Path) -> Option<Session> {
    let state = if path.extension().is_some_and(|ext| ext == "json") {
        serde_json::from_slice::<Value>(&std::fs::read(path).ok()?).ok()?
    } else {
        replay(&read_json_lines(path))
    };
    let cwd = path
        .parent()
        .and_then(Path::parent)
        .map(|workspace| workspace.join("workspace.json"))
        .and_then(|file| std::fs::read(file).ok())
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .and_then(|workspace| str_at(&workspace, &["folder"]).and_then(path_from_file_uri));
    Some(normalize(file_stem(path)?, &state, cwd))
}

/// Applies the operation log to an empty session.
pub fn replay(operations: &[Value]) -> Value {
    let mut state = Value::Null;
    for operation in operations {
        let value = operation.get("v").cloned().unwrap_or(Value::Null);
        match operation.get("kind").and_then(Value::as_i64) {
            Some(0) => state = value,
            Some(1) => set(&mut state, operation.get("k"), value),
            Some(2) => {
                let path = operation.get("k").and_then(Value::as_array);
                if let Some(Value::Array(items)) = path.and_then(|path| walk(&mut state, path)) {
                    match value {
                        Value::Array(more) => items.extend(more),
                        other => items.push(other),
                    }
                }
            }
            _ => {}
        }
    }
    state
}

/// The value at `path`; `None` (the operation is dropped) when any step is
/// missing, so a malformed log never invents structure.
fn walk<'a>(state: &'a mut Value, path: &[Value]) -> Option<&'a mut Value> {
    let mut current = state;
    for step in path {
        current = match step {
            Value::String(key) => current.get_mut(key.as_str())?,
            Value::Number(index) => current.get_mut(usize::try_from(index.as_u64()?).ok()?)?,
            _ => return None,
        };
    }
    Some(current)
}

/// Sets the value at `path`. The parent must exist; a new property may be
/// added to an existing object (VS Code sets `modelState` on completion).
fn set(state: &mut Value, path: Option<&Value>, value: Value) {
    let Some((last, parent)) = path
        .and_then(Value::as_array)
        .and_then(|path| path.split_last())
    else {
        return;
    };
    match (walk(state, parent), last) {
        (Some(Value::Object(object)), Value::String(key)) => {
            object.insert(key.clone(), value);
        }
        (Some(Value::Array(items)), Value::Number(index)) => {
            if let Some(slot) = index
                .as_u64()
                .and_then(|index| usize::try_from(index).ok())
                .and_then(|index| items.get_mut(index))
            {
                *slot = value;
            }
        }
        _ => {}
    }
}

pub fn normalize(id: String, state: &Value, cwd: Option<String>) -> Session {
    let mut records = Vec::new();
    for request in state
        .get("requests")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let asked_at = request
            .get("timestamp")
            .and_then(Value::as_f64)
            .and_then(from_epoch_millis);
        let prompt = str_at(request, &["message", "text"])
            .unwrap_or_default()
            .trim();
        if !prompt.is_empty() {
            records.push(Record::new(asked_at, RecordKind::Prompt, prompt).with_cwd(cwd.clone()));
        }
        let answered_at = request
            .get("modelState")
            .and_then(|state| state.get("completedAt"))
            .and_then(Value::as_f64)
            .and_then(from_epoch_millis)
            .or(asked_at);
        let reply = response_text(request.get("response"));
        if !reply.is_empty() {
            records.push(Record::new(answered_at, RecordKind::Reply, reply));
        }
    }
    Session {
        source: SourceId::CopilotVscode,
        id,
        title: str_at(state, &["customTitle"])
            .map(str::trim)
            .filter(|title| !title.is_empty())
            .map(str::to_string),
        cwd,
        records,
    }
}

fn response_text(parts: Option<&Value>) -> String {
    parts
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|part| match str_at(part, &["kind"]) {
            None => str_at(part, &["value"])
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(str::to_string),
            Some("toolInvocationSerialized") => {
                let tool = str_at(part, &["toolId"]).unwrap_or("?");
                let message = str_at(part, &["invocationMessage"])
                    .or_else(|| str_at(part, &["invocationMessage", "value"]))
                    .unwrap_or_default();
                Some(format!(
                    "[tool_use: {tool} {}]",
                    cap(message, TOOL_INPUT_CAP)
                ))
            }
            Some("textEditGroup") => {
                str_at(part, &["uri", "fsPath"]).map(|path| format!("[edit: {path}]"))
            }
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn replays_the_operation_log() {
        let state = replay(&[
            json!({"kind": 0, "v": {"customTitle": "fix the bug", "requests": [
                {"timestamp": 1_791_100_800_000_i64, "message": {"text": "fix the login bug"}, "response": []}]}}),
            json!({"kind": 2, "k": ["requests", 0, "response"], "v": [
                {"kind": "thinking", "value": "null check"},
                {"kind": "toolInvocationSerialized", "toolId": "copilot_readFile", "invocationMessage": "Reading auth.ts"}]}),
            json!({"kind": 2, "k": ["requests", 0, "response"], "v": [
                {"kind": "textEditGroup", "uri": {"fsPath": "/repo/auth.ts"}},
                {"value": "Added a null check."}]}),
            json!({"kind": 1, "k": ["requests", 0, "modelState"], "v": {"completedAt": 1_791_100_830_000_i64}}),
            json!({"kind": 1, "k": ["missing", 3], "v": 1}),
        ]);
        let session = normalize("s1".into(), &state, Some("/repo".into()));
        assert_eq!(session.title.as_deref(), Some("fix the bug"));
        assert_eq!(session.records.len(), 2);
        assert_eq!(session.records[0].kind, RecordKind::Prompt);
        assert_eq!(
            session.records[1].body,
            "[tool_use: copilot_readFile Reading auth.ts]\n[edit: /repo/auth.ts]\nAdded a null check."
        );
        let gap = session.records[1].at.unwrap() - session.records[0].at.unwrap();
        assert_eq!(gap.num_seconds(), 30);
    }
}
