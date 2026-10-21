use reqwest::{Client, ClientBuilder};
use std::time::Duration;

pub(crate) const DEFAULT_TIMEOUT_SECS: u64 = 60;
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(DEFAULT_TIMEOUT_SECS);
const DEFAULT_IDLE_TIMEOUT: Duration = Duration::from_secs(90);
/// Most bytes read from an upstream error body that is only logged by size.
pub(crate) const LOGGED_ERROR_BODY_LIMIT: usize = 64 * 1024;

pub fn default_client() -> Client {
    build_client(DEFAULT_TIMEOUT)
}

/// Client for Clovy API's own upstream calls: Groq, `OpenRouter`, fal and Tavily
/// (the app's `upstream_http` and `metered_inference_http`). It keeps the
/// shared timeout, pooling and user agent. It never follows a redirect and
/// never replays a request: a 307/308 would resend the paid POST body to the
/// `Location` (with the provider credential when it stays on the same host),
/// and a replayed submit can bill twice.
/// A build failure is an error, never a silent fallback to a client without
/// this configuration.
pub fn upstream_client(timeout: Duration) -> Result<Client, reqwest::Error> {
    client_builder(timeout)
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .build()
}

/// Reads an error body only to log its size, stopping once `limit` bytes have
/// arrived so a large or endless upstream error body is never buffered.
/// Returns the bytes read, capped at `limit`.
pub(crate) async fn read_body_len_capped(
    response: &mut reqwest::Response,
    limit: usize,
) -> Result<usize, reqwest::Error> {
    let mut read = 0;
    while read < limit {
        match response.chunk().await? {
            Some(chunk) => read += chunk.len(),
            None => break,
        }
    }
    Ok(read.min(limit))
}

fn build_client(timeout: Duration) -> Client {
    client_builder(timeout)
        .build()
        .unwrap_or_else(|_| Client::new())
}

fn client_builder(timeout: Duration) -> ClientBuilder {
    Client::builder()
        .timeout(timeout)
        .pool_idle_timeout(DEFAULT_IDLE_TIMEOUT)
        .tcp_keepalive(Some(Duration::from_secs(30)))
        .user_agent("clovy-api/0.1")
}

#[cfg(test)]
mod tests {
    use super::{default_client, read_body_len_capped};
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path},
    };

    #[tokio::test]
    async fn error_body_length_read_stops_at_the_limit() -> Result<(), reqwest::Error> {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/error"))
            .respond_with(ResponseTemplate::new(502).set_body_bytes(vec![b'x'; 256 * 1024]))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/short"))
            .respond_with(ResponseTemplate::new(502).set_body_bytes(vec![b'x'; 10]))
            .mount(&server)
            .await;
        let client = default_client();

        let mut long = client.get(format!("{}/error", server.uri())).send().await?;
        let mut short = client.get(format!("{}/short", server.uri())).send().await?;

        assert_eq!(read_body_len_capped(&mut long, 1024).await?, 1024);
        assert_eq!(read_body_len_capped(&mut short, 1024).await?, 10);
        Ok(())
    }
}
