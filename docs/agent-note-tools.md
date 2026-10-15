# Agent note tools

The in-loop host tools that let the Clovy agent (the local harness and
routines) read the user's notes, transcripts, and dictations. This document
owns their names under [spec/mcp-tool-naming.md](../spec/mcp-tool-naming.md);
the catalog lives in `src-tauri/src/agent_runtime/api.rs` (interactive),
`src-tauri/src/routines.rs` (`base_unattended_tools`), and dispatch in
`src-tauri/src/agent_runtime/tools.rs`.

These are not the tools of the Clovy MCP server for external clients
([mcp-server.md](mcp-server.md)), which has its own `search_notes` and
`get_note`. The in-loop `get_note` mirrors that name and its paging idea on
purpose.

| Tool | Arguments | Returns |
|---|---|---|
| `search_june` | `query` | Up to 20 notes (`id`, `title`, `updatedAt`, and excerpts of `note` and `transcript`) and up to 20 dictations (`id`, excerpt of `text`, `language`, `createdAt`) of the active profile that contain `query` (case-insensitive for ASCII). Each excerpt is at most 4,000 characters around the first match, marked with `...` where clipped. |
| `get_note` | `id`, optional `contentOffset`, `transcriptOffset` | One note of the active profile: `id`, `title`, `updatedAt`, a page of `content` (the edited note, else the generated note) and of the latest `transcript`. Each page is at most 256 KiB, cut on a character boundary, with `contentOffset` / `transcriptOffset` (actual start), `contentTotalBytes` / `transcriptTotalBytes`, and `nextContentOffset` / `nextTranscriptOffset` (null on the last page). Offsets are byte offsets; one inside a character starts at that character. A missing id, or a note of another profile, fails with `note_not_found`. |
| `get_dictation` | `id`, optional `textOffset` | One dictation of the active profile: `id`, `language`, `createdAt`, and a page of `text` with `textOffset`, `textTotalBytes`, and `nextTextOffset`, paged exactly like `get_note`. A missing id, or a dictation of another profile, fails with `dictation_not_found`. |

`search_june` keeps its June-era name: stored routine catalogs, tool
allowlists, and persisted runs refer to it, so it is not renamed here.

Routines see these tools when one of the `context_engine`, `memory`, or
`session_search` toolsets is enabled.

