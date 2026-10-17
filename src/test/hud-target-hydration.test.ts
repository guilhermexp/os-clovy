import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import markup from "../../hud.html?raw";

type Listener = (event: { payload: unknown }) => unknown;
const mocks = vi.hoisted(() => ({
  listeners: new Map<string, Listener>(),
  invoke: vi.fn(),
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn((name: string, listener: Listener) => {
    mocks.listeners.set(name, listener);
    return Promise.resolve(vi.fn());
  }),
}));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    hide: vi.fn().mockResolvedValue(undefined),
    startDragging: vi.fn(),
  }),
}));

const a = { app: "Finder", bundleIdentifier: "com.apple.finder", processIdentifier: 100 };
const b = { app: "Terminal", bundleIdentifier: "com.apple.Terminal", processIdentifier: 200 };
const c = { app: "Other", bundleIdentifier: "test.other", processIdentifier: 300 };
const iconB = "data:image/png;base64,dGVybWluYWw=";
let resolveSnapshot: (value: string) => void;

async function flush() {
  for (let i = 0; i < 12; i += 1) await Promise.resolve();
}

async function emit(type: string, payload: Record<string, unknown> = {}) {
  await mocks.listeners.get("dictation-event")?.({
    payload: { type, payload: { takeId: "hydration", ...payload } },
  });
  await flush();
}

function expectDestination(app?: string) {
  const label = app ? `Dictation destination: ${app}` : "Dictation destination unidentified";
  expect(document.querySelector("#hud-target")).toHaveAttribute("title", label);
  expect(document.querySelector("#hud-target")).toHaveAttribute("aria-label", label);
}

describe("dictation target hydration after a missed stop event", () => {
  beforeEach(async () => {
    vi.resetModules();
    vi.clearAllMocks();
    mocks.listeners.clear();
    localStorage.clear();
    document.body.innerHTML = new DOMParser().parseFromString(markup, "text/html").body.innerHTML;
    vi.stubGlobal("matchMedia", () => ({ matches: true }));
    mocks.invoke.mockImplementation((command, args) => {
      if (command === "latest_dictation_event") {
        return new Promise<string>((resolve) => {
          resolveSnapshot = resolve;
        });
      }
      return Promise.resolve(
        command === "activity_app_icon" && args.request.bundleId === b.bundleIdentifier
          ? iconB
          : undefined,
      );
    });
    await import("../hud");
    await flush();
  });

  afterEach(() => {
    window.dispatchEvent(new Event("pagehide"));
    vi.unstubAllGlobals();
  });

  it("hydrates authoritative B from paste_target after live final_transcript supersedes the snapshot", async () => {
    await emit("final_transcript", { text: "Synthetic transcript" });
    resolveSnapshot(
      JSON.stringify({
        type: "finalizing_transcript",
        payload: { takeId: "hydration", target: b },
      }),
    );
    await flush();
    expect(document.querySelector("#hud")).toHaveAttribute("data-state", "pasting");
    expectDestination();
    await emit("focus_target", { target: c });
    expectDestination();
    await emit("paste_target", { app: "Terminal", target: b });
    expectDestination("Terminal");
    expect(document.querySelector("#hud-target img")).toHaveAttribute("src", iconB);
    expect(document.querySelector("#hud-target img")).not.toHaveAttribute("hidden");
    await emit("focus_target", { target: c });
    expectDestination("Terminal");
  });

  it("replaces an unconfirmed candidate only with the authoritative paste destination", async () => {
    await emit("listening_started", { target: a });
    await emit("final_transcript");
    await emit("focus_target", { target: c });
    expectDestination("Finder");
    await emit("paste_target", { app: "Terminal", target: b });
    expectDestination("Terminal");
  });

  it("preserves known pinned B against focus during processing and conflicting paste metadata", async () => {
    await emit("listening_started", { target: a });
    await emit("finalizing_transcript", { target: b });
    expect(document.querySelector("#hud")).toHaveAttribute("data-state", "transcribing");
    await emit("focus_target", { target: c });
    expectDestination("Terminal");
    await emit("final_transcript");
    await emit("paste_target", { app: "Other", target: c });
    await emit("focus_target", { target: c });
    expectDestination("Terminal");
  });

  it.each([b, null])("preserves an explicit final_transcript target: %j", async (target) => {
    await emit("final_transcript", { target });
    await emit("paste_target", { app: "Other", target: c });
    expectDestination(target?.app);
  });

  it("says the paste skipped cleanup when cleanup failed for this take", async () => {
    resolveSnapshot("");
    await emit("listening_started", { target: a });
    await emit("finalizing_transcript", { target: b });
    await mocks.listeners.get("dictation-event")?.({
      payload: {
        type: "dictation_cleanup_skipped",
        payload: { code: "dictation_cleanup_timeout" },
      },
    });
    await flush();
    await emit("paste_target", { app: "Terminal", target: b });
    expect(document.querySelector("#hud")?.textContent).toContain(
      "Pasting into Terminal without cleanup",
    );
    await emit("paste_completed");
    await emit("listening_started", { takeId: "next", target: a });
    await emit("paste_target", { takeId: "next", app: "Terminal", target: b });
    expect(document.querySelector("#hud")?.textContent).not.toContain("without cleanup");
  });
});
