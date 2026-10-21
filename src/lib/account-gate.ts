import type { AccountStatus } from "./tauri";

// Single source of truth for whether an action that depends on OS Accounts
// should be blocked behind the sign-in prompt. Keep this pure — it's called
// from App.tsx and from tests.
export function shouldBlockOnSignIn(account: AccountStatus): boolean {
  return !account.signedIn;
}
