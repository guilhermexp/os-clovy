use super::*;

async fn fixture() -> (std::path::PathBuf, SqlitePool) {
    let file = std::env::temp_dir().join(format!("clovy-terminal-{}.sqlite3", Uuid::new_v4()));
    let pool = sqlx_sqlite::SqlitePoolOptions::new()
        .max_connections(3)
        .connect_with(
            sqlx_sqlite::SqliteConnectOptions::new()
                .filename(&file)
                .create_if_missing(true),
        )
        .await
        .unwrap();
    crate::db::migrations::run_migrations(&pool).await.unwrap();
    query("INSERT INTO agent_sessions(id,title,model,status,created_at,updated_at) VALUES ('session','Synthetic','test','running','now','now')").execute(&pool).await.unwrap();
    query("INSERT INTO agent_runs(id,session_id,model,status,started_at,updated_at,run_config_json) VALUES ('run','session','test','running','now','now','{\"engine\":\"claude\"}')").execute(&pool).await.unwrap();
    (file, pool)
}

fn frame(method: &str) -> RpcFrame {
    RpcFrame {
        jsonrpc: "2.0".into(),
        protocol_version: 1,
        session_id: "session".into(),
        run_id: "run".into(),
        sequence: 12,
        id: None,
        event_id: Some(Uuid::new_v4().to_string()),
        method: Some(method.into()),
        params: Some(
            json!({"error":"Loser failure","code":"agent_fixture","category":"tool","retryable":false}),
        ),
        result: None,
        error: None,
    }
}

async fn cleanup(file: std::path::PathBuf, pool: SqlitePool) {
    pool.close().await;
    std::fs::remove_file(file).unwrap();
}

#[tokio::test]
async fn stop_winner_replaces_late_success_or_failure_with_one_correlated_cancel() {
    for proposed in ["run.completed", "run.failed"] {
        let (file, pool) = fixture().await;
        let (changed, owned) = crate::agent_sessions::api::claim_ui_run_cancellation_with_signal(
            &pool,
            "session",
            "run",
            || true,
        )
        .await
        .unwrap();
        assert!(changed && owned);
        let proposal = frame(proposed);
        let event = persist_terminal_event(&pool, &proposal, super::super::events::sanitize_log)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(event.method, "run.cancelled");
        assert_eq!(event.event_id, proposal.event_id.unwrap());
        assert_eq!(event.sequence, 12);
        assert!(event.data.get("message").is_none());
        assert!(persist_terminal_event(
            &pool,
            &frame("run.cancelled"),
            super::super::events::sanitize_log
        )
        .await
        .unwrap()
        .is_none());
        let row = query("SELECT status,json_extract(run_config_json,'$.engine') AS engine FROM agent_runs WHERE id='run'").fetch_one(&pool).await.unwrap();
        assert_eq!(row.get::<String, _>("status"), "cancelled");
        assert_eq!(row.get::<String, _>("engine"), "claude");
        assert_eq!(
            query("SELECT COUNT(*) AS n FROM agent_items WHERE kind='error'")
                .fetch_one(&pool)
                .await
                .unwrap()
                .get::<i64, _>("n"),
            0
        );
        cleanup(file, pool).await;
    }
}

#[tokio::test]
async fn completion_winner_survives_late_stop_and_duplicate_publishers() {
    let (file, pool) = fixture().await;
    let completion = frame("run.completed");
    let event = persist_terminal_event(&pool, &completion, super::super::events::sanitize_log)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(event.method, "run.completed");
    let (changed, owned) = crate::agent_sessions::api::claim_ui_run_cancellation_with_signal(
        &pool,
        "session",
        "run",
        || panic!("late Stop must not signal completed run"),
    )
    .await
    .unwrap();
    assert!(!changed && !owned);
    assert!(persist_terminal_event(
        &pool,
        &frame("run.cancelled"),
        super::super::events::sanitize_log
    )
    .await
    .unwrap()
    .is_none());
    assert_eq!(
        query("SELECT status FROM agent_runs WHERE id='run'")
            .fetch_one(&pool)
            .await
            .unwrap()
            .get::<String, _>("status"),
        "completed"
    );
    cleanup(file, pool).await;
}

#[tokio::test]
async fn retry_copied_config_does_not_copy_the_previous_runs_publication_claim() {
    let (file, pool) = fixture().await;
    // Retry deliberately copies the previous run config. Its private terminal
    // claim belongs to that previous run and cannot suppress this run's event.
    query("UPDATE agent_runs SET run_config_json=json_set(run_config_json,'$.clovyTerminalEvent',json('{\"runId\":\"previous-run\",\"eventId\":\"previous-event\",\"method\":\"run.completed\",\"sequence\":99}')) WHERE id='run'")
        .execute(&pool).await.unwrap();
    let proposal = frame("run.completed");
    let event = persist_terminal_event(&pool, &proposal, super::super::events::sanitize_log)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(event.method, "run.completed");
    assert_eq!(event.event_id, proposal.event_id.unwrap());
    assert!(persist_terminal_event(
        &pool,
        &frame("run.completed"),
        super::super::events::sanitize_log
    )
    .await
    .unwrap()
    .is_none());
    assert_eq!(query("SELECT json_extract(run_config_json,'$.clovyTerminalEvent.runId') AS owner FROM agent_runs WHERE id='run'")
        .fetch_one(&pool).await.unwrap().get::<String,_>("owner"), "run");
    cleanup(file, pool).await;
}

#[tokio::test]
async fn terminal_publishers_share_one_writer_claim_and_preserve_another_live_run() {
    let (file, pool) = fixture().await;
    query("INSERT INTO agent_runs(id,session_id,model,status,started_at,updated_at) VALUES ('other','session','test','running','now','now')").execute(&pool).await.unwrap();
    let a = frame("run.completed");
    let b = frame("run.cancelled");
    let (a, b) = tokio::join!(
        persist_terminal_event(&pool, &a, super::super::events::sanitize_log),
        persist_terminal_event(&pool, &b, super::super::events::sanitize_log),
    );
    assert_eq!(
        usize::from(a.unwrap().is_some()) + usize::from(b.unwrap().is_some()),
        1
    );
    assert_eq!(
        query("SELECT status FROM agent_sessions WHERE id='session'")
            .fetch_one(&pool)
            .await
            .unwrap()
            .get::<String, _>("status"),
        "running"
    );
    assert_eq!(
        query("SELECT status FROM agent_runs WHERE id='other'")
            .fetch_one(&pool)
            .await
            .unwrap()
            .get::<String, _>("status"),
        "running"
    );
    cleanup(file, pool).await;
}

#[tokio::test]
async fn persisted_failure_winner_keeps_its_error_and_single_durable_card() {
    let (file, pool) = fixture().await;
    query("UPDATE agent_runs SET status='failed', completed_at='earlier',error_code='agent_original',error_message='Original password=synthetic' WHERE id='run'").execute(&pool).await.unwrap();
    let event = persist_terminal_event(
        &pool,
        &frame("run.completed"),
        super::super::events::sanitize_log,
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(event.method, "run.failed");
    assert_eq!(event.data["code"], "agent_original");
    assert_eq!(event.data["message"], "Original password=[redacted]");
    assert_eq!(event.data["completedAt"], "earlier");
    assert!(persist_terminal_event(
        &pool,
        &frame("run.failed"),
        super::super::events::sanitize_log
    )
    .await
    .unwrap()
    .is_none());
    let rows = query("SELECT payload_json,external_id FROM agent_items WHERE kind='error'")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(
        serde_json::from_str::<Value>(&rows[0].get::<String, _>("payload_json")).unwrap(),
        event.data
    );
    assert_eq!(rows[0].get::<String, _>("external_id"), event.event_id);
    cleanup(file, pool).await;
}
