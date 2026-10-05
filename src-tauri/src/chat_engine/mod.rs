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

pub mod cli;
#[cfg(test)]
#[path = "red_tests.rs"]
mod red_tests;
#[cfg(test)]
mod tests;
pub mod turn;

use crate::agent_runtime::protocol::RpcFrame;
use crate::agent_runtime::AgentRepository;
use crate::domain::types::AppError;
use crate::llm::cli::CliKind;
use serde::Serialize;
use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{AppHandle, Manager};
use tokio::sync::watch;

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

/// The CLI turns in progress, by run id, for cancellation.
#[derive(Default)]
pub struct ChatEngineHost {
    turns: Mutex<HashMap<String, watch::Sender<bool>>>,
}

impl ChatEngineHost {
    fn register(&self, run_id: &str) -> watch::Receiver<bool> {
        let (sender, receiver) = watch::channel(false);
        if let Ok(mut turns) = self.turns.lock() {
            turns.insert(run_id.to_string(), sender);
        }
        receiver
    }

    fn release(&self, run_id: &str) {
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
            .and_then(|turns| turns.get(run_id).map(|sender| sender.send(true).is_ok()))
            .unwrap_or(false)
    }
}

/// A message to run on a CLI engine.
pub struct CliTurnRequest {
    pub session_id: String,
    pub run_id: String,
    pub kind: CliKind,
    pub workspace: PathBuf,
    /// The user's message (with attachment paths already appended).
    pub input: String,
}

/// Resolves the CLI on the login-shell PATH and runs the turn in the
/// background. Errors before anything is spawned (CLI not installed) are
/// returned so the caller marks the dispatch failed.
pub async fn start_cli_turn(
    app: &AppHandle,
    repository: AgentRepository,
    request: CliTurnRequest,
) -> Result<(), AppError> {
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
    let context = turn::TurnContext {
        session_id: request.session_id,
        run_id: request.run_id,
        kind: request.kind,
        program,
        env: env.vars().clone(),
        workspace: request.workspace,
        input: request.input,
        mcp,
    };
    let host = app.state::<ChatEngineHost>();
    let cancel = host.register(&context.run_id);
    let sink = AppSink {
        app: app.clone(),
        repository: repository.clone(),
    };
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let run_id = context.run_id.clone();
        turn::execute_turn(&repository, &sink, context, cancel).await;
        app.state::<ChatEngineHost>().release(&run_id);
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
    let endpoints = crate::llm::registry()
        .endpoints
        .into_iter()
        .filter(|endpoint| !endpoint.model_id.trim().is_empty())
        .map(|endpoint| EndpointEngineDto {
            option_id: format!(
                "__june_local_generation__:{}",
                urlencoding::encode(endpoint.model_id.trim())
            ),
            id: endpoint.id,
            name: endpoint.name,
            model_id: endpoint.model_id,
        })
        .collect();
    Ok(ChatEngineCatalogDto { clis, endpoints })
}
