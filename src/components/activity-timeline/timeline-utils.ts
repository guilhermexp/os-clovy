import type { InterfaceLocale, MessageKey } from "../../i18n";
import type {
  ActivityCategory,
  TimelineGapKind,
  TimelinePauseReason,
} from "../../lib/activity-timeline";
export const CATEGORY_COLOR_VARS: Record<ActivityCategory, string> = {
  coding: "var(--activity-coding)",
  codeReview: "var(--activity-code-review)",
  meeting: "var(--activity-meeting)",
  communication: "var(--activity-communication)",
  design: "var(--activity-design)",
  documentation: "var(--activity-documentation)",
  planning: "var(--activity-planning)",
  deploymentDevops: "var(--activity-deployment-devops)",
  research: "var(--activity-research)",
  idlePersonal: "var(--activity-idle-personal)",
};

export const CATEGORY_LABEL_KEYS: Record<ActivityCategory, MessageKey> = {
  coding: "activity.category.coding",
  codeReview: "activity.category.codeReview",
  meeting: "activity.category.meeting",
  communication: "activity.category.communication",
  design: "activity.category.design",
  documentation: "activity.category.documentation",
  planning: "activity.category.planning",
  deploymentDevops: "activity.category.deploymentDevops",
  research: "activity.category.research",
  idlePersonal: "activity.category.idlePersonal",
};

export function gapLabelKey(
  kind: TimelineGapKind,
  pauseReason: TimelinePauseReason | null,
): MessageKey {
  if (kind === "idle") return "activity.gap.idle";
  if (kind === "sleep") return "activity.gap.sleep";
  switch (pauseReason) {
    case "workHours":
      return "activity.gap.pausedWorkHours";
    case "lowDisk":
      return "activity.gap.pausedLowDisk";
    case "protectedVideo":
      return "activity.gap.pausedProtectedVideo";
    default:
      return "activity.gap.pausedManual";
  }
}

/**
 * Formats duration in milliseconds into a concise localized string (e.g. "1h 24m", "45m", "1 h 24 min").
 */
export function formatDuration(durationMs: number, locale: InterfaceLocale = "en"): string {
  const isPt = locale === "pt-BR";
  if (durationMs < 60_000) {
    return isPt ? "< 1 min" : "< 1m";
  }
  const totalMins = Math.round(durationMs / 60_000);
  const hours = Math.floor(totalMins / 60);
  const mins = totalMins % 60;

  if (hours === 0) {
    return isPt ? `${mins} min` : `${mins}m`;
  }
  if (mins === 0) {
    return isPt ? `${hours} h` : `${hours}h`;
  }
  return isPt ? `${hours} h ${mins} min` : `${hours}h ${mins}m`;
}

/**
 * Formats an ISO timestamp to local clock time (e.g. "09:30" or "9:30 AM").
 */
export function formatTime(dateStr: string, locale: InterfaceLocale = "en"): string {
  const date = new Date(dateStr);
  return new Intl.DateTimeFormat(locale === "pt-BR" ? "pt-BR" : undefined, {
    hour: "numeric",
    minute: "2-digit",
  }).format(date);
}

/**
 * Formats a local date heading (e.g. "Wednesday, Oct 4" or "quarta-feira, 4 de out.").
 */
export function formatDayHeader(date: Date, locale: InterfaceLocale = "en"): string {
  return new Intl.DateTimeFormat(locale === "pt-BR" ? "pt-BR" : undefined, {
    weekday: "long",
    month: "short",
    day: "numeric",
  }).format(date);
}

/**
 * Checks whether two Date objects refer to the same calendar day in the local timezone.
 */
export function isSameDay(d1: Date, d2: Date): boolean {
  return (
    d1.getFullYear() === d2.getFullYear() &&
    d1.getMonth() === d2.getMonth() &&
    d1.getDate() === d2.getDate()
  );
}
