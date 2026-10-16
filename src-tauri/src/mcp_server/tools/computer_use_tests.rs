//! `computer_use` over the approval broker (`TestBroker`) and a fake driver:
//! one card per call answered once on the Mac or on a linked phone, the
//! action run only after it with the call's permit, and the lease the turn
//! binding grants.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::future::BoxFuture;
use serde_json::{json, Value};

use super::computer_use::{ComputerUseHost, ComputerUseTools, COMPUTER_USE_TOOL};
use super::{ActivityAccess, McpData, ToolCtx, ToolGroup};
use crate::approval_broker::testing::{Scripted, TestBroker};
use crate::companion::{CliComputerUseRequest, CliComputerUseRequests};
use crate::computer_use::{
    begin_turn_lease_in, check_eligibility, clear_leases_for_test, end_turn_lease_in,
    has_attended_run, watch_turn_in, ActionOrigin, BoxedBool, ComputerUseState, EligibilityProbe,
};
use crate::domain::types::AppError;
use crate::llm::cli::CliKind;
use crate::mcp_server::protocol;
use crate::mcp_server::turns::TurnBinding;

/// A fake Computer use driver and phone: records what the group asked.
#[derive(Default)]
struct FakeHost {
    phone_linked: bool,
    driver_result: Mutex<Option<Value>>,
    leases: Mutex<Vec<String>>,
    phone: CliComputerUseRequests,
    offered: Mutex<Vec<(String, String)>>,
    retired: Mutex<Vec<(String, bool)>>,
    executed: Mutex<Vec<(String, Value)>>,
}

impl FakeHost {
    fn new(phone_linked: bool) -> Arc<Self> {
        Arc::new(Self {
            phone_linked,
            ..Self::default()
        })
    }

    fn executed(&self) -> Vec<(String, Value)> {
        self.executed.lock().unwrap().clone()
    }
}

impl ComputerUseHost for FakeHost {
    fn begin_turn(&self, binding: &TurnBinding) -> Result<(), AppError> {
        self.leases.lock().unwrap().push(binding.run_id.clone());
        Ok(())
    }

    fn offer_to_companion<'a>(
        &'a self,
        binding: &'a TurnBinding,
        request_id: &'a str,
        call_id: &'a str,
        _arguments: &'a Value,
    ) -> BoxFuture<'a, bool> {
        Box::pin(async move {
            if !self.phone_linked {
                return false;
            }
            self.phone.insert(
                request_id,
                CliComputerUseRequest {
                    run_id: binding.run_id.clone(),
                    stored_session_id: binding.session_id.clone(),
                    tool_call_id: call_id.to_string(),
                },
            );
            self.offered
                .lock()
                .unwrap()
                .push((request_id.to_string(), call_id.to_string()));
            true
        })
    }

    fn retire_companion(&self, _binding: &TurnBinding, request_id: &str, approved: bool) {
        self.retired
            .lock()
            .unwrap()
            .push((request_id.to_string(), approved));
    }

    fn execute<'a>(
        &'a self,
        _binding: &'a TurnBinding,
        call_id: &'a str,
        arguments: Value,
    ) -> BoxFuture<'a, Value> {
        Box::pin(async move {
            self.executed
                .lock()
                .unwrap()
                .push((call_id.to_string(), arguments));
            self.driver_result
                .lock()
                .unwrap()
                .clone()
                .unwrap_or_else(|| {
                    json!({
                        "content": [
                            { "type": "text", "text": "[1] button \"Save\"" },
                            { "type": "image", "data": "iVBORw0KGgo=", "mimeType": "image/png" }
                        ],
                        "isError": false,
                        "structuredContent": { "target_app": "TextEdit" }
                    })
                })
        })
    }
}

fn data(broker: &TestBroker) -> McpData {
    McpData {
        notes: broker.pool().clone(),
        profile: "default".into(),
        memory_enabled: true,
        activity: ActivityAccess::Off,
        now: chrono::Utc::now(),
    }
}

fn ctx<'a>(data: &'a McpData, broker: &'a TestBroker, binding: &'a TurnBinding) -> ToolCtx<'a> {
    ToolCtx {
        data,
        app: None,
        binding: Some(binding),
        broker: Some(broker.broker()),
    }
}

fn capture() -> Value {
    json!({ "action": "capture", "app": "TextEdit" })
}

/// A desktop answer through the resolution command's broker path.
async fn desktop_answer(broker: &TestBroker, card: &Value, choice: &str) -> Result<bool, AppError> {
    crate::approval_broker::commands::resolve(
        broker.broker(),
        broker.pool(),
        card["runId"].as_str().unwrap(),
        card["id"].as_str().unwrap(),
        &json!({ "kind": "approval", "choice": choice }),
    )
    .await
}

#[tokio::test]
async fn computer_use_is_listed_only_for_attended_turns() {
    let broker = TestBroker::new([]).await;
    let data = data(&broker);
    let chat = broker.turn(CliKind::Claude, true).await;
    let routine = broker.turn(CliKind::Codex, false).await;
    let listed = |ctx: &ToolCtx<'_>| {
        super::descriptors(ctx)
            .into_iter()
            .filter(|tool| tool["name"] == COMPUTER_USE_TOOL)
            .collect::<Vec<_>>()
    };
    assert!(listed(&ToolCtx::unbound(&data)).is_empty());
    assert!(listed(&ctx(&data, &broker, &routine)).is_empty());
    let tool = listed(&ctx(&data, &broker, &chat));
    assert_eq!(tool.len(), 1);
    assert_eq!(tool[0]["annotations"]["readOnlyHint"], false);
    assert_eq!(tool[0]["inputSchema"]["required"], json!(["action"]));
    // Same name and arguments as the native agent's descriptor.
    let native = super::computer_use::runtime_descriptor();
    assert_eq!(native["name"], COMPUTER_USE_TOOL);
    assert_eq!(native["parameters"], tool[0]["inputSchema"]);
}

#[tokio::test]
async fn unbound_and_unattended_calls_get_no_card_lease_or_action() {
    let broker = TestBroker::new([Scripted::Approve]).await;
    let data = data(&broker);
    let unbound = super::call(&ToolCtx::unbound(&data), COMPUTER_USE_TOOL, &capture())
        .await
        .unwrap_err();
    assert_eq!(unbound.code, "turn_required");

    let host = FakeHost::new(true);
    let group = ComputerUseTools::with_host(host.clone());
    let routine = broker.turn(CliKind::Codex, false).await;
    let refused = group
        .call(
            &ctx(&data, &broker, &routine),
            COMPUTER_USE_TOOL,
            &capture(),
        )
        .await
        .unwrap_err();
    assert_eq!(refused.code, "unattended_requires_approval");
    assert!(broker.requests().is_empty());
    assert!(host.leases.lock().unwrap().is_empty());
    assert!(host.offered.lock().unwrap().is_empty());
    assert!(host.executed().is_empty());
}

#[tokio::test]
async fn desktop_approval_runs_one_action_with_its_permit_and_a_second_answer_is_refused() {
    let broker = TestBroker::new([Scripted::Approve]).await;
    let data = data(&broker);
    let chat = broker.turn(CliKind::Claude, true).await;
    let host = FakeHost::new(false);
    let group = ComputerUseTools::with_host(host.clone());

    let result = group
        .call(&ctx(&data, &broker, &chat), COMPUTER_USE_TOOL, &capture())
        .await
        .unwrap();
    let cards = broker.requests();
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0]["toolName"], COMPUTER_USE_TOOL);
    assert_eq!(cards[0]["engine"], "cli");
    let executed = host.executed();
    assert_eq!(executed.len(), 1);
    // The action runs with the card's call id as its one permit.
    assert_eq!(cards[0]["toolCallId"], executed[0].0.as_str());
    assert_eq!(executed[0].1, capture());
    assert_eq!(
        *host.leases.lock().unwrap(),
        std::slice::from_ref(&chat.run_id)
    );
    // The MCP result carries the capture as an image block, beside the
    // driver's text and structured content.
    let mcp = crate::mcp_server::protocol::tool_result(&result);
    assert_eq!(mcp["isError"], false);
    assert_eq!(
        mcp["content"],
        json!([
            { "type": "text", "text": "[1] button \"Save\"" },
            { "type": "image", "data": "iVBORw0KGgo=", "mimeType": "image/png" },
            { "type": "text", "text": "{\"target_app\":\"TextEdit\"}" }
        ])
    );

    let second = desktop_answer(&broker, &cards[0], "once")
        .await
        .unwrap_err();
    assert_eq!(second.code, "agent_interruption_expired");
    assert_eq!(host.executed().len(), 1);
}

#[tokio::test]
async fn denied_or_invalid_calls_never_reach_the_driver() {
    let broker = TestBroker::new([Scripted::Deny]).await;
    let data = data(&broker);
    let chat = broker.turn(CliKind::Codex, true).await;
    let host = FakeHost::new(true);
    let group = ComputerUseTools::with_host(host.clone());
    let ctx = ctx(&data, &broker, &chat);

    let denied = group
        .call(&ctx, COMPUTER_USE_TOOL, &capture())
        .await
        .unwrap_err();
    assert_eq!(denied.code, "user_denied");
    assert!(host.executed().is_empty());
    // The phone's card is retired with the Mac's outcome.
    let offered = host.offered.lock().unwrap().clone();
    assert_eq!(
        *host.retired.lock().unwrap(),
        [(offered[0].0.clone(), false)]
    );

    let invalid = group
        .call(&ctx, COMPUTER_USE_TOOL, &json!({ "action": "rm -rf" }))
        .await
        .unwrap_err();
    assert_eq!(invalid.code, "computer_use_action_invalid");
    assert_eq!(broker.requests().len(), 1);
    assert!(host.executed().is_empty());
}

#[tokio::test]
async fn phone_approval_settles_the_card_once_and_later_answers_are_refused() {
    let broker = TestBroker::new([Scripted::Leave]).await;
    let data = data(&broker);
    let chat = broker.turn(CliKind::Claude, true).await;
    let host = FakeHost::new(true);
    let group = ComputerUseTools::with_host(host.clone());
    let ctx = ctx(&data, &broker, &chat);

    let phone = async {
        // The phone answers once the card is up.
        let card = loop {
            if let Some(card) = broker.requests().into_iter().next() {
                break card;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        };
        let (request_id, _) = host.offered.lock().unwrap()[0].clone();
        let request = host.phone.get(&request_id, &chat.session_id).unwrap();
        crate::companion::resolve_cli_computer_use_card(
            broker.broker(),
            broker.pool(),
            &request,
            true,
        )
        .await
        .unwrap();
        (card, request)
    };
    let arguments = capture();
    let (result, (card, request)) =
        tokio::join!(group.call(&ctx, COMPUTER_USE_TOOL, &arguments), phone);
    result.unwrap();

    let executed = host.executed();
    assert_eq!(executed.len(), 1);
    assert_eq!(executed[0].0, request.tool_call_id);
    assert_eq!(card["toolCallId"], request.tool_call_id.as_str());

    // A second phone answer and a late Mac answer both find the card settled.
    let again = crate::companion::resolve_cli_computer_use_card(
        broker.broker(),
        broker.pool(),
        &request,
        true,
    )
    .await
    .unwrap_err();
    assert_eq!(again.code, "agent_interruption_expired");
    let late = desktop_answer(&broker, &card, "deny").await.unwrap_err();
    assert_eq!(late.code, "agent_interruption_expired");
    assert_eq!(host.executed().len(), 1);
}

#[tokio::test]
async fn phone_denial_never_reaches_the_driver() {
    let broker = TestBroker::new([Scripted::Leave]).await;
    let data = data(&broker);
    let chat = broker.turn(CliKind::Claude, true).await;
    let host = FakeHost::new(true);
    let group = ComputerUseTools::with_host(host.clone());
    let ctx = ctx(&data, &broker, &chat);
    let phone = async {
        while broker.requests().is_empty() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        let (request_id, _) = host.offered.lock().unwrap()[0].clone();
        let request = host.phone.get(&request_id, &chat.session_id).unwrap();
        crate::companion::resolve_cli_computer_use_card(
            broker.broker(),
            broker.pool(),
            &request,
            false,
        )
        .await
        .unwrap();
    };
    let arguments = capture();
    let (result, ()) = tokio::join!(group.call(&ctx, COMPUTER_USE_TOOL, &arguments), phone);
    assert_eq!(result.unwrap_err().code, "user_denied");
    assert!(host.executed().is_empty());
}

#[tokio::test]
async fn driver_failure_is_a_tool_error() {
    let broker = TestBroker::new([Scripted::Approve]).await;
    let data = data(&broker);
    let chat = broker.turn(CliKind::Claude, true).await;
    let host = FakeHost::new(false);
    *host.driver_result.lock().unwrap() = Some(json!({
        "content": [{ "type": "text", "text": "{\"error\":\"Computer use access to this app was denied, cancelled, or timed out.\"}" }],
        "isError": true,
    }));
    let group = ComputerUseTools::with_host(host.clone());
    let error = group
        .call(&ctx(&data, &broker, &chat), COMPUTER_USE_TOOL, &capture())
        .await
        .unwrap_err();
    assert_eq!(error.code, "computer_use_failed");
    assert!(error.message.contains("denied"), "{}", error.message);
}

fn binding(run_id: &str, attended: bool) -> TurnBinding {
    TurnBinding {
        session_id: "session-1".into(),
        run_id: run_id.into(),
        cli: CliKind::Claude,
        safety_mode: crate::agent_runtime::AgentSafetyMode::Sandboxed,
        attended,
        surface: if attended { "chat" } else { "routine" }.into(),
    }
}

#[test]
fn the_turn_binding_grants_the_lease_and_stop_stays_sticky_for_the_run() {
    let state = ComputerUseState::default();
    assert!(!has_attended_run(&state));
    assert_eq!(
        begin_turn_lease_in(&state, &binding("run-a", false))
            .unwrap_err()
            .code,
        "computer_use_attended_run_required"
    );
    assert!(!has_attended_run(&state));

    assert!(begin_turn_lease_in(&state, &binding("run-a", true)).unwrap());
    assert!(has_attended_run(&state));
    // Later calls of the same run keep the lease without reopening it.
    assert!(!begin_turn_lease_in(&state, &binding("run-a", true)).unwrap());

    // Stop clears the lease; the same run cannot take it back.
    clear_leases_for_test(&state);
    assert_eq!(
        begin_turn_lease_in(&state, &binding("run-a", true))
            .unwrap_err()
            .code,
        "computer_use_stopped"
    );
    assert!(!has_attended_run(&state));

    // The run's end forgets it; a new turn gets a fresh lease.
    assert_eq!(end_turn_lease_in(&state, "run-a").unwrap(), None);
    assert!(begin_turn_lease_in(&state, &binding("run-b", true)).unwrap());
    assert!(end_turn_lease_in(&state, "run-b").unwrap().is_some());
    assert!(!has_attended_run(&state));
}

#[test]
fn a_run_that_ends_before_its_lease_is_granted_never_gets_one() {
    let state = ComputerUseState::default();
    let turn = binding("run-c", true);
    // The end listener is registered first ...
    assert!(watch_turn_in(&state, &turn).unwrap());
    // ... and the run ends before the lease is granted.
    assert_eq!(end_turn_lease_in(&state, "run-c").unwrap(), None);
    assert_eq!(
        begin_turn_lease_in(&state, &turn).unwrap_err().code,
        "computer_use_turn_ended"
    );
    assert!(!has_attended_run(&state));
    assert_eq!(
        watch_turn_in(&state, &turn).unwrap_err().code,
        "computer_use_turn_ended"
    );

    // The normal order: watched, granted, then ended.
    let turn = binding("run-d", true);
    assert!(watch_turn_in(&state, &turn).unwrap());
    assert!(begin_turn_lease_in(&state, &turn).unwrap());
    assert!(!watch_turn_in(&state, &turn).unwrap());
    assert!(has_attended_run(&state));
    assert!(end_turn_lease_in(&state, "run-d").unwrap().is_some());
    assert!(!has_attended_run(&state));
}

/// Platform, rollout, grant and driver available; no Clovy plan and a
/// native model without vision. Records which facts were asked.
#[derive(Default)]
struct Probe {
    asked: Mutex<Vec<&'static str>>,
}

impl Probe {
    fn ask(&self, fact: &'static str, answer: bool) -> BoxedBool<'_> {
        self.asked.lock().unwrap().push(fact);
        Box::pin(async move { answer })
    }
}

impl EligibilityProbe for Probe {
    fn macos(&self) -> bool {
        true
    }
    fn rollout_enabled(&self) -> BoxedBool<'_> {
        self.ask("rollout", true)
    }
    fn grant_enabled(&self) -> BoxedBool<'_> {
        self.ask("grant", true)
    }
    fn plan_eligible(&self) -> BoxedBool<'_> {
        self.ask("plan", false)
    }
    fn native_model_supports_vision(&self) -> BoxedBool<'_> {
        self.ask("vision", false)
    }
    fn driver(&self) -> Result<(), AppError> {
        self.asked.lock().unwrap().push("driver");
        Ok(())
    }
}

#[tokio::test]
async fn a_cli_turn_needs_neither_the_clovy_plan_nor_a_native_vision_model() {
    let native = Probe::default();
    assert_eq!(
        check_eligibility(ActionOrigin::NativeAgent, &native)
            .await
            .unwrap_err()
            .code,
        "computer_use_plan_required"
    );

    let cli = Probe::default();
    check_eligibility(ActionOrigin::CliTurn, &cli)
        .await
        .unwrap();
    assert_eq!(*cli.asked.lock().unwrap(), ["rollout", "grant", "driver"]);
}

#[test]
fn content_values_become_mcp_blocks_and_plain_values_stay_one_text_block() {
    let plain = json!({ "saved": true });
    assert_eq!(
        protocol::tool_result(&plain),
        json!({ "content": [{ "type": "text", "text": "{\"saved\":true}" }], "isError": false })
    );
    let blocks = protocol::content_value(vec![
        protocol::text_block("caption"),
        protocol::image_block("AAAA", "image/png"),
    ]);
    assert_eq!(
        protocol::tool_result(&blocks)["content"][1],
        json!({ "type": "image", "data": "AAAA", "mimeType": "image/png" })
    );
    // Anything but well-formed text and image blocks stays JSON text.
    for odd in [
        json!({ protocol::CONTENT_KEY: [{ "type": "audio", "data": "AAAA" }] }),
        json!({ protocol::CONTENT_KEY: [{ "type": "image", "data": "AAAA", "mimeType": "text/html" }] }),
        json!({ protocol::CONTENT_KEY: [], }),
        json!({ protocol::CONTENT_KEY: [protocol::text_block("x")], "other": 1 }),
    ] {
        let result = protocol::tool_result(&odd);
        assert_eq!(result["content"].as_array().unwrap().len(), 1);
        assert_eq!(result["content"][0]["type"], "text");
    }
}
