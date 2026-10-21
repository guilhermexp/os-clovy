import type { AgentSessionsListHandle } from "../components/agent/AgentSessionsList";
import type { NotesListHandle } from "../components/notes-list/NotesList";
import type { SettingsTab } from "../components/settings/AppSettings";
import type { SidebarView } from "../components/sidebar/Sidebar";
import type { TabItem } from "../components/tabs/TabBar";
import type { TabNav } from "./tabs/tabs";
import type { FolderDto, NoteDto, RecordingStatusDto } from "../lib/tauri";
import type { AgentSessionDto } from "../lib/agent-runtime-contract";
import type { NoteChat } from "../components/note-chat/useNoteChat";
import type { ClovyUpdate } from "../lib/updater";
import type {
  UpdateInstallProgress,
  UpdatePromptPayload,
  UpdateStatusDisplayState,
} from "./update-decision";
import type { RecordingInactivityPrompt } from "./app-shell";
import type { NotesAction, NotesState } from "./state/app-state";
import type * as React from "react";

export type RenderAppLayoutDependencies = {
  accessibilityBannerDismissed: boolean;
  accessibilityBlocked: boolean;
  activateTab: (id: string) => void;
  activeTabId: string;
  activeView: SidebarView;
  agentSessions: AgentSessionDto[];
  agentSessionsListRef: React.MutableRefObject<AgentSessionsListHandle | null>;
  captureActive: boolean;
  changeSettingsTab: (tab: SettingsTab) => void;
  checkingUpdate: boolean;
  closeOtherTabs: (id: string) => void;
  closeTab: (id: string) => void;
  completedSessions: Record<string, string>;
  confirmDeleteNote: boolean;
  detailScrollerActive: boolean;
  dispatch: React.Dispatch<NotesAction>;
  error: string | null;
  handleCreateFolder: (name: string, description?: string) => Promise<FolderDto | undefined>;
  handleDeleteNote: (noteId: string) => Promise<void>;
  handleEnableAccessibility: () => void;
  handleKeepRecordingAfterInactivityPrompt: () => void;
  handleOpenNoteChatInAgent: (noteRef: { id: string; title: string }, sessionId?: string) => void;
  handleOpenRecordingNote: () => Promise<void>;
  handlePauseRecordingAfterInactivityPrompt: () => void;
  handleRelaunchUpdate: () => void;
  handleRemoveNoteFromFolder: (
    noteId: string,
    folderId: string,
    options?: { rethrow?: boolean },
  ) => Promise<void>;
  handleRemoveSessionFromFolder: (
    sessionId: string,
    folderId: string,
    options?: { rethrow?: boolean },
  ) => Promise<void>;
  handleRenameAgentSession: (sessionId: string, title: string) => void;
  handleReorderTabs: (orderedVisibleIds: string[]) => void;
  handleSelectNote: (noteId: string) => Promise<void>;
  handleSetNoteFolder: (
    noteId: string,
    folderId: string,
    options?: { rethrow?: boolean },
  ) => Promise<void>;
  handleSetSessionFolder: (
    sessionId: string,
    folderId: string,
    options?: { rethrow?: boolean },
  ) => Promise<void>;
  handleToggleSessionCompleted: (sessionId: string, completed: boolean) => Promise<void>;
  homeStoredSessionId: string | undefined;
  mainPanelBodyRef: React.MutableRefObject<HTMLDivElement | null>;
  moveDialogNoteIds: string[] | null;
  moveDialogSessionIds: string[] | null;
  noteChat: NoteChat;
  noteChatOpen: boolean;
  noteDetailScrollerActive: boolean;
  notesListRef: React.MutableRefObject<NotesListHandle | null>;
  openNewChatTab: () => void;
  openSettings: () => void;
  openTab: (nav: TabNav) => void;
  pendingSessionProjectRef: React.MutableRefObject<{
    folderId: string;
    knownSessionIds: Set<string>;
    partition: string;
  } | null>;
  pillIsDemo: boolean;
  pillStatus: RecordingStatusDto | null;
  preparingUpdate: boolean;
  readyUpdate: UpdatePromptPayload<ClovyUpdate> | null;
  recordingInactivityPrompt: RecordingInactivityPrompt | null;
  recordingInactivitySecondsRemaining: number;
  recordingNoteTitle: string;
  relaunchingUpdate: boolean;
  selectedNote: NoteDto | undefined;
  sessionFolders: Record<string, string[]>;
  setAccessibilityBannerDismissed: React.Dispatch<React.SetStateAction<boolean>>;
  setActiveAgentSession: (session: AgentSessionDto | undefined) => void;
  setActiveView: React.Dispatch<React.SetStateAction<SidebarView>>;
  setAgentOrigin: React.Dispatch<
    React.SetStateAction<{ kind: "project"; folderId: string } | { kind: "routines" } | undefined>
  >;
  setConfirmDeleteNote: React.Dispatch<React.SetStateAction<boolean>>;
  setError: React.Dispatch<React.SetStateAction<string | null>>;
  setFolderReturnTarget: React.Dispatch<
    React.SetStateAction<{ noteId: string; label: string } | undefined>
  >;
  setMoveDialogNoteIds: React.Dispatch<React.SetStateAction<string[] | null>>;
  setMoveDialogSessionIds: React.Dispatch<React.SetStateAction<string[] | null>>;
  setNoteChatOpen: React.Dispatch<React.SetStateAction<boolean>>;
  setNoteShareUrl: React.Dispatch<React.SetStateAction<string | null>>;
  setOriginAllNotes: React.Dispatch<React.SetStateAction<boolean>>;
  setOriginFolderId: React.Dispatch<React.SetStateAction<string | undefined>>;
  setShareNoteOpen: React.Dispatch<React.SetStateAction<boolean>>;
  setSidebarCollapsed: React.Dispatch<React.SetStateAction<boolean>>;
  setSidebarResizing: React.Dispatch<React.SetStateAction<boolean>>;
  setSidebarTransition: React.Dispatch<React.SetStateAction<"none" | "smooth">>;
  setSidebarWidth: React.Dispatch<React.SetStateAction<number>>;
  setUpdateProgress: React.Dispatch<React.SetStateAction<UpdateInstallProgress | null>>;
  setUpdateStatus: (status: string | null, failed?: boolean) => void;
  settingsDetailScrollerActive: boolean;
  settingsReturnView: SidebarView;
  settingsTab: SettingsTab;
  shareNoteOpen: boolean;
  sidebarCollapsed: boolean;
  sidebarRecorderStatus: RecordingStatusDto | null;
  sidebarResizing: boolean;
  sidebarTransition: "none" | "smooth";
  sidebarWidth: number;
  state: NotesState;
  tabItems: TabItem[];
  takeNewTabIntent: () => boolean;
  updateProgress: UpdateInstallProgress | null;
  updateProgressHiddenRef: React.MutableRefObject<boolean>;
  updateStatus: string | null;
  updateStatusDisplay: UpdateStatusDisplayState;
  updateStatusLeaving: boolean;
  workspaceContent: JSX.Element;
};
