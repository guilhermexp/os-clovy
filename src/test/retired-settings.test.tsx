import { cleanup, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { AppSettings } from "../components/settings/AppSettings";
import type { SettingsTab } from "../components/settings/settings-config";
import { applyInterfaceLocale } from "../i18n/locale";
import { AGENT_SOUNDS_ENABLED_KEY } from "../lib/agent-sound-settings";
import { representativeConnectorPolicy } from "./fixtures/connector-policy";

// An older profile/backend can still contain media settings. Opening Models
// must neither expose those choices nor use them to request media catalogs.
const legacySettings = {
  transcriptionProvider: "groq",
  transcriptionModel: "whisper-large-v3",
  generationModel: "z-ai/glm-5.2",
  imageModel: "fal-ai/flux/dev",
  videoModel: "fal-ai/wan/v2.2-a14b/text-to-video",
  providerApiKeysConfigured: { groq: true, openrouter: false, fal: true, tavily: false },
};

const invokeMock = vi.fn(async (command: string, args?: { request?: { mode?: string } }) => {
  switch (command) {
    case "dictation_settings":
      return { settings: { cleanupEnabled: false } };
    case "dictation_hotkey_status":
      return { type: "ready" };
    case "dictation_helper_command":
      return undefined;
    case "provider_model_settings":
      return { settings: legacySettings, effectiveSettings: legacySettings };
    case "list_provider_models":
      return { mode: args?.request?.mode, models: [] };
    case "llm_providers":
      return {
        usage: {
          chat: { kind: "cli", id: "claude" },
          notes: { kind: "openrouter" },
          activity: { kind: "none" },
        },
        endpoints: [],
        cliLevels: {},
      };
    case "connectors_list":
      return [
        {
          accountId: "notion-hosted-mcp",
          provider: "notion",
          email: "Notion",
          scopes: [],
          status: "connected",
          workspaceName: null,
          workspaceUrlKey: null,
          selectedTeams: [],
        },
      ];
    case "connectors_policy":
      return representativeConnectorPolicy();
    case "list_agent_mcp_servers":
      return [
        {
          id: "fixture-mcp",
          name: "Tasks",
          enabled: true,
          transport: "stdio",
          command: "node",
          args: [],
          toolVisibility: { include: [], exclude: [] },
          safety: { requiresApproval: true, allowSandboxed: true, approvalTools: [] },
          metadata: {},
        },
      ];
    case "llm_detect_clis":
    case "list_agent_skills":
      return [];
    default:
      throw new Error(`Unavailable test command: ${command}`);
  }
});

vi.mock("@tauri-apps/api/core", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@tauri-apps/api/core")>()),
  invoke: (command: string, args?: { request?: { mode?: string } }) => invokeMock(command, args),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => () => {}),
  emit: vi.fn(async () => undefined),
}));

function renderSettings(activeTab?: SettingsTab) {
  return render(
    <AppSettings
      sourceMode="microphoneOnly"
      checkingSourceReadiness={false}
      onSourceModeChange={() => {}}
      onEnableSystemAudio={() => {}}
      activeTab={activeTab}
      onTabChange={activeTab ? () => {} : undefined}
    />,
  );
}

describe("settings after the internal chat retirement", () => {
  beforeEach(() => {
    applyInterfaceLocale("en");
    invokeMock.mockClear();
  });

  afterEach(() => {
    cleanup();
    localStorage.removeItem(AGENT_SOUNDS_ENABLED_KEY);
    applyInterfaceLocale("en");
  });

  it("opens General for an old Agent destination", () => {
    renderSettings("agent" as SettingsTab);
    expect(screen.getByRole("heading", { name: "General" })).toBeInTheDocument();
    expect(screen.getByRole("tabpanel")).toHaveAttribute("id", "settings-panel-general");
    expect(screen.queryByRole("heading", { name: "Agent" })).not.toBeInTheDocument();
  });

  it("keeps connected Plugins and custom MCP controls reachable without Agent navigation", async () => {
    const user = userEvent.setup();
    renderSettings();
    expect(screen.queryByRole("tab", { name: "Agent" })).not.toBeInTheDocument();
    await user.click(screen.getByRole("tab", { name: "Plugins" }));
    expect(await screen.findByRole("button", { name: "Disconnect Notion" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Add server" })).toBeEnabled();
    expect(screen.getByRole("heading", { name: "Skills" })).toBeInTheDocument();
    await user.click(await screen.findByRole("button", { name: "Configure Tasks" }));
    const dialog = screen.getByRole("dialog", { name: "Configure Tasks" });
    expect(
      within(dialog).getByRole("switch", { name: "Require approval for every tool" }),
    ).toBeChecked();
    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(invokeMock.mock.calls.map(([command]) => command)).not.toContain(
      "update_agent_mcp_server",
    );
  });

  it("keeps the previous sound preference and persists a change in General", async () => {
    localStorage.setItem(AGENT_SOUNDS_ENABLED_KEY, "false");
    const user = userEvent.setup();
    const view = renderSettings("general");
    const sound = screen.getByRole("switch", { name: "Play routine sounds" });
    expect(sound).not.toBeChecked();
    await user.click(sound);
    expect(sound).toBeChecked();
    expect(localStorage.getItem(AGENT_SOUNDS_ENABLED_KEY)).toBe("true");
    view.unmount();
    renderSettings("general");
    expect(screen.getByRole("switch", { name: "Play routine sounds" })).toBeChecked();
  });

  it("does not offer media models or load their catalogs from older settings", async () => {
    const user = userEvent.setup();
    renderSettings("models");
    await user.click(screen.getByRole("button", { name: "More options for text" }));
    expect(await screen.findByLabelText(/groq api key/i)).toBeInTheDocument();
    expect(screen.getByLabelText(/openrouter api key/i)).toBeInTheDocument();
    expect(screen.queryByLabelText(/fal api key/i)).not.toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Image and video" })).not.toBeInTheDocument();
    const modes = invokeMock.mock.calls
      .filter(([command]) => command === "list_provider_models")
      .map(([, args]) => args?.request?.mode);
    expect(new Set(modes)).toEqual(new Set(["transcription", "generation"]));
  });

  it("offers Notes, Activity and dictation cleanup without a chat selector", async () => {
    renderSettings("models");
    expect(await screen.findByRole("combobox", { name: "Notes" })).toBeInTheDocument();
    expect(screen.getByRole("combobox", { name: "Activity" })).toBeInTheDocument();
    expect(screen.getByRole("switch", { name: "Clean up dictation text" })).not.toBeChecked();
    expect(screen.queryByRole("combobox", { name: "Default agent CLI" })).not.toBeInTheDocument();
  });

  it("recovers the retired destination and labels routine sounds in Portuguese", () => {
    applyInterfaceLocale("pt-BR");
    renderSettings("agent" as SettingsTab);
    expect(screen.getByRole("heading", { name: "Geral" })).toBeInTheDocument();
    expect(screen.getByRole("switch", { name: "Reproduzir sons das rotinas" })).toBeVisible();
  });
});
