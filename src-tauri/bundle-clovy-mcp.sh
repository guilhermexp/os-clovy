#!/usr/bin/env bash
# macOS beforeBundleCommand (tauri.macos.conf.json), after
# scripts/bundle-nm-shim.sh: copies the compiled clovy-mcp [[bin]] (the Clovy
# MCP server's stdio relay, src/mcp_server/stdio.rs) into .tauri-helper/ so the
# bundle.resources mapping ships the real binary instead of the build.rs
# placeholder, then signs it like the shim (Developer ID when
# APPLE_SIGNING_IDENTITY is set, ad-hoc otherwise).
#
# Target resolution mirrors scripts/bundle-nm-shim.sh: a universal build lipo's
# the two real-triple binaries; anything else takes the triple's (or the bare)
# target dir.
set -euo pipefail

cd "$(dirname "$0")/.."

profile="release"
if [[ "${TAURI_ENV_DEBUG:-false}" == "true" ]]; then
  profile="debug"
fi

triple="${TAURI_ENV_TARGET_TRIPLE:-}"
mkdir -p .tauri-helper
out=".tauri-helper/clovy-mcp"

if [[ "$triple" == "universal-apple-darwin" ]]; then
  arm="src-tauri/target/aarch64-apple-darwin/$profile/clovy-mcp"
  x86="src-tauri/target/x86_64-apple-darwin/$profile/clovy-mcp"
  for bin in "$arm" "$x86"; do
    if [[ ! -f "$bin" ]]; then
      echo "clovy-mcp missing for universal bundle: $bin" >&2
      exit 1
    fi
  done
  lipo -create "$arm" "$x86" -output "$out"
  archs="$(lipo -archs "$out")"
  if [[ "$archs" != *arm64* || "$archs" != *x86_64* ]]; then
    echo "universal clovy-mcp has wrong architectures: $archs" >&2
    exit 1
  fi
  src="lipo($arm, $x86)"
else
  candidates=(
    "src-tauri/target/${triple}/$profile/clovy-mcp"
    "src-tauri/target/$profile/clovy-mcp"
  )
  src=""
  for candidate in "${candidates[@]}"; do
    if [[ -f "$candidate" ]]; then
      src="$candidate"
      break
    fi
  done
  if [[ -z "$src" ]]; then
    echo "clovy-mcp binary not found (looked in: ${candidates[*]})" >&2
    exit 1
  fi
  cp -f "$src" "$out"
fi

chmod +x "$out"

identity="${APPLE_SIGNING_IDENTITY:-}"
if [[ -n "${identity// /}" ]]; then
  codesign --force --entitlements src-tauri/Entitlements.plist \
    --sign "$identity" --timestamp --options runtime "$out"
else
  codesign --force --entitlements src-tauri/Entitlements.plist \
    --sign - "$out"
fi

echo "bundled clovy-mcp from $src"
