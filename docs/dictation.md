# Dictation

How one **dictation take** goes from the shortcut to pasted text, and what
keeps it quick. Names follow [CONTEXT.md](../CONTEXT.md); the decisions are
[ADR-0014](adr/0014-pinned-dictation-paste-target.md) (pinned paste target),
[ADR-0049](adr/0049-cancelled-dictation-takes-never-deliver.md) (cancelled
takes never deliver), and
[ADR-0065](adr/0065-dictation-calls-the-users-own-provider-directly.md)
(direct calls with the user's own key). The latency work is the openspec change
`ditado-rapido`.

## Path of a take

1. **Shortcut.** The dictation helper (`src-tauri/native/mac-dictation-helper/main.swift`
   on macOS) reports `shortcut_key_down`; `ShortcutActivationController` in
   `src-tauri/src/dictation.rs` answers `start_listening` with a take id. The
   same key-down also rewarms the network route when it has been idle for a
   minute (`clovy_api::warm_dictation_route_if_idle`).
2. **Capture.** The helper starts recording at once: microphone permission is
   only requested when it is not already granted, and the recorder for the
   next take is prepared after each take and at launch. The start cue plays in
   parallel and is cut from the recording before it is handed over (see
   [Start cue](#start-cue)). `listening_started` follows the key-down.
3. **Stop.** On release the helper pins the paste target, stops the recorder,
   and emits `recording_ready` with the `.m4a` path and the peak meter level.
   No speech analysis runs before the upload.
4. **Transcription** (`clovy_api::dictate_transcribe`). With the user's own
   Groq key it posts straight to `api.groq.com/openai/v1/audio/transcriptions`
   (model, `response_format=verbose_json`, the short dictionary hint as `prompt`, and
   `language`), carrying only that key. Without one it posts to Clovy API's
   `/v1/dictate`.
5. **No speech.** A take under 0.5 s of audio (`audioDurationSeconds` in
   `recording_ready`) is not uploaded. On the direct Groq route the request
   asks for `verbose_json` and segments with `no_speech_prob` of 0.6 or more
   are dropped, since Whisper invents text over silence. A transcript with no
   letters or digits left (empty or only punctuation) ends the take with
   `no_speech` and nothing is pasted. The meter level is only logged.
6. **Cleanup** (optional, off by default). Off, the pasted text is Whisper's
   transcript only trimmed: no filler removal and no style. With **Clean up dictation text** on
   in Settings > Style, the transcript goes to the chosen OpenRouter model in
   one chat completion (`clovy_api::dictate_cleanup`): straight to
   `openrouter.ai` with the user's own key, else Clovy API's
   `/v1/dictate/cleanup`. Transcripts above 800 bytes are split at word
   boundaries and up to four chunks are cleaned at once, then rejoined in
   order. Any failure or timeout pastes the whole raw transcript, and the HUD
   says "Pasting into {app} without cleanup". Cleanup never runs on an agent
   CLI or endpoint. The output style (standard, casual, formal) applies only
   while cleanup is on.
7. **Paste.** The helper writes the text to the clipboard and posts Cmd+V into
   the pinned target. When the target is already frontmost it pastes at once;
   otherwise it activates the target, polls every 20 ms for up to 1 s, and
   waits a short settle delay after activation lands.

## Settings

`dictation-settings.json` holds `cleanupEnabled` (default `false`),
`cleanupModel` (an OpenRouter catalog id, default
`nvidia/nemotron-3-nano-30b-a3b`), and `cleanupMigrated`. A file saved before
optional cleanup has no `cleanupMigrated`; on the first launch it is migrated
once to cleanup off, whatever engine cleanup used before (Clovy API, an
endpoint, or a CLI). The `dictationCleanup` selection in
`provider-settings.json` is kept on disk but always reads as Clovy API.

## Warm-up

At launch and on a key-down after a minute idle, Clovy reads the user's Groq
and OpenRouter keys, each on its own, into the Keychain store's per-account
cache (the same cache a key save or removal updates; there is no second one),
fetches the OS Accounts token when the take will use Clovy API, and opens the
transcription connection without sending audio: `GET /models` on Groq with
the user's key, else Clovy API's `GET /healthz`. The helper preloads the cues
and prepares the next recorder after every take. No audio is recorded outside
a take.

## Start cue

The start cue used to play before recording began, which kept Clovy's sound
out of the transcript but delayed capture by the cue's length (336 ms). Now
recording starts on the key-down and the cue plays at the same time; when the
recording stops, the helper cuts the cue's span from the start of the file
with a passthrough export (no re-encoding) before `recording_ready`. If the
export fails, the uncut file is used. See the design of `ditado-rapido` (D3)
for why this technique was chosen.

## Latency targets

From the `dictation-events.log` of the app being measured:

| Interval | Target |
| --- | --- |
| `shortcut_key_down` to `listening_started` | 150 ms (300 ms on the first press after launch) |
| `shortcut_key_up` to `paste_completed`, take up to 10 s, cleanup off, own Groq key | 1.5 s |
| `paste_target` to `paste_completed`, target already frontmost | 50 ms |

## Tests

- `cargo test dictation` - settings migration, the pipeline with a fake
  transcriber and cleaner (cleanup off, on, failed, timed out, parallel
  chunks), and `no_speech` on an empty transcript.
- `cargo test dictation_route_tests` - fake Groq, OpenRouter, and Clovy API
  hosts: requests per destination, only the called provider's key, warm-up.
- `cargo test llm::registry` - the registry refuses a CLI or endpoint for
  dictation cleanup.
- `src/test/dictation-cleanup-settings.test.tsx` - the cleanup switch and the
  OpenRouter-only model picker.
