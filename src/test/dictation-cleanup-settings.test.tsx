import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { dispatchProviderModelSettingsChanged } from "../lib/model-privacy";
import { AppSettings } from "../components/settings/AppSettings";
import type { SettingsTab } from "../components/settings/settings-config";
import type { DictationSettingsDto } from "../lib/tauri";

const mocks = vi.hoisted(() => ({
  dictationSettings: vi.fn(),
  setDictationStyle: vi.fn(),
  setDictationCleanup: vi.fn(),
  setDictationCleanupModel: vi.fn(),
  setDictationHudPlacement: vi.fn(),
  setDictationLanguage: vi.fn(),
  setDictationMicrophone: vi.fn(),
  setDictationShortcut: vi.fn(),
  listProviderModels: vi.fn(),
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
    capabilities: { platform: "macos", shortcuts: true, microphoneSelection: true },
  }),
  dictationHotkeyStatus: vi.fn().mockResolvedValue({ type: "hotkey_trigger_ready" }),
  providerModelSettings: vi.fn().mockResolvedValue({ settings: { transcriptionProvider: "groq" } }),
  dictationHelperCommand: vi.fn().mockResolvedValue(undefined),
  dictationSettings: mocks.dictationSettings,
  setDictationStyle: mocks.setDictationStyle,
  setDictationCleanup: mocks.setDictationCleanup,
  setDictationCleanupModel: mocks.setDictationCleanupModel,
  setDictationHudPlacement: mocks.setDictationHudPlacement,
  setDictationLanguage: mocks.setDictationLanguage,
  setDictationMicrophone: mocks.setDictationMicrophone,
  setDictationShortcut: mocks.setDictationShortcut,
  listProviderModels: mocks.listProviderModels,
}));

function model(id: string, name: string, provider = "openrouter") {
  return { id, name, provider, modelType: "text", traits: [], capabilities: [] };
}

function settings(
  cleanupEnabled: boolean,
  hudPlacement: "bottom" | "top" = "bottom",
): DictationSettingsDto {
  return {
    pushToTalkShortcut: {
      keyCode: 0,
      code: "Fn",
      label: "Fn",
      pressCount: 1,
      modifiers: { command: false, control: false, option: false, shift: false, function: true },
    },
    toggleShortcut: {
      keyCode: 17,
      code: "KeyT",
      label: "Ctrl+Opt+T",
      pressCount: 1,
      modifiers: { command: false, control: true, option: true, shift: false, function: false },
    },
    microphone: {},
    style: "standard",
    cleanupEnabled,
    cleanupModel: "nvidia/nemotron-3-nano-30b-a3b",
    hudPlacement,
  };
}

function renderSettings(initialTab: SettingsTab = "dictation") {
  const page = (activeTab: SettingsTab) => (
    <AppSettings
      sourceMode="microphoneOnly"
      checkingSourceReadiness={false}
      onSourceModeChange={() => {}}
      onEnableSystemAudio={() => {}}
      activeTab={activeTab}
      onTabChange={() => {}}
    />
  );
  const view = render(page(initialTab));
  return { ...view, showTab: (tab: SettingsTab) => view.rerender(page(tab)) };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((done, fail) => {
    resolve = done;
    reject = fail;
  });
  return { promise, resolve, reject };
}

const cleanupToggle = () => screen.findByRole("switch", { name: "Clean up dictation text" });

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
    mocks.setDictationCleanupModel.mockImplementation(async (cleanupModel: string) => ({
      ...settings(true),
      cleanupModel,
    }));
    mocks.setDictationHudPlacement.mockImplementation(async (placement: "bottom" | "top") =>
      settings(false, placement),
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

  it("refreshes an open cleanup selector after the OpenRouter key changes", async () => {
    mocks.dictationSettings.mockResolvedValue({ settings: settings(true) });
    renderSettings();
    await screen.findByRole("option", { name: "GPT-5 mini" });
    await waitFor(() => expect(mocks.listProviderModels).toHaveBeenCalledWith("video"));
    const callsBeforeRefresh = mocks.listProviderModels.mock.calls.length;
    mocks.listProviderModels.mockResolvedValue({
      models: [model("vendor/new", "New catalog model")],
    });
    act(() => dispatchProviderModelSettingsChanged({ mode: "generation", modelId: "vendor/new" }));
    expect(await screen.findByRole("option", { name: "New catalog model" })).toBeInTheDocument();
    expect(mocks.listProviderModels).toHaveBeenCalledTimes(callsBeforeRefresh + 1);
  });

  it("starts off and hides the model and style until cleanup is turned on", async () => {
    renderSettings();

    const toggle = await screen.findByRole("switch", { name: "Clean up dictation text" });
    await waitFor(() => expect(toggle).toHaveAttribute("aria-checked", "false"));
    expect(screen.queryByRole("combobox", { name: "Cleanup model" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Formal" })).not.toBeInTheDocument();
  });

  it("turns cleanup on and offers only OpenRouter models", async () => {
    const user = userEvent.setup();
    renderSettings();

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
    expect(mocks.setDictationCleanupModel).toHaveBeenCalledWith("openai/gpt-5-mini");
    await waitFor(() => expect(picker).toHaveValue("openai/gpt-5-mini"));
    expect(mocks.setDictationCleanup).toHaveBeenCalledTimes(1);
  });

  it.each([
    true,
    false,
  ])("reflects saved cleanup %s in both tabs from one read", async (enabled) => {
    const saved = { ...settings(enabled), cleanupModel: "vendor/retired", style: "formal" };
    mocks.dictationSettings.mockResolvedValueOnce({ settings: saved });
    const view = renderSettings("models");

    const modelToggle = await cleanupToggle();
    expect(modelToggle).toHaveAttribute("aria-checked", String(enabled));
    const moreOptions = screen.getByRole("button", { name: "More options for voice" });
    expect(moreOptions).toHaveAttribute("aria-expanded", "false");
    expect(
      modelToggle.compareDocumentPosition(moreOptions) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();

    view.showTab("dictation");
    expect(await cleanupToggle()).toHaveAttribute("aria-checked", String(enabled));
    expect(mocks.dictationSettings).toHaveBeenCalledTimes(1);
    if (enabled) {
      expect(screen.getByRole("combobox", { name: "Cleanup model" })).toHaveValue("vendor/retired");
      expect(screen.getByRole("button", { name: "Formal" })).toHaveAttribute(
        "aria-pressed",
        "true",
      );
    }
  });

  it("shows an unknown preference during a pending or failed read in both tabs", async () => {
    const read = deferred<{ settings: DictationSettingsDto }>();
    mocks.dictationSettings.mockReturnValueOnce(read.promise);
    const view = renderSettings("models");

    expect(screen.getByRole("heading", { name: "Clean up dictation text" })).toBeVisible();
    expect(screen.getByText("Loading dictation settings…")).toBeVisible();
    expect(screen.queryByRole("switch", { name: "Clean up dictation text" })).toBeNull();

    await act(async () => read.reject(new Error("Settings unavailable")));
    expect(screen.getByText("Could not load dictation settings.")).toBeVisible();
    view.showTab("dictation");
    expect(screen.getByText("Could not load dictation settings.")).toBeVisible();
    expect(screen.queryByRole("switch", { name: "Clean up dictation text" })).toBeNull();
    expect(mocks.setDictationCleanup).not.toHaveBeenCalled();
  });

  it("locks rapid and cross-tab actions and keeps the confirmed value on save failure", async () => {
    const save = deferred<DictationSettingsDto>();
    mocks.dictationSettings.mockResolvedValueOnce({ settings: settings(true) });
    mocks.setDictationCleanup.mockReturnValueOnce(save.promise);
    const view = renderSettings("models");

    const toggle = await cleanupToggle();
    act(() => {
      fireEvent.click(toggle);
      fireEvent.click(toggle);
    });
    expect(mocks.setDictationCleanup).toHaveBeenCalledTimes(1);
    expect(mocks.setDictationCleanup).toHaveBeenCalledWith(false);
    expect(toggle).toHaveAttribute("aria-checked", "true");
    expect(toggle).toBeDisabled();

    view.showTab("dictation");
    expect(await cleanupToggle()).toBeDisabled();
    expect(screen.getByRole("combobox", { name: "Cleanup model" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Formal" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Top" })).toBeDisabled();
    await act(async () => save.reject(new Error("Could not write settings")));
    expect(await cleanupToggle()).toHaveAttribute("aria-checked", "true");
    expect(await cleanupToggle()).toBeEnabled();
    expect(screen.getByRole("alert")).toHaveTextContent("Could not write settings");
    view.showTab("models");
    expect(await cleanupToggle()).toHaveAttribute("aria-checked", "true");
    expect(screen.getByRole("alert")).toHaveTextContent("Could not write settings");
  });

  it("merges only the model and ON/OFF fields from their responses", async () => {
    const save = deferred<DictationSettingsDto>();
    const saved = { ...settings(true), style: "formal" as const };
    mocks.dictationSettings.mockResolvedValueOnce({ settings: saved });
    mocks.setDictationCleanupModel.mockReturnValueOnce(save.promise);
    const view = renderSettings();
    const user = userEvent.setup();
    const picker = await screen.findByRole("combobox", { name: "Cleanup model" });
    await within(picker).findByRole("option", { name: "GPT-5 mini" });
    await user.selectOptions(picker, "openai/gpt-5-mini");
    expect(mocks.setDictationCleanupModel).toHaveBeenCalledWith("openai/gpt-5-mini");
    view.showTab("models");
    const toggle = await cleanupToggle();
    expect(toggle).toBeDisabled();
    await act(async () => save.resolve({ ...settings(false), cleanupModel: "openai/gpt-5-mini" }));
    expect(toggle).toHaveAttribute("aria-checked", "true");

    // The OFF response carries a stale model and style.
    mocks.setDictationCleanup.mockResolvedValueOnce(settings(false));
    await user.click(toggle);
    expect(toggle).toHaveAttribute("aria-checked", "false");
    await user.click(toggle);
    view.showTab("dictation");
    expect(screen.getByRole("combobox", { name: "Cleanup model" })).toHaveValue(
      "openai/gpt-5-mini",
    );
    expect(screen.getByRole("button", { name: "Formal" })).toHaveAttribute("aria-pressed", "true");
  });

  it("does not present a style that could not be saved", async () => {
    const save = deferred<DictationSettingsDto>();
    mocks.dictationSettings.mockResolvedValueOnce({ settings: settings(true) });
    mocks.setDictationStyle.mockReturnValueOnce(save.promise);
    renderSettings();
    const user = userEvent.setup();

    const formal = await screen.findByRole("button", { name: "Formal" });
    await user.click(formal);
    expect(formal).toHaveAttribute("aria-pressed", "false");
    expect(screen.getByRole("button", { name: "Standard" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    await act(async () => save.reject(new Error("Style write failed")));
    expect(formal).toHaveAttribute("aria-pressed", "false");
    expect(screen.getByRole("alert")).toHaveTextContent("Style write failed");
  });

  it("late language, microphone and shortcut responses cannot re-enable cleanup", async () => {
    const language = deferred<DictationSettingsDto>();
    const microphone = deferred<DictationSettingsDto>();
    const shortcut = deferred<DictationSettingsDto>();
    mocks.dictationSettings.mockResolvedValueOnce({ settings: settings(true) });
    mocks.setDictationLanguage.mockReturnValueOnce(language.promise);
    mocks.setDictationMicrophone.mockReturnValueOnce(microphone.promise);
    mocks.setDictationShortcut.mockReturnValueOnce(shortcut.promise);
    const view = renderSettings();
    const user = userEvent.setup();

    await user.click(screen.getByRole("button", { name: "Default transcription language" }));
    await user.click(screen.getByRole("option", { name: "Portuguese" }));
    expect(mocks.setDictationLanguage).toHaveBeenCalledWith("pt");

    view.showTab("audio");
    await user.click(screen.getByRole("button", { name: "Auto-detect" }));
    await user.click(screen.getByRole("option", { name: "Auto-detect" }));
    expect(mocks.setDictationMicrophone).toHaveBeenCalledTimes(1);

    view.showTab("shortcuts");
    await user.click(
      screen.getByRole("button", { name: "Reset Push to talk shortcut to default" }),
    );
    expect(mocks.setDictationShortcut).toHaveBeenCalledTimes(1);

    view.showTab("models");
    const toggle = await cleanupToggle();
    await user.click(toggle);
    expect(toggle).toHaveAttribute("aria-checked", "false");
    await act(async () => shortcut.resolve(settings(true)));
    expect(toggle).toHaveAttribute("aria-checked", "false");
    await act(async () => language.resolve({ ...settings(true), language: "pt" }));
    expect(toggle).toHaveAttribute("aria-checked", "false");
    await act(async () => microphone.resolve(settings(true)));
    expect(toggle).toHaveAttribute("aria-checked", "false");
    view.showTab("dictation");
    expect(
      screen.getByRole("button", { name: "Default transcription language" }),
    ).toHaveTextContent("Portuguese");
  });

  it("keeps a language write that finishes before the initial settings read", async () => {
    const read = deferred<{ settings: DictationSettingsDto }>();
    mocks.dictationSettings.mockReturnValueOnce(read.promise);
    mocks.setDictationLanguage.mockResolvedValueOnce({ ...settings(false), language: "pt" });
    renderSettings();
    const user = userEvent.setup();

    const language = screen.getByRole("button", { name: "Default transcription language" });
    await user.click(language);
    await user.click(screen.getByRole("option", { name: "Portuguese" }));
    expect(language).toHaveTextContent("Portuguese");

    await act(async () => read.resolve({ settings: settings(true) }));
    expect(language).toHaveTextContent("Portuguese");
    expect(await cleanupToggle()).toHaveAttribute("aria-checked", "true");
  });

  it("ignores a read or save from a settings instance that has been closed", async () => {
    const read = deferred<{ settings: DictationSettingsDto }>();
    mocks.dictationSettings.mockReturnValueOnce(read.promise);
    renderSettings("models").unmount();

    const save = deferred<DictationSettingsDto>();
    mocks.setDictationCleanup.mockReturnValueOnce(save.promise);
    const second = renderSettings("dictation");
    await userEvent.setup().click(await cleanupToggle());
    second.unmount();

    renderSettings("models");
    await act(async () => {
      read.resolve({ settings: settings(true) });
      save.resolve(settings(true));
    });
    expect(await cleanupToggle()).toHaveAttribute("aria-checked", "false");
  });

  it("loads indicator position as top, attempts switching to bottom, and reverts to top if persistence fails", async () => {
    const user = userEvent.setup();
    mocks.dictationSettings.mockResolvedValue({ settings: settings(false, "top") });
    mocks.setDictationHudPlacement.mockRejectedValueOnce(new Error("Failed to save placement"));

    renderSettings();

    const topButton = await screen.findByRole("button", { name: "Top" });
    const bottomButton = screen.getByRole("button", { name: "Bottom" });
    await waitFor(() => expect(topButton).toHaveAttribute("aria-pressed", "true"));
    expect(bottomButton).toHaveAttribute("aria-pressed", "false");

    await user.click(bottomButton);

    await waitFor(() => expect(mocks.setDictationHudPlacement).toHaveBeenCalledWith("bottom"));
    await waitFor(() => expect(topButton).toHaveAttribute("aria-pressed", "true"));
    expect(bottomButton).toHaveAttribute("aria-pressed", "false");
    expect(screen.getByRole("alert")).toHaveTextContent("Failed to save placement");
  });
});
