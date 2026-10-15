---
status: proposed
date: 2026-10-15
supersedes: ADR-0058 ("everything is read-only", for turn-bound connections only); ADR-0040 ("June ships no MCP server of its own", for CLI chat engines only)
---

# CLI chat turns get Clovy's write tools through the MCP server and an approval broker

## Context

The owner decided on 2026-10-15 that everything the native agent does moves to
the CLI chat engines (Claude Code and Codex), and that the native agent is then
removed. The CLI engines reach Clovy only through `clovy-mcp`
([ADR-0058](0058-clovy-mcp-server-relays-to-the-running-app.md)), which is
read-only, stateless, and does not know which session a call belongs to. A CLI
turn runs in print mode and cannot ask the user anything mid-turn; an action
that needs approval fails today. More than fifty capabilities (memory writes,
media, recorder, Browser use, Computer use, routines, connectors, user MCP
servers) exist only as native host tools
([ADR-0040](0040-plugin-capabilities-as-host-tools.md)), whose safety depends
on the Agents SDK approval interruption.

Alternatives considered:

- **Pass the CLI pre-granting flags** (`--dangerously-skip-permissions`,
  `--dangerously-bypass-approvals-and-sandbox`). Rejected: removes the human
  approval boundary for every action, including ones reachable by prompt
  injection from notes and transcripts.
- **Give each capability its own MCP server or pass connector credentials in
  the CLI's `--mcp-config`.** Rejected: credentials would leave the app,
  breaking [ADR-0039](0039-june-owned-routines-and-mcp.md) and
  [ADR-0050](0050-linear-official-hosted-mcp.md), and each server would need
  its own approval story.
- **Keep the native agent for write actions.** Rejected by the owner's
  decision to remove it.

## Decision

- Each CLI turn gets a single-use **turn token** in `clovy-mcp`'s arguments.
  The relay sends it in the handshake (additive `turn` field, protocol version
  unchanged). The app binds the connection to the session, run, CLI, safety
  mode, and attended flag, and revokes the token when the run ends.
- Connections **without** a turn token (external clients) keep exactly the
  read-only surface of ADR-0058. Write tools are listed and callable only on
  turn-bound connections.
- Every Clovy write tool asks an in-app **approval broker** before any effect.
  The broker writes the same interruption item the native agent used, so the
  existing cards render it, waits with a timeout shorter than the CLI's tool
  timeout, accepts one decision, and turns denial, expiry, or cancellation
  into a tool error. Unattended turns are denied without a prompt.
  Clarification uses the same broker.
- When supported by the CLI, Claude Code's own permission prompts are routed
  to the broker through `--permission-prompt-tool`. No flag that skips or
  pre-grants approvals is ever passed; `--allowedTools` only names Clovy's own
  MCP tools, whose authority is the broker inside them.
- User MCP servers and Linear reach CLI turns through `clovy-mcp` as a gateway,
  so credentials stay in the Keychain and host policy (visibility, approval,
  Seatbelt) still applies.
- Companion remote approval keeps the scope of
  [ADR-0053](0053-companion-computer-use-approvals-are-one-shot.md): only
  `computer_use`, one shot.

Contract and tool names: OpenSpec change `cli-substitui-agente-nativo`
(`specs/clovy-mcp-server/spec.md` owns the names).

## Consequences

- One approval boundary for every engine that remains; the native SDK
  interruption can be deleted with the sidecar.
- `clovy-mcp` stops being read-only for turn-bound connections; its threat
  model now includes write authority gated by the turn token plus a human
  decision. The token is visible to same-user processes, which can already
  read the installation secret.
- Codex has no permission-prompt hook in `exec`; its own shell and file actions
  stay limited by its sandbox, while Clovy's tools still go through the broker.
- CLIs without MCP (pi, agy, cursor-agent) get no Clovy tools.
- A later ADR records the removal of the native runtime and supersedes
  ADR-0038, ADR-0040, ADR-0013, and the runtime parts of ADR-0039.
