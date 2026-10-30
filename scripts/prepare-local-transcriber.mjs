#!/usr/bin/env node
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { copyFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { computerUseBundleIdentifier } from "./computer-use-dev.mjs";
import {
  DEV_SIGNING_IDENTITY_ENV,
  resolveDevSigningIdentity,
  signatureSatisfies,
} from "./dev-signing-identity.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
if (
  process.platform !== "darwin" ||
  (process.env.TAURI_ENV_PLATFORM && process.env.TAURI_ENV_PLATFORM !== "darwin")
) {
  console.error("Skipping Local transcription helper: macOS only.");
  process.exit(0);
}
const source = path.join(root, "src-tauri/native/mac-local-transcriber");
const pin = JSON.parse(readFileSync(path.join(source, "pin.json"), "utf8"));
// Package.resolved is versioned with Package.swift; fail closed if the pinned lockfile is missing.
const hash = createHash("sha256");
for (const file of [
  "Package.swift",
  "Package.resolved",
  "Sources/main.swift",
  "FluidAudio-LICENSE",
]) {
  hash
    .update(file)
    .update("\0")
    .update(readFileSync(path.join(source, file)))
    .update("\0");
}
const fingerprint = hash.digest("hex");
if (fingerprint !== pin.sourceSha256)
  throw new Error("Local transcriber source differs from pin.json; review and refresh the pin.");
const bundle = path.join(root, ".tauri-helper/Clovy Local Transcriber.app");
const contents = path.join(bundle, "Contents");
const executable = path.join(contents, "MacOS/clovy-local-transcriber");
const stamp = path.join(contents, "Resources/pin.json");
// Signing changes executable bytes. Keep the post-sign digest outside the sealed bundle.
const outputDigest = path.join(root, ".tauri-helper/local-transcriber-executable.sha256");
const executableDigest = () => createHash("sha256").update(readFileSync(executable)).digest("hex");
const config = JSON.parse(readFileSync(path.join(root, "src-tauri/tauri.conf.json"), "utf8"));
const release = process.argv.includes("--release");
const identifier = computerUseBundleIdentifier({
  baseIdentifier: `${process.env.CLOVY_DEV_APP_IDENTIFIER || config.identifier}.local-transcriber`,
  profile: release ? "release" : "debug",
  worktreeRoot: root,
});
const configuredIdentity =
  (release ? process.env.APPLE_SIGNING_IDENTITY : process.env.CLOVY_DEV_SIGNING_IDENTITY)?.trim() ||
  undefined;
// The shared signature matcher compares the certificate hash, not its display name.
const identity = configuredIdentity
  ? resolveDevSigningIdentity({
      env: { ...process.env, [DEV_SIGNING_IDENTITY_ENV]: configuredIdentity },
    }).identity
  : undefined;
function run(command, args, options = {}) {
  const result = spawnSync(command, args, { cwd: root, stdio: "inherit", ...options });
  if (result.status !== 0) throw new Error(`${command} exited with ${result.status}`);
}
const reusable =
  existsSync(executable) &&
  existsSync(stamp) &&
  readFileSync(stamp, "utf8") === `${fingerprint}\n${identifier}\n` &&
  existsSync(outputDigest) &&
  readFileSync(outputDigest, "utf8").trim() === executableDigest();
if (!reusable) {
  run("swift", [
    "build",
    "--package-path",
    source,
    "-c",
    "release",
    "--arch",
    "arm64",
    "--disable-automatic-resolution",
  ]);
  mkdirSync(path.dirname(executable), { recursive: true });
  mkdirSync(path.dirname(stamp), { recursive: true });
  const location = spawnSync(
    "swift",
    ["build", "--package-path", source, "-c", "release", "--arch", "arm64", "--show-bin-path"],
    { cwd: root, encoding: "utf8" },
  );
  if (location.status !== 0) throw new Error("Swift could not resolve its build output");
  copyFileSync(path.join(location.stdout.trim(), "clovy-local-transcriber"), executable);
  writeFileSync(
    path.join(contents, "Info.plist"),
    `<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>clovy-local-transcriber</string>
<key>CFBundleIdentifier</key><string>${identifier}</string>
<key>CFBundleName</key><string>Clovy Local Transcriber</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleVersion</key><string>1</string>
<key>LSMinimumSystemVersion</key><string>14.0</string>
<key>LSUIElement</key><true/>
</dict></plist>\n`,
  );
  copyFileSync(
    path.join(source, "FluidAudio-LICENSE"),
    path.join(contents, "Resources/FluidAudio-LICENSE"),
  );
  writeFileSync(stamp, `${fingerprint}\n${identifier}\n`);
}
// As with the Computer use helper, an unspecified identity preserves a valid seal.
const signatureValid = identity
  ? signatureSatisfies(bundle, identity)
  : spawnSync("/usr/bin/codesign", ["--verify", "--deep", "--strict", bundle], {
      encoding: "utf8",
    }).status === 0;
if (!reusable || !signatureValid) {
  run("/usr/bin/codesign", [
    "--force",
    "--deep",
    "--sign",
    identity ?? "-",
    "--timestamp=none",
    bundle,
  ]);
}
run("/usr/bin/codesign", ["--verify", "--deep", "--strict", bundle]);
writeFileSync(outputDigest, `${executableDigest()}\n`);
console.error(
  reusable ? "Reusing Local transcriber helper." : "Prepared Local transcriber helper.",
);
