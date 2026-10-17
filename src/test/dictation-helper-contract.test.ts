// Source contract for the macOS dictation helper (ditado-rapido). The helper
// has no Swift test target, so these checks read main.swift and pin the
// ordering decisions the latency work depends on. The Mac proof (task 4.2)
// measures the real timings.
import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

const source = readFileSync("src-tauri/native/mac-dictation-helper/main.swift", "utf8");

/** The body of the first Swift declaration starting with `signature`. */
function body(signature: string): string {
  const start = source.indexOf(signature);
  if (start < 0) throw new Error(`missing ${signature}`);
  const open = source.indexOf("{", start);
  let depth = 0;
  for (let index = open; index < source.length; index += 1) {
    if (source[index] === "{") depth += 1;
    if (source[index] === "}") {
      depth -= 1;
      if (depth === 0) return source.slice(open, index + 1);
    }
  }
  throw new Error(`unterminated ${signature}`);
}

function code(text: string): string {
  return text
    .split("\n")
    .filter((line) => !line.trim().startsWith("//"))
    .join("\n");
}

describe("dictation helper: capture starts on the press (D3)", () => {
  it("asks for microphone access only when it is not already granted", () => {
    const start = code(body("func start(takeID: String? = nil)"));
    const authorized = start.indexOf("authorizationStatus(for: .audio) == .authorized");
    const request = start.indexOf("AVCaptureDevice.requestAccess");
    expect(authorized).toBeGreaterThan(-1);
    expect(request).toBeGreaterThan(authorized);
    // The granted path begins capture before any permission callback.
    expect(start.slice(authorized, request)).toContain("beginDictationCapture()");
  });

  it("starts recording before the start cue and never waits for the cue", () => {
    const begin = code(body("private func beginDictationCapture()"));
    const record = begin.indexOf("startRecording(purpose: .dictation");
    const cue = begin.indexOf("RecordingCuePlayer.play");
    expect(record).toBeGreaterThan(-1);
    expect(cue).toBeGreaterThan(record);
    expect(source).not.toContain("RecordingCuePlayer.play(.start, completion:");
  });

  it("cuts the cue's span from the take before handing it to Rust", () => {
    const stop = code(body("private func stopActiveRecording()"));
    expect(stop).toContain("finishDictationRecording(recordingInstanceID: recordingInstanceID)");
    const finish = code(body("private func finishDictationRecording("));
    expect(finish).toContain("trimLeadingAudio(");
    expect(finish).toContain("emitRecordingReady(recordingInstanceID: recordingInstanceID)");
    const trim = code(body("func trimLeadingAudio("));
    // Passthrough: no re-encoding on the release path.
    expect(trim).toContain("AVAssetExportPresetPassthrough");
  });
});
