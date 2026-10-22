import { AnimatePresence, motion } from "framer-motion";
import type { CSSProperties } from "react";
import { t } from "../i18n";
import { MoveNoteToFolderDialog } from "../components/folders/MoveNoteToFolderDialog";
import { MoveSessionToProjectDialog } from "../components/folders/MoveSessionToProjectDialog";
import { NoteChatPanel } from "../components/note-chat/NoteChatPanel";
import { ShareDialog } from "../components/share/ShareDialog";
import { buildNotePayload } from "../lib/share-payload";
import { GlobalRecorderPill } from "../components/recorder/GlobalRecorderPill";
import { PermissionBanner } from "../components/permissions/PermissionBanner";
import { Sidebar } from "../components/sidebar/Sidebar";
import { TabBar } from "../components/tabs/TabBar";
import { ConnectorApprovalsTray } from "../components/connectors/ConnectorApprovalsTray";
import { ComputerUseApprovalsTray } from "../components/agent/ComputerUseApprovalsTray";
import { BrowserApprovalsTray } from "../components/agent/BrowserApprovalsTray";
import { Dialog } from "../components/ui/Dialog";
import { isMacLikePlatform, isWindowsPlatform } from "../lib/platform";
import type { AgentSessionDto } from "../lib/agent-runtime-contract";
import type { NoteListItemDto } from "../lib/tauri";
import { ConfirmDialog } from "../components/ui/ConfirmDialog";
import { handleSidebarResizeStart } from "./sidebar-resize";
import { SidebarToggleGlyph, handleTitlebarPointerDown } from "./app-helpers";
import {
  SIDEBAR_COLLAPSE_WIDTH,
  SIDEBAR_DEFAULT_WIDTH,
  SIDEBAR_MIN_WIDTH,
  sidebarMaxWidth,
} from "./app-shell";
import { UpdateHub } from "./app-effects/update-ui";
import type { RenderAppLayoutDependencies } from "./app-layout-types";

export function renderAppLayout(dependencies: RenderAppLayoutDependencies) {
  const {
    updateHub,
    accessibilityBannerDismissed,
    accessibilityBlocked,
    activateTab,
    activeTabId,
    activeView,
    agentSessions,
    agentSessionsListRef,
    captureActive,
    changeSettingsTab,
    closeOtherTabs,
    closeTab,
    completedSessions,
    confirmDeleteNote,
    detailScrollerActive,
    dispatch,
    error,
    handleCreateFolder,
    handleDeleteNote,
    handleEnableAccessibility,
    handleKeepRecordingAfterInactivityPrompt,
    handleOpenNoteChatInAgent,
    handleOpenRecordingNote,
    handlePauseRecordingAfterInactivityPrompt,
    handleRemoveNoteFromFolder,
    handleRemoveSessionFromFolder,
    handleRenameAgentSession,
    handleReorderTabs,
    handleSelectNote,
    handleSetNoteFolder,
    handleSetSessionFolder,
    handleToggleSessionCompleted,
    homeStoredSessionId,
    mainPanelBodyRef,
    moveDialogNoteIds,
    moveDialogSessionIds,
    noteChat,
    noteChatOpen,
    noteDetailScrollerActive,
    notesListRef,
    openNewChatTab,
    openSettings,
    openTab,
    pendingSessionProjectRef,
    pillIsDemo,
    pillStatus,
    recordingInactivityPrompt,
    recordingInactivitySecondsRemaining,
    recordingNoteTitle,
    selectedNote,
    sessionFolders,
    setAccessibilityBannerDismissed,
    setActiveAgentSession,
    setActiveView,
    setAgentOrigin,
    setConfirmDeleteNote,
    setFolderReturnTarget,
    setMoveDialogNoteIds,
    setMoveDialogSessionIds,
    setNoteChatOpen,
    setNoteShareUrl,
    setOriginAllNotes,
    setOriginFolderId,
    setShareNoteOpen,
    setSidebarCollapsed,
    setSidebarResizing,
    setSidebarTransition,
    setSidebarWidth,
    settingsDetailScrollerActive,
    settingsReturnView,
    settingsTab,
    shareNoteOpen,
    sidebarCollapsed,
    sidebarRecorderStatus,
    sidebarResizing,
    sidebarTransition,
    sidebarWidth,
    state,
    tabItems,
    takeNewTabIntent,
    workspaceContent,
  } = dependencies;
  const noteChatVisible = activeView === "meetings" && selectedNote !== undefined && noteChatOpen;

  return (
    <main
      className={[
        "app-shell",
        activeView !== "settings" ? "app-shell-sidebar-default" : "",
        noteChatVisible ? "app-shell-note-chat-open" : "",
      ]
        .filter(Boolean)
        .join(" ")}
      data-platform={isWindowsPlatform() ? "windows" : undefined}
      data-sidebar={sidebarCollapsed ? "collapsed" : "expanded"}
      data-sidebar-resizing={sidebarResizing ? "true" : "false"}
      data-sidebar-transition={sidebarTransition}
      style={
        {
          // The grid columns read this directly, so collapsed must pin it to 0
          // (the stored width is preserved for the next expand). During a drag
          // the resize logic overrides it imperatively.
          "--sidebar-w-current": `${sidebarCollapsed ? 0 : sidebarWidth}px`,
        } as CSSProperties
      }
    >
      <div
        className="titlebar-drag"
        aria-hidden
        data-tauri-drag-region
        onPointerDown={handleTitlebarPointerDown}
      />
      <button
        type="button"
        className="chrome-sidebar-toggle"
        aria-label={sidebarCollapsed ? t("app.sidebar.show") : t("app.sidebar.hide")}
        aria-pressed={sidebarCollapsed}
        onClick={() => {
          setSidebarTransition("none");
          if (sidebarCollapsed) {
            setSidebarWidth((width) => Math.max(width, SIDEBAR_DEFAULT_WIDTH));
            setSidebarCollapsed(false);
            return;
          }
          setSidebarCollapsed(true);
        }}
      >
        <SidebarToggleGlyph />
      </button>
      <Sidebar
        notes={state.notes}
        activeView={activeView}
        homeEnabled={isMacLikePlatform()}
        homeStoredSessionId={homeStoredSessionId}
        settingsTab={settingsTab}
        onSettingsTabChange={changeSettingsTab}
        onChangeView={(view) => {
          if (takeNewTabIntent()) {
            openTab({ view });
            return;
          }
          if (view === "settings") openSettings();
          else setActiveView(view);
          setAgentOrigin(undefined);
          if (view !== "agent") {
            setActiveAgentSession(undefined);
            pendingSessionProjectRef.current = null;
          }
          if (view === "folders") {
            setFolderReturnTarget(undefined);
            dispatch({ type: "folderSelected", folderId: undefined });
          }
          if (view !== "meetings" && view !== "notes") {
            setOriginFolderId(undefined);
            setOriginAllNotes(false);
            setFolderReturnTarget(undefined);
          }
        }}
        onExitSettings={() => setActiveView(settingsReturnView)}
        onSelectNote={(noteId) => {
          if (takeNewTabIntent()) {
            openTab({ view: "meetings", noteId });
            return;
          }
          void handleSelectNote(noteId);
        }}
        onNewAgentSession={() => {
          pendingSessionProjectRef.current = null;
          setAgentOrigin(undefined);
          setActiveAgentSession(undefined);
          setActiveView("agent");
        }}
        onRenameAgentSession={handleRenameAgentSession}
        onSelectAgentSession={(session) => {
          if (takeNewTabIntent()) {
            openTab({ view: "agent", agentSessionId: session.id });
            return;
          }
          setAgentOrigin(undefined);
          setActiveAgentSession(session);
          setActiveView("agent");
        }}
        sessionFolderIds={sessionFolders}
        completedSessionIds={completedSessions}
        onToggleSessionCompleted={handleToggleSessionCompleted}
        onOpenSessionMoveDialog={(sessionId) => setMoveDialogSessionIds([sessionId])}
        onRemoveSessionFromFolder={(sessionId, folderId) =>
          void handleRemoveSessionFromFolder(sessionId, folderId)
        }
        recordingStatus={sidebarRecorderStatus}
        recordingTitle={recordingNoteTitle}
        onOpenRecording={() => (pillIsDemo ? undefined : void handleOpenRecordingNote())}
        collapsed={sidebarCollapsed}
        footerAccessory={<UpdateHub {...updateHub} />}
      />
      <div
        className="sidebar-resize-handle"
        role="separator"
        aria-orientation="vertical"
        aria-label={t("app.sidebar.resize")}
        onPointerDown={(event) =>
          handleSidebarResizeStart(event, sidebarWidth, {
            collapseWidth: SIDEBAR_COLLAPSE_WIDTH,
            minWidth: SIDEBAR_MIN_WIDTH,
            maxWidth: sidebarMaxWidth,
            onStart: () => {
              setSidebarResizing(true);
              setSidebarTransition("none");
            },
            onEnd: (finalWidth) => {
              if (finalWidth <= SIDEBAR_COLLAPSE_WIDTH) {
                setSidebarResizing(false);
                setSidebarTransition("smooth");
                setSidebarWidth(Math.max(0, finalWidth));
                setSidebarCollapsed(true);
                return;
              }
              const nextWidth = Math.min(
                sidebarMaxWidth(),
                Math.max(SIDEBAR_MIN_WIDTH, finalWidth),
              );
              setSidebarResizing(false);
              setSidebarCollapsed(false);
              setSidebarWidth(nextWidth);
            },
          })
        }
      />
      <div className="main-column">
        <TabBar
          tabs={tabItems}
          activeTabId={activeTabId}
          onActivate={activateTab}
          onClose={closeTab}
          onCloseOthers={closeOtherTabs}
          onNew={openNewChatTab}
          onReorder={handleReorderTabs}
          layoutFrozen={sidebarResizing}
          onDragRegionPointerDown={handleTitlebarPointerDown}
        />
        <section
          className={`main-panel${
            activeView === "agent" || activeView === "home" ? " main-panel-agent-view" : ""
          }`}
        >
          {accessibilityBlocked && !accessibilityBannerDismissed ? (
            <PermissionBanner
              onDismiss={() => setAccessibilityBannerDismissed(true)}
              onEnableAccessibility={handleEnableAccessibility}
            />
          ) : null}
          <div
            ref={mainPanelBodyRef}
            className="main-panel-body"
            data-active-view={activeView}
            data-detail-scroller={detailScrollerActive ? "true" : undefined}
            data-note-detail-scroller={
              noteDetailScrollerActive || settingsDetailScrollerActive ? "true" : undefined
            }
          >
            {error ? <p className="error-banner">{error}</p> : null}
            <div className="workspace">{workspaceContent}</div>
          </div>
          <AnimatePresence>
            {pillStatus ? (
              <motion.div
                key="global-recorder"
                className="global-recorder-dock"
                initial={{ opacity: 0, y: -8 }}
                animate={{
                  opacity: 1,
                  y: 0,
                  transition: { duration: 0.22, ease: [0.22, 1, 0.36, 1] },
                }}
                exit={{
                  opacity: 0,
                  y: -8,
                  transition: { duration: 0.14, ease: [0.22, 1, 0.36, 1] },
                }}
              >
                <GlobalRecorderPill
                  status={pillStatus}
                  title={recordingNoteTitle}
                  onOpen={() => (pillIsDemo ? undefined : void handleOpenRecordingNote())}
                />
              </motion.div>
            ) : null}
          </AnimatePresence>
        </section>
        {noteChatVisible && selectedNote ? (
          <NoteChatPanel
            note={{ id: selectedNote.id, title: selectedNote.title }}
            chat={noteChat}
            recordingActive={captureActive}
            onClose={() => setNoteChatOpen(false)}
            onOpenInAgent={(sessionId) => {
              setNoteChatOpen(false);
              handleOpenNoteChatInAgent(
                { id: selectedNote.id, title: selectedNote.title },
                sessionId,
              );
            }}
          />
        ) : null}
        <ConfirmDialog
          open={confirmDeleteNote && !!selectedNote}
          onClose={() => setConfirmDeleteNote(false)}
          onConfirm={async () => {
            setConfirmDeleteNote(false);
            if (selectedNote) await handleDeleteNote(selectedNote.id);
          }}
          title={t("app.deleteNote.title")}
          description={t("app.deleteNote.description")}
          confirmLabel={t("app.deleteNote.confirm")}
          destructive
        />
        {selectedNote ? (
          <ShareDialog
            key={selectedNote.id}
            open={shareNoteOpen}
            onClose={() => setShareNoteOpen(false)}
            onLinkChange={setNoteShareUrl}
            item={{
              kind: "note",
              itemId: selectedNote.id,
              title: selectedNote.title,
              // Notes share the rendered markdown: edited content when
              // present, generated otherwise. Snapshot at share time.
              buildPayload: () =>
                buildNotePayload({
                  title: selectedNote.title,
                  markdown: selectedNote.editedContent ?? selectedNote.generatedContent ?? "",
                }),
            }}
          />
        ) : null}
      </div>
      <Dialog
        open={recordingInactivityPrompt !== null}
        onClose={handleKeepRecordingAfterInactivityPrompt}
        title={t("app.inactivity.title")}
        description={t("app.inactivity.description")}
        width={420}
        footer={
          <>
            <button
              type="button"
              className="primary-action"
              onClick={handlePauseRecordingAfterInactivityPrompt}
            >
              {t("app.inactivity.pause")}
            </button>
            <button
              type="button"
              className="primary-action primary-solid"
              onClick={handleKeepRecordingAfterInactivityPrompt}
            >
              {t("app.inactivity.keep")}
            </button>
          </>
        }
      >
        <div className="dialog-body">
          <p className="recording-inactivity-copy">
            {t("app.inactivity.countdown", { count: recordingInactivitySecondsRemaining })}
          </p>
        </div>
      </Dialog>
      <MoveNoteToFolderDialog
        open={moveDialogNoteIds !== null}
        onClose={() => setMoveDialogNoteIds(null)}
        notes={
          moveDialogNoteIds
            ? moveDialogNoteIds
                .map((id) => state.notes.find((n) => n.id === id))
                .filter((note): note is NoteListItemDto => note !== undefined)
            : []
        }
        folders={state.folders}
        onSetFolder={(noteId, folderId) => handleSetNoteFolder(noteId, folderId, { rethrow: true })}
        onCreateFolder={(name) => handleCreateFolder(name)}
        onRemoveFolder={(noteId, folderId) =>
          handleRemoveNoteFromFolder(noteId, folderId, { rethrow: true })
        }
        onMoved={() => notesListRef.current?.resetSelection()}
      />
      <MoveSessionToProjectDialog
        open={moveDialogSessionIds !== null}
        onClose={() => setMoveDialogSessionIds(null)}
        sessions={
          moveDialogSessionIds
            ? moveDialogSessionIds
                .map((id) => agentSessions.find((s) => s.id === id))
                .filter((session): session is AgentSessionDto => session !== undefined)
            : []
        }
        sessionFolderIds={sessionFolders}
        folders={state.folders}
        onSetFolder={(sessionId, folderId) =>
          handleSetSessionFolder(sessionId, folderId, { rethrow: true })
        }
        onCreateFolder={(name) => handleCreateFolder(name)}
        onRemoveFolder={(sessionId, folderId) =>
          handleRemoveSessionFromFolder(sessionId, folderId, { rethrow: true })
        }
        onMoved={() => agentSessionsListRef.current?.resetSelection()}
      />
      {/* Connector action approvals (approval trust mode) can arrive from a
            routine or chat in any view, so the tray is mounted at the shell. */}
      <div className="shell-approvals-stack">
        <BrowserApprovalsTray />
        <ComputerUseApprovalsTray />
        <ConnectorApprovalsTray />
      </div>
    </main>
  );
}
