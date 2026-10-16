//! Routine tools reuse the same persistence and scheduling functions as the
//! legacy agent. Mutations wait for one approval before entering that code.
use super::{ToolCtx, ToolGroup};
use crate::approval_broker::{ApprovalKind, ApprovalRequest, ApprovalTurn};
use crate::domain::types::AppError;
use crate::mcp_server::turns::TURN_REQUIRED;
use futures_util::future::BoxFuture;
use serde_json::{json, Value};

pub struct RoutineTools;
const NAMES: [&str; 6] = [
    "list_routines",
    "create_routine",
    "update_routine",
    "pause_routine",
    "resume_routine",
    "delete_routine",
];

impl ToolGroup for RoutineTools {
    fn owns(&self, name: &str) -> bool {
        NAMES.contains(&name)
    }

    fn descriptors(&self, _ctx: &ToolCtx<'_>) -> Vec<Value> {
        NAMES.into_iter().map(|name| {
            let mut properties = json!({});
            let mut required = Vec::new();
            if matches!(name,"create_routine"|"update_routine") {
                properties = json!({"name":{"type":"string"},"prompt":{"type":"string"},"schedule":{"type":"string","description":"RFC 3339, every <n>m/h/d, or a five-field cron expression."},"safetyMode":{"type":"string","enum":["sandboxed","unrestricted"]}});
            }
            if !matches!(name,"list_routines"|"create_routine") {
                properties["routineId"] = json!({"type":"string"});
                required.push("routineId");
            }
            if name == "create_routine" { required.extend(["prompt","schedule","safetyMode"]); }
            json!({"name":name,"description":description(name),"inputSchema":{"type":"object","properties":properties,"required":required,"additionalProperties":false},"annotations":{"readOnlyHint":name=="list_routines","openWorldHint":false}})
        }).collect()
    }

    fn call<'a>(
        &'a self,
        ctx: &'a ToolCtx<'a>,
        name: &'a str,
        arguments: &'a Value,
    ) -> BoxFuture<'a, Result<Value, AppError>> {
        Box::pin(async move {
            let binding = ctx.binding.ok_or_else(|| {
                AppError::new(TURN_REQUIRED, "Routine tools require a Clovy turn.")
            })?;
            if name != "list_routines" {
                ctx.broker()?
                    .request(
                        &ApprovalTurn::from(binding),
                        ApprovalRequest {
                            tool: name.into(),
                            call_id: None,
                            kind: ApprovalKind::Approval {
                                title: title(name).into(),
                                description: description(name).into(),
                                arguments: arguments.clone(),
                                preview: None,
                            },
                        },
                    )
                    .await
                    .map_err(|denial| AppError::new(denial.code(), denial.message()))?;
            }
            let pool = &ctx.data.notes;
            let result = match name {
                "list_routines" => serde_json::to_value(crate::routines::list(pool).await?),
                "create_routine" => {
                    let request =
                        serde_json::from_value(arguments.clone()).map_err(invalid_arguments)?;
                    serde_json::to_value(crate::routines::create(pool, request).await?)
                }
                "update_routine" => {
                    let request =
                        serde_json::from_value(arguments.clone()).map_err(invalid_arguments)?;
                    serde_json::to_value(crate::routines::update(pool, request).await?)
                }
                "pause_routine" => serde_json::to_value(
                    crate::routines::pause(pool, routine_id(arguments)?).await?,
                ),
                "resume_routine" => serde_json::to_value(
                    crate::routines::resume(pool, routine_id(arguments)?).await?,
                ),
                "delete_routine" => {
                    let id = routine_id(arguments)?;
                    crate::routines::delete(pool, id).await?;
                    return Ok(json!({"deleted":true,"routineId":id}));
                }
                _ => return Err(AppError::new("mcp_tool_unknown", "Unknown routine tool.")),
            };
            result.map_err(|error| AppError::new("agent_tool_response_invalid", error.to_string()))
        })
    }
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
        _ => "List routines",
    }
}
fn description(name: &str) -> &'static str {
    match name {
        "create_routine" => "Create a Clovy routine with the requested instructions and timing.",
        "update_routine" => "Update the selected Clovy routine.",
        "pause_routine" => "Pause future runs of the selected Clovy routine.",
        "resume_routine" => "Resume scheduled runs of the selected Clovy routine.",
        "delete_routine" => "Permanently delete the selected Clovy routine.",
        _ => "List Clovy routines and their schedules.",
    }
}
