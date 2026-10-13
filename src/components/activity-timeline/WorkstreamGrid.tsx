import { useCallback, useEffect, useRef } from "react";
import { type InterfaceLocale, useLocale, useT } from "../../i18n";
import { useScrollFade } from "../../lib/use-scroll-fade";
import type { DayGridLayout } from "./day-grid-layout";
import { formatDuration, formatTime } from "./timeline-utils";

type WorkstreamGridProps = {
  layout: DayGridLayout;
  /** Today's current time, for the now line; null on other days. */
  nowMs: number | null;
  selectedId: number | null;
  onSelect: (id: number) => void;
  viewportRef?: (element: HTMLDivElement | null) => void;
};

/** "2 PM" / "14h": an hour known only by its whole local hour. */
function formatHourOnly(ms: number, locale: InterfaceLocale): string {
  const date = new Date(ms);
  if (locale === "pt-BR") return `${date.getHours()}h`;
  return new Intl.DateTimeFormat("en-US", { hour: "numeric" }).format(date);
}

/**
 * Start and end of a span for "{start} - {end}". A span with any record known
 * only by hour shows whole hours, never minutes that were not measured.
 */
export function spanLabels(
  span: { startMs: number; endMs: number; approximate: boolean },
  locale: InterfaceLocale,
): { start: string; end: string } {
  if (span.approximate) {
    return {
      start: formatHourOnly(span.startMs, locale),
      end: formatHourOnly(span.endMs, locale),
    };
  }
  return {
    start: formatTime(new Date(span.startMs).toISOString(), locale),
    end: formatTime(new Date(span.endMs).toISOString(), locale),
  };
}

/**
 * The day as a vertical grid of real local hours with one card per
 * workstream: the card spans its first to last sitting, small marks show the
 * sittings, and the active time printed on it comes from the data, not from
 * the card's height. Overlapping workstreams share the width in lanes.
 */
export function WorkstreamGrid({
  layout,
  nowMs,
  selectedId,
  onSelect,
  viewportRef,
}: WorkstreamGridProps) {
  const t = useT();
  const locale = useLocale();
  const scrollerRef = useRef<HTMLDivElement | null>(null);
  const setScroller = useCallback(
    (element: HTMLDivElement | null) => {
      scrollerRef.current = element;
      viewportRef?.(element);
    },
    [viewportRef],
  );
  const fade = useScrollFade(scrollerRef);
  const activeMs = layout.cards.reduce((total, card) => total + card.activeMs, 0);
  const firstCardTop = layout.cards[0]?.top ?? 0;
  const firstCardStart = layout.cards[0]?.startMs;
  const openedStart = useRef<number | undefined>();

  // Opening a day starts at its first workstream, not at the top of the axis.
  useEffect(() => {
    if (openedStart.current === firstCardStart) return;
    openedStart.current = firstCardStart;
    scrollerRef.current?.scrollTo?.({ top: Math.max(0, firstCardTop - 24) });
  }, [firstCardStart, firstCardTop]);

  return (
    <section
      className="day-grid"
      aria-label={t("activity.grid.label")}
      data-has-selection={selectedId !== null || undefined}
    >
      <header className="day-grid-header">
        <h3 className="day-grid-title">
          {t("activity.grid.count", { count: layout.cards.length })}
        </h3>
        <span className="day-grid-total">
          {t("activity.grid.active", { duration: formatDuration(activeMs, locale) })}
        </span>
      </header>
      <div className="day-grid-scroller scroll-fade-mask" ref={setScroller} {...fade.props}>
        <div className="day-grid-body" style={{ height: layout.height }}>
          <div className="day-grid-axis" aria-hidden="true">
            {layout.ticks.map((tick) => (
              <div key={tick.ms} className="day-grid-tick" style={{ top: tick.top }}>
                <span className="day-grid-tick-label">{tick.label}</span>
              </div>
            ))}
          </div>
          <div className="day-grid-lanes">
            {nowMs !== null ? (
              <div
                className="day-grid-now"
                style={{ top: layout.toPx(nowMs) }}
                title={t("activity.grid.now")}
                aria-hidden="true"
              />
            ) : null}
            {layout.cards.map((card) => {
              const { workstream } = card;
              const selected = workstream.id === selectedId;
              const range = t("activity.detail.timeRange", spanLabels(card, locale));
              const active = formatDuration(card.activeMs, locale);
              const notes = workstream.hours.filter((hour) => hour.note);
              return (
                <button
                  key={workstream.id}
                  type="button"
                  className="day-grid-card"
                  aria-pressed={selected}
                  aria-label={[
                    workstream.title,
                    t("activity.grid.active", { duration: active }),
                    range,
                    card.approximate ? t("activity.grid.byHour") : null,
                  ]
                    .filter(Boolean)
                    .join(", ")}
                  onClick={() => onSelect(workstream.id)}
                  style={{
                    top: card.top,
                    height: card.height,
                    left: `${(card.lane / card.laneCount) * 100}%`,
                    width: `calc(${100 / card.laneCount}% - var(--sp-2))`,
                  }}
                >
                  <span className="day-grid-card-rail" aria-hidden="true">
                    {card.segments.map((segment) => {
                      const top = layout.toPx(segment.startMs) - card.top;
                      return (
                        <span
                          key={`${segment.kind}-${segment.startMs}`}
                          className="day-grid-sitting"
                          data-approximate={segment.approximate || undefined}
                          data-testid={segment.approximate ? undefined : "workstream-sitting"}
                          style={{
                            top,
                            height: Math.max(layout.toPx(segment.endMs) - top - card.top, 3),
                          }}
                        />
                      );
                    })}
                  </span>
                  <span className="day-grid-card-body">
                    <span className="day-grid-card-title">{workstream.title}</span>
                    {/* In priority order (active time, by-hour badge, range): on a
                        narrow card the items that do not fit wrap out of view
                        whole, and the label and the details still carry them. */}
                    <span className="day-grid-card-meta">
                      <span className="day-grid-card-active">{active}</span>
                      {card.approximate ? (
                        <span className="day-grid-card-badge" title={t("activity.grid.byHourHint")}>
                          {t("activity.grid.byHour")}
                        </span>
                      ) : null}
                      <span>{range}</span>
                      {!card.approximate && card.sittings > 1 ? (
                        <span>{t("activity.grid.sittings", { count: card.sittings })}</span>
                      ) : null}
                    </span>
                    {notes.length > 0 ? (
                      <span className="day-grid-card-notes">
                        <span className="day-grid-card-note-lines">
                          {notes.map((hour) => (
                            <span key={hour.hour} className="day-grid-card-note">
                              {hour.note}
                            </span>
                          ))}
                        </span>
                      </span>
                    ) : null}
                  </span>
                </button>
              );
            })}
          </div>
        </div>
      </div>
    </section>
  );
}
