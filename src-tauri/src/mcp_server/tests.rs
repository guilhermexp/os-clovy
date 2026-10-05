//! MCP calls against test databases: the protocol handler directly, and the
//! real relay (`stdio::relay`) through the authenticated socket.

use std::io::Write;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration as StdDuration;

use chrono::{DateTime, Duration, Local, TimeZone, Utc};
use serde_json::{json, Value};
use sqlx::query::query;
use sqlx_sqlite::SqlitePool;

use super::tools::{ActivityAccess, McpData};
use super::*;
use crate::activity::key::MemoryKeyStore;
use crate::activity::settings::ActivitySettings;
use crate::activity::store::{ActivityStore, NewFrame, TextSource, ACTIVITY_DB_FILE};
use crate::activity::timeline::etl::run_pass;
use crate::db::repositories::Repositories;

pub(super) fn at(hour: u32, minute: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 1, hour, minute, 0).unwrap()
}

/// Notes, dictations, and memories in the default profile, and a decoy of
/// each in another profile.
pub(super) async fn notes_pool() -> SqlitePool {
    let pool = sqlx_sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    crate::db::migrations::run_migrations(&pool).await.unwrap();
    // Transcripts reference audio artifacts the fixture does not need.
    query("PRAGMA foreign_keys = OFF")
        .execute(&pool)
        .await
        .unwrap();
    let repositories = Repositories::new(pool.clone());
    for (id, profile, title, content, transcript) in [
        (
            "note-roadmap",
            "default",
            "Roadmap sync",
            "Decisions: ship the zephyr importer on Friday after QA signs off.",
            "Alice: the zephyr importer is blocked by QA until Thursday.",
        ),
        (
            "note-other-profile",
            "work",
            "Zephyr budget",
            "Other profile: zephyr budget is confidential.",
            "",
        ),
        (
            "note-unrelated",
            "default",
            "Lunch",
            "Pizza on Tuesday.",
            "",
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
        if !transcript.is_empty() {
            query("INSERT INTO transcripts (id, note_id, audio_artifact_id, text, provider, status, created_at, updated_at) VALUES (?, ?, 'audio', ?, 'test', 'completed', ?, ?)")
                .bind(format!("transcript-{id}"))
                .bind(id)
                .bind(transcript)
                .bind(Utc::now().to_rfc3339())
                .bind(Utc::now().to_rfc3339())
                .execute(&pool)
                .await
                .unwrap();
        }
    }
    for (id, profile, text) in [
        (
            "dictation-1",
            "default",
            "Remember to call Bruno about zephyr.",
        ),
        ("dictation-2", "default", "Buy coffee."),
        ("dictation-3", "work", "Zephyr work dictation."),
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
    repositories
        .create_memory("default", None, "Prefers short summaries.", "user")
        .await
        .unwrap();
    repositories
        .create_memory("work", None, "Work profile memory.", "user")
        .await
        .unwrap();
    pool
}

/// The morning of `at(9, 0)`: Code, Firefox on docs.rs, 1Password.
pub(super) async fn activity_store() -> (tempfile::TempDir, ActivityStore) {
    let dir = tempfile::tempdir().unwrap();
    let store = ActivityStore::open(
        &dir.path().join(ACTIVITY_DB_FILE),
        &MemoryKeyStore::default(),
    )
    .await
    .unwrap();
    let runs = [
        (
            at(9, 0),
            150,
            "Code",
            "etl.rs - os-clovy",
            None,
            "fn run_pass",
        ),
        (
            at(9, 5),
            60,
            "Firefox",
            "Rust FTS",
            Some("https://docs.rs/zephyrine"),
            "zephyrine crate docs",
        ),
        (at(9, 7), 30, "1Password", "Vault", None, "secret"),
    ];
    for (start, count, app, title, url, text) in runs {
        for index in 0..count {
            store
                .insert_frame(&NewFrame {
                    captured_at: start + Duration::seconds(index * 2),
                    app_name: app.into(),
                    bundle_id: None,
                    window_title: Some(title.into()),
                    browser_url: url.map(str::to_string),
                    text_source: TextSource::Accessibility,
                    text: Some(format!("{text} {index}")),
                })
                .await
                .unwrap();
        }
    }
    run_pass(&store, at(9, 8), &[]).await.unwrap();
    (dir, store)
}

pub(super) fn data(notes: SqlitePool, activity: ActivityAccess) -> McpData {
    McpData {
        notes,
        profile: "default".into(),
        memory_enabled: true,
        activity,
        now: at(9, 8),
    }
}

fn capture_on() -> ActivitySettings {
    ActivitySettings {
        enabled: true,
        ..ActivitySettings::default()
    }
}

async fn request(data: &McpData, method: &str, params: Value) -> Value {
    protocol::handle(
        data,
        &json!({ "jsonrpc": "2.0", "id": 7, "method": method, "params": params }),
    )
    .await
    .expect("a request is answered")
}

/// A `tools/call` result's JSON payload, or the error text.
async fn call(data: &McpData, name: &str, arguments: Value) -> Result<Value, String> {
    let response = request(
        data,
        "tools/call",
        json!({ "name": name, "arguments": arguments }),
    )
    .await;
    let result = &response["result"];
    let text = result["content"][0]["text"].as_str().unwrap_or_default();
    if result["isError"] == json!(true) {
        Err(text.to_string())
    } else {
        Ok(serde_json::from_str(text).expect("tool text is JSON"))
    }
}

fn tool_names(response: &Value) -> Vec<String> {
    response["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap().to_string())
        .collect()
}

#[tokio::test]
async fn activity_tools_are_listed_only_while_capture_is_on() {
    let pool = notes_pool().await;
    let off = tool_names(
        &request(
            &data(pool.clone(), ActivityAccess::Off),
            "tools/list",
            json!({}),
        )
        .await,
    );
    assert_eq!(
        off,
        vec![
            "search_notes",
            "get_note",
            "list_dictations",
            "list_memories"
        ]
    );
    let on = tool_names(&request(&data(pool, ActivityAccess::On), "tools/list", json!({})).await);
    assert_eq!(
        on,
        vec![
            "search_notes",
            "get_note",
            "list_dictations",
            "list_memories",
            "get_activity_timeline",
            "search_activity",
            "get_activity_stats",
            "get_active_session",
            "list_app_usage",
            "get_session_detail",
        ]
    );
}

#[tokio::test]
async fn activity_tool_calls_refuse_while_capture_is_off() {
    let data = data(notes_pool().await, ActivityAccess::Off);
    let error = call(&data, "get_activity_timeline", json!({}))
        .await
        .unwrap_err();
    assert!(error.contains("Activity capture is off"), "{error}");
}

#[tokio::test]
async fn search_notes_returns_title_and_snippet_from_the_active_profile_only() {
    let data = data(notes_pool().await, ActivityAccess::Off);
    let found = call(&data, "search_notes", json!({ "query": "Zephyr" }))
        .await
        .unwrap();
    let notes = found["notes"].as_array().unwrap();
    assert_eq!(notes.len(), 1, "{found}");
    assert_eq!(notes[0]["id"], json!("note-roadmap"));
    assert_eq!(notes[0]["title"], json!("Roadmap sync"));
    assert_eq!(notes[0]["matchedIn"], json!("note"));
    assert!(notes[0]["snippet"]
        .as_str()
        .unwrap()
        .contains("ship the zephyr importer"));

    let by_transcript = call(&data, "search_notes", json!({ "query": "Alice" }))
        .await
        .unwrap();
    assert_eq!(by_transcript["notes"][0]["matchedIn"], json!("transcript"));
}

#[tokio::test]
async fn get_note_reads_the_note_and_transcript_but_not_another_profile() {
    let data = data(notes_pool().await, ActivityAccess::Off);
    let note = call(&data, "get_note", json!({ "id": "note-roadmap" }))
        .await
        .unwrap();
    assert_eq!(note["title"], json!("Roadmap sync"));
    assert!(note["content"].as_str().unwrap().contains("Friday"));
    assert!(note["transcript"].as_str().unwrap().contains("Alice"));
    assert_eq!(note["nextTranscriptOffset"], Value::Null);

    let error = call(&data, "get_note", json!({ "id": "note-other-profile" }))
        .await
        .unwrap_err();
    assert!(error.contains("note_not_found"), "{error}");
}

#[tokio::test]
async fn dictations_and_memories_stay_in_the_active_profile() {
    let pool = notes_pool().await;
    let data_on = data(pool.clone(), ActivityAccess::Off);
    let dictations = call(&data_on, "list_dictations", json!({})).await.unwrap();
    let texts: Vec<&str> = dictations["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["text"].as_str().unwrap())
        .collect();
    assert_eq!(texts.len(), 2, "{dictations}");
    assert!(!texts.iter().any(|text| text.contains("work")));
    let filtered = call(&data_on, "list_dictations", json!({ "query": "zephyr" }))
        .await
        .unwrap();
    assert_eq!(filtered["count"], json!(1));

    let memories = call(&data_on, "list_memories", json!({})).await.unwrap();
    assert_eq!(memories["count"], json!(1));
    assert_eq!(
        memories["items"][0]["content"],
        json!("Prefers short summaries.")
    );

    let mut memory_off = data(pool, ActivityAccess::Off);
    memory_off.memory_enabled = false;
    let error = call(&memory_off, "list_memories", json!({}))
        .await
        .unwrap_err();
    assert!(error.contains("memory_disabled"), "{error}");
}

#[tokio::test]
async fn activity_tools_answer_with_exclusions_and_retention() {
    let (_dir, store) = activity_store().await;
    let pool = notes_pool().await;
    let open = |settings: ActivitySettings| ActivityAccess::Open {
        store: store.clone(),
        settings,
    };
    let data_all = data(pool.clone(), open(capture_on()));
    let range = json!({ "from": "2026-10-01T08:00:00Z", "to": "2026-10-01T12:00:00Z" });

    let timeline = call(&data_all, "get_activity_timeline", range.clone())
        .await
        .unwrap();
    let apps: Vec<&str> = timeline["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|session| session["app"].as_str().unwrap())
        .collect();
    assert_eq!(apps, vec!["Code", "Firefox", "1Password"]);
    let vault_id = timeline["sessions"][2]["id"].as_i64().unwrap();

    let usage = call(&data_all, "list_app_usage", range.clone())
        .await
        .unwrap();
    assert_eq!(
        usage["apps"][0],
        json!({ "app": "Code", "minutes": 5.0, "sessions": 1 })
    );
    let stats = call(&data_all, "get_activity_stats", range.clone())
        .await
        .unwrap();
    assert_eq!(stats["focusedMinutes"], json!(8.0));

    let detail = call(&data_all, "get_session_detail", json!({ "id": vault_id }))
        .await
        .unwrap();
    assert_eq!(detail["session"]["app"], json!("1Password"));
    assert_eq!(detail["windows"][0]["windowTitle"], json!("Vault"));

    let now = call(&data_all, "get_active_session", json!({}))
        .await
        .unwrap();
    let current = if now["session"].is_null() {
        &now["lastSession"]
    } else {
        &now["session"]
    };
    assert_eq!(current["app"], json!("1Password"), "{now}");

    let excluding = data(
        pool.clone(),
        open(ActivitySettings {
            ignored_apps: vec!["1Password".into()],
            ignored_domains: vec!["docs.rs".into()],
            ..capture_on()
        }),
    );
    let usage = call(&excluding, "list_app_usage", range.clone())
        .await
        .unwrap();
    let apps: Vec<&str> = usage["apps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|app| app["app"].as_str().unwrap())
        .collect();
    assert_eq!(apps, vec!["Code"]);
    let hidden = call(&excluding, "get_session_detail", json!({ "id": vault_id }))
        .await
        .unwrap_err();
    assert!(hidden.contains("activity_session_not_found"), "{hidden}");
    let searched = call(
        &excluding,
        "search_activity",
        json!({ "query": "zephyrine" }),
    )
    .await
    .unwrap();
    assert_eq!(searched["results"], json!([]));

    let mut expired = data(pool, open(capture_on()));
    expired.now = at(9, 8) + Duration::days(31);
    let gone = call(&expired, "get_session_detail", json!({ "id": vault_id }))
        .await
        .unwrap_err();
    assert!(gone.contains("activity_session_not_found"), "{gone}");
}

#[tokio::test]
async fn resources_give_the_current_context_and_the_guide() {
    let data = data(notes_pool().await, ActivityAccess::Off);
    let listed = request(&data, "resources/list", json!({})).await;
    let uris: Vec<&str> = listed["result"]["resources"]
        .as_array()
        .unwrap()
        .iter()
        .map(|resource| resource["uri"].as_str().unwrap())
        .collect();
    assert_eq!(uris, vec!["clovy://context", "clovy://guide"]);

    let guide = request(&data, "resources/read", json!({ "uri": "clovy://guide" })).await;
    assert!(guide["result"]["contents"][0]["text"]
        .as_str()
        .unwrap()
        .contains("search_notes"));
    let missing = request(&data, "resources/read", json!({ "uri": "clovy://nope" })).await;
    assert_eq!(missing["error"]["code"], json!(-32002));

    let context = request(&data, "resources/read", json!({ "uri": "clovy://context" })).await;
    let context: Value =
        serde_json::from_str(context["result"]["contents"][0]["text"].as_str().unwrap()).unwrap();
    let local = at(9, 8).with_timezone(&Local);
    assert_eq!(context["date"], json!(local.format("%Y-%m-%d").to_string()));
    assert_eq!(context["time"], json!(local.format("%H:%M").to_string()));
    assert!(context["timeZone"]
        .as_str()
        .is_some_and(|zone| !zone.is_empty()));
}

#[test]
fn context_reports_date_time_and_zone() {
    let now = chrono::FixedOffset::west_opt(3 * 3600)
        .unwrap()
        .with_ymd_and_hms(2026, 10, 4, 14, 5, 0)
        .unwrap()
        .with_timezone(&Local);
    let context = protocol::context_json(now, Some("America/Sao_Paulo".into()));
    assert_eq!(context["timeZone"], json!("America/Sao_Paulo"));
    assert_eq!(context["now"], json!(now.to_rfc3339()));
    assert_eq!(context["date"], json!(now.format("%Y-%m-%d").to_string()));
}

#[test]
fn configuration_snippets_point_at_the_bundled_relay() {
    let launch = launch(
        Path::new("/Applications/Clovy.app/Contents/Resources/native/bin/clovy-mcp"),
        Path::new("/Users/me/Library/Application Support/co.opensoftware.june/mcp"),
    );
    let claude = claude_code_config(&launch);
    assert_eq!(
        claude,
        json!({ "mcpServers": { "clovy": {
            "type": "stdio",
            "command": "/Applications/Clovy.app/Contents/Resources/native/bin/clovy-mcp",
            "args": ["--dir", "/Users/me/Library/Application Support/co.opensoftware.june/mcp"],
        }}})
    );
    assert_eq!(
        cursor_config(&launch)["mcpServers"]["clovy"]["command"],
        claude["mcpServers"]["clovy"]["command"]
    );
    assert_eq!(
        cursor_config(&launch)["mcpServers"]["clovy"].get("type"),
        None
    );
    assert_eq!(
        claude_code_command(&launch),
        "claude mcp add --scope user clovy -- /Applications/Clovy.app/Contents/Resources/native/bin/clovy-mcp --dir '/Users/me/Library/Application Support/co.opensoftware.june/mcp'"
    );
    assert_eq!(
        stdio::parse_dir(&["--dir".into(), "/tmp/x".into()]).unwrap(),
        Path::new("/tmp/x")
    );
    assert!(stdio::parse_dir(&[]).is_err());
}

#[test]
fn settings_files_without_the_switch_keep_the_server_off() {
    let settings: ActivitySettings = serde_json::from_str(r#"{"enabled":true}"#).unwrap();
    assert!(!settings.mcp_server);
    let saved = serde_json::to_value(ActivitySettings {
        mcp_server: true,
        ..ActivitySettings::default()
    })
    .unwrap();
    assert_eq!(saved["mcpServer"], json!(true));
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Collects what the relay writes to stdout.
#[derive(Clone, Default)]
pub(super) struct Captured(Arc<Mutex<Vec<u8>>>);

impl Write for Captured {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        lock(&self.0).extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Captured {
    /// Waits until `count` messages arrived (the app answers on another
    /// thread) and returns them by id.
    pub(super) fn messages(&self, count: usize) -> Vec<Value> {
        for _ in 0..200 {
            let text = String::from_utf8(lock(&self.0).clone()).unwrap();
            let messages: Vec<Value> = text
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
            if messages.len() >= count {
                return messages;
            }
            std::thread::sleep(StdDuration::from_millis(25));
        }
        panic!("relay wrote fewer than {count} messages");
    }
}

pub(super) fn by_id(messages: &[Value], id: i64) -> &Value {
    messages
        .iter()
        .find(|message| message["id"] == json!(id))
        .unwrap_or_else(|| panic!("no answer for id {id}: {messages:?}"))
}

pub(super) fn handler_for(data: McpData) -> channel::Handler {
    let data = Arc::new(data);
    Arc::new(move |message: Value| {
        let data = Arc::clone(&data);
        Box::pin(async move { protocol::handle(&data, &message).await })
    })
}

pub(super) async fn run_relay(dir: &Path, lines: &[Value]) -> Captured {
    let input: String = lines.iter().map(|line| format!("{line}\n")).collect();
    let output = Captured::default();
    let (dir, sink) = (dir.to_path_buf(), output.clone());
    tokio::task::spawn_blocking(move || {
        stdio::relay(&dir, std::io::Cursor::new(input), Box::new(sink));
    })
    .await
    .unwrap();
    output
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_relay_answers_mcp_calls_through_the_authenticated_socket() {
    let root = tempfile::tempdir().unwrap();
    let dir = channel::channel_dir(root.path());
    let listener = channel::Listener::start(
        &dir,
        handler_for(data(notes_pool().await, ActivityAccess::Off)),
    )
    .await
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&dir), 0o700);
    assert_eq!(mode(&channel::socket_path(&dir)), 0o600);
    assert_eq!(mode(&channel::secret_path(&dir)), 0o600);

    let output = run_relay(
        &dir,
        &[
            json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "test", "version": "1" } } }),
            json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
            json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }),
            json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": { "name": "search_notes", "arguments": { "query": "zephyr" } } }),
        ],
    )
    .await;
    let messages = output.messages(3);
    assert_eq!(
        by_id(&messages, 1)["result"]["serverInfo"]["name"],
        json!("clovy")
    );
    assert_eq!(
        by_id(&messages, 2)["result"]["tools"]
            .as_array()
            .unwrap()
            .len(),
        4
    );
    let found: Value = serde_json::from_str(
        by_id(&messages, 3)["result"]["content"][0]["text"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(found["notes"][0]["title"], json!("Roadmap sync"));
    listener.stop();
    assert!(!channel::socket_path(&dir).exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_relay_reports_a_server_that_is_off_or_never_enabled() {
    let root = tempfile::tempdir().unwrap();
    let dir = channel::channel_dir(root.path());
    let lines = [
        json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "2025-06-18" } }),
        json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": { "name": "search_notes", "arguments": { "query": "x" } } }),
        json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/list" }),
    ];
    let never = run_relay(&dir, &lines).await.messages(3);
    assert_eq!(
        by_id(&never, 2)["result"]["content"][0]["text"],
        json!(stdio::NEVER_ENABLED_MESSAGE)
    );

    // Turned on once, then off: the secret stays, the socket is gone.
    let listener = channel::Listener::start(
        &dir,
        handler_for(data(notes_pool().await, ActivityAccess::Off)),
    )
    .await
    .unwrap();
    listener.stop();
    let off = run_relay(&dir, &lines).await.messages(3);
    assert!(by_id(&off, 1)["result"]["instructions"]
        .as_str()
        .unwrap()
        .contains(stdio::OFF_MESSAGE));
    assert_eq!(by_id(&off, 2)["result"]["isError"], json!(true));
    assert_eq!(
        by_id(&off, 2)["result"]["content"][0]["text"],
        json!(stdio::OFF_MESSAGE)
    );
    assert_eq!(
        by_id(&off, 3)["error"]["message"],
        json!(stdio::OFF_MESSAGE)
    );
}
