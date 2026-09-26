import { writeText as writeClipboardText } from "@tauri-apps/plugin-clipboard-manager";
import { open as openFileDialog } from "@tauri-apps/plugin-dialog";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  companionApprovePairing,
  companionBeginPairing,
  companionComputerUseApprovalSettings,
  companionGrantBrowseRoot,
  companionListBrowseRoots,
  companionListDevices,
  companionPairingStatus,
  companionRenameDevice,
  companionRevokeBrowseRoot,
  companionRevokeDevice,
  companionSetComputerUseApprovalEnabled,
  type CompanionBrowseRoot,
  type CompanionCapability,
  type CompanionComputerUseApprovalSettings,
  type CompanionPairingQr,
  type CompanionPairingStatus,
  type LinkedCompanionDevice,
} from "../../lib/tauri";
import {
  formatDate as formatLocaleDate,
  intlLocale,
  type MessageKey,
  t as translate,
  useT,
} from "../../i18n";
import { Switch } from "../ui/Switch";

const capabilityLabels: Record<CompanionCapability, MessageKey> = {
  notesRead: "settingsPanels.devices.cap.notesRead",
  notesEdit: "settingsPanels.devices.cap.notesEdit",
  agentRead: "settingsPanels.devices.cap.agentRead",
  agentChat: "settingsPanels.devices.cap.agentChat",
  agentCancel: "settingsPanels.devices.cap.agentCancel",
  modelRead: "settingsPanels.devices.cap.modelRead",
  modelEdit: "settingsPanels.devices.cap.modelEdit",
  mediaRead: "settingsPanels.devices.cap.mediaRead",
  settingsRead: "settingsPanels.devices.cap.settingsRead",
  settingsEditSafe: "settingsPanels.devices.cap.settingsEditSafe",
  recordingControlExisting: "settingsPanels.devices.cap.recordingControlExisting",
  appFocus: "settingsPanels.devices.cap.appFocus",
  filesUpload: "settingsPanels.devices.cap.filesUpload",
  filesBrowse: "settingsPanels.devices.cap.filesBrowse",
  devicesReadSelf: "settingsPanels.devices.cap.devicesReadSelf",
  devicesRevokeSelf: "settingsPanels.devices.cap.devicesRevokeSelf",
  computerUseApprove: "settingsPanels.devices.cap.computerUseApprove",
};
const companionCapabilities = Object.keys(capabilityLabels) as CompanionCapability[];

export function LinkedDevicesSection() {
  const t = useT();
  const [devices, setDevices] = useState<LinkedCompanionDevice[]>([]);
  const [browseRoots, setBrowseRoots] = useState<CompanionBrowseRoot[]>([]);
  const [computerUseApprovals, setComputerUseApprovals] =
    useState<CompanionComputerUseApprovalSettings>();
  const [pairing, setPairing] = useState<CompanionPairingQr>();
  const [status, setStatus] = useState<CompanionPairingStatus>();
  const [editingId, setEditingId] = useState<string>();
  const [draftName, setDraftName] = useState("");
  const [busy, setBusy] = useState(false);
  const [pairingCodeCopied, setPairingCodeCopied] = useState(false);
  const [error, setError] = useState<string>();
  const activePairingIdRef = useRef<string>();
  const mountedRef = useRef(true);

  const endPairing = useCallback((nextError?: string) => {
    activePairingIdRef.current = undefined;
    setPairing(undefined);
    setStatus(undefined);
    setPairingCodeCopied(false);
    if (nextError) setError(nextError);
  }, []);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      activePairingIdRef.current = undefined;
    };
  }, []);

  const refreshDevices = useCallback(async () => {
    setDevices(await companionListDevices());
  }, []);

  const refreshBrowseRoots = useCallback(async () => {
    setBrowseRoots(await companionListBrowseRoots());
  }, []);

  useEffect(() => {
    void refreshDevices().catch((next) => setError(errorMessage(next)));
    void refreshBrowseRoots().catch((next) => setError(errorMessage(next)));
    void companionComputerUseApprovalSettings()
      .then(setComputerUseApprovals)
      .catch((next) => setError(errorMessage(next)));
  }, [refreshBrowseRoots, refreshDevices]);

  useEffect(() => {
    if (!pairing) return;
    let cancelled = false;
    const poll = async () => {
      try {
        const next = await companionPairingStatus(pairing.pairingId);
        if (!cancelled) setStatus(next);
      } catch (next) {
        if (!cancelled) setError(errorMessage(next));
      }
    };
    void poll();
    const interval = window.setInterval(() => void poll(), 1_000);
    return () => {
      cancelled = true;
      window.clearInterval(interval);
    };
  }, [pairing]);

  useEffect(() => {
    if (!pairing) return;
    const timeout = window.setTimeout(
      () => {
        endPairing(translate("settingsPanels.devices.codeExpiredError"));
      },
      Math.max(0, pairing.expiresAtMs - Date.now()),
    );
    return () => window.clearTimeout(timeout);
  }, [endPairing, pairing]);

  const qrSource = useMemo(
    () =>
      pairing ? `data:image/svg+xml;charset=utf-8,${encodeURIComponent(pairing.qrSvg)}` : undefined,
    [pairing],
  );

  const startPairing = async () => {
    setBusy(true);
    setError(undefined);
    try {
      const next = await companionBeginPairing();
      activePairingIdRef.current = next.pairingId;
      setPairing(next);
      setStatus(undefined);
      setPairingCodeCopied(false);
    } catch (next) {
      setError(errorMessage(next));
    } finally {
      setBusy(false);
    }
  };

  const copyPairingCode = async () => {
    if (!pairing) return;
    if (pairing.expiresAtMs <= Date.now()) {
      setError(t("settingsPanels.devices.codeExpiredError"));
      return;
    }
    setError(undefined);
    try {
      const pairingId = pairing.pairingId;
      const pairingCode = pairing.pairingCode;
      await writeClipboardText(pairingCode);
      if (
        !mountedRef.current ||
        activePairingIdRef.current !== pairingId ||
        pairing.expiresAtMs <= Date.now()
      ) {
        return;
      }
      setPairingCodeCopied(true);
    } catch {
      setError(t("settingsPanels.devices.copyFailed"));
    }
  };

  const approve = async () => {
    if (!pairing || !status?.mobileDeviceId) return;
    setBusy(true);
    setError(undefined);
    try {
      await companionApprovePairing(pairing.pairingId, status.mobileDeviceId);
      await refreshDevices();
      endPairing();
    } catch (next) {
      setError(errorMessage(next));
    } finally {
      setBusy(false);
    }
  };

  const saveName = async (deviceId: string) => {
    setBusy(true);
    setError(undefined);
    try {
      await companionRenameDevice(deviceId, draftName);
      await refreshDevices();
      setEditingId(undefined);
    } catch (next) {
      setError(errorMessage(next));
    } finally {
      setBusy(false);
    }
  };

  const revoke = async (device: LinkedCompanionDevice) => {
    if (!window.confirm(t("settingsPanels.devices.unlinkConfirm", { name: device.displayName })))
      return;
    setBusy(true);
    setError(undefined);
    try {
      await companionRevokeDevice(device.id);
      await refreshDevices();
    } catch (next) {
      setError(errorMessage(next));
    } finally {
      setBusy(false);
    }
  };

  const addBrowseRoot = async () => {
    setError(undefined);
    const selected = await openFileDialog({ directory: true, multiple: false });
    if (!selected || Array.isArray(selected)) return;
    setBusy(true);
    try {
      await companionGrantBrowseRoot(selected);
      await refreshBrowseRoots();
    } catch (next) {
      setError(errorMessage(next));
    } finally {
      setBusy(false);
    }
  };

  const removeBrowseRoot = async (root: CompanionBrowseRoot) => {
    if (!window.confirm(t("settingsPanels.devices.stopSharingConfirm", { name: root.name }))) {
      return;
    }
    setBusy(true);
    setError(undefined);
    try {
      await companionRevokeBrowseRoot(root.id);
      await refreshBrowseRoots();
    } catch (next) {
      setError(errorMessage(next));
    } finally {
      setBusy(false);
    }
  };

  const setComputerUseApprovalEnabled = async (enabled: boolean) => {
    setBusy(true);
    setError(undefined);
    try {
      setComputerUseApprovals(await companionSetComputerUseApprovalEnabled(enabled));
    } catch (next) {
      setError(errorMessage(next));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="settings-group companion-settings" aria-labelledby="linked-devices-heading">
      <header className="settings-page-header">
        <h2 id="linked-devices-heading" className="settings-page-title">
          {t("settingsPanels.devices.title")}
        </h2>
        <p className="settings-page-blurb">{t("settingsPanels.devices.blurb")}</p>
      </header>

      {error ? (
        <div className="inline-notice inline-notice-error" role="alert">
          {error}
        </div>
      ) : null}

      <div className="settings-card companion-pairing-card">
        <div className="settings-row-info">
          <h3 className="settings-row-title">{t("settingsPanels.devices.linkTitle")}</h3>
          <p className="settings-row-description">{t("settingsPanels.devices.linkDescription")}</p>
        </div>
        {!pairing ? (
          <button
            type="button"
            className="primary-action primary-solid"
            disabled={busy}
            onClick={() => void startPairing()}
          >
            {t("settingsPanels.devices.showCode")}
          </button>
        ) : (
          <div className="companion-pairing-flow">
            {qrSource ? (
              <img
                className="companion-pairing-qr"
                src={qrSource}
                alt={t("settingsPanels.devices.qrAlt")}
              />
            ) : null}
            <div className="companion-pairing-copy" aria-live="polite">
              <strong>{pairingLabel(t, status?.state)}</strong>
              <span>
                {t("settingsPanels.devices.expires", {
                  time: new Date(pairing.expiresAtMs).toLocaleTimeString(intlLocale()),
                })}
              </span>
              <details className="companion-manual-pairing">
                <summary>{t("settingsPanels.devices.enterCodeInstead")}</summary>
                <p>{t("settingsPanels.devices.enterCodeHelp")}</p>
                <code>{pairing.pairingCode}</code>
                <button
                  type="button"
                  className="primary-action"
                  onClick={() => void copyPairingCode()}
                >
                  {pairingCodeCopied
                    ? t("settingsPanels.devices.codeCopied")
                    : t("settingsPanels.devices.copyCode")}
                </button>
                <p>{t("settingsPanels.devices.clipboardNote")}</p>
              </details>
              {status?.state === "waitingForApproval" ? (
                <>
                  <span>
                    {status.mobileDisplayName
                      ? t("settingsPanels.devices.askingToLink", { name: status.mobileDisplayName })
                      : t("settingsPanels.devices.companionAskingToLink")}
                  </span>
                  <span>{t("settingsPanels.devices.willReceive")}</span>
                  <ul
                    className="companion-capabilities"
                    aria-label={t("settingsPanels.devices.capabilitiesToApprove")}
                  >
                    {companionCapabilities.map((capability) => (
                      <li className="companion-capability" key={capability}>
                        {t(capabilityLabels[capability])}
                      </li>
                    ))}
                  </ul>
                  <button
                    type="button"
                    className="primary-action primary-solid"
                    disabled={busy || !status.mobileDeviceId}
                    onClick={() => void approve()}
                  >
                    {t("settingsPanels.devices.approveDevice")}
                  </button>
                </>
              ) : null}
              <button
                type="button"
                className="primary-action"
                onClick={() => {
                  endPairing();
                }}
              >
                {t("common.cancel")}
              </button>
            </div>
          </div>
        )}
      </div>

      <div className="settings-card companion-browse-card">
        <div className="settings-row-info">
          <h3 className="settings-row-title">{t("settingsPanels.devices.macFolders")}</h3>
          <p className="settings-row-description">
            {t("settingsPanels.devices.macFoldersDescription")}
          </p>
        </div>
        <button
          type="button"
          className="primary-action"
          disabled={busy || browseRoots.length >= 16}
          onClick={() => void addBrowseRoot()}
        >
          {t("settingsPanels.devices.addFolder")}
        </button>
        {browseRoots.length ? (
          <ul
            className="companion-root-list"
            aria-label={t("settingsPanels.devices.sharedFoldersAria")}
          >
            {browseRoots.map((root) => (
              <li className="companion-root-row" key={root.id}>
                <div className="settings-row-info">
                  <span className="settings-row-title">{root.name}</span>
                  <span className="settings-row-description companion-root-path">{root.path}</span>
                </div>
                <button
                  type="button"
                  className="primary-action primary-destructive"
                  disabled={busy}
                  onClick={() => void removeBrowseRoot(root)}
                >
                  {t("settingsPanels.devices.stopSharing")}
                </button>
              </li>
            ))}
          </ul>
        ) : (
          <p className="settings-row-description">{t("settingsPanels.devices.noFolders")}</p>
        )}
      </div>

      <div className="settings-card">
        <div className="settings-rows">
          <div className="settings-row">
            <div className="settings-row-info">
              <h3 className="settings-row-title">
                {t("settingsPanels.devices.approveComputerUse")}
              </h3>
              <p className="settings-row-description">
                {computerUseApprovals?.available === false
                  ? t("settingsPanels.devices.computerUseUnavailable")
                  : t("settingsPanels.devices.computerUseDescription")}
              </p>
            </div>
            <div className="settings-row-control">
              <Switch
                checked={computerUseApprovals?.enabled ?? false}
                disabled={busy || !computerUseApprovals?.available}
                aria-label={t("settingsPanels.devices.approveComputerUse")}
                onCheckedChange={(enabled) => void setComputerUseApprovalEnabled(enabled)}
              />
            </div>
          </div>
        </div>
      </div>

      <div className="companion-device-list">
        {devices.filter((device) => !device.revokedAt).length ? (
          devices
            .filter((device) => !device.revokedAt)
            .map((device) => (
              <article className="settings-card companion-device-card" key={device.id}>
                <div className="companion-device-heading">
                  <div>
                    {editingId === device.id ? (
                      <input
                        aria-label={t("settingsPanels.devices.deviceName")}
                        className="settings-text-input companion-name-input"
                        maxLength={128}
                        value={draftName}
                        onChange={(event) => setDraftName(event.target.value)}
                      />
                    ) : (
                      <h3 className="settings-row-title">{device.displayName}</h3>
                    )}
                    <p className="settings-row-description">
                      {t("settingsPanels.devices.linkedAt", { date: formatDate(device.linkedAt) })}
                      {device.lastSeenAt
                        ? t("settingsPanels.devices.lastSeenSuffix", {
                            date: formatDate(device.lastSeenAt),
                          })
                        : ""}
                    </p>
                  </div>
                  <div className="companion-device-actions">
                    {editingId === device.id ? (
                      <>
                        <button
                          type="button"
                          className="primary-action primary-solid"
                          disabled={busy || !draftName.trim()}
                          onClick={() => void saveName(device.id)}
                        >
                          {t("common.save")}
                        </button>
                        <button
                          type="button"
                          className="primary-action"
                          onClick={() => setEditingId(undefined)}
                        >
                          {t("common.cancel")}
                        </button>
                      </>
                    ) : (
                      <>
                        <button
                          type="button"
                          className="primary-action"
                          onClick={() => {
                            setEditingId(device.id);
                            setDraftName(device.displayName);
                          }}
                        >
                          {t("common.rename")}
                        </button>
                        <button
                          type="button"
                          className="primary-action primary-destructive"
                          disabled={busy}
                          onClick={() => void revoke(device)}
                        >
                          {t("settingsPanels.devices.unlink")}
                        </button>
                      </>
                    )}
                  </div>
                </div>
                <ul
                  className="companion-capabilities"
                  aria-label={t("settingsPanels.devices.grantedCapabilities")}
                >
                  {device.capabilities.map((capability) => (
                    <li className="companion-capability" key={capability}>
                      {t(capabilityLabels[capability])}
                    </li>
                  ))}
                </ul>
                {device.capabilities.includes("filesUpload") ? (
                  <p className="settings-row-description">
                    {t("settingsPanels.devices.filesUploadNote")}
                  </p>
                ) : null}
              </article>
            ))
        ) : (
          <div className="settings-card companion-empty-device">
            <h3 className="settings-row-title">{t("settingsPanels.devices.emptyTitle")}</h3>
            <p className="settings-row-description">{t("settingsPanels.devices.emptyBody")}</p>
          </div>
        )}
      </div>
    </section>
  );
}

function pairingLabel(t: ReturnType<typeof useT>, state?: CompanionPairingStatus["state"]) {
  if (state === "waitingForApproval") return t("settingsPanels.devices.stateWaiting");
  if (state === "approved") return t("settingsPanels.devices.stateApproved");
  if (state === "expired") return t("settingsPanels.devices.stateExpired");
  return t("settingsPanels.devices.stateScan");
}

function formatDate(value: string) {
  return formatLocaleDate(new Date(value), { dateStyle: "medium", timeStyle: "short" });
}

function errorMessage(error: unknown) {
  if (typeof error === "object" && error && "message" in error) {
    const message = (error as { message?: unknown }).message;
    if (typeof message === "string") return message;
  }
  return translate("settingsPanels.devices.unavailable");
}
