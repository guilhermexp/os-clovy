//! Day intelligence tables in the encrypted `activity.sqlite3` (migration 4,
//! `day_intelligence`): hour reports, workstreams and their hours, day
//! summaries, and the scheduler's run records. This is also the read API
//! other slices use (the MCP `get_day_summary` tool): `summary_of_day`,
//! `workstreams_of_day`, `hour_reports_of_day`.

use chrono::{DateTime, Utc};
use serde_json::{json, Map, Value};
use sqlx::query::query;
use sqlx::row::Row;
use sqlx_sqlite::SqliteRow;

use super::hour::HourActivity;
use super::schedule::{next_attempt, RunOutcome, RunRecord};
use super::summary::{DaySummaryDto, HourReportDto, Insight, Standup, SummaryTrigger};
use super::workstreams::{FoldPlan, WorkstreamDto, WorkstreamHourDto};
use crate::activity::store::{timestamp, ActivityStore, StoreError};

/// A finished hour report to store.
#[derive(Clone, Debug, PartialEq)]
pub struct NewHourReport {
    pub hour: String,
    pub day: String,
    pub started_at: DateTime<Utc>,
    pub ended_at: DateTime<Utc>,
    pub active_minutes: i64,
    pub summary: String,
    pub activities: Vec<HourActivity>,
    pub distilled: String,
    pub distill_stats: Value,
    pub locale: String,
    pub provider: String,
    pub generated_at: DateTime<Utc>,
}

const REPORT_COLUMNS: &str =
    "hour, started_at, ended_at, active_minutes, summary, activities_json, provider, generated_at";

fn report_dto(row: &SqliteRow) -> HourReportDto {
    HourReportDto {
        hour: row.get(0),
        started_at: row.get(1),
        ended_at: row.get(2),
        active_minutes: row.get(3),
        summary: row.get(4),
        activities: serde_json::from_str(row.get::<&str, _>(5)).unwrap_or_default(),
        provider: row.get(6),
        generated_at: row.get(7),
    }
}

pub async fn insert_hour_report(
    store: &ActivityStore,
    report: &NewHourReport,
) -> Result<(), StoreError> {
    query(
        "INSERT INTO day_hour_reports (hour, day, started_at, ended_at, active_minutes, summary,
            activities_json, distilled, distill_json, locale, provider, generated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(hour) DO NOTHING",
    )
    .bind(&report.hour)
    .bind(&report.day)
    .bind(timestamp(report.started_at))
    .bind(timestamp(report.ended_at))
    .bind(report.active_minutes)
    .bind(&report.summary)
    .bind(serde_json::to_string(&report.activities).unwrap_or_else(|_| "[]".into()))
    .bind(&report.distilled)
    .bind(report.distill_stats.to_string())
    .bind(&report.locale)
    .bind(&report.provider)
    .bind(timestamp(report.generated_at))
    .execute(store.pool())
    .await?;
    Ok(())
}

pub async fn hour_report_exists(store: &ActivityStore, hour: &str) -> Result<bool, StoreError> {
    Ok(query("SELECT 1 FROM day_hour_reports WHERE hour = ?")
        .bind(hour)
        .fetch_optional(store.pool())
        .await?
        .is_some())
}

/// The day's hour reports, in hour order.
pub async fn hour_reports_of_day(
    store: &ActivityStore,
    day: &str,
) -> Result<Vec<HourReportDto>, StoreError> {
    Ok(query(&format!(
        "SELECT {REPORT_COLUMNS} FROM day_hour_reports WHERE day = ? ORDER BY hour"
    ))
    .bind(day)
    .fetch_all(store.pool())
    .await?
    .iter()
    .map(report_dto)
    .collect())
}

/// Reports of `day` not folded into workstreams yet, in hour order.
pub async fn unfolded_reports(
    store: &ActivityStore,
    day: &str,
) -> Result<Vec<HourReportDto>, StoreError> {
    Ok(query(&format!(
        "SELECT {REPORT_COLUMNS} FROM day_hour_reports
         WHERE day = ? AND folded_at IS NULL ORDER BY hour"
    ))
    .bind(day)
    .fetch_all(store.pool())
    .await?
    .iter()
    .map(report_dto)
    .collect())
}

/// The day's workstreams (oldest first) with their hours; minutes are the
/// sum of the hours' measured minutes.
pub async fn workstreams_of_day(
    store: &ActivityStore,
    day: &str,
) -> Result<Vec<WorkstreamDto>, StoreError> {
    let rows = query("SELECT id, title, summary FROM day_workstreams WHERE day = ? ORDER BY id")
        .bind(day)
        .fetch_all(store.pool())
        .await?;
    let mut workstreams = Vec::with_capacity(rows.len());
    for row in &rows {
        let id: i64 = row.get(0);
        let hours: Vec<WorkstreamHourDto> = query(
            "SELECT hour, minutes, note FROM day_workstream_hours WHERE workstream_id = ? ORDER BY hour",
        )
        .bind(id)
        .fetch_all(store.pool())
        .await?
        .iter()
        .map(|hour| WorkstreamHourDto {
            hour: hour.get(0),
            minutes: hour.get(1),
            note: hour.get(2),
        })
        .collect();
        workstreams.push(WorkstreamDto {
            id,
            title: row.get(1),
            summary: row.get(2),
            minutes: hours.iter().map(|hour| hour.minutes).sum(),
            hours,
        });
    }
    Ok(workstreams)
}

/// Applies an hour's fold in one transaction and marks the report folded.
/// Returns false (and writes nothing) when the hour was folded already.
pub async fn apply_fold(
    store: &ActivityStore,
    day: &str,
    hour: &str,
    plan: &FoldPlan,
    now: DateTime<Utc>,
) -> Result<bool, StoreError> {
    let at = timestamp(now);
    let mut tx = store.pool().begin_with("BEGIN IMMEDIATE").await?;
    let marked =
        query("UPDATE day_hour_reports SET folded_at = ? WHERE hour = ? AND folded_at IS NULL")
            .bind(&at)
            .bind(hour)
            .execute(&mut *tx)
            .await?
            .rows_affected();
    if marked == 0 {
        tx.rollback().await?;
        return Ok(false);
    }
    for append in &plan.appends {
        let inserted = query(
            "INSERT INTO day_workstream_hours (workstream_id, hour, minutes, note)
             SELECT id, ?, ?, ? FROM day_workstreams WHERE id = ? AND day = ?
             ON CONFLICT(workstream_id, hour) DO NOTHING",
        )
        .bind(hour)
        .bind(append.minutes)
        .bind(&append.note)
        .bind(append.workstream_id)
        .bind(day)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if inserted == 0 {
            continue;
        }
        query(
            "UPDATE day_workstreams
             SET summary = COALESCE(?, summary), last_hour = ?, updated_at = ?
             WHERE id = ?",
        )
        .bind(append.summary.as_deref())
        .bind(hour)
        .bind(&at)
        .bind(append.workstream_id)
        .execute(&mut *tx)
        .await?;
    }
    for create in &plan.creates {
        let id = query(
            "INSERT INTO day_workstreams (day, title, summary, first_hour, last_hour, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(day)
        .bind(&create.title)
        .bind(&create.summary)
        .bind(hour)
        .bind(hour)
        .bind(&at)
        .bind(&at)
        .execute(&mut *tx)
        .await?
        .last_insert_rowid();
        query("INSERT INTO day_workstream_hours (workstream_id, hour, minutes, note) VALUES (?, ?, ?, ?)")
            .bind(id)
            .bind(hour)
            .bind(create.minutes)
            .bind(&create.note)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(true)
}

pub async fn save_summary(
    store: &ActivityStore,
    summary: &DaySummaryDto,
) -> Result<(), StoreError> {
    let standup = json!({
        "done": summary.standup.done,
        "in_progress": summary.standup.in_progress,
        "blockers": summary.standup.blockers,
    });
    query(
        "INSERT INTO day_summaries (day, headline, narrative, insights_json, standup_json,
            hours_covered, locale, provider, trigger, generated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(day) DO UPDATE SET
            headline = excluded.headline, narrative = excluded.narrative,
            insights_json = excluded.insights_json, standup_json = excluded.standup_json,
            hours_covered = excluded.hours_covered, locale = excluded.locale,
            provider = excluded.provider, trigger = excluded.trigger,
            generated_at = excluded.generated_at",
    )
    .bind(&summary.day)
    .bind(&summary.headline)
    .bind(&summary.narrative)
    .bind(serde_json::to_string(&summary.insights).unwrap_or_else(|_| "[]".into()))
    .bind(standup.to_string())
    .bind(summary.hours_covered)
    .bind(&summary.locale)
    .bind(&summary.provider)
    .bind(summary.trigger.as_str())
    .bind(&summary.generated_at)
    .execute(store.pool())
    .await?;
    Ok(())
}

fn string_list(value: &Value, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// The stored summary of a local day ("YYYY-MM-DD").
pub async fn summary_of_day(
    store: &ActivityStore,
    day: &str,
) -> Result<Option<DaySummaryDto>, StoreError> {
    let Some(row) = query(
        "SELECT day, headline, narrative, insights_json, standup_json, hours_covered, locale,
                provider, trigger, generated_at
         FROM day_summaries WHERE day = ?",
    )
    .bind(day)
    .fetch_optional(store.pool())
    .await?
    else {
        return Ok(None);
    };
    let standup: Value = serde_json::from_str(row.get::<&str, _>(4)).unwrap_or(Value::Null);
    let insights: Vec<Insight> = serde_json::from_str(row.get::<&str, _>(3)).unwrap_or_default();
    Ok(Some(DaySummaryDto {
        day: row.get(0),
        headline: row.get(1),
        narrative: row.get(2),
        insights,
        standup: Standup {
            done: string_list(&standup, "done"),
            in_progress: string_list(&standup, "in_progress"),
            blockers: string_list(&standup, "blockers"),
        },
        hours_covered: row.get(5),
        locale: row.get(6),
        provider: row.get(7),
        trigger: SummaryTrigger::parse(row.get::<&str, _>(8)),
        generated_at: row.get(9),
    }))
}

pub async fn run_record(store: &ActivityStore, key: &str) -> Result<Option<RunRecord>, StoreError> {
    Ok(
        query("SELECT attempts, outcome, next_attempt_at FROM day_intelligence_runs WHERE key = ?")
            .bind(key)
            .fetch_optional(store.pool())
            .await?
            .map(|row| RunRecord {
                attempts: row.get(0),
                outcome: RunOutcome::parse(row.get::<&str, _>(1)),
                next_attempt_at: row
                    .get::<Option<&str>, _>(2)
                    .and_then(|at| DateTime::parse_from_rfc3339(at).ok())
                    .map(|at| at.with_timezone(&Utc)),
            }),
    )
}

/// Records an outcome; a failure counts an attempt and schedules the retry.
pub async fn record_run(
    store: &ActivityStore,
    key: &str,
    outcome: RunOutcome,
    error: Option<&str>,
    now: DateTime<Utc>,
) -> Result<RunRecord, StoreError> {
    let previous = run_record(store, key)
        .await?
        .map_or(0, |record| record.attempts);
    let attempts = previous + 1;
    let next = match outcome {
        RunOutcome::Failed => next_attempt(now, attempts),
        RunOutcome::Ok | RunOutcome::Empty => None,
    };
    query(
        "INSERT INTO day_intelligence_runs (key, attempts, outcome, last_error, next_attempt_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?)
         ON CONFLICT(key) DO UPDATE SET attempts = excluded.attempts, outcome = excluded.outcome,
            last_error = excluded.last_error, next_attempt_at = excluded.next_attempt_at,
            updated_at = excluded.updated_at",
    )
    .bind(key)
    .bind(attempts)
    .bind(outcome.as_str())
    .bind(error)
    .bind(next.map(timestamp))
    .bind(timestamp(now))
    .execute(store.pool())
    .await?;
    Ok(RunRecord {
        attempts,
        outcome,
        next_attempt_at: next,
    })
}

/// Retention: removes everything of days before `cutoff_day`.
pub async fn prune(store: &ActivityStore, cutoff_day: &str) -> Result<u64, StoreError> {
    let mut tx = store.pool().begin_with("BEGIN IMMEDIATE").await?;
    let mut removed = 0;
    for statement in [
        "DELETE FROM day_hour_reports WHERE day < ?",
        "DELETE FROM day_workstreams WHERE day < ?",
        "DELETE FROM day_summaries WHERE day < ?",
    ] {
        removed += query(statement)
            .bind(cutoff_day)
            .execute(&mut *tx)
            .await?
            .rows_affected();
    }
    // Run keys end with the hour or day they are about.
    query(
        "DELETE FROM day_intelligence_runs
         WHERE substr(key, instr(key, ':') + 1, 10) < ?",
    )
    .bind(cutoff_day)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(removed)
}

/// Window segments (local text) of a timeline session seen in `[from, to)`.
pub async fn session_documents(
    store: &ActivityStore,
    session_id: i64,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
) -> Result<Vec<(String, Option<String>, String)>, StoreError> {
    Ok(query(
        "SELECT ts.seen_at, ts.browser_url, ts.body
         FROM timeline_sessions s JOIN timeline_search ts
           ON ts.rowid BETWEEN s.search_rowid_first AND s.search_rowid_last
          AND ts.session_id = s.id
         WHERE s.id = ? AND ts.seen_at >= ? AND ts.seen_at < ? AND ts.body <> ''
         ORDER BY ts.rowid",
    )
    .bind(session_id)
    .bind(timestamp(from))
    .bind(timestamp(to))
    .fetch_all(store.pool())
    .await?
    .iter()
    .map(|row| {
        let url: String = row.get(1);
        (row.get(0), (!url.is_empty()).then_some(url), row.get(2))
    })
    .collect())
}

/// Debug export sections (development builds): the newest days.
pub async fn debug_sections(store: &ActivityStore) -> Result<Map<String, Value>, StoreError> {
    let days: Vec<String> =
        query("SELECT DISTINCT day FROM day_hour_reports ORDER BY day DESC LIMIT 3")
            .fetch_all(store.pool())
            .await?
            .iter()
            .map(|row| row.get(0))
            .collect();
    let mut reports = Vec::new();
    let mut workstreams = Vec::new();
    let mut summaries = Vec::new();
    for day in &days {
        for report in hour_reports_of_day(store, day).await? {
            reports.push(serde_json::to_value(report).unwrap_or(Value::Null));
        }
        workstreams.push(json!({
            "day": day,
            "workstreams": workstreams_of_day(store, day).await?,
        }));
    }
    for row in query("SELECT day FROM day_summaries ORDER BY day DESC LIMIT 3")
        .fetch_all(store.pool())
        .await?
    {
        if let Some(summary) = summary_of_day(store, row.get::<&str, _>(0)).await? {
            summaries.push(serde_json::to_value(summary).unwrap_or(Value::Null));
        }
    }
    let runs: Vec<Value> = query(
        "SELECT key, attempts, outcome, last_error, next_attempt_at, updated_at
         FROM day_intelligence_runs ORDER BY updated_at DESC LIMIT 100",
    )
    .fetch_all(store.pool())
    .await?
    .iter()
    .map(|row| {
        json!({
            "key": row.get::<String, _>(0),
            "attempts": row.get::<i64, _>(1),
            "outcome": row.get::<String, _>(2),
            "lastError": row.get::<Option<String>, _>(3),
            "nextAttemptAt": row.get::<Option<String>, _>(4),
            "updatedAt": row.get::<String, _>(5),
        })
    })
    .collect();
    let mut sections = Map::new();
    sections.insert("dayHourReports".into(), Value::Array(reports));
    sections.insert("dayWorkstreams".into(), Value::Array(workstreams));
    sections.insert("daySummaries".into(), Value::Array(summaries));
    sections.insert("dayIntelligenceRuns".into(), Value::Array(runs));
    Ok(sections)
}
