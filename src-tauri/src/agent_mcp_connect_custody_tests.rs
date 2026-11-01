//! Real callback/token/catalog/revocation consumers for operation-owned custody.
use super::*;
use sqlx_sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use std::str::FromStr;
use tokio::net::{TcpListener, TcpStream};

type CustodyRequests = Arc<Mutex<Vec<(String, String)>>>;

#[derive(Clone, Default)]
struct CustodyStore(
    Arc<Mutex<BTreeMap<String, McpSecretBundle>>>,
    Arc<std::sync::atomic::AtomicBool>,
    Arc<Mutex<Option<CustodyRequests>>>,
    Arc<std::sync::atomic::AtomicBool>,
    Arc<tokio::sync::Notify>,
);
impl McpSecretStore for CustodyStore {
    fn put(&self, key: &str, value: &McpSecretBundle) -> Result<(), AgentMcpError> {
        if key != "primary-R0-ref" && self.1.load(Ordering::SeqCst) {
            return Err(AgentMcpError::SecureStorage);
        }
        self.0.lock().unwrap().insert(key.into(), value.clone());
        if value.oauth.get("refresh_token").map(String::as_str) == Some("candidate-R1-refresh") {
            self.4.notify_one();
        }
        Ok(())
    }
    fn get(&self, key: &str) -> Result<Option<McpSecretBundle>, AgentMcpError> {
        Ok(self.0.lock().unwrap().get(key).cloned())
    }
    fn delete(&self, key: &str) -> Result<(), AgentMcpError> {
        if key != "primary-R0-ref" && self.3.load(Ordering::SeqCst) {
            return Err(AgentMcpError::SecureStorage);
        }
        let mut bundles = self.0.lock().unwrap();
        if let Some(token) = bundles
            .get(key)
            .and_then(|bundle| bundle.oauth.get("refresh_token"))
        {
            if let Some(wire) = self.2.lock().unwrap().as_ref() {
                assert!(
                    wire.lock()
                        .unwrap()
                        .iter()
                        .any(|(path, body)| path == "/revoke"
                            && url::form_urlencoded::parse(body.as_bytes())
                                .any(|(key, value)| key == "token" && value == token.as_str())),
                    "grant custody cannot be deleted before its own revocation request"
                );
            }
        }
        bundles.remove(key);
        Ok(())
    }
}
#[derive(Clone, Default)]
struct CustodyWire {
    requests: CustodyRequests,
    bearers: Arc<Mutex<Vec<String>>>,
    token_seen: Arc<tokio::sync::Notify>,
    token_release: Arc<tokio::sync::Notify>,
    catalog_seen: Arc<tokio::sync::Notify>,
    catalog_release: Arc<tokio::sync::Notify>,
    revoke_fails: Arc<std::sync::atomic::AtomicBool>,
    revoke_again_fails: Arc<std::sync::atomic::AtomicBool>,
    fresh_revoke_count: Arc<std::sync::atomic::AtomicUsize>,
    omit_revocation: Arc<std::sync::atomic::AtomicBool>,
    token_failure: Arc<Mutex<Option<(u16, Value)>>>,
}
struct CustodyHttp {
    origin: String,
    wire: CustodyWire,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for CustodyHttp {
    fn drop(&mut self) {
        self.task.abort();
    }
}
async fn custody_read(stream: &mut TcpStream) -> (String, String, String) {
    let mut bytes = Vec::new();
    loop {
        let mut chunk = [0; 4096];
        let n = stream.read(&mut chunk).await.unwrap();
        assert!(n > 0);
        bytes.extend_from_slice(&chunk[..n]);
        if let Some(end) = bytes.windows(4).position(|x| x == b"\r\n\r\n") {
            let headers = String::from_utf8_lossy(&bytes[..end]);
            let len = headers
                .lines()
                .find_map(|line| {
                    let (k, v) = line.split_once(':')?;
                    k.eq_ignore_ascii_case("content-length")
                        .then(|| v.trim().parse::<usize>().unwrap())
                })
                .unwrap_or(0);
            if bytes.len() >= end + 4 + len {
                return (
                    headers
                        .lines()
                        .next()
                        .unwrap()
                        .split_whitespace()
                        .nth(1)
                        .unwrap()
                        .into(),
                    headers.into_owned(),
                    String::from_utf8(bytes[end + 4..end + 4 + len].to_vec()).unwrap(),
                );
            }
        }
    }
}
async fn custody_reply(stream: &mut TcpStream, status: u16, value: Value) {
    let body = value.to_string();
    let bytes = format!("HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
    let _ = stream.write_all(bytes.as_bytes()).await;
}
impl CustodyHttp {
    async fn start(
        hold_token: bool,
        hold_catalog: bool,
        invalid_catalog: bool,
        lose_token: bool,
    ) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let wire = CustodyWire::default();
        let seen = wire.clone();
        let host = origin.clone();
        let task = tokio::spawn(async move {
            // Dropping this run-owned listener aborts all held request tasks.
            let mut handlers = tokio::task::JoinSet::new();
            loop {
                let (mut stream, _) = listener.accept().await.unwrap();
                let seen = seen.clone();
                let host = host.clone();
                handlers.spawn(async move {
                    let (path, headers, body) = custody_read(&mut stream).await;
                    seen.requests
                        .lock()
                        .unwrap()
                        .push((path.clone(), body.clone()));
                    let value = match path.as_str() {
                        "/.well-known/oauth-protected-resource/mcp" => {
                            json!({"resource":format!("{host}/mcp"),"authorization_servers":[host]})
                        }
                        "/.well-known/oauth-authorization-server" => {
                            let mut metadata = json!({"issuer":host,"authorization_endpoint":format!("{host}/authorize"),"token_endpoint":format!("{host}/token"),"registration_endpoint":format!("{host}/register"),"revocation_endpoint":format!("{host}/revoke"),"response_types_supported":["code"],"grant_types_supported":["authorization_code","refresh_token"],"code_challenge_methods_supported":["S256"],"token_endpoint_auth_methods_supported":["none"]});
                            if seen.omit_revocation.load(Ordering::SeqCst) { metadata.as_object_mut().unwrap().remove("revocation_endpoint"); }
                            metadata
                        }
                        "/register" => json!({"client_id":"owned-custody-fixture-client"}),
                        "/token" => {
                            seen.token_seen.notify_one();
                            if hold_token {
                                seen.token_release.notified().await;
                            }
                            if lose_token {
                                return;
                            }
                            let failure = seen.token_failure.lock().unwrap().clone();
                            if let Some((status, body)) = failure {
                                custody_reply(&mut stream, status, body).await;
                                return;
                            }
                            json!({"access_token":"candidate-R1-access","refresh_token":"candidate-R1-refresh","token_type":"Bearer","expires_in":3600})
                        }
                        "/revoke" => {
                            let form: BTreeMap<String, String> =
                                url::form_urlencoded::parse(body.as_bytes())
                                    .into_owned()
                                    .collect();
                            let fresh = form.get("token").map(String::as_str) == Some("candidate-R1-refresh");
                            let fresh_count = if fresh { seen.fresh_revoke_count.fetch_add(1, Ordering::SeqCst) + 1 } else { 0 };
                            if fresh && (seen.revoke_fails.load(Ordering::SeqCst) || (seen.revoke_again_fails.load(Ordering::SeqCst) && fresh_count > 1))
                            {
                                custody_reply(
                                    &mut stream,
                                    503,
                                    json!({"error":"temporarily_unavailable"}),
                                )
                                .await;
                                return;
                            }
                            Value::Null
                        }
                        "/mcp" => {
                            let bearer = headers.lines().find_map(|line| {
                                let (key, value) = line.split_once(':')?;
                                key.eq_ignore_ascii_case("authorization").then(|| value.trim().to_owned())
                            }).unwrap();
                            assert!(bearer == "Bearer candidate-R1-access" || bearer == "Bearer primary-R0-access");
                            seen.bearers.lock().unwrap().push(bearer.clone());
                            let req: Value = serde_json::from_str(&body).unwrap();
                            let method = req["method"].as_str().unwrap();
                            let result = match method {
                                "initialize" => {
                                    json!({"protocolVersion":MCP_PROTOCOL_VERSION,"capabilities":{"tools":{}},"serverInfo":{"name":"custody-fixture","version":"1"}})
                                }
                                "notifications/initialized" => {
                                    custody_reply(&mut stream, 202, Value::Null).await;
                                    return;
                                }
                                "tools/list" => {
                                    seen.catalog_seen.notify_one();
                                    if hold_catalog {
                                        seen.catalog_release.notified().await;
                                    }
                                    if invalid_catalog && bearer == "Bearer candidate-R1-access" {
                                        json!({"tools":"invalid"})
                                    } else {
                                        json!({"tools":[{"name":"read_fixture","inputSchema":{"type":"object"},"annotations":{"readOnlyHint":true}}]})
                                    }
                                }
                                "tools/call" => json!({"content":[{"type":"text","text":"owned-primary-control"}]}),
                                _ => panic!("unexpected custody method"),
                            };
                            json!({"jsonrpc":"2.0","id":req["id"],"result":result})
                        }
                        _ => panic!("unexpected custody route"),
                    };
                    custody_reply(&mut stream, 200, value).await;
                });
            }
        });
        Self { origin, wire, task }
    }
    fn token_posts(&self) -> usize {
        self.wire
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|(path, _)| path == "/token")
            .count()
    }
    fn fresh_revokes(&self) -> usize {
        self.wire
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|(path, body)| {
                path == "/revoke"
                    && url::form_urlencoded::parse(body.as_bytes())
                        .any(|(key, value)| key == "token" && value == "candidate-R1-refresh")
            })
            .count()
    }
}
async fn custody_repository(path: &std::path::Path) -> AgentMcpRepository {
    let options = SqliteConnectOptions::from_str(&format!("sqlite:{}", path.display()))
        .unwrap()
        .create_if_missing(true);
    let repository = AgentMcpRepository::new(
        SqlitePoolOptions::new()
            .max_connections(2)
            .connect_with(options)
            .await
            .unwrap(),
    );
    repository.ensure_schema().await.unwrap();
    repository
}
async fn custody_setup(
    fixture: &CustodyHttp,
    path: &std::path::Path,
) -> (
    AgentMcpRepository,
    CustodyStore,
    McpServerDefinition,
    McpSecretBundle,
) {
    let repository = custody_repository(path).await;
    let store = CustodyStore::default();
    *store.2.lock().unwrap() = Some(fixture.wire.requests.clone());
    let mut server = McpServerDefinition::new("custody-fixture", McpTransport::StreamableHttp);
    server.enabled = true;
    server.url = Some(format!("{}/mcp", fixture.origin));
    server.secret_ref = Some("primary-R0-ref".into());
    server.metadata =
        json!({"auth":"oauth","oauthConnected":true,"oauthRevision":"primary-R0-revision"});
    repository.create(&server).await.unwrap();
    let mut primary = McpSecretBundle::default();
    for (k, v) in [
        ("access_token", "primary-R0-access"),
        ("refresh_token", "primary-R0-refresh"),
        ("client_id", "owned-custody-fixture-client"),
    ] {
        primary.oauth.insert(k.into(), v.into());
    }
    primary.oauth.insert(
        "revocation_endpoint".into(),
        format!("{}/revoke", fixture.origin),
    );
    store.put("primary-R0-ref", &primary).unwrap();
    (repository, store, server, primary)
}
async fn custody_connect(
    repository: &AgentMcpRepository,
    store: &CustodyStore,
    server: &McpServerDefinition,
    deadline: Duration,
) -> Result<McpServerDefinition, crate::domain::types::AppError> {
    connect_oauth_with(
        repository,
        store,
        &server.id,
        None,
        deadline,
        |definition, bundle| async move {
            authorize_oauth_server_with(&definition, &bundle, |authorization| {
                let q: BTreeMap<String, String> = reqwest::Url::parse(authorization)
                    .unwrap()
                    .query_pairs()
                    .into_owned()
                    .collect();
                tokio::spawn(async move {
                    let mut url = reqwest::Url::parse(&q["redirect_uri"]).unwrap();
                    url.query_pairs_mut()
                        .append_pair("state", &q["state"])
                        .append_pair("code", "owned-fixture-code");
                    reqwest::get(url).await.unwrap();
                });
                Ok(())
            })
            .await
        },
    )
    .await
}
fn custody_forget_runtime(id: &str) {
    OAUTH_LIFECYCLES.get().unwrap().lock().unwrap().remove(id);
}

#[tokio::test]
async fn c5_disconnect_during_received_catalog_retains_failed_candidate_and_honest_outcome() {
    let fixture = CustodyHttp::start(false, true, false, false).await;
    fixture.wire.revoke_fails.store(true, Ordering::SeqCst);
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("custody.sqlite");
    let (repository, store, server, _) = custody_setup(&fixture, &path).await;
    let operation = custody_connect(&repository, &store, &server, Duration::from_secs(10));
    let withdrawal = async {
        fixture.wire.catalog_seen.notified().await;
        let result = disconnect_oauth(&repository, &store, &server.id)
            .await
            .unwrap();
        fixture.wire.catalog_release.notify_one();
        result
    };
    let (connected, disconnected) = tokio::join!(operation, withdrawal);
    assert!(connected.is_err());
    assert_eq!(
        disconnected.remote_revoked,
        Some(false),
        "R0 success cannot confirm independently issued R1"
    );
    assert!(
        !disconnected.local_cleared,
        "failed R1 revocation retains actual candidate custody"
    );
    assert!(fixture.fresh_revokes() >= 1);
    repository.pool.close().await;
    custody_forget_runtime(&server.id);
    let reopened = custody_repository(&path).await;
    let retained = reopened.get(&server.id).await.unwrap();
    assert!(retained
        .metadata
        .get("oauthConnectAttempts")
        .and_then(Value::as_array)
        .is_some_and(|entries| !entries.is_empty()));
    assert!(store.0.lock().unwrap().values().any(|bundle| bundle
        .oauth
        .get("refresh_token")
        .map(String::as_str)
        == Some("candidate-R1-refresh")));
    fixture.wire.revoke_fails.store(false, Ordering::SeqCst);
    let deleted = delete_mcp_definition(&reopened, &store, &server.id)
        .await
        .unwrap();
    assert!(deleted.local_deleted);
    assert_eq!(deleted.remote_revoked, Some(true));
    retire_server_sessions(&server.id).await;
}
#[tokio::test]
async fn c5_invalid_catalog_compensates_actual_candidate_and_preserves_primary() {
    let fixture = CustodyHttp::start(false, false, true, false).await;
    let temp = tempfile::tempdir().unwrap();
    let (repository, store, server, primary) =
        custody_setup(&fixture, &temp.path().join("custody.sqlite")).await;
    let error = custody_connect(&repository, &store, &server, Duration::from_secs(10))
        .await
        .unwrap_err();
    assert_eq!(error.code, "agent_mcp_protocol_error");
    assert_eq!(fixture.fresh_revokes(), 1);
    let saved = repository.get(&server.id).await.unwrap();
    assert_eq!(saved.secret_ref, server.secret_ref);
    assert!(saved.enabled);
    assert_eq!(
        store.get("primary-R0-ref").unwrap().as_ref(),
        Some(&primary)
    );
    assert_eq!(store.0.lock().unwrap().len(), 1);
    // The retained primary remains usable through the actual command,
    // discovery, registry and invocation consumers, with its own bearer.
    let tools = test_mcp_definition(&repository, &store, &server.id)
        .await
        .unwrap();
    let current = repository.get(&server.id).await.unwrap();
    let discovered = discover_server(&current, &primary, None).await.unwrap();
    assert_eq!(tools.len(), discovered.len());
    let mut registry = McpToolRegistry::default();
    registry.register(&current, discovered).unwrap();
    let name = runtime_tool_name(&current.name, "read_fixture").unwrap();
    invoke_custom_mcp(
        &repository,
        &store,
        registry.resolve(&name).unwrap(),
        json!({}),
        false,
        None,
        None,
    )
    .await
    .unwrap();
    {
        let bearers = fixture.wire.bearers.lock().unwrap();
        assert!(bearers
            .iter()
            .any(|value| value == "Bearer candidate-R1-access"));
        assert!(bearers
            .iter()
            .any(|value| value == "Bearer primary-R0-access"));
    }
    retire_server_sessions(&server.id).await;
}
#[tokio::test]
async fn c5_catalog_timeout_compensates_actual_candidate_without_changing_primary() {
    let fixture = CustodyHttp::start(false, true, false, false).await;
    let temp = tempfile::tempdir().unwrap();
    let (repository, store, server, primary) =
        custody_setup(&fixture, &temp.path().join("custody.sqlite")).await;
    let error = custody_connect(&repository, &store, &server, Duration::from_millis(300))
        .await
        .unwrap_err();
    assert_eq!(error.code, "agent_mcp_timeout");
    {
        let requests = fixture.wire.requests.lock().unwrap();
        assert!(
            requests.iter().any(|(path, body)| path == "/mcp"
                && serde_json::from_str::<Value>(body)
                    .ok()
                    .is_some_and(|request| request["method"] == "tools/list")),
            "the deadline occurred after the actual candidate catalog request"
        );
        assert!(fixture
            .wire
            .bearers
            .lock()
            .unwrap()
            .iter()
            .any(|bearer| bearer == "Bearer candidate-R1-access"));
    }
    assert_eq!(fixture.fresh_revokes(), 1);
    assert_eq!(store.get("primary-R0-ref").unwrap(), Some(primary));
    assert!(repository.get(&server.id).await.unwrap().enabled);
    fixture.wire.catalog_release.notify_one();
    retire_server_sessions(&server.id).await;
}
#[tokio::test]
async fn c5_configuration_cas_rejection_compensates_without_overwriting_new_configuration() {
    let fixture = CustodyHttp::start(false, true, false, false).await;
    let temp = tempfile::tempdir().unwrap();
    let (repository, store, server, primary) =
        custody_setup(&fixture, &temp.path().join("custody.sqlite")).await;
    let connect = custody_connect(&repository, &store, &server, Duration::from_secs(10));
    let edit = async {
        fixture.wire.catalog_seen.notified().await;
        let mut current = repository.get(&server.id).await.unwrap();
        current.metadata["ownedEdit"] = json!("new-configuration");
        repository.replace(&current).await.unwrap();
        fixture.wire.catalog_release.notify_one();
    };
    let (result, ()) = tokio::join!(connect, edit);
    assert!(result.is_err());
    assert_eq!(fixture.fresh_revokes(), 1);
    let current = repository.get(&server.id).await.unwrap();
    assert_eq!(current.metadata["ownedEdit"], "new-configuration");
    assert_eq!(current.secret_ref, server.secret_ref);
    assert_eq!(store.get("primary-R0-ref").unwrap(), Some(primary));
    retire_server_sessions(&server.id).await;
}
#[tokio::test]
async fn c5_disconnect_while_code_exchange_is_pending_never_confirms_complete_remote_cleanup() {
    let fixture = CustodyHttp::start(true, false, false, false).await;
    let temp = tempfile::tempdir().unwrap();
    let (repository, store, server, _) =
        custody_setup(&fixture, &temp.path().join("custody.sqlite")).await;
    let connect = custody_connect(&repository, &store, &server, Duration::from_secs(10));
    let cancel = async {
        fixture.wire.token_seen.notified().await;
        let result = disconnect_oauth(&repository, &store, &server.id)
            .await
            .unwrap();
        fixture.wire.token_release.notify_one();
        result
    };
    let (result, disconnected) = tokio::join!(connect, cancel);
    assert!(result.is_err());
    assert_eq!(disconnected.remote_revoked, Some(false));
    assert!(!disconnected.local_cleared);
    assert_eq!(fixture.fresh_revokes(), 1);
    let after = disconnect_oauth(&repository, &store, &server.id)
        .await
        .unwrap();
    assert!(after.local_cleared);
    assert!(store.0.lock().unwrap().is_empty());
    retire_server_sessions(&server.id).await;
}
#[tokio::test]
async fn c5_lost_code_exchange_response_survives_restart_as_historical_uncertainty() {
    let fixture = CustodyHttp::start(false, false, false, true).await;
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("custody.sqlite");
    let (repository, store, server, _) = custody_setup(&fixture, &path).await;
    assert!(
        custody_connect(&repository, &store, &server, Duration::from_secs(3))
            .await
            .is_err()
    );
    repository.pool.close().await;
    custody_forget_runtime(&server.id);
    let reopened = custody_repository(&path).await;
    let disconnected = disconnect_oauth(&reopened, &store, &server.id)
        .await
        .unwrap();
    assert_eq!(disconnected.remote_revoked, Some(false));
    assert_eq!(
        fixture.token_posts(),
        1,
        "a code exchange is not replayed after an ambiguous outcome"
    );
    let saved = reopened.get(&server.id).await.unwrap();
    assert!(saved
        .metadata
        .get("oauthRevocationHistory")
        .and_then(Value::as_array)
        .is_some_and(|entries| entries
            .iter()
            .any(|entry| entry["reason"] == "connect_exchange_unconfirmed")));
    retire_server_sessions(&server.id).await;
}

#[tokio::test]
async fn c5_missing_revocation_endpoint_retains_actual_candidate_and_primary() {
    let fixture = CustodyHttp::start(false, false, true, false).await;
    fixture.wire.omit_revocation.store(true, Ordering::SeqCst);
    let temp = tempfile::tempdir().unwrap();
    let (repository, store, server, primary) =
        custody_setup(&fixture, &temp.path().join("custody.sqlite")).await;
    assert!(
        custody_connect(&repository, &store, &server, Duration::from_secs(10))
            .await
            .is_err()
    );
    let saved = repository.get(&server.id).await.unwrap();
    assert_eq!(saved.secret_ref, server.secret_ref);
    assert_eq!(store.get("primary-R0-ref").unwrap(), Some(primary));
    assert_eq!(fixture.fresh_revokes(), 0);
    assert!(saved
        .metadata
        .get("oauthConnectAttempts")
        .and_then(Value::as_array)
        .is_some_and(|entries| !entries.is_empty()));
    assert!(store.0.lock().unwrap().values().any(|value| value
        .oauth
        .get("refresh_token")
        .map(String::as_str)
        == Some("candidate-R1-refresh")));
    assert!(!test_mcp_definition(&repository, &store, &server.id)
        .await
        .unwrap()
        .is_empty());
    assert!(delete_mcp_definition(&repository, &store, &server.id)
        .await
        .is_err());
    assert!(repository.get(&server.id).await.is_ok());
    retire_server_sessions(&server.id).await;
}
#[tokio::test]
async fn c5_failed_candidate_put_compensates_or_preserves_unknown_issuance_on_restart() {
    for revoke_fails in [false, true] {
        let fixture = CustodyHttp::start(false, false, false, false).await;
        fixture
            .wire
            .revoke_fails
            .store(revoke_fails, Ordering::SeqCst);
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("custody.sqlite");
        let (repository, store, server, primary) = custody_setup(&fixture, &path).await;
        store.1.store(true, Ordering::SeqCst);
        let error = custody_connect(&repository, &store, &server, Duration::from_secs(10))
            .await
            .unwrap_err();
        assert_eq!(error.code, "agent_mcp_secure_storage_failed");
        assert_eq!(fixture.fresh_revokes(), 1);
        assert_eq!(store.get("primary-R0-ref").unwrap(), Some(primary));
        assert_eq!(
            repository.get(&server.id).await.unwrap().secret_ref,
            server.secret_ref
        );
        repository.pool.close().await;
        custody_forget_runtime(&server.id);
        let reopened = custody_repository(&path).await;
        let disconnected = disconnect_oauth(&reopened, &store, &server.id)
            .await
            .unwrap();
        assert_eq!(disconnected.remote_revoked, Some(!revoke_fails));
        if revoke_fails {
            assert!(disconnected
                .server
                .metadata
                .get("oauthRevocationHistory")
                .and_then(Value::as_array)
                .is_some_and(|entries| !entries.is_empty()));
        }
        retire_server_sessions(&server.id).await;
    }
}
#[tokio::test]
async fn c5_process_interruption_after_received_candidate_recovers_exact_grant_before_delete() {
    let fixture = CustodyHttp::start(false, true, false, false).await;
    fixture.wire.revoke_fails.store(true, Ordering::SeqCst);
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("custody.sqlite");
    let (repository, store, server, _) = custody_setup(&fixture, &path).await;
    let worker_repository = repository.clone();
    let worker_store = store.clone();
    let worker_server = server.clone();
    let worker = tokio::spawn(async move {
        custody_connect(
            &worker_repository,
            &worker_store,
            &worker_server,
            Duration::from_secs(10),
        )
        .await
    });
    fixture.wire.catalog_seen.notified().await;
    worker.abort();
    assert!(worker.await.unwrap_err().is_cancelled());
    repository.pool.close().await;
    custody_forget_runtime(&server.id);
    let reopened = custody_repository(&path).await;
    let disconnected = disconnect_oauth(&reopened, &store, &server.id)
        .await
        .unwrap();
    assert_eq!(disconnected.remote_revoked, Some(false));
    assert!(!disconnected.local_cleared);
    assert!(fixture.fresh_revokes() >= 1);
    assert!(store.0.lock().unwrap().values().any(|bundle| bundle
        .oauth
        .get("refresh_token")
        .map(String::as_str)
        == Some("candidate-R1-refresh")));
    fixture.wire.revoke_fails.store(false, Ordering::SeqCst);
    assert!(
        delete_mcp_definition(&reopened, &store, &server.id)
            .await
            .unwrap()
            .local_deleted
    );
    retire_server_sessions(&server.id).await;
}
#[tokio::test]
async fn c5_failed_attempt_reservation_prevents_all_authorization_and_token_wire() {
    let fixture = CustodyHttp::start(false, false, false, false).await;
    let temp = tempfile::tempdir().unwrap();
    let (repository, store, server, primary) =
        custody_setup(&fixture, &temp.path().join("custody.sqlite")).await;
    query("CREATE TRIGGER reject_attempt BEFORE UPDATE ON agent_mcp_servers WHEN json_array_length(json_extract(NEW.metadata_json, '$.oauthConnectAttempts')) > 0 BEGIN SELECT RAISE(ABORT, 'fixture attempt reservation rejection'); END").execute(&repository.pool).await.unwrap();
    let error = custody_connect(&repository, &store, &server, Duration::from_secs(10))
        .await
        .unwrap_err();
    assert_eq!(error.code, "agent_mcp_storage_failed");
    assert!(fixture.wire.requests.lock().unwrap().is_empty());
    assert_eq!(store.get("primary-R0-ref").unwrap(), Some(primary));
    assert_eq!(
        repository.get(&server.id).await.unwrap().secret_ref,
        server.secret_ref
    );
}

#[tokio::test]
async fn c5_legacy_received_reference_is_revoked_before_filebacked_recovery_deletes_it() {
    let fixture = CustodyHttp::start(false, false, false, false).await;
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("custody.sqlite");
    let (repository, store, mut server, primary) = custody_setup(&fixture, &path).await;
    // Reproduce the old-version durable boundary using an actually issued
    // synthetic grant, not a fabricated token response or a pre-set outcome.
    let candidate = authorize_oauth_server_with(&server, &primary, |authorization| {
        let q: BTreeMap<String, String> = reqwest::Url::parse(authorization)
            .unwrap()
            .query_pairs()
            .into_owned()
            .collect();
        tokio::spawn(async move {
            let mut url = reqwest::Url::parse(&q["redirect_uri"]).unwrap();
            url.query_pairs_mut()
                .append_pair("state", &q["state"])
                .append_pair("code", "owned-legacy-fixture-code");
            reqwest::get(url).await.unwrap();
        });
        Ok(())
    })
    .await
    .unwrap();
    store.put("legacy-candidate-reference", &candidate).unwrap();
    server.metadata["oauthPendingSecretRef"] = json!("legacy-candidate-reference");
    repository.replace(&server).await.unwrap();
    oauth_lifecycle(&server.id);
    repository.pool.close().await;
    custody_forget_runtime(&server.id);
    let reopened = custody_repository(&path).await;
    let mut current = reopened.get(&server.id).await.unwrap();
    let lifecycle = oauth_lifecycle(&server.id);
    {
        let _gate = lifecycle.gate.lock().await;
        recover_pending_oauth(&reopened, &store, &mut current)
            .await
            .unwrap();
    }
    assert_eq!(fixture.fresh_revokes(), 1);
    assert!(store.get("legacy-candidate-reference").unwrap().is_none());
    assert_eq!(store.get("primary-R0-ref").unwrap(), Some(primary));
    assert_eq!(current.secret_ref, server.secret_ref);
    assert!(current.metadata.get("oauthPendingSecretRef").is_none());
    assert!(current.metadata.get("oauthConnectAttempts").is_none());
}

#[tokio::test]
async fn c5_ambiguous_provider_failures_keep_exchange_obligation_after_restart() {
    for (status, body) in [
        (503, json!({"error":"temporarily_unavailable"})),
        (500, json!({"error":"server_error"})),
        (500, json!({"error":"invalid_client"})),
        (500, json!({"unexpected":"malformed-error-contract"})),
    ] {
        let fixture = CustodyHttp::start(false, false, false, false).await;
        *fixture.wire.token_failure.lock().unwrap() = Some((status, body));
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("custody.sqlite");
        let (repository, store, server, primary) = custody_setup(&fixture, &path).await;
        assert!(
            custody_connect(&repository, &store, &server, Duration::from_secs(10))
                .await
                .is_err()
        );
        assert_eq!(store.get("primary-R0-ref").unwrap(), Some(primary));
        repository.pool.close().await;
        custody_forget_runtime(&server.id);
        let reopened = custody_repository(&path).await;
        let disconnected = disconnect_oauth(&reopened, &store, &server.id)
            .await
            .unwrap();
        assert_eq!(disconnected.remote_revoked, Some(false));
        assert_eq!(
            fixture.token_posts(),
            1,
            "a code exchange is not replayed after an ambiguous outcome"
        );
        assert!(disconnected
            .server
            .metadata
            .get("oauthRevocationHistory")
            .and_then(Value::as_array)
            .is_some_and(|entries| entries
                .iter()
                .any(|entry| entry["reason"] == "connect_exchange_unconfirmed")));
    }
}

#[tokio::test]
async fn c5_definitive_code_refusal_clears_only_attempt_and_keeps_primary_usable() {
    let fixture = CustodyHttp::start(false, false, false, false).await;
    *fixture.wire.token_failure.lock().unwrap() = Some((400, json!({"error":"invalid_grant"})));
    let temp = tempfile::tempdir().unwrap();
    let (repository, store, server, primary) =
        custody_setup(&fixture, &temp.path().join("custody.sqlite")).await;
    let failure = custody_connect(&repository, &store, &server, Duration::from_secs(10))
        .await
        .unwrap_err();
    assert_eq!(failure.code, "agent_mcp_oauth_provider_rejected");
    let current = repository.get(&server.id).await.unwrap();
    assert!(current.metadata.get("oauthConnectAttempts").is_none());
    assert!(current.metadata.get("oauthRevocationHistory").is_none());
    assert_eq!(store.get("primary-R0-ref").unwrap(), Some(primary));
    assert!(!test_mcp_definition(&repository, &store, &server.id)
        .await
        .unwrap()
        .is_empty());
    assert_eq!(
        fixture.token_posts(),
        1,
        "definitive code refusal does not repeat the POST"
    );
    let disconnected = disconnect_oauth(&repository, &store, &server.id)
        .await
        .unwrap();
    assert_eq!(disconnected.remote_revoked, Some(true));
    retire_server_sessions(&server.id).await;
}

#[tokio::test]
async fn c5_healthy_reconnect_keeps_superseded_grant_until_explicit_disconnect() {
    let fixture = CustodyHttp::start(false, false, false, false).await;
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("custody.sqlite");
    let (repository, store, server, primary) = custody_setup(&fixture, &path).await;
    let connected = custody_connect(&repository, &store, &server, Duration::from_secs(10))
        .await
        .unwrap();
    assert_ne!(connected.secret_ref, server.secret_ref);
    assert_eq!(store.get("primary-R0-ref").unwrap(), Some(primary));
    assert_eq!(fixture.fresh_revokes(), 0);
    assert!(connected.metadata["oauthConnectAttempts"]
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| entry["superseded"] == true && entry["secretRef"] == "primary-R0-ref"));
    assert!(connected.metadata["oauthRevocationHistory"]
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| entry["reason"] == "superseded_grant"
            && entry["affectedSecretRef"] == "primary-R0-ref"
            && entry["affectedRevision"] == "primary-R0-revision"));
    let mut listed = repository.get(&server.id).await.unwrap();
    {
        let lifecycle = oauth_lifecycle(&server.id);
        let _gate = lifecycle.gate.lock().await;
        recover_pending_oauth(&repository, &store, &mut listed)
            .await
            .unwrap();
    }
    assert_eq!(fixture.fresh_revokes(), 0);
    repository.pool.close().await;
    custody_forget_runtime(&server.id);
    let reopened = custody_repository(&path).await;
    let tools = test_mcp_definition(&reopened, &store, &server.id)
        .await
        .unwrap();
    let current = reopened.get(&server.id).await.unwrap();
    let mut registry = McpToolRegistry::default();
    registry.register(&current, tools).unwrap();
    let name = runtime_tool_name(&current.name, "read_fixture").unwrap();
    invoke_custom_mcp(
        &reopened,
        &store,
        registry.resolve(&name).unwrap(),
        json!({}),
        false,
        None,
        None,
    )
    .await
    .unwrap();
    let disconnected = disconnect_oauth(&reopened, &store, &server.id)
        .await
        .unwrap();
    assert_eq!(disconnected.remote_revoked, Some(true));
    assert!(disconnected.local_cleared);
    assert_eq!(fixture.fresh_revokes(), 1);
    assert!(fixture
        .wire
        .requests
        .lock()
        .unwrap()
        .iter()
        .any(|(path, body)| path == "/revoke"
            && url::form_urlencoded::parse(body.as_bytes())
                .any(|(key, value)| key == "token" && value == "primary-R0-refresh")));
    assert!(store.0.lock().unwrap().is_empty());
    assert!(disconnected
        .server
        .metadata
        .get("oauthConnectAttempts")
        .is_none());
    assert!(disconnected
        .server
        .metadata
        .get("oauthRevocationHistory")
        .and_then(Value::as_array)
        .map_or(true, |entries| entries.is_empty()));
    retire_server_sessions(&server.id).await;
}

#[tokio::test]
async fn c5_received_mark_cannot_downgrade_confirmed_revocation_after_disconnect_wins_gate() {
    let fixture = CustodyHttp::start(true, false, false, false).await;
    fixture
        .wire
        .revoke_again_fails
        .store(true, Ordering::SeqCst);
    let temp = tempfile::tempdir().unwrap();
    let (repository, store, server, _) =
        custody_setup(&fixture, &temp.path().join("custody.sqlite")).await;
    store.3.store(true, Ordering::SeqCst);
    let worker_repository = repository.clone();
    let worker_store = store.clone();
    let worker_server = server.clone();
    let connect = tokio::spawn(async move {
        custody_connect(
            &worker_repository,
            &worker_store,
            &worker_server,
            Duration::from_secs(10),
        )
        .await
    });
    fixture.wire.token_seen.notified().await;
    let lifecycle = oauth_lifecycle(&server.id);
    let epoch = lifecycle.epoch.load(Ordering::SeqCst);
    let gate = lifecycle.gate.lock().await;
    let disconnect_repository = repository.clone();
    let disconnect_store = store.clone();
    let id = server.id.clone();
    let disconnect = tokio::spawn(async move {
        disconnect_oauth(&disconnect_repository, &disconnect_store, &id).await
    });
    timeout(Duration::from_secs(2), async {
        while lifecycle.epoch.load(Ordering::SeqCst) == epoch {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    fixture.wire.token_release.notify_one();
    timeout(Duration::from_secs(2), store.4.notified())
        .await
        .unwrap();
    // Disconnect queued before mark_received and therefore owns the first
    // revocation/cleanup transition after this gate is released.
    drop(gate);
    let disconnected = disconnect.await.unwrap().unwrap();
    assert_eq!(disconnected.remote_revoked, Some(true));
    assert!(!disconnected.local_cleared);
    assert!(connect.await.unwrap().is_err());
    assert_eq!(
        fixture.fresh_revokes(),
        1,
        "an accepted grant revocation is not downgraded by another POST"
    );
    let current = repository.get(&server.id).await.unwrap();
    let entries = current.metadata["oauthConnectAttempts"].as_array().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["phase"], "revoked_but_local_cleanup_pending");
    store.3.store(false, Ordering::SeqCst);
    let retried = disconnect_oauth(&repository, &store, &server.id)
        .await
        .unwrap();
    assert_eq!(retried.remote_revoked, Some(true));
    assert!(retried.local_cleared);
    assert_eq!(fixture.fresh_revokes(), 1);
    assert!(store.0.lock().unwrap().is_empty());
    retire_server_sessions(&server.id).await;
}

#[tokio::test]
async fn c5_missing_primary_custody_cannot_be_confirmed_by_a_candidate_revocation() {
    let fixture = CustodyHttp::start(false, true, false, false).await;
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("custody.sqlite");
    let (repository, store, server, _) = custody_setup(&fixture, &path).await;
    let connect = custody_connect(&repository, &store, &server, Duration::from_secs(10));
    let withdraw = async {
        fixture.wire.catalog_seen.notified().await;
        // Simulate loss of current-primary custody independently from the
        // genuinely issued candidate; do not pre-populate an outcome flag.
        store.0.lock().unwrap().remove("primary-R0-ref");
        let outcome = disconnect_oauth(&repository, &store, &server.id)
            .await
            .unwrap();
        fixture.wire.catalog_release.notify_one();
        outcome
    };
    let (result, disconnected) = tokio::join!(connect, withdraw);
    assert!(result.is_err());
    assert!(disconnected.local_cleared);
    assert_eq!(disconnected.remote_revoked, Some(false));
    assert_eq!(fixture.fresh_revokes(), 1);
    assert!(disconnected.server.metadata["oauthRevocationHistory"]
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| entry["reason"] == "revocation_unconfirmed"
            && entry["affectedSecretRef"] == "primary-R0-ref"
            && entry["affectedRevision"] == "primary-R0-revision"));
    repository.pool.close().await;
    custody_forget_runtime(&server.id);
    let reopened = custody_repository(&path).await;
    let retry = disconnect_oauth(&reopened, &store, &server.id)
        .await
        .unwrap();
    assert_eq!(retry.remote_revoked, Some(false));
    // A later real callback/reconnect may use the new grant, while the
    // original missing generation remains an independent durable warning.
    let before = reopened.get(&server.id).await.unwrap();
    // This fixture only holds candidate catalogs when requested explicitly.
    fixture.wire.catalog_release.notify_one();
    let reconnected = custody_connect(&reopened, &store, &before, Duration::from_secs(10))
        .await
        .unwrap();
    assert!(reconnected.enabled);
    assert!(reconnected.metadata["oauthRevocationHistory"]
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| entry["reason"] == "revocation_unconfirmed"
            && entry["affectedSecretRef"] == "primary-R0-ref"));
    fixture.wire.catalog_release.notify_one();
    assert!(!test_mcp_definition(&reopened, &store, &server.id)
        .await
        .unwrap()
        .is_empty());
    assert_eq!(
        fixture.token_posts(),
        2,
        "only the explicitly requested reconnect submits a second code exchange"
    );
    retire_server_sessions(&server.id).await;
}

#[tokio::test]
async fn c5_legacy_retired_oauth_is_deferred_and_reconciled_without_losing_old_obligations() {
    for old_custody_missing in [false, true] {
        let fixture = CustodyHttp::start(false, false, false, false).await;
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("custody.sqlite");
        let (repository, store, mut server, primary) = custody_setup(&fixture, &path).await;
        let candidate = authorize_oauth_server_with(&server, &primary, |authorization| {
            let q: BTreeMap<String, String> = reqwest::Url::parse(authorization)
                .unwrap()
                .query_pairs()
                .into_owned()
                .collect();
            tokio::spawn(async move {
                let mut url = reqwest::Url::parse(&q["redirect_uri"]).unwrap();
                url.query_pairs_mut()
                    .append_pair("state", &q["state"])
                    .append_pair("code", "owned-retired-fixture-code");
                reqwest::get(url).await.unwrap();
            });
            Ok(())
        })
        .await
        .unwrap();
        store.put("current-R1-reference", &candidate).unwrap();
        store.put("legacy-R0-reference", &primary).unwrap();
        store.0.lock().unwrap().remove("primary-R0-ref");
        if old_custody_missing {
            store.0.lock().unwrap().remove("legacy-R0-reference");
        }
        // Reproduce the old-version activation checkpoint: the new primary
        // was committed, and the previous OAuth reference awaited deletion.
        server.secret_ref = Some("current-R1-reference".into());
        server.metadata["oauthRevision"] = json!("current-R1-policy");
        server.metadata["oauthGrantRevision"] = json!("current-R1-grant");
        server.metadata["oauthRetiredSecretRef"] = json!("legacy-R0-reference");
        repository.replace(&server).await.unwrap();
        oauth_lifecycle(&server.id);
        repository.pool.close().await;
        custody_forget_runtime(&server.id);
        let reopened = custody_repository(&path).await;
        let lifecycle = oauth_lifecycle(&server.id);
        let mut listed = reopened.get(&server.id).await.unwrap();
        {
            let _gate = lifecycle.gate.lock().await;
            recover_pending_oauth(&reopened, &store, &mut listed)
                .await
                .unwrap();
        }
        assert_eq!(fixture.fresh_revokes(), 0);
        let migrated_before_disconnect = listed.metadata.get("oauthRetiredSecretRef").is_none();
        assert!(
            fixture
                .wire
                .requests
                .lock()
                .unwrap()
                .iter()
                .all(|(path, _)| path != "/revoke"),
            "ordinary inventory recovery cannot revoke a potentially shared old grant family"
        );
        let primary_test_before_disconnect =
            test_mcp_definition(&reopened, &store, &server.id).await;
        if !old_custody_missing {
            assert_eq!(
                store.get("legacy-R0-reference").unwrap().as_ref(),
                Some(&primary)
            );
            store.3.store(true, Ordering::SeqCst);
            let interrupted = disconnect_oauth(&reopened, &store, &server.id)
                .await
                .unwrap();
            assert!(!interrupted.local_cleared);
            let old_revocations = fixture
                .wire
                .requests
                .lock()
                .unwrap()
                .iter()
                .filter(|(path, body)| {
                    path == "/revoke"
                        && url::form_urlencoded::parse(body.as_bytes())
                            .any(|(key, value)| key == "token" && value == "primary-R0-refresh")
                })
                .count();
            assert_eq!(
                old_revocations, 1,
                "a retained legacy OAuth grant needs its own actual revocation"
            );
            assert!(migrated_before_disconnect);
            assert!(!primary_test_before_disconnect.unwrap().is_empty());
            assert!(listed.metadata["oauthRevocationHistory"]
                .as_array()
                .unwrap()
                .iter()
                .any(|entry| entry["reason"] == "superseded_grant"
                    && entry["affectedSecretRef"] == "legacy-R0-reference"
                    && entry["affectedRevision"].is_null()));

            let entries = interrupted.server.metadata["oauthConnectAttempts"]
                .as_array()
                .unwrap();
            assert!(entries
                .iter()
                .any(|entry| entry["secretRef"] == "legacy-R0-reference"
                    && entry["phase"] == "revoked_but_local_cleanup_pending"));
            assert!(store.get("legacy-R0-reference").unwrap().is_some());
            reopened.pool.close().await;
            custody_forget_runtime(&server.id);
            store.3.store(false, Ordering::SeqCst);
            let restarted = custody_repository(&path).await;
            let completed = disconnect_oauth(&restarted, &store, &server.id)
                .await
                .unwrap();
            assert!(completed.local_cleared);
            assert_eq!(completed.remote_revoked, Some(true));
            assert!(completed
                .server
                .metadata
                .get("oauthRevocationHistory")
                .and_then(Value::as_array)
                .map_or(true, |entries| entries.is_empty()));
        } else {
            let outcome = disconnect_oauth(&reopened, &store, &server.id)
                .await
                .unwrap();
            assert!(outcome.local_cleared);
            assert_eq!(outcome.remote_revoked, Some(false));
            assert!(migrated_before_disconnect);
            assert!(!primary_test_before_disconnect.unwrap().is_empty());
            assert!(outcome.server.metadata["oauthRevocationHistory"]
                .as_array()
                .unwrap()
                .iter()
                .any(|entry| entry["reason"] == "revocation_unconfirmed"
                    && entry["affectedSecretRef"] == "legacy-R0-reference"
                    && entry["affectedRevision"].is_null()));
            reopened.pool.close().await;
            custody_forget_runtime(&server.id);
            let restarted = custody_repository(&path).await;
            assert_eq!(
                disconnect_oauth(&restarted, &store, &server.id)
                    .await
                    .unwrap()
                    .remote_revoked,
                Some(false)
            );
        }
        let old_revocations = fixture
            .wire
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|(path, body)| {
                path == "/revoke"
                    && url::form_urlencoded::parse(body.as_bytes())
                        .any(|(key, value)| key == "token" && value == "primary-R0-refresh")
            })
            .count();
        assert_eq!(old_revocations, usize::from(!old_custody_missing));
        assert!(fixture.fresh_revokes() >= 1);
        assert!(store.0.lock().unwrap().is_empty());
        retire_server_sessions(&server.id).await;
    }
}
