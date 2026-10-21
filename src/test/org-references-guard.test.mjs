import { describe, expect, it } from "vitest";
import { findOrgReferences, ORG_REFERENCE_SCOPES } from "../../scripts/check-org-references.mjs";

describe("organization reference guard", () => {
  it("flags each organization reference the guard exists for", () => {
    const [{ pattern }] = ORG_REFERENCE_SCOPES;
    for (const sample of [
      "https://app.opensoftware.co/api",
      "https://june.link/s/shr_1",
      "ghcr.io/open-software-network/clovy-api",
      "runs-on: blacksmith-4vcpu-ubuntu-2404",
    ]) {
      expect(sample).toMatch(pattern);
    }
    expect("co.opensoftware.june").not.toMatch(pattern);
  });

  it("finds no organization references in the repository, CI and app source", () => {
    expect(findOrgReferences()).toEqual([]);
  });
});
