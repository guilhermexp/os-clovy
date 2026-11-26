import { useEffect, useId, useState } from "react";
import { listProviderModels } from "../../lib/tauri";
import type {
  DictationHudPlacement,
  DictationSettingsDto,
  DictationStyle,
  ProviderModelDto,
} from "../../lib/tauri";
import {
  PROVIDER_MODEL_SETTINGS_CHANGED_EVENT,
  type ProviderModelSettingsChangedDetail,
} from "../../lib/model-privacy";
import { SegmentedControl } from "../ui/SegmentedControl";
import { Switch } from "../ui/Switch";
import { type MessageKey, useT } from "../../i18n";

const STYLE_OPTIONS = [
  { value: "standard" as const, labelKey: "settings.style.standard" },
  { value: "casualLowercase" as const, labelKey: "settings.style.casual" },
  { value: "formal" as const, labelKey: "settings.style.formal" },
] satisfies { value: DictationStyle; labelKey: MessageKey }[];

const HUD_PLACEMENT_OPTIONS = [
  { value: "bottom" as const, labelKey: "settings.style.hudPlacementBottom" },
  { value: "top" as const, labelKey: "settings.style.hudPlacementTop" },
] satisfies { value: DictationHudPlacement; labelKey: MessageKey }[];

const SAMPLES: Record<DictationStyle, { description: MessageKey; sample: MessageKey }> = {
  standard: {
    description: "settings.style.standardDescription",
    sample: "settings.style.standardSample",
  },
  casualLowercase: {
    description: "settings.style.casualDescription",
    sample: "settings.style.casualSample",
  },
  formal: {
    description: "settings.style.formalDescription",
    sample: "settings.style.formalSample",
  },
};

type CleanupModelOption = { id: string; name: string };

/** Dictation cleanup uses OpenRouter chat completions with catalog ids of the form `vendor/model`. */
function cleanupModelOptions(models: ProviderModelDto[]): CleanupModelOption[] {
  return models
    .filter(
      (model) =>
        model.provider === "openrouter" &&
        /^[^/\s:]+\/\S+$/.test(model.id) &&
        !model.id.startsWith("__"),
    )
    .map((model) => ({ id: model.id, name: model.name.trim() || model.id }));
}

type CleanupControl = {
  enabled?: boolean;
  readFailed: boolean;
  saving: boolean;
  error?: string;
  onToggle: (enabled: boolean) => void;
};

export function DictationCleanupRow({
  enabled,
  readFailed,
  saving,
  error,
  onToggle,
}: CleanupControl) {
  const t = useT();
  const titleId = useId();
  const descriptionId = useId();
  const statusId = useId();

  return (
    <div className="settings-row">
      <div className="settings-row-info">
        <h3 id={titleId} className="settings-row-title">
          {t("settings.style.cleanup")}
        </h3>
        <p id={descriptionId} className="settings-row-description">
          {t("settings.style.cleanupDescription")}
        </p>
        {readFailed ? (
          <p id={statusId} className="settings-row-error" role="status">
            {t("settings.style.readFailed")}
          </p>
        ) : error ? (
          <p id={statusId} className="settings-row-error" role="alert">
            {t("settings.style.saveFailed")} {error}
          </p>
        ) : enabled === undefined || saving ? (
          <p id={statusId} className="settings-row-description" role="status">
            {t(enabled === undefined ? "settings.style.loading" : "settings.style.saving")}
          </p>
        ) : null}
      </div>
      {enabled !== undefined ? (
        <div className="settings-row-control">
          <Switch
            checked={enabled}
            disabled={saving}
            onCheckedChange={onToggle}
            aria-labelledby={titleId}
            aria-describedby={saving || error ? `${descriptionId} ${statusId}` : descriptionId}
          />
        </div>
      ) : null}
    </div>
  );
}

type StyleSettingsProps = {
  cleanupControl: CleanupControl;
  settings?: Pick<DictationSettingsDto, "style" | "cleanupModel" | "hudPlacement">;
  onModelChange: (model: string) => void;
  onStyleChange: (style: DictationStyle) => void;
  onHudPlacementChange: (placement: DictationHudPlacement) => void;
};

export function StyleSettingsSection({
  cleanupControl,
  settings,
  onModelChange,
  onStyleChange,
  onHudPlacementChange,
}: StyleSettingsProps) {
  const t = useT();
  const { enabled: cleanupEnabled, saving } = cleanupControl;
  const [models, setModels] = useState<CleanupModelOption[]>([]);

  useEffect(() => {
    if (!cleanupEnabled) return;
    let cancelled = false;
    let request = 0;
    const reload = () => {
      const current = ++request;
      listProviderModels("generation")
        .then((response) => {
          if (!cancelled && current === request) setModels(cleanupModelOptions(response.models));
        })
        .catch(() => {
          // The saved cleanup model remains available when the catalog fails.
          if (!cancelled && current === request) setModels([]);
        });
    };
    const changed = (event: Event) => {
      if ((event as CustomEvent<ProviderModelSettingsChangedDetail>).detail.mode === "generation")
        reload();
    };
    reload();
    window.addEventListener(PROVIDER_MODEL_SETTINGS_CHANGED_EVENT, changed);
    return () => {
      cancelled = true;
      window.removeEventListener(PROVIDER_MODEL_SETTINGS_CHANGED_EVENT, changed);
    };
  }, [cleanupEnabled]);

  const current = settings ? SAMPLES[settings.style] : undefined;
  const cleanupModel = settings?.cleanupModel;
  const modelOptions =
    cleanupModel && !models.some((model) => model.id === cleanupModel)
      ? [{ id: cleanupModel, name: cleanupModel }, ...models]
      : models;

  return (
    <section className="settings-group" aria-labelledby="style-heading">
      <h2 id="style-heading" className="settings-group-heading">
        {t("settings.style.title")}
      </h2>
      <div className="settings-card">
        <div className="settings-rows">
          <div className="settings-row">
            <div className="settings-row-info">
              <h3 id="dictation-hud-placement-title" className="settings-row-title">
                {t("settings.style.hudPlacement")}
              </h3>
              <p className="settings-row-description">
                {t("settings.style.hudPlacementDescription")}
              </p>
            </div>
            {settings ? (
              <div className="settings-row-control">
                <SegmentedControl
                  value={settings.hudPlacement}
                  onValueChange={onHudPlacementChange}
                  disabled={saving}
                  options={HUD_PLACEMENT_OPTIONS.map((option) => ({
                    value: option.value,
                    label: t(option.labelKey),
                  }))}
                  aria-label={t("settings.style.hudPlacement")}
                />
              </div>
            ) : null}
          </div>
          <DictationCleanupRow {...cleanupControl} />
          {cleanupEnabled && settings && current ? (
            <>
              <div className="settings-row">
                <div className="settings-row-info">
                  <h3 className="settings-row-title">{t("settings.style.cleanupModel")}</h3>
                  <p className="settings-row-description">
                    {t("settings.style.cleanupModelDescription")}
                  </p>
                </div>
                <div className="settings-row-control">
                  <select
                    className="dialog-input"
                    aria-label={t("settings.style.cleanupModel")}
                    value={cleanupModel}
                    disabled={saving}
                    onChange={(event) => onModelChange(event.target.value)}
                  >
                    {modelOptions.map((model) => (
                      <option key={model.id} value={model.id}>
                        {model.name}
                      </option>
                    ))}
                  </select>
                </div>
              </div>
              <div className="settings-row">
                <div className="settings-row-info">
                  <h3 className="settings-row-title">{t("settings.style.outputStyle")}</h3>
                  <p className="settings-row-description">{t(current.description)}</p>
                </div>
                <div className="settings-row-control">
                  <SegmentedControl
                    value={settings.style}
                    onValueChange={onStyleChange}
                    disabled={saving}
                    options={STYLE_OPTIONS.map((option) => ({
                      value: option.value,
                      label: t(option.labelKey),
                    }))}
                    aria-label={t("settings.style.aria")}
                  />
                </div>
              </div>
            </>
          ) : null}
        </div>
        {cleanupEnabled && current ? (
          <div className="style-preview" aria-live="polite">
            <p className="style-preview-text">{t(current.sample)}</p>
          </div>
        ) : null}
      </div>
    </section>
  );
}
