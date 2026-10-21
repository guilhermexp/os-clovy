import { readFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const root = join(dirname(fileURLToPath(import.meta.url)), "../..");

async function read(relativePath) {
  return readFile(join(root, relativePath), "utf8");
}

function workflowTriggerPaths(source, trigger) {
  const start = source.indexOf(`  ${trigger}:\n`);
  expect(start).toBeGreaterThan(-1);
  const remainder = source.slice(start + trigger.length + 4);
  const nextTrigger = remainder.search(
    /\n(?: {2}(?:pull_request|push|workflow_dispatch):|[A-Za-z][\w-]*:)(?:\n|$)/,
  );
  const block = nextTrigger === -1 ? remainder : remainder.slice(0, nextTrigger);
  return [...block.matchAll(/^\s{6}- "([^"]+)"$/gm)].map((match) => match[1]);
}

describe("Clovy technical identity", () => {
  it("uses Clovy for canonical package, service, and helper names", async () => {
    const [
      packageJson,
      agentPackage,
      extensionPackage,
      desktopCargo,
      windowsHelperCargo,
      apiCargo,
      apiAppCargo,
      legacyCryptoCargo,
      legacyProtocolCargo,
    ] = await Promise.all([
      read("package.json").then(JSON.parse),
      read("agent-runtime/package.json").then(JSON.parse),
      read("extension/package.json").then(JSON.parse),
      read("src-tauri/Cargo.toml"),
      read("src-tauri/native/windows-dictation-helper/Cargo.toml"),
      read("clovy-api/Cargo.toml"),
      read("clovy-api/crates/app/Cargo.toml"),
      read("crates/june-companion-crypto/Cargo.toml"),
      read("crates/june-companion-protocol/Cargo.toml"),
    ]);

    expect(packageJson.name).toBe("clovy");
    expect(agentPackage.name).toBe("@clovy/agent-runtime");
    expect(agentPackage.bin).toEqual({ "clovy-agent-runtime": "./dist/main.js" });
    expect(extensionPackage.name).toBe("clovy-extension");
    expect(desktopCargo).toMatch(/^name = "clovy"$/m);
    expect(desktopCargo).toMatch(/^name = "clovy_lib"$/m);
    expect(desktopCargo).toMatch(/^name = "clovy-nm-shim"$/m);
    expect(windowsHelperCargo).toMatch(/^name = "clovy-windows-dictation-helper"$/m);
    expect(windowsHelperCargo).toMatch(/^name = "june-dictation-helper"$/m);
    expect(apiCargo).toContain('members = ["crates/*"]');
    expect(apiCargo).toContain('clovy-api = { path = "crates/api" }');
    expect(apiAppCargo).toMatch(/^name = "clovy-api-server"$/m);
    expect(apiAppCargo).toMatch(/^name = "clovy-api"$/m);
    expect(legacyCryptoCargo).toContain('name = "june-companion-crypto"');
    expect(legacyCryptoCargo).toContain(
      'clovy-companion-crypto = { path = "../clovy-companion-crypto" }',
    );
    expect(legacyProtocolCargo).toContain('name = "june-companion-protocol"');
    expect(legacyProtocolCargo).toContain(
      'clovy-companion-protocol = { path = "../clovy-companion-protocol" }',
    );
  });

  it("retains installed desktop and updater identities", async () => {
    const [
      tauri,
      macos,
      macosInfoPlist,
      macosLocalizedInfoPlist,
      windows,
      desktopCargo,
      bundleShim,
    ] = await Promise.all([
      read("src-tauri/tauri.conf.json").then(JSON.parse),
      read("src-tauri/tauri.macos.conf.json").then(JSON.parse),
      read("src-tauri/Info.plist"),
      read("src-tauri/resources/macos/en.lproj/InfoPlist.strings"),
      read("src-tauri/tauri.windows.conf.json").then(JSON.parse),
      read("src-tauri/Cargo.toml"),
      read("scripts/bundle-nm-shim.sh"),
    ]);

    expect(tauri.productName).toBe("Clovy");
    expect(tauri.identifier).toBe("co.opensoftware.june");
    expect(tauri.plugins["deep-link"].desktop.schemes).toEqual(["clovy", "osjune"]);
    expect(tauri.plugins.updater.endpoints).toEqual([
      "https://github.com/open-software-network/os-june-releases/releases/latest/download/latest.json",
    ]);
    expect(macos.productName).toBe("June");
    expect(macos.bundle.macOS.bundleName).toBe("Clovy");
    expect(macosInfoPlist).toContain("<key>CFBundleDisplayName</key>\n  <string>June</string>");
    expect(macosInfoPlist).toContain("<key>LSHasLocalizedDisplayName</key>\n  <true/>");
    expect(macosLocalizedInfoPlist).toContain('CFBundleDisplayName = "Clovy";');
    expect(macosLocalizedInfoPlist).toContain('CFBundleName = "Clovy";');
    expect(macos.bundle.resources["resources/macos/en.lproj/InfoPlist.strings"]).toBe(
      "en.lproj/InfoPlist.strings",
    );
    expect(macos.bundle.resources["../.tauri-helper/june-nm-shim"]).toBe("native/bin/june-nm-shim");
    expect(bundleShim).toContain('legacy_out=".tauri-helper/june-nm-shim"');
    expect(windows.productName).toBe("June");
    expect(desktopCargo).toMatch(/^default-run = "os-june"$/m);
    expect(desktopCargo).toMatch(/^name = "os-june"$/m);
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
    expect(windowsQualification).toContain("firstClovyEnvironmentRestored");
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
    expect(computerUseSelfTest).toContain("co.opensoftware.clovy.computer-use-self-test.${role}");
    expect(computerUseSelfTest).not.toContain(
      "co.opensoftware.june.computer-use-self-test.${role}",
    );
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

  it("keeps rollback-safe bridges for credentials, browser storage, headers, and hosts", async () => {
    const [
      credentials,
      storage,
      storageBootstrap,
      main,
      hud,
      agentHud,
      meetingHud,
      desktopApi,
      backendApi,
      extensionHost,
      extensionProtocol,
    ] = await Promise.all([
      read("src-tauri/src/credential_compat.rs"),
      read("src/lib/storage-compat.ts"),
      read("src/lib/storage-compat-bootstrap.ts"),
      read("src/main.tsx"),
      read("src/hud.ts"),
      read("src/agent-hud.ts"),
      read("src/meeting-hud.ts"),
      read("src-tauri/src/clovy_api.rs"),
      read("clovy-api/crates/api/src/lib.rs"),
      read("src-tauri/src/extension_host.rs"),
      read("extension/src/protocol.ts"),
    ]);

    expect(credentials).toContain("canonical_service");
    expect(credentials).toContain("legacy_service");
    expect(storage).toContain('key.startsWith("clovy:")');
    expect(storage).toContain('`june:${key.slice("clovy:".length)}`');
    expect(storage).toContain('key.startsWith("os-clovy:")');
    expect(storage).toContain('`os-june:${key.slice("os-clovy:".length)}`');
    expect(storageBootstrap).toContain("installStorageCompatibilityBridge();");
    for (const entrypoint of [main, hud, agentHud, meetingHud]) {
      expect(entrypoint.startsWith('import "./lib/storage-compat-bootstrap";')).toBe(true);
    }
    for (const source of [desktopApi, backendApi]) {
      expect(source).toContain("x-clovy-app-version");
      expect(source).toContain("x-june-app-version");
    }
    for (const source of [extensionHost, extensionProtocol]) {
      expect(source).toContain("co.opensoftware.clovy.extension");
      expect(source).toContain("co.opensoftware.june.extension");
    }
  });

  it("runs Clovy API checks when the canonical companion protocol changes", async () => {
    const apiChecks = await read(".github/workflows/clovy-api.yml");
    const apiCheckPaths = [
      "clovy-api/**",
      "crates/clovy-companion-protocol/**",
      ".github/workflows/clovy-api.yml",
      ".github/actions/setup-rust/**",
    ];

    expect(workflowTriggerPaths(apiChecks, "pull_request")).toEqual(apiCheckPaths);
    expect(workflowTriggerPaths(apiChecks, "push")).toEqual(apiCheckPaths);
  });

  it("preserves released API and C ABI contracts while publishing canonical aliases", async () => {
    const [
      desktopApi,
      canonicalHeader,
      canonicalCompatibilityHeader,
      oldPathCompatibilityHeader,
      cryptoSource,
    ] = await Promise.all([
      read("src-tauri/src/clovy_api.rs"),
      read("crates/clovy-companion-crypto/include/clovy_companion_crypto.h"),
      read("crates/clovy-companion-crypto/include/june_companion_crypto.h"),
      read("crates/june-companion-crypto/include/june_companion_crypto.h"),
      read("crates/clovy-companion-crypto/src/lib.rs"),
    ]);

    expect(desktopApi).toContain('"https://june-api.opensoftware.co"');
    expect(desktopApi).toContain('b"june-api-operation-id-v1\\0"');
    expect(desktopApi).toContain('format!("june-op-');
    expect(canonicalHeader).toContain("clovy_crypto_generate_identity");
    expect(canonicalCompatibilityHeader).toContain("june_crypto_generate_identity");
    expect(oldPathCompatibilityHeader).toContain("june_crypto_generate_identity");
    expect(oldPathCompatibilityHeader).toBe(canonicalCompatibilityHeader);
    expect(cryptoSource).toContain('pub unsafe extern "C" fn clovy_crypto_generate_identity');
    expect(cryptoSource).toContain('pub unsafe extern "C" fn june_crypto_generate_identity');
  });

  it("uses Clovy for newly written runtime state and private source concepts", async () => {
    const [
      sdkEngine,
      notifications,
      routines,
      computerUseDriver,
      meetingDetection,
      obsidian,
      desktopApi,
      httpClient,
      apiMain,
      apiConfig,
      driverContract,
      noteDrag,
    ] = await Promise.all([
      read("agent-runtime/src/sdk-engine.ts"),
      read("src-tauri/src/notifications.rs"),
      read("src-tauri/src/routines.rs"),
      read("src-tauri/src/computer_use_driver.rs"),
      read("src-tauri/src/meeting_detection.rs"),
      read("src-tauri/src/obsidian.rs"),
      read("src-tauri/src/clovy_api.rs"),
      read("src-tauri/src/http_client.rs"),
      read("clovy-api/crates/app/src/main.rs"),
      read("clovy-api/crates/config/src/lib.rs"),
      read("src-tauri/cua-driver-contract.json").then(JSON.parse),
      read("src/lib/dnd.ts"),
    ]);

    expect(sdkEngine).toContain("clovyVersion: SERIALIZED_STATE_ENVELOPE_VERSION");
    expect(sdkEngine).toContain("juneVersion: SERIALIZED_STATE_ENVELOPE_VERSION");
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
    expect(desktopApi).toContain("Clovy API returned an invalid response stream error");
    // The Clovy API client takes its user agent from the shared HTTP builder.
    expect(desktopApi).toContain("crate::http_client::base_client_builder()");
    expect(httpClient).toContain('pub const USER_AGENT: &str = concat!("clovy/"');
    expect(httpClient).toContain(".user_agent(USER_AGENT)");
    for (const source of [desktopApi, httpClient]) {
      expect(source).not.toContain('concat!("os-june/"');
    }
    expect(apiMain).not.toContain('"june=info,clovy_api=info');
    expect(apiConfig).toContain("viewer@localdev.clovy");
    expect(apiConfig).not.toContain("viewer@localdev.june");
    expect(driverContract.clovyActions).toBeInstanceOf(Array);
    expect(driverContract).not.toHaveProperty("juneActions");
    expect(noteDrag).toContain('NOTE_DND_MIME = "application/x-clovy-note-id"');
    expect(noteDrag).toContain('LEGACY_NOTE_DND_MIME = "application/x-june-note-id"');
  });

  it("publishes the Obsidian skill under Clovy with a rollback-safe June alias", async () => {
    const [bundledSkill, skillIdentity, agentApi] = await Promise.all([
      read("src-tauri/resources/agent-skills/clovy-obsidian/SKILL.md"),
      read("src-tauri/src/agent_runtime/skill_identity.rs"),
      read("src-tauri/src/agent_runtime/api.rs"),
    ]);

    expect(bundledSkill).toMatch(/^name: clovy-obsidian$/m);
    expect(skillIdentity).toContain('pub const CLOVY_OBSIDIAN_SKILL_ID: &str = "clovy-obsidian"');
    expect(skillIdentity).toContain('pub const LEGACY_OBSIDIAN_SKILL_ID: &str = "june-obsidian"');
    expect(skillIdentity).toContain("write_skill_ids");
    expect(agentApi).toContain("When the clovy-obsidian skill is available");
    expect(agentApi).not.toContain("When the june-obsidian skill is available");
  });

  it("uses Clovy for ephemeral build and dictation artifacts", async () => {
    const [
      signedDmg,
      tauri,
      macHelper,
      windowsHelper,
      dictation,
      computerUseFixture,
      reproducibleBuilds,
      index,
      onboardingPreview,
      styleguide,
    ] = await Promise.all([
      read("scripts/build-signed-dmg.sh"),
      read("src-tauri/tauri.conf.json").then(JSON.parse),
      read("src-tauri/native/mac-dictation-helper/main.swift"),
      read("src-tauri/native/windows-dictation-helper/src/audio.rs"),
      read("src-tauri/src/dictation.rs"),
      read("src-tauri/native/computer-use-fixture/main.swift"),
      read("docs/reproducible-builds.md"),
      read("index.html"),
      read("onboarding-preview.html"),
      read("styleguide.html"),
    ]);

    expect(signedDmg).toContain("clovy-signing.keychain-db");
    expect(signedDmg).not.toContain("os-june-signing.keychain-db");
    expect(tauri.app.security.assetProtocol.scope).toEqual([
      "$TEMP/clovy-dictation-*.m4a",
      "$TEMP/clovy-dictation-*.wav",
      "$TEMP/os-june-dictation-*.m4a",
      "$TEMP/os-june-dictation-*.wav",
    ]);
    expect(macHelper).toContain('appendingPathComponent("clovy-dictation-');
    expect(windowsHelper).toContain('.prefix("clovy-dictation-")');
    expect(dictation).toContain('format!("clovy-dictation-{utterance_id}-normalized.wav")');
    expect(computerUseFixture).toContain('"clovy-cu-observer-window"');
    expect(computerUseFixture).not.toContain('"june-cu-observer-window"');
    expect(reproducibleBuilds).toContain("the `clovy-api` binary");
    expect(index.indexOf('localStorage.getItem("os-clovy:theme")')).toBeLessThan(
      index.indexOf('localStorage.getItem("os-june:theme")'),
    );
    expect(onboardingPreview).toContain("clovy.replayOnboarding()");
    expect(onboardingPreview).toContain("window.__CLOVY_STUB__");
    expect(styleguide).toContain("else os-clovy:theme / system");
  });
});
