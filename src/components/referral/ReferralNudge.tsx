import { IconCrossMedium } from "central-icons/IconCrossMedium";
import { useEffect, useState } from "react";
import { type TFunction, useT } from "../../i18n";
import type { ReferralNudgeMoment } from "../../lib/referral-nudge";
import { ClovyMark } from "../brand/ClovyLogo";

export type { ReferralNudgeMoment };

/**
 * The referral delight nudge: a rare, dismissible invite card that appears at
 * moments Clovy has just delivered value (5th meeting note, first successful
 * agent task, 25th dictation, positive feedback). Deliberately a small cousin
 * of the referral dialog's hero — the same terracotta gradient, grain, and
 * Clovy mark — so clicking through into that dialog feels continuous.
 *
 * The card never auto-dismisses (a dictation lands while Clovy is backgrounded;
 * the card waits to be found) and never steals focus. Trigger moments and
 * frequency caps live with the caller, not here.
 */

/** Fired on click-through; the sidebar owns the referral dialog and listens. */
export const OPEN_REFERRAL_DIALOG_EVENT = "clovy:open-referral-dialog";

function momentCopy(t: TFunction, moment: ReferralNudgeMoment): { title: string; body: string } {
  switch (moment) {
    case "meetings":
      return {
        title: t("account.referral.meetingsTitle"),
        body: t("account.referral.meetingsBody"),
      };
    case "agent":
      return { title: t("account.referral.agentTitle"), body: t("account.referral.agentBody") };
    case "dictation":
      return {
        title: t("account.referral.dictationTitle"),
        body: t("account.referral.dictationBody"),
      };
    case "feedback":
      return {
        title: t("account.referral.feedbackTitle"),
        body: t("account.referral.feedbackBody"),
      };
  }
}

/** Matches the card's --t-med exit transition. */
const EXIT_MS = 160;

export function ReferralNudge({
  moment,
  onInvite,
  onDismiss,
}: {
  moment: ReferralNudgeMoment;
  onInvite: () => void;
  onDismiss: () => void;
}) {
  // Dismiss plays a short fade-down before the caller unmounts the card;
  // click-through is immediate (the opening dialog covers the exit).
  const t = useT();
  const [leaving, setLeaving] = useState(false);
  const copy = momentCopy(t, moment);

  function dismiss() {
    if (leaving) return;
    setLeaving(true);
    window.setTimeout(onDismiss, EXIT_MS);
  }

  // Escape clears the card from anywhere — it must never demand a mouse trip
  // to its X. Dialogs keep first claim on the key: while one is open (it sits
  // above the card anyway), Escape belongs to it, not to us.
  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      if (event.key !== "Escape") return;
      if (document.querySelector('[role="dialog"]')) return;
      dismiss();
    }

    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  });

  return (
    <aside className="referral-nudge" role="status" data-leaving={leaving || undefined}>
      <div className="referral-nudge-hero">
        {/* The same flat Clovy mark used at first sign-in, reprised at
            gift scale as the thing the user is invited to hand to a friend. */}
        <span className="referral-nudge-brand-mark" aria-hidden>
          <ClovyMark variant="mono" />
        </span>
        <p className="referral-nudge-title">{copy.title}</p>
      </div>
      <button
        type="button"
        className="referral-nudge-dismiss"
        aria-label={t("account.referral.dismiss")}
        onClick={dismiss}
      >
        <IconCrossMedium size={14} />
      </button>
      <div className="referral-nudge-body">
        <p className="referral-nudge-copy">{copy.body}</p>
      </div>
      <div className="referral-nudge-footer">
        <button
          type="button"
          className="primary-action primary-solid"
          onClick={onInvite}
          disabled={leaving}
        >
          {t("account.referral.invite")}
        </button>
      </div>
    </aside>
  );
}
