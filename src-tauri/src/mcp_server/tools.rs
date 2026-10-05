//! The Clovy MCP server's tools: read-only views of the active profile's
//! notes, dictations, and memories, and (only while capture is on) of the
//! activity timeline. Names are fixed in the `clovy-mcp-server` spec of the
//! OpenSpec change `add-activity-intelligence`; `docs/mcp-server.md` lists
//! them with their arguments and limits.

use chrono::{DateTime, Duration, Local, Utc};
use serde_json::{json, Value};
use sqlx::{query::query, row::Row};
use sqlx_sqlite::SqlitePool;

use crate::activity::settings::ActivitySettings;
use crate::activity::store::ActivityStore;
use crate::activity::timeline::agent_tools::{
    self, gap_json, local, local_midnight, minutes, session_json, stats_json, time_argument,
};
use crate::activity::timeline::context::ContextKind;
use crate::activity::timeline::{
    db, retention_floor, session_excluded, stats, timeline_view, TimelineView,
};
use crate::db::repositories::{
    dictation_history_cutoff_timestamp, Repositories, DICTATION_HISTORY_RETENTION_DAYS,
};
use crate::domain::types::AppError;

pub const SEARCH_NOTES: &str = "search_notes";
pub const GET_NOTE: &str = "get_note";
pub const LIST_DICTATIONS: &str = "list_dictations";
pub const LIST_MEMORIES: &str = "list_memories";
pub const GET_ACTIVITY_STATS: &str = "get_activity_stats";
pub const GET_ACTIVE_SESSION: &str = "get_active_session";
pub const LIST_APP_USAGE: &str = "list_app_usage";
pub const GET_SESSION_DETAIL: &str = "get_session_detail";

/// Activity tools, in catalog order. `list_coding_agent_sessions` joins this
/// list once the coding-agent slice is on `main`.
pub const ACTIVITY_TOOLS: [&str; 6] = [
    agent_tools::GET_ACTIVITY_TIMELINE,
    agent_tools::SEARCH_ACTIVITY,
    GET_ACTIVITY_STATS,
    GET_ACTIVE_SESSION,
    LIST_APP_USAGE,
    GET_SESSION_DETAIL,
];
const DATA_TOOLS: [&str; 4] = [SEARCH_NOTES, GET_NOTE, LIST_DICTATIONS, LIST_MEMORIES];

const SNIPPET_RADIUS_CHARS: usize = 160;
const NOTE_CONTENT_MAX_CHARS: usize = 20_000;
const TRANSCRIPT_PAGE_CHARS: usize = 40_000;
const DICTATION_MAX_CHARS: usize = 4_000;
const MEMORY_MAX_CHARS: usize = 4_000;
const DEFAULT_APP_USAGE: u64 = 20;
const MAX_APP_USAGE: u64 = 100;
/// How far back `get_active_session` looks for the session going on now.
const ACTIVE_LOOKBACK_HOURS: i64 = 12;

/// What one MCP request may read, resolved by the app per message
/// (`super::app_data`) or built directly by tests.
pub struct McpData {
    /// The main database (`notes.sqlite3`).
    pub notes: SqlitePool,
    /// Every query is limited to this profile.
    pub profile: String,
    /// Settings, Memory: `list_memories` refuses while memory is off.
    pub memory_enabled: bool,
    pub activity: ActivityAccess,
    pub now: DateTime<Utc>,
}

pub enum ActivityAccess {
    /// Capture is off: activity tools are not offered and refuse calls.
    Off,
    /// Capture is on; the store was not opened for this message.
    On,
    /// Capture is on and the store is open for an activity tool call.
    Open {
        store: ActivityStore,
        settings: ActivitySettings,
    },
    /// Capture is on but the history cannot be read right now.
    Unavailable(AppError),
}

impl ActivityAccess {
    pub fn offered(&self) -> bool {
        !matches!(self, ActivityAccess::Off)
    }
}

pub fn is_activity_tool(name: &str) -> bool {
    ACTIVITY_TOOLS.contains(&name)
}

pub fn is_known_tool(name: &str) -> bool {
    DATA_TOOLS.contains(&name) || is_activity_tool(name)
}

fn read_only(name: &str, description: &str, input_schema: Value) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": input_schema,
        "annotations": { "readOnlyHint": true, "openWorldHint": false, "idempotentHint": true },
    })
}

fn range_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "from": { "type": "string", "description": "Start, RFC 3339 with offset (default: local midnight today)." },
            "to": { "type": "string", "description": "End, RFC 3339 with offset (default: now)." }
        },
        "additionalProperties": false
    })
}

/// The `tools/list` catalog: data tools always, activity tools only while
/// capture is on.
pub fn descriptors(activity: &ActivityAccess) -> Vec<Value> {
    let mut tools = vec![
        read_only(
            SEARCH_NOTES,
            "Search the user's Clovy meeting notes (titles, note text, and transcripts) for a word or phrase. Returns each matching note's id, title, a snippet around the match, and dates, newest first. Use get_note with the id for the full note.",
            json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Word or phrase to find (case-insensitive)." },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 20, "default": 10 }
                },
                "required": ["query"],
                "additionalProperties": false
            }),
        ),
        read_only(
            GET_NOTE,
            "Read one Clovy note by id: title, note text, and its latest transcript. Long transcripts come in pages; pass `transcriptOffset` from `nextTranscriptOffset` to continue.",
            json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string" },
                    "transcriptOffset": { "type": "integer", "minimum": 0, "default": 0, "description": "Character offset into the transcript." }
                },
                "required": ["id"],
                "additionalProperties": false
            }),
        ),
        read_only(
            LIST_DICTATIONS,
            "List the user's recent Clovy dictations (kept for 7 days), newest first, optionally only those containing `query`.",
            json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string" },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 50, "default": 20 },
                    "offset": { "type": "integer", "minimum": 0, "default": 0 }
                },
                "additionalProperties": false
            }),
        ),
        read_only(
            LIST_MEMORIES,
            "List what Clovy remembers about the user (global memories and, with `projectId`, that project's), newest first.",
            json!({
                "type": "object",
                "properties": {
                    "projectId": { "type": "string" },
                    "includeGlobal": { "type": "boolean", "default": true },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 20, "default": 8 },
                    "offset": { "type": "integer", "minimum": 0, "default": 0 }
                },
                "additionalProperties": false
            }),
        ),
    ];
    if !activity.offered() {
        return tools;
    }
    for descriptor in agent_tools::descriptors(true) {
        tools.push(read_only(
            descriptor["name"].as_str().unwrap_or_default(),
            descriptor["description"].as_str().unwrap_or_default(),
            descriptor["parameters"].clone(),
        ));
    }
    tools.extend([
        read_only(
            GET_ACTIVITY_STATS,
            "Totals for a period of the user's activity: focused, idle, and away minutes, the top apps, and time per category. Omit both bounds for today so far.",
            range_schema(),
        ),
        read_only(
            GET_ACTIVE_SESSION,
            "What the user is doing right now: the app session in progress (app, browser domain or editor workspace, window, category, minutes) or the idle/away gap going on, and the last session when nothing is active.",
            json!({ "type": "object", "properties": {}, "additionalProperties": false }),
        ),
        read_only(
            LIST_APP_USAGE,
            "Time per app for a period, most used first, with the number of sessions. Omit both bounds for today so far.",
            json!({
                "type": "object",
                "properties": {
                    "from": { "type": "string", "description": "Start, RFC 3339 with offset (default: local midnight today)." },
                    "to": { "type": "string", "description": "End, RFC 3339 with offset (default: now)." },
                    "limit": { "type": "integer", "minimum": 1, "maximum": MAX_APP_USAGE, "default": DEFAULT_APP_USAGE }
                },
                "additionalProperties": false
            }),
        ),
        read_only(
            GET_SESSION_DETAIL,
            "One activity session in detail: the windows and URLs seen in it and an excerpt of its captured text. Session ids come from get_activity_timeline and search_activity.",
            json!({
                "type": "object",
                "properties": { "id": { "type": "integer" } },
                "required": ["id"],
                "additionalProperties": false
            }),
        ),
    ]);
    tools
}

/// Runs a known tool. Errors become `isError` tool results.
pub async fn call(data: &McpData, name: &str, arguments: &Value) -> Result<Value, AppError> {
    match name {
        SEARCH_NOTES => search_notes(data, arguments).await,
        GET_NOTE => get_note(data, arguments).await,
        LIST_DICTATIONS => list_dictations(data, arguments).await,
        LIST_MEMORIES => list_memories(data, arguments).await,
        name if is_activity_tool(name) => {
            let (store, settings) = match &data.activity {
                ActivityAccess::Off => return Err(AppError::new(
                    "activity_capture_off",
                    "Activity capture is off. The user can turn it on in Clovy Settings, Activity.",
                )),
                ActivityAccess::On => {
                    return Err(AppError::new(
                        "activity_database_closed",
                        "The activity history is not available right now.",
                    ))
                }
                ActivityAccess::Unavailable(error) => return Err(error.clone()),
                ActivityAccess::Open { store, settings } => (store, settings),
            };
            activity_tool(store, settings, name, arguments, data.now).await
        }
        _ => Err(AppError::new(
            "mcp_tool_unknown",
            format!("Unknown tool `{name}`."),
        )),
    }
}

fn invalid(message: impl Into<String>) -> AppError {
    AppError::new("mcp_invalid_arguments", message.into())
}

fn required_text<'a>(arguments: &'a Value, key: &str) -> Result<&'a str, AppError> {
    arguments
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| invalid(format!("`{key}` is required")))
}

fn optional_text<'a>(arguments: &'a Value, key: &str) -> Option<&'a str> {
    arguments
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn bounded(arguments: &Value, key: &str, default: u64, min: u64, max: u64) -> u64 {
    arguments
        .get(key)
        .and_then(Value::as_u64)
        .unwrap_or(default)
        .clamp(min, max)
}

fn like_pattern(text: &str) -> String {
    format!(
        "%{}%",
        text.replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_")
    )
}

/// The first `max` characters, and whether anything was cut.
fn clip(text: &str, max: usize) -> (String, bool) {
    match text.char_indices().nth(max) {
        Some((end, _)) => (text[..end].to_string(), true),
        None => (text.to_string(), false),
    }
}

/// About `SNIPPET_RADIUS_CHARS` characters on each side of the first
/// case-insensitive match of `needle`; `None` when it does not occur.
fn snippet(text: &str, needle: &str) -> Option<String> {
    let haystack: Vec<char> = text.chars().collect();
    let needle: Vec<char> = needle.chars().flat_map(char::to_lowercase).collect();
    if needle.is_empty() || needle.len() > haystack.len() {
        return None;
    }
    let lower = |c: char| c.to_lowercase().next().unwrap_or(c);
    let start = (0..=haystack.len() - needle.len()).find(|&start| {
        needle
            .iter()
            .enumerate()
            .all(|(offset, expected)| lower(haystack[start + offset]) == *expected)
    })?;
    let from = start.saturating_sub(SNIPPET_RADIUS_CHARS);
    let to = (start + needle.len() + SNIPPET_RADIUS_CHARS).min(haystack.len());
    let mut excerpt: String = haystack[from..to].iter().collect();
    excerpt = excerpt.split_whitespace().collect::<Vec<_>>().join(" ");
    if from > 0 {
        excerpt.insert_str(0, "...");
    }
    if to < haystack.len() {
        excerpt.push_str("...");
    }
    Some(excerpt)
}

const LATEST_TRANSCRIPT: &str =
    "COALESCE((SELECT text FROM transcripts t WHERE t.note_id = n.id ORDER BY t.created_at DESC LIMIT 1), '')";

async fn search_notes(data: &McpData, arguments: &Value) -> Result<Value, AppError> {
    let needle = required_text(arguments, "query")?;
    let limit = bounded(arguments, "limit", 10, 1, 20);
    let pattern = like_pattern(needle);
    let rows = query(&format!(
        "SELECT n.id, n.title, COALESCE(n.edited_content, n.generated_content, '') AS note,
                {LATEST_TRANSCRIPT} AS transcript, n.created_at, n.updated_at
         FROM notes n
         WHERE n.profile = ? AND (
            n.title LIKE ? ESCAPE '\\' OR n.generated_content LIKE ? ESCAPE '\\'
            OR n.edited_content LIKE ? ESCAPE '\\'
            OR EXISTS (SELECT 1 FROM transcripts t WHERE t.note_id = n.id AND t.text LIKE ? ESCAPE '\\')
         )
         ORDER BY n.updated_at DESC LIMIT ?"
    ))
    .bind(&data.profile)
    .bind(&pattern)
    .bind(&pattern)
    .bind(&pattern)
    .bind(&pattern)
    .bind(limit as i64)
    .fetch_all(&data.notes)
    .await?;
    let notes: Vec<Value> = rows
        .into_iter()
        .map(|row| {
            let title: String = row.get("title");
            let note: String = row.get("note");
            let transcript: String = row.get("transcript");
            let (matched_in, excerpt) = if let Some(excerpt) = snippet(&note, needle) {
                ("note", excerpt)
            } else if let Some(excerpt) = snippet(&transcript, needle) {
                ("transcript", excerpt)
            } else if snippet(&title, needle).is_some() {
                ("title", clip(&note, SNIPPET_RADIUS_CHARS * 2).0)
            } else {
                ("transcript", clip(&note, SNIPPET_RADIUS_CHARS * 2).0)
            };
            json!({
                "id": row.get::<String, _>("id"),
                "title": title,
                "snippet": excerpt,
                "matchedIn": matched_in,
                "createdAt": row.get::<String, _>("created_at"),
                "updatedAt": row.get::<String, _>("updated_at"),
            })
        })
        .collect();
    Ok(json!({ "count": notes.len(), "notes": notes }))
}

async fn get_note(data: &McpData, arguments: &Value) -> Result<Value, AppError> {
    let id = required_text(arguments, "id")?;
    let offset = arguments
        .get("transcriptOffset")
        .and_then(Value::as_u64)
        .unwrap_or(0) as usize;
    let row = query(&format!(
        "SELECT n.id, n.title, COALESCE(n.edited_content, n.generated_content, '') AS note,
                {LATEST_TRANSCRIPT} AS transcript, n.created_at, n.updated_at
         FROM notes n WHERE n.id = ? AND n.profile = ?"
    ))
    .bind(id)
    .bind(&data.profile)
    .fetch_optional(&data.notes)
    .await?
    .ok_or_else(|| {
        AppError::new(
            "note_not_found",
            "No note with this id in the active profile.",
        )
    })?;
    let (content, content_truncated) = clip(&row.get::<String, _>("note"), NOTE_CONTENT_MAX_CHARS);
    let transcript: String = row.get("transcript");
    let total = transcript.chars().count();
    let page: String = transcript
        .chars()
        .skip(offset)
        .take(TRANSCRIPT_PAGE_CHARS)
        .collect();
    let end = offset.saturating_add(TRANSCRIPT_PAGE_CHARS);
    Ok(json!({
        "id": row.get::<String, _>("id"),
        "title": row.get::<String, _>("title"),
        "createdAt": row.get::<String, _>("created_at"),
        "updatedAt": row.get::<String, _>("updated_at"),
        "content": content,
        "contentTruncated": content_truncated,
        "transcript": page,
        "transcriptOffset": offset,
        "transcriptTotalChars": total,
        "nextTranscriptOffset": (end < total).then_some(end),
    }))
}

async fn list_dictations(data: &McpData, arguments: &Value) -> Result<Value, AppError> {
    let limit = bounded(arguments, "limit", 20, 1, 50);
    let offset = arguments.get("offset").and_then(Value::as_u64).unwrap_or(0);
    let needle = optional_text(arguments, "query");
    let rows = query(
        "SELECT id, text, language, created_at FROM dictation_history
         WHERE profile = ? AND created_at >= ? AND (? IS NULL OR text LIKE ? ESCAPE '\\')
         ORDER BY created_at DESC, rowid DESC LIMIT ? OFFSET ?",
    )
    .bind(&data.profile)
    .bind(dictation_history_cutoff_timestamp())
    .bind(needle)
    .bind(needle.map(like_pattern))
    .bind((limit + 1) as i64)
    .bind(offset as i64)
    .fetch_all(&data.notes)
    .await?;
    let has_more = rows.len() as u64 > limit;
    let items: Vec<Value> = rows
        .into_iter()
        .take(limit as usize)
        .map(|row| {
            let (text, truncated) = clip(&row.get::<String, _>("text"), DICTATION_MAX_CHARS);
            json!({
                "id": row.get::<String, _>("id"),
                "text": text,
                "truncated": truncated,
                "language": row.get::<Option<String>, _>("language"),
                "createdAt": row.get::<String, _>("created_at"),
            })
        })
        .collect();
    Ok(json!({
        "count": items.len(),
        "items": items,
        "offset": offset,
        "hasMore": has_more,
        "nextOffset": has_more.then_some(offset + limit),
        "retentionDays": DICTATION_HISTORY_RETENTION_DAYS,
    }))
}

async fn list_memories(data: &McpData, arguments: &Value) -> Result<Value, AppError> {
    if !data.memory_enabled {
        return Err(AppError::new(
            "memory_disabled",
            "Memory is turned off in Clovy Settings.",
        ));
    }
    let project_id = optional_text(arguments, "projectId");
    if let Some(project_id) = project_id {
        let row = query(
            "SELECT memory_disabled FROM folders WHERE id = ? AND profile = ? AND deleted_at IS NULL",
        )
        .bind(project_id)
        .bind(&data.profile)
        .fetch_optional(&data.notes)
        .await?
        .ok_or_else(|| AppError::new("folder_not_found", "Project was not found."))?;
        if row.get::<i64, _>("memory_disabled") != 0 {
            return Err(AppError::new(
                "memory_disabled",
                "Memory is turned off for this project.",
            ));
        }
    }
    let include_global = arguments
        .get("includeGlobal")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let limit = bounded(arguments, "limit", 8, 1, 20) as usize;
    let offset = arguments.get("offset").and_then(Value::as_u64).unwrap_or(0) as usize;
    let memories = Repositories::new(data.notes.clone())
        .list_memories(&data.profile, project_id, include_global)
        .await?;
    let has_more = memories.len() > offset.saturating_add(limit);
    let items: Vec<Value> = memories
        .into_iter()
        .skip(offset)
        .take(limit)
        .map(|memory| {
            json!({
                "id": memory.id,
                "content": clip(&memory.content, MEMORY_MAX_CHARS).0,
                "createdAt": memory.created_at,
                "scope": if memory.folder_id.is_some() { "project" } else { "global" },
            })
        })
        .collect();
    Ok(json!({
        "count": items.len(),
        "items": items,
        "offset": offset,
        "hasMore": has_more,
        "nextOffset": has_more.then_some(offset + items.len()),
    }))
}

async fn activity_tool(
    store: &ActivityStore,
    settings: &ActivitySettings,
    name: &str,
    arguments: &Value,
    now: DateTime<Utc>,
) -> Result<Value, AppError> {
    if let Some(result) = agent_tools::dispatch(store, settings, name, arguments, now).await {
        return result;
    }
    match name {
        GET_ACTIVITY_STATS => {
            let (from, to) = range(arguments, now)?;
            let view = view(store, settings, from, to, now).await?;
            let mut value = stats_json(&view.stats);
            value["from"] = json!(local_at(from));
            value["to"] = json!(local_at(to));
            Ok(value)
        }
        GET_ACTIVE_SESSION => active_session(store, settings, now).await,
        LIST_APP_USAGE => app_usage(store, settings, arguments, now).await,
        GET_SESSION_DETAIL => session_detail(store, settings, arguments, now).await,
        _ => Err(AppError::new(
            "mcp_tool_unknown",
            format!("Unknown activity tool `{name}`."),
        )),
    }
}

fn range(
    arguments: &Value,
    now: DateTime<Utc>,
) -> Result<(DateTime<Utc>, DateTime<Utc>), AppError> {
    let from = time_argument(arguments, "from")?.unwrap_or_else(|| local_midnight(now));
    let to = time_argument(arguments, "to")?.unwrap_or(now);
    if to <= from {
        return Err(invalid("`to` must be after `from`"));
    }
    Ok((from, to))
}

fn local_at(at: DateTime<Utc>) -> String {
    at.with_timezone(&Local).to_rfc3339()
}

async fn view(
    store: &ActivityStore,
    settings: &ActivitySettings,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    now: DateTime<Utc>,
) -> Result<TimelineView, AppError> {
    timeline_view(store, settings, from, to, now)
        .await
        .map_err(|error| AppError::new("activity_timeline_failed", error.to_string()))
}

async fn active_session(
    store: &ActivityStore,
    settings: &ActivitySettings,
    now: DateTime<Utc>,
) -> Result<Value, AppError> {
    let view = view(
        store,
        settings,
        now - Duration::hours(ACTIVE_LOOKBACK_HOURS),
        now + Duration::minutes(1),
        now,
    )
    .await?;
    let active = view.sessions.iter().rev().find(|session| session.active);
    let ongoing_gap = view.gaps.iter().rev().find(|gap| gap.ongoing);
    Ok(json!({
        "now": local_at(now),
        "session": active.map(session_json),
        "ongoingGap": ongoing_gap.map(gap_json),
        "lastSession": if active.is_none() { view.sessions.last().map(session_json) } else { None },
    }))
}

async fn app_usage(
    store: &ActivityStore,
    settings: &ActivitySettings,
    arguments: &Value,
    now: DateTime<Utc>,
) -> Result<Value, AppError> {
    let (from, to) = range(arguments, now)?;
    let limit = bounded(arguments, "limit", DEFAULT_APP_USAGE, 1, MAX_APP_USAGE) as usize;
    let view = view(store, settings, from, to, now).await?;
    let mut apps: Vec<(String, i64, u32)> = Vec::new();
    for session in &view.sessions {
        let ms = stats::clipped_ms(&session.started_at, &session.ended_at, from, to);
        if ms == 0 {
            continue;
        }
        match apps.iter_mut().find(|(app, _, _)| *app == session.app_name) {
            Some((_, total, sessions)) => {
                *total += ms;
                *sessions += 1;
            }
            None => apps.push((session.app_name.clone(), ms, 1)),
        }
    }
    apps.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let total_apps = apps.len();
    Ok(json!({
        "from": local_at(from),
        "to": local_at(to),
        "apps": apps.iter().take(limit).map(|(app, ms, sessions)| json!({
            "app": app,
            "minutes": minutes(*ms),
            "sessions": sessions,
        })).collect::<Vec<_>>(),
        "totalApps": total_apps,
        "truncated": total_apps > limit,
    }))
}

async fn session_detail(
    store: &ActivityStore,
    settings: &ActivitySettings,
    arguments: &Value,
    now: DateTime<Utc>,
) -> Result<Value, AppError> {
    let id = arguments
        .get("id")
        .and_then(Value::as_i64)
        .ok_or_else(|| invalid("`id` must be a session id"))?;
    let missing = || {
        AppError::new(
            "activity_session_not_found",
            "This session is not in the activity history.",
        )
    };
    let detail = db::session_detail(store, id)
        .await
        .map_err(|error| AppError::new("activity_session_failed", error.to_string()))?
        .ok_or_else(missing)?;
    let session = &detail.session;
    let domain = (session.context_kind == Some(ContextKind::Domain))
        .then_some(session.context.as_deref())
        .flatten();
    let expired = crate::activity::timeline::builder::parse_time(&session.ended_at)
        .map_or(true, |ended| ended < retention_floor(settings, now));
    if expired
        || session_excluded(
            &session.app_name,
            session.bundle_id.as_deref(),
            domain,
            None,
            settings,
        )
    {
        return Err(missing());
    }
    let windows: Vec<Value> = detail
        .windows
        .iter()
        .filter(|window| {
            !window.browser_url.as_deref().is_some_and(|url| {
                session_excluded(&session.app_name, None, None, Some(url), settings)
            })
        })
        .map(|window| {
            json!({
                "windowTitle": window.window_title,
                "url": window.browser_url,
                "firstSeenAt": local(&window.first_seen_at),
                "lastSeenAt": local(&window.last_seen_at),
                "frames": window.frame_count,
            })
        })
        .collect();
    // The excerpt can come from any window of the session; drop it when an
    // excluded URL was among them.
    let text_excerpt = if windows.len() == detail.windows.len() {
        detail.text_excerpt.clone()
    } else {
        None
    };
    Ok(json!({
        "session": session_json(session),
        "windows": windows,
        "textExcerpt": text_excerpt,
    }))
}
