//! `chat_engine`: the agent CLIs as the engine of a chat session.
//!
//! A session's engine is part of its model id: `__clovy_cli_engine__:<cli>`
//! runs each message on that CLI ([`cli_for_model`]); the tagged endpoint ids
//! (`__june_local_generation__:<model>`) and Clovy models keep running on the
//! agent sidecar. A CLI turn is one child process per message that resumes
//! the CLI's own conversation, with its JSON stream translated into the same
//! runtime events and `agent_items` the sidecar produces
//! (`agent_runtime::host::persist_and_emit_event`). Cancelling stops the CLI's
//! whole process group; live steering is refused (`steer_agent_run` answers
//! `cli_engine`) so the composer sends the message as the next turn. CLIs that
//! accept a per-run MCP configuration get Clovy's MCP server while it is on.
//!
//! Contract and per-CLI formats: `docs/llm-providers.md` (CLI chat engine)
//! and [`cli`].

#[cfg(all(test, unix))]
#[path = "binding_tests.rs"]
mod binding_tests;
pub mod cli;
pub mod policy;
#[cfg(test)]
#[path = "red_tests.rs"]
mod red_tests;
#[cfg(test)]
mod tests;
pub mod turn;
#[cfg(all(test, windows))]
#[path = "windows_tests.rs"]
mod windows_tests;

use crate::agent_runtime::protocol::RpcFrame;
use crate::agent_runtime::{AgentRepository, AgentSafetyMode};
use crate::domain::types::AppError;
use crate::llm::cli::CliKind;
use serde::Serialize;
use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use tauri::{AppHandle, Manager};
use tokio::sync::{oneshot, watch};

/// Model id prefix of a session that runs on a CLI.
pub const CLI_ENGINE_MODEL_PREFIX: &str = "__clovy_cli_engine__:";

/// The model id of a session on `kind`.
pub fn engine_model_id(kind: CliKind) -> String {
    format!("{CLI_ENGINE_MODEL_PREFIX}{}", kind.id())
}

/// The CLI a session model id runs on, if it is a CLI engine id.
pub fn cli_for_model(model: &str) -> Option<CliKind> {
    model
        .trim()
        .strip_prefix(CLI_ENGINE_MODEL_PREFIX)
        .and_then(CliKind::parse)
}

/// Note references the composer writes into a message (`@note:<id>`, as
/// `noteReferenceToken` makes them), in order and without repeats.
pub fn note_references(message: &str) -> Vec<&str> {
    let mut ids = Vec::new();
    let mut rest = message;
    while let Some(start) = rest.find("@note:") {
        let after = &rest[start + "@note:".len()..];
        let end = after
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_'))
            .unwrap_or(after.len());
        let id = &after[..end];
        if !id.is_empty() && !ids.contains(&id) {
            ids.push(id);
        }
        rest = &after[end..];
    }
    ids
}

/// A CLI does not know what `@note:<id>` means: a message that references
/// notes (the note chat panel always references its note) gets a line that
/// tells it to read them with Clovy's `get_note` tool.
pub fn with_note_reference_context(message: &str) -> String {
    let ids = note_references(message);
    if ids.is_empty() {
        return message.to_string();
    }
    let list = ids
        .iter()
        .map(|id| format!("`{id}`"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "{message}\n\n(`@note:<id>` refers to a Clovy note. Read each referenced note ({list}) with the `get_note` tool of the Clovy MCP server, passing the id, before answering.)"
    )
}

/// Receives a turn's runtime events in order.
pub trait FrameSink: Send + Sync {
    fn publish(&self, frame: RpcFrame) -> impl Future<Output = ()> + Send;
}

/// Persists and emits through the sidecar's event path.
struct AppSink {
    app: AppHandle,
    repository: AgentRepository,
}

impl FrameSink for AppSink {
    async fn publish(&self, frame: RpcFrame) {
        if let Err(error) =
            crate::agent_runtime::host::persist_and_emit_event(&self.app, &self.repository, &frame)
                .await
        {
            tracing::warn!(
                code = %error.code,
                run_id = %frame.run_id,
                method = ?frame.method,
                "failed to persist a CLI chat engine event"
            );
        }
    }
}

/// How long app shutdown waits for CLI turns to stop their process groups
/// (SIGTERM, then SIGKILL after [`turn::CANCEL_GRACE`]).
const SHUTDOWN_WAIT: std::time::Duration = std::time::Duration::from_secs(5);

/// The CLI turns in progress, by run id: cancellation and completion.
#[derive(Default)]
pub struct ChatEngineHost {
    turns: Mutex<HashMap<String, ActiveTurn>>,
    closing: AtomicBool,
}

struct ActiveTurn {
    cancel: watch::Sender<bool>,
    /// Resolves when the turn's task drops its [`TurnTicket`].
    done: oneshot::Receiver<()>,
}

/// Owned by the task that runs one turn: its cancellation signal, and a
/// completion handle that fires when the ticket is dropped.
pub struct TurnTicket {
    pub cancel: watch::Receiver<bool>,
    _done: oneshot::Sender<()>,
}

impl ChatEngineHost {
    /// Takes cancellation ownership of `run_id`. Call it before awaiting
    /// anything for the run, so a cancel that arrives during launch
    /// preparation reaches the turn. Refused once the app is shutting down.
    pub fn register(&self, run_id: &str) -> Result<TurnTicket, AppError> {
        let (cancel, receiver) = watch::channel(false);
        let (done_sender, done) = oneshot::channel();
        let mut turns = self.turns.lock().map_err(|_| {
            AppError::new("agent_cli_unavailable", "The CLI engine is unavailable.")
        })?;
        if self.closing.load(Ordering::Acquire) {
            return Err(AppError::new(
                "agent_cli_shutting_down",
                "Clovy is quitting; the message was not sent.",
            ));
        }
        turns.insert(run_id.to_string(), ActiveTurn { cancel, done });
        Ok(TurnTicket {
            cancel: receiver,
            _done: done_sender,
        })
    }

    pub fn release(&self, run_id: &str) {
        if let Ok(mut turns) = self.turns.lock() {
            turns.remove(run_id);
        }
    }

    /// Stops the turn's process group; the turn then publishes
    /// `run.cancelled`. `false` when no CLI turn runs under `run_id`.
    pub fn cancel(&self, run_id: &str) -> bool {
        self.turns
            .lock()
            .ok()
            .and_then(|turns| turns.get(run_id).map(|turn| turn.cancel.send(true).is_ok()))
            .unwrap_or(false)
    }

    /// App shutdown: refuses new turns, cancels every turn, and waits (up to
    /// [`SHUTDOWN_WAIT`]) until each has stopped its CLI's process group and
    /// published its end.
    pub async fn shutdown(&self) {
        let turns = match self.turns.lock() {
            Ok(mut turns) => {
                self.closing.store(true, Ordering::Release);
                turns.drain().map(|(_, turn)| turn).collect::<Vec<_>>()
            }
            Err(_) => return,
        };
        let mut waiting = Vec::with_capacity(turns.len());
        for turn in turns {
            let _ = turn.cancel.send(true);
            waiting.push(turn.done);
        }
        let all_done = futures_util::future::join_all(waiting);
        if tokio::time::timeout(SHUTDOWN_WAIT, all_done).await.is_err() {
            tracing::warn!("a CLI chat turn did not stop before shutdown");
        }
    }
}

/// Cancels a CLI run (`cancel_agent_run`): settles the run's pending
/// approval requests as `run_cancelled` (their cards turn cancelled and the
/// waiting tools answer the CLI), revokes the run's turn token so its MCP
/// connections close, and stops the CLI's process group. The turn then
/// publishes `run.cancelled`. `false` when no CLI turn runs under `run_id`.
pub async fn cancel_cli_run(
    host: &ChatEngineHost,
    broker: Option<&crate::approval_broker::ApprovalBroker>,
    turns: Option<&crate::mcp_server::turns::TurnRegistry>,
    run_id: &str,
) -> bool {
    if let Some(broker) = broker {
        broker.cancel_run(run_id).await;
    }
    if let Some(turns) = turns {
        turns.revoke_run(run_id);
    }
    host.cancel(run_id)
}

/// A message to run on a CLI engine.
pub struct CliTurnRequest {
    pub session_id: String,
    pub run_id: String,
    pub kind: CliKind,
    pub workspace: PathBuf,
    /// The user's message (with attachment paths already appended).
    pub input: String,
    /// Image attachments of the message (absolute paths in the workspace).
    pub images: Vec<PathBuf>,
    /// The session's safety mode.
    pub safety_mode: AgentSafetyMode,
    /// A person watches the turn: `true` for chat, `false` for routines
    /// (read-only tools, and every approval is refused).
    pub attended: bool,
}

/// Refuses an unattended turn on a CLI whose tools Clovy cannot confine.
pub fn check_policy(kind: CliKind, attended: bool) -> Result<(), AppError> {
    if !attended && !cli::supports_unattended(kind) {
        return Err(AppError::new(
            "agent_cli_unattended_unsupported",
            format!(
                "{} cannot run a routine: Clovy can only limit Claude Code and Codex to read-only tools.",
                kind.display_name()
            ),
        ));
    }
    Ok(())
}

/// Resolves the CLI on the login-shell PATH and runs the turn in the
/// background under `ticket` (from [`ChatEngineHost::register`], taken before
/// any await for the run). Errors before anything is spawned (CLI not
/// installed) are returned so the caller marks the dispatch failed.
pub async fn start_cli_turn(
    app: &AppHandle,
    repository: AgentRepository,
    request: CliTurnRequest,
    ticket: TurnTicket,
) -> Result<(), AppError> {
    check_policy(request.kind, request.attended)?;
    let env = crate::llm::shell_env::login_env().await;
    let program = env
        .which(request.kind.id())
        .ok_or(crate::llm::LlmError::CliNotInstalled(request.kind))?;
    let mcp = if cli::accepts_mcp(request.kind) && crate::activity::mcp_server_enabled(app) {
        match crate::mcp_server::child_process_launch(app) {
            Ok(launch) => Some(launch),
            Err(error) => {
                tracing::warn!(code = %error.code, "CLI chat turn runs without Clovy's MCP server");
                None
            }
        }
    } else {
        None
    };
    let turns = app
        .try_state::<crate::mcp_server::McpServerState>()
        .map(|state| state.turns().clone());
    let context = turn::TurnContext {
        session_id: request.session_id,
        run_id: request.run_id,
        kind: request.kind,
        program,
        env: env.vars().clone(),
        workspace: request.workspace,
        input: request.input,
        mcp,
        turns,
        safety_mode: request.safety_mode,
        attended: request.attended,
        images: request.images,
    };
    let sink = AppSink {
        app: app.clone(),
        repository: repository.clone(),
    };
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let run_id = context.run_id.clone();
        let TurnTicket {
            cancel,
            _done: done,
        } = ticket;
        turn::execute_turn(&repository, &sink, context, cancel).await;
        app.state::<ChatEngineHost>().release(&run_id);
        drop(done);
    });
    Ok(())
}

/// Whether a CLI engine session gets Clovy's tools.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ClovyTools {
    /// The CLI takes Clovy's MCP server per run and the server is on.
    Available,
    /// The CLI takes it, but the server is off in Settings, Agent.
    ServerOff,
    /// The CLI cannot receive it without changing its own configuration.
    Unsupported,
}

pub fn clovy_tools(kind: CliKind, server_on: bool) -> ClovyTools {
    match (cli::accepts_mcp(kind), server_on) {
        (false, _) => ClovyTools::Unsupported,
        (true, true) => ClovyTools::Available,
        (true, false) => ClovyTools::ServerOff,
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CliEngineDto {
    pub id: CliKind,
    pub name: &'static str,
    pub installed: bool,
    pub reason: Option<String>,
    pub model_id: String,
    pub clovy_tools: ClovyTools,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EndpointEngineDto {
    pub id: String,
    pub name: String,
    pub model_id: String,
    /// The tagged model id that routes the Clovy agent to this endpoint.
    pub option_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatEngineCatalogDto {
    pub clis: Vec<CliEngineDto>,
    pub endpoints: Vec<EndpointEngineDto>,
}

/// The engine new chat work starts on (sessions, note chat, Home handoffs,
/// companion sessions; ADR-0062).
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ChatDefaultEngineDto {
    /// The session model id of the CLI chat runs on; `None` when chat is on
    /// the Clovy agent or an endpoint (callers keep their own default).
    pub model_id: Option<String>,
    pub cli: Option<CliKind>,
    /// Chat is on the legacy Clovy agent only because neither Claude Code
    /// nor Codex is installed.
    pub legacy_fallback: bool,
}

pub fn default_engine_of(
    settings: &crate::providers::ProviderModelSettings,
) -> ChatDefaultEngineDto {
    let cli = match &settings.llm_usage.chat {
        crate::llm::registry::ProviderRef::Cli { id } => Some(*id),
        _ => None,
    };
    ChatDefaultEngineDto {
        model_id: cli.map(engine_model_id),
        cli,
        legacy_fallback: crate::llm::registry::cli_default_pending(settings),
    }
}

/// The default chat engine, after the one-time move to the default CLI
/// (`crate::providers::ensure_cli_default`). Needs no network.
#[tauri::command]
pub async fn chat_default_engine(
    state: tauri::State<'_, crate::providers::ProviderSettingsState>,
) -> Result<ChatDefaultEngineDto, AppError> {
    crate::providers::ensure_cli_default(&state).await;
    Ok(default_engine_of(&crate::providers::settings_snapshot(
        &state,
    )?))
}

/// The engines a chat session can use: the six CLIs (installed or not, and
/// whether they get Clovy's tools) and the registered endpoints.
#[tauri::command]
pub async fn chat_engine_catalog(app: AppHandle) -> Result<ChatEngineCatalogDto, AppError> {
    let server_on = crate::activity::mcp_server_enabled(&app)
        && crate::mcp_server::child_process_launch(&app).is_ok();
    let env = crate::llm::shell_env::login_env().await;
    let detected = crate::llm::detect::detect_all(&env).await;
    let clis = detected
        .into_iter()
        .map(|cli| CliEngineDto {
            id: cli.id,
            name: cli.id.display_name(),
            installed: cli.installed,
            reason: cli.reason,
            model_id: engine_model_id(cli.id),
            clovy_tools: clovy_tools(cli.id, server_on),
        })
        .collect();
    Ok(ChatEngineCatalogDto {
        clis,
        endpoints: endpoint_engines(&crate::llm::registry()),
    })
}

/// One entry per registered endpoint. The option id names the endpoint as
/// well as its model, so two endpoints serving the same model stay distinct
/// through the picker, the session model, and the agent proxy.
pub fn endpoint_engines(registry: &crate::llm::registry::LlmRegistry) -> Vec<EndpointEngineDto> {
    registry
        .endpoints
        .iter()
        .filter(|endpoint| !endpoint.model_id.trim().is_empty())
        .map(|endpoint| EndpointEngineDto {
            option_id: crate::clovy_api::endpoint_option_id(&endpoint.model_id, &endpoint.id),
            id: endpoint.id.clone(),
            name: endpoint.name.clone(),
            model_id: endpoint.model_id.clone(),
        })
        .collect()
}
