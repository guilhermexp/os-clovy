//! The Clovy MCP server: lets local MCP clients (Claude Code, Cursor, and the
//! CLI chat engines) read the current data partition's notes, dictations, and
//! memories, and the installation's activity. Off by default; the switch is `mcpServer` in
//! `activity-settings.json`, changed only from Settings, Agent. Contract and
//! protocol: `docs/mcp-server.md`.
//!
//! Layout: `channel` (Unix socket + installation secret), `stdio` (the
//! `clovy-mcp` relay binary), `protocol` (MCP JSON-RPC and resources),
//! `tools` (the read-only tools). The binary never opens a database: it
//! relays to this process, which answers with the same queries and filters
//! the app uses.

pub mod channel;
pub mod protocol;
#[cfg(test)]
#[path = "red_tests.rs"]
mod red_tests;
pub mod stdio;
#[cfg(test)]
mod tests;
pub mod tools;

use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

use crate::domain::types::AppError;
use tools::{ActivityAccess, McpData};

/// The relay binary (`[[bin]]` in Cargo.toml, bundled under
/// `Resources/native/bin/`).
pub const BINARY_NAME: &str = "clovy-mcp";
/// The server's key in `mcpServers` configuration objects.
pub const SERVER_KEY: &str = "clovy";

/// The command an MCP client runs to reach Clovy. Needs no environment.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpLaunch {
    pub command: PathBuf,
    pub args: Vec<String>,
}

pub fn launch(binary: &Path, channel_dir: &Path) -> McpLaunch {
    McpLaunch {
        command: binary.to_path_buf(),
        args: vec!["--dir".into(), channel_dir.display().to_string()],
    }
}

/// `{"mcpServers": {"clovy": {...}}}` as Claude Code reads it (`.mcp.json`,
/// `--mcp-config`); also what CLI chat engines get for a child process.
pub fn claude_code_config(launch: &McpLaunch) -> Value {
    json!({
        "mcpServers": {
            SERVER_KEY: {
                "type": "stdio",
                "command": launch.command.display().to_string(),
                "args": launch.args,
            }
        }
    })
}

/// `~/.cursor/mcp.json` (or a project's `.cursor/mcp.json`).
pub fn cursor_config(launch: &McpLaunch) -> Value {
    json!({
        "mcpServers": {
            SERVER_KEY: {
                "command": launch.command.display().to_string(),
                "args": launch.args,
            }
        }
    })
}

fn shell_quote(value: &str) -> String {
    if !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"/._-=:@%+,".contains(&byte))
    {
        return value.to_string();
    }
    format!("'{}'", value.replace('\'', r"'\''"))
}

/// `claude mcp add` for the user scope (every project).
pub fn claude_code_command(launch: &McpLaunch) -> String {
    let mut parts = vec![
        "claude".to_string(),
        "mcp".into(),
        "add".into(),
        "--scope".into(),
        "user".into(),
        SERVER_KEY.into(),
        "--".into(),
        shell_quote(&launch.command.display().to_string()),
    ];
    parts.extend(launch.args.iter().map(|arg| shell_quote(arg)));
    parts.join(" ")
}

/// Where the bundled or freshly built relay lives, first existing first.
fn binary_candidates(resource_dir: Option<&Path>, exe: Option<&Path>) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(resources) = resource_dir {
        candidates.push(resources.join("native").join("bin").join(BINARY_NAME));
    }
    if let Some(exe_dir) = exe.and_then(Path::parent) {
        candidates.push(exe_dir.join(BINARY_NAME));
    }
    candidates
}

fn resolve_binary(app: &AppHandle) -> (PathBuf, bool) {
    let resource_dir = app.path().resource_dir().ok();
    let exe = std::env::current_exe().ok();
    let candidates = binary_candidates(resource_dir.as_deref(), exe.as_deref());
    match candidates.iter().find(|path| path.is_file()) {
        Some(found) => (
            dunce::canonicalize(found).unwrap_or_else(|_| found.clone()),
            true,
        ),
        None => (
            candidates
                .into_iter()
                .next()
                .unwrap_or_else(|| PathBuf::from(BINARY_NAME)),
            false,
        ),
    }
}

fn channel_dir(app: &AppHandle) -> Result<PathBuf, AppError> {
    crate::app_paths::app_data_dir(app)
        .map(|data_dir| channel::channel_dir(&data_dir))
        .map_err(|error| AppError::new("mcp_server_paths", error.to_string()))
}

/// The launch command for a child process (the CLI chat engine passes
/// `claude_code_config(&launch)` to CLIs that accept MCP configuration).
/// The server must be on for its tools to answer.
pub fn child_process_launch(app: &AppHandle) -> Result<McpLaunch, AppError> {
    let (binary, found) = resolve_binary(app);
    if !found {
        return Err(AppError::new(
            "mcp_server_binary_missing",
            format!("The {BINARY_NAME} executable was not found next to Clovy."),
        ));
    }
    Ok(launch(&binary, &channel_dir(app)?))
}

/// The running listener and the last start error, behind an async lock so
/// start and stop never interleave.
#[derive(Default)]
pub struct McpServerState(tokio::sync::Mutex<Slot>);

#[derive(Default)]
struct Slot {
    #[cfg(unix)]
    listener: Option<channel::Listener>,
    error: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerStatusDto {
    /// macOS with activity settings available.
    pub supported: bool,
    pub enabled: bool,
    pub running: bool,
    pub error: Option<String>,
    pub binary_path: String,
    pub binary_found: bool,
    pub claude_code_command: String,
    pub claude_code_config: String,
    pub cursor_config: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetMcpServerEnabledRequest {
    pub enabled: bool,
}

fn supported(app: &AppHandle) -> bool {
    cfg!(target_os = "macos") && crate::activity::activity_settings_supported(app)
}

async fn status(app: &AppHandle) -> Result<McpServerStatusDto, AppError> {
    let (binary, binary_found) = resolve_binary(app);
    let launch = launch(&binary, &channel_dir(app)?);
    let state = app.state::<McpServerState>();
    let slot = state.0.lock().await;
    #[cfg(unix)]
    let running = slot.listener.is_some();
    #[cfg(not(unix))]
    let running = false;
    let pretty = |value: Value| serde_json::to_string_pretty(&value).unwrap_or_default();
    Ok(McpServerStatusDto {
        supported: supported(app),
        enabled: crate::activity::mcp_server_enabled(app),
        running,
        error: slot.error.clone(),
        binary_path: binary.display().to_string(),
        binary_found,
        claude_code_command: claude_code_command(&launch),
        claude_code_config: pretty(claude_code_config(&launch)),
        cursor_config: pretty(cursor_config(&launch)),
    })
}

/// Starts or stops the listener to match the stored switch.
async fn reconcile(app: &AppHandle) {
    let enabled = crate::activity::mcp_server_enabled(app);
    let state = app.state::<McpServerState>();
    let mut slot = state.0.lock().await;
    #[cfg(unix)]
    {
        if enabled && slot.listener.is_none() {
            let started = match channel_dir(app) {
                Ok(dir) => channel::Listener::start(&dir, app_handler(app.clone()))
                    .await
                    .map_err(|error| error.to_string()),
                Err(error) => Err(error.message),
            };
            match started {
                Ok(listener) => {
                    tracing::info!(socket = %listener.socket().display(), "Clovy MCP server listening");
                    slot.listener = Some(listener);
                    slot.error = None;
                }
                Err(error) => {
                    tracing::warn!(%error, "Clovy MCP server could not start");
                    slot.error = Some(error);
                }
            }
        } else if !enabled {
            if let Some(listener) = slot.listener.take() {
                listener.stop();
            }
            slot.error = None;
        }
    }
    #[cfg(not(unix))]
    {
        let _ = enabled;
        slot.error = None;
    }
}

/// Registers the state and starts the listener when the switch is on.
pub fn setup(app: &mut tauri::App) {
    app.manage(McpServerState::default());
    let handle = app.handle().clone();
    tauri::async_runtime::spawn(async move { reconcile(&handle).await });
}

#[tauri::command]
pub async fn mcp_server_status(app: AppHandle) -> Result<McpServerStatusDto, AppError> {
    status(&app).await
}

#[tauri::command]
pub async fn mcp_server_set_enabled(
    app: AppHandle,
    request: SetMcpServerEnabledRequest,
) -> Result<McpServerStatusDto, AppError> {
    if !supported(&app) {
        return Err(AppError::new(
            "mcp_server_unsupported",
            "The Clovy MCP server is available on macOS only.",
        ));
    }
    crate::activity::set_mcp_server_enabled(&app, request.enabled)?;
    reconcile(&app).await;
    status(&app).await
}

#[cfg(unix)]
fn app_handler(app: AppHandle) -> channel::Handler {
    std::sync::Arc::new(move |message: Value| {
        let app = app.clone();
        Box::pin(async move {
            match app_data(&app, &message).await {
                Ok(data) => protocol::handle(&data, &message).await,
                Err(error) => protocol::request_id(&message).map(|id| {
                    protocol::error_response(id, protocol::SERVER_UNAVAILABLE, error.message)
                }),
            }
        })
    })
}

/// What `message` may read: the current data partition's main database, the memory
/// switch, and the activity store when the message calls an activity tool.
async fn app_data(app: &AppHandle, message: &Value) -> Result<McpData, AppError> {
    let notes = crate::commands::repositories(app).await?.pool;
    let profile = crate::commands::active_profile(app);
    let memory_enabled = crate::commands::memory_settings_path(app)
        .map(|path| crate::commands::load_memory_settings(&path).enabled)
        .unwrap_or(false);
    let activity = if !crate::activity::timeline::capture_enabled(app) {
        ActivityAccess::Off
    } else if protocol::calls_activity_tool(message) {
        match crate::activity::timeline::store_for_tools(app).await {
            Ok((store, settings)) => ActivityAccess::Open { store, settings },
            Err(error) => ActivityAccess::Unavailable(error),
        }
    } else {
        ActivityAccess::On
    };
    Ok(McpData {
        notes,
        profile,
        memory_enabled,
        activity,
        now: Utc::now(),
    })
}
