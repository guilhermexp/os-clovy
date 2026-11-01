import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AgentApprovalMode, AgentSessionDto } from "../lib/agent-runtime-contract";

const mocks = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({
  Channel: class {
    onmessage?: (event: unknown) => void;
  },
  convertFileSrc: (path: string) => path,
  invoke: mocks.invoke,
}));
vi.mock("@tauri-apps/api/event", () => ({ emit: vi.fn(), listen: vi.fn(async () => vi.fn()) }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

import { AgentWorkspace } from "../components/agent/AgentWorkspace";
import { AGENT_NEW_SESSION_EVENT } from "../lib/agent-events";
import { resetAgentSessionDraftsForTests } from "../lib/agent-session-drafts";
import { resetCurrentDataPartitionForTests } from "../lib/data-partition";

const model = "__clovy_cli_engine__:claude";
function fixture(failFirstSave = false, saveGate?: Promise<void>) {
  let session: AgentSessionDto | undefined;
  let run:
    | {
        id: string;
        sessionId: string;
        status: "running";
        model: string;
        approvalMode: AgentApprovalMode;
      }
    | undefined;
  const order: string[] = [];
  let rejectSave = failFirstSave;
  mocks.invoke.mockImplementation(
    async (command: string, args?: { sessionId: string; approvalMode: AgentApprovalMode }) => {
      switch (command) {
        case "chat_default_engine":
          return { modelId: model, cli: "claude" };
        case "chat_engine_catalog":
          return { clis: [{ id: "claude", name: "Claude Code", installed: true, modelId: model }] };
        case "list_agent_sessions":
          return session ? [session] : [];
        case "get_agent_session":
          return session;
        case "get_latest_agent_run":
          return run ?? null;
        case "list_agent_items":
        case "list_agent_artifacts":
        case "list_agent_skills":
          return [];
        case "list_provider_models":
          return { models: [] };
        case "create_agent_session":
          order.push("create");
          session = {
            id: "new-session",
            title: "Fixture",
            status: "idle",
            model,
            safetyMode: "sandboxed",
            approvalMode: "approvals",
            workspacePath: "/tmp/fixture",
            source: "user",
            createdAt: "2026-01-01",
            updatedAt: "2026-01-01",
          };
          return { ...session };
        case "set_agent_session_approval_mode":
          order.push("save");
          if (rejectSave) {
            rejectSave = false;
            throw new Error("Save failed");
          }
          if (saveGate) await saveGate;
          if (!session || !args || args.sessionId !== session.id)
            throw new Error("Unknown session");
          session = { ...session, approvalMode: args.approvalMode };
          return { ...session };
        case "start_agent_run":
          order.push("start");
          if (!session) throw new Error("Missing session");
          run = {
            id: "run-1",
            sessionId: session.id,
            status: "running",
            model,
            approvalMode: session.approvalMode ?? "approvals",
          };
          return { ...run };
        default:
          return undefined;
      }
    },
  );
  return { order, session: () => session };
}

describe("approval mode before the first run", () => {
  beforeEach(() => {
    mocks.invoke.mockReset();
    resetAgentSessionDraftsForTests();
    resetCurrentDataPartitionForTests();
    window.localStorage.clear();
  });

  it.each([
    false,
    true,
  ])("persists the new %s Home draft before starting and reopens it", async (homeMode) => {
    const state = fixture();
    const user = userEvent.setup();
    const view = render(<AgentWorkspace homeMode={homeMode} />);
    await user.selectOptions(
      await screen.findByRole("combobox", { name: "Tool approval" }),
      "bypass",
    );
    await user.type(screen.getByRole("textbox", { name: "Message Clovy" }), "First message");
    await user.click(screen.getByRole("button", { name: "Send message" }));
    await waitFor(() => expect(state.order).toEqual(["create", "save", "start"]));
    const startCall = mocks.invoke.mock.calls.find(([command]) => command === "start_agent_run");
    expect(startCall?.[1].request).not.toHaveProperty("approvalMode");
    expect(await screen.findByText(/Current run: Bypass/)).toBeVisible();
    const saved = state.session();
    expect(saved?.approvalMode).toBe("bypass");
    view.unmount();
    const reopened = render(<AgentWorkspace homeMode={homeMode} initialSession={saved} />);
    await waitFor(() =>
      expect(screen.getByRole("combobox", { name: "Tool approval" })).toHaveValue("bypass"),
    );
    if (homeMode) {
      await waitFor(() =>
        expect(screen.getByRole("textbox", { name: "Message Clovy" }).textContent?.trim()).toBe(""),
      );
    }
    if (!homeMode) {
      act(() => window.dispatchEvent(new CustomEvent(AGENT_NEW_SESSION_EVENT)));
      expect(screen.getByRole("combobox", { name: "Tool approval" })).toHaveValue("approvals");
    } else {
      reopened.unmount();
      render(<AgentWorkspace homeMode />);
      expect(await screen.findByRole("combobox", { name: "Tool approval" })).toHaveValue(
        "approvals",
      );
    }
  });

  it.each([
    false,
    true,
  ])("preserves the %s Home draft and refuses to start after a save failure", async (homeMode) => {
    const state = fixture(true);
    const user = userEvent.setup();
    render(<AgentWorkspace homeMode={homeMode} />);
    await user.selectOptions(
      await screen.findByRole("combobox", { name: "Tool approval" }),
      "bypass",
    );
    await user.type(screen.getByRole("textbox", { name: "Message Clovy" }), "Keep this prompt");
    await user.click(screen.getByRole("button", { name: "Send message" }));
    expect(await screen.findByText(/Save failed/)).toBeVisible();
    expect(state.order).toEqual(["create", "save"]);
    expect(screen.getByText(/Not saved/)).toBeVisible();
    expect(screen.getByRole("textbox", { name: "Message Clovy" })).toHaveTextContent(
      "Keep this prompt",
    );
    await user.click(screen.getByRole("button", { name: "Send message" }));
    await waitFor(() => expect(state.order).toEqual(["create", "save", "save", "start"]));
    expect(await screen.findByText(/Current run: Bypass/)).toBeVisible();
  });

  it("holds the first run while the preference is being saved", async () => {
    let release = () => {};
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    const state = fixture(false, gate);
    const user = userEvent.setup();
    render(<AgentWorkspace />);
    await user.selectOptions(
      await screen.findByRole("combobox", { name: "Tool approval" }),
      "bypass",
    );
    await user.type(screen.getByRole("textbox", { name: "Message Clovy" }), "One message");
    await user.click(screen.getByRole("button", { name: "Send message" }));
    await waitFor(() => expect(state.order).toEqual(["create", "save"]));
    await waitFor(() =>
      expect(screen.getByRole("combobox", { name: "Tool approval" })).toBeDisabled(),
    );
    expect(screen.getByRole("button", { name: "Send message" })).toBeDisabled();
    await act(async () => release());
    await waitFor(() => expect(state.order).toEqual(["create", "save", "start"]));
  });

  it("persists an explicit return to approvals before the first run", async () => {
    const state = fixture();
    const user = userEvent.setup();
    render(<AgentWorkspace />);
    const control = await screen.findByRole("combobox", { name: "Tool approval" });
    await user.selectOptions(control, "bypass");
    await user.selectOptions(control, "approvals");
    await user.type(screen.getByRole("textbox", { name: "Message Clovy" }), "First message");
    await user.click(screen.getByRole("button", { name: "Send message" }));
    await waitFor(() => expect(state.order).toEqual(["create", "save", "start"]));
    expect(await screen.findByText(/Current run: Approvals/)).toBeVisible();
  });
});
