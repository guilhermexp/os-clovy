import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  emit: vi.fn(async () => undefined),
  listeners: new Map<string, () => void>(),
}));

vi.mock("@tauri-apps/api/event", () => ({
  emit: mocks.emit,
  listen: vi.fn(async (name: string, callback: () => void) => {
    mocks.listeners.set(name, callback);
    return () => mocks.listeners.delete(name);
  }),
}));

import {
  INTERFACE_LOCALE_STORAGE_KEY,
  INTERFACE_LOCALE_TAURI_EVENT,
  initInterfaceLocale,
} from "../i18n/locale";

const flush = () => new Promise((resolve) => setTimeout(resolve, 0));

function localeBroadcasts() {
  return mocks.emit.mock.calls.filter(([name]) => name === INTERFACE_LOCALE_TAURI_EVENT);
}

/**
 * The main window can load before Rust registers its listener (Tauri builds
 * the configured window before `setup`), so the startup broadcast alone may be
 * lost: Rust then asks, and the window answers.
 */
describe("interface language handshake with Rust", () => {
  beforeEach(() => {
    mocks.emit.mockClear();
    mocks.listeners.clear();
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
    window.localStorage.setItem(INTERFACE_LOCALE_STORAGE_KEY, "pt-BR");
  });

  afterEach(() => {
    delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
    window.localStorage.removeItem(INTERFACE_LOCALE_STORAGE_KEY);
  });

  it("answers Rust's request with the current language", async () => {
    initInterfaceLocale();
    await flush();
    await flush();
    const request = mocks.listeners.get("clovy://interface-locale-request");
    expect(request).toBeDefined();
    const before = localeBroadcasts().length;
    request?.();
    await flush();
    expect(localeBroadcasts().length).toBe(before + 1);
    expect(localeBroadcasts().at(-1)?.[1]).toBe("pt-BR");
  });

  it("broadcasts again once it can hear the request", async () => {
    initInterfaceLocale();
    await flush();
    await flush();
    expect(localeBroadcasts().length).toBeGreaterThanOrEqual(2);
    expect(localeBroadcasts().every(([, locale]) => locale === "pt-BR")).toBe(true);
  });
});
