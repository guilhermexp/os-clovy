# Activity timeline

Turns [activity capture](activity-capture.md) frames into a day timeline:
app sessions with their context, classified gaps, deterministic categories,
day statistics, and full-text search, all inside the encrypted
`activity.sqlite3`. The "Today" view and the agent read it; later slices (day
summaries, the MCP server, coding-agent lanes) build on the query API below.
Terms are in [CONTEXT.md](../CONTEXT.md#activity-capture).

## Where the code lives

| Path | Role |
|---|---|
| `src-tauri/src/activity/timeline/builder.rs` | Pure state machine: frames → `Opened` / `Window` / `Closed` / `Gap` events; gap classification |
| `src-tauri/src/activity/timeline/context.rs` | Session context: browser domain, editor workspace from the window title |
| `src-tauri/src/activity/timeline/categorize.rs` | The ten-class categorizer and its confidence floor |
| `src-tauri/src/activity/timeline/stats.rs` | Day statistics (focused, idle, away, top apps, categories) |
| `src-tauri/src/activity/timeline/db.rs` | Timeline tables: batch persistence, day/detail/search queries, debug export sections |
| `src-tauri/src/activity/timeline/etl.rs` | One incremental pass over unprocessed frames |
| `src-tauri/src/activity/timeline/mod.rs` | Tauri commands, background loop, exclusion/retention filtering, agent tool wiring |
| `src-tauri/src/activity/timeline/agent_tools.rs` | `search_activity`, `get_activity_timeline` |
| `src-tauri/src/activity/timeline/debug_import.rs` | Development-only fixture import (`CLOVY_ACTIVITY_DEBUG_IMPORT`) |
| `src/components/activity-timeline/`, `src/lib/activity-timeline.ts` | The "Today" view and its typed bindings |

Tests: `cargo test --manifest-path src-tauri/Cargo.toml activity` (timeline
tests are under `activity::timeline::`) and
`pnpm exec vitest run --configLoader=runner src/test/activity-timeline`.

## ETL

`timeline::start` (from `activity::setup` on macOS) runs the background loop:
while a backlog remains it runs passes of at most 10,000 frames back to back,
releasing the pass lock between them; once caught up, one pass every 30 s while
the activity store is open. `activity_timeline`, `activity_timeline_search`, and
the agent tools call `refresh_before_read` first: a pass of at most 2,000
frames, skipped (not awaited) when another pass holds the lock, so a read never
waits for a historical backfill and answers from what is already built.

A pass reads frames after `processing_cursor('timeline')` in batches of up to
500 (`frames_after`), with that batch's input events (`input_events_between`)
and pause records (`pauses_between`), feeds them to the builder, and writes the
resulting rows, the builder state (`timeline_state`), and the cursor
(`advance_cursor_on`) in **one transaction**. A crash rolls the whole batch
back, so a restart never processes a frame twice or skips one. Retention (in
`ActivityStore::prune`) only deletes frames the cursor has passed. Only a pass
that caught up closes a session left open past the threshold (with a backlog,
later frames may still continue it). Meeting evidence (Clovy recordings) is
loaded from the older of the open session's start and the first unprocessed
frame, so a first build over weeks of retained frames still sees old meetings.

Rules (`builder.rs`):

- **Useful frame**: the app, window title, URL, or text changed since the
  previous frame, or input (click, key, switch, clipboard) happened from 3 s
  before to 1 s after it. Other frames are idle. `loginwindow` and
  `ScreenSaverEngine` frames are never useful.
- **Sessions**: consecutive useful frames of the same app (name and bundle id)
  and context. A new context splits a session only when both are known: the
  browser's domain (`filter::url_host`, no `www.`) or the editor's workspace
  (VS Code family: last title segment; Zed, Xcode, JetBrains: first segment).
  A session lasts from its first frame to the next session's first frame, or to
  its last useful frame + 2 s when a gap follows. At most one session is
  active; it is categorized provisionally on every pass and finally on close.
- **Gaps**: more than 5 minutes between the end of a useful frame and the next
  one. Classified as `paused` (a pause record of one reason covers at least
  half; the reason is kept: `manual`, `work_hours`, `low_disk`, or
  `protected_video`), else `idle` (idle frames cover at least half), else
  `sleep` (no frames: the Mac slept, capture was off, or an excluded app was in
  front). A gap closes the session before it, so its time is never session
  time. An active session left more than 5 minutes after its last useful frame
  is closed by the next pass that caught up.
- **Ongoing gap**: while the gap has not ended yet (an unchanged screen, an open
  pause, the Mac asleep), it is not stored; reads that are caught up with
  capture add it from the last useful frame to now (`ongoing: true`, `id` 0),
  classified the same way, so the view, stats, and agent count idle and away
  time as it happens. The next useful frame stores the real gap.
- **Time going backwards** (clock change, imported fixture): the open session
  closes and the builder restarts without a gap; the first frame past the time
  reached before the rewind restarts it again, so no gap spans time already
  built.

## Categories

`categorize.rs`, no model: scores per category from the app (a fixed table of
known apps; terminals get a small coding prior, browsers none), window titles
and URLs weighted by each window's share of the session's frames (pull
requests, devops consoles and files, planning, design, docs, communication,
meeting URLs, research sites, entertainment; unknown titles nudge research),
captured text tokens (devops commands, code keywords, meeting controls, diffs),
and meeting audio (a Clovy recording in `recording_sessions` overlapping the
session: strong with a meeting app or meeting URL, weak otherwise). Confidence
is the winner's share of the total; below `CONFIDENCE_FLOOR` (0.35) the session
is `idle_personal`.

| Stored (`category`) | DTO | UI |
|---|---|---|
| `coding` | `coding` | Coding / Programação |
| `code_review` | `codeReview` | Code review / Revisão de código |
| `meeting` | `meeting` | Meeting / Reunião |
| `communication` | `communication` | Communication / Comunicação |
| `design` | `design` | Design |
| `documentation` | `documentation` | Documentation / Documentação |
| `planning` | `planning` | Planning / Planejamento |
| `deployment_devops` | `deploymentDevops` | Deploy and DevOps / Deploy e DevOps |
| `research` | `research` | Research / Pesquisa |
| `idle_personal` | `idlePersonal` | Idle or personal / Ocioso ou pessoal |

## Schema (migration 2, `activity_timeline`)

Appended to the activity catalog in `store.rs` (append-only; never edit).

| Table | Columns | Notes |
|---|---|---|
| `timeline_sessions` | `id`, `app_name`, `bundle_id`, `context_kind` (`domain`/`workspace`), `context`, `started_at`, `ended_at`, `duration_ms`, `first_frame_id`, `last_frame_id`, `frame_count`, `idle_frame_count`, `status` (`active`/`closed`), `category`, `confidence`, `meeting_audio`, `search_rowid_first`, `search_rowid_last` | Unique partial index: one `active` row at most |
| `timeline_session_windows` | `session_id` → sessions (cascade), `window_title`, `browser_url`, `first_seen_at`, `last_seen_at`, `frame_count` | PK `(session_id, window_title, browser_url)`; the detail view and categorizer evidence |
| `timeline_gaps` | `id`, `started_at`, `ended_at`, `duration_ms`, `kind` (`idle`/`sleep`/`paused`), `pause_reason` | `pause_reason` set iff `kind = 'paused'` |
| `timeline_state` | `id` (=1), `state_json`, `updated_at` | Builder state, open session row id, open search document |
| `timeline_search` | FTS5 (`unicode61 remove_diacritics 2`): `window_title`, `browser_url`, `body`, `session_id` (unindexed), `seen_at` (unindexed) | One document per window segment of a session (same title and URL, at most 5 minutes); `body` is the distinct lines seen (≤ 32k chars) |

Retention: each sweep also deletes closed sessions, gaps, and search documents
older than the retention period, and every query clamps its range to the
period, so search and the agent never return expired data.

## Query API (Rust, for later slices)

All in `crate::activity::timeline` over an open `ActivityStore`:

| Function | Use |
|---|---|
| `timeline_view(store, settings, from, to, now)` | Sessions and gaps overlapping `[from, to)` plus `TimelineStatsDto`, after current exclusions and retention |
| `search_view(store, settings, query, from, to, limit, now)` | FTS5 results (best first) with session id, app, title, URL, `seen_at`, snippet |
| `db::session_detail(store, id)` | Session, its windows (most seen first, ≤ 20), and a text excerpt (≤ 1500 chars) |
| `db::sessions_between`, `db::gaps_between` | Raw rows without filtering |
| `refresh_before_read(app, store)` | Bounded catch-up before a read (at most 2,000 frames; skipped while another pass runs) |

Exclusions: a session is hidden while the app is in `ignoredApps` or its domain
matches `ignoredDomains`, so excluding something later also removes its history
from the view and the agent. Search applies the same rule with the session's
persisted domain and the document's own URL (a document written before the tab
URL was known has none), and pages through ranked FTS results until `limit`
allowed rows are found or the matches run out.

## Commands and event (frontend contract)

| Command | Request | Returns |
|---|---|---|
| `activity_timeline` | `{ request: { from, to } }` (RFC 3339) | `{ availability, message, captureEnabled, sessions, gaps, stats }`; a gap with `ongoing: true` (`id` 0) runs to the time of the read |
| `activity_timeline_session` | `{ request: { id } }` | `{ session, windows, textExcerpt }`; error `activity_session_not_found` |
| `activity_timeline_search` | `{ request: { query, from, to, limit } }` (`from`/`to`/`limit` nullable; limit 1 to 200, default 50) | `{ availability, results }` |

`availability`: `ready`, `neverEnabled` (no activity database and capture
off), `unsupported` (not macOS), `keyMissing`, `error`. When capture is off
but history exists, the view opens the database to show it. Event
`clovy://activity-timeline` (`{ changed: true }`) follows a pass that wrote
sessions or gaps. Types live in `src/lib/activity-timeline.ts`.

## Agent tools

Named in the `activity-timeline` spec of the OpenSpec change
`add-activity-intelligence`; offered in the catalog
(`agent_runtime/api.rs::tool_descriptors`) only while capture is enabled and
dispatched in `agent_runtime/tools.rs` to `dispatch_agent_tool`, which refuses
with `activity_capture_off` otherwise.

| Tool | Arguments | Result |
|---|---|---|
| `get_activity_timeline` | `from`, `to` (RFC 3339, optional; default local midnight to now) | Sessions (app, context, category, local start/end, minutes, active, window title; ≤ 300), gaps (kind, pause reason, ongoing; ≤ 300), `totalSessions`, `totalGaps`, `truncated` when either list was cut, stats in minutes |
| `search_activity` | `query`, optional `from`, `to`, `limit` (1 to 50, default 20) | Results with session id, app, window title, URL, local `seenAt`, snippet |

## "Today" view

First-level sidebar view (hidden where capture is unsupported). Day navigation,
a 24 h strip with one block per session colored by category and distinct gap
segments (idle, system sleep, paused: manual, work hours, low disk, protected
video), the live active session (refetch on the event and every 30 s), a
chronological list, session detail (windows, URLs, excerpt), statistics, and
search with a period filter whose results open the day at that session. Hour
ticks sit at the real local times of the selected day (`hourTicks`), so on
23- and 25-hour DST days they line up with the blocks.
The header switches between this timeline and the "Day summary" section
([day-intelligence.md](day-intelligence.md)); the day strip stays above both.

### Lane extension point

`src/components/activity-timeline/lanes.ts` is the hook for extra lanes (the
coding-agent lane of the next slice). Register a lane by appending to
`EXTRA_TIMELINE_LANES`:

```ts
type TimelineLane = {
  id: string;
  label: MessageKey; // catalog key, en and pt-BR
  render: (props: TimelineLaneProps) => ReactNode;
};
type TimelineLaneProps = {
  from: string; to: string;         // selected local day, RFC 3339
  dayStartMs: number; dayEndMs: number;
  xForTime(ms: number): number;     // 0..1 fraction of the day
};
```

`TimelineDayStrip` renders the sessions lane, then each registered lane in
order, each in its own row on the same 24 h scale. A lane fetches its own data
for `from`..`to` (and may listen to `clovy://activity-timeline` or its own
event); positions are `xForTime(ms) * 100%` of the track width.

## Debug data

Development builds only. `activity_debug_export` adds `timelineSessions`,
`timelineGaps`, `timelineSearchDocuments` (count), and `timelineState` to the
export. To verify with test data, launch the debug app with
`CLOVY_ACTIVITY_DEBUG_IMPORT=/path/fixture.json`; once the activity store is
open, the fixture is written through the store's write API (format in
`debug_import.rs`) and a marker in `<data dir>/activity-debug-imports/` keeps
it from being imported twice. Import with capture off, so live frames do not
interleave with the fixture's.
