import type { AgentSessionStatusDetail } from "../../lib/agent-events";

export function updateMenuBarSessionStatus(
  sessionId: string,
  status: AgentSessionStatusDetail["status"],
  sessions: { working: Set<string>; waiting: Set<string> },
) {
  if (status === "waitingForUser") {
    sessions.working.delete(sessionId);
    sessions.waiting.add(sessionId);
    return;
  }
  if (status === "starting" || status === "running") {
    sessions.waiting.delete(sessionId);
    sessions.working.add(sessionId);
    return;
  }
  if (status === "completed" || status === "failed" || status === "cancelled") {
    sessions.working.delete(sessionId);
    sessions.waiting.delete(sessionId);
  }
}
