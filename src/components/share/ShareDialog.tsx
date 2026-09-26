import { IconCircleInfo } from "central-icons/IconCircleInfo";
import { IconExclamationCircle } from "central-icons/IconExclamationCircle";
import { IconExclamationTriangle } from "central-icons/IconExclamationTriangle";
import { IconEyeOpen } from "central-icons/IconEyeOpen";
import { IconEyeSlash } from "central-icons/IconEyeSlash";
import { useCallback, useEffect, useRef, useState } from "react";
import { writeText as writeClipboardText } from "@tauri-apps/plugin-clipboard-manager";

import { useT, useTRich } from "../../i18n";
import { describeShareError, isShareNotFoundError } from "../../lib/errors";
import {
  buildLinkShareFragment,
  derivePasscodeKey,
  encryptPayload,
  fromBase64Url,
  generateKey,
  generatePasscodeSalt,
  toBase64,
  toBase64Url,
  wrapKey,
} from "../../lib/share-crypto";
import {
  getShareBaseUrl,
  shareCreate,
  shareDelete,
  shareGet,
  shareInviteKeySave,
  shareInviteKeysGet,
  shareKeyGet,
  shareKeySave,
  type ShareKind,
} from "../../lib/tauri";
import { ConfirmDialog } from "../ui/ConfirmDialog";
import { CopyLinkField } from "../ui/CopyLinkField";
import { CopyStateIcon } from "../ui/CopyStateIcon";
import { Dialog, DialogField } from "../ui/Dialog";
import { HoverTip } from "../ui/HoverTip";
import { InlineNotice } from "../ui/InlineNotice";
import { Switch } from "../ui/Switch";

export type ShareDialogItem = {
  kind: ShareKind;
  itemId: string;
  title: string;
  buildPayload: () => string;
};

const LINK_EMAIL = "link@share.invalid";
const PASSCODE_SALT_BYTES = 16;
const MIN_PASSCODE_LENGTH = 8;

export function ShareDialog({
  open,
  onClose,
  onLinkChange,
  item,
}: {
  open: boolean;
  onClose: () => void;
  onLinkChange?: (url: string | null) => void;
  item: ShareDialogItem;
}) {
  const t = useT();
  const tr = useTRich();
  const [loading, setLoading] = useState(false);
  const [busy, setBusy] = useState(false);
  const [shareId, setShareId] = useState<string | null>(null);
  const [inviteId, setInviteId] = useState<string | null>(null);
  const [linkMaterialB64, setLinkMaterialB64] = useState<string | null>(null);
  const [passwordProtected, setPasswordProtected] = useState(false);
  const [requirePasscode, setRequirePasscode] = useState(false);
  const [passcode, setPasscode] = useState("");
  const [revealPasscode, setRevealPasscode] = useState(false);
  const [baseUrl, setBaseUrl] = useState<string | null>(null);
  const [copied, setCopied] = useState<"link" | "passcode" | null>(null);
  const [copying, setCopying] = useState(false);
  const [legacyShare, setLegacyShare] = useState(false);
  const [loadFailed, setLoadFailed] = useState(false);
  const [loadVersion, setLoadVersion] = useState(0);
  const [confirmAction, setConfirmAction] = useState<"stop" | "reset" | null>(null);
  const [error, setError] = useState<string | null>(null);
  const busyRef = useRef(false);
  const copyingRef = useRef(false);
  const copyResetTimerRef = useRef<number>();
  const activeItemRef = useRef(`${item.kind}:${item.itemId}`);
  const previousOpenRef = useRef(open);
  activeItemRef.current = `${item.kind}:${item.itemId}`;
  const shouldLoadShare = open || Boolean(onLinkChange);

  useEffect(() => {
    const wasOpen = previousOpenRef.current;
    previousOpenRef.current = open;
    if (open && !wasOpen) setLoadVersion((version) => version + 1);
  }, [open]);

  const clearCopyFeedback = useCallback(() => {
    if (copyResetTimerRef.current !== undefined) {
      window.clearTimeout(copyResetTimerRef.current);
      copyResetTimerRef.current = undefined;
    }
    setCopied(null);
  }, []);

  const showCopyFeedback = useCallback((kind: "link" | "passcode") => {
    if (copyResetTimerRef.current !== undefined) {
      window.clearTimeout(copyResetTimerRef.current);
    }
    setCopied(kind);
    copyResetTimerRef.current = window.setTimeout(() => {
      setCopied(null);
      copyResetTimerRef.current = undefined;
    }, 1600);
  }, []);

  useEffect(
    () => () => {
      if (copyResetTimerRef.current !== undefined) {
        window.clearTimeout(copyResetTimerRef.current);
      }
    },
    [],
  );

  useEffect(() => {
    if (!shouldLoadShare) return;
    const startedItem = `${item.kind}:${item.itemId}`;
    let cancelled = false;
    setLoading(true);
    setShareId(null);
    setInviteId(null);
    setLinkMaterialB64(null);
    setPasswordProtected(false);
    setRequirePasscode(false);
    setPasscode("");
    setRevealPasscode(false);
    clearCopyFeedback();
    setLegacyShare(false);
    setLoadFailed(false);
    setConfirmAction(null);
    setError(null);

    void (async () => {
      try {
        const [saved, url] = await Promise.all([
          shareKeyGet(item.kind, item.itemId),
          getShareBaseUrl(),
        ]);
        if (cancelled || activeItemRef.current !== startedItem) return;
        setBaseUrl(url);
        if (!saved) return;
        setShareId(saved.shareId);
        try {
          const [remote, localKeys] = await Promise.all([
            shareGet(saved.shareId),
            shareInviteKeysGet(saved.shareId),
          ]);
          if (cancelled || activeItemRef.current !== startedItem) return;
          const linkInvite = remote.invites.find(
            (invite) => invite.email === LINK_EMAIL && invite.state !== "revoked",
          );
          const localKey = linkInvite
            ? localKeys.find((key) => key.inviteId === linkInvite.inviteId)
            : undefined;
          if (!linkInvite || !localKey) {
            setLegacyShare(true);
            return;
          }
          const material = fromBase64Url(localKey.inviteKeyB64);
          if (material.length !== PASSCODE_SALT_BYTES && material.length !== 32) {
            throw new Error("The local share link is invalid.");
          }
          setInviteId(linkInvite.inviteId);
          setLinkMaterialB64(localKey.inviteKeyB64);
          setPasswordProtected(material.length === PASSCODE_SALT_BYTES);
        } catch (loadError) {
          if (cancelled || activeItemRef.current !== startedItem) return;
          if (!isShareNotFoundError(loadError)) {
            setLoadFailed(true);
            throw loadError;
          }
          // The remote share is gone. Keep the ambiguous local key mapping so
          // another signed-in owner cannot destroy the original owner's keys.
          setShareId(null);
        }
      } catch (loadError) {
        if (!cancelled) setError(describeShareError(loadError));
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [shouldLoadShare, item.kind, item.itemId, clearCopyFeedback, loadVersion]);

  const copyExistingLink = useCallback(
    async (
      nextShareId: string,
      nextInviteId: string,
      materialB64: string,
      protectedLink: boolean,
      showFeedback = true,
    ) => {
      const url = baseUrl ?? (await getShareBaseUrl());
      const fragment = buildLinkShareFragment(
        nextInviteId,
        fromBase64Url(materialB64),
        protectedLink,
      );
      await writeClipboardText(`${url}/s/${nextShareId}#${fragment}`);
      setBaseUrl(url);
      if (showFeedback) showCopyFeedback("link");
    },
    [baseUrl, showCopyFeedback],
  );

  const runClipboardCopy = useCallback(async (copy: () => Promise<void>) => {
    copyingRef.current = true;
    setCopying(true);
    try {
      await copy();
    } finally {
      copyingRef.current = false;
      setCopying(false);
    }
  }, []);

  const handleCopyLink = useCallback(async () => {
    if (busyRef.current || copyingRef.current || loading || legacyShare || loadFailed) return;
    setError(null);
    clearCopyFeedback();
    if (shareId && inviteId && linkMaterialB64) {
      try {
        await runClipboardCopy(() =>
          copyExistingLink(shareId, inviteId, linkMaterialB64, passwordProtected),
        );
      } catch {
        setError(t("notes.share.copyLinkFailed"));
      }
      return;
    }
    if (requirePasscode && passcode.length < MIN_PASSCODE_LENGTH) {
      setError(t("notes.share.passcodeTooShort", { min: MIN_PASSCODE_LENGTH }));
      return;
    }

    const startedItem = `${item.kind}:${item.itemId}`;
    busyRef.current = true;
    setBusy(true);
    let createdShareId: string | null = null;
    try {
      const contentKey = await generateKey();
      const salt = requirePasscode ? generatePasscodeSalt() : null;
      const linkKey = salt ? await derivePasscodeKey(passcode, salt) : await generateKey();
      const storedMaterial = salt ?? linkKey;
      const { ciphertext, iv } = await encryptPayload(contentKey, item.buildPayload());
      const { envelope, iv: envelopeIv } = await wrapKey(linkKey, contentKey);
      const created = await shareCreate({
        kind: item.kind,
        ciphertextB64: toBase64(ciphertext),
        ivB64: toBase64(iv),
        invites: [
          {
            email: LINK_EMAIL,
            envelopeB64: toBase64(envelope),
            envelopeIvB64: toBase64(envelopeIv),
          },
        ],
      });
      createdShareId = created.shareId;
      const createdInvite = created.invites[0];
      if (!createdInvite) throw new Error("Clovy returned no share link.");
      const materialB64 = toBase64Url(storedMaterial);
      await shareKeySave({
        shareId: created.shareId,
        itemKind: item.kind,
        itemId: item.itemId,
        contentKeyB64: toBase64Url(contentKey),
      });
      await shareInviteKeySave({
        inviteId: createdInvite.inviteId,
        shareId: created.shareId,
        inviteKeyB64: materialB64,
      });
      createdShareId = null;
      if (activeItemRef.current === startedItem) {
        setShareId(created.shareId);
        setInviteId(createdInvite.inviteId);
        setLinkMaterialB64(materialB64);
        setPasswordProtected(Boolean(salt));
        setRevealPasscode(false);
        try {
          await runClipboardCopy(() =>
            copyExistingLink(
              created.shareId,
              createdInvite.inviteId,
              materialB64,
              Boolean(salt),
              false,
            ),
          );
        } catch {
          setError(t("notes.share.createdCopyFailed"));
        }
      }
    } catch (createError) {
      if (createdShareId) await shareDelete(createdShareId).catch(() => {});
      if (activeItemRef.current === startedItem) setError(describeShareError(createError));
    } finally {
      busyRef.current = false;
      setBusy(false);
    }
  }, [
    copyExistingLink,
    clearCopyFeedback,
    inviteId,
    item,
    legacyShare,
    linkMaterialB64,
    loadFailed,
    loading,
    passcode,
    passwordProtected,
    requirePasscode,
    runClipboardCopy,
    shareId,
    t,
  ]);

  const handleCopyPasscode = useCallback(async () => {
    if (!passcode || busyRef.current || copyingRef.current) return;
    setError(null);
    clearCopyFeedback();
    try {
      await runClipboardCopy(() => writeClipboardText(passcode));
      showCopyFeedback("passcode");
    } catch {
      setError(t("notes.share.copyPasscodeFailed"));
    }
  }, [clearCopyFeedback, passcode, runClipboardCopy, showCopyFeedback, t]);

  const handleStopSharing = useCallback(async () => {
    if (!shareId) return false;
    try {
      await shareDelete(shareId);
    } catch (stopError) {
      // Swallow the failure so the confirm dialog can close and the error
      // notice becomes visible in the share dialog, where Stop/Reset remain
      // available for retry.
      setError(describeShareError(stopError));
      return false;
    }
    setShareId(null);
    setInviteId(null);
    setLinkMaterialB64(null);
    setPasswordProtected(false);
    setLegacyShare(false);
    clearCopyFeedback();
    return true;
  }, [clearCopyFeedback, shareId]);

  // Resetting is stop-sharing plus intent: the owner wants a fresh link with a
  // fresh passcode, so land them on the create form with the toggle already on.
  const handleResetLink = useCallback(async () => {
    if (await handleStopSharing()) setRequirePasscode(true);
  }, [handleStopSharing]);

  const handleClose = useCallback(() => {
    if (!busyRef.current) onClose();
  }, [onClose]);

  const isNote = item.kind === "note";
  const itemTitle =
    item.title || (isNote ? t("notes.share.untitledNote") : t("notes.share.untitledSession"));
  const shareLinkLabel = t("notes.share.linkFor", { title: itemTitle });
  const hasLink = Boolean(shareId && inviteId && linkMaterialB64);
  const requiresPasscode = requirePasscode || passwordProtected;
  const shareUrl =
    shareId && inviteId && linkMaterialB64 && baseUrl
      ? `${baseUrl}/s/${shareId}#${buildLinkShareFragment(
          inviteId,
          fromBase64Url(linkMaterialB64),
          passwordProtected,
        )}`
      : null;

  useEffect(() => {
    onLinkChange?.(shareUrl);
  }, [onLinkChange, shareUrl]);

  return (
    <>
      <Dialog
        open={open}
        onClose={handleClose}
        disableBackdropClose={busy}
        title={isNote ? t("notes.share.titleNote") : t("notes.share.titleSession")}
        description={
          requiresPasscode
            ? t("notes.share.descriptionPasscode", { title: itemTitle })
            : t("notes.share.description", { title: itemTitle })
        }
        width={480}
        className="share-dialog"
        footer={
          shareId ? (
            <button
              type="button"
              className="primary-action share-unshare"
              disabled={busy}
              onClick={() => setConfirmAction("stop")}
            >
              {t("notes.share.stopSharing")}
            </button>
          ) : !loading && !legacyShare ? (
            <button
              type="button"
              className="primary-action primary-solid"
              disabled={busy || loadFailed}
              onClick={() => void handleCopyLink()}
            >
              {busy ? t("notes.share.creatingLink") : t("notes.share.createLink")}
            </button>
          ) : undefined
        }
      >
        <div className="dialog-body share-dialog-body">
          {loading ? <p className="share-dialog-caption">{t("notes.share.loading")}</p> : null}
          {!loading && legacyShare ? (
            <InlineNotice
              tone="info"
              aria-label={t("notes.share.legacyLabel")}
              icon={<IconCircleInfo size={14} aria-hidden />}
              body={t("notes.share.legacyBody")}
            />
          ) : null}
          {!loading && !legacyShare && !hasLink ? (
            <div className="share-dialog-section">
              <div className="share-option-row">
                <div className="share-option-info">
                  <span className="share-option-title" id="share-passcode-label">
                    {t("notes.share.requirePasscode")}
                  </span>
                  <span className="share-option-description">
                    {t("notes.share.requirePasscodeHint")}
                  </span>
                </div>
                <Switch
                  checked={requirePasscode}
                  disabled={busy || loadFailed}
                  aria-labelledby="share-passcode-label"
                  onCheckedChange={(next) => {
                    setRequirePasscode(next);
                    setError(null);
                  }}
                />
              </div>
              {requirePasscode ? (
                <div className="share-option-row">
                  <label className="share-option-title" htmlFor="share-passcode">
                    {t("notes.share.passcode")}
                  </label>
                  <div className="share-passcode-control">
                    <input
                      id="share-passcode"
                      className="dialog-input"
                      type={revealPasscode ? "text" : "password"}
                      autoComplete="new-password"
                      disabled={loadFailed}
                      value={passcode}
                      placeholder={t("notes.share.passcodePlaceholder", {
                        min: MIN_PASSCODE_LENGTH,
                      })}
                      onChange={(event) => setPasscode(event.currentTarget.value)}
                    />
                    <button
                      type="button"
                      className="icon-button share-passcode-reveal"
                      aria-label={
                        revealPasscode
                          ? t("notes.share.hidePasscode")
                          : t("notes.share.showPasscode")
                      }
                      aria-pressed={revealPasscode}
                      onClick={() => setRevealPasscode((value) => !value)}
                    >
                      {revealPasscode ? <IconEyeSlash size={16} /> : <IconEyeOpen size={16} />}
                    </button>
                  </div>
                </div>
              ) : null}
            </div>
          ) : null}
          {hasLink && shareUrl ? (
            <div className="share-link-block">
              {passwordProtected ? (
                <DialogField label={t("notes.share.link")} htmlFor="share-link-url">
                  <CopyLinkField
                    id="share-link-url"
                    value={shareUrl}
                    label={shareLinkLabel}
                    copied={copied === "link"}
                    disabled={busy || copying}
                    onCopy={() => void handleCopyLink()}
                  />
                </DialogField>
              ) : (
                <CopyLinkField
                  value={shareUrl}
                  label={shareLinkLabel}
                  copied={copied === "link"}
                  disabled={busy || copying}
                  onCopy={() => void handleCopyLink()}
                />
              )}
              {passwordProtected && passcode ? (
                <DialogField label={t("notes.share.passcode")} htmlFor="share-passcode-view">
                  <div className="copy-link-field share-passcode-field">
                    <input
                      id="share-passcode-view"
                      className="copy-link-url share-passcode-value"
                      type={revealPasscode ? "text" : "password"}
                      value={passcode}
                      readOnly
                      onFocus={(event) => event.currentTarget.select()}
                    />
                    <button
                      type="button"
                      className="icon-button share-passcode-field-reveal"
                      aria-label={
                        revealPasscode
                          ? t("notes.share.hidePasscode")
                          : t("notes.share.showPasscode")
                      }
                      aria-pressed={revealPasscode}
                      onClick={() => setRevealPasscode((value) => !value)}
                    >
                      {revealPasscode ? <IconEyeSlash size={16} /> : <IconEyeOpen size={16} />}
                    </button>
                    <HoverTip
                      compact
                      width={112}
                      tip={
                        copied === "passcode" ? t("common.copied") : t("notes.share.copyPasscode")
                      }
                      forceOpen={copied === "passcode"}
                      suppressed={busy || copying}
                      className="copy-link-action-tip"
                    >
                      <button
                        type="button"
                        className="copy-link-action"
                        aria-label={
                          copied === "passcode"
                            ? t("notes.share.passcodeCopied")
                            : t("notes.share.copyPasscode")
                        }
                        data-copied={copied === "passcode" ? "true" : undefined}
                        disabled={busy || copying}
                        onClick={() => void handleCopyPasscode()}
                      >
                        <CopyStateIcon copied={copied === "passcode"} />
                      </button>
                    </HoverTip>
                  </div>
                </DialogField>
              ) : null}
              {passwordProtected && passcode ? (
                <InlineNotice
                  tone="warning"
                  aria-label={t("notes.share.oneTimeLabel")}
                  icon={<IconExclamationTriangle size={14} aria-hidden />}
                  body={t("notes.share.oneTimeBody")}
                />
              ) : null}
              {passwordProtected && !passcode ? (
                <p className="share-dialog-caption">
                  {tr("notes.share.passcodeGone", {
                    reset: (chunks) => (
                      <button
                        type="button"
                        className="share-passcode-reset"
                        disabled={busy}
                        onClick={() => setConfirmAction("reset")}
                      >
                        {chunks}
                      </button>
                    ),
                  })}
                </p>
              ) : null}
            </div>
          ) : null}
          {error ? (
            <InlineNotice
              tone="destructive"
              role="alert"
              aria-label={t("notes.share.errorLabel")}
              icon={<IconExclamationCircle size={14} aria-hidden />}
              body={error}
            />
          ) : null}
        </div>
      </Dialog>
      <ConfirmDialog
        open={confirmAction !== null}
        onClose={() => setConfirmAction(null)}
        onConfirm={confirmAction === "reset" ? handleResetLink : handleStopSharing}
        title={
          confirmAction === "reset" ? t("notes.share.resetLink") : t("notes.share.stopSharing")
        }
        description={
          confirmAction === "reset"
            ? t("notes.share.resetDescription")
            : isNote
              ? t("notes.share.stopDescriptionNote")
              : t("notes.share.stopDescriptionSession")
        }
        confirmLabel={
          confirmAction === "reset" ? t("notes.share.resetLink") : t("notes.share.stopSharing")
        }
        confirmBusyLabel={
          confirmAction === "reset" ? t("notes.share.resetting") : t("notes.share.stopping")
        }
        destructive
      />
    </>
  );
}
