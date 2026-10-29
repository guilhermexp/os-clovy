import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

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

describe("Provider API keys settings", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    invokeMock.mockImplementation((command: string) => {
      if (command === "provider_model_settings") {
        return Promise.resolve({
          settings: baseSettings,
          effectiveSettings: baseSettings,
        });
      }
      if (command === "dictation_settings") {
        return Promise.resolve({
          language: "en",
          shortcut: "ctrl_alt_t",
        });
      }
      if (command === "list_provider_models") {
        return Promise.resolve({
          mode: "generation",
          selectedModel: "z-ai/glm-5.2",
          models: [],
        });
      }
      return Promise.reject(new Error("no tauri in tests"));
    });
  });

  afterEach(() => {
    cleanup();
  });

  function renderAppSettings() {
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

  it("saves a key with set_provider_api_key and shows configured state", async () => {
    const user = userEvent.setup();
    const updatedSettings: ProviderModelSettingsDto = {
      ...baseSettings,
      providerApiKeysConfigured: { ...baseSettings.providerApiKeysConfigured, groq: true },
    };

    invokeMock.mockImplementation((command: string) => {
      if (command === "provider_model_settings") {
        return Promise.resolve({ settings: baseSettings, effectiveSettings: baseSettings });
      }
      if (command === "list_provider_models") {
        return Promise.resolve({ mode: "generation", selectedModel: "z-ai/glm-5.2", models: [] });
      }
      if (command === "set_provider_api_key") {
        return Promise.resolve(updatedSettings);
      }
      return Promise.reject(new Error("no tauri in tests"));
    });

    renderAppSettings();

    const moreTextOptionsBtn = await screen.findByRole("button", {
      name: /more options for text/i,
    });
    await user.click(moreTextOptionsBtn);

    const groqInput = await screen.findByLabelText(/groq api key/i);
    await user.type(groqInput, "gsk_test123");

    const saveButtons = screen.getAllByRole("button", { name: "Save" });
    await user.click(saveButtons[0]);

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("set_provider_api_key", {
        request: { provider: "groq", apiKey: "gsk_test123" },
      });
    });

    await waitFor(() => {
      expect(screen.getByText("Configured")).toBeInTheDocument();
    });
  });

  it("shows provider refusal message on provider_api_key_rejected and does not mark configured", async () => {
    const user = userEvent.setup();

    invokeMock.mockImplementation((command: string) => {
      if (command === "provider_model_settings") {
        return Promise.resolve({ settings: baseSettings, effectiveSettings: baseSettings });
      }
      if (command === "list_provider_models") {
        return Promise.resolve({ mode: "generation", selectedModel: "z-ai/glm-5.2", models: [] });
      }
      if (command === "set_provider_api_key") {
        const error = new Error("Invalid Groq API key.");
        (error as unknown as { code: string }).code = "provider_api_key_rejected";
        return Promise.reject(error);
      }
      return Promise.reject(new Error("no tauri in tests"));
    });

    renderAppSettings();

    const moreTextOptionsBtn = await screen.findByRole("button", {
      name: /more options for text/i,
    });
    await user.click(moreTextOptionsBtn);

    const groqInput = await screen.findByLabelText(/groq api key/i);
    await user.type(groqInput, "gsk_bad");

    const saveButtons = screen.getAllByRole("button", { name: "Save" });
    await user.click(saveButtons[0]);

    await waitFor(() => {
      expect(screen.getByRole("alert")).toHaveTextContent("Invalid Groq API key.");
    });
    expect(screen.queryByText("Configured")).not.toBeInTheDocument();
  });

  it("removes a key with clear_provider_api_key and shows the row as not configured", async () => {
    const user = userEvent.setup();
    const configuredSettings: ProviderModelSettingsDto = {
      ...baseSettings,
      providerApiKeysConfigured: { ...baseSettings.providerApiKeysConfigured, groq: true },
    };

    invokeMock.mockImplementation((command: string) => {
      if (command === "list_provider_models") {
        return Promise.resolve({ mode: "generation", selectedModel: "z-ai/glm-5.2", models: [] });
      }
      if (command === "set_provider_api_key") {
        return Promise.resolve(configuredSettings);
      }
      if (command === "clear_provider_api_key") {
        return Promise.resolve(baseSettings);
      }
      return Promise.reject(new Error("no tauri in tests"));
    });

    renderAppSettings();

    const moreTextOptionsBtn = await screen.findByRole("button", {
      name: /more options for text/i,
    });
    await user.click(moreTextOptionsBtn);
    await user.type(await screen.findByLabelText(/groq api key/i), "gsk_test123");
    await user.click(screen.getAllByRole("button", { name: "Save" })[0]);
    expect(await screen.findByText("Configured")).toBeInTheDocument();

    const removeBtn = await screen.findByRole("button", { name: "Remove" });
    await user.click(removeBtn);

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("clear_provider_api_key", {
        request: { provider: "groq" },
      });
    });
    await waitFor(() => {
      expect(screen.queryByText("Configured")).not.toBeInTheDocument();
    });
    expect(screen.queryByRole("button", { name: "Remove" })).not.toBeInTheDocument();
  });
});
