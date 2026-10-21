---
status: accepted
date: 2026-10-21
supersedes: 0055
---

# Clovy takes its own identity with a one-time migration

## Context

ADR-0055 kept the June-era technical identities (`co.opensoftware.june`,
`June.app`, `os-june`, the `co.opensoftware.*` credential services and native
messaging hosts, the Web Store extension id) immutable, and bridged everything
else through dual reads and dual writes, because released June builds and their
users had to keep working through upgrades and rollbacks.

This fork has no published users: its only install is the owner's. It has
already left every Open Software service (no OS Accounts, billing, updater,
companion or telemetry), so a rollback to a released June build is not a path
anyone takes. The remaining June identity is pure cost: every new feature has to
reason about two names, and the app still presents a third party's identifier
to macOS, Windows, browsers and credential stores.

## Decision

- The app is `dev.guilhermexp.clovy` (development bundle
  `dev.guilhermexp.clovy.dev`, development profile `dev.guilhermexp.clovy-dev`).
  It ships as `Clovy.app` with executable `clovy` (`clovy.exe` on Windows),
  helpers `Clovy Computer Use Driver.app`, `Clovy Dictation Helper.app`,
  `Clovy Audio Capture.app`, `clovy-dictation-helper`, `clovy-nm-shim`, helper
  identifiers `dev.guilhermexp.clovy.<helper>`, the login item "Clovy", and the
  owner as publisher and author. Only `clovy://` is registered.
- `src-tauri/src/app_identity.rs` is the one place Rust spells the identifier.
  Credential services are `<compiled identifier>[-dev].<name>`, so a proof
  build with its own `CLOVY_DEV_APP_IDENTIFIER` never shares credentials with
  the owner's profile.
- The browser extension carries the owner's own key pair: the manifest `key` is
  its public half (extension id `cmfpkfaibipkocbjcpgamaednndpopcp`); the
  private half stays outside the repository. The native host is
  `dev.guilhermexp.clovy.extension`.
- The first launch under the new identity runs `identity_migration` before the
  Tauri builder (Tauri creates windows, and so the WebView store, before the
  setup hook). When the new data directory is absent or empty and the June-era
  one exists, it copies the data and config directories, the WebView store and
  every credential item (including the SQLCipher key of `activity.sqlite3`)
  into the new names. The copy is staged beside the target and verified by
  file count and size. Databases travel as `VACUUM INTO` snapshots (the
  SQLCipher one with the copied key) taken from a private scratch copy, so
  the source is never opened. Every path into the old profile (database
  text, WebKit localStorage, small text files, symlinks) is rewritten to the
  new one. The credentials are written, refusing a target credential that
  already holds a different value, and only then are the staged folders
  renamed into place, the data folder last, under a journal that lets the
  next launch finish or undo an interrupted publication. A lock per target
  keeps two launches apart. Nothing is copied while the old app runs: the
  launch shows an alert and quits, so the next one can migrate. Any other
  failure removes everything the run created, so the new profile stays
  empty. The source is never moved, changed or deleted.
- The June-era names survive only as migration sources: the source list in
  `identity_migration`, and one-way reads of a June-era persona file, Obsidian
  skill copy or skill setting row. Nothing June-era is written again. The
  rollback bridges of ADR-0055 (credential, browser storage, persona, skill,
  environment variables, IPC aliases, `june-nm-shim`, the June native host) are
  removed.

## Consequences

- The owner grants the macOS permissions again to the new app and helpers,
  re-pastes the MCP configuration (the data directory path changed), removes
  the old extension and loads the new id, and re-enables launch at login if it
  was on.
- A development build migrates the owner's `co.opensoftware.june-dev` profile
  only under the default identifier. Proofs use their own identifier plus the
  debug-only `CLOVY_DEV_IDENTITY_MIGRATION_SOURCE`, which names a synthetic
  source profile and never falls back to the June one.
- A migration that fails (a denied Keychain prompt, a conflicting target
  credential) leaves the app on an empty new profile with the June-era one
  intact; the error is logged beside the new folder
  (`<identifier>[-dev].identity-migration.log`). To retry, the owner quits,
  deletes the new folder and any `<identifier>[-dev].*` credential the failed
  launch created (the activity key `<identifier>[-dev].activity-db` above
  all), and relaunches.
- Released June-era wire and persisted names that are not identity (Clovy API
  headers and error codes, toolset ids such as `june_gmail`, the
  `__june_*__` model prefixes) are out of scope and stay as they are.
