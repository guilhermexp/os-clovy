# Computer use support runbook

Computer use has independent gates: Clovy's stored grant, the Clovy API rollout
decision, macOS access for the signed helper, a vision-capable model, and a
visible attended turn. No account or plan is required: the plan gate always
passes since the desktop has no sign-in. Do not treat one as proof that the others
are enabled.

The shipped helper bundle names and development executable aliases documented
below retain their June-era technical identities under
[ADR-0055](adr/0055-clovy-technical-identity-migrates-through-a-compatibility-bridge.md).

## State guide

| State | Meaning | Action |
| --- | --- | --- |
| `unsupported` | This is not macOS. | Computer use is not shipped on Windows. |
| `rollout_disabled` | Clovy API disabled this app/macOS version, or the first safety decision could not be fetched. | Keep the grant revocable, check connectivity, then inspect Clovy API's `[computer_use]` config. Do not bypass the gate. |
| `plan_required` | Kept in the state type but no longer reached: the plan check always passes without an account. | None. |
| `off` | The Clovy grant is off. The helper may still have macOS grants. | Enable from Plugins after reading the permission explanation. |
| `driver_missing` | The signed app does not contain the pinned helper/stamp. | Reinstall or update Clovy. Do not run an upstream installer. |
| `driver_mismatch` | The helper version does not match the repo pin. | Stop rollout and inspect the build provenance/SBOM. |
| `permission_missing` | Accessibility, Screen Recording, or the live capture probe is absent. | Use Continue to macOS access, then the separate Open settings links. |
| `model_unsupported` | The selected generation model lacks authoritative vision capability. | Choose a vision-capable model. |
| `ready` | Every runtime gate is satisfied. | Start an attended agent task. |
| `error` | The helper could not start, handshake, or probe. | Stop the task, inspect logs and signature, then run the self-test. |

## User recovery

1. Press Stop current task before changing permissions or reinstalling.
2. Confirm the intended helper exists in the installed app at
   `Contents/Resources/native/bin/June Computer Use Driver.app` and has bundle
   identifier `co.opensoftware.june.computer-use-driver`. A packaged helper
   also requires the signed outer Clovy app with the same Developer ID team.
3. In System Settings > Privacy & Security, inspect Accessibility and Screen
   Recording separately. Accessibility names `June Computer Use Driver`;
   Screen Recording names the outer `June` app because macOS assigns capture to
   the responsible launcher. If either entry is absent, drag the matching card
   from Clovy into that pane: the helper card for Accessibility, or the outer
   Clovy card for Screen Recording. Removing a macOS grant may require Clovy to
   be quit and reopened before macOS reports the new state.
4. Return to Clovy. The visible setup page polls one signed-helper probe at a
   time while incomplete and reconfigures the runtime when the helper becomes
   capturable. Polling pauses while the page is hidden and refreshes when it
   becomes visible again.
5. If the driver crashed, Stop clears its private child and the next eligible
   task starts a new one. Never start an upstream daemon beside Clovy.
6. The first access to each target app asks once for access during the current
   task. Click Allow for this task. Captures and actions in that verified app do
   not ask again until the task ends or Stop is pressed. If Clovy later asks to
   open that same already-approved running app by display name, the broker
   reuses its exact verified identity instead of showing a second alias prompt.
7. A Stage Manager shelf window is restored automatically after the app is
   allowed. Clovy first activates its own window, then raises the exact target
   window into Clovy's current group without a separate approval. If restore
   fails, Clovy stops retrying that window for the current task. Press Stop and
   start a new task rather than switching Spaces manually.
8. If the target changed while an authorization card waited, ask Clovy to retry
   the app. A stale-target failure is expected safety behavior.

Turning the Clovy switch off removes Clovy's runtime grant and immediately stops
the task. It cannot silently remove macOS TCC entries; the user removes those
from System Settings.

## Local worktree permission testing

Every macOS `make dev` launch gives the debug Computer use helper a stable
bundle identifier derived from the current worktree path and signs it with the
same development certificate as `Clovy Dev.app`
([development.md](development.md#the-macos-development-app),
[ADR-0060](adr/0060-development-app-runs-as-a-signed-bundle-that-owns-its-grants.md)).
The launch reuses an unchanged helper bundle and never resets privacy grants:
Accessibility granted to the helper and Screen Recording granted to the outer
`Clovy Dev` app survive restarts and rebuilds. A new worktree, a new signing
certificate, or `CLOVY_DEV_SIGNING_IDENTITY=-` is a new identity for macOS and
asks again. To repeat the first-permission walkthrough on purpose, remove the
entries in System Settings, or run `tccutil reset Accessibility
co.opensoftware.june.computer-use-driver.dev.w<worktree>` and `tccutil reset
ScreenCapture co.opensoftware.june.dev` yourself.

The Tauri dev runner starts the app from
`target/<profile>/Clovy Dev.app/Contents/MacOS/os-june`. The authenticated
debug helper accepts only `os-june` in one of this checkout's Cargo profile
directories (marked by Cargo's `.cargo-lock`): Cargo's own binary, or the main
executable of a bundle directly inside that directory. Other names, deeper
bundles, and paths outside this checkout's target tree are refused. If the
Plugins surface reports that the driver stopped while the default self-test
passes, rerun the self-test through the exact live launcher to check this
boundary:

```sh
node scripts/computer-use-self-test.mjs --permissions-only \
  --host "src-tauri/target/debug/Clovy Dev.app/Contents/MacOS/os-june"
```

The production helper always keeps `co.opensoftware.june.computer-use-driver`
and its release signature.

## Build and release diagnosis

Run the deterministic gate after a normal macOS Rust build has signed the
staging helper:

```sh
pnpm computer-use:prepare
cargo build --manifest-path src-tauri/Cargo.toml --bin os-june --locked
pnpm computer-use:self-test
```

The gate checks the exact upstream commit, Clovy helper source fingerprint,
bundle metadata, declared architecture slices, signature, SPDX SBOM, MIT
notice, direct-launch rejection, authenticated Clovy-parent handshake, narrow
driver schemas, and Clovy MCP schema. A schema failure means the pin cannot be
upgraded without reviewing `computer_use.rs`, `computer_use_driver.rs`, and
`cua-driver-contract.json` together.

After a signed universal app build, run the actual nested helper gate:

```sh
./scripts/computer-use-release-self-test.sh --live --target universal-apple-darwin
```

Live mode requires an interactive login plus Accessibility and Screen Recording
for the stable Developer ID helper. On a new desktop release runner, use the
same command with `--prompt-permissions` passed directly to
`computer-use-self-test.mjs`, approve the two macOS dialogs, then rerun without
prompting. macOS does not provide a supported command that fabricates these
user grants.

The live fixture runs through the real signed Clovy executable's fixed QA host,
which accepts only the bundled helper and the two disposable fixture bundle
identifiers. It captures the target, clicks a numbered button while the observer
remains frontmost, verifies the real pointer and current-Space window flags did
not change, kills both fixtures, and exits the private driver. The manual
Stage Manager pass additionally verifies that the exact shelf window joins
Clovy's current group without moving the pointer or showing another decision.
The release workflows do not run this fixture; computer use is validated on the
release-candidate channel before promotion. Run it manually when validating a
release runner or diagnosing a signed-build regression.

## OS update or regression

- Run the live fixture on the oldest and newest supported macOS releases.
- Recheck both TCC states after an OS update; a preflight `true` with live
  capture `false` is treated as missing permission.
- Record driver version, macOS version, operation class, and failure class only.
  Never attach the screenshot, AX tree, typed value, app title, or approval text
  to analytics or an issue without explicit user attachment.
- If focus, pointer, Space, target binding, or unapproved-mutation behavior
  regresses, hold the promotion and keep the previous release live.

## Emergency rollout

Clovy API's `[computer_use]` section supports:

```toml
[computer_use]
enabled = true
disabled_june_versions = ["0.0.33", "0.1.*"]
disabled_macos_versions = ["15.5.*"]
```

Entries are exact or use one trailing wildcard. After changing production
configuration, deploy Clovy API and verify `/v1/computer-use/rollout` with the
affected `x-june-app-version` and `x-june-macos-version` headers. A desktop
that observes a disable stops active work and keeps that disable sticky if the
API subsequently becomes unavailable. The normal successful-decision cache is
five minutes.
