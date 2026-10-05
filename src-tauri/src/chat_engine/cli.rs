//! Per-CLI turn contracts for the CLI chat engine: the argv, stdin, and
//! environment of one conversation turn (with the CLI's own resume id), and
//! the translation of each CLI's JSON stream into [`EngineEvent`]s.
//!
//! Formats measured on the installed CLIs (claude 2.1, codex 0.160, pi 1.0,
//! agy 1.2, cursor-agent 2026.09) and, for copilot (not installed), taken from
//! its programmatic reference (`--output-format json` session events):
//!
//! | CLI | Turn | Resume id | Clovy MCP server |
//! |---|---|---|---|
//! | claude | `-p --output-format stream-json --verbose --include-partial-messages`, prompt on stdin | `session_id` of `system/init`, `--resume <id>` | `--mcp-config <file>` |
//! | codex | `exec [resume] --json --skip-git-repo-check -c sandbox_mode="workspace-write" [<id>] -`, prompt on stdin | `thread_id` of `thread.started` | `-c mcp_servers.clovy.*` |
//! | pi | `--print --mode json --session-id <id>`, prompt on stdin | chosen by Clovy (UUID) | no MCP support |
//! | agy | `--print=<prompt> --output-format stream-json` | `conversation_id` of `init`, `--conversation <id>` | only through `agy mcp add` (global), not offered |
//! | cursor-agent | `-p --output-format stream-json --stream-partial-output --trust -- <prompt>` | `session_id` of `system/init`, `--resume <id>` | project servers need approval in Cursor, not offered |
//! | copilot | `--prompt=<prompt> --output-format json --session-id <id>` | chosen by Clovy (UUID) | `--additional-mcp-config @<file>` |
//!
//! No flag that skips or pre-grants the CLI's own permission prompts is ever
//! passed: each CLI runs in its default non-interactive mode in the session
//! workspace (codex with the `workspace-write` sandbox). An action the CLI
//! refuses for lack of approval comes back as a failed tool marked
//! `needs_approval`.

use crate::llm::cli::CliKind;
use crate::mcp_server::{McpLaunch, SERVER_KEY};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// What a CLI stream means for the session.
#[derive(Clone, Debug, PartialEq)]
pub enum EngineEvent {
    /// The CLI's own conversation id, used to resume the next turn.
    Conversation(String),
    /// Streamed answer text.
    TextDelta(String),
    /// A complete answer block that was not streamed as deltas.
    Text(String),
    /// Streamed (or complete) reasoning text.
    Reasoning(String),
    ToolStarted {
        call_id: String,
        name: String,
        arguments: Value,
    },
    ToolFinished {
        call_id: String,
        name: String,
        output: Value,
        error: Option<String>,
        /// The CLI refused the action because it needs the user's approval,
        /// which a non-interactive turn cannot give.
        needs_approval: bool,
    },
    /// The CLI reported that the turn failed.
    Failed(String),
}

/// Whether Clovy can hand its MCP server to the CLI for one turn without
/// touching the CLI's own configuration.
pub fn accepts_mcp(kind: CliKind) -> bool {
    matches!(kind, CliKind::Claude | CliKind::Codex | CliKind::Copilot)
}

/// CLIs whose conversation id Clovy chooses (a fresh UUID) on the first turn.
pub fn preassigns_conversation(kind: CliKind) -> bool {
    matches!(kind, CliKind::Pi | CliKind::Copilot)
}

/// One turn, ready to spawn in the session workspace.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TurnInvocation {
    pub args: Vec<String>,
    pub stdin: Option<String>,
    pub remove_env: Vec<&'static str>,
    /// Files to write before spawning (MCP configuration), path and content.
    pub files: Vec<(PathBuf, String)>,
}

/// Builds one turn. `conversation` is the id to resume (claude, codex, agy,
/// cursor-agent) or the id Clovy chose (pi, copilot; always `Some` for them).
/// `mcp` is offered only to CLIs that [`accepts_mcp`]; configuration files go
/// to `scratch`.
pub fn turn_invocation(
    kind: CliKind,
    prompt: &str,
    conversation: Option<&str>,
    mcp: Option<&McpLaunch>,
    scratch: &Path,
) -> TurnInvocation {
    let mut files = Vec::new();
    let mut remove_env = Vec::new();
    let mut stdin = None;
    let args = match kind {
        CliKind::Claude => {
            let mut args = strings(&[
                "-p",
                "--output-format",
                "stream-json",
                "--verbose",
                "--include-partial-messages",
            ]);
            if let Some(id) = conversation {
                args.extend(strings(&["--resume", id]));
            }
            if let Some(launch) = mcp {
                let file = scratch.join("clovy-mcp.json");
                files.push((
                    file.clone(),
                    crate::mcp_server::claude_code_config(launch).to_string(),
                ));
                args.push("--mcp-config".into());
                args.push(file.display().to_string());
            }
            // Same rule as one-shot calls: a stray API key in the profile
            // would move the user from their subscription to metered billing.
            remove_env.push("ANTHROPIC_API_KEY");
            stdin = Some(prompt.to_string());
            args
        }
        CliKind::Codex => {
            let mut args = vec!["exec".to_string()];
            if conversation.is_some() {
                args.push("resume".into());
            }
            args.extend(strings(&[
                "--json",
                "--skip-git-repo-check",
                "-c",
                "sandbox_mode=\"workspace-write\"",
            ]));
            if let Some(launch) = mcp {
                args.push("-c".into());
                args.push(format!(
                    "mcp_servers.{SERVER_KEY}.command={}",
                    Value::String(launch.command.display().to_string())
                ));
                args.push("-c".into());
                args.push(format!(
                    "mcp_servers.{SERVER_KEY}.args={}",
                    json!(launch.args)
                ));
            }
            if let Some(id) = conversation {
                args.push(id.to_string());
            }
            args.push("-".into());
            stdin = Some(prompt.to_string());
            args
        }
        CliKind::Pi => {
            let mut args = strings(&["--print", "--mode", "json"]);
            if let Some(id) = conversation {
                args.extend(strings(&["--session-id", id]));
            }
            stdin = Some(prompt.to_string());
            args
        }
        CliKind::Agy => {
            let mut args = vec![
                format!("--print={prompt}"),
                "--output-format".into(),
                "stream-json".into(),
            ];
            if let Some(id) = conversation {
                args.extend(strings(&["--conversation", id]));
            }
            args
        }
        CliKind::CursorAgent => {
            // `--trust` only trusts the session workspace, a directory Clovy
            // created for this session; it grants no tool permission.
            let mut args = strings(&[
                "-p",
                "--output-format",
                "stream-json",
                "--stream-partial-output",
                "--trust",
            ]);
            if let Some(id) = conversation {
                args.extend(strings(&["--resume", id]));
            }
            args.push("--".into());
            args.push(prompt.to_string());
            args
        }
        CliKind::Copilot => {
            let mut args = vec![
                format!("--prompt={prompt}"),
                "--output-format".into(),
                "json".into(),
            ];
            if let Some(id) = conversation {
                args.extend(strings(&["--session-id", id]));
            }
            if let Some(launch) = mcp {
                let file = scratch.join("clovy-mcp.json");
                let config = json!({
                    "mcpServers": {
                        SERVER_KEY: {
                            "type": "local",
                            "command": launch.command.display().to_string(),
                            "args": launch.args,
                            "tools": ["*"],
                        }
                    }
                });
                files.push((file.clone(), config.to_string()));
                args.push(format!("--additional-mcp-config=@{}", file.display()));
            }
            args
        }
    };
    TurnInvocation {
        args,
        stdin,
        remove_env,
        files,
    }
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}

/// Turns one CLI's JSON lines into [`EngineEvent`]s.
pub struct StreamTranslator {
    kind: CliKind,
    tool_names: HashMap<String, String>,
    started_tools: HashSet<String>,
    /// Text of the current message arrived as deltas (claude, pi).
    text_streamed: bool,
    reasoning_streamed: bool,
    /// cursor-agent: text streamed since the last segment boundary.
    segment: String,
    segment_deltas: usize,
    /// copilot: message and reasoning ids that streamed deltas.
    streamed_ids: HashSet<String>,
    /// pi and codex report failures that a retry may still recover from.
    pending_failure: Option<String>,
    completed: bool,
}

impl StreamTranslator {
    pub fn new(kind: CliKind) -> Self {
        Self {
            kind,
            tool_names: HashMap::new(),
            started_tools: HashSet::new(),
            text_streamed: false,
            reasoning_streamed: false,
            segment: String::new(),
            segment_deltas: 0,
            streamed_ids: HashSet::new(),
            pending_failure: None,
            completed: false,
        }
    }

    /// Translates one stdout line; lines that are not JSON objects (warnings
    /// some CLIs print) are ignored.
    pub fn translate_line(&mut self, line: &str) -> Vec<EngineEvent> {
        let Ok(event) = serde_json::from_str::<Value>(line.trim()) else {
            return Vec::new();
        };
        if !event.is_object() {
            return Vec::new();
        }
        let mut out = Vec::new();
        match self.kind {
            CliKind::Claude => self.claude(&event, &mut out),
            CliKind::Codex => self.codex(&event, &mut out),
            CliKind::Pi => self.pi(&event, &mut out),
            CliKind::Agy => self.agy(&event, &mut out),
            CliKind::CursorAgent => self.cursor(&event, &mut out),
            CliKind::Copilot => self.copilot(&event, &mut out),
        }
        out
    }

    /// A failure that the stream reported without a final verdict (codex
    /// `error` without `turn.completed`, pi's last assistant message ending
    /// in error).
    pub fn unresolved_failure(&self) -> Option<String> {
        match self.kind {
            CliKind::Codex if !self.completed => Some(
                self.pending_failure
                    .clone()
                    .unwrap_or_else(|| "Codex ended without completing the turn.".into()),
            ),
            CliKind::Pi => self.pending_failure.clone(),
            _ => None,
        }
    }

    fn start_tool(
        &mut self,
        call_id: &str,
        name: &str,
        arguments: Value,
        out: &mut Vec<EngineEvent>,
    ) {
        if !self.started_tools.insert(call_id.to_string()) {
            return;
        }
        self.tool_names
            .insert(call_id.to_string(), name.to_string());
        out.push(EngineEvent::ToolStarted {
            call_id: call_id.to_string(),
            name: name.to_string(),
            arguments,
        });
    }

    fn finish_tool(
        &mut self,
        call_id: &str,
        name: Option<&str>,
        output: Value,
        error: Option<String>,
        needs_approval: bool,
        out: &mut Vec<EngineEvent>,
    ) {
        let name = name
            .map(str::to_string)
            .or_else(|| self.tool_names.get(call_id).cloned())
            .unwrap_or_else(|| "tool".into());
        if !self.started_tools.contains(call_id) {
            self.start_tool(call_id, &name, Value::Null, out);
        }
        out.push(EngineEvent::ToolFinished {
            call_id: call_id.to_string(),
            name,
            output,
            error,
            needs_approval,
        });
    }

    fn claude(&mut self, event: &Value, out: &mut Vec<EngineEvent>) {
        // Subagent (Task tool) traffic belongs to the tool call, not the answer.
        if event
            .get("parent_tool_use_id")
            .is_some_and(|parent| parent.is_string())
        {
            return;
        }
        match str_at(event, "type") {
            Some("system") if str_at(event, "subtype") == Some("init") => {
                if let Some(id) = str_at(event, "session_id") {
                    out.push(EngineEvent::Conversation(id.to_string()));
                }
            }
            Some("stream_event") => {
                let inner = &event["event"];
                match str_at(inner, "type") {
                    Some("message_start") => {
                        self.text_streamed = false;
                        self.reasoning_streamed = false;
                    }
                    Some("content_block_delta") => {
                        let delta = &inner["delta"];
                        match str_at(delta, "type") {
                            Some("text_delta") => {
                                if let Some(text) = non_empty(delta, "text") {
                                    self.text_streamed = true;
                                    out.push(EngineEvent::TextDelta(text.to_string()));
                                }
                            }
                            Some("thinking_delta") => {
                                if let Some(text) = non_empty(delta, "thinking") {
                                    self.reasoning_streamed = true;
                                    out.push(EngineEvent::Reasoning(text.to_string()));
                                }
                            }
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
            Some("assistant") => {
                for block in content_blocks(&event["message"]) {
                    match str_at(block, "type") {
                        Some("text") if !self.text_streamed => {
                            if let Some(text) = non_empty(block, "text") {
                                out.push(EngineEvent::Text(text.to_string()));
                            }
                        }
                        Some("thinking") if !self.reasoning_streamed => {
                            if let Some(text) = non_empty(block, "thinking") {
                                out.push(EngineEvent::Reasoning(text.to_string()));
                            }
                        }
                        Some("tool_use") => {
                            if let (Some(id), Some(name)) =
                                (str_at(block, "id"), str_at(block, "name"))
                            {
                                self.start_tool(id, name, block["input"].clone(), out);
                            }
                        }
                        _ => {}
                    }
                }
            }
            Some("user") => {
                for block in content_blocks(&event["message"]) {
                    if str_at(block, "type") != Some("tool_result") {
                        continue;
                    }
                    let Some(id) = str_at(block, "tool_use_id") else {
                        continue;
                    };
                    let text = content_text(&block["content"]);
                    if block.get("is_error").and_then(Value::as_bool) == Some(true) {
                        // Measured: "Claude requested permissions to write to
                        // <path>, but you haven't granted it yet."
                        let needs_approval = text.contains("requested permissions");
                        self.finish_tool(id, None, Value::Null, Some(text), needs_approval, out);
                    } else {
                        self.finish_tool(id, None, Value::String(text), None, false, out);
                    }
                }
            }
            Some("result") => {
                let is_error = event.get("is_error").and_then(Value::as_bool) == Some(true);
                let subtype = str_at(event, "subtype");
                if is_error || !matches!(subtype, None | Some("success")) {
                    let detail = non_empty(event, "result")
                        .or(subtype)
                        .unwrap_or("Claude Code reported an error.");
                    out.push(EngineEvent::Failed(detail.to_string()));
                }
            }
            _ => {}
        }
    }

    fn codex(&mut self, event: &Value, out: &mut Vec<EngineEvent>) {
        match str_at(event, "type") {
            Some("thread.started") => {
                if let Some(id) = str_at(event, "thread_id") {
                    out.push(EngineEvent::Conversation(id.to_string()));
                }
            }
            Some("item.started") => {
                let item = &event["item"];
                if let (Some(id), Some((name, arguments))) = (str_at(item, "id"), codex_tool(item))
                {
                    self.start_tool(id, &name, arguments, out);
                }
            }
            Some("item.completed") => {
                let item = &event["item"];
                let Some(id) = str_at(item, "id") else {
                    return;
                };
                match str_at(item, "type") {
                    Some("agent_message") => {
                        if let Some(text) = non_empty(item, "text") {
                            out.push(EngineEvent::Text(text.to_string()));
                        }
                    }
                    Some("reasoning") => {
                        if let Some(text) = non_empty(item, "text") {
                            out.push(EngineEvent::Reasoning(format!("{text}\n")));
                        }
                    }
                    _ => {
                        let Some((name, arguments)) = codex_tool(item) else {
                            return;
                        };
                        self.start_tool(id, &name, arguments, out);
                        let status = str_at(item, "status");
                        let (output, error, needs_approval) = match str_at(item, "type") {
                            Some("command_execution") => {
                                let output = str_at(item, "aggregated_output").unwrap_or_default();
                                let exit = item.get("exit_code").and_then(Value::as_i64);
                                let declined = status == Some("declined");
                                if declined
                                    || status == Some("failed")
                                    || exit.is_some_and(|code| code != 0)
                                {
                                    let detail = if output.trim().is_empty() {
                                        exit.map_or_else(
                                            || "The command did not run.".to_string(),
                                            |code| format!("Exit code {code}."),
                                        )
                                    } else {
                                        output.to_string()
                                    };
                                    (Value::Null, Some(detail), declined)
                                } else {
                                    (Value::String(output.to_string()), None, false)
                                }
                            }
                            Some("mcp_tool_call") => {
                                match item.get("error").filter(|error| !error.is_null()) {
                                    Some(error) => (
                                        Value::Null,
                                        Some(
                                            str_at(error, "message")
                                                .map(str::to_string)
                                                .unwrap_or_else(|| error.to_string()),
                                        ),
                                        false,
                                    ),
                                    None if status == Some("failed") => {
                                        (Value::Null, Some("The tool call failed.".into()), false)
                                    }
                                    None => (item["result"].clone(), None, false),
                                }
                            }
                            _ if matches!(status, Some("failed" | "declined")) => (
                                Value::Null,
                                Some("The change was not applied.".into()),
                                status == Some("declined"),
                            ),
                            _ => (Value::Null, None, false),
                        };
                        self.finish_tool(id, Some(&name), output, error, needs_approval, out);
                    }
                }
            }
            Some("turn.completed") => {
                self.completed = true;
                self.pending_failure = None;
            }
            Some("turn.failed") => {
                let detail = event
                    .get("error")
                    .and_then(|error| str_at(error, "message"))
                    .unwrap_or("Codex could not complete the turn.");
                out.push(EngineEvent::Failed(detail.to_string()));
            }
            Some("error") => {
                // Also used for transient notices (reconnecting); final only
                // if the turn never completes.
                if let Some(message) = non_empty(event, "message") {
                    self.pending_failure = Some(message.to_string());
                }
            }
            _ => {}
        }
    }

    fn pi(&mut self, event: &Value, out: &mut Vec<EngineEvent>) {
        match str_at(event, "type") {
            Some("session") => {
                if let Some(id) = str_at(event, "id") {
                    out.push(EngineEvent::Conversation(id.to_string()));
                }
            }
            Some("message_start") if is_assistant(&event["message"]) => {
                self.text_streamed = false;
                self.reasoning_streamed = false;
            }
            Some("message_update") => {
                let update = &event["assistantMessageEvent"];
                match str_at(update, "type") {
                    Some("text_delta") => {
                        if let Some(text) = non_empty(update, "delta") {
                            self.text_streamed = true;
                            out.push(EngineEvent::TextDelta(text.to_string()));
                        }
                    }
                    Some("thinking_delta") => {
                        if let Some(text) = non_empty(update, "delta") {
                            self.reasoning_streamed = true;
                            out.push(EngineEvent::Reasoning(text.to_string()));
                        }
                    }
                    _ => {}
                }
            }
            Some("message_end") if is_assistant(&event["message"]) => {
                let message = &event["message"];
                if matches!(str_at(message, "stopReason"), Some("error" | "aborted")) {
                    self.pending_failure = Some(
                        non_empty(message, "errorMessage")
                            .unwrap_or("Pi stopped with an error.")
                            .to_string(),
                    );
                    return;
                }
                self.pending_failure = None;
                for block in content_blocks(message) {
                    match str_at(block, "type") {
                        Some("text") if !self.text_streamed => {
                            if let Some(text) = non_empty(block, "text") {
                                out.push(EngineEvent::Text(text.to_string()));
                            }
                        }
                        Some("thinking") if !self.reasoning_streamed => {
                            if let Some(text) = non_empty(block, "thinking") {
                                out.push(EngineEvent::Reasoning(text.to_string()));
                            }
                        }
                        _ => {}
                    }
                }
            }
            Some("tool_execution_start") => {
                if let (Some(id), Some(name)) =
                    (str_at(event, "toolCallId"), str_at(event, "toolName"))
                {
                    self.start_tool(id, name, event["args"].clone(), out);
                }
            }
            Some("tool_execution_end") => {
                if let Some(id) = str_at(event, "toolCallId") {
                    let result = &event["result"];
                    if event.get("isError").and_then(Value::as_bool) == Some(true) {
                        let detail = content_text(&result["content"]);
                        self.finish_tool(
                            id,
                            str_at(event, "toolName"),
                            Value::Null,
                            Some(detail),
                            false,
                            out,
                        );
                    } else {
                        let output = Value::String(content_text(&result["content"]));
                        self.finish_tool(id, str_at(event, "toolName"), output, None, false, out);
                    }
                }
            }
            _ => {}
        }
    }

    fn agy(&mut self, event: &Value, out: &mut Vec<EngineEvent>) {
        match str_at(event, "event") {
            Some("init") => {
                if let Some(id) = str_at(event, "conversation_id") {
                    out.push(EngineEvent::Conversation(id.to_string()));
                }
            }
            Some("step_update") => {
                let step = &event["step_update"];
                match str_at(step, "step_type") {
                    Some("agent_response") => {
                        if let Some(text) = non_empty(step, "thinking_delta") {
                            out.push(EngineEvent::Reasoning(text.to_string()));
                        }
                        if let Some(text) = non_empty(step, "text_delta") {
                            out.push(EngineEvent::TextDelta(text.to_string()));
                        }
                    }
                    Some("tool") => {
                        let call_id = format!(
                            "agy-step-{}",
                            step.get("step_index").and_then(Value::as_i64).unwrap_or(-1)
                        );
                        let info = &step["tool_info"];
                        let name = str_at(step, "tool_name")
                            .or_else(|| str_at(info, "name"))
                            .unwrap_or("tool")
                            .to_string();
                        self.start_tool(&call_id, &name, info["parameters"].clone(), out);
                        match str_at(step, "state") {
                            Some("DONE") => {
                                let output = info
                                    .get("result")
                                    .or_else(|| info.get("output"))
                                    .cloned()
                                    .unwrap_or(Value::Null);
                                self.finish_tool(&call_id, Some(&name), output, None, false, out);
                            }
                            Some("ERROR") => {
                                // Measured: "permission check failed for ...:
                                // user denied permission to run command".
                                let detail = info
                                    .get("error")
                                    .and_then(|error| str_at(error, "message"))
                                    .unwrap_or("The tool failed.")
                                    .to_string();
                                let needs_approval = detail.contains("permission");
                                self.finish_tool(
                                    &call_id,
                                    Some(&name),
                                    Value::Null,
                                    Some(detail),
                                    needs_approval,
                                    out,
                                );
                            }
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
            Some("result") => {
                let result = &event["result"];
                if let Some(status) = str_at(result, "status").filter(|status| *status != "SUCCESS")
                {
                    let detail = result
                        .get("error")
                        .and_then(|error| str_at(error, "message").or_else(|| error.as_str()))
                        .unwrap_or(status);
                    out.push(EngineEvent::Failed(detail.to_string()));
                }
            }
            _ => {}
        }
    }

    fn cursor(&mut self, event: &Value, out: &mut Vec<EngineEvent>) {
        match str_at(event, "type") {
            Some("system") if str_at(event, "subtype") == Some("init") => {
                if let Some(id) = str_at(event, "session_id") {
                    out.push(EngineEvent::Conversation(id.to_string()));
                }
            }
            Some("thinking") if str_at(event, "subtype") == Some("delta") => {
                if let Some(text) = non_empty(event, "text") {
                    out.push(EngineEvent::Reasoning(text.to_string()));
                }
            }
            Some("assistant") => {
                let text = content_blocks(&event["message"])
                    .filter(|block| str_at(block, "type") == Some("text"))
                    .filter_map(|block| str_at(block, "text"))
                    .collect::<String>();
                if text.is_empty() {
                    return;
                }
                let partial = event.get("timestamp_ms").is_some();
                // With --stream-partial-output each segment arrives as deltas
                // and then again whole (timestamped before a tool call, plain
                // at the end); the whole copy is dropped.
                if !self.segment.is_empty()
                    && text == self.segment
                    && (self.segment_deltas > 1 || !partial)
                {
                    self.segment.clear();
                    self.segment_deltas = 0;
                } else if partial {
                    self.segment.push_str(&text);
                    self.segment_deltas += 1;
                    out.push(EngineEvent::TextDelta(text));
                } else {
                    out.push(EngineEvent::Text(text));
                }
            }
            Some("tool_call") => {
                let Some(call_id) = str_at(event, "call_id") else {
                    return;
                };
                let Some((key, call)) =
                    event
                        .get("tool_call")
                        .and_then(Value::as_object)
                        .and_then(|object| {
                            object
                                .iter()
                                .find(|(key, value)| key.ends_with("ToolCall") && value.is_object())
                        })
                else {
                    return;
                };
                let name = cursor_tool_name(key, call);
                match str_at(event, "subtype") {
                    Some("started") => {
                        self.segment.clear();
                        self.segment_deltas = 0;
                        self.start_tool(call_id, &name, call["args"].clone(), out);
                    }
                    Some("completed") => {
                        let result = &call["result"];
                        if let Some(success) = result.get("success") {
                            self.finish_tool(
                                call_id,
                                Some(&name),
                                success.clone(),
                                None,
                                false,
                                out,
                            );
                        } else if let Some(rejected) = result.get("rejected") {
                            let reason = non_empty(rejected, "reason")
                                .unwrap_or("Cursor Agent did not get approval to run this.");
                            self.finish_tool(
                                call_id,
                                Some(&name),
                                Value::Null,
                                Some(reason.to_string()),
                                true,
                                out,
                            );
                        } else {
                            let detail = result
                                .as_object()
                                .and_then(|object| {
                                    object.values().find_map(|value| {
                                        str_at(value, "error").or_else(|| str_at(value, "message"))
                                    })
                                })
                                .unwrap_or("The tool failed.");
                            self.finish_tool(
                                call_id,
                                Some(&name),
                                Value::Null,
                                Some(detail.to_string()),
                                false,
                                out,
                            );
                        }
                    }
                    _ => {}
                }
            }
            Some("result") if event.get("is_error").and_then(Value::as_bool) == Some(true) => {
                let detail =
                    non_empty(event, "result").unwrap_or("Cursor Agent reported an error.");
                out.push(EngineEvent::Failed(detail.to_string()));
            }
            _ => {}
        }
    }

    fn copilot(&mut self, event: &Value, out: &mut Vec<EngineEvent>) {
        let data = &event["data"];
        match str_at(event, "type") {
            Some("assistant.message_delta") => {
                if let Some(id) = str_at(data, "messageId") {
                    self.streamed_ids.insert(id.to_string());
                }
                if let Some(text) = non_empty(data, "deltaContent") {
                    out.push(EngineEvent::TextDelta(text.to_string()));
                }
            }
            Some("assistant.message") => {
                let streamed =
                    str_at(data, "messageId").is_some_and(|id| self.streamed_ids.contains(id));
                if !streamed {
                    let text = content_text(&data["content"]);
                    if !text.trim().is_empty() {
                        out.push(EngineEvent::Text(text));
                    }
                }
            }
            Some("assistant.reasoning_delta") => {
                if let Some(id) = str_at(data, "reasoningId") {
                    self.streamed_ids.insert(format!("reasoning:{id}"));
                }
                if let Some(text) = non_empty(data, "deltaContent") {
                    out.push(EngineEvent::Reasoning(text.to_string()));
                }
            }
            Some("assistant.reasoning") => {
                let streamed = str_at(data, "reasoningId")
                    .is_some_and(|id| self.streamed_ids.contains(&format!("reasoning:{id}")));
                if !streamed {
                    if let Some(text) = non_empty(data, "content") {
                        out.push(EngineEvent::Reasoning(text.to_string()));
                    }
                }
            }
            Some("tool.execution_start") => {
                if let (Some(id), Some(name)) =
                    (str_at(data, "toolCallId"), str_at(data, "toolName"))
                {
                    self.start_tool(id, name, data["arguments"].clone(), out);
                }
            }
            Some("tool.execution_complete") => {
                let Some(id) = str_at(data, "toolCallId") else {
                    return;
                };
                if data.get("success").and_then(Value::as_bool) == Some(false) {
                    let error = &data["error"];
                    let detail = str_at(error, "message")
                        .unwrap_or("The tool failed.")
                        .to_string();
                    let code = str_at(error, "code").unwrap_or_default();
                    let needs_approval = detail.to_ascii_lowercase().contains("permission")
                        || code.contains("denied")
                        || code.contains("permission");
                    self.finish_tool(id, None, Value::Null, Some(detail), needs_approval, out);
                } else {
                    let result = &data["result"];
                    let output = str_at(result, "detailedContent")
                        .map(str::to_string)
                        .unwrap_or_else(|| content_text(&result["content"]));
                    self.finish_tool(id, None, Value::String(output), None, false, out);
                }
            }
            Some("session.error") => {
                let detail = non_empty(data, "message").unwrap_or("Copilot reported an error.");
                out.push(EngineEvent::Failed(detail.to_string()));
            }
            Some("result") => {
                let exit = event
                    .get("exitCode")
                    .or_else(|| data.get("exitCode"))
                    .and_then(Value::as_i64);
                if exit.is_some_and(|code| code != 0) {
                    out.push(EngineEvent::Failed(format!(
                        "Copilot ended with exit code {}.",
                        exit.unwrap_or_default()
                    )));
                }
            }
            _ => {}
        }
    }
}

fn str_at<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

fn non_empty<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    str_at(value, key).filter(|text| !text.is_empty())
}

fn is_assistant(message: &Value) -> bool {
    str_at(message, "role") == Some("assistant")
}

fn content_blocks(message: &Value) -> impl Iterator<Item = &Value> {
    message
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
}

/// Text of a tool result or message content: a string, or the `text` of an
/// array of content blocks.
fn content_text(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter_map(|block| str_at(block, "text"))
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// codex `exec --json` items that are tool activity, as (name, arguments).
fn codex_tool(item: &Value) -> Option<(String, Value)> {
    match str_at(item, "type")? {
        "command_execution" => Some(("shell".into(), json!({ "command": item["command"] }))),
        "mcp_tool_call" => Some((
            format!(
                "{}.{}",
                str_at(item, "server").unwrap_or("mcp"),
                str_at(item, "tool").unwrap_or("tool")
            ),
            item["arguments"].clone(),
        )),
        "file_change" => Some(("apply_patch".into(), json!({ "changes": item["changes"] }))),
        "web_search" => Some(("web_search".into(), json!({ "query": item["query"] }))),
        _ => None,
    }
}

/// `shellToolCall` -> `shell`; MCP calls carry the tool's own name.
fn cursor_tool_name(key: &str, call: &Value) -> String {
    let base = key.trim_end_matches("ToolCall");
    if base == "mcp" {
        let args = &call["args"];
        if let Some(tool) = str_at(args, "toolName").or_else(|| str_at(args, "name")) {
            return match str_at(args, "providerIdentifier") {
                Some(server) => format!("{server}.{tool}"),
                None => tool.to_string(),
            };
        }
    }
    base.to_string()
}
