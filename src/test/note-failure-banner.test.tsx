import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import {
  NoteFailureBanner,
  userFacingFailureMessage,
} from "../components/note-editor/NoteFailureBanner";

describe("userFacingFailureMessage", () => {
  it("turns no-speech provider codes into useful guidance", () => {
    expect(userFacingFailureMessage("Microphone: upstream_provider_failed; no_speech")).toBe(
      "Microphone: No speech detected. Try speaking louder or moving closer to the microphone.",
    );
  });

  it("hides raw JSON parser failures from saved notes", () => {
    expect(userFacingFailureMessage("expected value at line 1 column 1")).toBe(
      "The processing service returned an invalid response.",
    );
    expect(userFacingFailureMessage("Microphone: expected value at line 1 column 1")).toBe(
      "Microphone: The processing service returned an invalid response.",
    );
  });

  it("turns authorization denials into retry guidance", () => {
    expect(userFacingFailureMessage("authorization_denied")).toBe(
      "The service is busy right now. Wait a minute, then retry.",
    );
    expect(userFacingFailureMessage("Microphone: authorization_denied")).toBe(
      "Microphone: The service is busy right now. Wait a minute, then retry.",
    );
  });
});

describe("NoteFailureBanner", () => {
  it("shows only Retry for generic failures and reassures audio is saved", () => {
    render(
      <NoteFailureBanner
        errorMessage="Network unreachable"
        audioPreserved
        onRetry={() => undefined}
      />,
    );
    expect(screen.getByText(/Network unreachable/i)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Upgrade/i })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Retry/i })).toBeEnabled();
    expect(screen.getByText(/Your recording is saved locally/i)).toBeInTheDocument();
  });

  it("shows a friendly message for no-speech transcription failures", () => {
    render(
      <NoteFailureBanner
        errorMessage="Microphone: upstream_provider_failed; no_speech"
        audioPreserved
        onRetry={() => undefined}
      />,
    );

    expect(screen.getByText(/No speech detected/i)).toBeInTheDocument();
    expect(screen.queryByText(/upstream_provider_failed/i)).not.toBeInTheDocument();
  });

  it("shows a billing message for metering provider failures", () => {
    render(
      <NoteFailureBanner
        errorMessage="Microphone: metering_provider_failed"
        audioPreserved
        onRetry={() => undefined}
      />,
    );

    expect(screen.getByText(/Billing is temporarily unavailable/i)).toBeInTheDocument();
    expect(screen.queryByText(/metering_provider_failed/i)).not.toBeInTheDocument();
  });

  it("guards against double-click while a retry is in flight", async () => {
    let resolveRetry: () => void = () => {};
    const onRetry = vi.fn(
      () =>
        new Promise<void>((resolve) => {
          resolveRetry = resolve;
        }),
    );
    render(
      <NoteFailureBanner errorMessage="Network unreachable" audioPreserved onRetry={onRetry} />,
    );

    const retryButton = screen.getByRole("button", { name: /Retry/i });
    await userEvent.click(retryButton);

    // Button disables while the retry is in flight; aria-busy reflects it.
    expect(screen.getByRole("button", { name: /Retry/i })).toBeDisabled();
    expect(onRetry).toHaveBeenCalledTimes(1);

    // A second click while pending must not fire onRetry again.
    await userEvent.click(screen.getByRole("button", { name: /Retry/i }));
    expect(onRetry).toHaveBeenCalledTimes(1);

    // Resolve so the test doesn't hang on cleanup.
    resolveRetry();
  });

  it("disables Retry when no audio is preserved (e.g., recording itself failed)", () => {
    render(
      <NoteFailureBanner
        errorMessage="Recording sources not ready"
        audioPreserved={false}
        onRetry={() => undefined}
      />,
    );
    expect(screen.getByRole("button", { name: /Retry/i })).toBeDisabled();
  });
});
