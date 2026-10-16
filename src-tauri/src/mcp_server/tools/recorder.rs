//! The recorder group: `start_recording` and `stop_recording` (each asks the
//! approval broker first, then runs through the existing
//! `AgentRecorderBroker`, which has the frontend start or stop the visible
//! recording, ADR-0013) and `recording_status` (read-only). Bound to a turn;
//! names fixed in the `clovy-mcp-server` delta of the OpenSpec change
//! `cli-substitui-agente-nativo`.

use futures_util::future::BoxFuture;
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

use super::{ToolCtx, ToolGroup};
use crate::agent_recorder::{source_mode_argument, AgentRecorderAction, AgentRecorderBroker};
use crate::approval_broker::{ApprovalKind, ApprovalRequest, ApprovalTurn};
use crate::domain::types::{AppError, RecordingSourceMode};
use crate::mcp_server::turns::TURN_REQUIRED;

pub const START_RECORDING: &str = "start_recording";
pub const STOP_RECORDING: &str = "stop_recording";
pub const RECORDING_STATUS: &str = "recording_status";
const RECORDER_TOOLS: [&str; 3] = [START_RECORDING, STOP_RECORDING, RECORDING_STATUS];

/// The tools of this file as a [`ToolGroup`].
pub struct RecorderTools;

impl ToolGroup for RecorderTools {
    fn owns(&self, name: &str) -> bool {
        RECORDER_TOOLS.contains(&name)
    }

    fn descriptors(&self, _ctx: &ToolCtx<'_>) -> Vec<Value> {
        descriptors()
    }

    fn call<'a>(
        &'a self,
        ctx: &'a ToolCtx<'a>,
        name: &'a str,
        arguments: &'a Value,
    ) -> BoxFuture<'a, Result<Value, AppError>> {
        Box::pin(async move {
            let recorder = ctx.app.map(|app| AppRecorder { app });
            call(
                ctx,
                recorder.as_ref().map(|r| r as &dyn RecorderPort),
                name,
                arguments,
            )
            .await
        })
    }
}

fn descriptors() -> Vec<Value> {
    let no_arguments = json!({ "type": "object", "properties": {}, "required": [], "additionalProperties": false });
    vec![
        json!({
            "name": START_RECORDING,
            "description": "Start a visible Clovy recording only when the user explicitly asks to begin recording now. The user approves it in Clovy first.",
            "inputSchema": { "type": "object", "properties": { "sourceMode": { "type": "string", "enum": ["microphoneOnly", "microphonePlusSystem"] } }, "required": [], "additionalProperties": false },
            "annotations": { "readOnlyHint": false, "destructiveHint": false, "idempotentHint": false, "openWorldHint": false },
        }),
        json!({
            "name": STOP_RECORDING,
            "description": "Stop the recording currently visible in Clovy. The user approves it in Clovy first.",
            "inputSchema": no_arguments,
            "annotations": { "readOnlyHint": false, "destructiveHint": false, "idempotentHint": false, "openWorldHint": false },
        }),
        json!({
            "name": RECORDING_STATUS,
            "description": "Check whether Clovy is currently recording and return the active recording metadata.",
            "inputSchema": no_arguments,
            "annotations": { "readOnlyHint": true, "openWorldHint": false, "idempotentHint": true },
        }),
    ]
}

/// What starting or stopping a recording goes through, so tests can answer
/// as the frontend does.
pub(crate) trait RecorderPort: Send + Sync {
    fn request(
        &self,
        action: AgentRecorderAction,
        source_mode: Option<RecordingSourceMode>,
    ) -> BoxFuture<'_, Result<Value, AppError>>;
}

/// The app's recorder broker, which asks the main window.
struct AppRecorder<'a> {
    app: &'a AppHandle,
}

impl RecorderPort for AppRecorder<'_> {
    fn request(
        &self,
        action: AgentRecorderAction,
        source_mode: Option<RecordingSourceMode>,
    ) -> BoxFuture<'_, Result<Value, AppError>> {
        Box::pin(async move {
            let broker = self.app.state::<AgentRecorderBroker>();
            broker.request(self.app, action, source_mode).await
        })
    }
}

/// `recording_status`, as the native tool answers it.
pub(crate) fn recording_status() -> Result<Value, AppError> {
    serde_json::to_value(crate::audio::capture::current_status())
        .map_err(|error| AppError::new("agent_tool_response_invalid", error.to_string()))
}

/// One recorder call. `recorder` is `None` without the app (the tool refuses
/// before asking anyone).
pub(crate) async fn call(
    ctx: &ToolCtx<'_>,
    recorder: Option<&dyn RecorderPort>,
    name: &str,
    arguments: &Value,
) -> Result<Value, AppError> {
    let binding = ctx.binding.ok_or_else(|| {
        AppError::new(
            TURN_REQUIRED,
            "The recorder tools are available only inside a Clovy chat turn.",
        )
    })?;
    let (action, source_mode, title, description) = match name {
        RECORDING_STATUS => return recording_status(),
        START_RECORDING => {
            let source_mode = source_mode_argument(arguments);
            let sources = match source_mode {
                RecordingSourceMode::MicrophonePlusSystem => "the microphone and system audio",
                _ => "the microphone",
            };
            (
                AgentRecorderAction::Start,
                Some(source_mode),
                "Start a recording",
                format!("Clovy starts a visible recording of {sources} and opens its note."),
            )
        }
        STOP_RECORDING => (
            AgentRecorderAction::Stop,
            None,
            "Stop the recording",
            "Clovy stops the recording that is running now.".to_string(),
        ),
        other => {
            return Err(AppError::new(
                "mcp_tool_unknown",
                format!("Unknown tool `{other}`."),
            ))
        }
    };
    let recorder = recorder.ok_or_else(|| {
        AppError::new(
            "agent_recorder_unavailable",
            "The recorder request broker is unavailable.",
        )
    })?;
    let request = ApprovalRequest {
        tool: name.to_string(),
        call_id: None,
        kind: ApprovalKind::Approval {
            title: title.to_string(),
            description,
            arguments: arguments.clone(),
            preview: None,
        },
    };
    ctx.broker()?
        .request(&ApprovalTurn::from(binding), request)
        .await
        .map_err(|denial| AppError::new(denial.code(), denial.message()))?;
    recorder.request(action, source_mode).await
}
