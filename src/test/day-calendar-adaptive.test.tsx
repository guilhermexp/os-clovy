import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { layoutDayGrid } from "../components/activity-timeline/day-grid-layout";
import { WorkstreamGrid } from "../components/activity-timeline/WorkstreamGrid";
import { applyInterfaceLocale } from "../i18n/locale";
import type { WorkstreamDto } from "../lib/day-intelligence";

// Synthetic local day, independent of capture, providers and the owner's data.
const day = new Date(2026, 9, 12);
const at = (hour: number, minute = 0) =>
  new Date(day.getFullYear(), day.getMonth(), day.getDate(), hour, minute).getTime();
const work = (id: number, start: number, end: number): WorkstreamDto => ({
  id,
  title: `Frente ${id}: revisão detalhada do calendário e das informações de trabalho`,
  summary: "Synthetic fixture",
  minutes: 10,
  hours: [
    {
      hour: "2026-10-12T09",
      minutes: 10,
      locatedMinutes: 10,
      note: "Conferência das informações completas",
      periods: [{ startedAt: new Date(start).toISOString(), endedAt: new Date(end).toISOString() }],
    },
  ],
});
const sequential = [work(1, at(9), at(9, 10)), work(2, at(9, 30), at(9, 40))];
const options = (viewportHeight: number) => ({
  dayStartMs: at(0),
  dayEndMs: at(24),
  viewportHeight,
});

afterEach(cleanup);

describe("Adaptive calendar behavior", () => {
  it("uses the available height for a short day and keeps sequential cards in one column", () => {
    const layout = layoutDayGrid(sequential, options(600));
    expect(layout.height).toBeGreaterThan(90);
    expect(layout.height).toBeLessThanOrEqual(600);
    expect(layout.cards.map((card) => card.laneCount)).toEqual([1, 1]);
    const [first, second] = layout.cards;
    expect(first.top + first.height).toBeLessThan(second.top);
    expect(layout.toPx(at(9, 30)) - layout.toPx(at(9))).toBeGreaterThan(45);
    expect(first.startMs).toBe(at(9));
    expect(second.endMs).toBe(at(9, 40));
  });

  it("recalculates on resize, bounds growth and retains scrolling for a long day", () => {
    const short = layoutDayGrid(sequential, options(180));
    const tall = layoutDayGrid(sequential, options(600));
    expect(tall.cards[0].height).toBeGreaterThan(short.cards[0].height);
    expect(layoutDayGrid(sequential, options(600)).cards).toEqual(tall.cards);
    const huge = layoutDayGrid(sequential, options(10_000));
    expect(huge.toPx(at(9, 10)) - huge.toPx(at(9))).toBeLessThanOrEqual(90);
    const long = layoutDayGrid([work(3, at(0), at(24))], options(180));
    expect(long.height).toBeGreaterThan(180);
    expect(long.toPx(at(1)) - long.toPx(at(0))).toBeGreaterThanOrEqual(60);
    expect(Number.isFinite(layoutDayGrid(sequential, options(0)).height)).toBe(true);
  });

  it("separates real overlaps and minimum-height collisions instead of covering a card", () => {
    const overlapping = layoutDayGrid(
      [work(1, at(9), at(9, 10)), work(2, at(9, 5), at(9, 15))],
      options(600),
    );
    expect(overlapping.cards[0].lane).not.toBe(overlapping.cards[1].lane);
    const tiny = layoutDayGrid(
      [work(1, at(9), at(9, 1)), work(2, at(9, 2), at(9, 3))],
      options(180),
    );
    expect(tiny.cards[0].lane).not.toBe(tiny.cards[1].lane);
  });

  it("preserves one card, two sittings and active time independently of a pause", () => {
    const resumed = work(1, at(9), at(9, 10));
    resumed.minutes = 17;
    resumed.hours[0].minutes = 17;
    resumed.hours[0].locatedMinutes = 17;
    resumed.hours[0].periods?.push({
      startedAt: new Date(at(9, 30)).toISOString(),
      endedAt: new Date(at(9, 40)).toISOString(),
    });
    const layout = layoutDayGrid([resumed], options(600));
    expect(layout.cards).toHaveLength(1);
    expect(layout.cards[0].sittings).toBe(2);
    expect(layout.cards[0].activeMs).toBe(17 * 60_000);
    expect(layout.cards[0].segments.map(({ startMs, endMs }) => [startMs, endMs])).toEqual([
      [at(9), at(9, 10)],
      [at(9, 30), at(9, 40)],
    ]);
  });

  it.each([
    "en",
    "pt-BR",
  ] as const)("keeps complete accessible information and selection in %s", (locale) => {
    applyInterfaceLocale(locale);
    const select = vi.fn();
    render(
      <WorkstreamGrid
        layout={layoutDayGrid(sequential, options(180))}
        nowMs={null}
        selectedId={null}
        onSelect={select}
      />,
    );
    const card = screen.getByRole("button", { name: new RegExp(sequential[0].title) });
    expect(card.getAttribute("aria-label")).toContain("10");
    card.focus();
    expect(card).toHaveFocus();
    fireEvent.click(card);
    expect(select).toHaveBeenCalledWith(1);
  });
});
