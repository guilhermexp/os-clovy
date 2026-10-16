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
