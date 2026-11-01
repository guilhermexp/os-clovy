# Computer use runs a pinned engine behind its authenticated helper

Status: accepted, 2026-11-01. Implements the owner's D6 decision in
`openspec/changes/melhorias-agente-audio-automacao/design.md`. The integrator
assigns the final ADR number. Supersedes ADR-0028's engine and macOS-only
implementation details; its local approval and private-helper boundary remain.

The desktop needs one engine for macOS and Windows while retaining exact
window identity, task ownership, app authorization, sensitive-field checks,
and Stop. Loading the upstream FFI in Tauri would place native control in the
main process, omit window scoping in its snapshot API, and leave destruction
unable to cancel an in-flight call. Invoking the normal upstream CLI on macOS
would instead create a public same-user daemon outside the helper's ownership.

Clovy builds `vercel-labs/agent-desktop` at the exact source commit in
`src-tauri/agent-desktop-pin.json` and retains the Clovy-owned authenticated
helper. On macOS that helper starts the internal command host directly over
private stdio. On Windows each invocation belongs to a non-breakaway Job
Object with kill-on-close. The agent receives the existing 14 closed helper
operations through Clovy's broker; it cannot choose arbitrary argv, an
upstream MCP server, a CLI home, or another daemon.

The reviewed downstream patch keeps macOS controllers and overlays in the
helper's process group and revalidates physical targets under the engine's
interaction lease. Stop invalidates run/epoch authority and terminates owned
processes without waiting for the request mutex. Real applications opened
through the OS application broker remain the user's applications.

Every package includes the real engine, its manifest, Apache-2.0 notice,
source SPDX record, and source qualification. Preparation uses a separate
archive of the immutable source and applies only the pinned patch. Mac engine
and helper executables are signed before the outer app. Windows engine,
helper and host require valid signatures from the same publisher. The signed
helper embeds the actual prepared engine SHA-256 at compile time, and the
signed host embeds the staged helper SHA-256. Editable adjacent manifests
cannot substitute an older executable from the same publisher. Missing
artifacts, a mismatched source/patch/protocol, or a changed binary hash fail
closed. Ordinary native builds require `pnpm computer-use:prepare`; the
explicit helper-bootstrap build flag only breaks preparation recursion and
never creates a placeholder executable. There is no runtime engine download,
global installation, or fallback to the retired engine.

Semantic actions use background mode when supported. The owner authorized
visible input when background delivery is impossible; that switch happens
before dispatch or after a definitive unsupported/not-attempted result.
Delivered or uncertain input is never replayed in another mode. Visible mode
still requires target identity and task authority and grants no OS elevation,
UAC, TCC, login, password, or OTP authority.

The trade-off is a small maintained engine patch and two signed artifacts
instead of the FFI's simpler call boundary. The source pin is newer than seven
days for specific Windows corrections; its reviewed provenance and unchanged
registry dependency cooldown are recorded in
`src-tauri/agent-desktop/source-review.md`. Linux engine/supervisor tests are
narrow proof. Signed macOS and Windows acceptance remains required in
`docs/qa/computer-use-parity.md`.
