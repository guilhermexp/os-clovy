import { rawLocalGenerationModelId } from "./local-generation";
import { invoke } from "./tauri";

export const CLI_ENGINE_MODEL_PREFIX = "__clovy_cli_engine__:";

export type ChatEngineCliToolsStatus = "available" | "server_off" | "unsupported";

export type ChatEngineCliStatus = {
  id: string;
  name: string;
  installed: boolean;
  reason: string | null;
  modelId: string;
  clovyTools: ChatEngineCliToolsStatus;
};

export type ChatEngineEndpoint = {
  id: string;
  name: string;
  modelId: string;
  optionId: string;
};

export type ChatEngineCatalog = {
  clis: ChatEngineCliStatus[];
  endpoints: ChatEngineEndpoint[];
};

/** The engines a chat session can use. Missing lists read as empty, so a
 * partial answer never breaks the composer. */
export async function chatEngineCatalog(): Promise<ChatEngineCatalog> {
  const catalog = await invoke<Partial<ChatEngineCatalog> | null | undefined>(
    "chat_engine_catalog",
  );
  return {
    clis: Array.isArray(catalog?.clis) ? catalog.clis : [],
    endpoints: Array.isArray(catalog?.endpoints) ? catalog.endpoints : [],
  };
}

export type ChatEngineClassification =
  | { kind: "clovy" }
  | { kind: "cli"; cli: string }
  | { kind: "endpoint"; modelId: string };

export function classifyChatEngineModel(
  modelId: string | null | undefined,
  catalog?: Pick<ChatEngineCatalog, "endpoints"> | null,
  clovyModels?: ReadonlyArray<{ id: string }> | null,
): ChatEngineClassification {
  if (!modelId) {
    return { kind: "clovy" };
  }
  if (modelId.startsWith(CLI_ENGINE_MODEL_PREFIX)) {
    return { kind: "cli", cli: modelId.slice(CLI_ENGINE_MODEL_PREFIX.length) };
  }
  const rawLocal = rawLocalGenerationModelId(modelId);
  if (rawLocal !== null) {
    return { kind: "endpoint", modelId: rawLocal };
  }
  if (catalog?.endpoints?.some((ep) => ep.modelId === modelId)) {
    const isClovyModel = clovyModels?.some((model) => model.id === modelId) ?? false;
    if (!isClovyModel) {
      return { kind: "endpoint", modelId };
    }
  }
  return { kind: "clovy" };
}
