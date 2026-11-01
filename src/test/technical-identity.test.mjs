import { existsSync } from "node:fs";
import { readFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const root = join(dirname(fileURLToPath(import.meta.url)), "../..");

async function read(relativePath) {
  return readFile(join(root, relativePath), "utf8");
}

describe("Clovy technical identity", () => {
  it("uses Clovy for canonical package and helper names", async () => {
    const [packageJson, desktopCargo, windowsHelperCargo, legacyCryptoCargo, legacyProtocolCargo] =
      await Promise.all([
        read("package.json").then(JSON.parse),
        read("src-tauri/Cargo.toml"),
        read("src-tauri/native/windows-dictation-helper/Cargo.toml"),
        read("crates/june-companion-crypto/Cargo.toml"),
        read("crates/june-companion-protocol/Cargo.toml"),
      ]);

    expect(packageJson.name).toBe("clovy");
    expect(desktopCargo).toMatch(/^name = "clovy"$/m);
    expect(desktopCargo).toMatch(/^name = "clovy_lib"$/m);
    expect(windowsHelperCargo).toMatch(/^name = "clovy-windows-dictation-helper"$/m);
    expect(windowsHelperCargo).toMatch(/^name = "clovy-dictation-helper"$/m);
    expect(legacyCryptoCargo).toContain('name = "june-companion-crypto"');
    expect(legacyCryptoCargo).toContain(
      'clovy-companion-crypto = { path = "../clovy-companion-crypto" }',
    );
    expect(legacyProtocolCargo).toContain('name = "june-companion-protocol"');
    expect(legacyProtocolCargo).toContain(
      'clovy-companion-protocol = { path = "../clovy-companion-protocol" }',
    );
  });

  it("removes the backend workspace, workflow and build targets", async () => {
    for (const relativePath of [
      "clovy-api/Cargo.toml",
      "clovy-api/Cargo.lock",
      "clovy-api/config.toml",
      "agent-runtime/package.json",
      ".github/workflows/clovy-api.yml",
    ]) {
      expect(existsSync(join(root, relativePath)), relativePath).toBe(false);
    }
    const [packageJson, makefile] = await Promise.all([
      read("package.json").then(JSON.parse),
      read("Makefile"),
    ]);
    for (const [name, command] of Object.entries(packageJson.scripts)) {
      expect(`${name} ${command}`).not.toMatch(/clovy-api|agent-runtime/);
    }
    const retiredTargets = [
      ...makefile.matchAll(/^(dev-api|clovy-api-[\w-]+|agent-runtime[\w-]*):/gm),
    ]
      .map((match) => match[1])
      .filter((target) => target !== "clovy-api-references-check");
    expect(retiredTargets).toEqual([]);
    expect(makefile).not.toMatch(/(?:cd\s+|--manifest-path\s+)clovy-api(?:\/|\s)/);
    const setupRust = ".github/actions/setup-rust/action.yml";
    if (existsSync(join(root, setupRust))) {
      expect(await read(setupRust)).not.toContain("clovy-api");
    }
  });

  it("includes the retired API reference guard in the full verification gate", async () => {
    const makefile = await read("Makefile");
    const verify = makefile.match(/^verify:\s*([^\n]+)/m)?.[1] ?? "";
    expect(verify.split(/\s+/)).toContain("clovy-api-references-check");
    expect(makefile).toMatch(
      /^clovy-api-references-check:[^\n]*\n\t[^\n]*node scripts\/check-clovy-api-references\.mjs/m,
    );
    expect(verify).not.toMatch(/clovy-api-(?:fmt|lint|test)/);
  });

  it("starts development without backend processes, URL injection or a local bearer", async () => {
    const forbiddenNames = [
      ["CLOVY", "API", "URL"].join("_"),
      ["clovy", "api", "url"].join("_"),
      ["OS", "CLOVY", "LOCAL", "DEV", "BEARER", "TOKEN"].join("_"),
      ["local", "dev", "bearer", "token"].join("_"),
    ];
    for (const file of ["scripts/tauri-dev.mjs", "scripts/tauri-before-dev.mjs"]) {
      const source = await read(file);
      expect(source, file).not.toMatch(
        /clovy-api|CLOVY_API_PORT|CLOVY__SERVER__PORT|CLOVY_DEV_SKIP_LOCAL_API/,
      );
      for (const name of forbiddenNames) expect(source, file).not.toContain(name);
    }
  });

  it("preserves released C ABI contracts while publishing canonical aliases", async () => {
    const [
      canonicalHeader,
      canonicalCompatibilityHeader,
      oldPathCompatibilityHeader,
      cryptoSource,
    ] = await Promise.all([
      read("crates/clovy-companion-crypto/include/clovy_companion_crypto.h"),
      read("crates/clovy-companion-crypto/include/june_companion_crypto.h"),
      read("crates/june-companion-crypto/include/june_companion_crypto.h"),
      read("crates/clovy-companion-crypto/src/lib.rs"),
    ]);

    expect(canonicalHeader).toContain("clovy_crypto_generate_identity");
    expect(canonicalCompatibilityHeader).toContain("june_crypto_generate_identity");
    expect(oldPathCompatibilityHeader).toContain("june_crypto_generate_identity");
    expect(oldPathCompatibilityHeader).toBe(canonicalCompatibilityHeader);
    expect(cryptoSource).toContain('pub unsafe extern "C" fn clovy_crypto_generate_identity');
    expect(cryptoSource).toContain('pub unsafe extern "C" fn june_crypto_generate_identity');
  });

  it("ships the Obsidian skill with canonical Clovy metadata", async () => {
    const bundledSkill = await read("src-tauri/resources/agent-skills/clovy-obsidian/SKILL.md");
    expect(bundledSkill).toMatch(/^name: clovy-obsidian$/m);
    expect(bundledSkill).not.toMatch(/^name: june-obsidian$/m);
    // Alias read/write behavior and conflicting stored settings are exercised
    // by the production Rust identity and skill-admin regression suites.
  });

  it("uses Clovy for ephemeral build and dictation artifacts", async () => {
    const [
      signedDmg,
      tauri,
      macHelper,
      windowsHelper,
      dictation,
      computerUseFixture,
      index,
      onboardingPreview,
      styleguide,
    ] = await Promise.all([
      read("scripts/build-macos-release.sh"),
      read("src-tauri/tauri.conf.json").then(JSON.parse),
      read("src-tauri/native/mac-dictation-helper/main.swift"),
      read("src-tauri/native/windows-dictation-helper/src/audio.rs"),
      read("src-tauri/src/dictation.rs"),
      read("src-tauri/native/computer-use-fixture/main.swift"),
      read("index.html"),
      read("onboarding-preview.html"),
      read("styleguide.html"),
    ]);

    expect(signedDmg).toContain("clovy-signing.keychain-db");
    expect(signedDmg).not.toContain("os-june-signing.keychain-db");
    expect(tauri.app.security.assetProtocol.scope).toEqual([
      "$TEMP/clovy-dictation-*.m4a",
      "$TEMP/clovy-dictation-*.wav",
    ]);
    expect(macHelper).toContain('appendingPathComponent("clovy-dictation-');
    expect(windowsHelper).toContain('.prefix("clovy-dictation-")');
    expect(dictation).toContain('format!("clovy-dictation-{utterance_id}-normalized.wav")');
    expect(computerUseFixture).toContain('"clovy-cu-observer-window"');
    expect(computerUseFixture).not.toContain('"june-cu-observer-window"');
    expect(index).toContain('localStorage.getItem("os-clovy:theme")');
    expect(index).not.toContain("os-june:");
    expect(onboardingPreview).toContain("clovy.replayOnboarding()");
    expect(onboardingPreview).toContain("window.__CLOVY_STUB__");
    expect(styleguide).toContain("else os-clovy:theme / dark");
  });
});
