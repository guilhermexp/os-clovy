use super::{ActivityAccess, McpData, ToolCtx};
use crate::approval_broker::testing::{Scripted, TestBroker};
use crate::db::repositories::Repositories;
use crate::llm::cli::CliKind;
use serde_json::json;

#[tokio::test]
async fn memory_approved_save_and_forget_persist_with_tombstone() {
    for cli in [CliKind::Claude, CliKind::Codex] {
        let broker = TestBroker::new([Scripted::Approve, Scripted::Approve]).await;
        let binding = broker.turn(cli, true).await;
        let data = McpData {
            notes: broker.pool().clone(),
            profile: "default".into(),
            memory_enabled: true,
            activity: ActivityAccess::Off,
            now: chrono::Utc::now(),
        };
        let ctx = ToolCtx {
            data: &data,
            app: None,
            binding: Some(&binding),
            broker: Some(broker.broker()),
        };
        let settings = tempfile::tempdir().unwrap();
        let path = settings.path().join("memory.json");
        std::fs::write(&path, r#"{"enabled":true}"#).unwrap();
        let saved = super::memory::call_with_settings(
            &ctx,
            "save_memory",
            &json!({"content":"  Prefer concise notes  "}),
            &path,
        )
        .await
        .unwrap();
        let id = saved["id"].as_str().unwrap();
        assert_eq!(
            Repositories::new(data.notes.clone())
                .list_memories("default", None, false)
                .await
                .unwrap()[0]
                .content,
            "Prefer concise notes"
        );
        super::memory::call_with_settings(&ctx, "forget_memory", &json!({"id":id}), &path)
            .await
            .unwrap();
        assert!(Repositories::new(data.notes.clone())
            .list_memories("default", None, false)
            .await
            .unwrap()
            .is_empty());
        let count: i64 =
            sqlx::query_scalar::query_scalar("SELECT COUNT(*) FROM memory_tombstones WHERE id = ?")
                .bind(id)
                .fetch_one(&data.notes)
                .await
                .unwrap();
        assert_eq!(count, 1);
        assert_eq!(broker.requests().len(), 2);
    }
}

#[tokio::test]
async fn memory_denied_unattended_and_disabled_scopes_do_not_write() {
    let broker = TestBroker::new([Scripted::Deny, Scripted::Deny]).await;
    let binding = broker.turn(CliKind::Claude, true).await;
    let unattended = broker.turn(CliKind::Codex, false).await;
    let data = McpData {
        notes: broker.pool().clone(),
        profile: "default".into(),
        memory_enabled: true,
        activity: ActivityAccess::Off,
        now: chrono::Utc::now(),
    };
    let settings = tempfile::tempdir().unwrap();
    let path = settings.path().join("memory.json");
    std::fs::write(&path, r#"{"enabled":true}"#).unwrap();
    let repos = Repositories::new(data.notes.clone());
    let existing = repos
        .create_memory("default", None, "Keep", "user")
        .await
        .unwrap();
    let folder = repos
        .create_folder("default", "Project", None)
        .await
        .unwrap();
    let scoped = repos
        .create_memory("default", Some(&folder.id), "Keep project", "user")
        .await
        .unwrap();
    repos
        .set_folder_memory_disabled(&folder.id, true)
        .await
        .unwrap();
    let ctx = ToolCtx {
        data: &data,
        app: None,
        binding: Some(&binding),
        broker: Some(broker.broker()),
    };
    for (name, args) in [
        ("save_memory", json!({"content":"Denied"})),
        ("forget_memory", json!({"id":existing.id})),
    ] {
        assert_eq!(
            super::memory::call_with_settings(&ctx, name, &args, &path)
                .await
                .unwrap_err()
                .code,
            "user_denied"
        );
        assert_eq!(
            super::memory::call_with_settings(
                &ToolCtx {
                    binding: Some(&unattended),
                    ..ctx
                },
                name,
                &args,
                &path
            )
            .await
            .unwrap_err()
            .code,
            "unattended_requires_approval"
        );
        assert_eq!(
            super::memory::call_with_settings(&ToolCtx::unbound(&data), name, &args, &path)
                .await
                .unwrap_err()
                .code,
            "turn_required"
        );
        assert_eq!(
            super::memory::call_with_settings(
                &ToolCtx {
                    broker: None,
                    ..ctx
                },
                name,
                &args,
                &path
            )
            .await
            .unwrap_err()
            .code,
            "approval_unavailable"
        );
    }
    for (name, args) in [
        (
            "save_memory",
            json!({"content":"Disabled","projectId":folder.id}),
        ),
        ("forget_memory", json!({"id":scoped.id})),
    ] {
        assert_eq!(
            super::memory::call_with_settings(&ctx, name, &args, &path)
                .await
                .unwrap_err()
                .code,
            "memory_disabled"
        );
    }
    std::fs::write(&path, r#"{"enabled":false}"#).unwrap();
    assert_eq!(
        super::memory::call_with_settings(
            &ctx,
            "save_memory",
            &json!({"content":"Disabled"}),
            &path
        )
        .await
        .unwrap_err()
        .code,
        "memory_disabled"
    );
    let disabled_data = McpData {
        memory_enabled: false,
        notes: data.notes.clone(),
        profile: data.profile.clone(),
        activity: ActivityAccess::Off,
        now: data.now,
    };
    assert_eq!(
        super::memory::call_with_settings(
            &ToolCtx {
                data: &disabled_data,
                ..ctx
            },
            "forget_memory",
            &json!({"id":existing.id}),
            &path
        )
        .await
        .unwrap_err()
        .code,
        "memory_disabled"
    );
    assert_eq!(
        repos
            .list_memories("default", None, true)
            .await
            .unwrap()
            .len(),
        2
    );
    assert_eq!(broker.requests().len(), 2);
}

struct DeletePause {
    armed: std::sync::atomic::AtomicBool,
    acquisitions: std::sync::atomic::AtomicUsize,
    reached: tokio::sync::Notify,
    resume: tokio::sync::Notify,
}

async fn paused_memory_pool(
    path: &std::path::Path,
    boundary: usize,
) -> (sqlx_sqlite::SqlitePool, std::sync::Arc<DeletePause>) {
    use std::sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    };
    let pause = Arc::new(DeletePause {
        armed: AtomicBool::new(false),
        acquisitions: AtomicUsize::new(0),
        reached: tokio::sync::Notify::new(),
        resume: tokio::sync::Notify::new(),
    });
    let observer = pause.clone();
    let options = sqlx_sqlite::SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .journal_mode(sqlx_sqlite::SqliteJournalMode::Wal);
    let pool = sqlx_sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .before_acquire(move |_, _| {
            let pause = observer.clone();
            Box::pin(async move {
                if pause.armed.load(Ordering::SeqCst)
                    && pause.acquisitions.fetch_add(1, Ordering::SeqCst) + 1 == boundary
                {
                    pause.reached.notify_one();
                    pause.resume.notified().await;
                }
                Ok(true)
            })
        })
        .connect_with(options)
        .await
        .unwrap();
    crate::db::migrations::run_migrations(&pool).await.unwrap();
    (pool, pause)
}

#[tokio::test]
async fn forget_memory_serializes_global_toggle_until_delete_commit() {
    use std::sync::atomic::Ordering;
    let broker = TestBroker::new([Scripted::Approve]).await;
    let binding = broker.turn(CliKind::Claude, true).await;
    let dir = tempfile::tempdir().unwrap();
    let settings = dir.path().join("memory.json");
    std::fs::write(&settings, r#"{"enabled":true}"#).unwrap();
    // Two scope reads precede acquisition of the deletion transaction.
    let (pool, pause) = paused_memory_pool(&dir.path().join("notes.sqlite3"), 3).await;
    let repos = Repositories::new(pool.clone());
    let memory = repos
        .create_memory("default", None, "Keep until approved", "user")
        .await
        .unwrap();
    let data = McpData {
        notes: pool,
        profile: "default".into(),
        memory_enabled: true,
        activity: ActivityAccess::Off,
        now: chrono::Utc::now(),
    };
    let ctx = ToolCtx {
        data: &data,
        app: None,
        binding: Some(&binding),
        broker: Some(broker.broker()),
    };
    pause.armed.store(true, Ordering::SeqCst);
    let args = json!({"id":memory.id});
    let delete = super::memory::call_with_settings(&ctx, "forget_memory", &args, &settings);
    let toggle = async {
        pause.reached.notified().await;
        let mut toggle = Box::pin(crate::commands::set_memory_enabled_with_settings(
            &settings, false,
        ));
        let blocked = futures_util::poll!(toggle.as_mut()).is_pending();
        let enabled_before_commit = crate::commands::load_memory_settings(&settings).enabled;
        pause.resume.notify_one();
        if blocked {
            toggle.await.unwrap();
        }
        (blocked, enabled_before_commit)
    };
    let (result, (blocked, enabled_before_commit)) =
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            tokio::join!(delete, toggle)
        })
        .await
        .expect("both concurrent operations finish");
    result.unwrap();
    assert!(
        blocked,
        "the settings toggle must wait for the deletion transaction"
    );
    assert!(
        enabled_before_commit,
        "memory cannot turn off before an authorized delete commits"
    );
    assert!(!crate::commands::load_memory_settings(&settings).enabled);
    assert!(repos
        .list_memories("default", None, false)
        .await
        .unwrap()
        .is_empty());
    let count: i64 =
        sqlx::query_scalar::query_scalar("SELECT COUNT(*) FROM memory_tombstones WHERE id = ?")
            .bind(&memory.id)
            .fetch_one(&data.notes)
            .await
            .unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn forget_memory_revalidates_concurrent_project_toggle_in_delete_transaction() {
    use std::sync::atomic::Ordering;
    let broker = TestBroker::new([Scripted::Approve]).await;
    let binding = broker.turn(CliKind::Codex, true).await;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("notes.sqlite3");
    let settings = dir.path().join("memory.json");
    std::fs::write(&settings, r#"{"enabled":true}"#).unwrap();
    // Each scoped check reads the memory and its project: deletion acquires fifth.
    let (pool, pause) = paused_memory_pool(&path, 5).await;
    let repos = Repositories::new(pool.clone());
    let project = repos
        .create_folder("default", "Project", None)
        .await
        .unwrap();
    let memory = repos
        .create_memory("default", Some(&project.id), "Preserve this fact", "user")
        .await
        .unwrap();
    let writer = sqlx_sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            sqlx_sqlite::SqliteConnectOptions::new()
                .filename(&path)
                .journal_mode(sqlx_sqlite::SqliteJournalMode::Wal),
        )
        .await
        .unwrap();
    let writer = Repositories::new(writer);
    let data = McpData {
        notes: pool,
        profile: "default".into(),
        memory_enabled: true,
        activity: ActivityAccess::Off,
        now: chrono::Utc::now(),
    };
    let ctx = ToolCtx {
        data: &data,
        app: None,
        binding: Some(&binding),
        broker: Some(broker.broker()),
    };
    pause.armed.store(true, Ordering::SeqCst);
    let args = json!({"id":memory.id});
    let delete = super::memory::call_with_settings(&ctx, "forget_memory", &args, &settings);
    let toggle = async {
        pause.reached.notified().await;
        writer
            .set_folder_memory_disabled(&project.id, true)
            .await
            .unwrap();
        pause.resume.notify_one();
    };
    let (result, ()) = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        tokio::join!(delete, toggle)
    })
    .await
    .expect("both concurrent operations finish");
    assert_eq!(result.unwrap_err().code, "memory_disabled");
    assert_eq!(
        repos
            .list_memories("default", Some(&project.id), false)
            .await
            .unwrap()[0]
            .id,
        memory.id
    );
    let count: i64 =
        sqlx::query_scalar::query_scalar("SELECT COUNT(*) FROM memory_tombstones WHERE id = ?")
            .bind(&memory.id)
            .fetch_one(&data.notes)
            .await
            .unwrap();
    assert_eq!(count, 0);
    // Explicit user deletion retains its existing ability to remove disabled memories.
    repos.delete_memory("default", &memory.id).await.unwrap();
    assert!(repos
        .list_memories("default", Some(&project.id), false)
        .await
        .unwrap()
        .is_empty());
}
