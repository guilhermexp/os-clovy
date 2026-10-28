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
    const [
      packageJson,
      extensionPackage,
      desktopCargo,
      windowsHelperCargo,
      legacyCryptoCargo,
      legacyProtocolCargo,
    ] = await Promise.all([
      read("package.json").then(JSON.parse),
      read("extension/package.json").then(JSON.parse),
      read("src-tauri/Cargo.toml"),
      read("src-tauri/native/windows-dictation-helper/Cargo.toml"),
      read("crates/june-companion-crypto/Cargo.toml"),
      read("crates/june-companion-protocol/Cargo.toml"),
    ]);

    expect(packageJson.name).toBe("clovy");
    expect(extensionPackage.name).toBe("clovy-extension");
    expect(desktopCargo).toMatch(/^name = "clovy"$/m);
    expect(desktopCargo).toMatch(/^name = "clovy_lib"$/m);
    expect(desktopCargo).toMatch(/^name = "clovy-nm-shim"$/m);
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

  it("uses Clovy for generated build metadata and local build coordination", async () => {
    const [prepareDriver, selfTest, desktopBuild, windowsBuild, windowsQualification, sbom] =
      await Promise.all([
        read("scripts/prepare-cua-driver.mjs"),
        read("scripts/computer-use-self-test.mjs"),
        read("src-tauri/build.rs"),
        read("scripts/build-windows.ps1"),
        read("scripts/qualify-windows-build.ps1"),
        read("src-tauri/cua-driver-sbom.spdx.json").then(JSON.parse),
      ]);

    for (const source of [prepareDriver, selfTest, desktopBuild]) {
      expect(source).toContain("clovy-cua-driver-pin.json");
      expect(source).not.toContain("june-cua-driver-pin.json");
    }
    for (const source of [prepareDriver, selfTest]) {
      expect(source).toContain("clovy-cua-driver.spdx.json");
      expect(source).toContain("clovyBuild");
      expect(source).not.toContain("juneBuild");
    }
    expect(windowsBuild).toContain("ClovyWindowsBuild");
    expect(windowsQualification).toContain("ClovyWindowsBuild");
    expect(windowsQualification).toContain("firstPathRestored");
    expect(sbom.name).toBe("clovy-cua-driver-0.5.0");
  });

  it("uses canonical Clovy names for private runtime and Windows helper concepts", async () => {
    const [
      helperProtocol,
      dictation,
      computerUse,
      computerUseDriver,
      computerUseCursor,
      computerUseSelfTest,
      systemWindows,
      managedBrowser,
      agentNotifications,
      shutdown,
      soundsDemo,
      releaseAge,
    ] = await Promise.all([
      read("src-tauri/native/windows-dictation-helper/src/protocol.rs"),
      read("src-tauri/src/dictation.rs"),
      read("src-tauri/src/computer_use.rs"),
      read("src-tauri/src/computer_use_driver.rs"),
      read("src-tauri/src/computer_use_cursor.rs"),
      read("scripts/computer-use-self-test.mjs"),
      read("src-tauri/src/audio/system_windows.rs"),
      read("src-tauri/src/browser/managed.rs"),
      read("src/lib/agent-notifications.ts"),
      read("src-tauri/src/shutdown.rs"),
      read("src/lib/clovy-sounds-demo.ts"),
      read("scripts/check-pnpm-release-age.py"),
    ]);

    expect(helperProtocol).toContain("pub clovy_process_id");
    expect(helperProtocol).toContain("pub clovy_window_handle");
    expect(helperProtocol).toContain('rename = "juneProcessId"');
    expect(helperProtocol).toContain('rename = "juneWindowHandle"');
    for (const field of [
      '"clovyProcessId"',
      '"clovyWindowHandle"',
      '"juneProcessId"',
      '"juneWindowHandle"',
    ]) {
      expect(dictation).toContain(field);
    }
    expect(computerUse).toContain('format!("clovy-cua-{}", random_id())');
    expect(computerUse).not.toContain('format!("june-cua-{}", random_id())');
    for (const source of [computerUse, computerUseDriver]) {
      expect(source).toContain("clovyComputerUseCapability");
      expect(source).toContain("juneComputerUseCapability");
    }
    expect(computerUseSelfTest).toContain("dev.guilhermexp.clovy.computer-use-self-test.${role}");
    expect(computerUseDriver).toContain("clovy-helper-child-reaper");
    expect(computerUseDriver).not.toContain("june-helper-child-reaper");
    for (const source of [computerUseDriver, computerUseCursor]) {
      expect(source).toContain('"clovy/pointer"');
      expect(source).not.toContain('"june/pointer"');
    }
    expect(systemWindows).toContain("clovy-windows-system-audio");
    expect(systemWindows).not.toContain("june-windows-system-audio");
    for (const suffix of ["Epoch", "Observer", "State"]) {
      expect(managedBrowser).toContain(`__clovySnapshot${suffix}`);
      expect(managedBrowser).not.toContain(`__juneSnapshot${suffix}`);
    }
    expect(managedBrowser).toContain("__clovyMutationVersion");
    expect(managedBrowser).not.toContain("__juneMutationVersion");
    expect(agentNotifications).toContain("__clovyAgentNotificationTimes");
    expect(agentNotifications).not.toContain("__juneAgentNotificationTimes");
    expect(shutdown).toContain('name("clovy-shutdown-supervisor".to_string())');
    expect(shutdown).toContain('name("clovy-child-reaper".to_string())');
    expect(shutdown).not.toContain('name("june-shutdown-');
    expect(shutdown).not.toContain('name("june-child-reaper"');
    expect(soundsDemo).toContain("__clovySounds");
    expect(soundsDemo).not.toContain("__juneSounds");
    expect(releaseAge).toContain("clovy-pnpm-release-age-check");
    expect(releaseAge).not.toContain("os-june-pnpm-release-age-check");
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

  it("uses Clovy for newly written runtime state and private source concepts", async () => {
    const [
      notifications,
      routines,
      computerUseDriver,
      meetingDetection,
      obsidian,
      httpClient,
      driverContract,
      noteDrag,
    ] = await Promise.all([
      read("src-tauri/src/notifications.rs"),
      read("src-tauri/src/routines.rs"),
      read("src-tauri/src/computer_use_driver.rs"),
      read("src-tauri/src/meeting_detection.rs"),
      read("src-tauri/src/obsidian.rs"),
      read("src-tauri/src/http_client.rs"),
      read("src-tauri/cua-driver-contract.json").then(JSON.parse),
      read("src/lib/dnd.ts"),
    ]);

    expect(notifications).toContain('const SESSION_ID_KEY: &str = "clovyAgentSessionId"');
    expect(notifications).toContain('const LEGACY_SESSION_ID_KEY: &str = "juneAgentSessionId"');
    expect(routines).toContain('format!("clovy_browser_routine_{}"');
    expect(routines).not.toContain('format!("june_browser_routine_{}"');
    expect(computerUseDriver).toContain("fn parent_is_clovy()");
    expect(computerUseDriver).toContain("fn process_is_clovy(");
    expect(computerUseDriver).not.toContain("fn parent_is_june()");
    expect(computerUseDriver).not.toContain("fn process_is_june(");
    expect(meetingDetection).toContain("clovy_capture_active: bool");
    expect(meetingDetection).not.toContain("os_june_capture_active: bool");
    expect(obsidian).toContain(".clovy-obsidian-write-probe-");
    expect(obsidian).not.toContain(".june-obsidian-write-probe-");
    // Provider requests take their user agent from the shared HTTP builder.
    expect(httpClient).toContain('pub const USER_AGENT: &str = concat!("clovy/"');
    expect(httpClient).toContain(".user_agent(USER_AGENT)");
    expect(httpClient).not.toContain('concat!("os-june/"');
    expect(driverContract.clovyActions).toBeInstanceOf(Array);
    expect(driverContract).not.toHaveProperty("juneActions");
    expect(noteDrag).toContain('NOTE_DND_MIME = "application/x-clovy-note-id"');
    expect(noteDrag).toContain('LEGACY_NOTE_DND_MIME = "application/x-june-note-id"');
  });

  it("publishes the Obsidian skill under Clovy with a rollback-safe June alias", async () => {
    const [bundledSkill, skillIdentity, agentApi] = await Promise.all([
      read("src-tauri/resources/agent-skills/clovy-obsidian/SKILL.md"),
      read("src-tauri/src/agent_sessions/skill_identity.rs"),
      read("src-tauri/src/agent_sessions/api.rs"),
    ]);

    expect(bundledSkill).toMatch(/^name: clovy-obsidian$/m);
    expect(skillIdentity).toContain('pub const CLOVY_OBSIDIAN_SKILL_ID: &str = "clovy-obsidian"');
    expect(skillIdentity).toContain('pub const LEGACY_OBSIDIAN_SKILL_ID: &str = "june-obsidian"');
    expect(skillIdentity).toContain("write_skill_ids");
    expect(agentApi).toContain("write_skill_ids(skill_id)");
    expect(agentApi).toContain("read_skill_ids(&canonical_id)");
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
    expect(styleguide).toContain("else os-clovy:theme / system");
  });
});
