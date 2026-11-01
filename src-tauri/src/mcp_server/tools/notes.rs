//! Shared note/dictation reads and paging. The legacy agent keeps byte offsets;
//! MCP pages use character offsets and apply the MCP dictation retention floor.
use super::{ToolCtx, ToolGroup};
use crate::approval_broker::{ApprovalKind, ApprovalRequest, ApprovalTurn};
use crate::db::repositories::dictation_history_cutoff_timestamp;
use crate::db::repositories::Repositories;
use crate::domain::types::AppError;
use crate::mcp_server::turns::TURN_REQUIRED;
use futures_util::future::BoxFuture;
use serde_json::{json, Value};
use sqlx::{query::query, row::Row};
use sqlx_sqlite::{SqlitePool, SqliteRow};

pub struct NoteTools;
pub const GET_DICTATION: &str = "get_dictation";
pub(crate) use crate::db::repositories::AUTHORITATIVE_TRANSCRIPT;
const DICTATION_PAGE_CHARS: usize = 40_000;

impl ToolGroup for NoteTools {
    fn owns(&self, name: &str) -> bool {
        matches!(
            name,
            GET_DICTATION | "list_notes" | "create_note" | "update_note" | "delete_note"
        )
    }
    fn descriptors(&self, _ctx: &ToolCtx<'_>) -> Vec<Value> {
        vec![
            json!({"name":GET_DICTATION,"description":"Read a retained Clovy dictation by id. Continue with nextTextOffset; offsets count characters.","inputSchema":{"type":"object","properties":{"id":{"type":"string"},"textOffset":{"type":"integer","minimum":0,"default":0,"description":"Character offset into the dictation."}},"required":["id"],"additionalProperties":false},"annotations":{"readOnlyHint":true,"openWorldHint":false,"idempotentHint":true}}),
            json!({"name":"list_notes","description":"List notes in the bound data partition.","inputSchema":{"type":"object","properties":{"projectId":{"type":"string"},"limit":{"type":"integer","minimum":1,"maximum":100},"cursor":{"type":"string"}},"additionalProperties":false},"annotations":{"readOnlyHint":true,"openWorldHint":false}}),
            json!({"name":"create_note","description":"Create a note, optionally in a project.","inputSchema":{"type":"object","properties":{"projectId":{"type":"string"}},"additionalProperties":false},"annotations":{"readOnlyHint":false,"openWorldHint":false}}),
            json!({"name":"update_note","description":"Update title or content only if expectedRevision matches.","inputSchema":{"type":"object","properties":{"id":{"type":"string"},"expectedRevision":{"type":"integer","minimum":0},"title":{"type":"string"},"content":{"type":"string"}},"required":["id","expectedRevision"],"additionalProperties":false},"annotations":{"readOnlyHint":false,"openWorldHint":false}}),
            json!({"name":"delete_note","description":"Delete a note and its recording files.","inputSchema":{"type":"object","properties":{"id":{"type":"string"}},"required":["id"],"additionalProperties":false},"annotations":{"readOnlyHint":false,"openWorldHint":false,"destructiveHint":true}}),
        ]
    }
    fn catalog<'a>(&'a self, ctx: &'a ToolCtx<'a>) -> BoxFuture<'a, Vec<Value>> {
        Box::pin(async move {
            let Some(binding) = ctx.binding else {
                return Vec::new();
            };
            let mut visible = Vec::new();
            for descriptor in self.descriptors(ctx) {
                let Some(name) = descriptor["name"].as_str() else {
                    continue;
                };
                if super::routine_tool_allowed(&ctx.data.notes, binding, name)
                    .await
                    .unwrap_or(false)
                {
                    visible.push(descriptor);
                }
            }
            visible
        })
    }
    fn call<'a>(
        &'a self,
        ctx: &'a ToolCtx<'a>,
        name: &'a str,
        arguments: &'a Value,
    ) -> BoxFuture<'a, Result<Value, AppError>> {
        Box::pin(async move {
            let binding = ctx.binding.ok_or_else(|| {
                AppError::new(TURN_REQUIRED, "Dictation reads require a Clovy turn.")
            })?;
            // Recheck at dispatch: a routine's toolsets may have changed
            // since its CLI received the catalog.
            if !super::routine_tool_allowed(&ctx.data.notes, binding, name).await? {
                return Err(AppError::new(
                    "routine_tool_not_enabled",
                    "This tool is not enabled for the routine.",
                ));
            }
            if name != GET_DICTATION {
                return note_operation(ctx, name, arguments).await;
            }
            let id = arguments
                .get("id")
                .and_then(Value::as_str)
                .filter(|id| !id.trim().is_empty())
                .ok_or_else(|| AppError::new("agent_tool_arguments_invalid", "id is required."))?;
            let offset = character_offset(arguments, "textOffset")?;
            let cutoff = dictation_history_cutoff_timestamp();
            let row = read_dictation(&ctx.data.notes, &ctx.data.profile, id, Some(&cutoff)).await?;
            let text: String = row.get("text");
            let (page, total, next) = character_page(&text, offset, DICTATION_PAGE_CHARS);
            Ok(
                json!({"id":row.get::<String,_>("id"),"language":row.get::<Option<String>,_>("language"),"createdAt":row.get::<String,_>("created_at"),"text":page,"textOffset":offset,"textTotalChars":total,"nextTextOffset":next}),
            )
        })
    }
}

async fn note_operation(ctx: &ToolCtx<'_>, name: &str, args: &Value) -> Result<Value, AppError> {
    let binding = ctx
        .binding
        .ok_or_else(|| AppError::new(TURN_REQUIRED, "A bound turn is required."))?;
    let repos = Repositories::new(ctx.data.notes.clone());
    let object = args.as_object().ok_or_else(|| {
        AppError::new(
            "agent_tool_arguments_invalid",
            "Arguments must be an object.",
        )
    })?;
    let allowed: &[&str] = match name {
        "list_notes" => &["projectId", "limit", "cursor"],
        "create_note" => &["projectId"],
        "update_note" => &["id", "expectedRevision", "title", "content"],
        "delete_note" => &["id"],
        _ => return Err(AppError::new("mcp_tool_unknown", "Unknown note tool.")),
    };
    if object.keys().any(|key| !allowed.contains(&key.as_str())) {
        return Err(AppError::new(
            "agent_tool_arguments_invalid",
            "Unexpected argument.",
        ));
    }
    for key in ["projectId", "cursor", "title", "content"] {
        if let Some(value) = object.get(key) {
            if !value.is_string() {
                return Err(AppError::new(
                    "agent_tool_arguments_invalid",
                    format!("{key} must be a string."),
                ));
            }
        }
    }
    if let Some(limit) = object.get("limit") {
        if !limit.as_u64().is_some_and(|n| (1..=100).contains(&n)) {
            return Err(AppError::new(
                "agent_tool_arguments_invalid",
                "limit must be between 1 and 100.",
            ));
        }
    }
    let project = args.get("projectId").and_then(Value::as_str);
    if let Some(id) = project {
        let exists: i64 = sqlx::query_scalar::query_scalar(
            "SELECT COUNT(*) FROM folders WHERE id = ? AND profile = ? AND deleted_at IS NULL",
        )
        .bind(id)
        .bind(&ctx.data.profile)
        .fetch_one(&ctx.data.notes)
        .await?;
        if exists == 0 {
            return Err(AppError::new("folder_not_found", "Project was not found."));
        }
    }
    if name == "list_notes" {
        let limit = args
            .get("limit")
            .and_then(Value::as_i64)
            .unwrap_or(20)
            .clamp(1, 100);
        let result = repos
            .list_notes(
                &ctx.data.profile,
                project.map(str::to_string),
                limit,
                args.get("cursor")
                    .and_then(Value::as_str)
                    .map(str::to_string),
            )
            .await?;
        return serde_json::to_value(result)
            .map_err(|e| AppError::new("agent_tool_response_invalid", e.to_string()));
    }
    let id = if name == "create_note" {
        None
    } else {
        Some(
            args.get("id")
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty())
                .ok_or_else(|| AppError::new("agent_tool_arguments_invalid", "id is required."))?,
        )
    };
    if let Some(id) = id {
        read_note(&ctx.data.notes, &ctx.data.profile, id).await?;
    }
    let revision = if name == "update_note" {
        Some(
            args.get("expectedRevision")
                .and_then(Value::as_i64)
                .filter(|n| *n >= 0)
                .ok_or_else(|| {
                    AppError::new(
                        "agent_tool_arguments_invalid",
                        "expectedRevision is required.",
                    )
                })?,
        )
    } else {
        None
    };
    if name == "update_note"
        && args.get("title").and_then(Value::as_str).is_none()
        && args.get("content").and_then(Value::as_str).is_none()
    {
        return Err(AppError::new(
            "agent_tool_arguments_invalid",
            "title or content is required.",
        ));
    }
    ctx.broker()?
        .request(
            &ApprovalTurn::from(binding),
            ApprovalRequest {
                tool: name.into(),
                call_id: Some(uuid::Uuid::new_v4().to_string()),
                kind: ApprovalKind::Approval {
                    title: match name {
                        "create_note" => "Create a note",
                        "update_note" => "Update a note",
                        _ => "Delete a note",
                    }
                    .into(),
                    description: "Change notes in the bound data partition.".into(),
                    arguments: args.clone(),
                    preview: None,
                },
            },
        )
        .await
        .map_err(|e| AppError::new(e.code(), e.message()))?;
    ctx.revalidate().await?;
    let profile = ctx.data.profile.clone();
    if profile != ctx.data.profile {
        return Err(AppError::new(
            "turn_expired",
            "The bound data partition changed.",
        ));
    }
    let active: i64 = sqlx::query_scalar::query_scalar("SELECT COUNT(*) FROM agent_runs WHERE id = ? AND session_id = ? AND status IN ('queued', 'running', 'waiting_for_user')")
        .bind(&binding.run_id).bind(&binding.session_id).fetch_one(&ctx.data.notes).await?;
    if active == 0 {
        return Err(AppError::new(
            "turn_expired",
            "The bound run is no longer active.",
        ));
    }
    if !super::routine_tool_allowed(&ctx.data.notes, binding, name).await? {
        return Err(AppError::new(
            "routine_tool_not_enabled",
            "This tool is not enabled for the routine.",
        ));
    }
    if let Some(project) = project {
        let exists: i64 = sqlx::query_scalar::query_scalar(
            "SELECT COUNT(*) FROM folders WHERE id = ? AND profile = ? AND deleted_at IS NULL",
        )
        .bind(project)
        .bind(&profile)
        .fetch_one(&ctx.data.notes)
        .await?;
        if exists == 0 {
            return Err(AppError::new("folder_not_found", "Project was not found."));
        }
    }
    let result = match name {
        "create_note" => serde_json::to_value(
            repos
                .create_note_checked(
                    &profile,
                    project,
                    Some((&binding.session_id, &binding.run_id, &binding.authority)),
                )
                .await?,
        )
        .map_err(|e| AppError::new("agent_tool_response_invalid", e.to_string()))?,
        "update_note" => serde_json::to_value(
            repos
                .update_note_if_revision(
                    &profile,
                    id.unwrap_or_default(),
                    revision.unwrap_or_default(),
                    args.get("title").and_then(Value::as_str),
                    args.get("content").and_then(Value::as_str),
                    Some((&binding.session_id, &binding.run_id, &binding.authority)),
                )
                .await?,
        )
        .map_err(|e| AppError::new("agent_tool_response_invalid", e.to_string()))?,
        "delete_note" => {
            let app = ctx.app.ok_or_else(|| {
                AppError::new("storage_unavailable", "Recording storage is unavailable.")
            })?;
            let data_dir = crate::app_paths::app_data_dir(app)
                .map_err(|e| AppError::new("storage_unavailable", e.to_string()))?;
            let paths = crate::app_paths::AppPaths::from_data_dir(data_dir)
                .map_err(|e| AppError::new("storage_unavailable", e.to_string()))?;
            let files = repos
                .delete_note_in_profile(
                    &profile,
                    id.unwrap_or_default(),
                    Some((&binding.session_id, &binding.run_id, &binding.authority)),
                )
                .await?;
            let mut cleanup_failures = 0;
            for file in files {
                if let Err(error) = paths.remove_recording_file(file) {
                    cleanup_failures += 1;
                    tracing::warn!(%error, "Failed to remove deleted note audio");
                }
            }
            json!({"deleted":true,"id":id,"audioCleanupFailures":cleanup_failures})
        }
        _ => return Err(AppError::new("mcp_tool_unknown", "Unknown note tool.")),
    };
    if let Some(app) = ctx.app {
        use tauri::Emitter;
        let _ = app.emit(
            "clovy-data-changed",
            json!({"profile":profile,"kind":"notes","id":result.get("id"),"deleted":name == "delete_note"}),
        );
    }
    Ok(result)
}

pub(crate) async fn read_note(
    pool: &SqlitePool,
    profile: &str,
    id: &str,
) -> Result<SqliteRow, AppError> {
    query(
        "SELECT n.id, n.title, COALESCE(n.edited_content, n.generated_content, '') AS note,
                n.created_at, n.updated_at, n.revision
         FROM notes n WHERE n.id = ? AND n.profile = ?",
    )
    .bind(id)
    .bind(profile)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| {
        AppError::new(
            "note_not_found",
            "No note with this id in the active profile.",
        )
    })
}

pub(crate) async fn authoritative_transcripts(
    pool: &SqlitePool,
    note_id: &str,
    profile: &str,
    offset: i64,
    limit: i64,
) -> Result<Vec<SqliteRow>, AppError> {
    Ok(query(&format!("SELECT t.id, t.recording_session_id, t.source, t.start_ms, t.end_ms, t.turn_index, t.text, t.status, t.last_error, t.created_at FROM transcripts t JOIN notes n ON n.id = t.note_id LEFT JOIN recording_sessions rs ON rs.id = t.recording_session_id WHERE t.note_id = ? AND n.profile = ? AND {AUTHORITATIVE_TRANSCRIPT} ORDER BY COALESCE(rs.started_at, t.created_at), COALESCE(t.recording_session_id, t.id), COALESCE(t.start_ms, 0), COALESCE(t.turn_index, -1), t.created_at, t.id LIMIT ? OFFSET ?"))
        .bind(note_id).bind(profile).bind(limit).bind(offset).fetch_all(pool).await?)
}

pub(crate) async fn read_dictation(
    pool: &SqlitePool,
    profile: &str,
    id: &str,
    cutoff: Option<&str>,
) -> Result<SqliteRow, AppError> {
    query("SELECT id, text, language, created_at FROM dictation_history WHERE id = ? AND profile = ? AND (? IS NULL OR created_at >= ?)")
        .bind(id).bind(profile).bind(cutoff).bind(cutoff).fetch_optional(pool).await?
        .ok_or_else(|| AppError::new("dictation_not_found","No dictation with this id in the active profile."))
}

pub(super) fn character_offset(arguments: &Value, key: &str) -> Result<usize, AppError> {
    match arguments.get(key) {
        None | Some(Value::Null) => Ok(0),
        Some(value) => value
            .as_u64()
            .and_then(|offset| usize::try_from(offset).ok())
            .ok_or_else(|| {
                AppError::new(
                    "agent_tool_arguments_invalid",
                    format!("{key} must be a non-negative integer."),
                )
            }),
    }
}

pub(super) fn character_page(
    text: &str,
    offset: usize,
    limit: usize,
) -> (String, usize, Option<usize>) {
    let total = text.chars().count();
    let page: String = text.chars().skip(offset).take(limit).collect();
    let end = offset.saturating_add(limit);
    (page, total, (end < total).then_some(end))
}
