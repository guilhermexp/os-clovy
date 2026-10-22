import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { buildReleaseManifest, readReleaseAssets } from "../../scripts/release-manifest.mjs";

const MAC_SIG = "bWFjIHNpZ25hdHVyZQ==";
const WIN_SIG = "d2luZG93cyBzaWduYXR1cmU=";

function artifactDir() {
  const dir = mkdtempSync(join(tmpdir(), "clovy-release-"));
  writeFileSync(join(dir, "Clovy_universal.dmg"), "dmg");
  writeFileSync(join(dir, "Clovy.app.tar.gz"), "tar");
  writeFileSync(join(dir, "Clovy.app.tar.gz.sig"), `${MAC_SIG}\n`);
  writeFileSync(join(dir, "Clovy_0.1.0_x64-setup.exe"), "exe");
  writeFileSync(join(dir, "Clovy_0.1.0_x64-setup.exe.sig"), WIN_SIG);
  return dir;
}

describe("release manifest", () => {
  it("lists the three platforms with this release's URLs and .sig contents", () => {
    const assets = readReleaseAssets(artifactDir());
    const manifest = buildReleaseManifest({
      tag: "v0.1.0",
      repository: "guilhermexp/os-clovy",
      pubDate: "2026-10-22T12:00:00Z",
      assets,
    });

    const base = "https://github.com/guilhermexp/os-clovy/releases/download/v0.1.0";
    const mac = { signature: MAC_SIG, url: `${base}/Clovy.app.tar.gz` };
    expect(manifest).toEqual({
      version: "0.1.0",
      pub_date: "2026-10-22T12:00:00Z",
      platforms: {
        "darwin-aarch64": mac,
        "darwin-x86_64": mac,
        "windows-x86_64": {
          signature: WIN_SIG,
          url: `${base}/Clovy_0.1.0_x64-setup.exe`,
        },
      },
    });
  });

  it("names the six files the release publishes", () => {
    const assets = readReleaseAssets(artifactDir());
    expect(assets.files.map((file) => file.split("/").pop()).sort()).toEqual([
      "Clovy.app.tar.gz",
      "Clovy.app.tar.gz.sig",
      "Clovy_0.1.0_x64-setup.exe",
      "Clovy_0.1.0_x64-setup.exe.sig",
      "Clovy_universal.dmg",
    ]);
  });

  it("fails when a package or its signature is missing", () => {
    const dir = mkdtempSync(join(tmpdir(), "clovy-release-"));
    writeFileSync(join(dir, "Clovy.app.tar.gz"), "tar");
    expect(() => readReleaseAssets(dir)).toThrow(/Clovy\.app\.tar\.gz\.sig/);
  });
});
