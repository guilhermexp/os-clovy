# Activity capture

Opt-in, macOS-only, text-only recording of what the user sees and does, kept in
an encrypted database on the Mac. This doc is the contract for the slices that
build on it (timeline, coding-agent ingestion, day summaries). Terms are in
[CONTEXT.md](../CONTEXT.md#activity-capture); the encryption decision is
[ADR-0057](adr/0057-activity-database-uses-sqlcipher-linked-app-wide.md).

## Where the code lives

| Path | Role |
|---|---|
| `src-tauri/src/activity/mod.rs` | Tauri commands, managed state, `setup` (starts the capture thread), `publish` (frontend event + menu bar) |
| `src-tauri/src/activity/engine.rs` | `CaptureEngine::tick` (every 2 s), `ActivityShared` (state shared with commands and the tray) |
| `src-tauri/src/activity/platform.rs` | `ActivityPlatform` trait: the OS seam (window descriptors, AX/OCR text, input, clipboard, disk) |
| `src-tauri/src/activity/macos/` | `MacActivityPlatform`: Accessibility, ScreenCaptureKit + Vision OCR, event tap, pasteboard, permissions |
| `src-tauri/src/activity/store.rs` | `ActivityStore`: the only code that opens `activity.sqlite3`; schema, migrations, read/write API, retention, debug export |
| `src-tauri/src/activity/key.rs` | Keychain key (`ActivityKeyStore`) |
| `src-tauri/src/activity/{filter,schedule,input,redact,settings}.rs` | Exclusions and work hours, tick plan and state machine, input normalizer, clipboard redactor, `activity-settings.json` |
| `src/components/settings/ActivitySettingsSection.tsx`, `src/lib/activity-capture.ts` | Settings → Activity tab and its typed bindings |
| `src-tauri/src/menu_bar.rs` | Menu bar status line and "Pause capture" / "Resume capture" |

Tests: `cargo test --manifest-path src-tauri/Cargo.toml activity` (every Rust
test of the module is under `activity::`), and
`pnpm exec vitest run --configLoader=runner src/test/activity-settings`.

## Lifecycle

`activity::setup` (called from `lib.rs` after the tray exists) loads settings
and starts the `clovy-activity-capture` thread on macOS; elsewhere it manages an
unsupported state and the tab is hidden. The thread ticks every 2 s whether or
not capture is on (only permissions are read while it is off) and is woken
early by any command. Each tick:

1. Reads TCC permissions (Accessibility, Screen Recording, Input Monitoring).
2. Opens the database on first need (only once capture or a coding-agent
   source is enabled; a key is minted only then).
3. Evaluates the capture state (`schedule::evaluate`), in this precedence:
   `off` → `keyMissing` / `error` → `needsPermissions` (Accessibility and
   Screen Recording are required) → `paused` (`manual` → `workHours` →
   `lowDisk` → `protectedVideo`) → `active`.
4. Opens or closes a pause record on every pause-reason transition.
5. While active, captures the focused window, every 5th tick the secondary
   displays (when enabled), input events and the clipboard; every 30th tick
   checks free disk space; on the first tick and every 1800th runs retention.

State changes are emitted as `clovy://activity-state` (payload: the status DTO
below) and mirrored to the menu bar.

## Privacy rules (enforced before anything is read or written)

- **Text only.** Accessibility text first (Chromium/Electron apps get
  `AXManualAccessibility`/`AXEnhancedUserInterface` set once per process);
  window chrome (toolbars, menus, buttons, images, scroll bars) is not counted
  as text, so when the content has fewer than 20 non-whitespace characters
  Clovy falls back to OCR with Apple Vision on an in-memory ScreenCaptureKit
  image of exactly that window (`ocr.rs` calls ScreenCaptureKit and Vision
  through objc2). No image is encoded or written anywhere.
- **Bounded ticks.** Every AX element gets a 0.25 s messaging timeout (the
  system default is 6 s); the private-badge walk is capped at 150 ms, the URL
  walk at 250 ms, the text walk at 300 ms / 3000 nodes / 20k characters, and
  each ScreenCaptureKit completion at 2 s. A window that is still building its
  tree (a fresh Chrome window) cannot stall capture.
- **Exclusions** (`filter::skip_reason`): Clovy's own windows (own pid or a
  `co.opensoftware.june*`/`co.opensoftware.clovy*` bundle id), ignored apps
  (exact, case-insensitive name or bundle id), private/incognito windows (no
  OCR fallback either), URLs whose host is an ignored domain or one of its
  subdomains, and, while any domain is ignored, browser windows whose tab URL
  is not known yet (Chromium builds its accessibility tree a moment after
  Clovy first asks). An excluded window has its title and URL read to decide,
  but no window text is extracted, no frame is stored, and the tick's input
  and clipboard are discarded.
- **Private windows** are recognized by title markers (Chrome "Incognito" /
  pt-BR "Modo anônimo", Firefox "Private Browsing" / pt-BR "Navegação
  privativa", Safari "Private Browsing", Edge "InPrivate", and es/fr/de
  variants) or by an incognito/private badge in the first levels of the
  window's accessibility tree. The check runs for every app the one browser
  predicate (`filter::is_browser`: known bundle ids or a browser name as a
  whole word) accepts. False positives only drop frames.
- **Revalidation.** For browser windows the descriptor is read again after the
  text is extracted; the sample is dropped unless the same window still shows
  the same URL and is still capturable (a navigation to an ignored domain, a
  tab turning private, or an unknown re-read all discard it). Secondary frames
  follow the same rule.
- **Protected video** (setting, on by default): a known DRM streaming app or
  site focused pauses capture with reason `protected_video`; such windows on
  secondary displays are skipped.
- **Work hours** (setting): outside the configured local days/times capture
  pauses with reason `work_hours`. An end at or before the start spans midnight
  and belongs to the start day.
- **Low disk**: below 1 GiB free capture pauses (`low_disk`) and resumes above
  2 GiB.
- **Input** carries no content: the event tap reads only the event type; the
  normalizer drops any characters a platform reports. Clipboard text is
  redacted (`redact.rs`: private keys, `password=` / `password = value` /
  `"token": value` assignments, bearer values, provider token prefixes, JWTs,
  card numbers, high-entropy strings) and truncated to 1000 characters.
  Pasteboards marked concealed or transient (password managers) are skipped.
  Clicks, keys, and clipboard changes are buffered between ticks without their
  source window, so they are stored only when the same capturable window and
  URL were focused at both ends of the 2 s interval; any switch in between
  discards that interval's input (the app switch itself is still recorded).
- **Pause control.** "Pause capture" / "Resume capture" (settings and menu
  bar) act on the manual pause only; an automatic pause (work hours, low disk,
  protected video) shows "Pause capture" and resumes on its own.

## Database

`activity.sqlite3` in the app data dir (`co.opensoftware.june-dev` in debug
builds), SQLCipher with a random 32-byte raw key in the Keychain service
`co.opensoftware.clovy.activity-db` (`co.opensoftware.clovy-dev.activity-db` in
debug builds), WAL, `auto_vacuum = INCREMENTAL`. A file without its key is
unreadable ("file is not a database").

Lost key: if the file exists but the key is missing or wrong, the store
returns `KeyMissing`/`KeyRejected`, capture shows `keyMissing`, and nothing is
deleted or rekeyed until the user confirms "Recreate database"
(`activity_recreate_database`), which deletes the file and its WAL and starts
empty with a new key.

Timestamps are RFC 3339 UTC with microseconds and `Z`
(`2026-10-04T17:43:10.123456Z`), so SQL string comparison is chronological.

### Schema (migration 1, `activity_capture`)

Migrations are an append-only catalog in `store.rs` (`MIGRATIONS`), recorded in
`schema_migrations(version, name, applied_at)`; a database from a newer build
refuses to open (`NewerSchema`). Add a version; never edit one.

| Table | Columns | Notes |
|---|---|---|
| `frame_texts` | `id`, `hash` (sha256 hex, unique), `text` | Deduplicated text bodies; consecutive ticks of an unchanged window share a row |
| `frames` | `id` (AUTOINCREMENT, never reused), `captured_at`, `app_name`, `bundle_id`, `window_title`, `browser_url`, `text_source` (`accessibility`/`ocr`/`none`), `text_id` → `frame_texts` | One row per 2 s tick of the focused window; the timeline's input |
| `secondary_frames` | `id`, `captured_at`, `display_id`, `app_name`, `bundle_id`, `window_title`, `browser_url`, `text_source`, `text_id` | ~10 s context samples of other displays; never part of a session |
| `input_events` | `id`, `occurred_at`, `kind` (`click`/`key`/`app_switch`/`window_focus`/`clipboard`), `app_name`, `count`, `clipboard_text` | Clicks/keys coalesced per tick (`count`); `CHECK (kind = 'clipboard' OR clipboard_text IS NULL)` |
| `pauses` | `id`, `started_at`, `ended_at` (NULL while open), `reason` (`manual`/`work_hours`/`low_disk`/`protected_video`) | Pauses left open by a crash are closed at the next start |
| `processing_cursor` | `consumer` (PK), `last_frame_id`, `last_frame_at`, `updated_at` | Per-consumer high-water mark; `timeline` is the one retention honors |

Migration 2 (`activity_timeline`) adds the timeline's tables; see
[activity-timeline.md](activity-timeline.md#schema-migration-2-activity_timeline).
Migration 3 (`coding_agent_blocks`) belongs to coding-agent ingestion; see
[coding-agent-sessions.md](coding-agent-sessions.md#database-migration-3-coding_agent_blocks).

### Rust API (`crate::activity::store::ActivityStore`)

Get the open store from the managed `ActivityState` runtime
(`ActivityShared::store()`); it is `None` while capture was never enabled or the
key is missing. All methods are async and safe to call concurrently with
capture (two-connection WAL pool).

| Method | Use |
|---|---|
| `frames_after(after_id, limit)` | Timeline ETL read path, oldest first, text joined |
| `processing_cursor(consumer)` / `advance_processing_cursor(consumer, last_frame_id, last_frame_at)` | Read and move the cursor; never moves backwards. Use `TIMELINE_CONSUMER` for the timeline |
| `secondary_frames_between(from, to)`, `input_events_between(from, to)`, `pauses_between(from, to)` | Context, signals, and pause records for a time range (`pauses_between` includes a still-open pause) |
| `latest_frame_at()` | Newest frame time |
| `prune(now, retention_days)` | Retention (the engine calls it hourly) |
| `debug_export(dir, now, limit)` | Readable JSON for verification (debug builds only, via the command) |

Writes (`insert_frame`, `insert_secondary_frame`, `insert_input_events`,
`open_pause`, `close_pause`) belong to the engine.

### Retention

Default 30 days, configurable 1 to 365. A frame is deleted only when it is older
than the period **and** `id <= processing_cursor('timeline').last_frame_id`.
Secondary frames, input events, and closed pauses are deleted when older than
both the period and the cursor frame's `last_frame_at`. Unreferenced
`frame_texts` go with them. Each sweep that deleted anything then runs
`PRAGMA incremental_vacuum(500)`, so space returns gradually without a full
`VACUUM` blocking capture. Until the timeline advances the cursor, nothing is
deleted. The same sweep removes timeline sessions, gaps, and search documents
older than the period.

## Settings (`activity-settings.json`, app config dir)

`enabled` (false), `secondaryMonitors` (false), `inputEvents` (true),
`pauseOnProtectedVideo` (true), `ignoredApps`, `ignoredDomains` (bare hosts,
normalized on save), `workHours` `{enabled: false, days: [1..5] (ISO), start:
"09:00", end: "18:00"}`, `retentionDays` (30), `codingAgents` (every source
off; see [coding-agent-sessions.md](coding-agent-sessions.md)). Malformed
files load defaults. Manual pause is in memory only: a restart resumes capture.

## Commands and event (frontend contract)

| Command | Request | Returns |
|---|---|---|
| `activity_status` | none | status DTO |
| `activity_save_settings` | `{ request: { settings } }` | status DTO |
| `activity_set_paused` | `{ request: { paused } }` | status DTO |
| `activity_request_permission` | `{ request: { permission: "accessibility" \| "screenRecording" \| "inputMonitoring" } }` | status DTO |
| `activity_recreate_database` | none | status DTO |
| `activity_debug_export` | none | `{ path, frames, secondaryFrames, inputEvents, pauses, codingAgentBlocks }`; errors in release builds |
| `open_privacy_settings` | pane `"inputMonitoring"` added (`Privacy_ListenEvent`) | void |

Status DTO: `{ supported, settings, state: { kind: "off" | "active" |
"paused" (reason) | "needsPermissions" (missing) | "keyMissing" | "error"
(message) }, permissions: { accessibility, screenRecording, inputMonitoring:
"granted" | "denied" | "notDetermined" | "unsupported" }, manualPause,
lastFrameAt, debugExportAvailable }`. Types live in `src/lib/activity-capture.ts`.

## Debug export

`activity_debug_export` (development builds only; the Activity tab shows the
button only when `debugExportAvailable`) writes
`<data dir>/activity-debug-exports/activity-debug-export-<UTC>.json` (mode 0600)
with the newest 500 frames, all secondary frames, input events, pauses,
coding-agent blocks (without their transcripts), the cursor, and the schema
version. The key is never written; a test asserts it is
absent from the output.
