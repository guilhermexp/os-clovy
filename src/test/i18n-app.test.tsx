import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  composerFundingDisabledReason,
  recordingFundingDisabledReason,
  tabMeta,
} from "../app/app-shell";
import { UpdateHub } from "../app/app-effects/update-ui";
import { UP_TO_DATE_STATUS } from "../app/update-decision";
import { createWorkspaceLoader } from "../app/workspace-lazy";
import { t } from "../i18n";
import { applyInterfaceLocale } from "../i18n/locale";
import type { ClovyUpdate } from "../lib/updater";

function renderUpdateHub(overrides: Partial<Parameters<typeof UpdateHub>[0]> = {}) {
  return render(
    <UpdateHub
      readyUpdate={null}
      status={UP_TO_DATE_STATUS}
      failed={false}
      statusLeaving={false}
      checking={false}
      preparing={false}
      relaunching={false}
      progress={null}
      onDismissStatus={() => {}}
      onRelaunch={() => {}}
      {...overrides}
    />,
  );
}

afterEach(() => {
  cleanup();
  applyInterfaceLocale("en");
});

describe("app shell copy in English (default)", () => {
  it("keeps the English tab titles and update status", () => {
    expect(tabMeta({ view: "all-notes" }, [], [], []).title).toBe("All notes");
    expect(composerFundingDisabledReason()).toBe(
      "Add credits to send messages or generate images and videos.",
    );
    renderUpdateHub();
    expect(screen.getByRole("status")).toHaveTextContent("Clovy is up to date.");
    expect(screen.getByRole("button", { name: "Dismiss update status" })).toBeInTheDocument();
  });
});

describe("app shell copy in pt-BR", () => {
  beforeEach(() => applyInterfaceLocale("pt-BR"));

  it("translates tab titles and funding gates at call time", () => {
    expect(tabMeta({ view: "all-notes" }, [], [], []).title).toBe("Todas as notas");
    expect(tabMeta({ view: "home" }, [], [], []).title).toBe("Início");
    expect(tabMeta({ view: "settings" }, [], [], []).title).toBe("Configurações");
    expect(tabMeta({ view: "agent" }, [], [], []).title).toBe("Nova sessão");
    expect(recordingFundingDisabledReason()).toBe(
      "Adicione créditos antes de iniciar uma gravação. Você ainda pode navegar e editar.",
    );
  });

  it("renders the up-to-date sentinel and update controls in Portuguese", () => {
    renderUpdateHub();
    expect(screen.getByRole("status")).toHaveTextContent("O Clovy está atualizado.");
    expect(screen.getByRole("button", { name: "Dispensar status da atualização" })).toBeVisible();
  });

  it("interpolates the version into the relaunch card label", () => {
    renderUpdateHub({
      status: null,
      readyUpdate: { update: {} as ClovyUpdate, version: "1.2.3" },
    });
    expect(
      screen.getByRole("button", { name: "Reiniciar para atualizar para o Clovy 1.2.3" }),
    ).toHaveTextContent("Reiniciar para atualizar");
  });

  it("re-renders the update card when the language changes", () => {
    applyInterfaceLocale("en");
    renderUpdateHub({ preparing: true, status: "Downloading update..." });
    expect(screen.getByRole("button", { name: "Hide update progress" })).toBeInTheDocument();
    act(() => applyInterfaceLocale("pt-BR"));
    expect(
      screen.getByRole("button", { name: "Ocultar progresso da atualização" }),
    ).toBeInTheDocument();
  });

  it("pluralizes app toasts and countdowns", () => {
    expect(t("app.projects.added", { count: 1 })).toBe("1 projeto adicionado");
    expect(t("app.projects.added", { count: 3 })).toBe("3 projetos adicionados");
    expect(t("app.inactivity.countdown", { count: 30 })).toBe(
      "O Clovy vai pausar esta gravação em 30 segundos se você não responder.",
    );
  });

  it("shows the workspace load failure in Portuguese", async () => {
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => undefined);
    const workspace = createWorkspaceLoader(
      async (): Promise<{ Probe: () => null }> => {
        throw new Error("Chunk unavailable");
      },
      (module) => module.Probe,
    );
    render(<workspace.Component />);
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Não foi possível abrir esta visualização",
    );
    expect(screen.getByRole("button", { name: "Tentar de novo" })).toBeInTheDocument();
    consoleError.mockRestore();
  });
});
