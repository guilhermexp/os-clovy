import { type MessageKey, t } from "../../i18n";

export type SettingsTab =
  | "general"
  | "appearance"
  | "billing"
  | "shortcuts"
  | "dictation"
  | "audio"
  | "models"
  | "agent"
  | "memory"
  | "connectors"
  | "linked-devices"
  | "about";

const SETTINGS_TAB_LABEL_KEYS: Record<SettingsTab, MessageKey> = {
  general: "settings.tabs.general",
  appearance: "settings.appearance.title",
  billing: "settings.tabs.billing",
  shortcuts: "settings.tabs.shortcuts",
  dictation: "settings.tabs.dictation",
  audio: "settings.tabs.audio",
  models: "settings.tabs.models",
  agent: "settings.tabs.agent",
  memory: "settings.tabs.memory",
  connectors: "settings.tabs.connectors",
  "linked-devices": "settings.tabs.linkedDevices",
  about: "settings.tabs.about",
};

// `label` is a getter so it reads the current interface language each time a
// nav renders, instead of freezing whichever language was active at import.
function settingsTab(id: SettingsTab): { id: SettingsTab; label: string } {
  return {
    id,
    get label() {
      return t(SETTINGS_TAB_LABEL_KEYS[id]);
    },
  };
}

export const SETTINGS_TABS: { id: SettingsTab; label: string }[] = [
  settingsTab("general"),
  settingsTab("appearance"),
  settingsTab("billing"),
  settingsTab("shortcuts"),
  settingsTab("dictation"),
  settingsTab("audio"),
  settingsTab("models"),
  settingsTab("agent"),
  settingsTab("memory"),
  settingsTab("connectors"),
  settingsTab("linked-devices"),
  settingsTab("about"),
];

export function settingsTabsForCompanionPairing(companionPairingEnabled: boolean) {
  return SETTINGS_TABS.filter((tab) => companionPairingEnabled || tab.id !== "linked-devices");
}
