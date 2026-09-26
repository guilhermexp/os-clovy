import { IconCheckmark2Small } from "central-icons/IconCheckmark2Small";
import { IconCrossMedium } from "central-icons/IconCrossMedium";
import { IconLightBulbSimple } from "central-icons/IconLightBulbSimple";
import { useId, useState } from "react";
import { type TFunction, useT } from "../../../i18n";
import type { AgentApprovalChoice, AgentChatPart } from "../../../lib/agent-chat-runtime";
import { explainAgentApproval, type PendingBrowserApproval } from "../../../lib/tauri";
import { Spinner } from "../../ui/Spinner";
import {
  ApproveSplitButton,
  CollapsibleActionCard,
  ResolvedActionRow,
} from "./ActionCardPrimitives";

export function ClarifyPart({
  onClarify,
  part,
  submitting,
}: {
  onClarify: (part: Extract<AgentChatPart, { type: "clarify" }>, answer: string) => void;
  part: Extract<AgentChatPart, { type: "clarify" }>;
  submitting?: string;
}) {
  const t = useT();
  const [typing, setTyping] = useState(part.choices.length === 0);
  const [draft, setDraft] = useState("");
  const disabled = part.status !== "pending" || submitting !== undefined;

  // Resolved clarify collapses to a quiet receipt row: "Answered" (or "Skipped")
  // plus the question, expandable to the full question and answer.
  if (part.status !== "pending") {
    const answered = Boolean(part.answer?.trim());
    return (
      <ResolvedActionRow
        label={answered ? t("chat.clarify.answered") : t("chat.clarify.skipped")}
        detail={part.question}
      >
        <p>{part.question}</p>
        {answered ? <p className="agent-clarify-answer">{part.answer}</p> : null}
      </ResolvedActionRow>
    );
  }

  return (
    <article className="agent-clarify-card" data-status={part.status}>
      <div>
        <div className="agent-tool-title">
          <span>{t("chat.clarify.title")}</span>
        </div>
        <p className="agent-clarify-question">{part.question}</p>
        {part.status === "pending" ? (
          <>
            {!typing && part.choices.length ? (
              <div className="agent-clarify-choices">
                {part.choices.map((choice, index) => (
                  <button
                    type="button"
                    key={`${index}:${choice}`}
                    disabled={disabled}
                    onClick={() => onClarify(part, choice)}
                  >
                    <span>{index + 1}</span>
                    {choice}
                  </button>
                ))}
                <button
                  type="button"
                  disabled={submitting !== undefined}
                  onClick={() => setTyping(true)}
                >
                  <span>+</span>
                  {t("chat.clarify.other")}
                </button>
              </div>
            ) : null}
            {typing || !part.choices.length ? (
              <form
                className="agent-clarify-form"
                onSubmit={(event) => {
                  event.preventDefault();
                  const answer = draft.trim();
                  if (answer) onClarify(part, answer);
                }}
              >
                <textarea
                  className="dialog-textarea agent-clarify-textarea"
                  value={draft}
                  disabled={disabled}
                  rows={3}
                  placeholder={t("chat.clarify.placeholder")}
                  onChange={(event) => setDraft(event.currentTarget.value)}
                />
                <div>
                  {part.choices.length ? (
                    <button
                      type="button"
                      className="btn btn-ghost"
                      disabled={submitting !== undefined}
                      onClick={() => {
                        setDraft("");
                        setTyping(false);
                      }}
                    >
                      {t("common.back")}
                    </button>
                  ) : null}
                  <button
                    type="button"
                    className="btn btn-ghost"
                    disabled={disabled}
                    onClick={() => onClarify(part, "")}
                  >
                    {t("common.skip")}
                  </button>
                  <button
                    type="submit"
                    className="btn btn-secondary"
                    disabled={disabled || !draft.trim()}
                  >
                    {submitting !== undefined ? t("chat.clarify.sending") : t("chat.clarify.send")}
                  </button>
                </div>
              </form>
            ) : null}
          </>
        ) : null}
      </div>
    </article>
  );
}

export type AgentCliAccessCardProps = {
  /** undefined while the stored setting is still loading. */
  enabled?: boolean;
  submitting: boolean;
  onEnable: () => void;
};

type DismissibleAccessCardProps = {
  /** Controlled by the turn row when its owner needs to mirror card presence. */
  dismissed?: boolean;
  onDismiss?: () => void;
};

/** Clovy asked to enable "Agent CLI access" via the literal token its soul
 * teaches ([REQUEST:AGENT_CLI_ACCESS]). The agent can never flip the setting
 * itself — the flag file sits outside every sandbox write root — so this
 * card is the one-click, user-approved path. Resolution is derived from the
 * live setting rather than stored per message: a revisited transcript shows
 * "Enabled" once the grant is on, and re-offers the choice while it is off.
 * Mirrors the approval card chrome. */
export function AgentCliAccessCard({
  cliAccess,
  dismissed: controlledDismissed,
  onDismiss,
}: { cliAccess?: AgentCliAccessCardProps } & DismissibleAccessCardProps) {
  const t = useT();
  const [locallyDismissed, setLocallyDismissed] = useState(false);
  const dismissed = controlledDismissed ?? locallyDismissed;
  const enabled = cliAccess?.enabled === true;
  const resolved = enabled || dismissed;
  const busy = Boolean(cliAccess?.submitting);

  const description = <p>{t("chat.cliAccess.description")}</p>;

  // Resolved collapses to a quiet receipt row, expandable to the description.
  if (resolved) {
    return (
      <ResolvedActionRow
        denied={!enabled}
        label={enabled ? t("chat.cliAccess.enabled") : t("chat.access.notNow")}
      >
        {description}
      </ResolvedActionRow>
    );
  }

  return (
    <article className="agent-approval-card" data-status="pending">
      <div>
        <div className="agent-tool-title">
          <span>{t("chat.cliAccess.requested")}</span>
        </div>
        {description}
        <div className="agent-approval-actions">
          <button
            type="button"
            className="btn btn-secondary"
            disabled={busy || !cliAccess || cliAccess.enabled === undefined}
            onClick={() => cliAccess?.onEnable()}
          >
            {busy ? t("chat.access.enabling") : t("chat.cliAccess.enable")}
          </button>
          <button
            type="button"
            className="btn btn-ghost agent-approval-deny"
            disabled={busy}
            onClick={() => {
              if (controlledDismissed === undefined) setLocallyDismissed(true);
              onDismiss?.();
            }}
          >
            {t("chat.access.notNow")}
          </button>
        </div>
      </div>
    </article>
  );
}

export type AgentBrowserAccessCardProps = {
  /** undefined while the stored grant is still loading. */
  enabled?: boolean;
  submitting: boolean;
  onEnable: () => void;
};

export function BrowserApprovalCard({
  approval,
  submitting,
  onRespond,
}: {
  approval: PendingBrowserApproval;
  submitting: boolean;
  onRespond: (approve: boolean, allowSite: boolean) => void;
}) {
  const t = useT();
  const action = approval.action.charAt(0).toUpperCase() + approval.action.slice(1);
  return (
    <article className="agent-approval-card" data-status="pending">
      <div className="agent-tool-title">
        <span>{t("chat.browserApproval.title")}</span>
      </div>
      <p>
        {t("chat.browserApproval.site", { site: approval.site })}
        {"\n"}
        {t("chat.browserApproval.action", { action })}
        {"\n"}
        {t("chat.browserApproval.element", { element: approval.elementLabel })}
      </p>
      <div className="agent-approval-actions">
        <button
          type="button"
          className="btn btn-secondary"
          disabled={submitting}
          onClick={() => onRespond(true, false)}
        >
          {t("chat.approval.approve")}
        </button>
        <button
          type="button"
          className="btn btn-ghost"
          disabled={submitting}
          onClick={() => onRespond(true, true)}
        >
          {t("chat.browserApproval.approveSite")}
        </button>
        <button
          type="button"
          className="btn btn-ghost agent-approval-deny"
          disabled={submitting}
          onClick={() => onRespond(false, false)}
        >
          {t("chat.browserApproval.decline")}
        </button>
      </div>
    </article>
  );
}

/** Clovy asked to enable Browser use via the literal token its soul teaches
 * ([REQUEST:BROWSER_ACCESS]). The agent can never flip the setting itself —
 * the Browser access grant is a flag file outside every sandbox write root —
 * so this card is the one-click, user-approved path. Resolution is derived
 * from the live grant rather than stored per message, exactly like the Agent
 * CLI access card above: a revisited transcript shows "Enabled" once the
 * grant is on, and re-offers the choice while it is off. */
export function AgentBrowserAccessCard({
  browserAccess,
  dismissed: controlledDismissed,
  onDismiss,
}: { browserAccess?: AgentBrowserAccessCardProps } & DismissibleAccessCardProps) {
  const t = useT();
  const [locallyDismissed, setLocallyDismissed] = useState(false);
  const dismissed = controlledDismissed ?? locallyDismissed;
  const enabled = browserAccess?.enabled === true;
  const resolved = enabled || dismissed;
  const busy = Boolean(browserAccess?.submitting);

  const description = <p>{t("chat.browserAccess.description")}</p>;

  // Resolved collapses to a quiet receipt row, expandable to the description.
  if (resolved) {
    return (
      <ResolvedActionRow
        denied={!enabled}
        label={enabled ? t("chat.browserAccess.enabled") : t("chat.access.notNow")}
      >
        {description}
      </ResolvedActionRow>
    );
  }

  return (
    <article className="agent-approval-card" data-status="pending">
      <div>
        <div className="agent-tool-title">
          <span>{t("chat.browserAccess.requested")}</span>
        </div>
        {description}
        <div className="agent-approval-actions">
          <button
            type="button"
            className="btn btn-secondary"
            disabled={busy || !browserAccess || browserAccess.enabled === undefined}
            onClick={() => browserAccess?.onEnable()}
          >
            {busy ? t("chat.access.enabling") : t("chat.browserAccess.enable")}
          </button>
          <button
            type="button"
            className="btn btn-ghost agent-approval-deny"
            disabled={busy}
            onClick={() => {
              if (controlledDismissed === undefined) setLocallyDismissed(true);
              onDismiss?.();
            }}
          >
            {t("chat.access.notNow")}
          </button>
        </div>
      </div>
    </article>
  );
}

export function ApprovalPart({
  onApproval,
  part,
  submitting,
}: {
  onApproval: (
    part: Extract<AgentChatPart, { type: "approval" }>,
    choice: AgentApprovalChoice,
  ) => void;
  part: Extract<AgentChatPart, { type: "approval" }>;
  submitting?: AgentApprovalChoice;
}) {
  const t = useT();
  const disabled = Boolean(submitting) || part.status !== "pending";
  const activeChoice = part.choice ?? submitting;
  // A card that has actually resolved collapses to a receipt row. A submission
  // still in flight (submitting set, status pending) keeps the card so the
  // in-progress line ("Approving once") stays visible until it resolves.
  const resolved = part.status !== "pending";
  const showResult = resolved || activeChoice !== undefined;
  // The whole card is compact by default; expanding reveals the full body.
  const [expanded, setExpanded] = useState(false);
  const [explainOpen, setExplainOpen] = useState(false);
  // "Explain first" asks the generation model what this specific request
  // would do — the request stays parked, nothing is approved by asking.
  // The answer is cached for the card's lifetime; an error retries on the
  // next open and falls back to static copy meanwhile.
  const [explanation, setExplanation] = useState<string>();
  const [explainState, setExplainState] = useState<"idle" | "loading" | "ready" | "error">("idle");
  const explanationId = useId();

  function toggleExplain() {
    const nextOpen = !explainOpen;
    setExplainOpen(nextOpen);
    // Opening the explanation auto-expands the card so the panel has room.
    if (nextOpen) setExpanded(true);
    if (!nextOpen || explainState === "loading" || explainState === "ready") {
      return;
    }
    setExplainState("loading");
    explainAgentApproval({
      description: part.description,
      command: part.command || undefined,
    })
      .then((response) => {
        setExplanation(response.explanation);
        setExplainState("ready");
      })
      .catch(() => {
        setExplainState("error");
      });
  }

  // Resolved collapses to a quiet receipt row: the outcome label plus the
  // command (or description) truncated to one line, expandable to the full
  // description and command — no action buttons.
  if (resolved) {
    if (part.status === "expired") {
      const outcomeUnconfirmed = part.retiredReason === "unconfirmed";
      return (
        <ResolvedActionRow
          denied={!outcomeUnconfirmed}
          unknown={outcomeUnconfirmed}
          label={
            outcomeUnconfirmed ? t("chat.approval.outcomeUnknown") : t("chat.approval.expired")
          }
          detail={
            part.command ? (
              <span className="agent-resolved-mono">{part.command}</span>
            ) : (
              part.description
            )
          }
        >
          {outcomeUnconfirmed ? (
            <p>{t("chat.approval.unconfirmedBody")}</p>
          ) : (
            <p>{t("chat.approval.expiredBody")}</p>
          )}
          {part.command ? <pre>{part.command}</pre> : null}
        </ResolvedActionRow>
      );
    }
    return (
      <ResolvedActionRow
        denied={activeChoice === "deny"}
        label={approvalChoiceLabel(t, activeChoice)}
        detail={
          part.command ? (
            <span className="agent-resolved-mono">{part.command}</span>
          ) : (
            part.description
          )
        }
      >
        <p>{part.description}</p>
        {part.command ? <pre>{part.command}</pre> : null}
      </ResolvedActionRow>
    );
  }

  const footer = showResult ? (
    // Submission in flight (status still pending): the in-progress line stays
    // in the card until the request actually resolves.
    <p className="agent-approval-result" data-choice={activeChoice}>
      {activeChoice === "deny" ? <IconCrossMedium size={14} /> : <IconCheckmark2Small size={14} />}
      {approvalChoiceLabel(t, activeChoice, submitting !== undefined)}
    </p>
  ) : (
    // Compact footer: a split "Approve" (approves once, caret opens the scope
    // menu) and a quiet "Deny" anchor the row; "Explain first" demotes to a
    // plain text-level button pushed to the right edge.
    <div className="agent-approval-actions">
      <ApproveSplitButton
        disabled={disabled}
        allowPermanent={part.allowPermanent}
        onChoice={(choice) => onApproval(part, choice)}
      />
      <button
        type="button"
        className="btn btn-ghost agent-approval-deny"
        disabled={disabled}
        onClick={() => onApproval(part, "deny")}
      >
        {t("chat.approval.deny")}
      </button>
      <button
        type="button"
        className="btn btn-ghost agent-approval-explain"
        aria-expanded={explainOpen}
        // Only advertise the panel while it's actually in the DOM (the body
        // renders only when the card is expanded and the explanation is open).
        aria-controls={explainOpen ? explanationId : undefined}
        disabled={disabled}
        onClick={toggleExplain}
      >
        <IconLightBulbSimple size={14} aria-hidden />
        {explainOpen ? t("chat.approval.hideExplanation") : t("chat.approval.explainFirst")}
      </button>
    </div>
  );

  return (
    <CollapsibleActionCard
      title={t("chat.approval.required")}
      description={part.description}
      command={part.command ? <pre>{part.command}</pre> : null}
      // The command is always shown; the only expandable body is the optional
      // explanation, which its own "Explain first" button toggles.
      hasDetails={false}
      expanded={expanded}
      onToggleExpanded={() => {
        const next = !expanded;
        setExpanded(next);
        if (!next) setExplainOpen(false);
      }}
      footer={footer}
    >
      {explainOpen ? (
        <div className="agent-approval-explanation" id={explanationId}>
          {explainState === "loading" ? (
            <p className="agent-approval-explanation-loading" role="status" aria-live="polite">
              <Spinner aria-hidden />
              <span>{t("chat.approval.explaining")}</span>
            </p>
          ) : explainState === "ready" && explanation ? (
            explanation
              .split(/\n{2,}/)
              .map((paragraph) => paragraph.trim())
              .filter(Boolean)
              .map((paragraph, index) => <p key={index}>{paragraph}</p>)
          ) : (
            // Generation unavailable (offline, signed out): keep the
            // static framing rather than an empty panel.
            <p>{t("chat.approval.pausedFallback")}</p>
          )}
          <p>
            {part.allowPermanent
              ? t("chat.approval.scopeHelpPermanent")
              : t("chat.approval.scopeHelp")}
          </p>
        </div>
      ) : null}
    </CollapsibleActionCard>
  );
}

function approvalChoiceLabel(t: TFunction, choice?: AgentApprovalChoice, pending = false) {
  if (choice === "once")
    return pending ? t("chat.approval.approvingOnce") : t("chat.approval.approvedOnce");
  if (choice === "session")
    return pending ? t("chat.approval.approvingSession") : t("chat.approval.approvedSession");
  if (choice === "always")
    return pending ? t("chat.approval.approvingAlways") : t("chat.approval.approvedAlways");
  if (choice === "deny") return pending ? t("chat.approval.denying") : t("chat.approval.denied");
  return t("chat.approval.resolved");
}
