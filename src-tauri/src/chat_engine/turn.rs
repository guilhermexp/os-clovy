//! One CLI turn: resume lookup, prompt, the child process in its own process
//! group, stream translation, and the run events persisted through the same
//! path as the sidecar's (`agent_runtime::host::persist_runtime_event`).

use super::cli::{self, EngineEvent, StreamTranslator};
use super::{engine_model_id, FrameSink};
use crate::agent_runtime::protocol::{RpcFrame, PROTOCOL_VERSION};
use crate::agent_runtime::{AgentItemPayload, AgentRepository};
use crate::llm::cli::CliKind;
use crate::mcp_server::McpLaunch;
use serde_json::{json, Value};
use sqlx::row::Row;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::sync::watch;
use uuid::Uuid;

/// Longest stdout line kept; longer lines (a huge tool result) are skipped.
const MAX_LINE_BYTES: usize = 16 * 1024 * 1024;
/// Tail of stderr kept for error messages.
const STDERR_TAIL_BYTES: usize = 16 * 1024;
/// Earlier conversation handed to a CLI that starts without a resume id.
const MAX_HISTORY_CHARS: usize = 24_000;
/// Time a cancelled CLI gets to save its session after SIGTERM.
const CANCEL_GRACE: Duration = Duration::from_secs(2);

/// Everything one turn needs, resolved by the caller.
#[derive(Clone, Debug)]
pub struct TurnContext {
    pub session_id: String,
    pub run_id: String,
    pub kind: CliKind,
    pub program: PathBuf,
    /// The complete child environment (the login-shell environment).
    pub env: BTreeMap<String, String>,
    pub workspace: PathBuf,
    pub input: String,
    /// Clovy's MCP server, for CLIs that accept it.
    pub mcp: Option<McpLaunch>,
}

enum TurnEnd {
    Exited { success: bool, code: Option<i32> },
    Cancelled,
    SpawnFailed(String),
}

/// Numbers and publishes the run's events in order.
struct RunFrames<'a, S: FrameSink> {
    sink: &'a S,
    session_id: &'a str,
    run_id: &'a str,
    sequence: i64,
}

impl<S: FrameSink> RunFrames<'_, S> {
    async fn emit(&mut self, method: &str, params: Value) {
        self.sequence += 1;
        self.sink
            .publish(RpcFrame {
                jsonrpc: "2.0".into(),
                protocol_version: PROTOCOL_VERSION,
                session_id: self.session_id.to_string(),
                run_id: self.run_id.to_string(),
                sequence: self.sequence,
                id: None,
                event_id: Some(Uuid::new_v4().to_string()),
                method: Some(method.to_string()),
                params: Some(params),
                result: None,
                error: None,
            })
            .await;
    }
}

/// Runs one turn to its end and publishes `run.started`, the stream, and
/// exactly one terminal event (`run.completed`, `run.failed`, or
/// `run.cancelled`). `cancel` turning `true` stops the CLI's whole process
/// group.
pub async fn execute_turn<S: FrameSink>(
    repository: &AgentRepository,
    sink: &S,
    context: TurnContext,
    mut cancel: watch::Receiver<bool>,
) {
    let mut state = TurnState {
        frames: RunFrames {
            sink,
            session_id: &context.session_id,
            run_id: &context.run_id,
            sequence: 0,
        },
        repository,
        context: &context,
        text: String::new(),
        reported_failure: None,
        conversation_saved: false,
    };
    let frames = &mut state.frames;
    let name = context.kind.display_name();
    frames
        .emit(
            "run.started",
            json!({ "model": engine_model_id(context.kind) }),
        )
        .await;

    let resumed = match previous_conversation(repository, &context).await {
        Ok(found) => found,
        Err(error) => {
            tracing::warn!(%error, run_id = %context.run_id, "could not read the previous CLI conversation id");
            None
        }
    };
    let prompt = if resumed.is_some() {
        context.input.clone()
    } else {
        with_earlier_messages(repository, &context).await
    };
    let conversation = resumed
        .or_else(|| cli::preassigns_conversation(context.kind).then(|| Uuid::new_v4().to_string()));
    if let Some(id) = conversation.as_deref() {
        state.conversation_saved = save_conversation(repository, &context, id).await;
    }
    let frames = &mut state.frames;

    let scratch = match tempfile::Builder::new().prefix("clovy-chat-").tempdir() {
        Ok(scratch) => scratch,
        Err(error) => {
            frames
                .emit(
                    "run.failed",
                    failure(&format!("Could not prepare {name}: {error}")),
                )
                .await;
            return;
        }
    };
    let mcp = context
        .mcp
        .as_ref()
        .filter(|_| cli::accepts_mcp(context.kind));
    let invocation = cli::turn_invocation(
        context.kind,
        &prompt,
        conversation.as_deref(),
        mcp,
        scratch.path(),
    );
    for (path, content) in &invocation.files {
        if let Err(error) = std::fs::write(path, content) {
            frames
                .emit(
                    "run.failed",
                    failure(&format!("Could not prepare {name}: {error}")),
                )
                .await;
            return;
        }
    }
    let mut env = context.env.clone();
    for name in &invocation.remove_env {
        env.remove(*name);
    }

    let mut translator = StreamTranslator::new(context.kind);
    let mut stderr_tail = String::new();
    let end = run_process(
        &invocation,
        env,
        &mut translator,
        &mut cancel,
        &mut stderr_tail,
        &mut state,
    )
    .await;
    drop(scratch);

    let TurnState {
        mut frames,
        text,
        reported_failure,
        ..
    } = state;
    if !text.is_empty() {
        frames
            .emit("message.completed", json!({ "text": text }))
            .await;
    }
    match end {
        TurnEnd::Cancelled => frames.emit("run.cancelled", json!({})).await,
        TurnEnd::SpawnFailed(error) => {
            frames
                .emit(
                    "run.failed",
                    failure(&format!("Could not start {name}: {error}")),
                )
                .await
        }
        TurnEnd::Exited { success, code } => {
            let failed = reported_failure
                .or_else(|| translator.unresolved_failure())
                .or_else(|| {
                    (!success).then(|| {
                        last_line(&stderr_tail).map_or_else(
                            || match code {
                                Some(code) => format!("{name} exited with code {code}."),
                                None => format!("{name} stopped unexpectedly."),
                            },
                            |line| format!("{name}: {line}"),
                        )
                    })
                });
            match failed {
                Some(message) => frames.emit("run.failed", failure(&message)).await,
                None => frames.emit("run.completed", json!({})).await,
            }
        }
    }
}

fn failure(message: &str) -> Value {
    json!({
        "error": message,
        "category": "provider",
        "code": "agent_cli_failed",
        "retryable": true,
    })
}

fn last_line(text: &str) -> Option<&str> {
    text.lines().map(str::trim).rfind(|line| !line.is_empty())
}

async fn run_process<S: FrameSink>(
    invocation: &cli::TurnInvocation,
    env: BTreeMap<String, String>,
    translator: &mut StreamTranslator,
    cancel: &mut watch::Receiver<bool>,
    stderr_tail: &mut String,
    state: &mut TurnState<'_, S>,
) -> TurnEnd {
    let context = state.context;
    let mut command = tokio::process::Command::new(&context.program);
    command
        .args(&invocation.args)
        .env_clear()
        .envs(&env)
        .current_dir(&context.workspace)
        .stdin(if invocation.stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    command.process_group(0);
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => return TurnEnd::SpawnFailed(error.to_string()),
    };
    let group = child.id();
    if let (Some(input), Some(mut stdin)) = (invocation.stdin.clone(), child.stdin.take()) {
        tokio::spawn(async move {
            let _ = stdin.write_all(input.as_bytes()).await;
            let _ = stdin.shutdown().await;
        });
    }
    let stderr_task = child.stderr.take().map(|stderr| {
        tokio::spawn(async move {
            let mut reader = stderr;
            let mut kept = Vec::new();
            let mut buffer = [0_u8; 8 * 1024];
            loop {
                match reader.read(&mut buffer).await {
                    Ok(0) | Err(_) => break,
                    Ok(read) => {
                        kept.extend_from_slice(&buffer[..read]);
                        if kept.len() > STDERR_TAIL_BYTES {
                            kept.drain(..kept.len() - STDERR_TAIL_BYTES);
                        }
                    }
                }
            }
            String::from_utf8_lossy(&kept).into_owned()
        })
    });
    let Some(stdout) = child.stdout.take() else {
        stop_group(&mut child, group).await;
        return TurnEnd::SpawnFailed("stdout was unavailable".into());
    };
    let mut reader = BufReader::new(stdout);
    let mut line = Vec::new();
    let mut oversized = false;
    loop {
        tokio::select! {
            biased;
            () = cancelled(cancel) => {
                stop_group(&mut child, group).await;
                return TurnEnd::Cancelled;
            }
            read = read_line_capped(&mut reader, &mut line, &mut oversized) => {
                match read {
                    Ok(true) => {
                        if !oversized {
                            let text_line = String::from_utf8_lossy(&line).into_owned();
                            for event in translator.translate_line(&text_line) {
                                state.apply(event).await;
                            }
                        }
                        line.clear();
                        oversized = false;
                    }
                    Ok(false) | Err(_) => break,
                }
            }
        }
    }
    let status = tokio::select! {
        biased;
        () = cancelled(cancel) => {
            stop_group(&mut child, group).await;
            return TurnEnd::Cancelled;
        }
        status = child.wait() => status,
    };
    // Sweep helpers the CLI left in its group (MCP servers it spawned).
    signal_group(group, SweepSignal::Kill);
    if let Some(task) = stderr_task {
        if let Ok(Ok(tail)) = tokio::time::timeout(Duration::from_millis(500), task).await {
            *stderr_tail = tail;
        }
    }
    match status {
        Ok(status) => TurnEnd::Exited {
            success: status.success(),
            code: status.code(),
        },
        Err(error) => TurnEnd::SpawnFailed(error.to_string()),
    }
}

/// The run's frames and what the turn has gathered so far.
struct TurnState<'a, S: FrameSink> {
    frames: RunFrames<'a, S>,
    repository: &'a AgentRepository,
    context: &'a TurnContext,
    /// The whole answer, published again as `message.completed`.
    text: String,
    reported_failure: Option<String>,
    conversation_saved: bool,
}

impl<S: FrameSink> TurnState<'_, S> {
    async fn apply(&mut self, event: EngineEvent) {
        let name = self.context.kind.display_name();
        match event {
            EngineEvent::Conversation(id) => {
                if !self.conversation_saved {
                    self.conversation_saved =
                        save_conversation(self.repository, self.context, &id).await;
                }
            }
            EngineEvent::TextDelta(delta) => {
                self.text.push_str(&delta);
                self.frames
                    .emit("message.delta", json!({ "delta": delta }))
                    .await;
            }
            EngineEvent::Text(block) => {
                let delta = if self.text.is_empty() || self.text.ends_with("\n\n") {
                    block
                } else if self.text.ends_with('\n') {
                    format!("\n{block}")
                } else {
                    format!("\n\n{block}")
                };
                self.text.push_str(&delta);
                self.frames
                    .emit("message.delta", json!({ "delta": delta }))
                    .await;
            }
            EngineEvent::Reasoning(delta) => {
                self.frames
                    .emit("reasoning.delta", json!({ "delta": delta }))
                    .await;
            }
            EngineEvent::ToolStarted {
                call_id,
                name: tool,
                arguments,
            } => {
                self.frames
                    .emit(
                        "tool.started",
                        json!({ "callId": call_id, "name": tool, "arguments": arguments }),
                    )
                    .await;
            }
            EngineEvent::ToolFinished {
                call_id,
                name: tool,
                output: _,
                error: Some(error),
                needs_approval,
            } => {
                let error = if needs_approval {
                    format!(
                        "{name} needs your approval for this action, and a chat turn cannot ask for it. Allow it in {name}'s own settings to let it run. ({})",
                        error.trim()
                    )
                } else {
                    error
                };
                self.frames
                    .emit(
                        "tool.failed",
                        json!({ "callId": call_id, "name": tool, "error": error, "needsApproval": needs_approval }),
                    )
                    .await;
            }
            EngineEvent::ToolFinished {
                call_id,
                name: tool,
                output,
                error: None,
                ..
            } => {
                self.frames
                    .emit(
                        "tool.completed",
                        json!({ "callId": call_id, "name": tool, "output": output }),
                    )
                    .await;
            }
            EngineEvent::Failed(message) => {
                self.reported_failure.get_or_insert(message);
            }
        }
    }
}

/// Reads one LF-terminated line into `line` (without the LF). Bytes past
/// [`MAX_LINE_BYTES`] are dropped and `oversized` is set. `Ok(false)` at EOF
/// with nothing read.
async fn read_line_capped<R: tokio::io::AsyncBufRead + Unpin>(
    reader: &mut R,
    line: &mut Vec<u8>,
    oversized: &mut bool,
) -> std::io::Result<bool> {
    loop {
        let available = reader.fill_buf().await?;
        if available.is_empty() {
            return Ok(!line.is_empty() || *oversized);
        }
        let (chunk, done) = match available.iter().position(|byte| *byte == b'\n') {
            Some(end) => (&available[..end], Some(end + 1)),
            None => (available, None),
        };
        if line.len() + chunk.len() > MAX_LINE_BYTES {
            *oversized = true;
        } else if !*oversized {
            line.extend_from_slice(chunk);
        }
        let consumed = done.unwrap_or(available.len());
        reader.consume(consumed);
        if done.is_some() {
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            return Ok(true);
        }
    }
}

async fn cancelled(cancel: &mut watch::Receiver<bool>) {
    loop {
        if *cancel.borrow_and_update() {
            return;
        }
        if cancel.changed().await.is_err() {
            // Nobody can cancel any more; never resolve.
            std::future::pending::<()>().await;
        }
    }
}

enum SweepSignal {
    Terminate,
    Kill,
}

#[cfg(unix)]
fn signal_group(group: Option<u32>, signal: SweepSignal) {
    let Some(group) = group.and_then(|pid| i32::try_from(pid).ok()) else {
        return;
    };
    if group <= 1 {
        return;
    }
    let signal = match signal {
        SweepSignal::Terminate => libc::SIGTERM,
        SweepSignal::Kill => libc::SIGKILL,
    };
    // SAFETY: killpg only signals; the group id is the child's own pid because
    // it was spawned with `process_group(0)`, so this never reaches Clovy.
    unsafe {
        libc::killpg(group, signal);
    }
}

#[cfg(not(unix))]
fn signal_group(_group: Option<u32>, _signal: SweepSignal) {}

/// SIGTERM to the whole group (the CLI may save its session), then SIGKILL
/// after a short grace period; the child is reaped.
async fn stop_group(child: &mut tokio::process::Child, group: Option<u32>) {
    signal_group(group, SweepSignal::Terminate);
    if tokio::time::timeout(CANCEL_GRACE, child.wait())
        .await
        .is_err()
    {
        let _ = child.start_kill();
    }
    signal_group(group, SweepSignal::Kill);
    let _ = child.wait().await;
}

/// The CLI conversation to resume: the id saved by the latest earlier turn of
/// this session on the same CLI, as long as no turn on another engine came in
/// between (that CLI would not know those messages).
async fn previous_conversation(
    repository: &AgentRepository,
    context: &TurnContext,
) -> Result<Option<String>, sqlx::Error> {
    let engine = engine_model_id(context.kind);
    let rows = sqlx::query::query(
        "SELECT model, run_config_json FROM agent_runs
         WHERE session_id = ? AND id <> ?
         ORDER BY started_at DESC, rowid DESC",
    )
    .bind(&context.session_id)
    .bind(&context.run_id)
    .fetch_all(&repository.pool)
    .await?;
    for row in rows {
        if row.get::<String, _>("model") != engine {
            return Ok(None);
        }
        let Some(config) = row
            .get::<Option<String>, _>("run_config_json")
            .and_then(|config| serde_json::from_str::<Value>(&config).ok())
        else {
            continue;
        };
        if config.get("engine").and_then(Value::as_str) == Some("cli")
            && config.get("cli").and_then(Value::as_str) == Some(context.kind.id())
        {
            if let Some(id) = config.get("conversationId").and_then(Value::as_str) {
                return Ok(Some(id.to_string()));
            }
        }
    }
    Ok(None)
}

/// Saves the CLI's conversation id on this run (once), for the next turn.
async fn save_conversation(
    repository: &AgentRepository,
    context: &TurnContext,
    conversation_id: &str,
) -> bool {
    let config = json!({
        "engine": "cli",
        "cli": context.kind.id(),
        "conversationId": conversation_id,
    });
    match repository.set_run_config(&context.run_id, &config).await {
        Ok(()) => true,
        Err(error) => {
            tracing::warn!(%error, run_id = %context.run_id, "could not save the CLI conversation id");
            false
        }
    }
}

/// A CLI starting a new conversation in a session that already has messages
/// (another engine answered before, or the session was branched) gets them
/// as context ahead of the new message.
async fn with_earlier_messages(repository: &AgentRepository, context: &TurnContext) -> String {
    let items = match repository.items(&context.session_id).await {
        Ok(items) => items,
        Err(error) => {
            tracing::warn!(%error, "could not read earlier session messages");
            return context.input.clone();
        }
    };
    let mut lines = Vec::new();
    for item in items {
        if item.run_id.as_deref() == Some(context.run_id.as_str()) {
            continue;
        }
        match item.payload {
            AgentItemPayload::UserMessage(message) if !message.content.trim().is_empty() => {
                lines.push(format!("User: {}", message.content.trim()));
            }
            AgentItemPayload::AssistantMessage(message) if !message.content.trim().is_empty() => {
                lines.push(format!("Assistant: {}", message.content.trim()));
            }
            AgentItemPayload::ContextSummary(summary) if !summary.text.trim().is_empty() => {
                lines.push(format!(
                    "Summary of earlier messages: {}",
                    summary.text.trim()
                ));
            }
            _ => {}
        }
    }
    if lines.is_empty() {
        return context.input.clone();
    }
    let mut history = lines.join("\n\n");
    if history.chars().count() > MAX_HISTORY_CHARS {
        let skip = history.chars().count() - MAX_HISTORY_CHARS;
        history = format!("[...]{}", history.chars().skip(skip).collect::<String>());
    }
    format!(
        "Earlier messages in this conversation, for context:\n\n{history}\n\nNew message:\n\n{}",
        context.input
    )
}
