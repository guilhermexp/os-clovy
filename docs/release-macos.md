# Building Clovy for macOS

Releases come from `.github/workflows/release.yml`
([ADR-0069](adr/0069-the-owner-publishes-own-releases-with-a-one-channel-updater.md)):
a tag `vX.Y.Z` (or a manual run naming it) builds the universal Mac app and the
Windows installer, signs the updater packages with the owner's key and publishes
a GitHub release on `guilhermexp/os-clovy` with `Clovy_universal.dmg`,
`Clovy.app.tar.gz`, its `.sig`, the Windows `*-setup.exe`, its `.sig` and
`latest.json`. A manual run defaults to `dry_run=true`: it builds and keeps the
files as run artifacts and publishes nothing.

The build is `Clovy.app` (bundle identifier `dev.guilhermexp.clovy`,
executable `clovy`), per
[ADR-0068](adr/0068-clovy-takes-its-own-identity-with-a-one-time-migration.md).
`node scripts/verify-macos-app-name.mjs <path-to-Clovy.app>` checks the bundle
name, identifier, executable and the name Finder shows. A release starts with
an empty profile: only a development build migrates the June-era dev profile.

## Publishing a release

1. On `main`: `pnpm version:bump X.Y.Z`, commit `release: vX.Y.Z`.
2. Push the tag `vX.Y.Z`. The `version` job fails before any build when the tag
   differs from the four version files or a release for it already exists.
3. When both builds pass, `publish` creates the release; if either fails,
   nothing is published and the tag can be pushed again after the fix.

To roll back, delete the bad release (`gh release delete vX.Y.Z`): `latest`
falls back to the previous one. Installs that already updated reinstall the
previous `.dmg` by hand.

## Updater key

The updater packages are signed with the owner's minisign key. The private key
and its password are the repository secrets `TAURI_SIGNING_PRIVATE_KEY` and
`TAURI_SIGNING_PRIVATE_KEY_PASSWORD`, with a local copy in
`~/.config/clovy/updater.key` and `~/.config/clovy/updater.key.password`
(mode 0600). Keep a backup of both files outside this machine: an install only
accepts packages signed by the key whose public half is in
`plugins.updater.pubkey`, so losing it means reinstalling Clovy by hand
everywhere. The public key also sits in `~/.config/clovy/updater.key.pub`.

## Apple signing is optional

`scripts/build-macos-release.sh` (`pnpm tauri:build:release-mac`) decides from
the variables present, read from the environment (CI) or `.env.signing`
(local). `scripts/build-macos-release.sh --check` prints the plan without
building:

| Variables | Result |
|---|---|
| none | ad hoc signature, no notarization |
| `APPLE_SIGNING_IDENTITY` | app, helpers and DMG signed with that identity |
| identity plus `APPLE_API_ISSUER`, `APPLE_API_KEY` and one of `APPLE_API_KEY_PATH`, `APPLE_API_KEY_P8`, `APPLE_API_KEY_P8_BASE64` | signed, then the DMG is notarized and stapled |
| `TAURI_SIGNING_PRIVATE_KEY` (+ `_PASSWORD`) | adds `src-tauri/tauri.release.conf.json`: `Clovy.app.tar.gz` and its `.sig` |

In CI, `APPLE_CERTIFICATE` (base64 `.p12`) and `APPLE_CERTIFICATE_PASSWORD`
import the identity into a throwaway keychain; locally the identity is read
from the login keychain as it is. Notarization settings without an identity, or
an incomplete set, fail before the build. Adding the Apple secrets to the
repository is the only change needed to ship signed and notarized builds.

The script exports `CARGO_PROFILE_RELEASE_BUILD_OVERRIDE_STRIP=none` (the Xcode
27 proc-macro workaround) and leaves the files in
`src-tauri/target/release-artifacts/macos/` (or `CLOVY_RELEASE_OUT_DIR`).

## Installing an ad hoc build

Until the Apple secrets exist, the release is signed ad hoc and not notarized:

- **First download.** A `.dmg` downloaded in a browser is quarantined, and
  macOS refuses to open the app. Open it once, then go to System Settings >
  Privacy & Security and choose "Open Anyway" for Clovy. Updates installed by
  the app itself are not downloaded through a browser.
- **Permissions after every update.** An ad hoc signature changes with every
  build, so macOS treats each update as a new app: it asks again for
  Microphone, Screen Recording (system audio), Accessibility and the other
  grants. Grant them again after updating. This goes away once releases are
  signed with an Apple identity.
- **No Computer use.** The Computer use driver only accepts a host signed by
  an Apple team (`computer_use_driver.rs`), so an ad hoc release cannot start
  it; the release self-test checks only the bundled helper in that case.

## macOS support

Clovy supports macOS 14.0 and later on Apple Silicon and Intel Macs, including
macOS 15. Release builds are universal macOS apps. System
audio capture uses Core Audio process taps and is available only on macOS 14.2
and later. On macOS 14.0 or 14.1, recording falls back to microphone-only mode.
Chat uses the user's installed CLI. The app bundles the `clovy-mcp` relay and
native helpers, without a separate model-and-tool loop executable.

## Build

The local release recipe (frontend tooling and the Xcode strip workaround) is
in `AGENTS.md` under **Build**; it produces an
unsigned-updater build for local use. For the release files, with the updater
key and an ad hoc signature unless Apple variables are set:

```sh
PATH="$HOME/.local/share/node-official/node24/bin:$PATH" pnpm tauri:build:release-mac
```

## Validating a built app

Run these against the built app (an ad hoc build fails the `spctl` and
`stapler` lines by design; they apply to a notarized build):

```sh
APP="<path to the built Clovy.app>"
DMG="<path to the built Clovy_universal.dmg>"

codesign --verify --deep --strict --verbose=2 "$APP"
spctl --assess --type execute --verbose "$APP"
spctl --assess --type install --verbose "$DMG"
xcrun stapler validate "$APP"
xcrun stapler validate "$DMG"
plutil -extract CFBundleURLTypes xml1 -o - "$APP/Contents/Info.plist"
lipo -archs "$APP/Contents/MacOS/clovy"
lipo -archs "$APP/Contents/Resources/native/bin/Clovy Dictation Helper.app/Contents/MacOS/clovy-dictation-helper"
lipo -archs "$APP/Contents/Resources/native/bin/Clovy Audio Capture.app/Contents/MacOS/clovy-system-audio-recorder"
```

Confirm `clovy` appears in `CFBundleURLSchemes` and each `lipo` command
prints both `x86_64` and `arm64`. Validate CLI chat independently with an
installed, signed-in CLI and the isolated debug-app proof described in
`AGENTS.md`; a bundle architecture check does not prove a model turn.

Also confirm the compatibility path resolves to the Clovy presentation name:

```sh
node scripts/verify-macos-app-name.mjs "$APP"
```

Do not replace this with a raw `CFBundleDisplayName` assertion. Finder requires
the base display name to match the physical bundle name before it applies the
localized Clovy value.

The desktop no longer needs V8 executable-memory entitlements for an agent
process. Preserve the entitlements required by the remaining native helpers.
