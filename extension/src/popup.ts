// Popup: renders the pairing state the background worker holds. Copy follows
// the repo rules: sentence case, no em dashes, no all caps.

import { browserLocale, type ExtensionMessageKey, tx } from "./i18n";
import type { PairingState } from "./pairing";

type PopupCopy = { title: string; detail: string; retry: boolean };
type ShareState = "available" | "pending" | "shared" | "unavailable";
type ShareResponse = {
  success: boolean;
  state?: ShareState;
  shareId?: string;
  message?: string;
};

let pairingState: PairingState | undefined;
let activeShareId: string | undefined;

type CopyKeys = { title: ExtensionMessageKey; detail: ExtensionMessageKey; retry: boolean };

const copyKeys: Record<Exclude<PairingState["status"], "incompatible">, CopyKeys> = {
  disconnected: { title: "notConnectedTitle", detail: "notConnectedDetail", retry: true },
  connecting: { title: "connectingTitle", detail: "connectingDetail", retry: false },
  handshaking: { title: "connectingTitle", detail: "handshakingDetail", retry: false },
  paired: { title: "pairedTitle", detail: "pairedDetail", retry: false },
  unreachable: { title: "unreachableTitle", detail: "unreachableDetail", retry: true },
};

function copyFor(status: Exclude<PairingState["status"], "incompatible">): PopupCopy {
  const keys = copyKeys[status];
  return { title: tx(keys.title), detail: tx(keys.detail), retry: keys.retry };
}

function incompatibleCopy(state: Extract<PairingState, { status: "incompatible" }>): PopupCopy {
  const detail =
    state.remedy === "updateApp"
      ? tx("updateApp")
      : state.remedy === "updateExtension"
        ? tx("updateExtension")
        : tx("updateBoth");
  return { title: tx("updateRequiredTitle"), detail, retry: true };
}

/** Static popup.html copy is authored in English; localize it once on load. */
function localizeStaticCopy() {
  document.documentElement.lang = browserLocale();
  const set = (id: string, key: ExtensionMessageKey) => {
    const element = document.getElementById(id);
    if (element) element.textContent = tx(key);
  };
  set("detail", "checking");
  set("retry", "tryAgain");
  set("share-heading", "shareHeading");
  set("share-detail", "shareIntro");
  set("share", "shareButton");
  set("revoke-share", "stopSharing");
}

function render(state: PairingState) {
  const dot = document.getElementById("dot");
  const title = document.getElementById("title");
  const detail = document.getElementById("detail");
  const retry = document.getElementById("retry") as HTMLButtonElement | null;
  if (!dot || !title || !detail || !retry) return;
  const entry = state.status === "incompatible" ? incompatibleCopy(state) : copyFor(state.status);
  dot.dataset.status = state.status;
  title.textContent = entry.title;
  detail.textContent = entry.detail;
  retry.hidden = !entry.retry;
  pairingState = state;
  void refreshShare();
}

async function activeTabId(): Promise<number | undefined> {
  const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
  return tab?.id;
}

function renderShare(state: ShareState, detail?: string) {
  const section = document.getElementById("tab-share");
  const shareDetail = document.getElementById("share-detail");
  const share = document.getElementById("share") as HTMLButtonElement | null;
  const revoke = document.getElementById("revoke-share") as HTMLButtonElement | null;
  if (!section || !shareDetail || !share || !revoke) return;
  section.hidden = pairingState?.status !== "paired";
  if (section.hidden) return;

  shareDetail.textContent =
    detail ??
    (state === "shared"
      ? tx("tabShared")
      : state === "unavailable"
        ? tx("tabOwned")
        : state === "pending"
          ? tx("shareCode", { code: activeShareId ?? tx("preparing") })
          : tx("shareIntro"));
  share.hidden = state === "shared" || state === "unavailable";
  share.textContent = state === "pending" ? tx("copyShareCode") : tx("shareButton");
  revoke.hidden = state === "available" || state === "unavailable";
  revoke.textContent = state === "pending" ? tx("cancelShare") : tx("stopSharing");
}

async function refreshShare() {
  if (pairingState?.status !== "paired") {
    renderShare("available");
    return;
  }
  const tabId = await activeTabId();
  if (tabId === undefined) {
    renderShare("available", tx("openTabFirst"));
    return;
  }
  const response = (await chrome.runtime.sendMessage({
    type: "getTabShareState",
    tabId,
  })) as ShareResponse | undefined;
  if (!response?.success || !response.state) {
    renderShare("available", response?.message ?? tx("tabCannotBeShared"));
    return;
  }
  activeShareId = response.shareId;
  renderShare(response.state);
}

async function copyShareCode(shareId: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(shareId);
    return true;
  } catch {
    return false;
  }
}

async function refresh(reconnect = false) {
  const state = (await chrome.runtime.sendMessage({
    type: reconnect ? "reconnect" : "getPairingState",
  })) as PairingState | undefined;
  if (state) render(state);
}

document.getElementById("retry")?.addEventListener("click", () => {
  void refresh(true);
  // The handshake settles in the background; poll briefly so the popup
  // reflects the outcome without a manual reopen.
  setTimeout(() => void refresh(), 500);
  setTimeout(() => void refresh(), 1500);
});

document.getElementById("share")?.addEventListener("click", async () => {
  const tabId = await activeTabId();
  if (tabId === undefined) {
    renderShare("available", tx("openTabFirst"));
    return;
  }
  let shareId = activeShareId;
  if (!shareId) {
    const response = (await chrome.runtime.sendMessage({ type: "shareTab", tabId })) as
      | ShareResponse
      | undefined;
    if (!response?.success || !response.shareId) {
      renderShare("available", response?.message ?? tx("tabCouldNotBeShared"));
      return;
    }
    shareId = response.shareId;
    activeShareId = shareId;
  }
  const copied = await copyShareCode(shareId);
  renderShare("pending", copied ? tx("shareCodeCopied") : tx("shareCode", { code: shareId }));
});

document.getElementById("revoke-share")?.addEventListener("click", async () => {
  const tabId = await activeTabId();
  if (tabId === undefined) return;
  await chrome.runtime.sendMessage({ type: "revokeTabShare", tabId });
  activeShareId = undefined;
  await refreshShare();
});

localizeStaticCopy();
void refresh();
