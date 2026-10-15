import { localGenerationEndpointId, rawLocalGenerationModelId } from "./local-generation";
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
  /** `endpointId` is null for an older option that only names a model. */
  | { kind: "endpoint"; modelId: string; endpointId: string | null };

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
    return { kind: "endpoint", modelId: rawLocal, endpointId: localGenerationEndpointId(modelId) };
  }
  if (catalog?.endpoints?.some((ep) => ep.modelId === modelId)) {
    const isClovyModel = clovyModels?.some((model) => model.id === modelId) ?? false;
    if (!isClovyModel) {
      return { kind: "endpoint", modelId, endpointId: null };
    }
  }
  return { kind: "clovy" };
}

/** The catalog endpoint a session model selects: the one it names, else
 * (older options that only carry a model id) the first serving that model,
 * which is what the agent route resolves too. */
export function selectedChatEndpoint(
  model: string | null | undefined,
  catalog: Pick<ChatEngineCatalog, "endpoints"> | null | undefined,
  clovyModels?: ReadonlyArray<{ id: string }> | null,
): ChatEngineEndpoint | undefined {
  const engine = classifyChatEngineModel(model, catalog, clovyModels);
  if (engine.kind !== "endpoint") return undefined;
  const endpoints = catalog?.endpoints ?? [];
  if (engine.endpointId !== null) {
    return endpoints.find((endpoint) => endpoint.id === engine.endpointId);
  }
  return endpoints.find((endpoint) => endpoint.modelId === engine.modelId);
}

/** The CLIs that are the default chat engine, in order of preference
 * (ADR-0062): Claude Code, else Codex. */
export const DEFAULT_CHAT_CLI_IDS = ["claude", "codex"] as const;

export type ChatDefaultEngine = {
  /** The session model id of the CLI chat runs on; null when chat is on the
   * legacy Clovy agent or an endpoint (callers keep their own default). */
  modelId: string | null;
  cli: string | null;
  /** Chat is on the legacy Clovy agent only because neither Claude Code nor
   * Codex is installed. */
  legacyFallback: boolean;
};

/** The engine new chat work starts on: sessions, note chat, Home handoffs,
 * and companion sessions. Needs no network. */
export async function chatDefaultEngine(): Promise<ChatDefaultEngine> {
  const engine = await invoke<Partial<ChatDefaultEngine> | null | undefined>("chat_default_engine");
  const modelId =
    typeof engine?.modelId === "string" && engine.modelId.startsWith(CLI_ENGINE_MODEL_PREFIX)
      ? engine.modelId
      : null;
  return {
    modelId,
    cli: modelId ? modelId.slice(CLI_ENGINE_MODEL_PREFIX.length) : null,
    legacyFallback: engine?.legacyFallback === true,
  };
}

/** The model id new chat work starts on: the default CLI engine when chat is
 * on one, else `fallback` (also when the engine cannot be read). */
export async function defaultChatModelId(fallback: string): Promise<string> {
  try {
    return (await chatDefaultEngine()).modelId ?? fallback;
  } catch {
    return fallback;
  }
}

/** The CLI the engine picker marks as the default: Claude Code when it is
 * installed, else Codex when it is, else Claude Code. */
export function defaultChatCliId(
  clis: ReadonlyArray<Pick<ChatEngineCliStatus, "id" | "installed">>,
): string {
  return (
    DEFAULT_CHAT_CLI_IDS.find((id) => clis.some((cli) => cli.id === id && cli.installed)) ??
    DEFAULT_CHAT_CLI_IDS[0]
  );
}
