# Browser use owns a private pinned engine and a separate persistent profile

Date: 2026-11-01. Accepted by the owner in the browser-control change.
Supersedes the extension, shared-tab and detected-browser mechanisms of
ADR-0017. The capability, network and consequential-action boundaries remain.

The extension path does not provide a separate persistent login profile and
was never wired into the production broker. A stock browser CLI would expose
unprotected local commands and recover stale references by role/name, which
cannot preserve a task's authority after approval.

Clovy bundles agent-browser source commit
`5868f7c05020e047c00e68e4abf9253cce864f41` with two reviewable patches and a
private host entry point. A closed JSON protocol runs over inherited stdio
pipes with a nonce handshake. Chrome uses inherited CDP pipes; the host cannot
connect to a personal browser or expose streaming, dashboard, plugins, generic
evaluation or upstream MCP commands. The upstream browser, snapshot and
reference implementations remain the engine; private glue adds exact
document/node/origin/element guards without role/name recovery.

The app creates the supervised process group (Windows kill-on-close Job)
before the host can launch Chrome. Chrome stays inside that group/Job rather
than detaching into a separate group. This deliberately changes upstream's
group behavior: cancellation before CDP readiness can always terminate both
host and Chrome without learning a child PID from a delayed response. The
process-resource mutex never covers wire/CDP I/O. An interrupted dispatched
effect returns unknown and is never retried or verified by an unrelated image.

Attended sessions lease one dedicated persistent app-data profile exclusively.
No cookies, browser profile, storage state or password store are imported.
Human takeover stops capture/input, flushes and reopens this profile visibly,
discards old tab/reference authority and requires explicit resume. Native
credential storage is retained; no basic password store or mock keychain is
used. Routine grants select separate ephemeral profiles, capped at two and
reaped after five minutes idle. Turning off a grant, ending a run or quitting
terminates control before releasing leases. Only ephemeral profiles are deleted.

The cost is a larger signed application and a downstream protocol to maintain.
Exact source, Chrome archive digests, lockfile, patch/payload hashes, SPDX
metadata and per-file inventories make each prepared artifact reviewable.
Preparation is a build/dev/release step; runtime never installs a CLI or
downloads a browser. Native macOS/Windows signing, credential-store behavior
and both CLI walkthroughs require their own platform evidence.

## 2026-11-01 addendum: page metadata cannot exempt input from approval

The review confirmed that the inherited keyword classifier could silently
authorize a neutral or localized button whose handler deletes or publishes.
This is an inherited policy gap, not a regression introduced by the private
host. Every click, fill and key press now asks the central approval broker;
field-change and key handlers can have effects just like click handlers.
Page labels identify the target in a capped, quoted, explicitly untrusted
preview, but cannot supply authorization. Sensitive controls remain human-only,
including password autocomplete on a text input. Reads and explicit navigation
retain their existing grants. Unattended routines therefore keep reading and
navigating but cannot perform agent input without a person. This accepts more
attended approval cards rather than treating page-authored semantics as proof
of harmlessness; there is no browser-specific bypass or new approval surface.

The same source audit found that pinned CFT uses Chromium's default native
Safe Storage namespace. Current re-sign-only packaging does not establish a
`-dev` Keychain service. Persistent macOS development/proof launch stays blocked
until a supported namespace integration is chosen and qualified with the owner.
This is an explicit unresolved native boundary, not permission to use mock/basic
crypto, touch an existing owner's item or assume a profile/bundle rename changes
the credential service. Anonymous fixture evidence does not qualify that custody.

## 2026-11-01 addendum: coherent redaction and exact private metadata

An inventory taken before AX can miss a changed or newly inserted field. Native
field inventory and AX observation therefore share a frozen lifecycle boundary;
AX redaction also derives field values from the same raw response before
removing them. Native layout/text geometry covers inherited editable overflow,
closed shadow trees and pseudo-elements. Unsupported editable paint fails
closed instead of returning an unverified image. Every pause is restored, or
the task becomes terminal/unknown and is reaped. Mutation dispatch runs after
restoration with the exact synchronous element guard.

Sanitized labels can collide even when original metadata changes. Original
facts remain only in the private per-reference cache; the wire carries an
opaque random metadata revision, reused only while those facts are unchanged.
This preserves the approval binding without publishing a low-entropy secret
digest that could be guessed offline. This is an internal typed-wire extension,
not another model-facing operation or authority grant.

## 2026-11-01 addendum: classify original facts before redaction

An attribute such as current-password can become current-[redacted] when its
field content is removed from metadata. Reclassifying that DTO in Rust could
therefore request approval for a human-only field, even though the final guard
still refused execution. The private host now computes the single classification
on original facts and returns a required closed enum with the sanitized
inspection. The broker consumes it before requesting approval; it has no second
DOM heuristic or default for missing/unknown values. The enum is bound into the
opaque metadata revision, remains outside model tool schemas, and does not carry
raw facts or field content. Ordinary actions still require the same central
approval; this adds no authorization or browser bypass.

## 2026-11-01 addendum: preserve visibility during coherent observation

This supersedes the lifecycle mechanism in the coherent-redaction addendum,
while retaining its privacy boundary. Chromium156's frozen lifecycle command
calls WasHidden; active only unfreezes the page. It cannot restore an originally
visible page by itself. Bringing the target to the foreground would instead
change focus and activate an originally hidden tab.

The private host therefore uses a transient debugger agent with an owned random
source-URL breakpoint and a fixed, isolated void evaluation. It subscribes before
enabling, accepts only its exact breakpoint hit in the selected session, and
rejects pre-existing or unexpected pauses. Only registered breakpoint IDs pass
the private event filter; stack frames, source, locals and foreign IDs are
discarded. No debugger operation or evaluation becomes a model-facing tool.

Release removes the owned breakpoint and disables only this debugger agent.
V8 retains the pause when another enabled agent accepts it; the host never sends
a global resume command. Completion of its fixed evaluation confirms release.
Uncertain ownership, lost notifications or failed release makes the task
terminal/unknown and reaps it. Page tasks remain paused during inventory, AX,
inspection and capture, and ordinary mutation dispatch follows release with the
same exact synchronous guard. Screenshot animation, geometry and document
checks remain in force. The host does not enable virtual time: switching its
policy from pause to advance still enables a synthetic clock in Chromium156.
Screenshot capture instead pauses native animation playback while the debugger
barrier holds page tasks, then restores the observed finite playback rate.
No visibility emulation, foreground activation, browser
fork or product sandbox/network/crypto override is introduced.

Stock Chromium156's screenshot path requests a redraw and waits for frame
presentation, including with fromSurface=false. A hidden page with pending paint
can stall that request while the debugger's page scheduler pause holds. There
is no established supported parameter for a fresh hidden-frame capture without
that dependency. The host therefore refuses a hidden headed tab's screenshot
before changing animation state or requesting pixels, then releases its owned
pause normally. Snapshot/inspection and the original visibility/focus survive;
default headless background and visible headed captures remain supported.
The parent exposes a definite visibility requirement, with snapshots as the
read alternative and human takeover when an image is required. It never
activates the tab automatically or substitutes a retained surface image. Native
hidden-image capture remains a documented API limit, not a passing PNG claim.
