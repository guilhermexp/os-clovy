import { useEffect, useState } from "react";
import { dictationSettings, setDictationStyle } from "../../lib/tauri";
import type { DictationStyle } from "../../lib/tauri";
import { SegmentedControl } from "../ui/SegmentedControl";
import { type MessageKey, useT } from "../../i18n";

const STYLE_OPTIONS = [
  { value: "standard" as const, labelKey: "settings.style.standard" },
  { value: "casualLowercase" as const, labelKey: "settings.style.casual" },
  { value: "formal" as const, labelKey: "settings.style.formal" },
] satisfies { value: DictationStyle; labelKey: MessageKey }[];

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

export function StyleSettingsSection() {
  const t = useT();
  const [style, setStyle] = useState<DictationStyle>("standard");
  const [error, setError] = useState<string>();

  useEffect(() => {
    let cancelled = false;
    dictationSettings()
      .then((response) => {
        if (!cancelled) setStyle(response.settings.style);
      })
      .catch((caught: unknown) => {
        if (!cancelled) setError(messageFromError(caught));
      });
    return () => {
      cancelled = true;
    };
  }, []);

  async function selectStyle(nextStyle: DictationStyle) {
    setStyle(nextStyle);
    try {
      const next = await setDictationStyle(nextStyle);
      setStyle(next.style);
      setError(undefined);
    } catch (caught) {
      setError(messageFromError(caught));
    }
  }

  const current = SAMPLES[style];

  return (
    <section className="settings-group" aria-labelledby="style-heading">
      <h2 id="style-heading" className="settings-group-heading">
        {t("settings.style.title")}
      </h2>
      <div className="settings-card">
        <div className="settings-rows">
          <div className="settings-row">
            <div className="settings-row-info">
              <h3 className="settings-row-title">{t("settings.style.outputStyle")}</h3>
              <p className="settings-row-description">{t(current.description)}</p>
              {error ? <p className="settings-row-error">{error}</p> : null}
            </div>
            <div className="settings-row-control">
              <SegmentedControl
                value={style}
                onValueChange={(value) => void selectStyle(value)}
                options={STYLE_OPTIONS.map((option) => ({
                  value: option.value,
                  label: t(option.labelKey),
                }))}
                aria-label={t("settings.style.aria")}
              />
            </div>
          </div>
        </div>
        <div className="style-preview" aria-live="polite">
          <p className="style-preview-text">{t(current.sample)}</p>
        </div>
      </div>
    </section>
  );
}

function messageFromError(caught: unknown) {
  if (caught && typeof caught === "object" && "message" in caught) {
    return String((caught as { message: unknown }).message);
  }
  return String(caught);
}
