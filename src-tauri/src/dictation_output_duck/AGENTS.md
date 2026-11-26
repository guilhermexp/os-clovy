# Dictation output volume

- `../dictation_output_duck.rs` owns the serial executor, helper generations,
  takes, settings epochs and bounded confirmation receipts.
- `policy.rs` owns snapshots, relative attenuation and restoration decisions.
- `macos.rs` adapts CoreAudio HAL: default media output, UID, writable channels
  and property listeners. HAL work runs only on the executor; callbacks enqueue.
- `tests.rs` exercises the real policy and executor with a deterministic backend.

This is independent of meeting microphone ducking. Never change microphone
capture samples, pause players, unmute output or move the user's output device.
Respect a manual change; a successful SET does not confirm its effect. HAL does
not identify the writer of a notification or predict every quantized readback.
Ambiguous attenuation/restoration must return an unconfirmed result and warning,
never an arbitrary volume increase or a successful restoration receipt.

Keep pending-device acquisition distinct from a route retired by a manual
override or lack of support. A previous restore callback can resume a waiting
take only while that take remains current and enabled.

Native tests: `cargo test --manifest-path src-tauri/Cargo.toml --lib dictation_output_duck`
on the authorized macOS CI executor. Portable harness results do not prove HAL
hardware behavior or replace the full desktop Clippy/Cargo jobs.
Contract: [dictation specification](../../../openspec/specs/dictation/spec.md).
