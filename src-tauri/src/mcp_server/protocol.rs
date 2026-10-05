//! MCP (JSON-RPC 2.0) as the app answers it: lifecycle, `tools/*`, and
//! `resources/*`. Every message is answered on its own (no session state),
//! so any authenticated connection can send any request, and the stdio relay
//! can answer `initialize` itself while the app is unreachable.

use chrono::{DateTime, Local, NaiveDate, NaiveTime, TimeZone, Utc};
use serde_json::{json, Value};

use super::tools::{self, McpData};

pub const SERVER_NAME: &str = "clovy";
pub const CONTEXT_URI: &str = "clovy://context";
pub const GUIDE_URI: &str = "clovy://guide";
/// Newest first; an unknown client version gets the newest.
const PROTOCOL_VERSIONS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];

pub const PARSE_ERROR: i64 = -32700;
pub const INVALID_REQUEST: i64 = -32600;
pub const METHOD_NOT_FOUND: i64 = -32601;
pub const INVALID_PARAMS: i64 = -32602;
/// Server-defined: Clovy (or its MCP server) is unreachable or failed.
pub const SERVER_UNAVAILABLE: i64 = -32000;
const RESOURCE_NOT_FOUND: i64 = -32002;

const INSTRUCTIONS: &str = "Read-only access to the user's Clovy notes, dictations, and memories (from the data set open in Clovy) and, when activity capture is on, their activity timeline, day summaries, and coding-agent work on this Mac. Read clovy://context for the current date, time, and time zone before resolving words like \"today\"; clovy://guide explains which tool answers which question.";

pub fn result_response(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

pub fn error_response(id: Value, code: i64, message: impl Into<String>) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message.into() } })
}

/// What one incoming JSON-RPC message is.
#[derive(Debug, PartialEq)]
pub enum Incoming<'a> {
    Request {
        id: Value,
        method: &'a str,
    },
    Notification,
    /// A response the client sent back (Clovy sends it no requests).
    Response,
    /// Not a valid JSON-RPC 2.0 message: answered with `INVALID_REQUEST`
    /// and `id` (`null` when the id itself is unusable).
    Invalid {
        id: Value,
        reason: &'static str,
    },
}

pub fn classify(message: &Value) -> Incoming<'_> {
    let Some(object) = message.as_object() else {
        return Incoming::Invalid {
            id: Value::Null,
            reason: "Expected one JSON-RPC message object.",
        };
    };
    let id = object.get("id");
    let usable_id = id.filter(|id| id.is_string() || id.is_number()).cloned();
    let reply_id = usable_id.clone().unwrap_or(Value::Null);
    if object.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Incoming::Invalid {
            id: reply_id,
            reason: "`jsonrpc` must be \"2.0\".",
        };
    }
    match object.get("method") {
        Some(Value::String(method)) => match (id, usable_id) {
            (None, _) => Incoming::Notification,
            (Some(_), Some(id)) => Incoming::Request { id, method },
            (Some(_), None) => Incoming::Invalid {
                id: Value::Null,
                reason: "`id` must be a string or a number.",
            },
        },
        Some(_) => Incoming::Invalid {
            id: reply_id,
            reason: "`method` must be a string.",
        },
        None if id.is_some() && (object.contains_key("result") || object.contains_key("error")) => {
            Incoming::Response
        }
        None => Incoming::Invalid {
            id: reply_id,
            reason: "A request needs a `method`.",
        },
    }
}

/// The `id` of a valid request; `None` for anything else.
pub fn request_id(message: &Value) -> Option<Value> {
    match classify(message) {
        Incoming::Request { id, .. } => Some(id),
        _ => None,
    }
}

pub fn method(message: &Value) -> Option<&str> {
    message.get("method").and_then(Value::as_str)
}

/// `tools/call` for one of the activity tools: the app opens the activity
/// store only for these.
pub fn calls_activity_tool(message: &Value) -> bool {
    method(message) == Some("tools/call")
        && message
            .pointer("/params/name")
            .and_then(Value::as_str)
            .is_some_and(tools::is_activity_tool)
}

pub fn initialize_result(params: &Value, instructions: &str) -> Value {
    let requested = params.get("protocolVersion").and_then(Value::as_str);
    let version = PROTOCOL_VERSIONS
        .iter()
        .find(|version| Some(**version) == requested)
        .copied()
        .unwrap_or(PROTOCOL_VERSIONS[0]);
    json!({
        "protocolVersion": version,
        "capabilities": {
            "tools": { "listChanged": false },
            "resources": { "subscribe": false, "listChanged": false }
        },
        "serverInfo": { "name": SERVER_NAME, "title": "Clovy", "version": env!("CARGO_PKG_VERSION") },
        "instructions": instructions,
    })
}

/// A tool result whose single text block is `value` as JSON.
pub fn tool_result(value: &Value) -> Value {
    json!({
        "content": [{ "type": "text", "text": value.to_string() }],
        "isError": false,
    })
}

/// A failed tool call the model can read (MCP reports tool failures as
/// results, not protocol errors).
pub fn tool_error(message: &str) -> Value {
    json!({
        "content": [{ "type": "text", "text": message }],
        "isError": true,
    })
}

/// Answers one JSON-RPC message; `None` for notifications and responses.
pub async fn handle(data: &McpData, message: &Value) -> Option<Value> {
    let (id, method) = match classify(message) {
        Incoming::Request { id, method } => (id, method),
        Incoming::Notification | Incoming::Response => return None,
        Incoming::Invalid { id, reason } => {
            return Some(error_response(id, INVALID_REQUEST, reason))
        }
    };
    let params = message.get("params").cloned().unwrap_or_else(|| json!({}));
    let outcome = match method {
        "initialize" => Ok(initialize_result(&params, INSTRUCTIONS)),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": tools::descriptors(&data.activity) })),
        "tools/call" => call_tool(data, &params).await,
        "resources/list" => Ok(json!({ "resources": resources() })),
        "resources/templates/list" => Ok(json!({ "resourceTemplates": [] })),
        "resources/read" => read_resource(&params, data.now),
        other => Err((METHOD_NOT_FOUND, format!("Method not found: {other}"))),
    };
    Some(match outcome {
        Ok(result) => result_response(id, result),
        Err((code, message)) => error_response(id, code, message),
    })
}

async fn call_tool(data: &McpData, params: &Value) -> Result<Value, (i64, String)> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or((INVALID_PARAMS, "`name` is required".to_string()))?;
    if !tools::is_known_tool(name) {
        return Err((INVALID_PARAMS, format!("Unknown tool: {name}")));
    }
    let arguments = params
        .get("arguments")
        .filter(|arguments| !arguments.is_null())
        .cloned()
        .unwrap_or_else(|| json!({}));
    Ok(match tools::call(data, name, &arguments).await {
        Ok(value) => tool_result(&value),
        Err(error) => tool_error(&format!("{} ({})", error.message, error.code)),
    })
}

fn resources() -> Value {
    json!([
        {
            "uri": CONTEXT_URI,
            "name": "context",
            "title": "Current context",
            "description": "Current date, time, and time zone of the user's Mac.",
            "mimeType": "application/json"
        },
        {
            "uri": GUIDE_URI,
            "name": "guide",
            "title": "Usage guide",
            "description": "Which Clovy tool answers which question, and how dates, limits, and privacy work.",
            "mimeType": "text/markdown"
        }
    ])
}

fn read_resource(params: &Value, now: DateTime<Utc>) -> Result<Value, (i64, String)> {
    let uri = params
        .get("uri")
        .and_then(Value::as_str)
        .ok_or((INVALID_PARAMS, "`uri` is required".to_string()))?;
    let (mime_type, text) = match uri {
        CONTEXT_URI => (
            "application/json",
            context_json(now.with_timezone(&Local), local_time_zone_name()).to_string(),
        ),
        GUIDE_URI => ("text/markdown", GUIDE.to_string()),
        _ => return Err((RESOURCE_NOT_FOUND, format!("Resource not found: {uri}"))),
    };
    Ok(json!({ "contents": [{ "uri": uri, "mimeType": mime_type, "text": text }] }))
}

/// `clovy://context`: the moment in the Mac's local time. Calendar values
/// come from the local calendar (`yesterday` is the previous date, and
/// `startOfToday` is local midnight with the offset in force then), so they
/// stay right across daylight-saving changes.
pub fn context_json<Tz: TimeZone>(now: DateTime<Tz>, time_zone: Option<String>) -> Value
where
    Tz::Offset: std::fmt::Display,
{
    let offset = now.format("%:z").to_string();
    let today = now.date_naive();
    json!({
        "now": now.to_rfc3339(),
        "date": today.format("%Y-%m-%d").to_string(),
        "time": now.format("%H:%M").to_string(),
        "weekday": now.format("%A").to_string(),
        "timeZone": time_zone.unwrap_or_else(|| format!("UTC{offset}")),
        "utcOffset": offset,
        "yesterday": today.pred_opt().map(|day| day.format("%Y-%m-%d").to_string()),
        "startOfToday": start_of_day(&now.timezone(), today).map(|start| start.to_rfc3339()),
    })
}

/// The first local moment of `day`: midnight, or the first valid minute
/// after it where a daylight-saving jump skips midnight.
fn start_of_day<Tz: TimeZone>(zone: &Tz, day: NaiveDate) -> Option<DateTime<Tz>> {
    (0..=24 * 60).find_map(|minute| {
        let time = NaiveTime::from_num_seconds_from_midnight_opt(minute * 60, 0)?;
        zone.from_local_datetime(&day.and_time(time)).earliest()
    })
}

/// The IANA name of the Mac's time zone (`TZ`, else `/etc/localtime`).
fn local_time_zone_name() -> Option<String> {
    if let Some(name) = std::env::var("TZ")
        .ok()
        .map(|tz| tz.trim_start_matches(':').to_string())
        .filter(|tz| !tz.is_empty() && !tz.starts_with('/'))
    {
        return Some(name);
    }
    let target = std::fs::read_link("/etc/localtime").ok()?;
    let target = target.to_string_lossy();
    target
        .split_once("zoneinfo/")
        .map(|(_, name)| name.to_string())
        .filter(|name| !name.is_empty())
}

const GUIDE: &str = r#"# Clovy MCP guide

Clovy records meetings and dictations on the user's Mac, turns them into notes, keeps memories, and (when the user turned on activity capture) a text-only timeline of the apps and sites they used, day summaries, and their work with coding agents. Everything here is read-only. Notes, dictations, and memories come from the data set open in Clovy; activity belongs to the Mac, whichever data set is open.

## Which tool

| Question | Tool |
| --- | --- |
| "What did we decide about X in the meeting?" | `search_notes` with `query`, then `get_note` with the id |
| "Show me the notes of that call" | `get_note` (long transcripts come in pages: `nextTranscriptOffset`) |
| "What did I dictate yesterday?" | `list_dictations` (kept for 7 days; `query` filters) |
| "What do you know about me / this project?" | `list_memories` (`projectId` for a project) |
| "What did I do this morning?" | `get_activity_timeline` with `from`/`to` |
| "How much did I focus today?" | `get_activity_stats` |
| "Which apps did I use most this week?" | `list_app_usage` |
| "What am I doing right now?" | `get_active_session` |
| "When did I see X on screen?" | `search_activity` with `query` |
| "Details of that session" | `get_session_detail` with the session id |
| "What did I build with Claude Code today?" | `list_coding_agent_sessions` |
| "Summarize my day" / "What did I do yesterday?" | `get_day_summary` (`date`: YYYY-MM-DD, default today) |

## Tips

- Read `clovy://context` first: it has today's date, the time, and the time zone. Activity tools take RFC 3339 times with an offset (`2026-10-04T09:00:00-03:00`); omit `from`/`to` for today so far. `get_day_summary` takes a date (`YYYY-MM-DD`), default today.
- Activity tools exist only while activity capture is on. Apps and sites the user excluded never appear, and nothing older than the retention period is returned.
- Long results are capped: look at `truncated`, `hasMore`/`nextOffset`, or `nextTranscriptOffset`, and ask for a shorter period or the next page.
- Clovy must be open with the MCP server turned on (Settings, Agent). Otherwise every tool answers with an error saying so.
"#;
