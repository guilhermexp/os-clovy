//! Read-only browser viewer for private shares (JUN-308).
//!
//! The shell is static: the same bytes are served for every well-formed
//! share id (existence is never revealed by this route), all decryption
//! happens in the page via `WebCrypto` with key material from the URL
//! fragment, and the CSP pins the exact inline script/style hashes with
//! `connect-src 'self'` — the page cannot talk to anything but clovy-api.
//! Only passcode link shares open in the browser: invite shares need a signed-in
//! recipient, which the local-only server does not offer.

use crate::{error::ApiError, state::ApiState};
use axum::{
    extract::State,
    http::{HeaderValue, header},
    response::{Html, IntoResponse, Response},
};
use base64::Engine as _;
use sha2::{Digest, Sha256};
use std::sync::OnceLock;

const PAGE_TEMPLATE: &str = include_str!("share_viewer_page.html");
const PAGE_SCRIPT: &str = include_str!("share_viewer_page.js");
const PAGE_STYLE: &str = include_str!("share_viewer_page.css");

fn csp_hash(source: &str) -> String {
    let digest = Sha256::digest(source.as_bytes());
    format!(
        "'sha256-{}'",
        base64::engine::general_purpose::STANDARD.encode(digest)
    )
}

fn csp_header() -> &'static str {
    static CSP: OnceLock<String> = OnceLock::new();
    CSP.get_or_init(|| {
        format!(
            "default-src 'none'; script-src {script}; style-src {style}; \
             connect-src 'self'; base-uri 'none'; form-action 'none'; \
             frame-ancestors 'none'",
            script = csp_hash(PAGE_SCRIPT),
            style = csp_hash(PAGE_STYLE),
        )
    })
}

fn render_page() -> String {
    PAGE_TEMPLATE
        .replace("/*STYLE*/", PAGE_STYLE)
        .replace("/*SCRIPT*/", PAGE_SCRIPT)
}

fn shell_response() -> Response {
    let mut response = Html(render_page()).into_response();
    let headers = response.headers_mut();
    headers.insert(
        "x-robots-tag",
        HeaderValue::from_static("noindex, nofollow, noarchive"),
    );
    headers.insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    if let Ok(value) = HeaderValue::from_str(csp_header()) {
        headers.insert(header::CONTENT_SECURITY_POLICY, value);
    }
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

/// `GET /s/{share_id}` — one static shell for every share id.
pub(crate) async fn shell(State(state): State<ApiState>) -> Response {
    // Fail closed when sharing is disabled. This stays non-enumerating: the
    // response is byte-identical for every share id in this state.
    if state.share().is_none() {
        return ApiError::SharingUnavailable.into_response();
    }
    shell_response()
}

/// `GET /robots.txt` — private share pages must never be indexed.
pub(crate) async fn robots() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        "User-agent: *\nDisallow: /s/\n",
    )
}
