import { t } from "../i18n";
import { FundingNotice, fundingTierOf } from "../components/account/FundingNotice";
import { markAgentNewSessionPending } from "../components/agent/session-persistence";
import { AgentSessionsList } from "../components/agent/AgentSessionsList";
import { DictationHistoryView } from "../components/dictation/DictationHistoryView";
import { ShareLinkCopyAction } from "../components/share/ShareLinkCopyAction";
import { NotesList } from "../components/notes-list/NotesList";
import { BreadcrumbBar } from "../components/ui/BreadcrumbBar";
import { IconProjects } from "central-icons/IconProjects";
import { agentRuntimeBindings, retryProcessing } from "../lib/tauri";
import { selectSessionProjectContext } from "../lib/agent-project-context";
import { messageFromError } from "../lib/errors";
import {
  composerFundingDisabledReason,
  noteRetryFundingDisabledReason,
  recoveryFundingDisabledReason,
  recordingFundingDisabledReason,
  routineFundingDisabledReason,
} from "./app-shell";
import type { RenderAppWorkspaceDependencies } from "./app-workspace-view-types";
import {
  ActivityTimelineViewRoute,
  AgentWorkspaceRoute,
  AppSettingsRoute,
  FoldersWorkspaceRoute,
  NoteEditorRoute,
  RoutinesViewRoute,
} from "./workspace-lazy";

export function renderAppWorkspace(dependencies: RenderAppWorkspaceDependencies) {
  const {
    accessibilityStatus,
    account,
    accountLoading,
    currentDataPartitionName,
    activeAgentSessionFolder,
    activeAgentSessionId,
    activeAgentSessionSeed,
    activeView,
    agentOrigin,
    agentOriginFolder,
    agentProjectContextFolder,
    agentSessions,
    agentSessionsListRef,
    agentWaitingSessionIds,
    agentWorkingSessionIds,
    changeSettingsTab,
    checkingSourceReadiness,
    completedSessions,
    dispatch,
    folderReturnTarget,
    fundingAccount,
    fundingRequired,
    handleAccountChanged,
    handleCreateFolder,
    handleCreateNote,
    handleDeleteFolder,
    handleDeleteNote,
    handleDeleteNotes,
    handleEnableAccessibility,
    handleEnableMicrophone,
    handleEnableSystemAudio,
    handleFinishRecording,
    handleKeepRecordingAfterMeetingEnd,
    handleFlushNote,
    handleFoldersImported,
    handleNewAgentSession,
    handleNewAgentSessionInProject,
    handleOpenSessionProject,
    handlePauseRecording,
    handleReconcileToStable,
    handleRecovery,
    handleRelaunchUpdate,
    handleRemoveNoteFromFolder,
    handleRemoveSessionFromFolder,
    handleRenameAgentSession,
    handleRenameFolder,
    handleReportIssue,
    handleResumeRecording,
    handleSaveNoteNow,
    handleReturnToAgentOriginFolder,
    handleReturnToAgentsList,
    handleReturnToNote,
    handleReturnToRoutines,
    handleSelectFolder,
    handleSelectNote,
    handleSelectNoteFromAllNotes,
    handleSelectNoteFromFolder,
    handleSetNoteFolder,
    handleSetSessionFolder,
    handleSourceModeChange,
    handleStartBundleChat,
    handleStartRecording,
    handleStopNowAfterMeetingEnd,
    handleToggleSessionCompleted,
    handleTopUp,
    handleUpdateNote,
    homeStoredSessionId,
    memoryFolderFilter,
    meetingEndCountdown,
    microphoneBlocked,
    microphoneStatus,
    noteDetailScrollRef,
    noteShareUrl,
    noteToolbarActions,
    notesListRef,
    openMemorySettings,
    openTab,
    originAllNotes,
    originFolder,
    readyUpdate,
    recordNoticesConsentPinned,
    recordingNoteId,
    refreshAccount,
    refreshFundingAccount,
    rememberHomeSession,
    runUpdateCheck,
    selectedNote,
    selectedNoteId,
    selectedNoteLiveTranscript,
    selectedRecovery,
    sessionFolders,
    setActiveAgentSession,
    setActiveView,
    setAgentOrigin,
    setError,
    setFolderReturnTarget,
    setMoveDialogNoteIds,
    setMoveDialogSessionIds,
    setOriginAllNotes,
    setOriginFolderId,
    setSettingsDetailPinned,
    setSettingsReturnView,
    setSettingsTab,
    settingsTab,
    sourceMode,
    sourceReadiness,
    state,
    takeNewTabIntent,
    topUpLabel,
  } = dependencies;

  const resolveAgentSessionProjectContext = (sessionId: string) => {
    const folder =
      sessionId === activeAgentSessionId
        ? activeAgentSessionFolder
        : selectSessionProjectContext(state.folders, sessionFolders[sessionId]);
    return folder
      ? {
          id: folder.id,
          name: folder.name,
          instructions: folder.instructions,
          localPath: folder.localPath,
        }
      : undefined;
  };

  return activeView === "settings" ? (
    <AppSettingsRoute
      account={account}
      accountLoading={accountLoading}
      sourceMode={sourceMode}
      sourceReadiness={sourceReadiness}
      checkingSourceReadiness={checkingSourceReadiness}
      microphonePermissionStatus={microphoneStatus}
      accessibilityPermissionStatus={accessibilityStatus}
      onAccountChanged={handleAccountChanged}
      onAccountRefresh={refreshAccount}
      onSourceModeChange={handleSourceModeChange}
      onEnableMicrophone={handleEnableMicrophone}
      onEnableAccessibility={handleEnableAccessibility}
      onEnableSystemAudio={handleEnableSystemAudio}
      folders={state.folders}
      onFoldersImported={handleFoldersImported}
      memoryFolderFilter={memoryFolderFilter}
      onOpenProject={(folderId) => {
        handleSelectFolder(folderId);
        setActiveView("folders");
      }}
      activeTab={settingsTab}
      onTabChange={changeSettingsTab}
      onDetailPinnedChange={setSettingsDetailPinned}
      onCheckForUpdates={() => runUpdateCheck("manual")}
      updateReadyToRelaunch={readyUpdate != null}
      onRelaunch={handleRelaunchUpdate}
      onReconcileToStable={handleReconcileToStable}
      onReportIssue={handleReportIssue}
      onStartBundleChat={handleStartBundleChat}
    />
  ) : activeView === "dictation" ? (
    <DictationHistoryView
      onNavigateToSettings={(target) => {
        setSettingsReturnView(activeView);
        setActiveView("settings");
        setSettingsTab("dictation");
        const headingId = target === "style" ? "style-heading" : "dictionary-heading";
        window.setTimeout(() => {
          document.getElementById(headingId)?.scrollIntoView({
            behavior: "smooth",
            block: "start",
          });
        }, 80);
      }}
    />
  ) : activeView === "today" ? (
    <ActivityTimelineViewRoute
      onNavigateToSettings={() => {
        setSettingsReturnView(activeView);
        setActiveView("settings");
        setSettingsTab("activity");
      }}
    />
  ) : activeView === "routines" ? (
    <RoutinesViewRoute
      creditActionsDisabledReason={fundingRequired ? routineFundingDisabledReason() : undefined}
      onCreateRoutine={(prompt) => {
        markAgentNewSessionPending(prompt);
        setActiveAgentSession(undefined);
        setActiveView("agent");
      }}
      onOpenRun={(session) => {
        if (takeNewTabIntent()) {
          openTab({
            view: "agent",
            agentSessionId: session.id,
            agentOrigin: { kind: "routines" },
          });
          return;
        }
        setAgentOrigin({ kind: "routines" });
        setActiveAgentSession(session);
        setActiveView("agent");
      }}
    />
  ) : activeView === "home" ? (
    <AgentWorkspaceRoute
      key={`home:${currentDataPartitionName}`}
      homeMode
      homeUserDisplayName={account.user?.displayName}
      initialSessionId={homeStoredSessionId}
      onHomeSessionCreated={rememberHomeSession}
      onOpenHomeTaskSession={(sessionId, title) => {
        void (async () => {
          try {
            const session =
              agentSessions.find((item) => item.id === sessionId) ??
              (await agentRuntimeBindings.getSession(sessionId));
            setAgentOrigin(undefined);
            setActiveAgentSession(session);
            setActiveView("agent");
          } catch (cause) {
            setError(messageFromError(cause) || t("app.workspace.couldNotOpen", { title }));
          }
        })();
      }}
      creditActionsDisabledReason={fundingRequired ? composerFundingDisabledReason() : undefined}
      renderFundingNotice={
        fundingRequired
          ? (textFundingContext) => (
              <FundingNotice
                account={fundingAccount}
                onRefresh={refreshFundingAccount}
                textFundingContext={textFundingContext}
              />
            )
          : undefined
      }
      fundingTier={fundingTierOf(fundingAccount)}
      topUpLabel={topUpLabel}
      onTopUp={handleTopUp}
      resolveSessionProjectContext={resolveAgentSessionProjectContext}
    />
  ) : activeView === "agent" ? (
    // The origin crumbs render inside the workspace's own sticky
    // session bar, so they persist while the chat scrolls beneath.
    <AgentWorkspaceRoute
      initialSession={activeAgentSessionSeed}
      initialSessionId={activeAgentSessionId}
      onSessionSelected={setActiveAgentSession}
      creditActionsDisabledReason={fundingRequired ? composerFundingDisabledReason() : undefined}
      renderFundingNotice={
        fundingRequired
          ? (textFundingContext) => (
              <FundingNotice
                account={fundingAccount}
                onRefresh={refreshFundingAccount}
                textFundingContext={textFundingContext}
              />
            )
          : undefined
      }
      fundingTier={fundingTierOf(fundingAccount)}
      topUpLabel={topUpLabel}
      onTopUp={handleTopUp}
      sessionInProject={Boolean(activeAgentSessionFolder)}
      resolveSessionProjectContext={resolveAgentSessionProjectContext}
      projectContext={
        agentProjectContextFolder
          ? {
              id: agentProjectContextFolder.id,
              name: agentProjectContextFolder.name,
              instructions: agentProjectContextFolder.instructions,
              localPath: agentProjectContextFolder.localPath,
            }
          : undefined
      }
      onMoveSessionToProject={(sessionId) => setMoveDialogSessionIds([sessionId])}
      origin={
        agentOriginFolder
          ? {
              backLabel: t("app.breadcrumb.backTo", { name: agentOriginFolder.name }),
              onBack: handleReturnToAgentOriginFolder,
              crumbs: [
                {
                  label: t("app.nav.projects"),
                  onClick: () => {
                    setActiveView("folders");
                    dispatch({
                      type: "folderSelected",
                      folderId: undefined,
                    });
                    setActiveAgentSession(undefined);
                    setAgentOrigin(undefined);
                  },
                },
                {
                  label: agentOriginFolder.name,
                  icon: <IconProjects size={13} />,
                  onClick: handleReturnToAgentOriginFolder,
                },
              ],
            }
          : agentOrigin?.kind === "routines"
            ? {
                backLabel: t("app.breadcrumb.backToRoutines"),
                onBack: handleReturnToRoutines,
                crumbs: [
                  {
                    label: t("app.nav.routines"),
                    onClick: handleReturnToRoutines,
                  },
                ],
              }
            : activeAgentSessionFolder
              ? // Opened from the Sessions view or sidebar but filed in a
                // project: the crumb shows the session's home (back still
                // returns to where the user came from).
                {
                  backLabel: t("app.breadcrumb.backToSessions"),
                  onBack: handleReturnToAgentsList,
                  crumbs: [
                    {
                      label: activeAgentSessionFolder.name,
                      icon: <IconProjects size={13} />,
                      onClick: () => handleOpenSessionProject(activeAgentSessionFolder.id),
                    },
                  ],
                }
              : {
                  backLabel: t("app.breadcrumb.backToSessions"),
                  onBack: handleReturnToAgentsList,
                  crumbs: [
                    {
                      label: t("app.nav.sessions"),
                      onClick: handleReturnToAgentsList,
                    },
                  ],
                }
      }
    />
  ) : activeView === "agent-sessions" ? (
    <AgentSessionsList
      ref={agentSessionsListRef}
      sessions={agentSessions}
      folders={state.folders}
      sessionFolderIds={sessionFolders}
      completedSessionIds={completedSessions}
      onToggleCompleted={handleToggleSessionCompleted}
      workingSessionIds={agentWorkingSessionIds}
      waitingSessionIds={agentWaitingSessionIds}
      onSelectSession={(session) => {
        if (takeNewTabIntent()) {
          openTab({ view: "agent", agentSessionId: session.id });
          return;
        }
        setAgentOrigin(undefined);
        setActiveAgentSession(session);
        setActiveView("agent");
      }}
      onNewSession={handleNewAgentSession}
      onRenameSession={handleRenameAgentSession}
      onOpenMoveDialog={(sessionId) => setMoveDialogSessionIds([sessionId])}
      onOpenMoveSessions={(sessionIds) => setMoveDialogSessionIds(sessionIds)}
      onRemoveFromProject={(sessionId, folderId) =>
        void handleRemoveSessionFromFolder(sessionId, folderId)
      }
    />
  ) : activeView === "notes" || activeView === "all-notes" ? (
    <NotesList
      ref={notesListRef}
      notes={state.notes}
      activeRecordingNoteId={recordingNoteId}
      onSelectNote={(noteId) => {
        if (takeNewTabIntent()) {
          openTab({
            view: "meetings",
            noteId,
            originAllNotes: true,
          });
          return;
        }
        void handleSelectNoteFromAllNotes(noteId);
      }}
      onCreateNote={() => void handleCreateNote(null)}
      onOpenMoveDialog={(noteId) => setMoveDialogNoteIds([noteId])}
      onOpenMoveNotes={(noteIds) => setMoveDialogNoteIds(noteIds)}
      onDeleteNote={(noteId) => void handleDeleteNote(noteId)}
      onDeleteNotes={(noteIds) => void handleDeleteNotes(noteIds)}
    />
  ) : activeView === "folders" ? (
    <FoldersWorkspaceRoute
      folders={state.folders}
      notes={state.notes}
      sessions={agentSessions}
      sessionFolderIds={sessionFolders}
      selectedFolderId={state.selectedFolderId}
      folderBackTarget={
        folderReturnTarget
          ? {
              label: t("app.breadcrumb.backTo", { name: folderReturnTarget.label }),
              onBack: () => void handleReturnToNote(folderReturnTarget.noteId),
            }
          : undefined
      }
      onSelectFolder={(folderId) => handleSelectFolder(folderId)}
      onCreateFolder={(name, description) => handleCreateFolder(name, description)}
      onFoldersImported={handleFoldersImported}
      onRenameFolder={(folderId, name, description) =>
        handleRenameFolder(folderId, name, description)
      }
      onFolderUpdated={(folder) => dispatch({ type: "folderUpdated", folder })}
      onDeleteFolder={(folderId) => handleDeleteFolder(folderId)}
      onCreateNote={(folderId) => void handleCreateNote(folderId)}
      onSelectNote={(noteId) => {
        const folderId = state.selectedFolderId;
        if (takeNewTabIntent()) {
          openTab({
            view: "meetings",
            noteId,
            originFolderId: folderId,
          });
          return;
        }
        if (folderId) {
          void handleSelectNoteFromFolder(noteId, folderId);
        } else {
          void handleSelectNote(noteId).then(() => setActiveView("meetings"));
        }
      }}
      onAssignNoteToFolder={(noteId, folderId) =>
        handleSetNoteFolder(noteId, folderId, { rethrow: true })
      }
      onRemoveNoteFromFolder={(noteId, folderId) =>
        void handleRemoveNoteFromFolder(noteId, folderId)
      }
      onOpenMoveDialog={(noteId) => setMoveDialogNoteIds([noteId])}
      onDeleteNote={(noteId) => void handleDeleteNote(noteId)}
      onCreateSession={(folderId) => handleNewAgentSessionInProject(folderId)}
      onSelectSession={(session) => {
        // Remember the project so the agent view can breadcrumb
        // back to it.
        const agentOriginValue = state.selectedFolderId
          ? ({
              kind: "project",
              folderId: state.selectedFolderId,
            } as const)
          : undefined;
        if (takeNewTabIntent()) {
          openTab({
            view: "agent",
            agentSessionId: session.id,
            agentOrigin: agentOriginValue,
          });
          return;
        }
        setAgentOrigin(agentOriginValue);
        setActiveAgentSession(session);
        setActiveView("agent");
      }}
      onAssignSessionToFolder={(sessionId, folderId) =>
        handleSetSessionFolder(sessionId, folderId, {
          rethrow: true,
        })
      }
      onRemoveSessionFromFolder={(sessionId, folderId) =>
        void handleRemoveSessionFromFolder(sessionId, folderId)
      }
      onOpenSessionMoveDialog={(sessionId) => setMoveDialogSessionIds([sessionId])}
      onManageProjectMemory={(folderId) => openMemorySettings(folderId)}
    />
  ) : selectedNote ? (
    <div className="note-shell">
      {/* Every note gets the toolbar so its content starts at a
                        consistent height (aligning with the Ask Clovy panel) and
                        the note actions live in one predictable spot. The left
                        shows breadcrumb nav when there's a parent, else a quiet
                        "Notes" root. */}
      {originFolder ? (
        <BreadcrumbBar
          backLabel={t("app.breadcrumb.backTo", { name: originFolder.name })}
          onBack={() => {
            setActiveView("folders");
            dispatch({
              type: "folderSelected",
              folderId: originFolder.id,
            });
            setOriginFolderId(undefined);
          }}
          items={[
            {
              label: originFolder.name,
              icon: <IconProjects size={13} />,
              onClick: () => {
                setActiveView("folders");
                dispatch({
                  type: "folderSelected",
                  folderId: originFolder.id,
                });
                setOriginFolderId(undefined);
              },
            },
            {
              label: selectedNote.title.trim() || t("app.newNote"),
              action: noteShareUrl ? <ShareLinkCopyAction url={noteShareUrl} /> : null,
            },
          ]}
          actions={noteToolbarActions}
        />
      ) : originAllNotes ? (
        <BreadcrumbBar
          backLabel={t("app.breadcrumb.backToMeetingNotes")}
          onBack={() => {
            setActiveView("all-notes");
            setOriginAllNotes(false);
          }}
          items={[
            {
              label: t("app.nav.meetingNotes"),
              onClick: () => {
                setActiveView("all-notes");
                setOriginAllNotes(false);
              },
            },
            {
              label: selectedNote.title.trim() || t("app.newNote"),
              action: noteShareUrl ? <ShareLinkCopyAction url={noteShareUrl} /> : null,
            },
          ]}
          actions={noteToolbarActions}
        />
      ) : (
        <BreadcrumbBar
          items={[
            { label: t("app.nav.notes"), onClick: () => setActiveView("all-notes") },
            {
              label: selectedNote.title.trim() || t("app.newNote"),
              action: noteShareUrl ? <ShareLinkCopyAction url={noteShareUrl} /> : null,
            },
          ]}
          actions={noteToolbarActions}
        />
      )}
      <div ref={noteDetailScrollRef} className="note-detail-scroll" data-has-detail-bar="true">
        <NoteEditorRoute
          note={selectedNote}
          transcriptScrollRef={noteDetailScrollRef}
          folders={state.folders}
          recordingStatus={selectedNoteId === recordingNoteId ? state.recordingStatus : undefined}
          meetingEndCountdown={selectedNoteId === recordingNoteId ? meetingEndCountdown : null}
          recordingDisabled={Boolean(state.recordingStatus && selectedNoteId !== recordingNoteId)}
          recordingBlockedReason={fundingRequired ? recordingFundingDisabledReason() : undefined}
          fundingNotice={
            fundingRequired ? (
              <FundingNotice account={fundingAccount} onRefresh={refreshFundingAccount} />
            ) : undefined
          }
          fundingTier={fundingTierOf(fundingAccount)}
          retryBlockedReason={fundingRequired ? noteRetryFundingDisabledReason() : undefined}
          recoveryBlockedReason={fundingRequired ? recoveryFundingDisabledReason() : undefined}
          liveTranscript={selectedNoteLiveTranscript}
          sourceMode={sourceMode}
          sourceReadiness={sourceReadiness}
          recovery={selectedRecovery}
          onRecoverRecording={(sessionId) => handleRecovery(sessionId, "validate")}
          onDiscardRecording={(sessionId) => handleRecovery(sessionId, "discard")}
          onTitleChange={(title) => handleUpdateNote(selectedNote.id, { title })}
          onContentChange={(sourceNoteId, editedContent) => {
            handleUpdateNote(sourceNoteId, { editedContent });
          }}
          onFlushNote={(noteId) => void handleFlushNote(noteId)}
          onSourceModeChange={handleSourceModeChange}
          onEnableSystemAudio={handleEnableSystemAudio}
          onEnableMicrophone={handleEnableMicrophone}
          microphoneBlocked={microphoneBlocked}
          consentReminderPinned={
            import.meta.env.DEV && recordNoticesConsentPinned && selectedNoteId === recordingNoteId
          }
          onTabChange={(activeTab) =>
            void handleSaveNoteNow(selectedNote.id, {
              activeTab,
            })
          }
          onStartRecording={() => void handleStartRecording()}
          onPauseRecording={(sessionId) => void handlePauseRecording(sessionId)}
          onResumeRecording={(sessionId) => void handleResumeRecording(sessionId)}
          onFinishRecording={(sessionId) => void handleFinishRecording(sessionId)}
          onKeepRecordingAfterMeetingEnd={handleKeepRecordingAfterMeetingEnd}
          onStopNowAfterMeetingEnd={handleStopNowAfterMeetingEnd}
          onRetry={async () => {
            if (!selectedNote) return;
            if (fundingRequired) {
              setError(noteRetryFundingDisabledReason());
              return;
            }
            try {
              const note = await retryProcessing(
                selectedNote.id,
                selectedNote.retryRecordingSessionId,
              );
              dispatch({ type: "noteProcessingUpdated", note });
            } catch (err) {
              const message = messageFromError(err);
              dispatch({
                type: "noteProcessingUpdated",
                note: {
                  ...selectedNote,
                  processingStatus: "failed",
                  lastError: message,
                },
              });
              setError(null);
              throw err;
            }
          }}
          onTopUp={handleTopUp}
          topUpLabel={topUpLabel}
          onAssignFolder={(folderId) => void handleSetNoteFolder(selectedNote.id, folderId)}
          onRemoveFolder={(folderId) => void handleRemoveNoteFromFolder(selectedNote.id, folderId)}
          onNavigateToFolder={(folderId) => {
            setActiveView("folders");
            dispatch({ type: "folderSelected", folderId });
            setFolderReturnTarget({
              noteId: selectedNote.id,
              label: selectedNote.title.trim() || t("app.newNote"),
            });
            setOriginFolderId(undefined);
          }}
          onCreateAndAssignFolder={(name) => {
            void (async () => {
              const folder = await handleCreateFolder(name);
              if (folder) {
                await handleSetNoteFolder(selectedNote.id, folder.id);
              }
            })();
          }}
        />
      </div>
    </div>
  ) : (
    <section className="editor-empty" aria-label={t("app.workspace.openingNote")} />
  );
}
