//! Shared note/dictation reads and paging. The legacy agent keeps byte offsets;
//! MCP pages use character offsets and apply the MCP dictation retention floor.
use super::{ToolCtx, ToolGroup};
use crate::db::repositories::dictation_history_cutoff_timestamp;
use crate::domain::types::AppError;
use crate::mcp_server::turns::TURN_REQUIRED;
use futures_util::future::BoxFuture;
use serde_json::{json, Value};
use sqlx::{query::query, row::Row};
use sqlx_sqlite::{SqlitePool, SqliteRow};

pub struct NoteTools;
pub const GET_DICTATION: &str = "get_dictation";
const DICTATION_PAGE_CHARS: usize = 40_000;

impl ToolGroup for NoteTools {
    fn owns(&self, name: &str) -> bool {
        name == GET_DICTATION
    }
    fn descriptors(&self, _ctx: &ToolCtx<'_>) -> Vec<Value> {
        vec![
            json!({"name":GET_DICTATION,"description":"Read a retained Clovy dictation by id. Continue with nextTextOffset; offsets count characters.","inputSchema":{"type":"object","properties":{"id":{"type":"string"},"textOffset":{"type":"integer","minimum":0,"default":0,"description":"Character offset into the dictation."}},"required":["id"],"additionalProperties":false},"annotations":{"readOnlyHint":true,"openWorldHint":false,"idempotentHint":true}}),
        ]
    }
    fn catalog<'a>(&'a self, ctx: &'a ToolCtx<'a>) -> BoxFuture<'a, Vec<Value>> {
        Box::pin(async move {
            let Some(binding) = ctx.binding else {
                return Vec::new();
            };
            match crate::routines::routine_tool_allowed_for_session(
                &ctx.data.notes,
                &binding.session_id,
                GET_DICTATION,
            )
            .await
            {
                Ok(Some(false)) => Vec::new(),
                Ok(_) => self.descriptors(ctx),
                Err(error) => {
                    tracing::warn!(code = %error.code, "Dictation tool permissions unavailable");
                    Vec::new()
                }
            }
        })
    }
    fn call<'a>(
        &'a self,
        ctx: &'a ToolCtx<'a>,
        _name: &'a str,
        arguments: &'a Value,
    ) -> BoxFuture<'a, Result<Value, AppError>> {
        Box::pin(async move {
            let binding = ctx.binding.ok_or_else(|| {
                AppError::new(TURN_REQUIRED, "Dictation reads require a Clovy turn.")
            })?;
            // Recheck at dispatch: a routine's toolsets may have changed
            // since its CLI received the catalog.
            if crate::routines::routine_tool_allowed_for_session(
                &ctx.data.notes,
                &binding.session_id,
                GET_DICTATION,
            )
            .await?
                == Some(false)
            {
                return Err(AppError::new(
                    "routine_tool_not_enabled",
                    "This tool is not enabled for the routine.",
                ));
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

pub(crate) async fn read_note(
    pool: &SqlitePool,
    profile: &str,
    id: &str,
) -> Result<SqliteRow, AppError> {
    query("SELECT n.id, n.title, COALESCE(n.edited_content, n.generated_content, '') AS note,
                COALESCE((SELECT text FROM transcripts t WHERE t.note_id = n.id ORDER BY t.created_at DESC LIMIT 1), '') AS transcript,
                n.created_at, n.updated_at
         FROM notes n WHERE n.id = ? AND n.profile = ?")
        .bind(id).bind(profile).fetch_optional(pool).await?
        .ok_or_else(|| AppError::new("note_not_found","No note with this id in the active profile."))
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
