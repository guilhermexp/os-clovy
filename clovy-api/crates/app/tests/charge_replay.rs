//! Real OS Accounts HTTP client + real settlement against a stateful loopback
//! OS Accounts substitute that dedupes charges by idempotency key the way the
//! platform does: same `(token, key)` replays, a key already settled under
//! another token is rejected with 4001. No real account, key or inference is
//! used; the upstream work is an in-process fake.
#![allow(clippy::unwrap_used, clippy::too_many_lines)]

use async_trait::async_trait;
use clovy_config::{AppConfig, ModelPriceConfig, ModelType, PriceUnit};
use clovy_domain::{
    AudioDurationProbe, DomainError, GeneratedNote, GenerationRequest, Generator, ModelId,
    ProviderCredentials, TokenUsage, Transcriber, Transcript, TranscriptionRequest,
    UpstreamRouteMetadata, UserId,
};
use clovy_providers::{http, os_accounts::OsAccountsHttpClient};
use clovy_services::{
    NoteGenerateParams, NoteGenerateService, NoteGenerateServiceDeps, NoteTranscribeParams,
    NoteTranscribeService, NoteTranscribeServiceDeps, PricingTable,
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex},
    time::Duration,
};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate, matchers::path};

#[derive(Clone, Debug)]
struct ChargeCall {
    token: String,
    credits: u64,
    idempotency_key: String,
}

#[derive(Default)]
struct LedgerState {
    issued: u64,
    open_holds: BTreeSet<String>,
    /// `idempotency_key` -> (token that settled it, credits debited)
    settled_keys: BTreeMap<String, (String, u64)>,
    settled_tokens: BTreeSet<String>,
    charges: Vec<ChargeCall>,
    debited: u64,
}

#[derive(Clone, Default)]
struct FakeLedger(Arc<Mutex<LedgerState>>);

impl FakeLedger {
    fn state<R>(&self, read: impl FnOnce(&LedgerState) -> R) -> R {
        read(&self.0.lock().unwrap())
    }

    fn release_calls(&self) -> Vec<ChargeCall> {
        self.state(|state| {
            state
                .charges
                .iter()
                .filter(|call| call.idempotency_key.starts_with("release:"))
                .cloned()
                .collect()
        })
    }

    fn work_charges(&self) -> Vec<ChargeCall> {
        self.state(|state| {
            state
                .charges
                .iter()
                .filter(|call| !call.idempotency_key.starts_with("release:"))
                .cloned()
                .collect()
        })
    }
}

struct Authorize(FakeLedger);

impl Respond for Authorize {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let body: Value = serde_json::from_slice(&request.body).unwrap();
        let mut state = self.0.0.lock().unwrap();
        state.issued += 1;
        let token = format!("fixture-hold-{}", state.issued);
        state.open_holds.insert(token.clone());
        ResponseTemplate::new(200).set_body_json(json!({
            "success": true,
            "data": {
                "allowed": true,
                "token": token,
                "cap_credits": body["estimate_credits"],
            }
        }))
    }
}

struct Charge(FakeLedger);

impl Respond for Charge {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let body: Value = serde_json::from_slice(&request.body).unwrap();
        let call = ChargeCall {
            token: body["token"].as_str().unwrap().to_string(),
            credits: body["credits"].as_u64().unwrap(),
            idempotency_key: body["idempotency_key"].as_str().unwrap().to_string(),
        };
        let mut state = self.0.0.lock().unwrap();
        state.charges.push(call.clone());
        if let Some((token, credits)) = state.settled_keys.get(&call.idempotency_key).cloned() {
            if token == call.token {
                return ResponseTemplate::new(200).set_body_json(json!({
                    "success": true,
                    "data": {"credits_settled": credits, "idempotent_replay": true}
                }));
            }
            return ResponseTemplate::new(409).set_body_json(json!({
                "success": false,
                "data": null,
                "error_code": 4001,
                "error_message": "idempotency_key_collision"
            }));
        }
        if state.settled_tokens.contains(&call.token) {
            return ResponseTemplate::new(409).set_body_json(json!({
                "success": false,
                "data": null,
                "error_code": 4002,
                "error_message": "grant_already_used"
            }));
        }
        state.open_holds.remove(&call.token);
        state.settled_tokens.insert(call.token.clone());
        state
            .settled_keys
            .insert(call.idempotency_key.clone(), (call.token, call.credits));
        state.debited += call.credits;
        ResponseTemplate::new(200).set_body_json(json!({
            "success": true,
            "data": {"credits_settled": call.credits, "idempotent_replay": false}
        }))
    }
}

async fn fake_os_accounts() -> (MockServer, FakeLedger) {
    let server = MockServer::start().await;
    let ledger = FakeLedger::default();
    Mock::given(path("/authorize"))
        .respond_with(Authorize(ledger.clone()))
        .mount(&server)
        .await;
    Mock::given(path("/charge"))
        .respond_with(Charge(ledger.clone()))
        .mount(&server)
        .await;
    (server, ledger)
}

fn os_accounts_client(server: &MockServer) -> Arc<OsAccountsHttpClient> {
    Arc::new(OsAccountsHttpClient::new(
        http::default_client(),
        &server.uri(),
        "fixture",
    ))
}

fn model(unit: PriceUnit, model_type: ModelType) -> ModelPriceConfig {
    let template = AppConfig::default()
        .pricing
        .into_values()
        .find(|price| price.model_type == model_type)
        .unwrap();
    ModelPriceConfig {
        unit,
        credits_per_million_seconds: Some(1_000_000),
        input_credits_per_million_tokens: Some(1_000_000),
        output_credits_per_million_tokens: Some(1_000_000),
        ..template
    }
}

fn pricing(model_id: &str, price: ModelPriceConfig) -> Arc<PricingTable> {
    Arc::new(PricingTable::new(BTreeMap::from([(
        model_id.to_string(),
        price,
    )])))
}

struct FixedProbe;

impl AudioDurationProbe for FixedProbe {
    fn probe(&self, _audio: &[u8]) -> Result<Duration, DomainError> {
        Ok(Duration::from_secs(7))
    }
}

struct FixedTranscriber;

#[async_trait]
impl Transcriber for FixedTranscriber {
    async fn transcribe(&self, request: TranscriptionRequest) -> Result<Transcript, DomainError> {
        Ok(Transcript {
            text: "Transcribed audio".to_string(),
            language: request.language,
            provider: "fixture".to_string(),
        })
    }
}

struct FixedGenerator;

#[async_trait]
impl Generator for FixedGenerator {
    async fn generate(&self, _request: GenerationRequest) -> Result<GeneratedNote, DomainError> {
        Ok(GeneratedNote {
            content: "Generated note".to_string(),
            title_suggestion: None,
            provider: "fixture".to_string(),
            route: UpstreamRouteMetadata {
                provider: None,
                privacy_level: None,
                endpoint: None,
            },
            usage: TokenUsage {
                prompt_tokens: 20,
                completion_tokens: 20,
            },
        })
    }
}

#[tokio::test]
async fn retried_settled_transcription_releases_the_new_hold_and_reports_zero() {
    // F1: the client retries a final transcription whose first response was
    // lost. Its key already settled under the first hold, so OS Accounts
    // answers 4001 for the new hold. That hold must be released exactly once
    // and the retry's receipt must not claim credits.
    let (server, ledger) = fake_os_accounts().await;
    let service = NoteTranscribeService::new(NoteTranscribeServiceDeps {
        pricing: pricing("asr-fixture", model(PriceUnit::Seconds, ModelType::Asr)),
        os_accounts: os_accounts_client(&server),
        transcriber: Arc::new(FixedTranscriber),
        duration_probe: Arc::new(FixedProbe),
        hold_ttl_seconds: 60,
        flat_estimate_credits: 1024,
        preview_max_audio_seconds: 30,
    });
    let params = || NoteTranscribeParams {
        user_id: UserId("fixture-user".into()),
        note_id: "note-1-turn-1-chunk-0".into(),
        audio: vec![1, 2, 3],
        filename: "turn.wav".into(),
        context: None,
        language: None,
        model_id: ModelId("asr-fixture".into()),
        preview: false,
        preview_opted_in: false,
        provider_credentials: ProviderCredentials::default(),
    };

    let first = service.transcribe(params()).await.unwrap();
    let retry = service.transcribe(params()).await.unwrap();

    assert_eq!(first.receipt.credits_charged.0, 7);
    assert!(!first.receipt.idempotent_replay);
    assert_eq!(
        retry.receipt.credits_charged.0, 0,
        "retry claims no credits"
    );
    assert!(retry.receipt.idempotent_replay);

    let releases = ledger.release_calls();
    assert_eq!(releases.len(), 1, "release_hold called once: {releases:?}");
    assert_eq!(releases[0].token, "fixture-hold-2");
    assert_eq!(releases[0].credits, 0);
    ledger.state(|state| {
        assert!(
            state.open_holds.is_empty(),
            "open holds: {:?}",
            state.open_holds
        );
        assert_eq!(state.debited, 7, "the operation is debited once");
    });
}

#[tokio::test]
async fn regenerating_the_same_note_and_version_charges_each_generation() {
    // F2: the desktop sends the note id as the operation id, so two
    // generations of the same note and prompt version must still settle
    // under distinct keys and both be charged.
    let (server, ledger) = fake_os_accounts().await;
    let service = NoteGenerateService::new(NoteGenerateServiceDeps {
        pricing: pricing("text-fixture", model(PriceUnit::Tokens, ModelType::Text)),
        os_accounts: os_accounts_client(&server),
        generator: Arc::new(FixedGenerator),
        hold_ttl_seconds: 60,
        flat_estimate_credits: 1024,
    });
    let params = || NoteGenerateParams {
        user_id: UserId("fixture-user".into()),
        note_id: "note-1".into(),
        prompt_version: clovy_services::NOTE_GENERATE_PROMPT_VERSION.into(),
        title: "Standup".into(),
        transcript: "We shipped it.".into(),
        transcript_source_labels: false,
        manual_notes: None,
        language: None,
        existing_generated_note: None,
        model_id: ModelId("text-fixture".into()),
        provider_credentials: ProviderCredentials::default(),
    };

    let first = service.generate(params()).await.unwrap();
    let second = service.generate(params()).await.unwrap();

    assert_eq!(first.receipt.credits_charged.0, 40);
    assert_eq!(second.receipt.credits_charged.0, 40);
    assert!(!second.receipt.idempotent_replay);
    let charges = ledger.work_charges();
    assert_eq!(charges.len(), 2, "two charges: {charges:?}");
    assert_ne!(charges[0].idempotency_key, charges[1].idempotency_key);
    assert!(ledger.release_calls().is_empty());
    ledger.state(|state| {
        assert!(
            state.open_holds.is_empty(),
            "open holds: {:?}",
            state.open_holds
        );
        assert_eq!(state.debited, 80, "each generation is debited");
    });
}
