import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useRef } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useAgentSessionSync } from "../app/use-agent-session-sync";
import type { UseAgentSessionSyncDependencies } from "../app/use-agent-session-sync-types";
import { AgentWorkspace, AGENT_RUNTIME_EVENT } from "../components/agent/AgentWorkspace";
import { Sidebar } from "../components/sidebar/Sidebar";
import { AGENT_SESSIONS_CHANGED_EVENT, AGENT_SESSIONS_HYDRATED_EVENT } from "../lib/agent-events";
import type { AgentRuntimeEvent, AgentSessionDto } from "../lib/agent-runtime-contract";
import {
  readAgentSessionDraft,
  resetAgentSessionDraftsForTests,
  writeAgentSessionDraft,
} from "../lib/agent-session-drafts";
import {
  resetCurrentDataPartitionForTests,
  setCurrentDataPartitionName,
} from "../lib/data-partition";

const mocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  listen: vi.fn(),
  selected: vi.fn(),
  homeCreated: vi.fn(),
}));
vi.mock("@tauri-apps/api/core", () => ({
  Channel: class {
    onmessage?: (event: unknown) => void;
  },
  convertFileSrc: (path: string) => path,
  invoke: mocks.invoke,
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: mocks.listen, emit: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

const claude = "__clovy_cli_engine__:claude";
const a: AgentSessionDto = {
  id: "a-external",
  title: "Running A",
  status: "running",
  model: claude,
  safetyMode: "sandboxed",
  workspacePath: "/tmp/clovy-t4-a",
  source: "user",
  createdAt: "2026-11-01T00:00:00Z",
  updatedAt: "2026-11-01T00:00:00Z",
};
const b: AgentSessionDto = { ...a, id: "b-external", title: "Created B", status: "idle" };
type Change = { profile: string; kind: string; id?: string; deleted?: boolean };
const nativeListeners = new Set<(event: { payload: Change }) => void>();
const runtimeListeners = new Set<(event: { payload: AgentRuntimeEvent }) => void>();
let persisted: AgentSessionDto[];
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((finish, fail) => {
    resolve = finish;
    reject = fail;
  });
  return { promise, resolve, reject };
}
async function change(payload: Change) {
  await act(async () => {
    for (const listener of nativeListeners) listener({ payload });
  });
}
function Surface({
  workspace = false,
  home = false,
  initialSession,
}: {
  workspace?: boolean;
  home?: boolean;
  initialSession?: AgentSessionDto;
}) {
  const deps = useRef<UseAgentSessionSyncDependencies>({
    activeViewRef: { current: workspace ? "agent" : "notes" },
    activeAgentSessionIdRef: { current: workspace ? undefined : a.id },
    tabsRef: { current: [] },
    setTabs: vi.fn(),
    agentMenuBarLastStatusRef: { current: undefined },
    agentMenuBarSessionsRef: { current: [a] },
    agentMenuBarWorkingSessionIdsRef: { current: new Set([a.id]) },
    agentMenuBarWaitingSessionIdsRef: { current: new Set() },
    commitAgentSessions: vi.fn(),
    pendingSessionProjectRef: { current: null },
    publishAgentMenuBarState: vi.fn(),
    refreshSessionPartitions: async () => ({}),
    setActiveAgentSession: mocks.selected,
    setActiveAgentSessionId: mocks.selected,
    setActiveAgentSessionSeed: vi.fn(),
    setAgentOrigin: vi.fn(),
    setAgentSessions: vi.fn(),
    setAgentWaitingSessionIds: vi.fn(),
    setAgentWorkingSessionIds: vi.fn(),
    setSessionFolders: vi.fn(),
  }).current;
  useAgentSessionSync(deps);
  return workspace ? (
    <AgentWorkspace
      initialSession={initialSession}
      homeMode={home}
      onSessionSelected={mocks.selected}
      onHomeSessionCreated={mocks.homeCreated}
    />
  ) : (
    <Sidebar
      notes={[]}
      activeView="notes"
      onChangeView={vi.fn()}
      onSelectNote={vi.fn()}
      onNewAgentSession={vi.fn()}
      onRenameAgentSession={vi.fn()}
      onSelectAgentSession={vi.fn()}
    />
  );
}
beforeEach(() => {
  vi.clearAllMocks();
  window.localStorage.clear();
  resetCurrentDataPartitionForTests();
  resetAgentSessionDraftsForTests();
  persisted = [a];
  nativeListeners.clear();
  runtimeListeners.clear();
  mocks.listen.mockImplementation(async (name, callback) => {
    if (name === "clovy-data-changed") nativeListeners.add(callback);
    if (name === AGENT_RUNTIME_EVENT) runtimeListeners.add(callback);
    return () => {
      nativeListeners.delete(callback);
      runtimeListeners.delete(callback);
    };
  });
  mocks.invoke.mockImplementation(
    async (command: string, args?: { sessionId?: string; request?: { sessionId: string } }) => {
      switch (command) {
        case "list_agent_sessions":
          return [...persisted];
        case "list_session_profiles":
          return [];
        case "get_agent_session":
          return persisted.find((row) => row.id === args?.sessionId);
        case "list_agent_items":
        case "list_agent_artifacts":
        case "list_agent_skills":
          return [];
        case "chat_default_engine":
          return { modelId: claude, cli: "claude", legacyFallback: false };
        case "list_provider_models":
          return { mode: "generation", selectedModel: claude, models: [] };
        case "provider_model_settings":
          return { settings: {}, effectiveSettings: { providerApiKeysConfigured: {} } };
        case "get_latest_agent_run":
          return null;
        case "start_agent_run": {
          const sessionId = args?.request?.sessionId;
          if (!persisted.some((row) => row.id === sessionId)) {
            throw { code: "conversation_not_found", message: "Conversation was not found." };
          }
          return { id: "run-existing", sessionId, status: "running", model: claude };
        }
        default:
          return undefined;
      }
    },
  );
});
describe("external conversation mutations", () => {
  it("publishes MCP create and rename snapshots to Sidebar outside Workspace while A stays running", async () => {
    writeAgentSessionDraft(a.id, "Keep A's draft");
    render(<Surface />);
    await act(async () => {
      window.dispatchEvent(
        new CustomEvent(AGENT_SESSIONS_CHANGED_EVENT, {
          detail: {
            sessions: [a],
            selectedSessionId: a.id,
            workingSessionIds: [a.id],
            waitingSessionIds: [],
          },
        }),
      );
    });
    expect(await screen.findByText(a.title)).toBeVisible();
    persisted = [a, b];
    await change({ profile: "default", kind: "conversations", id: b.id });
    expect(await screen.findByText(b.title)).toBeVisible();
    persisted = [a, { ...b, title: "Renamed B" }];
    await change({ profile: "default", kind: "conversations", id: b.id });
    expect(await screen.findByText("Renamed B")).toBeVisible();
    expect(screen.queryByText(b.title)).not.toBeInTheDocument();
    const rowA = screen.getByText(a.title).closest("article");
    expect(rowA).toHaveAttribute("data-status", "running");
    expect(within(rowA as HTMLElement).getByRole("status", { name: "Working" })).toBeVisible();
    expect(mocks.selected).not.toHaveBeenCalled();
    expect(readAgentSessionDraft(a.id)).toBe("Keep A's draft");
    // A late startup snapshot cannot undo the accepted external snapshot.
    act(() =>
      window.dispatchEvent(
        new CustomEvent(AGENT_SESSIONS_HYDRATED_EVENT, {
          detail: { sessions: [a], partitions: {} },
        }),
      ),
    );
    expect(screen.getByText("Renamed B")).toBeVisible();
  });

  it("keeps the accepted MCP snapshot when a background Workspace's older list resolves", async () => {
    const oldList = deferred<AgentSessionDto[]>();
    const fallback = mocks.invoke.getMockImplementation();
    let holdList = true;
    let listRequested = false;
    mocks.invoke.mockImplementation((command: string, args?: unknown) => {
      if (command === "list_agent_sessions" && holdList) {
        listRequested = true;
        return oldList.promise;
      }
      return fallback?.(command, args);
    });
    render(
      <>
        <Surface />
        <div hidden>
          <AgentWorkspace initialSession={a} />
        </div>
      </>,
    );
    await waitFor(() => expect(listRequested).toBe(true));
    holdList = false;
    persisted = [a, b];
    await change({ profile: "default", kind: "conversations", id: b.id });
    const sidebar = screen.getByRole("complementary");
    expect(await within(sidebar).findByText(b.title)).toBeVisible();
    await act(async () => oldList.resolve([a]));
    expect(within(sidebar).getByText(b.title)).toBeVisible();
    expect(mocks.selected).not.toHaveBeenCalledWith(undefined);
  });

  it("keeps a pending MCP refresh authoritative when the older Workspace response arrives first", async () => {
    const oldList = deferred<AgentSessionDto[]>();
    const nativeList = deferred<AgentSessionDto[]>();
    const fallback = mocks.invoke.getMockImplementation();
    let nativePhase = false;
    let oldReadStarted = false;
    let nativeReadStarted = false;
    mocks.invoke.mockImplementation((command: string, args?: unknown) => {
      if (command === "list_agent_sessions") {
        if (!nativePhase) {
          oldReadStarted = true;
          return oldList.promise;
        }
        nativeReadStarted = true;
        return nativeList.promise;
      }
      return fallback?.(command, args);
    });
    render(
      <>
        <Surface />
        <div hidden>
          <AgentWorkspace initialSession={a} />
        </div>
      </>,
    );
    await waitFor(() => expect(oldReadStarted).toBe(true));
    nativePhase = true;
    persisted = [a, b];
    await change({ profile: "default", kind: "conversations", id: b.id });
    await waitFor(() => expect(nativeReadStarted).toBe(true));
    await act(async () => oldList.resolve([a]));
    await act(async () => nativeList.resolve([a, b]));
    expect(await within(screen.getByRole("complementary")).findByText(b.title)).toBeVisible();
    expect(mocks.selected).not.toHaveBeenCalledWith(undefined);
  });

  it("publishes a post-event terminal read after the pending MCP snapshot and keeps C working", async () => {
    const c: AgentSessionDto = { ...a, id: "c-external", title: "Running C" };
    const doneA: AgentSessionDto = { ...a, status: "idle", updatedAt: "2026-11-01T00:01:00Z" };
    const mcpSnapshot = deferred<AgentSessionDto[]>();
    const terminalSnapshot = deferred<AgentSessionDto[]>();
    const fallback = mocks.invoke.getMockImplementation();
    const selected = vi.fn();
    let phase: "initial" | "mcp" | "terminal" = "initial";
    let mcpReadStarted = false;
    let terminalReadStarted = false;
    persisted = [a, c, b];
    mocks.invoke.mockImplementation((command: string, args?: unknown) => {
      if (command === "list_agent_sessions" && phase === "mcp") {
        mcpReadStarted = true;
        return mcpSnapshot.promise;
      }
      if (command === "list_agent_sessions" && phase === "terminal") {
        terminalReadStarted = true;
        return terminalSnapshot.promise;
      }
      return fallback?.(command, args);
    });
    writeAgentSessionDraft(a.id, "Keep A's draft");
    writeAgentSessionDraft(c.id, "Keep C's draft");
    writeAgentSessionDraft(b.id, "Keep the viewed conversation draft");
    render(
      <>
        <Surface />
        <div hidden>
          <AgentWorkspace initialSession={b} onSessionSelected={selected} />
        </div>
      </>,
    );
    const sidebar = screen.getByRole("complementary");
    const row = (session: AgentSessionDto) =>
      within(sidebar).getByText(session.title).closest("article") as HTMLElement;
    await waitFor(() => {
      expect(within(row(a)).getByRole("status", { name: "Working" })).toBeVisible();
      expect(within(row(c)).getByRole("status", { name: "Working" })).toBeVisible();
      expect(selected).toHaveBeenLastCalledWith(b);
    });

    phase = "mcp";
    await change({ profile: "default", kind: "conversations", id: b.id });
    await waitFor(() => expect(mcpReadStarted).toBe(true));
    // Native terminal persistence has completed before publishing this real
    // runtime event. Its consumer starts the newer list read in the MCP epoch.
    phase = "terminal";
    persisted = [doneA, c, b];
    const frame: AgentRuntimeEvent = {
      protocolVersion: 1,
      sessionId: a.id,
      runId: "run-a",
      sequence: 2,
      eventId: "run-a-completed",
      method: "run.completed",
      data: { completedAt: doneA.updatedAt },
    };
    await act(async () => {
      for (const listener of runtimeListeners) listener({ payload: frame });
    });
    await waitFor(() => expect(terminalReadStarted).toBe(true));
    // The earlier MCP read still sees A working. It must not invalidate the
    // valid terminal read that was admitted after the native mutation event.
    await act(async () => mcpSnapshot.resolve([a, c, b]));
    expect(within(row(a)).getByRole("status", { name: "Working" })).toBeVisible();
    await act(async () => terminalSnapshot.resolve([doneA, c, b]));
    expect(within(row(a)).queryByRole("status", { name: "Working" })).not.toBeInTheDocument();
    expect(within(row(a)).getByRole("status", { name: "New reply" })).toBeVisible();
    expect(within(row(c)).getByRole("status", { name: "Working" })).toBeVisible();
    expect(selected).toHaveBeenLastCalledWith(b);
    expect(mocks.selected).not.toHaveBeenCalled();
    expect(readAgentSessionDraft(a.id)).toBe("Keep A's draft");
    expect(readAgentSessionDraft(c.id)).toBe("Keep C's draft");
    expect(readAgentSessionDraft(b.id)).toBe("Keep the viewed conversation draft");
  });

  it.each([
    "resolve",
    "reject",
  ])("discards a foreign-partition API read that returns after default hydration (%s)", async (outcome) => {
    // API/store fixture only: this does not claim a live profile-switch UI;
    // native active_profile still returns the literal default partition.
    const c: AgentSessionDto = { ...a, id: "c-partition", title: "Working C" };
    const foreign: AgentSessionDto = {
      ...a,
      id: "foreign-partition",
      title: "Foreign conversation",
    };
    const doneA: AgentSessionDto = { ...a, status: "idle" };
    const nativeSnapshot = deferred<AgentSessionDto[]>();
    const foreignRead = deferred<AgentSessionDto[]>();
    const fallback = mocks.invoke.getMockImplementation();
    const selected = vi.fn();
    const changed = vi.fn();
    let phase: "initial" | "native" | "foreign" = "initial";
    let nativeStarted = false;
    let foreignStarted = false;
    persisted = [a, c, b];
    mocks.invoke.mockImplementation((command: string, args?: unknown) => {
      if (command === "list_session_profiles")
        return Promise.resolve([{ sessionId: foreign.id, profile: "other" }]);
      if (command === "list_agent_sessions" && phase === "native") {
        nativeStarted = true;
        return nativeSnapshot.promise;
      }
      if (command === "list_agent_sessions" && phase === "foreign") {
        foreignStarted = true;
        return foreignRead.promise;
      }
      if (command === "get_latest_agent_run" && phase === "foreign")
        return Promise.resolve({
          id: "run-partition-a",
          sessionId: a.id,
          status: "completed",
          model: claude,
          completedAt: "2026-11-01T00:01:00Z",
        });
      return fallback?.(command, args);
    });
    writeAgentSessionDraft(a.id, "Keep selected A");
    writeAgentSessionDraft(c.id, "Keep working C");
    render(
      <>
        <Surface />
        <div hidden>
          <AgentWorkspace initialSession={a} onSessionSelected={selected} />
        </div>
      </>,
    );
    const sidebar = screen.getByRole("complementary");
    const row = (session: AgentSessionDto) =>
      within(sidebar).getByText(session.title).closest("article") as HTMLElement;
    await waitFor(() => {
      expect(within(row(c)).getByRole("status", { name: "Working" })).toBeVisible();
      expect(selected).toHaveBeenLastCalledWith(a);
    });
    phase = "native";
    await change({ profile: "default", kind: "conversations", id: b.id });
    await waitFor(() => expect(nativeStarted).toBe(true));
    act(() => setCurrentDataPartitionName("other"));
    phase = "foreign";
    persisted = [doneA, c, b];
    const frame: AgentRuntimeEvent = {
      protocolVersion: 1,
      sessionId: a.id,
      runId: "run-partition-a",
      sequence: 2,
      eventId: "partition-run-completed",
      method: "run.completed",
      data: { completedAt: "2026-11-01T00:01:00Z" },
    };
    await act(async () => {
      for (const listener of runtimeListeners) listener({ payload: frame });
    });
    await waitFor(() => expect(foreignStarted).toBe(true));
    act(() => setCurrentDataPartitionName("default"));
    await act(async () => nativeSnapshot.resolve([a, c, b]));
    window.addEventListener(AGENT_SESSIONS_CHANGED_EVENT, changed);
    try {
      await act(async () => {
        if (outcome === "resolve") foreignRead.resolve([foreign]);
        else foreignRead.reject(new Error("Foreign partition list failed"));
      });
      expect(changed).not.toHaveBeenCalled();
      expect(within(sidebar).getByText(a.title)).toBeVisible();
      expect(within(row(c)).getByRole("status", { name: "Working" })).toBeVisible();
      expect(within(sidebar).queryByText(foreign.title)).not.toBeInTheDocument();
      expect(screen.queryByText("Foreign partition list failed")).not.toBeInTheDocument();
      expect(selected.mock.calls.every(([session]) => session?.id === a.id)).toBe(true);
      expect(mocks.selected).not.toHaveBeenCalled();
      expect(readAgentSessionDraft(a.id)).toBe("Keep selected A");
      expect(readAgentSessionDraft(c.id)).toBe("Keep working C");
    } finally {
      window.removeEventListener(AGENT_SESSIONS_CHANGED_EVENT, changed);
    }
  });

  it("rejects a deleted delayed branch before replacing the source draft or selection", async () => {
    const parent: AgentSessionDto = { ...a, id: "branch-source", title: "Source", status: "idle" };
    const pending = deferred<AgentSessionDto>();
    const fallback = mocks.invoke.getMockImplementation();
    let branches = 0;
    persisted = [a, parent];
    mocks.invoke.mockImplementation((command: string, args?: unknown) => {
      if (command === "branch_agent_session") {
        branches += 1;
        persisted = [a, parent, b];
        return pending.promise;
      }
      if (command === "list_agent_items")
        return Promise.resolve([
          {
            id: "branch-answer",
            sessionId: parent.id,
            sequence: 0,
            kind: "message",
            role: "assistant",
            text: "An earlier answer",
            status: "complete",
            createdAt: a.createdAt,
          },
        ]);
      return fallback?.(command, args);
    });
    writeAgentSessionDraft(a.id, "Keep running A");
    writeAgentSessionDraft(parent.id, "Keep the source draft");
    render(<Surface workspace initialSession={parent} />);
    const user = userEvent.setup();
    await waitFor(() =>
      expect(screen.getByRole("textbox", { name: "Message Clovy" })).toHaveTextContent(
        "Keep the source draft",
      ),
    );
    await user.click(await screen.findByRole("button", { name: "Branch from here" }));
    await waitFor(() => expect(branches).toBe(1));
    persisted = [a, parent];
    await change({ profile: "default", kind: "conversations", id: b.id, deleted: true });
    await act(async () => pending.resolve(b));
    expect(screen.getByRole("textbox", { name: "Message Clovy" })).toHaveTextContent(
      "Keep the source draft",
    );
    expect(
      await screen.findByText("The new conversation was deleted before it could be opened."),
    ).toBeVisible();
    expect(screen.getByRole("textbox", { name: "Message Clovy" })).toHaveTextContent(
      "Keep the source draft",
    );
    expect(mocks.selected.mock.calls.some(([value]) => value?.id === b.id)).toBe(false);
    expect(readAgentSessionDraft(parent.id)).toBe("Keep the source draft");
    expect(readAgentSessionDraft(a.id)).toBe("Keep running A");
    expect(mocks.invoke.mock.calls.filter(([name]) => name === "start_agent_run")).toHaveLength(0);
    expect(branches).toBe(1);
  });

  it.each([
    "conversation",
    "Home",
    "retry",
  ])("rejects a tombstoned delayed %s create result and preserves the unsent prompt", async (route) => {
    const pending = deferred<AgentSessionDto>();
    const fallback = mocks.invoke.getMockImplementation();
    let creates = 0;
    mocks.invoke.mockImplementation((command: string, args?: unknown) => {
      if (command === "create_agent_session") {
        creates += 1;
        if (route === "retry" && creates === 1)
          return Promise.reject(new Error("Temporary create failure"));
        persisted = [a, b];
        return pending.promise;
      }
      return fallback?.(command, args);
    });
    writeAgentSessionDraft(a.id, "A is still running");
    render(<Surface workspace home={route === "Home"} />);
    const user = userEvent.setup();
    const composer = await screen.findByRole("textbox", { name: "Message Clovy" });
    await user.type(composer, "Preserve this unsent request");
    await user.click(screen.getByRole("button", { name: "Send message" }));
    if (route === "retry") {
      await screen.findByText("Temporary create failure");
      await user.click(screen.getByRole("button", { name: "Send message" }));
    }
    await waitFor(() => expect(creates).toBe(route === "retry" ? 2 : 1));
    // The synthetic command has persisted B; another bound caller deletes it
    // before the original response returns to the real Workspace consumer.
    persisted = [a];
    await change({ profile: "default", kind: "conversations", id: b.id, deleted: true });
    await act(async () => pending.resolve(b));
    await screen.findByRole("alert");
    expect(mocks.invoke.mock.calls.filter(([name]) => name === "start_agent_run")).toHaveLength(0);
    expect(
      screen.getByText("This conversation was deleted. Your message was not sent."),
    ).toBeVisible();
    expect(composer).toHaveTextContent("Preserve this unsent request");
    expect(creates).toBe(route === "retry" ? 2 : 1);
    expect(mocks.selected.mock.calls.some(([value]) => value?.id === b.id)).toBe(false);
    expect(mocks.homeCreated).not.toHaveBeenCalled();
    expect(readAgentSessionDraft(b.id)).toBeUndefined();
    expect(readAgentSessionDraft(a.id)).toBe("A is still running");
    expect(persisted).toEqual([a]);
  });
});
