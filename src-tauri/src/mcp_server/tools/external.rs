//! Tools whose prerequisites can be satisfied by a host-authenticated MCP
//! call. Discovery and dispatch share the allowlist. Other tools still need
//! their existing session, workspace or runtime grant.
use std::path::Path;

use serde_json::Value;

use super::{
    activity_management, argument_validation, browser, computer_use, connectors, dictation,
    gateway, memory, notes, projects, recorder, routines, vault_skills, ToolCtx, ToolGroup,
    TURN_REQUIRED,
};
use crate::approval_broker::external::ExternalCall;
use crate::domain::types::AppError;

#[derive(Clone, Copy)]
enum ExternalGroup {
    Notes,
    Memory,
    Dictation,
    Projects,
    Activity,
    Routines,
    Skills,
}

impl ExternalGroup {
    const ALL: [Self; 7] = [
        Self::Notes,
        Self::Memory,
        Self::Dictation,
        Self::Projects,
        Self::Activity,
        Self::Routines,
        Self::Skills,
    ];

    fn native(self) -> &'static dyn ToolGroup {
        match self {
            Self::Notes => &notes::NoteTools,
            Self::Memory => &memory::MemoryTools,
            Self::Dictation => &dictation::DictationTools,
            Self::Projects => &projects::ProjectTools,
            Self::Activity => &activity_management::ActivityManagementTools,
            Self::Routines => &routines::RoutineTools,
            Self::Skills => &vault_skills::VaultSkillTools,
        }
    }

    fn enabled(
        self,
        ctx: &ToolCtx<'_>,
        access: &ExternalCall<'_>,
        memory_settings: Option<&Path>,
        name: &str,
    ) -> bool {
        match self {
            Self::Notes => match name {
                "delete_note" => access.recording_data().is_some(),
                "list_notes" | notes::GET_DICTATION | "create_note" | "update_note" => true,
                _ => false,
            },
            Self::Memory => ctx.data.memory_enabled && memory_settings.is_some(),
            Self::Dictation => name == "delete_dictation",
            Self::Projects => match name {
                "delete_project" => access.recording_data().is_some(),
                "list_projects"
                | "get_project"
                | "create_project"
                | "update_project"
                | "assign_note_project"
                | "remove_note_project" => true,
                _ => false,
            },
            Self::Activity => {
                ctx.app
                    .is_some_and(crate::activity::activity_settings_supported)
                    && matches!(
                        name,
                        "get_activity_settings"
                            | "update_activity_settings"
                            | "pause_activity"
                            | "resume_activity"
                    )
            }
            Self::Routines => name != "run_routine" || ctx.app.is_some(),
            Self::Skills => {
                matches!(
                    name,
                    "list_skills"
                        | "load_skill"
                        | "list_skill_settings"
                        | "read_skill"
                        | "update_skill"
                        | "enable_skill"
                        | "disable_skill"
                ) && ctx.app.is_some_and(|app| {
                    crate::agent_sessions::skills::skill_roots(app)
                        .first()
                        .is_some_and(|(_, managed)| *managed)
                })
            }
        }
    }
}

pub(crate) async fn external_descriptors(
    ctx: &ToolCtx<'_>,
    access: &ExternalCall<'_>,
    memory_settings: Option<&Path>,
) -> Vec<Value> {
    let mut tools = super::descriptors(ctx).await;
    for group in ExternalGroup::ALL {
        tools.extend(group.native().descriptors(ctx).into_iter().filter(|tool| {
            tool["name"]
                .as_str()
                .is_some_and(|name| group.enabled(ctx, access, memory_settings, name))
        }));
    }
    tools.extend(connectors::CONNECTORS.external_catalog(ctx).await);
    if let Some(app) = ctx.app {
        tools.extend(
            crate::connectors::notion::external_descriptors(app, access)
                .await
                .unwrap_or_default()
                .iter()
                .map(connectors::descriptor),
        );
    }
    tools.extend(gateway::GATEWAY.external_catalog(ctx, access).await);
    tools.extend(browser::BROWSER.external_descriptors(ctx));
    tools.extend(computer_use::COMPUTER_USE.external_descriptors(ctx).await);
    tools.extend(recorder::external_descriptors(ctx));
    tools
}

pub(crate) async fn call_external(
    ctx: &ToolCtx<'_>,
    access: &ExternalCall<'_>,
    memory_settings: Option<&Path>,
    name: &str,
    arguments: &Value,
) -> Result<Value, AppError> {
    access.check_read_scope()?;
    // Preserve existing arguments, filters and results of the public reads.
    if super::ReadTools.owns(name) {
        let result = super::call(ctx, name, arguments).await?;
        access.check_read_scope()?;
        return Ok(result);
    }
    if connectors::is_account_tool(name) {
        return connectors::CONNECTORS
            .call_external(ctx, access, name, arguments)
            .await;
    }
    if crate::connectors::notion::runtime_name_to_provider(name).is_some() {
        let app = ctx.app.ok_or_else(|| {
            AppError::new(
                "connector_unavailable",
                "The connector host is unavailable.",
            )
        })?;
        return crate::connectors::notion::call_external(
            app,
            access,
            &ctx.data.profile,
            name,
            arguments,
        )
        .await;
    }
    if gateway::GATEWAY.owns(name) {
        return gateway::GATEWAY
            .call_external(ctx, access, name, arguments)
            .await;
    }
    if browser::is_browser_tool(name) {
        return browser::BROWSER
            .call_external(ctx, access, name, arguments)
            .await;
    }
    if computer_use::COMPUTER_USE.owns(name) {
        return computer_use::COMPUTER_USE
            .call_external(ctx, access, arguments)
            .await;
    }
    if recorder::RecorderTools.owns(name) {
        return recorder::call_external(ctx, access, name, arguments).await;
    }
    let group = ExternalGroup::ALL
        .into_iter()
        .find(|group| {
            group.native().owns(name) && group.enabled(ctx, access, memory_settings, name)
        })
        .ok_or_else(|| {
            AppError::new(
                TURN_REQUIRED,
                format!("The external MCP connection has no context for `{name}`."),
            )
        })?;
    let descriptor = group
        .native()
        .descriptors(ctx)
        .into_iter()
        .find(|tool| tool["name"] == name)
        .ok_or_else(|| AppError::new("mcp_tool_unknown", "The MCP tool was not found."))?;
    argument_validation::validate(&descriptor["inputSchema"], arguments)
        .map_err(|error| AppError::new("agent_tool_arguments_invalid", error))?;
    let result = match group {
        ExternalGroup::Notes => notes::call_note_tool(ctx, Some(access), name, arguments).await?,
        ExternalGroup::Memory => {
            let settings = memory_settings.ok_or_else(|| {
                AppError::new(
                    "memory_unavailable",
                    "Clovy's memory settings are unavailable.",
                )
            })?;
            memory::call_external(ctx, access, name, arguments, settings).await?
        }
        ExternalGroup::Dictation => dictation::delete_external(ctx, access, arguments).await?,
        ExternalGroup::Projects => projects::dispatch(ctx, Some(access), name, arguments).await?,
        ExternalGroup::Activity => {
            activity_management::call_external(ctx, access, name, arguments).await?
        }
        ExternalGroup::Routines => routines::dispatch(ctx, Some(access), name, arguments).await?,
        ExternalGroup::Skills => vault_skills::dispatch(ctx, Some(access), name, arguments).await?,
    };
    if descriptor["annotations"]["readOnlyHint"] == true {
        access.check_read_scope()?;
    }
    Ok(result)
}
