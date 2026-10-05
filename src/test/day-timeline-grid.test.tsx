import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import type * as TauriCore from "@tauri-apps/api/core";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const invokeMock = vi.fn();

vi.mock("@tauri-apps/api/core", async (importOriginal) => ({
  ...(await importOriginal<typeof TauriCore>()),
  invoke: async (command: string, args?: unknown) => {
    if (command === "coding_agents_blocks") return [];
    return args !== undefined ? invokeMock(command, args) : invokeMock(command);
  },
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => () => {}),
  emit: vi.fn(async () => undefined),
}));

import { ActivityTimelineView } from "../components/activity-timeline/ActivityTimelineView";
import { layoutDayGrid } from "../components/activity-timeline/day-grid-layout";
import { formatDuration, formatTime } from "../components/activity-timeline/timeline-utils";
import { applyInterfaceLocale } from "../i18n/locale";
import type { ActivityTimelineDto, TimelineSessionDto } from "../lib/activity-timeline";
import { type DayIntelligenceDto, dayKey, type WorkstreamDto } from "../lib/day-intelligence";

// Fixture, not captured data: a local day with one workstream resumed after a
// break, one overlapping it, and one legacy record known only by hour.
const today = new Date();
const at = (hour: number, minute = 0, base = today) =>
  new Date(base.getFullYear(), base.getMonth(), base.getDate(), hour, minute);
const iso = (hour: number, minute = 0, base = today) => at(hour, minute, base).toISOString();
const hourKey = (hour: number, base = today) => `${dayKey(base)}T${String(hour).padStart(2, "0")}`;
const yesterday = new Date(today.getFullYear(), today.getMonth(), today.getDate() - 1);

const resumed: WorkstreamDto = {
  id: 1,
  title: "Timeline grid refactor",
  summary: "Moved the day into a vertical grid.",
  minutes: 38,
  hours: [
    {
      hour: hourKey(9),
      minutes: 24,
      note: "Drew the hour axis",
      periods: [{ startedAt: iso(9, 0), endedAt: iso(9, 30) }],
      locatedMinutes: 24,
    },
    {
      hour: hourKey(11),
      minutes: 14,
      note: "Placed the cards in lanes",
      periods: [{ startedAt: iso(11, 0), endedAt: iso(11, 20) }],
      locatedMinutes: 14,
    },
  ],
};

const overlapping: WorkstreamDto = {
  id: 2,
  title: "Release review",
  summary: "Reviewed the release checklist.",
  minutes: 30,
  hours: [
    {
      hour: hourKey(9),
      minutes: 30,
      note: "Checked the release notes",
      periods: [{ startedAt: iso(9, 10), endedAt: iso(9, 50) }],
      locatedMinutes: 30,
    },
  ],
};

const legacy: WorkstreamDto = {
  id: 3,
  title: "Inbox triage",
  summary: "Answered email.",
  minutes: 35,
  hours: [
    {
      hour: hourKey(14),
      minutes: 35,
      note: "Answered the support queue",
      periods: null,
      locatedMinutes: 0,
    },
  ],
};

const session: TimelineSessionDto = {
  id: 101,
  appName: "Visual Studio Code",
  bundleId: "com.microsoft.VSCode",
  contextKind: "workspace",
  context: "os-clovy",
  startedAt: iso(9, 0),
  endedAt: iso(9, 30),
  durationMs: 30 * 60_000,
  active: false,
  category: "coding",
  confidence: 0.9,
  windowTitle: "DayGrid.tsx - os-clovy",
};

const timeline: ActivityTimelineDto = {
  availability: "ready",
  message: null,
  captureEnabled: true,
  sessions: [session],
  gaps: [],
  stats: {
    focusedMs: 103 * 60_000,
    idleMs: 0,
    awayMs: 0,
    topApps: [
      { appName: "Visual Studio Code", bundleId: "com.microsoft.VSCode", durationMs: 60 * 60_000 },
      { appName: "Removed App", bundleId: "com.example.removed", durationMs: 20 * 60_000 },
    ],
    categories: [{ category: "coding", durationMs: 103 * 60_000 }],
  },
};

function dayOf(day: string, workstreams: WorkstreamDto[], provider: "ready" | "missing" = "ready") {
  return {
    availability: "ready",
    day,
    provider,
    embedder: "ready",
    running: false,
    summaryTime: "18:00",
    summary: null,
    workstreams,
    hourReports: [],
    panels: {
      focusedMs: 0,
      idleMs: 0,
      awayMs: 0,
      categories: [],
      topApps: [],
      hours: [],
      workstreams: [],
      meetingCount: 0,
      meetingMs: 0,
      codingAgentBlocks: 0,
      codingAgentActiveSeconds: 0,
    },
  } satisfies DayIntelligenceDto;
}

const ICON = "data:image/png;base64,iVBORw0KGgo=";

function serve(workstreamsByDay: Record<string, WorkstreamDto[]>, provider?: "ready" | "missing") {
  invokeMock.mockImplementation(async (command: string, args?: { request?: unknown }) => {
    const request = (args?.request ?? {}) as Record<string, unknown>;
    if (command === "activity_timeline") return timeline;
    if (command === "day_intelligence_day") {
      const day = String(request.day);
      return dayOf(day, workstreamsByDay[day] ?? [], provider);
    }
    if (command === "activity_app_icon") {
      return request.bundleId === "com.microsoft.VSCode" ? ICON : null;
    }
    if (command === "activity_timeline_session") {
      return { session, windows: [], textExcerpt: null };
    }
    return undefined;
  });
}

const card = (title: string) => screen.findByRole("button", { name: new RegExp(title) });

describe("Today grid of workstreams", () => {
  beforeEach(() => {
    applyInterfaceLocale("en");
    serve({ [dayKey(today)]: [resumed, overlapping, legacy] });
  });

  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  it("groups a resumed workstream into one card with its sittings and active time, not its span", async () => {
    render(<ActivityTimelineView />);
    const grid = await card("Timeline grid refactor");

    expect(screen.getAllByRole("button", { name: /Timeline grid refactor/ })).toHaveLength(1);
    expect(within(grid).getAllByTestId("workstream-sitting")).toHaveLength(2);
    expect(within(grid).getByText("2 sittings")).toBeInTheDocument();
    // 38 active minutes, while the card spans 9:00 to 11:20.
    expect(within(grid).getByText(formatDuration(38 * 60_000, "en"))).toBeInTheDocument();
    expect(
      within(grid).getByText(`${formatTime(iso(9, 0))} - ${formatTime(iso(11, 20))}`),
    ).toBeInTheDocument();
  });

  it("puts overlapping workstreams side by side so both stay selectable", async () => {
    render(<ActivityTimelineView />);
    const first = await card("Timeline grid refactor");
    const second = await card("Release review");

    expect(first.style.left).not.toBe(second.style.left);
    fireEvent.click(second);
    expect(
      within(screen.getByRole("region", { name: "Workstream details" })).getByRole("heading", {
        name: "Release review",
      }),
    ).toBeInTheDocument();
  });

  it("shows a legacy record by hour, without invented minutes", async () => {
    render(<ActivityTimelineView />);
    const inbox = await card("Inbox triage");

    expect(within(inbox).getByText("By hour")).toBeInTheDocument();
    expect(within(inbox).queryByText(/sitting/)).not.toBeInTheDocument();
    expect(inbox.textContent).not.toContain(formatTime(iso(14, 0)));
  });

  it("marks a partly located workstream by hour and lists its minutes without an exact time", async () => {
    serve({
      [dayKey(today)]: [
        {
          id: 5,
          title: "Partly located work",
          summary: "",
          minutes: 30,
          hours: [
            {
              hour: hourKey(10),
              minutes: 30,
              note: "Reviewed the plan",
              periods: [{ startedAt: iso(10, 10), endedAt: iso(10, 20) }],
              locatedMinutes: 10,
            },
          ],
        },
      ],
    });
    render(<ActivityTimelineView />);
    const partial = await card("Partly located work");
    expect(within(partial).getByText("By hour")).toBeInTheDocument();

    fireEvent.click(partial);
    const detail = screen.getByRole("region", { name: "Workstream details" });
    expect(
      within(detail).getByText(
        `${formatDuration(20 * 60_000, "en")} in this hour without an exact time`,
      ),
    ).toBeVisible();
    // Unlocated minutes may sit anywhere in the hour: no break is claimed.
    expect(within(detail).queryByText(/Break/)).not.toBeInTheDocument();
  });

  it("opens the workstream in the side panel with sittings, breaks and what was done, then goes back", async () => {
    render(<ActivityTimelineView />);
    const grid = await card("Timeline grid refactor");
    expect(screen.getByText("Top apps")).toBeInTheDocument();

    fireEvent.click(grid);

    const detail = screen.getByRole("region", { name: "Workstream details" });
    expect(grid).toHaveAttribute("aria-pressed", "true");
    expect(within(detail).getByRole("heading", { name: "Timeline grid refactor" })).toBeVisible();
    expect(within(detail).getByText(`${formatDuration(38 * 60_000, "en")} active`)).toBeVisible();
    expect(within(detail).getAllByTestId("workstream-period")).toHaveLength(2);
    expect(within(detail).getByText(`Break · ${formatDuration(90 * 60_000, "en")}`)).toBeVisible();
    // Session bounds say where the work was, not how long it took: the
    // 9:00 to 9:30 session is not shown as 30 minutes of this workstream.
    for (const period of within(detail).getAllByTestId("workstream-period")) {
      expect(period.textContent).not.toContain(formatDuration(30 * 60_000, "en"));
      expect(period.textContent).not.toContain(formatDuration(20 * 60_000, "en"));
    }
    expect(within(detail).getByText("Drew the hour axis")).toBeVisible();
    expect(within(detail).getByText("Placed the cards in lanes")).toBeVisible();
    expect(screen.queryByText("Top apps")).not.toBeInTheDocument();

    fireEvent.click(within(detail).getByRole("button", { name: "Back to the day overview" }));
    expect(screen.queryByRole("region", { name: "Workstream details" })).not.toBeInTheDocument();
    expect(screen.getByText("Top apps")).toBeInTheDocument();

    fireEvent.click(grid);
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("region", { name: "Workstream details" })).not.toBeInTheDocument();
  });

  it("drops the selection when the day changes and reads the other day's workstreams", async () => {
    serve({
      [dayKey(today)]: [resumed],
      [dayKey(yesterday)]: [
        {
          ...legacy,
          id: 9,
          title: "Yesterday planning",
          hours: [{ ...legacy.hours[0], hour: hourKey(14, yesterday) }],
        },
      ],
    });
    render(<ActivityTimelineView />);
    fireEvent.click(await card("Timeline grid refactor"));
    expect(screen.getByRole("region", { name: "Workstream details" })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Previous day" }));

    expect(await card("Yesterday planning")).toBeInTheDocument();
    expect(screen.queryByRole("region", { name: "Workstream details" })).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /Timeline grid refactor/ }),
    ).not.toBeInTheDocument();
    expect(invokeMock).toHaveBeenCalledWith("day_intelligence_day", {
      request: { day: dayKey(yesterday) },
    });
  });

  it("keeps the captured sessions reachable when no workstream exists yet", async () => {
    serve({}, "missing");
    render(<ActivityTimelineView />);

    expect(await screen.findByText(/Workstreams need an activity provider/)).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /Timeline grid refactor/ }),
    ).not.toBeInTheDocument();
    fireEvent.click(await screen.findByText("DayGrid.tsx - os-clovy"));
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("activity_timeline_session", {
        request: { id: 101 },
      }),
    );
  });

  it("shows each top app with its own local icon and a neutral fallback when none is readable", async () => {
    render(<ActivityTimelineView />);
    const list = await screen.findByRole("list", { name: "Top apps" });
    const [code, removed] = within(list).getAllByRole("listitem");

    // The icon is decorative (alt=""): the app's name is written beside it.
    await waitFor(() => expect(code.querySelector("img")).toHaveAttribute("src", ICON));
    expect(within(code).getByText("Visual Studio Code")).toBeInTheDocument();
    // Identity, not name: only the bundle id that has an icon gets one.
    await act(async () => {});
    expect(removed.querySelector("img")).toBeNull();
    expect(within(removed).getByTestId("app-icon-fallback")).toBeInTheDocument();
    expect(within(removed).getByText("Removed App")).toBeInTheDocument();
    expect(within(removed).getByText(formatDuration(20 * 60_000, "en"))).toBeInTheDocument();
  });
});

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
