# Configuration reference

Clovy has two independent env surfaces: the **desktop app / client** (repo-root
`.env`, copied from `.env.example`) and **Clovy API**
(`clovy-api/.env` + `clovy-api/config.toml`). The client env must never hold a
server secret — service provider keys and the OS Accounts App API key live only
in Clovy API. A user's own Groq, OpenRouter, fal, or Tavily key is not env
configuration: it is saved from Settings into the Keychain (see
[User provider keys](#user-provider-keys)).

The fully-commented sources of truth are `.env.example`,
`clovy-api/.env.example`, and `clovy-api/config.toml`. This page summarizes them;
when they disagree, the files win.

## How Clovy API config resolves (Figment)

`AppConfig::default()` ← `config.toml` ← `CLOVY__SECTION__FIELD` env vars (a
double underscore separates sections; single underscores in field names stay
literal, so `BEARER_TOKEN` → `bearer_token`). `JUNE__*` variables are ignored.
Clovy API runs only in local mode: it authenticates one local bearer token,
never charges, and binds `127.0.0.1` by default.
Secrets are env-only and redacted in logs. Prices layer once more: built-in
fallback ← `config.toml` ← provider catalog data that Clovy API loads (remote
text prices come from the OpenRouter catalog; Groq ASR and fal media prices are
curated). `clovy-api/.env.example` and `config.toml` are authoritative.

## Desktop app / client (repo-root `.env`)

| Var | Purpose | Default |
|-----|---------|---------|
| `CLOVY_API_URL` | Clovy API base URL the app calls; share links use its origin | `http://127.0.0.1:8080` |
| `OS_CLOVY_LOCAL_DEV_BEARER_TOKEN` | The fixed local identity (there is no sign-in): bearer token sent to Clovy API | `local-dev-token` |
| `OS_CLOVY_DEV_PLAINTEXT_TOKEN_STORE` | Debug builds: store connector tokens in a file, not the Keychain (avoids prompts) | `0` |
| `GOOGLE_OAUTH_CLIENT_ID` / `_SECRET` | Google Desktop OAuth credential for Gmail/Calendar connectors. Google requires both at token exchange; the second value is not confidential in an installed app and grants no user-data access by itself | unset |
| `OS_NOTETAKER_TRANSCRIPTION_LANGUAGE` | Optional ISO-639-1 language hint | unset |

Dev-only toggles also read in code: `OS_CLOVY_ENABLE_DEV_SINGLE_INSTANCE`,
`OS_CLOVY_USE_PROD_ACCOUNTS_TOKENS`, `OS_CLOVY_USE_PROD_DATA_DIR`.

When set to `1`, `true`, `yes`, or `on`, `OS_CLOVY_USE_PROD_DATA_DIR` opts a
debug build into the production app data directory and the production
`provider-settings.json` location. Otherwise, app data and provider settings
use debug-only paths with the `-dev` suffix. Other files read directly from the
raw Tauri app config directory are unaffected.

June-era `OS_JUNE_*` variables remain lower-precedence compatibility inputs
through the bridge release.

## Clovy API (`clovy-api/.env`, `CLOVY__…`)

**Secrets — env only, never in `config.toml` or the client `.env`:**

| Var | Purpose |
|-----|---------|
| `CLOVY__UPSTREAMS__GROQ__API_KEY` | Groq service key: dictation and note transcription (Whisper) |
| `CLOVY__UPSTREAMS__OPENROUTER__API_KEY` | OpenRouter service key: remote text (chat, notes, dictation cleanup) |
| `CLOVY__UPSTREAMS__FAL__API_KEY` | fal service key: image generation and editing, video generation and animation |
| `CLOVY__UPSTREAMS__TAVILY__API_KEY` | Tavily service key: web search and page extraction |
| `CLOVY__LOCAL_DEV__BEARER_TOKEN` | The local bearer token; required to be non-placeholder on a non-loopback host |
| `CLOVY__COMPANION__DATABASE_URL` | Postgres trust-metadata store; required to enable the production companion relay |
| `CLOVY__COMPANION__APNS_PRIVATE_KEY_PEM` | Apple APNs `.p8` key; literal newlines or `\\n` escapes are accepted |

Non-secret (usually left to `config.toml`): `CLOVY__SERVER__HOST` / `PORT`,
`CLOVY__LOCAL_DEV__USER_ID`, `CLOVY__UPSTREAMS__*__BASE_URL`.

Companion APNs configuration also needs
`CLOVY__COMPANION__APNS_TEAM_ID`, `CLOVY__COMPANION__APNS_KEY_ID`,
`CLOVY__COMPANION__APNS_BUNDLE_ID`, and
`CLOVY__COMPANION__APNS_PRODUCTION`. If any APNs value is missing, relay and
foreground reconnect still work but background wake hints are disabled.

## Clovy API settings (`clovy-api/config.toml`)

- **Server:** `request_timeout_secs` 600, `max_audio_bytes` 25 MiB, `max_json_bytes` 512 KiB, `max_agent_chat_bytes` 12 MiB (dedicated `/v1/chat/completions` cap, aligned with the desktop client boundary and sized for a 1M-token context window; must be ≥ the 12 MiB client cap), `max_issue_report_bytes` 301 MiB total (one 300 MiB attachment plus multipart overhead), `max_image_edit_bytes` sized for a 50 MiB source image after base64 expansion.
- **Metering estimate:** `flat_estimate_credits` 250 — the flat credit Hold per metered action; skips per-request estimation. Local mode authorizes every Hold and never charges; the TTLs below still bound the inference client windows.
- **Hold TTLs (secs):** `note_transcribe` 60, `note_generate` 300, `dictate_transcribe` 30, `dictate_cleanup` 30, `web` 30, `image` defaults to `request_timeout_secs` + 30 (630) — validation rejects an image TTL that cannot outlive the request timeout, so a slow generation can still settle its charge.
- **Web tools:** `web_search_credits` 20, `web_fetch_credits` 20 (flat).
- **Media:** `image_pricing` (flat credits per generated image), `image_edit_pricing` (per edited image), and `video_pricing` / `video_animate_pricing` (markup on fal's quote). `fal-ai/flux-2/edit` is 96 credits, an estimate and not a proven account price: up to 2 MP in plus 2 MP out at fal's documented $0.012/MP is $0.048, with the existing margin of about 2x; fal's billing unit and rounding are not documented. A request naming a retired or unknown model is served by `default_image_model`, `default_image_edit_model`, `default_video_model`, or `default_video_animate_model` ([ADR-0021](adr/0021-june-api-v1-compatibility-policy.md)).
- **fal queue paths:** a run is submitted to its full endpoint (`{base}/fal-ai/wan/v2.2-a14b/text-to-video`), but its status and result are read under its app only (`{base}/fal-ai/wan/requests/{id}/status` and `.../requests/{id}`), as fal's own client does. Reading under the full endpoint gets 405 from the real queue: submits were accepted (and may be billed by fal) while image generation and edit failed and video jobs stayed `processing`. A fake for `upstreams.fal.base_url` has to answer the same way to catch this.
- **Image edit geometry:** an edit request carries no size. Clovy API keeps the oriented source's proportions. The image it sends holds at most 1024x1024 pixels, and the output size it asks fal for is explicit, 512 to 2048 per side within the same pixel budget. A source that fits and needs no orientation or canvas work is sent as is. A larger source is resized by Clovy API before anything reaches fal, as a whole image, keeping proportions and edges, never cropped or stretched; extreme ratios get neutral padding that is removed from the result. That resize costs Clovy API CPU and memory inside its blocking preparation work. The decoder keeps its default 512 MiB allocation limit (`max_alloc`), so an image that needs more is refused there with a useful error. Aborting the request does not interrupt a decode that is already running.
- **Image edit result size:** fal's edit must come back at the requested size, with one observed exception for `fal-ai/flux-2/edit`. Real runs asked for 1254x836 (a 1536x1024 source) answered 1280x864, the whole frame with each side rounded up to the next multiple of 32; fal documents no such rounding. For that endpoint only, Clovy API also accepts exactly that rounded size and scales the whole frame back to the requested size (as a PNG) before removing any canvas padding. A requested side already on that grid must come back exact, and any other size is still refused as `upstream_provider_failed`, after fal has run the edit.
- **Preview cap:** `note_transcribe_preview_max_audio_secs` 30.
- **Pricing:** one `[pricing."<model_id>"]` table per priced model (unit, credits, provider, model_type, capabilities, ...). A model with no pricing entry is rejected at the boundary (see [ADR-0007](adr/0007-model-capability-source-of-truth.md) and its addendum).
- **Issue reports:** logged by Clovy API only; `[issue_reports]` configures the optional server-side diagnosis model.

## User provider keys

Settings → More options lets a user save their own key for each provider Clovy
API calls on their behalf: Groq (transcription), OpenRouter (text models; Clovy
Auto never uses it), fal (images and video), and Tavily (web search). On save
the desktop checks the key against the provider's free, documented endpoint
(Groq `GET /openai/v1/models`, OpenRouter `GET /api/v1/key`, fal
`GET /v1/models/pricing`, Tavily `GET /usage`) and stores it only if accepted,
in the Keychain store the endpoints use (`crate::llm::secrets`), under the
account `builtin:<provider>`. `provider-settings.json` and the settings DTO
carry only `providerApiKeysConfigured` flags, never a key. A saved key is sent
only as that provider's private header (`x-groq-api-key`,
`x-openrouter-api-key`, `x-fal-api-key`, `x-tavily-api-key`) on the routes of
its capability. Video status polls carry the fal key too: Clovy API never
stores it, so a job created with it needs it on every poll and never falls
back to Clovy's key, while a Clovy-funded job ignores it. Any valid key from
the fal account that started the job works, so rotating the key within that
account is fine. A poll answered `fal_api_key_required`, `fal_api_key_rejected`,
or `fal_request_inaccessible` (fal could not find the job with the key sent,
possibly another account's key), or a poll that could not read the saved key,
leaves the job in place. The agent's `generate_video` tool then returns a
`waitingForProviderKey` result carrying the job id instead of failing the run;
after the user saves a valid key, calling the tool with only that `jobId`
continues the same job with no new submit. A poll answered 404 `job_not_found`
(past its lifetime, evicted, lost to a restart, never created, or another
user's) ends as `video_job_unavailable`, and nothing is recreated
automatically. A request carrying another provider's key, the retired
`x-venice-api-key` included, is rejected as `<provider>_api_key_not_accepted`
before any funding. Clovy API decides funding from the header it receives. A
key the provider revokes after it was saved is reported as rejected by the
request that uses it. When Clovy API has
no service key for a provider, its catalog rows say `serviceAvailable: false`
and requests without the user's key fail with `<provider>_not_configured`.
Model pickers leave out or flag such rows until
the key is saved; a missing key is a setting, not a macOS permission.

`GET /v1/models?type=image|video` lists the generation endpoints actually in
`image_pricing` / `video_pricing`, with the same DTO fields and the configured
fal service-key flag. The unfiltered, text and ASR responses are unchanged;
edit and animation endpoints keep their existing separate pricing. The native
`list_provider_models` command fetches all four modes. Media pickers retain
curated IDs, names, translations and defaults, merging only availability by
provider/type/ID. Missing rows/fields or a failed refresh mean **unknown**, with
a retry/add-key notice, not a false claim that the service is configured or
unconfigured. A saved fal key enables these rows independently of service
funding. Opening a picker refreshes it; saving/removing a key updates both
media pickers immediately and refreshes their catalogs. Generation/edit/video
actions still enforce funding at the API boundary and use the existing key
recovery flow; discovery does not authorize inference or change Auto funding.
An updated Clovy API deployment is required for this additive media discovery;
older APIs produce the explicit unknown state.

Provider-key changes hold the settings and published-view locks across the
secret operation and persistence. Settings are prepared on a candidate and
written via a temporary file in the same directory followed by atomic replace.
Only success publishes the new flags. If persistence fails, the previous secret
is restored through `SecretStore` (including its cache) before returning the
save error. If restoration also fails, the distinct
`provider_api_key_rollback_failed` error asks the user to save the previous key
again; it never masks that failure as a missing-key flag correction. These
semantics cover all four builtin routes and also avoid mutating ordinary
settings when a candidate is rejected or cannot be persisted.


Settings saved before the Groq / OpenRouter / fal cutover are migrated on
load: transcription becomes Groq Whisper, remote text moves to the same model
on OpenRouter when Clovy serves it (otherwise the default), and media models
move to the curated fal defaults, including profile overrides. CLI and
endpoint choices, endpoint keys, and stored notes, transcripts, and
conversations are not touched. The retired plaintext Venice key is dropped
from the file and never copied into the Keychain.

## Transcription context

Groq transcription (notes and dictation) sends Whisper a short spelling hint
as its `prompt`: whole custom dictionary terms, in dictionary order, joined by
`, `, within 200 UTF-8 bytes including the separators. A term that does not
fit is skipped, never cut, and later terms still get their chance; when no term
fits, no prompt is sent. 200 bytes is a conservative local envelope under
Groq's documented 224-token prompt limit, not a token count. The hint never
carries instructions, style, or earlier transcript text, and Clovy API
forwards it unchanged.

The full context goes to correction instead. Every Groq note transcript is
corrected on the dictation cleanup selection (Clovy API, an endpoint, or a
CLI) with style `note_transcript_cleanup`, which selects the note correction
instructions, and receives the whole dictionary plus the earlier turns of the
same source. Dictation sends the whole dictionary to its cleanup pass.


### Tool schemas and inline image limits

For tool guidance, typed `allOf` object branches merge `properties` recursively
and union `required` names in order without duplicates. Nonconflicting scalar
constraints survive; only actual competing keys use parent precedence (then
first-branch precedence). This is a constrained normalization for upstream
normalizers, not arbitrary JSON Schema intersection: incompatible types,
competing patterns/ranges and constraints such as `additionalProperties` are
not mathematically combined. Boolean/non-object branches and `allOf` without
a sibling type remain present. After branches merge, the resulting schema is
normalized again: an untyped branch refinement can now sit beside a type
inherited from the parent, including inside nested properties. This second
pass keeps the same precedence and unsupported-branch rules. The runtime
remains responsible for validating tool calls against its original contract.

fal inline image results enforce the same 50 MiB **decoded-byte** cap as CDN
results, in addition to the encoded-length guard. MIME comes from magic bytes,
not the data-URI declaration. A decoded over-limit result maps to
`image_too_large`; malformed base64/unsupported magic retain their existing
media-unavailable mapping.
