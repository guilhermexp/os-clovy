import { describe, expect, it } from "vitest";
import { formatDuration } from "../components/activity-timeline/timeline-utils";
import { layoutWorkstreamTimeline } from "../components/activity-timeline/workstream-timeline-layout";
import { dayKey, type WorkstreamDto } from "../lib/day-intelligence";

/*
 * Active minutes per card when a workstream is split at long pauses: each
 * card gets the located minutes of the measured periods inside it, even when
 * one hour report holds periods on both sides of a pause. Minutes no period
 * located stay with the hour's by-hour part (dashed bar, "By hour"), and the
 * cards always add up to the workstream's total.
 */

const day = new Date(2026, 5, 15);
const MINUTE = 60_000;
const at = (clock: string) => {
  const [hour, minute] = clock.split(":").map(Number);
  return new Date(day.getFullYear(), day.getMonth(), day.getDate(), hour, minute).getTime();
};
const iso = (clock: string) => new Date(at(clock)).toISOString();
const hourKey = (hour: string) => `${dayKey(day)}T${hour}`;

const layout = (workstreams: WorkstreamDto[]) =>
  layoutWorkstreamTimeline(workstreams, {
    dayStartMs: at("00:00"),
    dayEndMs: at("00:00") + 24 * 60 * MINUTE,
    canvasHeight: 903,
  });

const summary = (workstreams: WorkstreamDto[]) =>
  layout(workstreams).cards.map((card) => ({
    range: [
      new Date(card.startMs).toTimeString().slice(0, 5),
      new Date(card.endMs).toTimeString().slice(0, 5),
    ],
    minutes: card.activeMs / MINUTE,
    approximate: card.approximate,
  }));

describe("Active minutes of the cards of a split workstream", () => {
  it("gives each side of a long pause inside one hour the minutes of its own periods", () => {
    // One hour report, 14 located minutes: 13:00 to 13:10 and 13:56 to 14:00.
    const split: WorkstreamDto = {
      id: 1,
      title: "Split inside one hour",
      summary: "",
      minutes: 14,
      hours: [
        {
          hour: hourKey("13"),
          minutes: 14,
          locatedMinutes: 14,
          note: "Worked both before and after the pause",
          periods: [
            { startedAt: iso("13:00"), endedAt: iso("13:10") },
            { startedAt: iso("13:56"), endedAt: iso("14:00") },
          ],
        },
      ],
    };
    expect(summary([split])).toEqual([
      { range: ["13:00", "13:10"], minutes: 10, approximate: false },
      { range: ["13:56", "14:00"], minutes: 4, approximate: false },
    ]);
    const cards = layout([split]).cards;
    expect(cards.reduce((sum, card) => sum + card.activeMs, 0)).toBe(14 * MINUTE);
    expect(formatDuration(cards[1].activeMs, "en")).toBe("4m");
    // The hour's note goes to the stretch with most of its time.
    expect(cards.map((card) => card.notes)).toEqual([
      ["Worked both before and after the pause"],
      [],
    ]);
  });

  it("keeps minutes no period located in a by-hour stretch, flagged approximate", () => {
    const partly: WorkstreamDto = {
      id: 2,
      title: "Partly located",
      summary: "",
      minutes: 42,
      hours: [
        // Located only: a precise card of its own.
        {
          hour: hourKey("09"),
          minutes: 10,
          locatedMinutes: 10,
          note: "",
          periods: [{ startedAt: iso("09:00"), endedAt: iso("09:10") }],
        },
        // 5 of 20 minutes located: the other 15 sit somewhere in 15:00 to 16:00.
        {
          hour: hourKey("15"),
          minutes: 20,
          locatedMinutes: 5,
          note: "",
          periods: [{ startedAt: iso("15:10"), endedAt: iso("15:15") }],
        },
        // A legacy hour known only by hour.
        { hour: hourKey("18"), minutes: 12, locatedMinutes: 0, note: "", periods: null },
      ],
    };
    expect(summary([partly])).toEqual([
      { range: ["09:00", "09:10"], minutes: 10, approximate: false },
      { range: ["15:00", "16:00"], minutes: 20, approximate: true },
      { range: ["18:00", "19:00"], minutes: 12, approximate: true },
    ]);
    const cards = layout([partly]).cards;
    // The hour's window (dashed, from 15:00) and its measured period at 15:10.
    expect(cards[1].bars.map((bar) => bar.approximate)).toEqual([true, false]);
    expect(cards[2].bars.map((bar) => bar.approximate)).toEqual([true]);
    expect(cards.reduce((sum, card) => sum + card.activeMs, 0)).toBe(42 * MINUTE);
  });

  it("splits one hour's located minutes by its periods and keeps whole minutes adding up", () => {
    // 7 located minutes over periods of 3 and 2 minutes on both sides of a pause.
    const uneven: WorkstreamDto = {
      id: 3,
      title: "Uneven",
      summary: "",
      minutes: 7,
      hours: [
        {
          hour: hourKey("10"),
          minutes: 7,
          locatedMinutes: 7,
          note: "",
          periods: [
            { startedAt: iso("10:00"), endedAt: iso("10:03") },
            { startedAt: iso("10:55"), endedAt: iso("10:57") },
          ],
        },
      ],
    };
    // 7 x 3/5 = 4.2 and 7 x 2/5 = 2.8: whole minutes by largest remainder.
    expect(summary([uneven]).map((card) => card.minutes)).toEqual([4, 3]);
  });
});
