import { t } from "../i18n/translate";
import { agentModelSelection, agentRunModelId, AUTO_MODEL_ID } from "./agent-model-selection";
import {
  CLI_ENGINE_MODEL_PREFIX,
  type ChatEngineCliStatus,
  chatEngineCatalog,
  defaultChatCliId,
} from "./chat-engine";
import { modelAvailableForMode, modelMissingProviderKey, modelPrivacyBadge } from "./model-privacy";
import { pricingLabel } from "./model-pricing";
import { suggestedModelsForMode } from "./suggested-models";
import type {
  CompanionModelOption,
  CompanionSessionModelSelection,
  ProviderApiKeysConfigured,
  ProviderModelDto,
} from "./tauri";

const AUTO_MODEL: CompanionModelOption = {
  id: AUTO_MODEL_ID,
  name: "Auto",
  provider: "",
  get description() {
    return t("lib.companion.autoDescription");
  },
  routing: "automatic",
};

/** The installed agent CLIs first (the default one, Claude Code or else
 * Codex, at the top; ADR-0062), then the legacy Clovy agent: Auto plus the
 * curated generation models that can run right now. A row that needs the
 * user's own provider key is left out until that key is saved: the companion
 * cannot save keys, so it could only fail. */
export function companionModelOptions(
  models: ProviderModelDto[],
  providerKeys: ProviderApiKeysConfigured,
  clis: ReadonlyArray<ChatEngineCliStatus> = [],
): CompanionModelOption[] {
  const available = models.filter(
    (model) =>
      modelAvailableForMode("generation", model) && !modelMissingProviderKey(model, providerKeys),
  );
  const curated = suggestedModelsForMode("generation", available).flatMap(
    ({ model, reason }): CompanionModelOption[] => {
      const id = model.id.trim();
      if (!id || utf8Length(id) > 256) return [];
      const privacy = modelPrivacyBadge(model);
      const price = pricingLabel(model);
      return [
        {
          id,
          name: boundedText(model.name.trim() || id, 256),
          provider: boundedText(model.provider.trim(), 64),
          description: boundedText(reason, 512),
          routing: "remote",
          ...(privacy
            ? {
                privacy:
                  privacy.mode === "e2ee"
                    ? ("endToEndEncrypted" as const)
                    : (privacy.mode as "private" | "anonymous"),
                privacyLabel: boundedText(privacy.label, 128),
              }
            : {}),
          ...(price ? { priceLabel: boundedText(price, 128) } : {}),
        },
      ];
    },
  );
  return [...companionEngineOptions(clis), AUTO_MODEL, ...curated];
}

/** The agent CLIs the phone can pick; none when the catalog cannot be read,
 * so the legacy models stay available. */
export async function companionEngineClis(): Promise<ChatEngineCliStatus[]> {
  try {
    return (await chatEngineCatalog()).clis;
  } catch {
    return [];
  }
}

function companionEngineOptions(clis: ReadonlyArray<ChatEngineCliStatus>): CompanionModelOption[] {
  const defaultId = defaultChatCliId(clis);
  return clis
    .filter((cli) => cli.installed && cli.modelId.startsWith(CLI_ENGINE_MODEL_PREFIX))
    .sort((left, right) => Number(right.id === defaultId) - Number(left.id === defaultId))
    .map((cli) => ({
      id: cli.modelId,
      name: boundedText(cli.name, 256),
      provider: "",
      description: boundedText(
        t(
          cli.id === defaultId
            ? "lib.companion.cliEngineDefaultDescription"
            : "lib.companion.cliEngineDescription",
          { name: cli.name },
        ),
        512,
      ),
      // The wire only knows automatic and remote routing; a CLI engine is a
      // fixed choice, not automatic routing.
      routing: "remote" as const,
    }));
}

export function companionSessionModelSelection(
  storedSessionId: string,
  storedModel: string,
  models: ProviderModelDto[],
  defaultCostQuality: number,
  clis: ReadonlyArray<ChatEngineCliStatus> = [],
): CompanionSessionModelSelection {
  if (storedModel.startsWith(CLI_ENGINE_MODEL_PREFIX)) {
    const cliId = storedModel.slice(CLI_ENGINE_MODEL_PREFIX.length);
    const name = clis.find((cli) => cli.modelId === storedModel)?.name.trim() || cliId;
    return { storedSessionId, modelId: storedModel, modelName: boundedText(name, 256) };
  }
  const decoded = agentModelSelection(storedModel);
  const modelId =
    decoded.modelId === "auto" || !decoded.modelId.trim() ? AUTO_MODEL_ID : decoded.modelId;
  const modelName =
    modelId === AUTO_MODEL_ID
      ? "Auto"
      : (models.find((model) => model.id === modelId)?.name.trim() ?? "") || modelId;
  return {
    storedSessionId,
    modelId,
    modelName: boundedText(modelName, 256),
    ...(modelId === AUTO_MODEL_ID
      ? { costQuality: decoded.costQuality ?? normalizeCostQuality(defaultCostQuality) }
      : {}),
  };
}

export function companionStoredModelId(modelId: string, defaultCostQuality: number) {
  return modelId === AUTO_MODEL_ID
    ? agentRunModelId(AUTO_MODEL_ID, normalizeCostQuality(defaultCostQuality))
    : modelId;
}

function normalizeCostQuality(value: number) {
  return Math.max(0, Math.min(100, Math.round(value)));
}

function boundedText(value: string, maxBytes: number) {
  if (utf8Length(value) <= maxBytes) return value;
  const suffix = "...";
  const pieces: string[] = [];
  let bytes = 0;
  for (const character of value) {
    const characterBytes = utf8Length(character);
    if (bytes + characterBytes + suffix.length > maxBytes) break;
    pieces.push(character);
    bytes += characterBytes;
  }
  return `${pieces.join("").trimEnd()}${suffix}`;
}

function utf8Length(value: string) {
  return new TextEncoder().encode(value).byteLength;
}
