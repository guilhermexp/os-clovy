# Building Clovy for macOS

Clovy has no published releases or release workflows: the CI and runner that
signed, notarized and published builds belonged to the original organization
and were removed by the `desvincular-repo-ci` change. Build the app locally and
validate the bundle with the checks below.

`Clovy_*` names are the canonical build artifacts. June-named artifact aliases
and the installed `June.app` path remain
compatibility identities under
[ADR-0055](adr/0055-clovy-technical-identity-migrates-through-a-compatibility-bridge.md).
The bundle keeps `CFBundleDisplayName=June` in its base `Info.plist` so macOS
recognizes that the physical `June.app` path has not been renamed, then maps the
localized display name to Clovy through `en.lproj/InfoPlist.strings`. Finder and
newly pinned Dock items therefore present Clovy while updater and permission
continuity stay attached to `June.app`. A Dock item pinned before the rebrand
may retain its own cached `June` label after an in-place update. macOS exposes
no supported application API for rewriting that per-user Dock state; remove the
old item and choose **Keep in Dock** again once to refresh it to Clovy.

## macOS support

Clovy supports macOS 14.0 and later on Apple Silicon and Intel Macs, including
macOS 15. Release builds are universal macOS apps. System
audio capture uses Core Audio process taps and is available only on macOS 14.2
and later. On macOS 14.0 or 14.1, recording falls back to microphone-only mode.
The signed Node 24 agent sidecar carries separate `arm64` and `x86_64`
executables that are combined into one universal binary. Release builds verify
both architectures, the sidecar checksum, and startup from a path with spaces
before the universal app is assembled.

## Build

The release build recipe (Node 24 official binary for the agent sidecar, the
Xcode strip workaround, updater artifacts off) lives in `AGENTS.md` under
**Build**. For a locally signed and notarized DMG, `scripts/build-signed-dmg.sh`
reads the Apple signing values from `.env.signing`:

```sh
pnpm tauri:build:signed-dmg
```

## Validating a built app

Install the built app into `/Applications` and run:

```sh
APP="/Applications/June.app"
DMG="<path to the built Clovy_universal.dmg>"

codesign --verify --deep --strict --verbose=2 "$APP"
spctl --assess --type execute --verbose "$APP"
spctl --assess --type install --verbose "$DMG"
xcrun stapler validate "$APP"
xcrun stapler validate "$DMG"
plutil -extract CFBundleURLTypes xml1 -o - "$APP/Contents/Info.plist"
lipo -archs "$APP/Contents/MacOS/os-june"
lipo -archs "$APP/Contents/Resources/native/bin/June Dictation Helper.app/Contents/MacOS/june-dictation-helper"
lipo -archs "$APP/Contents/Resources/native/bin/June.app/Contents/MacOS/june-system-audio-recorder"
RUNTIME="$APP/Contents/Resources/native/bin/clovy-agent-runtime"
test "$(shasum -a 256 "$RUNTIME" | awk '{print $1}')" = "$(cat "$RUNTIME.sha256")"
codesign --verify --strict --verbose=2 "$RUNTIME"
/usr/bin/arch -arm64 "$RUNTIME"
/usr/bin/arch -x86_64 "$RUNTIME"
```

Confirm `osjune` appears in `CFBundleURLSchemes` and each `lipo` command
prints both `x86_64` and `arm64`. The runtime checksum and signature must pass,
and both architecture commands must execute successfully. On an
Apple Silicon validation host, the x86_64 command is a real Rosetta execution,
not an architecture inferred from file metadata.

Also confirm the compatibility path resolves to the Clovy presentation name:

```sh
node scripts/verify-macos-app-name.mjs "$APP"
```

Do not replace this with a raw `CFBundleDisplayName` assertion. Finder requires
the base display name to match the physical bundle name before it applies the
localized Clovy value.

The Node sidecar is a separate hardened executable and must retain both
`com.apple.security.cs.allow-jit` and
`com.apple.security.cs.allow-unsigned-executable-memory` after the outer app is
signed. These are the narrow V8 executable-memory exceptions. Do not replace
them with `disable-executable-page-protection` or disable library validation.
Check the packaged sidecar entitlements and run both architecture slices
before notarizing.
