# Coding-agent sessions

Clovy reads the local transcripts of the coding agents the user runs on the
Mac, cuts them into blocks, summarizes each block, and keeps the blocks in the
encrypted activity database, so the work done in a terminal or editor agent
(which screen capture barely sees) reaches the timeline and the day summary.
Terms are in [CONTEXT.md](../CONTEXT.md#activity-capture); the database is the
one described in [activity-capture.md](activity-capture.md).

## Where the code lives

| Path | Role |
|---|---|
| `src-tauri/src/coding_agents/mod.rs` | `SourceId`, Tauri commands, managed state, `setup` (starts the scan loop), `window_start` |
| `src-tauri/src/coding_agents/sources/` | One read-only reader per agent (`claude`, `codex`, `copilot_cli`, `copilot_vscode`, `cursor`, `cursor_cli`, `antigravity`), discovery and change fingerprints (`mod.rs`), SQLite snapshot reading (`sqlite.rs`) |
| `src-tauri/src/coding_agents/record.rs` | The normalized `Record` / `Session` every reader produces; the authorship-marker check |
| `src-tauri/src/coding_agents/segment.rs` | Block cutting and the transcript rendered for the summarizer |
| `src-tauri/src/coding_agents/ingest.rs` | `Scanner::scan` (one pass over the enabled sources) and `blocks_for` |
| `src-tauri/src/coding_agents/store.rs` | `coding_agent_blocks` queries (an `impl ActivityStore` block) |
| `src-tauri/src/coding_agents/summarize.rs` | Summarizer choice and the summary drain |
| `src-tauri/src/coding_agents/settings.rs` | `codingAgents` in `activity-settings.json` |
| `src/components/settings/CodingAgentSourcesSection.tsx` | Settings, Activity, "Coding agents": one switch per source |
| `src/components/coding-agents/CodingAgentSessionsStrip.tsx` | The coding-agent lane of the "Today" view |
| `src/lib/coding-agents.ts` | Typed bindings, DTOs, event |

Tests: `cargo test --manifest-path src-tauri/Cargo.toml --locked coding_agents`
(the spec scenarios end to end are in `coding_agents/red_tests.rs`) and
`pnpm exec vitest run --configLoader=runner src/test/coding-agents`.

## Lifecycle

`coding_agents::setup` (after `activity::setup`, macOS only) starts one async
loop. While at least one source is on it runs every 60 s, and right away after
the Activity settings are saved:

1. Turning any source on makes the capture engine open the activity database
   even when activity capture itself is off (`ActivitySettings::needs_store`).
   Until it is open the loop retries every 3 s; nothing is read meanwhile.
2. **Scan.** Live blocks whose last record is more than 1 hour old are sealed.
   Then, for each enabled source only, its store entries changed since the
   start of the window are listed; an entry whose size and mtime (and its
   SQLite WAL's) did not change since it was last loaded is skipped; the rest
   are loaded, Clovy's own CLI conversations are dropped, and every block that
   ended inside the window is upserted.
3. **Summaries.** Up to 4 sealed blocks of enabled sources are summarized per
   pass, newest first; with more due the loop comes back after 1 s.
4. **Retention**, hourly: blocks that ended before the activity retention
   period (`retentionDays`, default 30) are deleted.
5. When blocks or summaries changed, `clovy://coding-agents-updated` (no
   payload) is emitted.

The **window** is today and yesterday in local time (`BACKFILL_DAYS = 2`):
older history is never read, so turning a source on does not summarize weeks
of past sessions.

## Sources (read-only)

| Source | Store | Prompt vs reply | Working directory |
|---|---|---|---|
| Claude Code | `~/.claude/projects/<project>/<session>.jsonl` | `type: user` with text is a prompt; `type: user` with only `tool_result` blocks is a tool result; sidechain and meta records count only for time | `cwd` per record |
| Codex | `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl` | `event_msg` `item_completed` `UserMessage` / `AgentMessage` (current), or `user_message` / `agent_message` (earlier); `response_item` is skipped (injected context) | `session_meta` / `turn_context` `cwd` |
| Copilot CLI | `~/.copilot/session-state/<session>/events.jsonl` | `user.message` `data.content` (not `transformedContent`), `assistant.message`; `session.shutdown` / `session.end` is an exit | `session.start` `data.context.cwd` |
| Copilot in VS Code | `~/Library/Application Support/Code/User/workspaceStorage/<hash>/chatSessions/*.jsonl` (and `globalStorage/emptyWindowChatSessions/`) | Operation log replayed (`kind` 0 snapshot, 1 set, 2 append); `requests[].message.text`, response parts | `workspace.json` `folder` |
| Cursor | `~/Library/Application Support/Cursor/User/globalStorage/state.vscdb`, table `cursorDiskKV` | `composerData:<id>` gives the turn order, `bubbleId:<id>:<bubble>` type 1 / 2 | none |
| Cursor CLI | `~/.cursor/chats/<workspace-md5>/<chat>/store.db` | Root blob lists message blobs; user text is the `<user_query>` part | none |
| Antigravity | `~/.gemini/antigravity-cli/brain/<conversation>/.system_generated/logs/transcript.jsonl` (editor: `~/.gemini/antigravity/brain/`) | `USER_EXPLICIT` `USER_INPUT` (inside `<USER_REQUEST>`), `MODEL` `PLANNER_RESPONSE`, other `MODEL` steps are tool output | none |

Read-only rules:

- Files are opened for reading only. JSONL readers skip malformed and
  partially written lines (the next scan sees them complete).
- SQLite stores (Cursor, Cursor CLI) are never opened in place: opening a live
  WAL database, even read-only, can make SQLite create or write `-wal`/`-shm`
  next to it, and `immutable=1` would miss rows still in the WAL. The file and
  its WAL are copied to a private temporary directory and the copy is read.
- A disabled source's directories are not listed or read at all.
- Cursor CLI messages carry no timestamps: every record gets the chat's
  `createdAt` except the last, which gets the store's mtime.

**Self-ingestion.** Every CLI call Clovy makes starts its prompt with
`CLOVY_AUTHORSHIP_MARKER` (`[clovy-internal-ai-call]`, see
[llm-providers.md](llm-providers.md)). A session whose first prompt contains
it is skipped entirely (`Session::is_clovy_call`). Claude and Codex calls are
already not persisted (`--no-session-persistence`, `--ephemeral`); Cursor CLI
and others are caught by the marker.

## Blocks

`segment::segment` cuts one session. A new block starts when more than 1 hour
passed since the previous timestamped record, when the open block is at least
1 hour long and the record is a user prompt (so a block ends on a complete
agent turn and the next starts on what the user asked), or after an explicit
exit. Untimed turns join the open block; blocks without turns are dropped.
A long autonomous stretch stays in its block until the next prompt or a
1-hour silence. `active_seconds` sums the gaps between records, each capped at
2 minutes. Cutting depends only on earlier records, so appending to a
transcript never moves an existing block's start (its identity).

**Lifecycle**: every block but the session's last is written `sealed`; the
last stays `live` (rewritten on each scan) until the agent exits or it is idle
for more than 1 hour. Sealed blocks never change (the upsert only updates
`live` rows). A summary moves a block to `summarized`.

## Summaries

| Agent | Summarized by (when installed) |
|---|---|
| Claude Code | `claude` |
| Codex | `codex` |
| Copilot CLI, Copilot in VS Code | `copilot` |
| Cursor, Cursor CLI | `cursor-agent` |
| Antigravity | `agy` |

When the agent's CLI is not installed, the activity provider chosen in
Settings, Models summarizes (`llm::generate_for_activity`, which requires a
provider with JSON schema support). With neither, the block is not read for
summarizing, no call is made, no attempt is counted, and it stays `sealed`
until one becomes available. Own-CLI calls go through
`llm::generate_on_cli_for_activity`, which shares the activity semaphore (one
activity call at a time) and the CLI isolation contract.

The prompt asks for 1 to 3 plain sentences in the interface language (English
or Brazilian Portuguese) with the agent, project, session title, UTC time
range, and the block transcript (turns with their times; tool inputs capped at
400 and tool results at 800 characters; the whole transcript capped at 100,000
characters, keeping the first 70% and the last 30%). Answers are trimmed,
unfenced, and capped at 1,200 characters. A failed attempt is retried after
30 minutes times the attempt number; after 3 attempts the block stays sealed
with `summaryError`.

## Database (migration 2, `coding_agent_blocks`)

| Column | Notes |
|---|---|
| `id` | AUTOINCREMENT |
| `source` | `claude_code`, `codex`, `copilot_cli`, `copilot_vscode`, `cursor`, `cursor_cli`, `antigravity` |
| `session_id` | The agent's session id (file stem, `session_meta.id`, composer id, chat id, conversation id) |
| `started_at`, `ended_at` | RFC 3339 UTC with microseconds; `UNIQUE (source, session_id, started_at)` is the block identity |
| `cwd`, `project` | Working directory and its last component, when the agent records one |
| `title` | The agent's session title, when it has one |
| `first_prompt` | First user prompt of the block, capped at 200 characters |
| `prompt_count`, `reply_count`, `active_seconds` | Counts and capped active time |
| `transcript` | Rendered block text for the summarizer (never in a DTO) |
| `state` | `live`, `sealed`, `summarized` (`CHECK`: sealed and summarized have `sealed_at`; summarized has `summary`) |
| `sealed_at`, `summary`, `summary_source` (`cli:<id>`, `endpoint:<id>`), `summarized_at` | |
| `summary_attempts`, `summary_error`, `next_attempt_at` | Retry state |
| `updated_at` | Last write |

### Rust API for later slices (`crate::activity::store::ActivityStore`)

| Method | Use |
|---|---|
| `coding_agent_blocks_between(from, to)` | Blocks overlapping `[from, to)`, oldest first, as `CodingAgentBlock` (every column except `transcript`). The day summary, hour reports, and MCP tools read this |
| `upsert_coding_agent_block`, `seal_idle_coding_agent_blocks`, `coding_agent_blocks_to_summarize`, `record_coding_agent_summary`, `record_coding_agent_summary_failure`, `prune_coding_agent_blocks` | Owned by the scan loop |

Get the store from `ActivityState::shared()` → `ActivityShared::store()`.

## Settings, commands, event

`activity-settings.json` gains `codingAgents`: `{ claudeCode, codex,
copilotCli, copilotVscode, cursor, cursorCli, antigravity }`, all `false` by
default; older files load with every source off. They are saved with the rest
of the activity settings (`activity_save_settings`), which also wakes the scan.

| Command | Request | Returns |
|---|---|---|
| `coding_agents_status` | none | `{ supported, sources: [{ id, name, enabled, present }], databaseReady, lastScanAt, lastError }` |
| `coding_agents_blocks` | `{ request: { from, to } }` (ISO times) | `CodingAgentBlockDto[]` (empty while the database is not open) |
| `activity_debug_export` | none | adds `codingAgentBlocks` (count); the JSON file has a `codingAgentBlocks` array |

Event: `clovy://coding-agents-updated` (no payload).

## UI

- **Settings, Activity, "Coding agents"**: one switch per source with "Found on
  this Mac" / "Not found on this Mac", how summaries are made, and the last
  read time (or why the database is unavailable).
- **"Today" view lane**: `CodingAgentSessionsStrip` takes the local day to
  show (`<CodingAgentSessionsStrip day={selectedDay} />`), loads that day's
  blocks itself, and reloads on `clovy://coding-agents-updated`. Each block
  shows agent, project, time range, prompt count, and the summary (or "In
  progress" / "Waiting for a summary" with the first prompt). The "Today" view
  mounts it as its own lane next to the activity blocks.

## Verification

- Unit and integration: readers per agent with synthetic fixtures (including
  real SQLite stores for Cursor and Cursor CLI), segmenter, store lifecycle,
  summarizer choice and drain with a fake backend, and the spec scenarios over
  a temporary home (a disabled source is never read, transcripts keep hash and
  mtime and nothing is created next to them, 2 h 30 min becomes three blocks
  cut at prompts, a conversation started by Clovy's own summary prompt is
  skipped).
- Component: `src/test/coding-agents.test.tsx` (settings section and lane).
