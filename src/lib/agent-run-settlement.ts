import { listen as tauriListen, type UnlistenFn } from "@tauri-apps/api/event";
import { t } from "../i18n/translate";
import { dispatchAgentRunSettled, dispatchAgentSessionStatus } from "./agent-events";
import type { AgentRuntimeEvent } from "./agent-runtime-contract";

const AGENT_RUNTIME_EVENT = "clovy://agent-runtime-event";

/** Bounded memory of runs already settled, so a replayed terminal frame for
 * the same run cannot announce it twice. */
const SETTLED_RUN_MEMORY = 200;

type ListenToRuntimeEvents = (
  event: string,
  handler: (event: { payload: AgentRuntimeEvent }) => void,
) => Promise<UnlistenFn>;

export type AgentRunSettlementRelayOptions = {
  /** Current title for a stored session id, when the caller knows one. */
  resolveTitle: (sessionId: string) => string | undefined;
  listen?: ListenToRuntimeEvents;
};

/** Announces completed and failed runs once, even while another view is open. */
export function startAgentRunSettlementRelay(options: AgentRunSettlementRelayOptions) {
  const listen = options.listen ?? (tauriListen as ListenToRuntimeEvents);
  const activeRuns = new Set<string>();
  const finishedRuns = new Set<string>();
  let disposed = false;
  let unlisten: UnlistenFn | undefined;

  const rememberFinished = (runId: string) => {
    finishedRuns.add(runId);
    if (finishedRuns.size > SETTLED_RUN_MEMORY) {
      const oldest = finishedRuns.values().next().value;
      if (oldest !== undefined) finishedRuns.delete(oldest);
    }
  };

  void listen(AGENT_RUNTIME_EVENT, ({ payload }) => {
    if (!payload?.runId || payload.method === "run.diagnostic" || finishedRuns.has(payload.runId))
      return;
    const terminal =
      payload.method === "run.completed" ||
      payload.method === "run.cancelled" ||
      payload.method === "run.failed";
    if (!terminal) {
      activeRuns.add(payload.runId);
      return;
    }
    activeRuns.delete(payload.runId);
    rememberFinished(payload.runId);
    if (payload.method === "run.failed") {
      const title = options.resolveTitle(payload.sessionId)?.trim();
      dispatchAgentSessionStatus({
        sessionId: payload.sessionId,
        ...(title ? { title } : {}),
        status: "failed",
        summary: payload.data.message,
        activeCount: activeRuns.size,
      });
      return;
    }
    if (payload.method !== "run.completed") return;
    dispatchAgentRunSettled({
      sessionId: payload.sessionId,
      title: options.resolveTitle(payload.sessionId)?.trim() ?? "",
      summary: t("lib.agentNotify.finished"),
      activeCount: activeRuns.size,
    });
  })
    .then((dispose) => {
      if (disposed) dispose();
      else unlisten = dispose;
    })
    .catch(() => {});

  return () => {
    disposed = true;
    unlisten?.();
  };
}
