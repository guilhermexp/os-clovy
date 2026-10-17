//! Where dictation's calls go (ditado-rapido D2, D4): straight to Groq and
//! OpenRouter with the user's own key, through Clovy API without one. Fake
//! servers stand in for all three hosts and record every request.

use super::*;
use crate::llm::endpoint::test_server::{self, FakeServer};
use std::sync::Arc;

const CLOVY_TOKEN: &str = "clovy-access-token";

struct FixedTokens;

impl DictationTokens for FixedTokens {
    async fn access(&self) -> Result<String, AppError> {
        Ok(CLOVY_TOKEN.to_string())
    }

    async fn refresh(&self) -> Result<String, AppError> {
        Ok(CLOVY_TOKEN.to_string())
    }
}

struct Hosts {
    groq: FakeServer,
    openrouter: FakeServer,
    clovy: FakeServer,
    client: reqwest::Client,
}

impl Hosts {
    async fn start() -> Self {
        let groq = test_server::start(Arc::new(|_: &serde_json::Value| {
            (
                200,
                serde_json::json!({ "text": "send it to samir", "language": "en" }),
            )
        }))
        .await;
        let openrouter = test_server::start(Arc::new(|_: &serde_json::Value| {
            (
                200,
                test_server::chat_reply("<asr_transcript>Send it to Samir.</asr_transcript>"),
            )
        }))
        .await;
        let clovy = test_server::start(Arc::new(|body: &serde_json::Value| {
            // Multipart bodies arrive as null; JSON bodies are cleanup.
            let data = if body.is_null() {
                serde_json::json!({ "text": "via clovy", "language": null, "provider": "groq" })
            } else {
                serde_json::json!({ "text": "Cleaned via Clovy." })
            };
            (200, serde_json::json!({ "success": true, "data": data }))
        }))
        .await;
        Self {
            groq,
            openrouter,
            clovy,
            client: reqwest::Client::new(),
        }
    }

    fn transport(&self) -> DictationTransport<'_, FixedTokens> {
        DictationTransport {
            clovy_client: &self.client,
            direct_client: &self.client,
            // FakeServer bases end in `/v1`; Clovy API paths carry their own.
            clovy_base: self.clovy.base_url.trim_end_matches("/v1").to_string(),
            groq_base: self.groq.base_url.clone(),
            openrouter_base: self.openrouter.base_url.clone(),
            tokens: FixedTokens,
        }
    }
}

fn heads(server: &FakeServer) -> Vec<String> {
    server.heads.lock().unwrap().clone()
}

fn header<'a>(head: &'a str, name: &str) -> Option<&'a str> {
    head.lines().skip(1).find_map(|line| {
        let (key, value) = line.split_once(':')?;
        key.trim()
            .eq_ignore_ascii_case(name)
            .then_some(value.trim())
    })
}

/// The provider-key headers Clovy API reads; a direct request carries none.
fn assert_no_forwarded_keys(head: &str) {
    for name in [
        GROQ_API_KEY_HEADER,
        OPENROUTER_API_KEY_HEADER,
        FAL_API_KEY_HEADER,
        TAVILY_API_KEY_HEADER,
    ] {
        assert_eq!(header(head, name), None, "{name} in {head}");
    }
}

fn take_request(directory: &tempfile::TempDir) -> DictateTranscribeRequest {
    let audio_path = directory.path().join("take.m4a");
    std::fs::write(&audio_path, b"fake aac bytes").unwrap();
    DictateTranscribeRequest {
        audio_path,
        context: Some("Samir".to_string()),
        language: Some("pt".to_string()),
        session_id: "session".to_string(),
        utterance_id: "utterance".to_string(),
    }
}

fn cleanup_params() -> DictateCleanupRequestParams {
    DictateCleanupRequestParams {
        text: "send it to samir".to_string(),
        dictionary_context: Some("Samir".to_string()),
        app_context: None,
        style: "Writing style: standard.".to_string(),
        session_id: "session".to_string(),
        utterance_id: "utterance".to_string(),
    }
}

#[tokio::test]
async fn dictation_with_a_groq_key_goes_straight_to_groq_with_only_that_key() {
    let hosts = Hosts::start().await;
    let directory = tempfile::tempdir().unwrap();

    let result = dictate_transcribe_via(
        &hosts.transport(),
        Some("gsk-user"),
        "whisper-large-v3-turbo".to_string(),
        take_request(&directory),
    )
    .await
    .unwrap();

    assert_eq!(result.text, "send it to samir");
    assert_eq!(result.provider, "groq");
    let groq = heads(&hosts.groq);
    assert_eq!(groq.len(), 1);
    assert!(
        groq[0].starts_with("POST /v1/audio/transcriptions "),
        "{}",
        groq[0]
    );
    assert_eq!(header(&groq[0], "authorization"), Some("Bearer gsk-user"));
    assert_no_forwarded_keys(&groq[0]);
    assert_eq!(header(&groq[0], CLOVY_APP_VERSION_HEADER), None);
    assert!(heads(&hosts.clovy).is_empty());
    assert!(heads(&hosts.openrouter).is_empty());
}

#[tokio::test]
async fn dictation_without_a_groq_key_goes_through_clovy_api() {
    let hosts = Hosts::start().await;
    let directory = tempfile::tempdir().unwrap();

    let result = dictate_transcribe_via(
        &hosts.transport(),
        None,
        "whisper-large-v3-turbo".to_string(),
        take_request(&directory),
    )
    .await
    .unwrap();

    assert_eq!(result.text, "via clovy");
    let clovy = heads(&hosts.clovy);
    assert_eq!(clovy.len(), 1);
    assert!(clovy[0].starts_with("POST /v1/dictate "), "{}", clovy[0]);
    assert_eq!(
        header(&clovy[0], "authorization"),
        Some(format!("Bearer {CLOVY_TOKEN}").as_str())
    );
    assert_no_forwarded_keys(&clovy[0]);
    assert!(heads(&hosts.groq).is_empty());
}

#[tokio::test]
async fn cleanup_with_an_openrouter_key_goes_straight_to_openrouter_with_only_that_key() {
    let hosts = Hosts::start().await;

    let cleaned = dictate_cleanup_via(
        &hosts.transport(),
        Some("sk-or-user"),
        cleanup_params(),
        "openai/gpt-5-mini".to_string(),
    )
    .await
    .unwrap();

    assert_eq!(cleaned, "Send it to Samir.");
    let openrouter = heads(&hosts.openrouter);
    assert_eq!(openrouter.len(), 1);
    assert!(
        openrouter[0].starts_with("POST /v1/chat/completions "),
        "{}",
        openrouter[0]
    );
    assert_eq!(
        header(&openrouter[0], "authorization"),
        Some("Bearer sk-or-user")
    );
    assert_no_forwarded_keys(&openrouter[0]);
    let body = hosts.openrouter.requests.lock().unwrap()[0].clone();
    assert_eq!(body["model"], "openai/gpt-5-mini");
    assert_eq!(body["temperature"], 0.0);
    assert_eq!(body["messages"][0]["role"], "system");
    assert_eq!(
        body["messages"][0]["content"],
        DICTATE_CLEANUP_SYSTEM_PROMPT.trim()
    );
    assert_eq!(
        body["messages"][1]["content"],
        cleanup_user_message(
            "send it to samir",
            Some("Samir"),
            None,
            "Writing style: standard."
        )
    );
    assert!(heads(&hosts.clovy).is_empty());
    assert!(heads(&hosts.groq).is_empty());
}

#[tokio::test]
async fn cleanup_without_an_openrouter_key_goes_through_clovy_api_with_the_chosen_model() {
    let hosts = Hosts::start().await;

    let cleaned = dictate_cleanup_via(
        &hosts.transport(),
        None,
        cleanup_params(),
        "openai/gpt-5-mini".to_string(),
    )
    .await
    .unwrap();

    assert_eq!(cleaned, "Cleaned via Clovy.");
    let clovy = heads(&hosts.clovy);
    assert_eq!(clovy.len(), 1);
    assert!(
        clovy[0].starts_with("POST /v1/dictate/cleanup "),
        "{}",
        clovy[0]
    );
    assert_no_forwarded_keys(&clovy[0]);
    let body = hosts.clovy.requests.lock().unwrap()[0].clone();
    assert_eq!(body["model"], "openai/gpt-5-mini");
    assert!(heads(&hosts.openrouter).is_empty());
}

#[tokio::test]
async fn a_rejected_groq_key_is_reported_as_the_users_key() {
    let groq = test_server::start(Arc::new(|_: &serde_json::Value| {
        (
            401,
            serde_json::json!({ "error": { "message": "invalid key" } }),
        )
    }))
    .await;
    let hosts = Hosts::start().await;
    let transport = DictationTransport {
        groq_base: groq.base_url.clone(),
        ..hosts.transport()
    };
    let directory = tempfile::tempdir().unwrap();

    let error = dictate_transcribe_via(
        &transport,
        Some("gsk-bad"),
        "whisper-large-v3-turbo".to_string(),
        take_request(&directory),
    )
    .await
    .unwrap_err();

    assert_eq!(error.code, "groq_api_key_rejected");
    assert!(!error.message.contains("gsk-bad"));
}

#[tokio::test]
async fn warming_opens_the_transcription_host_without_sending_audio() {
    let hosts = Hosts::start().await;

    warm_dictation_via(&hosts.transport(), Some("gsk-user"))
        .await
        .unwrap();
    let groq = heads(&hosts.groq);
    assert_eq!(groq.len(), 1);
    assert!(groq[0].starts_with("GET /v1/models "), "{}", groq[0]);
    assert_eq!(header(&groq[0], "authorization"), Some("Bearer gsk-user"));
    assert!(heads(&hosts.clovy).is_empty());

    warm_dictation_via(&hosts.transport(), None).await.unwrap();
    let clovy = heads(&hosts.clovy);
    assert_eq!(clovy.len(), 1);
    assert!(clovy[0].starts_with("GET /healthz "), "{}", clovy[0]);
    assert_eq!(heads(&hosts.groq).len(), 1);
}

#[test]
fn the_dictation_route_rewarms_only_after_a_minute_idle() {
    let now = std::time::Instant::now();
    assert!(dictation_route_needs_warm(None, now));
    assert!(!dictation_route_needs_warm(
        Some(now - Duration::from_secs(30)),
        now
    ));
    assert!(dictation_route_needs_warm(
        Some(now - Duration::from_secs(61)),
        now
    ));
}

#[test]
fn the_desktop_cleanup_prompt_is_a_copy_of_clovy_apis() {
    assert_eq!(
        DICTATE_CLEANUP_SYSTEM_PROMPT,
        include_str!("../../clovy-api/crates/services/src/prompts/dictate_cleanup.md")
    );
}
