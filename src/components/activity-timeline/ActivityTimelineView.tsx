import { useEffect, useState } from "react";
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
import { TimelineDayStrip } from "./TimelineDayStrip";
import { TimelineSearch } from "./TimelineSearch";
import { TimelineSessionDetail } from "./TimelineSessionDetail";
import { TimelineSessionList } from "./TimelineSessionList";
import { TimelineStats } from "./TimelineStats";
import { formatDayHeader } from "./timeline-utils";

export type ActivityTimelineViewProps = {
  onNavigateToSettings?: () => void;
};

export function ActivityTimelineView({ onNavigateToSettings }: ActivityTimelineViewProps) {
  const t = useT();
  const locale = useLocale();

  const [selectedDate, setSelectedDate] = useState<Date>(() => new Date());
  const [timeline, setTimeline] = useState<ActivityTimelineDto | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [selectedSessionId, setSelectedSessionId] = useState<number | null>(null);
  const [nowMs, setNowMs] = useState<number>(() => Date.now());

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
        setTimeline(res);
        setError(null);
      } catch (err: unknown) {
        if (!active) return;
        const msg =
          err && typeof err === "object" && "message" in err && typeof err.message === "string"
            ? err.message
            : "Failed to load activity timeline";
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
  }, [selectedDate, isToday]);

  const handlePrevDay = () => {
    setSelectedDate((prev) => new Date(prev.getFullYear(), prev.getMonth(), prev.getDate() - 1));
  };

  const handleNextDay = () => {
    if (isToday) return;
    setSelectedDate((prev) => new Date(prev.getFullYear(), prev.getMonth(), prev.getDate() + 1));
  };

  const handleJumpToday = () => {
    setSelectedDate(new Date());
  };

  const handleSelectSearchResult = (result: TimelineSearchResultDto) => {
    const resultDate = new Date(result.seenAt);
    setSelectedDate(resultDate);
    setSelectedSessionId(result.sessionId);
  };

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
          <TimelineSearch onSelectResult={handleSelectSearchResult} />
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
                onClick={onNavigateToSettings}
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
                  onClick={onNavigateToSettings}
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
                  message: timeline?.message || error || "Unknown error",
                })}
              </span>
              {onNavigateToSettings ? (
                <button
                  type="button"
                  className="timeline-notice-action-btn"
                  onClick={onNavigateToSettings}
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

          {/* 24-hour horizontal Day Strip */}
          {timeline ? (
            <TimelineDayStrip
              sessions={timeline.sessions}
              gaps={timeline.gaps}
              dayStartMs={dayStartMs}
              dayEndMs={dayEndMs}
              from={range.from}
              to={range.to}
              selectedSessionId={selectedSessionId}
              onSelectSession={(id) => setSelectedSessionId(id)}
              nowMs={nowMs}
              isToday={isToday}
            />
          ) : null}

          {/* Bottom Area: Chronological Sessions List + Stats Panel + Detail Panel */}
          <div className="timeline-lower-grid">
            <div className="timeline-list-column">
              {timeline ? (
                <TimelineSessionList
                  sessions={timeline.sessions}
                  gaps={timeline.gaps}
                  selectedSessionId={selectedSessionId}
                  onSelectSession={(id) => setSelectedSessionId(id)}
                />
              ) : null}
            </div>

            {selectedSessionId !== null ? (
              <div className="timeline-detail-column">
                <TimelineSessionDetail
                  sessionId={selectedSessionId}
                  onClose={() => setSelectedSessionId(null)}
                />
              </div>
            ) : timeline?.stats ? (
              <div className="timeline-stats-column">
                <TimelineStats stats={timeline.stats} />
              </div>
            ) : null}
          </div>
        </div>
      )}
    </main>
  );
}
