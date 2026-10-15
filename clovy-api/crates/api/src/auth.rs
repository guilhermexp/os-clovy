use crate::{error::ApiError, state::ApiState, validation};
use axum::http::{HeaderMap, header};
use clovy_domain::{ProviderCredentials, UpstreamProvider, UserId};

pub(crate) const GROQ_API_KEY_HEADER: &str = "x-groq-api-key";
pub(crate) const OPENROUTER_API_KEY_HEADER: &str = "x-openrouter-api-key";
pub(crate) const FAL_API_KEY_HEADER: &str = "x-fal-api-key";
pub(crate) const TAVILY_API_KEY_HEADER: &str = "x-tavily-api-key";
pub(crate) const CREDITS_SPEND_SCOPE: &str = "credits:spend";
pub(crate) const PROFILE_READ_SCOPE: &str = "profile:read";

pub(crate) async fn authenticated_user(
    state: &ApiState,
    headers: &HeaderMap,
) -> Result<UserId, ApiError> {
    authenticated_user_with_scope(state, headers, CREDITS_SPEND_SCOPE).await
}

pub(crate) async fn authenticated_user_with_scope(
    state: &ApiState,
    headers: &HeaderMap,
    required_scope: &str,
) -> Result<UserId, ApiError> {
    let token = bearer_token(headers)?;
    state
        .token_verifier()
        .verify_scope(token, required_scope)
        .await
        .map_err(|error| crate::error::from_auth_error(&error))
}

pub(crate) fn client_address(headers: &HeaderMap) -> String {
    // dstack-ingress appends the peer address as the final x-forwarded-for
    // entry. Everything before it arrived from the client and is spoofable,
    // including earlier header lines when the ingress appends its own line
    // instead of extending the client's.
    headers
        .get_all("x-forwarded-for")
        .iter()
        .next_back()
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(',').next_back())
        .map_or_else(|| "unknown".to_string(), |value| value.trim().to_string())
}

pub(crate) fn bearer_token(headers: &HeaderMap) -> Result<&str, ApiError> {
    let value = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| ApiError::unauthorized("missing_bearer_token"))?;
    value
        .strip_prefix("Bearer ")
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .ok_or_else(|| ApiError::unauthorized("missing_bearer_token"))
}

/// Provider credentials ride as `x-<provider>-api-key`. A route accepts only its
/// own provider's header; any other non-blank one (another current provider's,
/// or a retired provider's a shipped client still sends) is refused before
/// funding or forwarding, so a key never pays for, or reaches, a provider it
/// was not meant for.
pub(crate) fn provider_credentials(
    headers: &HeaderMap,
    accepted: UpstreamProvider,
) -> Result<ProviderCredentials, ApiError> {
    let accepted_header = provider_header_name(accepted);
    for (name, value) in headers {
        if name.as_str() == accepted_header {
            continue;
        }
        let Some(provider) = credential_header_provider(name.as_str()) else {
            continue;
        };
        let is_non_blank = value.to_str().map_or(true, |text| !text.trim().is_empty());
        if is_non_blank {
            return Err(ApiError::bad_request(format!(
                "{provider}_api_key_not_accepted"
            )));
        }
    }

    let Some(raw_value) = headers.get(accepted_header) else {
        return Ok(ProviderCredentials::default());
    };

    let raw_str = raw_value
        .to_str()
        .map_err(|_| ApiError::bad_request(format!("{}_api_key_invalid", accepted.as_str())))?;
    let trimmed = raw_str.trim();
    if trimmed.is_empty() {
        return Ok(ProviderCredentials::default());
    }
    if raw_str
        .chars()
        .any(|character| !character.is_ascii() || character.is_control())
    {
        return Err(ApiError::bad_request(format!(
            "{}_api_key_invalid",
            accepted.as_str()
        )));
    }
    if trimmed.len() > validation::MAX_PROVIDER_API_KEY_CHARS {
        return Err(ApiError::bad_request(format!(
            "{}_api_key_too_long",
            accepted.as_str()
        )));
    }

    Ok(ProviderCredentials::for_provider(
        accepted,
        trimmed.to_string(),
    ))
}

fn provider_header_name(provider: UpstreamProvider) -> &'static str {
    match provider {
        UpstreamProvider::Groq => GROQ_API_KEY_HEADER,
        UpstreamProvider::Openrouter => OPENROUTER_API_KEY_HEADER,
        UpstreamProvider::Fal => FAL_API_KEY_HEADER,
        UpstreamProvider::Tavily => TAVILY_API_KEY_HEADER,
    }
}

/// The provider a `x-<provider>-api-key` header names, when it is a short
/// lowercase token; any other header is not a provider credential.
fn credential_header_provider(name: &str) -> Option<&str> {
    const MAX_PROVIDER_NAME_CHARS: usize = 32;
    name.strip_prefix("x-")
        .and_then(|rest| rest.strip_suffix("-api-key"))
        .filter(|provider| {
            !provider.is_empty()
                && provider.len() <= MAX_PROVIDER_NAME_CHARS
                && provider
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        })
}

#[cfg(test)]
mod tests {
    use super::{
        FAL_API_KEY_HEADER, GROQ_API_KEY_HEADER, OPENROUTER_API_KEY_HEADER, TAVILY_API_KEY_HEADER,
        client_address, provider_credentials,
    };
    use axum::http::{HeaderMap, HeaderValue};
    use clovy_domain::UpstreamProvider;
    use std::error::Error;

    #[test]
    fn provider_credentials_accepts_opaque_key_for_accepted_provider() -> Result<(), Box<dyn Error>>
    {
        let mut headers = HeaderMap::new();
        headers.insert(
            OPENROUTER_API_KEY_HEADER,
            HeaderValue::from_static("sk-or-v1-user-secret"),
        );

        let credentials = provider_credentials(&headers, UpstreamProvider::Openrouter)?;
        assert_eq!(
            credentials.user_key(UpstreamProvider::Openrouter),
            Some("sk-or-v1-user-secret")
        );
        Ok(())
    }

    #[test]
    fn provider_credentials_rejects_foreign_provider_header() -> Result<(), Box<dyn Error>> {
        let mut headers = HeaderMap::new();
        headers.insert(
            GROQ_API_KEY_HEADER,
            HeaderValue::from_static("gsk_user_secret"),
        );

        let error = provider_credentials(&headers, UpstreamProvider::Openrouter)
            .err()
            .ok_or("expected error")?;
        assert!(matches!(
            error,
            crate::ApiError::BadRequest { message, .. } if message == "groq_api_key_not_accepted"
        ));
        Ok(())
    }

    #[test]
    fn provider_credentials_rejects_a_retired_provider_header_but_not_other_headers()
    -> Result<(), Box<dyn Error>> {
        // A shipped client may still send its old provider's key; it is refused
        // like any other foreign key, never charged to Clovy or forwarded.
        let mut headers = HeaderMap::new();
        headers.insert("x-venice-api-key", HeaderValue::from_static("legacy-key"));
        let error = provider_credentials(&headers, UpstreamProvider::Openrouter)
            .err()
            .ok_or("expected error")?;
        assert!(matches!(
            error,
            crate::ApiError::BadRequest { message, .. } if message == "venice_api_key_not_accepted"
        ));

        let mut unrelated = HeaderMap::new();
        unrelated.insert("x-api-key", HeaderValue::from_static("value"));
        unrelated.insert("x-clovy-app-version", HeaderValue::from_static("0.1.0"));
        assert!(provider_credentials(&unrelated, UpstreamProvider::Openrouter).is_ok());
        Ok(())
    }

    #[test]
    fn provider_credentials_ignores_blank_foreign_header() -> Result<(), Box<dyn Error>> {
        let mut headers = HeaderMap::new();
        headers.insert(GROQ_API_KEY_HEADER, HeaderValue::from_static("   "));
        headers.insert(
            OPENROUTER_API_KEY_HEADER,
            HeaderValue::from_static("sk-or-v1-user-secret"),
        );

        let credentials = provider_credentials(&headers, UpstreamProvider::Openrouter)?;
        assert_eq!(
            credentials.user_key(UpstreamProvider::Openrouter),
            Some("sk-or-v1-user-secret")
        );
        Ok(())
    }

    #[test]
    fn provider_credentials_rejects_control_characters() -> Result<(), Box<dyn Error>> {
        let mut headers = HeaderMap::new();
        headers.insert(FAL_API_KEY_HEADER, HeaderValue::from_static("invalid\tkey"));

        let error = provider_credentials(&headers, UpstreamProvider::Fal)
            .err()
            .ok_or("expected error")?;
        assert!(matches!(
            error,
            crate::ApiError::BadRequest { message, .. } if message == "fal_api_key_invalid"
        ));
        Ok(())
    }

    #[test]
    fn provider_credentials_rejects_too_long_key() -> Result<(), Box<dyn Error>> {
        let mut headers = HeaderMap::new();
        let long_key = "a".repeat(crate::validation::MAX_PROVIDER_API_KEY_CHARS + 1);
        let header_val = HeaderValue::from_str(&long_key)?;
        headers.insert(TAVILY_API_KEY_HEADER, header_val);

        let error = provider_credentials(&headers, UpstreamProvider::Tavily)
            .err()
            .ok_or("expected error")?;
        assert!(matches!(
            error,
            crate::ApiError::BadRequest { message, .. } if message == "tavily_api_key_too_long"
        ));
        Ok(())
    }

    #[test]
    fn client_address_uses_the_ingress_appended_final_hop() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-forwarded-for",
            HeaderValue::from_static("203.0.113.9, 198.51.100.7"),
        );

        assert_eq!(client_address(&headers), "198.51.100.7");
    }

    #[test]
    fn client_address_uses_the_last_x_forwarded_for_header_line() {
        let mut headers = HeaderMap::new();
        headers.append(
            "x-forwarded-for",
            HeaderValue::from_static("203.0.113.9, 192.0.2.1"),
        );
        headers.append("x-forwarded-for", HeaderValue::from_static("198.51.100.7"));

        assert_eq!(client_address(&headers), "198.51.100.7");
    }
}
