import { describe, expect, it, vi } from "vitest";
const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({
  invoke,
  convertFileSrc: vi.fn(),
}));
import {
  cancelLocalTranscriptionModelInstall,
  installLocalTranscriptionModel,
  removeLocalTranscriptionModel,
} from "../lib/tauri";

describe("local transcription bindings", () => {
  it("qualifies each action by model and sends selection and removal consent separately", async () => {
    invoke.mockResolvedValue(undefined);
    await installLocalTranscriptionModel("phonon-2.5", true);
    expect(invoke).toHaveBeenLastCalledWith("install_local_transcription_model", {
      modelId: "phonon-2.5",
      activate: true,
    });
    await cancelLocalTranscriptionModelInstall("phonon-2.5");
    expect(invoke).toHaveBeenLastCalledWith("cancel_local_transcription_model_install", {
      modelId: "phonon-2.5",
    });
    invoke.mockResolvedValueOnce({ switched_to_groq: true });
    expect(await removeLocalTranscriptionModel("parakeet-ultra", true)).toEqual({
      switched_to_groq: true,
    });
    expect(invoke).toHaveBeenLastCalledWith("remove_local_transcription_model", {
      modelId: "parakeet-ultra",
      confirmActive: true,
    });
  });
});
