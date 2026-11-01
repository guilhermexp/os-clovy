import { spawnSync } from "node:child_process";
import { chmodSync, existsSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { afterEach, describe, expect, it } from "vitest";

const runner = resolve(__dirname, "../../scripts/cargo-clovy-runner.sh");

describe("development runner", () => {
  let root: string | undefined;

  afterEach(() => {
    if (root) rmSync(root, { recursive: true, force: true });
    root = undefined;
  });

  // The real runner prepares a signed .app and invokes its macOS launcher.
  // Native launch coverage also lives in cargo-clovy-runner.test.mjs.
  it.skipIf(process.platform !== "darwin")(
    "builds the clovy-mcp relay beside the app it launches",
    () => {
      root = mkdtempSync(join(tmpdir(), "clovy-runner-"));
      const bin = join(root, "bin");
      const target = join(root, "target");
      spawnSync("mkdir", ["-p", bin]);
      // A stand-in cargo: `build` writes one runnable file per `--bin`, the way
      // Cargo leaves binaries in target/debug.
      writeFileSync(
        join(bin, "cargo"),
        [
          "#!/bin/sh",
          '[ "$1" = build ] || exit 3',
          'mkdir -p "$CARGO_TARGET_DIR/debug"',
          "while [ $# -gt 0 ]; do",
          '  if [ "$1" = --bin ]; then shift; printf "#!/bin/sh\\nexit 0\\n" > "$CARGO_TARGET_DIR/debug/$1"; chmod +x "$CARGO_TARGET_DIR/debug/$1"; fi',
          "  shift",
          "done",
          "",
        ].join("\n"),
      );
      chmodSync(join(bin, "cargo"), 0o755);

      const result = spawnSync(
        "bash",
        [runner, "run", "--no-default-features", "--features", "helper-binaries", "--"],
        {
          cwd: root,
          // This fixture verifies Cargo output and relay placement, without
          // querying the owner's signing identities for its stand-in binaries.
          env: {
            ...process.env,
            PATH: `${bin}:${process.env.PATH}`,
            CARGO_TARGET_DIR: target,
            CLOVY_DEV_SIGNING_IDENTITY: "-",
          },
          encoding: "utf8",
        },
      );

      expect(result.status, result.stderr).toBe(0);
      expect(existsSync(join(target, "debug", "clovy"))).toBe(true);
      expect(existsSync(join(target, "debug", "clovy-mcp"))).toBe(true);
    },
  );
});
