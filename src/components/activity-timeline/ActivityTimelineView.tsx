import { useEffect, useMemo, useState } from "react";
import { IconChevronLeftSmall } from "central-icons/IconChevronLeftSmall";
import { IconChevronRightSmall } from "central-icons/IconChevronRightSmall";
import { IconCircleInfo } from "central-icons/IconCircleInfo";
import { IconLock } from "central-icons/IconLock";
import { IconSettingsGear4 } from "central-icons/IconSettingsGear4";
import { useLocale, useT } from "../../i18n";
import {
  activityTimeline,
  dayRange,
  onActivityTimelineUpdated,
  type ActivityTimelineDto,
  type TimelineSearchResultDto,
} from "../../lib/activity-timeline";
import {
  dateOfDayKey,
  dayKey,
  onTodayOpenRequest,
  takePendingTodayOpen,
} from "../../lib/day-intelligence";
import { DaySummaryPanel } from "./DaySummaryPanel";
import { layoutDayGrid } from "./day-grid-layout";
import { TimelineDayStrip } from "./TimelineDayStrip";
import { TimelineSearch } from "./TimelineSearch";
import { TimelineSessionDetail } from "./TimelineSessionDetail";
import { TimelineSessionList } from "./TimelineSessionList";
import { TimelineStats } from "./TimelineStats";
import { formatDayHeader } from "./timeline-utils";
import { useDayIntelligence } from "./use-day-intelligence";
import { WorkstreamDetail } from "./WorkstreamDetail";
import { WorkstreamGrid } from "./WorkstreamGrid";

export type ActivityTimelineViewProps = {
  onNavigateToSettings?: (tab?: "activity" | "models") => void;
};

type TodaySection = "timeline" | "summary";
/** The timeline as workstreams on the hour grid, or as captured sessions. */
type TimelineMode = "workstreams" | "sessions";
/** What the side panel shows instead of the day overview. */
type Selection = { kind: "session"; id: number } | { kind: "workstream"; id: number } | null;

export function ActivityTimelineView({ onNavigateToSettings }: ActivityTimelineViewProps) {
  const t = useT();
  const locale = useLocale();

  // A notification click may have asked for a day's summary before this
  // view mounted (it is lazy-loaded); later clicks arrive as requests.
  const [initialOpen] = useState(() => takePendingTodayOpen());
  const [selectedDate, setSelectedDate] = useState<Date>(
    () => (initialOpen && dateOfDayKey(initialOpen)) || new Date(),
  );
  const [section, setSection] = useState<TodaySection>(initialOpen ? "summary" : "timeline");
  const [timeline, setTimeline] = useState<ActivityTimelineDto | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [selection, setSelection] = useState<Selection>(null);
  const [mode, setMode] = useState<TimelineMode>("workstreams");
  const [nowMs, setNowMs] = useState<number>(() => Date.now());

  useEffect(
    () =>
      onTodayOpenRequest((day) => {
        const date = dateOfDayKey(day);
        if (!date) return;
        setSelection(null);
        setSelectedDate(date);
        setSection("summary");
      }),
    [],
  );

  const today = new Date();
  const isToday =
    selectedDate.getFullYear() === today.getFullYear() &&
    selectedDate.getMonth() === today.getMonth() &&
    selectedDate.getDate() === today.getDate();

  // Keep nowMs current every 10 seconds for the timeline now-cursor
  useEffect(() => {
    const timer = setInterval(() => {
      setNowMs(Date.now());
    }, 10_000);
    return () => clearInterval(timer);
  }, []);

  // Compute local day bounds
  const range = dayRange(selectedDate);
  const dayStartMs = new Date(range.from).getTime();
  const dayEndMs = new Date(range.to).getTime();

  // Load timeline data for the selected day range
  useEffect(() => {
    let active = true;
    const currentRange = dayRange(selectedDate);

    const load = async () => {
      try {
        const res = await activityTimeline(currentRange);
        if (!active) return;
        setTimeline(res);
        setError(null);
      } catch (err: unknown) {
        if (!active) return;
        const msg =
          err && typeof err === "object" && "message" in err && typeof err.message === "string"
            ? err.message
            : t("activity.state.unknownError");
        setError(msg);
      } finally {
        if (active) setLoading(false);
      }
    };

    void load();

    // Listen to background ETL completion events
    let unlisten: (() => void) | undefined;
    void onActivityTimelineUpdated(() => {
      void load();
    }).then((fn) => {
      unlisten = fn;
    });

    // When viewing today, also refetch every 30s so the active session grows
    let intervalTimer: number | undefined;
    if (isToday) {
      intervalTimer = window.setInterval(() => {
        void load();
      }, 30_000);
    }

    return () => {
      active = false;
      unlisten?.();
      clearInterval(intervalTimer);
    };
  }, [selectedDate, isToday, t]);

  const day = dayKey(selectedDate);
  const intelligence = useDayIntelligence(day);
  const workstreams = intelligence.data?.workstreams;
  const gridNowMs = isToday ? nowMs : null;
  const layout = useMemo(
    () =>
      layoutDayGrid(workstreams ?? [], {
        dayStartMs,
        dayEndMs,
        nowMs: gridNowMs,
      }),
    [workstreams, dayStartMs, dayEndMs, gridNowMs],
  );
  const selectedCard =
    selection?.kind === "workstream"
      ? layout.cards.find((card) => card.workstream.id === selection.id)
      : undefined;

  // Escape returns the side panel to the day overview.
  useEffect(() => {
    if (selection === null) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") setSelection(null);
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [selection]);

  // A selection belongs to one day: changing the day by hand clears it.
  const handlePrevDay = () => {
    setSelection(null);
    setSelectedDate((prev) => new Date(prev.getFullYear(), prev.getMonth(), prev.getDate() - 1));
  };

  const handleNextDay = () => {
    if (isToday) return;
    setSelection(null);
    setSelectedDate((prev) => new Date(prev.getFullYear(), prev.getMonth(), prev.getDate() + 1));
  };

  const handleJumpToday = () => {
    setSelection(null);
    setSelectedDate(new Date());
  };

  const handleSelectSearchResult = (result: TimelineSearchResultDto) => {
    const resultDate = new Date(result.seenAt);
    setSelectedDate(resultDate);
    setSelection({ kind: "session", id: result.sessionId });
  };

  const selectSession = (id: number) => setSelection({ kind: "session", id });
  const selectedSessionId = selection?.kind === "session" ? selection.id : null;

  // Header date display string
  const dateHeading = isToday
    ? t("activity.timeline.title")
    : formatDayHeader(selectedDate, locale);

  const availability = timeline?.availability;

  return (
    <main className="activity-timeline-view" aria-label={t("activity.timeline.title")}>
      {/* Top Header */}
      <header className="timeline-header">
        <div className="timeline-header-day-nav">
          <button
            type="button"
            className="timeline-nav-arrow-btn"
            onClick={handlePrevDay}
            aria-label={t("activity.timeline.prevDay")}
          >
            <IconChevronLeftSmall size={16} />
          </button>

          <h2 className="timeline-day-title">{dateHeading}</h2>

          <button
            type="button"
            className="timeline-nav-arrow-btn"
            onClick={handleNextDay}
            disabled={isToday}
            aria-label={t("activity.timeline.nextDay")}
          >
            <IconChevronRightSmall size={16} />
          </button>

          {!isToday ? (
            <button type="button" className="timeline-jump-today-btn" onClick={handleJumpToday}>
              {t("activity.timeline.jumpToday")}
            </button>
          ) : null}
        </div>

        <div className="timeline-header-actions">
          <div
            className="timeline-section-tabs"
            role="tablist"
            aria-label={t("dayIntelligence.tabs.label")}
          >
            {(["timeline", "summary"] as const).map((id) => (
              <button
                key={id}
                type="button"
                role="tab"
                className="timeline-section-tab"
                aria-selected={section === id}
                data-active={section === id || undefined}
                onClick={() => setSection(id)}
              >
                {id === "timeline"
                  ? t("dayIntelligence.tab.timeline")
                  : t("dayIntelligence.tab.summary")}
              </button>
            ))}
          </div>
          <TimelineSearch
            onSelectResult={(result) => {
              setSection("timeline");
              handleSelectSearchResult(result);
            }}
          />
        </div>
      </header>

      {/* Main Body depending on state */}
      {loading && !timeline ? (
        <div className="timeline-loading-state">
          <span className="dot-spinner" aria-hidden="true" />
        </div>
      ) : availability === "neverEnabled" ? (
        <section className="timeline-empty-state" aria-labelledby="timeline-empty-title">
          <div className="timeline-empty-card">
            <div className="timeline-empty-icon-wrap" aria-hidden="true">
              <IconLock size={28} />
            </div>
            <h3 id="timeline-empty-title" className="timeline-empty-title">
              {t("activity.emptyState.title")}
            </h3>
            <p className="timeline-empty-description">{t("activity.emptyState.description")}</p>
            {onNavigateToSettings ? (
              <button
                type="button"
                className="primary-action primary-solid timeline-empty-action-btn"
                onClick={() => onNavigateToSettings("activity")}
              >
                {t("activity.emptyState.turnOn")}
              </button>
            ) : null}
          </div>
        </section>
      ) : (
        <div className="timeline-content-body">
          {/* Banner notices for error, keyMissing, or capture disabled */}
          {availability === "keyMissing" ? (
            <div className="timeline-notice-banner timeline-notice-warning" role="alert">
              <IconCircleInfo size={16} aria-hidden="true" />
              <span className="timeline-notice-text">{t("activity.state.keyMissingNotice")}</span>
              {onNavigateToSettings ? (
                <button
                  type="button"
                  className="timeline-notice-action-btn"
                  onClick={() => onNavigateToSettings("activity")}
                >
                  <IconSettingsGear4 size={14} aria-hidden="true" />
                  {t("activity.state.openSettings")}
                </button>
              ) : null}
            </div>
          ) : availability === "error" || error ? (
            <div className="timeline-notice-banner timeline-notice-error" role="alert">
              <IconCircleInfo size={16} aria-hidden="true" />
              <span className="timeline-notice-text">
                {t("activity.state.errorNotice", {
                  message: timeline?.message || error || t("activity.state.unknownError"),
                })}
              </span>
              {onNavigateToSettings ? (
                <button
                  type="button"
                  className="timeline-notice-action-btn"
                  onClick={() => onNavigateToSettings("activity")}
                >
                  <IconSettingsGear4 size={14} aria-hidden="true" />
                  {t("activity.state.openSettings")}
                </button>
              ) : null}
            </div>
          ) : timeline && !timeline.captureEnabled ? (
            <div className="timeline-notice-banner timeline-notice-muted">
              <IconCircleInfo size={15} aria-hidden="true" />
              <span className="timeline-notice-text">
                {t("activity.timeline.capturePausedHint")}
              </span>
            </div>
          ) : null}

          {section === "summary" ? (
            <DaySummaryPanel day={day} onNavigateToSettings={onNavigateToSettings} />
          ) : (
            /* Main column (grid of workstreams, or captured sessions) + side panel */
            <div className="timeline-lower-grid">
              <div className="timeline-list-column">
                <fieldset className="timeline-mode-switch" aria-label={t("activity.view.label")}>
                  {(["workstreams", "sessions"] as const).map((id) => (
                    <button
                      key={id}
                      type="button"
                      className="timeline-section-tab"
                      aria-pressed={mode === id}
                      data-active={mode === id || undefined}
                      onClick={() => setMode(id)}
                    >
                      {id === "workstreams"
                        ? t("activity.view.workstreams")
                        : t("activity.view.sessions")}
                    </button>
                  ))}
                </fieldset>
                {mode === "sessions" ? (
                  timeline ? (
                    <>
                      <TimelineDayStrip
                        sessions={timeline.sessions}
                        gaps={timeline.gaps}
                        dayStartMs={dayStartMs}
                        dayEndMs={dayEndMs}
                        from={range.from}
                        to={range.to}
                        selectedSessionId={selectedSessionId}
                        onSelectSession={selectSession}
                        nowMs={nowMs}
                        isToday={isToday}
                      />
                      <TimelineSessionList
                        sessions={timeline.sessions}
                        gaps={timeline.gaps}
                        selectedSessionId={selectedSessionId}
                        onSelectSession={selectSession}
                      />
                    </>
                  ) : null
                ) : layout.cards.length > 0 ? (
                  <WorkstreamGrid
                    layout={layout}
                    nowMs={gridNowMs}
                    selectedId={selectedCard ? selectedCard.workstream.id : null}
                    onSelect={(id) =>
                      setSelection((current) =>
                        current?.kind === "workstream" && current.id === id
                          ? null
                          : { kind: "workstream", id },
                      )
                    }
                  />
                ) : !intelligence.loaded ? (
                  <div className="timeline-loading-state">
                    <span className="dot-spinner" aria-hidden="true" />
                  </div>
                ) : (
                  /* No workstream yet: say why, and keep the captured sessions at hand. */
                  <>
                    {timeline && timeline.sessions.length > 0 ? (
                      <div className="timeline-notice-banner timeline-notice-muted">
                        <IconCircleInfo size={15} aria-hidden="true" />
                        <span className="timeline-notice-text">
                          {intelligence.error
                            ? t("activity.grid.loadError", { message: intelligence.error })
                            : intelligence.data?.provider === "missing" ||
                                intelligence.data?.provider === "insufficient"
                              ? t("activity.grid.noProvider")
                              : t("activity.grid.pending")}
                        </span>
                        {!intelligence.error &&
                        intelligence.data?.provider !== "ready" &&
                        intelligence.data &&
                        onNavigateToSettings ? (
                          <button
                            type="button"
                            className="timeline-notice-action-btn"
                            onClick={() => onNavigateToSettings("models")}
                          >
                            <IconSettingsGear4 size={14} aria-hidden="true" />
                            {t("activity.grid.chooseProvider")}
                          </button>
                        ) : null}
                      </div>
                    ) : null}
                    {timeline ? (
                      <TimelineSessionList
                        sessions={timeline.sessions}
                        gaps={timeline.gaps}
                        selectedSessionId={selectedSessionId}
                        onSelectSession={selectSession}
                      />
                    ) : null}
                  </>
                )}
              </div>

              {selectedSessionId !== null ? (
                <div className="timeline-detail-column">
                  <TimelineSessionDetail
                    sessionId={selectedSessionId}
                    onClose={() => setSelection(null)}
                  />
                </div>
              ) : selectedCard ? (
                <div className="timeline-detail-column">
                  <WorkstreamDetail card={selectedCard} onBack={() => setSelection(null)} />
                </div>
              ) : timeline?.stats ? (
                <div className="timeline-stats-column">
                  <TimelineStats stats={timeline.stats} />
                </div>
              ) : null}
            </div>
          )}
        </div>
      )}
    </main>
  );
}
