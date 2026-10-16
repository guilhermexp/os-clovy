//! The browser group over a real `BrowserBroker` with a fake attended
//! transport: tools only for attended turns, reads straight through, and a
//! consequential click parked in the browser approval tray (never a second
//! card in the approval broker).

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};

use super::browser::{BrowserTools, BROWSER_TOOLS};
use super::{ActivityAccess, McpData, ToolCtx, ToolGroup};
use crate::approval_broker::testing::TestBroker;
use crate::browser::policy::InteractiveElement;
use crate::browser_broker::{
    BrowserBroker, BrowserTransport, BrowserTransportKind, TransportArtifact, TransportFuture,
    TransportResponse,
};
use crate::domain::types::AppError;
use crate::llm::cli::CliKind;
use crate::mcp_server::turns::TurnBinding;

/// The first bytes of a PNG file (the fake screenshot).
const PNG: &[u8] = b"\x89PNG\r\n\x1a\n";

/// An attended browser whose page has one button, labelled by the test.
struct FakeTransport {
    element: InteractiveElement,
    calls: Mutex<Vec<String>>,
    actions: AtomicUsize,
}

impl FakeTransport {
    fn with_button(label: &str) -> Arc<Self> {
        Arc::new(Self {
            element: InteractiveElement {
                tag: "button".into(),
                input_type: "button".into(),
                label: label.into(),
                ..InteractiveElement::default()
            },
            calls: Mutex::default(),
            actions: AtomicUsize::new(0),
        })
    }

    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }

    fn actions(&self) -> usize {
        self.actions.load(Ordering::SeqCst)
    }
}

impl BrowserTransport for FakeTransport {
    fn execute<'a>(&'a self, tool: &'a str, arguments: Value) -> TransportFuture<'a> {
        Box::pin(async move {
            self.calls.lock().unwrap().push(tool.to_string());
            let data = match tool {
                "open_tab" => json!({ "tabId": 7, "url": "about:blank" }),
                "list_tabs" => json!({ "tabs": [] }),
                "snapshot" => json!({ "epoch": 1, "snapshot": "button e0:n1 \"Send\"" }),
                "inspect_reference" => json!({
                    "element": self.element.clone(),
                    "url": "https://mail.example.com:443/compose",
                }),
                "click" | "fill" | "press" => {
                    let expected: InteractiveElement =
                        serde_json::from_value(arguments["expected"].clone()).map_err(|_| {
                            AppError::new("invalid_arguments", "expected is required")
                        })?;
                    if expected != self.element {
                        return Err(AppError::new("browser_stale_reference", "changed"));
                    }
                    self.actions.fetch_add(1, Ordering::SeqCst);
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
                    ))
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
    async fn new(button: &str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let flag = dir.path().join("browser-access");
        std::fs::write(&flag, b"1").unwrap();
        let transport = FakeTransport::with_button(button);
        let browser = Arc::new(BrowserBroker::default());
        browser.set_access_flag_path(flag);
        browser.configure_transport(
            BrowserTransportKind::Attended,
            transport.clone(),
            dir.path().join("images"),
            dir.path().join("artifacts"),
        );
        // No scripted decisions: any approval-broker card would stay pending
        // and show up in `requests()`.
        let approvals = TestBroker::new([]).await;
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

#[tokio::test]
async fn browser_tools_are_listed_only_for_attended_turns_with_read_only_hints() {
    let rig = Rig::new("Send").await;
    let chat = rig.approvals.turn(CliKind::Claude, true).await;
    let routine = rig.approvals.turn(CliKind::Codex, false).await;

    let unbound = names(&super::descriptors(&ToolCtx::unbound(&rig.data)).await);
    assert!(BROWSER_TOOLS
        .iter()
        .all(|tool| !unbound.contains(&tool.to_string())));
    assert!(rig.group().descriptors(&rig.ctx(Some(&routine))).is_empty());

    let listed = super::descriptors(&rig.ctx(Some(&chat))).await;
    let browser: Vec<&Value> = listed
        .iter()
        .filter(|tool| BROWSER_TOOLS.contains(&tool["name"].as_str().unwrap_or_default()))
        .collect();
    assert_eq!(
        browser
            .iter()
            .map(|tool| tool["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        BROWSER_TOOLS
    );
    for tool in browser {
        let name = tool["name"].as_str().unwrap();
        let read_only = matches!(name, "snapshot" | "screenshot" | "list_tabs");
        assert_eq!(tool["annotations"]["readOnlyHint"], read_only, "{name}");
        assert_eq!(tool["inputSchema"]["type"], "object", "{name}");
    }
    // Same names and arguments as the native agent's catalog.
    let native = super::browser::runtime_descriptors();
    assert_eq!(names(&native), BROWSER_TOOLS);
    let click = native.iter().find(|tool| tool["name"] == "click").unwrap();
    assert_eq!(
        click["parameters"]["required"],
        json!(["session_id", "ref"])
    );
}

#[tokio::test]
async fn unbound_and_unattended_calls_never_reach_the_browser() {
    let rig = Rig::new("Send").await;
    let unbound = super::call(
        &ToolCtx::unbound(&rig.data),
        "navigate",
        &json!({ "session_id": "s", "url": "https://example.com" }),
    )
    .await
    .unwrap_err();
    assert_eq!(unbound.code, "turn_required");

    let routine = rig.approvals.turn(CliKind::Codex, false).await;
    let refused = rig
        .group()
        .call(&rig.ctx(Some(&routine)), "start_session", &json!({}))
        .await
        .unwrap_err();
    assert_eq!(refused.code, "unattended_requires_approval");
    assert!(rig.transport.calls().is_empty());
    assert_eq!(rig.browser.pending_approvals().await.unwrap().len(), 0);
    assert!(rig.approvals.requests().is_empty());
}

async fn open_session(rig: &Rig, ctx: &ToolCtx<'_>) -> String {
    let group = rig.group();
    let started = group.call(ctx, "start_session", &json!({})).await.unwrap();
    let session_id = started["sessionId"].as_str().unwrap().to_string();
    group
        .call(ctx, "open_tab", &json!({ "session_id": session_id }))
        .await
        .unwrap();
    session_id
}

#[tokio::test]
async fn consequential_click_parks_in_the_browser_tray_and_runs_once_after_approval() {
    let rig = Rig::new("Send").await;
    let chat = rig.approvals.turn(CliKind::Claude, true).await;
    let ctx = rig.ctx(Some(&chat));
    let group = rig.group();
    let session_id = open_session(&rig, &ctx).await;

    // Reads run straight through, without any approval.
    let snapshot = group
        .call(
            &ctx,
            "snapshot",
            &json!({ "session_id": session_id, "tab_id": 7 }),
        )
        .await
        .unwrap();
    assert!(snapshot.to_string().contains("e0:n1"), "{snapshot}");

    let click = json!({ "session_id": session_id, "tab_id": 7, "ref": "e0:n1" });
    let parked = group.call(&ctx, "click", &click).await.unwrap();
    assert_eq!(parked["parked"], true);
    assert_eq!(rig.transport.actions(), 0);
    let pending = rig.browser.pending_approvals().await.unwrap();
    assert_eq!(pending.len(), 1);
    // The browser tray is the only approval: no approval-broker card.
    assert!(rig.approvals.requests().is_empty());

    rig.browser
        .respond_to_approval(parked["actionId"].as_str().unwrap(), true, false)
        .await
        .unwrap();
    assert_eq!(rig.transport.actions(), 1);
    let replay = group.call(&ctx, "click", &click).await.unwrap();
    assert_eq!(replay["executed"], true);
    assert_eq!(rig.transport.actions(), 1);
}

#[tokio::test]
async fn declined_consequential_click_never_runs() {
    let rig = Rig::new("Send").await;
    let chat = rig.approvals.turn(CliKind::Codex, true).await;
    let ctx = rig.ctx(Some(&chat));
    let group = rig.group();
    let session_id = open_session(&rig, &ctx).await;
    let click = json!({ "session_id": session_id, "tab_id": 7, "ref": "e0:n1" });
    let parked = group.call(&ctx, "click", &click).await.unwrap();
    rig.browser
        .respond_to_approval(parked["actionId"].as_str().unwrap(), false, false)
        .await
        .unwrap();
    let replay = group.call(&ctx, "click", &click).await.unwrap_err();
    assert_eq!(replay.code, "browser_action_declined");
    assert_eq!(rig.transport.actions(), 0);
    assert!(rig.approvals.requests().is_empty());
}

#[tokio::test]
async fn registered_group_without_an_app_reports_the_browser_unavailable() {
    let rig = Rig::new("Send").await;
    let chat = rig.approvals.turn(CliKind::Claude, true).await;
    let error = super::call(&rig.ctx(Some(&chat)), "start_session", &json!({}))
        .await
        .unwrap_err();
    assert_eq!(error.code, "browser_unavailable");
}

#[tokio::test]
async fn screenshot_reaches_the_cli_as_an_image_block() {
    use base64::Engine as _;
    let rig = Rig::new("Send").await;
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
    // Other tools keep the single JSON text block.
    let listed = rig
        .group()
        .call(&ctx, "list_tabs", &json!({ "session_id": session_id }))
        .await
        .unwrap();
    let listed = crate::mcp_server::protocol::tool_result(&listed);
    assert_eq!(listed["content"].as_array().unwrap().len(), 1);
    assert_eq!(listed["content"][0]["type"], "text");
}
