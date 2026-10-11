import type { ProviderModelDto } from "./tauri";

/** Keep curated identity/copy; only the current API can attest service funding.
 * Missing rows/fields (including an older API or failed fetch) stay unknown. */
export function mediaModelCatalog(
  curated: ProviderModelDto[],
  remote: ProviderModelDto[],
): ProviderModelDto[] {
  return curated.map((model) => {
    const live = remote.find(
      (row) =>
        row.id === model.id && row.provider === model.provider && row.modelType === model.modelType,
    );
    return { ...model, serviceAvailable: live?.serviceAvailable };
  });
}
