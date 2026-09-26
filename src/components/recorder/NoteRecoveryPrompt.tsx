import { useLayoutEffect, useRef, useState } from "react";
import { IconRecord } from "central-icons/IconRecord";
import { InlineNotice } from "../ui/InlineNotice";
import { formatNumber, useT } from "../../i18n";
import type { RecoverableRecordingDto } from "../../lib/tauri";

type NoteRecoveryPromptProps = {
  recovery: RecoverableRecordingDto;
  onRecover: (sessionId: string) => void;
  onDiscard: (sessionId: string) => void;
  disabled?: boolean;
  recoverBlockedReason?: string;
};

export function NoteRecoveryPrompt({
  recovery,
  onRecover,
  onDiscard,
  disabled,
  recoverBlockedReason,
}: NoteRecoveryPromptProps) {
  const t = useT();
  const isRecoveryBlocked = Boolean(recoverBlockedReason);
  const bodyRef = useRef<HTMLSpanElement>(null);
  const [bodyIsSingleLine, setBodyIsSingleLine] = useState(false);

  useLayoutEffect(() => {
    if (isRecoveryBlocked) {
      setBodyIsSingleLine(false);
      return;
    }

    const body = bodyRef.current;
    if (!body) return;

    const measure = () => setBodyIsSingleLine(hasOneRenderedLine(body));
    measure();

    if (typeof ResizeObserver === "undefined") {
      window.addEventListener("resize", measure);
      return () => window.removeEventListener("resize", measure);
    }

    const observer = new ResizeObserver(measure);
    observer.observe(body);
    return () => observer.disconnect();
  }, [isRecoveryBlocked]);

  const isCentered = !isRecoveryBlocked && bodyIsSingleLine;

  return (
    <InlineNotice
      className={[
        "note-recovery-prompt",
        isRecoveryBlocked && "note-recovery-prompt-blocked",
        isCentered && "note-recovery-prompt-single-line",
      ]
        .filter(Boolean)
        .join(" ")}
      aria-label={t("recorder.recovery.aria")}
      icon={<IconRecord size={14} aria-hidden />}
      bodyRef={bodyRef}
      body={
        <>
          {t("recorder.recovery.body", { size: formatBytes(recovery.bytesFound) })}
          {recoverBlockedReason ? (
            <>
              {" "}
              <span>{recoverBlockedReason}</span>
            </>
          ) : null}
        </>
      }
      actions={
        <>
          <button
            type="button"
            className="btn btn-ghost"
            disabled={disabled}
            onClick={() => onDiscard(recovery.sessionId)}
          >
            {t("recorder.recovery.discard")}
          </button>
          <button
            type="button"
            className="btn btn-secondary"
            disabled={disabled || isRecoveryBlocked}
            title={recoverBlockedReason}
            onClick={() => onRecover(recovery.sessionId)}
          >
            {t("recorder.recovery.recover")}
          </button>
        </>
      }
    />
  );
}

function hasOneRenderedLine(element: HTMLElement): boolean {
  const range = document.createRange();
  range.selectNodeContents(element);

  const lineTops: number[] = [];
  for (const rect of range.getClientRects()) {
    if (rect.width === 0 && rect.height === 0) continue;
    if (lineTops.every((top) => Math.abs(top - rect.top) >= 1)) {
      lineTops.push(rect.top);
    }
  }

  return lineTops.length === 1;
}

// One decimal, no grouping: the same digits as the old toFixed(1), with the
// decimal separator following the interface language (1,5 MB in pt-BR).
const ONE_DECIMAL: Intl.NumberFormatOptions = {
  minimumFractionDigits: 1,
  maximumFractionDigits: 1,
  useGrouping: false,
};

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${formatNumber(bytes / 1024, ONE_DECIMAL)} KB`;
  return `${formatNumber(bytes / (1024 * 1024), ONE_DECIMAL)} MB`;
}
