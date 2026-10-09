//! Tavily web search and page extraction behind the agent's web tools.
//!
//! `POST /search` and `POST /extract`
//! (<https://docs.tavily.com/documentation/api-reference/endpoint/search>,
//! <https://docs.tavily.com/documentation/api-reference/endpoint/extract>),
//! authenticated with `Authorization: Bearer <key>`: the user's own Tavily key
//! when the request carries one, else Clovy's service key, never another
//! provider's credential. Both run at the `basic` depth the flat web credits are
//! sized for (search: 1 credit; extract: 1 credit per 5 successful URLs, failed
//! URLs free, <https://docs.tavily.com/documentation/api-credits>). A single-URL
//! extract therefore averages 0.2 credit; Tavily's optional per-request usage
//! figure can read 0 until five successful extractions accumulate, which does
//! not make a call free (this adapter neither requests nor reads it).
//!
//! Results are normalized into the existing tool contract: search rows without
//! a URL are dropped, a blank title falls back to the URL, `content` is the
//! snippet and `published_date` the publication date. Extract answers 200 even
//! when every URL failed (`failed_results`); a page with no extracted content is
//! a usable `web_fetch_unsupported_url`, never placeholder text. The public-URL
//! checks stay at the `/v1/web/fetch` boundary; this adapter forwards the
//! canonical URL only. Each call is sent exactly once: Tavily documents no
//! idempotency, so a second POST after a lost answer could spend a credit the
//! user never sees. A transport error, 429 or 5xx is returned as a provider
//! failure instead.

use async_trait::async_trait;
use clovy_config::UpstreamConfig;
use clovy_domain::{
    DomainError, ProviderCredentials, WebFetchRequest, WebFetchResult, WebFetcher,
    WebSearchRequest, WebSearchResult, WebSearchResults, WebSearcher,
};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize, de::DeserializeOwned};

/// Provider label written into web results.
pub const PROVIDER_NAME: &str = "tavily";

const SEARCH_DEPTH: &str = "basic";
const EXTRACT_DEPTH: &str = "basic";
const EXTRACT_FORMAT: &str = "markdown";
/// "Key limit or Plan Limit exceeded" (search and extract API references).
const KEY_OR_PLAN_LIMIT_EXCEEDED: u16 = 432;
/// "`PayGo` limit exceeded" (search and extract API references).
const PAYGO_LIMIT_EXCEEDED: u16 = 433;

/// One client for both web traits: they share the base URL and credential.
pub struct TavilyAugment {
    http: reqwest::Client,
    api_key: String,
    base_url: String,
}

impl TavilyAugment {
    pub fn from_config(http: reqwest::Client, config: &UpstreamConfig) -> Self {
        Self {
            http,
            api_key: config.api_key.clone(),
            base_url: config.base_url.trim().trim_end_matches('/').to_string(),
        }
    }

    /// POSTs `body` to one endpoint once and parses the answer.
    async fn post<B, T>(
        &self,
        endpoint: Endpoint,
        body: &B,
        credentials: &ProviderCredentials,
    ) -> Result<T, DomainError>
    where
        B: Serialize + Sync,
        T: DeserializeOwned,
    {
        let key = TavilyKey::for_request(&self.api_key, credentials);
        let url = format!("{}{}", self.base_url, endpoint.path);
        let response = self
            .http
            .post(&url)
            .bearer_auth(key.value)
            .json(body)
            .send()
            .await
            .map_err(|error| {
                // A send error renders its kind and this fixed endpoint URL
                // (the key travels in a header); no response body exists yet.
                tracing::error!(%error, %url, "tavily: transport error");
                DomainError::UpstreamProvider
            })?;
        // Only the endpoint Clovy built may answer: a redirect elsewhere is
        // refused (reqwest drops the bearer when a redirect leaves the origin).
        if !reqwest::Url::parse(&url).is_ok_and(|expected| &expected == response.url()) {
            tracing::error!(%url, "tavily: request was redirected");
            return Err(DomainError::UpstreamProvider);
        }
        let status = response.status();
        if !status.is_success() {
            let body_bytes = response.bytes().await.as_ref().map_or(0, bytes::Bytes::len);
            tracing::error!(%status, %url, body_bytes, "tavily: non-success response");
            return Err(status_error(status, endpoint, key.user_supplied));
        }
        let body = response.bytes().await.map_err(|_| {
            tracing::error!(%url, "tavily: reading the response failed");
            DomainError::UpstreamProvider
        })?;
        serde_json::from_slice::<T>(&body).map_err(|error| {
            // Category only: serde's message can quote values from the body.
            tracing::error!(
                %url,
                category = ?error.classify(),
                "tavily: response does not match the API schema"
            );
            DomainError::UpstreamProvider
        })
    }
}

#[async_trait]
impl WebSearcher for TavilyAugment {
    async fn search(&self, request: WebSearchRequest) -> Result<WebSearchResults, DomainError> {
        let wire: SearchResponse = self
            .post(
                SEARCH,
                &SearchBody {
                    query: &request.query,
                    search_depth: SEARCH_DEPTH,
                    include_published_date: true,
                    max_results: request.limit,
                },
                &request.provider_credentials,
            )
            .await?;
        let results = wire
            .results
            .into_iter()
            .filter_map(|result| {
                // A row without a URL cannot be cited; drop it.
                let url = non_blank(result.url)?;
                Some(WebSearchResult {
                    title: non_blank(result.title).unwrap_or_else(|| url.clone()),
                    snippet: non_blank(result.content),
                    published_at: non_blank(result.published_date),
                    url,
                })
            })
            .collect();
        Ok(WebSearchResults {
            query: request.query,
            provider: PROVIDER_NAME.to_string(),
            results,
        })
    }
}

#[async_trait]
impl WebFetcher for TavilyAugment {
    async fn fetch(&self, request: WebFetchRequest) -> Result<WebFetchResult, DomainError> {
        let wire: ExtractResponse = self
            .post(
                EXTRACT,
                &ExtractBody {
                    urls: [request.url.as_str()],
                    extract_depth: EXTRACT_DEPTH,
                    format: EXTRACT_FORMAT,
                },
                &request.provider_credentials,
            )
            .await?;
        let failed_urls = wire.failed_results.len();
        let Some((url, content)) = wire.results.into_iter().find_map(|result| {
            let content = non_blank(result.raw_content)?;
            Some((non_blank(result.url), content))
        }) else {
            tracing::warn!(failed_urls, "tavily: extract returned no content");
            return Err(invalid_input(EXTRACT.client_error_reason));
        };
        Ok(WebFetchResult {
            url: url.unwrap_or(request.url),
            content,
            format: EXTRACT_FORMAT.to_string(),
            provider: PROVIDER_NAME.to_string(),
        })
    }
}

/// The credential one call runs under.
#[derive(Clone, Copy)]
struct TavilyKey<'a> {
    value: &'a str,
    user_supplied: bool,
}

impl<'a> TavilyKey<'a> {
    fn for_request(configured: &'a str, credentials: &'a ProviderCredentials) -> Self {
        match credentials
            .tavily_api_key
            .as_deref()
            .map(str::trim)
            .filter(|key| !key.is_empty())
        {
            Some(value) => Self {
                value,
                user_supplied: true,
            },
            None => Self {
                value: configured,
                user_supplied: false,
            },
        }
    }
}

#[derive(Clone, Copy)]
struct Endpoint {
    path: &'static str,
    /// Reason for a 400/422: Tavily refused this specific input.
    client_error_reason: &'static str,
}

const SEARCH: Endpoint = Endpoint {
    path: "/search",
    client_error_reason: "web_search_rejected",
};
const EXTRACT: Endpoint = Endpoint {
    path: "/extract",
    client_error_reason: "web_fetch_unsupported_url",
};

/// A 400/422 is Tavily refusing this input (for extract, "All URLs failed
/// validation"). For the user's own key, a 401 ("Your API key is wrong or
/// missing"; an undocumented 403 is read the same way) means the key itself
/// was refused, while 432/433 mean a key hit its plan or pay-as-you-go limit:
/// two different things for the user to fix, so they get two reasons.
/// Everything else, including Clovy's own key being refused or over its
/// limit, 429 and 5xx, is a provider failure.
fn status_error(status: StatusCode, endpoint: Endpoint, user_supplied_key: bool) -> DomainError {
    if status == StatusCode::BAD_REQUEST || status == StatusCode::UNPROCESSABLE_ENTITY {
        return invalid_input(endpoint.client_error_reason);
    }
    if user_supplied_key {
        if matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) {
            return invalid_input("tavily_api_key_rejected");
        }
        if matches!(
            status.as_u16(),
            KEY_OR_PLAN_LIMIT_EXCEEDED | PAYGO_LIMIT_EXCEEDED
        ) {
            return invalid_input("tavily_usage_limit_exceeded");
        }
    }
    DomainError::UpstreamProvider
}

fn invalid_input(reason: &str) -> DomainError {
    DomainError::InvalidInput {
        reason: reason.to_string(),
    }
}

fn non_blank(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.trim().is_empty())
}

#[derive(Serialize)]
struct SearchBody<'a> {
    query: &'a str,
    search_depth: &'static str,
    /// Asks for `published_date` on every row (Tavily marks it beta); rows
    /// without a detectable date carry `null`.
    include_published_date: bool,
    /// `None` keeps Tavily's default (10); the HTTP boundary clamps to 1..=20,
    /// Tavily's own range.
    #[serde(skip_serializing_if = "Option::is_none")]
    max_results: Option<u32>,
}

/// `results` is required by the search schema: an HTTP 200 without it is a
/// failure Tavily answered with 200, never an empty result list.
#[derive(Deserialize)]
struct SearchResponse {
    results: Vec<SearchResultWire>,
}

#[derive(Deserialize)]
struct SearchResultWire {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    published_date: Option<String>,
}

#[derive(Serialize)]
struct ExtractBody<'a> {
    urls: [&'a str; 1],
    extract_depth: &'static str,
    format: &'static str,
}

#[derive(Deserialize)]
struct ExtractResponse {
    #[serde(default)]
    results: Vec<ExtractResultWire>,
    /// Only counted: each `error` is free-form text about the page.
    #[serde(default)]
    failed_results: Vec<serde::de::IgnoredAny>,
}

#[derive(Deserialize)]
struct ExtractResultWire {
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    raw_content: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::TavilyAugment;
    use crate::http;
    use clovy_config::UpstreamConfig;
    use clovy_domain::{
        DomainError, ProviderCredentials, WebFetchRequest, WebFetcher, WebSearchRequest,
        WebSearcher,
    };
    use pretty_assertions::assert_eq;
    use serde_json::json;
    use std::time::Duration;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{body_partial_json, header, method, path},
    };

    fn augment(server: &MockServer) -> TavilyAugment {
        TavilyAugment::from_config(
            http::default_client(),
            &UpstreamConfig {
                api_key: "service_key".to_string(),
                base_url: server.uri(),
            },
        )
    }

    fn search(limit: Option<u32>) -> WebSearchRequest {
        WebSearchRequest {
            query: "rust async".to_string(),
            limit,
            provider_credentials: ProviderCredentials::default(),
        }
    }

    fn fetch() -> WebFetchRequest {
        WebFetchRequest {
            url: "https://example.com/post".to_string(),
            provider_credentials: ProviderCredentials::default(),
        }
    }

    fn user_key() -> ProviderCredentials {
        ProviderCredentials {
            tavily_api_key: Some("user_tavily_key".to_string()),
            ..ProviderCredentials::default()
        }
    }

    fn invalid(reason: &str) -> DomainError {
        DomainError::InvalidInput {
            reason: reason.to_string(),
        }
    }

    #[tokio::test]
    async fn search_returns_normalized_tavily_results() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/search"))
            .and(header("authorization", "Bearer service_key"))
            .and(body_partial_json(json!({
                "query": "rust async",
                "max_results": 5,
                "search_depth": "basic",
                "include_published_date": true,
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "query": "rust async",
                "answer": null,
                "images": [],
                "response_time": 0.8,
                "results": [
                    {
                        "title": "Async Rust",
                        "url": "https://example.com/async",
                        "content": "A guide to async.",
                        "score": 0.91,
                        "published_date": "Tue, 11 Mar 2025 17:00:00 GMT"
                    },
                    { "title": "", "url": "https://example.com/no-title", "content": null },
                    { "title": "Dropped", "url": "   ", "content": "no link" }
                ]
            })))
            .mount(&server)
            .await;

        let results = augment(&server)
            .search(search(Some(5)))
            .await
            .expect("search succeeds");

        assert_eq!(results.provider, "tavily");
        assert_eq!(results.query, "rust async");
        assert_eq!(results.results.len(), 2);
        assert_eq!(results.results[0].title, "Async Rust");
        assert_eq!(
            results.results[0].snippet.as_deref(),
            Some("A guide to async.")
        );
        assert_eq!(
            results.results[0].published_at.as_deref(),
            Some("Tue, 11 Mar 2025 17:00:00 GMT")
        );
        assert_eq!(results.results[1].title, "https://example.com/no-title");
        assert_eq!(results.results[1].snippet, None);
    }

    #[tokio::test]
    async fn search_runs_on_the_users_own_key() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/search"))
            .and(header("authorization", "Bearer user_tavily_key"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "results": [] })))
            .mount(&server)
            .await;

        let searched = augment(&server)
            .search(WebSearchRequest {
                provider_credentials: user_key(),
                ..search(None)
            })
            .await
            .expect("search runs on the user's key");

        assert!(searched.results.is_empty());
    }

    #[tokio::test]
    async fn a_used_up_key_is_a_limit_not_a_rejected_key() {
        let cases = [
            (
                401,
                "Unauthorized: missing or invalid API key.",
                "tavily_api_key_rejected",
            ),
            (
                432,
                "This request exceeds your plan's set usage limit.",
                "tavily_usage_limit_exceeded",
            ),
            (
                433,
                "This request exceeds the pay-as-you-go limit.",
                "tavily_usage_limit_exceeded",
            ),
        ];
        for (status, message, reason) in cases {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .and(path("/extract"))
                .respond_with(
                    ResponseTemplate::new(status)
                        .set_body_json(json!({ "detail": { "error": message } })),
                )
                .expect(2)
                .mount(&server)
                .await;
            let augment = augment(&server);

            let user_fetch = augment
                .fetch(WebFetchRequest {
                    provider_credentials: user_key(),
                    ..fetch()
                })
                .await;
            let service_fetch = augment.fetch(fetch()).await;

            assert_eq!(user_fetch, Err(invalid(reason)), "{status}");
            assert_eq!(
                service_fetch,
                Err(DomainError::UpstreamProvider),
                "{status}"
            );
        }
    }

    #[tokio::test]
    async fn fetch_returns_the_extracted_markdown() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/extract"))
            .and(header("authorization", "Bearer service_key"))
            .and(body_partial_json(json!({
                "urls": ["https://example.com/post"],
                "extract_depth": "basic",
                "format": "markdown",
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "results": [{
                    "url": "https://example.com/post",
                    "raw_content": "# Heading\n\nBody text.",
                    "images": []
                }],
                "failed_results": [],
                "response_time": 0.4
            })))
            .mount(&server)
            .await;

        let fetched = augment(&server)
            .fetch(fetch())
            .await
            .expect("fetch succeeds");

        assert_eq!(fetched.url, "https://example.com/post");
        assert_eq!(fetched.content, "# Heading\n\nBody text.");
        assert_eq!(fetched.format, "markdown");
        assert_eq!(fetched.provider, "tavily");
    }

    #[tokio::test]
    async fn a_page_tavily_could_not_extract_is_a_usable_error_not_content() {
        let failed_with_200 = json!({
            "results": [],
            "failed_results": [{ "url": "https://example.com/post", "error": "Failed to retrieve content" }]
        });
        let blank_content = json!({
            "results": [{ "url": "https://example.com/post", "raw_content": "  \n" }],
            "failed_results": []
        });
        for (status, body) in [
            (200, failed_with_200),
            (200, blank_content),
            (
                400,
                json!({ "detail": { "error": "All URLs failed validation." } }),
            ),
        ] {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .and(path("/extract"))
                .respond_with(ResponseTemplate::new(status).set_body_json(body))
                .mount(&server)
                .await;

            let error = augment(&server).fetch(fetch()).await;

            assert_eq!(error, Err(invalid("web_fetch_unsupported_url")));
        }
    }

    #[tokio::test]
    async fn search_failures_are_classified() {
        let cases = [
            (
                400,
                json!({ "detail": { "error": "Invalid topic." } }),
                invalid("web_search_rejected"),
            ),
            (
                401,
                json!({ "detail": { "error": "Unauthorized" } }),
                DomainError::UpstreamProvider,
            ),
            (
                200,
                json!({ "results": "nope" }),
                DomainError::UpstreamProvider,
            ),
            (
                200,
                json!({ "detail": { "error": "Internal error" } }),
                DomainError::UpstreamProvider,
            ),
        ];
        for (status, body, expected) in cases {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .and(path("/search"))
                .respond_with(ResponseTemplate::new(status).set_body_json(body))
                .expect(1)
                .mount(&server)
                .await;

            let error = augment(&server).search(search(None)).await;

            assert_eq!(error, Err(expected));
        }
    }

    #[tokio::test]
    async fn a_failed_call_is_sent_once() {
        for status in [429, 503] {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .and(path("/search"))
                .respond_with(ResponseTemplate::new(status))
                .expect(1)
                .mount(&server)
                .await;

            let error = augment(&server).search(search(None)).await;

            assert_eq!(error, Err(DomainError::UpstreamProvider), "{status}");
        }
    }

    /// Searches through the client the app builds for its upstream calls
    /// (`upstream_client`), against a search that answers `status` with
    /// `location`.
    async fn redirected_search(server: &MockServer, status: u16, location: &str) -> DomainError {
        Mock::given(method("POST"))
            .and(path("/search"))
            .respond_with(ResponseTemplate::new(status).insert_header("location", location))
            .mount(server)
            .await;
        let config = UpstreamConfig {
            api_key: "service_key".to_string(),
            base_url: server.uri(),
        };
        let client =
            http::upstream_client(Duration::from_secs(5)).expect("the upstream client builds");
        TavilyAugment::from_config(client, &config)
            .search(search(None))
            .await
            .expect_err("a redirect is not Tavily's answer")
    }

    async fn calls(server: &MockServer) -> Vec<String> {
        server
            .received_requests()
            .await
            .expect("recording is on")
            .iter()
            .map(|request| format!("{} {}", request.method, request.url.path()))
            .collect()
    }

    #[tokio::test]
    async fn a_redirect_to_another_host_is_never_followed() {
        for status in [307, 308] {
            let elsewhere = MockServer::start().await;
            let server = MockServer::start().await;
            let location = format!("{}/stolen", elsewhere.uri());

            let error = redirected_search(&server, status, &location).await;

            assert_eq!(error, DomainError::UpstreamProvider, "{status}");
            assert_eq!(calls(&server).await, ["POST /search"], "{status}");
            assert_eq!(calls(&elsewhere).await, Vec::<String>::new(), "{status}");
        }
    }

    #[tokio::test]
    async fn a_redirect_on_the_same_host_never_resends_the_search() {
        for status in [307, 308] {
            let server = MockServer::start().await;

            let error = redirected_search(&server, status, "/stolen").await;

            assert_eq!(error, DomainError::UpstreamProvider, "{status}");
            assert_eq!(calls(&server).await, ["POST /search"], "{status}");
        }
    }
}
