use super::*;
use crate::approval_broker::external::{one_pending, ExternalApprovalBroker};
use crate::mcp_server::channel::ExternalConnection;
use futures_util::future::BoxFuture;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncBufReadExt, BufReader};

struct HostedFixture {
    lifecycle: AsyncMutex<()>,
    revision: AtomicU64,
    endpoint: String,
    calls: Arc<Mutex<Vec<Value>>>,
    requests: Arc<AtomicU64>,
    server: tokio::task::JoinHandle<()>,
}

impl HostedFixture {
    async fn start() -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let endpoint = format!("http://{}/mcp", listener.local_addr().unwrap());
        let calls = Arc::new(Mutex::new(Vec::new()));
        let requests = Arc::new(AtomicU64::new(0));
        let logged_calls = Arc::clone(&calls);
        let request_count = Arc::clone(&requests);
        let server = tokio::spawn(async move {
            loop {
                let (socket, _) = listener.accept().await.unwrap();
                let mut stream = BufReader::new(socket);
                let mut length = None;
                let mut authenticated = false;
                loop {
                    let mut line = String::new();
                    stream.read_line(&mut line).await.unwrap();
                    if line == "\r\n" || line.is_empty() {
                        break;
                    }
                    if let Some((key, value)) = line.split_once(':') {
                        if key.eq_ignore_ascii_case("content-length") {
                            length = Some(value.trim().parse::<usize>().unwrap());
                        }
                        if key.eq_ignore_ascii_case("authorization") {
                            authenticated = value.trim() == "Bearer synthetic-access";
                        }
                    }
                }
                assert!(authenticated);
                let mut bytes = vec![0; length.unwrap()];
                stream.read_exact(&mut bytes).await.unwrap();
                let message: Value = serde_json::from_slice(&bytes).unwrap();
                request_count.fetch_add(1, Ordering::Relaxed);
                let result = match message["method"].as_str().unwrap() {
                    "initialize" => {
                        json!({"protocolVersion":MCP_PROTOCOL_VERSION,"capabilities":{"tools":{}},"serverInfo":{"name":"fixture","version":"1"}})
                    }
                    "notifications/initialized" => json!({}),
                    "tools/list" => json!({"tools":[
                        {"name":"notion-fetch","inputSchema":{"type":"object","properties":{"id":{"type":"string"}},"required":["id"]}},
                        {"name":"notion-update-page","inputSchema":{"type":"object"}},
                        {"name":"notion-delete-page","inputSchema":{"type":"object"}},
                    ]}),
                    "tools/call" => {
                        logged_calls.lock().unwrap().push(message["params"].clone());
                        json!({"content":[{"type":"text","text":"synthetic hosted result"}]})
                    }
                    method => panic!("unexpected method {method}"),
                };
                let body = json!({"jsonrpc":"2.0","id":message["id"],"result":result}).to_string();
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream
                    .get_mut()
                    .write_all(response.as_bytes())
                    .await
                    .unwrap();
            }
        });
        Self {
            lifecycle: AsyncMutex::new(()),
            revision: AtomicU64::new(0),
            endpoint,
            calls,
            requests,
            server,
        }
    }
}

impl ExternalNotionPort for HostedFixture {
    fn lifecycle(&self) -> &AsyncMutex<()> {
        &self.lifecycle
    }

    fn revision(&self) -> u64 {
        self.revision.load(Ordering::Acquire)
    }

    fn load_cached(&self) -> BoxFuture<'_, Result<StoredNotionConnection, AppError>> {
        Box::pin(async {
            Ok(StoredNotionConnection {
                connection_id: "synthetic-connection".into(),
                access_token: "synthetic-access".into(),
                refresh_token: None,
                client_id: "synthetic-client".into(),
                client_secret: None,
                expires_at_unix: Some(now_unix() + 3600),
                endpoint: MCP_ENDPOINT.into(),
                registration_redirect_uri: None,
            })
        })
    }

    fn endpoint(&self) -> &str {
        &self.endpoint
    }
}

impl Drop for HostedFixture {
    fn drop(&mut self) {
        self.server.abort();
    }
}

#[tokio::test]
async fn external_notion_requires_approval_and_freezes_the_connected_account() {
    let host = HostedFixture::start().await;
    let broker =
        ExternalApprovalBroker::new(|| Some("default".into()), || {}, Duration::from_secs(30));
    let (peer, _close, _cancel) = ExternalConnection::test_connection();
    let access = broker.scope(&peer, "default", None);
    let inventory = discover_external(&host, &access).await.unwrap().descriptors;
    assert_eq!(
        inventory
            .iter()
            .map(|tool| tool["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["notion-fetch", "notion-update-page"],
    );
    assert!(inventory
        .iter()
        .all(|tool| tool["requiresApproval"] == true));

    let args = json!({"page_id":"fixture-page","command":"update_properties","properties":{"title":"Fixture"}});
    let call = || call_external_with(&host, &access, "default", "notion-update-page", &args);
    let decision = |allow| {
        let (broker, host) = (&broker, &host);
        async move {
            let pending = one_pending(broker).await;
            assert!(host.calls.lock().unwrap().is_empty());
            assert!(pending["preview"]
                .as_str()
                .unwrap()
                .contains("fixture-page"));
            assert!(pending["preview"].as_str().unwrap().contains("Notion"));
            broker
                .decide(pending["id"].as_str().unwrap(), allow)
                .unwrap();
        }
    };
    let (denied, ()) = tokio::join!(call(), decision(false));
    assert_eq!(denied.unwrap_err().code, "user_denied");
    assert!(host.calls.lock().unwrap().is_empty());

    let change = async {
        let pending = one_pending(&broker).await;
        let _lifecycle = host.lifecycle.lock().await;
        host.revision.fetch_add(1, Ordering::Release);
        broker
            .decide(pending["id"].as_str().unwrap(), true)
            .unwrap();
    };
    let (changed, ()) = tokio::join!(call(), change);
    assert_eq!(changed.unwrap_err().code, "notion_connection_changed");
    assert!(host.calls.lock().unwrap().is_empty());

    let (applied, ()) = tokio::join!(call(), decision(true));
    assert_eq!(applied.unwrap()["toolName"], "notion-update-page");
    assert_eq!(
        *host.calls.lock().unwrap(),
        [json!({"name":"notion-update-page","arguments":args})]
    );

    let total = host.requests.load(Ordering::Relaxed);
    let hidden_args = json!({"id":"self"});
    let refused = call_external_with(&host, &access, "default", "notion-fetch", &hidden_args).await;
    assert_eq!(
        refused.unwrap_err().code,
        "notion_fetch_identity_unsupported"
    );
    assert_eq!(host.requests.load(Ordering::Relaxed), total);
    let foreign = json!({"id":"fixture-page","workspacePath":"/fixture"});
    let refused = call_external_with(&host, &access, "default", "notion-fetch", &foreign).await;
    assert_eq!(refused.unwrap_err().code, "agent_tool_arguments_invalid");

    let read_args = json!({"id":"fixture-page"});
    let read_permission = async {
        let pending = one_pending(&broker).await;
        assert_eq!(pending["tool"], "notion-fetch");
        assert_eq!(host.calls.lock().unwrap().len(), 1);
        broker
            .decide(pending["id"].as_str().unwrap(), true)
            .unwrap();
    };
    let read = call_external_with(&host, &access, "default", "notion-fetch", &read_args);
    let (read, ()) = tokio::join!(read, read_permission);
    assert_eq!(
        read.unwrap()["result"]["content"][0]["text"],
        "synthetic hosted result"
    );
    assert_eq!(
        host.calls.lock().unwrap()[1],
        json!({"name":"notion-fetch","arguments":read_args})
    );
}
