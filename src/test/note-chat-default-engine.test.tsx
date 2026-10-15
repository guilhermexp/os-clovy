import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  createSession: vi.fn(),
  startRun: vi.fn(),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => vi.fn()),
}));

vi.mock("../lib/tauri", () => ({
  invoke: mocks.invoke,
  agentRuntimeBindings: {
    createSession: mocks.createSession,
    startRun: mocks.startRun,
    getSession: vi.fn(),
    listItems: vi.fn(async () => []),
    cancelRun: vi.fn(),
    retryRun: vi.fn(),
  },
}));

import { useNoteChat } from "../components/note-chat/useNoteChat";

const note = { id: "note-7", title: "Kickoff" };

describe("note chat engine", () => {
  beforeEach(() => {
    window.localStorage.clear();
    mocks.invoke.mockReset();
    mocks.createSession.mockReset();
    mocks.startRun.mockReset();
    mocks.createSession.mockImplementation(async (request: { model: string }) => ({
      id: "note-session",
      title: "Kickoff",
      status: "idle",
      model: request.model,
      safetyMode: "sandboxed",
      workspacePath: "/tmp/note-session",
      source: "user",
      createdAt: "2026-10-15T12:00:00Z",
      updatedAt: "2026-10-15T12:00:00Z",
    }));
    mocks.startRun.mockResolvedValue({
      id: "run-1",
      sessionId: "note-session",
      status: "running",
      model: "__clovy_cli_engine__:claude",
    });
  });

  it("starts a new note chat on the default CLI with the note referenced", async () => {
    mocks.invoke.mockImplementation(async (command: string) =>
      command === "chat_default_engine"
        ? { modelId: "__clovy_cli_engine__:claude", cli: "claude", legacyFallback: false }
        : undefined,
    );
    const { result } = renderHook(() => useNoteChat(note));
    await waitFor(() => expect(result.current.model).toBe("__clovy_cli_engine__:claude"));

    await act(async () => {
      await result.current.submit("What did we decide?");
    });

    expect(mocks.createSession).toHaveBeenCalledWith(
      expect.objectContaining({ model: "__clovy_cli_engine__:claude" }),
    );
    expect(mocks.startRun).toHaveBeenCalledWith(
      expect.objectContaining({
        model: "__clovy_cli_engine__:claude",
        prompt: '@note:note-7 ("Kickoff") What did we decide?',
      }),
    );
  });

  it("uses the default CLI even when the message is sent before it was read", async () => {
    let resolveEngine: (value: unknown) => void = () => undefined;
    let calls = 0;
    mocks.invoke.mockImplementation((command: string) => {
      if (command !== "chat_default_engine") return Promise.resolve(undefined);
      calls += 1;
      return calls === 1
        ? new Promise((resolve) => {
            resolveEngine = resolve;
          })
        : Promise.resolve({ modelId: "__clovy_cli_engine__:codex" });
    });
    const { result } = renderHook(() => useNoteChat(note));

    await act(async () => {
      await result.current.submit("Summarize it");
    });
    resolveEngine({ modelId: "__clovy_cli_engine__:codex" });

    expect(mocks.startRun).toHaveBeenCalledWith(
      expect.objectContaining({ model: "__clovy_cli_engine__:codex" }),
    );
  });

  it("keeps the legacy Clovy agent when chat is not on a CLI", async () => {
    mocks.invoke.mockImplementation(async (command: string) =>
      command === "chat_default_engine"
        ? { modelId: null, cli: null, legacyFallback: true }
        : undefined,
    );
    const { result } = renderHook(() => useNoteChat(note));
    await act(async () => {
      await result.current.submit("Hello");
    });
    expect(mocks.startRun).toHaveBeenCalledWith(expect.objectContaining({ model: "auto" }));
  });

  it("keeps an engine picked in the panel", async () => {
    mocks.invoke.mockImplementation(async (command: string) =>
      command === "chat_default_engine" ? { modelId: "__clovy_cli_engine__:claude" } : undefined,
    );
    const { result } = renderHook(() => useNoteChat(note));
    act(() => result.current.setModel("__clovy_cli_engine__:codex"));
    await act(async () => {
      await result.current.submit("Hello");
    });
    expect(mocks.startRun).toHaveBeenCalledWith(
      expect.objectContaining({ model: "__clovy_cli_engine__:codex" }),
    );
  });
});
