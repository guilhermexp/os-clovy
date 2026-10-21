import { cleanup, render, screen, within, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, it, vi } from "vitest";

const invokeMock = vi.fn();

vi.mock("@tauri-apps/api/core", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@tauri-apps/api/core")>()),
  invoke: (command: string, args?: unknown) =>
    args !== undefined ? invokeMock(command, args) : invokeMock(command),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => () => {}),
  emit: vi.fn(async () => undefined),
}));

import { AppSettings } from "../components/settings/AppSettings";
import type { ProviderModelSettingsDto } from "../lib/tauri";

const baseSettings: ProviderModelSettingsDto = {
  transcriptionProvider: "groq",
  generationProvider: "openrouter",
  transcriptionModel: "whisper-large-v3-turbo",
  generationModel: "z-ai/glm-5.2",
  costQuality: 100,
  remoteGenerationModel: "z-ai/glm-5.2",
  imageModel: "fal-ai/flux/dev",
  videoModel: "fal-ai/wan/v2.2-a14b/text-to-video",
  providerApiKeysConfigured: {
    groq: false,
    openrouter: false,
    fal: false,
    tavily: false,
  },
  localGeneration: {
    baseUrl: "",
    modelId: "",
  },
  imageSafeMode: true,
  imageSafeModePromptDismissed: false,
  liveTranscription: true,
};

import { IMAGE_MODELS } from "../lib/image-models";
import { VIDEO_MODELS } from "../lib/video-models";

function renderSettings() {
  return render(
    <AppSettings
      sourceMode="microphoneOnly"
      checkingSourceReadiness={false}
      onSourceModeChange={() => {}}
      onEnableSystemAudio={() => {}}
      activeTab="models"
      onTabChange={() => {}}
    />,
  );
}

function backend(availability: boolean | "error", hasKey = false) {
  let settings = {
    ...baseSettings,
    providerApiKeysConfigured: { ...baseSettings.providerApiKeysConfigured, fal: hasKey },
  };
  const state = { availability };
  invokeMock.mockImplementation((command: string, args?: { request?: { mode?: string } }) => {
    if (command === "dictation_settings")
      return Promise.resolve({
        settings: {
          language: "en",
          microphone: {},
          style: "standard",
          pushToTalkShortcut: { modifiers: {} },
          toggleShortcut: { modifiers: {} },
        },
      });
    if (command === "dictation_hotkey_status") return Promise.resolve({ type: "ready" });
    if (command === "provider_model_settings")
      return Promise.resolve({ settings, effectiveSettings: settings });
    if (command === "list_provider_models") {
      if (state.availability === "error") return Promise.reject(new Error("Catalog unavailable"));
      return Promise.resolve({
        mode: args?.request?.mode,
        models: (args?.request?.mode === "image"
          ? IMAGE_MODELS
          : args?.request?.mode === "video"
            ? VIDEO_MODELS
            : []
        ).map((model) => ({ ...model, serviceAvailable: state.availability })),
      });
    }
    if (command === "set_provider_api_key" || command === "clear_provider_api_key") {
      settings = {
        ...settings,
        providerApiKeysConfigured: {
          ...settings.providerApiKeysConfigured,
          fal: command === "set_provider_api_key",
        },
      };
      return Promise.resolve(settings);
    }
    return Promise.reject(new Error("unavailable test command"));
  });
  return state;
}

afterEach(cleanup);

it.each([
  "image",
  "video",
] as const)("F3 %s picker uses live missing-key availability", async (mode) => {
  backend(false);
  renderSettings();
  const user = userEvent.setup();
  await user.click(
    await screen.findByRole("button", { name: new RegExp(`change ${mode} model`, "i") }),
  );
  expect(await screen.findByText(/Clovy has no fal service key here/)).toBeInTheDocument();
  expect(within(screen.getByRole("dialog")).queryAllByRole("option")).toHaveLength(0);
});

it.each([
  [true, false],
  [false, true],
  ["error", true],
] as const)("F3 funding service=%s BYOK=%s makes both media lists selectable", async (service, key) => {
  backend(service, key);
  renderSettings();
  const user = userEvent.setup();
  for (const mode of ["image", "video"]) {
    await user.click(
      await screen.findByRole("button", { name: new RegExp(`change ${mode} model`, "i") }),
    );
    await waitFor(() =>
      expect(within(screen.getByRole("dialog")).getAllByRole("option").length).toBeGreaterThan(0),
    );
    expect(screen.queryByText(/Clovy has no fal service key here/)).not.toBeInTheDocument();
    expect(screen.queryByText(/could not confirm media availability/)).not.toBeInTheDocument();
    await user.keyboard("{Escape}");
  }
});

it("F3 refresh failure clears prior availability and reopening recovers", async () => {
  const state = backend(true);
  renderSettings();
  const user = userEvent.setup();
  const button = await screen.findByRole("button", { name: /change image model/i });
  await user.click(button);
  await waitFor(() =>
    expect(within(screen.getByRole("dialog")).getAllByRole("option").length).toBeGreaterThan(0),
  );
  await user.keyboard("{Escape}");
  state.availability = "error";
  await user.click(button);
  expect(await screen.findByText(/could not confirm media availability/)).toBeInTheDocument();
  expect(within(screen.getByRole("dialog")).queryAllByRole("option")).toHaveLength(0);
  expect(screen.queryByText(/Clovy has no fal service key here/)).not.toBeInTheDocument();
  await user.keyboard("{Escape}");
  state.availability = false;
  await user.click(button);
  expect(await screen.findByText(/Clovy has no fal service key here/)).toBeInTheDocument();
});

it("F3 saving and clearing fal key immediately updates both media notices", async () => {
  backend(false);
  renderSettings();
  const user = userEvent.setup();
  await user.click(await screen.findByRole("button", { name: /more options for text/i }));
  await user.type(await screen.findByLabelText(/fal api key/i), "fixture-key");
  await user.click(screen.getAllByRole("button", { name: "Save" })[2]);
  expect(await screen.findByText("Configured")).toBeInTheDocument();
  for (const mode of ["image", "video"]) {
    await user.click(screen.getByRole("button", { name: new RegExp(`change ${mode} model`, "i") }));
    await waitFor(() =>
      expect(within(screen.getByRole("dialog")).getAllByRole("option").length).toBeGreaterThan(0),
    );
    await user.keyboard("{Escape}");
  }
  await user.click(screen.getByRole("button", { name: "Remove" }));
  await waitFor(() => expect(screen.queryByText("Configured")).not.toBeInTheDocument());
  for (const mode of ["image", "video"]) {
    await user.click(screen.getByRole("button", { name: new RegExp(`change ${mode} model`, "i") }));
    expect(await screen.findByText(/Clovy has no fal service key here/)).toBeInTheDocument();
    await user.keyboard("{Escape}");
  }
});
