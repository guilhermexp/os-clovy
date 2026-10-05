//! Time decisions, all pure so a simulated clock can drive them: local days
//! and hours, which completed hours are due for a report, when the daily
//! summary runs (once per day at the configured time, or right after waking
//! if the Mac slept through it), retry backoff, and quiet hours.

use chrono::{DateTime, Duration, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};

use crate::activity::settings::{parse_clock, QuietHours};

/// Failed units of work are retried this many times in total.
pub const MAX_ATTEMPTS: i64 = 4;
const FIRST_RETRY: Duration = Duration::minutes(5);

/// The user's local time zone; tests use a fixed offset.
pub trait LocalZone: Send + Sync {
    fn local(&self, at: DateTime<Utc>) -> NaiveDateTime;
    /// The instant of a local time; the earlier one when it repeats (DST
    /// fall back), `None` when it does not exist (DST spring forward).
    fn instant(&self, local: NaiveDateTime) -> Option<DateTime<Utc>>;
}

pub struct SystemZone;

impl LocalZone for SystemZone {
    fn local(&self, at: DateTime<Utc>) -> NaiveDateTime {
        at.with_timezone(&Local).naive_local()
    }

    fn instant(&self, local: NaiveDateTime) -> Option<DateTime<Utc>> {
        Local
            .from_local_datetime(&local)
            .earliest()
            .map(|at| at.with_timezone(&Utc))
    }
}

pub struct FixedZone(pub chrono::FixedOffset);

impl LocalZone for FixedZone {
    fn local(&self, at: DateTime<Utc>) -> NaiveDateTime {
        at.with_timezone(&self.0).naive_local()
    }

    fn instant(&self, local: NaiveDateTime) -> Option<DateTime<Utc>> {
        self.0
            .from_local_datetime(&local)
            .earliest()
            .map(|at| at.with_timezone(&Utc))
    }
}

/// One local clock hour: `hour` is "YYYY-MM-DDTHH", `day` "YYYY-MM-DD".
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HourWindow {
    pub hour: String,
    pub day: String,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
}

pub fn day_key(day: NaiveDate) -> String {
    day.format("%Y-%m-%d").to_string()
}

pub fn parse_day(value: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(value, "%Y-%m-%d").ok()
}

/// The first existing instant at or after a local time (a skipped midnight
/// starts the day at the first minute that exists).
fn first_instant(zone: &dyn LocalZone, local: NaiveDateTime) -> Option<DateTime<Utc>> {
    (0..=8).find_map(|step| zone.instant(local + Duration::minutes(15 * step)))
}

/// `[start, end)` of a local day.
pub fn day_bounds(zone: &dyn LocalZone, day: NaiveDate) -> Option<(DateTime<Utc>, DateTime<Utc>)> {
    let start = first_instant(zone, day.and_time(NaiveTime::MIN))?;
    let end = first_instant(zone, day.succ_opt()?.and_time(NaiveTime::MIN))?;
    (end > start).then_some((start, end))
}

/// The local clock hours of a day; an hour that does not exist (DST spring
/// forward) is skipped, a repeated one (fall back) is one longer window.
pub fn hours_of_day(zone: &dyn LocalZone, day: NaiveDate) -> Vec<HourWindow> {
    let Some((_, day_end)) = day_bounds(zone, day) else {
        return Vec::new();
    };
    let mut windows = Vec::new();
    for hour in 0..24u32 {
        let Some(local) = day.and_hms_opt(hour, 0, 0) else {
            continue;
        };
        let Some(start) = zone.instant(local) else {
            continue;
        };
        let end = (hour + 1..24)
            .filter_map(|next| day.and_hms_opt(next, 0, 0))
            .find_map(|next| zone.instant(next))
            .unwrap_or(day_end);
        if end > start {
            windows.push(HourWindow {
                hour: format!("{}T{hour:02}", day_key(day)),
                day: day_key(day),
                start,
                end,
            });
        }
    }
    windows
}

pub fn local_day(zone: &dyn LocalZone, at: DateTime<Utc>) -> NaiveDate {
    zone.local(at).date()
}

/// Completed hours of `day`: ended by `now` and by `built_until` (how far
/// the timeline has built sessions).
pub fn completed_hours(
    zone: &dyn LocalZone,
    day: NaiveDate,
    now: DateTime<Utc>,
    built_until: DateTime<Utc>,
) -> Vec<HourWindow> {
    let limit = now.min(built_until);
    hours_of_day(zone, day)
        .into_iter()
        .filter(|window| window.end <= limit)
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunOutcome {
    Ok,
    Failed,
    Empty,
}

impl RunOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            RunOutcome::Ok => "ok",
            RunOutcome::Failed => "failed",
            RunOutcome::Empty => "empty",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "ok" => RunOutcome::Ok,
            "empty" => RunOutcome::Empty,
            _ => RunOutcome::Failed,
        }
    }
}

/// Bookkeeping of one unit of scheduled work.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunRecord {
    pub attempts: i64,
    pub outcome: RunOutcome,
    pub next_attempt_at: Option<DateTime<Utc>>,
}

/// When a unit that failed `attempts` times may run again; `None` once it
/// has used every attempt.
pub fn next_attempt(now: DateTime<Utc>, attempts: i64) -> Option<DateTime<Utc>> {
    (attempts < MAX_ATTEMPTS).then(|| {
        let exponent = u32::try_from(attempts.saturating_sub(1).clamp(0, 6)).unwrap_or(0);
        now + FIRST_RETRY * 2i32.pow(exponent)
    })
}

/// Whether a unit with this record should run now (`force`: the user asked,
/// so a waiting retry runs early; a unit that used every attempt still runs).
pub fn may_run(record: Option<&RunRecord>, now: DateTime<Utc>, force: bool) -> bool {
    match record {
        None => true,
        Some(record) => match record.outcome {
            RunOutcome::Ok | RunOutcome::Empty => false,
            RunOutcome::Failed if force => true,
            RunOutcome::Failed => record.next_attempt_at.is_some_and(|at| at <= now),
        },
    }
}

/// The automatic summary of `today` is due once the local clock passed the
/// configured time and no automatic run of that day finished (or is
/// waiting to retry). A Mac asleep at the time catches up at the first tick
/// after waking; the run record makes it happen once.
pub fn auto_summary_due(
    now_local: NaiveDateTime,
    summary_time: &str,
    record: Option<&RunRecord>,
    now: DateTime<Utc>,
) -> bool {
    let Some(minutes) = parse_clock(summary_time) else {
        return false;
    };
    let at = NaiveTime::from_hms_opt(minutes / 60, minutes % 60, 0).unwrap_or(NaiveTime::MIN);
    now_local.time() >= at && may_run(record, now, false)
}

/// The local end of the quiet period `now_local` falls in, if any. An end at
/// or before the start spans midnight; equal times mean no quiet hours.
pub fn quiet_until(now_local: NaiveDateTime, quiet: &QuietHours) -> Option<NaiveDateTime> {
    if !quiet.enabled {
        return None;
    }
    let start = parse_clock(&quiet.start)?;
    let end = parse_clock(&quiet.end)?;
    if start == end {
        return None;
    }
    let minute = now_local
        .time()
        .signed_duration_since(NaiveTime::MIN)
        .num_minutes() as u32;
    let today = now_local.date();
    let end_time = NaiveTime::from_hms_opt(end / 60, end % 60, 0)?;
    if start < end {
        (start..end)
            .contains(&minute)
            .then(|| today.and_time(end_time))
    } else if minute >= start {
        Some(today.succ_opt()?.and_time(end_time))
    } else if minute < end {
        Some(today.and_time(end_time))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zone() -> FixedZone {
        FixedZone(chrono::FixedOffset::west_opt(3 * 3600).unwrap())
    }

    fn local(day: u32, hour: u32, minute: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 10, day)
            .unwrap()
            .and_hms_opt(hour, minute, 0)
            .unwrap()
    }

    #[test]
    fn a_day_has_24_hours_in_local_time() {
        let zone = zone();
        let hours = hours_of_day(&zone, NaiveDate::from_ymd_opt(2026, 10, 4).unwrap());
        assert_eq!(hours.len(), 24);
        assert_eq!(hours[0].hour, "2026-10-04T00");
        assert_eq!(hours[0].start.to_rfc3339(), "2026-10-04T03:00:00+00:00");
        assert_eq!(hours[23].end.to_rfc3339(), "2026-10-05T03:00:00+00:00");
    }

    #[test]
    fn only_hours_that_ended_and_were_built_are_completed() {
        let zone = zone();
        let day = NaiveDate::from_ymd_opt(2026, 10, 4).unwrap();
        let now = zone.instant(local(4, 15, 20)).unwrap();
        let built = zone.instant(local(4, 14, 30)).unwrap();
        let hours = completed_hours(&zone, day, now, built);
        assert_eq!(hours.last().unwrap().hour, "2026-10-04T13");
        assert_eq!(
            completed_hours(&zone, day, now, now).last().unwrap().hour,
            "2026-10-04T14"
        );
    }

    #[test]
    fn retries_back_off_and_stop() {
        let now = Utc.with_ymd_and_hms(2026, 10, 4, 12, 0, 0).unwrap();
        assert_eq!(next_attempt(now, 1), Some(now + Duration::minutes(5)));
        assert_eq!(next_attempt(now, 2), Some(now + Duration::minutes(10)));
        assert_eq!(next_attempt(now, MAX_ATTEMPTS), None);
        let failed = RunRecord {
            attempts: MAX_ATTEMPTS,
            outcome: RunOutcome::Failed,
            next_attempt_at: None,
        };
        assert!(!may_run(Some(&failed), now, false));
        assert!(may_run(Some(&failed), now, true));
    }

    #[test]
    fn quiet_hours_spanning_midnight() {
        let quiet = QuietHours {
            enabled: true,
            start: "22:00".into(),
            end: "08:00".into(),
        };
        assert_eq!(quiet_until(local(4, 23, 0), &quiet), Some(local(5, 8, 0)));
        assert_eq!(quiet_until(local(5, 3, 0), &quiet), Some(local(5, 8, 0)));
        assert_eq!(quiet_until(local(5, 8, 0), &quiet), None);
        assert_eq!(quiet_until(local(4, 21, 59), &quiet), None);
        let off = QuietHours {
            enabled: false,
            ..quiet
        };
        assert_eq!(quiet_until(local(4, 23, 0), &off), None);
    }
}
