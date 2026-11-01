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
