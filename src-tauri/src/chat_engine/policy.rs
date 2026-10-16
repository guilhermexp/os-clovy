//! Permission policy of a CLI turn (contract C3 of the change
//! `cli-substitui-agente-nativo`, `design.md` §3): the flags, configuration
//! overrides, and environment a turn gets from `(cli, safety_mode, attended)`.
//! [`super::cli::turn_invocation`] places them; nothing else in the engine
//! adds permission flags.
//!
//! | Turn | Claude Code | Codex |
//! |---|---|---|
//! | attended, `sandboxed` | `--permission-mode default`; with Clovy's server `--allowedTools mcp__clovy__*` | `sandbox_mode="workspace-write"`, no network, `approval_policy="never"` |
//! | attended, `unrestricted` | `--permission-mode acceptEdits` (edits in the workspace; shell still asks) | as `sandboxed` with network |
//! | unattended (routines) | `--permission-mode default`, `--tools`/`--allowedTools` of the read-only set, `--strict-mcp-config` | `sandbox_mode="read-only"`, `approval_policy="never"` |
//!
//! Every Codex turn gets `web_search="live"` (spike S5) and, with Clovy's
//! server, `default_tools_approval_mode="approve"` for that server only (S2:
//! the approval broker inside each write tool is the authority) and
//! `tool_timeout_sec` above the broker's wait (S3). Claude gets
//! `MCP_TOOL_TIMEOUT` (S3). The prompt always goes on stdin: claude's
//! `--allowedTools`/`--disallowedTools` take every value that follows (S1).
//! Attended turns never get `--strict-mcp-config`, so the MCP servers the
//! user configured in the CLI still load (D7).
//!
//! Never, in any combination: the flags in [`FORBIDDEN`].

use crate::agent_runtime::AgentSafetyMode;
use crate::approval_broker::APPROVAL_TIMEOUT;
use crate::llm::cli::CliKind;
use crate::mcp_server::SERVER_KEY;
use std::path::PathBuf;
use std::time::Duration;

/// How long a CLI waits for one call to a Clovy MCP tool (claude
/// `MCP_TOOL_TIMEOUT`, codex `tool_timeout_sec`). Above the broker's wait, so
/// the CLI receives the broker's answer, never its own timeout (measured in
/// spike S3 with 15 minutes on both CLIs).
pub const CLI_TOOL_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const _: () = assert!(CLI_TOOL_TIMEOUT.as_secs() > APPROVAL_TIMEOUT.as_secs());

/// Whether claude's own permission requests go to Clovy's `approve_action`
/// tool (`--permission-prompt-tool`, spike S1) and claude's built-in
/// question tool is replaced by `request_clarification`. Both tools arrive
/// with parity slice 2.11, which turns this on; until then claude refuses
/// what needs approval and the refusal shows as `needsApproval`.
pub const CLAUDE_INTERACTION_TOOLS: bool = false;

/// The Clovy tool claude sends its permission requests to.
pub const CLAUDE_PROMPT_TOOL: &str = "mcp__clovy__approve_action";
/// Every Clovy MCP tool, for `--allowedTools`: their authority is the broker
/// inside each write tool, so claude does not ask a second time.
pub const CLAUDE_CLOVY_TOOLS: &str = "mcp__clovy__*";
/// claude's built-in question tool, which a non-interactive turn cannot
/// answer.
pub const CLAUDE_QUESTION_TOOL: &str = "AskUserQuestion";

/// The built-in claude tools an unattended turn has (`--tools`); every other
/// built-in tool (Bash, Edit, Write, ...) does not exist in that turn.
pub const UNATTENDED_CLAUDE_TOOLS: &str = "Read,Grep,Glob,WebSearch,WebFetch";
/// The tools an unattended claude turn may use without asking
/// (`--allowedTools`): the read-only set above and Clovy's MCP tools.
pub const UNATTENDED_CLAUDE_ALLOWED: &str = "Read,Grep,Glob,WebSearch,WebFetch,mcp__clovy__*";

/// Arguments (and argument fragments) no turn may ever carry: each skips or
/// pre-grants approvals, or lifts the sandbox.
pub const FORBIDDEN: &[&str] = &[
    "--dangerously-skip-permissions",
    "--allow-dangerously-skip-permissions",
    "bypassPermissions",
    "--dangerously-bypass-approvals-and-sandbox",
    "danger-full-access",
    "--approve-for-me",
    "--yolo",
    "--full-auto",
    "--always-approve",
    "--allow-all",
    "--allow-all-tools",
    "--approve-mcps",
];

/// The permission inputs of a turn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TurnPermissions {
    pub safety_mode: AgentSafetyMode,
    /// A person watches the turn (`false` for routines).
    pub attended: bool,
}

/// The policy of one turn, placed by [`super::cli::turn_invocation`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TurnPolicy {
    /// Flags placed after the CLI's own turn flags and before any positional
    /// argument.
    pub args: Vec<String>,
    /// Flags that only make sense with Clovy's MCP server in the turn.
    pub mcp_args: Vec<String>,
    /// Environment set on the CLI process.
    pub env: Vec<(&'static str, String)>,
}

/// The policy of a turn on `kind`. CLIs without a table row (pi, agy,
/// cursor-agent, copilot) keep their default non-interactive mode; only
/// attended turns run on them (`super::check_policy`).
pub fn turn_policy(kind: CliKind, permissions: TurnPermissions) -> TurnPolicy {
    policy_with(kind, permissions, CLAUDE_INTERACTION_TOOLS)
}

pub(crate) fn policy_with(
    kind: CliKind,
    permissions: TurnPermissions,
    claude_interaction_tools: bool,
) -> TurnPolicy {
    match kind {
        CliKind::Claude => claude(permissions, claude_interaction_tools),
        CliKind::Codex => codex(permissions),
        CliKind::Pi | CliKind::Agy | CliKind::CursorAgent | CliKind::Copilot => {
            TurnPolicy::default()
        }
    }
}

fn claude(permissions: TurnPermissions, interaction_tools: bool) -> TurnPolicy {
    let mut policy = TurnPolicy {
        env: vec![("MCP_TOOL_TIMEOUT", CLI_TOOL_TIMEOUT.as_millis().to_string())],
        ..TurnPolicy::default()
    };
    if !permissions.attended {
        // Only the read-only built-ins exist, only they and Clovy's MCP tools
        // run without a prompt (nobody can answer one), and the user's other
        // MCP servers are not loaded. Clovy's write tools reach the broker,
        // which refuses them in an unattended turn.
        policy.args = strings(&[
            "--permission-mode",
            "default",
            "--tools",
            UNATTENDED_CLAUDE_TOOLS,
            "--allowedTools",
            UNATTENDED_CLAUDE_ALLOWED,
            "--strict-mcp-config",
        ]);
        return policy;
    }
    // Explicit, so a `defaultMode` in the user's own settings never decides.
    let mode = match permissions.safety_mode {
        AgentSafetyMode::Sandboxed => "default",
        AgentSafetyMode::Unrestricted => "acceptEdits",
    };
    policy.args = strings(&["--permission-mode", mode]);
    policy.mcp_args = strings(&["--allowedTools", CLAUDE_CLOVY_TOOLS]);
    if interaction_tools {
        policy.mcp_args.extend(strings(&[
            "--permission-prompt-tool",
            CLAUDE_PROMPT_TOOL,
            // The prompt tool answers claude, never the model (S1).
            "--disallowedTools",
            CLAUDE_PROMPT_TOOL,
            CLAUDE_QUESTION_TOOL,
        ]));
    }
    policy
}

fn codex(permissions: TurnPermissions) -> TurnPolicy {
    let mut config = Vec::new();
    if permissions.attended {
        config.push("sandbox_mode=\"workspace-write\"".to_string());
        let network = permissions.safety_mode == AgentSafetyMode::Unrestricted;
        config.push(format!("sandbox_workspace_write.network_access={network}"));
    } else {
        // Commands may read but never write or reach the network.
        config.push("sandbox_mode=\"read-only\"".to_string());
    }
    // `exec` never waits for an approval; Codex's own actions stay inside
    // the sandbox.
    config.push("approval_policy=\"never\"".to_string());
    config.push("web_search=\"live\"".to_string());
    let mcp_config = [
        format!("mcp_servers.{SERVER_KEY}.default_tools_approval_mode=\"approve\""),
        format!(
            "mcp_servers.{SERVER_KEY}.tool_timeout_sec={}",
            CLI_TOOL_TIMEOUT.as_secs()
        ),
    ];
    TurnPolicy {
        args: config_args(config),
        mcp_args: config_args(mcp_config),
        env: Vec::new(),
    }
}

/// `--image <path>` per image attachment for CLIs that take images (codex,
/// spike S4). Placed before other flags: `codex exec --image` takes every
/// value that follows. A path with a comma is left out (codex splits the
/// value on commas); it still reaches the CLI through the attachment
/// manifest.
pub fn image_args(kind: CliKind, images: &[PathBuf]) -> Vec<String> {
    if kind != CliKind::Codex {
        return Vec::new();
    }
    images
        .iter()
        .map(|path| path.display().to_string())
        .filter(|path| !path.contains(','))
        .flat_map(|path| ["--image".to_string(), path])
        .collect()
}

fn config_args(values: impl IntoIterator<Item = String>) -> Vec<String> {
    values
        .into_iter()
        .flat_map(|value| ["-c".to_string(), value])
        .collect()
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}

/// The forbidden fragment `arg` carries, if any.
pub fn forbidden_in(arg: &str) -> Option<&'static str> {
    FORBIDDEN
        .iter()
        .copied()
        .find(|forbidden| arg == *forbidden || arg.contains(forbidden))
}

#[cfg(test)]
#[path = "policy_tests.rs"]
mod tests;
