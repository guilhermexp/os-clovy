#![cfg_attr(test, allow(clippy::expect_used, clippy::unwrap_used, clippy::panic))]

pub mod audio_probe;
pub mod fal_image;
pub mod fal_image_edit;
pub mod fal_video;
pub mod groq;
pub mod http;
pub mod issue_reports;
pub mod jwks;
pub mod local_dev;
pub mod m4a_probe;
pub mod openrouter;
pub mod os_accounts;
pub mod p3a;
mod retry;
pub mod tavily;
mod transcription;
pub mod viewer_identity;
pub mod wav_probe;

pub use audio_probe::MultiFormatDurationProbe;
pub use fal_image::FalImageGenerator;
pub use fal_image_edit::FalImageEditor;
pub use fal_video::FalVideoProvider;
pub use groq::GroqTranscriber;
pub use http::{default_client, issue_report_client, jwks_client, upstream_client};
pub use issue_reports::{LogIssueReportSink, OsPlatformIssueReportSink};
pub use jwks::{JwksTokenVerifier, JwksTokenVerifierParams};
pub use local_dev::{LocalDevOsAccountsClient, LocalDevTokenVerifier};
pub use m4a_probe::{M4aDurationProbe, M4aProbeError};
pub use openrouter::{
    OpenRouterAgentChat, OpenRouterCleaner, OpenRouterGenerator, OpenRouterModelCatalog,
};
pub use os_accounts::{OsAccountsHttpClient, OsAccountsP3aSink};
pub use p3a::LogP3aSink;
pub use tavily::TavilyAugment;
pub use wav_probe::{ProbeError, WavDurationProbe};
