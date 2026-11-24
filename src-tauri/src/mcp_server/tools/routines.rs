//! Routine management uses durable claims and the unattended CLI domain path.
use super::{ToolCtx, ToolGroup};
use crate::approval_broker::{external::ExternalCall, ApprovalKind, ApprovalRequest};
use crate::domain::types::AppError;
use crate::llm::shell_env::LoginEnv;
use crate::routines::{AgentRoutineDto, AgentRoutineRunDto, Claim, RoutineMutation};
use futures_util::future::BoxFuture;
use serde_json::{json, Value};
use sqlx_sqlite::SqlitePool;
use std::sync::Arc;
use tauri::Emitter;

pub struct RoutineTools;
const NAMES: [&str; 8] = [
    "list_routines",
    "create_routine",
    "update_routine",
    "pause_routine",
    "resume_routine",
    "delete_routine",
    "run_routine",
    "list_routine_runs",
];
fn read_only(name: &str) -> bool {
    matches!(name, "list_routines" | "list_routine_runs")
}

impl ToolGroup for RoutineTools {
    fn owns(&self, name: &str) -> bool {
        NAMES.contains(&name)
    }
    fn descriptors(&self, _ctx: &ToolCtx<'_>) -> Vec<Value> {
        NAMES.into_iter().map(|name| {
            let mut properties = json!({});
            let mut required = Vec::new();
            if matches!(name, "create_routine" | "update_routine") {
                properties = json!({
                    "name":{"type":"string","minLength":1}, "prompt":{"type":"string","minLength":1},
                    "schedule":{"type":"string","description":"RFC 3339, every <n>m/h/d, or a five-field cron expression."},
                    "timezone":{"type":"string","description":"IANA timezone, for example America/Sao_Paulo; defaults to UTC."},
                    "model":{"type":"string","description":"auto or an installed CLI chat engine model id. Execution resolves to the supported unattended CLI, never an endpoint model."},
                    "safetyMode":{"type":"string","enum":["sandboxed","unrestricted"]},
                    "enabledToolsets":{"type":"array","items":{"type":"string"},"uniqueItems":true,"description":"Capability selection only. Does not create trust, grants, browser access or approval exemptions."},
                    "repeat":{"type":"string"},"deliver":{"type":"string","enum":["local"]},
                    "enabled":{"type":"boolean"},"state":{"type":"string","enum":["scheduled","paused","completed","needs_review"]}
                });
            }
            if !matches!(name, "list_routines" | "create_routine") {
                properties["routineId"] = json!({"type":"string","minLength":1});
                if name != "list_routine_runs" { required.push("routineId"); }
            }
            if name == "create_routine" { required.extend(["prompt", "schedule", "safetyMode"]); }
            if name == "list_routine_runs" {
                properties["limit"] = json!({"type":"integer","minimum":1,"maximum":100});
                properties["offset"] = json!({"type":"integer","minimum":0});
            }
            json!({"name":name,"description":description(name),"inputSchema":{"type":"object","properties":properties,"required":required,"additionalProperties":false},"annotations":{"readOnlyHint":read_only(name),"openWorldHint":name=="run_routine"}})
        }).collect()
    }
    fn call<'a>(
        &'a self,
        ctx: &'a ToolCtx<'a>,
        name: &'a str,
        arguments: &'a Value,
    ) -> BoxFuture<'a, Result<Value, AppError>> {
        Box::pin(dispatch(ctx, None, name, arguments))
    }
}

pub(super) trait RoutineRunPort: Send + Sync {
    fn environment(&self) -> BoxFuture<'_, Arc<LoginEnv>>;

    fn launch<'a>(
        &'a self,
        pool: &'a SqlitePool,
        claim: Claim,
        mutation: RoutineMutation<'a>,
    ) -> BoxFuture<'a, Result<AgentRoutineRunDto, AppError>>;
}

struct AppRoutineRun<'a>(&'a tauri::AppHandle);

impl RoutineRunPort for AppRoutineRun<'_> {
    fn environment(&self) -> BoxFuture<'_, Arc<LoginEnv>> {
        Box::pin(crate::llm::shell_env::login_env())
    }

    fn launch<'a>(
        &'a self,
        pool: &'a SqlitePool,
        claim: Claim,
        mutation: RoutineMutation<'a>,
    ) -> BoxFuture<'a, Result<AgentRoutineRunDto, AppError>> {
        Box::pin(crate::routines::start_claim_authorized(
            self.0, pool, claim, mutation,
        ))
    }
}

pub(super) async fn dispatch(
    ctx: &ToolCtx<'_>,
    external: Option<&ExternalCall<'_>>,
    name: &str,
    arguments: &Value,
) -> Result<Value, AppError> {
    let runner = ctx.app.map(AppRoutineRun);
    dispatch_with_runner(
        ctx,
        external,
        name,
        arguments,
        runner.as_ref().map(|port| port as &dyn RoutineRunPort),
    )
    .await
}

pub(super) async fn dispatch_with_runner(
    ctx: &ToolCtx<'_>,
    external: Option<&ExternalCall<'_>>,
    name: &str,
    arguments: &Value,
    runner: Option<&dyn RoutineRunPort>,
) -> Result<Value, AppError> {
    ctx.revalidate_local(external).await?;
    validate_arguments(&RoutineTools, ctx, name, arguments)?;
    if read_only(name) {
        return read(ctx, name, arguments, external.is_some()).await;
    }
    let pool = &ctx.data.notes;
    let id = if name == "create_routine" {
        None
    } else {
        Some(routine_id(arguments)?)
    };
    let target = match (external, id) {
        (Some(_), Some(id)) => Some(crate::routines::external_target(pool, id).await?),
        (None, Some(id)) => {
            crate::routines::get(pool, id).await?;
            None
        }
        _ => None,
    };
    if external.is_some() {
        let requests_unrestricted =
            arguments.get("safetyMode").and_then(Value::as_str) == Some("unrestricted");
        let existing_unrestricted = target
            .as_ref()
            .is_some_and(|target| target["safetyMode"] == "unrestricted");
        // Approval of an external operation never enables unrestricted local
        // command execution. The owner can still configure that in Rotinas.
        if requests_unrestricted
            || (existing_unrestricted
                && matches!(name, "run_routine" | "resume_routine" | "update_routine")
                && arguments.get("safetyMode").and_then(Value::as_str) != Some("sandboxed"))
        {
            return Err(AppError::new(
                "routine_external_safety_mode",
                "External MCP clients can only configure or start sandboxed routine execution.",
            ));
        }
    }
    let environment = if name == "run_routine" && external.is_some() {
        Some(runner.ok_or_else(runtime_unavailable)?.environment().await)
    } else {
        None
    };
    let target = if let Some(target) = target {
        let resolved = environment
            .as_ref()
            .map(|env| {
                let model = target["model"].as_str().unwrap_or("auto");
                crate::routines::external_routine_model(model, env)
            })
            .transpose()?;
        Some(json!({"routine":target,"resolvedModel":resolved}))
    } else {
        None
    };
    let grant = ctx
        .approve_local(
            external,
            ApprovalRequest {
                tool: name.into(),
                call_id: Some(uuid::Uuid::new_v4().to_string()),
                kind: ApprovalKind::Approval {
                    title: title(name).into(),
                    description: description(name).into(),
                    arguments: arguments.clone(),
                    preview: None,
                },
            },
            target.as_ref(),
        )
        .await?;
    ctx.revalidate_local(external).await?;
    let mutation = Some((ctx.data.profile.as_str(), grant));
    let routine_id =
        || id.ok_or_else(|| AppError::new("invalid_arguments", "A routine id is required."));
    let result = match name {
        "create_routine" => format_routine(
            crate::routines::create_authorized(
                pool,
                serde_json::from_value(arguments.clone()).map_err(invalid_arguments)?,
                mutation,
            )
            .await?,
            external.is_some(),
        )?,
        "update_routine" => format_routine(
            crate::routines::update_authorized(
                pool,
                serde_json::from_value(arguments.clone()).map_err(invalid_arguments)?,
                mutation,
            )
            .await?,
            external.is_some(),
        )?,
        "pause_routine" => format_routine(
            crate::routines::pause_authorized(pool, routine_id()?, mutation).await?,
            external.is_some(),
        )?,
        "resume_routine" => format_routine(
            crate::routines::resume_authorized(pool, routine_id()?, mutation).await?,
            external.is_some(),
        )?,
        "delete_routine" => {
            let id = routine_id()?;
            crate::routines::delete_authorized(pool, id, mutation).await?;
            json!({"deleted":true,"routineId":id})
        }
        "run_routine" => {
            let id = routine_id()?;
            let run = if let Some(environment) = environment {
                let runner = runner.ok_or_else(runtime_unavailable)?;
                crate::routines::trigger_using(pool, id, mutation, &environment, |claim| {
                    runner.launch(pool, claim, mutation)
                })
                .await?
            } else {
                let app = ctx.app.ok_or_else(runtime_unavailable)?;
                crate::routines::trigger_and_start_authorized(app, pool, id, mutation).await?
            };
            let run = match run {
                Some(run) => run,
                None => return Err(crate::routines::trigger_blocked_error(pool, id).await?),
            };
            if external.is_some() {
                external_run(run)
            } else {
                to_json(run)?
            }
        }
        _ => return Err(AppError::new("mcp_tool_unknown", "Unknown routine tool.")),
    };
    if let Some(app) = ctx.app {
        let _ = app.emit("clovy-data-changed", json!({"profile":ctx.data.profile,"kind":"routines","id":id.or_else(||result["id"].as_str())}));
    }
    Ok(result)
}

fn runtime_unavailable() -> AppError {
    AppError::new(
        "mcp_app_unavailable",
        "Routine execution requires the running Clovy app.",
    )
}

fn to_json(value: impl serde::Serialize) -> Result<Value, AppError> {
    serde_json::to_value(value)
        .map_err(|error| AppError::new("agent_tool_response_invalid", error.to_string()))
}

fn format_routine(routine: AgentRoutineDto, external: bool) -> Result<Value, AppError> {
    if external {
        Ok(external_routine(routine))
    } else {
        to_json(routine)
    }
}

fn external_routine(routine: AgentRoutineDto) -> Value {
    json!({
        "id":routine.id,"name":routine.name,"schedule":routine.schedule,
        "timezone":routine.timezone,"state":routine.state,"enabled":routine.enabled,
        "nextRunAt":routine.next_run_at,"lastRunAt":routine.last_run_at,"lastStatus":routine.last_status
    })
}

fn external_run(run: AgentRoutineRunDto) -> Value {
    json!({
        "id":run.id,"routineId":run.routine_id,"triggerKind":run.trigger_kind,
        "status":run.status,"scheduledFor":run.scheduled_for,"startedAt":run.started_at,
        "completedAt":run.completed_at,"model":run.model,"errorCode":run.error_code,
    })
}

/// Routine status is installation-wide. External projection omits the saved
/// prompt, grants, execution metadata, and linked conversation/run IDs.
pub(super) async fn read(
    ctx: &ToolCtx<'_>,
    name: &str,
    arguments: &Value,
    external: bool,
) -> Result<Value, AppError> {
    let pool = &ctx.data.notes;
    if name == "list_routines" {
        let routines = crate::routines::list(pool).await?;
        if !external {
            return serde_json::to_value(routines)
                .map_err(|error| AppError::new("agent_tool_response_invalid", error.to_string()));
        }
        return Ok(json!(routines
            .into_iter()
            .map(external_routine)
            .collect::<Vec<_>>()));
    }
    let id = arguments.get("routineId").and_then(Value::as_str);
    if let Some(id) = id {
        crate::routines::get(pool, id).await?;
    }
    let limit = arguments
        .get("limit")
        .and_then(Value::as_i64)
        .unwrap_or(20)
        .clamp(1, 100);
    let offset = arguments
        .get("offset")
        .and_then(Value::as_i64)
        .unwrap_or(0)
        .max(0);
    let mut runs = crate::routines::list_runs_page(pool, id, limit + 1, offset).await?;
    let has_more = runs.len() > limit as usize;
    runs.truncate(limit as usize);
    let items = if external {
        json!(runs.into_iter().map(external_run).collect::<Vec<_>>())
    } else {
        json!(runs)
    };
    Ok(
        json!({"items":items,"offset":offset,"hasMore":has_more,"nextOffset":has_more.then_some(offset.saturating_add(limit))}),
    )
}

fn invalid_arguments(error: serde_json::Error) -> AppError {
    AppError::new("invalid_arguments", error.to_string())
}
fn routine_id(arguments: &Value) -> Result<&str, AppError> {
    arguments
        .get("routineId")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .ok_or_else(|| AppError::new("agent_tool_arguments_invalid", "routineId is required."))
}
fn title(name: &str) -> &'static str {
    match name {
        "create_routine" => "Create a routine",
        "update_routine" => "Update a routine",
        "pause_routine" => "Pause a routine",
        "resume_routine" => "Resume a routine",
        "delete_routine" => "Delete a routine",
        "run_routine" => "Run a routine",
        _ => "List routines",
    }
}
fn description(name: &str) -> &'static str {
    match name {
        "create_routine" => "Create a Clovy routine with the requested instructions and timing.",
        "update_routine" => "Update routine configuration. Toolset selection does not grant authority.",
        "pause_routine" => "Pause future runs of the selected Clovy routine.",
        "resume_routine" => "Resume scheduled runs of the selected Clovy routine.",
        "delete_routine" => "Permanently delete the selected Clovy routine.",
        "run_routine" => "Start the selected routine through its single-flight unattended CLI path and saved grants. May incur the CLI provider's usage costs. Paused or active routines refuse execution.",
        "list_routine_runs" => "Read paginated routine execution history, status, CLI model and errors.",
        _ => "List Clovy routines and their schedules.",
    }
}

fn validate_arguments(
    group: &RoutineTools,
    ctx: &ToolCtx<'_>,
    name: &str,
    arguments: &Value,
) -> Result<(), AppError> {
    let object = arguments.as_object().ok_or_else(|| {
        AppError::new("invalid_arguments", "Routine arguments must be an object.")
    })?;
    let descriptor = group
        .descriptors(ctx)
        .into_iter()
        .find(|tool| tool["name"] == name)
        .ok_or_else(|| AppError::new("mcp_tool_unknown", "Unknown routine tool."))?;
    let properties = descriptor["inputSchema"]["properties"]
        .as_object()
        .ok_or_else(|| AppError::new("invalid_arguments", "Routine schema is unavailable."))?;
    if object.keys().any(|key| !properties.contains_key(key)) {
        return Err(AppError::new(
            "invalid_arguments",
            "Unknown routine argument. Host ids, metadata and grants cannot be supplied.",
        ));
    }
    Ok(())
}
