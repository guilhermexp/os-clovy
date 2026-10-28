//! Vault discovery and skill loading shared with the legacy agent. MCP uses
//! globally enabled, discovered skills; the native caller supplies its run snapshot.
use super::{ToolCtx, ToolGroup};
use crate::agent_sessions::skill_identity::{canonical_skill_id, read_skill_ids};
use crate::agent_sessions::AgentRepository;
use crate::domain::types::AppError;
use crate::mcp_server::turns::TURN_REQUIRED;
use futures_util::future::BoxFuture;
use serde_json::{json, Value};
use std::{collections::HashMap, path::PathBuf};
use tauri::AppHandle;

pub struct VaultSkillTools;
impl ToolGroup for VaultSkillTools {
    fn owns(&self, name: &str) -> bool {
        matches!(name, "get_obsidian_vault" | "list_skills" | "load_skill")
    }
    fn descriptors(&self, _ctx: &ToolCtx<'_>) -> Vec<Value> {
        [
            ("get_obsidian_vault","Discover the currently selected Obsidian vault for each distinct task. Discovery does not authorize writes; never infer an unavailable vault's path.",json!({}),vec![]),
            ("list_skills","List globally enabled Clovy skills.",json!({}),vec![]),
            ("load_skill","Load instructions for one globally enabled Clovy skill.",json!({"name":{"type":"string"}}),vec!["name"]),
        ].into_iter().map(|(name,description,properties,required)| json!({"name":name,"description":description,"inputSchema":{"type":"object","properties":properties,"required":required,"additionalProperties":false},"annotations":{"readOnlyHint":true,"openWorldHint":false,"idempotentHint":true}})).collect()
    }
    fn call<'a>(
        &'a self,
        ctx: &'a ToolCtx<'a>,
        name: &'a str,
        arguments: &'a Value,
    ) -> BoxFuture<'a, Result<Value, AppError>> {
        Box::pin(async move {
            require_turn(ctx)?;
            let app = ctx.app.ok_or_else(|| {
                AppError::new(
                    "mcp_app_unavailable",
                    "Clovy's vault and skill directories are unavailable.",
                )
            })?;
            if name == "get_obsidian_vault" {
                return vault_result(crate::obsidian::discovery_for_app(app));
            }
            call_with_roots(ctx, name, arguments, &skill_roots(app)).await
        })
    }
}

pub(crate) fn vault_result(
    discovery: Result<crate::obsidian::ObsidianDiscovery, AppError>,
) -> Result<Value, AppError> {
    serde_json::to_value(discovery?)
        .map_err(|error| AppError::new("agent_tool_response_invalid", error.to_string()))
}

pub(super) async fn call_with_roots(
    ctx: &ToolCtx<'_>,
    name: &str,
    arguments: &Value,
    roots: &[PathBuf],
) -> Result<Value, AppError> {
    require_turn(ctx)?;
    let requested = if name == "load_skill" {
        let requested = arguments
            .get("name")
            .and_then(Value::as_str)
            .filter(|name| !name.is_empty())
            .ok_or_else(|| AppError::new("agent_tool_arguments_invalid", "name is required."))?;
        Some(validate_skill_name(requested)?)
    } else {
        None
    };
    let enabled: HashMap<String, bool> = AgentRepository::new(ctx.data.notes.clone())
        .skills()
        .await?
        .into_iter()
        .map(|skill| (canonical_skill_id(&skill.id).to_string(), skill.enabled))
        .collect();
    let catalog = list_skills_from_roots(roots, &enabled, true).await?;
    match name {
        "list_skills" => Ok(catalog),
        "load_skill" => {
            // Only discovered, enabled names are admitted, as with the native
            // run snapshot. An arbitrary name is never an implicit file grant.
            let allowed = catalog["skills"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|skill| skill["name"].as_str())
                .map(|name| (name.to_string(), true))
                .collect();
            load_skill_from_roots(roots, &allowed, requested.unwrap_or_default()).await
        }
        _ => Err(AppError::new(
            "mcp_tool_unknown",
            "Unknown vault or skill tool.",
        )),
    }
}
fn require_turn(ctx: &ToolCtx<'_>) -> Result<(), AppError> {
    ctx.binding
        .map(|_| ())
        .ok_or_else(|| AppError::new(TURN_REQUIRED, "Vault and skill reads require a Clovy turn."))
}

pub(crate) fn skill_roots(app: &AppHandle) -> Vec<PathBuf> {
    let mut roots = crate::app_paths::app_data_dir(app)
        .ok()
        .map(|path| path.join("agents").join("skills"))
        .into_iter()
        .collect::<Vec<_>>();
    if let Some(home) = std::env::var_os("HOME") {
        roots.push(PathBuf::from(home).join(".agents").join("skills"));
    }
    roots
}

pub(crate) async fn list_skills_from_roots(
    roots: &[PathBuf],
    enabled: &HashMap<String, bool>,
    default_enabled: bool,
) -> Result<Value, AppError> {
    let mut skills = Vec::new();
    for root in roots {
        let Ok(mut entries) = tokio::fs::read_dir(root).await else {
            continue;
        };
        while let Ok(Some(entry)) = entries.next_entry().await {
            let stored_name = entry.file_name().to_string_lossy().into_owned();
            let name = canonical_skill_id(&stored_name).to_string();
            if stored_name != name && root.join(&name).join("SKILL.md").is_file() {
                continue;
            }
            if enabled.get(&name).copied().unwrap_or(default_enabled)
                && entry.path().join("SKILL.md").is_file()
                && !skills.iter().any(|skill: &Value| {
                    skill.get("name").and_then(Value::as_str) == Some(name.as_str())
                })
            {
                skills.push(json!({"name":name,"root":root}));
            }
        }
    }
    Ok(json!({"skills":skills}))
}

pub(crate) fn validate_skill_name(requested: &str) -> Result<&str, AppError> {
    if requested.contains('/') || requested.contains('\\') || requested == "." || requested == ".."
    {
        return Err(AppError::new(
            "agent_skill_invalid",
            "Skill name is invalid.",
        ));
    }
    Ok(canonical_skill_id(requested))
}

pub(crate) async fn load_skill_from_roots(
    roots: &[PathBuf],
    enabled: &HashMap<String, bool>,
    requested: &str,
) -> Result<Value, AppError> {
    let name = validate_skill_name(requested)?;
    if !enabled.get(name).copied().unwrap_or(false) {
        return Err(AppError::new(
            "agent_skill_disabled",
            "This skill is not enabled for the current run.",
        ));
    }
    for root in roots {
        for read_id in read_skill_ids(name) {
            let path = root.join(read_id).join("SKILL.md");
            if path.is_file() {
                let content = tokio::fs::read_to_string(&path)
                    .await
                    .map_err(|error| AppError::new("agent_tool_io_failed", error.to_string()))?;
                return Ok(json!({"name":name,"content":content,"path":path}));
            }
        }
    }
    Err(AppError::new(
        "agent_skill_not_found",
        "Skill was not found.",
    ))
}
