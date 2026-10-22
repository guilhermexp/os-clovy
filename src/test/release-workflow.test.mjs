import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

// actionlint checks the syntax; these pin the release contract (ADR-0069).
const workflow = readFileSync(".github/workflows/release.yml", "utf8");

function job(name) {
  const start = workflow.indexOf(`\n  ${name}:\n`);
  expect(start).toBeGreaterThan(-1);
  const rest = workflow.slice(start + 1);
  const next = rest.slice(1).search(/\n {2}[a-z]+:\n/);
  return next === -1 ? rest : rest.slice(0, next + 1);
}

describe("release workflow", () => {
  it("runs on vX.Y.Z tags and on a manual run that defaults to a dry run", () => {
    expect(workflow).toContain('tags: ["v*.*.*"]');
    expect(workflow).toMatch(/dry_run:\n(?: {8}.*\n)*? {8}default: true/);
  });

  it("checks the version before any build", () => {
    expect(job("macos")).toContain("needs: version");
    expect(job("windows")).toContain("needs: version");
    expect(job("version")).toContain("scripts/check-release-version.mjs");
  });

  it("publishes only after both builds, and only outside a dry run", () => {
    const publish = job("publish");
    expect(publish).toContain("needs: [version, macos, windows]");
    expect(publish).toContain("scripts/release-manifest.mjs");
    expect(publish).toContain("if: env.DRY_RUN != 'true'");
    expect(publish).toContain("gh release create");
  });

  it("grants write access only to the publish job", () => {
    expect(workflow).toMatch(/^permissions:\n {2}contents: read$/m);
    expect(workflow.match(/contents: write/g)).toHaveLength(1);
    expect(job("publish")).toContain("contents: write");
  });

  it("pins every action to a commit", () => {
    const uses = [...workflow.matchAll(/uses: (\S+)/g)].map((match) => match[1]);
    for (const action of uses.filter((name) => !name.startsWith("./"))) {
      expect(action).toMatch(/@[0-9a-f]{40}$/);
    }
  });
});
