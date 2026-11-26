// Source contract for the macOS dictation helper (ditado-rapido). The helper
// has no Swift test target, so these checks read main.swift and pin the
// ordering decisions the latency work depends on. The Mac proof (task 4.2)
// measures the real timings.
import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

const source = readFileSync("src-tauri/native/mac-dictation-helper/main.swift", "utf8");

/** The body of the first Swift declaration starting with `signature`. */
function body(signature: string, from = 0): string {
  const start = source.indexOf(signature, from);
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

  it("prepares the complete file after capture stops", () => {
    const stop = code(body("private func stopActiveRecording()"));
    expect(stop).toMatch(
      /selectedDeviceRecorder\.stop \{[\s\S]*?RecordingCuePlayer\.play\(\.stop\)[\s\S]*?finishDictationRecording\(/,
    );
    expect(stop).toMatch(
      /audioRecorder\?\.stop\(\)\s*RecordingCuePlayer\.play\(\.stop\)\s*finishDictationRecording\(/,
    );
    const finish = code(body("private func finishDictationRecording("));
    expect(finish).toContain("prepareDictationAudio(at: recordingURL");
    expect(finish).toContain("emitRecordingReady(recordingInstanceID: recordingInstanceID)");
    expect(source).not.toMatch(/trimLeadingAudio|startCueTrim|settleSelectedDeviceCueTrim/);
  });

  it("only publishes converted audio while its take owns finalization", () => {
    const finish = code(body("private func finishDictationRecording("));
    expect(finish.indexOf("recordingPurpose == .dictation")).toBeLessThan(
      finish.indexOf("prepareDictationAudio("),
    );
    const publish = finish.indexOf("moveItem(at: temporaryWAV");
    expect(publish).toBeGreaterThan(-1);
    for (const guard of [
      "self.recordingInstanceID == recordingInstanceID",
      "self.activeTakeID == takeID",
      "self.isFinalizing",
      "self.recordingURL == recordingURL",
    ]) {
      expect(finish.slice(0, publish)).toContain(guard);
    }
    const selfTest = source.indexOf(
      'CommandLine.arguments.contains("--self-test-audio-conversion")',
    );
    expect(selfTest).toBeGreaterThan(-1);
    expect(selfTest).toBeLessThan(source.indexOf("let app = NSApplication.shared"));
  });
});

describe("dictation helper: ready before the first press (D4)", () => {
  const launch = code(source.slice(source.indexOf('emit("ready")')));

  it("loads the cues and prepares the first recorder at launch", () => {
    expect(launch).toContain("RecordingCuePlayer.preload()");
    expect(launch).toContain("dictation.prepareNextRecording()");
    // Both before the run loop, so they are done by the first press.
    expect(launch.indexOf("dictation.prepareNextRecording()")).toBeLessThan(
      launch.indexOf("app.run()"),
    );
  });

  it("prepares a recorder without recording", () => {
    const prepare = code(body("func prepareNextRecording()"));
    expect(prepare).toContain("authorizationStatus(for: .audio) == .authorized");
    expect(prepare).toContain("prepareToRecord()");
    expect(prepare).not.toMatch(/\.record\(/);
  });

  it("uses the prepared recorder for a take and prepares the next one after it", () => {
    const start = code(body("private func startRecording("));
    expect(start).toContain("takePreparedRecorder()");
    const reset = code(body("private func resetRecordingState("));
    expect(reset).toContain("prepareNextRecording()");
  });

  it("drops a prepared recorder when the microphone choice or devices change", () => {
    expect(code(body("func setMicrophone("))).toContain("discardPreparedRecorder()");
    expect(source).toContain("AVCaptureDevice.wasConnectedNotification");
    expect(source).toContain("AVCaptureDevice.wasDisconnectedNotification");
    expect(code(body("func shutdown()"))).toContain("discardPreparedRecorder()");
  });
});

describe("dictation helper: paste without a fixed wait (D6)", () => {
  it("pastes at once into a target that was already frontmost", () => {
    const paste = code(body("static func paste(_ text: String, takeID: String?)"));
    expect(paste).toContain("let targetWasActive = target.isActive");
    expect(paste).toContain("settleAfterActivation: !targetWasActive");
    const wait = code(body("private static func waitForActivation("));
    const immediate = wait.indexOf("guard settleAfterActivation else");
    const settle = wait.indexOf("activationSettleDelay");
    expect(immediate).toBeGreaterThan(-1);
    expect(settle).toBeGreaterThan(immediate);
    expect(wait.slice(immediate, settle)).toContain("completion(true)");
  });

  it("still polls and settles for a target it had to activate", () => {
    const wait = code(body("private static func waitForActivation("));
    expect(wait).toContain("activationPollInterval");
    expect(wait).toMatch(/waitForActivation\(\s*of: target,[\s\S]*settleAfterActivation: true/);
  });
});

describe("dictation helper: reports the take's length (R7)", () => {
  it("sends the finished file's length with recording_ready", () => {
    const ready = code(body("private func emitRecordingReady("));
    expect(ready).toContain("audioDurationSeconds(at: recordingURL)");
    expect(ready).toContain('basePayload["audioDurationSeconds"]');
    expect(ready.indexOf('basePayload["audioDurationSeconds"]')).toBeLessThan(
      ready.indexOf('emit("recording_ready", basePayload)'),
    );
  });
});

describe("dictation helper: normal stop uses captured audio", () => {
  it("checks the recorder clock and invalidates the deferred stop on reset", () => {
    const stop = code(body("func stop(takeID: String? = nil)"));
    expect(stop).toContain("completeNormalStop(");
    const complete = code(body("private func completeNormalStop("));
    expect(complete).toContain("audioRecorder?.currentTime");
    expect(complete).toContain("selectedDeviceRecorder?.capturedDuration");
    expect(complete).toContain("recordingInstanceID");
    const reset = code(body("private func resetRecordingState("));
    expect(reset).toContain("pendingStop = nil");
  });

  it("publishes appended audio to a snapshot that main can read without joining capture", () => {
    const getter = code(body("var capturedDuration: TimeInterval"));
    expect(getter).toContain("durationSnapshot.seconds");
    expect(getter).not.toContain("queue.sync");
    const capture = code(body("func captureOutput("));
    const append = capture.indexOf("writerInput.append(sampleBuffer)");
    expect(append).toBeGreaterThan(-1);
    expect(capture.indexOf("durationSnapshot.add(")).toBeGreaterThan(append);
  });

  it("queues pinned recorder teardown on reset without waiting on main", () => {
    const recorder = source.indexOf("final class SelectedDeviceRecorder:");
    const cancel = code(body("func cancel(", recorder));
    expect(cancel).toContain("queue.async");
    expect(cancel).not.toContain("queue.sync");
    const reset = code(body("private func resetRecordingState("));
    expect(reset.indexOf("recordingInstanceID = nil")).toBeLessThan(
      reset.indexOf("selectedDeviceRecorder?.cancel("),
    );
  });
});

describe("dictation helper: every admitted start gets an answer (update gate)", () => {
  it("rejects a toggle start that arrives while the previous take finalizes", () => {
    // Rust registers the take id it sends with a start in the update gate,
    // which only lets go on that take's terminal event. A toggle ignored in
    // silence while finalizing would hold every update back for good.
    const toggle = code(body("func toggle(shortcut: String, takeID: String? = nil)"));
    const finalizing = toggle.indexOf("} else if let takeID {");
    expect(finalizing).toBeGreaterThan(toggle.indexOf("} else if !isFinalizing {"));
    const reject = toggle.slice(finalizing);
    expect(reject).toContain('emit("error", takePayload([');
    expect(reject).toContain('"code": "recording_start_busy"');
    expect(reject).toContain("], takeID: takeID))");
  });
});
