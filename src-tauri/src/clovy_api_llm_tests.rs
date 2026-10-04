//! Note generation through the user's own CLI providers (`crate::llm`),
//! exercised through `clovy_api::generate_note_from_transcript`. Kept in its
//! own file so the routing code in `clovy_api.rs` can change without touching
//! the test source.

use super::*;
use crate::db::repositories::Repositories;
use crate::llm::{
    cli::CliKind,
    registry::ProviderRef,
    shell_env::{set_login_env_for_tests, LoginEnv},
};
use std::{collections::BTreeMap, os::unix::fs::PermissionsExt};

/// Restores the process-wide settings and login environment on drop.
struct Restore;

impl Drop for Restore {
    fn drop(&mut self) {
        crate::providers::replace_current_settings_for_tests(
            crate::providers::default_settings_for_tests(),
        );
        set_login_env_for_tests(None);
    }
}

#[tokio::test]
async fn llm_note_generated_by_fake_cli_is_persisted_like_a_clovy_note() {
    let _lock = crate::providers::GLOBAL_SETTINGS_TEST_LOCK.lock().await;
    let _restore = Restore;
    let bin = tempfile::tempdir().unwrap();
    let stdin_log = bin.path().join("stdin.txt");
    let claude = bin.path().join("claude");
    // A fake `claude -p` that records its prompt and answers with a note.
    std::fs::write(
        &claude,
        format!(
            "#!/bin/sh\ncat > '{}'\necho '{{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":false,\"result\":\"## Summary\\\\n- Ship the updater fix\"}}'\n",
            stdin_log.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&claude, std::fs::Permissions::from_mode(0o755)).unwrap();
    set_login_env_for_tests(Some(LoginEnv::from_vars(BTreeMap::from([(
        "PATH".to_string(),
        format!("{}:/usr/bin:/bin", bin.path().display()),
    )]))));
    let mut settings = crate::providers::default_settings_for_tests();
    settings.llm_usage.notes = ProviderRef::Cli {
        id: CliKind::Claude,
    };
    crate::providers::replace_current_settings_for_tests(settings);

    let generated = generate_note_from_transcript(GenerationRequest {
        provider: crate::providers::generation_provider(),
        operation_id: Some("note-1".to_string()),
        title: "Weekly sync".to_string(),
        existing_generated_note: None,
        transcript: "We will ship the updater fix this week.".to_string(),
        transcript_source_labels: false,
        manual_notes: None,
        language: Some("en".to_string()),
    })
    .await
    .unwrap();
    assert_eq!(generated.content, "## Summary\n- Ship the updater fix");
    assert_eq!(generated.provider, "cli:claude");
    let prompt = std::fs::read_to_string(&stdin_log).unwrap();
    assert!(prompt.starts_with(crate::llm::CLOVY_AUTHORSHIP_MARKER));
    assert!(prompt.contains("We will ship the updater fix this week."));

    // Persist exactly as the processing pipeline does and read it back.
    let pool = sqlx_sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    crate::db::migrations::run_migrations(&pool).await.unwrap();
    let repos = Repositories::new(pool);
    let note = repos.create_note("default", None).await.unwrap();
    repos
        .create_recording_session(
            &note.id,
            "session-1",
            crate::domain::types::RecordingSourceMode::MicrophoneOnly,
            "microphone.partial.wav",
            "microphone.wav",
            None,
        )
        .await
        .unwrap();
    let artifact = repos
        .create_audio_artifact(&note.id, "session-1", "microphone.wav", 1_000, 10, "sum")
        .await
        .unwrap();
    let transcript = repos
        .create_transcript(
            &note.id,
            &artifact.id,
            "We will ship the updater fix this week.",
            Some("en".to_string()),
            "venice",
        )
        .await
        .unwrap();
    let result_id = repos
        .create_generation_result(
            &note.id,
            &transcript.id,
            &generated.content,
            generated.title_suggestion.clone(),
            &generated.provider,
            &generated.prompt_version,
        )
        .await
        .unwrap();
    repos
        .set_generated_note_for_session(
            &note.id,
            None,
            Some(&result_id),
            generated.title_suggestion,
            generated.content,
        )
        .await
        .unwrap();
    let stored = repos.get_note(&note.id).await.unwrap();
    assert_eq!(
        stored.generated_content.as_deref(),
        Some("## Summary\n- Ship the updater fix")
    );
}

/// Generates a note with the real `claude` CLI found through the real
/// login shell (no fakes). Needs `claude` installed and signed in.
#[tokio::test]
#[ignore = "requires an installed, signed-in claude CLI"]
async fn live_claude_cli_generates_a_note() {
    let _lock = crate::providers::GLOBAL_SETTINGS_TEST_LOCK.lock().await;
    let _restore = Restore;
    set_login_env_for_tests(None);
    let mut settings = crate::providers::default_settings_for_tests();
    settings.llm_usage.notes = ProviderRef::Cli {
        id: CliKind::Claude,
    };
    crate::providers::replace_current_settings_for_tests(settings);
    let generated = generate_note_from_transcript(GenerationRequest {
        provider: crate::providers::generation_provider(),
        operation_id: Some("live-claude-note".to_string()),
        title: "Weekly sync".to_string(),
        existing_generated_note: None,
        transcript: "Weekly sync. We agreed to ship the updater fix on Thursday. Ana will review the onboarding metrics next Monday.".to_string(),
        transcript_source_labels: false,
        manual_notes: None,
        language: Some("en".to_string()),
    })
    .await
    .expect("claude should generate the note");
    assert_eq!(generated.provider, "cli:claude");
    assert!(generated.content.to_lowercase().contains("updater"));
    eprintln!("live claude note:\n{}\n", generated.content);
}
