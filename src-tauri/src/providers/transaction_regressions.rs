use super::*;
use crate::llm::secrets::{MemorySecretStore, SecretStore};
use std::os::unix::fs::PermissionsExt;

struct Fixture {
    directory: tempfile::TempDir,
    state: ProviderSettingsState,
    store: MemorySecretStore,
}
impl Fixture {
    fn new(provider: BuiltinProvider) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let mut settings = default_settings();
        settings.provider_api_keys_configured.set(provider, true);
        let state = ProviderSettingsState {
            path: directory.path().join("settings.json"),
            settings: Mutex::new(settings.clone()),
        };
        let store = MemorySecretStore::default();
        store
            .set(provider.secret_account(), "fixture-prior-secret")
            .unwrap();
        save_settings(&state, &settings).unwrap();
        replace_current_settings(settings);
        Self {
            directory,
            state,
            store,
        }
    }
    fn reject_persistence(&self) {
        fs::set_permissions(&self.state.path, fs::Permissions::from_mode(0o400)).unwrap();
        fs::set_permissions(self.directory.path(), fs::Permissions::from_mode(0o500)).unwrap();
    }
    fn assert_preserved(&self, provider: BuiltinProvider, before: &str) {
        assert!(
            self.store
                .get(provider.secret_account())
                .unwrap()
                .as_deref()
                == Some("fixture-prior-secret"),
            "prior usable credential must survive settings failure"
        );
        assert!(
            self.state
                .settings
                .lock()
                .unwrap()
                .provider_api_keys_configured
                .get(provider),
            "managed flag must survive"
        );
        assert!(
            current_settings()
                .provider_api_keys_configured
                .get(provider),
            "published flag must survive"
        );
        assert_eq!(fs::read_to_string(&self.state.path).unwrap(), before);
        let loaded = settings_from_json_for_tests(before, &self.store).unwrap();
        assert!(
            saved_provider_api_key(&loaded, &self.store, provider)
                .unwrap()
                .as_deref()
                == Some("fixture-prior-secret"),
            "reload must retain usable credential"
        );
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::set_permissions(self.directory.path(), fs::Permissions::from_mode(0o700));
        let _ = fs::set_permissions(&self.state.path, fs::Permissions::from_mode(0o600));
    }
}

#[tokio::test]
async fn f2_credential_and_settings_transaction() {
    // Isolate the published process-wide view from unrelated settings tests.
    const CHILD: &str = "CLOVY_TEST_SETTINGS_TRANSACTION_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "providers::transaction_regressions::f2_credential_and_settings_transaction",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    let server =
        crate::llm::endpoint::test_server::start(std::sync::Arc::new(|_: &serde_json::Value| {
            (200, serde_json::json!({"data":[]}))
        }))
        .await;
    for provider in [
        BuiltinProvider::Groq,
        BuiltinProvider::OpenRouter,
        BuiltinProvider::Fal,
        BuiltinProvider::Tavily,
    ] {
        for replace in [false, true] {
            let fixture = Fixture::new(provider);
            let before = fs::read_to_string(&fixture.state.path).unwrap();
            fixture.reject_persistence();
            let result = if replace {
                set_provider_api_key_impl(
                    &fixture.state,
                    &fixture.store,
                    SetProviderApiKeyRequest {
                        provider,
                        api_key: "fixture-replacement-secret".into(),
                    },
                    &server.base_url,
                )
                .await
            } else {
                clear_provider_api_key_impl(&fixture.state, &fixture.store, provider)
            };
            assert_eq!(result.unwrap_err().code, "provider_settings_save_failed");
            fixture.assert_preserved(provider, &before);
        }
        let fixture = Fixture::new(provider);
        clear_provider_api_key_impl(&fixture.state, &fixture.store, provider).unwrap();
        assert!(fixture
            .store
            .get(provider.secret_account())
            .unwrap()
            .is_none());
        assert!(!current_settings()
            .provider_api_keys_configured
            .get(provider));
        let loaded = settings_from_json_for_tests(
            &fs::read_to_string(&fixture.state.path).unwrap(),
            &fixture.store,
        )
        .unwrap();
        assert!(!loaded.provider_api_keys_configured.get(provider));
    }
}

struct RefusingStore<'a>(&'a MemorySecretStore);
impl SecretStore for RefusingStore<'_> {
    fn get(&self, account: &str) -> Result<Option<String>, String> {
        self.0.get(account)
    }
    fn set(&self, _: &str, _: &str) -> Result<(), String> {
        Err("store unavailable".into())
    }
    fn delete(&self, _: &str) -> Result<(), String> {
        Err("store unavailable".into())
    }
}

#[tokio::test]
async fn f2_keychain_refusal_and_first_save_keep_settings_intact() {
    if isolated_test("f2_keychain_refusal_and_first_save_keep_settings_intact") {
        return;
    }
    let server =
        crate::llm::endpoint::test_server::start(std::sync::Arc::new(|_: &serde_json::Value| {
            (200, serde_json::json!({"data":[]}))
        }))
        .await;
    for provider in [
        BuiltinProvider::Groq,
        BuiltinProvider::OpenRouter,
        BuiltinProvider::Fal,
        BuiltinProvider::Tavily,
    ] {
        let fixture = Fixture::new(provider);
        let before = fs::read_to_string(&fixture.state.path).unwrap();
        let refusal = RefusingStore(&fixture.store);
        let result = clear_provider_api_key_impl(&fixture.state, &refusal, provider);
        assert_eq!(result.unwrap_err().code, "provider_api_key_store_failed");
        let result = set_provider_api_key_impl(
            &fixture.state,
            &refusal,
            SetProviderApiKeyRequest {
                provider,
                api_key: "fixture-replacement-secret".into(),
            },
            &server.base_url,
        )
        .await;
        assert_eq!(result.unwrap_err().code, "provider_api_key_store_failed");
        assert!(
            fixture
                .store
                .get(provider.secret_account())
                .unwrap()
                .as_deref()
                == Some("fixture-prior-secret")
        );
        assert_eq!(fs::read_to_string(&fixture.state.path).unwrap(), before);
        assert!(fixture
            .state
            .settings
            .lock()
            .unwrap()
            .provider_api_keys_configured
            .get(provider));

        clear_provider_api_key_impl(&fixture.state, &fixture.store, provider).unwrap();
        let before = fs::read_to_string(&fixture.state.path).unwrap();
        fixture.reject_persistence();
        let result = set_provider_api_key_impl(
            &fixture.state,
            &fixture.store,
            SetProviderApiKeyRequest {
                provider,
                api_key: "fixture-first-secret".into(),
            },
            &server.base_url,
        )
        .await;
        assert_eq!(result.unwrap_err().code, "provider_settings_save_failed");
        assert!(fixture
            .store
            .get(provider.secret_account())
            .unwrap()
            .is_none());
        assert!(!fixture
            .state
            .settings
            .lock()
            .unwrap()
            .provider_api_keys_configured
            .get(provider));
        assert_eq!(fs::read_to_string(&fixture.state.path).unwrap(), before);
    }
}

#[test]
fn f2_rejected_settings_candidate_is_never_published_or_saved() {
    if isolated_test("f2_rejected_settings_candidate_is_never_published_or_saved") {
        return;
    }
    let fixture = Fixture::new(BuiltinProvider::Fal);
    let before = fs::read_to_string(&fixture.state.path).unwrap();
    let result = update_settings_result(&fixture.state, |candidate| {
        candidate.image_model = "invalid".into();
        Err(AppError::new("rejected", "rejected"))
    });
    assert!(result.is_err());
    assert_eq!(
        fixture.state.settings.lock().unwrap().image_model,
        DEFAULT_IMAGE_MODEL
    );
    assert_eq!(fs::read_to_string(&fixture.state.path).unwrap(), before);
}

#[tokio::test]
async fn f3_native_media_catalog_preserves_remote_and_unknown_availability() {
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };
    const CHILD: &str = "CLOVY_TEST_MEDIA_CATALOG_CHILD";
    if std::env::var_os(CHILD).is_some() {
        let directory = tempfile::tempdir().unwrap();
        let state = ProviderSettingsState {
            path: directory.path().join("settings.json"),
            settings: Mutex::new(default_settings()),
        };
        for availability in [Some(false), Some(true), None] {
            for mode in [ModelMode::Image, ModelMode::Video] {
                let response = list_provider_models_impl(&state, ProviderModelsRequest { mode })
                    .await
                    .unwrap();
                assert_eq!(
                    response.models.len(),
                    1,
                    "media request must reach the live catalog"
                );
                assert_eq!(response.models[0].service_available, availability);
                assert_eq!(response.models[0].provider, "fal");
                assert_eq!(response.model_type, mode.api_type());
            }
        }
        return;
    }
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        for availability in [Some(false), Some(true), None] {
            for mode in ["image", "video"] {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = vec![0; 4096];
                let read = socket.read(&mut request).await.unwrap();
                let head = String::from_utf8_lossy(&request[..read]);
                assert!(head.starts_with(&format!("GET /v1/models?type={mode} ")));
                let mut row = serde_json::json!({"provider":"fal","id":"fixture-media","name":"Fixture media","modelType":mode,"description":null,"privacy":null,"pricing":null,"contextTokens":null,"traits":[],"capabilities":[],"priceUnit":mode,"priceDescription":""});
                if let Some(available) = availability {
                    row["serviceAvailable"] = available.into();
                }
                let payload = serde_json::json!({"success":true,"data":[row]}).to_string();
                let response = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}", payload.len());
                socket.write_all(response.as_bytes()).await.unwrap();
            }
        }
    });
    let output = tokio::task::spawn_blocking(move || std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "providers::transaction_regressions::f3_native_media_catalog_preserves_remote_and_unknown_availability", "--nocapture"])
        .env(CHILD, "1").env("CLOVY_API_URL", format!("http://{address}"))
        .output().unwrap()).await.unwrap();
    if !output.status.success() {
        server.abort();
    }
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    server.await.unwrap();
}

fn isolated_test(name: &str) -> bool {
    const CHILD: &str = "CLOVY_TRANSACTION_REGRESSION_CHILD";
    if std::env::var(CHILD).ok().as_deref() == Some(name) {
        return false;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            &format!("providers::transaction_regressions::{name}"),
            "--nocapture",
        ])
        .env(CHILD, name)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    true
}

struct RestoreFails<'a>(&'a MemorySecretStore);
impl SecretStore for RestoreFails<'_> {
    fn get(&self, account: &str) -> Result<Option<String>, String> {
        self.0.get(account)
    }
    fn delete(&self, account: &str) -> Result<(), String> {
        self.0.delete(account)
    }
    fn set(&self, _: &str, _: &str) -> Result<(), String> {
        Err("restore unavailable".into())
    }
}

#[test]
fn f2_failed_compensation_is_explicit_and_never_changes_settings_flags() {
    if isolated_test("f2_failed_compensation_is_explicit_and_never_changes_settings_flags") {
        return;
    }
    let fixture = Fixture::new(BuiltinProvider::Fal);
    let before = fs::read_to_string(&fixture.state.path).unwrap();
    fixture.reject_persistence();
    let result = clear_provider_api_key_impl(
        &fixture.state,
        &RestoreFails(&fixture.store),
        BuiltinProvider::Fal,
    );
    assert_eq!(result.unwrap_err().code, "provider_api_key_rollback_failed");
    assert_eq!(fs::read_to_string(&fixture.state.path).unwrap(), before);
    assert!(
        fixture
            .state
            .settings
            .lock()
            .unwrap()
            .provider_api_keys_configured
            .fal
    );
    assert!(current_settings().provider_api_keys_configured.fal);
}
