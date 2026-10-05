import { IconBubble3 } from "central-icons/IconBubble3";
import { IconCalendarClock } from "central-icons/IconCalendarClock";
import { IconHomeOpen } from "central-icons/IconHomeOpen";
import { IconMicrophone } from "central-icons/IconMicrophone";
import { IconNoteText } from "central-icons/IconNoteText";
import { IconProjects } from "central-icons/IconProjects";
import { IconSettingsGear4 } from "central-icons/IconSettingsGear4";
import { IconZap } from "central-icons/IconZap";
import type { ReactNode } from "react";
import { t } from "../i18n";
import type { FolderDto, NoteDto, NoteListItemDto } from "../lib/tauri";
import type { AgentSessionDto } from "../lib/agent-runtime-contract";
import { navEquals, type TabNav } from "./tabs/tabs";
// "Clovy is up to date." is a confirmation, not a call to action: linger, then
// hide on its own. Failures persist until dismissed; busy statuses advance
// when their operation resolves and may also be dismissed while in flight.
export const UP_TO_DATE_DISMISS_MS = 4000;
// Soft-exit window: the update-popover-out animation runs var(--t-med) (160ms);
// the status clears just after it finishes.
export const UP_TO_DATE_EXIT_MS = 220;

export const SIDEBAR_DEFAULT_WIDTH = 240;
export const SIDEBAR_MIN_WIDTH = 188;
export const SIDEBAR_MAX_WIDTH = 320;
export const SIDEBAR_COLLAPSE_WIDTH = 160;
export const CHECK_FOR_UPDATES_EVENT = "clovy://check-for-updates";
export const AGENT_MENU_BAR_SESSION_FETCH_LIMIT = 100;
export const AGENT_MENU_BAR_SESSION_LIMIT = 6;
export const AGENT_MENU_BAR_SESSION_RETRY_DELAYS_MS = [250, 500, 1000, 2000, 4000, 8000];
// Matches the Routines view's run-history cadence; a routine notification a
// few seconds late is fine, hammering the bridge is not.
export const ROUTINE_RUN_NOTIFY_POLL_MS = 15000;
export const ACCESSIBILITY_PERMISSION_REFRESH_INTERVAL_MS = 1000;
export const SYSTEM_AUDIO_PERMISSION_REFRESH_INTERVAL_MS = 1000;
export const SYSTEM_AUDIO_PERMISSION_REFRESH_TIMEOUT_MS = 120_000;
export const MEETING_START_LISTENER_RETRY_DELAYS_MS = [250, 1_000, 5_000] as const;
// Funding and start-expiry copy is read at call time so it follows the
// interface language.
export const meetingStartRequestExpiredMessage = () => t("app.recording.startExpired");
export const composerFundingDisabledReason = () => t("app.funding.composer");
export const recordingFundingDisabledReason = () => t("app.funding.recording");
export const noteRetryFundingDisabledReason = () => t("app.funding.noteRetry");
export const recoveryFundingDisabledReason = () => t("app.funding.recovery");
export const routineFundingDisabledReason = () => t("app.funding.routine");
// Floor for the note card so the sidebar can't be dragged wide enough to
// crush it into a sliver — it always keeps a usable width plus its gutters.
export const MAIN_PANEL_MIN_WIDTH = 420;

export function noteHasDownloadableAudio(note: NoteDto): boolean {
  const audioSources = note.audioSources?.length
    ? note.audioSources
    : note.audio
      ? [note.audio]
      : [];
  return audioSources.some((audio) => audio.format === "wav" && audio.sizeBytes > 0);
}

// Largest the sidebar may grow given the live window width: never past its own
// cap, and never so far that the main panel drops below its floor. Falls back
// to the sidebar min on very narrow windows where both can't be satisfied.
export function sidebarMaxWidth() {
  return Math.max(
    SIDEBAR_MIN_WIDTH,
    Math.min(SIDEBAR_MAX_WIDTH, window.innerWidth - MAIN_PANEL_MIN_WIDTH),
  );
}

export const TAB_ICON_SIZE = 14;

export type RecordingInactivityPrompt = {
  sessionId: string;
  expiresAt: number;
};

export type AgentRecorderRequestPayload = {
  requestId?: unknown;
  action?: unknown;
  sourceMode?: unknown;
};

export function agentSessionTabTitle(session?: AgentSessionDto): string | undefined {
  return session?.title.trim() || undefined;
}

export function refreshedTabNav(current: TabNav, live: TabNav): TabNav | undefined {
  if (!navEquals(current, live)) return live;
  if (current.view !== "agent" || live.view !== "agent") return undefined;

  const liveTitle = live.agentSessionTitle?.trim();
  if (!liveTitle || current.agentSessionTitle?.trim() === liveTitle) {
    return undefined;
  }

  return { ...current, agentSessionTitle: liveTitle };
}

// The icon + label a tab shows for a snapshot. Titles for entity views (note,
// project, agent session) are looked up live from the loaded data, so a tab's
// label tracks renames. Agent tabs also carry a fallback title so a newly
// created session is identifiable before the session list hydrates.
export function tabMeta(
  nav: TabNav,
  notes: NoteListItemDto[],
  folders: FolderDto[],
  sessions: AgentSessionDto[],
  settingsSectionLabel?: string,
): { title: string; icon: ReactNode } {
  switch (nav.view) {
    case "home":
      return {
        title: t("app.nav.home"),
        icon: <IconHomeOpen size={TAB_ICON_SIZE} />,
      };
    case "meetings": {
      const note = nav.noteId ? notes.find((n) => n.id === nav.noteId) : undefined;
      return {
        title: note?.title?.trim() || t("app.newNote"),
        icon: <IconNoteText size={TAB_ICON_SIZE} />,
      };
    }
    case "folders": {
      const folder = nav.folderId ? folders.find((f) => f.id === nav.folderId) : undefined;
      return {
        title: folder?.name?.trim() || t("app.nav.projects"),
        icon: <IconProjects size={TAB_ICON_SIZE} />,
      };
    }
    case "agent": {
      const session = nav.agentSessionId
        ? sessions.find((s) => s.id === nav.agentSessionId)
        : undefined;
      return {
        title:
          agentSessionTabTitle(session) || nav.agentSessionTitle?.trim() || t("app.newSession"),
        icon: <IconBubble3 size={TAB_ICON_SIZE} />,
      };
    }
    case "agent-sessions":
      return {
        title: t("app.nav.sessions"),
        icon: <IconBubble3 size={TAB_ICON_SIZE} />,
      };
    case "all-notes":
      return {
        title: t("app.nav.allNotes"),
        icon: <IconNoteText size={TAB_ICON_SIZE} />,
      };
    case "routines":
      return {
        title: t("app.nav.routines"),
        icon: <IconZap size={TAB_ICON_SIZE} />,
      };
    case "dictation":
      return {
        title: t("app.nav.dictation"),
        icon: <IconMicrophone size={TAB_ICON_SIZE} />,
      };
    case "today":
      return {
        title: t("app.nav.today"),
        icon: <IconCalendarClock size={TAB_ICON_SIZE} />,
      };
    case "settings":
      return {
        // Surface the active settings section (e.g. "MCP servers") in the tab
        // strip so the label says what you are looking at, not just "Settings".
        title: settingsSectionLabel?.trim() || t("common.settings"),
        icon: <IconSettingsGear4 size={TAB_ICON_SIZE} />,
      };
    default:
      return {
        title: t("app.nav.notes"),
        icon: <IconNoteText size={TAB_ICON_SIZE} />,
      };
  }
}
