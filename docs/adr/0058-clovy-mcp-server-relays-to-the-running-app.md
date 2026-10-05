---
status: accepted
date: 2026-10-04
---

# The Clovy MCP server relays to the running app over an authenticated Unix socket

## Context

MCP clients on the Mac (Claude Code, Cursor, and the CLI chat engines) should
read the user's notes, dictations, memories, and activity. They start MCP
servers themselves, as stdio child processes, so Clovy has to ship an
executable they can run. That executable runs as the user but outside the app.

The data it needs sits in `notes.sqlite3` and in the SQLCipher-encrypted
`activity.sqlite3`, whose key is in the Keychain for the app
([ADR-0057](0057-activity-database-uses-sqlcipher-linked-app-wide.md)). The
capture thread owns the activity store while capture runs, and every read must
apply the current exclusions, retention, and active profile.

This is an outbound server for external clients. It does not bring back
Clovy-managed MCP servers inside the agent loop, which
[ADR-0040](0040-plugin-capabilities-as-host-tools.md) retired; the in-app agent
keeps using host tools.

Alternatives considered:

- **The binary opens the databases itself.** It would need the activity key
  outside the app (a second Keychain identity or an exported key), would race
  the capture thread and the timeline ETL, and would duplicate the exclusion,
  retention, and profile rules.
- **A loopback TCP or HTTP MCP endpoint in the app.** Any local process (any
  user, any sandboxed app) can reach a port; clients would need a token in
  their configuration, and Streamable HTTP adds a second transport to keep
  correct.

## Decision

Ship a small stdio binary, `clovy-mcp`, that relays newline-delimited JSON-RPC
to the running app over a Unix socket in `<app data dir>/mcp/` (directory 0700,
socket 0600). The first line on a connection must carry the installation
secret, a random value stored next to the socket (file 0600); the app also
checks that the peer runs as the same user. The app answers MCP with the same
queries the agent tools and the "Today" view use. The server is off by
default, runs only while the app is open, and the relay answers by itself with
a clear error when it cannot reach the app.

Contract and protocol: [docs/mcp-server.md](../mcp-server.md).

## Consequences

- Tools answer only while Clovy is open with the server on; clients see an
  error saying so otherwise.
- One copy of the read rules: the app's queries, exclusions, retention, and
  active profile apply to MCP clients unchanged.
- Another bundled helper binary to build, sign, and keep at a stable path
  (`Resources/native/bin/clovy-mcp`); moving Clovy.app means copying the
  configuration again.
- The socket path must fit the 104-byte Unix socket limit; very deep data
  directories cannot run the server.
