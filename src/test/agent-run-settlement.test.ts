import { afterEach, describe, expect, it, vi } from "vitest";
import { AGENT_RUN_SETTLED_EVENT, type AgentRunSettledDetail } from "../lib/agent-events";
import { saveQueuedAgentFollowUps } from "../lib/agent-follow-up-queue";
import type { AgentRuntimeEvent } from "../lib/agent-runtime-contract";
import { startAgentRunSettlementRelay } from "../lib/agent-run-settlement";

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => () => undefined),
  emit: vi.fn(async () => undefined),
}));

type Handler = (event: { payload: AgentRuntimeEvent }) => void;

function frame(
  sessionId: string,
  runId: string,
  method: AgentRuntimeEvent["method"],
  sequence: number,
): AgentRuntimeEvent {
  const base = { protocolVersion: 1, sessionId, runId, sequence, eventId: `e-${sequence}` };
  switch (method) {
    case "message.delta":
      return {
        ...base,
        method,
        data: { itemId: "i", role: "assistant", delta: "x", createdAt: "now" },
      } as AgentRuntimeEvent;
    case "run.failed":
      return {
        ...base,
        method,
        data: { completedAt: "now", message: "boom", retryable: false },
      } as AgentRuntimeEvent;
    case "steering.consumed":
      return {
        ...base,
        method,
        data: { itemId: "i", messageId: "follow-up-1", text: "also do X", createdAt: "now" },
      } as AgentRuntimeEvent;
    case "run.started":
      return { ...base, method, data: { startedAt: "now", model: "m" } } as AgentRuntimeEvent;
    default:
      return { ...base, method, data: { completedAt: "now" } } as AgentRuntimeEvent;
  }
}

function startRelay(titles: Record<string, string> = {}) {
  let handler: Handler | undefined;
  const dispose = startAgentRunSettlementRelay({
    resolveTitle: (sessionId) => titles[sessionId],
    listen: async (_event, next) => {
      handler = next;
      return () => {
        handler = undefined;
      };
    },
  });
  const settled: AgentRunSettledDetail[] = [];
  const onSettled = (event: Event) =>
    settled.push((event as CustomEvent<AgentRunSettledDetail>).detail);
  window.addEventListener(AGENT_RUN_SETTLED_EVENT, onSettled);
  cleanups.push(() => {
    dispose();
    window.removeEventListener(AGENT_RUN_SETTLED_EVENT, onSettled);
  });
  const send = (event: AgentRuntimeEvent) => handler?.({ payload: event });
  return { send, settled };
}

const cleanups: Array<() => void> = [];

afterEach(() => {
  for (const cleanup of cleanups.splice(0)) cleanup();
  saveQueuedAgentFollowUps({});
});

function queueFollowUp(sessionId: string) {
  saveQueuedAgentFollowUps({
    [sessionId]: {
      messageId: "follow-up-1",
      prompt: "also do X",
      attachments: [],
      model: "m",
      thinkingLevel: "medium",
    },
  });
}

describe("agent run settlement relay", () => {
  it("tells run-settled listeners exactly once when a run completes", async () => {
    const { send, settled } = startRelay({ "session-1": "Plan the launch" });
    await Promise.resolve();

    send(frame("session-1", "run-1", "run.started", 1));
    for (let sequence = 2; sequence < 12; sequence += 1) {
      send(frame("session-1", "run-1", "message.delta", sequence));
    }
    expect(settled).toHaveLength(0);

    send(frame("session-1", "run-1", "run.completed", 12));
    // A replayed terminal frame for the same run must not announce it again.
    send(frame("session-1", "run-1", "run.completed", 13));

    expect(settled).toEqual([
      {
        sessionId: "session-1",
        title: "Plan the launch",
        summary: expect.any(String),
        activeCount: 0,
      },
    ]);
  });

  it("waits for the automatic follow-up run before announcing the session is ready", async () => {
    const { send, settled } = startRelay({ "session-1": "Plan the launch" });
    await Promise.resolve();

    // The user queued a follow-up the run never consumed: the workspace
    // submits it as the next run, so the session is not idle yet.
    queueFollowUp("session-1");
    send(frame("session-1", "run-1", "run.started", 1));
    send(frame("session-1", "run-1", "run.completed", 2));
    expect(settled).toHaveLength(0);

    saveQueuedAgentFollowUps({});
    send(frame("session-1", "run-2", "run.started", 1));
    send(frame("session-1", "run-2", "run.completed", 2));

    expect(settled).toHaveLength(1);
    expect(settled[0]).toMatchObject({ sessionId: "session-1", title: "Plan the launch" });
  });

  it("announces when the queued follow-up was consumed as live steering", async () => {
    const { send, settled } = startRelay();
    await Promise.resolve();

    queueFollowUp("session-1");
    send(frame("session-1", "run-1", "run.started", 1));
    send(frame("session-1", "run-1", "steering.consumed", 2));
    send(frame("session-1", "run-1", "run.completed", 3));

    expect(settled).toHaveLength(1);
  });

  it("counts runs that are still active when another one completes", async () => {
    const { send, settled } = startRelay();
    await Promise.resolve();

    send(frame("session-1", "run-1", "run.started", 1));
    send(frame("session-2", "run-2", "run.started", 1));
    send(frame("session-1", "run-1", "run.completed", 2));

    expect(settled).toHaveLength(1);
    expect(settled[0]).toMatchObject({ sessionId: "session-1", title: "", activeCount: 1 });
  });

  it("stays silent for failed and cancelled runs", async () => {
    const { send, settled } = startRelay();
    await Promise.resolve();

    send(frame("session-1", "run-1", "run.started", 1));
    send(frame("session-1", "run-1", "run.failed", 2));
    send(frame("session-2", "run-2", "run.started", 1));
    send(frame("session-2", "run-2", "run.cancelled", 2));

    expect(settled).toHaveLength(0);
  });
});
