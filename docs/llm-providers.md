# LLM providers (own providers)

Clovy can run text generation on the user's own providers instead of Clovy
API: the agent CLIs installed on the Mac and any number of named
OpenAI-compatible endpoints. The core lives in `src-tauri/src/llm/`; the
settings UI is `src/components/settings/LlmProvidersSection.tsx` (Settings,
Models). Transcription always stays on Clovy API (CLIs do not transcribe).

## Pieces

| Module | Role |
|---|---|
| `llm/mod.rs` | Public API: `generate`, `generate_for_activity`, `generate_on_cli_for_activity`, `provider_for`, `test_provider`, `run_cli`, `parse_json_value`, `StructuredOutputLevel`, `LlmError`, `CLOVY_AUTHORSHIP_MARKER` |
| `llm/cli.rs` | Catalog of the six CLIs (`claude`, `codex`, `pi`, `agy`, `cursor-agent`, `copilot`), one-shot argv/stdin/env per CLI, output parsing, schema strictifier |
| `llm/shell_env.rs` | Login-shell environment (`$SHELL -l -c 'env -0'`, captured once, merged over the app environment, plus fallback bin dirs) |
| `llm/detect.rs` | Installed / path / `--version` per CLI |
| `llm/process.rs` | Child runner: own process group, timeout kills the whole group, group swept after exit, 8 MiB output cap |
| `llm/endpoint.rs` | OpenAI-compatible `/chat/completions` one-shot calls, structured-output ladder probe, RPM pacing |
| `llm/registry.rs` | Persisted registry (endpoints, per-use selection, measured levels), legacy migration, validation |
| `llm/secrets.rs` | Keychain store for endpoint keys (`co.opensoftware.clovy.llm-providers`, `co.opensoftware.clovy-dev.llm-providers` in debug); in-memory under `cargo test` |
| `llm/commands.rs` | Tauri commands (below) |

## Generation API for feature code

```rust
use crate::llm::{self, registry::{LlmUsage, ProviderRef}, GenerateRequest};

// One call on an explicit provider (endpoint or CLI). Clovy/None -> LlmError::NotLocal.
let out = llm::generate(&provider, GenerateRequest {
    system: Some(instructions),
    prompt: user_text,
    schema: Some(json_schema),      // None for plain text
    timeout: None,                  // CLI default 180 s, endpoint default 120 s
}).await?;
out.text;      // raw answer
out.json;      // Some(object) when a schema was given; answers that do not
               // validate against the schema fail with LlmError::InvalidOutput
out.provider;  // "endpoint:<id>" or "cli:<id>"

// Background activity work (reports, day summary, agent-session summaries).
llm::generate_for_activity(request).await?;
```

`generate_for_activity` is the only entry point activity features may use:

- activity provider "none" (the default) returns `LlmError::ActivityProviderMissing`
  without any call, so no activity text leaves the machine and nothing falls
  back to Clovy API;
- a provider whose measured level is below `json_schema` (or never tested)
  returns `LlmError::StructuredOutputInsufficient`;
- calls are serialized by a process-wide semaphore (one at a time).

`generate_on_cli_for_activity(kind, request)` is the one exception: a
background call on a named CLI regardless of the activity selection, used to
summarize a coding-agent block with that agent's own CLI
([coding-agent-sessions.md](coding-agent-sessions.md)). It takes the same
semaphore and follows the CLI isolation contract below.

`llm::provider_for(LlmUsage::X)` returns the selection for chat, notes,
dictation cleanup, or activity.

## Structured output levels

`none < prompt < json_object < json_schema < strict`. Endpoints are measured by
the connection test: a plain prompt for latency, then the ladder strict
(`response_format.json_schema.strict = true`), json_schema (`strict = false`),
json_object, prompt; the first rung whose answer validates against the probe
schema (only `{"answer": "schema"}` is valid) wins. The strict and json_schema
rungs ask the prompt for a different object (`"color": "blue"`), so an
endpoint that ignores `response_format` follows the prompt, fails validation,
and is not credited with schema support. CLIs have a catalog level (codex
`strict` via `--output-schema`; claude and agy `json_schema` via
`--json-schema`; pi, cursor-agent, copilot `prompt`) that the test confirms
with the same discriminating probe (`none` when the answer does not validate).
A level is stored only if the endpoint still has the URL and model that were
tested. Features that need JSON must refuse levels below `json_schema`.

## CLI isolation contract

Every one-shot CLI call:

- resolves the executable on the login-shell PATH (a GUI app does not inherit
  the shell PATH) and runs with the login-shell environment, so `HOME`,
  `PI_CODING_AGENT_DIR`, and other profile variables reach the CLI unchanged;
  Clovy never logs in or edits CLI configuration;
- starts with `CLOVY_AUTHORSHIP_MARKER` (`[clovy-internal-ai-call]`) on the
  first prompt line and sets `CLOVY_ONESHOT=1`; session ingestion must skip any
  CLI session whose first user prompt contains the marker;
- runs in an empty scratch directory with sessions, tools, context files,
  skills, and slash commands disabled where the CLI allows it (claude
  `--no-session-persistence --tools "" --setting-sources "" --strict-mcp-config
  --disable-slash-commands`; codex `exec -s read-only --ephemeral`; pi
  `--no-session --no-tools --no-context-files --no-skills --no-prompt-templates
  --no-approve`; cursor-agent `--mode ask` in the scratch workspace; copilot
  `--no-custom-instructions --no-ask-user --disable-builtin-mcps`, every
  built-in tool in `--excluded-tools`, and `--deny-tool=shell,write,read,url,memory`,
  which wins over permissions the user saved for interactive use);
- strips `ANTHROPIC_API_KEY` for claude and `CURSOR_API_KEY` for cursor-agent so
  a stray key never switches the user to metered billing;
- is killed with its whole process group on timeout (`LlmError::TimedOut`), and
  the group is swept right after the CLI exits, before output is collected, so
  a lingering helper cannot hold stdout open; stdout, stderr, and codex's answer
  file are each capped at 8 MiB.

## Settings and routing

The registry is part of `provider-settings.json`: `llmEndpoints` (id, name,
`baseUrl`, `modelId`, `hasApiKey`, measured `structuredOutput`, optional
`rpmLimit`), `llmUsage` (`chat`, `notes`, `dictationCleanup`, `activity`, each
`{ "kind": "clovy" | "none" | "endpoint" | "cli", "id"? }`), `llmCliLevels`, and
`llmRegistryMigrated`. Endpoint keys are never in the file or any DTO.

- **Migration.** A legacy `localGeneration` endpoint becomes endpoint `local`
  ("Local model") with the same URL, model, and key (key moved to the
  Keychain). If the legacy local model was enabled, chat and notes select it.
  If the Keychain refuses the key, the legacy key stays in the file, keeps
  working, and the move is retried on the next launch. Saving a new key for
  `local` ends a pending migration and drops the legacy key, so the retry can
  never overwrite the new key.
- **Chat.** Chat on an endpoint is mirrored into the legacy
  `generationProvider = "local"` / `generationModel` fields, so the existing
  agent route, model picker, and capability lookups work unchanged. Agent
  requests resolve their endpoint by model id (chat endpoint first, then any
  endpoint serving that model). A CLI chosen for chat is saved, but the agent
  stays on Clovy API until the CLI chat engine ships; the UI says so. Picking a
  Clovy model in the text-model picker moves chat back to Clovy.
- **Notes.** `clovy_api::generate_note_from_transcript` uses the notes
  selection: Clovy API (with the saved remote model), the endpoint, or the CLI
  (same system prompt and source layout as the endpoint route; stored provider
  `cli:<id>`).
- **Dictation cleanup.** `clovy_api::cleanup_text` (dictation and note
  transcript cleanup) uses the dictation-cleanup selection with Clovy API's
  cleanup prompt and message layout.
- **Activity.** Defaults to none; Clovy API is not allowed.

## Tauri commands

| Command | Args | Returns |
|---|---|---|
| `llm_providers` | none | `{ endpoints, usage, cliLevels }` |
| `llm_detect_clis` | none | six `{ id, name, installed, path?, version?, reason?, structuredOutput }` |
| `llm_save_endpoint` | `request: { id?, name, baseUrl, modelId, apiKey?, clearApiKey? }` | registry DTO |
| `llm_delete_endpoint` | `id` | registry DTO (usages pointing at it reset) |
| `llm_set_usage` | `usage`, `provider` | registry DTO, or `llm_structured_output_insufficient`, `llm_provider_not_allowed`, `llm_cli_not_installed`, `llm_endpoint_not_found` |
| `llm_test_provider` | `provider` | `{ latencyMs, structuredOutput }` (level persisted) |

`probe_local_generation_endpoint` lists an endpoint's `/models` for the
endpoint form; with no typed key and an `endpointId`, the backend uses the
Keychain key saved for that endpoint (the key never reaches the webview).

## Verification

- `cargo test --manifest-path src-tauri/Cargo.toml --locked llm` covers the
  catalog, isolation flags and marker, login-shell resolution with fake shells
  and CLIs, timeouts without orphans, the endpoint probe against a fake HTTP
  server, migration, DTO secrecy, usage validation, activity gating and
  serialization, and a fake-CLI note persisted like a Clovy note.
- `pnpm exec vitest run src/test/llm-providers` covers the settings section.
