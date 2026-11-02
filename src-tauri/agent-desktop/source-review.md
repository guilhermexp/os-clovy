# Agent-desktop source qualification

Reviewed on 2026-11-01 for the T5D cutover. The owner selected
`vercel-labs/agent-desktop` commit
`9eb9b296192fcb70bda25e918cd6dd66abfa3d0d`. This is the exact source input,
not the `v1.0.2` release archive (`a47ed664`). The upstream package still
reports version 1.0.2. The preparer verifies the commit, source tree, lock,
license and Clovy patch; it archives a separate build checkout and preserves
the upstream evidence checkout and its builds.

The preparer also authenticates a snapshot of Clovy's shared private-browser
runtime identity source and stages those exact bytes in the archived workspace.
The Windows window inventory and strict window resolution now consume that
source to reject the private browser or an indeterminate process identity
before reading window titles. The process image and birth are read from the
same process handle; stored targets must match their expected generation.
The handle remains open through filesystem classification and the guarded
title read. Hosted frames retain both process handles; a window owned by its
target process needs only one qualification. Image lookup uses a small buffer
and retries with the Windows path limit only when the first buffer is too small.
Exact-window capture carries the stored window and owner generations into the
capture backends. WGC requalifies around session start and frame acquisition;
the PrintWindow worker requalifies after its responsiveness probe and retains
the process handles through each pixel read. A failed WGC window attempt is
terminal; the window path does not use the display BitBlt capture. These are
source changes and cross-compilation evidence, not Windows runtime acceptance.
Resolved Windows UIA handles now retain distinct root and element process
generations. Live element reads, text selection and caret reads qualify the
element's current native PID and those stored generations before provider
reads and requalify around later calls. Missing identity fails closed. Tree
traversal, batched cache and related-element reads still require their own
guards; this increment does not establish native UIA acceptance.
Process-generation mismatches and disappeared processes retain the StaleRef
contract; a private or indeterminate policy classification is PolicyDenied.
Live UIA qualification observes the existing deadline before and after its
native and filesystem checks. Window tree discovery now obtains child and
sibling elements without content-prefetch; each node's process and root
provenance are qualified before its element-scoped cache or live read. A
qualification error propagates through the walk and resolver. LabeledBy now
qualifies its related element separately before reading security state or name,
and propagates identity or policy failures. Native UIA acceptance and the
remaining input/action boundaries are still pending.

The live element and root-binding paths also check the original deadline
immediately after the initial native PID read, before process or filesystem
qualification. This is a source-level correction; it does not establish
native timing behavior.

Windows path, search, chrome-repair and anchor resolution now bind each
candidate's own process generation before reading matching evidence and require
that same binding when producing a native handle. The root generation remains
separate. The shared HRESULT import is available to its unconditional generic
error function on all targets; this permits a portable walker control-flow
proof and does not establish Windows UIA behavior.
Windows UIA descendants, capture, input and other native guards, macOS guards,
a newly prepared engine, and native acceptance remain pending. This partial
change does not qualify an existing engine artifact.

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

Follow-up source qualification on 2026-11-01 covers 78 audited engine files,
including 18 changes after the preserved e868/source6 generation. Window keys
without an element now carry the captured window identity into the private
consumer. The expected PID, process generation, title and geometry are checked
inside the interaction lease before focus or input; a reused handle cannot
become new authority. Stable targets retain their supported headed/headless
key behavior.

Preparatory focus is recorded independently from click, drag or key delivery.
Read-only checks run before focus where possible and remain fresh afterward.
If security, resolution or cancellation fails after focus was attempted, the
receipt retains that effect and unsafe partial/uncertain delivery rather than
claiming the whole operation was not delivered. The original native input or
guard disposition is retained separately before elevating the outer result,
so focus evidence does not erase not-delivered evidence about target input.
No such error authorizes replay or fallback; a pre-focus refusal can still
be not-delivered/safe.

macOS launch receipts now carry executable path, bundle and native process birth
from the retained NSRunningApplication completion object. Objective-C strings
remain alive through the synchronous Rust callback. Native/kernel path identity,
birth and object liveness are checked again after reads; unknown or changed
authority fails closed with any launch effect preserved. Existing-process
attachment retains its previously approved birth token. A fresh PID lookup is
not adopted as launch authority. The reviewed patch includes the matching
Objective-C bridge and Rust callback ABI, not only Rust source files.

Cached consumer regressions exercise handle reuse with zero focus/input,
post-focus failures with unsafe effect evidence, stable controls, and native
pure filesystem/birth decisions. These checks do not establish NSWorkspace,
libproc, signed native execution or a new engine artifact. The preserved
source6 binary authenticates only e868; this patch requires a fresh prepared
artifact and matching final qualification before delivery.

The Windows element action dispatcher retains process authority across
preparatory reads. Its UIA action mutations requalify the resolved element and
root immediately before the call, after desktop readiness, and retain the
authority through the call. Ancestor scroll targets are bound to their own
process identity before their properties are read. A prior attempted effect
is not made safe to retry by a later guard refusal. Engine window activation
retains the original target and owner generations and requalifies them before
restore and foreground writes; a later failure preserves an uncertain effect.
The private adapter accepts a closed set of tool names and constructs engine
arguments itself: the standalone shell and notification CLI commands are not
exposed through that route. An incomplete ancestor read reports stale unless
the actual deadline has expired. Keyboard focus preparation retains the
element authority through the focus read. Key-down and Unicode sends retain
the original foreground and focused process generations and qualify them at
each send; exact-window and ref paths also requalify their original target. The
ref path rechecks the focused UIA element after blocking identity queries.
Owned key-up cleanup is separate from new input. The private targeted mouse and
drag paths now pass the original window into Windows input and requalify it,
including its captured native owner, at each send and modifier key-down. A drag
abort releases its own button without moving the pointer, and errors after
attempted pointer movement are unsafe to retry. Windows targeted input now
checks the point window, current cursor for button and wheel sends, and GUI
capture reported by the foreground and point-window threads. It qualifies the
receiver against the retained target or owner process generations and repeats
native observations before posting. Ref clicks carry the ref's stored window
identity into that scope. A capture held by another GUI thread has not been
ruled out; native receiver and capture behavior remain unqualified. Scoped
mouse and drag still fail unsupported on platforms without an override.

Window and reference records now carry an optional typed native owner identity
through allocation, persistence and ref payloads. A Windows hosted window
without the original owner identity is refused at the qualified window boundary;
a same-process owner may use the existing target generation. Windows inventory
records the owner generation from the retained process authority before reading
the title, and carries it through the broker, references, focus and capture.
Scoped input checks Stop before its final native receiver and focus observations;
those observations and the effect have no intervening filesystem check.
This records the source-level boundary, not a native Windows acceptance.

Ref actions preserve preparatory focus and scroll evidence when a later target
guard or dispatch fails. The target refusal keeps its own disposition in the
error details, while the aggregate is unsafe to retry after a possible effect.

The macOS launch bridge qualifies the executable from its resolved application
URL before asking NSWorkspace to open it. Attachment rechecks the selected
process generation and executable before and after window waits. This source
review does not establish Cocoa or native macOS behavior.

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
