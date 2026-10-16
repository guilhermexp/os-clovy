//! The Clovy MCP server's tools, registered as groups ([`ToolGroup`]): one
//! file per group, so parallel work on different groups never shares a file
//! beyond the one-line registration in [`groups`]. A group whose tools act on
//! a CLI turn (anything that writes, or that needs the turn's session) is
//! offered only to a connection bound to a turn ([`ToolCtx::binding`]);
//! unbound clients (Cursor, `claude mcp add`) see the read group alone, as
//! ADR-0058 describes, and calling a bound-only tool answers `turn_required`.

mod read;

use chrono::{DateTime, Utc};
use futures_util::future::BoxFuture;
use serde_json::Value;
use sqlx_sqlite::SqlitePool;
use tauri::AppHandle;

use crate::activity::settings::ActivitySettings;
use crate::activity::store::ActivityStore;
use crate::approval_broker::ApprovalBroker;
use crate::domain::types::AppError;

pub use super::turns::TurnBinding;
use super::turns::TURN_REQUIRED;
pub use read::{
    is_activity_tool, ReadTools, ACTIVITY_TOOLS, GET_ACTIVE_SESSION, GET_ACTIVITY_STATS,
    GET_DAY_PANELS, GET_DAY_SCHEDULE, GET_DAY_SUMMARY, GET_NOTE, GET_SESSION_DETAIL,
    LIST_APP_USAGE, LIST_CODING_AGENT_SESSIONS, LIST_DICTATIONS, LIST_MEMORIES, SEARCH_NOTES,
};

/// What one MCP request may read, resolved by the app per message
/// (`super::app_data`) or built directly by tests.
pub struct McpData {
    /// The main database (`notes.sqlite3`).
    pub notes: SqlitePool,
    /// The current data partition: notes, dictations, and memories queries
    /// are limited to it (activity is not partitioned).
    pub profile: String,
    /// Settings, Memory: `list_memories` refuses while memory is off.
    pub memory_enabled: bool,
    pub activity: ActivityAccess,
    pub now: DateTime<Utc>,
}

pub enum ActivityAccess {
    /// Capture is off: activity tools are not offered and refuse calls.
    Off,
    /// Capture is on; the store was not opened for this message.
    On,
    /// Capture is on and the store is open for an activity tool call.
    Open {
        store: ActivityStore,
        settings: ActivitySettings,
    },
    /// Capture is on but the history cannot be read right now.
    Unavailable(AppError),
}

impl ActivityAccess {
    pub fn offered(&self) -> bool {
        !matches!(self, ActivityAccess::Off)
    }
}

/// What one tool call runs with: the data the message may read, the app (for
/// groups that act through it; `None` where no Tauri runtime exists, as in
/// tests), the CLI turn the connection is bound to (`None` for every client
/// outside a Clovy CLI turn), and the approval broker write tools ask before
/// acting (the app's, or a test broker:
/// `crate::approval_broker::testing::TestBroker`).
#[derive(Clone, Copy)]
pub struct ToolCtx<'a> {
    pub data: &'a McpData,
    pub app: Option<&'a AppHandle>,
    pub binding: Option<&'a TurnBinding>,
    pub broker: Option<&'a ApprovalBroker>,
}

impl<'a> ToolCtx<'a> {
    /// A context with neither app, turn, nor broker (what an unbound client
    /// gets apart from the app handle).
    pub fn unbound(data: &'a McpData) -> Self {
        Self {
            data,
            app: None,
            binding: None,
            broker: None,
        }
    }

    /// The approval broker; `approval_unavailable` when the context has none
    /// (the tool must not act without asking).
    pub fn broker(&self) -> Result<&'a ApprovalBroker, AppError> {
        self.broker.ok_or_else(|| {
            AppError::new(
                "approval_unavailable",
                "Clovy could not ask for approval: the approval broker is unavailable.",
            )
        })
    }
}

/// A set of tools served together. Registered once in [`groups`].
pub trait ToolGroup: Send + Sync {
    /// Whether the group is offered only to connections bound to a turn.
    /// Unbound connections never see its tools, and calling one answers
    /// `turn_required` before the group runs. Defaults to `true`: only the
    /// read group is open to every client.
    fn needs_turn(&self) -> bool {
        true
    }

    /// Whether `name` is one of the group's tools, whatever the context
    /// (decides `Unknown tool` against `turn_required`).
    fn owns(&self, name: &str) -> bool;

    /// The group's `tools/list` entries for this context. Called only when
    /// the context may see the group ([`ToolGroup::needs_turn`]); a group
    /// may still narrow by `ctx.binding` (for example by CLI).
    fn descriptors(&self, ctx: &ToolCtx<'_>) -> Vec<Value>;

    /// Runs one of the group's tools (`owns(name)` holds). Errors become
    /// `isError` tool results.
    fn call<'a>(
        &'a self,
        ctx: &'a ToolCtx<'a>,
        name: &'a str,
        arguments: &'a Value,
    ) -> BoxFuture<'a, Result<Value, AppError>>;
}

/// Every group, in catalog order. Registering a group is one line here.
pub fn groups() -> Vec<&'static dyn ToolGroup> {
    #[allow(unused_mut)]
    let mut groups: Vec<&'static dyn ToolGroup> = vec![&ReadTools];
    #[cfg(all(test, unix))]
    groups.push(&super::turn_tests::TurnProbe);
    groups
}

fn visible(group: &dyn ToolGroup, ctx: &ToolCtx<'_>) -> bool {
    ctx.binding.is_some() || !group.needs_turn()
}

/// The `tools/list` catalog for this context.
pub fn descriptors(ctx: &ToolCtx<'_>) -> Vec<Value> {
    groups()
        .into_iter()
        .filter(|group| visible(*group, ctx))
        .flat_map(|group| group.descriptors(ctx))
        .collect()
}

/// Whether some group owns `name` (bound-only tools included).
pub fn is_known_tool(name: &str) -> bool {
    groups().into_iter().any(|group| group.owns(name))
}

/// Runs a tool. A bound-only tool on an unbound connection answers
/// `turn_required` without running.
pub async fn call(ctx: &ToolCtx<'_>, name: &str, arguments: &Value) -> Result<Value, AppError> {
    let Some(group) = groups().into_iter().find(|group| group.owns(name)) else {
        return Err(AppError::new(
            "mcp_tool_unknown",
            format!("Unknown tool `{name}`."),
        ));
    };
    if !visible(group, ctx) {
        return Err(AppError::new(
            TURN_REQUIRED,
            format!("`{name}` is available only inside a Clovy chat turn."),
        ));
    }
    group.call(ctx, name, arguments).await
}
