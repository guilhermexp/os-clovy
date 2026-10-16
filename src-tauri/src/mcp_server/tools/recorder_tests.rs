//! The recorder group (parity slice 2.4): `start_recording` and
//! `stop_recording` ask the approval broker, then go through the real
//! `AgentRecorderBroker`, answered here by a fake frontend the way the main
//! window answers `clovy://agent-recorder-request`; `recording_status`
//! reads without a card.

use std::sync::{Arc, Mutex};

use futures_util::future::BoxFuture;
use serde_json::{json, Value};

use super::recorder::{self, RecorderPort, RecorderTools};
use super::{ActivityAccess, McpData, ToolCtx, ToolGroup};
use crate::agent_recorder::{
    AgentRecorderAction, AgentRecorderBroker, ResolveAgentRecorderRequest,
};
use crate::approval_broker::testing::{Scripted, TestBroker};
use crate::domain::types::{AppError, RecordingSourceMode};
use crate::llm::cli::CliKind;

/// The main window, faked: it receives each recorder request the broker
/// emits and answers it through `resolve_agent_recorder_request`'s path.
#[derive(Clone, Default)]
struct FakeFrontend {
    broker: Arc<AgentRecorderBroker>,
    received: Arc<Mutex<Vec<Value>>>,
}

impl RecorderPort for FakeFrontend {
    fn request(
        &self,
        action: AgentRecorderAction,
        source_mode: Option<RecordingSourceMode>,
    ) -> BoxFuture<'_, Result<Value, AppError>> {
        Box::pin(async move {
            self.broker
                .request_with(action, source_mode, |payload| {
                    self.received.lock().unwrap().push(payload.clone());
                    let broker = Arc::clone(&self.broker);
                    tokio::spawn(async move {
                        let request_id = payload["requestId"].as_str().unwrap().to_string();
                        broker
                            .resolve(ResolveAgentRecorderRequest {
                                request_id,
                                ok: true,
                                note_id: Some("note-1".into()),
                                note_title: Some("Meeting".into()),
                                error_code: None,
                                error_message: None,
                            })
                            .unwrap();
                    });
                    Ok(())
                })
                .await
        })
    }
}

fn data(broker: &TestBroker) -> McpData {
    crate::mcp_server::tests::data(broker.pool().clone(), ActivityAccess::Off)
}

#[tokio::test]
async fn recorder_start_and_stop_run_only_after_approval() {
    let broker = TestBroker::new([Scripted::Approve, Scripted::Approve, Scripted::Deny]).await;
    let binding = broker.turn(CliKind::Claude, true).await;
    let data = data(&broker);
    let ctx = ToolCtx {
        data: &data,
        app: None,
        binding: Some(&binding),
        broker: Some(broker.broker()),
    };
    let frontend = FakeFrontend::default();

    let started = recorder::call(
        &ctx,
        Some(&frontend),
        "start_recording",
        &json!({ "sourceMode": "microphonePlusSystem" }),
    )
    .await
    .unwrap();
    assert_eq!(
        started,
        json!({ "success": true, "noteId": "note-1", "noteTitle": "Meeting" })
    );
    let stopped = recorder::call(&ctx, Some(&frontend), "stop_recording", &json!({}))
        .await
        .unwrap();
    assert_eq!(stopped["success"], true);
    let received = frontend.received.lock().unwrap().clone();
    assert_eq!(received.len(), 2);
    assert_eq!(received[0]["action"], "start");
    assert_eq!(received[0]["sourceMode"], "microphonePlusSystem");
    assert_eq!(received[1]["action"], "stop");
    assert!(received[1].get("sourceMode").is_none());

    // Denied: the frontend never hears of it.
    let denied = recorder::call(&ctx, Some(&frontend), "start_recording", &json!({}))
        .await
        .unwrap_err();
    assert_eq!(denied.code, "user_denied");
    assert_eq!(frontend.received.lock().unwrap().len(), 2);

    // The cards carry the app's text, never the model's.
    let cards = broker.requests();
    assert_eq!(cards.len(), 3);
    assert_eq!(cards[0]["toolName"], "start_recording");
    assert_eq!(cards[0]["title"], "Start a recording");
    assert!(cards[0]["description"]
        .as_str()
        .unwrap()
        .contains("the microphone and system audio"));
    assert_eq!(cards[1]["title"], "Stop the recording");
    assert_eq!(cards[2]["engine"], "cli");
}

#[tokio::test]
async fn recorder_unattended_turn_is_refused_without_a_card() {
    let broker = TestBroker::new([Scripted::Approve]).await;
    let routine = broker.turn(CliKind::Codex, false).await;
    let data = data(&broker);
    let ctx = ToolCtx {
        data: &data,
        app: None,
        binding: Some(&routine),
        broker: Some(broker.broker()),
    };
    let frontend = FakeFrontend::default();
    for name in ["start_recording", "stop_recording"] {
        let refused = recorder::call(&ctx, Some(&frontend), name, &json!({}))
            .await
            .unwrap_err();
        assert_eq!(refused.code, "unattended_requires_approval", "{name}");
    }
    assert!(broker.requests().is_empty());
    assert!(frontend.received.lock().unwrap().is_empty());
}

#[tokio::test]
async fn recorder_without_a_broker_or_turn_never_records() {
    let broker = TestBroker::new([Scripted::Approve]).await;
    let binding = broker.turn(CliKind::Claude, true).await;
    let data = data(&broker);
    let frontend = FakeFrontend::default();

    // No broker: approval_unavailable, nothing emitted.
    let no_broker = ToolCtx {
        binding: Some(&binding),
        ..ToolCtx::unbound(&data)
    };
    let error = recorder::call(&no_broker, Some(&frontend), "start_recording", &json!({}))
        .await
        .unwrap_err();
    assert_eq!(error.code, "approval_unavailable");

    // No app (the group's own path): refused before any card.
    let with_broker = ToolCtx {
        broker: Some(broker.broker()),
        ..no_broker
    };
    let error = RecorderTools
        .call(&with_broker, "stop_recording", &json!({}))
        .await
        .unwrap_err();
    assert_eq!(error.code, "agent_recorder_unavailable");
    assert!(broker.requests().is_empty());

    // Unbound: not listed, and turn_required through the registry.
    let unbound = ToolCtx::unbound(&data);
    let listed = super::descriptors(&unbound).await;
    for name in ["start_recording", "stop_recording", "recording_status"] {
        assert!(listed.iter().all(|tool| tool["name"] != name), "{name}");
        let error = super::call(&unbound, name, &json!({})).await.unwrap_err();
        assert_eq!(error.code, "turn_required", "{name}");
    }
    assert!(frontend.received.lock().unwrap().is_empty());
}

#[tokio::test]
async fn recorder_status_reads_without_a_card_and_hints_are_correct() {
    let broker = TestBroker::new([]).await;
    let binding = broker.turn(CliKind::Codex, false).await;
    let data = data(&broker);
    let ctx = ToolCtx {
        data: &data,
        app: None,
        binding: Some(&binding),
        broker: Some(broker.broker()),
    };
    let status = RecorderTools
        .call(&ctx, "recording_status", &json!({}))
        .await
        .unwrap();
    assert_eq!(
        status,
        serde_json::to_value(crate::audio::capture::current_status()).unwrap()
    );
    assert!(broker.requests().is_empty());

    let listed = RecorderTools.descriptors(&ctx);
    let hint = |name: &str| {
        listed.iter().find(|tool| tool["name"] == name).unwrap()["annotations"]["readOnlyHint"]
            .clone()
    };
    assert_eq!(hint("start_recording"), false);
    assert_eq!(hint("stop_recording"), false);
    assert_eq!(hint("recording_status"), true);
}
