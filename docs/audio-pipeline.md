# Audio pipeline — capture to note

How Clovy records meeting audio, separates sources, detects conversation turns,
and transcribes into a note. It is **saved-audio-first**: the local WAV is the
source of truth, provider speed is secondary. See
[ADR-0005](adr/0005-source-separated-audio-capture.md) (one WAV per source),
[ADR-0004](adr/0004-out-of-process-system-audio-helper.md) (system-audio
helper), and [ADR-0002](adr/0002-live-transcript-preview-strategy.md) (live
preview).

## Provider boundary

Saved-note transcription and live preview use the selected transcription engine.
Groq calls use the user's key; missing keys fail before network access.
Local (Parakeet Ultra) needs an installed model and never reads a Groq key. Notes and transcript cleanup use the selected Notes
engine: OpenRouter with the user's key, CLI, or endpoint. No inference server
or provider fallback participates. Prompts remain local under
`src-tauri/src/prompts/`, and CLI/endpoint calls retain `llm::generate`.
See [llm-providers.md](llm-providers.md) for pacing, retries and saved jobs.

## Recording assistance and marks

The note editor offers recording marks and an explicit request for a relevant
question while recording. Marks persist against the existing note and recording
session; native capture supplies the active elapsed timestamp, so paused wall
time never advances a mark. They remain available when reopening the note from
history, independently of final transcription or note-generation success.

Question requests use bounded, recent preview text from the active recording
only. They require live transcription to be enabled and the recorder to be
running. The selected Notes engine answers in an isolated tool-free one-shot
call, with transcript content treated as untrusted data. Pause, stop or turning
live transcription off invalidates pending assistance. Provider errors leave
capture, saved audio and final processing intact. Requests are explicit because
the selected engine may process text remotely and incur provider usage; no
additional provider or automatic fallback is introduced.

If the selected CLI cannot disable all tools for this operation, assistance
fails with setup guidance instead of giving the transcript access to tools or
switching engines. The existing final note-generation routes retain their
documented isolation contracts in [llm-providers.md](llm-providers.md).

Preview remains ephemeral and separate from authoritative `transcripts` rows.
The existing final-transcription and note-generation pipeline produces the
summary and evidence-backed decisions and next steps on the same note. Marks
are user annotations, not inferred agreements or tasks. See
`openspec/changes/reuniao-assistida/` for the scenarios and verification scope.

## Data flow

1. **`start_recording`** → `capture::start_capture` opens `microphone.partial.wav`
   (a CPAL input stream) and, in meeting mode, starts the system-audio helper
   writing `system.partial.wav`.
2. Per input callback: convert samples to 16-bit PCM, update lock-free level
   atomics, and publish fixed-size blocks into a preallocated bounded audio
   ring. The callback allocates nothing, takes no locks, and performs no I/O.
   A dedicated non-real-time task drains the ring into the WAV writer and
   non-blockingly feeds the **live preview** sink; its worker transcribes ~8s
   chunks and emits ephemeral `live-transcript-event`s that are **never
   persisted**. The ring holds 30 seconds at the configured sample rate and
   channel count, with a memory cap for unusual high-channel devices. If disk
   writing ever falls more than that capacity behind, the oldest queued blocks
   are dropped, exact dropped-sample counts appear in recording status, and
   recovery/finalization checkpoints persist the count. Writer progress is
   tracked separately from callback production: `bytesWritten` advances only
   after successful WAV writes. The first writer I/O error, panic, unexpected
   exit, or sustained stall stops the drain and immediately enters the existing
   microphone warning path; recovery checkpoints persist its diagnostic code.
   Each queued block also carries its callback generation, so overflow cannot
   merge surviving live-preview audio across a dropped callback boundary.
3. **`finish_recording`** stops the input stream, drains and finalizes the
   writer task, then atomically renames
   `*.partial.wav` → `*.wav` (the durability commit), stops the helper, cancels
   preview. Completed microphone byte metadata is replaced from the writer
   watermark returned after that final drain.
4. **`process_saved_source_audio`** (`src-tauri/src/domain/processing.rs`) runs
   the batch pipeline for microphone-only and dual-Source recordings:
   `drop_silent_system_sources` → dual-Source `turns::detect_turns` (or one
   authoritative full-Source microphone job) → reconcile durable fingerprinted
   note-transcription jobs → bounded Turn preparation → one
   in-flight provider request per Source → atomically persist each successful
   job and transcript row → **note generation**. Full-Source fallbacks are
   prepared lazily when a Source is materially incomplete and atomically
   replace that Source's partial rows only after the replacement succeeds.

While capture is active, the native meeting-HUD supervisor samples capture at
20 Hz and emits the additive `recording-telemetry` Tauri event. Its narrow
payload carries the recording session id, state, elapsed time, audio levels,
and live warnings; both the main renderer and meeting HUD subscribe to that
single stream. In the main renderer, the latest sample stays in a narrow
external store: waveform subscribers receive level samples at 20 Hz, elapsed
time subscribers publish only on whole-second boundaries, and the root App
reducer receives only lifecycle, Source-health, and actionable-warning changes.
Stable metadata still comes from the recording commands, and
`get_recording_status` remains available as a read-only compatibility command.
Recovery durability is independent of telemetry: a recording-scoped worker
requests a ring watermark flush and checkpoints elapsed time every 500 ms after
the recording rows are created. The WAV task drains through that watermark and
flushes the WAV before the recovery row advances. A dead writer releases the
flush wait so recovery state and diagnostics continue advancing instead of
waiting for the full timeout.

## Meeting detection

A one-second native poll asks which processes own the microphone, drops
Clovy's own process and the dictation helper (`owned_pids`), and keeps the
ones on the meeting-app allowlist. When one appears and Clovy is not
recording, the HUD receives `meeting-detection-event` with the app labels
("Zoom", "Chrome") and asks whether to record. The signal is microphone
ownership only: no window titles or URLs, so Google Meet reads as "Chrome
holds the microphone".

The optional meeting-video offer separately enumerates visible windows and
uses their app and title to identify a capture candidate. It does not change
the microphone-ownership trigger or the meeting-end detector.

- **macOS:** CoreAudio process objects with `IsRunningInput` and an input
  device; apps are matched by bundle-id prefix.
- **Windows:** WASAPI capture sessions in `AudioSessionStateActive` on every
  active capture endpoint, each session pid lifted through a Toolhelp process
  snapshot to the app that owns it (out of `msedgewebview2.exe` into its host,
  and from a Chromium or Firefox child to the browser process, so a WebView2
  owned by Clovy is still Clovy). Apps are matched by lowercase executable
  name or, for packaged apps, by package name. The microphone consent store
  (`HKCU\...\CapabilityAccessManager\ConsentStore\microphone` and its
  `NonPackaged` subkey, in use while `LastUsedTimeStop` is 0) only adds an
  allowlisted app that no session showed and that has a running process; it
  never adds unlisted apps, so it cannot make meeting-end evidence ambiguous.
  The parser (`meeting_detection/windows_probe.rs`) is pure and tested on any
  host; only the reader is Windows-only.

## Meeting video (macOS)

The recording options and meeting-detected prompt offer an optional
"Record meeting video" switch when `detect_meeting_window` finds a meeting
window. It is off by default and identifies the app and window title. The
helper checks that exact window after audio starts. A Meet tab behind another
browser tab may not be detected because its title is not the window title.
Native Zoom and Teams candidates need meeting/webinar title indicators;
settings, chat, calendar and other known nonmeeting windows are excluded.
Utility exclusions match whole titles after removing known app labels, so
explicit meeting subjects may contain words such as calendar or chat.
Generic meeting titles also require microphone ownership by the app family.
Ambiguous custom titles are not selected. The displayed app and title identify
the target before opt-in; the selector never silently switches the chosen ID.
The HUD's native queue retains that exact choice, including video off, across
renderer retries. A renderer cannot replace it with another window.
Enumeration runs in the desktop process. If its Screen Recording permission
does not allow enumeration, detection returns no candidate and the switch
is hidden. The host grant is separate from the capture helper's grant.

The audio capture starts first. A separate signed `Clovy Video Capture.app`
uses ScreenCaptureKit's `desktopIndependentWindow` filter for that one
window. It writes silent H.264 MP4, at most 5 frames per second, with a long
side no larger than 1920 pixels and ten-second fragments. It has its own
Screen Recording permission; denial or helper failure leaves audio running.
The segmented AVAssetWriter output supplies an initialization segment and
media fragments, which a serialized delegate appends and synchronizes into
one MP4. Recovery checks declared sample extents against the available media
payload and retains earlier complete fragments when the tail is invalid.
Signals follow the audio helper convention: pause, resume and finish. Paused
frames are dropped and removed from video time. The artifact records the
video's start offset relative to recorded audio time.
Pause and resume first publish fixed-size, sequenced host-clock records to
an owner-only nonblocking FIFO, then change the audio state. Creating the
FIFO occurs after audio starts; queued controls survive a pending helper
launch. The helper drains it immediately before appending a frame. Signals
are wakeups and finalization requests, while the JSON ledger is diagnostic.
The audio control path never waits for a disk write, worker or acknowledgement.
A full or broken channel fails only the video and discards its unsafe files.
A clean FIFO end after the original host process terminates instead finishes
the recoverable video. The helper checks the parent's kernel birth identity,
applies complete queued controls and accepts no further frames. A live or
unknown parent, malformed controls or a truncated record still fails capture.
Shutdown arms an independent four-second deadline before status writes or
awaiting ScreenCaptureKit. A known failure is published before that await;
a stalled shutdown terminates immediately without cleanup I/O, preserving
already synchronized fragments and metadata for recovery.
Pause clears the cached frame, and resume rejects samples captured before
its boundary, including samples delivered late by ScreenCaptureKit.
Resume requests a fresh image of the same window so static content can
continue; a stale completion cannot cross a newer pause or replace a newer
stream frame.
The first-frame deadline excludes paused time. A frame timer keeps
static slides advancing through MP4 fragments without exceeding the frame cap.
The timer rearms at the next eligible deadline after each write. Early wakeups
wait for that deadline rather than dropping a whole interval; writer pressure
waits before retrying, and no catch-up burst can exceed the cap.
After a delayed control event, frames whose adjusted presentation time would
overlap the last successful write are deferred until the video timeline is
again at least 200 ms ahead. The original pause boundaries remain unchanged.

`video_artifacts` is separate from the WAV-oriented `audio_artifacts` table.
It tracks the session, paths, state, app, window title, offset, dimensions,
duration and failure reason. Session files are `window.partial.mp4` while
capturing and `window.mp4` after finalization. Note and recording deletion
include both paths. Recovery retains completed MP4 fragments; a fragment
that was not committed before a crash cannot be recovered. Video failures
do not discard the audio artifacts.
Deletion also covers projects deleted with their notes and helper metadata.
It revokes a per-run lease before removing files, so a delayed helper launch
cannot recreate them. A separate ownership monitor and the final database
update handle deletion while capture or finalization is already running.
Interrupted `recording` artifacts may recover complete fragments; explicitly
failed captures retain their failure notice and use transcript-only generation.
Errors while loading optional video are logged and produce the capture_failed
analysis notice while transcript generation continues.
Recovery and analysis require a contained artifact in the current profile's
exact session directory. Paths from another profile in a copied database are
rejected before reading or modifying the video.
An unfinished artifact is repaired only after terminal helper status or a
process identity check confirms that its helper is gone. The PID is paired
with process birth time and executable identity, so PID reuse by another
process cannot block recovery indefinitely. A live helper, unknown process
identity or pending launch is left untouched;
generation falls back instead of truncating a file that may still be written.
The helper durably checkpoints the audio offset before its first appended
frame. Recovery restores that timing before repairing a recording; missing
or invalid timing fails video analysis instead of silently using zero.
The host records the capture attempt and its own process birth identity before
creating the startup lease or launching the helper. Recovery cancels a pending
lease only after confirming that the original host is gone, revokes the lease
first, then rechecks helper registration. Live or unknown processes remain
protected, and a delayed launch cannot use the revoked lease. Checkpoints,
process identity and terminal status must match that attempt; stale metadata
cannot authorize repair of a newer live capture. After recovery renames a
file, its database update checks ownership again and removes the owned files
if note deletion won the race.
Worker liveness checks also run during paused startup. Signals require a
verified process identity and use kernel calls without spawning subprocesses.

Note generation passes a local video input only to the Claude Notes engine
when the user's `video-analysis` skill, Python and FFmpeg prerequisites exist.
The skill extracts evidence without transcribing; Claude combines those
frames with the existing transcript and cites timestamps. Other engines or
failed analysis generate from the transcript and persist a localized
"Video not analyzed: <reason>" notice. See the tool restrictions in
[llm-providers.md](llm-providers.md#meeting-video-exception) and
[ADR-0071](adr/0071-meeting-window-video-and-scoped-claude-analysis.md).

## Automatic meeting completion

Only a recording started from the native meeting prompt is eligible for
automatic completion. At start, Clovy snapshots the allowlisted external app
families that own the microphone and persists the recording origin, app-family
set, and eligibility on `recording_sessions`. Manual, agent-started, and
recovered recordings remain ineligible.

The meeting detector (CoreAudio on macOS, WASAPI on Windows) keeps polling
during capture. Meeting completion
does not inspect audio levels or speech activity: silence during a meeting is
not end evidence. Instead, every originating app family must be absent from 15
consecutive successful one-second microphone-ownership probes. That transition
opens a visible 15-second countdown in both the recording HUD and the main
recording surface. Microphone ownership returning cancels the countdown; Keep
recording suppresses it for the rest of that recording; Stop now skips the
remaining grace period. Probe failures, unknown external microphone processes,
sign-out, and polling gaps such as sleep reset the absence evidence and fail
open.

When the countdown expires, native code retains one idempotent finish request
until the main renderer receives and acknowledges it. The renderer uses the
ordinary `finish_recording` command, so writer drain, atomic WAV finalization,
validation, processing, and saved-audio recovery remain the same as a manual
stop.

After finalization, `process_saved_source_audio` emits additive
`note-processing-progress` events when a Note enters `transcribing` and
`generating`, then emits `done` with the terminal status and the Note row's
`updated_at` revision. The renderer updates stage-only state from intermediate
events and hydrates the full Note once on `done`; it drains any debounced
editor save for that Note before the hydration. `get_note` remains available
for navigation and compatibility, but is no longer polled during processing.

The terminal progress event also carries explicit microphone echo-discard spans
from this processing run, identified by recording session, Source and time.
The renderer removes a preview only when those spans cover its entire interval;
a partial interval or failed transcription without a discard remains visible.
These decisions describe detection, not an inference from missing final text.
Preview events and the terminal discard payload are ephemeral; saved WAVs and
transcription jobs remain the retry source of truth. Discard spans remain scoped
to the note and recording while a corresponding preview exists, including when
a later recording supersedes the first terminal hydration.

On Windows, loopback capture pads the active idle tail before pausing and starts
another active interval on resume. Stopping while paused adds no paused wall
time. Its pause state and timestamp are read as one synchronized snapshot
before the WASAPI call, so a concurrent resume cannot change the interval being
closed. The loopback still chooses the default render Console endpoint at startup;
selection and device changes require separate native investigation.

## Key files

- `src-tauri/src/audio/capture.rs` — mic capture lifecycle and the single
  global `ACTIVE_RECORDING` (one recorder at a time).
- `src-tauri/src/audio/capture_buffer.rs` — preallocated audio ring, atomic
  capture telemetry, recovery flush protocol, and non-real-time WAV drain.
- `src-tauri/src/audio/system_macos.rs` + `native/mac-system-audio-recorder/
  main.swift` — the system-audio helper and its readiness/permission probes.
- `src-tauri/src/audio/turns.rs` — turn detection, coalescing, WAV extraction,
  normalization, chunking, per-source configs.
- `src-tauri/src/audio/live_preview.rs` — mic/system preview workers, the
  `WavTailReader` that tails the helper's growing WAV.
- `src-tauri/src/audio/{validation,recovery}.rs` — artifact validation and
  crash recovery.
- `src-tauri/src/meeting_detection.rs` — external microphone ownership,
  meeting-start app-family snapshots, end debounce, grace period, and the
  retained finish mailbox.
- `src-tauri/src/meeting_detection/windows_probe.rs` — the Windows meeting-app
  list and the WASAPI, process-snapshot and consent-store probe.

Tauri commands: `start_recording`, `pause_recording`, `resume_recording`,
`get_recording_status`, `finish_recording`, `check_recording_source_readiness`,
`recover_recording`, `get_microphone_permission_state`. Events:
`recording-telemetry`, `note-processing-progress`, `meeting-end-state-event`,
and `clovy://meeting-end-finish`.

## System-audio helper IPC contract

The helper is controlled and observed out-of-process (see ADR-0004):

- **Control:** Unix signals — `SIGUSR1` / `SIGUSR2` = pause / resume,
  `SIGTERM` / `SIGKILL` = stop. Launched via `/usr/bin/open -n`.
- **Observation:** a `status.json` file with events `ready` / `level` / `error`
  / `stopped` (fields include `level` / `maxLevel` / `message`).
- **Routing:** a private stereo process tap is bound to the current default
  system output device, so it records the same device stream the user hears.
  The private aggregate contains the tap only; adding a physical output
  subdevice can create an output-only IO cycle with no tap callbacks. The helper
  performs at most one full-graph rebuild for missing callbacks or zero-filled
  buffers. If callbacks still stall or remain zero-filled after that rebuild,
  the helper reports the system source unavailable instead of silently writing
  a meeting-length silent WAV or entering a restart loop. Ordinary sustained
  silence remains subject to the saved-audio speech gate below.
- **CLI:** `--output` / `--status` / `--pid` / `--log`.
- **Timeouts:** ~30s readiness, ~75s probe. **macOS 14.2+** required for
  CoreAudio process taps; older systems get microphone-only.

## Turn detection

Energy-based, per-source, **no diarization**:

- 30 ms RMS windows; the activity threshold is the ~20th-percentile window
  energy times a per-source `noise_multiplier` (separates speech from
  background).
- Hysteresis: `start_active_ms` / `end_silence_ms` / `min_turn_ms` /
  `merge_gap_ms`, with separate microphone vs system config tables
  (`config_for_source`).
- Pre-provider silence checks require at least 180 ms of consecutive activity;
  one loud device-start window cannot certify an otherwise silent Source.
- Turns are ordered purely by `start_ms` / `turn_index`.
- **Speaker-echo trimming:** because the two sources are not captured through a
  single mixer, a remote participant's voice bleeding from the speakers into the
  mic can raise a false microphone turn. The detector trims the system-dominated
  spans out of a mic turn (keeping the genuine remainder) rather than dropping
  the whole turn, so a user's reply that merged with an echo survives.

## Normalization and chunking

Before transcription each turn WAV is downmixed to **mono**, resampled to
**16 kHz**, and gain-adjusted toward a target peak (bounded, with a
reuse-original shortcut when already loud enough). Normalization uses two
streaming passes over bounded raw-sample chunks: the first computes the
downmixed peak and sample count, and the second carries one global linear
resampler position across chunks while applying gain and writing incrementally.
Its working memory is fixed regardless of recording duration, and its global
resampler position preserves the historical whole-buffer output byte for byte.
Same-source turns separated by at most 2.5 s coalesce up to **600 s**
(`FINAL_TRANSCRIPTION_SPAN_MS`), never across the other Source's turn or a
trimmed bleed span. With the user's Groq key, consecutive turns of one Source
then go as **turn packs**: their normalized WAVs back to back, 0.5 s apart, up
to 600 s (19.2 MB, under Groq's 25 MB cap) in one request with segment times,
split back per turn; silence between turns is never sent. A WAV longer than
600 s (a full-Source job, an over-long turn) splits into 600 s chunks that
repeat the last **10 s** (`SPLIT_OVERLAP_MS`) at the start of the next one;
the repeated words are dropped when the texts are joined. A session that
already has turns transcribed in the 30 s layout of earlier builds keeps it,
so a retry never sends a finished turn again. Pacing and the rate-limit pause
are in `docs/llm-providers.md` (ADR-0070 addendum).

## Recovery

`scan_recoverable_recordings` reads `recording_sessions` + `audio_artifacts`.
The governing rule: **bytes on disk win over DB status** — the mic WAV is
flushed periodically and the finalized filename only appears after a clean
finalize, so a crash leaves replayable audio that recovery can finish
processing. Durable note-transcription jobs record exact Source spans and
attempt state; interrupted `running` jobs return to `pending`, and explicit
Retry resumes only jobs whose fingerprint has not already succeeded.

## Audio transcription engines

Settings > Models > Audio transcription selects Local, Groq or OpenRouter for
dictation, saved notes and live preview. An operation captures its provider and
model together. Groq keeps its timed Turn packs, pacing and quota handling.
OpenRouter and Local transcribe each Turn separately and use its saved bounds
for attribution. Long audio keeps the existing 600-second chunks and 10-second
overlap. Saved audio and completed chunks remain available for an explicit retry.

On macOS, [dictation can temporarily lower media output](dictation.md#system-output-volume-during-dictation-on-macos)
during capture. The host starts restoring the output when capture ends, ahead
of the transcription request.

### Local model installation

Local runs on Apple Silicon. Settings shows the download size, installation
progress, and occupied bytes per model. Selecting a missing model starts its
download. The current choice stays effective until verification and saving
succeed. Choosing a different model or provider supersedes the pending choice.
The management list can install without changing the active model.

| Model | Model ID | Minimum macOS | Download bytes |
| --- | --- | --- | ---: |
| Whisper large-v3-turbo | `whisper-large-v3-turbo-local` | 14 | 1,641,229,178 |
| Parakeet Ultra | `parakeet-ultra` | 14 | 632,162,964 |
| Fluid Speech Pro | `fluid-speech-pro` | 15 | 652,304,885 |
| Phonon-2.5 | `phonon-2.5` | 15 | 289,793,778 |

Each model has a fixed directory under `app_paths::app_data_dir()/models/`.
The existing `parakeet-ultra-coreml` directory and verified receipt are reused.
The bundled `src-tauri/resources/local-transcriber-model*.json` manifests pin
repositories, revisions, byte counts and every file's SHA-256. Whisper has
separate file sources for its pinned multilingual tokenizer. Install verifies
SHA-256 before finalizing a file and writes `installed.json` after all files
pass. Resume reuses intact files and requests the remaining bytes for a partial
file. Startup checks receipts and metadata. Without a valid receipt or when
sizes change, it verifies files on a worker.

Manage local models removes the selected package's weights, partial files and
receipt. The app refuses removal while a transcription holds the model.
Removing the active model requires agreeing to the switch to Groq. The app
saves the new choice before deleting the active model's files. If saving fails,
the previous choice and files remain. The app-specific model directories have
no shared download or tokenizer caches. Core ML owns its system-managed caches.

### Offline helper

`Clovy Local Transcriber.app` is an arm64 SwiftPM helper with minimum macOS 14.
Build it with `pnpm local-transcriber:prepare`. The build checks the pinned
package sources and resolution and embeds the licenses and
`Model-Attribution.txt`. FluidAudio v0.18.3
(`c8b41f98eae668794dd0083bbe028fe7e6585051`, Apache-2.0) loads Ultra and Phonon-2.5
with `loadLocal`. Phonon-2.5 uses the GPU encoder. The Fluid Speech Pro adapter
checks its Core ML tensor shapes and passes its four model components and
vocabulary to the FluidAudio v3 decoder. WhisperKit v1.1.1
(`1bbc57cb7fe6410248a685b1c5a3c6d56adc2ed4`, MIT) loads the pinned Turbo files and
tokenizer from managed directories. The helper supplies an offline tokenizer
loader that fails locally when its files are missing or invalid.
[FluidAudio v0.18.3](https://github.com/FluidInference/FluidAudio/releases/tag/v0.18.3),
[WhisperKit v1.1.1](https://github.com/argmaxinc/argmax-oss-swift/releases/tag/v1.1.1)

The JSON-lines protocol correlates `request_id` and `model_id` in both directions.

```json
{"op":"load","request_id":1,"model_id":"parakeet-ultra","model_dir":"/absolute/model/directory"}
{"op":"transcribe","request_id":2,"model_id":"parakeet-ultra","audio_path":"/absolute/take.m4a","language":"pt"}
{"op":"unload","request_id":3,"model_id":"parakeet-ultra"}
```

The helper keeps the active model loaded between takes. Rust serializes calls,
bounds responses and retires a superseded helper after any in-flight call
finishes. Each transcription creates fresh decoder state. Local requires no
remote credentials and reports a local error if loading or inference fails.
The self-test takes the path to a run-owned debug data directory and uses the
repository's synthetic Portuguese WAV.

### OpenRouter transcription

OpenRouter uses its dedicated `/api/v1/audio/transcriptions` endpoint and the
user's existing OpenRouter credential. The curated audio catalog contains
`openai/whisper-large-v3-turbo` (default), `openai/whisper-large-v3` and
`openai/whisper-1`. Each request uploads one canonical audio part with
`response_format=json`, the selected language when available, and the
dictionary hint when present. Multipart audio is limited to 25 MiB.
OpenRouter documents a 60-second upstream deadline. The desktop allows 60
seconds total for dictation and preview, and 600 seconds for saved-note
requests to allow time for uploads on slower connections. Results use the
returned text and the locally known offsets. Missing or rejected keys,
timeouts, network failures, oversized audio and empty results have distinct
error handling. A failed OpenRouter Turn does not cause a second upload of the
full Source. The captured account must still be configured before each upload.
[OpenRouter speech-to-text guide](https://openrouter.ai/docs/guides/overview/multimodal/stt),
[OpenRouter transcription API](https://openrouter.ai/docs/api/api-reference/stt/create-transcription)

## 2026-11-01: Bound audio admission after preparation

Agent starts carry a cloned host turn authority through native preparation.
After microphone device preparation, the final capture and watchdog locks are
held while the host checks authority and the exact pending request, calls
`stream.play()`, and publishes the recording. A revoked or abandoned start
cannot cross this boundary. Normal recorder controls retain their existing
unbound path.

System capture has its own effective boundary. Windows checks the host grant
after COM/device initialization immediately before `IAudioClient::Start`.
The macOS helper accepts `--admission <session-path>` for bound starts. It
prepares its graph, emits `prepared`, and waits for an exclusive host-created
grant file before `AudioDeviceStart`. The host rechecks its authority after
`prepared`; failure stops the prepared helper without granting capture. A
stale grant must be removed successfully (absence is allowed), and publication
uses `create_new` so it never overwrites an unexpected grant. A missing or old
helper fails closed through the side-effect-free `--admission-protocol` probe,
which runs without `--output`. Probing does not start a second capture.
The grant wait is bounded to 30 seconds. Existing abort escalation can kill a
prepared helper if it does not respond to termination while waiting.

A recording already admitted and published belongs to the recorder. Late ACK
failure or subsequent chat Stop does not end that meeting or require a new
grant for helper graph recovery. Admission cancellation and stopping an active
recording are separate operations. The wire decision is recorded in the dated
[system helper ADR addendum](adr/0004-out-of-process-system-audio-helper.md).

Agent retry and recovery preserve the live turn authority across the per-note
processing queue. At dequeue they check the bound run, profile, note and exact
saved recording before polling the processing future. A consumed UI request
is not reused as the lifetime of a queued retry. Rejection leaves prior note
results unchanged. Dictation checks the same host authority after command
generation/process serialization and immediately before writing its helper
command. Portable tests exercise the production admission gates, SQLite queue
checks, filesystem grant and IPC write; macOS Swift/CoreAudio and Windows COM
runtime acceptance still require a supported native host.
