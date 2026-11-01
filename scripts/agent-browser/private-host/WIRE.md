The `clovy-agent-browser` binary takes no arguments. Build the pinned upstream
crate with `--locked --features clovy-private --bin clovy-agent-browser`, after
applying both reviewed patches and copying the three Rust payload files and
`clovy_private_guard.js` to `cli/src/`. Ship only this private binary.

The parent creates the engine's process group or Windows Job before the engine
can create Chrome. The engine and all Chrome children inherit that group/Job.
The parent clears the environment and supplies the first stdin line
`{"protocol":1,"token":"<32 random bytes encoded as 64 lowercase hex characters>"}`.
The host echoes this handshake over the inherited private stdout pipe. It opens
no socket, discovery service, debugger port, provider plugin or streaming server.
The authentication capability is possession of the inherited private pipes;
the token correlates the child, and is never an argument or environment value.

Every subsequent JSON line is bounded to 16 MiB and contains `id:u64` and an
`action`. Unknown action fields are rejected. Responses are
`{id,delivery,data? ,error?:{code,message}}`; error codes come from a finite local
allowlist, and upstream diagnostic text never crosses the wire. Successful reads
are `not_attempted`, successful mutations are `delivered`, and errors after a
possible mutation are `unknown`. An unknown result invalidates every reference.
There is no retry or automatic change of modality.

Actions:

- `start`: `chromePath`, `profilePath`, `proxyPort:u16`, `persistent:bool`,
  `headed:bool`. Persistent profiles retain native credential storage; anonymous
  ephemeral profiles use only run-owned mock storage. Paths and the proxy are host-owned. Returns integer `tabId`,
  `targetId` and `url`. A fresh task tab is created directly; restored profile
  tabs are never adopted.
- `navigate`: `tab_id:u32`, `url`. Only credential-free HTTP(S) URLs. Returns
  final `url` and `tabId`.
- `back`, `snapshot`, `screenshot`: `tab_id`. Snapshot returns `snapshot`, `url`,
  `tabId`. Screenshot returns PNG `base64`, `url` and `tabId`; non-HTTP pages
  are refused before capture (except the empty about:blank page), and document,
  URL, field geometry and viewport must remain unchanged before emission.
- `tab_open`: optional `url`; `tab_list`; `tab_switch`, `tab_close`: `tab_id`.
  Only explicitly created task tabs are listed or addressable. IDs remain
  monotonic through a human takeover restart. Closed targets fail `tab_gone`.
- `inspect_reference`: `tab_id`, `ref`. Returns `url`, `origin`, `documentId`, `metadataId`,
  `backendNodeId`, and `element` containing strings `tag`, `inputType`, `role`,
  `name`, `id`, `label`, `autocomplete`, and booleans `inForm`, `contentEditable`.
  `name` is the HTML name attribute; `label` is the accessible caption.
  No field value is included. Free metadata is sanitized against the coherent
  field inventory. `metadataId` is a random private-cache revision, reused only
  while the original metadata and inspection facts remain unchanged. It is not
  a digest of field content. Original metadata stays private to the host.
- `guarded_action`: `tab_id`, `ref`, `operation`, full `expected` inspection,
  optional `text` or `key`. Supported operations are `click`, `fill`, `press`.
  Press currently supports Enter, Space, Escape, Backspace and Delete; other keys
  fail definitively before dispatch. The exact node, document, origin and
  metadata are rechecked in an isolated-world DOM task immediately before the
  semantic effect. The full expected revision and original metadata are checked,
  including when redaction makes two different captions look identical. Focus
  is followed by another check before typing/key events. Password, code and
  payment criteria apply to every interaction, including clicks on text inputs
  with password autocomplete tokens; approval cannot override this refusal.
  This uses DOM events, whose `isTrusted` is false; sites requiring trusted input
  need a human interaction. No role/name recovery, generic selectors, JavaScript,
  CDP, cookies, arbitrary browser arguments or file upload API is exposed.
- `suspend`, `resume`: suspend all page reads and actions; resume discards refs.
- `takeover`: host-only. Suspend immediately, close Chrome with a short flush
  grace, reopen the same profile visibly, create a fresh tab, return its IDs,
  `mode:"visible"`, `suspended:true`. Resume is explicit. Never exposed to a model.
- `shutdown`: short graceful Chrome close, then process termination. The parent
  kills/reaps the entire owned group/Job on completion or cancellation.

References are random opaque IDs minted per snapshot and bound in memory to a
single tab, exact main-frame id, loader id, document root and backend node.
Snapshot and inspection observations freeze the page lifecycle while the native
field inventory and accessibility data are collected. AX values and editable
descendants are also derived from each raw AX response before any formatting.
Lifecycle scheduling is restored before the synchronous final guard and effect,
so ordinary network handlers retain their normal behavior. Private snapshots do
not annotate the page or recurse into frames.

Screenshots freeze lifecycle tasks and pause virtual time/animations. Native DOM
layout and text-fragment bounds cover controls, inherited editable descendants,
closed shadow content, pseudo elements and iframes. Overflow outside an editable
ancestor's own box is covered independently. Native geometry and the viewport
are reread before emission. Unsupported text shadows, filters, reflections,
transforms, strokes or emphasis that cannot be bounded fail closed rather than
producing an image. Every scheduler restoration is attempted on every path;
failure closes the browser and returns a terminal unknown result so the parent
invalidates the task. The parent owns profile leases, lifecycle cancellation, grants, approvals,
network policy, artifacts and resume authorization.

Native macOS and Windows acceptance remains separate from the synthetic Linux
fixture. `private-host-smoke.py` accepts the exact host and Chrome paths and
writes real receipts/screenshots; it uses only a run-owned profile and a local
proxy which rejects every hostname except `fixture.invalid`.
The source-only `cdp_fixture.py` relay is not shipped. Its real HTTP image latch
exercises mutation between inventory and AX without adding a production hook or
injecting any CDP command.
