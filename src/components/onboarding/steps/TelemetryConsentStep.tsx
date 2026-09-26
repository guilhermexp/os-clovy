import { useState } from "react";
import { dispatchP3aSettingsChanged, TELEMETRY_INFO_URL } from "../../../lib/p3a";
import { useT, useTRich } from "../../../i18n";
import { setP3aEnabled } from "../../../lib/tauri";
import { Switch } from "../../ui/Switch";
import { StepActions, StepCard } from "../StepChrome";

export function TelemetryConsentStep({ onContinue }: { onContinue: () => void }) {
  const t = useT();
  const tr = useTRich();
  const [enabled, setEnabled] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string>();

  async function continueWithChoice() {
    setSaving(true);
    setError(undefined);
    try {
      const response = await setP3aEnabled(enabled);
      dispatchP3aSettingsChanged(response.settings);
      onContinue();
    } catch {
      setError(t("onboarding.telemetry.saveError"));
    } finally {
      setSaving(false);
    }
  }

  return (
    <StepCard
      title={t("onboarding.telemetry.title")}
      subtitle={t("onboarding.telemetry.subtitle")}
      wide
      className="onboarding-card-privacy"
    >
      <div className="onboarding-privacy-choice settings-card">
        <div className="settings-rows">
          <div className="settings-row">
            <div className="settings-row-info">
              <h2 className="settings-row-title">{t("onboarding.telemetry.rowTitle")}</h2>
              <p className="settings-row-description">
                {tr("onboarding.telemetry.rowDescription", {
                  link: (chunks) => (
                    <a
                      className="settings-inline-link"
                      href={TELEMETRY_INFO_URL}
                      target="_blank"
                      rel="noreferrer"
                    >
                      {chunks}
                    </a>
                  ),
                })}
              </p>
            </div>
            <div className="settings-row-control">
              <Switch
                checked={enabled}
                disabled={saving}
                aria-label={t("onboarding.telemetry.rowTitle")}
                onCheckedChange={setEnabled}
              />
            </div>
          </div>
        </div>
      </div>
      {error ? <p className="welcome-status">{error}</p> : null}
      <StepActions
        continueLabel={saving ? t("onboarding.telemetry.saving") : t("common.continue")}
        continueDisabled={saving}
        onContinue={() => void continueWithChoice()}
      />
    </StepCard>
  );
}
