export const CLOVY_HOME_SESSION_IDS_STORAGE_KEY = "clovy:home:session-ids:v1";
export const CLOVY_HOME_CHECK_INS_STORAGE_KEY = "clovy:home:check-ins:v1";
export const CLOVY_HOME_TASK_HANDOFFS_STORAGE_KEY = "clovy:home:task-handoffs:v1";
export const CLOVY_HOME_DIRECT_TURNS_STORAGE_KEY = "clovy:home:direct-turns:v1";
export const LEGACY_CLOVY_HOME_TASK_HANDOFFS_STORAGE_KEY = "clovy.home.taskHandoffs.v1";
export const LEGACY_CLOVY_HOME_DIRECT_TURNS_STORAGE_KEY = "clovy.home.directTurns.v1";
export const CLOVY_HOME_THREAD_CHANGED_EVENT = "clovy:agent:home-thread-changed";

const homeThreadRetargets = new Map<string, string | null>();

function storageOrUndefined(): Storage | undefined {
  try {
    return window.localStorage;
  } catch {
    return undefined;
  }
}

function readUnknownMap(key: string): Record<string, unknown> {
  try {
    const raw = storageOrUndefined()?.getItem(key);
    const parsed: unknown = raw ? JSON.parse(raw) : null;
    return parsed && typeof parsed === "object" && !Array.isArray(parsed)
      ? (parsed as Record<string, unknown>)
      : {};
  } catch {
    return {};
  }
}

function readStringMap(key: string): Record<string, string> {
  return Object.fromEntries(
    Object.entries(readUnknownMap(key)).filter((entry): entry is [string, string] => {
      return typeof entry[1] === "string";
    }),
  );
}

function writeJson(key: string, value: unknown): void {
  try {
    storageOrUndefined()?.setItem(key, JSON.stringify(value));
  } catch {
    // Home remains usable for this launch when storage is unavailable.
  }
}

export function readClovyHomeStoredSessionId(profile: string): string | undefined {
  const storedSessionId = readStringMap(CLOVY_HOME_SESSION_IDS_STORAGE_KEY)[profile]?.trim();
  return storedSessionId || undefined;
}

export function writeClovyHomeStoredSessionId(profile: string, storedSessionId: string): void {
  const normalizedProfile = profile.trim() || "default";
  const normalizedSessionId = storedSessionId.trim();
  if (!normalizedSessionId) return;
  // An explicit new profile mapping reactivates this id. This matters when a
  // test fixture or repaired prerelease database deliberately reuses an id
  // that was retired earlier in the same WebView process.
  homeThreadRetargets.delete(normalizedSessionId);
  writeJson(CLOVY_HOME_SESSION_IDS_STORAGE_KEY, {
    ...readStringMap(CLOVY_HOME_SESSION_IDS_STORAGE_KEY),
    [normalizedProfile]: normalizedSessionId,
  });
}

/** Resolve writes owned by an in-flight Home request after its profile was
 * moved or deleted. Moving redirects the late assistant reply into the merged
 * thread; permanent deletion drops it instead of recreating private data. */
export function resolveClovyHomeThreadSessionId(storedSessionId: string): string | undefined {
  let current = storedSessionId;
  const visited = new Set<string>();
  while (!visited.has(current) && homeThreadRetargets.has(current)) {
    visited.add(current);
    const target = homeThreadRetargets.get(current);
    if (!target) return undefined;
    current = target;
  }
  return current;
}

export function retargetClovyHomeThread(sourceSessionId: string, targetSessionId?: string): void {
  const source = sourceSessionId.trim();
  const target = targetSessionId?.trim();
  if (!source || source === target) return;
  homeThreadRetargets.set(source, target || null);
}

export function forgetClovyHomeStoredSessionId(
  profile: string,
  expectedStoredSessionId?: string,
): void {
  const records = readStringMap(CLOVY_HOME_SESSION_IDS_STORAGE_KEY);
  if (expectedStoredSessionId && records[profile] !== expectedStoredSessionId) return;
  if (!(profile in records)) return;
  delete records[profile];
  writeJson(CLOVY_HOME_SESSION_IDS_STORAGE_KEY, records);
}

export function dispatchClovyHomeThreadChanged(storedSessionId: string): void {
  window.dispatchEvent(
    new CustomEvent<{ storedSessionId: string }>(CLOVY_HOME_THREAD_CHANGED_EVENT, {
      detail: { storedSessionId },
    }),
  );
}

function mergedHomeArray(left: unknown, right: unknown): unknown[] {
  const values = [...(Array.isArray(left) ? left : []), ...(Array.isArray(right) ? right : [])];
  const byId = new Map<string, unknown>();
  const withoutId: unknown[] = [];
  for (const value of values) {
    const id =
      value && typeof value === "object" && !Array.isArray(value) && "id" in value
        ? (value as { id?: unknown }).id
        : undefined;
    if (typeof id === "string") byId.set(id, value);
    else withoutId.push(value);
  }
  return [...withoutId, ...byId.values()].sort((leftValue, rightValue) => {
    const createdAt = (value: unknown) =>
      value && typeof value === "object" && !Array.isArray(value) && "createdAt" in value
        ? String((value as { createdAt?: unknown }).createdAt ?? "")
        : "";
    const leftCreatedAt = createdAt(leftValue);
    const rightCreatedAt = createdAt(rightValue);
    return leftCreatedAt && rightCreatedAt ? leftCreatedAt.localeCompare(rightCreatedAt) : 0;
  });
}

function reconcileHomeThreadStore(
  storageKey: string,
  sourceSessionId: string,
  targetSessionId: string | undefined,
): void {
  const records = readUnknownMap(storageKey);
  if (!(sourceSessionId in records)) return;
  if (targetSessionId && targetSessionId !== sourceSessionId) {
    records[targetSessionId] = mergedHomeArray(records[targetSessionId], records[sourceSessionId]);
  }
  if (targetSessionId !== sourceSessionId) delete records[sourceSessionId];
  writeJson(storageKey, records);
}

export type ClovyHomeProfileRemovalPlan = {
  sourceSessionId?: string;
  targetSessionId?: string;
  redundantSessionId?: string;
};

export function clovyHomeProfileRemovalPlan(
  profile: string,
  disposition: "move" | "delete",
): ClovyHomeProfileRemovalPlan {
  const normalizedProfile = profile.trim();
  if (!normalizedProfile || normalizedProfile === "default") return {};
  const sessionIds = readStringMap(CLOVY_HOME_SESSION_IDS_STORAGE_KEY);
  const sourceSessionId = sessionIds[normalizedProfile]?.trim();
  const targetSessionId =
    disposition === "move" ? sessionIds.default?.trim() || sourceSessionId : undefined;
  return {
    ...(sourceSessionId ? { sourceSessionId } : {}),
    ...(targetSessionId ? { targetSessionId } : {}),
    ...(disposition === "move" && sourceSessionId && targetSessionId !== sourceSessionId
      ? { redundantSessionId: sourceSessionId }
      : {}),
  };
}

export function reconcileClovyHomeProfileRemoval(
  profile: string,
  disposition: "move" | "delete",
): ClovyHomeProfileRemovalPlan {
  const normalizedProfile = profile.trim();
  if (!normalizedProfile || normalizedProfile === "default") return {};

  const sessionIds = readStringMap(CLOVY_HOME_SESSION_IDS_STORAGE_KEY);
  const plan = clovyHomeProfileRemovalPlan(profile, disposition);
  const { sourceSessionId, targetSessionId } = plan;

  if (sourceSessionId) {
    retargetClovyHomeThread(sourceSessionId, targetSessionId);
    for (const storageKey of [
      CLOVY_HOME_DIRECT_TURNS_STORAGE_KEY,
      CLOVY_HOME_TASK_HANDOFFS_STORAGE_KEY,
      LEGACY_CLOVY_HOME_DIRECT_TURNS_STORAGE_KEY,
      LEGACY_CLOVY_HOME_TASK_HANDOFFS_STORAGE_KEY,
    ]) {
      reconcileHomeThreadStore(storageKey, sourceSessionId, targetSessionId);
    }
    if (targetSessionId) dispatchClovyHomeThreadChanged(targetSessionId);
  }

  delete sessionIds[normalizedProfile];
  if (disposition === "move" && targetSessionId) sessionIds.default = targetSessionId;
  writeJson(CLOVY_HOME_SESSION_IDS_STORAGE_KEY, sessionIds);

  const checkIns = readUnknownMap(CLOVY_HOME_CHECK_INS_STORAGE_KEY);
  if (disposition === "move" && !checkIns.default && checkIns[normalizedProfile]) {
    checkIns.default = checkIns[normalizedProfile];
  }
  delete checkIns[normalizedProfile];
  writeJson(CLOVY_HOME_CHECK_INS_STORAGE_KEY, checkIns);

  return plan;
}
