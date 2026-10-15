//! Shared construction for Clovy's long-lived outbound HTTP clients.

use std::time::Duration;

pub const USER_AGENT: &str = concat!("clovy/", env!("CARGO_PKG_VERSION"));

/// Direct connections only (ambient proxy variables could redirect
/// credentials), pooled keep-alive, and Clovy's user agent. Callers add their
/// own timeouts, headers, and redirect policy.
pub fn base_client_builder() -> reqwest::ClientBuilder {
    reqwest::Client::builder()
        .no_proxy()
        .pool_idle_timeout(Duration::from_secs(90))
        .tcp_keepalive(Some(Duration::from_secs(30)))
        .user_agent(USER_AGENT)
}

/// Builds `builder`. A failure is logged and falls back to a plain client
/// that still ignores proxies, never to one that would honor them.
pub fn build_client(builder: reqwest::ClientBuilder, purpose: &str) -> reqwest::Client {
    builder.build().unwrap_or_else(|error| {
        tracing::error!(%error, purpose, "HTTP client configuration failed; using defaults");
        reqwest::Client::builder()
            .no_proxy()
            .user_agent(USER_AGENT)
            .build()
            .expect("the HTTP client backend could not be initialized")
    })
}
