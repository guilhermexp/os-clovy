import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AppSettings } from "../components/settings/AppSettings";

const mocks = vi.hoisted(() => ({
  dictationSettings: vi.fn(),
  setDictationStyle: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", async (original) => ({
  ...(await original<typeof import("@tauri-apps/api/core")>()),
  invoke: vi.fn().mockRejectedValue(new Error("No desktop in tests")),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => () => {}),
  emit: vi.fn(),
}));

vi.mock("../lib/tauri", async (original) => ({
  ...(await original<typeof import("../lib/tauri")>()),
  dictationCapabilities: vi.fn().mockResolvedValue({
    capabilities: {
      available: true,
      platform: "macos",
      shortcuts: true,
      paste: true,
      microphoneSelection: true,
      accessibilityPermission: true,
      systemAudio: true,
    },
  }),
  dictationSettings: mocks.dictationSettings,
  setDictationStyle: mocks.setDictationStyle,
  setDictationCleanup: vi.fn(),
  setDictationHudPlacement: vi.fn(),
  listProviderModels: vi.fn().mockResolvedValue({ models: [] }),
}));

describe("StyleSettingsSection", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.dictationSettings.mockResolvedValue({
      settings: {
        pushToTalkShortcut: {
          code: "KeyD",
          label: "Ctrl+Opt+D",
          pressCount: 1,
          modifiers: {
            command: false,
            control: true,
            option: true,
            shift: false,
            function: false,
          },
        },
        toggleShortcut: {
          code: "KeyT",
          label: "Ctrl+Opt+T",
          pressCount: 1,
          modifiers: {
            command: false,
            control: true,
            option: true,
            shift: false,
            function: false,
          },
        },
        microphone: {},
        style: "casualLowercase",
        // The style applies only while cleanup is on.
        cleanupEnabled: true,
        cleanupModel: "nvidia/nemotron-3-nano-30b-a3b",
        hudPlacement: "bottom",
      },
    });
    mocks.setDictationStyle.mockImplementation(async (style) => ({
      style,
    }));
  });

  it("loads the active style and switches via the segmented control", async () => {
    const user = userEvent.setup();
    render(
      <AppSettings
        sourceMode="microphoneOnly"
        checkingSourceReadiness={false}
        onSourceModeChange={() => {}}
        onEnableSystemAudio={() => {}}
        activeTab="dictation"
        onTabChange={() => {}}
      />,
    );

    const casual = await screen.findByRole("button", { name: "Casual" });
    await waitFor(() => expect(casual).toHaveAttribute("aria-pressed", "true"));

    await user.click(screen.getByRole("button", { name: "Formal" }));

    await waitFor(() => expect(mocks.setDictationStyle).toHaveBeenCalledWith("formal"));
    expect(screen.getByRole("button", { name: "Formal" })).toHaveAttribute("aria-pressed", "true");
  });
});
