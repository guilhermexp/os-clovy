import { type MessageKey, t } from "../i18n/translate";
import type { ProviderModelDto } from "./tauri";

// Clovy's default video model. Mirrors DEFAULT_VIDEO_MODEL in the Rust providers
// module (src-tauri/src/providers/mod.rs). Keep the two in sync.
export const DEFAULT_VIDEO_MODEL = "fal-ai/wan/v2.2-a14b/text-to-video";

type VideoModelDefinition = Pick<ProviderModelDto, "id" | "name"> & {
  /** Catalog key for the description, read in the interface language. */
  descriptionKey: MessageKey;
};

function videoModel({ descriptionKey, ...model }: VideoModelDefinition): ProviderModelDto {
  const entry: ProviderModelDto = {
    provider: "fal",
    modelType: "video",
    capabilities: [],
    traits: [],
    ...model,
  };
  Object.defineProperty(entry, "description", {
    enumerable: true,
    configurable: true,
    get: () => t(descriptionKey),
  });
  return entry;
}

// Curated fal text-to-video endpoints for the settings picker. Video models are
// not part of the text/ASR catalog Clovy API serves, so the picker uses this
// local snapshot instead of fetching (same pattern as IMAGE_MODELS). Two hard
// constraints govern any entry; a model that breaks them fails at generation
// time, not here:
//   1. It must be priced in clovy-api's `video_pricing` map (ADR 0015): that map
//      doubles as the allowlist, and an unlisted model is rejected
//      `model_not_priced`. Keep these ids in sync with that map AND with
//      `KNOWN_VIDEO_MODELS` in src-tauri/src/providers/mod.rs.
//   2. It must accept the fixed fast-path shape Clovy injects (5s / 720p / 16:9;
//      see the DEFAULT_VIDEO_* constants in src-tauri/src/providers/mod.rs).
//      clovy-api's fal adapter validates each model's ranges before charging.
// Image-to-video (animation) uses the matching fal endpoints chosen by Clovy
// API, not this list. The entries carry no privacy label: fal is a third-party
// processor and Clovy makes no retention claim for it.
export const VIDEO_MODELS: ProviderModelDto[] = [
  videoModel({
    id: DEFAULT_VIDEO_MODEL,
    name: "Wan 2.2 A14B",
    descriptionKey: "lib.videoModels.wan22",
  }),
  videoModel({
    id: "alibaba/wan-3.0/text-to-video",
    name: "Wan 3.0",
    descriptionKey: "lib.videoModels.wan30",
  }),
];
