use crate::{
    auth::{authenticated_user, provider_credentials},
    error::ApiError,
    handlers::notes::{AgentModelRequirements, AutoSelection, resolve_text_model_and_credentials},
    state::ApiState,
    validation,
};
use axum::{
    Json,
    body::Body,
    extract::State,
    http::{HeaderMap, HeaderValue, StatusCode, header::CONTENT_TYPE},
    response::{IntoResponse, Response},
};
use clovy_domain::{ModelId, UpstreamProvider, UpstreamRouteMetadata};
use clovy_services::{AgentChatParams, AgentChatStreamOutput};
use tokio_stream::wrappers::UnboundedReceiverStream;

pub(crate) async fn chat_completions(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(mut body): Json<serde_json::Value>,
) -> Result<Response, ApiError> {
    let user_id = authenticated_user(&state, &headers).await?;
    let provider_credentials = provider_credentials(&headers, UpstreamProvider::Openrouter)?;
    let requested_model_id = body
        .get("model")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ApiError::bad_request("model_required"))?
        .to_string();
    validation::validate_text_len("model", &requested_model_id, validation::MAX_MODEL_CHARS)?;
    validation::validate_agent_chat_body(&body)?;

    let cost_quality = if let Some(auto_value) = body.get("auto") {
        if let Some(cq) = auto_value.get("cost_quality") {
            let cq_num = cq
                .as_f64()
                .ok_or_else(|| ApiError::unprocessable("cost_quality_invalid"))?;
            if !cq_num.is_finite() || !(0.0..=1.0).contains(&cq_num) {
                return Err(ApiError::unprocessable("cost_quality_invalid"));
            }
            Some(cq_num)
        } else {
            None
        }
    } else {
        None
    };

    if let Some(object) = body.as_object_mut() {
        object.remove("auto");
    }

    let requirements = AgentModelRequirements::from_body(&body);
    let (model_id, provider_credentials) = resolve_text_model_and_credentials(
        &state,
        &requested_model_id,
        AutoSelection {
            cost_quality,
            requirements: Some(requirements),
        },
        provider_credentials,
    )?;
    state.ensure_provider_configured(UpstreamProvider::Openrouter, &provider_credentials)?;

    if let Some(object) = body.as_object_mut() {
        object.insert(
            "model".to_string(),
            serde_json::Value::String(model_id.clone()),
        );
    }
    if body.get("stream").and_then(serde_json::Value::as_bool) == Some(true) {
        let output = state
            .agent_chat()
            .complete_stream(AgentChatParams {
                user_id,
                model_id: ModelId(model_id),
                body,
                provider_credentials,
            })
            .await?;
        // The route TimeoutLayer bounds this handler future; the streamed body
        // is bounded by the upstream reqwest client's total timeout.
        let AgentChatStreamOutput {
            content_type,
            provider,
            route,
            chunks,
        } = output;
        let mut response = (
            StatusCode::OK,
            [(CONTENT_TYPE, content_type)],
            Body::from_stream(UnboundedReceiverStream::new(chunks)),
        )
            .into_response();
        insert_upstream_route_headers(response.headers_mut(), &provider, &route);
        return Ok(response);
    }
    let output = state
        .agent_chat()
        .complete(AgentChatParams {
            user_id,
            model_id: ModelId(model_id),
            body,
            provider_credentials,
        })
        .await?;
    let completion = output.completion;
    let mut response = (
        StatusCode::OK,
        [(CONTENT_TYPE, completion.content_type)],
        completion.body,
    )
        .into_response();
    insert_upstream_route_headers(
        response.headers_mut(),
        &completion.provider,
        &completion.route,
    );
    Ok(response)
}

fn insert_upstream_route_headers(
    headers: &mut HeaderMap,
    provider: &str,
    route: &UpstreamRouteMetadata,
) {
    for (name, value) in [
        (
            "x-os-provider",
            route.provider.as_deref().or(Some(provider)),
        ),
        ("x-os-privacy-level", route.privacy_level.as_deref()),
        ("x-os-endpoint", route.endpoint.as_deref()),
    ] {
        if let Some(value) = value
            && let Ok(value) = HeaderValue::from_str(value)
        {
            headers.insert(name, value);
        }
    }
}

impl AgentModelRequirements {
    pub(crate) fn from_body(body: &serde_json::Value) -> Self {
        Self {
            vision: body.get("messages").is_some_and(chat_items_contain_image),
            tools: body
                .get("tools")
                .and_then(serde_json::Value::as_array)
                .is_some_and(|tools| !tools.is_empty()),
        }
    }
}

fn chat_items_contain_image(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Array(values) => values.iter().any(chat_items_contain_image),
        serde_json::Value::Object(object) => {
            matches!(
                object.get("type").and_then(serde_json::Value::as_str),
                Some("image_url")
            ) || object.get("content").is_some_and(chat_items_contain_image)
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::AgentModelRequirements;
    use crate::handlers::notes::has_capability;

    #[test]
    fn infers_image_and_tool_requirements_from_chat_content() {
        let body = serde_json::json!({
            "messages": [{
                "role": "user",
                "content": [{
                    "type": "image_url",
                    "image_url": { "url": "https://example.com/image.png" }
                }]
            }],
            "tools": [{ "type": "function" }]
        });

        assert_eq!(
            AgentModelRequirements::from_body(&body),
            AgentModelRequirements {
                vision: true,
                tools: true,
            }
        );
    }

    #[test]
    fn ignores_image_like_content_outside_supported_message_content() {
        let body = serde_json::json!({
            "messages": [{
                "role": "user",
                "content": "hello",
                "metadata": { "type": "image_url" }
            }],
            "input": [{
                "content": [{ "type": "image_url", "image_url": "unsupported-here" }]
            }]
        });

        assert_eq!(
            AgentModelRequirements::from_body(&body),
            AgentModelRequirements::default()
        );
    }

    #[test]
    fn capability_matching_uses_the_catalog_names_normalized() {
        assert!(has_capability(
            &["supports_vision".to_string()],
            "supportsvision"
        ));
        assert!(has_capability(
            &["parent.supportsVision".to_string()],
            "supportsvision"
        ));
        assert!(!has_capability(
            &["supportsVision".to_string()],
            "supportsfunctioncalling"
        ));
    }
}
