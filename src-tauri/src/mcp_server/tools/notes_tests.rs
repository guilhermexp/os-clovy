use super::{call, ActivityAccess, ToolCtx};
use crate::approval_broker::testing::TestBroker;
use crate::llm::cli::CliKind;
use crate::mcp_server::tests::{data, notes_pool};
use serde_json::json;
use sqlx::query::query;

#[tokio::test]
async fn note_content_pages_round_trip_multibyte_and_preserve_transcript_contract() {
    let pool = notes_pool().await;
    let text = "ação🙂漢字".repeat(12000);
    query("UPDATE notes SET edited_content = ? WHERE id = 'note-roadmap'")
        .bind(&text)
        .execute(&pool)
        .await
        .unwrap();
    let data = data(pool, ActivityAccess::Off);
    let ctx = ToolCtx::unbound(&data);
    let mut offset = 0;
    let mut joined = String::new();
    loop {
        let page = call(
            &ctx,
            "get_note",
            &json!({"id":"note-roadmap", "contentOffset":offset}),
        )
        .await
        .unwrap();
        assert_eq!(page["contentOffset"], offset);
        assert_eq!(page["contentTotalChars"], text.chars().count());
        let content = page["content"].as_str().unwrap();
        assert!(content.chars().count() <= 20000);
        joined.push_str(content);
        let Some(next) = page["nextContentOffset"].as_u64() else {
            break;
        };
        assert!(next > offset);
        offset = next;
    }
    assert_eq!(joined, text);
    let empty = call(
        &ctx,
        "get_note",
        &json!({"id":"note-roadmap","contentOffset":u64::MAX,"transcriptOffset":0}),
    )
    .await
    .unwrap();
    assert_eq!(empty["content"], "");
    assert!(empty["nextContentOffset"].is_null());
    assert!(empty["transcriptTotalChars"].as_u64().unwrap() > 0);
}

#[tokio::test]
async fn dictation_pages_round_trip_and_enforce_retention_and_partition() {
    let broker = TestBroker::new([]).await;
    let binding = broker.turn(CliKind::Codex, true).await;
    let pool = notes_pool().await;
    let text = "ação🙂漢字".repeat(12000);
    query("UPDATE dictation_history SET text = ? WHERE id = 'dictation-1'")
        .bind(&text)
        .execute(&pool)
        .await
        .unwrap();
    query("UPDATE dictation_history SET created_at = ? WHERE id = 'dictation-2'")
        .bind((chrono::Utc::now() - chrono::Duration::days(8)).to_rfc3339())
        .execute(&pool)
        .await
        .unwrap();
    let data = data(pool, ActivityAccess::Off);
    let ctx = ToolCtx {
        binding: Some(&binding),
        ..ToolCtx::unbound(&data)
    };
    let mut offset = 0;
    let mut joined = String::new();
    loop {
        let page = call(
            &ctx,
            "get_dictation",
            &json!({"id":"dictation-1","textOffset":offset}),
        )
        .await
        .unwrap();
        assert_eq!(page["textOffset"], offset);
        assert_eq!(page["textTotalChars"], text.chars().count());
        joined.push_str(page["text"].as_str().unwrap());
        let Some(next) = page["nextTextOffset"].as_u64() else {
            break;
        };
        assert!(next > offset);
        offset = next;
    }
    assert_eq!(joined, text);
    for id in ["dictation-2", "dictation-3", "missing"] {
        assert_eq!(
            call(&ctx, "get_dictation", &json!({"id":id}))
                .await
                .unwrap_err()
                .code,
            "dictation_not_found"
        );
    }
    assert_eq!(
        call(
            &ToolCtx::unbound(&data),
            "get_dictation",
            &json!({"id":"dictation-1"})
        )
        .await
        .unwrap_err()
        .code,
        "turn_required"
    );
    assert!(broker.requests().is_empty());
}

async fn routine_dictation_fixture() -> (super::McpData, crate::mcp_server::turns::TurnBinding) {
    let broker = TestBroker::new([]).await;
    let binding = broker.turn(CliKind::Claude, false).await;
    let pool = notes_pool().await;
    let now = "2026-10-28T00:00:00Z";
    query("INSERT INTO routines (id, name, prompt, schedule, created_at, updated_at, metadata_json, tool_catalog_version) VALUES ('dictation-routine', 'Daily', 'Summarise', 'every 1d', ?, ?, '{\"enabledToolsets\":[]}', 1)")
        .bind(now).bind(now).execute(&pool).await.unwrap();
    query("INSERT INTO routine_runs (id, routine_id, agent_session_id, claim_token, trigger_kind, status, model, safety_mode, created_at, updated_at) VALUES ('dictation-routine-run', 'dictation-routine', ?, 'claim', 'schedule', 'running', 'claude', 'sandboxed', ?, ?)")
        .bind(&binding.session_id).bind(now).bind(now).execute(&pool).await.unwrap();
    (data(pool, ActivityAccess::Off), binding)
}

#[tokio::test]
async fn routine_dictation_catalog_requires_an_enabled_context_toolset() {
    let (data, binding) = routine_dictation_fixture().await;
    let ctx = ToolCtx {
        binding: Some(&binding),
        ..ToolCtx::unbound(&data)
    };
    assert!(!super::descriptors(&ctx)
        .await
        .iter()
        .any(|tool| tool["name"] == "get_dictation"));
    query("UPDATE routines SET metadata_json = '{\"enabledToolsets\":[\"context_engine\"]}' WHERE id = 'dictation-routine'")
        .execute(&data.notes).await.unwrap();
    assert!(super::descriptors(&ctx)
        .await
        .iter()
        .any(|tool| tool["name"] == "get_dictation"));
}

#[tokio::test]
async fn routine_dictation_dispatch_checks_permission_and_rechecks_after_revocation() {
    let (data, binding) = routine_dictation_fixture().await;
    let ctx = ToolCtx {
        binding: Some(&binding),
        ..ToolCtx::unbound(&data)
    };
    let args = json!({"id":"dictation-1"});
    assert_eq!(
        call(&ctx, "get_dictation", &args).await.unwrap_err().code,
        "routine_tool_not_enabled"
    );
    query("UPDATE routines SET metadata_json = '{\"enabledToolsets\":[\"context_engine\"]}' WHERE id = 'dictation-routine'")
        .execute(&data.notes).await.unwrap();
    assert_eq!(
        call(&ctx, "get_dictation", &args).await.unwrap()["id"],
        "dictation-1"
    );
    query("UPDATE routines SET metadata_json = '{\"enabledToolsets\":[]}' WHERE id = 'dictation-routine'")
        .execute(&data.notes).await.unwrap();
    assert_eq!(
        call(&ctx, "get_dictation", &args).await.unwrap_err().code,
        "routine_tool_not_enabled"
    );
    assert!(!super::descriptors(&ctx)
        .await
        .iter()
        .any(|tool| tool["name"] == "get_dictation"));
}
