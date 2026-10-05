//! Catalog of the six agent CLIs and their one-shot invocation contracts.
//!
//! A one-shot prompt carries untrusted text (meeting transcripts, notes,
//! coding-agent transcripts), so a one-shot call only runs on a CLI that can
//! be started with **every tool turned off** ([`CliKind::tools_disabled`]):
//! text in the prompt can then never make the CLI read files, run commands,
//! or reach the network. The other CLIs are refused with
//! [`LlmError::CliToolsNotDisabled`] before anything is spawned.
//!
//! Each call is also isolated as far as the CLI allows: no session persisted,
//! no project context files, skills, or slash commands, an empty scratch
//! working directory, and a timeout enforced by [`super::process`]. The
//! prompt always starts with [`super::CLOVY_AUTHORSHIP_MARKER`] on its own
//! first line so session ingestion can skip Clovy's own calls, and the child
//! gets `CLOVY_ONESHOT=1` for the same purpose.

use super::{
    parse_json_value, process::ProcessOutput, GenerateRequest, LlmError, StructuredOutputLevel,
    CLOVY_AUTHORSHIP_MARKER,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const ERROR_DETAIL_CHARS: usize = 300;

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

    /// Whether a one-shot call can run with every tool turned off.
    ///
    /// - claude: `--tools ""` turns off every built-in tool, and
    ///   `--strict-mcp-config` without `--mcp-config` (plus `--setting-sources ""`)
    ///   loads no MCP server or plugin.
    /// - pi: `--no-tools` turns off built-in and extension tools, and
    ///   `--no-extensions` loads none.
    /// - codex (`exec` always offers shell, patch, MCP, and app tools that new
    ///   versions keep adding), agy (no flag), cursor-agent (print mode "has
    ///   access to all tools"), and copilot (MCP servers from the user's
    ///   configuration and plugins cannot all be excluded) cannot guarantee it.
    pub fn tools_disabled(self) -> bool {
        matches!(self, CliKind::Claude | CliKind::Pi)
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
}

/// The single prompt text a CLI receives: the marker alone on the first
/// line, then the system instructions, the user content, and (for
/// prompt-level CLIs) the schema.
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

/// Builds the argv/stdin/env for one isolated call with every tool off.
/// CLIs that cannot guarantee that are refused
/// ([`LlmError::CliToolsNotDisabled`]); nothing is spawned for them.
pub fn build_invocation(
    kind: CliKind,
    request: &GenerateRequest,
) -> Result<CliInvocation, LlmError> {
    let mut set_env = vec![
        ("CLOVY_ONESHOT".to_string(), "1".to_string()),
        ("DO_NOT_TRACK".to_string(), "1".to_string()),
    ];
    let mut remove_env = Vec::new();
    let args = match kind {
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
            if let Some(schema) = &request.schema {
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
            args
        }
        CliKind::Pi => {
            set_env.push(("PI_TELEMETRY".to_string(), "0".to_string()));
            strings(&[
                "--print",
                "--mode",
                "json",
                "--no-session",
                "--no-tools",
                "--no-extensions",
                "--no-approve",
                "--no-context-files",
                "--no-skills",
                "--no-prompt-templates",
            ])
        }
        CliKind::Codex | CliKind::Agy | CliKind::CursorAgent | CliKind::Copilot => {
            return Err(LlmError::CliToolsNotDisabled(kind));
        }
    };
    Ok(CliInvocation {
        args,
        stdin: Some(compose_prompt(kind, request)),
        set_env,
        remove_env,
    })
}

/// Extracts the model's answer from a finished CLI call.
pub fn parse_output(
    kind: CliKind,
    output: &ProcessOutput,
    wants_json: bool,
) -> Result<String, LlmError> {
    let failed = |detail: String| LlmError::CliFailed {
        cli: kind,
        detail: truncate(&detail, ERROR_DETAIL_CHARS),
    };
    let text = match kind {
        CliKind::Claude => {
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
        CliKind::Pi => parse_pi_stream(&output.stdout).map_err(failed)?,
        CliKind::Codex | CliKind::Agy | CliKind::CursorAgent | CliKind::Copilot => {
            return Err(LlmError::CliToolsNotDisabled(kind));
        }
    };
    if !output.success && text.trim().is_empty() {
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
    fn llm_every_cli_prompt_starts_with_the_marker_alone_on_its_first_line() {
        for kind in CliKind::ALL
            .into_iter()
            .filter(|kind| kind.tools_disabled())
        {
            let invocation = build_invocation(kind, &request(None)).unwrap();
            let prompt = invocation.stdin.clone().unwrap();
            assert_eq!(
                prompt.lines().next(),
                Some(CLOVY_AUTHORSHIP_MARKER),
                "{} prompt lacks the marker line",
                kind.id()
            );
            assert!(prompt.contains("System rules") && prompt.contains("User content"));
            assert!(invocation
                .set_env
                .contains(&("CLOVY_ONESHOT".to_string(), "1".to_string())));
        }
    }

    #[test]
    fn llm_one_shot_calls_run_only_with_every_tool_off() {
        let claude = build_invocation(CliKind::Claude, &request(None))
            .unwrap()
            .args;
        assert!(claude.contains(&"--no-session-persistence".to_string()));
        let tools = claude.iter().position(|arg| arg == "--tools").unwrap();
        assert_eq!(claude[tools + 1], "", "every built-in tool off");
        assert!(claude.contains(&"--strict-mcp-config".to_string()));
        assert!(
            !claude.iter().any(|arg| arg == "--mcp-config"),
            "no MCP server is loaded"
        );
        let pi = build_invocation(CliKind::Pi, &request(None)).unwrap().args;
        for flag in [
            "--no-session",
            "--no-tools",
            "--no-extensions",
            "--no-context-files",
            "--no-skills",
        ] {
            assert!(pi.contains(&flag.to_string()), "pi lacks {flag}");
        }
        for kind in [
            CliKind::Codex,
            CliKind::Agy,
            CliKind::CursorAgent,
            CliKind::Copilot,
        ] {
            assert!(!kind.tools_disabled());
            assert_eq!(
                build_invocation(kind, &request(None)),
                Err(LlmError::CliToolsNotDisabled(kind)),
                "{} cannot turn its tools off and must be refused",
                kind.id()
            );
        }
    }

    #[test]
    fn llm_schema_goes_to_native_flags_or_into_the_prompt() {
        let schema = json!({"type": "object", "properties": {"ok": {"type": "boolean"}}});
        let claude = build_invocation(CliKind::Claude, &request(Some(schema.clone()))).unwrap();
        let flag = claude
            .args
            .iter()
            .position(|arg| arg == "--json-schema")
            .unwrap();
        assert_eq!(claude.args[flag + 1], schema.to_string());
        assert!(!claude.stdin.unwrap().contains("matches this JSON schema"));

        let pi = build_invocation(CliKind::Pi, &request(Some(schema))).unwrap();
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
            parse_output(CliKind::Claude, &text, false).unwrap(),
            "Hello"
        );
        let structured = finished(
            r#"{"subtype":"success","is_error":false,"result":"","structured_output":{"ok":true}}"#,
            true,
        );
        assert_eq!(
            parse_output(CliKind::Claude, &structured, true).unwrap(),
            r#"{"ok":true}"#
        );
        let failed = finished(
            r#"{"subtype":"error_during_execution","is_error":true,"result":"Not logged in"}"#,
            false,
        );
        assert!(matches!(
            parse_output(CliKind::Claude, &failed, false),
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
            parse_output(CliKind::Pi, &finished(&stream, true), false).unwrap(),
            "Hi"
        );
        let truncated = stream.lines().next().unwrap().to_string();
        assert!(parse_output(CliKind::Pi, &finished(&truncated, true), false).is_err());
    }

    #[test]
    fn llm_json_request_rejects_prose_answers() {
        let envelope = |result: &str| {
            finished(
                &json!({"subtype": "success", "is_error": false, "result": result}).to_string(),
                true,
            )
        };
        assert!(matches!(
            parse_output(CliKind::Claude, &envelope("Sure! here you go"), true),
            Err(LlmError::InvalidOutput(_))
        ));
        let fenced = envelope("```json\n{\"ok\": true}\n```");
        assert!(parse_output(CliKind::Claude, &fenced, true).is_ok());
    }
}
