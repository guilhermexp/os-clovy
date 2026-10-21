use clap::{Parser, Subcommand};
use clovy_api::{
    ApiLimits, ApiState, ApiStateParams, AttestationInfo, ShareViewerInfo, UpstreamServiceKeys,
};
use clovy_config::{AppConfig, ModelPriceConfig, image_client_timeout_secs};
use clovy_providers::{
    FalImageEditor, FalImageGenerator, FalVideoProvider, GroqTranscriber, JwksTokenVerifier,
    LocalDevOsAccountsClient, LocalDevTokenVerifier, LogIssueReportSink, LogP3aSink,
    MultiFormatDurationProbe, OpenRouterAgentChat, OpenRouterCleaner, OpenRouterGenerator,
    OpenRouterModelCatalog, OsAccountsHttpClient, OsAccountsP3aSink, OsPlatformIssueReportSink,
    TavilyAugment, default_client, issue_report_client, jwks_client, upstream_client,
};
use clovy_services::{
    AgentChatService, AgentChatServiceDeps, DictateService, DictateServiceDeps, ImageModelPrice,
    ImageService, ImageServiceDeps, IssueReportService, IssueReportServiceDeps,
    NoteGenerateService, NoteGenerateServiceDeps, NoteTranscribeService, NoteTranscribeServiceDeps,
    P3aReportService, P3aReportServiceDeps, PricingTable, VideoModelPrice, VideoService,
    VideoServiceDeps, WebAugmentService, WebAugmentServiceDeps,
};
use std::{collections::BTreeMap, net::SocketAddr, sync::Arc, time::Duration};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Debug, Parser)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    Serve,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = dotenvy::dotenv();
    init_tracing();
    let cli = Cli::parse();
    match cli.command.unwrap_or(Command::Serve) {
        Command::Serve => serve().await,
    }
}

async fn serve() -> anyhow::Result<()> {
    let config = clovy_config::load()?;
    let address: SocketAddr = format!("{}:{}", config.server.host, config.server.port).parse()?;
    let http = default_client();
    let upstream_http = upstream_client(Duration::from_secs(
        config.server.request_timeout_secs.max(1),
    ))?;
    // Shared bounded client for every long-running metered inference call
    // (image generate/edit, note generation, agent chat): route timeout minus
    // the authorize + settlement budgets, so settlement always lands inside
    // the hold TTL.
    let metered_inference_http = upstream_client(Duration::from_secs(image_client_timeout_secs(
        config.server.request_timeout_secs,
    )))?;
    // os-platform attachment uploads can carry the full 300 MiB file budget.
    // Give that dedicated client the route's configured window instead of the
    // shared client's 60 seconds. The client deliberately has no retry layer:
    // file and Issue creation POSTs are not idempotent.
    let issue_report_http =
        issue_report_client(Duration::from_secs(config.server.request_timeout_secs))?;
    let initial_pricing = try_load_pricing(&config, upstream_http.clone())
        .await
        .unwrap_or_else(|| config.pricing.clone());
    tracing::info!(
        count = initial_pricing.len(),
        "loaded initial model catalog"
    );
    let pricing = Arc::new(PricingTable::new(initial_pricing));
    spawn_model_catalog_refresh(&config, upstream_http.clone(), pricing.clone());
    let clients = HttpClients {
        default: &http,
        upstream: &upstream_http,
        metered_inference: &metered_inference_http,
        issue_reports: &issue_report_http,
    };
    // Private sharing (JUN-308): optional — the API runs share-less until a
    // database is configured, so this cannot regress existing deployments.
    let share_store: Option<Arc<dyn clovy_domain::ShareStore>> =
        match config.share.database_url.trim() {
            "" => {
                tracing::info!("share store not configured; sharing endpoints disabled");
                None
            }
            _ if !config.local_dev.enabled && config.share.viewer_client_id.trim().is_empty() => {
                // The browser viewer signs recipients in with this OAuth client
                // id; without it, links created here would be unusable (the
                // viewer would redirect with an empty client_id). Fail closed to
                // 501 rather than mint dead links. Local dev seeds the token
                // directly and needs no client id.
                tracing::warn!(
                    "share database configured but CLOVY__SHARE__VIEWER_CLIENT_ID is unset; \
                     sharing endpoints disabled to avoid creating unusable links"
                );
                None
            }
            database_url => match clovy_persistence::PgShareStore::connect(database_url).await {
                Ok(store) => {
                    tracing::info!("share store connected");
                    Some(Arc::new(store))
                }
                Err(error) => {
                    tracing::error!(
                        %error,
                        "share store connection failed; sharing endpoints disabled"
                    );
                    None
                }
            },
        };
    let companion = load_companion_runtime(&config).await;
    let app = build_router(
        &config,
        clients,
        pricing,
        RuntimeStores {
            share: share_store,
            companion,
        },
    );
    let listener = tokio::net::TcpListener::bind(address).await?;
    tracing::info!(%address, "clovy-api listening");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn load_companion_runtime(config: &AppConfig) -> CompanionRuntime {
    let (store, snapshot): (
        Option<Arc<dyn clovy_domain::CompanionStore>>,
        clovy_domain::CompanionSnapshot,
    ) = match config.companion.database_url.trim() {
        "" => {
            if !config.local_dev.enabled {
                tracing::warn!(
                    "companion store not configured; companion relay endpoints disabled"
                );
            }
            (None, clovy_domain::CompanionSnapshot::default())
        }
        database_url => match clovy_persistence::PgCompanionStore::connect(database_url).await {
            Ok(store) => match clovy_domain::CompanionStore::snapshot(&store).await {
                Ok(snapshot) => {
                    tracing::info!(
                        devices = snapshot.devices.len(),
                        links = snapshot.links.len(),
                        "companion store connected"
                    );
                    (Some(Arc::new(store)), snapshot)
                }
                Err(error) => {
                    tracing::error!(%error, "companion store load failed; relay disabled");
                    (None, clovy_domain::CompanionSnapshot::default())
                }
            },
            Err(error) => {
                tracing::error!(%error, "companion store connection failed; relay disabled");
                (None, clovy_domain::CompanionSnapshot::default())
            }
        },
    };
    let enabled = config.local_dev.enabled || store.is_some();
    let push = if config.companion.apns_team_id.trim().is_empty()
        || config.companion.apns_key_id.trim().is_empty()
        || config.companion.apns_private_key_pem.trim().is_empty()
        || config.companion.apns_bundle_id.trim().is_empty()
    {
        tracing::info!("APNs credentials not configured; opaque companion wakes disabled");
        None
    } else {
        Some(clovy_api::CompanionPushConfig {
            team_id: config.companion.apns_team_id.clone(),
            key_id: config.companion.apns_key_id.clone(),
            private_key_pem: config.companion.apns_private_key_pem.replace("\\n", "\n"),
            bundle_id: config.companion.apns_bundle_id.clone(),
            production: config.companion.apns_production,
        })
    };
    CompanionRuntime {
        store,
        snapshot,
        enabled,
        push,
    }
}

/// The model catalog: the packaged pricing extended by `OpenRouter`'s public
/// model list (no key needed). Every priced model stays listed whether or not
/// Clovy holds that provider's service key: a request then needs the user's
/// own key and otherwise fails `<provider>_not_configured`, so the catalog is
/// never emptied by a missing key. `None` keeps the last known catalog when
/// `OpenRouter` is unreachable.
async fn try_load_pricing(
    config: &AppConfig,
    http: reqwest::Client,
) -> Option<BTreeMap<String, ModelPriceConfig>> {
    match OpenRouterModelCatalog::from_config(http, &config.upstreams.openrouter)
        .priced_models()
        .await
    {
        Ok(models) => {
            let mut pricing = config.pricing.clone();
            pricing.extend(models);
            Some(pricing)
        }
        Err(error) => {
            tracing::warn!(
                %error,
                "failed to refresh the OpenRouter model catalog; keeping the last known catalog"
            );
            None
        }
    }
}

fn spawn_model_catalog_refresh(
    config: &AppConfig,
    http: reqwest::Client,
    pricing: Arc<PricingTable>,
) {
    let config = config.clone();
    let refresh_every = Duration::from_secs(config.server.model_catalog_refresh_secs);
    let task = tokio::spawn(async move {
        loop {
            tokio::time::sleep(refresh_every).await;
            if let Some(models) = try_load_pricing(&config, http.clone()).await {
                let count = models.len();
                pricing.replace(models);
                tracing::info!(
                    count,
                    refresh_secs = refresh_every.as_secs(),
                    "refreshed model catalog"
                );
            }
        }
    });
    // Dropping a Tokio join handle detaches the long-lived refresh task.
    drop(task);
}

/// Which upstreams Clovy can fund with its own service key. A missing key is
/// allowed only in local dev (validated at load); requests to that upstream
/// then need the user's own key.
fn upstream_service_keys(config: &AppConfig) -> UpstreamServiceKeys {
    let keys = UpstreamServiceKeys::for_providers(
        clovy_domain::UpstreamProvider::ALL
            .into_iter()
            .filter(|provider| upstream_config(config, *provider).has_service_key()),
    );
    for provider in clovy_domain::UpstreamProvider::ALL {
        if !keys.has(provider) {
            tracing::warn!(
                upstream = provider.as_str(),
                "no service key configured; requests to this upstream need the user's own key"
            );
        }
    }
    keys
}

fn upstream_config(
    config: &AppConfig,
    provider: clovy_domain::UpstreamProvider,
) -> &clovy_config::UpstreamConfig {
    match provider {
        clovy_domain::UpstreamProvider::Groq => &config.upstreams.groq,
        clovy_domain::UpstreamProvider::Openrouter => &config.upstreams.openrouter,
        clovy_domain::UpstreamProvider::Fal => &config.upstreams.fal,
        clovy_domain::UpstreamProvider::Tavily => &config.upstreams.tavily,
    }
}

// The dependency-injection composition root: it wires every provider and
// service into the router, so its length grows by a line or two with each new
// capability (image generation is the latest). Splitting it further would scatter
// the wiring without making it clearer.
#[allow(clippy::too_many_lines)]
fn build_router(
    config: &AppConfig,
    clients: HttpClients<'_>,
    pricing: Arc<PricingTable>,
    stores: RuntimeStores,
) -> axum::Router {
    let os_accounts = build_os_accounts_client(config, clients.default);
    let transcriber: Arc<dyn clovy_domain::Transcriber> = Arc::new(GroqTranscriber::from_config(
        clients.upstream.clone(),
        &config.upstreams.groq,
    ));
    // Note generation and agent chat can now run long (streamed/kept-alive
    // responses), so their upstream window must leave the settlement budget
    // inside the authorization hold — the same route/client/hold math the
    // image path pins (see image_client_timeout_secs and
    // validate_long_inference_hold_ttl). The full-route `upstream` client
    // would let a 300-600s call reach `charge` after its hold expired.
    // Unmetered (user-OpenRouter-key) requests have no hold to protect and
    // keep the full-route client — see AgentChatRequest::unmetered.
    let generator: Arc<dyn clovy_domain::Generator> = Arc::new(OpenRouterGenerator::from_config(
        clients.metered_inference.clone(),
        clients.upstream.clone(),
        &config.upstreams.openrouter,
    ));
    let cleaner: Arc<dyn clovy_domain::Cleaner> = Arc::new(OpenRouterCleaner::from_config(
        clients.upstream.clone(),
        &config.upstreams.openrouter,
    ));
    let agent_chat_completer: Arc<dyn clovy_domain::AgentChatCompleter> =
        Arc::new(OpenRouterAgentChat::from_config(
            clients.metered_inference.clone(),
            clients.upstream.clone(),
            &config.upstreams.openrouter,
        ));
    // One client backs both web traits (search + fetch) over the same Tavily
    // credential and base URL.
    let web_augment = Arc::new(TavilyAugment::from_config(
        clients.upstream.clone(),
        &config.upstreams.tavily,
    ));
    let duration_probe: Arc<dyn clovy_domain::AudioDurationProbe> =
        Arc::new(MultiFormatDurationProbe);
    let token_verifier = build_token_verifier(config);
    let issue_report_sink = build_issue_report_sink(config, clients.issue_reports);
    let p3a_sink = build_p3a_sink(config, clients.default);
    let issue_reports = Arc::new(IssueReportService::new(IssueReportServiceDeps {
        sink: issue_report_sink,
        chat_completer: agent_chat_completer.clone(),
        config: config.issue_reports.clone(),
    }));
    let p3a_reports = Arc::new(P3aReportService::new(P3aReportServiceDeps {
        sink: p3a_sink,
    }));

    let flat_estimate_credits = config.os_accounts.flat_estimate_credits;

    let note_transcribe = Arc::new(NoteTranscribeService::new(NoteTranscribeServiceDeps {
        pricing: pricing.clone(),
        os_accounts: os_accounts.clone(),
        transcriber: transcriber.clone(),
        duration_probe: duration_probe.clone(),
        hold_ttl_seconds: config.os_accounts.authorize_hold_ttl_note_transcribe_secs,
        flat_estimate_credits,
        preview_max_audio_seconds: config.os_accounts.note_transcribe_preview_max_audio_secs,
    }));
    let note_generate = Arc::new(NoteGenerateService::new(NoteGenerateServiceDeps {
        pricing: pricing.clone(),
        os_accounts: os_accounts.clone(),
        generator,
        hold_ttl_seconds: config.os_accounts.authorize_hold_ttl_note_generate_secs,
        flat_estimate_credits,
    }));
    let agent_chat = Arc::new(AgentChatService::new(AgentChatServiceDeps {
        pricing: pricing.clone(),
        os_accounts: os_accounts.clone(),
        chat_completer: agent_chat_completer,
        hold_ttl_seconds: config.os_accounts.authorize_hold_ttl_note_generate_secs,
        flat_estimate_credits,
    }));
    let web = Arc::new(WebAugmentService::new(WebAugmentServiceDeps {
        os_accounts: os_accounts.clone(),
        searcher: web_augment.clone() as Arc<dyn clovy_domain::WebSearcher>,
        fetcher: web_augment as Arc<dyn clovy_domain::WebFetcher>,
        search_credits: config.os_accounts.web_search_credits,
        fetch_credits: config.os_accounts.web_fetch_credits,
        hold_ttl_seconds: config.os_accounts.authorize_hold_ttl_web_secs,
    }));
    let image = Arc::new(ImageService::new(ImageServiceDeps {
        os_accounts: os_accounts.clone(),
        generator: build_image_generator(
            clients.metered_inference,
            &config.upstreams.fal,
            Duration::from_secs(image_client_timeout_secs(
                config.server.request_timeout_secs,
            )),
        ),
        editor: build_image_editor(
            clients.metered_inference,
            &config.upstreams.fal,
            Duration::from_secs(image_client_timeout_secs(
                config.server.request_timeout_secs,
            )),
        ),
        pricing: config
            .image_pricing
            .iter()
            .map(|(model, credits)| (model.clone(), ImageModelPrice::new(*credits)))
            .collect(),
        default_model: config.default_image_model.clone(),
        edit_pricing: config
            .image_edit_pricing
            .iter()
            .map(|(model, credits)| (model.clone(), ImageModelPrice::new(*credits)))
            .collect(),
        default_edit_model: config.default_image_edit_model.clone(),
        // Edits are the same latency class as generation, so they reuse the
        // image hold TTL rather than adding a second knob.
        hold_ttl_seconds: config.os_accounts.authorize_hold_ttl_image_secs,
    }));
    let video = Arc::new(VideoService::new(VideoServiceDeps {
        os_accounts: os_accounts.clone(),
        provider: build_video_provider(
            clients.metered_inference,
            &config.upstreams.fal,
            Duration::from_secs(image_client_timeout_secs(
                config.server.request_timeout_secs,
            )),
            config.video_max_response_bytes,
        ),
        pricing: config
            .video_pricing
            .iter()
            .map(|(model, markup)| (model.clone(), VideoModelPrice::new(*markup)))
            .collect(),
        default_model: config.default_video_model.clone(),
        animate_pricing: config
            .video_animate_pricing
            .iter()
            .map(|(model, markup)| (model.clone(), VideoModelPrice::new(*markup)))
            .collect(),
        default_animate_model: config.default_video_animate_model.clone(),
        max_credits_per_request: config.video_max_credits_per_request,
        hold_ttl_seconds: config.os_accounts.authorize_hold_ttl_video_secs,
    }));
    let dictate = Arc::new(DictateService::new(DictateServiceDeps {
        pricing: pricing.clone(),
        os_accounts,
        transcriber,
        cleaner,
        duration_probe,
        transcribe_hold_ttl_seconds: config
            .os_accounts
            .authorize_hold_ttl_dictate_transcribe_secs,
        cleanup_hold_ttl_seconds: config.os_accounts.authorize_hold_ttl_dictate_cleanup_secs,
        flat_estimate_credits,
    }));

    let share = stores.share.map(|store| {
        Arc::new(clovy_services::ShareService::new(
            clovy_services::ShareServiceDeps {
                store,
                identity: if config.local_dev.enabled {
                    Arc::new(clovy_providers::local_dev::LocalDevViewerIdentity::new(
                        config.local_dev.viewer_bearer_token.clone(),
                        config.local_dev.viewer_email.clone(),
                    )) as Arc<dyn clovy_domain::ViewerIdentity>
                } else {
                    Arc::new(
                        clovy_providers::viewer_identity::OsAccountsViewerIdentity::new(
                            clients.default.clone(),
                            &config.os_accounts.api_url,
                        ),
                    )
                },
                max_ciphertext_bytes: config.share.max_ciphertext_bytes,
            },
        ))
    });
    let share_viewer = ShareViewerInfo {
        accounts_url: if config.share.viewer_accounts_url.trim().is_empty() {
            config.os_accounts.iss.clone()
        } else {
            config.share.viewer_accounts_url.clone()
        },
        accounts_api_url: config.os_accounts.api_url.clone(),
        client_id: config.share.viewer_client_id.clone(),
    };

    let state = ApiState::new(ApiStateParams {
        pricing,
        computer_use: config.computer_use.clone(),
        upstream_service_keys: upstream_service_keys(config),
        token_verifier,
        companion_store: stores.companion.store,
        companion_snapshot: stores.companion.snapshot,
        companion_enabled: stores.companion.enabled,
        companion_push: stores.companion.push,
        note_transcribe,
        note_generate,
        agent_chat,
        dictate,
        web,
        image,
        video,
        issue_reports,
        p3a_reports,
        share,
        share_viewer,
        limits: ApiLimits {
            max_audio_bytes: config.server.max_audio_bytes,
            max_json_bytes: config.server.max_json_bytes,
            max_issue_report_bytes: config.server.max_issue_report_bytes,
            max_image_edit_bytes: config.server.max_image_edit_bytes,
            // Base64 inflates by 4/3; leave headroom for envelopes + JSON.
            max_share_body_bytes: config.share.max_ciphertext_bytes / 3 * 4 + 64 * 1024,
            max_agent_chat_bytes: config.server.max_agent_chat_bytes,
            max_agent_inflight_body_bytes: config.server.max_agent_inflight_body_bytes,
            max_agent_concurrent_requests_per_user: config
                .server
                .max_agent_concurrent_requests_per_user,
            request_timeout_secs: config.server.request_timeout_secs,
        },
        attestation: AttestationInfo {
            source_commit: config.attestation.source_commit.clone(),
            source_repo_url: config.attestation.source_repo_url.clone(),
            image_repo: config.attestation.image_repo.clone(),
            trust_center_url: config.attestation.trust_center_url.clone(),
        },
        browser_transports: config.browser_transports,
    });
    if config.share.viewer_only {
        clovy_api::viewer_router(state)
    } else {
        clovy_api::router(state)
    }
}

struct CompanionRuntime {
    store: Option<Arc<dyn clovy_domain::CompanionStore>>,
    snapshot: clovy_domain::CompanionSnapshot,
    enabled: bool,
    push: Option<clovy_api::CompanionPushConfig>,
}

struct RuntimeStores {
    share: Option<Arc<dyn clovy_domain::ShareStore>>,
    companion: CompanionRuntime,
}

#[derive(Clone, Copy)]
struct HttpClients<'a> {
    default: &'a reqwest::Client,
    upstream: &'a reqwest::Client,
    metered_inference: &'a reqwest::Client,
    issue_reports: &'a reqwest::Client,
}

fn build_image_generator(
    upstream_http: &reqwest::Client,
    fal: &clovy_config::UpstreamConfig,
    leg_budget: Duration,
) -> Arc<dyn clovy_domain::ImageGenerator> {
    Arc::new(FalImageGenerator::from_config(
        upstream_http.clone(),
        fal,
        leg_budget,
    ))
}

fn build_image_editor(
    upstream_http: &reqwest::Client,
    fal: &clovy_config::UpstreamConfig,
    leg_budget: Duration,
) -> Arc<dyn clovy_domain::ImageEditor> {
    Arc::new(FalImageEditor::from_config(
        upstream_http.clone(),
        fal,
        leg_budget,
    ))
}

fn build_video_provider(
    upstream_http: &reqwest::Client,
    fal: &clovy_config::UpstreamConfig,
    call_timeout: Duration,
    max_response_bytes: u64,
) -> Arc<dyn clovy_domain::VideoProvider> {
    Arc::new(FalVideoProvider::from_config(
        upstream_http.clone(),
        fal,
        call_timeout,
        max_response_bytes,
    ))
}

fn build_os_accounts_client(
    config: &AppConfig,
    http: &reqwest::Client,
) -> Arc<dyn clovy_domain::OsAccountsClient> {
    if config.local_dev.enabled {
        tracing::warn!("local dev mode enabled; OS Accounts metering is disabled");
        Arc::new(LocalDevOsAccountsClient)
    } else {
        Arc::new(OsAccountsHttpClient::from_config(
            http.clone(),
            &config.os_accounts,
        ))
    }
}

fn build_token_verifier(config: &AppConfig) -> Arc<dyn clovy_domain::TokenVerifier> {
    if config.local_dev.enabled {
        Arc::new(
            LocalDevTokenVerifier::new(
                config.local_dev.bearer_token.clone(),
                config.local_dev.user_id.clone(),
            )
            .with_viewer(
                config.local_dev.viewer_bearer_token.clone(),
                config.local_dev.viewer_user_id.clone(),
            ),
        )
    } else {
        Arc::new(JwksTokenVerifier::from_config(
            jwks_client(),
            &config.os_accounts,
        ))
    }
}

fn build_issue_report_sink(
    config: &AppConfig,
    http: &reqwest::Client,
) -> Arc<dyn clovy_domain::IssueReportSink> {
    if let Some(sink) = OsPlatformIssueReportSink::from_config(http.clone(), &config.issue_reports)
    {
        tracing::info!("issue reports will be filed as os-platform issues");
        Arc::new(sink)
    } else {
        tracing::info!("no issue report sink configured; reports will be logged only");
        Arc::new(LogIssueReportSink)
    }
}

fn build_p3a_sink(config: &AppConfig, http: &reqwest::Client) -> Arc<dyn clovy_domain::P3aSink> {
    if config.local_dev.enabled {
        tracing::info!("local dev mode enabled; P3A reports will be logged only");
        Arc::new(LogP3aSink)
    } else if let Some(sink) = OsAccountsP3aSink::from_config(http.clone(), &config.os_accounts) {
        tracing::info!("P3A reports will be forwarded to OS Accounts");
        Arc::new(sink)
    } else {
        tracing::warn!(
            "P3A ingest disabled: CLOVY__OS_ACCOUNTS__P3A_INGEST_TOKEN not set; reports will be logged only"
        );
        Arc::new(LogP3aSink)
    }
}

fn init_tracing() {
    let _ = tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                "clovy_api=info,clovy_services=info,clovy_providers=info,tower_http=info".into()
            }),
        )
        .with(tracing_subscriber::fmt::layer().json())
        .try_init();
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path},
    };

    /// Local mode is the only mode: a default config, with no OS Accounts
    /// settings and no opt-in flag, authenticates the local bearer token and
    /// rejects requests without it.
    #[tokio::test]
    async fn default_config_authenticates_the_local_token() {
        let config = AppConfig::default();
        let http = reqwest::Client::new();
        let app = build_router(
            &config,
            HttpClients {
                default: &http,
                upstream: &http,
                metered_inference: &http,
                issue_reports: &http,
            },
            Arc::new(PricingTable::new(config.pricing.clone())),
            RuntimeStores {
                share: None,
                companion: CompanionRuntime {
                    store: None,
                    snapshot: clovy_domain::CompanionSnapshot::default(),
                    enabled: true,
                    push: None,
                },
            },
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await });
        let url = format!("http://{address}/v1/video/status/vid_missing");

        let with_token = http
            .get(&url)
            .bearer_auth(clovy_config::LOCAL_DEV_BEARER_TOKEN_PLACEHOLDER)
            .send()
            .await
            .unwrap();
        let without_token = http.get(&url).send().await.unwrap();
        server.abort();

        assert_ne!(with_token.status(), reqwest::StatusCode::UNAUTHORIZED);
        assert_eq!(without_token.status(), reqwest::StatusCode::UNAUTHORIZED);
    }

    /// Local dev without service keys used to drop every priced model (an
    /// empty catalog, so dictation failed `model_not_priced`). The catalog now
    /// keeps the packaged Groq and `OpenRouter` rows plus the live `OpenRouter`
    /// list whether or not any key is configured, so a user's own key can use
    /// them.
    #[tokio::test]
    async fn catalog_keeps_every_priced_model_without_service_keys() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/models"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{
                    "id": "vendor/new-text-model",
                    "name": "Vendor: New text model",
                    "description": "A text model listed only by the live catalog.",
                    "context_length": 8192,
                    "architecture": {
                        "input_modalities": ["text"],
                        "output_modalities": ["text"]
                    },
                    "pricing": { "prompt": "0.000001", "completion": "0.000002" },
                    "supported_parameters": ["tools"]
                }]
            })))
            .mount(&server)
            .await;
        let mut config = AppConfig::default();
        config.local_dev.enabled = true;
        config.upstreams.openrouter.base_url = server.uri();

        let pricing = try_load_pricing(&config, reqwest::Client::new())
            .await
            .unwrap_or_default();

        for model_id in [
            "whisper-large-v3-turbo",
            "whisper-large-v3",
            "z-ai/glm-5.2",
            "vendor/new-text-model",
        ] {
            assert!(pricing.contains_key(model_id), "{model_id} missing");
        }
    }
}
