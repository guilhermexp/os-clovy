//! Orchestration over injected dependencies (store, provider, embedder,
//! sources, zone, clock), so the whole path runs in tests with a fake
//! provider: completed hours → distillation → hour report → workstream fold,
//! then the day summary; and the read model of the "Today" view.

use chrono::{DateTime, Duration, NaiveDate, Utc};
use futures_util::future::BoxFuture;
use serde::Serialize;

use super::db::{self, NewHourReport};
use super::distill::{self, SessionText};
use super::embedder::{Embedder, EmbedderStatus};
use super::hour::{self, HourInput, SessionLine};
use super::prompts;
use super::schedule::{self, day_key, HourWindow, LocalZone, RunOutcome};
use super::sources::{hhmm, DaySources};
use super::summary::{
    self, DayInput, DayPanelsDto, DaySummaryDto, HourFocusDto, HourReportDto, SummaryTrigger,
};
use super::workstreams::{self, WorkstreamDto};
use crate::activity::settings::ActivitySettings;
use crate::activity::store::{ActivityStore, StoreError, TIMELINE_CONSUMER};
use crate::activity::timeline::{self, db::TimelineSessionDto, stats::day_stats};
use crate::interface_locale::UiLocale;
use crate::llm::{GenerateOutput, GenerateRequest, LlmError};

/// One activity call; production goes through `llm::generate_for_activity`.
pub trait Generator: Send + Sync {
    fn generate(&self, request: GenerateRequest)
        -> BoxFuture<'_, Result<GenerateOutput, LlmError>>;
}

pub struct ActivityProvider;

impl Generator for ActivityProvider {
    fn generate(
        &self,
        request: GenerateRequest,
    ) -> BoxFuture<'_, Result<GenerateOutput, LlmError>> {
        Box::pin(crate::llm::generate_for_activity(request))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PipelineError {
    #[error(transparent)]
    Llm(#[from] LlmErrorWrap),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("the provider answer was not usable: {0}")]
    Unusable(&'static str),
}

/// `LlmError` has no `Error` impl; this carries it with a message.
#[derive(Debug)]
pub struct LlmErrorWrap(pub LlmError);

impl std::fmt::Display for LlmErrorWrap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let error: crate::domain::types::AppError = self.0.clone().into();
        write!(f, "{}", error.message)
    }
}

impl std::error::Error for LlmErrorWrap {}

impl From<LlmError> for PipelineError {
    fn from(error: LlmError) -> Self {
        PipelineError::Llm(LlmErrorWrap(error))
    }
}

pub struct Deps<'a> {
    pub store: &'a ActivityStore,
    pub settings: &'a ActivitySettings,
    pub generator: &'a dyn Generator,
    pub embedder: &'a dyn Embedder,
    pub sources: &'a dyn DaySources,
    pub zone: &'a dyn LocalZone,
    pub locale: UiLocale,
    pub now: DateTime<Utc>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HourOutcome {
    Reported,
    /// No session time in the hour.
    Empty,
}

fn minutes_of(ms: i64) -> i64 {
    (ms + 30_000) / 60_000
}

async fn call(
    deps: &Deps<'_>,
    system: &str,
    prompt: String,
    schema: serde_json::Value,
) -> Result<GenerateOutput, PipelineError> {
    Ok(deps
        .generator
        .generate(GenerateRequest {
            system: Some(prompts::system_prompt(system, deps.locale)),
            prompt,
            schema: Some(schema),
            timeout: None,
        })
        .await?)
}

fn session_line(
    session: &TimelineSessionDto,
    window: &HourWindow,
    zone: &dyn LocalZone,
) -> SessionLine {
    let start = timeline::builder::parse_time(&session.started_at)
        .unwrap_or(window.start)
        .max(window.start);
    let end = timeline::builder::parse_time(&session.ended_at)
        .unwrap_or(window.end)
        .min(window.end);
    SessionLine {
        start: hhmm(zone, start),
        end: hhmm(zone, end),
        app: session.app_name.clone(),
        context: session.context.clone(),
        category: session.category.as_db().to_string(),
        minutes: minutes_of((end - start).num_milliseconds().max(0)),
        window: session.window_title.clone(),
    }
}

/// Generates the report of one completed hour and stores it.
pub async fn report_hour(
    deps: &Deps<'_>,
    window: &HourWindow,
) -> Result<HourOutcome, PipelineError> {
    let view = timeline::timeline_view(
        deps.store,
        deps.settings,
        window.start,
        window.end,
        deps.now,
    )
    .await?;
    let active_minutes = minutes_of(view.stats.focused_ms);
    if active_minutes == 0 {
        return Ok(HourOutcome::Empty);
    }
    let mut texts = Vec::new();
    let mut lines = Vec::new();
    for session in &view.sessions {
        let line = session_line(session, window, deps.zone);
        let start = timeline::builder::parse_time(&session.started_at).unwrap_or(window.start);
        let end = timeline::builder::parse_time(&session.ended_at).unwrap_or(window.end);
        let seconds = (end.min(window.end) - start.max(window.start)).num_seconds();
        let documents = db::session_documents(deps.store, session.id, window.start, window.end)
            .await?
            .into_iter()
            .filter(|(_, url, _)| {
                !url.as_deref().is_some_and(|url| {
                    timeline::session_excluded("", None, None, Some(url), deps.settings)
                })
            })
            .map(|(seen_at, _, body)| {
                let at = timeline::builder::parse_time(&seen_at).unwrap_or(window.start);
                (hhmm(deps.zone, at), body)
            })
            .collect();
        texts.push(SessionText {
            session_id: session.id,
            app: session.app_name.clone(),
            window: session.window_title.clone().unwrap_or_default(),
            time: line.start.clone(),
            seconds,
            documents,
        });
        lines.push(line);
    }
    let label = format!("{} {}:00", window.day, &window.hour[11..13]);
    let header = format!(
        "=== HOUR {label} · {} sessions · {active_minutes} min active ===",
        texts.len()
    );
    let distilled = distill::distill(&texts, &header, deps.embedder).await;
    let blocks = deps
        .sources
        .coding_blocks(deps.store, window.start, window.end, deps.zone)
        .await;
    let meetings = deps
        .sources
        .meetings(window.start, window.end, deps.zone)
        .await;
    let prompt = hour::prompt(&HourInput {
        label: &label,
        active_minutes,
        sessions: &lines,
        coding_blocks: &blocks,
        meetings: &meetings,
        distilled: &distilled.body,
    });
    let output = call(
        deps,
        prompts::HOUR_REPORT,
        prompt,
        prompts::hour_report_schema(),
    )
    .await?;
    let answer = output
        .json
        .as_ref()
        .and_then(hour::parse_answer)
        .ok_or(PipelineError::Unusable("hour report without activities"))?;
    let activities = hour::assemble(&answer, active_minutes);
    db::insert_hour_report(
        deps.store,
        &NewHourReport {
            hour: window.hour.clone(),
            day: window.day.clone(),
            started_at: window.start,
            ended_at: window.end,
            active_minutes,
            summary: answer.summary,
            activities,
            distilled: distilled.body,
            distill_stats: serde_json::to_value(&distilled.stats).unwrap_or_default(),
            locale: prompts::locale_tag(deps.locale).to_string(),
            provider: output.provider,
            generated_at: deps.now,
        },
    )
    .await?;
    Ok(HourOutcome::Reported)
}

/// Folds one stored hour report into the day's workstreams. When the
/// provider keeps failing (`give_up`), the hour lands as one new workstream
/// without a call, so no hour is ever left out.
pub async fn fold_hour(
    deps: &Deps<'_>,
    report: &HourReportDto,
    give_up: bool,
) -> Result<bool, PipelineError> {
    let day = &report.hour[..10];
    let prior = db::workstreams_of_day(deps.store, day).await?;
    let placements = if give_up {
        Vec::new()
    } else {
        let label = format!("{}:00", &report.hour[11..13]);
        let output = call(
            deps,
            prompts::WORKSTREAM_FOLD,
            workstreams::prompt(&prior, &label, &report.activities),
            prompts::workstream_schema(),
        )
        .await?;
        output
            .json
            .as_ref()
            .and_then(workstreams::parse_placements)
            .ok_or(PipelineError::Unusable(
                "workstream answer without placements",
            ))?
    };
    let plan = workstreams::plan_fold(&prior, &report.activities, &placements);
    Ok(db::apply_fold(deps.store, day, &report.hour, &plan, deps.now).await?)
}

/// How far the timeline has built sessions: now when caught up with
/// capture, else the last processed frame.
pub async fn built_until(
    store: &ActivityStore,
    now: DateTime<Utc>,
) -> Result<DateTime<Utc>, StoreError> {
    let cursor = store.processing_cursor(TIMELINE_CONSUMER).await?;
    if store.latest_frame_at().await? == cursor.last_frame_at {
        return Ok(now);
    }
    Ok(cursor
        .last_frame_at
        .as_deref()
        .and_then(timeline::builder::parse_time)
        .unwrap_or(DateTime::<Utc>::UNIX_EPOCH))
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CatchUp {
    pub reported: usize,
    pub folded: usize,
    pub failed: usize,
}

/// Reports every completed hour of `day` that has none yet (respecting retry
/// backoff unless `force`), then folds every unfolded report in hour order.
pub async fn catch_up_day(
    deps: &Deps<'_>,
    day: NaiveDate,
    force: bool,
) -> Result<CatchUp, StoreError> {
    let mut result = CatchUp::default();
    let built = built_until(deps.store, deps.now).await?;
    for window in schedule::completed_hours(deps.zone, day, deps.now, built) {
        if db::hour_report_exists(deps.store, &window.hour).await? {
            continue;
        }
        let key = format!("hour:{}", window.hour);
        let record = db::run_record(deps.store, &key).await?;
        if !schedule::may_run(record.as_ref(), deps.now, force) {
            continue;
        }
        match report_hour(deps, &window).await {
            Ok(HourOutcome::Reported) => {
                db::record_run(deps.store, &key, RunOutcome::Ok, None, deps.now).await?;
                result.reported += 1;
            }
            Ok(HourOutcome::Empty) => {
                db::record_run(deps.store, &key, RunOutcome::Empty, None, deps.now).await?;
            }
            Err(PipelineError::Store(error)) => return Err(error),
            Err(error) => {
                tracing::warn!(hour = %window.hour, %error, "hour report failed");
                db::record_run(
                    deps.store,
                    &key,
                    RunOutcome::Failed,
                    Some(&error.to_string()),
                    deps.now,
                )
                .await?;
                result.failed += 1;
            }
        }
    }
    for report in db::unfolded_reports(deps.store, &day_key(day)).await? {
        let key = format!("fold:{}", report.hour);
        let record = db::run_record(deps.store, &key).await?;
        let used_up = record
            .as_ref()
            .is_some_and(|record| record.attempts >= schedule::MAX_ATTEMPTS);
        if !used_up && !schedule::may_run(record.as_ref(), deps.now, force) {
            // Later hours fold after this one; wait for its retry.
            break;
        }
        match fold_hour(deps, &report, used_up).await {
            Ok(_) => {
                db::record_run(deps.store, &key, RunOutcome::Ok, None, deps.now).await?;
                result.folded += 1;
            }
            Err(PipelineError::Store(error)) => return Err(error),
            Err(error) => {
                tracing::warn!(hour = %report.hour, %error, "workstream fold failed");
                db::record_run(
                    deps.store,
                    &key,
                    RunOutcome::Failed,
                    Some(&error.to_string()),
                    deps.now,
                )
                .await?;
                result.failed += 1;
                break;
            }
        }
    }
    Ok(result)
}

/// The day's measured statistics and per-hour focus from the timeline.
async fn day_measures(
    deps_store: &ActivityStore,
    settings: &ActivitySettings,
    zone: &dyn LocalZone,
    day: NaiveDate,
    now: DateTime<Utc>,
) -> Result<(timeline::stats::TimelineStatsDto, Vec<HourFocusDto>), StoreError> {
    let Some((from, to)) = schedule::day_bounds(zone, day) else {
        return Ok(Default::default());
    };
    let view = timeline::timeline_view(deps_store, settings, from, to, now).await?;
    let hours = schedule::hours_of_day(zone, day)
        .into_iter()
        .map(|window| HourFocusDto {
            focused_ms: day_stats(&view.sessions, &view.gaps, window.start, window.end).focused_ms,
            hour: window.hour,
        })
        .collect();
    Ok((view.stats, hours))
}

/// Generates (or regenerates) the summary of `day` from what is stored.
pub async fn generate_summary(
    deps: &Deps<'_>,
    day: NaiveDate,
    trigger: SummaryTrigger,
) -> Result<Option<DaySummaryDto>, PipelineError> {
    let Some((from, to)) = schedule::day_bounds(deps.zone, day) else {
        return Ok(None);
    };
    let key = day_key(day);
    let (stats, _) = day_measures(deps.store, deps.settings, deps.zone, day, deps.now).await?;
    let workstreams = db::workstreams_of_day(deps.store, &key).await?;
    let reports = db::hour_reports_of_day(deps.store, &key).await?;
    let meetings = deps.sources.meetings(from, to, deps.zone).await;
    let blocks = deps
        .sources
        .coding_blocks(deps.store, from, to, deps.zone)
        .await;
    if reports.is_empty() && meetings.is_empty() && blocks.is_empty() {
        return Ok(None);
    }
    let prompt = summary::prompt(&DayInput {
        day: &key,
        stats: &stats,
        workstreams: &workstreams,
        hour_reports: &reports,
        meetings: &meetings,
        coding_blocks: &blocks,
    });
    let output = call(
        deps,
        prompts::DAY_SUMMARY,
        prompt,
        prompts::day_summary_schema(),
    )
    .await?;
    let answer = output
        .json
        .as_ref()
        .and_then(summary::parse_answer)
        .ok_or(PipelineError::Unusable("day summary without text"))?;
    let dto = DaySummaryDto {
        day: key,
        headline: answer.headline,
        narrative: answer.narrative,
        insights: answer.insights,
        standup: answer.standup,
        hours_covered: reports.len() as i64,
        locale: prompts::locale_tag(deps.locale).to_string(),
        provider: output.provider,
        trigger,
        generated_at: crate::activity::store::timestamp(deps.now),
    };
    db::save_summary(deps.store, &dto).await?;
    Ok(Some(dto))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ProviderState {
    Ready,
    /// No activity provider chosen ("none"): nothing is generated.
    Missing,
    /// The chosen provider has no JSON schema support (or was never tested).
    Insufficient,
}

/// Everything the "Today" view shows for a day's intelligence.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DayIntelligenceDto {
    pub availability: timeline::Availability,
    pub day: String,
    pub provider: ProviderState,
    pub embedder: EmbedderStatus,
    pub running: bool,
    pub summary_time: String,
    pub summary: Option<DaySummaryDto>,
    pub workstreams: Vec<WorkstreamDto>,
    pub hour_reports: Vec<HourReportDto>,
    pub panels: DayPanelsDto,
}

pub struct ReadContext<'a> {
    pub store: &'a ActivityStore,
    pub settings: &'a ActivitySettings,
    pub sources: &'a dyn DaySources,
    pub zone: &'a dyn LocalZone,
    pub now: DateTime<Utc>,
}

/// The read model: stored texts plus panels computed from the data now.
pub async fn day_view(
    context: &ReadContext<'_>,
    day: NaiveDate,
) -> Result<
    (
        Option<DaySummaryDto>,
        Vec<WorkstreamDto>,
        Vec<HourReportDto>,
        DayPanelsDto,
    ),
    StoreError,
> {
    let key = day_key(day);
    let (stats, hours) = day_measures(
        context.store,
        context.settings,
        context.zone,
        day,
        context.now,
    )
    .await?;
    let workstreams = db::workstreams_of_day(context.store, &key).await?;
    let reports = db::hour_reports_of_day(context.store, &key).await?;
    let (meetings, blocks) = match schedule::day_bounds(context.zone, day) {
        Some((from, to)) => (
            context.sources.meetings(from, to, context.zone).await,
            context
                .sources
                .coding_blocks(context.store, from, to, context.zone)
                .await,
        ),
        None => (Vec::new(), Vec::new()),
    };
    let panels = summary::panels(&stats, hours, &workstreams, &meetings, &blocks);
    let stored = db::summary_of_day(context.store, &key).await?;
    Ok((stored, workstreams, reports, panels))
}

/// Retention for the day intelligence tables (same period as capture).
pub async fn prune(
    store: &ActivityStore,
    settings: &ActivitySettings,
    zone: &dyn LocalZone,
    now: DateTime<Utc>,
) -> Result<u64, StoreError> {
    let cutoff = now - Duration::days(i64::from(settings.retention_days));
    db::prune(store, &day_key(schedule::local_day(zone, cutoff))).await
}
