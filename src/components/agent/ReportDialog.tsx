import { open as openFileDialog } from "@tauri-apps/plugin-dialog";
import { IconCrossSmall } from "central-icons/IconCrossSmall";
import { IconPaperclip1 } from "central-icons/IconPaperclip1";
import { useId, useMemo, useState } from "react";

import { useLocale, useT } from "../../i18n";
import { messageFromError } from "../../lib/errors";
import { recordPositiveFeedbackSent } from "../../lib/referral-nudge";
import { submitIssueReport } from "../../lib/tauri";
import { DotSpinner } from "../DotSpinner";
import { Dialog, DialogField } from "../ui/Dialog";
import { SegmentedControl } from "../ui/SegmentedControl";
import { CategoryIcon } from "./composer/CategoryIcon";
import {
  ISSUE_REPORT_ATTACHMENTS_ONLY_DESCRIPTION,
  REPORT_CATEGORIES,
  type ReportCategory,
  reportCategoryDef,
} from "./composer/reportCategory";
import { FileTypeIcon } from "./FileTypeIcon";

type ReportDialogProps = {
  category: ReportCategory;
  storedSessionId?: string;
  onCategoryChange: (category: ReportCategory) => void;
  onClose: () => void;
};

export function ReportDialog({
  category,
  storedSessionId,
  onCategoryChange,
  onClose,
}: ReportDialogProps) {
  const t = useT();
  const locale = useLocale();
  const [description, setDescription] = useState("");
  const [attachmentPaths, setAttachmentPaths] = useState<string[]>([]);
  const [submitting, setSubmitting] = useState(false);
  const [sent, setSent] = useState(false);
  const [includeFailureDetails, setIncludeFailureDetails] = useState(true);
  const [error, setError] = useState<string>();
  const descriptionId = useId();
  // Category labels resolve in the interface language, so rebuild on a
  // locale change.
  // biome-ignore lint/correctness/useExhaustiveDependencies: locale is the rebuild trigger
  const categoryOptions = useMemo(
    () =>
      REPORT_CATEGORIES.map((item) => ({
        value: item.key,
        ariaLabel: item.label,
        label: (
          <>
            <CategoryIcon category={item.key} size={14} />
            <span className="report-dialog-category-label">{item.label}</span>
          </>
        ),
      })),
    [locale],
  );
  const trimmedDescription = description.trim();
  const canSubmit = Boolean(trimmedDescription || attachmentPaths.length);
  const canAttachFailureDetails =
    category === "bug" && Boolean(storedSessionId) && attachmentPaths.length < 20;

  async function pickAttachments() {
    const selected = await openFileDialog({ multiple: true, title: t("agent.report.pickerTitle") });
    if (!selected) return;
    const paths = Array.isArray(selected) ? selected : [selected];
    setAttachmentPaths((current) =>
      [...new Set([...current, ...paths])].slice(
        0,
        category === "bug" && storedSessionId && includeFailureDetails ? 19 : 20,
      ),
    );
  }

  async function send() {
    if (!canSubmit || submitting) return;
    setSubmitting(true);
    setError(undefined);
    try {
      await submitIssueReport({
        category,
        description: trimmedDescription || ISSUE_REPORT_ATTACHMENTS_ONLY_DESCRIPTION,
        attachmentNames: attachmentPaths.map((path) => path.split(/[\\/]/).pop() || path),
        attachmentPaths,
        ...(canAttachFailureDetails && includeFailureDetails ? { storedSessionId } : {}),
      });
      if (category === "feedback") recordPositiveFeedbackSent();
      setSent(true);
    } catch (cause) {
      setError(t("agent.report.sendFailed", { error: messageFromError(cause) }));
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <Dialog
      open
      onClose={onClose}
      title={t("agent.report.title")}
      className="report-dialog"
      initialFocusSelector=".dialog-textarea"
      footer={
        sent ? (
          <button type="button" className="primary-action primary-solid" onClick={onClose}>
            {t("common.done")}
          </button>
        ) : (
          <>
            <button
              type="button"
              className="btn btn-ghost report-dialog-add-files"
              disabled={submitting}
              onClick={() => void pickAttachments()}
            >
              <IconPaperclip1 size={16} aria-hidden />
              {t("agent.report.addFiles")}
            </button>
            <button
              type="button"
              className="primary-action primary-solid"
              disabled={!canSubmit || submitting}
              aria-busy={submitting || undefined}
              onClick={() => void send()}
            >
              {submitting ? <DotSpinner className="report-dialog-submit-spinner" /> : null}
              {submitting ? t("agent.report.sending") : t("agent.report.send")}
            </button>
          </>
        )
      }
    >
      {sent ? (
        <p className="report-dialog-sent" role="status">
          {t("agent.report.sent")}
        </p>
      ) : (
        <div className="dialog-body report-dialog-drop">
          <SegmentedControl
            value={category}
            onValueChange={onCategoryChange}
            options={categoryOptions}
            className="report-dialog-category"
            aria-label={t("agent.report.category")}
          />
          <DialogField label={t("agent.report.description")} htmlFor={descriptionId}>
            <textarea
              id={descriptionId}
              className="dialog-textarea"
              value={description}
              disabled={submitting}
              rows={5}
              placeholder={reportCategoryDef(category)?.placeholder}
              onChange={(event) => setDescription(event.currentTarget.value)}
            />
          </DialogField>
          {category === "bug" && storedSessionId ? (
            <label
              className="report-dialog-diagnostics"
              data-disabled={!canAttachFailureDetails || undefined}
            >
              <input
                type="checkbox"
                checked={includeFailureDetails && canAttachFailureDetails}
                disabled={!canAttachFailureDetails || submitting}
                onChange={(event) => setIncludeFailureDetails(event.currentTarget.checked)}
              />
              <span>{t("agent.report.diagnostics")}</span>
            </label>
          ) : null}
          {attachmentPaths.length ? (
            <ul className="report-dialog-file-list" aria-label={t("agent.report.attachedFiles")}>
              {attachmentPaths.map((path) => {
                const name = path.split(/[\\/]/).pop() || path;
                return (
                  <li key={path} className="report-dialog-file">
                    <FileTypeIcon name={name} size={14} />
                    <span className="report-dialog-file-name">{name}</span>
                    <button
                      type="button"
                      aria-label={t("agent.report.removeFile", { name })}
                      disabled={submitting}
                      onClick={() =>
                        setAttachmentPaths((current) => current.filter((item) => item !== path))
                      }
                    >
                      <IconCrossSmall size={12} aria-hidden />
                    </button>
                  </li>
                );
              })}
            </ul>
          ) : null}
          {error ? (
            <p className="report-dialog-error" role="alert">
              {error}
            </p>
          ) : null}
        </div>
      )}
    </Dialog>
  );
}
