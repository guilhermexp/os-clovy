# Day intelligence

Turns the [activity timeline](activity-timeline.md) into hour reports, the
day's workstreams, and a day summary with a standup, written by the user's
**activity provider** ([llm-providers.md](llm-providers.md)), delivered on
schedule and announced by notifications. Terms are in
[CONTEXT.md](../CONTEXT.md#day-intelligence). Everything lives in the
encrypted `activity.sqlite3`.

## Where the code lives

| Path | Role |
|---|---|
| `src-tauri/src/day_intelligence/distill.rs` | Local distillation of an hour's text (no network) |
| `src-tauri/src/day_intelligence/embedder.rs` | On-device BGE small embeddings (candle), one-time pinned download |
| `src-tauri/src/day_intelligence/hour.rs` | Hour report prompt, answer parsing, minute normalization |
| `src-tauri/src/day_intelligence/workstreams.rs` | Anchored incremental fold of an hour into the day's workstreams |
| `src-tauri/src/day_intelligence/summary.rs` | Day summary prompt and answer, panels, DTOs |
| `src-tauri/src/day_intelligence/prompts.rs` | System prompts, language directive, JSON schemas |
| `src-tauri/src/day_intelligence/schedule.rs` | Pure time decisions: local hours, completed hours, daily run, retries, quiet hours |
| `src-tauri/src/day_intelligence/notify.rs` | Notice queue, quiet hours, copy |
| `src-tauri/src/day_intelligence/sources.rs` | Meetings and notes (main database), coding-agent blocks (integration point) |
| `src-tauri/src/day_intelligence/db.rs` | Tables and the read API |
| `src-tauri/src/day_intelligence/pipeline.rs` | Orchestration over injected dependencies |
| `src-tauri/src/day_intelligence/mod.rs` | Tauri state, background loop, commands |
| `src-tauri/src/notifications.rs` | Native activity notifications (click opens "Today", snooze button) |
| `src/components/activity-timeline/DaySummaryPanel.tsx`, `src/lib/day-intelligence.ts` | The "Day summary" section of the "Today" view and its bindings |
| `src/components/settings/DaySummarySettingsSection.tsx` | Settings, Activity: summary time, notifications, quiet hours |

Tests: `cargo test --manifest-path src-tauri/Cargo.toml day_intelligence`
(real store, fake provider) and
`pnpm exec vitest run --configLoader=runner src/test/day-intelligence`. The real
embedding model test is ignored by default:
`CLOVY_TEST_EMBEDDER_MODELS=<dir with bge-small-en-v1.5> cargo test day_intelligence::embedder -- --ignored`.

## Gating and privacy

- Inactive without an activity provider with JSON schema support (`provider`
  `missing` / `insufficient` in the DTO): no call, no model download. Every
  call goes through `llm::generate_for_activity` (activity selection only,
  never Clovy API, one call at a time).
- Inactive while capture is off. Current exclusions apply: excluded sessions
  and documents with an ignored domain never reach a prompt.
- Distillation is local. The only network access of this module is the
  one-time download of the embedding model into `<app data dir>/models/`
  (`BAAI/bge-small-en-v1.5` at revision `5c38ec7c405ec4b44b94cc5a9bb96e735b38267a`,
  every file verified by SHA-256). Until it is present, or if it fails, the
  distiller runs its lexical stages only.
- Notes and meetings of the day (title, time, a 400-character excerpt of the
  generated note) go to the activity provider with the day summary.

## Pipeline

The background loop (`day_intelligence::setup`, after `activity::setup`)
wakes every minute, on a capture failure, and when a notice is due:

1. **Completed hours.** For today and yesterday, every local hour that ended
   and that the timeline has built (`built_until`: now when the ETL caught up
   with capture, else the cursor's last frame) and has no report gets one.
   Hours without session time are recorded `empty` and skipped.
2. **Distillation** (`distill.rs`, Meridian's pipeline): segment lines over 140
   characters, drop junk (under 18 characters, spinners, low letter ratio) and
   lines without function words (English and Portuguese stop words; code
   passes), cut lines seen in at least max(3, 25%) of the hour's sessions unless
   they name an entity, dedupe by normalized 80-character prefix, then
   semantically (cosine above 0.86, except an entity line in another session),
   pick 3 to 14 diverse lines per session (facility location), and rescue up to
   4 dropped lines per session whose entities (ticket keys, PR numbers, file
   paths, commit hashes) are not covered. Sessions under 15 s are flicker.
3. **Hour report**: one call with the measured sessions, coding-agent blocks,
   meetings, and the distilled text. The answer's minutes are weights: the
   hour's measured active minutes (focused time from the timeline, rounded)
   are split by largest remainder, so 42 measured minutes with estimates 30
   and 30 show 21 and 21.
4. **Workstream fold**: one call with the day's workstreams as anchors (id,
   title, summary) and the hour's numbered activities; the answer places this
   hour only. A placement on an existing id appends the hour and may refresh
   that workstream's summary; id 0 opens one; activities left out open one
   workstream named after the first of them. Titles and untouched workstreams
   never change. Hours fold in order; a fold that failed its 4 attempts lands
   as one new workstream without a call.
5. **Day summary**: at the configured time (default 18:00), once per day: due
   when the local clock is past the time and no automatic run of the day
   finished, so a Mac asleep at 18:00 generates it at the first tick after
   waking. On demand ("Generate summary") it reports the day's pending hours
   (retrying failed ones now) and regenerates. Text follows the interface
   language (`interface_locale::current()`); JSON keys stay English. A day with
   no reports, meetings, or blocks has no summary and no call.

Failures are recorded per unit (`hour:<hour>`, `fold:<hour>`, `auto:<day>`)
and retried after 5, 10, 20 minutes; 4 attempts in total (a manual run retries
anyway).

## Notifications

`notify.rs`: "Your day summary is ready" (body: the headline), "The day
summary could not be generated" (first failure of the scheduled run), and
"Activity capture stopped" (capture entered `error` or `keyMissing`, once per
distinct failure). The Settings, Activity switch drops them all; inside quiet
hours they wait for the end (22:00 to 08:00 and ready at 23:00 → shown at
08:00). Pending notices are in memory, one per kind and day.

Delivery (`notifications::send_today_notification`) uses Clovy's own
`NSUserNotification` path on macOS: `userInfo` carries `clovyTodayDay` and
`clovyTodayKind`. A click focuses the window and emits `clovy:today:open`
(`{ day }`), queued until the webview calls `today_open_ready` (same handshake
as agent notifications); the app switches to "Today", selects the day, and
opens the "Day summary" section. The notification's action button "Snooze 1h"
posts it again an hour later; macOS shows it only when Clovy's alert style is
"Alerts" (with "Banners" only the click works). `tauri-plugin-notification`
has no action or click support on macOS, so unbundled dev runs fall back to
it without click-through.

## Schema (migration 3, `day_intelligence`)

Days are local `YYYY-MM-DD`, hours local `YYYY-MM-DDTHH`; `started_at` /
`ended_at` are UTC (RFC 3339, microseconds).

| Table | Columns | Notes |
|---|---|---|
| `day_hour_reports` | `hour` (PK), `day`, `started_at`, `ended_at`, `active_minutes`, `summary`, `activities_json` (`[{description, minutes}]`, Clovy's minutes), `distilled`, `distill_json` (stats), `locale`, `provider`, `generated_at`, `folded_at` | One per completed hour with activity |
| `day_workstreams` | `id`, `day`, `title`, `summary`, `first_hour`, `last_hour`, `created_at`, `updated_at` | Title never changes |
| `day_workstream_hours` | `workstream_id` → workstreams (cascade), `hour`, `minutes`, `note` | PK `(workstream_id, hour)`; minutes are the sum |
| `day_summaries` | `day` (PK), `headline`, `narrative`, `insights_json`, `standup_json` (`{done, in_progress, blockers}`), `hours_covered`, `locale`, `provider`, `trigger` (`scheduled`/`manual`), `generated_at` | Regenerating replaces the row |
| `day_intelligence_runs` | `key` (PK), `attempts`, `outcome` (`ok`/`failed`/`empty`), `last_error`, `next_attempt_at`, `updated_at` | Scheduler bookkeeping |

Retention follows capture's period: rows of days before it are deleted hourly.
The debug export adds `dayHourReports`, `dayWorkstreams`, `daySummaries`, and
`dayIntelligenceRuns` (newest three days).

## Read API (Rust, for the MCP server and the chat engine)

In `crate::day_intelligence::db`, over an open `ActivityStore`:

| Function | Returns |
|---|---|
| `summary_of_day(store, "YYYY-MM-DD")` | `Option<DaySummaryDto>`: headline, narrative, insights, standup `{done, inProgress, blockers}`, `hoursCovered`, `locale`, `provider`, `trigger`, `generatedAt` |
| `workstreams_of_day(store, day)` | `Vec<WorkstreamDto>`: id, title, summary, minutes, hours `[{hour, minutes, note}]` |
| `hour_reports_of_day(store, day)` | `Vec<HourReportDto>`: hour, bounds, active minutes, summary, activities with minutes |

`pipeline::day_view(&ReadContext, day)` returns those plus `DayPanelsDto`
(numbers read now from the timeline, workstreams, meetings, and blocks). A
`get_day_summary` tool should return `summary_of_day` with the panels, not
numbers from the summary text.

## Commands and event (frontend contract)

| Command | Request | Returns |
|---|---|---|
| `day_intelligence_day` | `{ request: { day } }` | `{ availability, day, provider: "ready" \| "missing" \| "insufficient", embedder: "absent" \| "downloading" \| "ready" \| "failed", running, summaryTime, summary, workstreams, hourReports, panels }` |
| `day_intelligence_generate` | `{ request: { day } }` | Same DTO; errors `llm_activity_provider_missing`, `llm_structured_output_insufficient`, `activity_database_closed`, `day_summary_no_activity`, provider errors (`llm_timeout`, ...) |
| `today_open_ready` | none | Day of a notification clicked before the webview listened, or null |

`panels`: `focusedMs`, `idleMs`, `awayMs`, `categories`, `topApps` (the day's
timeline stats), `hours` (24 local hours with focused ms; they add up to
`focusedMs`), `workstreams` (`{id, title, minutes}`), `meetingCount`,
`meetingMs`, `codingAgentBlocks`, `codingAgentActiveSeconds`.

Event `clovy://day-intelligence` (`{ day, running }`) follows a run's start and
end and new reports. Types live in `src/lib/day-intelligence.ts`.

## Settings (`activity-settings.json`)

`daySummary.time` ("18:00"), `notifications.enabled` (true),
`notifications.quietHours` `{enabled: false, start: "22:00", end: "08:00"}`.
Invalid times load the defaults.

## Coding-agent blocks

`sources::coding_blocks_between` is the integration point for coding-agent
ingestion: until that slice is merged it returns none. Hour prompts, the
summary prompt, and the panels already take blocks (source, project, title,
local times, active seconds, summary) and are tested with synthetic ones.
