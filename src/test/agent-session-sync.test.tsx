import { act, renderHook } from "@testing-library/react";
import { useState } from "react";
import { describe, expect, it, vi } from "vitest";
import { useAgentSessionSync } from "../app/use-agent-session-sync";
import type { AgentSessionDto } from "../lib/agent-runtime-contract";
import { AGENT_SESSIONS_CHANGED_EVENT, dispatchAgentSessionStatus } from "../lib/agent-events";

vi.mock("@tauri-apps/api/event", () => ({
  emit: vi.fn(async () => undefined),
  listen: vi.fn(async () => () => {}),
}));

function useSyncedSessionIds(overrides: Partial<Parameters<typeof useAgentSessionSync>[0]> = {}) {
  const [working, setWorking] = useState<ReadonlySet<string>>(new Set());
  const [waiting, setWaiting] = useState<ReadonlySet<string>>(new Set());
  const ref = <T,>(current: T) => ({ current });
  useAgentSessionSync({
    activeViewRef: ref("home"),
    agentMenuBarLastStatusRef: ref(undefined),
    agentMenuBarSessionsRef: ref([]),
    agentMenuBarWaitingSessionIdsRef: useState(() => ref(new Set<string>()))[0],
    agentMenuBarWorkingSessionIdsRef: useState(() => ref(new Set<string>()))[0],
    commitAgentSessions: vi.fn(),
    pendingSessionProjectRef: ref(null),
    publishAgentMenuBarState: vi.fn(),
    refreshSessionPartitions: vi.fn(async () => ({})),
    setActiveAgentSession: vi.fn(),
    setActiveAgentSessionId: vi.fn(),
    setActiveAgentSessionSeed: vi.fn(),
    setAgentOrigin: vi.fn(),
    setAgentSessions: vi.fn(),
    setAgentWaitingSessionIds: setWaiting,
    setAgentWorkingSessionIds: setWorking,
    setSessionFolders: vi.fn(),
    ...overrides,
  });
  return { working, waiting };
}

describe("agent session sync", () => {
  it("keeps the same working set while a run keeps reporting the same status", () => {
    const { result } = renderHook(() => useSyncedSessionIds());

    act(() => dispatchAgentSessionStatus({ sessionId: "session-1", status: "running" }));
    const working = result.current.working;
    const waiting = result.current.waiting;
    expect([...working]).toEqual(["session-1"]);

    act(() => dispatchAgentSessionStatus({ sessionId: "session-1", status: "running" }));
    expect(result.current.working).toBe(working);
    expect(result.current.waiting).toBe(waiting);

    act(() => dispatchAgentSessionStatus({ sessionId: "session-1", status: "completed" }));
    expect(result.current.working.size).toBe(0);
  });

  it("commits the newest session list when an older broadcast's map read resolves last", async () => {
    const reads: ((partitions: Record<string, string>) => void)[] = [];
    const refreshSessionPartitions = vi.fn(
      () => new Promise<Record<string, string>>((resolve) => reads.push(resolve)),
    );
    const commitAgentSessions = vi.fn();
    renderHook(() => useSyncedSessionIds({ commitAgentSessions, refreshSessionPartitions }));
    const broadcast = (title: string) =>
      window.dispatchEvent(
        new CustomEvent(AGENT_SESSIONS_CHANGED_EVENT, {
          detail: {
            sessions: [{ id: "session-1", title } as AgentSessionDto],
            workingSessionIds: [],
          },
        }),
      );

    act(() => {
      broadcast("Older title");
      broadcast("Newer title");
    });
    await act(async () => {
      reads[1]({});
      await Promise.resolve();
    });
    await act(async () => {
      reads[0]({});
      await Promise.resolve();
    });

    expect(commitAgentSessions.mock.calls.map(([sessions]) => sessions[0].title)).toEqual([
      "Newer title",
    ]);
  });
});
