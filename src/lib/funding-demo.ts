// Dev-only console driver for the out-of-credits surfaces (FundingNotice
// above the composers, FundingChip in the sidebar footer).
//
//   window.__fundingDemo("free")     starter credits used up -> subscribe
//   window.__fundingDemo("pro")      depleted Pro -> in-place Max upgrade
//   window.__fundingDemo("max")      depleted Max -> top up via portal
//   window.__fundingDemo("billing")  past-due subscription -> manage billing
//   window.__fundingDemo("off")      back to the real account snapshot
//
// The surfaces derive everything from the account snapshot, so the driver
// swaps a synthetic snapshot into App's funding override state; the real
// account state is untouched and comes back on "off". Never bundled in
// production: App gates the dynamic import on import.meta.env.DEV.

import type { AccountStatus } from "./tauri";

export type FundingDemoBranch = "free" | "pro" | "max" | "billing";

export type FundingDemoApi = {
  /** Remove the window hook and restore the real account snapshot. */
  dispose: () => void;
};

const HELP = [
  "Out-of-credits surfaces demo:",
  '  __fundingDemo("free")     starter credits used up (subscribe prompt)',
  '  __fundingDemo("pro")      depleted Pro (in-place Max upgrade + confirm)',
  '  __fundingDemo("max")      depleted Max (top up via the account portal)',
  '  __fundingDemo("billing")  past-due subscription (manage billing)',
  '  __fundingDemo("off")      back to the real account',
  "",
  "Shows the composer notice, sidebar chip, and note-editor notice on any view. Dev only.",
].join("\n");

const BASE: Pick<AccountStatus, "signedIn" | "configured" | "user"> = {
  signedIn: true,
  configured: true,
  user: { id: "usr_demo", handle: "demo" },
};

const DEMO_ACCOUNTS: Record<FundingDemoBranch, AccountStatus> = {
  free: {
    ...BASE,
    subscription: { subscribed: false },
    balance: { credits: 0, usdMillis: 0 },
  },
  pro: {
    ...BASE,
    subscription: { subscribed: true, status: "active", plan: "pro" },
    balance: { credits: -3, usdMillis: 0 },
  },
  max: {
    ...BASE,
    subscription: { subscribed: true, status: "active", plan: "max" },
    balance: { credits: -3, usdMillis: 0 },
  },
  billing: {
    ...BASE,
    subscription: { subscribed: true, status: "past_due", plan: "pro" },
    balance: { credits: 0, usdMillis: 0 },
  },
};

export function registerFundingDemo({
  setOverride,
}: {
  setOverride: (account: AccountStatus | null) => void;
}): FundingDemoApi {
  function drive(branch?: FundingDemoBranch | "off") {
    if (branch === undefined) return HELP;
    if (branch === "off") {
      setOverride(null);
      return "funding demo off";
    }
    const preset = DEMO_ACCOUNTS[branch];
    if (!preset) return HELP;
    setOverride(preset);
    return `funding demo: ${branch}`;
  }

  (window as unknown as { __fundingDemo?: typeof drive }).__fundingDemo = drive;
  return {
    dispose: () => {
      delete (window as unknown as { __fundingDemo?: typeof drive }).__fundingDemo;
      setOverride(null);
    },
  };
}
