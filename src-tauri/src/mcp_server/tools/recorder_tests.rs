//! Real persistence and broker correlation tests. No audio capture is started.
use super::recorder::{self, RecorderTools};
use super::{ActivityAccess, ToolCtx, ToolGroup};
use crate::approval_broker::testing::TestBroker;
use crate::db::repositories::Repositories;
use crate::domain::types::RecordingSourceMode;
use crate::llm::cli::CliKind;
use serde_json::json;

#[tokio::test]
async fn external_recorder_discard_uses_the_visible_bridge_and_the_approved_saved_take() {
    use crate::agent_recorder::{
        AgentRecorderAction, AgentRecorderBroker, AgentRecorderTarget, RecordingRecoveryAction,
        ResolveAgentRecorderRequest,
    };
    use crate::approval_broker::external::{one_pending, ExternalApprovalBroker};
    use crate::domain::types::AppError;
    use crate::mcp_server::channel::ExternalConnection;
    use futures_util::future::BoxFuture;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    struct VisibleRecorder {
        broker: AgentRecorderBroker,
        repositories: Repositories,
        paths: crate::app_paths::AppPaths,
        dispatched: AtomicUsize,
    }
    impl recorder::RecorderPort for VisibleRecorder {
        fn request(
            &self,
            action: AgentRecorderAction,
            source_mode: Option<RecordingSourceMode>,
            target: AgentRecorderTarget,
        ) -> BoxFuture<'_, Result<serde_json::Value, AppError>> {
            Box::pin(async move {
                let (emit, payload) = tokio::sync::oneshot::channel();
                let delivered = self
                    .broker
                    .request_with(action, source_mode, target, |value| {
                        self.dispatched.fetch_add(1, Ordering::SeqCst);
                        emit.send(value).unwrap();
                        Ok(())
                    });
                let handle = async {
                    let payload = payload.await.unwrap();
                    let request = payload["requestId"].as_str().unwrap();
                    let profile = payload["profile"].as_str().unwrap();
                    let note = payload["noteId"].as_str().unwrap();
                    let take = payload["sessionId"].as_str().unwrap();
                    assert!(payload.get("authority").is_none());
                    assert_eq!(payload["recoveryAction"], "discard");
                    let authorize = || {
                        self.broker.saved_authorization(
                            request,
                            profile,
                            note,
                            take,
                            AgentRecorderAction::Recover,
                            Some(RecordingRecoveryAction::Discard),
                        )
                    };
                    assert!(matches!(
                        authorize().unwrap(),
                        crate::agent_recorder::RecorderAuthority::External { .. }
                    ));
                    let info = self
                        .repositories
                        .recording_recovery_info(take)
                        .await
                        .unwrap()
                        .unwrap();
                    crate::commands::discard_recoverable_recording(
                        &self.repositories,
                        &self.paths,
                        &info,
                        Some(profile),
                        async { authorize().map(Some) },
                    )
                    .await
                    .unwrap();
                    self.broker
                        .resolve(
                            serde_json::from_value::<ResolveAgentRecorderRequest>(json!({
                                "requestId":request,"ok":true,"noteId":note,"sessionId":take,
                            }))
                            .unwrap(),
                        )
                        .unwrap();
                };
                tokio::join!(delivered, handle).0
            })
        }
    }

    let root = tempfile::tempdir().unwrap();
    let paths = crate::app_paths::AppPaths::from_data_dir(root.path().to_path_buf()).unwrap();
    let data = crate::mcp_server::tests::data(
        crate::mcp_server::tests::notes_pool().await,
        ActivityAccess::Off,
    );
    let repositories = Repositories::new(data.notes.clone());
    let note = repositories.create_note(&data.profile, None).await.unwrap();
    let take = "synthetic-recovery-take";
    let recording = paths.recording_session_dir(&note.id, take).unwrap();
    let partial = recording.join("fixture.part");
    let final_path = recording.join("fixture.wav");
    std::fs::create_dir_all(&recording).unwrap();
    std::fs::write(&partial, b"synthetic partial fixture").unwrap();
    repositories
        .create_recording_session(
            &note.id,
            take,
            RecordingSourceMode::MicrophoneOnly,
            &partial.to_string_lossy(),
            &final_path.to_string_lossy(),
            None,
        )
        .await
        .unwrap();
    repositories
        .mark_recording_recoverable(take, &note.id)
        .await
        .unwrap();
    let visible = VisibleRecorder {
        broker: AgentRecorderBroker::default(),
        repositories,
        paths,
        dispatched: AtomicUsize::new(0),
    };
    let broker = ExternalApprovalBroker::new(
        {
            let profile = data.profile.clone();
            move || Some(profile.clone())
        },
        || {},
        Duration::from_secs(10),
    );
    let (peer, _closed, cancelled) = ExternalConnection::test_connection();
    let ctx = ToolCtx::unbound(&data);
    let args = json!({"sessionId":take, "action":"discard"});
    assert_eq!(recorder::external_tool_schemas(&ctx, true).len(), 8);
    assert_eq!(recorder::external_descriptors(&ctx).len(), 2);
    let handle = |decision| {
        let (broker, visible) = (&broker, &visible);
        async move {
            let pending = one_pending(broker).await;
            assert_eq!(visible.dispatched.load(Ordering::SeqCst), 0);
            assert!(pending["preview"]
                .as_str()
                .unwrap()
                .contains("Permanently discard"));
            broker
                .decide(pending["id"].as_str().unwrap(), decision)
                .unwrap();
        }
    };
    let scope = broker.scope(&peer, &data.profile, Some(root.path()));
    assert_eq!(
        recorder::call_external_with(
            &ctx,
            &scope,
            Some(&visible),
            "recover_recording",
            &json!({"sessionId":take, "action":"discard", "profile":"work"}),
        )
        .await
        .unwrap_err()
        .code,
        "agent_tool_arguments_invalid",
    );
    assert_eq!(visible.dispatched.load(Ordering::SeqCst), 0);
    assert!(broker.pending().is_empty());
    let (denied, ()) = tokio::join!(
        recorder::call_external_with(&ctx, &scope, Some(&visible), "recover_recording", &args),
        handle(false),
    );
    assert_eq!(denied.unwrap_err().code, "user_denied");
    assert_eq!(visible.dispatched.load(Ordering::SeqCst), 0);
    assert!(partial.is_file());
    let cancel = async {
        let pending = one_pending(&broker).await;
        cancelled.send(true).unwrap();
        assert!(broker
            .decide(pending["id"].as_str().unwrap(), true)
            .is_err());
    };
    let (cancelled, ()) = tokio::join!(
        recorder::call_external_with(&ctx, &scope, Some(&visible), "recover_recording", &args),
        cancel,
    );
    assert_eq!(cancelled.unwrap_err().code, "mcp_external_cancelled");
    assert_eq!(visible.dispatched.load(Ordering::SeqCst), 0);
    assert!(partial.is_file());

    let (peer, _closed, _cancelled) = ExternalConnection::test_connection();
    let scope = broker.scope(&peer, &data.profile, Some(root.path()));
    let (approved, ()) = tokio::join!(
        recorder::call_external_with(&ctx, &scope, Some(&visible), "recover_recording", &args),
        handle(true),
    );
    assert_eq!(approved.unwrap()["sessionId"], take);
    assert_eq!(visible.dispatched.load(Ordering::SeqCst), 1);
    assert!(!partial.exists());
    let state: String =
        sqlx::query_scalar::query_scalar("SELECT status FROM recording_sessions WHERE id=?")
            .bind(take)
            .fetch_one(&data.notes)
            .await
            .unwrap();
    assert_eq!(state, "failed");
}

#[tokio::test]
async fn artifact_inventory_retains_note_session_sources_and_duration_without_paths() {
    let broker = TestBroker::new([]).await;
    let binding = broker.turn(CliKind::Claude, true).await;
    let data = crate::mcp_server::tests::data(broker.pool().clone(), ActivityAccess::Off);
    let repos = Repositories::new(data.notes.clone());
    let note = repos.create_note(&data.profile, None).await.unwrap();
    repos
        .create_recording_session(
            &note.id,
            "take-one",
            RecordingSourceMode::MicrophonePlusSystem,
            "/private/synthetic-mic.part",
            "/private/synthetic-mic.wav",
            None,
        )
        .await
        .unwrap();
    repos
        .create_pending_source_artifact(
            &note.id,
            "take-one",
            "microphone",
            "/private/synthetic-mic.part",
            "/private/synthetic-mic.wav",
        )
        .await
        .unwrap();
    repos
        .create_pending_source_artifact(
            &note.id,
            "take-one",
            "system",
            "/private/synthetic-system.part",
            "/private/synthetic-system.wav",
        )
        .await
        .unwrap();
    sqlx::query::query("UPDATE audio_artifacts SET duration_ms=1234,size_bytes=990,status='valid' WHERE recording_session_id='take-one'").execute(&data.notes).await.unwrap();
    let ctx = ToolCtx {
        data: &data,
        app: None,
        binding: Some(&binding),
        broker: Some(broker.broker()),
    };
    let result = RecorderTools
        .call(
            &ctx,
            "list_recording_artifacts",
            &json!({"sessionId":"take-one"}),
        )
        .await
        .unwrap();
    assert_eq!(result["sessionId"], "take-one");
    assert_eq!(result["sourceMode"], "microphonePlusSystem");
    assert_eq!(result["noteId"], note.id);
    assert_eq!(result["artifacts"][0]["source"], "microphone");
    assert_eq!(result["artifacts"][1]["source"], "system");
    assert_eq!(result["artifacts"][0]["durationMs"], 1234);
    assert!(!result.to_string().contains("/private/"));
    sqlx::query::query("UPDATE notes SET profile='other-partition' WHERE id=?")
        .bind(&note.id)
        .execute(&data.notes)
        .await
        .unwrap();
    assert_eq!(
        RecorderTools
            .call(
                &ctx,
                "list_recording_artifacts",
                &json!({"sessionId":"take-one"})
            )
            .await
            .unwrap_err()
            .code,
        "recording_not_found"
    );
    assert!(broker.requests().is_empty());
}

#[tokio::test]
async fn recovery_rejects_invalid_action_or_other_note_before_approval() {
    let broker = TestBroker::new([]).await;
    let binding = broker.turn(CliKind::Claude, true).await;
    let data = crate::mcp_server::tests::data(broker.pool().clone(), ActivityAccess::Off);
    let repos = Repositories::new(data.notes.clone());
    let note = repos.create_note(&data.profile, None).await.unwrap();
    repos
        .create_recording_session(
            &note.id,
            "take-recover",
            RecordingSourceMode::MicrophoneOnly,
            "fixture.part",
            "fixture.wav",
            None,
        )
        .await
        .unwrap();
    let ctx = ToolCtx {
        data: &data,
        app: None,
        binding: Some(&binding),
        broker: Some(broker.broker()),
    };
    assert_eq!(
        recorder::call(
            &ctx,
            None,
            "recover_recording",
            &json!({"sessionId":"take-recover","action":"erase_everything"})
        )
        .await
        .unwrap_err()
        .code,
        "agent_tool_arguments_invalid"
    );
    assert_eq!(
        recorder::call(
            &ctx,
            None,
            "retry_note_processing",
            &json!({"sessionId":"take-recover","noteId":"other-note"})
        )
        .await
        .unwrap_err()
        .code,
        "recording_target_mismatch"
    );
    assert!(broker.requests().is_empty());
}

#[tokio::test]
async fn status_serializes_the_exact_authorized_snapshot_after_database_wait() {
    let broker = TestBroker::new([]).await;
    let binding = broker.turn(CliKind::Claude, true).await;
    let data = crate::mcp_server::tests::data(broker.pool().clone(), ActivityAccess::Off);
    let repos = Repositories::new(data.notes.clone());
    let note = repos.create_note(&data.profile, None).await.unwrap();
    repos
        .create_recording_session(
            &note.id,
            "snapshot-take",
            RecordingSourceMode::MicrophoneOnly,
            "fixture.part",
            "fixture.wav",
            None,
        )
        .await
        .unwrap();
    let ctx = ToolCtx {
        data: &data,
        app: None,
        binding: Some(&binding),
        broker: Some(broker.broker()),
    };
    let status=serde_json::from_value(json!({"sessionId":"snapshot-take","noteId":note.id,"sourceMode":"microphoneOnly","state":"paused","elapsedMs":4123,"level":{"peak":0,"rms":0,"recentPeaks":[]},"silenceWarning":false,"bytesWritten":100})).unwrap();
    let result = recorder::recording_status_snapshot(&ctx, Some(status))
        .await
        .unwrap();
    assert_eq!(result["sessionId"], "snapshot-take");
    assert_eq!(result["elapsedMs"], 4123);
    assert_eq!(result["state"], "paused");
}
