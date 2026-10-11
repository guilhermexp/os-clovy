//! Real HTTP provider + real settlement, with loopback recording upstream and
//! OS Accounts substitutes. No real account, key or inference is used.
#![allow(clippy::unwrap_used, clippy::too_many_lines)]

use clovy_config::{AppConfig, UpstreamConfig};
use clovy_domain::{ModelId, ProviderCredentials, UserId};
use clovy_providers::{http, openrouter::OpenRouterAgentChat, os_accounts::OsAccountsHttpClient};
use clovy_services::{AgentChatParams, AgentChatService, AgentChatServiceDeps, PricingTable};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

#[tokio::test]
async fn regressions_f1_real_stream_settlement_releases_failed_hold_and_charges_success() {
    for (body, expected) in [
        (
            "data: {\"error\":{\"code\":\"server_error\"},\"choices\":[{\"delta\":{},\"finish_reason\":\"error\"}]}\n\n",
            0,
        ),
        (
            "data: {\"choices\":[{\"delta\":{\"content\":\"answer\"}}]}\n\ndata: [DONE]\n\n",
            40,
        ),
        (
            "data: {\"choices\":[{\"delta\":{\"content\":\"partial\"}}]}\n\ndata: {\"error\":{\"code\":503}}\n\n",
            0,
        ),
        (
            "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":2,\"completion_tokens\":3}}\n\ndata: [DONE]\n\n",
            5,
        ),
    ] {
        let upstream = MockServer::start().await;
        let accounts = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(body, "text/event-stream"))
            .mount(&upstream)
            .await;
        Mock::given(path("/authorize")).respond_with(ResponseTemplate::new(200).set_body_json(json!({"success":true,"data":{"allowed":true,"token":"fixture-hold","cap_credits":40}}))).mount(&accounts).await;
        Mock::given(path("/charge")).respond_with(ResponseTemplate::new(200).set_body_json(json!({"success":true,"data":{"credits_settled":expected,"idempotent_replay":false}}))).mount(&accounts).await;
        let mut prices = AppConfig::default().pricing;
        let model = "z-ai/glm-5.2";
        let price = prices.get_mut(model).unwrap();
        price.input_credits_per_million_tokens = Some(1_000_000);
        price.output_credits_per_million_tokens = Some(1_000_000);
        let service = AgentChatService::new(AgentChatServiceDeps {
            pricing: Arc::new(PricingTable::new(prices)),
            os_accounts: Arc::new(OsAccountsHttpClient::new(
                http::default_client(),
                &accounts.uri(),
                "fixture",
            )),
            chat_completer: Arc::new(OpenRouterAgentChat::from_config(
                http::default_client(),
                http::default_client(),
                &UpstreamConfig {
                    api_key: String::new(),
                    base_url: upstream.uri(),
                },
            )),
            hold_ttl_seconds: 60,
            flat_estimate_credits: 100,
        });
        let mut output = service
            .complete_stream(AgentChatParams {
                user_id: UserId("fixture-user".into()),
                model_id: ModelId(model.into()),
                body: json!({"stream":true,"messages":[]}),
                provider_credentials: ProviderCredentials::default(),
            })
            .await
            .unwrap();
        while output.chunks.recv().await.is_some() {}
        let settled = tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let requests = accounts.received_requests().await.unwrap();
                if let Some(request) = requests.iter().find(|r| r.url.path() == "/charge") {
                    break serde_json::from_slice::<Value>(&request.body).unwrap();
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(
            settled["credits"], expected,
            "failed stream must debit zero; successful stream must settle"
        );
        assert_eq!(
            settled["token"], "fixture-hold",
            "settlement releases this authorization"
        );
        assert_eq!(
            settled["idempotency_key"]
                .as_str()
                .unwrap()
                .starts_with("release:agent_chat:"),
            expected == 0
        );
        assert_eq!(
            accounts
                .received_requests()
                .await
                .unwrap()
                .iter()
                .filter(|r| r.url.path() == "/authorize")
                .count(),
            1
        );
    }
}
