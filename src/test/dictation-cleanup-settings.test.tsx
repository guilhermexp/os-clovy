import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { StyleSettingsSection } from "../components/settings/StyleSettingsSection";

const mocks = vi.hoisted(() => ({
  dictationSettings: vi.fn(),
  setDictationStyle: vi.fn(),
  setDictationCleanup: vi.fn(),
  listProviderModels: vi.fn(),
}));

vi.mock("../lib/tauri", () => ({
  dictationSettings: mocks.dictationSettings,
  setDictationStyle: mocks.setDictationStyle,
  setDictationCleanup: mocks.setDictationCleanup,
  listProviderModels: mocks.listProviderModels,
}));

function model(id: string, name: string, provider = "openrouter") {
  return { id, name, provider, modelType: "text", traits: [], capabilities: [] };
}

function settings(cleanupEnabled: boolean) {
  return {
    pushToTalkShortcut: {
      code: "Fn",
      label: "Fn",
      pressCount: 1,
      modifiers: { command: false, control: false, option: false, shift: false, function: true },
    },
    toggleShortcut: {
      code: "KeyT",
      label: "Ctrl+Opt+T",
      pressCount: 1,
      modifiers: { command: false, control: true, option: true, shift: false, function: false },
    },
    microphone: {},
    style: "standard",
    cleanupEnabled,
    cleanupModel: "nvidia/nemotron-3-nano-30b-a3b",
  };
}

describe("dictation cleanup settings", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.dictationSettings.mockResolvedValue({ settings: settings(false) });
    mocks.setDictationCleanup.mockImplementation(
      async (enabled: boolean, cleanupModel?: string) => ({
        ...settings(enabled),
        cleanupModel: cleanupModel ?? "nvidia/nemotron-3-nano-30b-a3b",
      }),
    );
    mocks.listProviderModels.mockResolvedValue({
      mode: "generation",
      modelType: "text",
      selectedModel: "open-software/auto",
      models: [
        model("open-software/auto", "Auto", "open-software"),
        model("nvidia/nemotron-3-nano-30b-a3b", "Nemotron 3 Nano"),
        model("openai/gpt-5-mini", "GPT-5 mini"),
        model("cli:claude", "Claude Code", "cli"),
      ],
    });
  });

  it("starts off and hides the model and style until cleanup is turned on", async () => {
    render(<StyleSettingsSection />);

    const toggle = await screen.findByRole("switch", { name: "Clean up dictation text" });
    await waitFor(() => expect(toggle).toHaveAttribute("aria-checked", "false"));
    expect(screen.queryByRole("combobox", { name: "Cleanup model" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Formal" })).not.toBeInTheDocument();
  });

  it("turns cleanup on and offers only OpenRouter models", async () => {
    const user = userEvent.setup();
    render(<StyleSettingsSection />);

    const toggle = await screen.findByRole("switch", { name: "Clean up dictation text" });
    await user.click(toggle);

    await waitFor(() => expect(mocks.setDictationCleanup).toHaveBeenCalledWith(true));
    await waitFor(() => expect(toggle).toHaveAttribute("aria-checked", "true"));
    const picker = await screen.findByRole("combobox", { name: "Cleanup model" });
    const labels = within(picker)
      .getAllByRole("option")
      .map((option) => option.textContent);
    expect(labels).toEqual(["Nemotron 3 Nano", "GPT-5 mini"]);
    expect(screen.getByRole("button", { name: "Formal" })).toBeInTheDocument();

    await user.selectOptions(picker, "openai/gpt-5-mini");
    await waitFor(() =>
      expect(mocks.setDictationCleanup).toHaveBeenLastCalledWith(true, "openai/gpt-5-mini"),
    );
  });
});
