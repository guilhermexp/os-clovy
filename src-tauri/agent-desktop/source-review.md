# Agent-desktop source qualification

Reviewed on 2026-11-01 for the T5D cutover. The owner selected
`vercel-labs/agent-desktop` commit
`9eb9b296192fcb70bda25e918cd6dd66abfa3d0d`. This is the exact source input,
not the `v1.0.2` release archive (`a47ed664`). The upstream package still
reports version 1.0.2. The preparer verifies the commit, source tree, lock,
license and Clovy patch; it archives a separate build checkout and preserves
the upstream evidence checkout and its builds.

The source commit is less than seven days old. The specific reason to use it
is the Windows CLI shutdown correction in `0768da43` (the main thread's UIA
COM client must not release after COM teardown), plus `d77f910d` (CR LF must
produce one line break) and `9b1a4346` (overlay points are already physical
pixels; scaling them twice displaces feedback). The owner explicitly selected
this pin; this is a source-specific qualification, not a dependency cooldown
exclusion. No dependency-age exclusion was added. The preserved release-age
receipt checked 160 new locked crate versions with zero exclusions.

The 16 post-tag commits change 99 files. The source comparison includes the
Windows UIA lifetime, keyboard Unicode/CR LF handling, overlay coordinate
changes, core text interval dispatch, caret and presentation behavior, and the
macOS Objective-C build script. The lock delta only adds the existing `png`
dependency to the Windows workspace member. The macOS build script compiles
checked-in Objective-C through `xcrun` and archives it locally; it downloads
nothing. Interval typing and arbitrary overlay controls are not part of the
helper's tool surface. Exact-window and sensitive-control guards remain in
Clovy. The Clovy patch adds process ownership and target revalidation at the
engine dispatch boundary. It also creates only the helper-owned private
presentation session, with cursor feedback enabled and tracing disabled, and
refuses to reopen an ended session. Its digest is pinned and shipped in the
manifest. Modifier drag uses per-event macOS flags. Supervised Windows
click, drag, wheel and keyboard chords use a native input owner that refuses
preheld keys/buttons and checks the active interactive desktop, foreground
integrity and process generation around key-state observations. Inner guards
release only keys/buttons whose down events were posted; there is no outer
repair of unposted planned keys. Posting is not delivery evidence. Cancellation
acknowledgement requires two clear OS observations within a 40 ms budget,
checked before and after each observation. Query failure, desktop change or
budget expiry produces no acknowledgement and retains an unsafe receipt.
Constructor-only patch hunks provide the default empty modifier field to
existing upstream callers. Windows native semantic and foreground writes have
fresh desktop-context checks after reads and immediately before effects or
retries, so startup readiness cannot authorize writes after locking. Desktop
access loss stays sticky for the command and retains prior action results and
delivery disposition. Private helper invocations also recheck readiness. Text insertion enforces its
10,000-character bound by Unicode characters rather than UTF-8 bytes; the
upstream ref-action regression exercises a full 10,000 multibyte characters.
Supervised live sensitive-field checks run under the existing interaction
lease before semantic, window-key and physical dispatch. Missing or incomplete
safety evidence refuses delivery rather than guessing. Control metadata,
including placeholders, is rechecked before dispatch; document-window titles
alone do not classify their unrelated controls as secret fields. The private
snapshot evidence remains inside the helper and is stripped from model output.
Cancellation allows 120 ms for cooperative cleanup before hard stop. Native
workers acknowledge cancellation only after querying actual release state for
owned input; a failed release query produces no acknowledgement. Linux worker
regressions cover release-before-acknowledgement and no later sends. That is
Linux execution evidence, not macOS or Windows runtime acceptance. macOS
text insertion checks whether AXSelectedText is settable before attempting a
write. Unsupported insertion returns not-delivered/safe with no mutation; an
actual write failure preserves its delivery disposition and never retries via
AXValue or the clipboard.

This qualification authenticates the chosen content by its pinned Git object
and SHA-256 inputs. It does not assert a verified upstream author signature:
local Git signature verification was unavailable. It does not establish
macOS or Windows behavior, code signing, TCC, UIA, mixed DPI, Stage Manager,
or Stop acceptance. Those require the signed native candidate tests in
`docs/qa/computer-use-parity.md`. The Linux smoke is separate, narrower
execution evidence.

Licensing: upstream `LICENSE` is Apache-2.0, SHA-256
`cfc7749b96f63bd31c3c42b5c471bf756814053e847c10f3eb003417bc523d30`.
The distributed SPDX source record identifies this exact commit and patch;
the pinned Cargo.lock records transitive resolution. The per-target manifest
records the actual signed executable digest and architecture slices. There
is no first-use engine download or global CLI installation.
