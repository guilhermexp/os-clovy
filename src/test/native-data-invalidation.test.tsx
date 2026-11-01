import { act, renderHook, waitFor } from "@testing-library/react";
import { useReducer, useRef, useState } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { NoteSaveController } from "../app/note-save-controller";
import { createInitialState, notesReducer } from "../app/state/app-state";
import { useNoteProcessingEvents } from "../app/use-note-processing-events";
import { useSessionMetadata } from "../app/use-session-metadata";
import type { ClovyDataChange } from "../lib/clovy-data-events";
import { resetCurrentDataPartitionForTests } from "../lib/data-partition";
import {
  NOTE_PROCESSING_PROGRESS_EVENT,
  type NoteDto,
  type NoteProcessingProgressDto,
} from "../lib/tauri";

const mocks = vi.hoisted(() => ({
  listen: vi.fn(),
  getNote: vi.fn(),
  listNotes: vi.fn(),
  listCompletedSessions: vi.fn(),
  listSessionFolders: vi.fn(),
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: mocks.listen }));
vi.mock("../lib/tauri", async (original) => ({
  ...(await original<typeof import("../lib/tauri")>()),
  ...mocks,
}));
const listeners = new Set<{
  name: string;
  callback: (event: { payload: ClovyDataChange | NoteProcessingProgressDto }) => void;
}>();
function deferred<T>() {
  let resolve: (value: T) => void = () => {
    throw new Error("Deferred not initialized");
  };
  const promise = new Promise<T>((finish) => {
    resolve = finish;
  });
  return { promise, resolve };
}
async function emit(change: ClovyDataChange) {
  await act(async () => {
    for (const listener of listeners)
      if (listener.name === "clovy-data-changed") listener.callback({ payload: change });
  });
}
const stamp = "2026-11-01T00:00:00Z";
function note(content: string): NoteDto {
  return {
    id: "note",
    title: content,
    preview: content,
    editedContent: content,
    activeTab: "notes",
    processingStatus: "ready",
    createdAt: stamp,
    updatedAt: stamp,
    folderIds: [],
  };
}
function useNotes() {
  const [state, dispatch] = useReducer(notesReducer, {
    ...createInitialState(),
    notes: [note("original")],
    selectedNote: note("original"),
    selectedNoteId: "note",
  });
  const [error, setError] = useState<string | null>(null);
  const controller = useRef(
    new NoteSaveController({
      persist: async () => {
        throw new Error("Unexpected save");
      },
      debounceMs: 60_000,
    }),
  ).current;
  useNoteProcessingEvents({
    dispatch,
    noteSaveController: controller,
    notes: state.notes,
    setError,
  });
  return { state, dispatch, controller, error };
}
beforeEach(() => {
  vi.clearAllMocks();
  listeners.clear();
  resetCurrentDataPartitionForTests();
  mocks.listen.mockImplementation(async (name, callback) => {
    const entry = { name, callback };
    listeners.add(entry);
    return () => listeners.delete(entry);
  });
  mocks.listNotes.mockResolvedValue({ items: [note("new")], hasMore: false });
  mocks.listSessionFolders.mockResolvedValue([]);
});

describe("native mutation invalidation", () => {
  it("does not resurrect deleted notes from a delayed terminal hydration or its retry", async () => {
    const old = deferred<NoteDto>();
    mocks.getNote.mockReturnValueOnce(old.promise);
    const { result } = renderHook(useNotes);
    const progress = {
      noteId: "note",
      recordingSessionId: "take",
      stage: "done",
      revision: stamp,
      processingStatus: "ready",
    } as NoteProcessingProgressDto;
    await act(async () => {
      for (const listener of listeners)
        if (listener.name === NOTE_PROCESSING_PROGRESS_EVENT)
          listener.callback({ payload: progress });
    });
    await waitFor(() => expect(mocks.getNote).toHaveBeenCalledTimes(1));
    mocks.listNotes.mockResolvedValue({ items: [], hasMore: false });
    await emit({ profile: "default", kind: "notes", id: "note", deleted: true });
    await act(async () => {
      old.resolve(note("stale completed result"));
    });
    expect(result.current.state.notes).toEqual([]);
    expect(result.current.state.selectedNote).toBeUndefined();
    await act(async () => {
      for (const listener of listeners)
        if (listener.name === NOTE_PROCESSING_PROGRESS_EVENT)
          listener.callback({ payload: progress });
      window.dispatchEvent(new Event("focus"));
    });
    expect(mocks.getNote).toHaveBeenCalledTimes(1);
    expect(result.current.error).toBeNull();
  });
  it("does not resurrect a deleted note from an older hydration response", async () => {
    const old = deferred<NoteDto>();
    mocks.getNote.mockReturnValueOnce(old.promise);
    const { result } = renderHook(useNotes);
    await emit({ profile: "default", kind: "notes", id: "note" });
    await waitFor(() => expect(mocks.getNote).toHaveBeenCalledTimes(1));
    mocks.listNotes.mockResolvedValue({ items: [], hasMore: false });
    await emit({ profile: "default", kind: "notes", id: "note", deleted: true });
    expect(result.current.state.selectedNote).toBeUndefined();
    await act(async () => {
      old.resolve(note("stale"));
    });
    expect(result.current.state.notes).toEqual([]);
    expect(result.current.state.selectedNote).toBeUndefined();
  });

  it("keeps the newest response when two native edits finish out of order", async () => {
    const old = deferred<NoteDto>();
    const latest = deferred<NoteDto>();
    mocks.getNote.mockReturnValueOnce(old.promise).mockReturnValueOnce(latest.promise);
    const { result } = renderHook(useNotes);
    await emit({ profile: "default", kind: "notes", id: "note" });
    await waitFor(() => expect(mocks.getNote).toHaveBeenCalledTimes(1));
    await emit({ profile: "default", kind: "notes", id: "note" });
    await waitFor(() => expect(mocks.getNote).toHaveBeenCalledTimes(2));
    await act(async () => {
      latest.resolve(note("new"));
    });
    expect(result.current.state.selectedNote?.editedContent).toBe("new");
    await act(async () => {
      old.resolve(note("old"));
    });
    expect(result.current.state.selectedNote?.editedContent).toBe("new");
  });

  it("preserves an optimistic edit queued while the native snapshot is loading", async () => {
    const loaded = deferred<NoteDto>();
    mocks.getNote.mockReturnValueOnce(loaded.promise);
    const { result } = renderHook(useNotes);
    await emit({ profile: "default", kind: "notes", id: "note" });
    await waitFor(() => expect(mocks.getNote).toHaveBeenCalledTimes(1));
    act(() => {
      result.current.controller.queue("note", { title: "local", editedContent: "local" });
      result.current.dispatch({ type: "noteUpdated", note: note("local") });
    });
    await act(async () => {
      loaded.resolve(note("old database snapshot"));
    });
    expect(result.current.state.selectedNote?.editedContent).toBe("local");
    expect(result.current.state.notes[0].title).toBe("local");
    result.current.controller.discard("note");
  });

  it("reconciles every conversation invalidated during an overlapping archive reload", async () => {
    const first = deferred<Array<{ sessionId: string; completedAt: string }>>();
    mocks.listCompletedSessions.mockReturnValueOnce(first.promise).mockResolvedValueOnce([
      { sessionId: "a", completedAt: stamp },
      { sessionId: "b", completedAt: stamp },
    ]);
    const { result } = renderHook(() => {
      const [completed, setCompletedSessions] = useState<Record<string, string>>({});
      const [, setSessionFolders] = useState<Record<string, string[]>>({});
      const [, setError] = useState<string | null>(null);
      const touched = useRef(new Set<string>());
      useSessionMetadata({
        appBlocked: false,
        bootstrapped: false,
        sessionCompletionTouchedRef: touched,
        setCompletedSessions,
        setSessionFolders,
        setError,
      });
      return completed;
    });
    await emit({ profile: "default", kind: "conversations", id: "a" });
    await waitFor(() => expect(mocks.listCompletedSessions).toHaveBeenCalledTimes(1));
    await emit({ profile: "default", kind: "conversations", id: "b" });
    await waitFor(() => expect(result.current).toEqual({ a: stamp, b: stamp }));
    await act(async () => {
      first.resolve([]);
    });
    expect(result.current).toEqual({ a: stamp, b: stamp });
  });
});
