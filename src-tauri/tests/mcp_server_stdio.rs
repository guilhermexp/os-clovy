//! The `clovy-mcp` binary driven the way an MCP client drives it (spawned,
//! JSON-RPC over stdio), with the app side of the local channel (Unix socket
//! + installation secret) served in this process against test databases.
#![cfg(unix)]

use std::path::Path;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration as StdDuration;

use chrono::{DateTime, Duration, TimeZone, Utc};
use clovy_lib::activity::key::{ActivityKeyStore, KeyStoreError};
use clovy_lib::activity::settings::ActivitySettings;
use clovy_lib::activity::store::{ActivityStore, NewFrame, TextSource, ACTIVITY_DB_FILE};
use clovy_lib::activity::timeline::etl::run_pass;
use clovy_lib::db::repositories::Repositories;
use clovy_lib::mcp_server::tools::{ActivityAccess, McpData};
use clovy_lib::mcp_server::{channel, protocol, stdio};
use serde_json::{json, Value};
use sqlx::query::query;
use sqlx_sqlite::SqlitePool;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use zeroize::Zeroizing;

const RANGE_FROM: &str = "2026-10-01T08:00:00Z";
const RANGE_TO: &str = "2026-10-01T12:00:00Z";

fn at(hour: u32, minute: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 1, hour, minute, 0).unwrap()
}

/// The activity key held in memory: these tests never touch the Keychain.
#[derive(Default)]
struct MemoryKey(Mutex<Option<String>>);

impl MemoryKey {
    fn slot(&self) -> std::sync::MutexGuard<'_, Option<String>> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl ActivityKeyStore for MemoryKey {
    fn load(&self) -> Result<Option<Zeroizing<String>>, KeyStoreError> {
        Ok(self.slot().clone().map(Zeroizing::new))
    }
    fn store(&self, key_hex: &str) -> Result<(), KeyStoreError> {
        *self.slot() = Some(key_hex.to_string());
        Ok(())
    }
    fn delete(&self) -> Result<(), KeyStoreError> {
        *self.slot() = None;
        Ok(())
    }
}

/// A note and a dictation in the active profile (`default`) and decoys in
/// another profile.
async fn notes_pool() -> SqlitePool {
    let pool = sqlx_sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    clovy_lib::db::migrations::run_migrations(&pool)
        .await
        .unwrap();
    let repositories = Repositories::new(pool.clone());
    for (id, profile, title, content) in [
        (
            "note-roadmap",
            "default",
            "Roadmap sync",
            "Decisions: ship the zephyr importer on Friday.",
        ),
        (
            "note-other-profile",
            "work",
            "Zephyr budget",
            "Other profile: zephyr budget is confidential.",
        ),
    ] {
        repositories
            .create_note_with_id(profile, None, id)
            .await
            .unwrap();
        query("UPDATE notes SET title = ?, edited_content = ? WHERE id = ?")
            .bind(title)
            .bind(content)
            .bind(id)
            .execute(&pool)
            .await
            .unwrap();
    }
    for (id, profile, text) in [
        ("dictation-1", "default", "Call Bruno about zephyr."),
        ("dictation-2", "work", "Zephyr work dictation."),
    ] {
        query("INSERT INTO dictation_history (id, text, language, provider, created_at, profile) VALUES (?, ?, 'en', 'test', ?, ?)")
            .bind(id)
            .bind(text)
            .bind(Utc::now().to_rfc3339())
            .bind(profile)
            .execute(&pool)
            .await
            .unwrap();
    }
    pool
}

/// Five minutes in Code, then two in Firefox, on the morning of `at(9, 0)`.
async fn activity_store(dir: &Path) -> ActivityStore {
    let store = ActivityStore::open(&dir.join(ACTIVITY_DB_FILE), &MemoryKey::default())
        .await
        .unwrap();
    for (start, count, app, url) in [
        (at(9, 0), 150, "Code", None),
        (at(9, 5), 60, "Firefox", Some("https://docs.rs/zephyrine")),
    ] {
        for index in 0..count {
            store
                .insert_frame(&NewFrame {
                    captured_at: start + Duration::seconds(index * 2),
                    app_name: app.into(),
                    bundle_id: None,
                    window_title: Some(format!("{app} window")),
                    browser_url: url.map(str::to_string),
                    text_source: TextSource::Accessibility,
                    text: Some(format!("{app} text {index}")),
                })
                .await
                .unwrap();
        }
    }
    run_pass(&store, at(9, 8), &[]).await.unwrap();
    store
}

/// The app side: each message is answered from the test databases, activity
/// capture on, in the `default` profile.
fn handler(notes: SqlitePool, store: ActivityStore) -> channel::Handler {
    Arc::new(move |message: Value| {
        let data = McpData {
            notes: notes.clone(),
            profile: "default".into(),
            memory_enabled: true,
            activity: ActivityAccess::Open {
                store: store.clone(),
                settings: ActivitySettings {
                    enabled: true,
                    ..ActivitySettings::default()
                },
            },
            now: at(9, 8),
        };
        Box::pin(async move { protocol::handle(&data, &message).await })
    })
}

/// `clovy-mcp --dir <dir>` as an MCP client spawns it.
struct Client {
    child: Child,
    stdin: ChildStdin,
    stdout: Lines<BufReader<ChildStdout>>,
    next_id: i64,
}

impl Client {
    async fn spawn(dir: &Path) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_clovy-mcp"))
            .arg("--dir")
            .arg(dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .expect("clovy-mcp starts");
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap()).lines();
        let mut client = Self {
            child,
            stdin,
            stdout,
            next_id: 0,
        };
        let initialized = client
            .request(
                "initialize",
                json!({ "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "integration-test", "version": "1" } }),
            )
            .await;
        assert_eq!(
            initialized["result"]["serverInfo"]["name"],
            json!("clovy"),
            "{initialized}"
        );
        client
            .send(json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }))
            .await;
        client
    }

    async fn send(&mut self, message: Value) {
        self.stdin
            .write_all(format!("{message}\n").as_bytes())
            .await
            .unwrap();
        self.stdin.flush().await.unwrap();
    }

    async fn request(&mut self, method: &str, params: Value) -> Value {
        self.next_id += 1;
        let id = self.next_id;
        self.send(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }))
            .await;
        loop {
            let line = tokio::time::timeout(StdDuration::from_secs(10), self.stdout.next_line())
                .await
                .unwrap_or_else(|_| panic!("no answer to {method} within 10 s"))
                .unwrap()
                .expect("clovy-mcp closed stdout");
            let message: Value = serde_json::from_str(&line).expect("stdout carries JSON-RPC");
            if message["id"] == json!(id) {
                return message;
            }
        }
    }

    /// A `tools/call` result: `Ok(payload)` or `Err(text)` for `isError`.
    async fn call(&mut self, name: &str, arguments: Value) -> Result<Value, String> {
        let response = self
            .request(
                "tools/call",
                json!({ "name": name, "arguments": arguments }),
            )
            .await;
        let result = &response["result"];
        let text = result["content"][0]["text"]
            .as_str()
            .unwrap_or_else(|| panic!("tools/call answered without text: {response}"));
        if result["isError"] == json!(true) {
            Err(text.to_string())
        } else {
            Ok(serde_json::from_str(text).expect("tool text is JSON"))
        }
    }

    async fn close(mut self) {
        drop(self.stdin);
        let status = tokio::time::timeout(StdDuration::from_secs(10), self.child.wait())
            .await
            .expect("clovy-mcp exits when stdin closes")
            .unwrap();
        assert!(status.success(), "{status}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mcp_server_binary_answers_a_stdio_client_from_the_active_profile() {
    let root = tempfile::tempdir().unwrap();
    let dir = channel::channel_dir(root.path());
    let notes = notes_pool().await;
    let store = activity_store(root.path()).await;
    let listener = channel::Listener::start(&dir, handler(notes.clone(), store.clone()))
        .await
        .unwrap();
    let mut client = Client::spawn(&dir).await;

    let listed = client.request("tools/list", json!({})).await;
    let mut names: Vec<&str> = listed["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();
    names.sort_unstable();
    assert_eq!(
        names,
        vec![
            "get_active_session",
            "get_activity_stats",
            "get_activity_timeline",
            "get_note",
            "get_session_detail",
            "list_app_usage",
            "list_dictations",
            "list_memories",
            "search_activity",
            "search_notes",
        ]
    );

    let found = client
        .call("search_notes", json!({ "query": "zephyr" }))
        .await
        .unwrap();
    let titles: Vec<&str> = found["notes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|note| note["title"].as_str().unwrap())
        .collect();
    assert_eq!(titles, vec!["Roadmap sync"], "{found}");
    let other = client
        .call("get_note", json!({ "id": "note-other-profile" }))
        .await;
    assert!(other.is_err(), "another profile's note leaked: {other:?}");
    let dictations = client.call("list_dictations", json!({})).await.unwrap();
    assert_eq!(
        dictations["items"].as_array().unwrap().len(),
        1,
        "{dictations}"
    );
    assert_eq!(
        dictations["items"][0]["text"],
        json!("Call Bruno about zephyr.")
    );

    let timeline = client
        .call(
            "get_activity_timeline",
            json!({ "from": RANGE_FROM, "to": RANGE_TO }),
        )
        .await
        .unwrap();
    let apps: Vec<&str> = timeline["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|session| session["app"].as_str().unwrap())
        .collect();
    assert_eq!(apps, vec!["Code", "Firefox"], "{timeline}");

    let guide = client
        .request("resources/read", json!({ "uri": "clovy://guide" }))
        .await;
    assert!(guide["result"]["contents"][0]["text"]
        .as_str()
        .unwrap()
        .contains("search_notes"));
    let context = client
        .request("resources/read", json!({ "uri": "clovy://context" }))
        .await;
    let context: Value =
        serde_json::from_str(context["result"]["contents"][0]["text"].as_str().unwrap()).unwrap();
    assert!(context["timeZone"]
        .as_str()
        .is_some_and(|zone| !zone.is_empty()));

    // Turned off in Settings: the same client process gets the reason, and
    // answers again once the server is back on.
    listener.stop();
    assert_eq!(
        client
            .call("search_notes", json!({ "query": "zephyr" }))
            .await,
        Err(stdio::OFF_MESSAGE.to_string())
    );
    let listener = channel::Listener::start(&dir, handler(notes, store))
        .await
        .unwrap();
    assert!(client
        .call("search_notes", json!({ "query": "zephyr" }))
        .await
        .is_ok());

    client.close().await;
    listener.stop();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mcp_server_binary_reports_never_enabled_and_a_refused_secret() {
    let root = tempfile::tempdir().unwrap();
    let dir = channel::channel_dir(root.path());

    let mut never = Client::spawn(&dir).await;
    assert_eq!(
        never
            .call("search_notes", json!({ "query": "zephyr" }))
            .await,
        Err(stdio::NEVER_ENABLED_MESSAGE.to_string())
    );
    never.close().await;

    let store = activity_store(root.path()).await;
    let listener = channel::Listener::start(&dir, handler(notes_pool().await, store))
        .await
        .unwrap();
    // A secret that is not the installation's: the app refuses before any
    // tool runs.
    std::fs::write(channel::secret_path(&dir), "ab".repeat(32)).unwrap();
    let mut refused = Client::spawn(&dir).await;
    assert_eq!(
        refused
            .call("search_notes", json!({ "query": "zephyr" }))
            .await,
        Err(stdio::REFUSED_MESSAGE.to_string())
    );
    let listed = refused.request("tools/list", json!({})).await;
    assert_eq!(listed["error"]["message"], json!(stdio::REFUSED_MESSAGE));
    refused.close().await;
    listener.stop();
}
