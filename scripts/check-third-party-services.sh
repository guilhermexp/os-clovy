#!/usr/bin/env sh
# Fails when the app source names an Open Software or June service address.
# The fork talks only to servers its owner configures (openspec local-account).
# Tests are skipped: some pin repository and release metadata owned elsewhere.
set -eu

cd "$(dirname "$0")/.."

pattern='opensoftware\.co\b|june\.link|open-software-network'
matches=$(grep -rnE "$pattern" src src-tauri/src src-tauri/*.json | grep -v '^src/test/' || true)

if [ -n "$matches" ]; then
  echo "Open Software service addresses found in app source:" >&2
  echo "$matches" >&2
  exit 1
fi
echo "No Open Software service addresses in app source."
