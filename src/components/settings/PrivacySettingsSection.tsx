import { useEffect, useState } from "react";
import { dispatchP3aSettingsChanged, TELEMETRY_INFO_URL } from "../../lib/p3a";
import { p3aSettings, setP3aEnabled, type P3aSettingsDto } from "../../lib/tauri";
import { Switch } from "../ui/Switch";
import { t as translate, useT } from "../../i18n";

const DEFAULT_P3A_SETTINGS: P3aSettingsDto = {
  enabled: false,
  consentVersion: 1,
  consentedAtWeek: null,
};

export function PrivacySettingsSection() {
  const t = useT();
  const [settings, setSettings] = useState<P3aSettingsDto>(DEFAULT_P3A_SETTINGS);
  const [saving, setSaving] = useState(false);
  const [status, setStatus] = useState<string>();

  useEffect(() => {
    let cancelled = false;
    p3aSettings()
      .then((response) => {
        if (!cancelled) setSettings(response.settings);
      })
      .catch(() => {
        if (!cancelled) setStatus(translate("settings.privacy.loadError"));
      });
    return () => {
      cancelled = true;
    };
  }, []);

  async function toggleUsageStatistics(enabled: boolean) {
    setSaving(true);
    setStatus(undefined);
    try {
      const response = await setP3aEnabled(enabled);
      setSettings(response.settings);
      dispatchP3aSettingsChanged(response.settings);
      setStatus(enabled ? t("settings.privacy.on") : t("settings.privacy.off"));
    } catch {
      setStatus(t("settings.privacy.updateError"));
    } finally {
      setSaving(false);
    }
  }

  return (
    <section className="settings-group" aria-labelledby="general-privacy-heading">
      <h2 id="general-privacy-heading" className="settings-group-heading">
        {t("settings.privacy.title")}
      </h2>
      <p className="settings-group-description">{t("settings.privacy.description")}</p>
      <div className="settings-card">
        <div className="settings-rows">
          <div className="settings-row">
            <div className="settings-row-info">
              <h3 className="settings-row-title">{t("settings.privacy.share")}</h3>
              <p className="settings-row-description">
                {t("settings.privacy.shareDescription")}{" "}
                <a
                  className="settings-inline-link"
                  href={TELEMETRY_INFO_URL}
                  target="_blank"
                  rel="noreferrer"
                >
                  {t("settings.privacy.learnHow")}
                </a>
              </p>
            </div>
            <div className="settings-row-control">
              <Switch
                checked={settings.enabled}
                disabled={saving}
                aria-label={t("settings.privacy.share")}
                onCheckedChange={(enabled) => void toggleUsageStatistics(enabled)}
              />
            </div>
          </div>
        </div>
      </div>
      {status ? (
        <p className="settings-status" role="status">
          {status}
        </p>
      ) : null}
    </section>
  );
}
