import { describe, expect, it } from "vitest";
import {
  companionModelOptions,
  companionSessionModelSelection,
  companionStoredModelId,
} from "../lib/companion-models";
import type { ChatEngineCliStatus } from "../lib/chat-engine";
import type { ProviderApiKeysConfigured, ProviderModelDto } from "../lib/tauri";

function cliEngine(id: string, name: string, installed = true): ChatEngineCliStatus {
  return {
    id,
    name,
    installed,
    reason: installed ? null : "Not found in PATH",
    modelId: `__clovy_cli_engine__:${id}`,
    clovyTools: "available",
  };
}

const NO_KEYS: ProviderApiKeysConfigured = {
  groq: false,
  openrouter: false,
  fal: false,
  tavily: false,
};

function model(id: string, overrides: Partial<ProviderModelDto> = {}): ProviderModelDto {
  return {
    provider: "openrouter",
    id,
    name: id,
    modelType: "text",
    traits: [],
    capabilities: ["supportsFunctionCalling"],
    priceUnit: "tokens",
    priceDescription: "",
    inputCreditsPerMillionTokens: 850,
    outputCreditsPerMillionTokens: 4660,
    ...overrides,
  };
}

describe("companion model projection", () => {
  it("returns Auto plus only live, agent-capable desktop suggestions in curated order", () => {
    const options = companionModelOptions(
      [
        model("moonshotai/kimi-k2.6", { name: "Kimi K2.6" }),
        model("uncurated-model"),
        model("moonshotai/kimi-k3", { name: "Kimi K3" }),
        model("z-ai/glm-5.2", { name: "GLM 5.2" }),
        model("z-ai/glm-5.1", { name: "GLM 5.1", capabilities: [] }),
      ],
      NO_KEYS,
    );

    expect(options.map((option) => option.id)).toEqual([
      "open-software/auto",
      "z-ai/glm-5.2",
      "moonshotai/kimi-k3",
      "moonshotai/kimi-k2.6",
    ]);
    expect(options[0]).toMatchObject({
      name: "Auto",
      routing: "automatic",
    });
    expect(options[3]).toMatchObject({
      name: "Kimi K2.6",
      routing: "remote",
      priceLabel: "$0.85 input / $4.66 output per 1M tokens",
    });
  });

  it("offers a model that needs the user's own key only once that key is saved", () => {
    const catalog = [
      model("z-ai/glm-5.2", { name: "GLM 5.2" }),
      model("moonshotai/kimi-k3", { name: "Kimi K3", serviceAvailable: false }),
    ];

    expect(companionModelOptions(catalog, NO_KEYS).map((option) => option.id)).toEqual([
      "open-software/auto",
      "z-ai/glm-5.2",
    ]);
    expect(
      companionModelOptions(catalog, { ...NO_KEYS, openrouter: true }).map((option) => option.id),
    ).toEqual(["open-software/auto", "z-ai/glm-5.2", "moonshotai/kimi-k3"]);
    // Another provider's key does not unlock an OpenRouter row.
    expect(
      companionModelOptions(catalog, { ...NO_KEYS, groq: true }).map((option) => option.id),
    ).toEqual(["open-software/auto", "z-ai/glm-5.2"]);
  });

  it("projects staged Auto preferences and names historical concrete models", () => {
    expect(
      companionSessionModelSelection("session-1", "__june_auto_generation__:20", [], 100),
    ).toEqual({
      storedSessionId: "session-1",
      modelId: "open-software/auto",
      modelName: "Auto",
      costQuality: 20,
    });
    expect(
      companionSessionModelSelection(
        "session-2",
        "retired-model",
        [model("retired-model", { name: "Retired model" })],
        100,
      ),
    ).toEqual({
      storedSessionId: "session-2",
      modelId: "retired-model",
      modelName: "Retired model",
    });
    expect(companionStoredModelId("open-software/auto", 49.6)).toBe("__june_auto_generation__:50");
  });

  it("offers the installed agent CLIs first, the default one on top, then the legacy models", () => {
    const options = companionModelOptions([model("z-ai/glm-5.2", { name: "GLM 5.2" })], NO_KEYS, [
      cliEngine("pi", "Pi"),
      cliEngine("codex", "Codex"),
      cliEngine("claude", "Claude Code"),
      cliEngine("copilot", "GitHub Copilot", false),
    ]);
    expect(options.map((option) => option.id)).toEqual([
      "__clovy_cli_engine__:claude",
      "__clovy_cli_engine__:pi",
      "__clovy_cli_engine__:codex",
      "open-software/auto",
      "z-ai/glm-5.2",
    ]);
    expect(options[0]).toMatchObject({
      name: "Claude Code",
      description: "Default. Runs on Claude Code on your Mac.",
      routing: "remote",
    });
    expect(options[2].description).toBe("Runs on Codex on your Mac.");
  });

  it("names a session on a CLI engine by the CLI", () => {
    expect(
      companionSessionModelSelection("session-cli", "__clovy_cli_engine__:codex", [], 50, [
        cliEngine("codex", "Codex"),
      ]),
    ).toEqual({
      storedSessionId: "session-cli",
      modelId: "__clovy_cli_engine__:codex",
      modelName: "Codex",
    });
    expect(companionStoredModelId("__clovy_cli_engine__:codex", 50)).toBe(
      "__clovy_cli_engine__:codex",
    );
  });
});
