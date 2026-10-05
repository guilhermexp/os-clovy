import type { WorkstreamDto } from "../../lib/day-intelligence";

/**
 * Pure layout of the "Today" grid: workstreams as cards on a vertical axis of
 * real local hours. Positions come from the bounds of the measured sessions a
 * workstream's work was located in: where it happened, not how long it took
 * (a session can hold several workstreams), so a segment never carries a
 * duration of its own. Work known only by hour (a legacy record, or the
 * minutes of an hour no session was named for) is drawn as its whole local
 * hour and flagged approximate, never given invented minutes. Hours with no
 * work collapse into one short step, and the scale stays linear in real time
 * inside every hour with work, so DST days (23 or 25 hours) keep real
 * durations.
 */

/**
 * One stretch of a card: `session` is where located work happened (bounds of
 * measured sessions, joined); `hour` is a legacy hour known only by hour;
 * `unlocated` is the part of an hour's minutes no session was named for.
 */
export type GridSegment = {
  startMs: number;
  endMs: number;
  kind: "session" | "hour" | "unlocated";
  /** Known only by hour (`hour` or `unlocated`). */
  approximate: boolean;
  /** For an approximate segment: its active minutes in that hour. */
  minutes: number | null;
};

export type GridCard = {
  workstream: WorkstreamDto;
  /** Ascending by start; session bounds already joined. */
  segments: GridSegment[];
  startMs: number;
  endMs: number;
  /** Some of its work is known only by hour. */
  approximate: boolean;
  /** Separate session stretches (resumptions), not counting hour-only parts. */
  sittings: number;
  /** Active time from the data, independent of the card's span. */
  activeMs: number;
  lane: number;
  laneCount: number;
  top: number;
  height: number;
};

export type GridTick = { ms: number; label: string; top: number };

export type DayGridLayout = {
  cards: GridCard[];
  ticks: GridTick[];
  height: number;
  toPx: (ms: number) => number;
};

/** Pixels per real minute inside an hour with work (90 px per hour). */
const PX_PER_MIN = 1.5;
/** Height of a run of consecutive hours without any work. */
const IDLE_RUN_PX = 36;
/** The smallest card, so its title and times stay readable. */
export const MIN_CARD_PX = 52;
/** Measured periods closer than this read as one sitting (second rounding). */
const SITTING_JOIN_MS = 60_000;
/** Small enough to land on every local hour, including +x:30/+x:45 zones. */
const SCAN_STEP_MS = 15 * 60_000;

type LocalHour = { ms: number; label: string; key: string };

/**
 * Every real local hour start in `[dayStartMs, dayEndMs)`: 23, 24, or 25 of
 * them. `key` is the hour's local "YYYY-MM-DDTHH", the same key the hour
 * reports use; on a fall-back day the repeated hour has two starts with one
 * key, and that key's window runs through both.
 */
function localHours(dayStartMs: number, dayEndMs: number, timeZone?: string): LocalHour[] {
  const format = new Intl.DateTimeFormat("en-US", {
    timeZone,
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    hourCycle: "h23",
  });
  const hours: LocalHour[] = [];
  for (let ms = dayStartMs; ms < dayEndMs; ms += SCAN_STEP_MS) {
    const fields: Record<string, string> = {};
    for (const part of format.formatToParts(new Date(ms))) fields[part.type] = part.value;
    if (Number(fields.minute) !== 0) continue;
    const hour = fields.hour.padStart(2, "0");
    hours.push({
      ms,
      label: `${hour}:00`,
      key: `${fields.year}-${fields.month}-${fields.day}T${hour}`,
    });
  }
  return hours;
}

/** The local window of an hour report key: first start to the next other key. */
function hourWindow(
  hours: LocalHour[],
  key: string,
  dayEndMs: number,
): { startMs: number; endMs: number } | null {
  const index = hours.findIndex((local) => local.key === key);
  if (index < 0) return null;
  const next = hours.slice(index + 1).find((local) => local.key !== key);
  return { startMs: hours[index].ms, endMs: next?.ms ?? dayEndMs };
}

function cardSegments(
  workstream: WorkstreamDto,
  hours: LocalHour[],
  dayStartMs: number,
  dayEndMs: number,
): GridSegment[] {
  const located: { startMs: number; endMs: number }[] = [];
  const byHour: GridSegment[] = [];
  for (const hour of workstream.hours) {
    const unlocatedMinutes = hour.periods ? hour.minutes - hour.locatedMinutes : hour.minutes;
    for (const period of hour.periods ?? []) {
      const startMs = Math.max(Date.parse(period.startedAt), dayStartMs);
      const endMs = Math.min(Date.parse(period.endedAt), dayEndMs);
      if (endMs > startMs) located.push({ startMs, endMs });
    }
    if (hour.periods && unlocatedMinutes <= 0) continue;
    const span = hourWindow(hours, hour.hour, dayEndMs);
    if (!span) continue;
    byHour.push({
      ...span,
      kind: hour.periods ? "unlocated" : "hour",
      approximate: true,
      minutes: unlocatedMinutes,
    });
  }
  located.sort((a, b) => a.startMs - b.startMs);
  const sessions: GridSegment[] = [];
  for (const period of located) {
    const last = sessions[sessions.length - 1];
    if (last && period.startMs - last.endMs <= SITTING_JOIN_MS) {
      last.endMs = Math.max(last.endMs, period.endMs);
    } else {
      sessions.push({ ...period, kind: "session", approximate: false, minutes: null });
    }
  }
  return [...sessions, ...byHour].sort((a, b) => a.startMs - b.startMs || a.endMs - b.endMs);
}

type Bucket = { startMs: number; endMs: number; active: boolean; top: number; height: number };

function buildScale(
  hours: LocalHour[],
  dayEndMs: number,
  busy: { startMs: number; endMs: number }[],
): { buckets: Bucket[]; height: number } {
  const first = Math.min(...busy.map((range) => range.startMs));
  const last = Math.max(...busy.map((range) => range.endMs));
  const buckets: Bucket[] = [];
  let top = 0;
  hours.forEach((hour, index) => {
    const endMs = hours[index + 1]?.ms ?? dayEndMs;
    if (endMs <= first || hour.ms >= last) return;
    const active = busy.some((range) => range.startMs < endMs && range.endMs > hour.ms);
    const previous = buckets[buckets.length - 1];
    if (previous && !previous.active && !active) {
      previous.endMs = endMs;
      return;
    }
    const height = active ? ((endMs - hour.ms) / 60_000) * PX_PER_MIN : IDLE_RUN_PX;
    buckets.push({ startMs: hour.ms, endMs, active, top, height });
    top += height;
  });
  return { buckets, height: top };
}

function assignLanes(cards: GridCard[]) {
  const sorted = [...cards].sort((a, b) => a.top - b.top || b.height - a.height);
  const laneEnds: number[] = [];
  let cluster: GridCard[] = [];
  let clusterEnd = Number.NEGATIVE_INFINITY;
  const closeCluster = () => {
    const count = Math.max(1, ...cluster.map((card) => card.lane + 1));
    for (const card of cluster) card.laneCount = count;
    cluster = [];
    laneEnds.length = 0;
  };
  for (const card of sorted) {
    if (card.top >= clusterEnd) closeCluster();
    let lane = laneEnds.findIndex((end) => end <= card.top);
    if (lane < 0) lane = laneEnds.length;
    laneEnds[lane] = card.top + card.height;
    card.lane = lane;
    cluster.push(card);
    clusterEnd = Math.max(clusterEnd, card.top + card.height);
  }
  closeCluster();
}

export function layoutDayGrid(
  workstreams: WorkstreamDto[],
  options: { dayStartMs: number; dayEndMs: number; nowMs?: number | null; timeZone?: string },
): DayGridLayout {
  const { dayStartMs, dayEndMs, nowMs, timeZone } = options;
  const hours = localHours(dayStartMs, dayEndMs, timeZone);
  const placed = workstreams
    .map((workstream) => ({
      workstream,
      segments: cardSegments(workstream, hours, dayStartMs, dayEndMs),
    }))
    .filter((entry) => entry.segments.length > 0);
  if (placed.length === 0) {
    return { cards: [], ticks: [], height: 0, toPx: () => 0 };
  }
  const busy = placed.flatMap((entry) => entry.segments);
  const withNow =
    nowMs != null && nowMs > dayStartMs && nowMs < dayEndMs
      ? [...busy, { startMs: nowMs, endMs: nowMs + 1 }]
      : busy;
  const { buckets, height } = buildScale(hours, dayEndMs, withNow);
  const toPx = (ms: number) => {
    const bucket =
      buckets.find((candidate) => ms >= candidate.startMs && ms < candidate.endMs) ??
      (ms < buckets[0].startMs ? buckets[0] : buckets[buckets.length - 1]);
    const clamped = Math.min(Math.max(ms, bucket.startMs), bucket.endMs);
    const fraction = (clamped - bucket.startMs) / (bucket.endMs - bucket.startMs);
    return Math.round(bucket.top + fraction * bucket.height);
  };
  const cards: GridCard[] = placed.map(({ workstream, segments }) => {
    const startMs = segments[0].startMs;
    const endMs = Math.max(...segments.map((segment) => segment.endMs));
    const top = toPx(startMs);
    return {
      workstream,
      segments,
      startMs,
      endMs,
      approximate: segments.some((segment) => segment.approximate),
      sittings: segments.filter((segment) => !segment.approximate).length,
      activeMs: workstream.minutes * 60_000,
      lane: 0,
      laneCount: 1,
      top,
      height: Math.max(toPx(endMs) - top, MIN_CARD_PX),
    };
  });
  assignLanes(cards);
  const ticks: GridTick[] = buckets.map((bucket) => ({
    ms: bucket.startMs,
    label: hours.find((hour) => hour.ms === bucket.startMs)?.label ?? "",
    top: bucket.top,
  }));
  const lastBucket = buckets[buckets.length - 1];
  const closing = hours.find((hour) => hour.ms === lastBucket.endMs);
  ticks.push({ ms: lastBucket.endMs, label: closing?.label ?? "24:00", top: height });
  cards.sort((a, b) => a.startMs - b.startMs || a.workstream.id - b.workstream.id);
  return {
    cards,
    ticks,
    height: Math.max(height, ...cards.map((card) => card.top + card.height)),
    toPx,
  };
}
