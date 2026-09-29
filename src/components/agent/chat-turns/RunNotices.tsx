import { useT } from "../../../i18n";
import type { AgentChatPart } from "../../../lib/agent-chat-runtime";

export function ContextOverflowNoticePart() {
  const t = useT();
  return <div className="agent-system-notice">{t("chat.notice.contextOverflow")}</div>;
}

export function CreditsNoticePart({ onTopUp }: { onTopUp?: () => void; [key: string]: unknown }) {
  const t = useT();
  return (
    <div className="agent-system-notice">
      {t("chat.notice.needCredits")}
      {onTopUp ? (
        <button type="button" onClick={onTopUp}>
          {t("chat.notice.addCredits")}
        </button>
      ) : null}
    </div>
  );
}

export function UpstreamProviderFailureNoticePart({
  onRetry,
  kind = "upstream-provider",
}: {
  onRetry?: () => void;
  kind?: "upstream-provider" | "tool" | "runtime";
  [key: string]: unknown;
}) {
  const t = useT();
  const message =
    kind === "tool"
      ? t("chat.notice.toolFailed")
      : kind === "runtime"
        ? t("chat.notice.runtimeStopped")
        : t("chat.notice.upstreamFailed");
  return (
    <div className="agent-system-notice">
      {message}
      {onRetry ? (
        <button type="button" onClick={onRetry}>
          {t("common.tryAgain")}
        </button>
      ) : null}
    </div>
  );
}

export function SteeringPart({ part }: { part: Extract<AgentChatPart, { type: "steering" }> }) {
  return <div className="agent-system-notice">{part.text}</div>;
}
