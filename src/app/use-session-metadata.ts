import { useEffect, useRef } from "react";
import { listCompletedSessions, listSessionFolders } from "../lib/tauri";
import { messageFromError } from "../lib/errors";
import { useClovyDataChanged } from "../lib/clovy-data-events";
import type { UseSessionMetadataDependencies } from "./use-session-metadata-types";

export function useSessionMetadata(dependencies: UseSessionMetadataDependencies) {
  const {
    appBlocked,
    bootstrapped,
    sessionCompletionTouchedRef,
    setCompletedSessions,
    setError,
    setSessionFolders,
  } = dependencies;
  const invalidatedCompletions = useRef(new Set<string>());

  async function refreshAssignments(isCurrent: () => boolean) {
    try {
      const assignments = await listSessionFolders();
      if (!isCurrent()) return;
      const next: Record<string, string[]> = {};
      for (const assignment of assignments) {
        next[assignment.sessionId] ??= [];
        next[assignment.sessionId].push(assignment.folderId);
      }
      setSessionFolders(next);
    } catch (error) {
      if (isCurrent()) setError(messageFromError(error));
    }
  }

  useClovyDataChanged("projects", (_change, isCurrent) => refreshAssignments(isCurrent));
  useClovyDataChanged("conversations", async (change, isCurrent) => {
    if (change.id) {
      invalidatedCompletions.current.add(change.id);
      // This persisted event supersedes an earlier local toggle. A toggle
      // made during the asynchronous reload will mark the ID again.
      sessionCompletionTouchedRef.current.delete(change.id);
    }
    await refreshAssignments(isCurrent);
    try {
      const rows = await listCompletedSessions();
      if (!isCurrent()) return;
      // The latest reload reconciles every pending target, including those
      // whose earlier reads were superseded by a second native event.
      const invalidated = new Set(invalidatedCompletions.current);
      setCompletedSessions((previous) => {
        if (!isCurrent()) return previous;
        const next = { ...previous };
        const targets = change.id
          ? invalidated
          : new Set([...Object.keys(previous), ...rows.map((row) => row.sessionId)]);
        for (const id of targets) {
          if (!sessionCompletionTouchedRef.current.has(id)) {
            const row = rows.find((row) => row.sessionId === id);
            if (row) next[id] = row.completedAt;
            else delete next[id];
          }
          invalidatedCompletions.current.delete(id);
        }
        return next;
      });
    } catch (error) {
      if (isCurrent()) setError(messageFromError(error));
    }
  });

  useEffect(() => {
    if (appBlocked || !bootstrapped) return;
    let cancelled = false;
    void listSessionFolders()
      .then((assignments) => {
        if (cancelled) return;
        const next: Record<string, string[]> = {};
        for (const assignment of assignments) {
          next[assignment.sessionId] ??= [];
          next[assignment.sessionId].push(assignment.folderId);
        }
        setSessionFolders(next);
      })
      .catch((err: unknown) => {
        if (!cancelled) setError(messageFromError(err));
      });
    void listCompletedSessions()
      .then((rows) => {
        if (cancelled) return;
        // The session list can be interactive before this settles on a cold
        // launch. Merge rather than replace: this snapshot predates any toggle
        // the user already made, so locally toggled ids keep their local value
        // (applying the snapshot over them would show a completed session as
        // active while the database has it completed, until restart). Every
        // other row still applies, so the rest of the persisted completed
        // sessions are not lost (JUN-203 review).
        setCompletedSessions((prev) => {
          const next: Record<string, string> = {};
          for (const row of rows) next[row.sessionId] = row.completedAt;
          for (const touchedId of sessionCompletionTouchedRef.current) {
            const local = prev[touchedId];
            if (local === undefined) delete next[touchedId];
            else next[touchedId] = local;
          }
          return next;
        });
      })
      .catch((err: unknown) => {
        // Surface it like the sibling loads: a silent failure would present
        // previously completed sessions as active with no indication their
        // persisted state was unavailable (JUN-203 review).
        if (!cancelled) setError(messageFromError(err));
      });
    return () => {
      cancelled = true;
    };
  }, [appBlocked, bootstrapped]);
}
