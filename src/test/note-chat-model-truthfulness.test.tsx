import { render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { NoteChatPanel } from "../components/note-chat/NoteChatPanel";
import type { NoteChat } from "../components/note-chat/useNoteChat";
import { AGENT_RUN_AUTO_MODEL_PREFIX } from "../lib/agent-model-selection";
import { loadSessionModels, rememberSessionModel } from "../lib/agent-session-models";

const mocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  listProviderModels: vi.fn(),
  providerModelSettings: vi.fn(),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(),
}));

vi.mock("../lib/tauri", () => ({
  dictationHelperCommand: vi.fn(),
  invoke: mocks.invoke,
  listProviderModels: mocks.listProviderModels,
  providerModelSettings: mocks.providerModelSettings,
}));

describe("note chat model truthfulness", () => {
  beforeEach(() => {
    window.localStorage.clear();
    mocks.listProviderModels.mockReset();
    mocks.listProviderModels.mockResolvedValue({
      mode: "generation",
      modelType: "text",
      selectedModel: "concrete-default",
      models: [
        {
          provider: "openrouter",
          id: "concrete-default",
          name: "Concrete default",
          modelType: "text",
          traits: ["private"],
          capabilities: ["supportsFunctionCalling"],
        },
      ],
    });
    mocks.providerModelSettings.mockReset();
    mocks.providerModelSettings.mockResolvedValue({
      settings: { generationModel: "concrete-default", costQuality: 100 },
      effectiveSettings: {
        generationModel: "concrete-default",
        costQuality: 100,
        providerApiKeysConfigured: { groq: false, openrouter: false, fal: false, tavily: false },
      },
    });
  });

  it("never lets catalog hydration overwrite an acknowledged session choice", async () => {
    const stagedAuto = `${AGENT_RUN_AUTO_MODEL_PREFIX}20`;
    rememberSessionModel("note-session", stagedAuto);
    const setModel = vi.fn((model: string) => rememberSessionModel("note-session", model));
    const chat: NoteChat = {
      turns: [],
      working: false,
      submissionPending: false,
      loading: false,
      error: null,
      storedSessionId: "note-session",
      model: "auto",
      setModel,
      submit: vi.fn(async () => ({ accepted: true, current: true })),
      retry: vi.fn(async () => true),
      stop: vi.fn(),
    };

    render(
      <NoteChatPanel
        note={{ id: "note-1", title: "Planning" }}
        chat={chat}
        onClose={vi.fn()}
        onOpenInAgent={vi.fn()}
      />,
    );

    await waitFor(() => expect(mocks.listProviderModels).toHaveBeenCalledOnce());
    expect(setModel).not.toHaveBeenCalled();
    expect(loadSessionModels()["note-session"]).toBe(stagedAuto);
  });

  it("picks the chat engine for a CLI chat", async () => {
    mocks.invoke.mockImplementation(async (command: string) =>
      command === "chat_engine_catalog"
        ? {
            clis: [
              {
                id: "claude",
                name: "Claude Code",
                installed: true,
                reason: null,
                modelId: "__clovy_cli_engine__:claude",
                clovyTools: "available",
              },
            ],
            endpoints: [],
          }
        : undefined,
    );
    const chat: NoteChat = {
      turns: [],
      working: false,
      submissionPending: false,
      loading: false,
      error: null,
      storedSessionId: undefined,
      model: "__clovy_cli_engine__:claude",
      setModel: vi.fn(),
      submit: vi.fn(async () => ({ accepted: true, current: true })),
      retry: vi.fn(async () => true),
      stop: vi.fn(),
    };

    render(
      <NoteChatPanel
        note={{ id: "note-1", title: "Planning" }}
        chat={chat}
        onClose={vi.fn()}
        onOpenInAgent={vi.fn()}
      />,
    );

    expect(
      await screen.findByRole("button", { name: "Chat engine: Claude Code" }),
    ).toBeInTheDocument();
    // The Clovy model picker belongs to the legacy engine only.
    expect(screen.queryByRole("button", { name: /^Model/ })).not.toBeInTheDocument();
  });
});
