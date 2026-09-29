import { useCallback, useEffect, useState } from "react";
import { IconCalendar1 } from "central-icons/IconCalendar1";
import { IconLock } from "central-icons/IconLock";
import { IconMicrophone } from "central-icons/IconMicrophone";
import { IconSparkle } from "central-icons/IconSparkle";
import { IconTelegram } from "central-icons/IconTelegram";
import { fallbackDictationCapabilities } from "../../../lib/platform";
import { clovyOpenCommunityPage, osAccountsCancelLogin, osAccountsLogin } from "../../../lib/tauri";
import type { AccountStatus } from "../../../lib/tauri";
import {
  INTERFACE_LOCALE_OPTIONS,
  type InterfaceLocale,
  type MessageKey,
  setInterfaceLocale,
  useLocale,
  useT,
  useTRich,
} from "../../../i18n";
import { OsMark } from "../../account/AccountGate";
import { Select } from "../../ui/Select";
import { OnboardingPrimaryButton, StepCard } from "../StepChrome";

type WelcomePoint = {
  icon: typeof IconSparkle;
  title: MessageKey;
  detail: MessageKey;
};

// Desktop platforms with bundled helpers can introduce the full agent,
// dictation, and notes surface. Unsupported platforms narrow the welcome
// promise until native helpers are turnkey there.
const CLOVY_POINTS: WelcomePoint[] = [
  {
    icon: IconSparkle,
    title: "onboarding.signIn.point.delegate.title",
    detail: "onboarding.signIn.point.delegate.detail",
  },
  {
    icon: IconMicrophone,
    title: "onboarding.signIn.point.voice.title",
    detail: "onboarding.signIn.point.voice.detail",
  },
  {
    icon: IconCalendar1,
    title: "onboarding.signIn.point.meetings.title",
    detail: "onboarding.signIn.point.meetings.detail",
  },
  {
    icon: IconLock,
    title: "onboarding.signIn.point.private.title",
    detail: "onboarding.signIn.point.private.detail",
  },
];

const WINDOWS_CLOVY_POINTS: WelcomePoint[] = [
  {
    icon: IconSparkle,
    title: "onboarding.signIn.point.together.title",
    detail: "onboarding.signIn.point.together.detail",
  },
  {
    icon: IconMicrophone,
    title: "onboarding.signIn.point.meetings.title",
    detail: "onboarding.signIn.point.recordings.detail",
  },
  CLOVY_POINTS[3],
];

/**
 * Step 1: welcome + sign-in, fused into one screen so the wizard frames the
 * very first thing a new user sees. The browser handoff resolves through the
 * deep link; when `osAccountsLogin` returns the step flips to a signed-in
 * greeting — one continue, no re-finding the app.
 */
export function SignInStep({
  account,
  onAccountChanged,
  onContinue,
}: {
  account: AccountStatus;
  onAccountChanged: (next: AccountStatus) => void;
  onContinue: () => void;
}) {
  const t = useT();
  const tr = useTRich();
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState<string>();
  const capabilities = fallbackDictationCapabilities();
  const points = capabilities.available ? CLOVY_POINTS : WINDOWS_CLOVY_POINTS;
  const introClassName = capabilities.available ? "welcome-card-intro" : undefined;

  const cancelInFlight = useCallback(async () => {
    try {
      await osAccountsCancelLogin();
    } catch {
      // The pending login promise rejects with "login_canceled"; handleSignIn's
      // catch surfaces the message, so there's nothing to do here.
    }
  }, []);

  useEffect(() => {
    return () => {
      if (busy) void cancelInFlight();
    };
  }, [busy, cancelInFlight]);

  async function handleSignIn() {
    setBusy(true);
    setStatus(undefined);
    try {
      const next = await osAccountsLogin();
      if (next.signedIn) {
        onAccountChanged(next);
        onContinue();
      } else {
        setStatus(t("onboarding.signIn.incomplete"));
      }
    } catch (error) {
      setStatus(messageFromError(error));
    } finally {
      setBusy(false);
    }
  }

  return (
    <StepCard
      title={t("onboarding.signIn.title")}
      subtitle={t("onboarding.signIn.subtitle")}
      mark
      wide
      className={introClassName}
    >
      <ul className="onboarding-points">
        {points.map(({ icon: Icon, title, detail }) => (
          <li key={title}>
            <span className="onboarding-point-icon" aria-hidden>
              <Icon size={15} />
            </span>
            <div>
              <span className="onboarding-point-label">{t(title)}</span>
              <span className="onboarding-point-detail">{t(detail)}</span>
            </div>
          </li>
        ))}
      </ul>
      <p className="onboarding-community">
        <button
          type="button"
          className="onboarding-community-link"
          onClick={() => void clovyOpenCommunityPage().catch(() => undefined)}
        >
          <IconTelegram size={16} aria-hidden />
          <span>{t("onboarding.signIn.community")}</span>
        </button>
      </p>
      {account.configured ? (
        <div className="welcome-providers">
          {busy ? (
            <div
              className="welcome-auth-progress onboarding-waiting"
              role="status"
              aria-live="polite"
            >
              <span className="welcome-progress-label">
                <span>{t("onboarding.signIn.waiting")}</span>
              </span>
              <button
                type="button"
                className="welcome-cancel-btn"
                onClick={() => void cancelInFlight()}
              >
                {t("common.cancel")}
              </button>
            </div>
          ) : (
            <OnboardingPrimaryButton onClick={() => void handleSignIn()}>
              <OsMark />
              <span>{t("onboarding.signIn.continue")}</span>
            </OnboardingPrimaryButton>
          )}
        </div>
      ) : (
        <p className="welcome-status welcome-status-info">{t("onboarding.signIn.notConfigured")}</p>
      )}
      {status ? <p className="welcome-status">{status}</p> : null}
      <p className="welcome-terms">
        {tr("onboarding.signIn.terms", {
          terms: (chunks) => (
            <a href="https://accounts.opensoftware.co/terms" target="_blank" rel="noreferrer">
              {chunks}
            </a>
          ),
          privacy: (chunks) => (
            <a href="https://accounts.opensoftware.co/privacy" target="_blank" rel="noreferrer">
              {chunks}
            </a>
          ),
        })}
      </p>
      <InterfaceLanguageChooser />
    </StepCard>
  );
}

/**
 * A quiet interface-language switch on the welcome screen, so a new user can
 * read the rest of onboarding in their language before anything else. Same
 * store as Settings > Interface language; each option is named natively.
 */
function InterfaceLanguageChooser() {
  const t = useT();
  const locale = useLocale();
  const current = INTERFACE_LOCALE_OPTIONS.find((option) => option.value === locale);
  return (
    <div className="onboarding-language">
      <Select
        className="onboarding-language-select"
        value={locale}
        options={INTERFACE_LOCALE_OPTIONS.map((option) => ({
          value: option.value,
          label: option.label,
          lang: option.value,
        }))}
        placeholder="English"
        ariaLabel={t("onboarding.language.aria", { language: current?.label ?? "English" })}
        onChange={(value) => setInterfaceLocale(value as InterfaceLocale)}
      />
    </div>
  );
}

function messageFromError(error: unknown) {
  if (error && typeof error === "object" && "message" in error) {
    return String((error as { message: unknown }).message);
  }
  return String(error);
}
