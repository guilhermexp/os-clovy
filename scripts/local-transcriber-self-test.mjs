#!/usr/bin/env node
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const [dataDir, ...selectedIds] = process.argv.slice(2);
assert.ok(dataDir, "Pass a run-owned dev app data directory with installed Local models.");
const localModels = {
  "parakeet-ultra": "parakeet-ultra-coreml",
  "phonon-2.5": "phonon-2.5-coreml",
  "fluid-speech-pro": "fluid-speech-pro-coreml",
  "whisper-large-v3-turbo-local": "whisper-large-v3-turbo-coreml",
};
const executable = path.join(
  root,
  ".tauri-helper/Clovy Local Transcriber.app/Contents/MacOS/clovy-local-transcriber",
);
const fixture = path.join(root, "src-tauri/tests/fixtures/parakeet-pt.wav");
const requests = [];
function request(modelId, fields) {
  requests.push({ request_id: requests.length + 1, model_id: modelId, ...fields });
}
function installed(id) {
  return existsSync(path.join(path.resolve(dataDir), "models", localModels[id], "installed.json"));
}
const ids = selectedIds.length ? selectedIds : Object.keys(localModels).filter(installed);
assert.ok(ids.length, "No installed local speech models found in the test profile.");
for (const id of ids) {
  assert.ok(Object.hasOwn(localModels, id), `Unknown local model ${id}`);
  const directory = path.join(path.resolve(dataDir), "models", localModels[id]);
  assert.ok(installed(id), `Install ${id} in the test profile.`);
  request(id, { op: "load", model_dir: directory });
  request(id, { op: "transcribe", audio_path: fixture, language: "pt" });
  request(id, { op: "transcribe", audio_path: fixture, language: "pt" });
  request(id, { op: "load", model_dir: directory, expect_reuse: true });
  request(id, { op: "unload" });
  request(id, { op: "transcribe", audio_path: fixture, language: "pt", expect_unloaded: true });
}
const result = spawnSync(executable, [], {
  input: `${requests.map(({ expect_reuse, expect_unloaded, ...request }) => JSON.stringify(request)).join("\n")}\n`,
  encoding: "utf8",
  timeout: 360_000,
  maxBuffer: 4 * 1024 * 1024,
});
assert.equal(result.status, 0, result.error?.message ?? result.stderr);
const responses = result.stdout
  .trim()
  .split("\n")
  .map((line) => JSON.parse(line));
assert.equal(responses.length, requests.length);
for (const [index, response] of responses.entries()) {
  const request = requests[index];
  assert.equal(response.request_id, request.request_id);
  assert.equal(response.model_id, request.model_id);
  if (request.expect_unloaded) {
    assert.equal(response.ok, false);
    assert.match(response.error, /Load the requested model/);
    continue;
  }
  assert.equal(response.ok, true, response.error);
  if (request.op === "load") assert.equal(response.reused, request.expect_reuse ?? false);
  if (request.op === "transcribe") {
    assert.match(response.text.toLocaleLowerCase("pt-BR"), /reunião/);
    assert.match(response.text.toLocaleLowerCase("pt-BR"), /português/);
    assert.ok(Number.isSafeInteger(response.duration_ms) && response.duration_ms > 0);
    assert.ok(Array.isArray(response.tokens) && response.tokens.length > 0);
    for (const token of response.tokens) {
      assert.equal(typeof token.t, "string");
      assert.ok(Number.isSafeInteger(token.start_ms) && token.start_ms >= 0);
      assert.ok(Number.isSafeInteger(token.end_ms) && token.end_ms >= token.start_ms);
      assert.ok(token.end_ms <= response.duration_ms);
    }
  }
}
console.log(JSON.stringify(responses, null, 2));
