//! The browser group: the 14 Browser use tools of the native agent, served to
//! an attended CLI turn over the same `BrowserBroker` (contract C4 of the
//! change `cli-substitui-agente-nativo`; names fixed by its `clovy-mcp-server`
//! delta). The broker keeps its own approval: a consequential action on an
//! origin the user has not allowed parks in the browser approval tray
//! (`browser_approvals_pending` / `browser_approval_respond`) and the tool
//! answers `{parked: true, actionId}`, exactly as for the native agent. The
//! approval broker is not asked again, so the user never sees two cards for
//! one click. A turn nobody watches (routines) gets no browser: the tools are
//! hidden and refuse with `unattended_requires_approval`.
//!
//! The catalog and the dispatch are shared with the native agent
//! (`agent_runtime::api::tool_descriptors`, `agent_runtime::tools`).

use std::sync::Arc;

use futures_util::future::BoxFuture;
use serde_json::{json, Value};
use tauri::Manager;

use super::{ToolCtx, ToolGroup};
use crate::approval_broker::ApprovalDenial;
use crate::browser_broker::{BrowserBroker, BrowserBrokerContext};
use crate::domain::types::AppError;
use crate::mcp_server::turns::TURN_REQUIRED;

/// The Browser use tools, in catalog order.
pub const BROWSER_TOOLS: [&str; 14] = [
    "start_session",
    "close_session",
    "navigate",
    "snapshot",
    "screenshot",
    "click",
    "fill",
    "press",
    "back",
    "list_tabs",
    "open_tab",
    "switch_tab",
    "close_tab",
    "accept_shared_tab",
];

/// Tools that only read the browser session (`readOnlyHint`).
const READ_ONLY: [&str; 3] = ["snapshot", "screenshot", "list_tabs"];

pub fn is_browser_tool(name: &str) -> bool {
    BROWSER_TOOLS.contains(&name)
}

fn session_only() -> Value {
    json!({ "type": "object", "properties": { "session_id": { "type": "string" } }, "required": ["session_id"], "additionalProperties": true })
}

/// `(name, description, JSON Schema of the arguments)` of every Browser use
/// tool, shared by the native agent's catalog and this group.
pub(crate) fn tool_specs() -> Vec<(&'static str, &'static str, Value)> {
    vec![
        (
            "start_session",
            "Start an attended Clovy Browser use session.",
            json!({ "type": "object", "properties": {}, "required": [], "additionalProperties": false }),
        ),
        (
            "close_session",
            "Close an attended Clovy Browser use session.",
            json!({ "type": "object", "properties": { "session_id": { "type": "string" } }, "required": ["session_id"], "additionalProperties": false }),
        ),
        (
            "navigate",
            "Navigate a Clovy Browser use session to a URL.",
            json!({ "type": "object", "properties": { "session_id": { "type": "string" }, "url": { "type": "string" } }, "required": ["session_id", "url"], "additionalProperties": true }),
        ),
        (
            "snapshot",
            "Read the visible page and interactive references from a Clovy Browser use session.",
            session_only(),
        ),
        (
            "screenshot",
            "Capture a screenshot from a Clovy Browser use session.",
            session_only(),
        ),
        (
            "click",
            "Click an interactive reference in a Clovy Browser use session.",
            json!({ "type": "object", "properties": { "session_id": { "type": "string" }, "ref": { "type": "string" } }, "required": ["session_id", "ref"], "additionalProperties": true }),
        ),
        (
            "fill",
            "Fill an interactive reference in a Clovy Browser use session.",
            json!({ "type": "object", "properties": { "session_id": { "type": "string" }, "ref": { "type": "string" }, "value": { "type": "string" } }, "required": ["session_id", "ref", "value"], "additionalProperties": true }),
        ),
        (
            "press",
            "Press a key in a Clovy Browser use session.",
            json!({ "type": "object", "properties": { "session_id": { "type": "string" }, "key": { "type": "string" } }, "required": ["session_id", "key"], "additionalProperties": true }),
        ),
        (
            "back",
            "Navigate back in a Clovy Browser use session.",
            session_only(),
        ),
        (
            "list_tabs",
            "List tabs owned by a Clovy Browser use session.",
            session_only(),
        ),
        (
            "open_tab",
            "Open a task-owned tab in a Clovy Browser use session.",
            json!({ "type": "object", "properties": { "session_id": { "type": "string" }, "url": { "type": "string" } }, "required": ["session_id"], "additionalProperties": true }),
        ),
        (
            "switch_tab",
            "Switch the active task-owned tab in a Clovy Browser use session.",
            json!({ "type": "object", "properties": { "session_id": { "type": "string" }, "tab_id": {} }, "required": ["session_id", "tab_id"], "additionalProperties": true }),
        ),
        (
            "close_tab",
            "Close a task-owned tab in a Clovy Browser use session.",
            json!({ "type": "object", "properties": { "session_id": { "type": "string" }, "tab_id": {} }, "required": ["session_id", "tab_id"], "additionalProperties": true }),
        ),
        (
            "accept_shared_tab",
            "Accept a one-use browser tab share code supplied by the user.",
            json!({ "type": "object", "properties": { "session_id": { "type": "string" }, "share_code": { "type": "string" } }, "required": ["session_id", "share_code"], "additionalProperties": true }),
        ),
    ]
}

/// The native agent's descriptors (`parameters`, no annotations).
pub(crate) fn runtime_descriptors() -> Vec<Value> {
    tool_specs()
        .into_iter()
        .map(|(name, description, parameters)| {
            json!({ "name": name, "description": description, "parameters": parameters })
        })
        .collect()
}

/// Runs one Browser use tool in the attended context, where consequential
/// actions park in the browser approval tray. Shared with the native agent.
pub(crate) async fn execute_attended(
    broker: &BrowserBroker,
    name: &str,
    arguments: Value,
) -> Result<Value, AppError> {
    broker
        .execute_for(BrowserBrokerContext::Attended, name, arguments)
        .await
}

/// The browser group. The registered instance uses the app's broker; tests
/// build one over their own broker and fake transport.
pub struct BrowserTools {
    broker: Option<Arc<BrowserBroker>>,
}

/// The instance registered in [`super::groups`].
pub static BROWSER: BrowserTools = BrowserTools { broker: None };

impl BrowserTools {
    #[cfg(test)]
    pub(crate) fn with_broker(broker: Arc<BrowserBroker>) -> Self {
        Self {
            broker: Some(broker),
        }
    }

    fn broker(&self, ctx: &ToolCtx<'_>) -> Result<Arc<BrowserBroker>, AppError> {
        if let Some(broker) = &self.broker {
            return Ok(Arc::clone(broker));
        }
        ctx.app
            .and_then(|app| app.try_state::<Arc<BrowserBroker>>())
            .map(|state| Arc::clone(state.inner()))
            .ok_or_else(|| {
                AppError::new(
                    "browser_unavailable",
                    "Browser use is unavailable in this Clovy process.",
                )
            })
    }
}

impl ToolGroup for BrowserTools {
    fn owns(&self, name: &str) -> bool {
        is_browser_tool(name)
    }

    fn descriptors(&self, ctx: &ToolCtx<'_>) -> Vec<Value> {
        if !ctx.binding.is_some_and(|binding| binding.attended) {
            return Vec::new();
        }
        tool_specs()
            .into_iter()
            .map(|(name, description, parameters)| {
                json!({
                    "name": name,
                    "description": description,
                    "inputSchema": parameters,
                    "annotations": {
                        "readOnlyHint": READ_ONLY.contains(&name),
                        "openWorldHint": true,
                    },
                })
            })
            .collect()
    }

    fn call<'a>(
        &'a self,
        ctx: &'a ToolCtx<'a>,
        name: &'a str,
        arguments: &'a Value,
    ) -> BoxFuture<'a, Result<Value, AppError>> {
        Box::pin(async move {
            let binding = ctx.binding.ok_or_else(|| {
                AppError::new(
                    TURN_REQUIRED,
                    format!("`{name}` is available only inside a Clovy chat turn."),
                )
            })?;
            if !binding.attended {
                let denial = ApprovalDenial::UnattendedRequiresApproval;
                return Err(AppError::new(denial.code(), denial.message()));
            }
            let broker = self.broker(ctx)?;
            execute_attended(&broker, name, arguments.clone()).await
        })
    }
}
