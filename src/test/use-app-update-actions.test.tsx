import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

const updater = vi.hoisted(() => ({
  updatesEnabled: vi.fn(async () => true),
  checkClovyUpdate: vi.fn(),
  relaunchClovy: vi.fn(async () => undefined),
  postponeUpdateInstall: vi.fn(async () => undefined),
}));

vi.mock("../lib/updater", () => updater);

import { useAppUpdateActions } from "../app/use-app-update-actions";
import type { ClovyUpdate } from "../lib/updater";

function stagedUpdate(): ClovyUpdate {
  return {
    version: "0.1.1",
    downloadAndInstall: vi.fn(async () => undefined),
  };
}

async function renderOffered(dictationActive: boolean, update = stagedUpdate()) {
  updater.checkClovyUpdate.mockResolvedValue(update);
  const dictationWorkflowActiveRef = { current: dictationActive };
  const hook = renderHook(() =>
    useAppUpdateActions({
      appBlocked: false,
      recordingActive: false,
      dictationWorkflowActiveRef,
      flushNotes: async () => undefined,
    }),
  );
  await waitFor(() => expect(hook.result.current.updateHub.offer).not.toBeNull());
  return { hook, update, dictationWorkflowActiveRef };
}

afterEach(() => {
  vi.clearAllMocks();
});

describe("useAppUpdateActions", () => {
  it("waits for a dictation that is already running when the owner accepts", async () => {
    const { hook, update } = await renderOffered(true);

    act(() => hook.result.current.updateHub.onInstall());

    expect(update.downloadAndInstall).not.toHaveBeenCalled();
    expect(hook.result.current.updateHub.waitingForCapture).toBe(true);
  });

  it("installs once the dictation it waited for ends", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      const { hook, update, dictationWorkflowActiveRef } = await renderOffered(true);
      act(() => hook.result.current.updateHub.onInstall());

      dictationWorkflowActiveRef.current = false;
      await act(async () => {
        await vi.advanceTimersByTimeAsync(1000);
      });

      expect(update.downloadAndInstall).toHaveBeenCalledTimes(1);
    } finally {
      vi.useRealTimers();
    }
  });

  it("lets the owner retry a restart that the shutdown cancelled", async () => {
    updater.relaunchClovy.mockRejectedValueOnce({
      code: "update_relaunch_cancelled",
      message: "Pending note edits could not be saved, so Clovy did not restart. Try again.",
    });
    const { hook } = await renderOffered(false);
    act(() => hook.result.current.updateHub.onInstall());

    await waitFor(() => expect(updater.relaunchClovy).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(hook.result.current.updateHub.relaunching).toBe(false));
    expect(hook.result.current.updateHub.ready).toBe(true);
    expect(hook.result.current.updateHub.failed).toBe(true);

    act(() => hook.result.current.updateHub.onRelaunch());
    await waitFor(() => expect(updater.relaunchClovy).toHaveBeenCalledTimes(2));
  });

  it("shows the wait card while Rust holds the install for a capture", async () => {
    let rejectInstall: (error: unknown) => void = () => {};
    const update: ClovyUpdate = {
      version: "0.1.1",
      downloadAndInstall: vi.fn(
        (onEvent) =>
          new Promise<void>((_, reject) => {
            onEvent?.({ event: "Finished" });
            onEvent?.({ event: "WaitingForCapture" });
            rejectInstall = reject;
          }),
      ),
    };
    const { hook } = await renderOffered(false, update);
    act(() => hook.result.current.updateHub.onInstall());
    await waitFor(() => expect(hook.result.current.updateHub.waitingForCapture).toBe(true));

    act(() => hook.result.current.updateHub.onPostpone());
    expect(updater.postponeUpdateInstall).toHaveBeenCalledTimes(1);

    await act(async () => {
      rejectInstall({ code: "update_install_postponed", message: "The update was postponed." });
    });
    await waitFor(() => expect(hook.result.current.updateHub.offer).toBeNull());
    expect(hook.result.current.updateHub.waitingForCapture).toBe(false);
    expect(hook.result.current.updateHub.status).toBeNull();
  });
});
