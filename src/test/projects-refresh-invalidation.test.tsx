import { act, renderHook, waitFor } from "@testing-library/react";
import { useCallback, useReducer, useRef, useState } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { NoteSaveController } from "../app/note-save-controller";
import { createInitialState, notesReducer } from "../app/state/app-state";
import { useAgentSessionSync } from "../app/use-agent-session-sync";
import { useDataPartitionRefresh } from "../app/use-data-partition-refresh";
import { useNoteProcessingEvents } from "../app/use-note-processing-events";
import type { Tab } from "../app/tabs/tabs";
import type { AgentSessionDto } from "../lib/agent-runtime-contract";
import {
  readAgentSessionDraft,
  resetAgentSessionDraftsForTests,
  writeAgentSessionDraft,
} from "../lib/agent-session-drafts";
import type { ClovyDataChange } from "../lib/clovy-data-events";
import { resetCurrentDataPartitionForTests } from "../lib/data-partition";
import type { FolderDto, LiveTranscriptEventDto, NoteDto } from "../lib/tauri";

const mocks = vi.hoisted(() => ({
  listen: vi.fn(),
  getNote: vi.fn(),
  listNotes: vi.fn(),
  listFolders: vi.fn(),
  listAgentSessions: vi.fn(),
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: mocks.listen }));
vi.mock("../lib/tauri", async (original) => ({
  ...(await original<typeof import("../lib/tauri")>()),
  ...mocks,
}));
const listeners = new Set<(event: { payload: ClovyDataChange }) => void>();
const stamp = "2026-11-01T00:00:00Z";
const a: AgentSessionDto = {
  id: "a",
  title: "A",
  status: "idle",
  model: "test",
  safetyMode: "sandboxed",
  source: "user",
  workspacePath: "/tmp/clovy-t4-synthetic-workspace",
  createdAt: stamp,
  updatedAt: stamp,
};
const b: AgentSessionDto = { ...a, id: "b", title: "B" };
const note: NoteDto = {
  id: "note",
  title: "Note",
  preview: "Note",
  activeTab: "notes",
  processingStatus: "ready",
  createdAt: stamp,
  updatedAt: stamp,
  folderIds: ["project"],
};
const folder: FolderDto = {
  id: "project",
  name: "Project",
  memoryDisabled: false,
  createdAt: stamp,
  updatedAt: stamp,
};
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((finish) => {
    resolve = finish;
  });
  return { promise, resolve };
}
async function emit(payload: ClovyDataChange) {
  await act(async () => {
    for (const listener of listeners) listener({ payload });
  });
}
function useSurfaces(selected = "b") {
  const [state, dispatch] = useReducer(notesReducer, {
    ...createInitialState(),
    notes: [note],
    selectedNote: note,
    selectedNoteId: note.id,
  });
  const [sessions, setAgentSessions] = useState([a, b]);
  const [active, setActive] = useState<string | undefined>(selected);
  const [seed, setSeed] = useState<AgentSessionDto | undefined>(selected === "a" ? a : b);
  const activeId = useRef<string | undefined>(selected);
  const [tabs, setTabs] = useState<Tab[]>([
    { id: "tab-a", nav: { view: "agent", agentSessionId: "a" } },
    { id: "tab-b", nav: { view: "agent", agentSessionId: "b" } },
  ]);
  const tabsRef = useRef(tabs);
  tabsRef.current = tabs;
  const activeViewRef = useRef<"agent">("agent");
  const sessionRows = useRef([a, b]);
  const working = useRef(new Set<string>());
  const waiting = useRef(new Set<string>());
  const lastStatus = useRef(undefined);
  const pendingProject = useRef(null);
  const [error, setError] = useState<string | null>(null);
  const [liveTranscriptEvents, setLiveTranscriptEvents] = useState<LiveTranscriptEventDto[]>([]);
  const [, setOrigin] = useState<{ kind: "project"; folderId: string } | { kind: "routines" }>();
  const [, setSessionFolders] = useState<Record<string, string[]>>({});
  const [, setWorking] = useState<ReadonlySet<string>>(new Set());
  const [, setWaiting] = useState<ReadonlySet<string>>(new Set());
  const publish = useCallback(() => {}, []);
  const commit = useCallback((rows: readonly AgentSessionDto[]) => setAgentSessions([...rows]), []);
  const refreshPartitions = useCallback(async () => ({}), []);
  const setActiveSession = useCallback((session: AgentSessionDto | undefined) => {
    activeId.current = session?.id;
    setActive(session?.id);
    setSeed(session);
  }, []);
  const controller = useRef(
    new NoteSaveController({
      persist: async () => {
        throw Error("Unexpected save");
      },
      debounceMs: 60_000,
    }),
  ).current;
  useNoteProcessingEvents({
    dispatch,
    noteSaveController: controller,
    notes: state.notes,
    selectedNote: state.selectedNote,
    liveTranscriptEvents,
    setLiveTranscriptEvents,
    setError,
  });
  useAgentSessionSync({
    activeViewRef,
    activeAgentSessionIdRef: activeId,
    tabsRef,
    setTabs,
    agentMenuBarLastStatusRef: lastStatus,
    agentMenuBarSessionsRef: sessionRows,
    agentMenuBarWaitingSessionIdsRef: waiting,
    agentMenuBarWorkingSessionIdsRef: working,
    commitAgentSessions: commit,
    pendingSessionProjectRef: pendingProject,
    publishAgentMenuBarState: publish,
    refreshSessionPartitions: refreshPartitions,
    setActiveAgentSession: setActiveSession,
    setActiveAgentSessionId: setActive,
    setActiveAgentSessionSeed: setSeed,
    setAgentOrigin: setOrigin,
    setAgentSessions,
    setAgentWaitingSessionIds: setWaiting,
    setAgentWorkingSessionIds: setWorking,
    setSessionFolders,
  });
  useDataPartitionRefresh({
    currentDataPartitionName: "default",
    activeViewRef,
    appBlocked: false,
    bootstrapped: true,
    commitAgentSessions: commit,
    crossPartitionRecordingNoteIdRef: useRef(undefined),
    dispatch,
    lastDataPartitionRef: useRef("default"),
    lastDataPartitionRefreshRevisionRef: useRef(0),
    pendingSessionProjectRef: pendingProject,
    dataPartitionRefreshRevision: 0,
    recordingNoteIdRef: useRef(undefined),
    refreshSessionPartitions: refreshPartitions,
    setActiveAgentSession: setActiveSession,
    setActiveView: vi.fn(),
    setAgentOrigin: setOrigin,
    setError,
    setFolderReturnTarget: vi.fn(),
    setOriginAllNotes: vi.fn(),
    setOriginFolderId: vi.fn(),
    setTabs,
    tabsRef,
  });
  return { state, sessions, active, seed, tabs, error };
}
beforeEach(() => {
  vi.clearAllMocks();
  listeners.clear();
  resetAgentSessionDraftsForTests();
  resetCurrentDataPartitionForTests();
  mocks.listen.mockImplementation(async (name, callback) => {
    if (name !== "clovy-data-changed") return () => {};
    listeners.add(callback);
    return () => listeners.delete(callback);
  });
  mocks.listNotes.mockResolvedValue({ items: [], hasMore: false });
  mocks.listFolders.mockResolvedValue([folder]);
  mocks.listAgentSessions.mockResolvedValue([a]);
});
describe("project refresh and confirmed deletion", () => {
  it("does not restore notes or conversations from an older project refresh", async () => {
    const notes = deferred<{ items: NoteDto[]; hasMore: boolean }>();
    const sessions = deferred<AgentSessionDto[]>();
    const folders = deferred<FolderDto[]>();
    mocks.listNotes.mockReturnValueOnce(notes.promise);
    mocks.listAgentSessions.mockReturnValueOnce(sessions.promise);
    mocks.listFolders.mockReturnValueOnce(folders.promise);
    writeAgentSessionDraft("a", "Keep A");
    writeAgentSessionDraft("b", "Remove B");
    const { result } = renderHook(() => useSurfaces());
    await emit({ profile: "default", kind: "projects", id: "project" });
    await waitFor(() => expect(mocks.listNotes).toHaveBeenCalled());
    await waitFor(() => expect(mocks.listAgentSessions).toHaveBeenCalled());
    await emit({ profile: "default", kind: "notes", id: "note", deleted: true });
    await emit({ profile: "default", kind: "conversations", id: "b", deleted: true });
    await waitFor(() => expect(result.current.state.notes).toEqual([]));
    await waitFor(() => expect(result.current.sessions.map((s) => s.id)).toEqual(["a"]));
    expect(result.current.active).toBeUndefined();
    expect(result.current.seed).toBeUndefined();
    expect(result.current.tabs[1].nav).toEqual({ view: "agent" });
    expect(result.current.tabs[0].nav.agentSessionId).toBe("a");
    await act(async () => {
      notes.resolve({ items: [note], hasMore: false });
      sessions.resolve([a, b]);
      folders.resolve([folder]);
    });
    expect(result.current.state.notes).toEqual([]);
    expect(result.current.sessions.map((s) => s.id)).toEqual(["a"]);
    expect(result.current.state.folders).toEqual([folder]);
    expect(readAgentSessionDraft("b")).toBeUndefined();
    expect(readAgentSessionDraft("a")).toBe("Keep A");
  });
  it("deletes B without closing selected A or clearing A's draft", async () => {
    writeAgentSessionDraft("a", "A remains");
    const { result } = renderHook(() => useSurfaces("a"));
    await emit({ profile: "default", kind: "conversations", id: "b", deleted: true });
    await waitFor(() => expect(result.current.sessions.map((s) => s.id)).toEqual(["a"]));
    expect(result.current.active).toBe("a");
    expect(result.current.seed?.id).toBe("a");
    expect(readAgentSessionDraft("a")).toBe("A remains");
  });
});
