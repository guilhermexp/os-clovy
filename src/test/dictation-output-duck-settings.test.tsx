import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AppSettings } from "../components/settings/AppSettings";
import type { SettingsTab } from "../components/settings/settings-config";

const ipc = vi.hoisted(() => ({ invoke: vi.fn() }));

vi.mock("@tauri-apps/api/core", async (original) => ({
  ...(await original<typeof import("@tauri-apps/api/core")>()),
  invoke: ipc.invoke,
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => () => {}),
  emit: vi.fn(),
}));

const setting = {
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
  microphone: { id: "studio", name: "Studio microphone" },
  style: "formal",
  cleanupEnabled: false,
  cleanupModel: "nvidia/nemotron-3-nano-30b-a3b",
  cleanupMigrated: true,
  hudPlacement: "bottom",
  language: "pt",
};

const label = "Reduce system volume during dictation";
const toggle = () => screen.findByRole("switch", { name: label });

function openSettings() {
  const page = (activeTab: SettingsTab) => (
    <AppSettings
      sourceMode="microphoneOnly"
      checkingSourceReadiness={false}
      onSourceModeChange={() => {}}
      onEnableSystemAudio={() => {}}
      activitySupported={false}
      activeTab={activeTab}
      onTabChange={() => {}}
    />
  );
  const view = render(page("dictation"));
  return { ...view, showAudio: () => view.rerender(page("audio")) };
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

describe("dictation output volume settings", () => {
  let platform: string;
  let stored: typeof setting & { reduceSystemVolume?: boolean };
  let save: ReturnType<typeof deferred<typeof stored>> | undefined;

  beforeEach(() => {
    platform = "macos";
    stored = { ...setting };
    save = undefined;
    ipc.invoke.mockReset();
    ipc.invoke.mockImplementation(async (command: string, args?: { enabled: boolean }) => {
      switch (command) {
        case "dictation_capabilities":
          return {
            capabilities: {
              available: true,
              platform,
              shortcuts: true,
              paste: true,
              microphoneSelection: true,
              accessibilityPermission: true,
              systemAudio: false,
            },
          };
        case "dictation_settings":
          return { settings: stored };
        case "set_dictation_reduce_system_volume":
          if (save) return save.promise;
          stored = { ...stored, reduceSystemVolume: args?.enabled };
          return stored;
        case "dictation_hotkey_status":
          return { type: "hotkey_trigger_ready" };
        case "provider_model_settings":
          return { settings: { transcriptionProvider: "groq" } };
        case "list_dictionary_entries":
          return [];
        case "list_provider_models":
          return { models: [] };
        default:
          return undefined;
      }
    });
  });

  it("defaults older snapshots to off and persists the new value across reopening", async () => {
    const view = openSettings();
    const user = userEvent.setup();
    const control = await toggle();
    expect(control).toHaveAttribute("aria-checked", "false");
    expect(control).toHaveAccessibleDescription(/20% of its current volume/);
    await user.click(control);
    expect(control).toHaveAttribute("aria-checked", "true");
    expect(ipc.invoke).toHaveBeenCalledWith("set_dictation_reduce_system_volume", {
      enabled: true,
    });
    view.unmount();

    openSettings();
    const reopened = await toggle();
    expect(reopened).toHaveAttribute("aria-checked", "true");
    await user.click(reopened);
    expect(reopened).toHaveAttribute("aria-checked", "false");
  });

  it("keeps the confirmed value and reports a rejected save", async () => {
    stored.reduceSystemVolume = true;
    save = deferred();
    openSettings();
    const control = await toggle();
    await userEvent.setup().click(control);
    expect(control).toBeDisabled();
    expect(control).toHaveAttribute("aria-checked", "true");
    await act(async () => save?.reject(new Error("Could not write settings")));
    expect(control).toBeEnabled();
    expect(control).toHaveAttribute("aria-checked", "true");
    expect(screen.getByRole("alert")).toHaveTextContent("Could not write settings");
  });

  it("merges only the volume preference from the save response", async () => {
    save = deferred();
    const view = openSettings();
    await userEvent.setup().click(await toggle());
    await act(async () =>
      save?.resolve({
        ...setting,
        microphone: { id: "other", name: "Old mic" },
        reduceSystemVolume: true,
      }),
    );
    expect(await toggle()).toHaveAttribute("aria-checked", "true");
    view.showAudio();
    expect(screen.getByRole("button", { name: "Studio microphone" })).toBeVisible();
  });

  it("ignores a late save from a closed settings instance", async () => {
    save = deferred();
    const closed = openSettings();
    await userEvent.setup().click(await toggle());
    closed.unmount();

    openSettings();
    expect(await toggle()).toHaveAttribute("aria-checked", "false");
    await act(async () => save?.resolve({ ...stored, reduceSystemVolume: true }));
    expect(await toggle()).toHaveAttribute("aria-checked", "false");
  });

  it.each(["windows", "unsupported"])("omits the control on %s", async (value) => {
    platform = value;
    openSettings();
    await waitFor(() => expect(ipc.invoke).toHaveBeenCalledWith("dictation_settings"));
    expect(await screen.findByRole("heading", { name: "Dictation" })).toBeVisible();
    expect(screen.queryByRole("switch", { name: label })).toBeNull();
  });
});
