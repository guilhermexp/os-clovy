//! Coding-agent session ingestion: brings the work done with local coding
//! agents (what screen capture cannot see) into the encrypted activity
//! database as summarized blocks. See `docs/coding-agent-sessions.md`.
//!
//! Pipeline, per scan (every minute, or right after the settings change):
//! `sources` read each enabled agent's transcripts read-only → `record`
//! normalizes them → `ingest` drops Clovy's own CLI calls and `segment` cuts
//! blocks (idle over 1 h, or 1 h long cut at a prompt) → `store` upserts them
//! (`live` → `sealed`) → `summarize` writes summaries with the agent's own CLI
//! or the activity provider (`sealed` → `summarized`).

pub mod ingest;
pub mod record;
#[cfg(test)]
#[path = "red_tests.rs"]
mod red_tests;
pub mod segment;
pub mod settings;
pub mod sources;
pub mod store;
pub mod summarize;

use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use chrono::{DateTime, Days, Local, NaiveTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::Notify;

use crate::activity::engine::ActivityShared;
use crate::domain::types::AppError;
use crate::llm::cli::CliKind;
use sources::SourceRoots;
use store::CodingAgentBlock;

/// Emitted (no payload) when blocks or summaries changed.
pub const CODING_AGENTS_UPDATED_EVENT: &str = "clovy://coding-agents-updated";
const SCAN_INTERVAL: Duration = Duration::from_secs(60);
/// While a source is on but the activity database is still opening.
const STORE_RETRY: Duration = Duration::from_secs(3);
/// Next drain right away when more summaries are due.
const DRAIN_AGAIN: Duration = Duration::from_secs(1);
const PRUNE_EVERY: Duration = Duration::from_secs(60 * 60);
/// History read on first sight: today and yesterday (local days).
pub const BACKFILL_DAYS: u64 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceId {
    ClaudeCode,
    Codex,
    CopilotCli,
    CopilotVscode,
    Cursor,
    CursorCli,
    Antigravity,
}

impl SourceId {
    pub const ALL: [SourceId; 7] = [
        SourceId::ClaudeCode,
        SourceId::Codex,
        SourceId::CopilotCli,
        SourceId::CopilotVscode,
        SourceId::Cursor,
        SourceId::CursorCli,
        SourceId::Antigravity,
    ];

    /// Stored identifier (the `source` column).
    pub fn as_str(self) -> &'static str {
        match self {
            SourceId::ClaudeCode => "claude_code",
            SourceId::Codex => "codex",
            SourceId::CopilotCli => "copilot_cli",
            SourceId::CopilotVscode => "copilot_vscode",
            SourceId::Cursor => "cursor",
            SourceId::CursorCli => "cursor_cli",
            SourceId::Antigravity => "antigravity",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|source| source.as_str() == value)
    }

    pub fn display_name(self) -> &'static str {
        match self {
            SourceId::ClaudeCode => "Claude Code",
            SourceId::Codex => "Codex",
            SourceId::CopilotCli => "Copilot CLI",
            SourceId::CopilotVscode => "Copilot in VS Code",
            SourceId::Cursor => "Cursor",
            SourceId::CursorCli => "Cursor CLI",
            SourceId::Antigravity => "Antigravity",
        }
    }

    /// The CLI that summarizes this agent's blocks.
    pub fn own_cli(self) -> CliKind {
        match self {
            SourceId::ClaudeCode => CliKind::Claude,
            SourceId::Codex => CliKind::Codex,
            SourceId::CopilotCli | SourceId::CopilotVscode => CliKind::Copilot,
            SourceId::Cursor | SourceId::CursorCli => CliKind::CursorAgent,
            SourceId::Antigravity => CliKind::Agy,
        }
    }
}

#[derive(Clone, Debug, Default)]
struct ScanStatus {
    last_scan_at: Option<String>,
    last_error: Option<String>,
}

/// Tauri-managed handle. Without activity support (non-macOS) or a home
/// directory, the loop never starts and the commands report `supported: false`.
#[derive(Default)]
pub struct CodingAgentsState {
    runtime: Option<Runtime>,
}

struct Runtime {
    activity: Arc<ActivityShared>,
    roots: SourceRoots,
    wake: Arc<Notify>,
    status: Arc<Mutex<ScanStatus>>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceStatusDto {
    pub id: SourceId,
    pub name: &'static str,
    pub enabled: bool,
    /// The agent's data directory exists on this Mac.
    pub present: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodingAgentsStatusDto {
    pub supported: bool,
    pub sources: Vec<SourceStatusDto>,
    /// The activity database is open (blocks can be stored and read).
    pub database_ready: bool,
    pub last_scan_at: Option<String>,
    pub last_error: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodingAgentBlocksRequest {
    pub from: DateTime<Utc>,
    pub to: DateTime<Utc>,
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[tauri::command]
pub fn coding_agents_status(state: State<'_, CodingAgentsState>) -> CodingAgentsStatusDto {
    let Some(runtime) = state.runtime.as_ref() else {
        return CodingAgentsStatusDto {
            supported: false,
            sources: Vec::new(),
            database_ready: false,
            last_scan_at: None,
            last_error: None,
        };
    };
    let enabled = runtime.activity.settings().coding_agents;
    let status = lock(&runtime.status).clone();
    CodingAgentsStatusDto {
        supported: true,
        sources: SourceId::ALL
            .into_iter()
            .map(|source| SourceStatusDto {
                id: source,
                name: source.display_name(),
                enabled: enabled.is_enabled(source),
                present: runtime.roots.is_present(source),
            })
            .collect(),
        database_ready: runtime.activity.store().is_some(),
        last_scan_at: status.last_scan_at,
        last_error: status.last_error,
    }
}

/// Blocks overlapping `[from, to)`, oldest first. Empty while the activity
/// database is not open.
#[tauri::command]
pub async fn coding_agents_blocks(
    state: State<'_, CodingAgentsState>,
    request: CodingAgentBlocksRequest,
) -> Result<Vec<CodingAgentBlock>, AppError> {
    let Some(store) = state
        .runtime
        .as_ref()
        .and_then(|runtime| runtime.activity.store())
    else {
        return Ok(Vec::new());
    };
    store
        .coding_agent_blocks_between(request.from, request.to)
        .await
        .map_err(|error| AppError::new("coding_agents_read_failed", error.to_string()))
}

/// Runs a scan now (after the sources were changed in settings).
pub fn wake(app: &AppHandle) {
    if let Some(runtime) = app.state::<CodingAgentsState>().runtime.as_ref() {
        runtime.wake.notify_one();
    }
}

/// Registers the managed state and starts the scan loop. Must run after
/// `activity::setup`, whose database this module shares.
pub fn setup(app: &mut tauri::App) {
    let activity = app.state::<crate::activity::ActivityState>().shared();
    let runtime = activity
        .zip(SourceRoots::current())
        .map(|(activity, roots)| Runtime {
            activity,
            roots,
            wake: Arc::new(Notify::new()),
            status: Arc::new(Mutex::new(ScanStatus::default())),
        });
    if let Some(runtime) = &runtime {
        tauri::async_runtime::spawn(run(
            app.handle().clone(),
            Arc::clone(&runtime.activity),
            runtime.roots.clone(),
            Arc::clone(&runtime.wake),
            Arc::clone(&runtime.status),
        ));
    }
    app.manage(CodingAgentsState { runtime });
}

/// Local midnight `BACKFILL_DAYS - 1` days before `now`.
pub fn window_start(now: DateTime<Local>) -> DateTime<Utc> {
    let day = now
        .date_naive()
        .checked_sub_days(Days::new(BACKFILL_DAYS - 1))
        .unwrap_or(now.date_naive());
    Local
        .from_local_datetime(&day.and_time(NaiveTime::MIN))
        .earliest()
        .map_or_else(
            || now.with_timezone(&Utc),
            |start| start.with_timezone(&Utc),
        )
}

async fn run(
    app: AppHandle,
    activity: Arc<ActivityShared>,
    roots: SourceRoots,
    wake: Arc<Notify>,
    status: Arc<Mutex<ScanStatus>>,
) {
    let mut scanner = ingest::Scanner::new(roots);
    let mut last_prune: Option<Instant> = None;
    loop {
        let settings = activity.settings();
        let mut wait = SCAN_INTERVAL;
        if settings.coding_agents.any_enabled() {
            match activity.store() {
                None => wait = STORE_RETRY,
                Some(store) => {
                    let now = Utc::now();
                    let window = window_start(Local::now());
                    let mut changed = false;
                    match scanner
                        .scan(&store, &settings.coding_agents, now, window)
                        .await
                    {
                        Ok(report) => {
                            changed |= report.changed();
                            *lock(&status) = ScanStatus {
                                last_scan_at: Some(crate::activity::store::timestamp(now)),
                                last_error: None,
                            };
                        }
                        Err(error) => {
                            tracing::warn!(%error, "coding agents: scan failed");
                            lock(&status).last_error = Some(error.to_string());
                        }
                    }
                    match summarize::drain(
                        &store,
                        &summarize::LlmBackend,
                        &settings.coding_agents,
                        crate::interface_locale::current(),
                        window,
                        Utc::now,
                    )
                    .await
                    {
                        Ok(report) => {
                            changed |= report.summarized + report.failed > 0;
                            if report.more {
                                wait = DRAIN_AGAIN;
                            }
                        }
                        Err(error) => tracing::warn!(%error, "coding agents: summaries failed"),
                    }
                    if !last_prune.is_some_and(|at| at.elapsed() < PRUNE_EVERY) {
                        let before =
                            now - chrono::Duration::days(i64::from(settings.retention_days));
                        if let Err(error) = store.prune_coding_agent_blocks(before).await {
                            tracing::warn!(%error, "coding agents: retention failed");
                        }
                        last_prune = Some(Instant::now());
                    }
                    if changed {
                        let _ = app.emit(CODING_AGENTS_UPDATED_EVENT, ());
                    }
                }
            }
        }
        tokio::select! {
            _ = wake.notified() => {}
            _ = tokio::time::sleep(wait) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_starts_at_local_midnight_of_yesterday() {
        let now = Local.with_ymd_and_hms(2026, 10, 4, 15, 30, 0).unwrap();
        let start = window_start(now).with_timezone(&Local);
        assert_eq!(
            start.naive_local(),
            chrono::NaiveDate::from_ymd_opt(2026, 10, 3)
                .unwrap()
                .and_time(NaiveTime::MIN)
        );
    }

    #[test]
    fn source_ids_round_trip() {
        for source in SourceId::ALL {
            assert_eq!(SourceId::parse(source.as_str()), Some(source));
            assert_eq!(
                serde_json::to_value(source).unwrap(),
                serde_json::Value::String(source.as_str().to_string())
            );
        }
    }
}
