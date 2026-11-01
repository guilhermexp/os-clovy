//! MCP (JSON-RPC 2.0) as the app answers it: lifecycle, `tools/*`, and
//! `resources/*`. Every message is answered on its own (no session state),
//! so any authenticated connection can send any request, and the stdio relay
//! can answer `initialize` itself while the app is unreachable.

use chrono::{DateTime, Local, NaiveDate, NaiveTime, TimeZone, Utc};
use serde_json::{json, Value};

use super::tools::{self, McpData, ToolCtx};

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
const BOUND_INSTRUCTIONS: &str = "Clovy tools for this host-bound chat turn. Notes, conversations, projects, dictations and memories use the conversation's data partition even if the app view changes. Activity belongs to the installation. Read clovy://context for dates and host context and clovy://guide for operations, pagination, effects and costs. Use only the tools actually listed for this turn. Mutations use the host's approval broker and revalidate authority after waits; tool configuration never grants extra access. Media tools can incur direct provider charges without a separate broker card. Status and reading a skill do not activate permissions or execute instructions.";

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

/// A tool result whose single text block is `value` as JSON, or, when the
/// tool returned [`content_value`], those MCP content blocks as they are.
pub fn tool_result(value: &Value) -> Value {
    if let Some(blocks) = content_blocks(value) {
        return json!({ "content": blocks, "isError": false });
    }
    json!({
        "content": [{ "type": "text", "text": value.to_string() }],
        "isError": false,
    })
}

/// The reserved key of [`content_value`]. Tools that return plain JSON never
/// use it, so their results keep the single text block.
pub const CONTENT_KEY: &str = "$mcpContent";

/// A tool return value carrying MCP content blocks instead of one JSON text
/// block (addendum to contract C4, 2026-10-15): `text` and `image` blocks
/// only, built with [`text_block`] and [`image_block`].
pub fn content_value(blocks: Vec<Value>) -> Value {
    json!({ CONTENT_KEY: blocks })
}

pub fn text_block(text: impl Into<String>) -> Value {
    json!({ "type": "text", "text": text.into() })
}

/// An image block: `data` is base64, `mime_type` an `image/*` type.
pub fn image_block(data: impl Into<String>, mime_type: impl Into<String>) -> Value {
    json!({ "type": "image", "data": data.into(), "mimeType": mime_type.into() })
}

/// The blocks of a [`content_value`], when every one is a well-formed text
/// or image block; anything else is serialized as JSON text like before.
fn content_blocks(value: &Value) -> Option<&Vec<Value>> {
    let object = value.as_object()?;
    if object.len() != 1 {
        return None;
    }
    let blocks = object.get(CONTENT_KEY)?.as_array()?;
    let valid = |block: &Value| match block.get("type").and_then(Value::as_str) {
        Some("text") => block.get("text").is_some_and(Value::is_string),
        Some("image") => {
            block.get("data").is_some_and(Value::is_string)
                && block
                    .get("mimeType")
                    .and_then(Value::as_str)
                    .is_some_and(|mime| mime.starts_with("image/"))
        }
        _ => false,
    };
    (!blocks.is_empty() && blocks.iter().all(valid)).then_some(blocks)
}

/// A failed tool call the model can read (MCP reports tool failures as
/// results, not protocol errors).
pub fn tool_error(message: &str) -> Value {
    json!({
        "content": [{ "type": "text", "text": message }],
        "isError": true,
    })
}

pub(crate) fn tool_error_for(error: &crate::domain::types::AppError) -> Value {
    let mut result = tool_error(&format!("{} ({})", error.message, error.code));
    result["structuredContent"] = json!({"error": {
        "code": error.code, "message": error.message, "details": error.details,
    }});
    // Keep the familiar first text block while making delivery and retry
    // disposition visible to clients that consume text only.
    if let Some(details) = error.details.as_ref() {
        if let Some(content) = result["content"].as_array_mut() {
            content.push(text_block(
                json!({"code":error.code,"details":details}).to_string(),
            ));
        }
    }
    result
}

/// MCP error context is host-owned. Do not forward arbitrary provider error
/// bodies or other AppError details through the relay.
fn app_tool_error(error: &crate::domain::types::AppError) -> Value {
    let mut result = tool_error(&format!("{} ({})", error.message, error.code));
    if let Some(diagnostic) = error
        .details
        .as_ref()
        .and_then(|details| {
            serde_json::from_value::<crate::agent_mcp::AgentMcpDiagnostic>(details.clone()).ok()
        })
        .filter(|diagnostic| {
            diagnostic.origin == "clovy_managed"
                && diagnostic.code == error.code
                && matches!(
                    diagnostic.phase.as_str(),
                    "connect" | "discover" | "refresh" | "call" | "disconnect"
                )
        })
    {
        result["_meta"] = json!({"clovy": {"diagnostic": diagnostic}});
    }
    result
}

fn call_tool_error(name: &str, error: &crate::domain::types::AppError) -> Value {
    if tools::is_gateway_tool(name) {
        app_tool_error(error)
    } else {
        tool_error_for(error)
    }
}

#[cfg(test)]
mod diagnostic_tests {
    use super::*;
    use crate::{agent_mcp::AgentMcpError, domain::types::AppError};

    #[test]
    fn relay_retains_host_diagnostic_but_discards_arbitrary_details() {
        let error = AgentMcpError::Remote(-32601)
            .diagnostic("private", Some("lookup"), "call")
            .into_app_error();
        let result = app_tool_error(&error);
        assert_eq!(result["isError"], true);
        let diagnostic = &result["_meta"]["clovy"]["diagnostic"];
        assert_eq!(diagnostic["server"], "private");
        assert_eq!(diagnostic["remoteCode"], -32601);
        assert_eq!(diagnostic["phase"], "call");
        let mut unrelated = AppError::new("provider_failure", "Provider unavailable");
        unrelated.details = Some(json!({"authorization": "fixture-private-payload"}));
        let result = app_tool_error(&unrelated);
        assert!(result.get("_meta").is_none());
        assert!(!result.to_string().contains("fixture-private-payload"));
        let result = call_tool_error("mcp_example_read", &unrelated);
        assert!(!result.to_string().contains("fixture-private-payload"));

        let mut native = AppError::new("turn_expired", "Turn expired");
        native.details = Some(json!({"retrySafe": false}));
        let result = call_tool_error("computer_use", &native);
        assert_eq!(
            result["structuredContent"]["error"]["details"]["retrySafe"],
            false
        );
        let result = call_tool_error("mcp_example_read", &error);
        assert_eq!(result["_meta"]["clovy"]["diagnostic"]["remoteCode"], -32601);
    }
}

/// Answers one JSON-RPC message for a client with neither app nor turn;
/// `None` for notifications and responses.
pub async fn handle(data: &McpData, message: &Value) -> Option<Value> {
    handle_in(&ToolCtx::unbound(data), message).await
}

/// Answers one JSON-RPC message in `ctx` (the connection's turn decides
/// which tool groups exist); `None` for notifications and responses.
pub async fn handle_in(ctx: &ToolCtx<'_>, message: &Value) -> Option<Value> {
    let (id, method) = match classify(message) {
        Incoming::Request { id, method } => (id, method),
        Incoming::Notification | Incoming::Response => return None,
        Incoming::Invalid { id, reason } => {
            return Some(error_response(id, INVALID_REQUEST, reason))
        }
    };
    let params = message.get("params").cloned().unwrap_or_else(|| json!({}));
    let outcome = match method {
        "initialize" => Ok(initialize_result(
            &params,
            if ctx.binding.is_some() {
                BOUND_INSTRUCTIONS
            } else {
                INSTRUCTIONS
            },
        )),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": tools::descriptors(ctx).await })),
        "tools/call" => call_tool(ctx, &params).await,
        "resources/list" => Ok(json!({ "resources": resources() })),
        "resources/templates/list" => Ok(json!({ "resourceTemplates": [] })),
        "resources/read" => read_resource(ctx, &params, ctx.data.now).await,
        other => Err((METHOD_NOT_FOUND, format!("Method not found: {other}"))),
    };
    Some(match outcome {
        Ok(result) => result_response(id, result),
        Err((code, message)) => error_response(id, code, message),
    })
}

async fn call_tool(ctx: &ToolCtx<'_>, params: &Value) -> Result<Value, (i64, String)> {
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
    Ok(match tools::call(ctx, name, &arguments).await {
        Ok(value) => tool_result(&value),
        Err(error) => call_tool_error(name, &error),
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

async fn read_resource(
    ctx: &ToolCtx<'_>,
    params: &Value,
    now: DateTime<Utc>,
) -> Result<Value, (i64, String)> {
    let uri = params
        .get("uri")
        .and_then(Value::as_str)
        .ok_or((INVALID_PARAMS, "`uri` is required".to_string()))?;
    let (mime_type, text) = match uri {
        CONTEXT_URI => {
            let mut context = context_json(now.with_timezone(&Local), local_time_zone_name());
            if let Some(binding) = ctx.binding {
                use sqlx::row::Row;
                let policy = ctx
                    .broker()
                    .map_err(|_| (SERVER_UNAVAILABLE, "Approval policy unavailable".into()))?
                    .effective_policy(&crate::approval_broker::ApprovalTurn::from(binding))
                    .await
                    .map_err(|e| {
                        (
                            SERVER_UNAVAILABLE,
                            format!("{} ({})", e.message(), e.code()),
                        )
                    })?;
                let row = sqlx::query::query("SELECT COALESCE(json_extract(r.run_config_json, '$.workspace'), s.workspace_path) AS workspace_path, s.safety_mode, s.approval_mode, COALESCE(p.profile, 'default') AS profile FROM agent_sessions s JOIN agent_runs r ON r.session_id = s.id LEFT JOIN session_profiles p ON p.session_id = s.id WHERE s.id = ? AND r.id = ?")
                    .bind(&binding.session_id).bind(&binding.run_id).fetch_optional(&ctx.data.notes).await
                    .map_err(|_| (SERVER_UNAVAILABLE, "Session context unavailable".into()))?
                    .ok_or((SERVER_UNAVAILABLE, "Session context unavailable".into()))?;
                let folders = sqlx::query::query(
                    "SELECT folder_id FROM session_folders WHERE session_id = ? ORDER BY folder_id",
                )
                .bind(&binding.session_id)
                .fetch_all(&ctx.data.notes)
                .await
                .map_err(|_| (SERVER_UNAVAILABLE, "Project context unavailable".into()))?
                .into_iter()
                .map(|r| r.get::<String, _>("folder_id"))
                .collect::<Vec<_>>();
                context["session"] = json!({"id": binding.session_id, "runId": binding.run_id, "engine": binding.cli.id(), "surface": binding.surface, "partition": row.get::<String, _>("profile"), "projectIds": folders, "workspace": row.get::<Option<String>, _>("workspace_path"), "executionPolicy": binding.safety_mode, "configuredExecutionPolicy": row.get::<String, _>("safety_mode"), "approvalMode": row.get::<String, _>("approval_mode"), "effectiveApprovalMode": policy.mode});
            }
            ("application/json", context.to_string())
        }
        GUIDE_URI => (
            "text/markdown",
            if ctx.binding.is_some() {
                format!("{GUIDE}\n{BOUND_GUIDE}")
            } else {
                GUIDE.to_string()
            },
        ),
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

Clovy records meetings and dictations, turns them into notes, keeps memories, and (when activity capture is on) a text-only timeline, day summaries, and coding-agent work. External clients receive only the read tools in this section, from the data set open in Clovy. Bound chat turns read their conversation's host-owned data partition. Activity belongs to the installation, whichever data set is open.

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
| "How was my day in numbers?" (focus per hour, categories, meetings, coding agents) | `get_day_panels` (`date`: YYYY-MM-DD, default today) |
| "When does Clovy write my day summary?" / "Are notifications on?" | `get_day_schedule` |

## Tips

- Read `clovy://context` first: it has today's date, the time, and the time zone. Activity tools take RFC 3339 times with an offset (`2026-10-04T09:00:00-03:00`); omit `from`/`to` for today so far. `get_day_summary` and `get_day_panels` take a date (`YYYY-MM-DD`), default today; a date before the retention period answers `state: "past_retention"`.
- Activity tools exist only while activity capture is on. Apps and sites the user excluded never appear, and nothing older than the retention period is returned.
- Timeline sessions and gaps retain their full original bounds when they overlap [from,to). Totals count only the overlap. Active state comes from the materialized ETL, updated approximately every 30 seconds; it is not an instantaneous capture watermark.
- Long results are capped: look at `truncated`, `hasMore`/`nextOffset`, or `nextTranscriptOffset`, and ask for a shorter period or the next page.
- Clovy must be open with the MCP server turned on (Settings, Agent). Otherwise every tool answers with an error saying so.
"#;

const BOUND_GUIDE: &str = r#"## Management in this bound turn

The catalog is authoritative for this connection. Routine toolsets restrict discovery and dispatch and do not create grants. A project organizes objects within a partition; knowing an ID does not grant access. Stop revokes the current run. After an approval wait, the host checks the live run, partition and target again. The approval broker decides the effective policy and records its receipt before an effect.

- Notes: list_notes/create_note/update_note/delete_note. Read get_note.revision before update_note and supply expectedRevision; note_conflict requires rereading, never blind retry. list_note_transcripts includes all sessions and legacy records, with replaced aggregates omitted by identity, not text. Continue both nextOffset and nextTextOffset. search_notes returns a matching transcript ID and snippet.
- Conversations: list_conversations/get_conversation; list_conversation_messages/search_conversation_messages page human/assistant messages in total run/sequence/ID order, excluding tool payloads. Continue nextCursor and nextTextOffset for long messages. list_conversation_artifacts/read_conversation_artifact require conversationId plus artifactId; UTF-8 pages count bytes and binary content is an explicit error. get_conversation_run/cancel_conversation_run target an exact conversation/run pair. A turn cannot cancel itself or delete its conversation. Creating/managing conversations does not start another model turn.
- Projects: list_projects/get_project/create_project/update_project/delete_project and assign/remove note/conversation project operations. Associations are plural. update_project applies the compound change atomically. delete_project requires explicit deleteNotes and previews affected data; a changed preview needs another approval.
- Routines: CRUD/pause/resume plus run_routine/list_routine_runs. Timing includes an IANA timezone, resolved CLI model and enabledToolsets. Running can return started, busy or blocked; only started means a run was actually admitted. History is paginated. Configuration never creates connector, browser or execution grants. Running a routine can invoke the configured CLI/provider and incur its normal usage.
- Memory: save_memory/update_memory/forget_memory/search_memories/list_memories. Updates preserve identity, source, creation and scope. Disabled or deleted projects are filtered before paging. Changing scope is not supported.
- Activity: get_activity_settings/update_activity_settings/pause_activity/resume_activity/generate_day_summary. Settings work while capture is off and cannot alter internal MCP/default fields. Generation requires local day (YYYY-MM-DD), may call the configured activity provider and preserves provider, empty-data, database and pending-hour errors. These settings belong to the installation. No manual category editing or database rebuild is offered.
- Dictation: list/get/delete_dictation and start_dictation/get_dictation_status/stop_dictation/cancel_dictation. Start pins a take, destination composer and window; status confirms actual helper state. Stop delivers to that same destination; cancel discards. No arbitrary target app or helper command passthrough is accepted. Audio may use the user's configured transcription provider; cleanup may incur OpenRouter usage.
- Recorder: start_recording/recording_status/stop_recording/pause_recording/resume_recording/recover_recording/retry_note_processing/list_recording_artifacts. IDs, source mode and duration refer to the actual session. Recovery is explicitly validate or discard. Finalizing/retrying can use configured transcription and note providers; listing metadata does not grant paths. Recording controls use the real main-window bridge and may need native permissions.
- Skills: list_skills/load_skill describe active skills; list_skill_settings/read_skill also inspect disabled settings for administration. update_skill/enable_skill/disable_skill use approval. Only managed skills are editable; global roots are read-only. Reading presence or content does not execute instructions. Hidden entries and objective editor backups are excluded; legitimate foo-backup names remain valid.

Reads above are local and do not activate capture, login, permissions or a feature. Native mutations and browser input use the approval broker's effective policy; clarification always requires an answer. Browser ownership, network and target guards also apply. Media generation/editing is the existing exception: direct provider usage can be charged without another broker card. Status is not authorization. Inspect error codes and delivery state; never resend an effect whose delivery is uncertain.
"#;
