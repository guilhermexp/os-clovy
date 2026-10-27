---
status: accepted
date: 2026-10-27
amends: 0065
---

# Notes call the user's own provider directly

## Context

ADR-0065 let dictation go straight from the desktop to Groq and OpenRouter
with the user's own key, and kept everything else on Clovy API (§5: "Note
transcription, note generation, chat, media, and web keep their Clovy API
routes with BYOK forwarding").

The published v0.1.2 does not ship Clovy API, and the desktop's default
`CLOVY_API_URL` is `http://127.0.0.1:8080`. On the owner's Mac and Windows
installs every meeting recording ended in a failed note reading "System:
error sending request for url (http://127.0.0.1:8080/v1/notes/transcribe) |
Microphone: ...", while dictation, already direct, worked with the same keys.

With a user key, Clovy API only relays note requests: no Clovy credits move,
and the request it sends upstream is fixed (Groq transcription parameters,
the note generation prompt, the note transcript cleanup prompt at
temperature 0). The two options were to bundle Clovy API inside the desktop
or to extend the direct route. The owner chose the direct route on
2026-10-27.

## Decision

1. **§5 of ADR-0065 now also excludes notes.** With the notes engine set to
   Clovy and the user's key saved, note transcription (final turns, the
   per-source fallback, and live preview) posts to Groq, and note generation
   and note transcript cleanup post one chat completion to OpenRouter.
   Without the key, each call goes through Clovy API exactly as before.
2. **The direct request is the one Clovy API makes.** Transcription keeps
   `response_format=json` (dictation's `verbose_json` filtering does not
   apply), the canonical `audio.wav` / `audio.m4a` file name, the dictionary
   prompt, the language, the 25 MB audio cap, and one retry of a transient
   failure. Generation and cleanup use Clovy API's own prompt files,
   compiled in from `clovy-api/crates/services/src/prompts/`.
3. **Auto stays on Clovy API.** `open-software/auto` is a selection Clovy API
   resolves, so it never takes the direct route.
4. **A request carries only the called provider's key** (ADR-0065 §2), and
   service-managed keys still live only in Clovy API (§3).
5. **Without a key and without a reachable Clovy API, the note names the
   missing key** (Groq for transcription, OpenRouter for generation) and
   where to save it, in the interface language, instead of the HTTP
   library's error. Retry stays available.
6. Clovy API's v1 routes and contracts do not change (ADR-0021).

## Consequences

- A packaged desktop with saved Groq and OpenRouter keys writes complete
  notes without running Clovy API.
- Clovy API no longer sees BYOK note requests; its logs and metrics
  undercount them, as with dictation.
- Endpoint and CLI note engines are unaffected.
- A user on Auto without Clovy API still gets a failed generation; choosing
  an explicit model is the remedy.
- Chat, media, and web keep their Clovy API routes; extending the direct
  route to them needs its own decision.
