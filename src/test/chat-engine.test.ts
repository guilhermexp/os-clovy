import { describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  invoke: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: mocks.invoke,
}));

import {
  CLI_ENGINE_MODEL_PREFIX,
  type ChatEngineCatalog,
  chatEngineCatalog,
  classifyChatEngineModel,
} from "../lib/chat-engine";

describe("chat-engine helpers", () => {
  const sampleCatalog: ChatEngineCatalog = {
    clis: [
      {
        id: "claude",
        name: "Claude Code",
        installed: true,
        reason: null,
        modelId: "__clovy_cli_engine__:claude",
        clovyTools: "available",
      },
      {
        id: "pi",
        name: "Pi",
        installed: false,
        reason: "Executable not found",
        modelId: "__clovy_cli_engine__:pi",
        clovyTools: "unsupported",
      },
    ],
    endpoints: [
      {
        id: "local",
        name: "Local model",
        modelId: "llama3",
        optionId: "__june_local_generation__:llama3",
      },
      {
        id: "remote-ep",
        name: "Remote endpoint",
        modelId: "qwen-2.5",
        optionId: "__june_local_generation__:qwen-2.5",
      },
    ],
  };

  it("classifies undefined and null models as Clovy", () => {
    expect(classifyChatEngineModel(undefined)).toEqual({ kind: "clovy" });
    expect(classifyChatEngineModel(null)).toEqual({ kind: "clovy" });
    expect(classifyChatEngineModel("")).toEqual({ kind: "clovy" });
  });

  it("classifies CLI engine models by prefix", () => {
    expect(classifyChatEngineModel(`${CLI_ENGINE_MODEL_PREFIX}claude`)).toEqual({
      kind: "cli",
      cli: "claude",
    });
    expect(classifyChatEngineModel(`${CLI_ENGINE_MODEL_PREFIX}codex`)).toEqual({
      kind: "cli",
      cli: "codex",
    });
  });

  it("classifies local generation tagged option ids as endpoints", () => {
    expect(classifyChatEngineModel("__june_local_generation__:llama3")).toEqual({
      kind: "endpoint",
      modelId: "llama3",
    });
    expect(classifyChatEngineModel("__june_local_generation__:meta%2Fllama-3")).toEqual({
      kind: "endpoint",
      modelId: "meta/llama-3",
    });
  });

  it("classifies raw model ids matching a catalog endpoint when not in Clovy models", () => {
    expect(classifyChatEngineModel("llama3", sampleCatalog, [])).toEqual({
      kind: "endpoint",
      modelId: "llama3",
    });
  });

  it("prefers Clovy catalog when a model id exists in Clovy models list", () => {
    expect(classifyChatEngineModel("llama3", sampleCatalog, [{ id: "llama3" }])).toEqual({
      kind: "clovy",
    });
  });

  it("classifies standard remote models as Clovy", () => {
    expect(classifyChatEngineModel("gpt-4o", sampleCatalog)).toEqual({
      kind: "clovy",
    });
    expect(classifyChatEngineModel("auto", sampleCatalog)).toEqual({
      kind: "clovy",
    });
  });

  it("invokes chat_engine_catalog command", async () => {
    mocks.invoke.mockResolvedValueOnce(sampleCatalog);
    const result = await chatEngineCatalog();
    expect(mocks.invoke).toHaveBeenCalledWith("chat_engine_catalog");
    expect(result).toEqual(sampleCatalog);
  });
});
