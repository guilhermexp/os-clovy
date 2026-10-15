//! Turn bindings: which Clovy CLI chat turn an MCP connection belongs to.

use crate::agent_runtime::domain::AgentSafetyMode;
use crate::llm::cli::CliKind;

/// The CLI turn a connection is bound to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TurnBinding {
    pub session_id: String,
    pub run_id: String,
    pub cli: CliKind,
    pub safety_mode: AgentSafetyMode,
    /// A person watches the turn and can answer an approval card (`false`
    /// for routines: every approval is denied at once).
    pub attended: bool,
    /// Where the turn started (`chat`, `home`, `note_chat`, `companion`,
    /// `routine`); a free label for logs and the approval broker.
    pub surface: String,
}
