import { IconChevronDownSmall } from "central-icons/IconChevronDownSmall";
import { useEffect, useId, useRef, useState } from "react";
import { useLocale, useT } from "../../i18n";
import { errorCode, messageFromError } from "../../lib/errors";
import {
  type LocalTranscriptionModelStatus,
  type ProviderModelDto,
  type ProviderModelSettingsDto,
  cancelLocalTranscriptionModelInstall,
  installLocalTranscriptionModel,
  localTranscriptionModelState,
  onLocalTranscriptionModelState,
  removeLocalTranscriptionModel,
} from "../../lib/tauri";
import { ConfirmDialog } from "../ui/ConfirmDialog";
import "./local-transcription-settings.css";

type TranscriptionSelection = Pick<
  ProviderModelSettingsDto,
  "transcriptionProvider" | "transcriptionModel"
>;

export const DEFAULT_GROQ_TRANSCRIPTION_MODEL = "whisper-large-v3";

const remoteDefaults = {
  groq: DEFAULT_GROQ_TRANSCRIPTION_MODEL,
  openrouter: "openai/whisper-large-v3-turbo",
};

const downloadLabels = {
  install: ["settings.localTranscription.install", "settings.localTranscription.installModel"],
  cancel: ["settings.localTranscription.cancel", "settings.localTranscription.cancelModel"],
  retry: ["settings.localTranscription.retry", "settings.localTranscription.retryModel"],
} as const;

export function LocalTranscriptionSettings({
  supported,
  provider,
  model,
  models = [],
  openrouterKeyConfigured,
  onSelect,
  onActivated,
  onRemoved,
  readOnly = false,
}: {
  supported: boolean;
  provider: string;
  model: string;
  models?: ProviderModelDto[];
  openrouterKeyConfigured?: boolean;
  onSelect: (model: string) => Promise<void>;
  onActivated: (selection: TranscriptionSelection) => void;
  onRemoved: (modelId: string, result?: { switched_to_groq: boolean }) => Promise<void>;
  readOnly?: boolean;
}) {
  const t = useT();
  const locale = useLocale();
  const managementId = useId();
  const [managing, setManaging] = useState(false);
  const [entries, setEntries] = useState<LocalTranscriptionModelStatus[]>([]);
  const [choosingLocal, setChoosingLocal] = useState(false);
  const [pending, setPending] = useState<string>();
  const [saving, setSaving] = useState(false);
  const [confirmRemoval, setConfirmRemoval] = useState<LocalTranscriptionModelStatus>();
  const [removing, setRemoving] = useState<string>();
  const [error, setError] = useState<string>();
  const [notice, setNotice] = useState<string>();
  const revision = useRef(0);
  const lastRemote = useRef({ ...remoteDefaults });
  const activeEntry = entries.find((entry) => entry.modelId === model);
  const activeName =
    activeEntry?.name ??
    models.find((entry) => entry.id === model)?.name ??
    model.replace(/^openai\//, "");
  const selectedProvider = choosingLocal ? "local" : provider;
  const selectedModel = selectedProvider === provider ? model : "";
  const pendingEntry = entries.find((entry) => entry.modelId === pending);

  useEffect(() => {
    if (provider === "groq" || provider === "openrouter") lastRemote.current[provider] = model;
  }, [provider, model]);

  useEffect(() => {
    if (!supported) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    const received = new Map<string, LocalTranscriptionModelStatus>();
    void (async () => {
      try {
        const stop = await onLocalTranscriptionModelState((entry) => {
          if (disposed) return;
          received.set(entry.modelId, entry);
          setEntries((current) =>
            current.some((item) => item.modelId === entry.modelId)
              ? current.map((item) => (item.modelId === entry.modelId ? entry : item))
              : [...current, entry],
          );
        });
        if (disposed) {
          stop();
          return;
        }
        unlisten = stop;
        const snapshot = await localTranscriptionModelState();
        if (!disposed) {
          setEntries(snapshot.map((entry) => received.get(entry.modelId) ?? entry));
        }
      } catch (cause) {
        if (!disposed) setError(messageFromError(cause));
      }
    })();
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [supported]);

  const size = (bytes: number) =>
    `${new Intl.NumberFormat(locale, { maximumFractionDigits: 1 }).format(bytes / 1_000_000)} MB`;

  async function select(id: string) {
    revision.current += 1;
    setPending(undefined);
    setError(undefined);
    setNotice(undefined);
    setSaving(true);
    try {
      await onSelect(id);
      setChoosingLocal(false);
    } catch (cause) {
      setError(messageFromError(cause));
    } finally {
      setSaving(false);
    }
  }

  async function install(entry: LocalTranscriptionModelStatus, activate: boolean) {
    const attempt = activate ? ++revision.current : undefined;
    if (activate) setPending(entry.modelId);
    setError(undefined);
    setNotice(undefined);
    try {
      const result = await installLocalTranscriptionModel(entry.modelId, activate);
      if (attempt === revision.current) {
        if (result.settings) {
          onActivated(result.settings);
          setChoosingLocal(false);
        }
        setPending(undefined);
      }
    } catch (cause) {
      if (attempt !== undefined && attempt !== revision.current) return;
      // File and network errors are announced on the model row.
      if (errorCode(cause) !== "local_model_install_failed") setError(messageFromError(cause));
    }
  }

  async function cancel(id: string) {
    const attempt = revision.current;
    try {
      await cancelLocalTranscriptionModelInstall(id);
      if (pending === id && attempt === revision.current) {
        revision.current += 1;
        setPending(undefined);
      }
    } catch (cause) {
      setError(messageFromError(cause));
    }
  }

  async function remove(entry: LocalTranscriptionModelStatus, confirmed: boolean) {
    setRemoving(entry.modelId);
    setError(undefined);
    setNotice(undefined);
    try {
      const result = await removeLocalTranscriptionModel(entry.modelId, confirmed);
      if (result.switched_to_groq) {
        setChoosingLocal(false);
        setNotice(t("settings.localTranscription.removed"));
      }
      await onRemoved(entry.modelId, result);
    } catch (cause) {
      await onRemoved(entry.modelId).catch(() => undefined);
      setError(messageFromError(cause));
    } finally {
      setRemoving(undefined);
    }
  }

  function statusText(entry: LocalTranscriptionModelStatus) {
    switch (entry.status.state) {
      case "ready":
        return t("settings.localTranscription.ready", { size: size(entry.installedBytes) });
      case "downloading":
        return `${size(entry.status.received)} / ${size(entry.status.total)}`;
      default:
        return entry.installedBytes > 0
          ? t("settings.localTranscription.partial", { size: size(entry.installedBytes) })
          : t("settings.localTranscription.available", { size: size(entry.downloadBytes) });
    }
  }

  function downloadActions(entry: LocalTranscriptionModelStatus, activate: boolean) {
    const status = entry.status;
    let action: keyof typeof downloadLabels = "install";
    if (status.state === "downloading") action = "cancel";
    if (status.state === "failed" || status.state === "ready") action = "retry";
    const [label, accessibleLabel] = downloadLabels[action];
    return (
      <>
        {status.state === "downloading" && (
          <progress
            aria-label={t("settings.localTranscription.progress", { model: entry.name })}
            max={status.total}
            value={status.received}
          />
        )}
        {status.state === "failed" && (
          <p className="settings-row-description" role="alert">
            {status.reason.startsWith("network: ")
              ? t("settings.localTranscription.networkFailed", { file: status.reason.slice(9) })
              : t("settings.localTranscription.failed", { reason: status.reason })}
          </p>
        )}
        {entry.compatible && (status.state !== "ready" || activate) && (
          <button
            type="button"
            className="btn btn-secondary"
            disabled={readOnly || removing === entry.modelId}
            aria-label={t(accessibleLabel, { model: entry.name })}
            onClick={() => {
              if (status.state === "downloading") void cancel(entry.modelId);
              else void install(entry, activate);
            }}
          >
            {t(label)}
          </button>
        )}
      </>
    );
  }

  function modelOptionLabel(entry: LocalTranscriptionModelStatus) {
    if (!entry.compatible) {
      const requirement = t("settings.localTranscription.requiresMacos", {
        version: entry.minimumMacos,
      });
      return `${entry.name} (${requirement})`;
    }
    if (entry.status.state === "ready") return entry.name;
    return `${entry.name} (${size(entry.downloadBytes)})`;
  }

  const remoteModels = models.filter((entry) => entry.provider === selectedProvider);
  return (
    <section
      className="local-transcription-settings"
      aria-label={t("settings.models.transcription")}
    >
      <div className="local-transcription-selects">
        <label>
          <span>{t("settings.localTranscription.provider")}</span>
          <select
            value={selectedProvider}
            disabled={readOnly || saving}
            onChange={(event) => {
              const value = event.currentTarget.value;
              if (value === "local") setChoosingLocal(true);
              else if (value === "groq" || value === "openrouter")
                void select(lastRemote.current[value]);
            }}
          >
            <option value="groq">Groq</option>
            <option value="openrouter">OpenRouter</option>
            {supported && <option value="local">Local</option>}
          </select>
        </label>
        <label>
          <span>{t("settings.localTranscription.model")}</span>
          <select
            value={selectedModel}
            disabled={readOnly || saving || (selectedProvider === "local" && !entries.length)}
            onChange={(event) => {
              const id = event.currentTarget.value;
              const entry = entries.find((candidate) => candidate.modelId === id);
              if (entry && entry.status.state !== "ready") void install(entry, true);
              else void select(id);
            }}
          >
            {!selectedModel && (
              <option value="" disabled>
                {t("settings.localTranscription.chooseModel")}
              </option>
            )}
            {selectedProvider === "local" ? (
              entries.map((entry) => (
                <option key={entry.modelId} value={entry.modelId} disabled={!entry.compatible}>
                  {modelOptionLabel(entry)}
                </option>
              ))
            ) : (
              <>
                {!remoteModels.some((entry) => entry.id === selectedModel) && (
                  <option value={selectedModel}>{activeName}</option>
                )}
                {remoteModels.map((entry) => (
                  <option key={entry.id} value={entry.id}>
                    {entry.name}
                  </option>
                ))}
              </>
            )}
          </select>
        </label>
      </div>
      {selectedProvider === "openrouter" && !openrouterKeyConfigured && (
        <p className="settings-row-description">{t("settings.localTranscription.openrouterKey")}</p>
      )}
      {choosingLocal && !pending && provider !== "local" && (
        <p className="settings-row-description" role="status">
          {t("settings.localTranscription.stillActive", { model: activeName })}
        </p>
      )}
      {pendingEntry && (
        <div className="local-transcription-model-status">
          <p className="settings-row-description" role="status">
            {t("settings.localTranscription.pending", {
              pending: pendingEntry.name,
              status: statusText(pendingEntry),
              model: activeName,
            })}
          </p>
          {downloadActions(pendingEntry, true)}
          <button
            type="button"
            className="btn btn-secondary"
            disabled={readOnly || saving}
            aria-label={t("settings.localTranscription.keepActive", { model: activeName })}
            onClick={() => void select(model)}
          >
            {t("settings.localTranscription.keepCurrent")}
          </button>
        </div>
      )}
      {provider === "local" && activeEntry && !pending && !choosingLocal && (
        <div className="local-transcription-model-status">
          <p className="settings-row-description" role="status">
            {activeEntry.compatible
              ? statusText(activeEntry)
              : t("settings.localTranscription.requiresMacos", {
                  version: activeEntry.minimumMacos,
                })}
          </p>
          {!managing && downloadActions(activeEntry, false)}
        </div>
      )}
      {supported && (
        <div className="local-transcription-management">
          <button
            type="button"
            className="local-transcription-manage-trigger"
            aria-expanded={managing}
            aria-controls={managementId}
            onClick={() => setManaging((open) => !open)}
          >
            {t("settings.localTranscription.manage")}
            <IconChevronDownSmall aria-hidden />
          </button>
          <div id={managementId} hidden={!managing}>
            {managing && !entries.length && (
              <p className="settings-row-description" role="status">
                {t("settings.localTranscription.loading")}
              </p>
            )}
            {managing && (
              <ul>
                {entries.map((entry) => {
                  const active = provider === "local" && model === entry.modelId;
                  return (
                    <li key={entry.modelId}>
                      <div className="local-transcription-model-info">
                        <span className="settings-row-title">{entry.name}</span>
                        <span className="settings-row-description">
                          {entry.compatible
                            ? statusText(entry)
                            : t("settings.localTranscription.requiresMacos", {
                                version: entry.minimumMacos,
                              })}
                          {active && ` · ${t("settings.localTranscription.active")}`}
                        </span>
                      </div>
                      <div className="local-transcription-model-status">
                        {pending !== entry.modelId && downloadActions(entry, false)}
                        {entry.installedBytes > 0 && entry.status.state !== "downloading" && (
                          <button
                            type="button"
                            className="btn btn-secondary"
                            aria-label={t("settings.localTranscription.removeModel", {
                              model: entry.name,
                            })}
                            disabled={readOnly || removing !== undefined}
                            onClick={() => {
                              if (active) setConfirmRemoval(entry);
                              else void remove(entry, false);
                            }}
                          >
                            {t("settings.localTranscription.remove")}
                          </button>
                        )}
                      </div>
                    </li>
                  );
                })}
              </ul>
            )}
          </div>
        </div>
      )}
      {error && (
        <p className="settings-row-description" role="alert">
          {error}
        </p>
      )}
      {notice && (
        <p className="settings-row-description" role="status">
          {notice}
        </p>
      )}
      <ConfirmDialog
        open={confirmRemoval !== undefined}
        title={t("settings.localTranscription.removeModel", { model: confirmRemoval?.name ?? "" })}
        description={t("settings.localTranscription.removeActiveDescription")}
        confirmLabel={t("settings.localTranscription.removeAndSwitch")}
        destructive
        onClose={() => setConfirmRemoval(undefined)}
        onConfirm={async () => {
          if (confirmRemoval) await remove(confirmRemoval, true);
        }}
      />
    </section>
  );
}
