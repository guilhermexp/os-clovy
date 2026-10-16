use super::skill_identity::{canonical_skill_id, reconcile_managed_obsidian_skill};
use super::{AgentRepository, AgentSafetyMode};
use crate::domain::types::AppError;
use crate::mcp_server::tools::media::{self, parse_video_call, VideoCall, VideoJobOutcome};
use serde_json::{json, Value};
use sqlx::{query::query, row::Row};
use std::{
    fs,
    future::Future,
    path::{Path, PathBuf},
    process::Stdio,
    sync::atomic::{AtomicU64, Ordering},
};
use tauri::{AppHandle, Manager};
use tokio::{io::AsyncReadExt, process::Command, sync::oneshot};

const MAX_TOOL_OUTPUT_BYTES: usize = 1_048_576;
/// Bytes of a note's content or transcript one `get_note` page returns.
const NOTE_PAGE_BYTES: usize = 256 * 1_024;
/// Characters of each note, transcript, or dictation a search returns.
const SEARCH_EXCERPT_CHARS: usize = 4_000;
/// How long to keep draining shell pipes after the process group is swept;
/// only a helper that escaped the group could still hold a pipe open.
const SHELL_PIPE_DRAIN_GRACE: std::time::Duration = std::time::Duration::from_millis(500);
const MAX_SEARCH_FILE_BYTES: u64 = 1_048_576;
const MAX_SEARCH_SCANNED_BYTES: u64 = 32 * 1_048_576;
const MAX_SEARCH_FILES: usize = 10_000;
const MAX_SEARCH_MATCHES: usize = 200;

#[derive(Clone)]
pub struct ToolContext {
    pub app: AppHandle,
    pub repository: AgentRepository,
    pub workspace: PathBuf,
    pub safety_mode: AgentSafetyMode,
    pub session_id: String,
    pub run_id: String,
    pub cancellations: ToolCancellationRegistry,
    pub call_id: Option<String>,
}

#[derive(Clone, Default)]
pub struct ToolCancellationRegistry {
    inner: std::sync::Arc<std::sync::Mutex<CancellationSenders>>,
    next_id: std::sync::Arc<AtomicU64>,
}

type CancellationSenders = std::collections::HashMap<String, Vec<(u64, oneshot::Sender<()>)>>;

pub(crate) struct ToolCancellationRegistration {
    receiver: oneshot::Receiver<()>,
    inner: std::sync::Arc<std::sync::Mutex<CancellationSenders>>,
    run_id: String,
    id: u64,
}

impl Drop for ToolCancellationRegistration {
    fn drop(&mut self) {
        let mut entries = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let remove_run = if let Some(senders) = entries.get_mut(&self.run_id) {
            senders.retain(|(id, _)| *id != self.id);
            senders.is_empty()
        } else {
            false
        };
        if remove_run {
            entries.remove(&self.run_id);
        }
    }
}

impl ToolCancellationRegistry {
    pub(crate) async fn register(&self, run_id: &str) -> ToolCancellationRegistration {
        let (send, receive) = oneshot::channel();
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .entry(run_id.to_string())
            .or_default()
            .push((id, send));
        ToolCancellationRegistration {
            receiver: receive,
            inner: self.inner.clone(),
            run_id: run_id.to_string(),
            id,
        }
    }
    pub async fn cancel(&self, run_id: &str) {
        let senders = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(run_id);
        if let Some(senders) = senders {
            for (_, sender) in senders {
                let _ = sender.send(());
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn registration_count(&self, run_id: &str) -> usize {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(run_id)
            .map_or(0, Vec::len)
    }
}

impl ToolCancellationRegistration {
    pub(crate) async fn cancelled(&mut self) {
        let _ = (&mut self.receiver).await;
    }
}

pub async fn dispatch_tool(
    context: &ToolContext,
    name: &str,
    arguments: Value,
) -> Result<Value, AppError> {
    if !name.starts_with("mcp_")
        && crate::routines::routine_tool_allowed_for_session(
            &context.repository.pool,
            &context.session_id,
            name,
        )
        .await?
            == Some(false)
    {
        return Err(AppError::new(
            "routine_tool_not_enabled",
            "This tool is not enabled for the routine.",
        ));
    }
    if let Some(result) =
        crate::agent_runtime::native_connectors::dispatch(&context.app, name, arguments.clone())
            .await?
    {
        return Ok(result);
    }
    match name {
        "search_june" => search_clovy_notes(context, &arguments).await,
        "get_note" => get_clovy_note(context, &arguments).await,
        "get_dictation" => get_clovy_dictation(context, &arguments).await,
        "list_memories" => list_memories(context, &arguments).await,
        "save_memory" => save_memory(context, &arguments).await,
        "forget_memory" => forget_memory(context, &arguments).await,
        "generate_image" => generate_image(context, &arguments).await,
        "edit_image" => edit_image(context, &arguments).await,
        "generate_video" => generate_video(context, &arguments).await,
        "get_obsidian_vault" => crate::mcp_server::tools::vault_skills::vault_result(
            crate::obsidian::discovery_for_app(&context.app),
        ),
        "start_recording" => {
            let source_mode = crate::agent_recorder::source_mode_argument(&arguments);
            context
                .app
                .state::<crate::agent_recorder::AgentRecorderBroker>()
                .request(
                    &context.app,
                    crate::agent_recorder::AgentRecorderAction::Start,
                    Some(source_mode),
                )
                .await
        }
        "stop_recording" => {
            context
                .app
                .state::<crate::agent_recorder::AgentRecorderBroker>()
                .request(
                    &context.app,
                    crate::agent_recorder::AgentRecorderAction::Stop,
                    None,
                )
                .await
        }
        "recording_status" => crate::mcp_server::tools::recorder::recording_status(),
        "web_search" => web(context, "/v1/web/search", &arguments).await,
        "web_fetch" => web(context, "/v1/web/fetch", &arguments).await,
        "list_files" => list_files(context, &arguments).await,
        "read_file" => read_file(context, &arguments).await,
        "write_file" => write_file(context, &arguments).await,
        "patch_file" => patch_file(context, &arguments).await,
        "import_file" => import_file(context, &arguments).await,
        "preview_file" => preview_file(context, &arguments).await,
        "search_files" => search_files(context, &arguments).await,
        "run_shell" => run_shell(context, &arguments).await,
        "list_skills" => list_skills(context).await,
        "load_skill" => load_skill(context, &arguments).await,
        "list_routines" => {
            serde_json::to_value(crate::routines::list(&context.repository.pool).await?)
                .map_err(|error| AppError::new("agent_tool_response_invalid", error.to_string()))
        }
        "create_routine" => create_routine(context, arguments).await,
        "update_routine" => update_routine(context, arguments).await,
        "pause_routine" => pause_routine(context, &arguments).await,
        "resume_routine" => resume_routine(context, &arguments).await,
        "delete_routine" => {
            let routine_id = required_string(&arguments, "routineId")?;
            crate::routines::delete(&context.repository.pool, routine_id).await?;
            Ok(json!({ "deleted": true, "routineId": routine_id }))
        }
        "request_clarification" => consume_clarification_answer(context).await,
        name if crate::activity::timeline::agent_tools::is_activity_tool(name) => {
            crate::activity::timeline::dispatch_agent_tool(&context.app, name, &arguments).await
        }
        "request_secret" => consume_secret_reference(context).await,
        "computer_use" => Ok(crate::computer_use::handle_proxy_action_for_call(
            &context.app,
            arguments,
            &context.session_id,
            context.call_id.as_deref(),
        )
        .await),
        name if crate::connectors::notion::runtime_name_to_provider(name).is_some() => {
            notion_tool(context, name, &arguments).await
        }
        name if crate::mcp_server::tools::browser::is_browser_tool(name) => {
            let broker = context
                .app
                .state::<std::sync::Arc<crate::browser_broker::BrowserBroker>>();
            crate::mcp_server::tools::browser::execute_attended(&broker, name, arguments).await
        }
        name if name.starts_with("mcp_") => mcp_tool(context, name, arguments).await,
        // These capabilities stay behind Rust-owned seams. Their existing brokers
        // can be connected without granting the runtime direct credentials or UI access.
        name if name.starts_with("browser_")
            || name.starts_with("computer_")
            || name.starts_with("connector_") =>
        {
            Err(AppError::new(
                "agent_tool_unavailable",
                format!("{name} is not enabled for this runtime yet."),
            ))
        }
        _ => Err(AppError::new(
            "agent_tool_unsupported",
            format!("Unsupported agent tool: {name}"),
        )),
    }
}

async fn create_routine(context: &ToolContext, arguments: Value) -> Result<Value, AppError> {
    let request: crate::routines::CreateAgentRoutineRequest = serde_json::from_value(arguments)
        .map_err(|error| AppError::new("invalid_arguments", error.to_string()))?;
    serde_json::to_value(crate::routines::create(&context.repository.pool, request).await?)
        .map_err(|error| AppError::new("agent_tool_response_invalid", error.to_string()))
}

async fn update_routine(context: &ToolContext, arguments: Value) -> Result<Value, AppError> {
    let request: crate::routines::UpdateAgentRoutineRequest = serde_json::from_value(arguments)
        .map_err(|error| AppError::new("invalid_arguments", error.to_string()))?;
    serde_json::to_value(crate::routines::update(&context.repository.pool, request).await?)
        .map_err(|error| AppError::new("agent_tool_response_invalid", error.to_string()))
}

async fn pause_routine(context: &ToolContext, arguments: &Value) -> Result<Value, AppError> {
    let routine_id = required_string(arguments, "routineId")?;
    serde_json::to_value(crate::routines::pause(&context.repository.pool, routine_id).await?)
        .map_err(|error| AppError::new("agent_tool_response_invalid", error.to_string()))
}

async fn resume_routine(context: &ToolContext, arguments: &Value) -> Result<Value, AppError> {
    let routine_id = required_string(arguments, "routineId")?;
    serde_json::to_value(crate::routines::resume(&context.repository.pool, routine_id).await?)
        .map_err(|error| AppError::new("agent_tool_response_invalid", error.to_string()))
}

async fn mcp_tool(context: &ToolContext, name: &str, arguments: Value) -> Result<Value, AppError> {
    let subsystem = crate::agent_mcp::AgentMcpSubsystem::new(
        crate::agent_mcp::AgentMcpRepository::new(context.repository.pool.clone()),
        crate::agent_mcp::KeychainMcpSecretStore,
    );
    subsystem
        .refresh_registry_for_workspace_with_managed_linear(
            &context.app,
            context.safety_mode == AgentSafetyMode::Sandboxed,
            Some(&context.workspace),
        )
        .await
        .map_err(agent_mcp_error)?;
    let current_policy = subsystem
        .policy_for_tool(name)
        .map_err(agent_mcp_error)?
        .ok_or_else(|| AppError::new("agent_mcp_tool_failed", "MCP tool is unavailable."))?;
    let server_name = subsystem
        .server_name_for_tool(name)
        .map_err(agent_mcp_error)?
        .ok_or_else(|| AppError::new("agent_mcp_tool_failed", "MCP tool is unavailable."))?;
    if crate::routines::routine_mcp_server_allowed_for_session(
        &context.repository.pool,
        &context.session_id,
        &current_policy.server_id,
        &server_name,
        current_policy.requires_approval,
    )
    .await?
        == Some(false)
    {
        return Err(AppError::new(
            "routine_tool_not_enabled",
            "This MCP server is not enabled for the routine.",
        ));
    }
    if !crate::agent_mcp::run_policy_matches(
        &context.repository.pool,
        &context.run_id,
        name,
        &current_policy,
    )
    .await
    .map_err(agent_mcp_error)?
    {
        return Err(AppError::new(
            "agent_mcp_policy_changed",
            "This MCP server changed after the run started. Retry the turn to use its current approval policy.",
        ));
    }
    let elicitation_answer = latest_mcp_elicitation_answer(context).await?;
    let invocation = subsystem.invoke_in_workspace_with_elicitation_and_managed_linear(
        &context.app,
        name,
        arguments,
        context.safety_mode == AgentSafetyMode::Sandboxed,
        Some(&context.workspace),
        elicitation_answer
            .as_ref()
            .map(|(_, answer)| answer.as_str()),
    );
    let mut cancelled = context.cancellations.register(&context.run_id).await;
    tokio::select! {
        result = invocation => match result {
            Ok(value) => {
                if let Some((item_id, _)) = elicitation_answer {
                    mark_mcp_elicitation_answer_consumed(context, &item_id).await?;
                }
                Ok(value)
            }
            Err(crate::agent_mcp::AgentMcpError::ElicitationRequired(message)) => Ok(json!({
                "elicitationRequired": true,
                "clarificationQuestion": format!("MCP server {server_name} asks: {message}"),
                "instruction": "Call request_clarification with clarificationQuestion exactly, then retry this MCP tool with the same arguments."
            })),
            Err(error) => Err(agent_mcp_error(error)),
        },
        _ = &mut cancelled.receiver => {
            crate::agent_mcp::retire_server_sessions(&current_policy.server_id).await;
            Err(AppError::new("agent_tool_cancelled", "MCP tool call was cancelled."))
        }
    }
}

async fn latest_mcp_elicitation_answer(
    context: &ToolContext,
) -> Result<Option<(String, String)>, AppError> {
    let row = query("SELECT id, payload_json FROM agent_items WHERE run_id = ? AND kind = 'interruption' AND json_extract(payload_json, '$.kind') = 'clarification' AND json_extract(payload_json, '$.question') LIKE 'MCP server % asks: %' AND json_extract(payload_json, '$.answer') IS NOT NULL AND COALESCE(json_extract(payload_json, '$.mcpAnswerConsumed'), 0) = 0 ORDER BY created_at DESC LIMIT 1")
        .bind(&context.run_id)
        .fetch_optional(&context.repository.pool)
        .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let payload: Value = serde_json::from_str(&row.get::<String, _>("payload_json"))
        .map_err(|error| AppError::new("agent_interruption_invalid", error.to_string()))?;
    Ok(payload
        .get("answer")
        .and_then(Value::as_str)
        .map(|answer| (row.get("id"), answer.to_string())))
}

async fn mark_mcp_elicitation_answer_consumed(
    context: &ToolContext,
    item_id: &str,
) -> Result<(), AppError> {
    let row = query("SELECT payload_json FROM agent_items WHERE id = ? AND run_id = ?")
        .bind(item_id)
        .bind(&context.run_id)
        .fetch_one(&context.repository.pool)
        .await?;
    let mut payload: Value = serde_json::from_str(&row.get::<String, _>("payload_json"))
        .map_err(|error| AppError::new("agent_interruption_invalid", error.to_string()))?;
    payload["mcpAnswerConsumed"] = Value::Bool(true);
    query("UPDATE agent_items SET payload_json = ? WHERE id = ?")
        .bind(payload.to_string())
        .bind(item_id)
        .execute(&context.repository.pool)
        .await?;
    Ok(())
}

fn agent_mcp_error(error: crate::agent_mcp::AgentMcpError) -> AppError {
    AppError::new("agent_mcp_tool_failed", error.to_string())
}

async fn search_clovy_notes(context: &ToolContext, arguments: &Value) -> Result<Value, AppError> {
    let query_text = required_string(arguments, "query")?;
    let profile = crate::commands::active_profile(&context.app);
    search_clovy_notes_for_profile(&context.repository.pool, &profile, query_text).await
}

async fn search_clovy_notes_for_profile(
    pool: &sqlx_sqlite::SqlitePool,
    profile: &str,
    query_text: &str,
) -> Result<Value, AppError> {
    let pattern = format!(
        "%{}%",
        query_text
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_")
    );
    let rows = query(
        "SELECT n.id, n.title, COALESCE(n.edited_content, n.generated_content, '') AS note,
                COALESCE((SELECT text FROM transcripts t WHERE t.note_id = n.id ORDER BY t.created_at DESC LIMIT 1), '') AS transcript,
                n.updated_at
         FROM notes n
         WHERE n.profile = ? AND (
            n.title LIKE ? ESCAPE '\\' OR n.generated_content LIKE ? ESCAPE '\\'
            OR n.edited_content LIKE ? ESCAPE '\\'
            OR EXISTS (SELECT 1 FROM transcripts t WHERE t.note_id = n.id AND t.text LIKE ? ESCAPE '\\')
         )
         ORDER BY n.updated_at DESC LIMIT 20",
    )
    .bind(profile).bind(&pattern).bind(&pattern).bind(&pattern).bind(&pattern)
    .fetch_all(pool).await?;
    let notes: Vec<Value> = rows
        .into_iter()
        .map(|row| {
            json!({
                "id": row.get::<String, _>("id"), "title": row.get::<String, _>("title"),
                "note": search_excerpt(&row.get::<String, _>("note"), query_text),
                "transcript": search_excerpt(&row.get::<String, _>("transcript"), query_text),
                "updatedAt": row.get::<String, _>("updated_at")
            })
        })
        .collect();
    let dictations = query("SELECT id, text, language, created_at FROM dictation_history WHERE profile = ? AND text LIKE ? ESCAPE '\\' ORDER BY created_at DESC LIMIT 20")
        .bind(profile).bind(&pattern).fetch_all(pool).await?;
    let dictations: Vec<Value> = dictations.into_iter().map(|row| json!({
        "id": row.get::<String, _>("id"), "text": search_excerpt(&row.get::<String, _>("text"), query_text),
        "language": row.get::<Option<String>, _>("language"), "createdAt": row.get::<String, _>("created_at")
    })).collect();
    Ok(json!({ "notes": notes, "dictations": dictations }))
}

async fn get_clovy_note(context: &ToolContext, arguments: &Value) -> Result<Value, AppError> {
    let note_id = required_string(arguments, "id")?;
    let profile = crate::commands::active_profile(&context.app);
    get_clovy_note_for_profile(
        &context.repository.pool,
        &profile,
        note_id,
        byte_offset(arguments, "contentOffset")?,
        byte_offset(arguments, "transcriptOffset")?,
    )
    .await
}

/// One note of the active profile with its latest transcript. Each text comes
/// in pages of at most `NOTE_PAGE_BYTES`, cut on character boundaries; a
/// `next*Offset` names the byte offset of the following page.
async fn get_clovy_note_for_profile(
    pool: &sqlx_sqlite::SqlitePool,
    profile: &str,
    note_id: &str,
    content_offset: usize,
    transcript_offset: usize,
) -> Result<Value, AppError> {
    let row = crate::mcp_server::tools::notes::read_note(pool, profile, note_id).await?;
    let note: String = row.get("note");
    let transcript: String = row.get("transcript");
    let (content, content_offset, next_content_offset) = text_page(&note, content_offset);
    let (transcript_page, transcript_offset, next_transcript_offset) =
        text_page(&transcript, transcript_offset);
    Ok(json!({
        "id": row.get::<String, _>("id"),
        "title": row.get::<String, _>("title"),
        "updatedAt": row.get::<String, _>("updated_at"),
        "content": content,
        "contentOffset": content_offset,
        "contentTotalBytes": note.len(),
        "nextContentOffset": next_content_offset,
        "transcript": transcript_page,
        "transcriptOffset": transcript_offset,
        "transcriptTotalBytes": transcript.len(),
        "nextTranscriptOffset": next_transcript_offset,
    }))
}

async fn get_clovy_dictation(context: &ToolContext, arguments: &Value) -> Result<Value, AppError> {
    let dictation_id = required_string(arguments, "id")?;
    let profile = crate::commands::active_profile(&context.app);
    get_clovy_dictation_for_profile(
        &context.repository.pool,
        &profile,
        dictation_id,
        byte_offset(arguments, "textOffset")?,
    )
    .await
}

/// One dictation of the active profile, its text paged like `get_note`.
async fn get_clovy_dictation_for_profile(
    pool: &sqlx_sqlite::SqlitePool,
    profile: &str,
    dictation_id: &str,
    text_offset: usize,
) -> Result<Value, AppError> {
    let row =
        crate::mcp_server::tools::notes::read_dictation(pool, profile, dictation_id, None).await?;
    let text: String = row.get("text");
    let (page, text_offset, next_text_offset) = text_page(&text, text_offset);
    Ok(json!({
        "id": row.get::<String, _>("id"),
        "language": row.get::<Option<String>, _>("language"),
        "createdAt": row.get::<String, _>("created_at"),
        "text": page,
        "textOffset": text_offset,
        "textTotalBytes": text.len(),
        "nextTextOffset": next_text_offset,
    }))
}

fn byte_offset(arguments: &Value, key: &str) -> Result<usize, AppError> {
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

/// The page of `text` starting at `offset` (moved back to a character
/// boundary, clamped to the end) and at most `NOTE_PAGE_BYTES` long, ending
/// on a character boundary. Returns the page, its actual start, and the next
/// page's start when text remains.
fn text_page(text: &str, offset: usize) -> (&str, usize, Option<usize>) {
    crate::mcp_server::tools::notes::byte_page(text, offset, NOTE_PAGE_BYTES)
}

async fn list_memories(context: &ToolContext, arguments: &Value) -> Result<Value, AppError> {
    let settings_path = crate::commands::memory_settings_path(&context.app)?;
    if !crate::commands::load_memory_settings(&settings_path).enabled {
        return Err(AppError::new(
            "memory_disabled",
            "Memory is disabled for this scope.",
        ));
    }
    let project_id = optional_nonempty_string(arguments, "projectId")?;
    ensure_memory_scope_enabled(context, project_id.as_deref()).await?;
    let include_global = arguments
        .get("includeGlobal")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let limit = arguments
        .get("limit")
        .and_then(Value::as_u64)
        .unwrap_or(8)
        .clamp(1, 20) as usize;
    let offset = arguments.get("offset").and_then(Value::as_u64).unwrap_or(0) as usize;
    let profile = crate::commands::active_profile(&context.app);
    let repositories = crate::commands::repositories(&context.app).await?;
    let memories = repositories
        .list_memories(&profile, project_id.as_deref(), include_global)
        .await?;
    let has_more = memories.len() > offset.saturating_add(limit);
    let items = memories
        .into_iter()
        .skip(offset)
        .take(limit)
        .map(|memory| {
            json!({
                "id": memory.id,
                "content": memory.content,
                "createdAt": memory.created_at,
                "scope": if memory.folder_id.is_some() { "project" } else { "global" }
            })
        })
        .collect::<Vec<_>>();
    Ok(json!({
        "count": items.len(),
        "items": items,
        "offset": offset,
        "hasMore": has_more,
        "nextOffset": has_more.then_some(offset + items.len())
    }))
}

async fn save_memory(context: &ToolContext, arguments: &Value) -> Result<Value, AppError> {
    let content = required_string(arguments, "content")?;
    let project_id = optional_nonempty_string(arguments, "projectId")?;
    let profile = crate::commands::active_profile(&context.app);
    let repositories = crate::commands::repositories(&context.app).await?;
    let settings_path = crate::commands::memory_settings_path(&context.app)?;
    let memory = crate::commands::create_memory_with_settings(
        &repositories,
        &settings_path,
        &profile,
        project_id.as_deref(),
        content,
        "agent",
    )
    .await?;
    serde_json::to_value(memory)
        .map_err(|error| AppError::new("agent_tool_response_invalid", error.to_string()))
}

async fn forget_memory(context: &ToolContext, arguments: &Value) -> Result<Value, AppError> {
    let id = required_string(arguments, "id")?;
    let profile = crate::commands::active_profile(&context.app);
    crate::commands::repositories(&context.app)
        .await?
        .delete_memory(&profile, id)
        .await?;
    Ok(json!({ "forgotten": true, "id": id }))
}

async fn generate_image(context: &ToolContext, arguments: &Value) -> Result<Value, AppError> {
    let prompt = required_string(arguments, "prompt")?.trim();
    let generated = crate::providers::generate_image(crate::providers::GenerateImageRequest {
        prompt: prompt.to_string(),
        model: None,
        request_id: context.call_id.clone(),
        safe_mode: None,
    })
    .await?;
    media::persist_generated_image(&artifact_target(context), prompt, generated).await
}

async fn edit_image(context: &ToolContext, arguments: &Value) -> Result<Value, AppError> {
    let source = resolve_path(context, required_string(arguments, "sourcePath")?, true)?;
    let instruction = required_string(arguments, "instruction")?.trim();
    let (image, mime_type) = media::read_edit_source(&source).await?;
    let generated = crate::providers::edit_image(crate::providers::EditImageRequest {
        image,
        prompt: instruction.to_string(),
        request_id: context.call_id.clone(),
        mime_type: Some(mime_type.to_string()),
        model: None,
    })
    .await?;
    media::persist_generated_image(&artifact_target(context), instruction, generated).await
}

async fn generate_video(context: &ToolContext, arguments: &Value) -> Result<Value, AppError> {
    let call = parse_video_call(arguments, context.call_id.clone())?;
    let prompt = call.prompt();
    let mut cancelled = context.cancellations.register(&context.run_id).await;
    let (job_id, outcome) = run_video_call(
        call,
        crate::providers::video_generate,
        |job_id| {
            crate::providers::video_status(
                context.app.clone(),
                crate::providers::VideoStatusRequest { job_id },
            )
        },
        &mut cancelled,
    )
    .await?;
    media::finish_video_call(
        &artifact_target(context),
        job_id,
        outcome,
        prompt,
        context.call_id.clone(),
    )
    .await
}

/// The session workspace and rows a native tool's artifact goes to.
fn artifact_target(context: &ToolContext) -> media::ArtifactTarget<'_> {
    media::ArtifactTarget {
        pool: &context.repository.pool,
        workspace: &context.workspace,
        session_id: &context.session_id,
        run_id: &context.run_id,
    }
}

/// Runs one `generate_video` call until its job ends or the run is stopped
/// (the shared loop: `media::follow_video_call`).
async fn run_video_call<S, SFut, P, PFut>(
    call: VideoCall,
    submit: S,
    poll: P,
    cancelled: &mut ToolCancellationRegistration,
) -> Result<(String, VideoJobOutcome), AppError>
where
    S: FnOnce(crate::providers::GenerateVideoRequest) -> SFut,
    SFut: Future<Output = Result<crate::clovy_api::VideoJobDto, AppError>>,
    P: FnMut(String) -> PFut,
    PFut: Future<Output = Result<crate::clovy_api::VideoStatusDto, AppError>>,
{
    media::follow_video_call(
        call,
        submit,
        poll,
        cancelled.cancelled(),
        media::VideoPolling::UNTIL_DONE,
    )
    .await
}

async fn ensure_memory_scope_enabled(
    context: &ToolContext,
    project_id: Option<&str>,
) -> Result<(), AppError> {
    let Some(project_id) = project_id else {
        return Ok(());
    };
    let profile = crate::commands::active_profile(&context.app);
    let row = sqlx::query::query(
        "SELECT memory_disabled FROM folders
         WHERE id = ? AND profile = ? AND deleted_at IS NULL",
    )
    .bind(project_id)
    .bind(profile)
    .fetch_optional(&context.repository.pool)
    .await?
    .ok_or_else(|| {
        AppError::new(
            "folder_not_found",
            "Project was not found or has already been deleted.",
        )
    })?;
    if row.get::<i64, _>("memory_disabled") != 0 {
        return Err(AppError::new(
            "memory_disabled",
            "Memory is disabled for this scope.",
        ));
    }
    Ok(())
}

fn optional_nonempty_string(arguments: &Value, key: &str) -> Result<Option<String>, AppError> {
    let Some(value) = arguments.get(key) else {
        return Ok(None);
    };
    let value = value.as_str().ok_or_else(|| {
        AppError::new(
            "invalid_arguments",
            format!("{key} must be a non-empty string."),
        )
    })?;
    let value = value.trim();
    if value.is_empty() {
        return Err(AppError::new(
            "invalid_arguments",
            format!("{key} must be a non-empty string."),
        ));
    }
    Ok(Some(value.to_string()))
}

async fn web(context: &ToolContext, path: &str, arguments: &Value) -> Result<Value, AppError> {
    let request = web_request(arguments, context.call_id.as_deref());
    let response = crate::clovy_api::forward_web_request(path, &request).await?;
    if response.status >= 400 {
        return Err(AppError::new(
            "agent_web_failed",
            String::from_utf8_lossy(&response.body).into_owned(),
        ));
    }
    serde_json::from_slice(&response.body)
        .map_err(|error| AppError::new("agent_web_invalid_response", error.to_string()))
}

fn web_request(arguments: &Value, call_id: Option<&str>) -> Value {
    let mut request = arguments.clone();
    request["requestId"] = Value::String(
        call_id
            .map(str::to_string)
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
    );
    request
}

async fn list_files(context: &ToolContext, arguments: &Value) -> Result<Value, AppError> {
    let path = resolve_path_with_scope(
        context,
        arguments.get("path").and_then(Value::as_str).unwrap_or("."),
        true,
        PathScope::WorkspaceOrCurrentObsidianVaultRead,
    )?;
    let mut entries = tokio::fs::read_dir(&path).await.map_err(io_error)?;
    let mut result = Vec::new();
    while let Some(entry) = entries.next_entry().await.map_err(io_error)? {
        let metadata = entry.metadata().await.map_err(io_error)?;
        let entry_path = entry.path();
        result.push(json!({ "name": entry.file_name().to_string_lossy(), "path": crate::filesystem::normalize_path_for_external_use(&entry_path), "directory": metadata.is_dir(), "sizeBytes": metadata.len() }));
        if result.len() >= 500 {
            break;
        }
    }
    Ok(
        json!({ "path": crate::filesystem::normalize_path_for_external_use(&path), "entries": result }),
    )
}

async fn read_file(context: &ToolContext, arguments: &Value) -> Result<Value, AppError> {
    let path = resolve_path_with_scope(
        context,
        required_string(arguments, "path")?,
        true,
        PathScope::WorkspaceOrCurrentObsidianVaultRead,
    )?;
    let bytes = tokio::fs::read(&path).await.map_err(io_error)?;
    if bytes.len() > MAX_TOOL_OUTPUT_BYTES {
        return Err(AppError::new(
            "agent_tool_output_too_large",
            "File exceeds the 1 MB read limit.",
        ));
    }
    let content = String::from_utf8(bytes)
        .map_err(|_| AppError::new("agent_file_not_text", "File is not UTF-8 text."))?;
    Ok(
        json!({ "path": crate::filesystem::normalize_path_for_external_use(&path), "content": content }),
    )
}

async fn write_file(context: &ToolContext, arguments: &Value) -> Result<Value, AppError> {
    let path = resolve_path(context, required_string(arguments, "path")?, false)?;
    let content = required_string(arguments, "content")?;
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(io_error)?;
    }
    tokio::fs::write(&path, content).await.map_err(io_error)?;
    record_artifact(context, &path, "created", None).await?;
    Ok(
        json!({ "path": crate::filesystem::normalize_path_for_external_use(&path), "sizeBytes": content.len() }),
    )
}

async fn patch_file(context: &ToolContext, arguments: &Value) -> Result<Value, AppError> {
    let path = resolve_path(context, required_string(arguments, "path")?, true)?;
    let before = required_string(arguments, "before")?;
    let after = required_string(arguments, "after")?;
    let content = tokio::fs::read_to_string(&path).await.map_err(io_error)?;
    let occurrences = content.matches(before).count();
    if occurrences != 1 {
        return Err(AppError::new(
            "agent_patch_ambiguous",
            format!("Patch target must occur exactly once, but occurred {occurrences} times."),
        ));
    }
    tokio::fs::write(&path, content.replacen(before, after, 1))
        .await
        .map_err(io_error)?;
    record_artifact(context, &path, "updated", None).await?;
    Ok(
        json!({ "path": crate::filesystem::normalize_path_for_external_use(&path), "updated": true }),
    )
}

async fn import_file(context: &ToolContext, arguments: &Value) -> Result<Value, AppError> {
    let source = PathBuf::from(required_string(arguments, "sourcePath")?)
        .canonicalize()
        .map_err(io_error)?;
    if !source.is_file() {
        return Err(AppError::new(
            "agent_import_invalid",
            "Import source is not a file.",
        ));
    }
    let name = source
        .file_name()
        .ok_or_else(|| AppError::new("agent_import_invalid", "Import source has no file name."))?;
    let destination = resolve_path(
        context,
        &format!("imports/{}", name.to_string_lossy()),
        false,
    )?;
    if let Some(parent) = destination.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(io_error)?;
    }
    tokio::fs::copy(&source, &destination)
        .await
        .map_err(io_error)?;
    record_artifact(context, &destination, "imported", Some(&source)).await?;
    Ok(
        json!({ "path": crate::filesystem::normalize_path_for_external_use(&destination), "sourcePath": crate::filesystem::normalize_path_for_external_use(&source) }),
    )
}

async fn preview_file(context: &ToolContext, arguments: &Value) -> Result<Value, AppError> {
    let path = resolve_path(context, required_string(arguments, "path")?, true)?;
    let metadata = tokio::fs::metadata(&path).await.map_err(io_error)?;
    let preview = if metadata.len() <= 64 * 1024 {
        tokio::fs::read_to_string(&path).await.ok()
    } else {
        None
    };
    Ok(
        json!({ "path": crate::filesystem::normalize_path_for_external_use(&path), "sizeBytes": metadata.len(), "text": preview }),
    )
}

async fn record_artifact(
    context: &ToolContext,
    path: &Path,
    action: &str,
    original: Option<&Path>,
) -> Result<(), AppError> {
    let metadata = tokio::fs::metadata(path).await.map_err(io_error)?;
    sqlx::query::query("INSERT INTO agent_artifacts(id, session_id, run_id, provenance, action, path, original_path, size_bytes, available, created_at) VALUES (?, ?, ?, 'tool', ?, ?, ?, ?, 1, ?)")
        .bind(uuid::Uuid::new_v4().to_string()).bind(&context.session_id).bind(&context.run_id)
        .bind(action).bind(path.to_string_lossy().as_ref()).bind(original.map(|value| value.to_string_lossy().into_owned()))
        .bind(metadata.len() as i64).bind(crate::db::repositories::timestamp()).execute(&context.repository.pool).await?;
    Ok(())
}

async fn search_files(context: &ToolContext, arguments: &Value) -> Result<Value, AppError> {
    let needle = required_string(arguments, "query")?.to_string();
    let root = resolve_path_with_scope(
        context,
        arguments.get("path").and_then(Value::as_str).unwrap_or("."),
        true,
        PathScope::WorkspaceOrCurrentObsidianVaultRead,
    )?;
    let result = tokio::task::spawn_blocking(move || search_text_files(&root, &needle))
        .await
        .map_err(|error| AppError::new("agent_file_search_failed", error.to_string()))?;
    Ok(json!({ "matches": result.matches, "truncated": result.truncated }))
}

struct FileSearchResult {
    matches: String,
    truncated: bool,
}

fn search_text_files(root: &Path, needle: &str) -> FileSearchResult {
    let mut output = String::new();
    let mut stack = vec![root.to_path_buf()];
    let mut scanned_bytes = 0_u64;
    let mut scanned_files = 0_usize;
    let mut match_count = 0_usize;
    let mut truncated = false;

    while let Some(path) = stack.pop() {
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            continue;
        };
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_dir() {
            let Ok(entries) = fs::read_dir(&path) else {
                continue;
            };
            let mut children = entries
                .flatten()
                .filter_map(|entry| {
                    let name = entry.file_name();
                    let name = name.to_string_lossy();
                    (!name.starts_with('.')).then(|| entry.path())
                })
                .collect::<Vec<_>>();
            children.sort_by(|left, right| right.cmp(left));
            stack.extend(children);
            continue;
        }
        if !metadata.is_file() {
            continue;
        }
        if scanned_files >= MAX_SEARCH_FILES
            || scanned_bytes.saturating_add(metadata.len()) > MAX_SEARCH_SCANNED_BYTES
        {
            truncated = true;
            break;
        }
        scanned_files += 1;
        if metadata.len() > MAX_SEARCH_FILE_BYTES {
            truncated = true;
            continue;
        }
        scanned_bytes = scanned_bytes.saturating_add(metadata.len());
        let Ok(bytes) = fs::read(&path) else {
            continue;
        };
        if bytes.contains(&0) {
            continue;
        }
        let Ok(text) = std::str::from_utf8(&bytes) else {
            continue;
        };
        for (line_index, line) in text.lines().enumerate() {
            if !line.contains(needle) {
                continue;
            }
            let entry = format!(
                "{}:{}:{}\n",
                crate::filesystem::normalize_path_for_external_use(&path).display(),
                line_index + 1,
                line
            );
            if match_count >= MAX_SEARCH_MATCHES
                || output.len().saturating_add(entry.len()) > MAX_TOOL_OUTPUT_BYTES
            {
                truncated = true;
                break;
            }
            output.push_str(&entry);
            match_count += 1;
        }
        if truncated && (match_count >= MAX_SEARCH_MATCHES || output.len() >= MAX_TOOL_OUTPUT_BYTES)
        {
            break;
        }
    }

    FileSearchResult {
        matches: output,
        truncated,
    }
}

async fn run_shell(context: &ToolContext, arguments: &Value) -> Result<Value, AppError> {
    #[cfg(target_os = "windows")]
    if context.safety_mode == AgentSafetyMode::Sandboxed {
        return Err(AppError::new(
            "agent_shell_sandbox_unavailable",
            "Shell execution is unavailable in Sandboxed mode on Windows.",
        ));
    }
    let script = required_string(arguments, "command")?;
    let mut command = if cfg!(target_os = "windows") {
        let mut command = Command::new("cmd");
        command.arg("/C").arg(script);
        command
    } else if cfg!(target_os = "macos") && context.safety_mode == AgentSafetyMode::Sandboxed {
        let profile = sandbox_profile(&context.workspace);
        let mut command = Command::new("/usr/bin/sandbox-exec");
        command
            .arg("-p")
            .arg(profile)
            .arg("/bin/zsh")
            .arg("-lc")
            .arg(script);
        command
    } else {
        let mut command = Command::new("/bin/sh");
        command.arg("-lc").arg(script);
        command
    };
    command.current_dir(&context.workspace);
    if let Some(secret_env) = arguments.get("secretEnv").and_then(Value::as_object) {
        for (name, secret_ref) in secret_env {
            if name.is_empty()
                || !name
                    .chars()
                    .all(|character| character == '_' || character.is_ascii_alphanumeric())
                || !name
                    .chars()
                    .next()
                    .is_some_and(|character| character == '_' || character.is_ascii_alphabetic())
            {
                return Err(AppError::new(
                    "agent_secret_environment_invalid",
                    "A secret environment variable name is invalid.",
                ));
            }
            let secret_ref = secret_ref.as_str().ok_or_else(|| {
                AppError::new(
                    "agent_secret_reference_invalid",
                    "A secret reference is invalid.",
                )
            })?;
            let value = super::secrets::take(secret_ref).await?.ok_or_else(|| {
                AppError::new(
                    "agent_secret_reference_expired",
                    "The requested secret is no longer available.",
                )
            })?;
            command.env(name, value);
        }
    }
    let mut cancelled = context.cancellations.register(&context.run_id).await;
    run_shell_command(command, cancelled.cancelled()).await
}

/// Runs a prepared shell command in its own process group. Whatever the
/// script left in the background is swept with the group once the shell
/// exits (or on cancellation), and the pipes are drained for a bounded grace
/// period, so a backgrounded helper that inherited stdout can neither hang
/// the tool nor outlive it.
async fn run_shell_command(
    mut command: Command,
    cancelled: impl Future<Output = ()>,
) -> Result<Value, AppError> {
    command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    command.process_group(0);
    let mut child = command.spawn().map_err(io_error)?;
    let group = child.id();
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| AppError::new("agent_shell_failed", "Shell stdout was unavailable."));
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| AppError::new("agent_shell_failed", "Shell stderr was unavailable."));
    let (stdout, stderr) = match (stdout, stderr) {
        (Ok(stdout), Ok(stderr)) => (stdout, stderr),
        (Err(error), _) | (_, Err(error)) => {
            crate::process_group::kill_group(group);
            return Err(error);
        }
    };
    let stdout = spawn_bounded_reader(stdout);
    let stderr = spawn_bounded_reader(stderr);
    let status = tokio::select! {
        status = child.wait() => status.map_err(io_error),
        () = cancelled => {
            crate::process_group::kill_group(group);
            let _ = child.kill().await;
            return Err(AppError::new("agent_tool_cancelled", "Shell command was cancelled."));
        }
    };
    // Sweep helpers the script left behind first: one that inherited stdout
    // would otherwise keep the pipe open and the output would never reach EOF.
    crate::process_group::kill_group(group);
    let status = status?;
    let stdout_text = collect_bounded(stdout).await;
    let stderr_text = collect_bounded(stderr).await;
    Ok(json!({ "exitCode": status.code(), "stdout": stdout_text, "stderr": stderr_text }))
}

async fn notion_tool(
    context: &ToolContext,
    runtime_name: &str,
    arguments: &Value,
) -> Result<Value, AppError> {
    let Some((tool_name, action)) =
        crate::connectors::notion::runtime_name_to_provider(runtime_name)
    else {
        return Err(AppError::new(
            "notion_tool_not_allowed",
            "That Notion tool is not enabled.",
        ));
    };
    let tool_arguments = arguments.clone();
    let request = crate::connectors::notion::NotionHostedToolCallRequest {
        tool_name: tool_name.to_string(),
        arguments: tool_arguments.clone(),
        deadline_unix_ms: None,
    };
    let result = if action {
        let call_id = context.call_id.as_deref().ok_or_else(|| {
            AppError::new(
                "notion_action_binding_missing",
                "The approved Notion action has no call identity.",
            )
        })?;
        let row = query("SELECT payload_json FROM agent_items WHERE run_id = ? AND kind = 'interruption' AND json_extract(payload_json, '$.id') = ? AND json_extract(payload_json, '$.status') = 'resolved' AND json_extract(payload_json, '$.approved') = 1 ORDER BY created_at DESC LIMIT 1")
            .bind(&context.run_id).bind(call_id).fetch_optional(&context.repository.pool).await?
            .ok_or_else(|| AppError::new("notion_action_binding_missing", "The approved Notion action binding is unavailable."))?;
        let payload: Value = serde_json::from_str(&row.get::<String, _>("payload_json"))
            .map_err(|error| AppError::new("notion_action_binding_invalid", error.to_string()))?;
        let digest = payload
            .get("approvalBinding")
            .and_then(|binding| binding.get("digest"))
            .and_then(Value::as_str)
            .ok_or_else(|| {
                AppError::new(
                    "notion_action_binding_missing",
                    "The approved Notion action binding is unavailable.",
                )
            })?;
        let revision = crate::connectors::notion::capture_execution_revision(
            &context.app,
            runtime_name,
            &tool_arguments,
            &context.run_id,
            call_id,
            digest,
        )
        .await?;
        let consumed = query("UPDATE agent_items SET payload_json = json_set(payload_json, '$.executionState', 'consumed') WHERE run_id = ? AND kind = 'interruption' AND json_extract(payload_json, '$.id') = ? AND json_extract(payload_json, '$.status') = 'resolved' AND json_extract(payload_json, '$.approved') = 1 AND COALESCE(json_extract(payload_json, '$.executionState'), 'unconsumed') = 'unconsumed'")
            .bind(&context.run_id).bind(call_id).execute(&context.repository.pool).await?.rows_affected();
        if consumed != 1 {
            return Err(AppError::new(
                "notion_action_binding_consumed",
                "This approved Notion action was already invoked.",
            ));
        }
        crate::connectors::notion::call_hosted_action_tool_approved(&context.app, request, revision)
            .await?
    } else {
        crate::connectors::notion::call_hosted_tool(&context.app, request).await?
    };
    serde_json::to_value(result)
        .map_err(|error| AppError::new("agent_connector_response_invalid", error.to_string()))
}

async fn list_skills(context: &ToolContext) -> Result<Value, AppError> {
    let enabled = context
        .repository
        .run_enabled_skills(&context.run_id)
        .await?
        .into_iter()
        .map(|name| (canonical_skill_id(&name).to_string(), true))
        .collect();
    crate::mcp_server::tools::vault_skills::list_skills_from_roots(
        &crate::mcp_server::tools::vault_skills::skill_roots(&context.app),
        &enabled,
        false,
    )
    .await
}

async fn load_skill(context: &ToolContext, arguments: &Value) -> Result<Value, AppError> {
    let requested = required_string(arguments, "name")?;
    // Preserve native validation before reading the run's enabled snapshot.
    let name = crate::mcp_server::tools::vault_skills::validate_skill_name(requested)?;
    let enabled = context
        .repository
        .run_enabled_skills(&context.run_id)
        .await?
        .into_iter()
        .map(|name| (canonical_skill_id(&name).to_string(), true))
        .collect();
    crate::mcp_server::tools::vault_skills::load_skill_from_roots(
        &crate::mcp_server::tools::vault_skills::skill_roots(&context.app),
        &enabled,
        name,
    )
    .await
}

async fn consume_clarification_answer(context: &ToolContext) -> Result<Value, AppError> {
    consume_clarification_answer_from_pool(&context.repository.pool, &context.run_id).await
}

async fn consume_secret_reference(context: &ToolContext) -> Result<Value, AppError> {
    let row = query("SELECT id, payload_json FROM agent_items WHERE run_id = ? AND kind = 'interruption' AND json_extract(payload_json, '$.kind') = 'secret' AND json_extract(payload_json, '$.secretRef') IS NOT NULL AND COALESCE(json_extract(payload_json, '$.secretReferenceConsumed'), 0) = 0 ORDER BY created_at DESC LIMIT 1")
        .bind(&context.run_id)
        .fetch_one(&context.repository.pool)
        .await?;
    let id: String = row.get("id");
    let mut payload: Value = serde_json::from_str(&row.get::<String, _>("payload_json"))
        .map_err(|error| AppError::new("agent_interruption_invalid", error.to_string()))?;
    let secret_ref = payload
        .get("secretRef")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            AppError::new(
                "agent_secret_unanswered",
                "The secret request has not been answered.",
            )
        })?
        .to_string();
    payload["secretReferenceConsumed"] = Value::Bool(true);
    query("UPDATE agent_items SET payload_json = ? WHERE id = ?")
        .bind(payload.to_string())
        .bind(id)
        .execute(&context.repository.pool)
        .await?;
    Ok(json!({ "secretRef": secret_ref, "available": true }))
}

async fn consume_clarification_answer_from_pool(
    pool: &sqlx_sqlite::SqlitePool,
    run_id: &str,
) -> Result<Value, AppError> {
    // The Agents SDK assigns a fresh execution call id after an approval resumes.
    // That id is not guaranteed to match the provider tool-call id persisted on
    // the interruption. The run can only resume one answered clarification at a
    // time, so select the newest answered, unconsumed clarification for the run.
    let row = query("SELECT id, payload_json FROM agent_items WHERE run_id = ? AND kind = 'interruption' AND json_extract(payload_json, '$.kind') = 'clarification' AND json_extract(payload_json, '$.answer') IS NOT NULL AND COALESCE(json_extract(payload_json, '$.answerConsumed'), 0) = 0 ORDER BY created_at DESC LIMIT 1")
        .bind(run_id).fetch_one(pool).await?;
    let id: String = row.get("id");
    let mut payload: Value = serde_json::from_str(&row.get::<String, _>("payload_json"))
        .map_err(|error| AppError::new("agent_interruption_invalid", error.to_string()))?;
    let answer = payload
        .get("answer")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            AppError::new(
                "agent_clarification_unanswered",
                "The clarification has not been answered.",
            )
        })?
        .to_string();
    payload["answerConsumed"] = Value::Bool(true);
    query("UPDATE agent_items SET payload_json = ? WHERE id = ?")
        .bind(payload.to_string())
        .bind(id)
        .execute(pool)
        .await?;
    Ok(json!({ "answer": answer }))
}

fn resolve_path(
    context: &ToolContext,
    requested: &str,
    must_exist: bool,
) -> Result<PathBuf, AppError> {
    resolve_path_with_scope(context, requested, must_exist, PathScope::WorkspaceOnly)
}

#[derive(Clone, Copy)]
enum PathScope {
    WorkspaceOnly,
    WorkspaceOrCurrentObsidianVaultRead,
}

/// Component-wise containment check: returns true if `path` is `root` or a
/// descendant of `root`. Uses `Path::components()` so textual-prefix collisions
/// (e.g. `/Vault` vs `/Vault-backup`) are rejected.
fn is_within(path: &Path, root: &Path) -> bool {
    path.starts_with(root)
}

fn resolve_path_with_scope(
    context: &ToolContext,
    requested: &str,
    must_exist: bool,
    scope: PathScope,
) -> Result<PathBuf, AppError> {
    let resolved = media::resolve_requested_path(&context.workspace, requested, must_exist)?;
    if context.safety_mode == AgentSafetyMode::Sandboxed {
        let workspace = context.workspace.canonicalize().map_err(io_error)?;
        if !is_within(&resolved, &workspace) {
            if let PathScope::WorkspaceOrCurrentObsidianVaultRead = scope {
                if let Some(vault_root) = crate::obsidian::current_vault_root(&context.app) {
                    if is_within(&resolved, &vault_root) {
                        return Ok(resolved);
                    }
                }
            }
            return Err(AppError::new(
                "agent_path_denied",
                "Path is outside this session's workspace.",
            ));
        }
    }
    Ok(resolved)
}

/// Seeds bundled agent skills (packaged as Tauri resources) into the managed
/// skill directory. Copies only when the managed destination is absent so user
/// edits and enable/disable settings are preserved across app launches.
pub fn seed_bundled_skills(app: &AppHandle) {
    let Some(managed_root) = crate::app_paths::app_data_dir(app)
        .ok()
        .map(|path| path.join("agents").join("skills"))
    else {
        return;
    };
    let resource_root = app
        .path()
        .resource_dir()
        .ok()
        .map(|dir| dir.join("native").join("agent-skills"));
    let debug_root = std::env::var("CARGO_MANIFEST_DIR").ok().map(|manifest| {
        PathBuf::from(manifest)
            .join("resources")
            .join("agent-skills")
    });
    let source_root = resource_root.filter(|path| path.is_dir()).or(debug_root);
    let Some(source_root) = source_root else {
        tracing::warn!("could not resolve bundled agent-skills resource directory");
        return;
    };
    if let Err(error) = reconcile_managed_obsidian_skill(&managed_root) {
        tracing::warn!(?error, "could not promote the legacy Obsidian skill");
    }
    let Ok(entries) = std::fs::read_dir(&source_root) else {
        return;
    };
    for entry in entries.flatten() {
        let skill_file = entry.path().join("SKILL.md");
        if !skill_file.is_file() {
            continue;
        }
        let skill_id = entry.file_name().to_string_lossy().into_owned();
        let dest_dir = managed_root.join(&skill_id);
        let dest_file = dest_dir.join("SKILL.md");
        if dest_file.is_file() {
            continue;
        }
        if let Err(error) = std::fs::create_dir_all(&dest_dir) {
            tracing::warn!(?error, skill = %skill_id, "could not create managed skill directory");
            continue;
        }
        let temp = dest_dir.join(".SKILL.md.tmp");
        if let Err(error) = std::fs::copy(&skill_file, &temp) {
            tracing::warn!(?error, skill = %skill_id, "could not copy bundled skill");
            continue;
        }
        if let Err(error) = std::fs::rename(&temp, &dest_file) {
            tracing::warn!(?error, skill = %skill_id, "could not finalize bundled skill");
            let _ = std::fs::remove_file(&temp);
        }
    }
    if let Err(error) = reconcile_managed_obsidian_skill(&managed_root) {
        tracing::warn!(?error, "could not publish the legacy Obsidian skill alias");
    }
}

pub(crate) fn sandbox_profile(workspace: &Path) -> String {
    let escaped = workspace.to_string_lossy().replace('"', "\\\"");
    format!("(version 1) (allow default) (deny file-write*) (allow file-write* (subpath \"{escaped}\")) (allow file-write* (subpath \"/private/tmp\"))")
}

#[derive(Default)]
struct BoundedOutput {
    bytes: Vec<u8>,
    truncated: bool,
}

type SharedOutput = std::sync::Arc<std::sync::Mutex<BoundedOutput>>;

fn spawn_bounded_reader(
    mut reader: impl tokio::io::AsyncRead + Unpin + Send + 'static,
) -> (tokio::task::JoinHandle<()>, SharedOutput) {
    let output = SharedOutput::default();
    let task = tokio::spawn({
        let output = output.clone();
        async move { read_bounded(&mut reader, &output).await }
    });
    (task, output)
}

/// Waits briefly for EOF, then returns whatever was read; bytes already
/// captured are never discarded.
async fn collect_bounded(
    (mut task, output): (tokio::task::JoinHandle<()>, SharedOutput),
) -> String {
    if tokio::time::timeout(SHELL_PIPE_DRAIN_GRACE, &mut task)
        .await
        .is_err()
    {
        task.abort();
    }
    let output = output
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    bounded_text(&output)
}

/// Reads to EOF (or a read error), keeping at most `MAX_TOOL_OUTPUT_BYTES`.
async fn read_bounded(reader: &mut (impl tokio::io::AsyncRead + Unpin), output: &SharedOutput) {
    let mut buffer = [0_u8; 8 * 1_024];
    loop {
        let read = match reader.read(&mut buffer).await {
            Ok(0) | Err(_) => break,
            Ok(read) => read,
        };
        let mut output = output
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let remaining = MAX_TOOL_OUTPUT_BYTES.saturating_sub(output.bytes.len());
        let retained = remaining.min(read);
        output.bytes.extend_from_slice(&buffer[..retained]);
        output.truncated |= retained < read;
    }
}

fn bounded_text(output: &BoundedOutput) -> String {
    let mut text = String::from_utf8_lossy(&output.bytes).into_owned();
    if output.truncated {
        text.push_str("\n[output truncated]");
    }
    text
}

fn required_string<'a>(value: &'a Value, key: &str) -> Result<&'a str, AppError> {
    value
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
/// A window of at most `SEARCH_EXCERPT_CHARS` characters around the first
/// case-insensitive match of `query` (the start of the text when it does not
/// match), marked with `...` where text was clipped. A search returns up to
/// 40 rows, so whole notes and transcripts would flood the model's context;
/// `get_note` and `get_dictation` read a whole note or dictation.
fn search_excerpt(text: &str, query: &str) -> String {
    if text.chars().count() <= SEARCH_EXCERPT_CHARS {
        return text.to_string();
    }
    // ASCII lowercasing keeps byte offsets, and matches LIKE's case folding.
    let match_start = text
        .to_ascii_lowercase()
        .find(&query.to_ascii_lowercase())
        .unwrap_or(0);
    let chars_before_match = text[..match_start].chars().count();
    let first_char = chars_before_match.saturating_sub(SEARCH_EXCERPT_CHARS / 4);
    let mut excerpt: String = text
        .chars()
        .skip(first_char)
        .take(SEARCH_EXCERPT_CHARS)
        .collect();
    if first_char + SEARCH_EXCERPT_CHARS < text.chars().count() {
        excerpt.push_str("...");
    }
    if first_char > 0 {
        excerpt.insert_str(0, "...");
    }
    excerpt
}
fn io_error(error: std::io::Error) -> AppError {
    AppError::new("agent_tool_io_failed", error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx_sqlite::SqlitePoolOptions;

    #[tokio::test]
    async fn settled_cancellation_registration_is_removed() {
        let registry = ToolCancellationRegistry::default();
        let registration = registry.register("run-1").await;
        assert_eq!(registry.registration_count("run-1"), 1);

        drop(registration);

        assert_eq!(registry.registration_count("run-1"), 0);
    }

    #[tokio::test]
    async fn cancelling_a_run_notifies_and_clears_every_registration() {
        let registry = ToolCancellationRegistry::default();
        let mut first = registry.register("run-1").await;
        let mut second = registry.register("run-1").await;

        registry.cancel("run-1").await;

        assert_eq!(registry.registration_count("run-1"), 0);
        assert!((&mut first.receiver).await.is_ok());
        assert!((&mut second.receiver).await.is_ok());
    }

    #[tokio::test]
    async fn bounded_reader_drains_after_retaining_its_prefix() {
        let bytes = vec![b'x'; MAX_TOOL_OUTPUT_BYTES + 4_096];
        let mut input = bytes.as_slice();

        let captured = SharedOutput::default();
        read_bounded(&mut input, &captured).await;
        let output = bounded_text(&captured.lock().unwrap());

        assert!(input.is_empty());
        assert_eq!(output.matches('x').count(), MAX_TOOL_OUTPUT_BYTES);
        assert!(output.ends_with("\n[output truncated]"));
    }

    #[cfg(unix)]
    fn process_is_gone(pid: i32) -> bool {
        for _ in 0..50 {
            // SAFETY: signal 0 only checks for existence.
            if unsafe { libc::kill(pid, 0) } != 0 {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        false
    }

    #[cfg(unix)]
    fn shell(directory: &Path, script: &str) -> Command {
        let mut command = Command::new("/bin/sh");
        command.arg("-c").arg(script).current_dir(directory);
        command
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn shell_returns_when_a_background_helper_keeps_stdout_open() {
        let directory = tempfile::tempdir().unwrap();
        let started = std::time::Instant::now();

        let result = run_shell_command(
            shell(
                directory.path(),
                "sleep 60 & echo $! > helper.pid; echo done",
            ),
            std::future::pending(),
        )
        .await
        .unwrap();

        assert!(started.elapsed() < std::time::Duration::from_secs(5));
        assert_eq!(result["exitCode"], 0);
        assert_eq!(result["stdout"].as_str().unwrap().trim(), "done");
        let helper: i32 = fs::read_to_string(directory.path().join("helper.pid"))
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        assert!(process_is_gone(helper), "helper {helper} survived the tool");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn cancelling_a_shell_kills_its_background_helpers() {
        let directory = tempfile::tempdir().unwrap();
        let pid_file = directory.path().join("helper.pid");
        let watched = pid_file.clone();
        let cancelled = async move {
            while !fs::read_to_string(&watched).is_ok_and(|pid| pid.ends_with('\n')) {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        };

        let error = run_shell_command(
            shell(directory.path(), "sleep 60 & echo $! > helper.pid; wait"),
            cancelled,
        )
        .await
        .unwrap_err();

        assert_eq!(error.code, "agent_tool_cancelled");
        let helper: i32 = fs::read_to_string(&pid_file)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        assert!(
            process_is_gone(helper),
            "helper {helper} survived cancellation"
        );
    }

    #[test]
    fn sandbox_profile_only_grants_workspace_and_tmp_writes() {
        let profile = sandbox_profile(Path::new("/Users/example/Clovy Workspace"));
        assert!(profile.contains("(deny file-write*)"));
        assert!(profile.contains("/Users/example/Clovy Workspace"));
        assert!(!profile.contains("(allow file-write*)"));
    }

    #[test]
    fn web_requests_use_the_tool_call_id() {
        let request = web_request(&json!({ "query": "OpenAI Agents SDK" }), Some("call-42"));

        assert_eq!(request["query"], "OpenAI Agents SDK");
        assert_eq!(request["requestId"], "call-42");
    }

    #[tokio::test]
    async fn clarification_resume_consumes_answer_when_sdk_changes_call_id() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        query(
            "CREATE TABLE agent_items (
                id TEXT PRIMARY KEY,
                run_id TEXT NOT NULL,
                kind TEXT NOT NULL,
                payload_json TEXT NOT NULL,
                created_at TEXT NOT NULL
            )",
        )
        .execute(&pool)
        .await
        .unwrap();
        query("INSERT INTO agent_items (id, run_id, kind, payload_json, created_at) VALUES (?, ?, 'interruption', ?, ?)")
            .bind("item-1")
            .bind("run-1")
            .bind(json!({
                "id": "provider-call-id",
                "kind": "clarification",
                "answer": "Bullets"
            }).to_string())
            .bind("2026-07-25T00:00:00Z")
            .execute(&pool)
            .await
            .unwrap();

        let result = consume_clarification_answer_from_pool(&pool, "run-1")
            .await
            .unwrap();

        assert_eq!(result, json!({ "answer": "Bullets" }));
        let payload: String = query("SELECT payload_json FROM agent_items WHERE id = 'item-1'")
            .fetch_one(&pool)
            .await
            .unwrap()
            .get("payload_json");
        let payload: Value = serde_json::from_str(&payload).unwrap();
        assert_eq!(payload["answerConsumed"], true);
    }

    #[tokio::test]
    async fn clovy_search_is_scoped_to_the_active_profile() {
        let pool = search_pool().await;
        query("INSERT INTO notes (id, title, generated_content, profile, updated_at) VALUES ('mine', 'Shared needle', 'mine', 'profile-a', '2026-01-01'), ('theirs', 'Shared needle', 'theirs', 'profile-b', '2026-01-02')")
            .execute(&pool).await.unwrap();
        query("INSERT INTO dictation_history (id, text, profile, created_at) VALUES ('dictation-a', 'needle mine', 'profile-a', '2026-01-01'), ('dictation-b', 'needle theirs', 'profile-b', '2026-01-02')")
            .execute(&pool).await.unwrap();

        let result = search_clovy_notes_for_profile(&pool, "profile-a", "needle")
            .await
            .unwrap();

        assert_eq!(result["notes"].as_array().unwrap().len(), 1);
        assert_eq!(result["notes"][0]["id"], "mine");
        assert_eq!(result["dictations"].as_array().unwrap().len(), 1);
        assert_eq!(result["dictations"][0]["id"], "dictation-a");
    }

    async fn search_pool() -> sqlx_sqlite::SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        for statement in [
            "CREATE TABLE notes (id TEXT PRIMARY KEY, title TEXT NOT NULL, generated_content TEXT, edited_content TEXT, profile TEXT NOT NULL, created_at TEXT NOT NULL DEFAULT '', updated_at TEXT NOT NULL)",
            "CREATE TABLE transcripts (id TEXT PRIMARY KEY, note_id TEXT NOT NULL, text TEXT NOT NULL, created_at TEXT NOT NULL)",
            "CREATE TABLE dictation_history (id TEXT PRIMARY KEY, text TEXT NOT NULL, language TEXT, profile TEXT NOT NULL, created_at TEXT NOT NULL)",
        ] {
            query(statement).execute(&pool).await.unwrap();
        }
        pool
    }

    #[tokio::test]
    async fn clovy_search_matches_a_query_ending_in_a_backslash() {
        let pool = search_pool().await;
        query("INSERT INTO notes (id, title, generated_content, profile, updated_at) VALUES ('windows', 'Paths', 'Saved under C:\\Users', 'profile-a', '2026-01-01')")
            .execute(&pool).await.unwrap();

        let result = search_clovy_notes_for_profile(&pool, "profile-a", "C:\\")
            .await
            .unwrap();

        assert_eq!(result["notes"].as_array().unwrap().len(), 1);
        assert_eq!(result["notes"][0]["id"], "windows");
    }

    #[tokio::test]
    async fn clovy_search_returns_a_bounded_excerpt_around_the_match_in_multibyte_text() {
        let pool = search_pool().await;
        // One leading ASCII byte puts the old 1 MiB byte cut inside an `é`.
        let note = format!(
            "a{}needle{}",
            "é".repeat(MAX_TOOL_OUTPUT_BYTES / 2),
            "é".repeat(MAX_TOOL_OUTPUT_BYTES / 2)
        );
        query("INSERT INTO notes (id, title, generated_content, profile, updated_at) VALUES ('long', 'Long', ?, 'profile-a', '2026-01-01')")
            .bind(&note)
            .execute(&pool).await.unwrap();
        query("INSERT INTO dictation_history (id, text, profile, created_at) VALUES ('dictation', ?, 'profile-a', '2026-01-01')")
            .bind(&note)
            .execute(&pool).await.unwrap();

        let result = search_clovy_notes_for_profile(&pool, "profile-a", "NEEDLE")
            .await
            .unwrap();

        for excerpt in [
            &result["notes"][0]["note"],
            &result["dictations"][0]["text"],
        ] {
            let excerpt = excerpt.as_str().unwrap();
            assert!(excerpt.contains("needle"));
            assert!(excerpt.starts_with("...") && excerpt.ends_with("..."));
            assert!(excerpt.chars().count() <= SEARCH_EXCERPT_CHARS + 6);
        }
    }

    /// Reads every page of one text through `get_note`, following `next*Offset`.
    async fn read_all_pages(
        pool: &sqlx_sqlite::SqlitePool,
        note_id: &str,
        text: &str,
        offset: &str,
        next: &str,
    ) -> String {
        let mut read = String::new();
        let mut position = 0_usize;
        loop {
            let (content_offset, transcript_offset) = if text == "content" {
                (position, 0)
            } else {
                (0, position)
            };
            let page = get_clovy_note_for_profile(
                pool,
                "profile-a",
                note_id,
                content_offset,
                transcript_offset,
            )
            .await
            .unwrap();
            assert_eq!(page[offset], position);
            let chunk = page[text].as_str().unwrap();
            assert!(chunk.len() <= NOTE_PAGE_BYTES);
            read.push_str(chunk);
            match page[next].as_u64() {
                Some(next) => position = next as usize,
                None => return read,
            }
        }
    }

    #[tokio::test]
    async fn a_note_found_by_search_is_read_whole_through_get_note() {
        let pool = search_pool().await;
        // Multibyte text a few pages long, with the match deep inside it.
        let note = format!(
            "a{}needle{}",
            "é".repeat(NOTE_PAGE_BYTES),
            "ç".repeat(NOTE_PAGE_BYTES / 3)
        );
        let transcript = format!("{}needle", "日本".repeat(NOTE_PAGE_BYTES / 2));
        query("INSERT INTO notes (id, title, generated_content, profile, updated_at) VALUES ('long', 'Long', ?, 'profile-a', '2026-01-01'), ('other', 'Other', 'needle', 'profile-b', '2026-01-01')")
            .bind(&note)
            .execute(&pool).await.unwrap();
        query("INSERT INTO transcripts (id, note_id, text, created_at) VALUES ('t1', 'long', ?, '2026-01-01')")
            .bind(&transcript)
            .execute(&pool).await.unwrap();

        let found = search_clovy_notes_for_profile(&pool, "profile-a", "needle")
            .await
            .unwrap();
        let note_id = found["notes"][0]["id"].as_str().unwrap();
        assert!(found["notes"][0]["note"].as_str().unwrap().len() < note.len());

        let content = read_all_pages(
            &pool,
            note_id,
            "content",
            "contentOffset",
            "nextContentOffset",
        )
        .await;
        let transcript_read = read_all_pages(
            &pool,
            note_id,
            "transcript",
            "transcriptOffset",
            "nextTranscriptOffset",
        )
        .await;
        assert_eq!(content, note);
        assert_eq!(transcript_read, transcript);

        // An offset inside a character starts at that character instead.
        let page = get_clovy_note_for_profile(&pool, "profile-a", note_id, 2, 0)
            .await
            .unwrap();
        assert_eq!(page["contentOffset"], 1);
        assert!(page["content"].as_str().unwrap().starts_with('é'));
        // Another profile's note is not readable.
        let error = get_clovy_note_for_profile(&pool, "profile-a", "other", 0, 0)
            .await
            .unwrap_err();
        assert_eq!(error.code, "note_not_found");
    }

    #[tokio::test]
    async fn a_dictation_found_by_search_is_read_whole_through_get_dictation() {
        let pool = search_pool().await;
        // Multibyte, longer than an excerpt and than one page, match at the end.
        let text = format!(
            "a{}needle",
            "ã".repeat(NOTE_PAGE_BYTES / 2 + SEARCH_EXCERPT_CHARS)
        );
        query("INSERT INTO dictation_history (id, text, language, profile, created_at) VALUES ('long', ?, 'pt', 'profile-a', '2026-01-01'), ('other', 'needle', NULL, 'profile-b', '2026-01-01')")
            .bind(&text)
            .execute(&pool).await.unwrap();

        let found = search_clovy_notes_for_profile(&pool, "profile-a", "needle")
            .await
            .unwrap();
        assert_eq!(found["dictations"].as_array().unwrap().len(), 1);
        let dictation_id = found["dictations"][0]["id"].as_str().unwrap();
        assert!(found["dictations"][0]["text"].as_str().unwrap().len() < text.len());

        let mut read = String::new();
        let mut offset = 0_usize;
        let mut pages = 0;
        loop {
            let page = get_clovy_dictation_for_profile(&pool, "profile-a", dictation_id, offset)
                .await
                .unwrap();
            assert_eq!(page["textOffset"], offset);
            assert_eq!(page["language"], "pt");
            let chunk = page["text"].as_str().unwrap();
            assert!(chunk.len() <= NOTE_PAGE_BYTES);
            read.push_str(chunk);
            pages += 1;
            match page["nextTextOffset"].as_u64() {
                Some(next) => offset = next as usize,
                None => break,
            }
        }
        assert!(pages > 1);
        assert_eq!(read, text);

        let page = get_clovy_dictation_for_profile(&pool, "profile-a", dictation_id, 2)
            .await
            .unwrap();
        assert_eq!(page["textOffset"], 1);
        for (profile, id) in [("profile-a", "other"), ("profile-a", "missing")] {
            let error = get_clovy_dictation_for_profile(&pool, profile, id, 0)
                .await
                .unwrap_err();
            assert_eq!(error.code, "dictation_not_found");
        }
    }

    #[test]
    fn file_search_is_native_bounded_and_skips_hidden_or_binary_files() {
        let directory = tempfile::tempdir().unwrap();
        let nested = directory.path().join("nested");
        fs::create_dir_all(&nested).unwrap();
        fs::write(directory.path().join("first.txt"), "needle one\nother\n").unwrap();
        fs::write(nested.join("second.txt"), "other\nneedle two\n").unwrap();
        fs::write(nested.join("binary.bin"), b"needle\0hidden").unwrap();
        fs::write(directory.path().join(".hidden.txt"), "needle hidden\n").unwrap();

        let result = search_text_files(directory.path(), "needle");

        assert!(!result.truncated);
        assert!(result.matches.contains("first.txt:1:needle one"));
        assert!(result.matches.contains("second.txt:2:needle two"));
        assert!(!result.matches.contains("binary.bin"));
        assert!(!result.matches.contains("hidden.txt"));
    }

    #[test]
    fn is_within_accepts_root_and_descendants() {
        let root = Path::new("/Users/example/Vault");
        assert!(is_within(root, root));
        assert!(is_within(&root.join("notes"), root));
        assert!(is_within(&root.join("notes").join("daily.md"), root));
    }

    #[test]
    fn is_within_rejects_textual_prefix_collisions() {
        let vault = Path::new("/Users/example/Vault");
        let sibling = Path::new("/Users/example/Vault-backup");
        assert!(!is_within(sibling, vault));
    }

    #[test]
    fn is_within_rejects_unrelated_paths() {
        let vault = Path::new("/Users/example/Vault");
        let other = Path::new("/Users/other/Documents");
        assert!(!is_within(other, vault));
    }

    fn completed_video() -> crate::clovy_api::VideoStatusDto {
        crate::clovy_api::VideoStatusDto::Completed {
            path: "/tmp/generated-video.mp4".to_string(),
            mime_type: "video/mp4".to_string(),
            size_bytes: 3,
            model: "fal-ai/wan/v2.2-a14b/text-to-video".to_string(),
        }
    }

    fn completed_outcome() -> VideoJobOutcome {
        VideoJobOutcome::Completed {
            path: "/tmp/generated-video.mp4".to_string(),
            mime_type: "video/mp4".to_string(),
            model: "fal-ai/wan/v2.2-a14b/text-to-video".to_string(),
        }
    }

    /// One fake Clovy API video service for every call of a test: it counts
    /// submits, records the polled job ids, and answers polls from a script.
    #[derive(Default)]
    struct FakeVideoService {
        submit_failure: Option<&'static str>,
        submits: std::cell::Cell<usize>,
        polled: std::cell::RefCell<Vec<String>>,
        answers: std::cell::RefCell<
            std::collections::VecDeque<Result<crate::clovy_api::VideoStatusDto, AppError>>,
        >,
    }

    impl FakeVideoService {
        fn answering(answers: Vec<Result<crate::clovy_api::VideoStatusDto, AppError>>) -> Self {
            Self {
                answers: std::cell::RefCell::new(answers.into()),
                ..Self::default()
            }
        }

        /// One `generate_video` call, from the model's arguments to its outcome.
        async fn call(
            &self,
            arguments: Value,
            cancelled: &mut ToolCancellationRegistration,
        ) -> Result<(String, VideoJobOutcome), AppError> {
            run_video_call(
                parse_video_call(&arguments, Some("call-1".to_string()))?,
                |_request| {
                    self.submits.set(self.submits.get() + 1);
                    let job = match self.submit_failure {
                        Some(code) => Err(AppError::new(code, "submit failed")),
                        None => Ok(crate::clovy_api::VideoJobDto {
                            job_id: "job-1".to_string(),
                        }),
                    };
                    async move { job }
                },
                |job_id| {
                    self.polled.borrow_mut().push(job_id);
                    let answer = self
                        .answers
                        .borrow_mut()
                        .pop_front()
                        .expect("no poll beyond the script");
                    async move { answer }
                },
                cancelled,
            )
            .await
        }
    }

    #[tokio::test]
    async fn a_video_waiting_for_the_fal_key_continues_the_same_job_with_no_new_submit() {
        for reason in [
            "fal_api_key_required",
            "fal_api_key_rejected",
            "fal_request_inaccessible",
            // The saved key could not be read, so the poll never left the desktop.
            "provider_api_key_unavailable",
        ] {
            let service = FakeVideoService::answering(vec![
                Err(AppError::new(reason, "key problem")),
                Ok(completed_video()),
            ]);
            let registry = ToolCancellationRegistry::default();
            let mut cancelled = registry.register("run-1").await;

            let (job_id, outcome) = service
                .call(json!({ "prompt": "a calm lake" }), &mut cancelled)
                .await
                .unwrap();
            assert_eq!(
                outcome,
                VideoJobOutcome::WaitingForProviderKey {
                    reason: reason.to_string()
                }
            );

            // The user saves a valid key; the model continues with only the job id.
            let (resumed_job_id, outcome) = service
                .call(json!({ "jobId": job_id }), &mut cancelled)
                .await
                .unwrap();
            assert_eq!(resumed_job_id, "job-1");
            assert_eq!(outcome, completed_outcome());
            assert_eq!(service.submits.get(), 1, "{reason}");
            assert_eq!(*service.polled.borrow(), ["job-1", "job-1"]);
        }
    }

    #[tokio::test]
    async fn a_video_that_cannot_continue_ends_without_a_fake_result_or_a_new_job() {
        let registry = ToolCancellationRegistry::default();
        let mut cancelled = registry.register("run-1").await;

        // Gone from the server for any reason, its lifetime included: terminal,
        // and nothing is created in its place.
        let service = FakeVideoService::answering(vec![Err(AppError::new(
            "video_job_unavailable",
            "not available",
        ))]);
        let error = service
            .call(json!({ "jobId": "job-1" }), &mut cancelled)
            .await
            .unwrap_err();
        assert_eq!(error.code, "video_job_unavailable");
        assert_eq!(service.submits.get(), 0);

        // A failed job or another poll error stays an error, not a waiting result.
        for (answer, code) in [
            (
                Ok(crate::clovy_api::VideoStatusDto::Failed {
                    reason: "content_rejected".to_string(),
                }),
                "video_generation_failed",
            ),
            (
                Err(AppError::new(
                    "clovy_request_failed",
                    "upstream_provider_failed",
                )),
                "clovy_request_failed",
            ),
        ] {
            let service = FakeVideoService::answering(vec![answer]);
            let error = service
                .call(json!({ "prompt": "a calm lake" }), &mut cancelled)
                .await
                .unwrap_err();
            assert_eq!(error.code, code);
            assert_eq!(service.submits.get(), 1);
        }

        // A submit that fails leaves no job to wait for or poll.
        let service = FakeVideoService {
            submit_failure: Some("provider_api_key_unavailable"),
            ..FakeVideoService::default()
        };
        let error = service
            .call(json!({ "prompt": "a calm lake" }), &mut cancelled)
            .await
            .unwrap_err();
        assert_eq!(error.code, "provider_api_key_unavailable");
        assert!(service.polled.borrow().is_empty());

        // Stop while a continued job is still running ends the call as cancelled.
        let mut stopping = registry.register("run-2").await;
        registry.cancel("run-2").await;
        let submits = std::cell::Cell::new(0);
        let error = run_video_call(
            parse_video_call(&json!({ "jobId": "job-1" }), None).unwrap(),
            |_request| {
                submits.set(submits.get() + 1);
                async {
                    Ok(crate::clovy_api::VideoJobDto {
                        job_id: "job-2".to_string(),
                    })
                }
            },
            |_job_id| std::future::pending::<Result<crate::clovy_api::VideoStatusDto, AppError>>(),
            &mut stopping,
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, "agent_tool_cancelled");
        assert_eq!(submits.get(), 0);
    }
}
