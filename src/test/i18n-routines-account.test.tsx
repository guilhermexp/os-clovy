import { act, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { AccountGate } from "../components/account/AccountGate";
import { FundingChip, FundingNotice } from "../components/account/FundingNotice";
import { ReferralNudge } from "../components/referral/ReferralNudge";
import { RoutineModePicker } from "../components/routines/RoutineModePicker";
import { formatRunTime } from "../components/routines/RoutineRunList";
import { ROUTINE_TEMPLATES } from "../components/routines/routine-templates";
import { SchedulePicker } from "../components/routines/SchedulePicker";
import { applyInterfaceLocale } from "../i18n/locale";
import { clearMaxGrantWait } from "../lib/max-upgrade";
import type { AccountStatus } from "../lib/tauri";

vi.mock("../lib/tauri", () => ({
  osAccountsCancelLogin: vi.fn(),
  osAccountsChangePlan: vi.fn(),
  osAccountsLogin: vi.fn(),
  osAccountsOpenPortal: vi.fn(),
  osAccountsUpgrade: vi.fn(),
  osAccountsUpgradeSession: vi.fn(),
}));

const freeAccount: AccountStatus = {
  signedIn: true,
  configured: true,
  user: { id: "usr_123", handle: "alex", displayName: "Alex" },
  balance: { credits: 0, usdMillis: 0 },
  subscription: { subscribed: false },
};

describe("routines and account in Portuguese", () => {
  beforeEach(() => {
    clearMaxGrantWait();
    applyInterfaceLocale("pt-BR");
  });
  afterEach(() => applyInterfaceLocale("en"));

  it("translates the funding notice and chip", () => {
    render(<FundingNotice account={freeAccount} onRefresh={vi.fn(async () => freeAccount)} />);
    expect(
      screen.getByText(
        "Seus créditos iniciais acabaram. Faça upgrade para continuar usando o Clovy.",
      ),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Fazer upgrade para o Pro" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Ou vá de Max" })).toBeInTheDocument();
  });

  it("labels the funding chip in Portuguese", () => {
    render(<FundingChip account={freeAccount} onRefresh={vi.fn(async () => freeAccount)} />);
    expect(screen.getByRole("button", { name: "Sem créditos" })).toBeInTheDocument();
  });

  it("renders the sign-in gate with translated terms links", () => {
    render(<AccountGate account={freeAccount} loading={false} onAccountChanged={vi.fn()} />);
    expect(screen.getByText("Boas-vindas ao Clovy")).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Termos" })).toHaveAttribute(
      "href",
      "https://accounts.opensoftware.co/terms",
    );
    expect(screen.getByRole("link", { name: "Política de privacidade" })).toBeInTheDocument();
  });

  it("translates the referral nudge", () => {
    render(<ReferralNudge moment="agent" onInvite={vi.fn()} onDismiss={vi.fn()} />);
    expect(screen.getByText("Dê um mês, ganhe um mês")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Dispensar" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Convidar amigos" })).toBeInTheDocument();
  });

  it("names schedule kinds and weekdays in Portuguese", () => {
    render(<SchedulePicker draft={{ kind: "weekly", day: 1, time: "09:00" }} onChange={vi.fn()} />);
    expect(screen.getByText("Semanalmente")).toBeInTheDocument();
    expect(screen.getByText("Segunda-feira")).toBeInTheDocument();
    expect(screen.getByLabelText("Horário")).toBeInTheDocument();
  });

  it("translates the sandbox picker and starter templates", () => {
    render(<RoutineModePicker unrestricted={false} onChange={vi.fn()} />);
    expect(screen.getByRole("button", { name: "Em sandbox" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Sem restrições" })).toBeInTheDocument();
    expect(ROUTINE_TEMPLATES[0].name).toBe("Resumo da manhã");
    // Prompts are sent to the model and stay English.
    expect(ROUTINE_TEMPLATES[0].prompt).toMatch(/^Put together a short morning brief/);
  });

  it("interpolates relative run times", () => {
    const now = new Date();
    now.setHours(9, 5, 0, 0);
    expect(formatRunTime(now.toISOString())).toMatch(/^hoje às 0?9:05$/);
  });

  it("switches back to English at runtime", () => {
    render(<ReferralNudge moment="agent" onInvite={vi.fn()} onDismiss={vi.fn()} />);
    act(() => applyInterfaceLocale("en"));
    expect(screen.getByText("Give a month, get a month")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Invite friends" })).toBeInTheDocument();
    expect(ROUTINE_TEMPLATES[0].name).toBe("Morning brief");
  });
});
