use super::*;
use crate::http;
use clovy_domain::ModelId;
use serde_json::json;
use wiremock::{Mock, MockServer, ResponseTemplate, matchers::method};

#[tokio::test]
async fn f1_failed_or_empty_stream_never_completes_without_usage() {
    for body in [
        "",
        ": processing\n\ndata: [DONE]\n\n",
        "data: {\"choices\":[]}\n\n",
        "data: {\"error\":{\"code\":503,\"message\":\"unavailable\"}}\n\n",
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"error\"}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"partial\"}}]}\n\ndata: {\"error\":{\"code\":\"server_error\"}}\n\n",
        "data: {\"error\":{},\"usage\":{\"prompt_tokens\":2,\"completion_tokens\":0}}\n\n",
    ] {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(body, "text/event-stream"))
            .mount(&server)
            .await;
        let agent = OpenRouterAgentChat::from_config(
            http::default_client(),
            http::default_client(),
            &UpstreamConfig {
                api_key: String::new(),
                base_url: server.uri(),
            },
        );
        let stream = agent
            .complete_stream(AgentChatRequest {
                body: json!({"messages":[], "stream":true}),
                model: ModelId("text-model".into()),
                provider_credentials: ProviderCredentials::default(),
                price_cap: None,
                unmetered: false,
            })
            .await
            .unwrap();
        assert_eq!(
            stream.outcome.await.unwrap(),
            AgentChatStreamOutcome::Failed,
            "failed/no-response stream must not debit"
        );
    }
}

#[test]
fn f4_request_keeps_all_required_properties_and_nested_constraints() {
    let body = prepare_agent_chat_body(json!({"messages":[], "tools":[{"type":"function","function":{"name":"test","parameters":{
        "type":"object", "properties":{"a":{"type":"string"}, "nested":{"type":"object","properties":{"x":{"type":"string"}},"required":["x"]}}, "required":["a"],
        "allOf":[
            {"properties":{"b":{"type":"string","pattern":"^b"},"nested":{"type":"object","properties":{"y":{"type":"number"}},"required":["y"]}},"required":["b","a"]},
            {"properties":{"c":{"type":"boolean"}},"required":["c"]}
        ]
    }}}]}), &ModelId("text-model".into()), None).unwrap();
    let schema = &body["tools"][0]["function"]["parameters"];
    assert_eq!(schema["required"], json!(["a", "b", "c"]));
    assert_eq!(schema["properties"]["b"]["pattern"], "^b");
    assert_eq!(schema["properties"]["c"]["type"], "boolean");
    assert_eq!(
        schema["properties"]["nested"]["required"],
        json!(["x", "y"])
    );
    assert_eq!(
        schema["properties"]["nested"]["properties"]["y"]["type"],
        "number"
    );
}

#[test]
fn f4_branches_without_parent_containers_are_combined() {
    let mut schema = json!({"type":"object","allOf":[{"properties":{"a":{"type":"string"}},"required":["a"]},{"properties":{"b":{"type":"number"}},"required":["a","b"]}]});
    strip_conflicting_all_of(&mut schema);
    assert_eq!(schema["required"], json!(["a", "b"]));
    assert_eq!(schema["properties"]["b"]["type"], "number");
}
