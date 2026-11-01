import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  AGENT_DELETE_SESSION_EVENT,
  AGENT_SESSIONS_CHANGED_EVENT,
  AGENT_SESSIONS_HYDRATED_EVENT,
  type AgentSessionsChangedDetail,
  type AgentSessionsHydratedDetail,
  invalidateAgentSessionSnapshots,
} from "../lib/agent-events";
import { assignSessionToFolder, listAgentSessions } from "../lib/tauri";
import { AGENT_SESSION_STATUS_EVENT, type AgentSessionStatusDetail } from "../lib/agent-events";
import { getCurrentDataPartitionName } from "../lib/data-partition";
import { invalidateAgentSessionDraft, isAgentSessionDeleted } from "../lib/agent-session-drafts";
import { updateMenuBarSessionStatus } from "./app-effects/menu-bar-session-status";
import type { UseAgentSessionSyncDependencies } from "./use-agent-session-sync-types";

/** A copy of `next` for state, or the current set when the members match. */
function nextSessionIds(next: ReadonlySet<string>) {
  return (current: ReadonlySet<string>) =>
    current.size === next.size && [...next].every((id) => current.has(id))
      ? current
      : new Set(next);
}

export function useAgentSessionSync(dependencies: UseAgentSessionSyncDependencies) {
  const {
    activeViewRef,
    activeAgentSessionIdRef,
    tabsRef,
    setTabs,
    agentMenuBarLastStatusRef,
    agentMenuBarSessionsRef,
    agentMenuBarWaitingSessionIdsRef,
    agentMenuBarWorkingSessionIdsRef,
    commitAgentSessions,
    pendingSessionProjectRef,
    publishAgentMenuBarState,
    refreshSessionPartitions,
    setActiveAgentSession,
    setActiveAgentSessionId,
    setActiveAgentSessionSeed,
    setAgentOrigin,
    setAgentSessions,
    setAgentWaitingSessionIds,
    setAgentWorkingSessionIds,
    setSessionFolders,
  } = dependencies;

  useEffect(() => {
    let cancelled = false;
    // Only the latest broadcast commits its list: an older one whose map read
    // resolves last must not overwrite it.
    let latestRequest = 0;
    let unlistenDataChanged: (() => void) | undefined;
    void listen<{ profile: string; kind: string; id?: string; deleted?: boolean }>(
      "clovy-data-changed",
      ({ payload }) => {
        if (
          !["conversations", "projects"].includes(payload.kind) ||
          payload.profile !== getCurrentDataPartitionName()
        )
          return;
        invalidateAgentSessionSnapshots();
        if (payload.kind === "conversations" && payload.deleted && payload.id) {
          // Clear App's owner before child workspace listeners clear their own
          // selection; otherwise the child's callback can erase the identity
          // needed to retire this conversation's origin.
          if (activeAgentSessionIdRef.current === payload.id) {
            activeAgentSessionIdRef.current = undefined;
            setActiveAgentSession(undefined);
            setAgentOrigin(undefined);
          }
          window.dispatchEvent(
            new CustomEvent(AGENT_DELETE_SESSION_EVENT, { detail: { sessionId: payload.id } }),
          );
        }
        const request = ++latestRequest;
        void Promise.all([listAgentSessions(), refreshSessionPartitions()])
          .then(([sessions, partitions]) => {
            if (
              !cancelled &&
              request === latestRequest &&
              payload.profile === getCurrentDataPartitionName()
            ) {
              const visibleSessions = sessions.filter(
                (session) => !isAgentSessionDeleted(session.id),
              );
              commitAgentSessions(visibleSessions, partitions);
              window.dispatchEvent(
                new CustomEvent<AgentSessionsHydratedDetail>(AGENT_SESSIONS_HYDRATED_EVENT, {
                  detail: { sessions: visibleSessions, partitions, replaceExisting: true },
                }),
              );
            }
          })
          .catch(() => {});
      },
    )
      .then((cleanup) => {
        if (cancelled) cleanup();
        else unlistenDataChanged = cleanup;
      })
      .catch(() => {
        if (!cancelled) console.warn("Clovy conversation refresh subscription is unavailable.");
      });

    function handleSessionsChanged(event: Event) {
      const detail = (event as CustomEvent<AgentSessionsChangedDetail>).detail;
      if (!detail) return;
      const request = ++latestRequest;
      void refreshSessionPartitions()
        .then((partitions) => {
          if (cancelled || request !== latestRequest) return;
          commitAgentSessions(
            detail.sessions.filter((session) => !isAgentSessionDeleted(session.id)),
            partitions,
          );
        })
        .catch(() => {
          if (cancelled || request !== latestRequest) return;
          commitAgentSessions(
            detail.sessions.filter((session) => !isAgentSessionDeleted(session.id)),
          );
        });
      if (
        activeViewRef.current === "agent" &&
        (!detail.selectedSessionId || !isAgentSessionDeleted(detail.selectedSessionId))
      ) {
        const selectedSessionId =
          detail.selectedSessionId && !isAgentSessionDeleted(detail.selectedSessionId)
            ? detail.selectedSessionId
            : undefined;
        if (selectedSessionId) {
          setActiveAgentSessionId(selectedSessionId);
          setActiveAgentSessionSeed((current) =>
            current?.id === selectedSessionId ? current : undefined,
          );
        } else {
          setActiveAgentSession(undefined);
        }
      }
      // "New session" started from a project: file the first brand-new
      // session that gets selected; switching to a known session instead
      // abandons the intent.
      const pendingProject = pendingSessionProjectRef.current;
      if (
        pendingProject &&
        detail.selectedSessionId &&
        !isAgentSessionDeleted(detail.selectedSessionId)
      ) {
        pendingSessionProjectRef.current = null;
        const sessionId = detail.selectedSessionId;
        if (pendingProject.partition !== getCurrentDataPartitionName()) {
          setAgentOrigin(undefined);
        } else if (!pendingProject.knownSessionIds.has(sessionId)) {
          void assignSessionToFolder(sessionId, pendingProject.folderId)
            .then(() => {
              if (isAgentSessionDeleted(sessionId)) return;
              setSessionFolders((prev) => ({
                ...prev,
                [sessionId]: [pendingProject.folderId],
              }));
            })
            .catch(() => {});
        } else {
          // User switched to an existing session — abandon the pending
          // project intent so the workspace doesn't show misleading crumbs.
          setAgentOrigin(undefined);
        }
      }
      const nextWorkingSessionIds = new Set(
        detail.workingSessionIds.filter((id) => !isAgentSessionDeleted(id)),
      );
      const nextWaitingSessionIds = new Set(
        (detail.waitingSessionIds ?? []).filter((id) => !isAgentSessionDeleted(id)),
      );
      agentMenuBarWorkingSessionIdsRef.current = nextWorkingSessionIds;
      agentMenuBarWaitingSessionIdsRef.current = nextWaitingSessionIds;
      setAgentWorkingSessionIds(nextSessionIds(nextWorkingSessionIds));
      setAgentWaitingSessionIds(nextSessionIds(nextWaitingSessionIds));
      publishAgentMenuBarState();
    }

    function handleAgentStatusForMenuBar(event: Event) {
      const detail = (event as CustomEvent<AgentSessionStatusDetail>).detail;
      if (!detail || (detail.sessionId && isAgentSessionDeleted(detail.sessionId))) return;
      agentMenuBarLastStatusRef.current = detail;
      if (detail.sessionId) {
        updateMenuBarSessionStatus(detail.sessionId, detail.status, {
          working: agentMenuBarWorkingSessionIdsRef.current,
          waiting: agentMenuBarWaitingSessionIdsRef.current,
        });
        setAgentWorkingSessionIds(nextSessionIds(agentMenuBarWorkingSessionIdsRef.current));
        setAgentWaitingSessionIds(nextSessionIds(agentMenuBarWaitingSessionIdsRef.current));
      }
      publishAgentMenuBarState();
    }

    function handleAgentSessionDeleted(event: Event) {
      const detail = (event as CustomEvent<{ sessionId?: string }>).detail;
      const storedSessionId = detail?.sessionId;
      if (!storedSessionId) return;
      invalidateAgentSessionDraft(storedSessionId);
      latestRequest += 1;
      if (activeAgentSessionIdRef.current === storedSessionId) {
        activeAgentSessionIdRef.current = undefined;
        setActiveAgentSession(undefined);
        setAgentOrigin(undefined);
      } else {
        setActiveAgentSessionSeed((current) =>
          current?.id === storedSessionId ? undefined : current,
        );
      }
      const nextTabs = tabsRef.current.map((tab) =>
        tab.nav.view === "agent" && tab.nav.agentSessionId === storedSessionId
          ? { ...tab, nav: { view: "agent" as const } }
          : tab,
      );
      if (nextTabs.some((tab, index) => tab !== tabsRef.current[index])) {
        tabsRef.current = nextTabs;
        setTabs(nextTabs);
      }
      setSessionFolders((current) => {
        if (!(storedSessionId in current)) return current;
        const next = { ...current };
        delete next[storedSessionId];
        return next;
      });
      agentMenuBarSessionsRef.current = agentMenuBarSessionsRef.current.filter(
        (session) => session.id !== storedSessionId,
      );
      setAgentSessions((current) => current.filter((session) => session.id !== storedSessionId));
      agentMenuBarWorkingSessionIdsRef.current.delete(storedSessionId);
      agentMenuBarWaitingSessionIdsRef.current.delete(storedSessionId);
      setAgentWorkingSessionIds(new Set(agentMenuBarWorkingSessionIdsRef.current));
      setAgentWaitingSessionIds(new Set(agentMenuBarWaitingSessionIdsRef.current));
      publishAgentMenuBarState();
    }

    window.addEventListener(AGENT_SESSIONS_CHANGED_EVENT, handleSessionsChanged);
    window.addEventListener(AGENT_SESSION_STATUS_EVENT, handleAgentStatusForMenuBar);
    window.addEventListener(AGENT_DELETE_SESSION_EVENT, handleAgentSessionDeleted);
    return () => {
      cancelled = true;
      unlistenDataChanged?.();
      window.removeEventListener(AGENT_SESSIONS_CHANGED_EVENT, handleSessionsChanged);
      window.removeEventListener(AGENT_SESSION_STATUS_EVENT, handleAgentStatusForMenuBar);
      window.removeEventListener(AGENT_DELETE_SESSION_EVENT, handleAgentSessionDeleted);
    };
  }, [
    activeAgentSessionIdRef,
    tabsRef,
    setTabs,
    commitAgentSessions,
    publishAgentMenuBarState,
    refreshSessionPartitions,
  ]);
}
