//! Catalog of the six agent CLIs and their one-shot invocation contracts.
//!
//! A one-shot prompt carries untrusted text (meeting transcripts, notes,
//! coding-agent transcripts), so a one-shot call only runs on a CLI that can
//! be started with **every tool turned off** ([`CliKind::tools_disabled`]),
//! or, by the owner's decision, on codex and agy in their least-privileged
//! mode ([`CliKind::one_shot_allowed`]): codex in a read-only, offline
//! sandbox that never asks for approval, agy in print mode with its terminal
//! sandbox and no auto-approval. The other CLIs are refused with
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
/// agy reads the prompt from argv only; longer prompts are elided in the
/// middle.
const ARGV_PROMPT_CAP_CHARS: usize = 180_000;
/// Codex writes its final answer to this file in the scratch directory.
pub const CODEX_LAST_MESSAGE_FILE: &str = "last_message.txt";
/// The strict output schema codex validates against, in the scratch directory.
pub const CODEX_SCHEMA_FILE: &str = "schema.json";

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

    /// Whether a one-shot call (notes, activity) may run on this CLI: the
    /// tool-free ones, plus codex and agy in their least-privileged mode
    /// (owner decision): codex `exec --sandbox read-only` with approval
    /// `never`, no web search, no network, no saved session; agy `--print`
    /// with `--sandbox`, where nobody can approve a permission request.
    /// cursor-agent and copilot stay refused.
    pub fn one_shot_allowed(self) -> bool {
        self.tools_disabled() || matches!(self, CliKind::Codex | CliKind::Agy)
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
    /// Files written into the scratch directory before the call (name, text).
    pub files: Vec<(String, String)>,
    /// The answer is in this file of the scratch directory, not on stdout.
    pub output_file: Option<&'static str>,
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

/// Builds the argv/stdin/env for one isolated call: every tool off, or the
/// CLI's least-privileged mode (codex, agy). The call runs in an empty
/// scratch directory; relative file names below live there. CLIs that allow
/// neither are refused ([`LlmError::CliToolsNotDisabled`]); nothing is
/// spawned for them.
pub fn build_invocation(
    kind: CliKind,
    request: &GenerateRequest,
) -> Result<CliInvocation, LlmError> {
    let mut set_env = vec![
        ("CLOVY_ONESHOT".to_string(), "1".to_string()),
        ("DO_NOT_TRACK".to_string(), "1".to_string()),
    ];
    let mut remove_env = Vec::new();
    let mut files = Vec::new();
    let mut output_file = None;
    let mut stdin = Some(compose_prompt(kind, request));
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
        CliKind::Codex => {
            let mut args = strings(&[
                "exec",
                "--sandbox",
                "read-only",
                "--skip-git-repo-check",
                "--ephemeral",
                "--color",
                "never",
                "-c",
                "approval_policy=\"never\"",
                "-c",
                "web_search=\"disabled\"",
                "-c",
                "sandbox_workspace_write.network_access=false",
                "-c",
                "analytics.enabled=false",
                // Plugins and apps bring MCP servers that run outside the
                // sandbox; the user's own servers are turned off by name
                // ([`codex_mcp_off_args`]). Hooks and the `notify` program
                // also run outside it and receive the call's content.
                "-c",
                "features.plugins=false",
                "-c",
                "features.apps=false",
                "-c",
                "features.hooks=false",
                "-c",
                "notify=[]",
                "--output-last-message",
                CODEX_LAST_MESSAGE_FILE,
            ]);
            if let Some(schema) = &request.schema {
                files.push((CODEX_SCHEMA_FILE.to_string(), strictify(schema).to_string()));
                args.push("--output-schema".to_string());
                args.push(CODEX_SCHEMA_FILE.to_string());
            }
            args.push("-".to_string());
            output_file = Some(CODEX_LAST_MESSAGE_FILE);
            args
        }
        CliKind::Agy => {
            let prompt = stdin.take().unwrap_or_default();
            let mut args = vec![
                "--print".to_string(),
                cap_middle(&prompt, ARGV_PROMPT_CAP_CHARS),
                "--output-format".to_string(),
                "json".to_string(),
                "--sandbox".to_string(),
                "--disable-slash-commands".to_string(),
            ];
            if let Some(schema) = &request.schema {
                args.push("--json-schema".to_string());
                args.push(schema.to_string());
            }
            args
        }
        CliKind::CursorAgent | CliKind::Copilot => {
            return Err(LlmError::CliToolsNotDisabled(kind));
        }
    };
    Ok(CliInvocation {
        args,
        stdin,
        set_env,
        remove_env,
        files,
        output_file,
    })
}

/// Lists the MCP servers a one-shot codex call would start, resolved by
/// codex itself across every config layer, with plugins and apps already
/// off as in the call.
pub const CODEX_MCP_LIST_ARGS: [&str; 7] = [
    "-c",
    "features.plugins=false",
    "-c",
    "features.apps=false",
    "mcp",
    "list",
    "--json",
];

/// `-c mcp_servers.<name>.enabled=false` for every server in the output of
/// [`CODEX_MCP_LIST_ARGS`]. `--ignore-user-config` would also drop the
/// user's Windows sandbox and model settings, so servers are turned off one
/// by one. A list that does not parse, or a name that `-c` cannot address
/// as a single key, fails the call: nothing is sent to codex.
pub fn codex_mcp_off_args(list: &str) -> Result<Vec<String>, String> {
    let servers: Vec<Value> = serde_json::from_str(list.trim())
        .map_err(|_| "could not list its MCP servers to turn them off".to_string())?;
    let mut args = Vec::new();
    for server in servers {
        let name = server.get("name").and_then(Value::as_str).unwrap_or("");
        let addressable = !name.is_empty()
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
        if !addressable {
            return Err(format!(
                "could not turn off the MCP server {name:?}; rename it with letters, digits, - or _"
            ));
        }
        args.push("-c".to_string());
        args.push(format!("mcp_servers.{name}.enabled=false"));
    }
    Ok(args)
}

/// Why agy cannot take a one-shot call with the configuration under `home`:
/// print mode starts every enabled MCP server and plugin outside
/// `--sandbox`, and agy has no per-call switch to turn them off.
pub fn agy_extensions_on(home: &std::path::Path) -> Option<String> {
    let config = home.join(".gemini").join("config");
    let mut on = Vec::new();
    if let Ok(text) = std::fs::read_to_string(config.join("mcp_config.json")) {
        let Ok(parsed) = serde_json::from_str::<Value>(&text) else {
            return Some("its MCP configuration could not be read".to_string());
        };
        if let Some(servers) = parsed.get("mcpServers").and_then(Value::as_object) {
            on.extend(
                servers
                    .iter()
                    .filter(|(_, server)| {
                        !server
                            .get("disabled")
                            .and_then(Value::as_bool)
                            .unwrap_or(false)
                    })
                    .map(|(name, _)| format!("MCP server {name}")),
            );
        }
    }
    if let Ok(entries) = std::fs::read_dir(config.join("plugins")) {
        on.extend(
            entries
                .flatten()
                .map(|entry| format!("plugin {}", entry.file_name().to_string_lossy())),
        );
    }
    (!on.is_empty()).then(|| {
        format!(
            "it would start {} outside its sandbox. Turn them off with `agy mcp disable` or `agy plugin disable`, or choose another provider.",
            on.join(", ")
        )
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
        // `run_cli` puts the last-message file in place of stdout.
        CliKind::Codex => {
            if !output.success {
                return Err(failed(codex_error(&output.stderr)));
            }
            output.stdout.clone()
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
        CliKind::CursorAgent | CliKind::Copilot => {
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

/// The message of codex's `ERROR: {json}` line, else its first stderr line.
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
        for kind in [CliKind::CursorAgent, CliKind::Copilot] {
            assert!(!kind.tools_disabled());
            assert_eq!(
                build_invocation(kind, &request(None)),
                Err(LlmError::CliToolsNotDisabled(kind)),
                "{} cannot turn its tools off and must be refused",
                kind.id()
            );
        }
    }

    /// Codex keeps tools it cannot turn off, so a one-shot call runs it in its
    /// least-privileged mode: read-only sandbox (no writes, no network for
    /// commands), never asking for approval, no web search, no saved
    /// session, in the empty scratch directory, prompt on stdin.
    #[test]
    fn llm_codex_one_shot_runs_read_only_ephemeral_and_offline() {
        let codex = build_invocation(CliKind::Codex, &request(None)).unwrap();
        let args = &codex.args;
        assert_eq!(args.first().map(String::as_str), Some("exec"));
        assert!(args
            .windows(2)
            .any(|pair| pair == ["--sandbox", "read-only"]));
        for flag in ["--ephemeral", "--skip-git-repo-check"] {
            assert!(args.contains(&flag.to_string()), "codex lacks {flag}");
        }
        for config in [
            "approval_policy=\"never\"",
            "web_search=\"disabled\"",
            "sandbox_workspace_write.network_access=false",
        ] {
            assert!(
                args.windows(2)
                    .any(|pair| pair[0] == "-c" && pair[1] == config),
                "codex lacks -c {config}"
            );
        }
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--output-last-message", CODEX_LAST_MESSAGE_FILE]),
            "the answer is read from a file in the scratch directory"
        );
        assert_eq!(
            args.last().map(String::as_str),
            Some("-"),
            "prompt on stdin"
        );
        assert!(!args.iter().any(|arg| arg.contains("danger")));
        let prompt = codex.stdin.unwrap();
        assert_eq!(prompt.lines().next(), Some(CLOVY_AUTHORSHIP_MARKER));
    }

    #[test]
    fn llm_codex_schema_goes_to_a_strict_output_schema_file() {
        let schema = json!({"type": "object", "properties": {"ok": {"type": "boolean"}}});
        let codex = build_invocation(CliKind::Codex, &request(Some(schema.clone()))).unwrap();
        assert!(codex
            .args
            .windows(2)
            .any(|pair| pair == ["--output-schema", CODEX_SCHEMA_FILE]));
        assert_eq!(
            codex.files,
            vec![(
                CODEX_SCHEMA_FILE.to_string(),
                strictify(&schema).to_string()
            )]
        );
    }

    /// agy has no flag that turns its tools off; print mode denies every
    /// permission request (nobody can approve), `--sandbox` restricts its
    /// terminal, and nothing auto-approves.
    #[test]
    fn llm_agy_one_shot_uses_its_most_restricted_mode() {
        let schema = json!({"type": "object", "properties": {"ok": {"type": "boolean"}}});
        let agy = build_invocation(CliKind::Agy, &request(Some(schema.clone()))).unwrap();
        let args = &agy.args;
        let prompt = args.iter().position(|arg| arg == "--print").unwrap();
        assert!(args[prompt + 1].starts_with(CLOVY_AUTHORSHIP_MARKER));
        for flag in ["--sandbox", "--disable-slash-commands"] {
            assert!(args.contains(&flag.to_string()), "agy lacks {flag}");
        }
        assert!(args
            .windows(2)
            .any(|pair| pair == ["--output-format", "json"]));
        let schema_text = schema.to_string();
        assert!(args
            .windows(2)
            .any(|pair| pair[0] == "--json-schema" && pair[1] == schema_text));
        assert!(!args.iter().any(|arg| arg.contains("dangerously")));
        assert!(agy.stdin.is_none());
    }

    #[test]
    fn llm_codex_and_agy_answers_are_parsed() {
        // run_cli puts codex's last-message file in place of stdout.
        assert_eq!(
            parse_output(CliKind::Codex, &finished("Resumo\n", true), false).unwrap(),
            "Resumo"
        );
        let failed = ProcessOutput {
            stderr: "ERROR: {\"error\":{\"message\":\"Not signed in\"}}".to_string(),
            ..finished("", false)
        };
        assert!(matches!(
            parse_output(CliKind::Codex, &failed, false),
            Err(LlmError::CliFailed { detail, .. }) if detail == "Not signed in"
        ));
        assert_eq!(
            parse_output(
                CliKind::Agy,
                &finished(r#"{"status":"SUCCESS","response":"Olá"}"#, true),
                false
            )
            .unwrap(),
            "Olá"
        );
        assert_eq!(
            parse_output(
                CliKind::Agy,
                &finished(
                    r#"{"status":"SUCCESS","structured_output":{"ok":true}}"#,
                    true
                ),
                true
            )
            .unwrap(),
            r#"{"ok":true}"#
        );
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
