# Local development

Day-to-day development reference for the desktop app and a local Clovy API.
Configuration details (every env var, pricing, custom models, connected mode)
live in [configuration.md](configuration.md); when the two disagree, the env
example files win.

## Checks in nested worktrees

Biome excludes the checkout's own `.tmp` with the root-relative `!!.tmp`
pattern. Keep this exclusion relative to the project root: `!!**/.tmp` also
prunes a checkout located under an ancestor `.tmp/worktrees`, causing
`pnpm check` to process zero files. The local temporary evidence directory
remains excluded while the source files are checked normally.

## Quick start

Clone the repo, copy both env examples, add the service keys for the providers
you want to exercise, and run the desktop app:

```sh
cp .env.example .env
cp clovy-api/.env.example clovy-api/.env
# Edit clovy-api/.env and set the keys you need: CLOVY__UPSTREAMS__GROQ__API_KEY
# (transcription), CLOVY__UPSTREAMS__OPENROUTER__API_KEY (remote text),
# CLOVY__UPSTREAMS__FAL__API_KEY (images and video), CLOVY__UPSTREAMS__TAVILY__API_KEY (web).
pnpm install
pnpm tauri:dev
```

`pnpm tauri:dev` starts Vite and a local Clovy API when their ports are free.
If `127.0.0.1:1421` or `127.0.0.1:8080` is already listening, the script
reuses the existing service. Set `VITE_PORT` or `CLOVY_API_PORT` to choose a
different port. Set `CLOVY_DEV_SKIP_LOCAL_API=1` to skip the local Clovy API
entirely and leave the port probe alone; the staging and ephemeral targets
below already do this.

Replay first-run onboarding without wiping all app data:

```sh
pnpm tauri:dev --replay-onboarding
```

You can also run Clovy API directly:

```sh
(cd clovy-api && cargo run -p clovy-api-server -- serve)
```

Restart `pnpm tauri:dev` after changing the root `.env`. The running Tauri
process does not reload client configuration.

The desktop app has no sign-in, billing or credit charges: it always runs on a
fixed local identity (`src-tauri/src/local_identity.rs`), user
`OS_CLOVY_LOCAL_DEV_USER_ID` (default `usr_local_dev`) and bearer token
`OS_CLOVY_LOCAL_DEV_BEARER_TOKEN` (default `local-dev-token`), and calls Clovy
API at `http://127.0.0.1:8080` unless `CLOVY_API_URL` says otherwise. The
example env files run Clovy API in local mode, which accepts that bearer token.
The token must match in `.env` and `clovy-api/.env`; it is not an OS Accounts
token, just the shared secret between the local desktop app and the local Clovy
API. No provider keys go in the desktop env. The Clovy
API env example binds local mode to `127.0.0.1`; if you bind it to a network
interface, replace the default local bearer token in both env files first.

Provider keys and the OS Accounts App API key belong only in `clovy-api/.env`,
never in the root desktop `.env`. Outside local mode Clovy API refuses to start
without all four service keys. In local mode a blank or placeholder key is
allowed: requests to that provider then need the user's own key, sent by the
desktop app, and fail with `<provider>_not_configured` without one. A missing
service key does not remove a model from the `/v1/models` catalog: Groq and
OpenRouter models report `serviceAvailable: false` instead.

## Local connector OAuth

Connector OAuth application identifiers belong in the root desktop `.env`.
They identify the installed client but do not grant access to user data by
themselves; user grants stay in Keychain.

Create a Google Desktop OAuth client and set `GOOGLE_OAUTH_CLIENT_ID` and
`GOOGLE_OAUTH_CLIENT_SECRET`. Restart `pnpm tauri:dev` after changing either
value.

## Running against hosted Clovy API

Two ways to run the desktop app against a Clovy API in a real TEE instead of
the local one: the shared staging deployment, or a disposable Phala CVM built
from your working tree.

### Staging

```sh
make dev-staging
```

This fork's desktop app cannot use staging. Staging Clovy API boots with
`CLOVY__LOCAL_DEV__ENABLED` unset, so it accepts only OS Accounts JWTs and
charges credits, and the desktop no longer signs in to OS Accounts: it sends
only the local identity's bearer token, which staging rejects. The target still
runs `pnpm tauri:dev` with `CLOVY_API_URL` pointing at staging, but requests
fail authentication.

`https://june-api-staging.opensoftware.co` is the compatibility hostname served by a `dstack-ingress`
container inside the staging CVM. It terminates TLS on 443 and proxies to
`clovy-api:8080` over the internal compose network; the app publishes no
external port of its own.

Phala sealed envs are full-replacement on `phala envs update`: every variable
must be re-supplied on each update, including the ingress ones. Seal new
variables BEFORE deploying a compose that references them; containers boot
against whatever is already sealed, so the reverse order boots the ingress
without its Cloudflare credentials. Read the comments in
`clovy-api/deploy/docker-compose.staging.yml` before touching them.

### Ephemeral Phala CVM

Deploy the clovy-api in your working tree to a disposable Phala CVM, use it,
delete it. Backed by `scripts/ephemeral-clovy-api.sh`.

```sh
make ephemeral-api            # deploy, health-check, print the URL; leaves it up
make dev-with-ephemeral-api   # deploy, run the app against it, delete on exit
make ephemeral-api-down       # delete the CVM recorded in the state file
```

`ephemeral-api` builds clovy-api for `linux/amd64`, pushes it to `ttl.sh`,
deploys a `tdx.small` CVM, and polls `/healthz` for up to 10 minutes. On
timeout it exits non-zero and leaves the CVM up and billing, so tear it down
by hand. `dev-with-ephemeral-api` deploys a fresh CVM, runs `pnpm tauri:dev`
against it, and always deletes it on exit, whether you quit cleanly, hit
Ctrl-C, close the terminal, or the run fails after the CVM came up. A hard
kill or power loss can still skip cleanup; `make ephemeral-api-down` recovers
from the state file, so run it if a session ended abnormally. That is the
invariant worth remembering: only `dev-with-ephemeral-api` cleans up after
itself. `ephemeral-api` leaves the CVM running until `ephemeral-api-down`.

Prerequisites: Docker running, the `phala` CLI installed and authenticated
(`phala auth login`), and a `clovy-api/.env` holding the upstream provider
keys. The script also needs `jq`, `curl`, `openssl`, `perl`, and `uuidgen`. It
copies `CLOVY__UPSTREAMS__GROQ__API_KEY`, `CLOVY__UPSTREAMS__OPENROUTER__API_KEY`,
`CLOVY__UPSTREAMS__FAL__API_KEY`, and `CLOVY__UPSTREAMS__TAVILY__API_KEY` from
`clovy-api/.env` into the CVM's sealed env verbatim, falling back to the
`JUNE__UPSTREAMS__<PROVIDER>__API_KEY` alias when the Clovy name is missing or
empty. Leftover Venice and OpenAI keys are not copied. A quoted value or one
with a trailing comment stops the script before the deploy, because the sealed
env would keep the quotes or comment as part of the key. A key missing under
both names stays missing; the CVM runs in local mode, so that provider then
works only with the user's own key, as described above.

Cost: `tdx.small` bills $0.058/hr from creation until you delete the CVM. The
image is pushed to `ttl.sh` with a `4h` tag. Tags there expire; the CVM keeps
running past expiry, but it can no longer re-pull its image, so a restart
after that point is fatal. Treat an ephemeral CVM as dead once its tag
expires.

Security: the ttl.sh image is briefly public and carries no secrets. Secrets
ride Phala sealed env and are injected at boot inside the CVM, and the deploy
passes `--no-public-logs --no-public-sysinfo`. Auth is local-dev mode with a
random bearer token minted per run, never printed, and gone when the VM is
deleted. Local-dev mode replaces JWT verification and disables OS Accounts
metering, so no OS Accounts App API key ever reaches an ephemeral CVM.

The CVM name, URL, bearer token, image ref, git sha, and creation time land in
`.ephemeral-clovy-api.json` (mode 600, gitignored). It is written before the
deploy starts, because a deploy that dies halfway can still leave a billing
CVM behind and the state file is the only record of its name. `ephemeral-api`
refuses to run while that file exists. To point a manual session at a CVM left
up by `ephemeral-api`:

```sh
export CLOVY_API_URL="$(jq -r .url .ephemeral-clovy-api.json)"
export OS_CLOVY_LOCAL_DEV_BEARER_TOKEN="$(jq -r .token .ephemeral-clovy-api.json)"
export CLOVY_DEV_SKIP_LOCAL_API=1
```

Ephemeral CVMs get no `dstack-ingress` and no custom domain. The dstack
gateway's own HTTPS endpoint for the published port is the only way in.

## Local data

The app data directory is resolved by Tauri at runtime. In development, inspect
the platform app data path for:

- `notes.sqlite3`
- `recordings/{note_id}/{session_id}.wav`
- `recordings/{note_id}/{session_id}/microphone.wav`
- `recordings/{note_id}/{session_id}/system.wav` when `Microphone + system audio`
  is selected

Saved audio is the source of truth for retry. If transcription or generation
fails after capture, Clovy keeps the audio and processing metadata so work can be
retried without recording again.

## Agent skills

The agent loads skills from its managed `skills` folder and, when the folder
exists, from `~/.agents/skills` in your home directory (the same location the
`skills` CLI installs into). Drop a skill folder there and every agent session
picks it up the next time it starts. Home-folder skills load read-only: the
macOS write-jail grants writes only under Clovy's own data directory, so the
agent can use these skills but cannot modify them.

## Permissions

Clovy asks for permissions only where the feature needs them:

- **Microphone:** required for meeting notes and dictation.
- **Accessibility:** required for dictation paste into the previously focused
  app and for activity capture.
- **Screen and system audio recording:** required when using
  `Microphone + system audio` on macOS, for activity text recognition, and for
  Computer use captures.
- **File access:** requested by agent workflows when a task needs a specific
  scope.

The macOS bundle includes `NSMicrophoneUsageDescription`,
`NSAudioCaptureUsageDescription`, and `NSScreenCaptureUsageDescription` in
[src-tauri/Info.plist](../src-tauri/Info.plist).

### The macOS development app

`pnpm tauri:dev` (and `make dev`) runs the app from a signed bundle beside
Cargo's output, so macOS can show it in System Settings and keep its grants
across restarts and rebuilds
([ADR-0060](adr/0060-development-app-runs-as-a-signed-bundle-that-owns-its-grants.md)).
The launcher never resets permissions.

- **Bundle:** `src-tauri/target/debug/Clovy Dev.app`, bundle identifier
  `<identifier>.dev` (`co.opensoftware.june.dev` on a normal branch). The
  compiled Tauri identifier, and with it the `<identifier>-dev` data
  directory, does not change. A branch identity (`codex/jun-278-…`) or
  `CLOVY_DEV_APP_IDENTIFIER` gets its own bundle, for example
  `Clovy Dev permproof.app` for `co.opensoftware.june.permproof`. The browser
  extension's native messaging host follows the compiled identity too: the
  shim built with it reads only the release app's descriptor or the one in
  `<identifier>-dev`, never another profile's.
- **Bundle contents:** besides `os-june`, `clovy-mcp`, and `clovy-nm-shim`,
  the bundle carries the resources `tauri.conf.json` declares for every
  platform (browser extension files, agent skills) at their bundle paths, and
  rebuilds when they change. Helpers are not copied in: debug builds resolve
  the Computer use, dictation, and system audio helpers from `.tauri-helper`
  at compile time, since the bundle runs without Cargo's runtime environment.
- **Signing identity:** `CLOVY_DEV_SIGNING_IDENTITY` (a SHA-1 or exact name
  from `security find-identity -v -p codesigning`), otherwise the single valid
  Apple Development identity. Several Apple Development identities, or none,
  stop the launch with the next step to take; nothing falls back silently.
  `CLOVY_DEV_SIGNING_IDENTITY=-` opts into ad hoc signing, which macOS ties to
  the exact code hash: every rebuild then asks for each permission again. In
  these debug builds the selected identity also wins over an
  `APPLE_SIGNING_IDENTITY` inherited from the shell; release builds keep
  signing with `APPLE_SIGNING_IDENTITY`.
- **One certificate everywhere:** the same identity signs `Clovy Dev.app`
  (with `clovy-mcp` and `clovy-nm-shim` beside `os-june` in
  `Contents/MacOS`) and the helpers in `.tauri-helper` (Computer use driver,
  dictation helper, system audio helper). Their designated requirement is
  `identifier "<bundle id>" and anchor apple generic and certificate
  leaf[subject.CN] = "<certificate>" and …`, so a rebuild changes the code
  hash but not the requirement macOS checks a grant against. Changing the
  certificate or the bundle identifier is a new identity for macOS.
- **Reuse:** an unchanged Cargo output reuses the signed bundle as is, and
  unchanged helpers keep their bundles and signatures. A changed binary
  replaces the bundle at the same path. Switching between a certificate and
  `CLOVY_DEV_SIGNING_IDENTITY=-` re-signs the app and the helpers in the new
  mode even when their content is unchanged. Plain `cargo build` and
  `cargo test`, which select no identity, keep any valid helper signature.
- **First authorization:** macOS asks once for each permission the dev app
  uses, or add `Clovy Dev.app` with + in System Settings > Privacy & Security.
  When the checkout lives in a protected folder (`~/Documents`, `~/Desktop`,
  `~/Downloads`), macOS may also ask for access to that folder, possibly before
  the main window appears. Check which request macOS actually shows and the
  state under Files and Folders; whether to allow it is the owner's decision.
  Folder access alone does not make the app ready. macOS may also ask once to
  let the newly signed app read an existing `-dev` Keychain item; that is an
  authorization to read the item, never an unlock or change of the Keychain,
  and Always Allow is recorded against the same requirement.
- **Web storage:** the unbundled app kept its WebKit storage in
  `~/Library/WebKit/Clovy`; the first bundled launch copies it to
  `~/Library/WebKit/<bundle id>` and leaves the original in place.

What macOS sees in development:

| Component | Identity | Started by |
| --- | --- | --- |
| `Clovy Dev.app` | `<identifier>.dev` | the runner, responsible for its own requests |
| `.tauri-helper/June Dictation Helper.app` | `co.opensoftware.june.dictation-helper.dev.w<worktree>` | Clovy Dev, as a child process |
| `.tauri-helper/June.app` (system audio) | `co.opensoftware.june.audio-capture.dev.w<worktree>` | Clovy Dev, through LaunchServices |
| `.tauri-helper/June Computer Use Driver.app` | `co.opensoftware.june.computer-use-driver.dev.w<worktree>` | Clovy Dev, through LaunchServices |

`w<worktree>` is the first 12 hex digits of the SHA-256 of the checkout path,
the same key for the three helpers, so helper grants never land on an
installed release or on another worktree. Release builds keep the published
identifiers (`co.opensoftware.june.dictation-helper`,
`co.opensoftware.june.audio-capture`,
`co.opensoftware.june.computer-use-driver`). Requests Clovy Dev makes itself
(microphone for notes, Accessibility and Screen Recording for activity
capture, notifications) are listed as Clovy Dev. Computer use Accessibility
belongs to the driver and its Screen Recording to the outer responsible app,
Clovy Dev
([ADR-0028 addendum](adr/0028-private-stdio-broker-for-computer-use.md)).

macOS attributes a privacy request to the process responsible for it, and a
process started from a terminal inherits the terminal as responsible. The
runner therefore starts the bundle through `clovy-dev-launch`
([src-tauri/native/dev-launcher](../src-tauri/native/dev-launcher/clovy-dev-launch.c)),
which execs the app in place with the terminal's responsibility disclaimed.
The process id, arguments, environment, stdio, and exit status stay the ones
Tauri dev supervises. It uses `responsibility_spawnattrs_setdisclaim`, which
libSystem exports but Apple does not document; if a macOS release removes it,
the launch fails with a message instead of inheriting the terminal's grants.

To reset a development grant on purpose, name the dev bundle, never the
release identifier:

```sh
tccutil reset Microphone co.opensoftware.june.dev
```

## Verification commands

```sh
pnpm check
pnpm typecheck
pnpm test
pnpm test:rust
pnpm test:clovy-api
pnpm build
pnpm tauri:build
```

`make verify` mirrors CI.

Useful validation docs:

- [specs/001-tauri-note-mvp/manual-validation.md](../specs/001-tauri-note-mvp/manual-validation.md)
- [specs/002-system-audio-source-mode/quickstart.md](../specs/002-system-audio-source-mode/quickstart.md)
- [specs/003-conversation-turns/quickstart.md](../specs/003-conversation-turns/quickstart.md)

## Dictation destination HUD verification

The native path is `src-tauri/native/mac-dictation-helper/main.swift`
(`FocusTargetController` and `DictationController`) →
`src-tauri/src/dictation.rs` (take correlation and `latest_hud_snapshot`) →
`src/hud.ts` / `hud.html` / `src/styles/hud.css`. The existing
`src-tauri/src/activity/app_icon.rs` command resolves local PNG icons, through
`activityAppIcon` in `src/lib/activity-timeline.ts`; custom app commands use
the existing Tauri command registration, so no new window capability is
required. `focus_target` is visual metadata, not a workflow start or finish.
The replay snapshot retains a listening lifecycle across meter/focus updates
and carries the pinned target forward through processing. No per-level icon
lookup or PNG logging is added. See [ADR-0014](adr/0014-pinned-dictation-paste-target.md).

A live `final_transcript` without `target` may supersede the startup snapshot.
In that case `paste_target` can still hydrate the authoritative destination.
Focus updates are accepted only during listening, so this hydration window
cannot replace the destination with an unrelated foreground app. An explicit
target (including null) or a previously pinned destination stays authoritative.

`src/test/hud-target.test.ts` tests the actual HUD markup and consumer across
candidate changes, stop, paste, error, cancellation, delayed icons and legacy
fallback. `i18n-hud.test.ts` covers labels and late-subscriber hydration;
`hud-target-hydration.test.ts` covers a missed stop snapshot followed by live
transcript/paste events without changing the original RED witness;
`dictation.rs` tests snapshot reduction. Browser smoke with a controlled Tauri
bridge proves rendering and control requests only, not Swift focus, Fn,
permissions or paste. Keep screenshots and the bridge script in the run's
ignored evidence folder; never ship the shim.

Native proof belongs to the operator of an isolated debug identity. Build,
but do not launch, with the official Node 24 PATH and a run-owned identifier:

```sh
PATH="$HOME/.local/share/node-official/node24/bin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin" \
  pnpm tauri:build -- --debug --bundles app \
  --config src-tauri/tauri.macos.conf.json \
  --config '{"identifier":"co.opensoftware.june.targetproof20261012","bundle":{"createUpdaterArtifacts":false}}'
```

This compiles a separate data identity; editing Info.plist is insufficient.
The release build receipt is packaging evidence, not the binary to open for
native proof. Follow AGENTS → Verification: evidence-owned copy of the bundle,
normal HOME, isolated copied profile excluding live MCP sockets, and `lsof`
confirmation of that exact process's database paths before interactions. The
worker does not copy the owner's profile or open any native app. The operator
must stop if protected-folder, microphone, Accessibility or Keychain access is
missing; only the owner decides those prompts. Never alter TCC or Keychains.

A separate bundle/data identity does not isolate global Fn listeners. The
owner's dev helper and the proof helper can react to the same key. Before Fn
proof, the operator must isolate shortcut activation using supported controls,
or temporarily suspend only the dev instance started by Main for this proof.
Record which exact process/listener was suspended and its restoration; do not
stop an unrelated owner's instance or assume the proof bundle owns Fn. Restore
the prior activation/dev state after the run. The worker neither suspends dev
processes nor exercises Fn. If activation cannot be isolated within this
authority, record the native Fn proof as blocked.

Use two run-owned scratch text destinations and a third app for focus drift:

1. With existing permissions and shortcut activation isolated as above,
   start Fn dictation in A. Observe A's real icon/name on the first useful HUD
   frame; switch to B while listening and observe B. Record helper events with
   the same takeId, without audio, transcript or credential content in evidence.
2. Release Fn or click Stop in B, then focus C. Observe the correlated
   `finalizing_transcript.target` for B, and B beside the processing spinner.
   If a local provider stub is used to avoid remote inference, label it as
   such; it must return synthetic text through the real transcription path,
   never emit helper/HUD events or paste commands as a substitute for proof.
   Without an authorized configured route, full processing/paste remains
   unproven. Do not change provider settings in the owner's profile.
3. If authorized transcription is available, verify only B receives the text,
   and `paste_target` and the visible indicator agree. Close B after stop in a
   separate take: observe B on the error, clipboard recovery, and no paste in C.
4. Cancel with Esc and the HUD cancel button in separate takes. Start a new
   take in A immediately; verify no B icon or delayed delivery reappears.
   Check drag, Stop, Esc hint, EN/pt-BR tooltip and accessible destination name.
5. Capture the real HUD and read back the isolated take outcome using the
   app's own supported export/read path. Check run-owned autostart cleanup by
   exact ProgramArguments, and stop only this run's processes. Preserve the
   evidence report's explicit limits (native, liveGroq, Keychain, deployment).

A fresh debug identity may be blocked on permissions; that is a recorded
blocker, not permission to use release data or synthesize native success.

## Releases

Production desktop releases are cut from GitHub Actions. macOS produces signed
and notarized DMGs with Tauri updater artifacts. Windows produces signed NSIS
installers and merges Windows updater metadata into the shared release. Start
with:

- [release-macos.md](release-macos.md)
- [release-windows.md](release-windows.md)
- [reproducible-builds.md](reproducible-builds.md)

Changes to the local agent sidecar or its OpenAI Agents SDK pin must pass
`pnpm agent-runtime:typecheck`, `pnpm agent-runtime:test`, and
`pnpm agent-runtime:build`. Release jobs package the result as a signed Node 24
single executable and verify its checksum and startup from a path with spaces.
