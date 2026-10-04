//! The activity timeline: an incremental ETL from captured frames to app
//! sessions (with browser domain / editor workspace context), classified gaps,
//! deterministic categories, day statistics, and FTS5 search, all in the
//! encrypted activity database. Read by the "Today" view and the agent tools.
//! Contract and schema: `docs/activity-timeline.md`.
//!
//! Layout: `builder` (pure frames → events state machine), `context`,
//! `categorize`, `stats` (pure), `db` (tables, persistence, queries), `etl`
//! (one pass), `agent_tools` (`search_activity`, `get_activity_timeline`).

pub mod agent_tools;
pub mod builder;
pub mod categorize;
pub mod context;
pub mod db;
pub mod etl;
#[cfg(test)]
#[path = "red_tests.rs"]
mod red_tests;
pub mod stats;

use std::sync::{Arc, LazyLock};
use std::time::Duration as StdDuration;

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, State};

use super::engine::ActivityShared;
use super::filter::{app_is_ignored, url_matches_domains};
use super::schedule::{CaptureState, StoreStatus};
use super::settings::ActivitySettings;
use super::store::{ActivityStore, StoreError};
use super::{ActivityRuntime, ActivityState};
use crate::domain::types::AppError;
use context::ContextKind;
use db::{
    Interval, TimelineGapDto, TimelineSearchResultDto, TimelineSessionDetailDto, TimelineSessionDto,
};
use stats::TimelineStatsDto;

/// Emitted after an ETL pass wrote sessions or gaps.
pub const TIMELINE_EVENT: &str = "clovy://activity-timeline";
/// How often the background pass runs (the view also triggers one).
const PASS_INTERVAL: StdDuration = StdDuration::from_secs(30);
const DEFAULT_SEARCH_LIMIT: u32 = 50;
const MAX_SEARCH_LIMIT: u32 = 200;
/// Recordings older than this before the first unprocessed frame cannot
/// overlap a session the pass will touch.
const MEETING_LOOKBACK: Duration = Duration::days(2);

/// Serializes passes from the background loop, commands, and agent tools.
static PASS_LOCK: LazyLock<tokio::sync::Mutex<()>> = LazyLock::new(|| tokio::sync::Mutex::new(()));

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Availability {
    Ready,
    NeverEnabled,
    Unsupported,
    KeyMissing,
    Error,
}

/// Sessions, gaps, and stats of a range after exclusions and retention.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TimelineView {
    pub sessions: Vec<TimelineSessionDto>,
    pub gaps: Vec<TimelineGapDto>,
    pub stats: TimelineStatsDto,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityTimelineDto {
    pub availability: Availability,
    pub message: Option<String>,
    pub capture_enabled: bool,
    pub sessions: Vec<TimelineSessionDto>,
    pub gaps: Vec<TimelineGapDto>,
    pub stats: TimelineStatsDto,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityTimelineSearchDto {
    pub availability: Availability,
    pub results: Vec<TimelineSearchResultDto>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineRangeRequest {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineSessionRequest {
    pub id: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineSearchRequest {
    pub query: String,
    pub from: Option<String>,
    pub to: Option<String>,
    pub limit: Option<u32>,
}

/// A session is hidden while the user's current exclusions match its app or
/// its domain: removing something from capture also removes its history
/// from the view and the agent.
fn session_excluded(
    app_name: &str,
    bundle_id: Option<&str>,
    domain: Option<&str>,
    url: Option<&str>,
    settings: &ActivitySettings,
) -> bool {
    app_is_ignored(app_name, bundle_id, &settings.ignored_apps)
        || domain.is_some_and(|domain| {
            url_matches_domains(&format!("https://{domain}/"), &settings.ignored_domains)
        })
        || url.is_some_and(|url| url_matches_domains(url, &settings.ignored_domains))
}

fn retention_floor(settings: &ActivitySettings, now: DateTime<Utc>) -> DateTime<Utc> {
    now - Duration::days(i64::from(settings.retention_days))
}

pub async fn timeline_view(
    store: &ActivityStore,
    settings: &ActivitySettings,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    now: DateTime<Utc>,
) -> Result<TimelineView, StoreError> {
    let from = from.max(retention_floor(settings, now));
    if to <= from {
        return Ok(TimelineView::default());
    }
    let sessions: Vec<TimelineSessionDto> = db::sessions_between(store, from, to)
        .await?
        .into_iter()
        .filter(|session| {
            let domain = (session.context_kind == Some(ContextKind::Domain))
                .then_some(session.context.as_deref())
                .flatten();
            !session_excluded(
                &session.app_name,
                session.bundle_id.as_deref(),
                domain,
                None,
                settings,
            )
        })
        .collect();
    let gaps = db::gaps_between(store, from, to).await?;
    let stats = stats::day_stats(&sessions, &gaps, from, to);
    Ok(TimelineView {
        sessions,
        gaps,
        stats,
    })
}

pub async fn search_view(
    store: &ActivityStore,
    settings: &ActivitySettings,
    query: &str,
    from: Option<DateTime<Utc>>,
    to: Option<DateTime<Utc>>,
    limit: u32,
    now: DateTime<Utc>,
) -> Result<Vec<TimelineSearchResultDto>, StoreError> {
    let floor = retention_floor(settings, now);
    let from = from.map_or(floor, |from| from.max(floor));
    let to = to.unwrap_or(now + Duration::days(1));
    if to <= from {
        return Ok(Vec::new());
    }
    // Over-fetch so excluded rows do not starve the page.
    let rows = db::search(store, query, from, to, limit.saturating_mul(3)).await?;
    Ok(rows
        .into_iter()
        .filter(|row| {
            !session_excluded(
                &row.app_name,
                row.bundle_id.as_deref(),
                None,
                row.browser_url.as_deref(),
                settings,
            )
        })
        .take(limit as usize)
        .collect())
}

fn parse_request_time(value: &str, field: &str) -> Result<DateTime<Utc>, AppError> {
    builder::parse_time(value).ok_or_else(|| {
        AppError::new(
            "activity_timeline_invalid_range",
            format!("`{field}` must be an RFC 3339 timestamp"),
        )
    })
}

/// How the timeline can be read right now.
enum StoreAccess {
    Open(ActivityStore),
    /// Capture is on but its database is not open yet: an empty, ready view.
    Empty,
    Unavailable(Availability, Option<String>),
}

async fn store_access(runtime: &ActivityRuntime) -> StoreAccess {
    if let Some(store) = runtime.shared.store() {
        return StoreAccess::Open(store);
    }
    let settings = runtime.shared.settings();
    if !runtime.db_path.exists() {
        return if settings.enabled {
            StoreAccess::Empty
        } else {
            StoreAccess::Unavailable(Availability::NeverEnabled, None)
        };
    }
    if settings.enabled {
        // The capture thread owns opening while capture is on.
        return match runtime.shared.state() {
            CaptureState::KeyMissing => StoreAccess::Unavailable(Availability::KeyMissing, None),
            CaptureState::Error { message } => {
                StoreAccess::Unavailable(Availability::Error, Some(message))
            }
            _ => StoreAccess::Empty,
        };
    }
    // Capture is off but history exists: open it read-mostly for the view.
    match ActivityStore::open(&runtime.db_path, runtime.keys.as_ref()).await {
        Ok(store) => {
            runtime
                .shared
                .replace_store(Some(store.clone()), StoreStatus::Ready);
            StoreAccess::Open(store)
        }
        Err(StoreError::KeyMissing | StoreError::KeyRejected) => {
            StoreAccess::Unavailable(Availability::KeyMissing, None)
        }
        Err(error) => StoreAccess::Unavailable(Availability::Error, Some(error.to_string())),
    }
}

/// Recording intervals from the main database: the categorizer's evidence of
/// meeting audio.
async fn meeting_intervals(app: &AppHandle, since: DateTime<Utc>) -> Vec<Interval> {
    let Ok(repositories) = crate::commands::repositories(app).await else {
        return Vec::new();
    };
    let rows = sqlx::query::query(
        "SELECT started_at, ended_at FROM recording_sessions ORDER BY started_at DESC LIMIT 500",
    )
    .fetch_all(&repositories.pool)
    .await
    .unwrap_or_default();
    let now = Utc::now();
    rows.iter()
        .filter_map(|row| {
            use sqlx::row::Row;
            let start = builder::parse_time(row.try_get::<&str, _>(0).ok()?)?;
            let end = row
                .try_get::<Option<&str>, _>(1)
                .ok()
                .flatten()
                .and_then(builder::parse_time)
                .unwrap_or(now);
            (end >= since).then_some((start, end))
        })
        .collect()
}

/// Runs one ETL pass (serialized) and tells the frontend when it changed
/// something.
pub async fn run_pass(
    app: &AppHandle,
    store: &ActivityStore,
) -> Result<etl::PassReport, StoreError> {
    let _guard = PASS_LOCK.lock().await;
    let since = db::load_state(store)
        .await?
        .builder
        .open
        .map(|open| open.started_at)
        .unwrap_or_else(Utc::now)
        .min(Utc::now())
        - MEETING_LOOKBACK;
    let meetings = meeting_intervals(app, since).await;
    let report = etl::run_pass(store, Utc::now(), &meetings).await?;
    if report.changed {
        let _ = app.emit(TIMELINE_EVENT, serde_json::json!({ "changed": true }));
    }
    Ok(report)
}

/// Background loop: a pass every 30 s while the store is open, so the
/// cursor (and with it retention) keeps moving without the view open.
pub fn start(app: AppHandle, shared: Arc<ActivityShared>) {
    tauri::async_runtime::spawn(async move {
        loop {
            if let Some(store) = shared.store() {
                if let Err(error) = run_pass(&app, &store).await {
                    tracing::warn!(%error, "activity timeline pass failed");
                }
            }
            tokio::time::sleep(PASS_INTERVAL).await;
        }
    });
}

fn runtime_of<'a>(state: &'a State<'_, ActivityState>) -> Option<&'a ActivityRuntime> {
    state.0.as_ref()
}

fn empty_timeline(
    availability: Availability,
    message: Option<String>,
    capture_enabled: bool,
) -> ActivityTimelineDto {
    ActivityTimelineDto {
        availability,
        message,
        capture_enabled,
        sessions: Vec::new(),
        gaps: Vec::new(),
        stats: TimelineStatsDto::default(),
    }
}

/// The "Today" view's read path: sessions, gaps, and stats overlapping
/// `[from, to)`, after a fresh pass.
#[tauri::command]
pub async fn activity_timeline(
    app: AppHandle,
    state: State<'_, ActivityState>,
    request: TimelineRangeRequest,
) -> Result<ActivityTimelineDto, AppError> {
    let from = parse_request_time(&request.from, "from")?;
    let to = parse_request_time(&request.to, "to")?;
    let Some(runtime) = runtime_of(&state) else {
        return Ok(empty_timeline(Availability::Unsupported, None, false));
    };
    let settings = runtime.shared.settings();
    let store = match store_access(runtime).await {
        StoreAccess::Open(store) => store,
        StoreAccess::Empty => {
            return Ok(empty_timeline(Availability::Ready, None, settings.enabled))
        }
        StoreAccess::Unavailable(availability, message) => {
            return Ok(empty_timeline(availability, message, settings.enabled))
        }
    };
    if let Err(error) = run_pass(&app, &store).await {
        tracing::warn!(%error, "activity timeline pass failed before a read");
    }
    match timeline_view(&store, &settings, from, to, Utc::now()).await {
        Ok(view) => Ok(ActivityTimelineDto {
            availability: Availability::Ready,
            message: None,
            capture_enabled: settings.enabled,
            sessions: view.sessions,
            gaps: view.gaps,
            stats: view.stats,
        }),
        Err(error) => Ok(empty_timeline(
            Availability::Error,
            Some(error.to_string()),
            settings.enabled,
        )),
    }
}

#[tauri::command]
pub async fn activity_timeline_session(
    state: State<'_, ActivityState>,
    request: TimelineSessionRequest,
) -> Result<TimelineSessionDetailDto, AppError> {
    let missing = || {
        AppError::new(
            "activity_session_not_found",
            "This session is no longer in the activity history.",
        )
    };
    let runtime = runtime_of(&state).ok_or_else(missing)?;
    let StoreAccess::Open(store) = store_access(runtime).await else {
        return Err(missing());
    };
    let detail = db::session_detail(&store, request.id)
        .await
        .map_err(|error| AppError::new("activity_session_failed", error.to_string()))?
        .ok_or_else(missing)?;
    let settings = runtime.shared.settings();
    let session = &detail.session;
    let domain = (session.context_kind == Some(ContextKind::Domain))
        .then_some(session.context.as_deref())
        .flatten();
    if session_excluded(
        &session.app_name,
        session.bundle_id.as_deref(),
        domain,
        None,
        &settings,
    ) {
        return Err(missing());
    }
    Ok(detail)
}

#[tauri::command]
pub async fn activity_timeline_search(
    app: AppHandle,
    state: State<'_, ActivityState>,
    request: TimelineSearchRequest,
) -> Result<ActivityTimelineSearchDto, AppError> {
    let from = request
        .from
        .as_deref()
        .map(|value| parse_request_time(value, "from"))
        .transpose()?;
    let to = request
        .to
        .as_deref()
        .map(|value| parse_request_time(value, "to"))
        .transpose()?;
    let limit = request
        .limit
        .unwrap_or(DEFAULT_SEARCH_LIMIT)
        .clamp(1, MAX_SEARCH_LIMIT);
    let Some(runtime) = runtime_of(&state) else {
        return Ok(ActivityTimelineSearchDto {
            availability: Availability::Unsupported,
            results: Vec::new(),
        });
    };
    let store = match store_access(runtime).await {
        StoreAccess::Open(store) => store,
        StoreAccess::Empty => {
            return Ok(ActivityTimelineSearchDto {
                availability: Availability::Ready,
                results: Vec::new(),
            })
        }
        StoreAccess::Unavailable(availability, _) => {
            return Ok(ActivityTimelineSearchDto {
                availability,
                results: Vec::new(),
            })
        }
    };
    if let Err(error) = run_pass(&app, &store).await {
        tracing::warn!(%error, "activity timeline pass failed before a search");
    }
    let settings = runtime.shared.settings();
    let results = search_view(
        &store,
        &settings,
        &request.query,
        from,
        to,
        limit,
        Utc::now(),
    )
    .await
    .map_err(|error| AppError::new("activity_search_failed", error.to_string()))?;
    Ok(ActivityTimelineSearchDto {
        availability: Availability::Ready,
        results,
    })
}

/// Agent catalog entries: present only while capture is enabled.
pub fn agent_tool_descriptors(app: &AppHandle) -> Vec<Value> {
    let enabled = app
        .state::<ActivityState>()
        .0
        .as_ref()
        .is_some_and(|runtime| runtime.shared.settings().enabled);
    agent_tools::descriptors(enabled)
}

/// Agent dispatch for `search_activity` / `get_activity_timeline`.
pub async fn dispatch_agent_tool(
    app: &AppHandle,
    name: &str,
    arguments: &Value,
) -> Result<Value, AppError> {
    let state = app.state::<ActivityState>();
    let runtime = state
        .0
        .as_ref()
        .filter(|runtime| runtime.shared.settings().enabled)
        .ok_or_else(|| {
            AppError::new(
                "activity_capture_off",
                "Activity capture is off. The user can turn it on in Settings, Activity.",
            )
        })?;
    let StoreAccess::Open(store) = store_access(runtime).await else {
        return Err(AppError::new(
            "activity_database_closed",
            "The activity history is not available right now.",
        ));
    };
    if let Err(error) = run_pass(app, &store).await {
        tracing::warn!(%error, "activity timeline pass failed before an agent tool");
    }
    let settings = runtime.shared.settings();
    agent_tools::dispatch(&store, &settings, name, arguments, Utc::now())
        .await
        .unwrap_or_else(|| {
            Err(AppError::new(
                "agent_tool_unknown",
                format!("Unknown activity tool `{name}`."),
            ))
        })
}
