//! Catalog of the six agent CLIs and their one-shot invocation contracts.
//!
//! Each one-shot call is isolated as far as the CLI allows: no session
//! persisted, no tools, no project context files, skills, or slash commands,
//! an empty scratch working directory, and a timeout enforced by
//! [`super::process`]. The prompt always starts with
//! [`super::CLOVY_AUTHORSHIP_MARKER`] so session ingestion can skip Clovy's own
//! calls, and the child gets `CLOVY_ONESHOT=1` for the same purpose.

use super::{
    parse_json_value, process::ProcessOutput, GenerateRequest, LlmError, StructuredOutputLevel,
    CLOVY_AUTHORSHIP_MARKER,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// Prompts above this many characters are elided in the middle before they
/// are passed on argv (agy and copilot read the prompt from argv only).
const ARGV_PROMPT_CAP_CHARS: usize = 180_000;
const ERROR_DETAIL_CHARS: usize = 300;
/// Copilot CLI's built-in tool names (`--excluded-tools`), per GitHub's CLI
/// command reference.
const COPILOT_TOOLS: &[&str] = &[
    "bash",
    "powershell",
    "list_bash",
    "list_powershell",
    "read_bash",
    "read_powershell",
    "stop_bash",
    "stop_powershell",
    "write_bash",
    "write_powershell",
    "apply_patch",
    "create",
    "edit",
    "view",
    "list_agents",
    "read_agent",
    "task",
    "write_agent",
    "ask_user",
    "glob",
    "grep",
    "rg",
    "skill",
    "web_fetch",
];
/// Copilot CLI permission kinds (`--deny-tool`); denies win over allows.
const COPILOT_PERMISSION_KINDS: &[&str] = &["shell", "write", "read", "url", "memory"];

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CliKind {
    #[serde(rename = "claude")]
    Claude,
    #[serde(rename = "codex")]
    Codex,
    #[serde(rename = "pi")]
    Pi,
    #[serde(rename = "agy")]
    Agy,
    #[serde(rename = "cursor-agent")]
    CursorAgent,
    #[serde(rename = "copilot")]
    Copilot,
}

impl CliKind {
    pub const ALL: [CliKind; 6] = [
        CliKind::Claude,
        CliKind::Codex,
        CliKind::Pi,
        CliKind::Agy,
        CliKind::CursorAgent,
        CliKind::Copilot,
    ];

    /// Stable id, also the executable name looked up on PATH.
    pub fn id(self) -> &'static str {
        match self {
            CliKind::Claude => "claude",
            CliKind::Codex => "codex",
            CliKind::Pi => "pi",
            CliKind::Agy => "agy",
            CliKind::CursorAgent => "cursor-agent",
            CliKind::Copilot => "copilot",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            CliKind::Claude => "Claude Code",
            CliKind::Codex => "Codex",
            CliKind::Pi => "Pi",
            CliKind::Agy => "Antigravity",
            CliKind::CursorAgent => "Cursor Agent",
            CliKind::Copilot => "GitHub Copilot",
        }
    }

    /// What the CLI can enforce. Codex validates `--output-schema` strictly;
    /// claude and agy accept `--json-schema`; the rest only follow a schema
    /// written into the prompt.
    pub fn structured_output(self) -> StructuredOutputLevel {
        match self {
            CliKind::Codex => StructuredOutputLevel::Strict,
            CliKind::Claude | CliKind::Agy => StructuredOutputLevel::JsonSchema,
            CliKind::Pi | CliKind::CursorAgent | CliKind::Copilot => StructuredOutputLevel::Prompt,
        }
    }

    pub fn parse(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.id() == id)
    }
}

/// A fully built CLI call, ready for [`super::process::run`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CliInvocation {
    pub args: Vec<String>,
    pub stdin: Option<String>,
    pub set_env: Vec<(String, String)>,
    pub remove_env: Vec<&'static str>,
    /// Codex writes its final message to this file instead of stdout.
    pub output_file: Option<PathBuf>,
}

/// The single prompt text a CLI receives: marker first, then the system
/// instructions, the user content, and (for prompt-level CLIs) the schema.
pub fn compose_prompt(kind: CliKind, request: &GenerateRequest) -> String {
    let mut text = String::from(CLOVY_AUTHORSHIP_MARKER);
    if let Some(system) = request
        .system
        .as_deref()
        .map(str::trim)
        .filter(|system| !system.is_empty())
    {
        text.push('\n');
        text.push_str(system);
    }
    text.push_str("\n\n");
    text.push_str(request.prompt.trim());
    if let Some(schema) = &request.schema {
        if kind.structured_output() < StructuredOutputLevel::JsonSchema {
            text.push_str("\n\n");
            text.push_str(&schema_instruction(schema));
        }
    }
    text
}

pub fn schema_instruction(schema: &Value) -> String {
    format!(
        "Respond with only one JSON object that matches this JSON schema. Do not add prose or code fences.\n{schema}"
    )
}

/// Builds the argv/stdin/env for one isolated call. `workdir` is an empty
/// scratch directory owned by the caller (also used for codex's files).
pub fn build_invocation(
    kind: CliKind,
    request: &GenerateRequest,
    workdir: &Path,
) -> Result<CliInvocation, LlmError> {
    let prompt = compose_prompt(kind, request);
    let schema = request.schema.as_ref();
    let mut set_env = vec![
        ("CLOVY_ONESHOT".to_string(), "1".to_string()),
        ("DO_NOT_TRACK".to_string(), "1".to_string()),
    ];
    let mut remove_env = Vec::new();
    let mut output_file = None;
    let (args, stdin) = match kind {
        CliKind::Claude => {
            let mut args = strings(&[
                "-p",
                "--output-format",
                "json",
                "--no-session-persistence",
                "--strict-mcp-config",
                "--tools",
                "",
                "--setting-sources",
                "",
                "--disable-slash-commands",
            ]);
            if let Some(schema) = schema {
                args.push("--json-schema".to_string());
                args.push(schema.to_string());
            }
            set_env.push((
                "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC".to_string(),
                "1".to_string(),
            ));
            // An API key in the profile would silently switch the user from
            // their subscription to metered API billing.
            remove_env.push("ANTHROPIC_API_KEY");
            (args, Some(prompt))
        }
        CliKind::Codex => {
            let last_message = workdir.join("last_message.txt");
            let mut args = strings(&[
                "exec",
                "-s",
                "read-only",
                "--skip-git-repo-check",
                "--ephemeral",
                "--color",
                "never",
                "-c",
                "analytics.enabled=false",
            ]);
            args.push("-o".to_string());
            args.push(last_message.to_string_lossy().into_owned());
            args.push("-C".to_string());
            args.push(workdir.to_string_lossy().into_owned());
            if let Some(schema) = schema {
                let schema_path = workdir.join("schema.json");
                std::fs::write(&schema_path, strictify(schema).to_string()).map_err(|error| {
                    LlmError::CliFailed {
                        cli: kind,
                        detail: format!("could not write the output schema: {error}"),
                    }
                })?;
                args.push("--output-schema".to_string());
                args.push(schema_path.to_string_lossy().into_owned());
            }
            output_file = Some(last_message);
            (args, Some(prompt))
        }
        CliKind::Pi => {
            set_env.push(("PI_TELEMETRY".to_string(), "0".to_string()));
            (
                strings(&[
                    "--print",
                    "--mode",
                    "json",
                    "--no-session",
                    "--no-tools",
                    "--no-approve",
                    "--no-context-files",
                    "--no-skills",
                    "--no-prompt-templates",
                ]),
                Some(prompt),
            )
        }
        CliKind::Agy => {
            let mut args = vec![
                "-p".to_string(),
                cap_middle(&prompt, ARGV_PROMPT_CAP_CHARS),
                "--output-format".to_string(),
                "json".to_string(),
                "--disable-slash-commands".to_string(),
            ];
            if let Some(schema) = schema {
                args.push("--json-schema".to_string());
                args.push(schema.to_string());
            }
            (args, None)
        }
        CliKind::CursorAgent => {
            remove_env.push("CURSOR_API_KEY");
            let mut args = strings(&["-p", "--output-format", "json", "--mode", "ask", "--trust"]);
            args.push("--workspace".to_string());
            args.push(workdir.to_string_lossy().into_owned());
            (args, Some(prompt))
        }
        CliKind::Copilot => {
            let mut args = vec![
                "-p".to_string(),
                cap_middle(&prompt, ARGV_PROMPT_CAP_CHARS),
                "-s".to_string(),
                "--no-color".to_string(),
                "--log-level".to_string(),
                "none".to_string(),
                "--no-custom-instructions".to_string(),
                "--no-ask-user".to_string(),
                "--disable-builtin-mcps".to_string(),
            ];
            // Hide every built-in tool from the model, and deny every
            // permission kind: deny rules win over any permission the user
            // persisted for interactive use (even `--allow-all`).
            args.push(format!("--excluded-tools={}", COPILOT_TOOLS.join(",")));
            args.push(format!(
                "--deny-tool={}",
                COPILOT_PERMISSION_KINDS.join(",")
            ));
            (args, None)
        }
    };
    Ok(CliInvocation {
        args,
        stdin,
        set_env,
        remove_env,
        output_file,
    })
}

/// Extracts the model's answer from a finished CLI call. `output_file` is the
/// content of [`CliInvocation::output_file`] when the CLI uses one.
pub fn parse_output(
    kind: CliKind,
    output: &ProcessOutput,
    output_file: Option<&str>,
    wants_json: bool,
) -> Result<String, LlmError> {
    let failed = |detail: String| LlmError::CliFailed {
        cli: kind,
        detail: truncate(&detail, ERROR_DETAIL_CHARS),
    };
    let text = match kind {
        CliKind::Claude | CliKind::CursorAgent => {
            let envelope = parse_envelope(&output.stdout)
                .ok_or_else(|| failed(first_line_or(&output.stderr, &output.stdout)))?;
            let is_error = envelope
                .get("is_error")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let subtype = envelope.get("subtype").and_then(Value::as_str);
            if is_error || !matches!(subtype, None | Some("success")) {
                return Err(failed(
                    envelope
                        .get("result")
                        .and_then(Value::as_str)
                        .or(subtype)
                        .unwrap_or("error")
                        .to_string(),
                ));
            }
            match envelope
                .get("structured_output")
                .filter(|value| !value.is_null())
            {
                Some(structured) if wants_json => structured.to_string(),
                _ => envelope
                    .get("result")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            }
        }
        CliKind::Agy => {
            let envelope = parse_envelope(&output.stdout)
                .ok_or_else(|| failed(first_line_or(&output.stderr, &output.stdout)))?;
            let status = envelope.get("status").and_then(Value::as_str);
            if status.is_some_and(|status| status != "SUCCESS") {
                return Err(failed(
                    envelope
                        .get("error")
                        .or_else(|| envelope.get("message"))
                        .and_then(Value::as_str)
                        .unwrap_or("error")
                        .to_string(),
                ));
            }
            match envelope
                .get("structured_output")
                .filter(|value| !value.is_null())
            {
                Some(Value::String(text)) => text.clone(),
                Some(structured) => structured.to_string(),
                None => envelope
                    .get("response")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            }
        }
        CliKind::Codex => {
            if !output.success {
                return Err(failed(codex_error(&output.stderr)));
            }
            output_file.unwrap_or_default().to_string()
        }
        CliKind::Pi => parse_pi_stream(&output.stdout).map_err(failed)?,
        CliKind::Copilot => {
            if !output.success {
                return Err(failed(first_line_or(&output.stderr, &output.stdout)));
            }
            output.stdout.clone()
        }
    };
    if !output.success
        && !matches!(kind, CliKind::Codex | CliKind::Copilot)
        && text.trim().is_empty()
    {
        return Err(failed(first_line_or(&output.stderr, &output.stdout)));
    }
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err(LlmError::InvalidOutput(format!(
            "{} returned an empty answer.",
            kind.display_name()
        )));
    }
    if wants_json && parse_json_value(&text).is_none() {
        return Err(LlmError::InvalidOutput(format!(
            "{} did not return valid JSON.",
            kind.display_name()
        )));
    }
    Ok(text)
}

fn parse_envelope(stdout: &str) -> Option<Value> {
    let trimmed = stdout.trim();
    serde_json::from_str::<Value>(trimmed)
        .ok()
        .or_else(|| {
            // Some CLIs print a warning line before the JSON envelope.
            trimmed
                .lines()
                .rev()
                .find_map(|line| serde_json::from_str::<Value>(line.trim()).ok())
        })
        .filter(Value::is_object)
}

/// Reads pi's `--mode json` event stream: text comes from the assistant's
/// `message_end`, and the run must reach `agent_end` without an error event.
fn parse_pi_stream(stdout: &str) -> Result<String, String> {
    let mut text = String::new();
    let mut saw_agent_end = false;
    for line in stdout.lines() {
        let Ok(event) = serde_json::from_str::<Value>(line.trim()) else {
            continue;
        };
        match event.get("type").and_then(Value::as_str) {
            Some("error") => {
                return Err(["error", "message", "errorMessage"]
                    .iter()
                    .find_map(|key| event.get(*key).and_then(Value::as_str))
                    .unwrap_or("pi reported an error")
                    .to_string());
            }
            Some("message_end") => {
                let Some(message) = event.get("message") else {
                    continue;
                };
                if message.get("role").and_then(Value::as_str) != Some("assistant") {
                    continue;
                }
                if let Some(error) = message.get("errorMessage").and_then(Value::as_str) {
                    return Err(error.to_string());
                }
                if let Some(reason) = message.get("stopReason").and_then(Value::as_str) {
                    if reason != "stop" {
                        return Err(format!("pi stopped early ({reason})"));
                    }
                }
                text = match message.get("content") {
                    Some(Value::String(content)) => content.clone(),
                    Some(Value::Array(blocks)) => blocks
                        .iter()
                        .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
                        .filter_map(|block| block.get("text").and_then(Value::as_str))
                        .collect::<Vec<_>>()
                        .join(""),
                    _ => String::new(),
                };
            }
            Some("agent_end") => saw_agent_end = true,
            _ => {}
        }
    }
    if !saw_agent_end {
        return Err("pi ended without completing the answer".to_string());
    }
    Ok(text)
}

fn codex_error(stderr: &str) -> String {
    if let Some((_, after)) = stderr.split_once("ERROR:") {
        let after = after.trim();
        if let Ok(value) = serde_json::from_str::<Value>(after) {
            if let Some(message) = value
                .pointer("/error/message")
                .or_else(|| value.pointer("/message"))
                .and_then(Value::as_str)
            {
                return message.to_string();
            }
        }
        return after.lines().next().unwrap_or(after).to_string();
    }
    first_line_or(stderr, "")
}

/// Adapts a schema to OpenAI's strict dialect: every object closes
/// `additionalProperties` and lists all of its properties as required.
pub fn strictify(schema: &Value) -> Value {
    match schema {
        Value::Object(map) => {
            let mut out = serde_json::Map::new();
            for (key, value) in map {
                out.insert(key.clone(), strictify(value));
            }
            let is_object = out.get("type").and_then(Value::as_str) == Some("object")
                || out.contains_key("properties");
            if is_object {
                let keys: Vec<Value> = out
                    .get("properties")
                    .and_then(Value::as_object)
                    .map(|properties| properties.keys().cloned().map(Value::String).collect())
                    .unwrap_or_default();
                out.insert("required".to_string(), Value::Array(keys));
                out.insert("additionalProperties".to_string(), Value::Bool(false));
            }
            Value::Object(out)
        }
        Value::Array(items) => Value::Array(items.iter().map(strictify).collect()),
        other => other.clone(),
    }
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}

fn first_line_or(primary: &str, secondary: &str) -> String {
    primary
        .lines()
        .chain(secondary.lines())
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("The CLI failed without output.")
        .to_string()
}

fn truncate(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

/// Keeps the head (70%) and tail (30%) of an oversized prompt.
fn cap_middle(value: &str, max_chars: usize) -> String {
    let count = value.chars().count();
    if count <= max_chars {
        return value.to_string();
    }
    let head = max_chars * 7 / 10;
    let tail = max_chars - head;
    let head_text: String = value.chars().take(head).collect();
    let tail_text: String = value.chars().skip(count - tail).collect();
    format!("{head_text}\n[...]\n{tail_text}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn request(schema: Option<Value>) -> GenerateRequest {
        GenerateRequest {
            system: Some("System rules".to_string()),
            prompt: "User content".to_string(),
            schema,
            timeout: None,
        }
    }

    #[test]
    fn llm_every_cli_prompt_starts_with_the_authorship_marker() {
        let dir = tempfile::tempdir().unwrap();
        for kind in CliKind::ALL {
            let invocation = build_invocation(kind, &request(None), dir.path()).unwrap();
            let prompt = invocation
                .stdin
                .clone()
                .or_else(|| {
                    let index = invocation.args.iter().position(|arg| arg == "-p")?;
                    invocation.args.get(index + 1).cloned()
                })
                .unwrap_or_else(|| panic!("{} has no prompt", kind.id()));
            assert!(
                prompt.starts_with(CLOVY_AUTHORSHIP_MARKER),
                "{} prompt lacks the marker",
                kind.id()
            );
            assert!(prompt.contains("System rules") && prompt.contains("User content"));
            assert!(invocation
                .set_env
                .contains(&("CLOVY_ONESHOT".to_string(), "1".to_string())));
        }
    }

    #[test]
    fn llm_cli_invocations_disable_sessions_and_tools() {
        let dir = tempfile::tempdir().unwrap();
        let args = |kind| {
            build_invocation(kind, &request(None), dir.path())
                .unwrap()
                .args
        };
        let claude = args(CliKind::Claude);
        assert!(claude.contains(&"--no-session-persistence".to_string()));
        let tools = claude.iter().position(|arg| arg == "--tools").unwrap();
        assert_eq!(claude[tools + 1], "");
        let codex = args(CliKind::Codex);
        assert!(codex.contains(&"--ephemeral".to_string()));
        assert!(codex.windows(2).any(|pair| pair == ["-s", "read-only"]));
        let pi = args(CliKind::Pi);
        for flag in [
            "--no-session",
            "--no-tools",
            "--no-context-files",
            "--no-skills",
        ] {
            assert!(pi.contains(&flag.to_string()), "pi lacks {flag}");
        }
        assert!(args(CliKind::CursorAgent).contains(&"ask".to_string()));
        let copilot = args(CliKind::Copilot);
        assert!(copilot.contains(&"--no-custom-instructions".to_string()));
        let excluded = copilot
            .iter()
            .find_map(|arg| arg.strip_prefix("--excluded-tools="))
            .unwrap();
        for tool in ["bash", "edit", "create", "view", "web_fetch", "task"] {
            assert!(
                excluded.split(',').any(|t| t == tool),
                "copilot keeps {tool}"
            );
        }
        let denied = copilot
            .iter()
            .find_map(|arg| arg.strip_prefix("--deny-tool="))
            .unwrap();
        for kind in ["shell", "write", "read", "url", "memory"] {
            assert!(
                denied.split(',').any(|k| k == kind),
                "copilot allows {kind}"
            );
        }
    }

    #[test]
    fn llm_schema_goes_to_native_flags_or_into_the_prompt() {
        let dir = tempfile::tempdir().unwrap();
        let schema = json!({"type": "object", "properties": {"ok": {"type": "boolean"}}});
        let claude =
            build_invocation(CliKind::Claude, &request(Some(schema.clone())), dir.path()).unwrap();
        let flag = claude
            .args
            .iter()
            .position(|arg| arg == "--json-schema")
            .unwrap();
        assert_eq!(claude.args[flag + 1], schema.to_string());
        assert!(!claude.stdin.unwrap().contains("matches this JSON schema"));

        let codex =
            build_invocation(CliKind::Codex, &request(Some(schema.clone())), dir.path()).unwrap();
        let flag = codex
            .args
            .iter()
            .position(|arg| arg == "--output-schema")
            .unwrap();
        let written: Value =
            serde_json::from_str(&std::fs::read_to_string(&codex.args[flag + 1]).unwrap()).unwrap();
        assert_eq!(written["additionalProperties"], json!(false));
        assert_eq!(written["required"], json!(["ok"]));

        let pi = build_invocation(CliKind::Pi, &request(Some(schema)), dir.path()).unwrap();
        assert!(pi.stdin.unwrap().contains("matches this JSON schema"));
    }

    fn finished(stdout: &str, success: bool) -> ProcessOutput {
        ProcessOutput {
            success,
            code: Some(if success { 0 } else { 1 }),
            stdout: stdout.to_string(),
            stderr: String::new(),
        }
    }

    #[test]
    fn llm_claude_envelope_yields_result_or_structured_output() {
        let text = finished(
            r#"{"type":"result","subtype":"success","is_error":false,"result":"Hello"}"#,
            true,
        );
        assert_eq!(
            parse_output(CliKind::Claude, &text, None, false).unwrap(),
            "Hello"
        );
        let structured = finished(
            r#"{"subtype":"success","is_error":false,"result":"","structured_output":{"ok":true}}"#,
            true,
        );
        assert_eq!(
            parse_output(CliKind::Claude, &structured, None, true).unwrap(),
            r#"{"ok":true}"#
        );
        let failed = finished(
            r#"{"subtype":"error_during_execution","is_error":true,"result":"Not logged in"}"#,
            false,
        );
        assert!(matches!(
            parse_output(CliKind::Claude, &failed, None, false),
            Err(LlmError::CliFailed { detail, .. }) if detail == "Not logged in"
        ));
    }

    #[test]
    fn llm_pi_stream_requires_agent_end() {
        let stream = [
            r#"{"type":"message_end","message":{"role":"assistant","stopReason":"stop","content":[{"type":"text","text":"Hi"}]}}"#,
            r#"{"type":"agent_end"}"#,
        ]
        .join("\n");
        assert_eq!(
            parse_output(CliKind::Pi, &finished(&stream, true), None, false).unwrap(),
            "Hi"
        );
        let truncated = stream.lines().next().unwrap().to_string();
        assert!(parse_output(CliKind::Pi, &finished(&truncated, true), None, false).is_err());
    }

    #[test]
    fn llm_json_request_rejects_prose_answers() {
        let output = finished("Sure! here you go", true);
        assert!(matches!(
            parse_output(CliKind::Copilot, &output, None, true),
            Err(LlmError::InvalidOutput(_))
        ));
        let fenced = finished("```json\n{\"ok\": true}\n```", true);
        assert!(parse_output(CliKind::Copilot, &fenced, None, true).is_ok());
    }
}
