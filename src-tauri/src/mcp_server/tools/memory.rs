//! Memory mutations share the native settings-locked creation path and the
//! repository's transactional deletion/tombstone path.
use super::{ToolCtx, ToolGroup};
use crate::approval_broker::{ApprovalKind, ApprovalRequest, ApprovalTurn};
use crate::db::repositories::Repositories;
use crate::domain::types::AppError;
use crate::mcp_server::turns::TURN_REQUIRED;
use futures_util::future::BoxFuture;
use serde_json::{json, Value};
use sqlx::{query::query, row::Row};
use std::path::Path;

pub struct MemoryTools;
impl ToolGroup for MemoryTools {
    fn owns(&self, name: &str) -> bool {
        matches!(name, "save_memory" | "forget_memory")
    }
    fn descriptors(&self, _ctx: &ToolCtx<'_>) -> Vec<Value> {
        [
            ("save_memory","Save a durable fact, preference, or decision in Clovy's memory store.",json!({"content":{"type":"string","maxLength":4000},"projectId":{"type":"string"}}),vec!["content"]),
            ("forget_memory","Permanently forget one Clovy memory by id.",json!({"id":{"type":"string"}}),vec!["id"]),
        ].into_iter().map(|(name,description,properties,required)| json!({"name":name,"description":description,"inputSchema":{"type":"object","properties":properties,"required":required,"additionalProperties":false},"annotations":{"readOnlyHint":false,"openWorldHint":false}})).collect()
    }
    fn call<'a>(
        &'a self,
        ctx: &'a ToolCtx<'a>,
        name: &'a str,
        arguments: &'a Value,
    ) -> BoxFuture<'a, Result<Value, AppError>> {
        Box::pin(async move {
            let app = ctx.app.ok_or_else(|| {
                AppError::new(
                    "approval_unavailable",
                    "Clovy's memory settings are unavailable.",
                )
            })?;
            let path = crate::commands::memory_settings_path(app)?;
            call_with_settings(ctx, name, arguments, &path).await
        })
    }
}

/// The app resolves its settings path per call. Tests supply a temporary file,
/// keeping the same broker, validation and persistence path without Tauri.
pub(super) async fn call_with_settings(
    ctx: &ToolCtx<'_>,
    name: &str,
    arguments: &Value,
    settings_path: &Path,
) -> Result<Value, AppError> {
    let binding = ctx
        .binding
        .ok_or_else(|| AppError::new(TURN_REQUIRED, "Memory writes require a Clovy turn."))?;
    ensure_enabled(ctx, settings_path).await?;
    let project_id = match arguments.get("projectId") {
        None => None,
        Some(value) => Some(
            value
                .as_str()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| {
                    AppError::new("invalid_arguments", "projectId must be a non-empty string.")
                })?,
        ),
    };
    let id = if name == "forget_memory" {
        Some(required(arguments, "id")?)
    } else {
        None
    };
    let content = if name == "save_memory" {
        Some(required(arguments, "content")?)
    } else {
        None
    };
    check_scope(ctx, project_id, id).await?;
    ctx.broker()?
        .request(
            &ApprovalTurn::from(binding),
            ApprovalRequest {
                tool: name.into(),
                call_id: None,
                kind: ApprovalKind::Approval {
                    title: if name == "save_memory" {
                        "Save a memory"
                    } else {
                        "Forget a memory"
                    }
                    .into(),
                    description: if name == "save_memory" {
                        "Save the requested fact in Clovy's memory store."
                    } else {
                        "Permanently delete the selected Clovy memory."
                    }
                    .into(),
                    arguments: arguments.clone(),
                    preview: None,
                },
            },
        )
        .await
        .map_err(|denial| AppError::new(denial.code(), denial.message()))?;
    // Settings or project membership may change while the card waits.
    ensure_enabled(ctx, settings_path).await?;
    check_scope(ctx, project_id, id).await?;
    let repos = Repositories::new(ctx.data.notes.clone());
    match name {
        "save_memory" => {
            let memory = crate::commands::create_memory_with_settings(
                &repos,
                settings_path,
                &ctx.data.profile,
                project_id,
                content.unwrap_or_default(),
                "agent",
            )
            .await?;
            serde_json::to_value(memory)
                .map_err(|error| AppError::new("agent_tool_response_invalid", error.to_string()))
        }
        "forget_memory" => {
            let id = id.unwrap_or_default();
            repos.delete_memory(&ctx.data.profile, id).await?;
            Ok(json!({"forgotten":true,"id":id}))
        }
        _ => Err(AppError::new("mcp_tool_unknown", "Unknown memory tool.")),
    }
}

async fn ensure_enabled(ctx: &ToolCtx<'_>, path: &Path) -> Result<(), AppError> {
    if ctx.data.memory_enabled && crate::commands::load_memory_settings(path).enabled {
        Ok(())
    } else {
        Err(AppError::new(
            "memory_disabled",
            "Memory is disabled for this scope.",
        ))
    }
}
async fn check_scope(
    ctx: &ToolCtx<'_>,
    project_id: Option<&str>,
    memory_id: Option<&str>,
) -> Result<(), AppError> {
    let stored_project: Option<String> = if let Some(id) = memory_id {
        let row = query("SELECT folder_id FROM memories WHERE id = ? AND profile = ?")
            .bind(id)
            .bind(&ctx.data.profile)
            .fetch_optional(&ctx.data.notes)
            .await?
            .ok_or_else(|| AppError::new("memory_not_found", "Memory was not found."))?;
        row.get("folder_id")
    } else {
        project_id.map(str::to_string)
    };
    if let Some(project) = stored_project {
        let row = query("SELECT memory_disabled FROM folders WHERE id = ? AND profile = ? AND deleted_at IS NULL").bind(project).bind(&ctx.data.profile).fetch_optional(&ctx.data.notes).await?.ok_or_else(|| AppError::new("folder_not_found","Project was not found or has already been deleted."))?;
        if row.get::<i64, _>("memory_disabled") != 0 {
            return Err(AppError::new(
                "memory_disabled",
                "Memory is disabled for this scope.",
            ));
        }
    }
    Ok(())
}
fn required<'a>(arguments: &'a Value, key: &str) -> Result<&'a str, AppError> {
    arguments
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            AppError::new(
                "agent_tool_arguments_invalid",
                format!("{key} is required."),
            )
        })
}
