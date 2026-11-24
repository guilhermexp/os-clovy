export const AGENT_NEW_SESSION_EVENT = "clovy:agent:new-session";
export const AGENT_NEW_SESSION_PENDING_KEY = "clovy:agent:new-session-pending";
export const AGENT_SESSION_STATUS_EVENT = "clovy:agent:session-status";
export const AGENT_RUN_SETTLED_EVENT = "clovy:agent:run-settled";
export const AGENT_OPEN_EVENT = "clovy:agent:open";

export type AgentSessionStatusKind =
  | "received"
  | "starting"
  | "running"
  | "waitingForUser"
  | "completed"
  | "failed"
  | "cancelled";

export type AgentSessionStatusDetail = {
  sessionId?: string;
  title?: string;
  prompt?: string;
  status: AgentSessionStatusKind;
  summary?: string;
  activeCount?: number;
  needsUserCount?: number;
};

export type AgentRunSettledDetail = {
  sessionId: string;
  title: string;
  summary: string;
  activeCount: number;
};

export function dispatchAgentSessionStatus(detail: AgentSessionStatusDetail) {
  window.dispatchEvent(
    new CustomEvent<AgentSessionStatusDetail>(AGENT_SESSION_STATUS_EVENT, {
      detail,
    }),
  );
  void import("@tauri-apps/api/event")
    .then((api) =>
      typeof api.emit === "function" ? api.emit(AGENT_SESSION_STATUS_EVENT, detail) : undefined,
    )
    .catch(() => {});
}

export function dispatchAgentRunSettled(detail: AgentRunSettledDetail) {
  window.dispatchEvent(
    new CustomEvent<AgentRunSettledDetail>(AGENT_RUN_SETTLED_EVENT, {
      detail,
    }),
  );
  void import("@tauri-apps/api/event")
    .then((api) =>
      typeof api.emit === "function" ? api.emit(AGENT_RUN_SETTLED_EVENT, detail) : undefined,
    )
    .catch(() => {});
}
