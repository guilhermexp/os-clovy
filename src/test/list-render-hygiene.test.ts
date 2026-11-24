import { describe, expect, it } from "vitest";
import { indexNotesByFolder } from "../components/folders/folder-item-index";
import { filterNotesByQuery } from "../components/notes-list/notes-list-helpers";
import type { NoteListItemDto } from "../lib/tauri";

const NOW = "2026-07-22T12:00:00Z";

function note(
  id: string,
  title: string,
  preview: string,
  folderIds: string[] = [],
): NoteListItemDto {
  return {
    id,
    title,
    preview,
    processingStatus: "ready",
    folderIds,
    createdAt: NOW,
    updatedAt: NOW,
  };
}

describe("list render hygiene helpers", () => {
  it("filters notes by normalized title or preview and reuses the unfiltered list", () => {
    const notes = [
      note("roadmap", "Product roadmap", "Q3 priorities"),
      note("retro", "Weekly retro", "Decisions and follow-ups"),
    ];

    expect(filterNotesByQuery(notes, "  PRODUCT ")).toEqual([notes[0]]);
    expect(filterNotesByQuery(notes, "FOLLOW-UP")).toEqual([notes[1]]);
    expect(filterNotesByQuery(notes, "   ")).toBe(notes);
  });

  it("indexes folder notes once while preserving source order", () => {
    const first = note("first", "First", "", ["project-a", "project-a", "project-b"]);
    const second = note("second", "Second", "", ["project-a"]);
    const index = indexNotesByFolder([first, second]);

    expect(index.get("project-a")).toEqual([first, second]);
    expect(index.get("project-b")).toEqual([first]);
    expect(index.has("missing")).toBe(false);
  });
});
