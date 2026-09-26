/**
 * The three issue report categories. New report entry points use them in the
 * direct-submit dialog, where no model runs and the server creates the
 * team-facing diagnosis. The inline composer chip still uses the same values
 * for restored older drafts.
 */

import { t } from "../../../i18n";

export type ReportCategory = "bug" | "feedback" | "feature";

// Sent to the server as the report body (team-facing), so it stays English.
export const ISSUE_REPORT_ATTACHMENTS_ONLY_DESCRIPTION =
  "No description was typed; see the attachments.";

export type ReportCategoryDef = {
  key: ReportCategory;
  /** Chip and menu label. Sentence case, no dashes (see CLAUDE.md). */
  label: string;
  /** Short helper copy for report-specific surfaces. */
  hint: string;
  /** Description placeholder in the report dialog, tailored per category. */
  placeholder: string;
  /** Extra terms report-category search can match beyond the label. */
  keywords: string[];
};

export const REPORT_CATEGORIES: ReportCategoryDef[] = [
  {
    key: "bug",
    get label() {
      return t("chat.report.bug.label");
    },
    get hint() {
      return t("chat.report.bug.hint");
    },
    get placeholder() {
      return t("chat.report.bug.placeholder");
    },
    keywords: ["bug", "issue", "report", "broken", "problem", "error", "crash"],
  },
  {
    key: "feedback",
    get label() {
      return t("chat.report.feedback.label");
    },
    get hint() {
      return t("chat.report.feedback.hint");
    },
    get placeholder() {
      return t("chat.report.feedback.placeholder");
    },
    keywords: ["feedback", "thoughts", "comment", "suggestion"],
  },
  {
    key: "feature",
    get label() {
      return t("chat.report.feature.label");
    },
    get hint() {
      return t("chat.report.feature.hint");
    },
    get placeholder() {
      return t("chat.report.feature.placeholder");
    },
    keywords: ["feature", "request", "idea", "wish", "want"],
  },
];

const BY_KEY = new Map<ReportCategory, ReportCategoryDef>(
  REPORT_CATEGORIES.map((category) => [category.key, category]),
);

export function reportCategoryDef(
  key: ReportCategory | string | null | undefined,
): ReportCategoryDef | undefined {
  if (!key) return undefined;
  return BY_KEY.get(key as ReportCategory);
}

export function isReportCategory(value: unknown): value is ReportCategory {
  return typeof value === "string" && BY_KEY.has(value as ReportCategory);
}

/** Ranks categories against the text typed after "/". Empty query keeps the
 * canonical order so the menu opens as a stable three-item palette. */
export function matchReportCategories(query: string): ReportCategoryDef[] {
  const trimmed = query.trim().toLowerCase();
  if (!trimmed) return REPORT_CATEGORIES;
  return REPORT_CATEGORIES.filter((category) => {
    if (category.label.toLowerCase().includes(trimmed)) return true;
    if (category.key.includes(trimmed)) return true;
    return category.keywords.some((keyword) => keyword.includes(trimmed));
  });
}
