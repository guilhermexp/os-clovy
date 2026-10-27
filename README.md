<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="public/clovy-wordmark.svg">
    <img src="public/clovy-wordmark.svg" alt="Clovy" width="160">
  </picture>
</p>

<h3 align="center">Private AI on your Mac</h3>

<p align="center">
  Clovy brings chat, voice dictation, meeting notes, and a local agent into a single
  private workspace. Local by default, with remote inference routed through a
  Clovy API you run yourself, and open source so the privacy claims can be
  checked instead of believed.
</p>

<p align="center">
  <a href="LICENSE">
    <img alt="License: MIT" src="https://img.shields.io/badge/license-MIT-blue">
  </a>
</p>

## Why Clovy

Most AI apps ask you to hand over your most sensitive data and trust them with
it. Every prompt, file, and meeting reveals something about you, and a cloud
agent with that reach is a remote company's window into your work.

Clovy is built the other way around. The app and the agent run on your Mac.
Notes, recordings, transcripts, files, sessions, and agent memory stay on your
machine by default. When Clovy needs remote inference, the request goes
through Clovy API, an open source service that runs on your own machine,
keeps provider keys out of the desktop app, and routes each capability to its
provider: Groq for audio, OpenRouter for text, fal for images and video, and
Tavily for web. What a provider retains depends on that provider and route.
You do not have to take any of this on faith: the entire product is MIT
licensed.

## What Clovy does

- **Chat.** Ask questions, do research, brainstorm, and build plans.
- **Dictation.** Hold a key, talk, release. Clovy turns your voice into clean,
  polished writing and pastes it into whatever app you were using, with
  push-to-talk and hands-free modes and selectable writing styles.
- **Meeting notes.** Clovy detects supported meetings and offers to take notes,
  without a bot joining the call. It records microphone or microphone plus
  system audio, orders the transcript into conversation turns, and generates
  editable notes. Saved audio is kept so failed steps can be retried without
  recording again.
- **Agent.** A Clovy-owned local agent built on the OpenAI Agents SDK that
  helps with files, research, and drafts. Sessions are sandboxed by default
  and risky actions wait for your approval. Extend it with skills.
- **Image generation.** Create images from a prompt. Clovy API sends image
  requests to fal.
- **Your choice of models.** Pick generation, transcription, and dictation
  models from the live catalog. A model shows a privacy tier only when the
  catalog states one. Bring your own Groq, OpenRouter, fal, or Tavily key in
  Settings if you prefer.

<table>
  <tr>
    <td width="50%">
      <img src=".github/assets/clovy-meeting-notes.jpg" alt="A meeting note in Clovy with a live transcription preview while recording">
    </td>
    <td width="50%">
      <img src=".github/assets/clovy-agent-analysis.jpg" alt="A Clovy agent session in private mode, reporting its analysis of a spreadsheet">
    </td>
  </tr>
  <tr>
    <td align="center">Meeting notes with live transcription, no bot in the call</td>
    <td align="center">The agent working through a spreadsheet in private mode</td>
  </tr>
</table>

## How Clovy keeps it private

1. **Local by default.** App state, recordings, transcripts, and agent memory
   live on your machine. The agent runs locally inside a macOS write-jail
   unless you opt a session out.
2. **Provider-dependent model privacy.** Clovy API sends audio to Groq, text
   to OpenRouter, images and video to fal, and web search and fetch to Tavily.
   What a provider retains or trains on depends on that provider and route.
   A model without a stated privacy tier makes no retention claim; unknown
   does not mean zero retention.
3. **No accounts, no billing.** Clovy API runs locally with a single bearer
   token. There is no sign-in and no metering; prompts, audio, transcripts, and
   files are not stored by Clovy API.
4. **Verifiable, not promised.** The desktop app and Clovy API are built from
   this repository, so you can read and build the exact code you run. Model
   privacy remains explicit provider evidence: Groq, OpenRouter, fal, and
   Tavily retain what their own policies and routes allow.

## Platforms

Clovy runs on macOS 14 or later, Apple Silicon and Intel. There are no
published releases: build it from source (see below).

Windows builds cover the app shell, sign-in, microphone recording, notes, and
the bundled agent runtime. On Windows 10 1903+ and Windows 11 x64 they also
include encrypted text-only [activity capture](docs/activity-capture.md), the
[activity timeline](docs/activity-timeline.md), local coding-agent session
ingestion, and CPU embeddings for day intelligence. Global dictation paste,
system audio capture, and the macOS sandbox remain macOS-only. macOS is the
primary target.

## Repository layout

This repo contains the full product: the desktop app and the service that
powers its remote AI calls.

```text
src/         React and TypeScript frontend
src-tauri/   Tauri v2 Rust desktop backend and native helpers
clovy-api/    Clovy API: models, transcription, generation, and web (local only)
docs/        Architecture notes, ADRs, subsystem guides, and runbooks
spec/        Enforceable coding rules
specs/       Feature specs, plans, and validation notes
```

Clovy's service keys for Groq, OpenRouter, fal, and Tavily live only in
Clovy API, never in the desktop app or its
`.env`. A key you bring for one of those providers is saved from Settings in
the system credential store (the Keychain on macOS) and sent to Clovy API only
with that provider's requests. Start with [docs/index.md](docs/index.md) for
the full doc map, [CONTEXT.md](CONTEXT.md) for the domain glossary, and
[AGENTS.md](AGENTS.md) for the contributor guide.

## Build from source

You need Node.js with pnpm 11 and a Rust toolchain. The exact pnpm version is
pinned in `package.json`.

Windows native builds also need Visual Studio C++ Build Tools, Strawberry Perl,
and NASM on `PATH` for vendored OpenSSL and SQLCipher. The Windows build script
[`scripts/build-windows.ps1`](scripts/build-windows.ps1) checks Perl and NASM
before Cargo starts. Activity data uses the system credential store and remains
encrypted on both supported platforms.

```sh
git clone https://github.com/guilhermexp/os-clovy
cd os-clovy
cp .env.example .env
cp clovy-api/.env.example clovy-api/.env
# Edit clovy-api/.env and set the CLOVY__UPSTREAMS__*__API_KEY service keys you need.
pnpm install
pnpm tauri:dev
```

Clovy API runs only in local mode: no login, no billing, bound to
`127.0.0.1`, and authenticated by a shared bearer token.
Service keys belong only in `clovy-api/.env`, never in the root desktop
`.env`: Groq for transcription and dictation, OpenRouter for text (chat,
notes, dictation cleanup), fal for images and video, and Tavily for web search
and fetch. A provider without a service key still works with your own key
for it, saved in Settings. With your own Groq and OpenRouter keys, dictation
and notes call those providers directly, so a packaged app needs no Clovy API
for them.

See [docs/development.md](docs/development.md) for the day-to-day development
guide (ports, onboarding replay, local data, permissions, agent skills, and
test commands) and [docs/configuration.md](docs/configuration.md) for the full
configuration reference, including exposing your own models.

## Contributing

Fork integration history: [versioned records](.upstream-sync/history/).

Clovy is also the canonical technical name for repository-controlled packages,
crates, environment variables, workflows, and release artifacts. Released
June-era inputs and rollback outputs remain compatibility aliases, while the
installed bundle, executable, updater, and permission identities stay stable.
See [ADR-0055](docs/adr/0055-clovy-technical-identity-migrates-through-a-compatibility-bridge.md).

```sh
pnpm check         # lint and format (Biome)
pnpm typecheck
pnpm test          # frontend (Vitest)
pnpm test:rust     # desktop Rust
pnpm test:clovy-api # Clovy API
```

`make verify` mirrors CI. Start with [CONTRIBUTING.md](CONTRIBUTING.md), then
[AGENTS.md](AGENTS.md) for the full contributor guide; the enforceable UI
rules live in [spec/](spec/index.md). Report bugs through GitHub issues, and
report security vulnerabilities privately per [SECURITY.md](SECURITY.md).

## License

Clovy is MIT licensed. See [LICENSE](LICENSE). Bundled third-party runtime
notices are tracked in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
