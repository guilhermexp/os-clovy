import type { WorkstreamDto } from "../../lib/day-intelligence";

/**
 * Pure layout of the Today → Timeline → Workstreams column: one card per
 * workstream on a vertical axis of the day's real local hours.
 *
 * - The window runs from 20 minutes before the first sitting to 20 minutes
 *   after the last one (and to now, today), at least 150 minutes centered on
 *   the work.
 * - An hour with any work is drawn whole; a run of consecutive hours without
 *   work takes at most the height of one hour.
 * - The base scale is 1 px per minute; a day shorter than the canvas grows by
 *   one factor (capped at 9 px per minute) until it fills it.
 * - Positions come from the bounds of the measured sessions a workstream's
 *   work was located in (its sittings), never from its active minutes. Work
 *   known only by hour (a legacy record, or the minutes of an hour no session
 *   was named for) spans its whole local hour and is flagged approximate.
 * - DST days keep their 23 or 25 real local hours.
 */

/**
 * One stretch of a card: `session` is where located work happened (bounds of
 * measured sessions, joined); `hour` is a legacy hour known only by hour;
 * `unlocated` is the part of an hour's minutes no session was named for.
 */
export type TimelineSegment = {
  startMs: number;
  endMs: number;
  kind: "session" | "hour" | "unlocated";
  /** Known only by hour (`hour` or `unlocated`). */
  approximate: boolean;
  /** For an approximate segment: its active minutes in that hour. */
  minutes: number | null;
};

/** A sitting bar on the card's left rail, in the card's padding box. */
export type TimelineBar = { top: number; height: number; approximate: boolean };

/** What a card shows at its drawn height (spec section 5). */
export type CardDensity = "pill" | "title" | "meta" | "notes";

export type TimelineCard = {
  workstream: WorkstreamDto;
  /** Ascending by start; session bounds already joined. */
  segments: TimelineSegment[];
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
  bars: TimelineBar[];
  density: CardDensity;
};

/** A labeled hour on the rail: `hour` is its local hour, 0 to 23. */
export type TimelineHour = { ms: number; hour: number; top: number; current: boolean };

export type WorkstreamTimelineLayout = {
  cards: TimelineCard[];
  hours: TimelineHour[];
  height: number;
  pxPerMin: number;
  toPx: (ms: number) => number;
};

const MINUTE_MS = 60_000;
const HOUR_MS = 60 * MINUTE_MS;
/** Breathing room before the first and after the last sitting. */
const WINDOW_PAD_MS = 20 * MINUTE_MS;
const MIN_WINDOW_MS = 150 * MINUTE_MS;
const BASE_PX_PER_MIN = 1;
const MAX_PX_PER_MIN = 9;
/** Measured periods closer than this read as one sitting (second rounding). */
const SITTING_JOIN_MS = MINUTE_MS;
/** Small enough to land on every local hour, including +x:30/+x:45 zones. */
const SCAN_STEP_MS = 15 * MINUTE_MS;
/** Card border, so bars stay inside the padding box. */
const CARD_BORDER_PX = 1;
const BAR_INSET_PX = 10;
const BAR_INSET_SHARE = 0.25;
const BAR_MIN_PX = 4;
/** Drawn-height thresholds at the default text size (spec section 5). */
const DENSITY_PX = { title: 46, meta: 62, notes: 104 } as const;
/** Note slots: header measured, then 8 px, then 17 px per slot. */
const NOTES_GAP_PX = 8;
const NOTE_SLOT_PX = 17;
/** Header height (title and meta, one title line) before it is measured. */
export const ESTIMATED_HEADER_PX = 38;

type LocalHour = { ms: number; hour: number; key: string };

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
      hour: Number(hour),
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
): TimelineSegment[] {
  const located: { startMs: number; endMs: number }[] = [];
  const byHour: TimelineSegment[] = [];
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
  const sessions: TimelineSegment[] = [];
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

/** The visible stretch of the day, before any scale is applied. */
function timeWindow(
  firstMs: number,
  lastMs: number,
  nowMs: number | null,
  dayStartMs: number,
  dayEndMs: number,
): { startMs: number; endMs: number } {
  let startMs = firstMs - WINDOW_PAD_MS;
  let endMs = Math.max(lastMs + WINDOW_PAD_MS, nowMs ?? Number.NEGATIVE_INFINITY);
  if (endMs - startMs < MIN_WINDOW_MS) {
    const middle = (firstMs + lastMs) / 2;
    startMs = Math.floor((middle - MIN_WINDOW_MS / 2) / MINUTE_MS) * MINUTE_MS;
    endMs = Math.ceil((middle + MIN_WINDOW_MS / 2) / MINUTE_MS) * MINUTE_MS;
    // Centered on the work alone it could end before now, today.
    if (nowMs !== null) endMs = Math.max(endMs, nowMs);
  }
  if (startMs < dayStartMs) {
    endMs += dayStartMs - startMs;
    startMs = dayStartMs;
  }
  if (endMs > dayEndMs) {
    startMs = Math.max(dayStartMs, startMs - (endMs - dayEndMs));
    endMs = dayEndMs;
  }
  return { startMs, endMs };
}

/** A piece of the window: part of one local hour, or a run of idle hours. */
type Piece = { startMs: number; endMs: number; natural: number; top: number; height: number };

function buildPieces(
  hours: LocalHour[],
  dayEndMs: number,
  window: { startMs: number; endMs: number },
  busy: (startMs: number, endMs: number) => boolean,
): Piece[] {
  const pieces: (Piece & { active: boolean })[] = [];
  hours.forEach((hour, index) => {
    const hourEnd = hours[index + 1]?.ms ?? dayEndMs;
    const startMs = Math.max(hour.ms, window.startMs);
    const endMs = Math.min(hourEnd, window.endMs);
    if (endMs <= startMs) return;
    // One minute of work makes the whole hour active.
    const active = busy(hour.ms, hourEnd);
    const previous = pieces[pieces.length - 1];
    if (previous && !previous.active && !active) {
      previous.endMs = endMs;
      return;
    }
    pieces.push({ startMs, endMs, natural: 0, top: 0, height: 0, active });
  });
  for (const piece of pieces) {
    const minutes = (piece.endMs - piece.startMs) / MINUTE_MS;
    piece.natural = piece.active ? minutes : Math.min(minutes, HOUR_MS / MINUTE_MS);
  }
  return pieces;
}

/** A card before lanes, with its real (not minimum) height. */
type PlacedCard = TimelineCard & { realHeight: number };

/** Leftmost free lane per card, then the widest lane count of each group. */
function assignLanes(cards: PlacedCard[], minHeight: number, gap: number) {
  const sorted = [...cards].sort(
    (a, b) => a.top - b.top || b.height - a.height || a.workstream.id - b.workstream.id,
  );
  const laneEnds: number[] = [];
  let group: PlacedCard[] = [];
  let groupEnd = Number.NEGATIVE_INFINITY;
  const closeGroup = () => {
    const count = Math.max(1, ...group.map((card) => card.lane + 1));
    for (const card of group) card.laneCount = count;
    group = [];
    laneEnds.length = 0;
  };
  for (const card of sorted) {
    if (card.top >= groupEnd) closeGroup();
    // A card stretched to the minimum height also keeps the gap below it;
    // a real span lets the next card touch it.
    const end = card.top + Math.max(card.realHeight, minHeight + gap);
    let lane = laneEnds.findIndex((laneEnd) => laneEnd <= card.top);
    if (lane < 0) lane = laneEnds.length;
    laneEnds[lane] = end;
    card.lane = lane;
    group.push(card);
    groupEnd = Math.max(groupEnd, end);
  }
  closeGroup();
}

function barsFor(
  segments: TimelineSegment[],
  cardTop: number,
  cardHeight: number,
  toPx: (ms: number) => number,
): TimelineBar[] {
  const inset = Math.min(BAR_INSET_PX, cardHeight * BAR_INSET_SHARE);
  const innerHeight = cardHeight - 2 * CARD_BORDER_PX;
  return segments.map((segment) => {
    const top = Math.max(toPx(segment.startMs) - cardTop, inset);
    const bottom = Math.min(toPx(segment.endMs) - cardTop, cardHeight - inset);
    const height = Math.max(BAR_MIN_PX, bottom - top);
    // A sitting at the very end of a card stays inside it, never cut.
    return {
      top: Math.max(0, Math.min(top, innerHeight - height)),
      height,
      approximate: segment.approximate,
    };
  });
}

export function densityFor(height: number, textScale = 1): CardDensity {
  if (height >= DENSITY_PX.notes * textScale) return "notes";
  if (height >= DENSITY_PX.meta * textScale) return "meta";
  if (height >= DENSITY_PX.title * textScale) return "title";
  return "pill";
}

/**
 * How many notes a card shows: the slots below the measured header; when not
 * all fit, one slot goes to the "+N more" count, which is never shown alone.
 */
export function visibleNotes(
  count: number,
  cardHeight: number,
  headerHeight: number,
  textScale = 1,
): { shown: number; more: number } {
  const slots = Math.max(
    0,
    Math.floor((cardHeight - headerHeight - NOTES_GAP_PX) / (NOTE_SLOT_PX * textScale)),
  );
  if (count <= slots) return { shown: count, more: 0 };
  const shown = Math.max(0, slots - 1);
  return shown > 0 ? { shown, more: count - shown } : { shown: 0, more: 0 };
}

export function layoutWorkstreamTimeline(
  workstreams: WorkstreamDto[],
  options: {
    dayStartMs: number;
    dayEndMs: number;
    /** Today's current time; null on other days. */
    nowMs?: number | null;
    timeZone?: string;
    /** Usable canvas height: the scroller's content box. */
    canvasHeight?: number;
    /** Resolved --control-xs / title line box and --sp-2 from the interface. */
    minCardHeight?: number;
    cardGap?: number;
    /** The interface text-size preference (1 at the default size). */
    textScale?: number;
  },
): WorkstreamTimelineLayout {
  const { dayStartMs, dayEndMs, timeZone } = options;
  const minHeight = options.minCardHeight ?? 22;
  const gap = options.cardGap ?? 6;
  const textScale = options.textScale ?? 1;
  const nowMs =
    options.nowMs != null && options.nowMs > dayStartMs && options.nowMs < dayEndMs
      ? options.nowMs
      : null;
  const hours = localHours(dayStartMs, dayEndMs, timeZone);
  const placed = workstreams
    .map((workstream) => ({
      workstream,
      segments: cardSegments(workstream, hours, dayStartMs, dayEndMs),
    }))
    .filter((entry) => entry.segments.length > 0);
  if (placed.length === 0) {
    return { cards: [], hours: [], height: 0, pxPerMin: BASE_PX_PER_MIN, toPx: () => 0 };
  }
  const busySegments = placed.flatMap((entry) => entry.segments);
  const busy = (startMs: number, endMs: number) =>
    busySegments.some((segment) => segment.startMs < endMs && segment.endMs > startMs);
  const window = timeWindow(
    Math.min(...busySegments.map((segment) => segment.startMs)),
    Math.max(...busySegments.map((segment) => segment.endMs)),
    nowMs,
    dayStartMs,
    dayEndMs,
  );
  const pieces = buildPieces(hours, dayEndMs, window, busy);
  const natural = pieces.reduce((sum, piece) => sum + piece.natural, 0);
  const available = Number.isFinite(options.canvasHeight) ? (options.canvasHeight ?? 0) : 0;
  const pxPerMin =
    available > natural * BASE_PX_PER_MIN
      ? Math.min(MAX_PX_PER_MIN, available / natural)
      : BASE_PX_PER_MIN;
  let top = 0;
  for (const piece of pieces) {
    piece.top = top;
    piece.height = piece.natural * pxPerMin;
    top += piece.height;
  }
  const height = Math.round(top);
  const toPx = (ms: number) => {
    const clamped = Math.min(Math.max(ms, window.startMs), window.endMs);
    const piece =
      pieces.find((candidate) => clamped >= candidate.startMs && clamped < candidate.endMs) ??
      pieces[pieces.length - 1];
    const fraction = (clamped - piece.startMs) / (piece.endMs - piece.startMs);
    return Math.round(piece.top + fraction * piece.height);
  };

  const cards = placed.map(({ workstream, segments }): PlacedCard => {
    const startMs = segments[0].startMs;
    const endMs = Math.max(...segments.map((segment) => segment.endMs));
    const cardTop = toPx(startMs);
    const realHeight = toPx(endMs) - cardTop;
    const cardHeight = Math.max(realHeight, minHeight);
    return {
      workstream,
      segments,
      startMs,
      endMs,
      approximate: segments.some((segment) => segment.approximate),
      sittings: segments.filter((segment) => !segment.approximate).length,
      activeMs: workstream.minutes * MINUTE_MS,
      lane: 0,
      laneCount: 1,
      top: cardTop,
      height: cardHeight,
      realHeight,
      bars: barsFor(segments, cardTop, cardHeight, toPx),
      density: densityFor(cardHeight, textScale),
    };
  });
  assignLanes(cards, minHeight, gap);

  // Labels: every hour with work and the hour right after one (where the work
  // stopped); today, nothing after the current hour, which is always marked.
  const marks: TimelineHour[] = [];
  hours.forEach((hour, index) => {
    const hourEnd = hours[index + 1]?.ms ?? dayEndMs;
    const current = nowMs !== null && hour.ms <= nowMs && nowMs < hourEnd;
    if (nowMs !== null && hour.ms > nowMs) return;
    const previous = hours[index - 1];
    const worked = busy(hour.ms, hourEnd);
    const workedBefore = previous !== undefined && busy(previous.ms, hour.ms);
    if (!worked && !workedBefore && !current) return;
    marks.push({ ms: hour.ms, hour: hour.hour, top: toPx(hour.ms), current });
  });

  cards.sort((a, b) => a.startMs - b.startMs || a.workstream.id - b.workstream.id);
  return {
    cards: cards.map(({ realHeight: _realHeight, ...card }) => card),
    hours: marks,
    height: Math.max(height, ...cards.map((card) => card.top + card.height)),
    pxPerMin,
    toPx,
  };
}
