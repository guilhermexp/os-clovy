import { listen } from "@tauri-apps/api/event";
import { IconCheckmark2Small } from "central-icons/IconCheckmark2Small";
import { IconChevronDownSmall } from "central-icons/IconChevronDownSmall";
import { IconCircleCheck } from "central-icons/IconCircleCheck";
import { IconCircleQuestionmark } from "central-icons/IconCircleQuestionmark";
import { IconCircleX } from "central-icons/IconCircleX";
import { IconExclamationCircle } from "central-icons/IconExclamationCircle";
import { IconMoonStar } from "central-icons/IconMoonStar";
import { IconSun } from "central-icons/IconSun";
import { IconTelevision } from "central-icons/IconTelevision";
import { useEffect, useMemo, useRef, useState } from "react";
import type { CSSProperties, ReactNode, RefObject } from "react";
import {
  INTERFACE_LOCALE_OPTIONS,
  type InterfaceLocale,
  type MessageKey,
  setInterfaceLocale,
  t as translate,
  type TFunction,
  useLocale,
  useT,
} from "../../i18n";
import {
  CLOVY_COMMUNITY_URL,
  dictationHotkeyStatus,
  dictationHelperCommand,
  dictationSettings,
  listVeniceModels,
  localAudioFileSrc,
  providerModelSettings,
  clovyOpenCommunityPage,
  clovyOpenVerifyPage,
  clearVeniceApiKey,
  setDictationLanguage,
  setDictationMicrophone,
  setDictationShortcut,
  setImageSafeMode,
  setLiveTranscription,
  setCostQuality,
  setVeniceApiKey,
  setVeniceModel,
  connectorsApplyRuntime,
  unpackBundledExtension,
} from "../../lib/tauri";
import { LANGUAGE_OPTIONS, languageLabel } from "../../lib/dictation-languages";
import { autostartEnabled, autostartSupported, setAutostartEnabled } from "../../lib/autostart";
import { replayOnboarding } from "../../lib/onboarding";
import type {
  AccountStatus,
  DictationHelperEvent,
  DictationMicrophoneDeviceDto,
  DictationShortcutKind,
  DictationSettingsDto,
  DictationShortcutModifiers,
  DictationShortcutSetting,
  FolderDto,
  ProviderModelMode,
  ProviderModelSettingsDto,
  RecordingSourceMode,
  RecordingSourceReadinessDto,
  VeniceModelDto,
} from "../../lib/tauri";
import { AccountSettingsSection, BillingSettingsSection } from "../account/AccountSettings";
import { KeycapShortcut } from "../shortcuts/KeycapShortcut";
import { chordFromKeyEvent, shortcutFromCapturePayload } from "../shortcuts/use-shortcut-capture";
import {
  Select,
  selectPopoverPlacement,
  selectPopoverStyle,
  type SelectPopoverPlacement,
} from "../ui/Select";
import { SegmentedControl } from "../ui/SegmentedControl";
import { InlineNotice } from "../ui/InlineNotice";
import { Switch } from "../ui/Switch";
import { HoverTip } from "../ui/HoverTip";
import { toast } from "../ui/Toaster";
import { APP_COMMIT_HASH, APP_VERSION } from "../../app/build-info";
import type { ReportCategory } from "../agent/composer/reportCategory";
import { getStoredTheme, setStoredTheme, type ThemePreference } from "../../lib/theme";
import { BRAND_PRESETS, getStoredBrand, setStoredBrand, type BrandId } from "../../lib/brand";
import {
  FONT_SCALE_PRESETS,
  setStoredFontScale,
  useFontScaleId,
  type FontScaleId,
} from "../../lib/font-scale";
import {
  getReleaseChannel,
  reconcileToStable,
  setReleaseChannel,
  type ReleaseChannel,
} from "../../lib/updater";
import {
  fallbackDictationCapabilities,
  isSystemAudioSupportedPlatform,
  useDictationCapabilities,
} from "../../lib/platform";
import { systemAudioAvailability } from "../../lib/source-readiness";
import { parseDictationHelperEvent } from "../../lib/dictation-events";
import {
  dispatchProviderModelSettingsChanged,
  modelAvailableForMode,
} from "../../lib/model-privacy";
import { localGenerationOptionId, withLocalGenerationOption } from "../../lib/local-generation";
import { LlmProvidersSection } from "./LlmProvidersSection";
import { ProviderLogo } from "./ProviderLogo";
import { AUTO_MODEL_ID, modelOptions, selectedModel } from "./ModelPickerDialog";
import { ConfirmDialog } from "../ui/ConfirmDialog";
import {
  DEFAULT_GENERATION_SUGGESTION_ID,
  suggestedModelsForMode,
} from "../../lib/suggested-models";
import {
  AUTO_PREFERENCE_VALUES,
  autoPreferenceFromCostQuality,
  ModelPickerCardContent,
  ModelPickerPopover,
  type AutoPreference,
  type ModelPickerFlyout,
} from "./ModelPickerPopover";
import { DEFAULT_IMAGE_MODEL, imageModelCatalog } from "../../lib/image-models";
import { IMAGE_GENERATION_ENABLED, VIDEO_GENERATION_ENABLED } from "../../lib/feature-flags";
import {
  INITIAL_EXPERIMENTAL_UNLOCK_CLICK_STATE,
  registerExperimentalUnlockClick,
  setExperimentalFlags,
  useExperimentalFlags,
} from "../../lib/experimental-flags";
import { DEFAULT_VIDEO_MODEL, VIDEO_MODELS } from "../../lib/video-models";
import { AgentSettingsSection } from "./AgentSettingsSection";
import { AgentMcpServersSection } from "./AgentMcpServersSection";
import { ConnectorsSection } from "./ConnectorsSection";
import { LinkedDevicesSection } from "./LinkedDevicesSection";
import { DictionarySettingsSection } from "./DictionarySettingsSection";
import { MemorySettingsSection } from "./MemorySettingsSection";
import { MicTestControl, type MicTestState } from "./MicTestControl";
import { StyleSettingsSection } from "./StyleSettingsSection";
import { PrivacySettingsSection } from "./PrivacySettingsSection";
import { SETTINGS_TABS, type SettingsTab } from "./settings-config";
import { DEFAULT_DATA_PARTITION, useCurrentDataPartitionName } from "../../lib/data-partition";
import {
  getStoredDateFormat,
  setStoredDateFormat,
  type DateFormatPreference,
} from "../../lib/date-format";

function themeOptions(t: TFunction): readonly {
  value: ThemePreference;
  label: ReactNode;
  ariaLabel: string;
}[] {
  return [
    {
      value: "system",
      label: (
        <>
          <IconTelevision size={14} />
          {t("settings.theme.system")}
        </>
      ),
      ariaLabel: t("settings.theme.systemAria"),
    },
    {
      value: "light",
      label: (
        <>
          <IconSun size={14} />
          {t("settings.theme.light")}
        </>
      ),
      ariaLabel: t("settings.theme.lightAria"),
    },
    {
      value: "dark",
      label: (
        <>
          <IconMoonStar size={14} />
          {t("settings.theme.dark")}
        </>
      ),
      ariaLabel: t("settings.theme.darkAria"),
    },
  ];
}

const FONT_SCALE_LABEL_KEYS = {
  default: "settings.textSize.default",
  large: "settings.textSize.large",
  larger: "settings.textSize.larger",
} as const satisfies Record<FontScaleId, MessageKey>;

function fontScaleOptions(t: TFunction): readonly {
  value: FontScaleId;
  label: ReactNode;
  ariaLabel: string;
}[] {
  return FONT_SCALE_PRESETS.map((preset) => {
    const label = t(FONT_SCALE_LABEL_KEYS[preset.id]);
    return {
      value: preset.id,
      label,
      ariaLabel: t("settings.textSize.optionAria", { size: label }),
    };
  });
}

function dateFormatOptions(t: TFunction) {
  return [
    { value: "system", label: t("settings.dateFormat.system") },
    { value: "month-first", label: t("settings.dateFormat.monthFirst") },
    { value: "day-first", label: t("settings.dateFormat.dayFirst") },
  ] satisfies { value: DateFormatPreference; label: string }[];
}

function releaseChannelOptions(t: TFunction): readonly {
  value: ReleaseChannel;
  label: ReactNode;
}[] {
  return [
    { value: "stable", label: t("settings.releaseChannel.stable") },
    { value: "rc", label: t("settings.releaseChannel.rc") },
  ];
}

function autoPreferenceOptions(t: TFunction): readonly {
  value: AutoPreference;
  label: ReactNode;
}[] {
  return [
    { value: "cost", label: t("settings.autoPreference.cost") },
    { value: "balanced", label: t("settings.autoPreference.balanced") },
    { value: "quality", label: t("settings.autoPreference.quality") },
  ];
}

const EMPTY_MODIFIERS: DictationShortcutModifiers = {
  command: false,
  control: false,
  option: false,
  shift: false,
  function: false,
};

const DEFAULT_SETTINGS: DictationSettingsDto = {
  pushToTalkShortcut: {
    keyCode: 0x02,
    code: "KeyD",
    label: "Ctrl+Opt+D",
    pressCount: 1,
    modifiers: {
      ...EMPTY_MODIFIERS,
      control: true,
      option: true,
    },
  },
  toggleShortcut: {
    keyCode: 0x11,
    code: "KeyT",
    label: "Ctrl+Opt+T",
    pressCount: 1,
    modifiers: {
      ...EMPTY_MODIFIERS,
      control: true,
      option: true,
    },
  },
  microphone: {},
  style: "standard",
  language: undefined,
};

const WINDOWS_DEFAULT_SHORTCUTS: Record<DictationShortcutKind, DictationShortcutSetting> = {
  push_to_talk: {
    ...DEFAULT_SETTINGS.pushToTalkShortcut,
    label: "Ctrl+Alt+D",
  },
  toggle: {
    ...DEFAULT_SETTINGS.toggleShortcut,
    label: "Ctrl+Alt+T",
  },
};

const DEFAULT_PROVIDER_MODELS: ProviderModelSettingsDto = {
  transcriptionProvider: "venice",
  generationProvider: "venice",
  transcriptionModel: "nvidia/parakeet-tdt-0.6b-v3",
  // Mirrors DEFAULT_GENERATION_MODEL in the Rust providers module and the
  // leading Suggested pick in lib/suggested-models.ts.
  generationModel: "zai-org-glm-5-2",
  // Mirrors DEFAULT_COST_QUALITY in the Rust providers module.
  costQuality: 100,
  remoteGenerationModel: "zai-org-glm-5-2",
  // Mirrors DEFAULT_IMAGE_MODEL in the Rust providers module.
  imageModel: DEFAULT_IMAGE_MODEL,
  // Mirrors DEFAULT_VIDEO_MODEL in the Rust providers module.
  videoModel: DEFAULT_VIDEO_MODEL,
  veniceApiKeyConfigured: false,
  localGeneration: {
    baseUrl: "",
    modelId: "",
  },
  // On by default, matching the Rust providers default.
  imageSafeMode: true,
  imageSafeModePromptDismissed: false,
  liveTranscription: true,
};

type ProviderModelSettingsSnapshot = {
  settings: ProviderModelSettingsDto;
  effectiveSettings?: ProviderModelSettingsDto;
};

function mergeProviderModelSettings(settings?: Partial<ProviderModelSettingsDto>) {
  return {
    ...DEFAULT_PROVIDER_MODELS,
    ...settings,
  };
}

function providerModelSettingsSnapshot(response: ProviderModelSettingsSnapshot) {
  const settings = mergeProviderModelSettings(response.settings);
  return {
    settings,
    effectiveSettings: mergeProviderModelSettings(response.effectiveSettings ?? response.settings),
  };
}

const MIC_TEST_DURATION_SECONDS = 5;

export type { SettingsTab };
// Same list (and translated `label` getters) as the sidebar's settings nav.
export { SETTINGS_TABS };

export function appSettingsTabsForCompanionPairing(companionPairingEnabled: boolean) {
  return SETTINGS_TABS.filter((tab) => companionPairingEnabled || tab.id !== "linked-devices");
}

/**
 * The shared settings page header (Codex-app style): a large serif page title
 * with one muted one-line blurb beneath, and generous space before the content.
 * Every settings panel opens with this so the page announces what it is; panels
 * with multiple sub-groups keep their small `.settings-group-heading` labels
 * below it.
 */
export function SettingsPageHeader({
  id,
  title,
  blurb,
}: {
  /** Ties the panel's `aria-labelledby` to the visible page title. */
  id?: string;
  title: ReactNode;
  blurb?: ReactNode;
}) {
  return (
    <header className="settings-page-header">
      <h2 id={id} className="settings-page-title">
        {title}
      </h2>
      {blurb ? <p className="settings-page-blurb">{blurb}</p> : null}
    </header>
  );
}

type AppSettingsProps = {
  folders?: FolderDto[];
  onFoldersImported?: (folders: FolderDto[]) => void;
  /** When Memory is opened from a project, the manager pre-filters to it. */
  memoryFolderFilter?: string;
  /** Drill from a memory's project tag into that project. */
  onOpenProject?: (folderId: string) => void;
  account: AccountStatus;
  accountLoading: boolean;
  sourceMode: RecordingSourceMode;
  sourceReadiness?: RecordingSourceReadinessDto;
  checkingSourceReadiness: boolean;
  microphonePermissionStatus?: string;
  accessibilityPermissionStatus?: string;
  onAccountChanged: (next: AccountStatus) => void;
  onAccountRefresh: () => Promise<AccountStatus | undefined>;
  onSourceModeChange: (mode: RecordingSourceMode) => void;
  onEnableMicrophone?: () => void;
  onEnableAccessibility?: () => void;
  onEnableSystemAudio: () => void;
  // When the host (the sidebar settings nav) drives the active section, it
  // passes both of these so AppSettings becomes a controlled panel and hides
  // its own header + in-page tab nav. Left undefined, AppSettings keeps its
  // own nav — the standalone path exercised by app-settings tests.
  activeTab?: SettingsTab;
  onTabChange?: (tab: SettingsTab) => void;
  // Reports when a drill-in detail (skill detail) is open so the host can hand
  // this view the scroll container, notes-style: the shell sets
  // data-note-detail-scroller on .main-panel-body and the detail owns its own
  // scroll region under a pinned breadcrumb bar.
  onDetailPinnedChange?: (pinned: boolean) => void;
  // Runs the app updater's manual check flow.
  onCheckForUpdates?: () => void;
  // True when an update is downloaded and waiting for a relaunch. The bundle
  // swap is what can kill the dictation helper, so the "dictation paused"
  // notice points the user at the relaunch that finishes the update.
  updateReadyToRelaunch?: boolean;
  // Relaunches Clovy to finish a staged update (also restores the helper).
  onRelaunch?: () => void;
  // Confirmed leave-rc reconcile: downloads and installs the current stable,
  // even if it is older than the running prerelease build (Q4-Q8).
  onReconcileToStable?: () => void;
  // Opens Agent with the direct issue report dialog preselected.
  onReportIssue?: (category: ReportCategory) => void;
  // Opens a new agent session that runs a skill bundle's slash command.
  onStartBundleChat?: (prompt: string) => void;
};

export function AppSettings({
  folders = [],
  onFoldersImported,
  memoryFolderFilter,
  onOpenProject,
  account,
  accountLoading,
  sourceMode,
  sourceReadiness,
  checkingSourceReadiness,
  microphonePermissionStatus,
  accessibilityPermissionStatus,
  onAccountChanged,
  onAccountRefresh,
  onSourceModeChange,
  onEnableMicrophone,
  onEnableAccessibility,
  onEnableSystemAudio,
  activeTab: controlledTab,
  onTabChange,
  onDetailPinnedChange,
  onCheckForUpdates,
  updateReadyToRelaunch,
  onRelaunch,
  onReconcileToStable,
  onReportIssue,
  onStartBundleChat,
}: AppSettingsProps) {
  const [settings, setSettings] = useState<DictationSettingsDto>(DEFAULT_SETTINGS);
  const [providerSettings, setProviderSettings] =
    useState<ProviderModelSettingsDto>(DEFAULT_PROVIDER_MODELS);
  const [effectiveProviderSettings, setEffectiveProviderSettings] =
    useState<ProviderModelSettingsDto>(DEFAULT_PROVIDER_MODELS);
  const providerSettingsProfileRef = useRef<string | null>(null);
  const currentDataPartitionLabel = useCurrentDataPartitionName();
  const showingPartitionModels = currentDataPartitionLabel !== DEFAULT_DATA_PARTITION;
  const [partitionGenerationModel, setPartitionGenerationModel] = useState<string>();
  const [veniceModels, setVeniceModels] = useState<Record<ProviderModelMode, VeniceModelDto[]>>({
    transcription: [],
    generation: [],
    // Image options come from a curated local list, not the fetched catalog;
    // this stays empty and `imageOptions` supplies the picker. Video follows
    // the same curated-local pattern while the first fast path is fixed-shape.
    image: [],
    video: [],
  });
  const [microphones, setMicrophones] = useState<DictationMicrophoneDeviceDto[]>([]);
  const [defaultMicrophone, setDefaultMicrophone] = useState<DictationMicrophoneDeviceDto>();
  const [capturingShortcut, setCapturingShortcut] = useState<DictationShortcutKind>();
  const capturingShortcutRef = useRef<DictationShortcutKind>();
  const [shortcutError, setShortcutError] = useState<string>();
  const [shortcutErrorKind, setShortcutErrorKind] = useState<DictationShortcutKind>();
  const [status, setStatus] = useState<string>();
  // Set when the dictation helper dies (crash or the bundle swap after an
  // update) and cleared once it re-arms the hotkey, so the shortcuts pane never
  // silently shows a dead hotkey.
  const [helperUnavailable, setHelperUnavailable] = useState<{
    reason: string;
    message: string;
  }>();
  const [micOpen, setMicOpen] = useState(false);
  const t = useT();
  const interfaceLocale = useLocale();
  const [theme, setTheme] = useState<ThemePreference>(() => getStoredTheme());
  const [brand, setBrand] = useState<BrandId>(() => getStoredBrand());
  const fontScale = useFontScaleId();
  const [dateFormat, setDateFormat] = useState<DateFormatPreference>(() => getStoredDateFormat());
  const [releaseChannel, setReleaseChannelValue] = useState<ReleaseChannel>("stable");
  const experimentalFlags = useExperimentalFlags();
  const experimentalUnlockClicksRef = useRef({
    ...INITIAL_EXPERIMENTAL_UNLOCK_CLICK_STATE,
  });
  const experimentalUnlockingRef = useRef(false);
  const runtimeFlagBaselineCandidateRef = useRef<boolean | null>(null);
  const runtimeFlagStatusLoadedRef = useRef(false);
  const [runtimeBrowserUseBaseline, setRuntimeBrowserUseBaseline] = useState<boolean | null>(null);
  const [agentRuntimeRunning, setAgentRuntimeRunning] = useState(false);
  const [experimentalOperation, setExperimentalOperation] = useState<
    "flags" | "restart" | "unpack"
  >();
  const [experimentalError, setExperimentalError] = useState<string>();
  // Set only when a leave-rc switch turns up an installable stable, so the
  // bespoke in-context confirm below the toggle can name the exact version.
  const [reconcileVersion, setReconcileVersion] = useState<string>();
  const [pickerMode, setPickerMode] = useState<ProviderModelMode>();
  const [modelPickerFlyout, setModelPickerFlyout] = useState<ModelPickerFlyout>(null);
  const [modelSearch, setModelSearch] = useState("");
  const modelPickerTriggerRef = useRef<HTMLButtonElement>(null);
  const modelPickerPopoverRef = useRef<HTMLDivElement>(null);
  const modelPickerSearchRef = useRef<HTMLInputElement>(null);
  const costQualitySaveChainRef = useRef<Promise<void>>(Promise.resolve());
  const latestCostQualitySaveRef = useRef(0);
  const confirmedCostQualityRef = useRef(DEFAULT_PROVIDER_MODELS.costQuality);
  const [veniceApiKeyDraft, setVeniceApiKeyDraft] = useState("");
  // Saving a Venice key while Auto is the text model would silently keep
  // billing Clovy credits (Auto never uses the key), so the save surfaces an
  // explicit billing choice: switch to a Venice model or knowingly keep Auto.
  const [veniceKeyAutoBillingChoiceOpen, setVeniceKeyAutoBillingChoiceOpen] = useState(false);
  const [showMoreVoiceOptions, setShowMoreVoiceOptions] = useState(false);
  const [showMoreTextOptions, setShowMoreTextOptions] = useState(false);
  const [showMoreImageOptions, setShowMoreImageOptions] = useState(false);
  const [internalTab, setInternalTab] = useState<SettingsTab>("general");
  const [micPopoverPlacement, setMicPopoverPlacement] =
    useState<SelectPopoverPlacement>("align-selected");
  const [languageOpen, setLanguageOpen] = useState(false);
  const [languagePopoverPlacement, setLanguagePopoverPlacement] =
    useState<SelectPopoverPlacement>("align-selected");
  const [micTestState, setMicTestState] = useState<MicTestState>("idle");
  const [micTestLevel, setMicTestLevel] = useState(0);
  const [micTestStartedAt, setMicTestStartedAt] = useState<number>();
  const [micTestElapsedMs, setMicTestElapsedMs] = useState(0);
  const [micTestSampleSrc, setMicTestSampleSrc] = useState<string>();

  const [micTestError, setMicTestError] = useState<string>();
  const [micTestPlaying, setMicTestPlaying] = useState(false);
  const controlled = controlledTab !== undefined && onTabChange !== undefined;
  const activeTab = controlled ? controlledTab : internalTab;
  // The skill opened from Installed skills. While set (and the skills tab is
  // active) the whole settings page swaps for the notes-style detail shell:
  // pinned breadcrumb bar on top, its own scroll region beneath.
  const detailPinned = false;
  useEffect(() => {
    onDetailPinnedChange?.(detailPinned);
  }, [detailPinned, onDetailPinnedChange]);
  // Never leave the host thinking a detail scroller is active after unmount.
  useEffect(() => () => onDetailPinnedChange?.(false), [onDetailPinnedChange]);
  const settingsTabs = appSettingsTabsForCompanionPairing(
    experimentalFlags.companionPairingEnabled,
  ).filter((tab) => !(account.localDev && tab.id === "billing"));
  const capabilities = useDictationCapabilities();
  const macLikePlatform = capabilities.platform === "macos";
  const systemAudioSupportedPlatform = capabilities.systemAudio || isSystemAudioSupportedPlatform();
  const defaultShortcuts =
    capabilities.platform === "windows"
      ? WINDOWS_DEFAULT_SHORTCUTS
      : {
          push_to_talk: DEFAULT_SETTINGS.pushToTalkShortcut,
          toggle: DEFAULT_SETTINGS.toggleShortcut,
        };
  const modifierRequiredMessage =
    capabilities.platform === "windows"
      ? t("settings.shortcuts.modifierRequiredWindows")
      : t("settings.shortcuts.modifierRequiredMac");

  useEffect(() => {
    if (!experimentalFlags.loaded || runtimeFlagStatusLoadedRef.current) return;
    runtimeFlagBaselineCandidateRef.current ??= experimentalFlags.browserUseEnabled;
    let cancelled = false;
    const baseline = runtimeFlagBaselineCandidateRef.current;
    Promise.resolve()
      .then(() => {
        if (cancelled) return;
        runtimeFlagStatusLoadedRef.current = true;
        setAgentRuntimeRunning(true);
        setRuntimeBrowserUseBaseline(baseline);
      })
      .catch(() => {
        if (cancelled) return;
        runtimeFlagStatusLoadedRef.current = true;
        setRuntimeBrowserUseBaseline(baseline);
      });
    return () => {
      cancelled = true;
    };
  }, [experimentalFlags.loaded, experimentalFlags.browserUseEnabled]);
  const setActiveTab = (tab: SettingsTab) => {
    if (controlled) {
      onTabChange?.(tab);
    } else {
      setInternalTab(tab);
    }
  };
  const micWrapRef = useRef<HTMLDivElement>(null);
  const languageWrapRef = useRef<HTMLDivElement>(null);
  const systemOn = sourceMode === "microphonePlusSystem";
  const systemReadiness = sourceReadiness?.sources.find((source) => source.source === "system");
  const microphoneReadiness = sourceReadiness?.sources.find(
    (source) => source.source === "microphone",
  );
  const systemAvailability = systemAudioAvailability(sourceReadiness);
  // Denied and granted-but-uncapturable both lock the switch, but only a real
  // denial is fixable in System Settings: the uncapturable helper recovers on
  // restart, so sending the user to grant an already-granted permission would
  // be a dead end. The status label tells the two apart.
  const systemDenied = systemAvailability === "denied";
  const systemLocked = systemDenied || systemAvailability === "unavailable";
  const systemUnavailable = !systemAudioSupportedPlatform || systemAvailability === "unsupported";

  useEffect(() => {
    capturingShortcutRef.current = capturingShortcut;
  }, [capturingShortcut]);

  // Load the persisted release channel once the updater is available. Gated on
  // a stable boolean (not the onCheckForUpdates prop itself, which is an inline
  // arrow with a new identity each render) so this loads once, not per render.
  const updaterAvailable = Boolean(onCheckForUpdates);
  useEffect(() => {
    if (!updaterAvailable) return;
    let active = true;
    void getReleaseChannel()
      .then((channel) => {
        if (active) setReleaseChannelValue(channel);
      })
      .catch(() => undefined);
    return () => {
      active = false;
    };
  }, [updaterAvailable]);

  const handleReleaseChannelChange = (next: ReleaseChannel) => {
    setReleaseChannelValue(next);
    // Any channel change dismisses a stale reconcile offer (e.g. toggling back
    // to rc, or stable -> rc -> stable) before we decide whether to re-offer.
    setReconcileVersion(undefined);
    void setReleaseChannel(next)
      .then(() => {
        // Leaving rc for stable while running a prerelease build: stable is
        // normally older than the rc you are on, so a routine check would never
        // pull it. Offer a one-time reconcile down onto the current stable (Q4-Q8).
        if (next === "stable" && isPrereleaseBuild()) {
          void offerReconcileToStable();
        }
      })
      .catch(() => {
        // Persist failed: re-read so the toggle reflects the real saved channel
        // rather than an optimistic value that never reached disk.
        void getReleaseChannel()
          .then(setReleaseChannelValue)
          .catch(() => undefined);
      });
  };

  async function offerReconcileToStable() {
    try {
      const update = await reconcileToStable();
      // Only prompt when a stable is actually installable. If stable has already
      // caught up or passed the rc, the routine updater handles it (no reconcile).
      if (update) setReconcileVersion(update.version);
    } catch {
      // A failed reconcile check is silent: the channel is already saved and the
      // routine update flow will retry on its next check.
    }
  }

  function confirmReconcileToStable() {
    setReconcileVersion(undefined);
    onReconcileToStable?.();
  }

  useEffect(() => {
    setMicOpen(false);
    setLanguageOpen(false);
    if (activeTab !== "audio" && micTestState !== "idle") {
      void resetMicTestState(true);
    }
  }, [activeTab]);

  useEffect(() => {
    if (!account.localDev || activeTab !== "billing") {
      return;
    }
    if (controlled) {
      onTabChange?.("general");
      return;
    }
    setInternalTab("general");
  }, [account.localDev, activeTab, controlled, onTabChange]);

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;

    async function boot() {
      try {
        const response = await dictationSettings();
        if (cancelled) return;
        setSettings(response.settings);
        const hotkeyStatus = await dictationHotkeyStatus();
        if (cancelled) return;
        handleHelperEvent(hotkeyStatus);
        const modelResponse = await providerModelSettings();
        if (cancelled) return;
        // Merge over defaults so a settings payload that predates a field
        // (e.g. imageModel from an older backend) still has every model set.
        const modelSnapshot = providerModelSettingsSnapshot(modelResponse);
        confirmedCostQualityRef.current = modelSnapshot.settings.costQuality;
        setProviderSettings(modelSnapshot.settings);
        setEffectiveProviderSettings(modelSnapshot.effectiveSettings);
        providerSettingsProfileRef.current = currentDataPartitionLabel;
        await requestMicrophones();
        await Promise.all([
          requestVeniceModels("transcription"),
          requestVeniceModels("generation"),
        ]);
      } catch (error) {
        if (!cancelled) setStatus(messageFromError(error));
      }
    }

    void listen<string>("dictation-event", (event) => {
      const helperEvent = parseDictationHelperEvent(event.payload);
      if (helperEvent) handleHelperEvent(helperEvent);
    }).then((cleanup) => {
      // Unmount can race the listen() promise — unsubscribe immediately
      // instead of leaking the listener.
      if (cancelled) cleanup();
      else unlisten = cleanup;
    });
    void boot();

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    if (
      providerSettingsProfileRef.current === null ||
      providerSettingsProfileRef.current === currentDataPartitionLabel
    ) {
      return;
    }
    let cancelled = false;

    async function refreshProviderModelsForProfile() {
      try {
        const modelResponse = await providerModelSettings();
        if (cancelled) return;
        const modelSnapshot = providerModelSettingsSnapshot(modelResponse);
        setProviderSettings(modelSnapshot.settings);
        setEffectiveProviderSettings(modelSnapshot.effectiveSettings);
        providerSettingsProfileRef.current = currentDataPartitionLabel;
      } catch (error) {
        if (!cancelled) setStatus(messageFromError(error));
      }
    }

    void refreshProviderModelsForProfile();
    return () => {
      cancelled = true;
    };
  }, [currentDataPartitionLabel]);

  useEffect(() => {
    if (!showingPartitionModels) {
      setPartitionGenerationModel(undefined);
      return;
    }
    let cancelled = false;
    setPartitionGenerationModel(undefined);

    async function loadActiveProfileTextModel() {
      try {
        if (!cancelled) setPartitionGenerationModel(effectiveProviderSettings.generationModel);
      } catch {
        if (!cancelled) setPartitionGenerationModel(undefined);
      }
    }

    void loadActiveProfileTextModel();
    return () => {
      cancelled = true;
    };
  }, [effectiveProviderSettings.generationModel, showingPartitionModels]);

  useEffect(() => {
    if (!micOpen) return;
    function onPointer(event: MouseEvent) {
      if (!micWrapRef.current?.contains(event.target as Node)) {
        setMicOpen(false);
      }
    }
    function onKey(event: KeyboardEvent) {
      if (event.key === "Escape") setMicOpen(false);
    }
    window.addEventListener("mousedown", onPointer);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("mousedown", onPointer);
      window.removeEventListener("keydown", onKey);
    };
  }, [micOpen]);

  useEffect(() => {
    if (!languageOpen) return;
    function onPointer(event: MouseEvent) {
      if (!languageWrapRef.current?.contains(event.target as Node)) {
        setLanguageOpen(false);
      }
    }
    function onKey(event: KeyboardEvent) {
      if (event.key === "Escape") setLanguageOpen(false);
    }
    window.addEventListener("mousedown", onPointer);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("mousedown", onPointer);
      window.removeEventListener("keydown", onKey);
    };
  }, [languageOpen]);

  // The capture effect below must call the latest saveShortcut, not the one
  // from the render in which capturing began (it is a plain function,
  // redefined every render). Same ref pattern as use-shortcut-capture.
  const saveShortcutRef = useRef(saveShortcut);
  useEffect(() => {
    saveShortcutRef.current = saveShortcut;
  });

  useEffect(() => {
    if (!capturingShortcut) return;
    const kind = capturingShortcut;
    function onKey(event: KeyboardEvent) {
      if (event.key === "Escape") {
        event.preventDefault();
        void cancelShortcutCapture();
        return;
      }
      // Key chords are read here in the DOM (the window is focused during a
      // rebind); the helper's flagsChanged monitor only contributes fn and
      // bare-modifier chords. This split is what lets the helper run without
      // the Input Monitoring permission.
      const result = chordFromKeyEvent(event);
      if (result.kind === "ignore") return;
      event.preventDefault();
      event.stopPropagation();
      if (result.kind === "needsModifier") {
        setShortcutError(modifierRequiredMessage);
        setShortcutErrorKind(kind);
        setStatus(modifierRequiredMessage);
        return;
      }
      setShortcutError(undefined);
      void dictationHelperCommand({ type: "cancel_shortcut_capture" }).catch(() => undefined);
      void saveShortcutRef.current(kind, result.shortcut);
    }
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [capturingShortcut]);

  async function requestMicrophones() {
    try {
      await dictationHelperCommand({ type: "list_microphones" });
    } catch (error) {
      setStatus(messageFromError(error));
    }
  }

  async function requestVeniceModels(mode: ProviderModelMode) {
    try {
      const response = await listVeniceModels(mode);
      setVeniceModels((models) => ({
        ...models,
        [mode]: response.models,
      }));
    } catch (error) {
      setStatus(messageFromError(error));
    }
  }

  function handleHelperEvent(helperEvent: DictationHelperEvent) {
    if (helperEvent.type === "microphone_devices") {
      setMicrophones(helperEvent.payload?.devices ?? []);
      setDefaultMicrophone(helperEvent.payload?.defaultDevice);
      return;
    }
    if (helperEvent.type === "mic_test_started") {
      setMicTestState("recording");
      setMicTestError(undefined);
      setMicTestSampleSrc(undefined);
      setMicTestLevel(0);
      setMicTestStartedAt(Date.now());
      setMicTestElapsedMs(0);
      setMicTestPlaying(false);
      return;
    }
    if (helperEvent.type === "mic_test_level") {
      setMicTestLevel(numericPayload(helperEvent.payload?.level));
      return;
    }
    if (helperEvent.type === "mic_test_ready") {
      const path = stringPayload(helperEvent.payload?.path);
      if (!path) {
        setMicTestState("error");
        setMicTestError(translate("settings.micTest.noSample"));
        return;
      }
      setMicTestState("ready");
      setMicTestStartedAt(undefined);
      setMicTestElapsedMs(0);
      setMicTestLevel(numericPayload(helperEvent.payload?.observedAudioLevel));
      setMicTestError(undefined);
      setMicTestPlaying(false);
      setMicTestSampleSrc(localAudioFileSrc(path));
      return;
    }
    if (helperEvent.type === "mic_test_error") {
      const message = helperEvent.payload?.message ?? translate("settings.micTest.couldNotRecord");
      setMicTestState("error");
      setMicTestStartedAt(undefined);
      setMicTestError(message);
      setMicTestPlaying(false);
      setStatus(message);
      return;
    }
    if (helperEvent.type === "fn_monitor_unavailable") {
      setStatus(helperEvent.payload?.message ?? translate("settings.shortcuts.monitorUnavailable"));
      return;
    }
    if (helperEvent.type === "helper_unavailable") {
      setHelperUnavailable({
        reason: stringPayload(helperEvent.payload?.reason) ?? "restarting",
        message: helperEvent.payload?.message ?? translate("settings.helper.restarting"),
      });
      return;
    }
    if (helperEvent.type === "hotkey_trigger_ready") {
      // The helper re-armed the hotkey, so it is back: clear any down notice.
      setHelperUnavailable(undefined);
      return;
    }
    if (helperEvent.type === "hotkey_trigger_unavailable") {
      const message = helperEvent.payload?.message ?? translate("settings.shortcuts.unavailable");
      const kind = shortcutKindPayload(helperEvent.payload?.kind);
      setHelperUnavailable(undefined);
      setShortcutError(message);
      setShortcutErrorKind(kind);
      setStatus(message);
      return;
    }
    if (helperEvent.type === "shortcut_capture_started") {
      setStatus(translate("settings.shortcuts.pressToRecord"));
      return;
    }
    if (helperEvent.type === "shortcut_capture_error") {
      const message = helperEvent.payload?.message ?? translate("settings.shortcuts.captureFailed");
      const kind = shortcutKindPayload(helperEvent.payload?.kind) ?? capturingShortcutRef.current;
      setCapturingShortcut(undefined);
      setShortcutError(message);
      setShortcutErrorKind(kind);
      setStatus(message);
      return;
    }
    if (helperEvent.type === "shortcut_capture_cancelled") {
      setCapturingShortcut(undefined);
      setShortcutError(undefined);
      setShortcutErrorKind(undefined);
      setStatus(translate("settings.shortcuts.captureEnded"));
      return;
    }
    if (helperEvent.type === "shortcut_captured") {
      const kind = capturingShortcutRef.current;
      if (!kind) {
        setShortcutError(translate("settings.shortcuts.noTarget"));
        setStatus(translate("settings.shortcuts.noTarget"));
        return;
      }
      const shortcut = shortcutFromCapturePayload(helperEvent.payload?.shortcut, 1);
      if (!shortcut) {
        setShortcutError(translate("settings.shortcuts.invalidData"));
        setStatus(translate("settings.shortcuts.invalidData"));
        return;
      }
      setShortcutError(undefined);
      setShortcutErrorKind(undefined);
      void saveShortcut(kind, shortcut);
      return;
    }
    if (helperEvent.type === "error") {
      setStatus(helperEvent.payload?.message ?? translate("settings.helper.failed"));
    }
  }

  async function selectMicrophone(id?: string, name?: string) {
    try {
      if (micTestState !== "idle") {
        await resetMicTestState(true);
      }
      const next = await setDictationMicrophone(id, name);
      setSettings(next);
      setMicOpen(false);
      setStatus(
        name
          ? translate("settings.audio.microphoneSet", { name })
          : translate("settings.audio.microphoneAuto"),
      );
    } catch (error) {
      setStatus(messageFromError(error));
    }
  }

  async function saveShortcut(
    kind: DictationShortcutKind,
    shortcut: Pick<DictationShortcutSetting, "code" | "modifiers" | "label" | "pressCount">,
  ) {
    try {
      const next = await setDictationShortcut(kind, shortcut);
      setSettings(next);
      setCapturingShortcut(undefined);
      setShortcutError(undefined);
      setShortcutErrorKind(undefined);
      setStatus(
        translate("settings.shortcuts.set", {
          shortcut: shortcutKindLabel(kind),
          label: shortcutForKind(next, kind).label,
        }),
      );
    } catch (error) {
      setShortcutError(messageFromError(error));
      setShortcutErrorKind(kind);
      setStatus(messageFromError(error));
    }
  }

  async function startShortcutCapture(kind: DictationShortcutKind) {
    setShortcutError(undefined);
    setShortcutErrorKind(undefined);
    setCapturingShortcut(kind);
    try {
      await dictationHelperCommand({
        type: "start_shortcut_capture",
        kind,
        pressCount: 1,
      });
    } catch (error) {
      setCapturingShortcut(undefined);
      setShortcutError(messageFromError(error));
      setShortcutErrorKind(kind);
      setStatus(messageFromError(error));
    }
  }

  async function cancelShortcutCapture() {
    setCapturingShortcut(undefined);
    setShortcutError(undefined);
    setShortcutErrorKind(undefined);
    try {
      await dictationHelperCommand({ type: "cancel_shortcut_capture" });
    } catch (error) {
      setStatus(messageFromError(error));
    }
  }

  async function startMicTest() {
    setMicTestState("recording");
    setMicTestError(undefined);
    setMicTestSampleSrc(undefined);
    setMicTestLevel(0);
    setMicTestStartedAt(Date.now());
    setMicTestElapsedMs(0);
    setMicTestPlaying(false);
    try {
      await dictationHelperCommand({
        type: "start_mic_test",
        durationSeconds: MIC_TEST_DURATION_SECONDS,
      });
    } catch (error) {
      const message = messageFromError(error);
      setMicTestState("error");
      setMicTestStartedAt(undefined);
      setMicTestError(message);
      setStatus(message);
    }
  }

  async function startOverMicTest() {
    await resetMicTestState(true);
    await startMicTest();
  }

  async function resetMicTestState(discardHelper = false) {
    setMicTestState("idle");
    setMicTestLevel(0);
    setMicTestStartedAt(undefined);
    setMicTestElapsedMs(0);
    setMicTestSampleSrc(undefined);
    setMicTestError(undefined);
    setMicTestPlaying(false);
    if (!discardHelper) return;
    try {
      await dictationHelperCommand({ type: "discard_mic_test" });
    } catch {
      // Resetting the settings UI should not surface stale helper cleanup errors.
    }
  }

  // Returns whether the switch persisted, so confirmation flows (the Venice
  // key billing choice) can stay open instead of closing over a failed save.
  async function selectVeniceModel(mode: ProviderModelMode, modelId: string): Promise<boolean> {
    if (showingPartitionModels) return false;
    try {
      const next = await setVeniceModel(mode, modelId);
      setProviderSettings(next);
      setEffectiveProviderSettings(next);
      dispatchProviderModelSettingsChanged({ mode, modelId });
      setStatus(translate(`settings.models.updated.${mode}`));
      return true;
    } catch (error) {
      setStatus(messageFromError(error));
      return false;
    }
  }

  function saveCostQuality(value: number) {
    const version = ++latestCostQualitySaveRef.current;
    const save = costQualitySaveChainRef.current.then(() => setCostQuality(value));
    costQualitySaveChainRef.current = save.then(
      () => undefined,
      () => undefined,
    );
    void save.then(
      (next) => {
        confirmedCostQualityRef.current = next.costQuality;
        if (version !== latestCostQualitySaveRef.current) return;
        setProviderSettings((current) => ({
          ...current,
          costQuality: next.costQuality,
        }));
        setStatus(translate("settings.autoPreference.updated"));
      },
      (error) => {
        if (version !== latestCostQualitySaveRef.current) return;
        setProviderSettings((current) => ({
          ...current,
          costQuality: confirmedCostQualityRef.current,
        }));
        setStatus(messageFromError(error));
      },
    );
  }

  function closeModelPicker() {
    setPickerMode(undefined);
    setModelPickerFlyout(null);
    setModelSearch("");
  }

  // Optimistic apply + persisted save for the Models row's segmented Auto
  // preference control.
  function applyCostQuality(costQuality: number) {
    setProviderSettings((current) => ({ ...current, costQuality }));
    saveCostQuality(costQuality);
  }

  function selectModelFromPicker(
    mode: ProviderModelMode,
    modelId: string,
    costQuality?: number,
    options?: { keepOpen?: boolean },
  ) {
    // Named profiles show their own models read-only; a pick is a no-op.
    if (showingPartitionModels) {
      closeModelPicker();
      return;
    }
    const picked = modelOptionsForMode(mode).find((model) => model.id === modelId);
    if (mode === "generation" && costQuality !== undefined) {
      applyCostQuality(costQuality);
    }
    // The local option is the endpoint already serving chat; the provider is
    // switched in the Providers section, so picking it changes nothing.
    if (!(mode === "generation" && picked?.provider === "local")) {
      void selectVeniceModel(mode, modelId);
    }
    // The Auto toggle switches models mid-flow, so it asks to keep the picker
    // open; a row pick is a final choice and closes it.
    if (!options?.keepOpen) closeModelPicker();
  }

  async function saveVeniceApiKey() {
    const apiKey = veniceApiKeyDraft.trim();
    if (!apiKey) {
      setStatus(translate("settings.venice.enterKey"));
      return;
    }
    try {
      const next = await setVeniceApiKey(apiKey);
      setProviderSettings(next);
      setVeniceApiKeyDraft("");
      setStatus(translate("settings.venice.saved"));
      // The workspace's model picker shows a billing note while a key is
      // saved, so let it refresh its provider settings snapshot.
      dispatchProviderModelSettingsChanged({ mode: "generation", modelId: next.generationModel });
      if (next.generationModel === AUTO_MODEL_ID) {
        setVeniceKeyAutoBillingChoiceOpen(true);
      }
    } catch (error) {
      setStatus(messageFromError(error));
    }
  }

  async function removeVeniceApiKey() {
    try {
      const next = await clearVeniceApiKey();
      setProviderSettings(next);
      setVeniceApiKeyDraft("");
      setStatus(translate("settings.venice.removed"));
      dispatchProviderModelSettingsChanged({ mode: "generation", modelId: next.generationModel });
    } catch (error) {
      setStatus(messageFromError(error));
    }
  }

  async function toggleLiveTranscription(enabled: boolean) {
    try {
      const next = await setLiveTranscription(enabled);
      setProviderSettings(next);
      setStatus(
        enabled
          ? translate("settings.liveTranscription.on")
          : translate("settings.liveTranscription.off"),
      );
    } catch (error) {
      setStatus(messageFromError(error));
    }
  }

  async function toggleImageSafeMode(enabled: boolean) {
    try {
      const next = await setImageSafeMode(enabled);
      setProviderSettings(next);
      setStatus(enabled ? translate("settings.safeMode.on") : translate("settings.safeMode.off"));
    } catch (error) {
      setStatus(messageFromError(error));
    }
  }

  async function selectLanguage(language: string) {
    try {
      const next = await setDictationLanguage(language || undefined);
      setSettings(next);
      setLanguageOpen(false);
      setStatus(
        language
          ? translate("settings.dictation.languageSet", { language: languageLabel(language) })
          : translate("settings.dictation.languageAuto"),
      );
    } catch (error) {
      setStatus(messageFromError(error));
    }
  }

  const microphoneName = settings.microphone.name ?? t("settings.audio.autoDetect");
  const microphoneDescription = settings.microphone.id
    ? t("settings.audio.microphoneDescription")
    : defaultMicrophone?.name
      ? t("settings.audio.autoDetectUses", { name: defaultMicrophone.name })
      : t("settings.audio.autoDetectSystem");
  const microphoneOptions = [
    { id: undefined, name: t("settings.audio.autoDetect") },
    ...microphones,
  ];
  const selectedMicrophoneIndex = Math.max(
    0,
    microphoneOptions.findIndex((option) => (option.id ?? "") === (settings.microphone.id ?? "")),
  );
  const selectedLanguageIndex = Math.max(
    0,
    LANGUAGE_OPTIONS.findIndex((option) => option.value === (settings.language ?? "")),
  );
  const displayProviderSettings = showingPartitionModels
    ? effectiveProviderSettings
    : providerSettings;
  const transcriptionOptions = modelOptions(
    veniceModels.transcription,
    displayProviderSettings.transcriptionModel,
  );
  const localModelEnabled = providerSettings.generationProvider === "local";
  const generationCatalog = useMemo(
    () => withLocalGenerationOption(veniceModels.generation, providerSettings.localGeneration),
    [
      veniceModels.generation,
      providerSettings.localGeneration.baseUrl,
      providerSettings.localGeneration.modelId,
    ],
  );
  // Pass the prefixed local id (when local is enabled) so it matches the
  // catalog's local option. Passing the raw generationModel let modelOptions
  // prepend a bare duplicate entry that persisted the local id as the remote
  // model when clicked.
  const generationOptions = modelOptions(generationCatalog, modelValueForMode("generation"));
  // Where the billing-choice dialog lands when the user opts out of Auto to
  // use their Venice key: the leading suggested pick, else the first Venice
  // catalog model, drawn from the same selectable list as the model picker so
  // the dialog can never persist a model the picker would exclude (the factory
  // default stays the last resort for an empty catalog).
  const selectableGenerationOptions = generationOptions.filter((option) =>
    modelAvailableForMode("generation", option),
  );
  const veniceKeySwitchTarget =
    suggestedModelsForMode("generation", selectableGenerationOptions).find(
      (item) => item.model.id !== AUTO_MODEL_ID,
    )?.model ??
    selectableGenerationOptions.find(
      (option) => option.provider === "venice" && option.id !== AUTO_MODEL_ID,
    );
  const imageOptions = IMAGE_GENERATION_ENABLED
    ? modelOptions(imageModelCatalog(), displayProviderSettings.imageModel)
    : [];
  const videoOptions = VIDEO_GENERATION_ENABLED
    ? modelOptions(VIDEO_MODELS, displayProviderSettings.videoModel)
    : [];

  useEffect(() => {
    if (showingPartitionModels) closeModelPicker();
  }, [showingPartitionModels]);

  useEffect(() => {
    if (!pickerMode) return;
    function onPointer(event: MouseEvent) {
      const target = event.target as Node;
      if (modelPickerPopoverRef.current?.contains(target)) return;
      if (modelPickerTriggerRef.current?.contains(target)) return;
      // The hover detail cards are portaled to document.body, so a click inside
      // one (its "Show more" toggle) lands outside the popover — treat it as in.
      if (target instanceof Element && target.closest(".agent-composer-model-hovercard")) return;
      closeModelPicker();
    }
    function onKey(event: KeyboardEvent) {
      if (event.key !== "Escape") return;
      event.preventDefault();
      if (modelPickerFlyout?.kind === "all") {
        setModelPickerFlyout(null);
        setModelSearch("");
      } else {
        closeModelPicker();
      }
    }
    window.addEventListener("mousedown", onPointer);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("mousedown", onPointer);
      window.removeEventListener("keydown", onKey);
    };
  }, [pickerMode, modelPickerFlyout]);

  useEffect(() => {
    if (pickerMode === "image" || modelPickerFlyout?.kind === "all") {
      modelPickerSearchRef.current?.focus();
    }
  }, [pickerMode, modelPickerFlyout]);

  useEffect(() => {
    if (micOpen) updateMicrophonePopoverPlacement();
  }, [micOpen, microphoneOptions.length, selectedMicrophoneIndex]);

  useEffect(() => {
    if (languageOpen) updateLanguagePopoverPlacement();
  }, [languageOpen, selectedLanguageIndex]);

  useEffect(() => {
    if (micTestState !== "recording" || !micTestStartedAt) return;
    const interval = window.setInterval(() => {
      setMicTestElapsedMs(Date.now() - micTestStartedAt);
    }, 100);
    return () => window.clearInterval(interval);
  }, [micTestState, micTestStartedAt]);

  function updateMicrophonePopoverPlacement() {
    setMicPopoverPlacement(
      selectPopoverPlacement(micWrapRef.current, microphoneOptions.length, selectedMicrophoneIndex),
    );
  }

  function updateLanguagePopoverPlacement() {
    setLanguagePopoverPlacement(
      selectPopoverPlacement(
        languageWrapRef.current,
        LANGUAGE_OPTIONS.length,
        selectedLanguageIndex,
      ),
    );
  }

  function modelOptionsForMode(mode: ProviderModelMode) {
    if (mode === "transcription") return transcriptionOptions;
    if (mode === "image") return IMAGE_GENERATION_ENABLED ? imageOptions : [];
    if (mode === "video") return VIDEO_GENERATION_ENABLED ? videoOptions : [];
    return generationOptions;
  }

  function modelValueForMode(mode: ProviderModelMode) {
    if (mode === "transcription") return displayProviderSettings.transcriptionModel;
    if (mode === "image") return displayProviderSettings.imageModel;
    if (mode === "video") return displayProviderSettings.videoModel;
    if (showingPartitionModels) {
      return partitionGenerationModel ?? globalGenerationModelValue();
    }
    return globalGenerationModelValue();
  }

  function globalGenerationModelValue() {
    if (localModelEnabled && providerSettings.localGeneration.modelId.trim()) {
      return localGenerationOptionId(providerSettings.localGeneration.modelId);
    }
    return providerSettings.generationModel;
  }

  function openModelPicker(mode: ProviderModelMode) {
    if (showingPartitionModels) return;
    if (mode === "image" && !IMAGE_GENERATION_ENABLED) return;
    if (mode === "video" && !VIDEO_GENERATION_ENABLED) return;
    setPickerMode(mode);
    setModelPickerFlyout(null);
    setModelSearch("");
    // Image and video models are curated local lists, not fetched catalogs.
    if (mode !== "image" && mode !== "video") void requestVeniceModels(mode);
  }

  function microphonePopoverStyle(): CSSProperties {
    return selectPopoverStyle(micPopoverPlacement, selectedMicrophoneIndex);
  }

  function languagePopoverStyle(): CSSProperties {
    return selectPopoverStyle(languagePopoverPlacement, selectedLanguageIndex);
  }

  function handleReleaseVersionClick() {
    if (
      !experimentalFlags.loaded ||
      experimentalFlags.unlocked ||
      experimentalUnlockingRef.current
    ) {
      return;
    }
    const next = registerExperimentalUnlockClick(experimentalUnlockClicksRef.current, Date.now());
    experimentalUnlockClicksRef.current = next.state;
    if (!next.unlocked) return;

    experimentalUnlockingRef.current = true;
    setExperimentalError(undefined);
    void setExperimentalFlags({
      unlocked: true,
      browser_use: experimentalFlags.browser_use,
      companion_pairing: experimentalFlags.companion_pairing,
      companion_computer_use_approvals: experimentalFlags.companion_computer_use_approvals,
    })
      .then(() => toast(translate("settings.experiments.unlocked")))
      .catch((error) => setExperimentalError(messageFromError(error)))
      .finally(() => {
        experimentalUnlockingRef.current = false;
      });
  }

  async function updateExperimentalFlags(update: {
    unlocked?: boolean;
    browser_use?: boolean;
    companion_pairing?: boolean;
    companion_computer_use_approvals?: boolean;
  }) {
    setExperimentalOperation("flags");
    setExperimentalError(undefined);
    try {
      await setExperimentalFlags({
        unlocked: update.unlocked ?? experimentalFlags.unlocked,
        browser_use: update.browser_use ?? experimentalFlags.browser_use,
        companion_pairing: update.companion_pairing ?? experimentalFlags.companion_pairing,
        companion_computer_use_approvals:
          update.companion_computer_use_approvals ??
          experimentalFlags.companion_computer_use_approvals,
      });
    } catch (error) {
      setExperimentalError(messageFromError(error));
    } finally {
      setExperimentalOperation(undefined);
    }
  }

  async function restartAgentForExperimentalFlags() {
    setExperimentalOperation("restart");
    setExperimentalError(undefined);
    try {
      await connectorsApplyRuntime();
      setRuntimeBrowserUseBaseline(experimentalFlags.browserUseEnabled);
      setAgentRuntimeRunning(true);
    } catch (error) {
      setExperimentalError(messageFromError(error));
    } finally {
      setExperimentalOperation(undefined);
    }
  }

  async function unpackExperimentalExtension() {
    setExperimentalOperation("unpack");
    setExperimentalError(undefined);
    try {
      await unpackBundledExtension();
    } catch (error) {
      setExperimentalError(messageFromError(error));
    } finally {
      setExperimentalOperation(undefined);
    }
  }

  const experimentalRestartNeeded =
    agentRuntimeRunning &&
    runtimeBrowserUseBaseline !== null &&
    runtimeBrowserUseBaseline !== experimentalFlags.browserUseEnabled;

  return (
    <div className="settings-page" data-controlled={controlled || undefined}>
      {controlled ? null : (
        <>
          <header className="settings-header">
            <h1 className="settings-title">{t("common.settings")}</h1>
            <p className="settings-description">{t("settings.page.description")}</p>
          </header>

          <nav className="settings-nav" role="tablist" aria-label={t("settings.page.navAria")}>
            {settingsTabs.map((tab) => (
              <button
                key={tab.id}
                type="button"
                role="tab"
                aria-selected={activeTab === tab.id}
                aria-controls={`settings-panel-${tab.id}`}
                id={`settings-tab-${tab.id}`}
                onClick={() => setActiveTab(tab.id)}
              >
                {tab.label}
              </button>
            ))}
          </nav>
        </>
      )}

      <div
        className="settings-tab-panel"
        role="tabpanel"
        id={`settings-panel-${activeTab}`}
        aria-labelledby={`settings-tab-${activeTab}`}
      >
        {activeTab === "general" ? (
          <>
            <SettingsPageHeader
              title={t("settings.tabs.general")}
              blurb={t("settings.general.blurb")}
            />
            <AccountSettingsSection
              account={account}
              loading={accountLoading}
              onAccountChanged={onAccountChanged}
              onRefresh={onAccountRefresh}
            />

            <PermissionsSettingsSection
              microphonePermissionStatus={microphonePermissionStatus}
              microphoneReadiness={microphoneReadiness}
              accessibilityPermissionStatus={accessibilityPermissionStatus}
              systemReadiness={systemReadiness}
              onEnableMicrophone={onEnableMicrophone}
              onEnableAccessibility={onEnableAccessibility}
              onEnableSystemAudio={onEnableSystemAudio}
            />

            <StartupSettingsSection />

            <PrivacySettingsSection />
          </>
        ) : null}

        {activeTab === "appearance" ? (
          <section className="settings-group" aria-labelledby="appearance-heading">
            <SettingsPageHeader
              id="appearance-heading"
              title={t("settings.appearance.title")}
              blurb={t("settings.appearance.blurb")}
            />
            <div className="settings-card">
              <div className="settings-rows">
                <div className="settings-row">
                  <div className="settings-row-info">
                    <h3 className="settings-row-title">{t("settings.interfaceLanguage.title")}</h3>
                    <p className="settings-row-description">
                      {t("settings.interfaceLanguage.description")}
                    </p>
                  </div>
                  <div className="settings-row-control">
                    <Select
                      value={interfaceLocale}
                      options={INTERFACE_LOCALE_OPTIONS.map((option) => ({
                        value: option.value,
                        label: option.label,
                        lang: option.value,
                      }))}
                      placeholder="English"
                      ariaLabel={t("settings.interfaceLanguage.aria", {
                        language:
                          INTERFACE_LOCALE_OPTIONS.find(
                            (option) => option.value === interfaceLocale,
                          )?.label ?? "English",
                      })}
                      onChange={(value) => setInterfaceLocale(value as InterfaceLocale)}
                    />
                  </div>
                </div>
                <div className="settings-row">
                  <div className="settings-row-info">
                    <h3 className="settings-row-title">{t("settings.theme.title")}</h3>
                    <p className="settings-row-description">{t("settings.theme.description")}</p>
                  </div>
                  <div className="settings-row-control">
                    <SegmentedControl<ThemePreference>
                      aria-label={t("settings.theme.aria")}
                      value={theme}
                      options={themeOptions(t)}
                      onValueChange={(next) => {
                        setTheme(next);
                        setStoredTheme(next);
                      }}
                    />
                  </div>
                </div>
                <div className="settings-row">
                  <div className="settings-row-info">
                    <h3 className="settings-row-title">{t("settings.textSize.title")}</h3>
                    <p className="settings-row-description">{t("settings.textSize.description")}</p>
                  </div>
                  <div className="settings-row-control">
                    <SegmentedControl<FontScaleId>
                      aria-label={t("settings.textSize.title")}
                      value={fontScale}
                      options={fontScaleOptions(t)}
                      onValueChange={setStoredFontScale}
                    />
                  </div>
                </div>
                <div className="settings-row">
                  <div className="settings-row-info">
                    <h3 className="settings-row-title">{t("settings.accent.title")}</h3>
                    <p className="settings-row-description">{t("settings.accent.description")}</p>
                  </div>
                  <div className="settings-row-control">
                    <Select
                      className="accent-select"
                      popoverWidth="trigger"
                      value={brand}
                      options={BRAND_PRESETS.map((preset) => ({
                        value: preset.id,
                        label: preset.label,
                        color: preset.value,
                      }))}
                      placeholder="Clay"
                      ariaLabel={t("settings.accent.aria", {
                        color:
                          BRAND_PRESETS.find((preset) => preset.id === brand)?.label ??
                          BRAND_PRESETS[0].label,
                      })}
                      onChange={(id) => {
                        setBrand(id as BrandId);
                        setStoredBrand(id as BrandId);
                      }}
                    />
                  </div>
                </div>
                <div className="settings-row">
                  <div className="settings-row-info">
                    <h3 className="settings-row-title">{t("settings.dateFormat.title")}</h3>
                    <p className="settings-row-description">
                      {t("settings.dateFormat.description")}
                    </p>
                  </div>
                  <div className="settings-row-control">
                    <Select
                      value={dateFormat}
                      options={dateFormatOptions(t)}
                      placeholder={t("settings.dateFormat.system")}
                      ariaLabel={t("settings.dateFormat.aria", {
                        format:
                          dateFormatOptions(t).find((option) => option.value === dateFormat)
                            ?.label ?? t("settings.dateFormat.system"),
                      })}
                      onChange={(value) => {
                        const next = value as DateFormatPreference;
                        setDateFormat(next);
                        setStoredDateFormat(next);
                      }}
                    />
                  </div>
                </div>
              </div>
            </div>
          </section>
        ) : null}

        {activeTab === "billing" && !account.localDev ? (
          <BillingSettingsSection account={account} onRefresh={onAccountRefresh} />
        ) : null}

        {activeTab === "shortcuts" ? (
          <section className="settings-group" aria-labelledby="shortcuts-heading">
            <SettingsPageHeader
              id="shortcuts-heading"
              title={t("settings.tabs.shortcuts")}
              blurb={t("settings.shortcuts.blurb")}
            />
            {helperUnavailable ? (
              <InlineNotice
                role="alert"
                aria-label={t("settings.helper.unavailableAria")}
                eyebrow={
                  updateReadyToRelaunch
                    ? t("settings.helper.relaunchEyebrow")
                    : t("settings.helper.pausedEyebrow")
                }
                body={
                  updateReadyToRelaunch
                    ? t("settings.helper.pausedUntilRelaunch")
                    : helperUnavailable.message
                }
                actions={
                  updateReadyToRelaunch && onRelaunch ? (
                    <button type="button" className="btn btn-secondary" onClick={onRelaunch}>
                      {t("settings.helper.relaunch")}
                    </button>
                  ) : undefined
                }
              />
            ) : null}
            <div className="settings-card">
              <div className="settings-rows">
                {capabilities.shortcuts ? (
                  <>
                    <ShortcutRow
                      title={t("settings.shortcuts.pushToTalk")}
                      description={t("settings.shortcuts.pushToTalkDescription")}
                      shortcut={settings.pushToTalkShortcut}
                      defaultShortcut={defaultShortcuts.push_to_talk}
                      capturing={capturingShortcut === "push_to_talk"}
                      disabled={!!capturingShortcut && capturingShortcut !== "push_to_talk"}
                      error={shortcutErrorKind === "push_to_talk" ? shortcutError : undefined}
                      onChange={() => void startShortcutCapture("push_to_talk")}
                      onReset={() =>
                        void saveShortcut("push_to_talk", defaultShortcuts.push_to_talk)
                      }
                      onCancel={() => void cancelShortcutCapture()}
                      platform={capabilities.platform}
                    />

                    <ShortcutRow
                      title={t("settings.shortcuts.toggle")}
                      description={t("settings.shortcuts.toggleDescription")}
                      shortcut={settings.toggleShortcut}
                      defaultShortcut={defaultShortcuts.toggle}
                      capturing={capturingShortcut === "toggle"}
                      disabled={!!capturingShortcut && capturingShortcut !== "toggle"}
                      error={shortcutErrorKind === "toggle" ? shortcutError : undefined}
                      onChange={() => void startShortcutCapture("toggle")}
                      onReset={() => void saveShortcut("toggle", defaultShortcuts.toggle)}
                      onCancel={() => void cancelShortcutCapture()}
                      platform={capabilities.platform}
                    />
                  </>
                ) : (
                  <div className="settings-row">
                    <div className="settings-row-info">
                      <h3 className="settings-row-title">
                        {t("settings.shortcuts.unavailableTitle")}
                      </h3>
                      <p className="settings-row-description">
                        {t("settings.shortcuts.unavailableDescription")}
                      </p>
                    </div>
                  </div>
                )}
              </div>
            </div>
          </section>
        ) : null}

        {activeTab === "dictation" ? (
          <>
            <section className="settings-group" aria-labelledby="dictation-heading">
              <SettingsPageHeader
                id="dictation-heading"
                title={t("settings.tabs.dictation")}
                blurb={t("settings.dictation.blurb")}
              />
              <div className="settings-card">
                <div className="settings-rows">
                  <div className="settings-row">
                    <div className="settings-row-info">
                      <h3 className="settings-row-title">{t("common.language")}</h3>
                      <p className="settings-row-description">
                        {t("settings.dictation.languageDescription")}
                      </p>
                    </div>
                    <div className="settings-row-control" ref={languageWrapRef}>
                      <button
                        type="button"
                        className="select-trigger settings-language-select"
                        aria-label={t("settings.dictation.languageAria")}
                        aria-haspopup="listbox"
                        aria-expanded={languageOpen}
                        onClick={() => setLanguageOpen((value) => !value)}
                      >
                        <span>{languageLabel(settings.language ?? "")}</span>
                        <IconChevronDownSmall size={14} />
                      </button>
                      {languageOpen ? (
                        <ul
                          className="select-popover"
                          role="listbox"
                          data-placement={languagePopoverPlacement}
                          style={languagePopoverStyle()}
                        >
                          {LANGUAGE_OPTIONS.map((option) => {
                            const selected = option.value === (settings.language ?? "");
                            return (
                              <li key={option.value || "auto"}>
                                <button
                                  type="button"
                                  role="option"
                                  aria-selected={selected}
                                  data-selected={selected}
                                  onClick={() => void selectLanguage(option.value)}
                                >
                                  <span>{option.label}</span>
                                  <span className="select-check" aria-hidden>
                                    {selected ? <IconCheckmark2Small size={14} /> : null}
                                  </span>
                                </button>
                              </li>
                            );
                          })}
                        </ul>
                      ) : null}
                    </div>
                  </div>
                </div>
              </div>
            </section>

            <StyleSettingsSection />

            <DictionarySettingsSection />
          </>
        ) : null}

        {activeTab === "audio" ? (
          <section className="settings-group" aria-labelledby="audio-heading">
            <SettingsPageHeader
              id="audio-heading"
              title={t("settings.tabs.audio")}
              blurb={
                capabilities.platform === "windows"
                  ? t("settings.audio.blurbWindows")
                  : t("settings.audio.blurb")
              }
            />
            <div className="settings-card">
              <div className="settings-rows">
                <div className="settings-row">
                  <div className="settings-row-info">
                    <h3 className="settings-row-title">{t("settings.audio.microphone")}</h3>
                    <p className="settings-row-description">{microphoneDescription}</p>
                  </div>
                  <div className="settings-row-control" ref={micWrapRef}>
                    <button
                      type="button"
                      className="select-trigger"
                      aria-haspopup="listbox"
                      aria-expanded={micOpen}
                      onClick={() => {
                        setMicOpen((value) => !value);
                        void requestMicrophones();
                      }}
                    >
                      <span>{microphoneName}</span>
                      <IconChevronDownSmall size={14} />
                    </button>
                    {micOpen ? (
                      // selectPopoverStyle offsets for the popover chrome and
                      // the trigger/row height difference, so the selected
                      // item overlays the trigger exactly with no visual jump.
                      <ul
                        className="select-popover"
                        role="listbox"
                        data-placement={micPopoverPlacement}
                        style={microphonePopoverStyle()}
                      >
                        {microphoneOptions.map((option) => {
                          const selected = (option.id ?? "") === (settings.microphone.id ?? "");
                          return (
                            <li key={option.id ?? "auto"}>
                              <button
                                type="button"
                                role="option"
                                aria-selected={selected}
                                data-selected={selected}
                                onClick={() =>
                                  void selectMicrophone(
                                    option.id,
                                    option.id ? option.name : undefined,
                                  )
                                }
                              >
                                <span>{option.name}</span>
                                <span className="select-check" aria-hidden>
                                  {selected ? <IconCheckmark2Small size={14} /> : null}
                                </span>
                              </button>
                            </li>
                          );
                        })}
                      </ul>
                    ) : null}
                  </div>
                </div>

                {capabilities.platform === "macos" || capabilities.platform === "windows" ? (
                  <MicTestControl
                    state={micTestState}
                    level={micTestLevel}
                    elapsedMs={micTestElapsedMs}
                    sampleSrc={micTestSampleSrc}
                    error={micTestError}
                    playing={micTestPlaying}
                    durationSeconds={MIC_TEST_DURATION_SECONDS}
                    onStart={() => void startMicTest()}
                    onStartOver={() => void startOverMicTest()}
                    onPlaybackError={() => {
                      setMicTestError(t("settings.micTest.playbackUnavailable"));
                    }}
                    onPlayingChange={setMicTestPlaying}
                  />
                ) : null}

                {systemUnavailable ? null : (
                  <div className="settings-row">
                    <div className="settings-row-info">
                      <h3 className="settings-row-title">{t("settings.audio.systemAudio")}</h3>
                      <p className="settings-row-description">
                        {t("settings.audio.systemAudioDescription")}
                      </p>
                    </div>
                    <div className="settings-row-control">
                      {systemDenied ? (
                        <button
                          type="button"
                          className="btn btn-secondary"
                          onClick={onEnableSystemAudio}
                        >
                          {t("common.enable")}
                        </button>
                      ) : null}
                      <Switch
                        checked={systemOn}
                        disabled={checkingSourceReadiness || systemLocked}
                        aria-label={t("settings.audio.systemAudioAria")}
                        onCheckedChange={(next) =>
                          onSourceModeChange(next ? "microphonePlusSystem" : "microphoneOnly")
                        }
                      />
                    </div>
                  </div>
                )}
              </div>
            </div>
          </section>
        ) : null}

        {activeTab === "models" ? (
          <>
            <SettingsPageHeader
              title={t("settings.tabs.models")}
              blurb={t("settings.models.blurb")}
            />
            <section
              className="settings-group settings-models-group"
              aria-labelledby="voice-models-heading"
            >
              <h2 id="voice-models-heading" className="settings-group-heading">
                {t("settings.models.voice")}
              </h2>
              <p className="settings-group-description">{t("settings.models.voiceDescription")}</p>
              {showingPartitionModels ? (
                <p className="settings-models-profile-note">
                  Showing models for the current data set: {currentDataPartitionLabel}. Switch to
                  the default data set to edit global models.
                </p>
              ) : null}
              <div className="settings-card settings-models-card">
                <div className="settings-rows">
                  <ModelRow
                    mode="transcription"
                    beforeDivider
                    title={t("settings.models.transcription")}
                    description={t("settings.models.transcriptionDescription")}
                    value={modelValueForMode("transcription")}
                    options={transcriptionOptions}
                    open={pickerMode === "transcription"}
                    summarySuppressed={pickerMode !== undefined}
                    flyout={modelPickerFlyout}
                    search={modelSearch}
                    triggerRef={modelPickerTriggerRef}
                    popoverRef={modelPickerPopoverRef}
                    searchRef={modelPickerSearchRef}
                    onToggle={() =>
                      pickerMode === "transcription"
                        ? closeModelPicker()
                        : openModelPicker("transcription")
                    }
                    onFlyoutChange={setModelPickerFlyout}
                    onSearchChange={setModelSearch}
                    onSelect={(modelId) => selectModelFromPicker("transcription", modelId)}
                    readOnly={showingPartitionModels}
                  />
                  <div className="settings-row-divider" aria-hidden />
                  <button
                    type="button"
                    className="settings-more-options-trigger settings-more-options-row"
                    aria-label={t("settings.models.moreVoiceAria")}
                    aria-expanded={showMoreVoiceOptions}
                    aria-controls="voice-more-options-panel"
                    onClick={() => setShowMoreVoiceOptions((open) => !open)}
                  >
                    <span className="settings-row-info">
                      <span className="settings-row-title">{t("common.moreOptions")}</span>
                      <span className="settings-row-description">
                        {t("settings.models.moreVoiceDescription")}
                      </span>
                    </span>
                    <IconChevronDownSmall
                      className="settings-more-options-chevron"
                      size={14}
                      aria-hidden
                    />
                  </button>
                  {showMoreVoiceOptions ? (
                    <div id="voice-more-options-panel" className="settings-more-options-panel">
                      <div className="settings-row">
                        <div className="settings-row-info">
                          <h3 className="settings-row-title">
                            {t("settings.liveTranscription.title")}
                          </h3>
                          <p className="settings-row-description">
                            {t("settings.liveTranscription.description")}
                          </p>
                        </div>
                        <div className="settings-row-control">
                          <Switch
                            checked={providerSettings.liveTranscription}
                            aria-label={t("settings.liveTranscription.aria")}
                            onCheckedChange={toggleLiveTranscription}
                          />
                        </div>
                      </div>
                    </div>
                  ) : null}
                </div>
              </div>
            </section>

            <section
              className="settings-group settings-models-group"
              aria-labelledby="text-models-heading"
            >
              <h2 id="text-models-heading" className="settings-group-heading">
                {t("settings.models.text")}
              </h2>
              <p className="settings-group-description">
                {t("settings.models.textGroupDescription")}
              </p>
              {showingPartitionModels ? (
                <p className="settings-models-profile-note">
                  Showing models for the current data set: {currentDataPartitionLabel}. Switch to
                  the default data set to edit global models.
                </p>
              ) : null}
              <div className="settings-card settings-models-card">
                <div className="settings-rows">
                  <ModelRow
                    mode="generation"
                    beforeDivider={providerSettings.generationModel !== "open-software/auto"}
                    title={t("settings.models.text")}
                    description={t("settings.models.textDescription")}
                    value={modelValueForMode("generation")}
                    options={generationOptions}
                    costQuality={providerSettings.costQuality}
                    veniceApiKeyConfigured={providerSettings.veniceApiKeyConfigured}
                    open={pickerMode === "generation"}
                    summarySuppressed={pickerMode !== undefined}
                    flyout={modelPickerFlyout}
                    search={modelSearch}
                    triggerRef={modelPickerTriggerRef}
                    popoverRef={modelPickerPopoverRef}
                    searchRef={modelPickerSearchRef}
                    onToggle={() =>
                      pickerMode === "generation"
                        ? closeModelPicker()
                        : openModelPicker("generation")
                    }
                    onFlyoutChange={setModelPickerFlyout}
                    onSearchChange={setModelSearch}
                    onSelect={(modelId, costQuality, options) =>
                      selectModelFromPicker("generation", modelId, costQuality, options)
                    }
                    onCostQualityChange={applyCostQuality}
                    readOnly={showingPartitionModels}
                  />
                  {providerSettings.generationModel === "open-software/auto" ? (
                    <div className="settings-row settings-row-before-divider">
                      <div className="settings-row-info">
                        <span className="settings-row-title">
                          {t("settings.autoPreference.title")}
                        </span>
                        <span className="settings-row-description">
                          {t("settings.autoPreference.description")}
                        </span>
                        {providerSettings.veniceApiKeyConfigured ? (
                          <span className="settings-row-description settings-row-substatus">
                            {t("settings.autoPreference.veniceNote")}
                          </span>
                        ) : null}
                      </div>
                      <div className="settings-row-control">
                        <SegmentedControl<AutoPreference>
                          aria-label={t("settings.autoPreference.title")}
                          value={autoPreferenceFromCostQuality(providerSettings.costQuality)}
                          options={autoPreferenceOptions(t)}
                          onValueChange={(preference) =>
                            applyCostQuality(AUTO_PREFERENCE_VALUES[preference])
                          }
                        />
                      </div>
                    </div>
                  ) : null}
                  <div className="settings-row-divider" aria-hidden />
                  <button
                    type="button"
                    className="settings-more-options-trigger settings-more-options-row"
                    aria-label={t("settings.models.moreTextAria")}
                    aria-expanded={showMoreTextOptions}
                    aria-controls="text-more-options-panel"
                    onClick={() => setShowMoreTextOptions((open) => !open)}
                  >
                    <span className="settings-row-info">
                      <span className="settings-row-title">{t("common.moreOptions")}</span>
                      <span className="settings-row-description">
                        {t("settings.models.moreTextDescription")}
                      </span>
                    </span>
                    <IconChevronDownSmall
                      className="settings-more-options-chevron"
                      size={14}
                      aria-hidden
                    />
                  </button>
                  {showMoreTextOptions ? (
                    <div id="text-more-options-panel" className="settings-more-options-panel">
                      <VeniceApiKeyRow
                        configured={providerSettings.veniceApiKeyConfigured}
                        value={veniceApiKeyDraft}
                        onValueChange={setVeniceApiKeyDraft}
                        onSave={() => void saveVeniceApiKey()}
                        onRemove={() => void removeVeniceApiKey()}
                      />
                    </div>
                  ) : null}
                </div>
              </div>
            </section>

            <LlmProvidersSection
              onChatProviderChanged={async () => {
                try {
                  const modelResponse = await providerModelSettings();
                  const modelSnapshot = providerModelSettingsSnapshot(modelResponse);
                  setProviderSettings(modelSnapshot.settings);
                  setEffectiveProviderSettings(modelSnapshot.effectiveSettings);
                  dispatchProviderModelSettingsChanged({
                    mode: "generation",
                    modelId: modelSnapshot.settings.generationModel,
                  });
                } catch {
                  // Provider settings refresh failed
                }
              }}
            />

            <ConfirmDialog
              open={veniceKeyAutoBillingChoiceOpen}
              onClose={() => setVeniceKeyAutoBillingChoiceOpen(false)}
              onConfirm={async () => {
                const switched = await selectVeniceModel(
                  "generation",
                  veniceKeySwitchTarget?.id ?? DEFAULT_GENERATION_SUGGESTION_ID,
                );
                // Keep the dialog open over a failed save so the choice is
                // never silently dropped; the status line carries the error.
                if (!switched) throw new Error("venice_model_switch_failed");
              }}
              title={t("settings.venice.autoDialogTitle")}
              description={t("settings.venice.autoDialogDescription", {
                model: veniceKeySwitchTarget?.name ?? t("settings.venice.aVeniceModel"),
              })}
              confirmLabel={t("settings.venice.useModel", {
                model: veniceKeySwitchTarget?.name ?? t("settings.venice.aVeniceModel"),
              })}
              cancelLabel={t("settings.venice.keepAuto")}
            />

            {IMAGE_GENERATION_ENABLED || VIDEO_GENERATION_ENABLED ? (
              <section
                className="settings-group settings-models-group"
                aria-labelledby="media-generation-heading"
              >
                <h2 id="media-generation-heading" className="settings-group-heading">
                  {t("settings.models.imageAndVideo")}
                </h2>
                <p className="settings-group-description">
                  {t("settings.models.mediaDescription")}
                </p>
                {showingPartitionModels ? (
                  <p className="settings-models-profile-note">
                    Showing models for the current data set: {currentDataPartitionLabel}. Switch to
                    the default data set to edit global models.
                  </p>
                ) : null}
                <div className="settings-card settings-models-card">
                  <div className="settings-rows">
                    {IMAGE_GENERATION_ENABLED ? (
                      <ModelRow
                        mode="image"
                        beforeDivider={!VIDEO_GENERATION_ENABLED}
                        title={t("settings.models.image")}
                        description={t("settings.models.imageDescription")}
                        value={modelValueForMode("image")}
                        options={imageOptions}
                        open={pickerMode === "image"}
                        summarySuppressed={pickerMode !== undefined}
                        flyout={modelPickerFlyout}
                        search={modelSearch}
                        triggerRef={modelPickerTriggerRef}
                        popoverRef={modelPickerPopoverRef}
                        searchRef={modelPickerSearchRef}
                        onToggle={() =>
                          pickerMode === "image" ? closeModelPicker() : openModelPicker("image")
                        }
                        onFlyoutChange={setModelPickerFlyout}
                        onSearchChange={setModelSearch}
                        onSelect={(modelId) => selectModelFromPicker("image", modelId)}
                        readOnly={showingPartitionModels}
                      />
                    ) : null}
                    {VIDEO_GENERATION_ENABLED ? (
                      <ModelRow
                        mode="video"
                        beforeDivider
                        title={t("settings.models.video")}
                        description={t("settings.models.videoDescription")}
                        value={modelValueForMode("video")}
                        options={videoOptions}
                        open={pickerMode === "video"}
                        summarySuppressed={pickerMode !== undefined}
                        flyout={modelPickerFlyout}
                        search={modelSearch}
                        triggerRef={modelPickerTriggerRef}
                        popoverRef={modelPickerPopoverRef}
                        searchRef={modelPickerSearchRef}
                        onToggle={() =>
                          pickerMode === "video" ? closeModelPicker() : openModelPicker("video")
                        }
                        onFlyoutChange={setModelPickerFlyout}
                        onSearchChange={setModelSearch}
                        onSelect={(modelId) => selectModelFromPicker("video", modelId)}
                        readOnly={showingPartitionModels}
                      />
                    ) : null}
                    <div className="settings-row-divider" aria-hidden />
                    <button
                      type="button"
                      className="settings-more-options-trigger settings-more-options-row"
                      aria-label={t("settings.models.moreMediaAria")}
                      aria-expanded={showMoreImageOptions}
                      aria-controls="image-more-options-panel"
                      onClick={() => setShowMoreImageOptions((open) => !open)}
                    >
                      <span className="settings-row-info">
                        <span className="settings-row-title">{t("common.moreOptions")}</span>
                        <span className="settings-row-description">
                          {t("settings.models.moreMediaDescription")}
                        </span>
                      </span>
                      <IconChevronDownSmall
                        className="settings-more-options-chevron"
                        size={14}
                        aria-hidden
                      />
                    </button>
                    {showMoreImageOptions ? (
                      <div id="image-more-options-panel" className="settings-more-options-panel">
                        <div className="settings-row">
                          <div className="settings-row-info">
                            <h3 className="settings-row-title">{t("settings.safeMode.title")}</h3>
                            <p className="settings-row-description">
                              {VIDEO_GENERATION_ENABLED
                                ? t("settings.safeMode.descriptionWithVideo")
                                : t("settings.safeMode.description")}
                            </p>
                          </div>
                          <div className="settings-row-control">
                            <Switch
                              checked={providerSettings.imageSafeMode}
                              aria-label={t("settings.safeMode.aria")}
                              onCheckedChange={toggleImageSafeMode}
                            />
                          </div>
                        </div>
                      </div>
                    ) : null}
                  </div>
                </div>
              </section>
            ) : null}

            {status ? (
              <p className="settings-status" role="status">
                {status}
              </p>
            ) : null}
          </>
        ) : null}

        {activeTab === "agent" ? (
          <AgentSettingsSection folders={folders} onFoldersImported={onFoldersImported} />
        ) : null}

        {activeTab === "memory" ? (
          <MemorySettingsSection
            folders={folders}
            initialFolderFilter={memoryFolderFilter}
            onOpenProject={onOpenProject}
          />
        ) : null}

        {activeTab === "connectors" ? (
          <>
            <ConnectorsSection
              onOpenModels={() => setActiveTab("models")}
              onOpenBilling={() => setActiveTab("billing")}
            />
            <AgentMcpServersSection />
          </>
        ) : null}

        {activeTab === "linked-devices" && experimentalFlags.companionPairingEnabled ? (
          <LinkedDevicesSection />
        ) : null}

        {activeTab === "about" ? (
          <section className="settings-group" aria-labelledby="about-heading">
            <SettingsPageHeader
              id="about-heading"
              title={t("settings.tabs.about")}
              blurb={t("settings.about.blurb")}
            />
            <div className="settings-card">
              <div className="settings-rows">
                <div className="settings-row settings-row-meta">
                  <div className="settings-row-info">
                    <h3 className="settings-row-title settings-meta-label">
                      {t("settings.about.releaseVersion")}
                    </h3>
                  </div>
                  <div className="settings-row-control">
                    <button
                      type="button"
                      className="settings-meta-value settings-meta-unlock-trigger"
                      onClick={handleReleaseVersionClick}
                    >
                      {APP_VERSION}
                    </button>
                  </div>
                </div>

                <div className="settings-row settings-row-meta">
                  <div className="settings-row-info">
                    <h3 className="settings-row-title settings-meta-label">
                      {t("settings.about.commit")}
                    </h3>
                  </div>
                  <div className="settings-row-control">
                    <span className="settings-meta-value settings-meta-value-mono">
                      {APP_COMMIT_HASH}
                    </span>
                  </div>
                </div>

                {onCheckForUpdates ? (
                  <>
                    <div className="settings-row">
                      <div className="settings-row-info">
                        <h3 className="settings-row-title">{t("settings.about.updates")}</h3>
                        <p className="settings-row-description">
                          {t("settings.about.updatesDescription")}
                        </p>
                      </div>
                      <div className="settings-row-control">
                        <button
                          type="button"
                          className="btn btn-secondary"
                          onClick={onCheckForUpdates}
                        >
                          {t("settings.about.checkForUpdates")}
                        </button>
                      </div>
                    </div>

                    <div className="settings-row">
                      <div className="settings-row-info">
                        <h3 className="settings-row-title">{t("settings.releaseChannel.title")}</h3>
                        <p className="settings-row-description">
                          {t("settings.releaseChannel.description")}
                        </p>
                      </div>
                      <div className="settings-row-control">
                        <SegmentedControl<ReleaseChannel>
                          aria-label={t("settings.releaseChannel.title")}
                          value={releaseChannel}
                          options={releaseChannelOptions(t)}
                          onValueChange={handleReleaseChannelChange}
                        />
                      </div>
                    </div>

                    {reconcileVersion ? (
                      <div className="settings-row">
                        <InlineNotice
                          aria-label={t("settings.releaseChannel.switchAria")}
                          eyebrow={t("settings.releaseChannel.switchEyebrow")}
                          body={t("settings.releaseChannel.switchBody", {
                            version: reconcileVersion,
                            base: baseVersion(),
                          })}
                          actions={
                            <>
                              <button
                                type="button"
                                className="btn btn-ghost"
                                onClick={() => setReconcileVersion(undefined)}
                              >
                                {t("settings.releaseChannel.notNow")}
                              </button>
                              <button
                                type="button"
                                className="btn btn-secondary"
                                onClick={confirmReconcileToStable}
                              >
                                {t("settings.releaseChannel.switch")}
                              </button>
                            </>
                          }
                        />
                      </div>
                    ) : null}
                  </>
                ) : null}

                <div className="settings-row">
                  <div className="settings-row-info">
                    <h3 className="settings-row-title">{t("settings.about.community")}</h3>
                    <p className="settings-row-description">
                      {t("settings.about.communityDescription", {
                        url: CLOVY_COMMUNITY_URL.replace("https://", ""),
                      })}
                    </p>
                  </div>
                  <div className="settings-row-control">
                    <button
                      type="button"
                      className="btn btn-secondary"
                      onClick={() => void clovyOpenCommunityPage().catch(() => undefined)}
                    >
                      {t("settings.about.joinCommunity")}
                    </button>
                  </div>
                </div>

                <div className="settings-row">
                  <div className="settings-row-info">
                    <h3 className="settings-row-title">{t("settings.about.verification")}</h3>
                    <p className="settings-row-description">
                      {t("settings.about.verificationDescription")}
                    </p>
                  </div>
                  <div className="settings-row-control">
                    <button
                      type="button"
                      className="btn btn-secondary"
                      onClick={() => void clovyOpenVerifyPage().catch(() => undefined)}
                    >
                      {t("settings.about.verify")}
                    </button>
                  </div>
                </div>

                {onReportIssue ? (
                  <div className="settings-row">
                    <div className="settings-row-info">
                      <h3 className="settings-row-title">{t("settings.about.reportIssue")}</h3>
                      <p className="settings-row-description">
                        {t("settings.about.reportIssueDescription")}
                      </p>
                    </div>
                    <div className="settings-row-control">
                      <button
                        type="button"
                        className="btn btn-secondary"
                        onClick={() => onReportIssue("bug")}
                      >
                        {t("settings.about.reportIssue")}
                      </button>
                    </div>
                  </div>
                ) : null}

                {import.meta.env.DEV ? (
                  // Dev builds only: same helper the devtools console exposes
                  // as clovy.replayOnboarding() — clears completion and
                  // reloads into the wizard.
                  <div className="settings-row">
                    <div className="settings-row-info">
                      <h3 className="settings-row-title">{t("settings.about.replayOnboarding")}</h3>
                      <p className="settings-row-description">
                        {t("settings.about.replayOnboardingDescription")}
                      </p>
                    </div>
                    <div className="settings-row-control">
                      <button
                        type="button"
                        className="btn btn-secondary"
                        onClick={() => replayOnboarding()}
                      >
                        {t("settings.about.replayOnboarding")}
                      </button>
                    </div>
                  </div>
                ) : null}
              </div>
            </div>

            {experimentalFlags.unlocked ? (
              <div className="settings-card experiments-card">
                <div className="settings-rows">
                  <div className="settings-row">
                    <div className="settings-row-info">
                      <h3 className="settings-row-title">{t("settings.experiments.title")}</h3>
                      <p className="settings-row-description">
                        {t("settings.experiments.description")}
                      </p>
                    </div>
                    <div className="settings-row-control">
                      <button
                        type="button"
                        className="btn btn-secondary"
                        disabled={experimentalOperation !== undefined}
                        onClick={() => void updateExperimentalFlags({ unlocked: false })}
                      >
                        {t("settings.experiments.hide")}
                      </button>
                    </div>
                  </div>

                  <div className="settings-row">
                    <div className="settings-row-info">
                      <h3 className="settings-row-title">{t("settings.experiments.browserUse")}</h3>
                      <p className="settings-row-description">
                        {t("settings.experiments.browserUseDescription")}
                      </p>
                    </div>
                    <div className="settings-row-control">
                      <Switch
                        checked={experimentalFlags.browser_use}
                        disabled={experimentalOperation !== undefined}
                        aria-label={t("settings.experiments.browserUseAria")}
                        onCheckedChange={(browser_use) =>
                          void updateExperimentalFlags({ browser_use })
                        }
                      />
                    </div>
                  </div>

                  <div className="settings-row">
                    <div className="settings-row-info">
                      <h3 className="settings-row-title">{t("settings.experiments.companion")}</h3>
                      <p className="settings-row-description">
                        {experimentalFlags.companion_pairing ===
                        experimentalFlags.companionPairingEnabled
                          ? t("settings.experiments.companionDescription")
                          : experimentalFlags.companionPairingEnabled
                            ? t("settings.experiments.companionPendingOff")
                            : t("settings.experiments.companionPendingOn")}
                      </p>
                    </div>
                    <div className="settings-row-control">
                      <Switch
                        checked={experimentalFlags.companion_pairing}
                        disabled={experimentalOperation !== undefined}
                        aria-label={t("settings.experiments.companionAria")}
                        onCheckedChange={(companion_pairing) =>
                          void updateExperimentalFlags({ companion_pairing })
                        }
                      />
                    </div>
                  </div>

                  {experimentalRestartNeeded ? (
                    <div className="settings-row">
                      <div className="settings-row-info">
                        <h3 className="settings-row-title">
                          {t("settings.experiments.agentRuntime")}
                        </h3>
                        <p className="settings-row-description">
                          {t("settings.experiments.agentRuntimeDescription")}
                        </p>
                      </div>
                      <div className="settings-row-control">
                        <button
                          type="button"
                          className="btn btn-secondary"
                          disabled={experimentalOperation === "restart"}
                          onClick={() => void restartAgentForExperimentalFlags()}
                        >
                          {experimentalOperation === "restart"
                            ? t("settings.experiments.restarting")
                            : t("settings.experiments.restart")}
                        </button>
                      </div>
                    </div>
                  ) : null}

                  <div className="settings-row">
                    <div className="settings-row-info">
                      <h3 className="settings-row-title">{t("settings.experiments.extension")}</h3>
                      <p className="settings-row-description">
                        {t("settings.experiments.extensionDescription")}
                      </p>
                    </div>
                    <div className="settings-row-control">
                      <button
                        type="button"
                        className="btn btn-secondary"
                        disabled={experimentalOperation === "unpack"}
                        onClick={() => void unpackExperimentalExtension()}
                      >
                        {experimentalOperation === "unpack"
                          ? t("settings.experiments.unpacking")
                          : t("settings.experiments.unpack")}
                      </button>
                    </div>
                  </div>
                </div>
                {experimentalError ? (
                  <p className="settings-row-error" role="alert">
                    {experimentalError}
                  </p>
                ) : null}
              </div>
            ) : null}
          </section>
        ) : null}
      </div>
    </div>
  );
}

type PermissionStatusTone = "allowed" | "attention" | "blocked" | "unsupported" | "unknown";

type PermissionStatusView = {
  label: string;
  tone: PermissionStatusTone;
};

/** Launch-at-login toggle. Reads and writes the OS login item directly (the
 * LaunchAgent is the single source of truth), so state here can never drift
 * from what System Settings shows. Hidden in browser previews, where no
 * autostart backend exists. */
function StartupSettingsSection() {
  const t = useT();
  const [enabled, setEnabled] = useState<boolean>();
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string>();

  useEffect(() => {
    if (!autostartSupported()) return;
    let cancelled = false;
    autostartEnabled()
      .then((value) => {
        if (!cancelled) setEnabled(value);
      })
      .catch(() => {
        if (!cancelled) setError(translate("settings.startup.readError"));
      });
    return () => {
      cancelled = true;
    };
  }, []);

  async function toggle(next: boolean) {
    setSaving(true);
    setError(undefined);
    try {
      await setAutostartEnabled(next);
      setEnabled(next);
    } catch {
      setError(translate("settings.startup.updateError"));
    } finally {
      setSaving(false);
    }
  }

  if (!autostartSupported() || (enabled === undefined && !error)) return null;

  return (
    <section className="settings-group" aria-labelledby="startup-heading">
      <h2 id="startup-heading" className="settings-group-heading">
        {t("settings.startup.title")}
      </h2>
      <p className="settings-group-description">{t("settings.startup.description")}</p>
      <div className="settings-card">
        <div className="settings-rows">
          <div className="settings-row">
            <div className="settings-row-info">
              <h3 className="settings-row-title">{t("settings.startup.openAtLogin")}</h3>
              <p className="settings-row-description">
                {t("settings.startup.openAtLoginDescription")}
              </p>
            </div>
            <div className="settings-row-control">
              <Switch
                checked={enabled === true}
                disabled={saving || enabled === undefined}
                aria-label={t("settings.startup.openAtLogin")}
                onCheckedChange={(next) => void toggle(next)}
              />
            </div>
          </div>
          {error ? <p className="settings-row-description">{error}</p> : null}
        </div>
      </div>
    </section>
  );
}

function PermissionsSettingsSection({
  microphonePermissionStatus,
  microphoneReadiness,
  accessibilityPermissionStatus,
  systemReadiness,
  onEnableMicrophone,
  onEnableAccessibility,
  onEnableSystemAudio,
}: {
  microphonePermissionStatus?: string;
  microphoneReadiness?: RecordingSourceReadinessDto["sources"][number];
  accessibilityPermissionStatus?: string;
  systemReadiness?: RecordingSourceReadinessDto["sources"][number];
  onEnableMicrophone?: () => void;
  onEnableAccessibility?: () => void;
  onEnableSystemAudio: () => void;
}) {
  const t = useT();
  const macLikePlatform = fallbackDictationCapabilities().platform === "macos";
  const systemAudioSupportedPlatform = isSystemAudioSupportedPlatform();
  return (
    <section className="settings-group" aria-labelledby="permissions-heading">
      <h2 id="permissions-heading" className="settings-group-heading">
        {macLikePlatform
          ? t("settings.permissions.systemTitle")
          : t("settings.permissions.audioAccessTitle")}
      </h2>
      <p className="settings-group-description">
        {macLikePlatform
          ? t("settings.permissions.macDescription")
          : systemAudioSupportedPlatform
            ? t("settings.permissions.systemAudioDescription")
            : t("settings.permissions.micOnlyDescription")}
      </p>
      <div className="settings-card">
        <div className="settings-rows">
          <PermissionRow
            title={t("settings.audio.microphone")}
            description={t("settings.permissions.microphoneDescription")}
            status={permissionStatus(
              t,
              microphonePermissionStatus ?? microphoneReadiness?.permissionState,
            )}
            onManage={onEnableMicrophone}
          />

          {macLikePlatform ? (
            <>
              <PermissionRow
                title={t("settings.permissions.accessibility")}
                description={t("settings.permissions.accessibilityDescription")}
                status={permissionStatus(t, accessibilityPermissionStatus)}
                onManage={onEnableAccessibility}
              />

              {systemAudioSupportedPlatform ? (
                <PermissionRow
                  title={t("settings.audio.systemAudio")}
                  description={t("settings.permissions.systemAudioRowDescription")}
                  status={sourcePermissionStatus(t, systemReadiness, macLikePlatform)}
                  onManage={onEnableSystemAudio}
                />
              ) : null}
            </>
          ) : systemAudioSupportedPlatform ? (
            <PermissionRow
              title={t("settings.audio.systemAudio")}
              description={t("settings.permissions.systemAudioRowDescription")}
              status={sourcePermissionStatus(t, systemReadiness, macLikePlatform)}
            />
          ) : null}
        </div>
      </div>
    </section>
  );
}

function PermissionRow({
  title,
  description,
  status,
  onManage,
  actionLabel,
  actionText,
}: {
  title: string;
  description: string;
  status: PermissionStatusView;
  onManage?: () => void;
  actionLabel?: string;
  actionText?: string;
}) {
  const t = useT();
  const actionDisabled = status.tone === "unsupported" || !onManage;
  return (
    <div className="settings-row">
      <div className="settings-row-info">
        <h3 className="settings-row-title">{title}</h3>
        <p className="settings-row-description">{description}</p>
      </div>
      <div className="settings-row-control settings-permission-control">
        <span
          className="settings-permission-status"
          data-status={status.tone}
          role="img"
          aria-label={status.label}
          title={status.label}
        >
          <PermissionStatusIcon tone={status.tone} />
        </span>
        {onManage ? (
          <button
            type="button"
            className="btn btn-secondary"
            disabled={actionDisabled}
            aria-label={actionLabel ?? t("settings.permissions.manageAria", { name: title })}
            onClick={onManage}
          >
            {actionText ?? t("settings.permissions.manage")}
          </button>
        ) : null}
      </div>
    </div>
  );
}

function PermissionStatusIcon({ tone }: { tone: PermissionStatusTone }) {
  if (tone === "allowed") return <IconCircleCheck size={16} />;
  if (tone === "unknown") return <IconCircleQuestionmark size={16} />;
  if (tone === "unsupported") return <IconCircleX size={16} />;
  return <IconExclamationCircle size={16} />;
}

function permissionStatus(t: TFunction, state?: string): PermissionStatusView {
  switch (state) {
    case "granted":
      return { label: t("settings.permissions.status.allowed"), tone: "allowed" };
    case "denied":
      return { label: t("settings.permissions.status.blocked"), tone: "blocked" };
    case "restricted":
      return { label: t("settings.permissions.status.restricted"), tone: "blocked" };
    case "missing":
      return { label: t("settings.permissions.status.needsAccess"), tone: "attention" };
    case "not_determined":
      return { label: t("settings.permissions.status.notRequested"), tone: "attention" };
    case "unavailable":
      return { label: t("settings.permissions.status.noMicrophone"), tone: "attention" };
    case "unsupported":
      return { label: t("settings.permissions.status.unsupported"), tone: "unsupported" };
    case "unknown":
      return { label: t("settings.permissions.status.unknown"), tone: "unknown" };
    default:
      return { label: t("settings.permissions.status.checking"), tone: "unknown" };
  }
}

function sourcePermissionStatus(
  t: TFunction,
  source: RecordingSourceReadinessDto["sources"][number] | undefined,
  macLikePlatform: boolean,
): PermissionStatusView {
  if (!source) return { label: t("settings.permissions.status.checking"), tone: "unknown" };
  // The two halves are independent: permissionState is the platform grant or
  // endpoint status, while `ready` says whether this device can actually
  // capture. A microphone-only check never asks for the grant/status, and a
  // granted source can still be uncapturable.
  if (source.permissionState === "granted") {
    return source.ready
      ? {
          label: macLikePlatform
            ? t("settings.permissions.status.allowed")
            : t("settings.permissions.status.available"),
          tone: "allowed",
        }
      : { label: t("settings.permissions.status.unavailable"), tone: "attention" };
  }
  return permissionStatus(t, source.permissionState);
}

function stringPayload(value: unknown) {
  return typeof value === "string" ? value : undefined;
}

function numericPayload(value: unknown) {
  if (typeof value === "number" && Number.isFinite(value)) {
    return Math.max(0, Math.min(1, value));
  }
  if (typeof value === "string") {
    const parsed = Number(value);
    if (Number.isFinite(parsed)) return Math.max(0, Math.min(1, parsed));
  }
  return 0;
}

function ModelRow({
  mode,
  beforeDivider = false,
  title,
  description,
  value,
  options,
  costQuality,
  veniceApiKeyConfigured,
  open,
  flyout,
  search,
  triggerRef,
  popoverRef,
  searchRef,
  onToggle,
  onFlyoutChange,
  onSearchChange,
  onSelect,
  onCostQualityChange,
  readOnly = false,
  summarySuppressed,
}: {
  mode: ProviderModelMode;
  beforeDivider?: boolean;
  title: string;
  description: string;
  value: string;
  options: VeniceModelDto[];
  costQuality?: number;
  veniceApiKeyConfigured?: boolean;
  open: boolean;
  flyout: ModelPickerFlyout;
  search: string;
  triggerRef: RefObject<HTMLButtonElement>;
  popoverRef: RefObject<HTMLDivElement>;
  searchRef: RefObject<HTMLInputElement>;
  onToggle: () => void;
  onFlyoutChange: (flyout: ModelPickerFlyout) => void;
  onSearchChange: (value: string) => void;
  onSelect: (modelId: string, costQuality?: number, options?: { keepOpen?: boolean }) => void;
  onCostQualityChange?: (value: number) => void;
  readOnly?: boolean;
  summarySuppressed?: boolean;
}) {
  const t = useT();
  const model = selectedModel(options, value);
  const modelLabel = t(`settings.models.modelLabel.${mode}`);
  return (
    <div
      className={`settings-row settings-model-row${
        beforeDivider ? " settings-row-before-divider" : ""
      }`}
    >
      <div className="settings-row-info">
        <h3 className="settings-row-title">{title}</h3>
        <p className="settings-row-description">{description}</p>
      </div>
      <div className="settings-row-control settings-model-control">
        <HoverTip
          tip={<ModelSummaryHoverDetails model={model} />}
          className="model-summary-tip-anchor"
          width={280}
          delay={280}
          suppressed={summarySuppressed || open}
          interactive
        >
          <button
            ref={open && !readOnly ? triggerRef : undefined}
            type="button"
            className="model-summary-button"
            onClick={readOnly ? undefined : onToggle}
            aria-label={t("settings.models.changeAria", { model: modelLabel })}
            aria-haspopup="dialog"
            aria-expanded={readOnly ? false : open}
            disabled={readOnly}
          >
            <span
              className="model-summary-logo"
              data-brand={model.id === AUTO_MODEL_ID ? "clovy" : undefined}
              aria-hidden
            >
              <ProviderLogo provider={model.provider} id={model.id} name={model.name} />
            </span>
            <span className="model-summary-name">{model.name}</span>
            <IconChevronDownSmall size={14} aria-hidden />
          </button>
        </HoverTip>
        {open && !readOnly ? (
          <ModelPickerPopover
            mode={mode}
            flyout={flyout}
            model={model}
            options={options}
            costQuality={costQuality}
            veniceApiKeyConfigured={veniceApiKeyConfigured}
            search={search}
            popoverRef={popoverRef}
            searchRef={searchRef}
            className="settings-model-popover"
            title={mode === "generation" ? undefined : t(`settings.models.popoverTitle.${mode}`)}
            ariaLabel={t("settings.models.chooseAria", { model: modelLabel })}
            onFlyoutChange={onFlyoutChange}
            onSearchChange={onSearchChange}
            onSelect={onSelect}
            onCostQualityChange={onCostQualityChange}
            showAutoPreference={false}
          />
        ) : null}
      </div>
    </div>
  );
}

function ModelSummaryHoverDetails({ model }: { model: VeniceModelDto }) {
  return (
    <div className="agent-composer-model-detail model-summary-hovercard">
      {/* Read-only summary card: full description in one hover (the card shows
          it in a capped scroll box; there is no "Show more" toggle anywhere). */}
      <ModelPickerCardContent model={model} withDescription />
    </div>
  );
}

function VeniceApiKeyRow({
  id,
  configured,
  value,
  onValueChange,
  onSave,
  onRemove,
}: {
  id?: string;
  configured: boolean;
  value: string;
  onValueChange: (value: string) => void;
  onSave: () => void;
  onRemove: () => void;
}) {
  const t = useT();
  const canSave = value.trim().length > 0;
  return (
    <div id={id} className="settings-row settings-row-venice-key">
      <div className="settings-row-info">
        <h3 className="settings-row-title">{t("settings.venice.title")}</h3>
        <p className="settings-row-description">{t("settings.venice.description")}</p>
        {configured ? (
          <p className="settings-row-description settings-row-substatus">
            {t("settings.venice.keySaved")}
          </p>
        ) : null}
      </div>
      <div className="settings-row-control settings-secret-control">
        <label className="settings-field settings-secret-field">
          <span>{t("settings.venice.apiKey")}</span>
          <input
            type="password"
            className="dialog-input"
            value={value}
            autoComplete="off"
            spellCheck={false}
            placeholder={configured ? t("settings.venice.savedHidden") : t("settings.venice.title")}
            aria-label={t("settings.venice.title")}
            onChange={(event) => onValueChange(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter" && canSave) onSave();
            }}
          />
        </label>
        <button type="button" className="btn btn-secondary" disabled={!canSave} onClick={onSave}>
          {t("common.save")}
        </button>
        {configured ? (
          <button type="button" className="btn btn-secondary" onClick={onRemove}>
            {t("common.remove")}
          </button>
        ) : null}
      </div>
    </div>
  );
}

function ShortcutRow({
  title,
  description,
  shortcut,
  defaultShortcut,
  capturing,
  disabled,
  error,
  onChange,
  onReset,
  onCancel,
  platform,
}: {
  title: string;
  description: string;
  shortcut: DictationShortcutSetting;
  defaultShortcut: DictationShortcutSetting;
  capturing: boolean;
  disabled: boolean;
  error?: string;
  onChange: () => void;
  onReset: () => void;
  onCancel: () => void;
  platform: "macos" | "windows" | "unsupported";
}) {
  const t = useT();
  const canReset = !capturing && !shortcutsMatch(shortcut, defaultShortcut) && !disabled;

  return (
    <div className="settings-row">
      <div className="settings-row-info">
        <h3 className="settings-row-title">{title}</h3>
        <p className="settings-row-description">{description}</p>
        {error ? <p className="settings-row-error">{error}</p> : null}
      </div>
      <div className="settings-row-control">
        <KeycapShortcut label={shortcut.label} capturing={capturing} platform={platform} />
        <button
          type="button"
          className="btn btn-secondary"
          disabled={disabled}
          onClick={capturing ? onCancel : onChange}
        >
          {capturing ? t("common.cancel") : t("settings.shortcuts.change")}
        </button>
        {canReset ? (
          <button
            type="button"
            className="btn btn-secondary"
            aria-label={t("settings.shortcuts.resetAria", { name: title })}
            onClick={onReset}
          >
            {t("settings.shortcuts.reset")}
          </button>
        ) : null}
      </div>
    </div>
  );
}

function shortcutKindPayload(value: unknown): DictationShortcutKind | undefined {
  return value === "push_to_talk" || value === "toggle" ? value : undefined;
}

function shortcutKindLabel(kind: DictationShortcutKind) {
  return kind === "toggle"
    ? translate("settings.shortcuts.toggle")
    : translate("settings.shortcuts.pushToTalk");
}

function shortcutForKind(settings: DictationSettingsDto, kind: DictationShortcutKind) {
  return kind === "toggle" ? settings.toggleShortcut : settings.pushToTalkShortcut;
}

function shortcutsMatch(first: DictationShortcutSetting, second: DictationShortcutSetting) {
  const keyCodesMatch =
    first.keyCode === undefined || second.keyCode === undefined || first.keyCode === second.keyCode;

  return (
    keyCodesMatch &&
    first.code === second.code &&
    first.label === second.label &&
    first.pressCount === second.pressCount &&
    first.modifiers.command === second.modifiers.command &&
    first.modifiers.control === second.modifiers.control &&
    first.modifiers.option === second.modifiers.option &&
    first.modifiers.shift === second.modifiers.shift &&
    first.modifiers.function === second.modifiers.function
  );
}

function stringPayloadValue(value: unknown) {
  return typeof value === "string" ? value : undefined;
}

function messageFromError(error: unknown) {
  if (error && typeof error === "object" && "message" in error) {
    return String((error as { message: unknown }).message);
  }
  return String(error);
}

// The running build is a release candidate (X.Y.Z-rc.N). Only these get the
// leave-rc reconcile offer; a clean stable build has nothing to reconcile.
function isPrereleaseBuild() {
  return APP_VERSION.includes("-rc");
}

// The base version an rc will become once promoted (0.0.25-rc.2 -> 0.0.25), used
// to reassure the user which stable they will land on when it ships.
function baseVersion() {
  return APP_VERSION.split("-")[0];
}
