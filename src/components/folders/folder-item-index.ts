import type { NoteListItemDto } from "../../lib/tauri";

export function indexNotesByFolder(
  notes: readonly NoteListItemDto[],
): ReadonlyMap<string, readonly NoteListItemDto[]> {
  const index = new Map<string, NoteListItemDto[]>();
  for (const note of notes) {
    for (const folderId of new Set(note.folderIds)) {
      const items = index.get(folderId);
      if (items) items.push(note);
      else index.set(folderId, [note]);
    }
  }
  return index;
}
