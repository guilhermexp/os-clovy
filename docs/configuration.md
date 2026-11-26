# Configuration reference

The desktop `.env` (copied from `.env.example`) holds build and connector
configuration. Provider keys are saved through Settings > Models > More
options > Provider API keys, in the operating system credential store. There
is no Clovy inference server, server config file, service-key fallback, or
local bearer token. See
[ADR-0073](adr/0073-desktop-calls-user-providers-without-a-clovy-server.md).

## Desktop configuration

| Variable | Purpose | Default |
|---|---|---|
| `OS_CLOVY_DEV_PLAINTEXT_TOKEN_STORE` | Debug connector-token storage for isolated fixtures | `0` |
| `GOOGLE_OAUTH_CLIENT_ID` / `_SECRET` | Google Desktop OAuth client credentials; user grants remain in the credential store | unset |
| `OS_NOTETAKER_TRANSCRIPTION_LANGUAGE` | Optional ISO-639-1 transcription language hint | unset |
| `CLOVY_DEV_APP_IDENTIFIER` | Run-owned compiled debug identity, bundle and data directories | default development identity |

Debug/release paths must use `app_paths::app_data_dir` and
`app_paths::app_config_dir`. Never enable `OS_CLOVY_USE_PROD_DATA_DIR` in tests
or verification. See [development.md](development.md) for isolated native proofs
and the signed dev launcher. The current `.env.example` and code are the
source of truth for supported variables.

On macOS, Settings > General > Hide from Dock controls the local
`app-preferences.json` preference `hideFromDock`, off by default. When enabled,
the app switches to accessory activation only while the main window is hidden;
it restores regular activation before revealing the window. The menu-bar app
continues running. Other platforms do not expose this setting.

## Provider keys and routes

| Capability | Provider and credential |
|---|---|
| Dictation, saved-note transcription, live preview | Local, or Groq or OpenRouter with the corresponding user key |
| Remote note generation and transcript cleanup | OpenRouter, with the user's OpenRouter key |
| Optional dictation cleanup | OpenRouter only, with the user's OpenRouter key |
| Routines | Installed CLI and its own login |
| CLI/endpoint note generation | Selected engine through `llm::generate` |

The desktop checks saved provider keys against the provider's own key-check
endpoint. Keys are held by `llm::secrets` under `builtin:<provider>`;
`provider-settings.json` and DTOs contain configured flags, never keys.
A missing Groq or OpenRouter key fails before a request. Unreadable saved
credentials report a credential-store error. There is no switch to another
provider or shared account. The CLI supplies its own web search; the desktop
has no separate web-search service.

Provider-key changes hold settings/published-view locks across the secret
operation and persistence. Candidate settings are written through a same-directory
temporary file and atomic replace. Only success publishes the new flags. A
persistence failure restores the previous secret and cache; a failed restore
reports `provider_api_key_rollback_failed` and asks the user to save the old key
again. These guarantees also apply when removing a key.

## Text catalog and engine selection

The text-model picker reads `https://openrouter.ai/api/v1/models`, optionally
with the saved OpenRouter key. It includes every model whose output modalities
contain text, including models without tools. Mapping preserves ID, name,
description, context length, input vision support, supported parameters, and
USD token prices. No unknown privacy tier is invented.

`providers/catalog.rs` uses a 10-second total deadline, including cache-lock
wait, an 8 MiB response cap, and a 10-minute memory cache. Saving/removing the
key invalidates that cache and reloads the picker. Failed loads show an error
and retry action. Catalog access is discovery, not permission to run inference.

Changing the selected text model updates the model for remote note generation.
The picker description names the independently selected Notes engine. Notes
support OpenRouter, CLI or an explicit OpenAI-compatible endpoint.
Transcription pickers use local curated lists. Remote transcription requires
the selected provider's key.

A one-time persisted migration changes old server-based note selections to
OpenRouter and the old Auto model to the generation default. CLI/endpoint
note choices, endpoint keys and saved history are preserved. Old native or
endpoint chat selections become the CLI default (ADR-0072). SQL migrations
remain append-only.

## Transcription context

Groq receives a short spelling hint in `prompt`: whole custom dictionary
terms, in order, joined by `, ` within 200 UTF-8 bytes. A term that does not fit
is skipped without truncation. The hint contains no instructions, style or
previous transcript. The full dictionary and earlier same-source turns go to
note transcript cleanup, using the selected Notes engine and the prompt in
`src-tauri/src/prompts/note_transcript_cleanup.md`.

Dictation cleanup is separate: optional, off by default, and always OpenRouter.
When it cannot run, dictation delivers the raw transcript with an explicit HUD
notice. See [dictation.md](dictation.md) and [llm-providers.md](llm-providers.md).
