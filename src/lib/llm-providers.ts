import { invoke } from "./tauri";

export type StructuredOutputLevel = "none" | "prompt" | "json_object" | "json_schema" | "strict";
export type LlmCliId = "claude" | "codex" | "pi" | "agy" | "cursor-agent" | "copilot";
export type LlmUsage = "chat" | "notes" | "dictationCleanup" | "activity";

export type LlmProviderRef =
  | { kind: "clovy" }
  | { kind: "none" }
  | { kind: "endpoint"; id: string }
  | { kind: "cli"; id: LlmCliId };

export type LlmCliStatusDto = {
  id: LlmCliId;
  name: string;
  installed: boolean;
  path?: string;
  version?: string;
  reason?: string;
  structuredOutput: StructuredOutputLevel;
  /** Runs one-shot calls with every tool off; only these take notes, dictation cleanup, and activity. */
  toolsDisabled: boolean;
};

export type LlmEndpointDto = {
  id: string;
  name: string;
  baseUrl: string;
  modelId: string;
  hasApiKey: boolean;
  structuredOutput?: StructuredOutputLevel;
};

export type LlmUsageSelection = {
  chat: LlmProviderRef;
  notes: LlmProviderRef;
  dictationCleanup: LlmProviderRef;
  activity: LlmProviderRef;
};

export type LlmProvidersDto = {
  endpoints: LlmEndpointDto[];
  usage: LlmUsageSelection;
  cliLevels: Partial<Record<LlmCliId, StructuredOutputLevel>>;
};

export type LlmProviderTestResultDto = {
  latencyMs: number;
  structuredOutput: StructuredOutputLevel;
};

export type LlmSaveEndpointRequest = {
  id?: string;
  name: string;
  baseUrl: string;
  modelId: string;
  apiKey?: string;
  clearApiKey?: boolean;
};

export function llmProviders(): Promise<LlmProvidersDto> {
  return invoke<LlmProvidersDto>("llm_providers");
}

export function llmDetectClis(): Promise<LlmCliStatusDto[]> {
  return invoke<LlmCliStatusDto[]>("llm_detect_clis");
}

export function llmSaveEndpoint(request: LlmSaveEndpointRequest): Promise<LlmProvidersDto> {
  return invoke<LlmProvidersDto>("llm_save_endpoint", { request });
}

export function llmDeleteEndpoint(id: string): Promise<LlmProvidersDto> {
  return invoke<LlmProvidersDto>("llm_delete_endpoint", { id });
}

export function llmTestProvider(provider: LlmProviderRef): Promise<LlmProviderTestResultDto> {
  return invoke<LlmProviderTestResultDto>("llm_test_provider", { provider });
}

export function llmSetUsage(usage: LlmUsage, provider: LlmProviderRef): Promise<LlmProvidersDto> {
  return invoke<LlmProvidersDto>("llm_set_usage", { usage, provider });
}

export function isActivityEligible(level?: StructuredOutputLevel): boolean {
  return level === "json_schema" || level === "strict";
}

export function providerRefKey(ref: LlmProviderRef): string {
  switch (ref.kind) {
    case "clovy":
      return "clovy";
    case "none":
      return "none";
    case "endpoint":
      return `endpoint:${ref.id}`;
    case "cli":
      return `cli:${ref.id}`;
  }
}

export function parseProviderRefKey(key: string): LlmProviderRef {
  if (key === "clovy") {
    return { kind: "clovy" };
  }
  if (key === "none") {
    return { kind: "none" };
  }
  if (key.startsWith("endpoint:")) {
    return { kind: "endpoint", id: key.slice("endpoint:".length) };
  }
  if (key.startsWith("cli:")) {
    return { kind: "cli", id: key.slice("cli:".length) as LlmCliId };
  }
  return { kind: "none" };
}
