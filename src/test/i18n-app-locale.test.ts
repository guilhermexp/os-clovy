import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { formatTime } from "../components/activity-timeline/timeline-utils";
import { applyInterfaceLocale } from "../i18n/locale";
import { formatNumber, intlLocale } from "../i18n/translate";
import { clovyHomeDayLabel } from "../lib/clovy-home";
import { formatCalendarDate, formatListTimestamp } from "../lib/date-format";
import { messageForErrorCode, messageFromError } from "../lib/errors";
import { humanizeSchedule } from "../lib/routine-schedule";

// Every date, time, and named number follows the interface language, never
// the system's (ADR-0061). Each case runs with a system locale that differs
// from the interface language: `__clovyTestSystemLocale` is what a formatter
// given no locale falls back to (see setup.ts), standing in for macOS's
// regional format.
function withSystemLocale(locale: string) {
  globalThis.__clovyTestSystemLocale = locale;
}

/** ICU writes "8:00 AM" with a narrow no-break space. */
function plain(text: string) {
  return text.replace(/[  ]/g, " ");
}

// Thursday, October 15, 2026; three days after Monday, October 12.
const NOW = new Date(2026, 9, 15, 10, 0);
const MONDAY_8AM = new Date(2026, 9, 12, 8, 0).toISOString();

describe("interface language owns every date and time", () => {
  beforeEach(() => applyInterfaceLocale("en"));
  afterEach(() => {
    globalThis.__clovyTestSystemLocale = undefined;
    applyInterfaceLocale("en");
  });

  it("reads Monday at 8:00 AM in English on a Portuguese system", () => {
    withSystemLocale("pt-BR");
    applyInterfaceLocale("en");
    expect(plain(clovyHomeDayLabel(MONDAY_8AM, NOW))).toBe("Monday at 8:00 AM");
    expect(intlLocale()).toBe("en");
    expect(formatListTimestamp(new Date(2026, 9, 12, 8), NOW)).toBe("Mon");
    expect(plain(formatTime(new Date(2026, 9, 15, 9, 30).toISOString()))).toBe("9:30 AM");
    expect(humanizeSchedule("0 8 * * 1").replace(/ /g, " ")).toBe("Every Monday at 8:00 AM");
    expect(formatNumber(1234.5)).toBe("1,234.5");
  });

  it("reads segunda-feira às 08:00 in Portuguese on an English system", () => {
    withSystemLocale("en-US");
    applyInterfaceLocale("pt-BR");
    expect(formatTime(new Date(2026, 9, 15, 9, 30).toISOString())).toBe("9:30");
    expect(clovyHomeDayLabel(MONDAY_8AM, NOW)).toBe("Segunda-feira às 08:00");
    expect(formatListTimestamp(new Date(2026, 9, 12, 8), NOW)).toBe("seg.");
    expect(humanizeSchedule("0 8 * * 1")).toBe("Toda segunda-feira às 08:00");
  });

  it("takes month names from the interface language and day/month order from the preference", () => {
    const date = new Date(2026, 9, 12, 12);
    withSystemLocale("pt-BR");
    applyInterfaceLocale("en");
    // `system` follows the system's order (day first in pt-BR) with English names.
    expect(formatCalendarDate(date, "system")).toBe("12 Oct");
    expect(formatCalendarDate(date, "month-first")).toBe("Oct 12");

    withSystemLocale("en-US");
    expect(formatCalendarDate(date, "system")).toBe("Oct 12");

    withSystemLocale("pt-BR");
    applyInterfaceLocale("pt-BR");
    // Same order as the language's own pattern: keep its connector.
    expect(formatCalendarDate(date, "system")).toBe("12 de out.");
    expect(formatCalendarDate(date, "day-first")).toBe("12 out.");
  });

  it("phrases stored intervals in the interface language", () => {
    withSystemLocale("en-US");
    applyInterfaceLocale("pt-BR");
    expect(humanizeSchedule("every 30m")).toBe("A cada 30 minutos");
    applyInterfaceLocale("en");
    expect(humanizeSchedule("every 30m")).toBe("Every 30 minutes");
  });
});

describe("native error codes", () => {
  afterEach(() => applyInterfaceLocale("en"));

  it("words a known code in the interface language and falls back to the native message", () => {
    applyInterfaceLocale("pt-BR");
    expect(messageForErrorCode("accessibility_permission_missing", "English text")).toBe(
      "O Clovy não conseguiu colar automaticamente. Sua transcrição está na área de transferência, então você pode colar com Cmd+V.",
    );
    expect(
      messageForErrorCode("shortcut_register_failed", "Could not register the shortcut F5.", {
        shortcut: "F5",
      }),
    ).toBe("Não foi possível registrar o atalho F5.");
    expect(messageForErrorCode("some_new_code", "Native text.")).toBe("Native text.");
    expect(messageForErrorCode(undefined, "Native text.")).toBe("Native text.");
    expect(
      messageFromError({
        code: "home_chat_failed",
        message: "Clovy couldn't reply just now. Your message is still here, so you can try again.",
      }),
    ).toBe(
      "O Clovy não conseguiu responder agora. Sua mensagem continua aqui, então você pode tentar de novo.",
    );
  });

  it("keeps English byte-identical to the native wording", () => {
    applyInterfaceLocale("en");
    expect(
      messageForErrorCode(
        "paste_target_unavailable",
        "Clovy couldn't paste automatically. Your transcript is on the clipboard, so you can paste it with Cmd+V.",
      ),
    ).toBe(
      "Clovy couldn't paste automatically. Your transcript is on the clipboard, so you can paste it with Cmd+V.",
    );
    expect(messageFromError({ code: "not_signed_in", message: "Sign in to use dictation." })).toBe(
      "Sign in to use dictation.",
    );
  });
});
