import { IconArrowRotateClockwise } from "central-icons/IconArrowRotateClockwise";
import { useState } from "react";
import { t as translate, useT } from "../../i18n";
import { isInsufficientCreditsMessage } from "../../lib/errors";
import { TierMiniCard } from "../account/FundingNotice";
import type { FundingTier } from "../account/FundingNotice";

export type FailureKind = "balance_low" | "generic";

type Props = {
  errorMessage?: string;
  audioPreserved: boolean;
  onRetry: () => void | Promise<void>;
  onTopUp: () => void;
  topUpLabel?: string;
  retryBlockedReason?: string;
  /** The user's current plan; balance failures lead with its tier card so
   * the banner reads as "your card declined", matching the other credits
   * surfaces. */
  tier?: FundingTier;
};

// String match (see isInsufficientCreditsMessage) is intentional and a known
// weakness — the backend currently persists only the error message on the
// note, not the structured code (see commands.rs::finish_recording where
// set_note_status is called with Some(error.message)). When we start storing
// the code we can switch to a strict equality check on the backend billing
// error code.
export function classifyFailure(message?: string): FailureKind {
  return isInsufficientCreditsMessage(message) ? "balance_low" : "generic";
}

export function userFacingFailureMessage(message?: string) {
  if (!message) return undefined;
  return message
    .split("|")
    .map((part) => friendlyFailureSegment(part.trim()))
    .filter(Boolean)
    .join(" | ");
}

function friendlyFailureSegment(message: string) {
  const source = message.match(/^(Microphone|System):\s*/i)?.[1];
  const body = source ? message.replace(/^(Microphone|System):\s*/i, "") : message;
  const normalized = body.toLowerCase();
  let friendly = body;
  if (normalized.includes("no_speech") || normalized.includes("no speech")) {
    friendly = translate("notes.failure.noSpeech");
  } else if (isInvalidClovyResponseMessage(body)) {
    friendly = translate("notes.failure.invalidResponse");
  } else if (normalized.includes("metering_provider_failed")) {
    friendly = translate("notes.failure.billingUnavailable");
  } else if (normalized.includes("upstream_provider_failed")) {
    friendly = translate("notes.failure.providerFailed");
  } else if (normalized.includes("authorization_denied")) {
    friendly = translate("notes.failure.serviceBusy");
  }
  return source
    ? translate("notes.failure.sourcePrefixed", {
        source: failureSourceLabel(source),
        message: friendly,
      })
    : friendly;
}

/** The transcript source a backend failure names ("Microphone: ..."), shown in
 * the interface language; any other casing passes through untouched. */
function failureSourceLabel(source: string) {
  if (source === "Microphone") return translate("notes.editor.source.microphone");
  if (source === "System") return translate("notes.editor.source.system");
  return source;
}

export function isInvalidClovyResponseMessage(message: string) {
  const normalized = message.trim().toLowerCase();
  return (
    normalized.includes("clovy_api_response_invalid") ||
    normalized.includes("processing service returned an invalid response") ||
    /^expected value at line \d+ column \d+$/.test(normalized)
  );
}

export function NoteFailureBanner({
  errorMessage,
  audioPreserved,
  onRetry,
  onTopUp,
  topUpLabel: topUpLabelProp,
  retryBlockedReason,
  tier,
}: Props) {
  const t = useT();
  const topUpLabel = topUpLabelProp ?? t("notes.editor.upgrade");
  const kind = classifyFailure(errorMessage);
  const isBalanceIssue = kind === "balance_low";
  const displayMessage = userFacingFailureMessage(errorMessage);
  const topUpAction = topUpLabel.toLowerCase();
  // Local busy flag so a fast double-click can't fire onRetry twice. The
  // banner unmounts when the note transitions out of `failed` status, so we
  // don't need to reset this state ourselves; the catch covers the case
  // where onRetry rejects and the note stays in `failed`.
  const [retrying, setRetrying] = useState(false);
  // Mirror the settings balance-refresh affordance: each click advances the
  // rotation by a full turn so the arrow sweeps once on press.
  const [spins, setSpins] = useState(0);

  async function handleRetry() {
    if (retrying) return;
    setRetrying(true);
    setSpins((turns) => turns + 1);
    try {
      await onRetry();
    } catch {
      // Parent already surfaces errors; release the gate so the user can try
      // again rather than getting stuck in a frozen spinner.
      setRetrying(false);
    }
  }

  return (
    <aside className="note-failure-banner" role="alert" data-kind={kind}>
      {isBalanceIssue && tier ? <TierMiniCard tier={tier} /> : null}
      <p className="note-failure-message">
        {isBalanceIssue
          ? audioPreserved
            ? t("notes.failure.balanceRanOut", { action: topUpAction })
            : t("notes.failure.balanceTooLow", { action: topUpLabel })
          : (displayMessage ?? t("notes.failure.generic"))}
        {!isBalanceIssue && audioPreserved ? ` ${t("notes.failure.savedCanRetry")}` : null}
        {retryBlockedReason ? ` ${retryBlockedReason}` : null}
      </p>
      <div className="note-failure-actions">
        {isBalanceIssue ? (
          <button type="button" className="btn btn-secondary" onClick={onTopUp} disabled={retrying}>
            {topUpLabel}
          </button>
        ) : null}
        <button
          type="button"
          className="btn btn-ghost"
          onClick={() => void handleRetry()}
          disabled={!audioPreserved || retrying || Boolean(retryBlockedReason)}
          aria-busy={retrying || undefined}
        >
          <IconArrowRotateClockwise
            size={14}
            className="balance-refresh-icon"
            style={{ transform: `rotate(${spins * 360}deg)` }}
          />
          {t("common.retry")}
        </button>
      </div>
    </aside>
  );
}
