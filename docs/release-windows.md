# Building Clovy for Windows

Clovy has no published Windows releases or release workflow: the signing and
publishing pipeline belonged to the original organization and was removed by
the `desvincular-repo-ci` change. Build the NSIS installer locally with
`scripts/build-windows.ps1` (prerequisites in `AGENTS.md`) and validate it
with the checks below.

The installer installs `clovy.exe` under `$env:LOCALAPPDATA\Clovy` with the owner
as publisher, per
[ADR-0068](adr/0068-clovy-takes-its-own-identity-with-a-one-time-migration.md);
`scripts/verify-windows-artifacts.ps1` checks the payload.

The one-time identity migration of ADR-0068 runs only on macOS: Clovy on
Windows always starts with a new profile.

## Windows support

The Windows installer supports the app shell, microphone
recording, dictation shortcuts and paste, note generation, folders, and settings
backed by Clovy API. macOS system-audio capture and Seatbelt
sandbox features are macOS-only.

Windows builds bundle the signed Node 24 agent sidecar, so Clovy can
start the agent on a clean machine without Python, GitHub downloads, or a
first-run runtime install. Sandboxed mode restricts host file tools to the
workspace and disables shell execution on Windows. Unrestricted mode supports
shell execution with approval for sensitive actions.

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

Confirm the app launches as Clovy, the bundled agent starts on a clean VM with
no Python installed, and Windows dictation can record from the microphone and
paste into a normal app. Record from the microphone and generate a note against
a running Clovy API. `Get-AuthenticodeSignature` reports `Valid` only for a
build signed with your own Authenticode certificate; an unsigned build reports
`NotSigned`.
