#!/usr/bin/env node

// Build-time source preparation only. No engine download happens at runtime.
import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const pin = JSON.parse(readFileSync(join(root, "src-tauri", "agent-desktop-pin.json"), "utf8"));
const args = process.argv.slice(2);
const index = args.indexOf("--target");
if (args.length !== 0 && (args.length !== 2 || index !== 0 || !args[1])) {
  throw new Error("Usage: prepare-agent-desktop.mjs [--target <triple>]");
}
const target = index < 0 ? hostTriple() : args[index + 1];
const supported = [
  "aarch64-apple-darwin",
  "x86_64-apple-darwin",
  "x86_64-pc-windows-msvc",
  "aarch64-pc-windows-msvc",
  "x86_64-unknown-linux-gnu",
  "aarch64-unknown-linux-gnu",
];
if (!supported.includes(target)) throw new Error(`Unsupported agent-desktop target: ${target}`);
if (target.includes("apple-darwin") && process.platform !== "darwin")
  throw new Error("macOS engine signing requires macOS");
if (
  target.includes("windows") &&
  (process.platform !== "win32" || !process.env.WINDOWS_CERTIFICATE_THUMBPRINT)
)
  throw new Error("Windows engine signing requires Windows and WINDOWS_CERTIFICATE_THUMBPRINT");
const source = join(root, ".tauri-helper", "agent-desktop-source");
const output = join(root, ".tauri-helper", "agent-desktop", target);
const targetDir = resolve(
  process.env.CARGO_TARGET_DIR || join(root, "src-tauri", "target", "agent-desktop"),
);
mkdirSync(dirname(source), { recursive: true });
const newSource = !existsSync(source);
if (newSource)
  run("git", ["-c", "core.autocrlf=false", "clone", "--no-checkout", pin.source, source], root);
if (
  !newSource &&
  run(
    "git",
    [
      "-c",
      "core.autocrlf=false",
      "status",
      "--porcelain",
      "--untracked-files=all",
      "--ignored=matching",
    ],
    source,
    true,
  ).trim()
) {
  throw new Error("agent-desktop source checkout has local changes; refusing to build it");
}
run("git", ["fetch", "--no-tags", pin.source, pin.sourceCommit], source);
run("git", ["-c", "core.autocrlf=false", "checkout", "--detach", pin.sourceCommit], source);
if (run("git", ["rev-parse", "HEAD"], source, true).trim() !== pin.sourceCommit) {
  throw new Error("agent-desktop source commit mismatch");
}
verifyHash(join(source, "Cargo.lock"), pin.cargoLockSha256);
verifyHash(join(root, "src-tauri", "agent-desktop", "Cargo.lock"), pin.cargoLockSha256);
verifyHash(join(source, "LICENSE"), pin.licenseSha256);
run(
  process.env.CARGO || "cargo",
  ["build", "--locked", "--release", "--package", "agent-desktop", "--target", target],
  source,
  false,
  { ...process.env, CARGO_TARGET_DIR: targetDir },
);
const executableName = target.includes("windows") ? "agent-desktop.exe" : "agent-desktop";
mkdirSync(output, { recursive: true });
const executable = join(output, executableName);
copyFileSync(join(targetDir, target, "release", executableName), executable);
if (target.includes("apple-darwin")) {
  run(
    "/usr/bin/codesign",
    [
      "--force",
      "--sign",
      process.env.APPLE_SIGNING_IDENTITY || process.env.CLOVY_DEV_SIGNING_IDENTITY || "-",
      executable,
    ],
    root,
  );
  run("/usr/bin/codesign", ["--verify", "--strict", executable], root);
} else if (target.includes("windows")) {
  run(
    "signtool",
    ["sign", "/sha1", process.env.WINDOWS_CERTIFICATE_THUMBPRINT, "/fd", "sha256", executable],
    root,
  );
  run("signtool", ["verify", "/pa", executable], root);
}
copyFileSync(join(source, "LICENSE"), join(output, "LICENSE"));
const manifest = {
  source: pin.source,
  sourceCommit: pin.sourceCommit,
  version: pin.version,
  protocolVersion: pin.protocolVersion,
  target,
  executable: executableName,
  executableSha256: hash(executable),
  cargoLockSha256: pin.cargoLockSha256,
  license: pin.license,
  licenseSha256: pin.licenseSha256,
};
writeFileSync(join(output, "manifest.json"), `${JSON.stringify(manifest, null, 2)}\n`);
console.error(
  `Prepared agent-desktop ${pin.sourceCommit} for ${target}: ${manifest.executableSha256}`,
);

function hash(path) {
  return createHash("sha256").update(readFileSync(path)).digest("hex");
}
function verifyHash(path, expected) {
  if (hash(path) !== expected) throw new Error(`Source hash mismatch: ${path}`);
}
function run(command, argv, cwd, capture = false, env = process.env) {
  const result = spawnSync(command, argv, {
    cwd,
    env,
    encoding: "utf8",
    stdio: capture ? ["ignore", "pipe", "inherit"] : "inherit",
    shell: false,
  });
  if (result.error || result.status !== 0)
    throw result.error || new Error(`${command} failed with exit ${result.status}`);
  return result.stdout || "";
}
function hostTriple() {
  const arch =
    process.arch === "arm64" ? "aarch64" : process.arch === "x64" ? "x86_64" : "unsupported";
  const os =
    process.platform === "darwin"
      ? "apple-darwin"
      : process.platform === "win32"
        ? "pc-windows-msvc"
        : process.platform === "linux"
          ? "unknown-linux-gnu"
          : "unsupported";
  return `${arch}-${os}`;
}
