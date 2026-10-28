# Building Clovy for Windows

Windows releases come from `.github/workflows/release.yml` (see
[ADR-0069](adr/0069-the-owner-publishes-own-releases-with-a-one-channel-updater.md)
and [release-macos.md](release-macos.md)): its `windows` job runs
`scripts/build-windows.ps1` with the updater key and publishes the NSIS
installer and its `.sig`. You can also build the installer locally with
`scripts/build-windows.ps1` (prerequisites in `AGENTS.md`) and validate it
with the checks below.

The installer has no Authenticode signature, so SmartScreen warns on the first
run ("More info", then "Run anyway"). Updates are verified by the updater's own
minisign signature, not by Authenticode:

- With `TAURI_SIGNING_PRIVATE_KEY` (and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`)
  set, `build-windows.ps1` adds `src-tauri/tauri.release.conf.json`, and the
  output directory holds `Clovy_<version>_x64-setup.exe` and
  `Clovy_<version>_x64-setup.exe.sig`. `verify-windows-artifacts.ps1` then
  requires that `.sig` (a minisign signature) next to the installer.
- Without the key the build is the same unsigned installer with no `.sig`, and
  the verifier does not look for one.

Either way the verifier still requires `NotSigned` from Authenticode.

The installer installs `clovy.exe` under `$env:LOCALAPPDATA\Clovy` with the owner
as publisher, per
[ADR-0068](adr/0068-clovy-takes-its-own-identity-with-a-one-time-migration.md);
`scripts/verify-windows-artifacts.ps1` checks the payload.

The one-time identity migration of ADR-0068 runs only on macOS: Clovy on
Windows always starts with a new profile.

## Windows support

The Windows installer supports the app shell, microphone
recording, dictation shortcuts and paste, note generation, folders, and settings
using direct user-key providers or the selected CLI/endpoint Notes engine. macOS system-audio capture and Seatbelt
sandbox features are macOS-only.

Chat requires an installed CLI and its own login. Claude Code is the default,
with Codex as the existing local fallback. The installer bundles the `clovy-mcp`
relay for Clovy tools, not a model-and-tool loop. CLI execution policy and
Clovy's local approval broker govern each turn; see
[llm-providers.md](llm-providers.md#cli-chat-engine).

## Qualifying an unsigned build on native Windows

Before merging Windows build changes, run the tracked qualification helper in
PowerShell 7 on a native x64 Windows machine. Pass the standalone repository,
branch, and exact 40-character commit SHA to qualify:

```powershell
$repo = "E:\clones\work\os-june-butler"
$branch = "<branch-to-qualify>"
$expectedSha = "<exact-40-character-commit-sha>"

& "$repo\scripts\qualify-windows-build.ps1" `
  -RepoPath $repo `
  -Branch $branch `
  -ExpectedSha $expectedSha
```

The helper fetches `origin`, synchronizes the named GitButler branch to its
remote state, and fails unless both resolve to the expected SHA.
It then runs two complete unsigned builders and an additional explicit artifact
verification after each build. It does not install dependencies, sign or upload
artifacts, push branches, or discard source changes.

The default evidence directory is
`.tmp/windows-qualification/<branch>/<short-sha>/`. Pass
`-EvidenceDirectory` to use another location. Each run writes `evidence.md` and
`build-transcript.txt`; certificate values are not included.

## Validation

Copy a locally built `Clovy_x64-setup.exe` to a clean Windows 11 VM and run:

```powershell
$installer = "$env:USERPROFILE\Downloads\Clovy_x64-setup.exe"
Get-AuthenticodeSignature $installer | Format-List
Start-Process -FilePath $installer -ArgumentList "/S" -Wait
Start-Process "$env:LOCALAPPDATA\Clovy\clovy.exe"
```

The installed app is branded as Clovy, but the current Windows binary on disk is
`clovy.exe` under `$env:LOCALAPPDATA\Clovy`.

Confirm the app launches as Clovy and an installed CLI starts with its own login, and Windows dictation can record from the microphone and
paste into a normal app. Record from the microphone and generate a note against
the configured Groq key and selected Notes engine. `Get-AuthenticodeSignature` reports `Valid` only for a
build signed with your own Authenticode certificate; an unsigned build reports
`NotSigned`.
