use figment::{
    Figment, Provider,
    providers::{Env, Format, Serialized, Toml},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fmt::{self, Debug},
    net::IpAddr,
};
use thiserror::Error;
use url::Url;

const REDACTED: &str = "<redacted>";
pub const LOCAL_DEV_BEARER_TOKEN_PLACEHOLDER: &str = "local-dev-token";
pub const GROQ_API_KEY_PLACEHOLDER: &str = "GROQ_API_KEY_REPLACE_ME";
pub const OPENROUTER_API_KEY_PLACEHOLDER: &str = "OPENROUTER_API_KEY_REPLACE_ME";
pub const FAL_API_KEY_PLACEHOLDER: &str = "FAL_API_KEY_REPLACE_ME";
pub const TAVILY_API_KEY_PLACEHOLDER: &str = "TAVILY_API_KEY_REPLACE_ME";
/// Every published service-key placeholder. A configured key equal to any of
/// them is not a usable key.
const UPSTREAM_API_KEY_PLACEHOLDERS: &[&str] = &[
    GROQ_API_KEY_PLACEHOLDER,
    OPENROUTER_API_KEY_PLACEHOLDER,
    FAL_API_KEY_PLACEHOLDER,
    TAVILY_API_KEY_PLACEHOLDER,
];
pub const IMAGE_EDIT_SOURCE_MAX_BYTES: usize = 50 * 1024 * 1024;
pub const DEFAULT_REQUEST_TIMEOUT_SECS: u64 = 600;
/// Matches os-platform's general attachment cap. Clovy API is the authenticated
/// report boundary, so it must accept the same video size the downstream file
/// API accepts instead of silently imposing the platform's former 10 MiB cap.
pub const ISSUE_REPORT_ATTACHMENT_MAX_BYTES: usize = 300 * 1024 * 1024;
/// Multipart framing and the report's text fields need a small amount of room
/// beyond one maximum-sized attachment. This remains a total request cap, so
/// many attachments cannot multiply the per-file allowance into unbounded RAM.
pub const DEFAULT_MAX_ISSUE_REPORT_BYTES: usize = ISSUE_REPORT_ATTACHMENT_MAX_BYTES + (1024 * 1024);
/// OS Accounts rejects `/authorize` holds outside its deployed cap as
/// `invalid_ttl` (envelope code 4201). This mirrors `MAX_HOLD_TTL_SECONDS`
/// in the os-accounts repo (`api/crates/services/src/grants.rs`), which is
/// being raised to 900 in a coordinated deploy alongside Clovy API. Keep the
/// two values in lockstep: a value above the deployed os-accounts cap is
/// rejected at runtime as `invalid_ttl` (4201).
pub const OS_ACCOUNTS_MAX_HOLD_TTL_SECS: u64 = 900;
/// Budget reserved between the image client call ending and the hold
/// expiring, sized to the WORST-CASE charge path, not a nominal grace:
/// settling can spend `CHARGE_RETRY_ATTEMPTS` HTTP calls at the 60-second
/// client timeout plus backoff (~121s) before giving up. Keep in sync with
/// `CHARGE_RETRY_ATTEMPTS` / `CHARGE_RETRY_BACKOFF` (`os_accounts.rs`) and the
/// default HTTP client timeout (`http.rs`) in clovy-providers — a contract test
/// there pins this margin above that budget.
pub const IMAGE_SETTLEMENT_TIMEOUT_MARGIN_SECS: u64 = 150;
/// Budget reserved before generation for the OS Accounts authorize call —
/// it runs on the same 60-second default HTTP client (pinned by the same
/// clovy-providers contract test as the settlement budget). The route's
/// `TimeoutLayer` bounds authorize + generate + settle together, so the
/// image client window must leave room for both metering legs.
pub const OS_ACCOUNTS_AUTHORIZE_TIMEOUT_BUDGET_SECS: u64 = 60;
pub const DEFAULT_IMAGE_CLIENT_TIMEOUT_SECS: u64 = DEFAULT_REQUEST_TIMEOUT_SECS
    - OS_ACCOUNTS_AUTHORIZE_TIMEOUT_BUDGET_SECS
    - IMAGE_SETTLEMENT_TIMEOUT_MARGIN_SECS;
// 10s, not 30: the diagnosis runs inline before delivery, so its timeout is
// user-facing "Sending" time. Typical completions land in 2-6s; anything
// slower delivers undiagnosed rather than holding the dialog hostage.
pub const DEFAULT_ISSUE_REPORT_DIAGNOSIS_TIMEOUT_SECS: u64 = 10;
/// Nobody files more than a handful of legitimate reports an hour; the cap
/// only bounds Clovy-funded diagnosis calls, never report delivery.
pub const DEFAULT_ISSUE_REPORT_DIAGNOSIS_MAX_PER_USER_PER_HOUR: u64 = 6;
/// The hold is minted after authorize returns and must cover generation
/// plus settlement: 390 + 150 = 540, inside the platform cap. Anchoring on the
/// route timeout instead produced 630, past the former 600-second cap, and every
/// image authorize was rejected `invalid_ttl` (4201) in production.
pub const DEFAULT_IMAGE_HOLD_TTL_SECS: u64 =
    DEFAULT_IMAGE_CLIENT_TIMEOUT_SECS + IMAGE_SETTLEMENT_TIMEOUT_MARGIN_SECS;
// The full route budget must fit its three legs.
const _: () = assert!(
    OS_ACCOUNTS_AUTHORIZE_TIMEOUT_BUDGET_SECS
        + DEFAULT_IMAGE_CLIENT_TIMEOUT_SECS
        + IMAGE_SETTLEMENT_TIMEOUT_MARGIN_SECS
        <= DEFAULT_REQUEST_TIMEOUT_SECS
);
// Compile-time regression pin: a default past the cap fails every image
// authorize as invalid_ttl (4201) in production.
const _: () = assert!(DEFAULT_IMAGE_HOLD_TTL_SECS <= OS_ACCOUNTS_MAX_HOLD_TTL_SECS);
const IMAGE_EDIT_JSON_OVERHEAD_BYTES: usize = 16 * 1024;
pub const DEFAULT_MAX_IMAGE_EDIT_BYTES: usize =
    base64_encoded_len(IMAGE_EDIT_SOURCE_MAX_BYTES) + IMAGE_EDIT_JSON_OVERHEAD_BYTES;
/// Dedicated request-body cap for `/v1/chat/completions`. Sized to the
/// desktop provider proxy's 12 MiB chat body cap so an in-window agent chat
/// request the proxy forwards is never rejected here by a stricter outer gate before
/// `validate_agent_chat_body` can size-check it (JUN-336). 12 MiB is the
/// byte-image of the 6M-char semantic cap (`MAX_AGENT_TOTAL_STRING_CHARS`) at
/// ~2 bytes/char, sized for a 1M-token context window. This is only an abuse
/// ceiling above every valid agent chat request; semantic size rejection stays
/// in `validate_agent_chat_body`. Keep this in sync with the desktop proxy
/// across the src-tauri / clovy-api workspace boundary.
pub const DEFAULT_MAX_AGENT_CHAT_BYTES: usize = 12 * 1024 * 1024;
/// Global cap on the total in-flight request-body bytes buffered across the
/// large-body agent routes, so concurrent authenticated requests cannot exhaust
/// the shared TEE (JUN-336). Conservative default; tune against real traffic.
pub const DEFAULT_MAX_AGENT_INFLIGHT_BODY_BYTES: usize = 1024 * 1024 * 1024;
/// Max concurrent large-body agent requests a single user may have in flight
/// before Clovy API load-sheds with 503 (JUN-336).
pub const DEFAULT_MAX_AGENT_CONCURRENT_REQUESTS_PER_USER: usize = 8;

// --- Video generation (ADR 0015) ---------------------------------------------
//
// Video is an async job billed at completion: the hold is minted at
// `/v1/video/generate` and must stay valid until the completing status poll
// charges. Unlike image (a single synchronous request), the hold has to cover
// the whole job lifetime — queue, the provider run, then the settlement charge.
// The desktop polls for 900s, so video reserves 750s for the job and 150s for
// settlement, matching that full client poll window (ADR 0015 Decision 2).
/// Worst-case seconds a supported video job may take from queue to a completing
/// poll. The first-cut allowlist/durations are picked to finish within this.
pub const DEFAULT_VIDEO_JOB_MAX_SECS: u64 = 750;
/// Budget reserved after the job completes for the settlement charge path,
/// sized like the image settlement margin (worst-case OS Accounts charge
/// retries at the default HTTP timeout plus backoff).
pub const VIDEO_SETTLEMENT_TIMEOUT_MARGIN_SECS: u64 = IMAGE_SETTLEMENT_TIMEOUT_MARGIN_SECS;
/// Video hold TTL: the job budget plus the settlement margin. Pinned at the OS
/// Accounts platform cap; the job budget is what gets capped to keep it there.
pub const DEFAULT_VIDEO_HOLD_TTL_SECS: u64 =
    DEFAULT_VIDEO_JOB_MAX_SECS + VIDEO_SETTLEMENT_TIMEOUT_MARGIN_SECS;
/// Desktop `/video` polls for 360 attempts at 2.5s each.
const VIDEO_CLIENT_POLL_WINDOW_SECS: u64 = 900;
/// Defensive per-request credit ceiling. The longest, highest-resolution clip
/// the curated fal models accept (30s at 1080p, $6.00) costs 12000 credits at
/// the 2.0x default markup (`$1 = 1000 credits`). A quote above this is
/// rejected before authorize so a price change cannot authorize an unbounded
/// hold (ADR 0015 Decision 1).
pub const DEFAULT_VIDEO_MAX_CREDITS_PER_REQUEST: u64 = 20_000;
/// Maximum raw video body Clovy API will download from the video provider.
pub const DEFAULT_VIDEO_MAX_RESPONSE_BYTES: u64 = 100 * 1024 * 1024;
// The hold must fit the platform cap; the job must fit inside the hold.
const _: () = assert!(DEFAULT_VIDEO_HOLD_TTL_SECS <= OS_ACCOUNTS_MAX_HOLD_TTL_SECS);
const _: () = assert!(DEFAULT_VIDEO_HOLD_TTL_SECS >= VIDEO_CLIENT_POLL_WINDOW_SECS);
const _: () = assert!(
    DEFAULT_VIDEO_JOB_MAX_SECS + VIDEO_SETTLEMENT_TIMEOUT_MARGIN_SECS
        <= DEFAULT_VIDEO_HOLD_TTL_SECS
);

const fn base64_encoded_len(byte_count: usize) -> usize {
    byte_count.div_ceil(3) * 4
}

pub const fn image_client_timeout_secs(route_timeout_secs: u64) -> u64 {
    // Saturating with a 1s floor: validated configs can never get here with a
    // route timeout at or below the budgets (validate_image_timeout_margin
    // rejects them), but directly-built states (tests) bypass validation and
    // a clamped window beats an underflow panic.
    let window = route_timeout_secs
        .saturating_sub(OS_ACCOUNTS_AUTHORIZE_TIMEOUT_BUDGET_SECS)
        .saturating_sub(IMAGE_SETTLEMENT_TIMEOUT_MARGIN_SECS);
    if window == 0 { 1 } else { window }
}

#[derive(Clone, Deserialize, Serialize)]
pub struct AppConfig {
    pub server: ServerConfig,
    #[serde(default)]
    pub local_dev: LocalDevConfig,
    #[serde(default)]
    pub browser_transports: BrowserTransportsConfig,
    pub os_accounts: OsAccountsConfig,
    pub upstreams: UpstreamsConfig,
    pub attestation: AttestationConfig,
    #[serde(default)]
    pub issue_reports: IssueReportsConfig,
    /// Emergency rollout control for the local Computer use driver. The
    /// desktop fetches this before exposing the capability, allowing an OS or
    /// app-version regression to be disabled without a desktop update.
    #[serde(default)]
    pub computer_use: ComputerUseConfig,
    /// Private sharing (JUN-308). Sharing endpoints return 501
    /// `sharing_unavailable` until `database_url` is configured, so the
    /// feature cannot regress deployments that predate it.
    #[serde(default)]
    pub share: ShareConfig,
    /// Clovy Companion relay persistence and optional opaque APNs wake hints.
    #[serde(default)]
    pub companion: CompanionConfig,
    pub pricing: BTreeMap<String, ModelPriceConfig>,
    /// Flat credits charged per generated image, keyed by image model id. Kept
    /// separate from `pricing` (the text/ASR catalog) so image models never leak
    /// into the served model pickers. A model absent here is rejected at the
    /// `/image/generate` boundary (`model_not_priced`), so the settings picker
    /// must only offer models listed here. `$1 = 1000 credits`; values are the
    /// fal worst-case per-image cost at Clovy's largest size with a ~2x margin
    /// (FLUX.1 [dev] costs at most $0.05, charged 100).
    #[serde(default = "default_image_pricing")]
    pub image_pricing: BTreeMap<String, u64>,
    /// Flat credits charged per EDITED image, keyed by edit model id. Edit
    /// models are separate fal endpoints from generation (default
    /// `fal-ai/flux-2/edit`), so they have their own price map. A model absent
    /// here is a retired or unknown id and is served by `default_image_edit_model`
    /// (ADR 0021). Same
    /// units as `image_pricing`: worst-case fal cost with margin.
    #[serde(default = "default_image_edit_pricing")]
    pub image_edit_pricing: BTreeMap<String, u64>,
    /// The edit model used when a request names none or a retired one — the
    /// image MCP never sends one, so this on-server default governs every edit.
    /// Must be a key in `image_edit_pricing`.
    #[serde(default = "default_image_edit_model")]
    pub default_image_edit_model: String,
    /// The generation model that serves a request naming a retired or unknown
    /// image model id (ADR 0021: shipped clients keep sending ids that stopped
    /// being served). Must be a key in `image_pricing`.
    #[serde(default = "default_image_model")]
    pub default_image_model: String,
    /// Markup (fixed-point thousandths, `2000` = 2.0x) applied to the video
    /// provider's quote for each text-to-video model, keyed by model id. Video is
    /// quote-priced, not flat-priced: `credits = ceil(quote_usd * markup_millis)`
    /// (ADR 0015 Decision 1). A model absent here is a retired or unknown id and
    /// is served by `default_video_model` (ADR 0021), so this doubles as the
    /// allowlist. `$1 = 1000 credits`.
    #[serde(default = "default_video_pricing")]
    pub video_pricing: BTreeMap<String, u32>,
    /// The text-to-video model that serves a request naming a retired or
    /// unknown model id. Must be a key in `video_pricing`.
    #[serde(default = "default_video_model")]
    pub default_video_model: String,
    /// Markup per IMAGE-to-video (animate) model, keyed by model id.
    /// Image-to-video models are separate from text-to-video models (mirroring
    /// `image_edit_pricing`), so they have their own allowlist. Same units as
    /// `video_pricing`.
    #[serde(default = "default_video_animate_pricing")]
    pub video_animate_pricing: BTreeMap<String, u32>,
    /// The image-to-video model used when an animate request names none or a
    /// retired one. Must be a key in `video_animate_pricing`.
    #[serde(default = "default_video_animate_model")]
    pub default_video_animate_model: String,
    /// Defensive per-request credit ceiling for a quoted video. A computed price
    /// above this is rejected `price_overflow` before authorize, bounding a
    /// catalog change from authorizing an unbounded hold.
    #[serde(default = "default_video_max_credits_per_request")]
    pub video_max_credits_per_request: u64,
    /// Maximum raw video bytes Clovy API will download from the provider before
    /// rejecting the job. Keeps one oversized result from exhausting memory and
    /// is enforced before charge settlement.
    #[serde(default = "default_video_max_response_bytes")]
    pub video_max_response_bytes: u64,
}

impl Debug for AppConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AppConfig")
            .field("server", &self.server)
            .field("local_dev", &self.local_dev)
            .field("browser_transports", &self.browser_transports)
            .field("os_accounts", &self.os_accounts)
            .field("upstreams", &self.upstreams)
            .field("attestation", &self.attestation)
            .field("issue_reports", &self.issue_reports)
            .field("computer_use", &self.computer_use)
            .field("share", &RedactedShare(&self.share))
            .field("companion", &RedactedCompanion(&self.companion))
            .field("pricing", &self.pricing)
            .field("image_pricing", &self.image_pricing)
            .field("image_edit_pricing", &self.image_edit_pricing)
            .field("default_image_edit_model", &self.default_image_edit_model)
            .field("default_image_model", &self.default_image_model)
            .field("video_pricing", &self.video_pricing)
            .field("default_video_model", &self.default_video_model)
            .field("video_animate_pricing", &self.video_animate_pricing)
            .field(
                "default_video_animate_model",
                &self.default_video_animate_model,
            )
            .field(
                "video_max_credits_per_request",
                &self.video_max_credits_per_request,
            )
            .field("video_max_response_bytes", &self.video_max_response_bytes)
            .finish()
    }
}

/// Remote operational switches for Clovy's two Browser use transports.
/// Both fail open by default so older deployments and fresh configuration
/// preserve the shipped capability until ops explicitly disables one track.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct BrowserTransportsConfig {
    #[serde(default = "default_true")]
    pub attended_enabled: bool,
    #[serde(default = "default_true")]
    pub managed_enabled: bool,
}

impl Default for BrowserTransportsConfig {
    fn default() -> Self {
        Self {
            attended_enabled: true,
            managed_enabled: true,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ComputerUseConfig {
    /// Global emergency switch. Defaults on so existing deployments remain
    /// backward compatible when the new config section is absent.
    #[serde(default = "default_computer_use_enabled")]
    pub enabled: bool,
    /// Exact Clovy versions or `major.minor.*` prefixes to disable.
    #[serde(default, alias = "disabled_june_versions")]
    pub disabled_clovy_versions: Vec<String>,
    /// Exact macOS versions or `major.minor.*` prefixes to disable.
    #[serde(default)]
    pub disabled_macos_versions: Vec<String>,
}

fn default_computer_use_enabled() -> bool {
    true
}

impl Default for ComputerUseConfig {
    fn default() -> Self {
        Self {
            enabled: default_computer_use_enabled(),
            disabled_clovy_versions: Vec::new(),
            disabled_macos_versions: Vec::new(),
        }
    }
}

const fn default_true() -> bool {
    true
}

/// Where user-submitted issue reports get forwarded. The destination defaults
/// to Clovy's legacy `june` product handle in OS Platform; only the bot API key is
/// environment specific. Without that key, reports land in structured logs only.
#[derive(Clone, Deserialize, Serialize)]
pub struct IssueReportsConfig {
    /// Base URL of the os-platform (fellow) API.
    #[serde(default = "default_issue_report_api_url")]
    pub os_platform_api_url: String,
    /// os-platform API key of the reporting bot user — that user
    /// must be a member of the target Org/Project. Redacted Debug.
    /// `CLOVY__ISSUE_REPORTS__OS_PLATFORM_API_KEY`.
    #[serde(default)]
    pub os_platform_api_key: String,
    /// Target Org handle (or opaque `org_…` id).
    #[serde(default = "default_issue_report_org")]
    pub os_platform_org: String,
    /// Target Project handle (or opaque `prj_…` id).
    #[serde(default = "default_issue_report_project")]
    pub os_platform_project: String,
    /// Label slug attached to every report Issue.
    #[serde(default = "default_issue_report_label")]
    pub os_platform_label: String,
    /// Reward asset symbol for the zero-reward Issue (e.g. "POINTS").
    /// Issues are bounties under the hood, and creation fails when neither
    /// the Project nor the Org has a default reward asset — naming one here
    /// sidesteps that. Empty omits the field and relies on the defaults.
    #[serde(default = "default_issue_report_reward_asset")]
    pub os_platform_reward_asset: String,
    /// Optional model id used by Clovy API to diagnose issue reports before
    /// delivery. `None` preserves direct delivery with no upstream model call.
    #[serde(default)]
    pub diagnosis_model: Option<String>,
    /// Wall-clock budget for the internal diagnosis call.
    #[serde(default = "default_issue_report_diagnosis_timeout_secs")]
    pub diagnosis_timeout_secs: u64,
    /// Per-user hourly cap on Clovy-funded diagnosis calls. The cap only skips
    /// the diagnosis; report delivery itself is never limited.
    #[serde(default = "default_issue_report_diagnosis_max_per_user_per_hour")]
    pub diagnosis_max_per_user_per_hour: u64,
}

fn default_issue_report_api_url() -> String {
    "https://app.opensoftware.co/api".to_string()
}

fn default_issue_report_org() -> String {
    "june".to_string()
}

fn default_issue_report_project() -> String {
    "bug-reports".to_string()
}

fn default_issue_report_label() -> String {
    "bug".to_string()
}

fn default_issue_report_reward_asset() -> String {
    "POINTS".to_string()
}

fn default_issue_report_diagnosis_timeout_secs() -> u64 {
    DEFAULT_ISSUE_REPORT_DIAGNOSIS_TIMEOUT_SECS
}

fn default_issue_report_diagnosis_max_per_user_per_hour() -> u64 {
    DEFAULT_ISSUE_REPORT_DIAGNOSIS_MAX_PER_USER_PER_HOUR
}

impl Default for IssueReportsConfig {
    fn default() -> Self {
        Self {
            os_platform_api_url: default_issue_report_api_url(),
            os_platform_api_key: String::new(),
            os_platform_org: default_issue_report_org(),
            os_platform_project: default_issue_report_project(),
            os_platform_label: default_issue_report_label(),
            os_platform_reward_asset: default_issue_report_reward_asset(),
            diagnosis_model: None,
            diagnosis_timeout_secs: default_issue_report_diagnosis_timeout_secs(),
            diagnosis_max_per_user_per_hour: default_issue_report_diagnosis_max_per_user_per_hour(),
        }
    }
}

impl Debug for IssueReportsConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("IssueReportsConfig")
            .field("os_platform_api_url", &self.os_platform_api_url)
            .field(
                "os_platform_api_key",
                if self.os_platform_api_key.is_empty() {
                    &"<unset>"
                } else {
                    &REDACTED
                },
            )
            .field("os_platform_org", &self.os_platform_org)
            .field("os_platform_project", &self.os_platform_project)
            .field("os_platform_label", &self.os_platform_label)
            .field("os_platform_reward_asset", &self.os_platform_reward_asset)
            .field("diagnosis_model", &self.diagnosis_model)
            .field("diagnosis_timeout_secs", &self.diagnosis_timeout_secs)
            .field(
                "diagnosis_max_per_user_per_hour",
                &self.diagnosis_max_per_user_per_hour,
            )
            .finish()
    }
}

/// Public facts the `/verify` attestation page reports. Nothing here is a
/// secret; all fields have working defaults so local builds need no setup.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AttestationConfig {
    /// Full git commit the running image was built from. Stamped into the
    /// image env by the Docker build (`ARG GIT_SHA`); empty for local builds.
    #[serde(default)]
    pub source_commit: String,
    pub source_repo_url: String,
    pub image_repo: String,
    pub trust_center_url: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub request_timeout_secs: u64,
    pub model_catalog_refresh_secs: u64,
    pub max_audio_bytes: usize,
    pub max_json_bytes: usize,
    /// Total multipart body cap for `/v1/issue-reports`.
    pub max_issue_report_bytes: usize,
    /// JSON body cap for `/v1/image/edit`. It is sized for a 50 MiB source
    /// image after base64 expansion plus fixed request overhead.
    pub max_image_edit_bytes: usize,
    /// JSON body cap for `/v1/chat/completions`, sized to the desktop proxy's
    /// 12 MiB chat body cap so an in-window agent chat request is not rejected by
    /// a stricter outer gate before semantic validation (JUN-336).
    pub max_agent_chat_bytes: usize,
    /// Global in-flight request-body budget for the large-body agent routes.
    pub max_agent_inflight_body_bytes: usize,
    /// Per-user concurrent request cap for the large-body agent routes.
    pub max_agent_concurrent_requests_per_user: usize,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct LocalDevConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_local_dev_bearer_token")]
    pub bearer_token: String,
    #[serde(default = "default_local_dev_user_id")]
    pub user_id: String,
    /// Second local identity so recipient-side share flows (JUN-308) can be
    /// exercised without real OS Accounts. Empty disables the second token.
    #[serde(default)]
    pub viewer_bearer_token: String,
    #[serde(default = "default_local_dev_viewer_user_id")]
    pub viewer_user_id: String,
    #[serde(default = "default_local_dev_viewer_email")]
    pub viewer_email: String,
}

fn default_local_dev_viewer_user_id() -> String {
    "usr_local_dev_viewer".to_string()
}

fn default_local_dev_viewer_email() -> String {
    "viewer@localdev.clovy".to_string()
}

fn default_local_dev_bearer_token() -> String {
    LOCAL_DEV_BEARER_TOKEN_PLACEHOLDER.to_string()
}

fn default_local_dev_user_id() -> String {
    "usr_local_dev".to_string()
}

impl Default for LocalDevConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            bearer_token: default_local_dev_bearer_token(),
            user_id: default_local_dev_user_id(),
            viewer_bearer_token: String::new(),
            viewer_user_id: default_local_dev_viewer_user_id(),
            viewer_email: default_local_dev_viewer_email(),
        }
    }
}

impl Debug for LocalDevConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalDevConfig")
            .field("enabled", &self.enabled)
            .field(
                "bearer_token",
                if self.bearer_token.is_empty() {
                    &"<unset>"
                } else {
                    &REDACTED
                },
            )
            .field("user_id", &self.user_id)
            .field(
                "viewer_bearer_token",
                if self.viewer_bearer_token.is_empty() {
                    &"<unset>"
                } else {
                    &REDACTED
                },
            )
            .field("viewer_user_id", &self.viewer_user_id)
            .field("viewer_email", &self.viewer_email)
            .finish()
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ShareConfig {
    /// Postgres URL for the share store. Empty disables sharing.
    #[serde(default)]
    pub database_url: String,
    /// OS Accounts site origin the viewer sends recipients to for sign-in
    /// (e.g. `https://accounts.opensoftware.co`). Empty falls back to the
    /// canonical issuer from `os_accounts.iss`.
    #[serde(default)]
    pub viewer_accounts_url: String,
    /// Public OAuth client id registered for the browser viewer.
    #[serde(default)]
    pub viewer_client_id: String,
    /// Restrict this process to the read-only browser viewer surface. This is
    /// used by the isolated june.link CVM so its ingress and certificate
    /// lifecycle cannot expose or disrupt the primary Clovy API deployment.
    #[serde(default)]
    pub viewer_only: bool,
    /// Max accepted ciphertext, in bytes.
    #[serde(default = "default_share_max_ciphertext_bytes")]
    pub max_ciphertext_bytes: usize,
}

#[derive(Clone, Deserialize, Serialize, Default)]
#[serde(deny_unknown_fields)]
pub struct CompanionConfig {
    /// Dedicated Postgres store for linked-device trust metadata. Empty
    /// disables companion endpoints outside local development.
    #[serde(default)]
    pub database_url: String,
    #[serde(default)]
    pub apns_team_id: String,
    #[serde(default)]
    pub apns_key_id: String,
    /// Apple .p8 signing key, injected as a secret environment value.
    #[serde(default)]
    pub apns_private_key_pem: String,
    #[serde(default)]
    pub apns_bundle_id: String,
    /// False targets Apple's sandbox endpoint; true targets production APNs.
    #[serde(default)]
    pub apns_production: bool,
}

struct RedactedCompanion<'a>(&'a CompanionConfig);

impl Debug for RedactedCompanion<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CompanionConfig")
            .field(
                "database_url",
                &if self.0.database_url.is_empty() {
                    "<unset>"
                } else {
                    REDACTED
                },
            )
            .field("apns_team_id", &self.0.apns_team_id)
            .field("apns_key_id", &self.0.apns_key_id)
            .field(
                "apns_private_key_pem",
                &if self.0.apns_private_key_pem.is_empty() {
                    "<unset>"
                } else {
                    REDACTED
                },
            )
            .field("apns_bundle_id", &self.0.apns_bundle_id)
            .field("apns_production", &self.0.apns_production)
            .finish()
    }
}

impl Default for ShareConfig {
    // Hand-written so the derived Default can't diverge from the serde field
    // defaults. `load()` seeds `AppConfig::default()` as the figment base, so a
    // `usize::default()` zero here would be *present* in the merged config and
    // shadow `default_share_max_ciphertext_bytes` (which only applies when the
    // field is absent), capping every share at 0 bytes on a deployment that
    // sets only the share envs.
    fn default() -> Self {
        Self {
            database_url: String::new(),
            viewer_accounts_url: String::new(),
            viewer_client_id: String::new(),
            viewer_only: false,
            max_ciphertext_bytes: default_share_max_ciphertext_bytes(),
        }
    }
}

fn default_share_max_ciphertext_bytes() -> usize {
    10 * 1024 * 1024
}

/// Debug view of `ShareConfig` that never prints the database URL (it embeds
/// credentials); everything else in the section is public.
struct RedactedShare<'a>(&'a ShareConfig);

impl Debug for RedactedShare<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ShareConfig")
            .field(
                "database_url",
                &if self.0.database_url.is_empty() {
                    "<unset>"
                } else {
                    "<redacted>"
                },
            )
            .field("viewer_accounts_url", &self.0.viewer_accounts_url)
            .field("viewer_client_id", &self.0.viewer_client_id)
            .field("viewer_only", &self.0.viewer_only)
            .field("max_ciphertext_bytes", &self.0.max_ciphertext_bytes)
            .finish()
    }
}

#[derive(Clone, Deserialize, Serialize)]
pub struct OsAccountsConfig {
    pub api_url: String,
    pub app_api_key: String,
    /// Optional service token for anonymous P3A aggregate ingestion into OS
    /// Accounts. When unset (empty), P3A ingest is disabled: reports fall back
    /// to the log-only sink while startup and all other ingest proceed
    /// normally.
    #[serde(default)]
    pub p3a_ingest_token: String,
    pub iss: String,
    pub aud: String,
    pub jwks_refresh_secs: u64,
    pub jwks_miss_min_backoff_secs: u64,
    pub authorize_hold_ttl_note_transcribe_secs: u64,
    pub authorize_hold_ttl_note_generate_secs: u64,
    pub authorize_hold_ttl_dictate_transcribe_secs: u64,
    pub authorize_hold_ttl_dictate_cleanup_secs: u64,
    /// Maximum audio duration accepted by no-charge live preview requests.
    /// Desktop chunks are shorter; this server cap prevents arbitrary-length
    /// preview transcription if a client replays the public endpoint.
    pub note_transcribe_preview_max_audio_secs: u64,
    /// Skip per-request estimation entirely and authorize this many credits
    /// for every metered action. Trades a bigger Hold (and thus a tighter
    /// max concurrency per user) for not needing to probe audio duration or
    /// pre-count tokens. Set via `CLOVY__OS_ACCOUNTS__FLAT_ESTIMATE_CREDITS`.
    pub flat_estimate_credits: u64,
    /// Flat credits charged per web search (`/v1/web/search`). A Tavily basic
    /// search costs one Tavily credit, $0.008 pay-as-you-go (about 8 credits);
    /// the surplus covers overhead. The authorize estimate and the settled
    /// charge are both this amount, since the upstream call is flat priced.
    pub web_search_credits: u64,
    /// Flat credits charged per web fetch (`/v1/web/fetch`). A Tavily basic
    /// extract of one URL costs at most one Tavily credit.
    pub web_fetch_credits: u64,
    /// Hold TTL for the metered web search and web fetch actions.
    pub authorize_hold_ttl_web_secs: u64,
    /// Hold TTL for the metered image generation action.
    pub authorize_hold_ttl_image_secs: u64,
    /// Hold TTL for the metered video generation/animation actions. Video is an
    /// async job billed at completion, so the hold must cover the whole job
    /// lifetime (queue -> completing poll -> charge), not a single request. Sized
    /// to the job budget plus the settlement margin and capped at the platform
    /// max (ADR 0015 Decision 2).
    #[serde(default = "default_video_hold_ttl_secs")]
    pub authorize_hold_ttl_video_secs: u64,
}

impl Debug for OsAccountsConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OsAccountsConfig")
            .field("api_url", &self.api_url)
            .field("app_api_key", &REDACTED)
            .field(
                "p3a_ingest_token",
                if self.p3a_ingest_token.trim().is_empty() {
                    &"<unset>"
                } else {
                    &REDACTED
                },
            )
            .field("iss", &self.iss)
            .field("aud", &self.aud)
            .field("jwks_refresh_secs", &self.jwks_refresh_secs)
            .field(
                "jwks_miss_min_backoff_secs",
                &self.jwks_miss_min_backoff_secs,
            )
            .field(
                "authorize_hold_ttl_note_transcribe_secs",
                &self.authorize_hold_ttl_note_transcribe_secs,
            )
            .field(
                "authorize_hold_ttl_note_generate_secs",
                &self.authorize_hold_ttl_note_generate_secs,
            )
            .field(
                "authorize_hold_ttl_dictate_transcribe_secs",
                &self.authorize_hold_ttl_dictate_transcribe_secs,
            )
            .field(
                "authorize_hold_ttl_dictate_cleanup_secs",
                &self.authorize_hold_ttl_dictate_cleanup_secs,
            )
            .field(
                "note_transcribe_preview_max_audio_secs",
                &self.note_transcribe_preview_max_audio_secs,
            )
            .field("flat_estimate_credits", &self.flat_estimate_credits)
            .field("web_search_credits", &self.web_search_credits)
            .field("web_fetch_credits", &self.web_fetch_credits)
            .field(
                "authorize_hold_ttl_web_secs",
                &self.authorize_hold_ttl_web_secs,
            )
            .field(
                "authorize_hold_ttl_image_secs",
                &self.authorize_hold_ttl_image_secs,
            )
            .field(
                "authorize_hold_ttl_video_secs",
                &self.authorize_hold_ttl_video_secs,
            )
            .finish()
    }
}

/// Clovy's service access to each upstream: Groq for audio, `OpenRouter` for
/// text, fal for images and video, Tavily for web search and fetch.
#[derive(Clone, Deserialize, Serialize)]
pub struct UpstreamsConfig {
    pub groq: UpstreamConfig,
    pub openrouter: UpstreamConfig,
    pub fal: UpstreamConfig,
    pub tavily: UpstreamConfig,
}

impl Debug for UpstreamsConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("UpstreamsConfig")
            .field("groq", &self.groq)
            .field("openrouter", &self.openrouter)
            .field("fal", &self.fal)
            .field("tavily", &self.tavily)
            .finish()
    }
}

/// One upstream API. A request carrying the user's own key for this upstream
/// goes to the same `base_url` with that key instead of `api_key`.
#[derive(Clone, Deserialize, Serialize)]
pub struct UpstreamConfig {
    #[serde(default)]
    pub api_key: String,
    pub base_url: String,
}

impl UpstreamConfig {
    /// True when Clovy holds a usable service key for this upstream: set, not
    /// blank, and not a published placeholder. Without one, only requests that
    /// carry the user's own key can use the upstream.
    pub fn has_service_key(&self) -> bool {
        let key = self.api_key.trim();
        !key.is_empty() && !UPSTREAM_API_KEY_PLACEHOLDERS.contains(&key)
    }
}

impl Debug for UpstreamConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("UpstreamConfig")
            .field("api_key", &REDACTED)
            .field("base_url", &self.base_url)
            .finish()
    }
}

/// The upstream that serves a priced text or ASR model.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelProvider {
    Groq,
    Openrouter,
}

impl ModelProvider {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Groq => "groq",
            Self::Openrouter => "openrouter",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelType {
    Asr,
    Text,
}

impl ModelType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Asr => "asr",
            Self::Text => "text",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PriceUnit {
    Seconds,
    Tokens,
}

impl PriceUnit {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Seconds => "seconds",
            Self::Tokens => "tokens",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ModelPriceConfig {
    pub unit: PriceUnit,
    #[serde(default)]
    pub credits_per_million_seconds: Option<u64>,
    #[serde(default)]
    pub input_credits_per_million_tokens: Option<u64>,
    #[serde(default)]
    pub output_credits_per_million_tokens: Option<u64>,
    pub provider: ModelProvider,
    pub model_type: ModelType,
    pub display_name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub privacy: Option<String>,
    #[serde(default)]
    pub pricing: Option<serde_json::Value>,
    #[serde(default)]
    pub context_tokens: Option<i64>,
    #[serde(default)]
    pub traits: Vec<String>,
    #[serde(default)]
    pub capabilities: Vec<String>,
}

#[derive(Clone, Copy)]
struct TextModelFallback {
    id: &'static str,
    display_name: &'static str,
    input_credits_per_million_tokens: u64,
    output_credits_per_million_tokens: u64,
    context_tokens: i64,
    capabilities: &'static [&'static str],
}

/// Built-in pricing fallback. ASR prices are Groq's published rates; text rows
/// are the `OpenRouter` catalog prices for Clovy's default models, used only
/// when the live `OpenRouter` catalog can't be reached at startup so metered
/// charges still settle. The live catalog carries the authoritative numbers
/// and extends over this on every boot. Split out of `AppConfig::default` to
/// keep that constructor under the line limit.
///
/// Usage credit prices pass through upstream cost without a Clovy markup.
/// `$1 = 1000 credits`.
fn default_pricing() -> BTreeMap<String, ModelPriceConfig> {
    let mut pricing = BTreeMap::new();
    // Groq bills Whisper per hour of audio: $0.04 for Large v3 Turbo and
    // $0.111 for Large v3 (console.groq.com/docs/speech-to-text, 2026-10-08),
    // here per million seconds rounded up. Groq bills at least 10 seconds per
    // request; at these rates that costs under one credit, and Clovy rounds
    // every metered request up to at least one credit.
    pricing.insert(
        "whisper-large-v3-turbo".to_string(),
        asr_model_config(
            "Whisper Large v3 Turbo",
            11_112,
            "Fast multilingual Whisper transcription on Groq.",
        ),
    );
    pricing.insert(
        "whisper-large-v3".to_string(),
        asr_model_config(
            "Whisper Large v3",
            30_834,
            "Most accurate multilingual Whisper transcription on Groq.",
        ),
    );
    // OpenRouter catalog prices fetched 2026-10-08, per million tokens rounded
    // up. Keep GLM 5.2 in sync with DEFAULT_GENERATION_MODEL in the Tauri
    // providers module, and Kimi K2.6 with the vision fallback.
    for model in [
        TextModelFallback {
            id: "z-ai/glm-5.2",
            display_name: "Z.ai: GLM 5.2",
            input_credits_per_million_tokens: 30,
            output_credits_per_million_tokens: 10_000,
            context_tokens: 1_048_576,
            capabilities: TEXT_TOOL_CAPABILITIES,
        },
        TextModelFallback {
            id: "moonshotai/kimi-k2.6",
            display_name: "MoonshotAI: Kimi K2.6",
            input_credits_per_million_tokens: 438,
            output_credits_per_million_tokens: 2_450,
            context_tokens: 262_144,
            // Image input, so it is the vision fallback the agent switches to
            // when an image is attached to a text-only model. Declared here so
            // that fallback still resolves when only these defaults load.
            capabilities: VISION_TOOL_CAPABILITIES,
        },
        TextModelFallback {
            id: "nvidia/nemotron-3-nano-30b-a3b",
            display_name: "NVIDIA: Nemotron 3 Nano 30B A3B",
            input_credits_per_million_tokens: 60,
            output_credits_per_million_tokens: 240,
            context_tokens: 262_144,
            capabilities: TEXT_TOOL_CAPABILITIES,
        },
        TextModelFallback {
            id: "z-ai/glm-5.1",
            display_name: "Z.ai: GLM 5.1",
            input_credits_per_million_tokens: 966,
            output_credits_per_million_tokens: 3_036,
            context_tokens: 204_800,
            capabilities: TEXT_TOOL_CAPABILITIES,
        },
        TextModelFallback {
            id: "z-ai/glm-5",
            display_name: "Z.ai: GLM 5",
            input_credits_per_million_tokens: 600,
            output_credits_per_million_tokens: 1_920,
            context_tokens: 204_800,
            capabilities: TEXT_TOOL_CAPABILITIES,
        },
        TextModelFallback {
            id: "moonshotai/kimi-k3",
            display_name: "MoonshotAI: Kimi K3",
            input_credits_per_million_tokens: 990,
            output_credits_per_million_tokens: 14_000,
            context_tokens: 1_048_576,
            capabilities: VISION_TOOL_CAPABILITIES,
        },
    ] {
        pricing.insert(model.id.to_string(), text_model_config(model));
    }
    pricing
}

/// Capability names the `OpenRouter` catalog adapter derives from a model's
/// `supported_parameters` (`tools`, `reasoning`, `structured_outputs`) and
/// input modalities (`image`).
const TEXT_TOOL_CAPABILITIES: &[&str] = &[
    "supportsFunctionCalling",
    "supportsReasoning",
    "supportsResponseSchema",
];
const VISION_TOOL_CAPABILITIES: &[&str] = &[
    "supportsFunctionCalling",
    "supportsReasoning",
    "supportsResponseSchema",
    "supportsVision",
];

fn asr_model_config(
    display_name: &str,
    credits_per_million_seconds: u64,
    description: &str,
) -> ModelPriceConfig {
    ModelPriceConfig {
        unit: PriceUnit::Seconds,
        credits_per_million_seconds: Some(credits_per_million_seconds),
        input_credits_per_million_tokens: None,
        output_credits_per_million_tokens: None,
        provider: ModelProvider::Groq,
        model_type: ModelType::Asr,
        display_name: display_name.to_string(),
        description: Some(description.to_string()),
        privacy: None,
        pricing: None,
        context_tokens: None,
        // Whisper takes a short spelling/context prompt (Groq caps it at 224
        // tokens).
        traits: vec!["prompt".to_string()],
        capabilities: Vec::new(),
    }
}

/// Per-image credit price for the curated fal image models Clovy offers. Keep
/// the ids in sync with `IMAGE_MODELS` in the frontend (`src/lib/image-models.ts`)
/// and `DEFAULT_IMAGE_MODEL` in the Tauri providers module; every id is a fal
/// endpoint id the image adapter supports. fal prices these per megapixel
/// (model pages, 2026-10-08), so each value is the cost at Clovy's largest
/// image (1280x1280, billed as 2 MP) with a ~2x margin, mirroring the flat
/// web-tool pricing. `$1 = 1000 credits`.
fn default_image_pricing() -> BTreeMap<String, u64> {
    BTreeMap::from([
        // FLUX.1 [dev]: $0.025/MP, at most $0.05.
        ("fal-ai/flux/dev".to_string(), 100),
        // FLUX.1 [schnell]: $0.003/MP, at most $0.006.
        ("fal-ai/flux/schnell".to_string(), 12),
        // Qwen Image: $0.02/MP, at most $0.04.
        ("fal-ai/qwen-image".to_string(), 80),
    ])
}

/// Flat per-edit prices, keyed by fal edit endpoint id (`$1 = 1000 credits`).
///
/// FLUX 2 Edit: a list-price estimate from the fal model page, $0.012 per
/// megapixel of input (resized to 1 MP) plus output; the page's example (1 MP
/// in, 1024x1024 out) is $0.024. The edit adapter requests an output within
/// that 1024x1024 pixel budget, but the page does not say how a megapixel is
/// counted: if it is 10^6 pixels rounded up per image, input and output bill
/// up to 2 MP each, $0.048. The price takes that conservative reading at the
/// same ~2x margin as generation: 96. Neither a rate nor a charge observed on
/// an account, nor a proven maximum. Additional edit models get added here
/// (with their own verified price) when offered.
fn default_image_edit_pricing() -> BTreeMap<String, u64> {
    BTreeMap::from([("fal-ai/flux-2/edit".to_string(), 96)])
}

/// The default edit model, and the one every MCP-driven edit uses (the tool
/// never names a model).
fn default_image_edit_model() -> String {
    "fal-ai/flux-2/edit".to_string()
}

/// The generation model that serves a retired or unknown image model id.
fn default_image_model() -> String {
    "fal-ai/flux/dev".to_string()
}

/// Curated text-to-video allowlist with per-model markups (`2000` = 2.0x, the
/// image margin). Every id is a fal text-to-video endpoint the video adapter
/// supports and quotes from its published per-second price at the requested
/// resolution. `credits = ceil(quote_usd * markup)`.
///
/// Every model here accepts the desktop fast path's fixed default duration and
/// resolution (5s / 720p) that the proxy injects when the client names none, so
/// a `/video` shot or an MCP `generate_video` call never queues an invalid
/// combination. Whether the longest 1080p clips finish inside
/// `DEFAULT_VIDEO_JOB_MAX_SECS` is not yet measured on fal.
fn default_video_pricing() -> BTreeMap<String, u32> {
    // The curated text-to-video allowlist: every id Clovy's picker offers must be
    // here; any other id is served by `default_video_model`. Markup is a
    // uniform 2.0x on the quote (ADR 0015 Decision 1), so pricier models cost
    // more without a per-model markup table. Mirrors `VIDEO_MODELS` in
    // src/lib/video-models.ts and `KNOWN_VIDEO_MODELS` in the desktop providers
    // module; keep the three in sync.
    BTreeMap::from([
        ("fal-ai/wan/v2.2-a14b/text-to-video".to_string(), 2000),
        ("alibaba/wan-3.0/text-to-video".to_string(), 2000),
    ])
}

/// Curated image-to-video (animate) allowlist: fal image-to-video endpoints,
/// keyed by model id with the same markup units.
fn default_video_animate_pricing() -> BTreeMap<String, u32> {
    BTreeMap::from([
        ("fal-ai/wan/v2.2-a14b/image-to-video".to_string(), 2000),
        ("alibaba/wan-3.0/image-to-video".to_string(), 2000),
    ])
}

/// The default image-to-video model — the one every MCP-driven animate uses
/// (the tool never names a model). Must be a key in `video_animate_pricing`.
fn default_video_animate_model() -> String {
    "fal-ai/wan/v2.2-a14b/image-to-video".to_string()
}

/// The text-to-video model that serves a retired or unknown video model id.
fn default_video_model() -> String {
    "fal-ai/wan/v2.2-a14b/text-to-video".to_string()
}

fn default_video_max_credits_per_request() -> u64 {
    DEFAULT_VIDEO_MAX_CREDITS_PER_REQUEST
}

fn default_video_max_response_bytes() -> u64 {
    DEFAULT_VIDEO_MAX_RESPONSE_BYTES
}

fn default_video_hold_ttl_secs() -> u64 {
    DEFAULT_VIDEO_HOLD_TTL_SECS
}

fn text_model_config(model: TextModelFallback) -> ModelPriceConfig {
    ModelPriceConfig {
        unit: PriceUnit::Tokens,
        credits_per_million_seconds: None,
        input_credits_per_million_tokens: Some(model.input_credits_per_million_tokens),
        output_credits_per_million_tokens: Some(model.output_credits_per_million_tokens),
        provider: ModelProvider::Openrouter,
        model_type: ModelType::Text,
        display_name: model.display_name.to_string(),
        description: None,
        privacy: None,
        pricing: None,
        context_tokens: Some(model.context_tokens),
        traits: Vec::new(),
        capabilities: model
            .capabilities
            .iter()
            .map(|capability| (*capability).to_string())
            .collect(),
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            server: ServerConfig {
                host: "127.0.0.1".to_string(),
                port: 8080,
                request_timeout_secs: DEFAULT_REQUEST_TIMEOUT_SECS,
                model_catalog_refresh_secs: 300,
                max_audio_bytes: 26_214_400,
                max_json_bytes: 524_288,
                max_issue_report_bytes: DEFAULT_MAX_ISSUE_REPORT_BYTES,
                max_image_edit_bytes: DEFAULT_MAX_IMAGE_EDIT_BYTES,
                max_agent_chat_bytes: DEFAULT_MAX_AGENT_CHAT_BYTES,
                max_agent_inflight_body_bytes: DEFAULT_MAX_AGENT_INFLIGHT_BODY_BYTES,
                max_agent_concurrent_requests_per_user:
                    DEFAULT_MAX_AGENT_CONCURRENT_REQUESTS_PER_USER,
            },
            local_dev: LocalDevConfig::default(),
            browser_transports: BrowserTransportsConfig::default(),
            os_accounts: OsAccountsConfig {
                api_url: String::new(),
                app_api_key: String::new(),
                p3a_ingest_token: String::new(),
                iss: "os-accounts-dev".to_string(),
                aud: "clovy-api-dev".to_string(),
                jwks_refresh_secs: 300,
                jwks_miss_min_backoff_secs: 5,
                authorize_hold_ttl_note_transcribe_secs: 60,
                authorize_hold_ttl_note_generate_secs: DEFAULT_IMAGE_HOLD_TTL_SECS,
                authorize_hold_ttl_dictate_transcribe_secs: 30,
                authorize_hold_ttl_dictate_cleanup_secs: 30,
                note_transcribe_preview_max_audio_secs: 30,
                flat_estimate_credits: 250,
                web_search_credits: 20,
                web_fetch_credits: 20,
                authorize_hold_ttl_web_secs: 30,
                authorize_hold_ttl_image_secs: DEFAULT_IMAGE_HOLD_TTL_SECS,
                authorize_hold_ttl_video_secs: DEFAULT_VIDEO_HOLD_TTL_SECS,
            },
            upstreams: UpstreamsConfig {
                groq: UpstreamConfig {
                    api_key: String::new(),
                    base_url: "https://api.groq.com/openai/v1".to_string(),
                },
                openrouter: UpstreamConfig {
                    api_key: String::new(),
                    base_url: "https://openrouter.ai/api/v1".to_string(),
                },
                fal: UpstreamConfig {
                    api_key: String::new(),
                    base_url: "https://queue.fal.run".to_string(),
                },
                tavily: UpstreamConfig {
                    api_key: String::new(),
                    base_url: "https://api.tavily.com".to_string(),
                },
            },
            attestation: AttestationConfig {
                source_commit: String::new(),
                source_repo_url: "https://github.com/open-software-network/os-clovy".to_string(),
                image_repo: "ghcr.io/open-software-network/clovy-api".to_string(),
                trust_center_url:
                    "https://trust.phala.com/app/6514acb0e08dc4825e2b6e22a46f0ed0ff455b54"
                        .to_string(),
            },
            issue_reports: IssueReportsConfig::default(),
            computer_use: ComputerUseConfig::default(),
            share: ShareConfig::default(),
            companion: CompanionConfig::default(),
            pricing: default_pricing(),
            image_pricing: default_image_pricing(),
            image_edit_pricing: default_image_edit_pricing(),
            default_image_edit_model: default_image_edit_model(),
            default_image_model: default_image_model(),
            video_pricing: default_video_pricing(),
            default_video_model: default_video_model(),
            video_animate_pricing: default_video_animate_pricing(),
            default_video_animate_model: default_video_animate_model(),
            video_max_credits_per_request: default_video_max_credits_per_request(),
            video_max_response_bytes: default_video_max_response_bytes(),
        }
    }
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error(transparent)]
    Figment(#[from] Box<figment::Error>),
    #[error("missing required config `{field}`")]
    MissingRequired { field: &'static str },
    #[error("invalid required config `{field}`: {reason}")]
    InvalidRequired {
        field: &'static str,
        reason: &'static str,
    },
    #[error("invalid pricing for model `{model}`: {reason}")]
    InvalidPricing { model: String, reason: String },
}

fn merge_compatibility_namespaces(
    figment: Figment,
    legacy: impl Provider,
    canonical: impl Provider,
) -> Figment {
    figment.merge(legacy).merge(canonical)
}

pub fn load() -> Result<AppConfig, ConfigError> {
    let base = Figment::new()
        .merge(Serialized::defaults(AppConfig::default()))
        .merge(Toml::file("config.toml"));
    // Released deployments still provide JUNE__ variables. Merge them first,
    // then let the Clovy-canonical namespace override field by field so
    // rollout can be gradual and reversible.
    let config: AppConfig = merge_compatibility_namespaces(
        base,
        Env::prefixed("JUNE__").split("__"),
        Env::prefixed("CLOVY__").split("__"),
    )
    .extract()
    .map_err(Box::new)
    .map_err(ConfigError::from)?;
    validate(&config)?;
    Ok(config)
}

const LEGACY_OS_ACCOUNTS_APP_API_KEY_PLACEHOLDER: &str = concat!("osk", "_REPLACE_ME");
const OS_ACCOUNTS_APP_API_KEY_PLACEHOLDERS: &[&str] = &[
    "REPLACE_WITH_OS_ACCOUNTS_APP_API_KEY",
    LEGACY_OS_ACCOUNTS_APP_API_KEY_PLACEHOLDER,
];
const LOCAL_DEV_BEARER_TOKEN_PLACEHOLDERS: &[&str] = &[LOCAL_DEV_BEARER_TOKEN_PLACEHOLDER];

fn validate(config: &AppConfig) -> Result<(), ConfigError> {
    if config.share.viewer_only {
        return validate_viewer_only(config);
    }

    validate_server_config(config)?;
    if config.local_dev.enabled {
        validate_local_dev_bearer_token(config)?;
        validate_required_text("local_dev.user_id", &config.local_dev.user_id)?;
        if !config.local_dev.user_id.starts_with("usr_") {
            return Err(ConfigError::InvalidRequired {
                field: "local_dev.user_id",
                reason: "must start with usr_",
            });
        }
    } else {
        validate_absolute_http_url("os_accounts.api_url", &config.os_accounts.api_url)?;
        validate_required_secret(
            "os_accounts.app_api_key",
            &config.os_accounts.app_api_key,
            OS_ACCOUNTS_APP_API_KEY_PLACEHOLDERS,
        )?;
        // os_accounts.p3a_ingest_token is deliberately not validated here:
        // the token is optional, and when unset the P3A sink degrades to
        // log-only instead of failing startup (JUN-231).
    }
    validate_request_limits(config)?;
    validate_issue_report_diagnosis(config)?;

    // Every capability's upstream needs its base URL. Outside local dev Clovy
    // also needs its own service key for each, so service-funded requests work
    // at startup; requests carrying the user's own key need no service key.
    for (base_url_field, api_key_field, upstream) in [
        (
            "upstreams.groq.base_url",
            "upstreams.groq.api_key",
            &config.upstreams.groq,
        ),
        (
            "upstreams.openrouter.base_url",
            "upstreams.openrouter.api_key",
            &config.upstreams.openrouter,
        ),
        (
            "upstreams.fal.base_url",
            "upstreams.fal.api_key",
            &config.upstreams.fal,
        ),
        (
            "upstreams.tavily.base_url",
            "upstreams.tavily.api_key",
            &config.upstreams.tavily,
        ),
    ] {
        validate_absolute_http_url(base_url_field, &upstream.base_url)?;
        if !config.local_dev.enabled {
            validate_required_secret(
                api_key_field,
                &upstream.api_key,
                UPSTREAM_API_KEY_PLACEHOLDERS,
            )?;
        }
    }

    for (model_id, pricing) in &config.pricing {
        let expected_unit = match pricing.model_type {
            ModelType::Asr => PriceUnit::Seconds,
            ModelType::Text => PriceUnit::Tokens,
        };
        if pricing.unit != expected_unit {
            return Err(ConfigError::InvalidPricing {
                model: model_id.clone(),
                reason: format!(
                    "model_type `{}` requires unit `{}`, got `{}`",
                    pricing.model_type.as_str(),
                    expected_unit.as_str(),
                    pricing.unit.as_str()
                ),
            });
        }
        match pricing.model_type {
            ModelType::Asr => {
                validate_positive_rate(
                    model_id,
                    pricing.credits_per_million_seconds,
                    "credits_per_million_seconds",
                )?;
            }
            ModelType::Text => {
                validate_positive_rate(
                    model_id,
                    pricing.input_credits_per_million_tokens,
                    "input_credits_per_million_tokens",
                )?;
                validate_positive_rate(
                    model_id,
                    pricing.output_credits_per_million_tokens,
                    "output_credits_per_million_tokens",
                )?;
            }
        }
    }
    validate_image_pricing(config)?;
    validate_video_pricing(config)?;
    Ok(())
}

fn validate_server_config(config: &AppConfig) -> Result<(), ConfigError> {
    validate_positive_config(
        "server.model_catalog_refresh_secs",
        config.server.model_catalog_refresh_secs,
    )
}

/// The isolated short-link process does not expose product routes and must not
/// need their billing/provider credentials. Validate only the network origins
/// and limits its viewer routes actually use. Sharing itself still fails closed
/// at runtime (501) when the database or public client id is absent; the deploy
/// contract probes the shell and a missing link so a partially sealed CVM
/// cannot pass activation.
fn validate_viewer_only(config: &AppConfig) -> Result<(), ConfigError> {
    validate_absolute_http_url("os_accounts.api_url", &config.os_accounts.api_url)?;
    let (viewer_accounts_field, viewer_accounts_url) =
        if config.share.viewer_accounts_url.trim().is_empty() {
            ("os_accounts.iss", &config.os_accounts.iss)
        } else {
            (
                "share.viewer_accounts_url",
                &config.share.viewer_accounts_url,
            )
        };
    validate_absolute_http_url(viewer_accounts_field, viewer_accounts_url)?;
    validate_request_limits(config)?;
    if config.share.max_ciphertext_bytes == 0 {
        return Err(ConfigError::InvalidRequired {
            field: "share.max_ciphertext_bytes",
            reason: "must be > 0",
        });
    }
    Ok(())
}

/// Video markups (thousandths) must be > 0 — a zero markup would charge nothing
/// on a paid quote — and the ceiling must be > 0. The default animate model must
/// be priced, since every MCP-driven animate uses it. Mirrors
/// `validate_image_pricing`, adapted for quote-derived pricing.
fn validate_video_pricing(config: &AppConfig) -> Result<(), ConfigError> {
    for (model_id, markup) in config
        .video_pricing
        .iter()
        .chain(config.video_animate_pricing.iter())
    {
        if *markup == 0 {
            return Err(ConfigError::InvalidPricing {
                model: model_id.clone(),
                reason: "video markup_millis must be > 0".to_string(),
            });
        }
    }
    if config.video_max_credits_per_request == 0 {
        return Err(ConfigError::InvalidRequired {
            field: "video_max_credits_per_request",
            reason: "must be > 0",
        });
    }
    if config.video_max_response_bytes == 0 {
        return Err(ConfigError::InvalidRequired {
            field: "video_max_response_bytes",
            reason: "must be > 0",
        });
    }
    for (field, model, allowlist) in [
        (
            "default_video_model",
            &config.default_video_model,
            &config.video_pricing,
        ),
        (
            "default_video_animate_model",
            &config.default_video_animate_model,
            &config.video_animate_pricing,
        ),
    ] {
        if !allowlist.contains_key(model) {
            return Err(ConfigError::InvalidPricing {
                model: model.clone(),
                reason: format!("{field} must be present in its video pricing allowlist"),
            });
        }
    }
    Ok(())
}

fn validate_issue_report_diagnosis(config: &AppConfig) -> Result<(), ConfigError> {
    if config
        .issue_reports
        .diagnosis_model
        .as_deref()
        .map(str::trim)
        .is_some_and(|model| !model.is_empty())
    {
        validate_positive_config(
            "issue_reports.diagnosis_timeout_secs",
            config.issue_reports.diagnosis_timeout_secs,
        )?;
        validate_positive_config(
            "issue_reports.diagnosis_max_per_user_per_hour",
            config.issue_reports.diagnosis_max_per_user_per_hour,
        )?;
    }
    Ok(())
}

fn validate_request_limits(config: &AppConfig) -> Result<(), ConfigError> {
    validate_positive_config(
        "os_accounts.note_transcribe_preview_max_audio_secs",
        config.os_accounts.note_transcribe_preview_max_audio_secs,
    )?;
    validate_image_timeout_margin(config)?;
    validate_positive_usize_config(
        "server.max_image_edit_bytes",
        config.server.max_image_edit_bytes,
    )?;
    validate_positive_usize_config(
        "server.max_agent_chat_bytes",
        config.server.max_agent_chat_bytes,
    )?;
    validate_positive_usize_config(
        "server.max_agent_inflight_body_bytes",
        config.server.max_agent_inflight_body_bytes,
    )?;
    validate_positive_usize_config(
        "server.max_agent_concurrent_requests_per_user",
        config.server.max_agent_concurrent_requests_per_user,
    )?;
    // The global in-flight body budget must be at least the largest single
    // large-body route cap (image edit, ~66 MiB), or the admission control would
    // load-shed EVERY request on that route — and, worse, an operator could tune
    // the budget below a route cap and defeat the memory-safety guarantee it
    // exists to provide (JUN-336 review). max_image_edit_bytes is the largest of
    // the agent route caps (image/video 66 MiB > audio 25 MiB > chat 12 MiB).
    if config.server.max_agent_inflight_body_bytes < config.server.max_image_edit_bytes {
        return Err(ConfigError::InvalidRequired {
            field: "server.max_agent_inflight_body_bytes",
            reason: "must be >= the largest agent route body cap (server.max_image_edit_bytes)",
        });
    }
    // The extractor cap must never sit BELOW the desktop provider proxy's fixed
    // 12 MiB chat body cap (mirrored here as `DEFAULT_MAX_AGENT_CHAT_BYTES`), or a
    // configured override silently reintroduces the JUN-336 regression: the proxy
    // still forwards a 1-12 MiB agent chat request, but this route 413s it before
    // `validate_agent_chat_body` runs. `max_json_bytes` is NOT the right floor —
    // an override of e.g. 1 MiB clears it yet is still stricter than the proxy.
    // The compile-time asserts only pin the default; this guards overrides.
    if config.server.max_agent_chat_bytes < DEFAULT_MAX_AGENT_CHAT_BYTES {
        return Err(ConfigError::InvalidRequired {
            field: "server.max_agent_chat_bytes",
            reason: "must be >= the 12 MiB desktop proxy chat body cap",
        });
    }
    validate_positive_usize_config(
        "server.max_issue_report_bytes",
        config.server.max_issue_report_bytes,
    )?;
    validate_image_hold_ttl(config)?;
    validate_video_hold_ttl(config)?;
    validate_long_inference_hold_ttl(config)?;
    validate_hold_ttl_bounds(config)?;
    Ok(())
}

/// The video hold has to cover the whole async job: the per-job budget plus the
/// settlement margin for the completing poll's charge. A hold below that would
/// expire mid-job and strand the charge (ADR 0015 Decision 2).
fn validate_video_hold_ttl(config: &AppConfig) -> Result<(), ConfigError> {
    let minimum = DEFAULT_VIDEO_JOB_MAX_SECS.saturating_add(VIDEO_SETTLEMENT_TIMEOUT_MARGIN_SECS);
    if config.os_accounts.authorize_hold_ttl_video_secs < minimum {
        return Err(ConfigError::InvalidRequired {
            field: "os_accounts.authorize_hold_ttl_video_secs",
            reason: "must cover the video job budget plus the settlement margin",
        });
    }
    Ok(())
}

fn validate_image_hold_ttl(config: &AppConfig) -> Result<(), ConfigError> {
    // The hold has to cover the image *client* timeout (route timeout minus
    // the settlement budget) plus that same settlement budget for the charge
    // path. Anchoring on the route timeout plus a margin demanded 630s, which
    // OS Accounts rejects (see OS_ACCOUNTS_MAX_HOLD_TTL_SECS). Saturating
    // math keeps this safe even when the route timeout is itself invalid
    // (rejected by the margin check).
    let minimum = config
        .server
        .request_timeout_secs
        .saturating_sub(OS_ACCOUNTS_AUTHORIZE_TIMEOUT_BUDGET_SECS)
        .saturating_sub(IMAGE_SETTLEMENT_TIMEOUT_MARGIN_SECS)
        .saturating_add(IMAGE_SETTLEMENT_TIMEOUT_MARGIN_SECS);
    if config.os_accounts.authorize_hold_ttl_image_secs < minimum {
        return Err(ConfigError::InvalidRequired {
            field: "os_accounts.authorize_hold_ttl_image_secs",
            reason: "must cover the image client timeout plus the settlement budget",
        });
    }
    Ok(())
}

/// Note generation and agent chat run on the bounded metered-inference client
/// (same window as images) and settle AFTER the upstream call — streamed chat
/// settles after the body drains, streamed generate keeps the connection
/// alive for the whole window. Their shared hold must therefore cover that
/// client window plus the settlement budget, exactly like the image hold;
/// a shorter hold silently expires before `charge` on every long call.
fn validate_long_inference_hold_ttl(config: &AppConfig) -> Result<(), ConfigError> {
    let minimum = config
        .server
        .request_timeout_secs
        .saturating_sub(OS_ACCOUNTS_AUTHORIZE_TIMEOUT_BUDGET_SECS)
        .saturating_sub(IMAGE_SETTLEMENT_TIMEOUT_MARGIN_SECS)
        .saturating_add(IMAGE_SETTLEMENT_TIMEOUT_MARGIN_SECS);
    if config.os_accounts.authorize_hold_ttl_note_generate_secs < minimum {
        return Err(ConfigError::InvalidRequired {
            field: "os_accounts.authorize_hold_ttl_note_generate_secs",
            reason: "must cover the metered-inference client timeout plus the settlement budget",
        });
    }
    Ok(())
}

/// Every authorize hold TTL must fit the OS Accounts platform bounds; a value
/// past the cap is rejected `invalid_ttl` on every request, which surfaces to
/// users as `metering_provider_failed` and disables the whole action.
fn validate_hold_ttl_bounds(config: &AppConfig) -> Result<(), ConfigError> {
    let ttls: [(&'static str, u64); 7] = [
        (
            "os_accounts.authorize_hold_ttl_note_transcribe_secs",
            config.os_accounts.authorize_hold_ttl_note_transcribe_secs,
        ),
        (
            "os_accounts.authorize_hold_ttl_note_generate_secs",
            config.os_accounts.authorize_hold_ttl_note_generate_secs,
        ),
        (
            "os_accounts.authorize_hold_ttl_dictate_transcribe_secs",
            config
                .os_accounts
                .authorize_hold_ttl_dictate_transcribe_secs,
        ),
        (
            "os_accounts.authorize_hold_ttl_dictate_cleanup_secs",
            config.os_accounts.authorize_hold_ttl_dictate_cleanup_secs,
        ),
        (
            "os_accounts.authorize_hold_ttl_web_secs",
            config.os_accounts.authorize_hold_ttl_web_secs,
        ),
        (
            "os_accounts.authorize_hold_ttl_image_secs",
            config.os_accounts.authorize_hold_ttl_image_secs,
        ),
        (
            "os_accounts.authorize_hold_ttl_video_secs",
            config.os_accounts.authorize_hold_ttl_video_secs,
        ),
    ];
    for (field, value) in ttls {
        if !(1..=OS_ACCOUNTS_MAX_HOLD_TTL_SECS).contains(&value) {
            return Err(ConfigError::InvalidRequired {
                field,
                reason: "must be within the OS Accounts hold TTL bounds (1..=900 seconds)",
            });
        }
    }
    Ok(())
}

fn validate_image_timeout_margin(config: &AppConfig) -> Result<(), ConfigError> {
    if config.server.request_timeout_secs
        <= OS_ACCOUNTS_AUTHORIZE_TIMEOUT_BUDGET_SECS + IMAGE_SETTLEMENT_TIMEOUT_MARGIN_SECS
    {
        return Err(ConfigError::InvalidRequired {
            field: "server.request_timeout_secs",
            reason: "must exceed the authorize budget plus the image settlement margin",
        });
    }
    Ok(())
}

/// A zero per-image price would silently generate/edit for free — reject it like
/// the per-model rate validation so a misconfigured price fails fast. Also
/// guards that the default edit model is actually priced, since every MCP-driven
/// edit uses it and an unpriced model would fail every edit at runtime.
fn validate_image_pricing(config: &AppConfig) -> Result<(), ConfigError> {
    for (model_id, credits) in config
        .image_pricing
        .iter()
        .chain(config.image_edit_pricing.iter())
    {
        if *credits == 0 {
            return Err(ConfigError::InvalidPricing {
                model: model_id.clone(),
                reason: "credits_per_image must be > 0".to_string(),
            });
        }
    }
    for (field, model, allowlist) in [
        (
            "default_image_model",
            &config.default_image_model,
            &config.image_pricing,
        ),
        (
            "default_image_edit_model",
            &config.default_image_edit_model,
            &config.image_edit_pricing,
        ),
    ] {
        if !allowlist.contains_key(model) {
            return Err(ConfigError::InvalidPricing {
                model: model.clone(),
                reason: format!("{field} must be present in its image pricing allowlist"),
            });
        }
    }
    Ok(())
}

/// Local-dev mode skips real OS Accounts auth and metering, so its bearer
/// token is the only credential on the API. The placeholder default is publicly
/// known (it ships in `.env.example` and in the desktop shell), which is safe
/// only while the server binds a loopback address. On a non-loopback bind the
/// placeholder is rejected at startup (security audit 2026-07-17): anyone who
/// could reach the port would otherwise authenticate as the local dev user.
fn validate_local_dev_bearer_token(config: &AppConfig) -> Result<(), ConfigError> {
    validate_required_text("local_dev.bearer_token", &config.local_dev.bearer_token)?;
    if is_loopback_host(&config.server.host) {
        return Ok(());
    }
    if LOCAL_DEV_BEARER_TOKEN_PLACEHOLDERS
        .iter()
        .any(|placeholder| config.local_dev.bearer_token.trim() == *placeholder)
    {
        return Err(ConfigError::InvalidRequired {
            field: "local_dev.bearer_token",
            reason: "the default placeholder token is publicly known, and local-dev mode skips \
                metering, so binding a non-loopback host with it lets anyone who can reach this \
                port authenticate as the local dev user; set CLOVY__LOCAL_DEV__BEARER_TOKEN to a \
                random value (e.g. `openssl rand -hex 32`) or bind server.host to a loopback \
                address (127.0.0.0/8 or ::1)",
        });
    }
    Ok(())
}

/// Loopback is the whole IPv4 127.0.0.0/8 range or IPv6 `::1`, not just
/// 127.0.0.1: any of them keeps the API unreachable from off-host. Hostnames
/// other than `localhost` and unparseable values are treated as non-loopback
/// (fail closed) — the placeholder token check then applies.
fn is_loopback_host(host: &str) -> bool {
    let host = host.trim();
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    // Accept the bracketed IPv6 form ("[::1]") as written in some configs.
    let host = host
        .strip_prefix('[')
        .and_then(|inner| inner.strip_suffix(']'))
        .unwrap_or(host);
    host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

fn validate_required_text(field: &'static str, value: &str) -> Result<(), ConfigError> {
    if value.trim().is_empty() {
        return Err(ConfigError::MissingRequired { field });
    }
    Ok(())
}

fn validate_required_secret(
    field: &'static str,
    value: &str,
    placeholders: &[&str],
) -> Result<(), ConfigError> {
    validate_required_text(field, value)?;
    if placeholders
        .iter()
        .any(|placeholder| value.trim() == *placeholder)
    {
        return Err(ConfigError::InvalidRequired {
            field,
            reason: "placeholder value must be replaced",
        });
    }
    Ok(())
}

fn validate_absolute_http_url(field: &'static str, value: &str) -> Result<(), ConfigError> {
    validate_required_text(field, value)?;
    let parsed = Url::parse(value.trim()).map_err(|_| ConfigError::InvalidRequired {
        field,
        reason: "must be an absolute http or https URL",
    })?;
    if matches!(parsed.scheme(), "http" | "https") && parsed.has_host() {
        return Ok(());
    }
    Err(ConfigError::InvalidRequired {
        field,
        reason: "must be an absolute http or https URL",
    })
}

fn validate_positive_config(field: &'static str, value: u64) -> Result<(), ConfigError> {
    if value == 0 {
        return Err(ConfigError::InvalidRequired {
            field,
            reason: "must be > 0",
        });
    }
    Ok(())
}

fn validate_positive_usize_config(field: &'static str, value: usize) -> Result<(), ConfigError> {
    if value == 0 {
        return Err(ConfigError::InvalidRequired {
            field,
            reason: "must be > 0",
        });
    }
    Ok(())
}

fn validate_positive_rate(
    model_id: &str,
    value: Option<u64>,
    field: &str,
) -> Result<(), ConfigError> {
    match value {
        Some(value) if value > 0 => Ok(()),
        _ => Err(ConfigError::InvalidPricing {
            model: model_id.to_string(),
            reason: format!("{field} must be > 0"),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AppConfig, ConfigError, DEFAULT_IMAGE_CLIENT_TIMEOUT_SECS, DEFAULT_IMAGE_HOLD_TTL_SECS,
        DEFAULT_MAX_AGENT_CHAT_BYTES, DEFAULT_MAX_AGENT_CONCURRENT_REQUESTS_PER_USER,
        DEFAULT_MAX_AGENT_INFLIGHT_BODY_BYTES, DEFAULT_MAX_IMAGE_EDIT_BYTES,
        DEFAULT_MAX_ISSUE_REPORT_BYTES, DEFAULT_REQUEST_TIMEOUT_SECS, DEFAULT_VIDEO_HOLD_TTL_SECS,
        DEFAULT_VIDEO_JOB_MAX_SECS, DEFAULT_VIDEO_MAX_RESPONSE_BYTES, IMAGE_EDIT_SOURCE_MAX_BYTES,
        IMAGE_SETTLEMENT_TIMEOUT_MARGIN_SECS, ISSUE_REPORT_ATTACHMENT_MAX_BYTES, ModelPriceConfig,
        ModelProvider, ModelType, OS_ACCOUNTS_APP_API_KEY_PLACEHOLDERS,
        OS_ACCOUNTS_AUTHORIZE_TIMEOUT_BUDGET_SECS, OS_ACCOUNTS_MAX_HOLD_TTL_SECS, PriceUnit,
        UPSTREAM_API_KEY_PLACEHOLDERS, UpstreamConfig, VIDEO_CLIENT_POLL_WINDOW_SECS,
        VIDEO_SETTLEMENT_TIMEOUT_MARGIN_SECS, image_client_timeout_secs, is_loopback_host,
        merge_compatibility_namespaces, validate, validate_request_limits,
    };
    use pretty_assertions::assert_eq;
    use std::collections::BTreeMap;

    fn valid_config() -> AppConfig {
        let mut config = AppConfig::default();
        config.os_accounts.api_url = "http://127.0.0.1:3000".to_string();
        config.os_accounts.app_api_key = "osk_test".to_string();
        config.os_accounts.p3a_ingest_token = "p3a-test-token".to_string();
        config.upstreams.groq.api_key = "gsk-test".to_string();
        config.upstreams.openrouter.api_key = "sk-or-test".to_string();
        config.upstreams.fal.api_key = "fal-test".to_string();
        config.upstreams.tavily.api_key = "tvly-test".to_string();
        config
    }

    fn packaged_config_toml() -> AppConfig {
        use figment::{
            Figment,
            providers::{Format, Serialized, Toml},
        };
        let toml_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../config.toml");
        Figment::new()
            .merge(Serialized::defaults(AppConfig::default()))
            .merge(Toml::file(toml_path))
            .extract::<AppConfig>()
            .unwrap_or_default()
    }

    #[test]
    fn clovy_environment_namespace_wins_with_june_as_fallback() {
        use figment::{Figment, providers::Serialized};
        use serde_json::json;

        let both = merge_compatibility_namespaces(
            Figment::new().merge(Serialized::defaults(AppConfig::default())),
            Serialized::defaults(json!({ "server": { "port": 8181 } })),
            Serialized::defaults(json!({ "server": { "port": 8282 } })),
        )
        .extract::<AppConfig>();
        assert!(both.is_ok());
        let both = both.unwrap_or_default();
        assert_eq!(both.server.port, 8282);

        let legacy_only = merge_compatibility_namespaces(
            Figment::new().merge(Serialized::defaults(AppConfig::default())),
            Serialized::defaults(json!({ "server": { "port": 8181 } })),
            Serialized::defaults(json!({})),
        )
        .extract::<AppConfig>();
        assert!(legacy_only.is_ok());
        let legacy_only = legacy_only.unwrap_or_default();
        assert_eq!(legacy_only.server.port, 8181);
    }

    #[test]
    fn computer_use_accepts_the_released_june_version_field() {
        use figment::{
            Figment,
            providers::{Format, Toml},
        };

        let config = Figment::new()
            .merge(Toml::string(
                "enabled = true\ndisabled_june_versions = [\"0.0.38\"]\n",
            ))
            .extract::<super::ComputerUseConfig>();
        assert!(config.is_ok());
        let config = config.unwrap_or_default();
        assert_eq!(config.disabled_clovy_versions, vec!["0.0.38"]);
    }

    #[test]
    fn share_max_ciphertext_survives_default_seeding() -> Result<(), Box<dyn std::error::Error>> {
        // load() seeds AppConfig::default() as the figment base via
        // Serialized::defaults, then layers config.toml/env on top. If
        // ShareConfig::default left max_ciphertext_bytes at usize::default()
        // (0), that 0 is present in the base and shadows the serde field
        // default, so a deployment that sets only the share envs would build a
        // 0-byte cap and reject every share. Mirror that merge with no overlay
        // and assert the real cap survives.
        use figment::{Figment, providers::Serialized};
        let config: AppConfig = Figment::new()
            .merge(Serialized::defaults(AppConfig::default()))
            .extract()?;
        assert_eq!(config.share.max_ciphertext_bytes, 10 * 1024 * 1024);
        Ok(())
    }

    #[test]
    fn viewer_only_config_does_not_require_product_secrets() {
        let mut config = AppConfig::default();
        config.share.viewer_only = true;
        config.os_accounts.api_url = "https://accounts-api.example".to_string();
        config.os_accounts.iss = "https://accounts.example".to_string();

        assert!(validate(&config).is_ok());
    }

    #[test]
    fn viewer_only_config_still_validates_its_accounts_origin() {
        let mut config = AppConfig::default();
        config.share.viewer_only = true;
        config.os_accounts.api_url = "not-a-url".to_string();
        config.os_accounts.iss = "https://accounts.example".to_string();

        assert!(validate(&config).is_err());
    }

    #[test]
    fn viewer_only_config_names_the_invalid_fallback_accounts_origin() {
        let mut config = AppConfig::default();
        config.share.viewer_only = true;
        config.os_accounts.api_url = "https://accounts-api.example".to_string();
        config.os_accounts.iss = "not-a-url".to_string();

        assert!(matches!(
            validate(&config),
            Err(ConfigError::InvalidRequired {
                field: "os_accounts.iss",
                ..
            })
        ));
    }

    #[test]
    fn viewer_only_config_names_an_invalid_explicit_accounts_origin() {
        let mut config = AppConfig::default();
        config.share.viewer_only = true;
        config.os_accounts.api_url = "https://accounts-api.example".to_string();
        config.os_accounts.iss = "https://accounts.example".to_string();
        config.share.viewer_accounts_url = "not-a-url".to_string();

        assert!(matches!(
            validate(&config),
            Err(ConfigError::InvalidRequired {
                field: "share.viewer_accounts_url",
                ..
            })
        ));
    }

    #[test]
    fn default_kimi_declares_vision_but_glm_does_not() {
        // Kimi K2.6 is the image-input fallback the app switches to, so it must
        // read as vision-capable even from these built-in defaults, before the
        // live OpenRouter catalog loads (JUN-165). The text-only GLM default
        // must not claim vision, or the fallback could land on it.
        let config = AppConfig::default();
        let declares_vision = |id: &str| {
            config
                .pricing
                .get(id)
                .is_some_and(|model| model.capabilities.iter().any(|c| c == "supportsVision"))
        };
        assert!(
            declares_vision("moonshotai/kimi-k2.6"),
            "moonshotai/kimi-k2.6 default capabilities should declare supportsVision"
        );
        assert!(
            config.pricing.contains_key("z-ai/glm-5.2"),
            "z-ai/glm-5.2 should be present in default pricing"
        );
        assert!(
            !declares_vision("z-ai/glm-5.2"),
            "GLM 5.2 default must not claim vision"
        );
    }

    #[test]
    fn browser_transport_switches_default_enabled_in_code_and_packaged_config() {
        for policy in [
            AppConfig::default().browser_transports,
            packaged_config_toml().browser_transports,
        ] {
            assert!(policy.attended_enabled);
            assert!(policy.managed_enabled);
        }
    }

    #[test]
    fn packaged_config_toml_keeps_kimi_vision_in_sync() {
        // config.toml overrides default_pricing() via Figment and ships in the
        // Docker image, so it is what `/models` serves when the live OpenRouter
        // catalog is unreachable. It must agree that Kimi is a vision model, or
        // the image-attach fallback breaks in the packaged build (JUN-165).
        let config = packaged_config_toml();
        // The diagnosis model is set only in config.toml, so it proves the file
        // actually merged and a failed load can't pass on the default alone.
        assert_eq!(
            config.issue_reports.diagnosis_model.as_deref(),
            Some("z-ai/glm-5.2"),
            "packaged config.toml did not merge"
        );
        let kimi_declares_vision = config
            .pricing
            .get("moonshotai/kimi-k2.6")
            .is_some_and(|model| model.capabilities.iter().any(|c| c == "supportsVision"));
        assert!(
            kimi_declares_vision,
            "packaged config.toml must declare supportsVision for moonshotai/kimi-k2.6"
        );
    }

    #[test]
    fn packaged_config_toml_passes_through_upstream_cost() {
        let config = packaged_config_toml();
        let rates = |id: &str| {
            config.pricing.get(id).map(|model| {
                (
                    model.provider,
                    model.credits_per_million_seconds,
                    model.input_credits_per_million_tokens,
                    model.output_credits_per_million_tokens,
                )
            })
        };

        // Groq's per-HOUR Whisper prices ($0.04 turbo, $0.111 large v3) per
        // million seconds, rounded up, at upstream cost.
        assert_eq!(
            rates("whisper-large-v3-turbo"),
            Some((ModelProvider::Groq, Some(11_112), None, None))
        );
        assert_eq!(
            rates("whisper-large-v3"),
            Some((ModelProvider::Groq, Some(30_834), None, None))
        );
        // OpenRouter catalog prices per million tokens, rounded up.
        for (model_id, input, output) in [
            ("z-ai/glm-5.2", 30, 10_000),
            ("moonshotai/kimi-k2.6", 438, 2_450),
            ("nvidia/nemotron-3-nano-30b-a3b", 60, 240),
        ] {
            assert_eq!(
                rates(model_id),
                Some((ModelProvider::Openrouter, None, Some(input), Some(output))),
                "{model_id}"
            );
        }
    }

    #[test]
    fn config_debug_redacts_secrets() {
        let mut config = AppConfig::default();
        config.os_accounts.app_api_key = "app_api_key_secret_value".to_string();
        config.local_dev.bearer_token = "local-secret-token".to_string();
        config.upstreams.groq.api_key = "gsk-secret".to_string();
        config.upstreams.openrouter.api_key = "sk-or-secret".to_string();
        config.upstreams.fal.api_key = "fal-secret".to_string();
        config.upstreams.tavily.api_key = "tvly-secret".to_string();
        let dump = format!("{config:?}");
        assert!(
            !dump.contains("app_api_key_secret_value"),
            "app_api_key leaked in Debug: {dump}"
        );
        assert!(!dump.contains("local-secret-token"));
        for secret in ["gsk-secret", "sk-or-secret", "fal-secret", "tvly-secret"] {
            assert!(!dump.contains(secret), "{secret} leaked in Debug: {dump}");
        }
        assert!(dump.contains("<redacted>"));
    }

    #[test]
    fn validate_rejects_asr_model_with_token_unit() {
        let mut pricing = BTreeMap::new();
        pricing.insert(
            "bad-asr".to_string(),
            ModelPriceConfig {
                unit: PriceUnit::Tokens,
                credits_per_million_seconds: None,
                input_credits_per_million_tokens: Some(1),
                output_credits_per_million_tokens: Some(1),
                provider: ModelProvider::Groq,
                model_type: ModelType::Asr,
                display_name: "bad".to_string(),
                description: None,
                privacy: None,
                pricing: None,
                context_tokens: None,
                traits: Vec::new(),
                capabilities: Vec::new(),
            },
        );
        let config = AppConfig {
            pricing,
            ..valid_config()
        };

        let result = validate(&config);

        assert!(result.is_err(), "expected validation error");
    }

    #[test]
    fn validate_rejects_zero_rate() {
        let mut pricing = BTreeMap::new();
        pricing.insert(
            "free-asr".to_string(),
            ModelPriceConfig {
                unit: PriceUnit::Seconds,
                credits_per_million_seconds: Some(0),
                input_credits_per_million_tokens: None,
                output_credits_per_million_tokens: None,
                provider: ModelProvider::Groq,
                model_type: ModelType::Asr,
                display_name: "free".to_string(),
                description: None,
                privacy: None,
                pricing: None,
                context_tokens: None,
                traits: Vec::new(),
                capabilities: Vec::new(),
            },
        );
        let config = AppConfig {
            pricing,
            ..valid_config()
        };

        let result = validate(&config);

        assert!(result.is_err());
    }

    #[test]
    fn validate_rejects_zero_preview_audio_cap() {
        let mut config = valid_config();
        config.os_accounts.note_transcribe_preview_max_audio_secs = 0;

        let result = validate(&config);

        assert!(result.is_err());
    }

    #[test]
    fn validate_rejects_zero_image_price() {
        // A zero per-image price would silently generate for free; reject it.
        let mut config = valid_config();
        config.image_pricing.insert("free-image".to_string(), 0);

        let result = validate(&config);

        assert!(result.is_err());
    }

    #[test]
    fn validate_rejects_zero_issue_report_diagnosis_timeout_when_model_is_set() {
        let mut config = valid_config();
        config.issue_reports.diagnosis_model = Some("text-model".to_string());
        config.issue_reports.diagnosis_timeout_secs = 0;

        let result = validate(&config);

        assert!(matches!(
            result,
            Err(ConfigError::InvalidRequired {
                field: "issue_reports.diagnosis_timeout_secs",
                reason: "must be > 0"
            })
        ));
    }

    #[test]
    fn default_image_edit_body_limit_matches_source_image_cap() {
        let expected_base64_len = IMAGE_EDIT_SOURCE_MAX_BYTES.div_ceil(3) * 4;
        assert_eq!(
            DEFAULT_MAX_IMAGE_EDIT_BYTES,
            expected_base64_len + (16 * 1024)
        );
        assert_eq!(
            AppConfig::default().server.max_image_edit_bytes,
            DEFAULT_MAX_IMAGE_EDIT_BYTES
        );
    }

    #[test]
    fn default_agent_chat_body_limit_is_the_dedicated_12_mib_cap() {
        assert_eq!(DEFAULT_MAX_AGENT_CHAT_BYTES, 12 * 1024 * 1024);
        assert_eq!(
            AppConfig::default().server.max_agent_chat_bytes,
            DEFAULT_MAX_AGENT_CHAT_BYTES
        );
        // The dedicated agent chat cap must never be stricter than the shared
        // small-JSON cap, or JUN-336 reopens.
        assert!(
            AppConfig::default().server.max_agent_chat_bytes
                >= AppConfig::default().server.max_json_bytes
        );
    }

    #[test]
    fn default_agent_admission_limits_match_their_constants() {
        let server = AppConfig::default().server;

        assert_eq!(
            server.max_agent_inflight_body_bytes,
            DEFAULT_MAX_AGENT_INFLIGHT_BODY_BYTES
        );
        assert_eq!(
            server.max_agent_concurrent_requests_per_user,
            DEFAULT_MAX_AGENT_CONCURRENT_REQUESTS_PER_USER
        );
        // The default budget must clear the largest route cap so admission never
        // load-sheds every request on a route.
        assert!(server.max_agent_inflight_body_bytes >= server.max_image_edit_bytes);
    }

    #[test]
    fn agent_inflight_budget_below_largest_route_cap_is_rejected() {
        // An operator override that drops the global budget below a single route
        // cap must fail loudly at load, not silently defeat the memory guarantee.
        let mut config = AppConfig::default();
        config.server.max_agent_inflight_body_bytes = config.server.max_image_edit_bytes - 1;
        assert!(matches!(
            validate_request_limits(&config),
            Err(ConfigError::InvalidRequired {
                field: "server.max_agent_inflight_body_bytes",
                ..
            })
        ));
    }

    #[test]
    fn agent_chat_body_limit_below_proxy_cap_is_rejected() {
        // An override BELOW the 12 MiB desktop proxy cap must fail loudly at load,
        // even when it clears the shared small-JSON cap — otherwise the proxy
        // forwards a 1-3 MiB agent chat request that this route then 413s,
        // reopening JUN-336 (Codex review on PR #776).
        let mut config = AppConfig::default();
        config.server.max_agent_chat_bytes = DEFAULT_MAX_AGENT_CHAT_BYTES - 1;
        assert!(config.server.max_agent_chat_bytes > config.server.max_json_bytes);
        assert!(matches!(
            validate_request_limits(&config),
            Err(ConfigError::InvalidRequired {
                field: "server.max_agent_chat_bytes",
                ..
            })
        ));
    }

    #[test]
    fn default_issue_report_body_limit_matches_platform_attachment_cap() {
        assert_eq!(ISSUE_REPORT_ATTACHMENT_MAX_BYTES, 300 * 1024 * 1024);
        assert_eq!(
            AppConfig::default().server.max_issue_report_bytes,
            ISSUE_REPORT_ATTACHMENT_MAX_BYTES + (1024 * 1024)
        );
    }

    #[test]
    fn production_ingress_accepts_the_issue_report_body_budget() -> Result<(), String> {
        let compose = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../deploy/docker-compose.production.yml"
        ));
        let configured_limits = compose
            .lines()
            .filter_map(|line| {
                line.trim()
                    .strip_prefix("- CLIENT_MAX_BODY_SIZE=")
                    .map(str::trim)
            })
            .collect::<Vec<_>>();
        assert_eq!(
            configured_limits.len(),
            1,
            "production compose must declare one ingress body limit"
        );
        let configured_limit = configured_limits
            .first()
            .copied()
            .ok_or_else(|| "production ingress body limit is missing".to_string())?;
        let configured_mib = configured_limit
            .strip_suffix('m')
            .ok_or_else(|| "ingress limit should use nginx MiB units".to_string())?
            .parse::<usize>()
            .map_err(|error| format!("ingress limit should be numeric: {error}"))?;
        let configured_bytes = configured_mib
            .checked_mul(1024 * 1024)
            .ok_or_else(|| "ingress body limit should fit usize".to_string())?;

        assert!(
            configured_bytes >= DEFAULT_MAX_ISSUE_REPORT_BYTES,
            "production ingress body limit ({configured_bytes}) is below Clovy API's issue-report limit ({DEFAULT_MAX_ISSUE_REPORT_BYTES})"
        );
        Ok(())
    }

    #[test]
    fn default_image_hold_ttl_covers_client_timeout_within_platform_cap() {
        let config = AppConfig::default();
        assert_eq!(
            config.os_accounts.authorize_hold_ttl_image_secs,
            image_client_timeout_secs(config.server.request_timeout_secs)
                + IMAGE_SETTLEMENT_TIMEOUT_MARGIN_SECS
        );
        assert_eq!(
            config.os_accounts.authorize_hold_ttl_image_secs,
            DEFAULT_IMAGE_HOLD_TTL_SECS
        );
        // The <= OS_ACCOUNTS_MAX_HOLD_TTL_SECS regression pin lives as a
        // compile-time const assertion next to the constant itself.
    }

    #[test]
    fn validate_rejects_image_hold_ttl_above_os_accounts_cap() {
        let mut config = valid_config();
        config.os_accounts.authorize_hold_ttl_image_secs = OS_ACCOUNTS_MAX_HOLD_TTL_SECS + 30;

        let result = validate(&config);

        assert!(matches!(
            result,
            Err(ConfigError::InvalidRequired {
                field: "os_accounts.authorize_hold_ttl_image_secs",
                ..
            })
        ));
    }

    #[test]
    fn validate_rejects_any_hold_ttl_outside_platform_bounds() {
        let mut config = valid_config();
        config.os_accounts.authorize_hold_ttl_web_secs = OS_ACCOUNTS_MAX_HOLD_TTL_SECS + 1;
        assert!(matches!(
            validate(&config),
            Err(ConfigError::InvalidRequired {
                field: "os_accounts.authorize_hold_ttl_web_secs",
                ..
            })
        ));

        let mut config = valid_config();
        config.os_accounts.authorize_hold_ttl_dictate_cleanup_secs = 0;
        assert!(matches!(
            validate(&config),
            Err(ConfigError::InvalidRequired {
                field: "os_accounts.authorize_hold_ttl_dictate_cleanup_secs",
                ..
            })
        ));
    }

    #[test]
    fn default_image_client_timeout_leaves_settlement_margin_before_route_timeout() {
        let config = AppConfig::default();
        let image_client_timeout = image_client_timeout_secs(config.server.request_timeout_secs);

        assert_eq!(image_client_timeout, DEFAULT_IMAGE_CLIENT_TIMEOUT_SECS);
        assert!(image_client_timeout < config.server.request_timeout_secs);
        assert!(
            image_client_timeout + IMAGE_SETTLEMENT_TIMEOUT_MARGIN_SECS
                <= config.server.request_timeout_secs
        );
    }

    #[test]
    fn validate_rejects_image_route_timeout_without_settlement_margin() {
        let mut config = valid_config();
        config.server.request_timeout_secs =
            OS_ACCOUNTS_AUTHORIZE_TIMEOUT_BUDGET_SECS + IMAGE_SETTLEMENT_TIMEOUT_MARGIN_SECS;
        config.os_accounts.authorize_hold_ttl_image_secs = DEFAULT_IMAGE_HOLD_TTL_SECS;

        let result = validate(&config);

        assert!(matches!(
            result,
            Err(ConfigError::InvalidRequired {
                field: "server.request_timeout_secs",
                ..
            })
        ));
    }

    #[test]
    fn validate_rejects_image_hold_ttl_below_client_timeout_margin() {
        let mut config = valid_config();
        config.server.request_timeout_secs = DEFAULT_REQUEST_TIMEOUT_SECS;
        config.os_accounts.authorize_hold_ttl_image_secs = 60;

        let result = validate(&config);

        assert!(matches!(
            result,
            Err(ConfigError::InvalidRequired {
                field: "os_accounts.authorize_hold_ttl_image_secs",
                ..
            })
        ));
    }

    #[test]
    fn validate_rejects_zero_video_markup() {
        // A zero markup would charge nothing on a paid quote; reject it.
        let mut config = valid_config();
        config.video_pricing.insert("free-video".to_string(), 0);

        assert!(validate(&config).is_err());
    }

    #[test]
    fn validate_rejects_zero_video_ceiling() {
        let mut config = valid_config();
        config.video_max_credits_per_request = 0;

        assert!(matches!(
            validate(&config),
            Err(ConfigError::InvalidRequired {
                field: "video_max_credits_per_request",
                ..
            })
        ));
    }

    #[test]
    fn validate_rejects_zero_video_response_cap() {
        let mut config = valid_config();
        config.video_max_response_bytes = 0;

        assert!(matches!(
            validate(&config),
            Err(ConfigError::InvalidRequired {
                field: "video_max_response_bytes",
                ..
            })
        ));
    }

    #[test]
    fn validate_rejects_unpriced_default_media_models() {
        let unpriced_defaults: [fn(&mut AppConfig); 4] = [
            |config| config.default_image_model = "not-priced".to_string(),
            |config| config.default_image_edit_model = "not-priced".to_string(),
            |config| config.default_video_model = "not-priced".to_string(),
            |config| config.default_video_animate_model = "not-priced".to_string(),
        ];
        for unpriced in unpriced_defaults {
            let mut config = valid_config();
            unpriced(&mut config);

            assert!(matches!(
                validate(&config),
                Err(ConfigError::InvalidPricing { model, .. }) if model == "not-priced"
            ));
        }
    }

    #[test]
    fn default_video_hold_ttl_covers_job_budget_within_platform_cap() {
        let config = AppConfig::default();
        assert_eq!(
            config.os_accounts.authorize_hold_ttl_video_secs,
            DEFAULT_VIDEO_HOLD_TTL_SECS
        );
        // The hold covers the whole job (budget + settlement margin) ...
        assert!(
            config.os_accounts.authorize_hold_ttl_video_secs
                >= DEFAULT_VIDEO_JOB_MAX_SECS + VIDEO_SETTLEMENT_TIMEOUT_MARGIN_SECS
        );
        // ... and covers the full desktop polling window (360 x 2.5s).
        assert!(config.os_accounts.authorize_hold_ttl_video_secs >= VIDEO_CLIENT_POLL_WINDOW_SECS);
        // ... and still fits inside the OS Accounts platform cap.
        assert!(config.os_accounts.authorize_hold_ttl_video_secs <= OS_ACCOUNTS_MAX_HOLD_TTL_SECS);
        // The default ceiling is positive (a zero ceiling would reject every quote).
        assert_eq!(config.video_max_credits_per_request, 20_000);
        assert_eq!(
            config.video_max_response_bytes,
            DEFAULT_VIDEO_MAX_RESPONSE_BYTES
        );
    }

    #[test]
    fn validate_rejects_video_hold_ttl_below_job_budget() {
        let mut config = valid_config();
        config.os_accounts.authorize_hold_ttl_video_secs = DEFAULT_VIDEO_JOB_MAX_SECS;

        assert!(matches!(
            validate(&config),
            Err(ConfigError::InvalidRequired {
                field: "os_accounts.authorize_hold_ttl_video_secs",
                ..
            })
        ));
    }

    #[test]
    fn validate_rejects_video_hold_ttl_above_os_accounts_cap() {
        let mut config = valid_config();
        config.os_accounts.authorize_hold_ttl_video_secs = OS_ACCOUNTS_MAX_HOLD_TTL_SECS + 30;

        assert!(matches!(
            validate(&config),
            Err(ConfigError::InvalidRequired {
                field: "os_accounts.authorize_hold_ttl_video_secs",
                ..
            })
        ));
    }

    #[test]
    fn validate_passes_for_complete_config() {
        assert_eq!(validate(&valid_config()).is_ok(), true);
    }

    #[test]
    fn validate_rejects_missing_os_accounts_key() {
        let mut config = valid_config();
        config.os_accounts.app_api_key = String::new();

        let result = validate(&config);

        assert!(result.is_err());
    }

    #[test]
    fn validate_accepts_missing_p3a_ingest_token() {
        let mut config = valid_config();
        config.os_accounts.p3a_ingest_token = String::new();

        let result = validate(&config);

        assert!(
            result.is_ok(),
            "missing P3A ingest token must not fail validation: {result:?}"
        );
    }

    #[test]
    fn validate_rejects_schemeless_os_accounts_api_url() {
        let mut config = valid_config();
        config.os_accounts.api_url = "accounts.opensoftware.co/api".to_string();

        let result = validate(&config);

        assert!(matches!(
            result,
            Err(ConfigError::InvalidRequired {
                field: "os_accounts.api_url",
                reason: "must be an absolute http or https URL"
            })
        ));
    }

    #[test]
    fn validate_rejects_os_accounts_placeholder_keys() {
        for placeholder in OS_ACCOUNTS_APP_API_KEY_PLACEHOLDERS {
            let mut config = valid_config();
            config.os_accounts.app_api_key = (*placeholder).to_string();

            let result = validate(&config);

            assert!(result.is_err(), "accepted placeholder: {placeholder}");
        }
    }

    #[test]
    fn validate_rejects_provider_placeholder_keys() {
        for placeholder in UPSTREAM_API_KEY_PLACEHOLDERS {
            for upstream in 0..4 {
                let mut config = valid_config();
                let target = match upstream {
                    0 => &mut config.upstreams.groq,
                    1 => &mut config.upstreams.openrouter,
                    2 => &mut config.upstreams.fal,
                    _ => &mut config.upstreams.tavily,
                };
                target.api_key = (*placeholder).to_string();

                let result = validate(&config);

                assert!(
                    result.is_err(),
                    "accepted placeholder {placeholder} for upstream {upstream}"
                );
            }
        }
    }

    #[test]
    fn only_a_real_service_key_counts_as_configured() {
        let upstream = |api_key: &str| UpstreamConfig {
            api_key: api_key.to_string(),
            base_url: "https://api.example".to_string(),
        };

        assert!(upstream("gsk_real").has_service_key());
        assert!(!upstream("").has_service_key());
        assert!(!upstream("   ").has_service_key());
        for placeholder in UPSTREAM_API_KEY_PLACEHOLDERS {
            assert!(
                !upstream(placeholder).has_service_key(),
                "{placeholder} counted as a service key"
            );
        }
    }

    #[test]
    fn validate_local_dev_skips_os_accounts_requirements() {
        let mut config = AppConfig::default();
        config.local_dev.enabled = true;
        config.upstreams.groq.api_key = "gsk-test".to_string();

        let result = validate(&config);

        assert!(result.is_ok());
    }

    #[test]
    fn validate_rejects_zero_model_catalog_refresh_interval() {
        let mut config = valid_config();
        config.server.model_catalog_refresh_secs = 0;

        let result = validate(&config);

        assert!(result.is_err());
    }

    #[test]
    fn validate_local_dev_allows_missing_provider_keys() {
        let mut config = AppConfig::default();
        config.local_dev.enabled = true;
        config.upstreams.groq.api_key = String::new();
        config.upstreams.openrouter.api_key = "OPENROUTER_API_KEY_REPLACE_ME".to_string();
        config.upstreams.fal.api_key = "FAL_API_KEY_REPLACE_ME".to_string();
        config.upstreams.tavily.api_key = String::new();

        let result = validate(&config);

        assert!(result.is_ok());
    }

    #[test]
    fn validate_local_dev_rejects_blank_bearer_token() {
        let mut config = AppConfig::default();
        config.local_dev.enabled = true;
        config.local_dev.bearer_token = "  ".to_string();

        let result = validate(&config);

        assert!(result.is_err());
    }

    #[test]
    fn validate_local_dev_rejects_default_bearer_token_on_non_loopback_host() {
        let mut config = AppConfig::default();
        config.local_dev.enabled = true;
        config.server.host = "0.0.0.0".to_string();

        let result = validate(&config);

        assert!(result.is_err());
    }

    #[test]
    fn loopback_hosts_cover_the_full_ipv4_range_and_ipv6() {
        for host in [
            "127.0.0.1",
            "127.0.0.2",
            "127.255.255.255",
            "localhost",
            "LOCALHOST",
            "::1",
            "[::1]",
        ] {
            assert!(is_loopback_host(host), "{host} should be loopback");
        }
        for host in [
            "0.0.0.0",
            "10.0.0.1",
            "192.168.1.1",
            "::",
            "::2",
            "example.com",
        ] {
            assert!(!is_loopback_host(host), "{host} should not be loopback");
        }
    }

    #[test]
    fn validate_local_dev_allows_placeholder_bearer_token_on_loopback_host() {
        // The publicly known placeholder is acceptable only while the bind
        // address keeps the API off the network: any 127.0.0.0/8 address or ::1.
        for host in ["127.0.0.1", "127.0.0.2", "::1"] {
            let mut config = AppConfig::default();
            config.local_dev.enabled = true;
            config.server.host = host.to_string();

            let result = validate(&config);

            assert!(result.is_ok(), "loopback host {host} rejected: {result:?}");
        }
    }

    #[test]
    fn validate_local_dev_rejects_placeholder_bearer_token_on_non_loopback_host_with_guidance() {
        // The fail-fast error must explain the risk and the fix, not just name
        // the field (security audit 2026-07-17).
        let mut config = AppConfig::default();
        config.local_dev.enabled = true;
        config.server.host = "0.0.0.0".to_string();

        let result = validate(&config);

        assert!(matches!(
            &result,
            Err(ConfigError::InvalidRequired {
                field: "local_dev.bearer_token",
                ..
            })
        ));
        if let Err(ConfigError::InvalidRequired { reason, .. }) = &result {
            assert!(
                reason.contains("openssl rand"),
                "error should explain how to set a random token: {reason}"
            );
        }
    }

    #[test]
    fn validate_local_dev_allows_random_bearer_token_on_non_loopback_host() {
        let mut config = AppConfig::default();
        config.local_dev.enabled = true;
        config.server.host = "0.0.0.0".to_string();
        config.local_dev.bearer_token =
            "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08".to_string();

        let result = validate(&config);

        assert!(
            result.is_ok(),
            "random token on non-loopback rejected: {result:?}"
        );
    }

    #[test]
    fn validate_local_dev_rejects_non_user_id() {
        let mut config = AppConfig::default();
        config.local_dev.enabled = true;
        config.local_dev.user_id = "local_dev".to_string();

        let result = validate(&config);

        assert!(result.is_err());
    }
}
