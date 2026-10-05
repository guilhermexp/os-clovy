import { describe, expect, it } from "vitest";
import { layoutDayGrid } from "../components/activity-timeline/day-grid-layout";
import type { WorkstreamDto } from "../lib/day-intelligence";

// Pure layout of the "Today" grid, pinned to a DST zone. New York local days,
// given as their UTC instants (local midnight to next local midnight).
describe("Day grid layout", () => {
  const ZONE = "America/New_York";

  it("places overlapping workstreams in separate lanes and separate ones in the same lane", () => {
    const start = Date.parse("2026-06-15T04:00:00Z");
    const end = Date.parse("2026-06-16T04:00:00Z");
    const period = (from: string, to: string) => ({
      startedAt: `2026-06-15T${from}:00Z`,
      endedAt: `2026-06-15T${to}:00Z`,
    });
    const ws = (id: number, from: string, to: string): WorkstreamDto => ({
      id,
      title: `W${id}`,
      summary: "",
      minutes: 10,
      hours: [
        {
          hour: "2026-06-15T09",
          minutes: 10,
          note: "",
          periods: [period(from, to)],
          locatedMinutes: 10,
        },
      ],
    });
    const layout = layoutDayGrid(
      [ws(1, "13:00", "13:40"), ws(2, "13:20", "13:50"), ws(3, "16:00", "16:30")],
      {
        dayStartMs: start,
        dayEndMs: end,
        timeZone: ZONE,
      },
    );
    const lane = (id: number) => layout.cards.find((c) => c.workstream.id === id);
    expect(lane(1)?.laneCount).toBe(2);
    expect(lane(1)?.lane).not.toBe(lane(2)?.lane);
    expect(lane(3)?.laneCount).toBe(1);
    expect(lane(3)?.lane).toBe(0);
  });

  it("measures a spring-forward day by real time and places a legacy hour at its real local start", () => {
    // 2026-03-08 in New York has no 02:00: 01:30 to 03:30 local is one real hour.
    const start = Date.parse("2026-03-08T05:00:00Z");
    const end = Date.parse("2026-03-09T04:00:00Z");
    const precise: WorkstreamDto = {
      id: 1,
      title: "Across the change",
      summary: "",
      minutes: 60,
      hours: [
        {
          hour: "2026-03-08T01",
          minutes: 60,
          note: "",
          periods: [{ startedAt: "2026-03-08T06:30:00Z", endedAt: "2026-03-08T07:30:00Z" }],
          locatedMinutes: 60,
        },
      ],
    };
    const old: WorkstreamDto = {
      id: 2,
      title: "Legacy",
      summary: "",
      minutes: 20,
      hours: [{ hour: "2026-03-08T03", minutes: 20, note: "", periods: null, locatedMinutes: 0 }],
    };
    const layout = layoutDayGrid([precise, old], {
      dayStartMs: start,
      dayEndMs: end,
      timeZone: ZONE,
    });
    const legacyCard = layout.cards.find((c) => c.workstream.id === 2);
    expect(legacyCard?.approximate).toBe(true);
    expect(legacyCard?.segments[0].startMs).toBe(Date.parse("2026-03-08T07:00:00Z"));
    expect(legacyCard?.segments[0].endMs).toBe(Date.parse("2026-03-08T08:00:00Z"));
    const preciseCard = layout.cards.find((c) => c.workstream.id === 1);
    expect(preciseCard?.approximate).toBe(false);
    expect(preciseCard?.endMs).toBe(preciseCard ? preciseCard.startMs + 3_600_000 : NaN);
    expect(
      layout.toPx(Date.parse("2026-03-08T07:30:00Z")) -
        layout.toPx(Date.parse("2026-03-08T06:30:00Z")),
    ).toBe(
      layout.toPx(Date.parse("2026-03-08T08:00:00Z")) -
        layout.toPx(Date.parse("2026-03-08T07:00:00Z")),
    );
    expect(layout.ticks.map((tick) => tick.label)).not.toContain("02:00");
  });

  it("keeps an hour's work that no session located known by hour instead of treating it as located", () => {
    const start = Date.parse("2026-06-15T04:00:00Z");
    const end = Date.parse("2026-06-16T04:00:00Z");
    const partial: WorkstreamDto = {
      id: 1,
      title: "Partly located",
      summary: "",
      minutes: 30,
      hours: [
        {
          hour: "2026-06-15T09",
          minutes: 30,
          note: "",
          periods: [{ startedAt: "2026-06-15T13:10:00Z", endedAt: "2026-06-15T13:20:00Z" }],
          locatedMinutes: 10,
        },
      ],
    };
    const [card] = layoutDayGrid([partial], {
      dayStartMs: start,
      dayEndMs: end,
      timeZone: ZONE,
    }).cards;
    expect(card.approximate).toBe(true);
    expect(card.sittings).toBe(1);
    const unlocated = card.segments.find((segment) => segment.kind === "unlocated");
    expect(unlocated).toMatchObject({
      startMs: Date.parse("2026-06-15T13:00:00Z"),
      endMs: Date.parse("2026-06-15T14:00:00Z"),
      minutes: 20,
    });
  });

  it("draws a legacy repeated fall-back hour over its whole two-hour window", () => {
    // 2026-11-01 in New York repeats 01:00: the key "T01" spans 01:00 EDT to 02:00 EST.
    const start = Date.parse("2026-11-01T04:00:00Z");
    const end = Date.parse("2026-11-02T05:00:00Z");
    const old: WorkstreamDto = {
      id: 1,
      title: "Legacy",
      summary: "",
      minutes: 50,
      hours: [{ hour: "2026-11-01T01", minutes: 50, note: "", periods: null, locatedMinutes: 0 }],
    };
    const [card] = layoutDayGrid([old], { dayStartMs: start, dayEndMs: end, timeZone: ZONE }).cards;
    expect(card.segments).toHaveLength(1);
    expect(card.startMs).toBe(Date.parse("2026-11-01T05:00:00Z"));
    expect(card.endMs).toBe(Date.parse("2026-11-01T07:00:00Z"));
  });
});
