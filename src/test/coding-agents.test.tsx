import { cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type * as TauriCore from "@tauri-apps/api/core";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const invokeMock = vi.fn();

vi.mock("@tauri-apps/api/core", async (importOriginal) => ({
  ...(await importOriginal<typeof TauriCore>()),
  invoke: (command: string, args?: unknown) =>
    args !== undefined ? invokeMock(command, args) : invokeMock(command),
}));

const listeners = new Map<string, () => void>();
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (event: string, handler: () => void) => {
    listeners.set(event, handler);
    return () => listeners.delete(event);
  }),
  emit: vi.fn(async () => undefined),
}));

import { CodingAgentSessionsStrip } from "../components/coding-agents/CodingAgentSessionsStrip";
import { ActivitySettingsSection } from "../components/settings/ActivitySettingsSection";
import { applyInterfaceLocale } from "../i18n/locale";
import { type ActivityStatusDto, DEFAULT_ACTIVITY_SETTINGS } from "../lib/activity-capture";
import {
  CODING_AGENTS_UPDATED_EVENT,
  type CodingAgentBlockDto,
  type CodingAgentsStatusDto,
} from "../lib/coding-agents";

const activityStatus: ActivityStatusDto = {
  supported: true,
  settings: { ...DEFAULT_ACTIVITY_SETTINGS, ignoredApps: ["Terminal"] },
  state: { kind: "off" },
  permissions: {
    accessibility: "granted",
    screenRecording: "granted",
    inputMonitoring: "granted",
  },
  manualPause: false,
  lastFrameAt: null,
  debugExportAvailable: false,
};

const agentsStatus: CodingAgentsStatusDto = {
  supported: true,
  sources: [
    { id: "claude_code", name: "Claude Code", enabled: false, present: true },
    { id: "codex", name: "Codex", enabled: false, present: true },
    { id: "copilot_cli", name: "Copilot CLI", enabled: false, present: false },
    { id: "copilot_vscode", name: "Copilot in VS Code", enabled: false, present: false },
    { id: "cursor", name: "Cursor", enabled: false, present: false },
    { id: "cursor_cli", name: "Cursor CLI", enabled: false, present: false },
    { id: "antigravity", name: "Antigravity", enabled: false, present: false },
  ],
  databaseReady: false,
  lastScanAt: null,
  lastError: null,
};

function block(overrides: Partial<CodingAgentBlockDto>): CodingAgentBlockDto {
  return {
    id: 1,
    source: "codex",
    sessionId: "s1",
    startedAt: new Date(2026, 9, 4, 10, 0).toISOString(),
    endedAt: new Date(2026, 9, 4, 10, 40).toISOString(),
    cwd: "/Users/me/api",
    project: "api",
    title: null,
    firstPrompt: "add a health check",
    promptCount: 3,
    replyCount: 3,
    activeSeconds: 1200,
    state: "summarized",
    sealedAt: null,
    summary: "Added a /health endpoint with a database ping and its test.",
    summarySource: "cli:codex",
    summaryAttempts: 0,
    summaryError: null,
    ...overrides,
  };
}

afterEach(() => {
  cleanup();
  applyInterfaceLocale("en");
  listeners.clear();
  vi.clearAllMocks();
});

describe("CodingAgentSourcesSection in Settings, Activity", () => {
  let current: ActivityStatusDto;

  beforeEach(() => {
    current = structuredClone(activityStatus);
    invokeMock.mockImplementation(async (command: string, args?: { request?: unknown }) => {
      if (command === "activity_status") return current;
      if (command === "coding_agents_status") return agentsStatus;
      if (command === "activity_save_settings") {
        const { settings } = args?.request as { settings: ActivityStatusDto["settings"] };
        current = { ...current, settings };
        return current;
      }
      throw new Error(`Unhandled command: ${command}`);
    });
  });

  it("lists the seven sources, all off, with whether each was found", async () => {
    render(<ActivitySettingsSection />);

    const section = await screen.findByRole("region", { name: "Coding agents" });
    const switches = within(section).getAllByRole("switch");
    expect(switches).toHaveLength(7);
    for (const toggle of switches) expect(toggle).not.toBeChecked();
    await waitFor(() => {
      expect(within(section).getAllByText("Found on this Mac")).toHaveLength(2);
    });
    expect(within(section).getAllByText("Not found on this Mac")).toHaveLength(5);
  });

  it("turning a source on saves it with the rest of the activity settings", async () => {
    const user = userEvent.setup();
    render(<ActivitySettingsSection />);

    await user.click(await screen.findByRole("switch", { name: "Read Claude Code sessions" }));
    await user.click(screen.getByRole("switch", { name: "Read Codex sessions" }));

    const saves = invokeMock.mock.calls.filter(([command]) => command === "activity_save_settings");
    expect(saves).toHaveLength(2);
    expect(saves[1][1]).toEqual({
      request: {
        settings: {
          ...activityStatus.settings,
          codingAgents: { ...activityStatus.settings.codingAgents, claudeCode: true, codex: true },
        },
      },
    });
    expect(screen.getByRole("switch", { name: "Read Codex sessions" })).toBeChecked();
    expect(await screen.findByText(/The activity database is not available/)).toBeInTheDocument();
  });

  it("is in Portuguese when the interface is", async () => {
    applyInterfaceLocale("pt-BR");
    render(<ActivitySettingsSection />);
    expect(await screen.findByRole("region", { name: "Agentes de código" })).toBeInTheDocument();
    expect(screen.getByRole("switch", { name: "Ler sessões do Cursor" })).toBeInTheDocument();
  });
});

describe("CodingAgentSessionsStrip", () => {
  it("shows the day's blocks with agent, project, time range, and summary", async () => {
    const blocks = [
      block({}),
      block({
        id: 2,
        source: "claude_code",
        project: null,
        state: "sealed",
        summary: null,
        firstPrompt: "fix the login bug",
        promptCount: 1,
        startedAt: new Date(2026, 9, 4, 11, 5).toISOString(),
        endedAt: new Date(2026, 9, 4, 11, 30).toISOString(),
      }),
      block({ id: 3, source: "cursor_cli", state: "live", summary: null, firstPrompt: null }),
    ];
    invokeMock.mockResolvedValue(blocks);

    render(<CodingAgentSessionsStrip day={new Date(2026, 9, 4, 15, 0)} />);

    const list = await screen.findByRole("list", { name: "Coding agent sessions" });
    const items = within(list).getAllByRole("listitem");
    expect(items).toHaveLength(3);

    const format: Intl.DateTimeFormatOptions = { hour: "2-digit", minute: "2-digit" };
    const range = `${new Date(2026, 9, 4, 10, 0).toLocaleTimeString([], format)} to ${new Date(2026, 9, 4, 10, 40).toLocaleTimeString([], format)}`;
    const first = within(items[0]);
    expect(first.getByText("Codex")).toBeInTheDocument();
    expect(first.getByText("api")).toBeInTheDocument();
    expect(first.getByText(range)).toBeInTheDocument();
    expect(first.getByText("3 prompts")).toBeInTheDocument();
    expect(
      first.getByText("Added a /health endpoint with a database ping and its test."),
    ).toBeInTheDocument();

    const second = within(items[1]);
    expect(second.getByText("Claude Code")).toBeInTheDocument();
    expect(second.getByText("No project")).toBeInTheDocument();
    expect(second.getByText("1 prompt")).toBeInTheDocument();
    expect(second.getByText("Waiting for a summary")).toBeInTheDocument();
    expect(second.getByText("fix the login bug")).toBeInTheDocument();

    expect(within(items[2]).getByText("In progress")).toBeInTheDocument();

    expect(invokeMock).toHaveBeenCalledWith("coding_agents_blocks", {
      request: {
        from: new Date(2026, 9, 4).toISOString(),
        to: new Date(2026, 9, 5).toISOString(),
      },
    });
  });

  it("reloads when the scanner reports new blocks", async () => {
    invokeMock.mockResolvedValueOnce([]);
    render(<CodingAgentSessionsStrip day={new Date(2026, 9, 4)} />);
    expect(await screen.findByText("No coding agent sessions on this day.")).toBeInTheDocument();

    invokeMock.mockResolvedValueOnce([block({})]);
    await waitFor(() => expect(listeners.has(CODING_AGENTS_UPDATED_EVENT)).toBe(true));
    listeners.get(CODING_AGENTS_UPDATED_EVENT)?.();

    expect(await screen.findByText("Codex")).toBeInTheDocument();
  });
});
