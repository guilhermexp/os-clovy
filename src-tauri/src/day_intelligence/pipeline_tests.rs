//! The whole path over a real encrypted activity store and a fake provider:
//! frames → timeline → hour report → workstreams → day summary → view.

use serde_json::json;

use super::db;
use super::embedder::NoEmbedder;
use super::pipeline::{self, ReadContext};
use super::prompts;
use super::schedule::{self, day_key, RunOutcome};
use super::summary::SummaryTrigger;
use super::test_support::*;
use crate::activity::timeline::timeline_view;
use crate::interface_locale::UiLocale;
use crate::llm::LlmError;

fn hour_answer() -> serde_json::Value {
    json!({
        "summary": "Fixed the login redirect (KAN-123) and reviewed a pull request.",
        "activities": [
            {"description": "Fixed the login redirect for KAN-123", "minutes": 30},
            {"description": "Reviewed the pagination pull request", "minutes": 30}
        ]
    })
}

fn summary_answer() -> serde_json::Value {
    json!({
        "headline": "Um dia focado no login",
        "narrative": "Corrigiu o redirecionamento do login e participou da reunião Weekly sync.",
        "insights": [{"title": "Foco longo", "text": "A maior parte do dia foi em uma frente só."}],
        "standup": {
            "done": ["Corrigiu o redirecionamento do login (KAN-123)"],
            "in_progress": ["Revisão do PR de paginação"],
            "blockers": []
        }
    })
}

/// A morning of work: 13:00-13:30 Slack, 14:00-14:42 Zed, nothing after.
async fn day_store() -> (tempfile::TempDir, crate::activity::store::ActivityStore) {
    let (dir, store) = store().await;
    work(
        &store,
        local(13, 0),
        30,
        "Slack",
        "#auth - Slack",
        &["We should start the login fix today because users are blocked"],
    )
    .await;
    work(
        &store,
        local(14, 0),
        42,
        "Zed",
        "login.rs - os-clovy",
        &[
            "Pull requests Issues Marketplace Explore Codespaces Sign out",
            "Fixed the login redirect for KAN-123 in src/auth/login.rs today",
        ],
    )
    .await;
    build_timeline(&store, local(16, 0)).await;
    (dir, store)
}

#[tokio::test]
async fn completed_hours_get_reports_with_measured_minutes_and_fold_into_workstreams() {
    let (_dir, store) = day_store().await;
    let settings = settings();
    let zone = zone();
    let sources = sources();
    let provider = FakeProvider::new(|request| {
        Ok(match kind(request) {
            "hour" => hour_answer(),
            "fold" => json!({"placements": [{
                "workstream_id": 0, "title": "Login fix", "summary": "Fixing the login redirect.",
                "activities": [1, 2]
            }]}),
            _ => summary_answer(),
        })
    });
    let deps = deps(
        &store,
        &settings,
        &provider,
        &sources,
        &zone,
        UiLocale::En,
        local(16, 5),
    );
    let result = pipeline::catch_up_day(&deps, day(), false).await.unwrap();
    assert_eq!((result.reported, result.folded, result.failed), (2, 2, 0));

    let reports = db::hour_reports_of_day(&store, "2026-10-04").await.unwrap();
    let hours: Vec<&str> = reports.iter().map(|report| report.hour.as_str()).collect();
    assert_eq!(hours, vec!["2026-10-04T13", "2026-10-04T14"]);
    let fourteen = &reports[1];
    assert_eq!(fourteen.active_minutes, 42);
    let minutes: Vec<i64> = fourteen
        .activities
        .iter()
        .map(|activity| activity.minutes)
        .collect();
    assert_eq!(minutes, vec![21, 21]);

    // The hour prompt carries the distilled text (entities kept, chrome cut),
    // the measured sessions, and the coding-agent block of the hour.
    let hour_calls = provider.calls_of(prompts::HOUR_REPORT);
    let prompt = &hour_calls[1].prompt;
    assert!(prompt.contains("Measured active time: 42 min"), "{prompt}");
    assert!(prompt.contains("KAN-123") && prompt.contains("src/auth/login.rs"));
    assert!(!prompt.contains("Marketplace"));
    assert!(prompt.contains("claude_code") && prompt.contains("Fix login redirect"));
    assert!(hour_calls[1]
        .system
        .as_deref()
        .unwrap()
        .ends_with("Write every text value in English."));

    // A second pass has nothing left to do.
    let again = pipeline::catch_up_day(&deps, day(), false).await.unwrap();
    assert_eq!((again.reported, again.folded), (0, 0));
    assert_eq!(provider.calls().len(), 4);

    let workstreams = db::workstreams_of_day(&store, "2026-10-04").await.unwrap();
    assert_eq!(workstreams.len(), 2);
    assert_eq!(workstreams[1].minutes, 42);
}

#[tokio::test]
async fn a_new_hour_joins_its_workstream_and_leaves_the_others_untouched() {
    let (_dir, store) = day_store().await;
    let settings = settings();
    let zone = zone();
    let sources = sources();
    let provider = FakeProvider::new(|request| {
        Ok(match kind(request) {
            "hour" if request.prompt.contains("HOUR 2026-10-04 13:00") => json!({
                "summary": "Planned the login fix and answered email.",
                "activities": [
                    {"description": "Planned the login fix", "minutes": 20},
                    {"description": "Answered email", "minutes": 10}
                ]
            }),
            "hour" => hour_answer(),
            "fold" if request.prompt.contains("NEW HOUR 13:00") => json!({"placements": [
                {"workstream_id": 0, "title": "Corrigir bug de login", "summary": "Planejou a correção.", "activities": [1]},
                {"workstream_id": 0, "title": "Email", "summary": "Respondeu emails.", "activities": [2]}
            ]}),
            "fold" => json!({"placements": [{
                "workstream_id": 1, "title": "Something else", "summary": "Planejou e corrigiu o login.",
                "activities": [1, 2]
            }]}),
            _ => summary_answer(),
        })
    });
    let deps = deps(
        &store,
        &settings,
        &provider,
        &sources,
        &zone,
        UiLocale::En,
        local(14, 30),
    );
    pipeline::catch_up_day(&deps, day(), false).await.unwrap();
    let before = db::workstreams_of_day(&store, "2026-10-04").await.unwrap();
    assert_eq!(before.len(), 2);

    let deps = super::test_support::deps(
        &store,
        &settings,
        &provider,
        &sources,
        &zone,
        UiLocale::En,
        local(16, 0),
    );
    pipeline::catch_up_day(&deps, day(), false).await.unwrap();
    let fold_prompt = &provider.calls_of(prompts::WORKSTREAM_FOLD)[1].prompt;
    assert!(
        fold_prompt.contains("\"title\": \"Corrigir bug de login\""),
        "{fold_prompt}"
    );

    let after = db::workstreams_of_day(&store, "2026-10-04").await.unwrap();
    assert_eq!(after.len(), 2, "no workstream opened");
    assert_eq!(
        after[0].title, "Corrigir bug de login",
        "title never rewritten"
    );
    assert_eq!(after[0].summary, "Planejou e corrigiu o login.");
    let hours: Vec<&str> = after[0]
        .hours
        .iter()
        .map(|hour| hour.hour.as_str())
        .collect();
    assert_eq!(hours, vec!["2026-10-04T13", "2026-10-04T14"]);
    assert_eq!(after[0].minutes, before[0].minutes + 42);
    assert_eq!(after[1], before[1], "the other workstream is unchanged");
}

#[tokio::test]
async fn portuguese_summary_reads_meetings_and_its_panels_match_the_day_stats() {
    let (_dir, store) = day_store().await;
    let settings = settings();
    let zone = zone();
    let sources = sources();
    let provider = FakeProvider::new(|request| {
        Ok(match kind(request) {
            "hour" => hour_answer(),
            "fold" => json!({"placements": []}),
            _ => summary_answer(),
        })
    });
    let now = local(19, 0);
    let deps = deps(
        &store,
        &settings,
        &provider,
        &sources,
        &zone,
        UiLocale::PtBr,
        now,
    );
    pipeline::catch_up_day(&deps, day(), false).await.unwrap();
    let summary = pipeline::generate_summary(&deps, day(), SummaryTrigger::Manual)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(summary.locale, "pt-BR");
    assert_eq!(summary.hours_covered, 2);
    assert_eq!(
        summary.standup.done,
        vec!["Corrigiu o redirecionamento do login (KAN-123)"]
    );

    let call = provider.calls_of(prompts::DAY_SUMMARY).pop().unwrap();
    assert!(call
        .system
        .as_deref()
        .unwrap()
        .contains("Brazilian Portuguese"));
    assert!(
        call.prompt.contains("meeting \"Weekly sync\" (30 min)"),
        "{}",
        call.prompt
    );
    assert!(call.prompt.contains("Decided to ship the beta on Friday"));
    assert!(call.prompt.contains("CODING-AGENT SESSIONS"));

    let context = ReadContext {
        store: &store,
        settings: &settings,
        sources: &sources,
        zone: &zone,
        now,
    };
    let (stored, _, reports, panels) = pipeline::day_view(&context, day()).await.unwrap();
    assert_eq!(stored.as_ref(), Some(&summary));
    assert_eq!(reports.len(), 2);
    let (from, to) = schedule::day_bounds(&zone, day()).unwrap();
    let stats = timeline_view(&store, &settings, from, to, now)
        .await
        .unwrap()
        .stats;
    assert_eq!(panels.focused_ms, stats.focused_ms);
    assert_eq!(panels.idle_ms, stats.idle_ms);
    assert_eq!(panels.away_ms, stats.away_ms);
    assert_eq!(panels.categories, stats.categories);
    assert_eq!(panels.hours.len(), 24);
    assert_eq!(
        panels.hours.iter().map(|hour| hour.focused_ms).sum::<i64>(),
        stats.focused_ms
    );
    assert_eq!(
        panels.workstreams.iter().map(|w| w.minutes).sum::<i64>(),
        30 + 42
    );
    assert_eq!((panels.meeting_count, panels.meeting_ms), (1, 30 * 60_000));
    assert_eq!(
        (
            panels.coding_agent_blocks,
            panels.coding_agent_active_seconds
        ),
        (1, 1500)
    );
}

#[tokio::test]
async fn failed_hours_back_off_and_a_forced_run_retries_them() {
    let (_dir, store) = day_store().await;
    let settings = settings();
    let zone = zone();
    let sources = sources();
    let provider = FakeProvider::new(|_| Err(LlmError::TimedOut));
    let deps = deps(
        &store,
        &settings,
        &provider,
        &sources,
        &zone,
        UiLocale::En,
        local(16, 0),
    );
    let result = pipeline::catch_up_day(&deps, day(), false).await.unwrap();
    assert_eq!((result.reported, result.failed), (0, 2));
    let record = db::run_record(&store, "hour:2026-10-04T14")
        .await
        .unwrap()
        .unwrap();
    assert_eq!((record.attempts, record.outcome), (1, RunOutcome::Failed));

    pipeline::catch_up_day(&deps, day(), false).await.unwrap();
    assert_eq!(provider.calls().len(), 2, "waiting for the retry time");
    pipeline::catch_up_day(&deps, day(), true).await.unwrap();
    assert_eq!(provider.calls().len(), 4, "the user asked: retry now");
}

#[tokio::test]
async fn a_fold_that_keeps_failing_still_places_the_hour() {
    let (_dir, store) = day_store().await;
    let settings = settings();
    let zone = zone();
    let sources = sources();
    let provider = FakeProvider::new(|request| match kind(request) {
        "hour" => Ok(hour_answer()),
        _ => Err(LlmError::InvalidOutput("no".into())),
    });
    // Each hour folds only after the one before it: hour 13 uses its
    // attempts, lands without a call, then hour 14 does the same.
    let mut now = local(16, 0);
    for _ in 0..=2 * schedule::MAX_ATTEMPTS {
        let deps = deps(
            &store,
            &settings,
            &provider,
            &sources,
            &zone,
            UiLocale::En,
            now,
        );
        pipeline::catch_up_day(&deps, day(), true).await.unwrap();
        now += chrono::Duration::hours(1);
    }
    let folds = provider.calls_of(prompts::WORKSTREAM_FOLD).len() as i64;
    assert_eq!(folds, 2 * schedule::MAX_ATTEMPTS);
    let workstreams = db::workstreams_of_day(&store, "2026-10-04").await.unwrap();
    assert_eq!(workstreams.len(), 2, "each hour became its own workstream");
    assert!(db::unfolded_reports(&store, "2026-10-04")
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn days_without_anything_have_no_summary_and_no_call() {
    let (_dir, store) = store().await;
    let settings = settings();
    let zone = zone();
    let sources = super::sources::tests::FixedSources::default();
    let provider = FakeProvider::new(|_| Ok(summary_answer()));
    let deps = super::pipeline::Deps {
        store: &store,
        settings: &settings,
        generator: &provider,
        embedder: &NoEmbedder,
        sources: &sources,
        zone: &zone,
        locale: UiLocale::En,
        now: local(19, 0),
    };
    assert!(
        pipeline::generate_summary(&deps, day(), SummaryTrigger::Scheduled)
            .await
            .unwrap()
            .is_none()
    );
    assert!(provider.calls().is_empty());
    assert_eq!(day_key(day()), "2026-10-04");
}

/// The app's coding-agent path (`sources::coding_blocks_between` over the
/// store), without meetings.
struct StoredBlocks;

impl super::sources::DaySources for StoredBlocks {
    fn meetings<'a>(
        &'a self,
        _from: chrono::DateTime<chrono::Utc>,
        _to: chrono::DateTime<chrono::Utc>,
        _zone: &'a dyn super::schedule::LocalZone,
    ) -> futures_util::future::BoxFuture<'a, Vec<super::sources::MeetingNote>> {
        Box::pin(async { Vec::new() })
    }

    fn coding_blocks<'a>(
        &'a self,
        store: &'a crate::activity::store::ActivityStore,
        from: chrono::DateTime<chrono::Utc>,
        to: chrono::DateTime<chrono::Utc>,
        zone: &'a dyn super::schedule::LocalZone,
    ) -> futures_util::future::BoxFuture<'a, Vec<super::sources::CodingBlock>> {
        Box::pin(super::sources::coding_blocks_between(store, from, to, zone))
    }
}

async fn store_block(
    store: &crate::activity::store::ActivityStore,
    source: crate::coding_agents::SourceId,
    session: &str,
    (start, end): (chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>),
    active_seconds: i64,
    first_prompt: &str,
) {
    let block = crate::coding_agents::store::NewBlock {
        source,
        session_id: session.into(),
        started_at: start,
        ended_at: end,
        cwd: Some("/Users/dev/os-clovy".into()),
        project: Some("os-clovy".into()),
        title: None,
        first_prompt: Some(first_prompt.into()),
        prompt_count: 1,
        reply_count: 1,
        active_seconds,
        transcript: format!("user: {first_prompt}"),
        sealed: true,
    };
    store
        .upsert_coding_agent_block(&block, local(18, 0))
        .await
        .unwrap();
}

#[tokio::test]
async fn coding_agent_blocks_written_by_ingestion_reach_hour_reports_summary_and_panels() {
    use crate::coding_agents::SourceId;
    use chrono::Duration;

    let (_dir, store) = day_store().await;
    // Claude Code 14:05-14:35, summarized by ingestion.
    store_block(
        &store,
        SourceId::ClaudeCode,
        "claude-1",
        (local(14, 5), local(14, 35)),
        1500,
        "fix the login redirect loop",
    )
    .await;
    let claude = store
        .coding_agent_blocks_between(local(14, 0), local(15, 0))
        .await
        .unwrap()
        .into_iter()
        .find(|block| block.session_id == "claude-1")
        .unwrap();
    store
        .record_coding_agent_summary(
            claude.id,
            "Patched the redirect loop and added a test",
            "cli:claude",
            local(18, 0),
        )
        .await
        .unwrap();
    // Codex 13:50-14:10, not summarized yet: half of it in each hour.
    store_block(
        &store,
        SourceId::Codex,
        "codex-1",
        (local(13, 50), local(14, 10)),
        1200,
        "add a pagination cursor to the notes API",
    )
    .await;
    // Ends exactly when the day starts: not part of it.
    store_block(
        &store,
        SourceId::Cursor,
        "cursor-1",
        (local(0, 0) - Duration::minutes(30), local(0, 0)),
        900,
        "late night refactor",
    )
    .await;

    let settings = settings();
    let zone = zone();
    let sources = StoredBlocks;
    let provider = FakeProvider::new(|request| {
        Ok(match kind(request) {
            "hour" => hour_answer(),
            "fold" => json!({"placements": []}),
            _ => summary_answer(),
        })
    });
    let now = local(19, 0);
    let deps = super::pipeline::Deps {
        store: &store,
        settings: &settings,
        generator: &provider,
        embedder: &NoEmbedder,
        sources: &sources,
        zone: &zone,
        locale: UiLocale::En,
        now,
    };
    pipeline::catch_up_day(&deps, day(), false).await.unwrap();
    pipeline::generate_summary(&deps, day(), SummaryTrigger::Manual)
        .await
        .unwrap()
        .unwrap();

    let hour_calls = provider.calls_of(prompts::HOUR_REPORT);
    let (thirteen, fourteen) = (&hour_calls[0].prompt, &hour_calls[1].prompt);
    assert!(
        thirteen.contains("13:50-14:10 · Codex · 10 min")
            && thirteen.contains("add a pagination cursor to the notes API"),
        "{thirteen}"
    );
    assert!(!thirteen.contains("Claude Code"), "{thirteen}");
    assert!(
        fourteen.contains("14:05-14:35 · Claude Code · 25 min · project os-clovy")
            && fourteen.contains("Patched the redirect loop and added a test"),
        "{fourteen}"
    );
    assert!(
        fourteen.contains("13:50-14:10 · Codex · 10 min"),
        "{fourteen}"
    );
    assert!(!fourteen.contains("fix the login redirect loop"));

    let summary_prompt = &provider.calls_of(prompts::DAY_SUMMARY)[0].prompt;
    assert!(summary_prompt.contains("Patched the redirect loop and added a test"));
    assert!(summary_prompt.contains("add a pagination cursor to the notes API"));
    assert!(!summary_prompt.contains("late night refactor"));

    let context = ReadContext {
        store: &store,
        settings: &settings,
        sources: &sources,
        zone: &zone,
        now,
    };
    let (_, _, _, panels) = pipeline::day_view(&context, day()).await.unwrap();
    assert_eq!(
        (
            panels.coding_agent_blocks,
            panels.coding_agent_active_seconds
        ),
        (2, 1500 + 1200)
    );
}
