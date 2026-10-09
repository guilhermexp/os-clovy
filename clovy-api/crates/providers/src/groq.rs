use crate::retry::{self, UpstreamAttemptError};
use crate::transcription::TranscriptionWireResponse;
use async_trait::async_trait;
use clovy_config::UpstreamConfig;
use clovy_domain::{DomainError, Transcriber, Transcript, TranscriptionRequest, UpstreamProvider};
use reqwest::{
    StatusCode,
    multipart::{Form, Part},
};

pub const PROVIDER_NAME: &str = "groq";

pub struct GroqTranscriber {
    http: reqwest::Client,
    api_key: String,
    base_url: String,
}

impl GroqTranscriber {
    #[must_use]
    pub fn from_config(http: reqwest::Client, config: &UpstreamConfig) -> Self {
        Self {
            http,
            api_key: config.api_key.clone(),
            base_url: config.base_url.trim_end_matches('/').to_string(),
        }
    }
}

#[async_trait]
impl Transcriber for GroqTranscriber {
    async fn transcribe(&self, request: TranscriptionRequest) -> Result<Transcript, DomainError> {
        let url = format!("{}/audio/transcriptions", self.base_url);
        for attempt in 0..retry::UPSTREAM_ATTEMPTS {
            let error = match self.transcribe_once(&url, &request).await {
                Ok(transcript) => return Ok(transcript),
                Err(error) => error,
            };
            if error.retryable && attempt + 1 < retry::UPSTREAM_ATTEMPTS {
                tracing::warn!(
                    %url,
                    model = %request.model.0,
                    attempt,
                    "groq: transient upstream failure, retrying"
                );
                tokio::time::sleep(retry::UPSTREAM_RETRY_BACKOFF).await;
                continue;
            }
            return Err(error.error);
        }
        Err(DomainError::UpstreamProvider)
    }
}

impl GroqTranscriber {
    async fn transcribe_once(
        &self,
        url: &str,
        request: &TranscriptionRequest,
    ) -> Result<Transcript, UpstreamAttemptError> {
        let model_id = &request.model.0;
        let form = transcription_form(url, request)?;

        let api_key = request
            .provider_credentials
            .user_key(UpstreamProvider::Groq)
            .unwrap_or(&self.api_key);

        let response = self
            .http
            .post(url)
            .bearer_auth(api_key)
            .multipart(form)
            .send()
            .await
            .map_err(|error| {
                let retryable = retry::is_retryable_transport_error(&error);
                tracing::error!(
                    %error,
                    %url,
                    model = %model_id,
                    retryable,
                    "groq: transport error"
                );
                UpstreamAttemptError {
                    error: DomainError::UpstreamProvider,
                    retryable,
                }
            })?;

        let status = response.status();
        if !status.is_success() {
            let retryable = retry::is_retryable_status(status);
            let body_bytes = response.bytes().await.unwrap_or_default();
            tracing::error!(
                %status,
                %url,
                model = %model_id,
                body_bytes = body_bytes.len(),
                retryable,
                "groq: non-success response"
            );
            let has_user_key = request
                .provider_credentials
                .has_user_key(UpstreamProvider::Groq);
            return Err(failure_for_status(status, has_user_key, model_id));
        }

        let parsed = response
            .json::<TranscriptionWireResponse>()
            .await
            .map_err(|error| {
                tracing::error!(
                    %error,
                    %url,
                    model = %model_id,
                    "groq: response JSON parse failed"
                );
                UpstreamAttemptError::fatal(DomainError::UpstreamProvider)
            })?;
        let text = parsed.text.trim().to_string();
        if text.is_empty() {
            tracing::info!(%url, model = %model_id, "groq: no speech in audio");
            return Err(UpstreamAttemptError::fatal(DomainError::InvalidInput {
                reason: "no_speech".to_string(),
            }));
        }
        Ok(Transcript {
            text,
            language: parsed.language,
            provider: PROVIDER_NAME.to_string(),
        })
    }
}

/// The multipart body of one transcription: the model, a JSON response, the
/// audio, and the context prompt and language when they are not blank.
fn transcription_form(
    url: &str,
    request: &TranscriptionRequest,
) -> Result<Form, UpstreamAttemptError> {
    let model_id = &request.model.0;
    let audio_part = Part::bytes(request.audio.clone())
        .file_name(request.format.upstream_filename())
        .mime_str(request.format.mime())
        .map_err(|error| {
            tracing::error!(%error, %url, model = %model_id, "groq: audio mime build failed");
            UpstreamAttemptError::fatal(DomainError::UpstreamProvider)
        })?;
    let mut form = Form::new()
        .text("model", model_id.clone())
        .text("response_format", "json")
        .part("file", audio_part);
    if let Some(prompt) = request
        .context
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        form = form.text("prompt", prompt.to_string());
    }
    if let Some(language) = request
        .language
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        form = form.text("language", language.to_string());
    }
    Ok(form)
}

/// The error a non-success Groq status becomes. Only an unclassified status
/// keeps its transient retry.
fn failure_for_status(
    status: StatusCode,
    has_user_key: bool,
    model_id: &str,
) -> UpstreamAttemptError {
    if has_user_key && matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) {
        return UpstreamAttemptError::fatal(DomainError::InvalidInput {
            reason: "groq_api_key_rejected".to_string(),
        });
    }
    if status == StatusCode::NOT_FOUND {
        return UpstreamAttemptError::fatal(DomainError::InvalidInput {
            reason: "groq_model_unavailable".to_string(),
        });
    }
    if status == StatusCode::PAYLOAD_TOO_LARGE {
        return UpstreamAttemptError::fatal(DomainError::InvalidInput {
            reason: "audio_too_large".to_string(),
        });
    }
    if matches!(
        status,
        StatusCode::BAD_REQUEST | StatusCode::UNPROCESSABLE_ENTITY
    ) {
        // Never log the upstream message: a rejected prompt or audio can be
        // echoed back in it. Status and the typed reason identify the case.
        tracing::warn!(
            provider = PROVIDER_NAME,
            %status,
            model = %model_id,
            reason = "transcription_rejected",
            "groq: transcription rejected"
        );
        return UpstreamAttemptError::fatal(DomainError::InvalidInput {
            reason: "transcription_rejected".to_string(),
        });
    }
    UpstreamAttemptError {
        error: DomainError::UpstreamProvider,
        retryable: retry::is_retryable_status(status),
    }
}

#[cfg(test)]
mod tests {
    use super::{GroqTranscriber, PROVIDER_NAME};
    use crate::http;
    use clovy_config::UpstreamConfig;
    use clovy_domain::{
        AudioFormat, DomainError, ModelId, ProviderCredentials, Transcriber, TranscriptionRequest,
        UpstreamProvider,
    };
    use pretty_assertions::assert_eq;
    use serde_json::json;
    use std::error::Error;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{header, method, path},
    };

    fn transcriber(server: &MockServer) -> GroqTranscriber {
        GroqTranscriber::from_config(
            http::default_client(),
            &UpstreamConfig {
                api_key: "groq_service_key".to_string(),
                base_url: server.uri(),
            },
        )
    }

    fn sample_request() -> TranscriptionRequest {
        TranscriptionRequest {
            audio: b"fake wav bytes".to_vec(),
            format: AudioFormat::Wav,
            context: None,
            language: None,
            model: ModelId("whisper-large-v3-turbo".to_string()),
            provider_credentials: ProviderCredentials::default(),
        }
    }

    #[tokio::test]
    async fn multipart_carries_model_language_prompt_and_canonical_filename()
    -> Result<(), Box<dyn Error>> {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/audio/transcriptions"))
            .and(header("authorization", "Bearer groq_service_key"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({ "text": "Hello world" })),
            )
            .expect(1)
            .mount(&server)
            .await;

        let mut req = sample_request();
        req.format = AudioFormat::Mp4;
        req.context = Some("custom context prompt".to_string());
        req.language = Some("pt".to_string());

        let result = transcriber(&server).transcribe(req).await?;
        assert_eq!(result.text, "Hello world");
        assert_eq!(result.provider, PROVIDER_NAME);
        assert_eq!(result.language, None);

        let received = server.received_requests().await.unwrap_or_default();
        assert_eq!(received.len(), 1);
        let body = String::from_utf8_lossy(&received[0].body);
        assert!(body.contains("filename=\"audio.m4a\""), "body: {body}");
        assert!(body.contains("Content-Type: audio/mp4"), "body: {body}");
        assert!(body.contains("name=\"model\""), "body: {body}");
        assert!(body.contains("whisper-large-v3-turbo"), "body: {body}");
        assert!(body.contains("name=\"prompt\""), "body: {body}");
        assert!(body.contains("custom context prompt"), "body: {body}");
        assert!(body.contains("name=\"language\""), "body: {body}");
        assert!(body.contains("pt"), "body: {body}");
        assert!(body.contains("name=\"response_format\""), "body: {body}");
        assert!(body.contains("json"), "body: {body}");
        Ok(())
    }

    #[tokio::test]
    async fn auth_uses_user_key_when_present_and_service_key_otherwise()
    -> Result<(), Box<dyn Error>> {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/audio/transcriptions"))
            .and(header("authorization", "Bearer user_key_123"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({ "text": "User key text" })),
            )
            .expect(1)
            .mount(&server)
            .await;

        let mut req = sample_request();
        req.provider_credentials =
            ProviderCredentials::for_provider(UpstreamProvider::Groq, "user_key_123".to_string());

        let result = transcriber(&server).transcribe(req).await?;
        assert_eq!(result.text, "User key text");
        Ok(())
    }

    #[tokio::test]
    async fn user_key_401_maps_to_rejected_and_service_key_to_upstream()
    -> Result<(), Box<dyn Error>> {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/audio/transcriptions"))
            .respond_with(ResponseTemplate::new(401).set_body_json(json!({
                "error": { "message": "Invalid API Key" }
            })))
            .mount(&server)
            .await;

        let mut user_req = sample_request();
        user_req.provider_credentials =
            ProviderCredentials::for_provider(UpstreamProvider::Groq, "bad_key".to_string());

        let user_err = transcriber(&server).transcribe(user_req).await;
        assert_eq!(
            user_err,
            Err(DomainError::InvalidInput {
                reason: "groq_api_key_rejected".to_string()
            })
        );

        let service_req = sample_request();
        let service_err = transcriber(&server).transcribe(service_req).await;
        assert_eq!(service_err, Err(DomainError::UpstreamProvider));
        Ok(())
    }

    #[tokio::test]
    async fn retries_transient_503_then_succeeds_and_does_not_retry_400()
    -> Result<(), Box<dyn Error>> {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/audio/transcriptions"))
            .respond_with(ResponseTemplate::new(503))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/audio/transcriptions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "text": "Recovered" })))
            .mount(&server)
            .await;

        let result = transcriber(&server).transcribe(sample_request()).await?;
        assert_eq!(result.text, "Recovered");

        let bad_server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/audio/transcriptions"))
            .respond_with(ResponseTemplate::new(400).set_body_json(json!({
                "error": { "message": "unsupported audio format" }
            })))
            .expect(1)
            .mount(&bad_server)
            .await;

        let err = transcriber(&bad_server).transcribe(sample_request()).await;
        assert_eq!(
            err,
            Err(DomainError::InvalidInput {
                reason: "transcription_rejected".to_string()
            })
        );
        Ok(())
    }

    #[tokio::test]
    async fn blank_transcript_maps_to_no_speech() -> Result<(), Box<dyn Error>> {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/audio/transcriptions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "text": "   " })))
            .expect(1)
            .mount(&server)
            .await;

        let err = transcriber(&server).transcribe(sample_request()).await;
        assert_eq!(
            err,
            Err(DomainError::InvalidInput {
                reason: "no_speech".to_string()
            })
        );
        Ok(())
    }

    #[tokio::test]
    async fn status_404_and_413_map_to_domain_errors() -> Result<(), Box<dyn Error>> {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/audio/transcriptions"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;

        let not_found_err = transcriber(&server).transcribe(sample_request()).await;
        assert_eq!(
            not_found_err,
            Err(DomainError::InvalidInput {
                reason: "groq_model_unavailable".to_string()
            })
        );

        let large_server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/audio/transcriptions"))
            .respond_with(ResponseTemplate::new(413))
            .mount(&large_server)
            .await;

        let large_err = transcriber(&large_server)
            .transcribe(sample_request())
            .await;
        assert_eq!(
            large_err,
            Err(DomainError::InvalidInput {
                reason: "audio_too_large".to_string()
            })
        );
        Ok(())
    }
}
