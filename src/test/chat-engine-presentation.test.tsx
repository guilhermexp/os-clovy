import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AgentSessionDto } from "../lib/agent-runtime-contract";
import type { ChatEngineCatalog } from "../lib/chat-engine";

const mocks = vi.hoisted(() => ({ invoke: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => ({
  Channel: class {
    onmessage?: (event: unknown) => void;
  },
  convertFileSrc: vi.fn((path: string) => path),
  invoke: mocks.invoke,
}));

vi.mock("@tauri-apps/api/event", () => ({
  emit: vi.fn(),
  listen: vi.fn(async () => vi.fn()),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

import { AgentWorkspace } from "../components/agent/AgentWorkspace";
import { ChatEnginePicker } from "../components/agent/composer/ChatEnginePicker";
import { resetAgentSessionDraftsForTests } from "../lib/agent-session-drafts";
import { resetCurrentDataPartitionForTests } from "../lib/data-partition";

// The three tool states a CLI used to be announced with.
const catalog = {
  clis: [
    {
      id: "claude",
      name: "Claude Code",
      installed: true,
      reason: null,
      modelId: "__clovy_cli_engine__:claude",
    },
    {
      id: "codex",
      name: "Codex",
      installed: true,
      reason: null,
      modelId: "__clovy_cli_engine__:codex",
    },
    { id: "pi", name: "Pi", installed: true, reason: null, modelId: "__clovy_cli_engine__:pi" },
  ],
  endpoints: [],
} as ChatEngineCatalog;

const session: AgentSessionDto = {
  id: "session-cli-1",
  title: "Claude Code session",
  status: "idle",
  model: "__clovy_cli_engine__:claude",
  safetyMode: "sandboxed",
  workspacePath: "/tmp/session-cli-1",
  source: "user",
  createdAt: "2026-07-22T12:00:00Z",
  updatedAt: "2026-07-22T12:00:00Z",
};

function answer(command: string, sessions: AgentSessionDto[]) {
  if (command === "list_agent_sessions") return Promise.resolve(sessions);
  if (command === "get_agent_session") return Promise.resolve(sessions[0]);
  if (command === "list_agent_items") return Promise.resolve([]);
  if (command === "list_agent_artifacts") return Promise.resolve([]);
  if (command === "list_agent_skills") return Promise.resolve([]);
  if (command === "chat_engine_catalog") return Promise.resolve(catalog);
  if (command === "list_provider_models") {
    return Promise.resolve({
      mode: "generation",
      selectedModel: "open-software/auto",
      modelType: "text",
      models: [
        {
          provider: "june",
          id: "open-software/auto",
          name: "Auto",
          modelType: "text",
          traits: [],
          capabilities: [],
        },
      ],
    });
  }
  return Promise.resolve(undefined);
}

describe("chat engine presentation", () => {
  beforeEach(() => {
    resetAgentSessionDraftsForTests();
    resetCurrentDataPartitionForTests();
    window.localStorage.clear();
    mocks.invoke.mockReset();
  });

  it("shows the hero suggestions without a privacy footnote", async () => {
    mocks.invoke.mockImplementation((command: string) => answer(command, []));
    const { container } = render(<AgentWorkspace />);

    await waitFor(() => expect(container.querySelector(".agent-hero-suggestions")).not.toBeNull());
    expect(container.querySelector(".agent-hero-footnote")).toBeNull();
    expect(screen.queryByText(/Clovy runs locally/i)).not.toBeInTheDocument();
  });

  it.each([
    "claude",
    "codex",
    "pi",
  ])("shows no engine notice in the composer for %s", async (cli) => {
    const cliSession = { ...session, model: `__clovy_cli_engine__:${cli}` };
    mocks.invoke.mockImplementation((command: string) => answer(command, [cliSession]));
    const { container } = render(<AgentWorkspace initialSession={cliSession} />);

    await waitFor(() =>
      expect(screen.getByRole("button", { name: /Chat engine/i })).toBeInTheDocument(),
    );
    expect(container.querySelector(".agent-composer-notice")).toBeNull();
    expect(screen.queryByText(/Clovy tools/i)).not.toBeInTheDocument();
  });

  it.each([
    "claude",
    "codex",
    "pi",
  ])("shows no engine notice in the picker menu for %s", async (cli) => {
    const user = userEvent.setup();
    const { container } = render(
      <ChatEnginePicker
        model={`__clovy_cli_engine__:${cli}`}
        setModel={vi.fn()}
        catalog={catalog}
      />,
    );

    expect(container.querySelector(".agent-composer-notice")).toBeNull();
    await user.click(screen.getByRole("button", { name: /Chat engine/i }));
    expect(document.querySelector(".agent-composer-notice")).toBeNull();
    expect(screen.queryByText(/Clovy tools|Clovy's tools/i)).not.toBeInTheDocument();
  });
});
