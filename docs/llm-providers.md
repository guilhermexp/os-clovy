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
| `llm/shell_env.rs` | Login-shell environment (`$SHELL -l -c 'env -0'`, captured once, merged over the app environment, plus fallback bin dirs); on Windows the app environment plus the installers' directories, with `PATHEXT` resolution |
| `llm/detect.rs` | Installed / path / `--version` per CLI |
| `llm/process.rs` | Child runner: own process group, timeout kills the whole group, group swept after exit, 8 MiB output cap |
| `llm/endpoint.rs` | OpenAI-compatible `/chat/completions` one-shot calls, structured-output ladder probe, RPM pacing |
| `llm/registry.rs` | Persisted registry (endpoints, per-use selection, measured levels), legacy migration, validation |
| `llm/secrets.rs` | Keychain store for endpoint keys (`dev.guilhermexp.clovy.llm-providers`, `dev.guilhermexp.clovy-dev.llm-providers` in debug); in-memory under `cargo test` |
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
dictation cleanup, or activity. The dictation-cleanup selection is always
Clovy API and nothing reads it (see below).

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
(`CliKind::tools_disabled`), **or, by the owner's decision (2026-10-23), on
codex and agy in their least-privileged mode** (`CliKind::one_shot_allowed`):

| CLI | One-shot calls | Why |
|---|---|---|
| claude | Yes | `--tools ""` turns off every built-in tool; `--strict-mcp-config` without `--mcp-config` and `--setting-sources ""` load no MCP server, plugin, or hook |
| pi | Yes | `--no-tools` (built-in and extension tools) and `--no-extensions` |
| codex | Yes, least privilege | `exec --sandbox read-only --ephemeral --skip-git-repo-check -c approval_policy="never" -c web_search="disabled" -c sandbox_workspace_write.network_access=false -c features.plugins=false -c features.apps=false -c features.hooks=false -c notify=[] --output-last-message last_message.txt [--output-schema schema.json] -c mcp_servers.<name>.enabled=false… -`, prompt on stdin: no writes, no network for commands, no approval prompt, no saved session; the answer is read from the last-message file in the scratch directory, under the 8 MiB capture cap (a larger answer is refused). Its built-in tools stay on (it can still read files), which the owner accepted. MCP servers, plugins, hooks and `notify` run outside its sandbox, so all are off: before the call, `codex -c features.plugins=false -c features.apps=false mcp list --json` lists the servers codex resolves from every config layer, and each is turned off by name. `--ignore-user-config` is not used because it also drops the user's `[windows] sandbox` and model settings. A list that fails or a name `-c` cannot address (outside `[A-Za-z0-9_-]`) fails the call before codex sees any content |
| agy | Yes, least privilege, without MCP servers or plugins | `--print <prompt> --output-format json --sandbox --disable-slash-commands [--json-schema <schema>]`: no flag turns its tools off; in print mode nobody can approve a permission request, so tool steps fail ("user denied permission"), and `--sandbox` restricts its terminal. agy has no flag to skip saving its conversation. Print mode starts every enabled MCP server (`~/.gemini/config/mcp_config.json`) and plugin (`~/.gemini/config/plugins/`) outside `--sandbox`, and agy 1.3 has no per-call switch to turn them off (only `agy mcp disable`, which edits its global configuration, or another `HOME`, which hides its login), so a one-shot call is refused while any is on, naming them. The prompt goes in argv: text print mode does not read stdin ("empty prompt"); on Windows the whole command line is measured in UTF-16 with its quoting and a call over 32,766 units fails before agy starts |
| cursor-agent | Refused | print mode "has access to all tools"; `--mode ask` still reads |
| copilot | Refused | MCP servers from the user's configuration and plugins cannot all be excluded |

cursor-agent and copilot stay refused. A refused CLI fails with `LlmError::CliToolsNotDisabled` (`llm_cli_tools_not_disabled`)
before anything is spawned, in `generate`, `generate_for_activity`,
`generate_on_cli_for_activity`, and the connection test. `llm_set_usage`
refuses it for notes and activity (chat goes through the
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
  CLI chat engine; Clovy never logs in or edits CLI configuration; on
  Windows see [Windows resolution](#windows-resolution);
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
- is killed with its whole process tree on timeout (`LlmError::TimedOut`), and
  the tree is swept right after the CLI exits, before output is collected, so
  a lingering helper cannot hold stdout open (Unix: the process group;
  Windows: a kill-on-close Job Object, `process_group::ProcessTree`); stdout
  and stderr are each capped at 8 MiB;
- on Windows starts without a console window (`hide_console`), like every
  process Clovy starts.

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
- **Default chat engine** ([ADR-0062](adr/0062-cli-engines-are-the-default-agent-native-harness-is-legacy.md)).
  Claude Code, else Codex, is the default chat engine; the Clovy agent is
  the legacy engine. `providers::ensure_cli_default` runs at launch and
  before `chat_default_engine` answers: chat on Clovy (the old default, new
  installs included) moves once to the first of claude, codex on the
  login-shell PATH (`llm::registry::apply_cli_default`), recorded as
  `llmCliDefaultApplied`. Any other chat choice is kept and recorded; an
  explicit chat choice later (`llm_set_usage`, picking a Clovy model) is
  recorded too and never moved. With neither CLI installed nothing is
  recorded, the next launch retries, and `llm_providers` reports
  `cliDefaultPending: true`, which Settings shows under Chat. The chat
  select lists the CLIs first and "Clovy (legacy agent)" last.
- **Chat.** Chat on an endpoint is mirrored into the legacy
  `generationProvider = "local"` / `generationModel` fields, so the existing
  agent route, model picker, and capability lookups work unchanged. Agent
  requests resolve their endpoint by model id (chat endpoint first, then any
  endpoint serving that model). A CLI chosen for chat makes new chat sessions
  start on that CLI: `list_provider_models("generation").selectedModel` returns
  its engine id (below). Picking a Clovy model in the text-model picker moves
  chat back to Clovy, whose remote text runs on OpenRouter through Clovy API
  (`generationProvider = "openrouter"`). An agent session that recorded a text
  model id from before the OpenRouter cutover is resolved to the same model's
  current id at the remote boundary; the session's runs and messages keep the
  id they recorded. An agent request that carries tools, `tool_choice`, or
  `response_format` reaches OpenRouter with `provider.require_parameters:
  true`, so a route that cannot honor them declines the request instead of
  running it without tools; the run then fails (the exact status OpenRouter
  returns in that case has not been observed yet).
- **Notes.** `clovy_api::generate_note_from_transcript` uses the notes
  selection: Clovy (with the saved remote model), the endpoint, or the CLI
  (same system prompt and source layout as the endpoint route; stored provider
  `cli:<id>`). On Clovy with the user's own OpenRouter key and an explicit
  model, the note is one chat completion straight to OpenRouter with Clovy
  API's `note_generate.md` prompt, source layout, and source-label cleanup
  (stored provider `openrouter`); Auto or no key goes through
  `/v1/notes/generate` (ADR-0070).
- **Note transcription.** `clovy_api::transcribe_saved_audio` (final turns, the
  per-source fallback, live preview) posts straight to Groq with the user's
  own Groq key, with the parameters Clovy API sends (`response_format=json`,
  canonical `audio.wav`/`audio.m4a` name, dictionary prompt, language, 25 MB
  cap, one retry of a 408/5xx or transport failure; an empty transcript is
  `no_speech`), and through `/v1/notes/transcribe` without one (ADR-0070).
  A Groq 429 carries its `retry-after` (else `x-ratelimit-reset-requests`) as
  `details.retryAfterMs`; `send_direct` waits it out only up to 2 s. Final
  transcription runs behind one process-wide pacer (`domain/processing.rs`
  `RequestPacer`): 18 requests a minute and 7,200 s of billed audio an hour
  (each request billed at least 10 s) across both Sources, every note and the
  live preview. A 429 holds every caller and repeats the same request; a wait
  over 90 s pauses the note with `transcription_rate_limited` and
  `details.resumeAtMs`, the note says when it continues, and `commands.rs`
  resumes it then while Clovy is open. With the key, consecutive speech
  turns of one Source go as turn packs of up to 600 s through
  `transcribe_saved_audio_timed` (`verbose_json`, segment times) and come
  back per turn. The live preview takes a request only when one is free and
  within 3,600 s of the hour's audio, otherwise it drops the chunk (ADR-0070
  addendum).
  Without the key and without a reachable Clovy API, the note names the
  missing key (`groq_api_key_missing`, `openrouter_api_key_missing`) instead of
  the HTTP error.
- **Dictation cleanup.** Never an own provider: `llm_set_usage` refuses a CLI,
  an endpoint, or none for `dictationCleanup` (`llm_provider_not_allowed`), and
  `normalize` moves a stored CLI or endpoint choice back to Clovy API, so the
  Models tab has no dictation cleanup row. Dictation cleanup is optional and
  off by default; when on, `clovy_api::dictate_cleanup` sends one OpenRouter
  chat completion per chunk on the model chosen in the dictation settings,
  straight to OpenRouter with the user's own key or through
  `/v1/dictate/cleanup` without it ([dictation.md](dictation.md), ADR-0065).
- **Note transcript cleanup.** `clovy_api::cleanup_text` sends style
  `note_transcript_cleanup` on the notes selection (the engine that writes the
  note: Clovy, the endpoint, or the CLI; on Clovy it goes straight to
  OpenRouter with the user's key, with Clovy API's `note_transcript_cleanup.md`
  prompt at temperature 0, else through `/v1/dictate/cleanup`); it selects the note correction prompt with the full
  dictionary and earlier same-source turns as context; the Whisper prompt
  itself only carries the short dictionary hint (see
  [configuration.md](configuration.md#transcription-context)).
- **Activity.** Defaults to none; Clovy API is not allowed.

## CLI chat engine

A chat session's **chat engine** is part of its model id
(`crate::chat_engine`): `__clovy_cli_engine__:<cli>` runs each message on that
CLI; a tagged endpoint id runs the Clovy agent on that endpoint (Clovy's tools,
streaming, and live steering, as on Clovy API); anything else is a Clovy model.
New work starts on the default engine: `chat_default_engine` returns
`{ modelId, cli, legacyFallback }` (`modelId` is the CLI engine id when chat
is on a CLI, else `null` and the caller keeps its own default) without the
network, and the desktop's new sessions, Home's turns
([home-assistant.md](home-assistant.md), ADR-0064), and note chat create their
session on it.
The engine picker writes `__june_local_generation__:<model>@<endpoint id>`
(both URL-encoded), so two endpoints serving the same model stay distinct and
the agent proxy uses exactly the chosen endpoint (a removed endpoint is
refused with `local_model_unavailable`, never replaced by another); the older
`__june_local_generation__:<model>` still routes by model id. The composer's
engine picker (`src/components/agent/composer/`, also in the note chat panel)
lists the six CLIs first (the default one, Claude Code or else Codex, marked
"Default"), then the registered endpoints, then Clovy under "Legacy"
(`chat_engine_catalog`). The Clovy model picker shows only on the Clovy
engine, and Clovy credits only gate the Clovy engine.

One message on a CLI engine is one CLI process in the session workspace:

| CLI | Turn | Resume id | Clovy's tools |
|---|---|---|---|
| claude | `-p --output-format stream-json --verbose --include-partial-messages`, prompt on stdin | `session_id` of `system/init`; `--resume <id>` | `--mcp-config <temporary file>` |
| codex | `exec [resume] --json --skip-git-repo-check [--image <path>]... -c <policy>... [<id>] -`, prompt on stdin | `thread_id` of `thread.started` | `-c mcp_servers.clovy.command=… -c mcp_servers.clovy.args=…` |
| pi | `--print --mode json --session-id <id>`, prompt on stdin | a UUID Clovy chooses on the first turn | none (pi has no MCP) |
| agy | `--print=<prompt> --output-format stream-json` | `conversation_id` of `init`; `--conversation <id>` | none (only `agy mcp add`, which edits its global configuration) |
| cursor-agent | `-p --output-format stream-json --stream-partial-output --trust -- <prompt>` | `session_id` of `system/init`; `--resume <id>` | none (project MCP servers need approval inside Cursor) |
| copilot | `--prompt=<prompt> --output-format json --session-id <id>` | a UUID Clovy chooses on the first turn | `--additional-mcp-config=@<temporary file>` |

On Windows a CLI resolved as a `.cmd` or `.bat` launcher (npm installs, the
Cursor agent installer) runs through `cmd.exe`, which refuses an argument
with a line break and a command line over 8191 characters. For such a
launcher cursor-agent gets its prompt on stdin instead of `-- <prompt>` (it
reads stdin when `-p` has no prompt), and copilot gets it on stdin instead of
`--prompt=<prompt>` (`chat_engine::cli::prompt_on_stdin`). agy keeps the
argument: its installer ships `agy.exe`.

- **Permissions.** The flags come from one table, `chat_engine::policy`, by
  CLI, the session's safety mode (`sandboxed` or `unrestricted`), and whether
  a person watches the turn (`attended`: chat `true`, routines `false`); the
  turn builder only places them. The rule is: never a flag that skips or
  pre-grants approvals (`--dangerously-skip-permissions`,
  `--allow-dangerously-skip-permissions`, `--permission-mode bypassPermissions`,
  `--dangerously-bypass-approvals-and-sandbox`, `danger-full-access`,
  `--approve-for-me`, `--yolo`, `--full-auto`, `--always-approve`,
  `--allow-all*`, `--approve-mcps`; `policy::FORBIDDEN`, swept over every
  combination by a test), and in an attended turn `--allowedTools` only for
  `mcp__clovy__*`:

  | Turn | claude | codex |
  |---|---|---|
  | attended, `sandboxed` | `--permission-mode default`; with Clovy's server `--allowedTools mcp__clovy__*` | `sandbox_mode="workspace-write"`, `sandbox_workspace_write.network_access=false`, `approval_policy="never"` |
  | attended, `unrestricted` | `--permission-mode acceptEdits` (edits in the workspace without asking; shell and the rest still ask), same `--allowedTools` | as `sandboxed` with `network_access=true` |
  | unattended (routines) | see below | see below |

  Every codex turn also gets `web_search="live"` and, with Clovy's server,
  `mcp_servers.clovy.default_tools_approval_mode="approve"` (codex `exec`
  fails an MCP tool without `readOnlyHint` otherwise; the approval broker
  inside each Clovy write tool is the authority, for that server only) and
  `mcp_servers.clovy.tool_timeout_sec=900`; claude gets `MCP_TOOL_TIMEOUT=900000`.
  Both outlast the broker's 10-minute wait (`approval_broker::APPROVAL_TIMEOUT`,
  checked at compile time). Image attachments reach codex as one `--image`
  per file, placed before the `-c` flags because `exec --image` takes every
  value that follows (a path with a comma stays in the manifest only); claude
  reads them from the manifest. The permission mode is always explicit, so a
  `defaultMode` in the user's own claude settings never decides it. Attended
  turns never get `--strict-mcp-config`: the MCP servers and claude.ai
  connectors the user configured in the CLI also load, and a server of the
  user's named `clovy` is replaced by the turn's. With Clovy's server, claude
  also gets `--permission-prompt-tool mcp__clovy__approve_action
  --disallowedTools mcp__clovy__approve_action AskUserQuestion`
  (`policy::CLAUDE_INTERACTION_TOOLS`): its own permission requests (shell,
  edits, reads outside the workspace) go to Clovy's `approve_action` tool and
  become Clovy approval cards through the approval broker, the model cannot
  call that tool itself, and it asks questions through Clovy's
  `request_clarification` instead of `AskUserQuestion`
  ([mcp-server.md](mcp-server.md#approval-broker)). Codex has no such hook
  in `exec`: its own actions stay inside its sandbox. On codex, and on claude
  without Clovy's server, an action the
  CLI refuses for lack of approval becomes a failed tool item whose error
  says so (`needsApproval: true` in the event). Measured: claude answers the
  tool with "Claude requested permissions to …, but you haven't granted it
  yet"; codex marks the command `declined` or the sandbox blocks it
  ("operation not permitted"); agy fails the step with "permission check
  failed … user denied permission"; cursor-agent completes the call as
  `rejected`; copilot fails the tool with a permission error. pi, agy,
  cursor-agent, and copilot get no policy flags (their default
  non-interactive mode). `--trust` (cursor-agent) and `--skip-git-repo-check`
  (codex) only accept the session workspace, a directory Clovy created, as a
  workspace.
- **Unattended turns** (`attended: false`, routines only). Nobody can
  approve anything, so the tools are confined instead: claude gets
  `--permission-mode default`, `--tools Read,Grep,Glob,WebSearch,WebFetch` (no
  other built-in exists in the turn),
  `--allowedTools Read,Grep,Glob,WebSearch,WebFetch,mcp__clovy__*` (the only
  `--allowedTools` beyond Clovy's tools: the read-only set itself), and
  `--strict-mcp-config` (Clovy's server only), plus, with Clovy's server,
  `--disallowedTools mcp__clovy__approve_action` (no permission prompt to
  answer, so the model never sees that tool); codex gets
  `sandbox_mode="read-only"`, `approval_policy="never"`, and the same web
  search and server settings as an attended turn. Clovy's write tools reach
  the broker, which refuses them at once (`unattended_requires_approval`).
  `chat_engine::check_policy` refuses an unattended turn on any other CLI
  (`agent_cli_unattended_unsupported`).
- **Turn binding.** When Clovy's server is in the turn, the turn issues a
  token in the server's turn registry (`mcp_server::turns::TurnRegistry`,
  binding: session, run, CLI, safety mode, attended) and the CLI's server
  arguments carry `--turn <token>` (claude and copilot in the temporary
  configuration file, codex in `mcp_servers.clovy.args`); the token never
  goes into the prompt or the environment. Connections opened with it are
  bound to the run and see the tools that need a turn. It is revoked when
  the turn ends, fails, or is cancelled (`chat_engine::turn::TurnGrant`), and
  on cancel before the process has stopped; a new handshake with it is then
  refused with `turn_expired`. A revoked connection reads no new request but
  still delivers the answers of the calls it was serving (at most 2 s) before
  it closes; a call still unanswered then gets `turn_expired` from the relay.
  App shutdown revokes every token after the turns stop
  (`shutdown::run_cleanup`). Each run gets a new token.
- **Note references.** A message that references notes (`@note:<id>`, which
  the note chat panel always puts first) reaches the CLI with a line asking it
  to read them with Clovy's `get_note` tool
  (`chat_engine::with_note_reference_context`); the stored user message is
  unchanged.
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
  starts nothing. `cancel_agent_run` (`chat_engine::cancel_cli_run`) first
  settles the run's pending approval cards as cancelled (the waiting tool
  answers `run_cancelled`), revokes the run's turn token, then sends SIGTERM to
  the CLI's process group, then SIGKILL after 2 s.
- **End of the CLI.** The CLI's exit is watched alongside its stdout: when it
  exits, its process group is swept (MCP servers and helpers it started,
  including one still holding stdout), and output still in the pipe is read
  for at most 1 s.
- **App shutdown.** The shutdown coordinator (`shutdown::run_cleanup`, also
  used for restart and update) calls `ChatEngineHost::shutdown`: new turns are
  refused, every turn is cancelled, and it waits up to 5 s for each to stop its
  process group and publish `run.cancelled`; each turn's token is revoked as
  its turn ends.
- **Messages during a run.** `steer_agent_run` answers
  `{ accepted: false, reason: "cli_engine" }`; the composer's follow-up queue
  sends the message as the next turn when the run ends.
- **Clovy's tools.** Given to claude, codex, and copilot while the Clovy MCP
  server is on ([mcp-server.md](mcp-server.md)); `chat_engine_catalog` reports
  `clovyTools` (`available`, `server_off`, `unsupported`) and the session says
  when it has none. A bound turn gets the read tools plus the turn-bound
  tools (notes, memory, routines, vault and skills, media, recorder,
  connectors, the MCP gateway, interaction, and, in attended turns, Browser
  use and Computer use); every Clovy write asks the approval broker first and
  an unattended turn's writes are refused (`unattended_requires_approval`),
  see [mcp-server.md](mcp-server.md#tools-for-turn-bound-connections). The
  agent's web search and fetch come from the CLI itself (claude's
  `WebSearch`/`WebFetch`, codex `web_search="live"`), not Clovy API. Compaction is refused (`agent_compaction_unsupported`):
  the CLI manages its own context.

## Routines on a CLI engine

A routine whose stored model is Auto runs on the chat engine when it is
Claude Code or Codex (`providers::default_unattended_cli`); the claim
snapshots `__clovy_cli_engine__:<cli>` as its model. `routines::start_claim`
writes the session, run, prompt, and claim mapping as for the Clovy agent
(`persist_claim_run`) and starts the turn with `chat_engine::start_cli_turn`
unattended (`attended: false`, with the routine's safety mode) under the
policy above. The prompt tells the CLI it runs
unattended and which tools it has. Its events go through the same persistence
as a chat turn, which projects the routine run's end, status, and error into
the routine history (`mark_agent_run_terminal`); the CLI's answer is the
session's assistant message. The routine's toolset choices apply only on the
Clovy agent: a routine with an explicit model, or with chat on anything else,
still runs there.

| Command | Args | Returns |
|---|---|---|
| `chat_default_engine` | none | `{ modelId: string \| null, cli: string \| null, legacyFallback: boolean }` |
| `chat_engine_catalog` | none | `{ clis: [{ id, name, installed, reason?, modelId, clovyTools }], endpoints: [{ id, name, modelId, optionId }] }` |

## Tauri commands

| Command | Args | Returns |
|---|---|---|
| `llm_providers` | none | `{ endpoints, usage, cliLevels, cliDefaultPending }` |
| `llm_detect_clis` | none | six `{ id, name, installed, path?, version?, reason?, structuredOutput, toolsDisabled }` |
| `llm_save_endpoint` | `request: { id?, name, baseUrl, modelId, apiKey?, clearApiKey? }` | registry DTO |
| `llm_delete_endpoint` | `id` | registry DTO (usages pointing at it reset) |
| `llm_set_usage` | `usage`, `provider` | registry DTO, or `llm_structured_output_insufficient`, `llm_provider_not_allowed`, `llm_cli_not_installed`, `llm_cli_tools_not_disabled`, `llm_endpoint_not_found` |
| `llm_test_provider` | `provider` | `{ latencyMs, structuredOutput }` (level persisted) |

`probe_local_generation_endpoint` lists an endpoint's `/models` for the
endpoint form; with no typed key and an `endpointId`, the backend uses the
Keychain key saved for that endpoint (the key never reaches the webview).

## Windows resolution

Windows has no login shell. `llm::shell_env` uses the app's own environment
(an app started from Explorer inherits the user's and the machine's `Path`),
captured once per run like the macOS login environment:

- the search path is the existing variable that matches `PATH` ignoring case
  (normally `Path`); fallback directories are written back to that same
  variable, so a child never sees two;
- `which` tries, in each directory, the name plus each `PATHEXT` extension in
  order (default `.COM;.EXE;.BAT;.CMD`), keeping only `.com`, `.exe`, `.bat`
  and `.cmd`, and never the bare name: npm writes a POSIX `codex` script
  beside `codex.cmd`;
- the fallback directories are where the official installers put each CLI:
  `%APPDATA%\npm` (npm global installs), `%USERPROFILE%\.local\bin` (claude),
  `%LOCALAPPDATA%\Programs\OpenAI\Codex\bin` and `%USERPROFILE%\.codex\bin`
  (codex standalone, current and earlier layout), `%LOCALAPPDATA%\agy\bin`
  (agy), `%LOCALAPPDATA%\cursor-agent` (cursor-agent),
  `%LOCALAPPDATA%\Microsoft\WinGet\Links` (copilot through winget),
  `%USERPROFILE%\.bun\bin` and `%LOCALAPPDATA%\Volta\bin`.

A CLI installed while the app is open appears after a restart, as on macOS.
One-shot calls always send the prompt on stdin, which `.cmd` launchers pass
through byte for byte; for chat turns see [CLI chat engine](#cli-chat-engine).

## Verification

- `cargo test --manifest-path src-tauri/Cargo.toml --locked llm` covers the
  catalog, isolation flags and marker, login-shell resolution with fake shells
  and CLIs, timeouts without orphans, the endpoint probe against a fake HTTP
  server, migration, DTO secrecy, usage validation, activity gating and
  serialization, and a fake-CLI note persisted like a Clovy note. On Windows
  it also covers the `Path` key, `PATHEXT`, the installers' directories, and
  a one-shot prompt through an npm `.cmd` shim.
- `cargo test --manifest-path src-tauri/Cargo.toml --locked chat_engine` covers
  the CLI chat engine with a fake CLI per CLI (two turns with the resume id,
  stream to events and items, cancel without orphans, a message sent during a
  run, the MCP configuration each CLI receives, approval refusals, failures;
  on Windows, multi-line and over-limit prompts reaching cursor-agent and
  copilot `.cmd` launchers intact),
  the permission table and the forbidden-flag sweep (`chat_engine::policy`),
  the turn token (present in each CLI's server arguments, bound to the run,
  revoked at the end) and a write tool approved, denied, and cancelled through
  the broker over a real socket and the relay code (`chat_engine::binding_tests`),
  and the endpoint as the agent's model against a fake streaming server with a
  tool call. `cargo test chat_engine_real -- --ignored --nocapture` runs two
  turns on the installed claude, codex, and pi, and the policy's argv on the
  installed claude and codex with the built `clovy-mcp` and an approval card
  (`chat_engine_real_policy_smoke`, after `cargo build --bin clovy-mcp`).
- The `windows-rust` CI job runs the `llm` and `chat_engine` filters on
  Windows, where the `Path`, `PATHEXT`, and `.cmd` tests compile.
- `pnpm exec vitest run src/test/llm-providers src/test/chat-engine` covers the
  settings section and the engine picker; `src/test/note-chat-default-engine`
  and the default-engine cases in
  `src/test/agent-workspace-runtime` and `src/test/app-shortcut` cover the
  entry points that start on the default engine.
- `cargo test --locked llm::registry` covers the one-time move to the default
  CLI; `cargo test --locked routines` covers a routine on a fake claude under
  the unattended policy, recorded in the routine history.
