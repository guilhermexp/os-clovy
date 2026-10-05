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
| `llm/cli.rs` | Catalog of the six CLIs (`claude`, `codex`, `pi`, `agy`, `cursor-agent`, `copilot`), which of them can run with every tool off, one-shot argv/stdin/env for those, output parsing, schema strictifier |
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
`--json-schema`; pi, cursor-agent, copilot `prompt`); for the CLIs Clovy may
call (below) the test confirms it with the same discriminating probe (`none`
when the answer does not validate).
A level is stored only if the endpoint still has the URL and model that were
tested. Features that need JSON must refuse levels below `json_schema`.

## CLI isolation contract

A one-shot prompt carries untrusted text (meeting transcripts, notes,
dictation, coding-agent transcripts). Text in a prompt must never be able to
make the CLI read files, run commands, or reach the network, so **a one-shot
call only runs on a CLI that can be started with every tool off**
(`CliKind::tools_disabled`):

| CLI | One-shot calls | Why |
|---|---|---|
| claude | Yes | `--tools ""` turns off every built-in tool; `--strict-mcp-config` without `--mcp-config` and `--setting-sources ""` load no MCP server, plugin, or hook |
| pi | Yes | `--no-tools` (built-in and extension tools) and `--no-extensions` |
| codex | Refused | `exec` always offers shell, patch, MCP, and app tools (new versions keep adding more); `-s read-only` still lets it run commands and read files |
| agy | Refused | no flag turns its tools off |
| cursor-agent | Refused | print mode "has access to all tools"; `--mode ask` still reads |
| copilot | Refused | MCP servers from the user's configuration and plugins cannot all be excluded |

A refused CLI fails with `LlmError::CliToolsNotDisabled` (`llm_cli_tools_not_disabled`)
before anything is spawned, in `generate`, `generate_for_activity`,
`generate_on_cli_for_activity`, and the connection test. `llm_set_usage`
refuses it for notes, dictation cleanup, and activity (chat goes through the
[CLI chat engine](#cli-chat-engine), which keeps every CLI), and the Models tab
disables it in those menus and hides its test button. A selection saved before
this rule fails at use time with the same error; nothing is rerouted silently.

Every allowed one-shot call also:

- resolves the executable on the login-shell PATH (a GUI app does not inherit
  the shell PATH) and runs with the login-shell environment, so `HOME`,
  `PI_CODING_AGENT_DIR`, and other profile variables reach the CLI unchanged;
  Clovy's own variables (`OS_CLOVY_*`, `OS_JUNE_*`, `OS_ACCOUNTS_*`, `CLOVY_*`,
  `JUNE_*`, `GOOGLE_OAUTH_CLIENT_*`, such as the local development bearer token
  from `.env`) are removed by `llm::shell_env::cli_environment`, the one filter
  for one-shot calls, `--version` detection, the login-shell capture, and the
  CLI chat engine; Clovy never logs in or edits CLI configuration;
- starts with `CLOVY_AUTHORSHIP_MARKER` (`[clovy-internal-ai-call]`) alone on
  the first prompt line and sets `CLOVY_ONESHOT=1`; session ingestion skips
  any CLI session whose first prompt's first line is exactly the marker;
- runs in an empty scratch directory with sessions, context files, skills,
  and slash commands off (claude `--no-session-persistence --tools ""
  --setting-sources "" --strict-mcp-config --disable-slash-commands`; pi
  `--no-session --no-tools --no-extensions --no-context-files --no-skills
  --no-prompt-templates --no-approve`);
- strips `ANTHROPIC_API_KEY` for claude so a stray key never switches the user
  to metered billing;
- is killed with its whole process group on timeout (`LlmError::TimedOut`), and
  the group is swept right after the CLI exits, before output is collected, so
  a lingering helper cannot hold stdout open; stdout and stderr are each
  capped at 8 MiB.

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
  endpoint serving that model). A CLI chosen for chat makes new chat sessions
  start on that CLI: `list_venice_models("generation").selectedModel` returns
  its engine id (below). Picking a Clovy model in the text-model picker moves
  chat back to Clovy.
- **Notes.** `clovy_api::generate_note_from_transcript` uses the notes
  selection: Clovy API (with the saved remote model), the endpoint, or the CLI
  (same system prompt and source layout as the endpoint route; stored provider
  `cli:<id>`).
- **Dictation cleanup.** `clovy_api::cleanup_text` (dictation and note
  transcript cleanup) uses the dictation-cleanup selection with Clovy API's
  cleanup prompt and message layout.
- **Activity.** Defaults to none; Clovy API is not allowed.

## CLI chat engine

A chat session's **chat engine** is part of its model id
(`crate::chat_engine`): `__clovy_cli_engine__:<cli>` runs each message on that
CLI; a tagged endpoint id runs the Clovy agent on that endpoint (Clovy's tools,
streaming, and live steering, as on Clovy API); anything else is a Clovy model.
The engine picker writes `__june_local_generation__:<model>@<endpoint id>`
(both URL-encoded), so two endpoints serving the same model stay distinct and
the agent proxy uses exactly the chosen endpoint (a removed endpoint is
refused with `local_model_unavailable`, never replaced by another); the older
`__june_local_generation__:<model>` still routes by model id. The composer's
engine picker (`src/components/agent/composer/`) lists Clovy, the registered
endpoints, and the six CLIs (`chat_engine_catalog`).

One message on a CLI engine is one CLI process in the session workspace:

| CLI | Turn | Resume id | Clovy's tools |
|---|---|---|---|
| claude | `-p --output-format stream-json --verbose --include-partial-messages`, prompt on stdin | `session_id` of `system/init`; `--resume <id>` | `--mcp-config <temporary file>` |
| codex | `exec [resume] --json --skip-git-repo-check -c sandbox_mode="workspace-write" [<id>] -`, prompt on stdin | `thread_id` of `thread.started` | `-c mcp_servers.clovy.command=… -c mcp_servers.clovy.args=…` |
| pi | `--print --mode json --session-id <id>`, prompt on stdin | a UUID Clovy chooses on the first turn | none (pi has no MCP) |
| agy | `--print=<prompt> --output-format stream-json` | `conversation_id` of `init`; `--conversation <id>` | none (only `agy mcp add`, which edits its global configuration) |
| cursor-agent | `-p --output-format stream-json --stream-partial-output --trust -- <prompt>` | `session_id` of `system/init`; `--resume <id>` | none (project MCP servers need approval inside Cursor) |
| copilot | `--prompt=<prompt> --output-format json --session-id <id>` | a UUID Clovy chooses on the first turn | `--additional-mcp-config=@<temporary file>` |

- **Permissions.** No flag that skips or pre-grants a CLI's approvals is ever
  passed (`--dangerously-skip-permissions`, `--yolo`, `--force`,
  `--dangerously-bypass-approvals-and-sandbox`, `--always-approve`,
  `--allow-all*`, `--approve-mcps`, `--permission-mode`, `--allowedTools`).
  `--trust` (cursor-agent) and `--skip-git-repo-check` (codex) only accept the
  session workspace, a directory Clovy created, as a workspace. An action the
  CLI refuses for lack of approval becomes a failed tool item whose error says
  so (`needsApproval: true` in the event). Measured: claude answers the tool
  with "Claude requested permissions to …, but you haven't granted it yet";
  codex marks the command `declined` or the sandbox blocks it ("operation not
  permitted"); agy fails the step with "permission check failed … user denied
  permission"; cursor-agent completes the call as `rejected`; copilot fails
  the tool with a permission error.
- **Environment.** The login-shell environment, as for one-shot calls (PATH,
  `HOME`, `PI_CODING_AGENT_DIR`, and the user's other profile variables
  unchanged), minus Clovy's own variables (`OS_CLOVY_*`, `OS_JUNE_*`,
  `OS_ACCOUNTS_*`, `CLOVY_*`, `JUNE_*`, `GOOGLE_OAUTH_CLIENT_*`; for example the
  local development bearer token loaded from `.env`), which would otherwise
  reach the CLI, its tools, and its MCP servers (`llm::shell_env::cli_environment`,
  the same filter as one-shot calls).
  `ANTHROPIC_API_KEY` is removed for claude. No authorship marker: these are
  the user's own conversations.
- **Continuity.** The CLI's conversation id is saved in the run's config
  (`{"engine": "cli", "cli", "conversationId"}`) as soon as it is known, and
  the next message resumes it while the session's previous turns stayed on the
  same CLI. A CLI that starts a new conversation in a session with earlier
  messages (another engine answered before, or a branch) gets them, bounded
  to 24,000 characters, ahead of the new message.
- **Stream.** Text, reasoning, and tool activity become the runtime events the
  sidecar emits (`message.delta`, `reasoning.delta`, `tool.started`,
  `tool.completed`/`tool.failed`, `message.completed`, `run.completed`/
  `run.failed`/`run.cancelled`) and are persisted through the same function
  (`agent_runtime::host::persist_runtime_event`), so the session shows and
  keeps them like a Clovy run.
- **Cancel.** The run's cancellation is registered (`ChatEngineHost::register`)
  before anything is awaited for it, and the turn checks it and the run's
  status again right before spawning, so a cancel during launch preparation
  starts nothing. Cancelling sends SIGTERM to the CLI's process group, then
  SIGKILL after 2 s.
- **End of the CLI.** The CLI's exit is watched alongside its stdout: when it
  exits, its process group is swept (MCP servers and helpers it started,
  including one still holding stdout), and output still in the pipe is read
  for at most 1 s.
- **App shutdown.** The shutdown coordinator (`shutdown::run_cleanup`, also
  used for restart and update) calls `ChatEngineHost::shutdown`: new turns are
  refused, every turn is cancelled, and it waits up to 5 s for each to stop its
  process group and publish `run.cancelled`.
- **Messages during a run.** `steer_agent_run` answers
  `{ accepted: false, reason: "cli_engine" }`; the composer's follow-up queue
  sends the message as the next turn when the run ends.
- **Clovy's tools.** Given to claude, codex, and copilot while the Clovy MCP
  server is on ([mcp-server.md](mcp-server.md)); `chat_engine_catalog` reports
  `clovyTools` (`available`, `server_off`, `unsupported`) and the session says
  when it has none. Compaction is refused (`agent_compaction_unsupported`):
  the CLI manages its own context.

| Command | Args | Returns |
|---|---|---|
| `chat_engine_catalog` | none | `{ clis: [{ id, name, installed, reason?, modelId, clovyTools }], endpoints: [{ id, name, modelId, optionId }] }` |

## Tauri commands

| Command | Args | Returns |
|---|---|---|
| `llm_providers` | none | `{ endpoints, usage, cliLevels }` |
| `llm_detect_clis` | none | six `{ id, name, installed, path?, version?, reason?, structuredOutput, toolsDisabled }` |
| `llm_save_endpoint` | `request: { id?, name, baseUrl, modelId, apiKey?, clearApiKey? }` | registry DTO |
| `llm_delete_endpoint` | `id` | registry DTO (usages pointing at it reset) |
| `llm_set_usage` | `usage`, `provider` | registry DTO, or `llm_structured_output_insufficient`, `llm_provider_not_allowed`, `llm_cli_not_installed`, `llm_cli_tools_not_disabled`, `llm_endpoint_not_found` |
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
- `cargo test --manifest-path src-tauri/Cargo.toml --locked chat_engine` covers
  the CLI chat engine with a fake CLI per CLI (two turns with the resume id,
  stream to events and items, cancel without orphans, a message sent during a
  run, the MCP configuration each CLI receives, approval refusals, failures)
  and the endpoint as the agent's model against a fake streaming server with a
  tool call. `cargo test chat_engine_real -- --ignored --nocapture` runs two
  turns on the installed claude, codex, and pi.
- `pnpm exec vitest run src/test/llm-providers src/test/chat-engine` covers the
  settings section and the engine picker.
