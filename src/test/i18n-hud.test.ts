import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { AGENT_SESSION_STATUS_EVENT } from "../lib/agent-events";

type TauriListener = (event: { payload: unknown }) => unknown;

const LOCALE_KEY = "os-clovy:interface-locale";

const mocks = vi.hoisted(() => ({
  listeners: new Map<string, TauriListener>(),
  invoke: vi.fn().mockResolvedValue(undefined),
  listen: vi.fn((event: string, listener: TauriListener) => {
    mocks.listeners.set(event, listener);
    return Promise.resolve(vi.fn());
  }),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: mocks.listen, emit: vi.fn() }));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    hide: vi.fn().mockResolvedValue(undefined),
    startDragging: vi.fn().mockResolvedValue(undefined),
  }),
}));

// The real window markup, so the static chrome is exercised as shipped.
function loadPage(file: string) {
  const html = readFileSync(resolve(process.cwd(), file), "utf8");
  document.body.innerHTML = new DOMParser()
    .parseFromString(html, "text/html")
    .body.innerHTML.replace(/<script[\s\S]*?<\/script>/g, "");
}

async function flush() {
  for (let i = 0; i < 12; i += 1) await Promise.resolve();
}

// Modules are reset per test, so the locale module must be the instance the
// HUD itself imported.
async function switchLocale(locale: "en" | "pt-BR") {
  const { applyInterfaceLocale } = await import("../i18n/locale");
  applyInterfaceLocale(locale);
}

describe("HUD windows in pt-BR", () => {
  beforeEach(async () => {
    vi.resetModules();
    vi.clearAllMocks();
    mocks.listeners.clear();
    mocks.invoke.mockResolvedValue(undefined);
    localStorage.clear();
    const { markOnboardingComplete } = await import("../lib/onboarding");
    markOnboardingComplete();
    localStorage.setItem(LOCALE_KEY, "pt-BR");
  });

  afterEach(() => {
    window.dispatchEvent(new Event("pagehide"));
    localStorage.removeItem(LOCALE_KEY);
  });

  it("renders the meeting prompt in Portuguese and follows a language change", async () => {
    loadPage("hud.html");
    await import("../hud");
    await flush();

    expect(document.documentElement.lang).toBe("pt-BR");
    expect(document.querySelector("#hud-cancel")).toHaveAttribute("aria-label", "Cancelar ditado");
    expect(document.querySelector("#hud-stop")).toHaveAttribute("aria-label", "Parar ditado");

    await mocks.listeners.get("meeting-detection-event")?.({
      payload: JSON.stringify({ type: "meeting_detected", payload: {} }),
    });

    expect(document.querySelector("#hud-meeting-label")).toHaveTextContent("Reunião detectada");
    expect(document.querySelector("#hud-meeting-app")).toHaveTextContent("Microfone em uso");
    expect(document.querySelector("#hud-meeting-start")).toHaveTextContent("Gravar");
    expect(document.querySelector("#hud-meeting-start .hud-meeting-start-icon svg")).toBeTruthy();
    expect(document.querySelector("#hud-meeting-dismiss")).toHaveAttribute(
      "aria-label",
      "Dispensar aviso de reunião",
    );
    expect(document.querySelector("#hud-status")).toHaveTextContent("Reunião detectada");

    await switchLocale("en");

    expect(document.documentElement.lang).toBe("en");
    expect(document.querySelector("#hud-meeting-label")).toHaveTextContent("Meeting detected");
    expect(document.querySelector("#hud-meeting-app")).toHaveTextContent("Microphone in use");
    expect(document.querySelector("#hud-meeting-start")).toHaveTextContent("Record");
    expect(document.querySelector("#hud-status")).toHaveTextContent("Meeting detected");
  });

  it("translates dictation status lines, including interpolation", async () => {
    loadPage("hud.html");
    await import("../hud");
    await flush();

    await mocks.listeners.get("dictation-event")?.({
      payload: JSON.stringify({ type: "paste_target", payload: { app: "Notes" } }),
    });
    expect(document.querySelector("#hud-status")).toHaveTextContent("Colando em Notes");

    await switchLocale("en");
    expect(document.querySelector("#hud-status")).toHaveTextContent("Pasting into Notes");
  });

  it("pluralizes the agent HUD pill and re-renders on a language change", async () => {
    loadPage("agent-hud.html");
    await import("../agent-hud");
    await flush();

    for (const title of ["First task", "Second task"]) {
      window.dispatchEvent(
        new CustomEvent(AGENT_SESSION_STATUS_EVENT, {
          detail: { status: "waitingForUser", title, prompt: title },
        }),
      );
    }
    await flush();

    const pill = document.querySelector("#agent-hud-pill");
    expect(document.querySelector("#agent-hud-pill-label")).toHaveTextContent(
      "2 precisam de resposta",
    );
    expect(pill).toHaveAttribute("aria-label", "Recolher atividade dos agentes");
    expect(document.querySelector(".agent-hud-surface")).toHaveAttribute(
      "aria-label",
      "Atividade dos agentes",
    );
    expect(document.querySelector("#agent-hud-hide")).toHaveTextContent("Ocultar HUD de sessões");

    await switchLocale("en");

    expect(document.querySelector("#agent-hud-pill-label")).toHaveTextContent("2 need input");
    expect(pill).toHaveAttribute("aria-label", "Collapse agent activity");
    expect(document.querySelector("#agent-hud-hide")).toHaveTextContent("Hide sessions HUD");
  });

  it("labels the recording pill and meeting-end prompt in Portuguese", async () => {
    loadPage("meeting-hud.html");
    await import("../meeting-hud");
    await flush();

    expect(document.querySelector("#mhud")).toHaveAttribute(
      "aria-label",
      "Gravando. Clique para abrir o Clovy",
    );
    const stop = document.querySelector("#mhud-end-stop");
    expect(stop).toHaveTextContent("Parar agora");
    expect(stop).toHaveAttribute("aria-label", "Parar a gravação agora");
    expect(document.querySelector("#mhud-end-seconds")).toBeTruthy();
    expect(document.querySelector("#mhud-end-keep")).toHaveTextContent("Continuar gravando");

    await switchLocale("en");
    expect(document.querySelector("#mhud")).toHaveAttribute(
      "aria-label",
      "Recording. Click to open Clovy",
    );
    expect(stop).toHaveTextContent("Stop now");
  });
});
