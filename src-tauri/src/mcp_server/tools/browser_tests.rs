//! MCP consumers over the real browser and central approval brokers. The fake
//! transport isolates browser policy; it is not evidence of engine integration.

use std::collections::HashSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};

use super::browser::{BrowserTools, BROWSER_TOOLS};
use super::{ActivityAccess, McpData, ToolCtx, ToolGroup};
use crate::approval_broker::testing::{Scripted, TestBroker};
use crate::browser::policy::InteractiveElement;
use crate::browser_broker::{
    BrowserBroker, BrowserTransport, BrowserTransportKind, TransportArtifact, TransportFuture,
    TransportResponse,
};
use crate::db::repositories::Repositories;
use crate::domain::types::AppError;
use crate::llm::cli::CliKind;
use crate::mcp_server::turns::TurnBinding;

const PNG: &[u8] = b"\x89PNG\r\n\x1a\n";

struct FakeTransport {
    element: InteractiveElement,
    calls: Mutex<Vec<String>>,
    sessions: Mutex<HashSet<String>>,
    suspended: Mutex<HashSet<String>>,
    actions: AtomicUsize,
    after_action: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
}

impl FakeTransport {
    fn new(element: InteractiveElement) -> Arc<Self> {
        Arc::new(Self {
            element,
            calls: Mutex::default(),
            sessions: Mutex::default(),
            suspended: Mutex::default(),
            actions: AtomicUsize::new(0),
            after_action: Mutex::new(None),
        })
    }

    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }

    fn actions(&self) -> usize {
        self.actions.load(Ordering::SeqCst)
    }

    fn inspection(&self) -> Value {
        json!({
            "element": self.element,
            "url": "https://mail.example.com/compose",
            "origin": "https://mail.example.com",
            "documentId": "document-1",
            "backendNodeId": 42,
        })
    }
}

impl BrowserTransport for FakeTransport {
    fn reserve_session(&self, id: &str) -> Result<(), AppError> {
        self.sessions.lock().unwrap().insert(id.into());
        Ok(())
    }

    fn terminate_session(&self, id: &str) {
        self.sessions.lock().unwrap().remove(id);
        self.suspended.lock().unwrap().remove(id);
    }

    fn session_state(&self, id: &str) -> Result<Value, AppError> {
        if !self.sessions.lock().unwrap().contains(id) {
            return Err(AppError::new("browser_session_not_found", "Session ended"));
        }
        let suspended = self.suspended.lock().unwrap().contains(id);
        Ok(
            json!({ "sessionId": id, "mode": if suspended { "visible" } else { "background" }, "suspended": suspended }),
        )
    }

    fn human_takeover<'a>(&'a self, id: &'a str) -> TransportFuture<'a> {
        Box::pin(async move {
            self.session_state(id)?;
            self.suspended.lock().unwrap().insert(id.into());
            Ok(TransportResponse::data(json!({ "tabId": 7 })))
        })
    }

    fn resume_control<'a>(&'a self, id: &'a str) -> TransportFuture<'a> {
        Box::pin(async move {
            self.session_state(id)?;
            self.suspended.lock().unwrap().remove(id);
            Ok(TransportResponse::data(json!({})))
        })
    }

    fn execute<'a>(&'a self, tool: &'a str, arguments: Value) -> TransportFuture<'a> {
        Box::pin(async move {
            self.calls.lock().unwrap().push(tool.to_string());
            let id = arguments["session_id"].as_str().unwrap_or_default();
            self.session_state(id)?;
            if self.suspended.lock().unwrap().contains(id) {
                return Err(AppError::new(
                    "browser_human_control",
                    "Human control is active",
                ));
            }
            let data = match tool {
                "start_session" => json!({ "tabId": 7 }),
                "open_tab" => json!({ "tabId": 8, "url": "https://example.com/" }),
                "list_tabs" => json!({ "tabs": [{ "tabId": 7 }] }),
                "snapshot" => json!({ "epoch": 1, "snapshot": "button e0:n1 \"Send\"" }),
                "inspect_reference" => self.inspection(),
                "click" | "fill" | "press" => {
                    if arguments["expected"] != self.inspection() {
                        return Err(AppError::new("browser_stale_reference", "Target changed"));
                    }
                    self.actions.fetch_add(1, Ordering::SeqCst);
                    if let Some(callback) = self.after_action.lock().unwrap().clone() {
                        callback();
                    }
                    json!({ "epoch": 2, "snapshot": "sent" })
                }
                "screenshot" => {
                    return Ok(TransportResponse::artifact(
                        json!({ "epoch": 1 }),
                        TransportArtifact {
                            kind: "screenshot".into(),
                            mime_type: "image/png".into(),
                            bytes: PNG.to_vec(),
                        },
                    ));
                }
                _ => json!({}),
            };
            Ok(TransportResponse::data(data))
        })
    }
}

struct Rig {
    _dir: tempfile::TempDir,
    browser: Arc<BrowserBroker>,
    transport: Arc<FakeTransport>,
    approvals: TestBroker,
    data: McpData,
}

impl Rig {
    async fn new(button: &str, script: impl IntoIterator<Item = Scripted>) -> Self {
        Self::with_element(
            InteractiveElement {
                tag: "button".into(),
                input_type: "button".into(),
                label: button.into(),
                ..InteractiveElement::default()
            },
            script,
        )
        .await
    }

    async fn with_element(
        element: InteractiveElement,
        script: impl IntoIterator<Item = Scripted>,
    ) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let flag = dir.path().join("browser-access");
        std::fs::write(&flag, b"1").unwrap();
        let transport = FakeTransport::new(element);
        let browser = Arc::new(BrowserBroker::default());
        browser.set_access_flag_path(flag);
        browser.configure_transport(
            BrowserTransportKind::Attended,
            transport.clone(),
            dir.path().join("images"),
            dir.path().join("artifacts"),
        );
        let approvals = TestBroker::new(script).await;
        browser.configure_outcome_repository(Repositories::new(approvals.pool().clone()));
        let data = McpData {
            notes: approvals.pool().clone(),
            profile: "default".into(),
            memory_enabled: true,
            activity: ActivityAccess::Off,
            now: chrono::Utc::now(),
        };
        Self {
            _dir: dir,
            browser,
            transport,
            approvals,
            data,
        }
    }

    fn ctx<'a>(&'a self, binding: Option<&'a TurnBinding>) -> ToolCtx<'a> {
        ToolCtx {
            data: &self.data,
            app: None,
            binding,
            broker: Some(self.approvals.broker()),
        }
    }

    fn group(&self) -> BrowserTools {
        BrowserTools::with_broker(Arc::clone(&self.browser))
    }
}

fn names(descriptors: &[Value]) -> Vec<String> {
    descriptors
        .iter()
        .filter_map(|tool| tool["name"].as_str().map(str::to_string))
        .collect()
}

async fn open_session(rig: &Rig, ctx: &ToolCtx<'_>) -> String {
    let started = rig
        .group()
        .call(ctx, "start_session", &json!({}))
        .await
        .unwrap();
    assert_eq!(started["tabId"], 7);
    started["sessionId"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn browser_tools_are_bound_and_publish_thirteen_targeted_contracts() {
    let rig = Rig::new("Send", []).await;
    let chat = rig.approvals.turn(CliKind::Claude, true).await;
    let unbound = names(&super::descriptors(&ToolCtx::unbound(&rig.data)).await);
    assert!(BROWSER_TOOLS
        .iter()
        .all(|tool| !unbound.contains(&tool.to_string())));
    let listed = rig.group().descriptors(&rig.ctx(Some(&chat)));
    assert_eq!(names(&listed), BROWSER_TOOLS);
    assert_eq!(listed.len(), 13);
    assert!(!rig.group().owns("accept_shared_tab"));
    for tool in listed {
        let name = tool["name"].as_str().unwrap();
        assert_eq!(
            tool["annotations"]["readOnlyHint"],
            matches!(name, "snapshot" | "screenshot" | "list_tabs")
        );
        if matches!(
            name,
            "navigate" | "snapshot" | "screenshot" | "click" | "fill" | "press" | "back"
        ) {
            assert_eq!(
                tool["inputSchema"]["properties"]["tab_id"]["type"],
                "integer"
            );
            assert!(tool["inputSchema"]["required"]
                .as_array()
                .unwrap()
                .contains(&json!("tab_id")));
        }
        if name == "fill" {
            assert!(tool["inputSchema"]["required"]
                .as_array()
                .unwrap()
                .contains(&json!("text")));
            assert!(tool["inputSchema"]["properties"].get("value").is_none());
        }
        if name == "press" {
            assert!(tool["inputSchema"]["required"]
                .as_array()
                .unwrap()
                .contains(&json!("ref")));
        }
    }
}

#[tokio::test]
async fn unbound_calls_never_reach_the_browser() {
    let rig = Rig::new("Send", []).await;
    let error = rig
        .group()
        .call(&rig.ctx(None), "start_session", &json!({}))
        .await
        .unwrap_err();
    assert_eq!(error.code, "turn_required");
    assert!(rig.transport.calls().is_empty());
    assert!(rig.approvals.requests().is_empty());
}

#[tokio::test]
async fn unattended_turns_need_an_actual_host_owned_routine_run() {
    let rig = Rig::new("Next", []).await;
    let routine = rig.approvals.turn(CliKind::Codex, false).await;
    let error = rig
        .group()
        .call(&rig.ctx(Some(&routine)), "start_session", &json!({}))
        .await
        .unwrap_err();
    assert_eq!(error.code, "routine_browser_access_disabled");
    assert!(rig.transport.calls().is_empty());
    assert!(rig.approvals.requests().is_empty());
}

#[tokio::test]
async fn routine_dispatch_uses_its_own_bound_grant_and_rechecks_persisted_revocation() {
    use crate::{browser_broker::RoutineBrowserGrant, db::repositories::RoutineBrowserGrantRecord};
    let rig = Rig::new("Next", []).await;
    let turn = rig.approvals.turn(CliKind::Codex, false).await;
    let now = "2026-11-01T00:00:00Z";
    sqlx::query::query("INSERT INTO routines (id,name,prompt,schedule,created_at,updated_at,metadata_json,tool_catalog_version) VALUES ('browser-routine','Fixture','Inspect','every 1d',?,?,'{}',1)").bind(now).bind(now).execute(rig.approvals.pool()).await.unwrap();
    sqlx::query::query("INSERT INTO routine_runs (id,routine_id,agent_session_id,agent_run_id,claim_token,trigger_kind,status,model,safety_mode,created_at,updated_at) VALUES ('browser-routine-run','browser-routine',?,?,'browser-claim','schedule','running','codex','sandboxed',?,?)").bind(&turn.session_id).bind(&turn.run_id).bind(now).bind(now).execute(rig.approvals.pool()).await.unwrap();
    let repos = Repositories::new(rig.approvals.pool().clone());
    let grant = RoutineBrowserGrantRecord {
        job_id: "browser-routine".into(),
        server_name: "fixture-browser".into(),
        token: "synthetic-token".into(),
        enabled: true,
    };
    repos.set_routine_browser_grant(&grant).await.unwrap();
    rig.browser.set_routine_grant(RoutineBrowserGrant {
        job_id: grant.job_id.clone(),
        enabled: true,
    });
    rig.browser.configure_transport(
        BrowserTransportKind::Managed,
        rig.transport.clone(),
        rig._dir.path().join("images"),
        rig._dir.path().join("artifacts"),
    );
    // The attended opt-in is deliberately off. It cannot confer or revoke
    // authority on this separately bound anonymous routine profile.
    std::fs::remove_file(rig._dir.path().join("browser-access")).unwrap();
    let ctx = rig.ctx(Some(&turn));
    let session = open_session(&rig, &ctx).await;
    rig.group()
        .call(&ctx, "snapshot", &json!({"session_id":session,"tab_id":7}))
        .await
        .unwrap();
    let before = rig.transport.calls();
    repos
        .set_routine_browser_grant(&RoutineBrowserGrantRecord {
            enabled: false,
            ..grant
        })
        .await
        .unwrap();
    assert_eq!(
        rig.group()
            .call(&ctx, "snapshot", &json!({"session_id":session,"tab_id":7}))
            .await
            .unwrap_err()
            .code,
        "routine_browser_access_disabled"
    );
    assert_eq!(rig.transport.calls(), before);
    assert!(!rig.transport.sessions.lock().unwrap().contains(&session));
}

#[tokio::test]
async fn hostile_fields_and_new_run_in_same_conversation_never_reach_transport() {
    let rig = Rig::new("Next", []).await;
    let owner = rig.approvals.turn(CliKind::Claude, true).await;
    let session = open_session(&rig, &rig.ctx(Some(&owner))).await;
    let before = rig.transport.calls();
    let mut other = owner.clone();
    other.run_id = "different-host-run".into();
    assert_eq!(
        rig.group()
            .call(
                &rig.ctx(Some(&other)),
                "snapshot",
                &json!({"session_id":session,"tab_id":7})
            )
            .await
            .unwrap_err()
            .code,
        "browser_session_not_found"
    );
    for (name, args) in [
        (
            "start_session",
            json!({"profile":"Default","chromePath":"/bin/sh"}),
        ),
        (
            "fill",
            json!({"session_id":session,"tab_id":7,"ref":"e0:n1","value":"obsolete"}),
        ),
        (
            "press",
            json!({"session_id":session,"tab_id":7,"key":"Enter"}),
        ),
    ] {
        assert_eq!(
            rig.group()
                .call(&rig.ctx(Some(&owner)), name, &args)
                .await
                .unwrap_err()
                .code,
            "invalid_arguments"
        );
    }
    assert_eq!(rig.transport.calls(), before);
}

#[tokio::test]
async fn revocation_after_delivery_is_unknown_and_old_handles_cannot_replay() {
    let rig = Rig::new("Next", []).await;
    let owner = rig.approvals.turn(CliKind::Claude, true).await;
    let ctx = rig.ctx(Some(&owner));
    let session = open_session(&rig, &ctx).await;
    let broker = Arc::downgrade(&rig.browser);
    let run = owner.run_id.clone();
    *rig.transport.after_action.lock().unwrap() = Some(Arc::new(move || {
        if let Some(broker) = broker.upgrade() {
            broker.terminate_run(&run);
        }
    }));
    let args = json!({"session_id":session,"tab_id":7,"ref":"e0:n1"});
    assert_eq!(
        rig.group()
            .call(&ctx, "click", &args)
            .await
            .unwrap_err()
            .code,
        "browser_outcome_unknown"
    );
    assert_eq!(rig.transport.actions(), 1);
    assert_eq!(
        rig.group()
            .call(&ctx, "click", &args)
            .await
            .unwrap_err()
            .code,
        "browser_session_not_found"
    );
    assert_eq!(rig.transport.actions(), 1);
}

#[tokio::test]
async fn consequential_click_uses_one_central_card_and_dispatches_once() {
    let rig = Rig::new("Send", [Scripted::Approve]).await;
    let chat = rig.approvals.turn(CliKind::Claude, true).await;
    let ctx = rig.ctx(Some(&chat));
    let group = rig.group();
    let session_id = open_session(&rig, &ctx).await;
    let snapshot = group
        .call(
            &ctx,
            "snapshot",
            &json!({ "session_id": session_id, "tab_id": 7 }),
        )
        .await
        .unwrap();
    assert!(snapshot.to_string().contains("e0:n1"));
    assert!(rig.approvals.requests().is_empty());
    group
        .call(
            &ctx,
            "click",
            &json!({ "session_id": session_id, "tab_id": 7, "ref": "e0:n1" }),
        )
        .await
        .unwrap();
    assert_eq!(rig.transport.actions(), 1);
    assert_eq!(rig.approvals.requests().len(), 1);
    let outcomes = Repositories::new(rig.approvals.pool().clone())
        .browser_action_outcomes_for_session(&session_id)
        .await
        .unwrap();
    let clicks: Vec<_> = outcomes
        .iter()
        .filter(|outcome| outcome.operation == "click")
        .collect();
    assert_eq!(clicks.len(), 1);
    assert_eq!(clicks[0].result_kind, "executed");
    assert!(!clicks[0].outcome_verified);
    assert_eq!(
        rig.transport
            .calls()
            .iter()
            .filter(|call| call.as_str() == "click")
            .count(),
        1
    );
}

#[tokio::test]
async fn declined_central_approval_never_dispatches_the_action() {
    let rig = Rig::new("Send", [Scripted::Deny]).await;
    let chat = rig.approvals.turn(CliKind::Codex, true).await;
    let ctx = rig.ctx(Some(&chat));
    let session_id = open_session(&rig, &ctx).await;
    let error = rig
        .group()
        .call(
            &ctx,
            "click",
            &json!({ "session_id": session_id, "tab_id": 7, "ref": "e0:n1" }),
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, "user_denied");
    assert_eq!(rig.transport.actions(), 0);
    assert_eq!(rig.approvals.requests().len(), 1);
}

#[tokio::test]
async fn another_conversation_cannot_reuse_a_session_or_reach_its_transport() {
    let rig = Rig::new("Next", []).await;
    let owner = rig.approvals.turn(CliKind::Claude, true).await;
    let other = rig.approvals.turn(CliKind::Codex, true).await;
    let session_id = open_session(&rig, &rig.ctx(Some(&owner))).await;
    let before = rig.transport.calls();
    let error = rig
        .group()
        .call(
            &rig.ctx(Some(&other)),
            "snapshot",
            &json!({ "session_id": session_id, "tab_id": 7 }),
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, "browser_session_not_found");
    assert_eq!(rig.transport.calls(), before);
    assert!(rig.approvals.requests().is_empty());
}

#[tokio::test]
async fn closed_tabs_and_invalid_tab_ids_do_not_fall_back_to_another_target() {
    let rig = Rig::new("Next", []).await;
    let owner = rig.approvals.turn(CliKind::Claude, true).await;
    let ctx = rig.ctx(Some(&owner));
    let session_id = open_session(&rig, &ctx).await;
    let group = rig.group();
    group
        .call(
            &ctx,
            "close_tab",
            &json!({ "session_id": session_id, "tab_id": 7 }),
        )
        .await
        .unwrap();
    let before = rig.transport.calls();
    for tab in [json!(7), json!("7"), json!(0), Value::Null] {
        assert!(group
            .call(
                &ctx,
                "snapshot",
                &json!({ "session_id": session_id, "tab_id": tab })
            )
            .await
            .is_err());
    }
    assert_eq!(rig.transport.calls(), before);
}

#[tokio::test]
async fn sensitive_input_is_refused_before_central_approval_or_input_dispatch() {
    let rig = Rig::with_element(
        InteractiveElement {
            tag: "input".into(),
            input_type: "password".into(),
            label: "Password".into(),
            ..InteractiveElement::default()
        },
        [],
    )
    .await;
    let owner = rig.approvals.turn(CliKind::Claude, true).await;
    let ctx = rig.ctx(Some(&owner));
    let session_id = open_session(&rig, &ctx).await;
    let error = rig.group().call(&ctx, "fill", &json!({ "session_id": session_id, "tab_id": 7, "ref": "e0:n1", "text": "synthetic-only" })).await.unwrap_err();
    assert_eq!(error.code, "browser_human_takeover_required");
    assert_eq!(rig.transport.actions(), 0);
    assert!(rig.approvals.requests().is_empty());
}

#[tokio::test]
async fn revoked_access_and_ended_runs_cannot_reuse_browser_handles() {
    let rig = Rig::new("Next", []).await;
    let owner = rig.approvals.turn(CliKind::Claude, true).await;
    let ctx = rig.ctx(Some(&owner));
    let session_id = open_session(&rig, &ctx).await;
    rig.browser.terminate_run(&owner.run_id);
    let before = rig.transport.calls();
    assert!(rig
        .group()
        .call(&ctx, "list_tabs", &json!({ "session_id": session_id }))
        .await
        .is_err());
    assert_eq!(rig.transport.calls(), before);
    assert!(rig.transport.sessions.lock().unwrap().is_empty());
    let next = rig.approvals.turn(CliKind::Claude, true).await;
    rig.browser.set_enabled(false).await.unwrap();
    let error = rig
        .group()
        .call(&rig.ctx(Some(&next)), "start_session", &json!({}))
        .await
        .unwrap_err();
    assert_eq!(error.code, "browser_access_disabled");
}

#[tokio::test]
async fn registered_group_without_an_app_reports_the_browser_unavailable() {
    let rig = Rig::new("Send", []).await;
    let chat = rig.approvals.turn(CliKind::Claude, true).await;
    let error = super::call(&rig.ctx(Some(&chat)), "start_session", &json!({}))
        .await
        .unwrap_err();
    assert_eq!(error.code, "browser_unavailable");
}

#[tokio::test]
async fn screenshot_reaches_the_cli_as_an_image_block() {
    use base64::Engine as _;
    let rig = Rig::new("Send", []).await;
    let chat = rig.approvals.turn(CliKind::Claude, true).await;
    let ctx = rig.ctx(Some(&chat));
    let session_id = open_session(&rig, &ctx).await;
    let result = rig
        .group()
        .call(
            &ctx,
            "screenshot",
            &json!({ "session_id": session_id, "tab_id": 7 }),
        )
        .await
        .unwrap();
    let mcp = crate::mcp_server::protocol::tool_result(&result);
    assert_eq!(mcp["content"][0]["type"], "image");
    assert_eq!(mcp["content"][0]["mimeType"], "image/png");
    assert_eq!(
        mcp["content"][0]["data"],
        base64::engine::general_purpose::STANDARD.encode(PNG)
    );
    let text: Value = serde_json::from_str(mcp["content"][1]["text"].as_str().unwrap()).unwrap();
    assert!(text["fileReference"]
        .as_str()
        .unwrap()
        .starts_with("browser-screenshot-"));
    let listed = rig
        .group()
        .call(&ctx, "list_tabs", &json!({ "session_id": session_id }))
        .await
        .unwrap();
    let listed = crate::mcp_server::protocol::tool_result(&listed);
    assert_eq!(listed["content"].as_array().unwrap().len(), 1);
    assert_eq!(listed["content"][0]["type"], "text");
}
