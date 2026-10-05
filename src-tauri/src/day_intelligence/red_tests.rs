//! The day-intelligence spec scenarios that are unit-level, as written in
//! the OpenSpec change `add-activity-intelligence` (spec `day-intelligence`).

use chrono::{Duration, NaiveDate, NaiveDateTime};

use super::distill::{distill, SessionText};
use super::embedder::NoEmbedder;
use super::hour::{assemble, normalize_minutes, HourAnswer};
use super::notify::{NoticeKind, NoticeQueue};
use super::schedule::{auto_summary_due, FixedZone, LocalZone, RunOutcome, RunRecord};
use super::workstreams::{plan_fold, Append, Placement, WorkstreamDto, WorkstreamHourDto};
use crate::activity::settings::{ActivityNotificationSettings, QuietHours};

fn zone() -> FixedZone {
    FixedZone(chrono::FixedOffset::west_opt(3 * 3600).unwrap())
}

fn local(day: u32, hour: u32, minute: u32) -> NaiveDateTime {
    NaiveDate::from_ymd_opt(2026, 10, day)
        .unwrap()
        .and_hms_opt(hour, minute, 0)
        .unwrap()
}

/// Scenario "Minutos normalizados": 42 active minutes, the model estimates
/// 30 and 30 → the report shows 21 and 21.
#[test]
fn reported_minutes_are_the_measured_minutes_split_by_the_model_weights() {
    assert_eq!(normalize_minutes(42, &[30.0, 30.0]), vec![21, 21]);
    let answer = HourAnswer {
        summary: "s".into(),
        activities: vec![
            ("Fixed the login bug".into(), 30.0),
            ("Reviewed a PR".into(), 30.0),
        ],
    };
    let minutes: Vec<i64> = assemble(&answer, 42)
        .iter()
        .map(|activity| activity.minutes)
        .collect();
    assert_eq!(minutes, vec![21, 21]);
}

/// Scenario "Entidades preservadas": repeated interface lines go, the
/// `KAN-123` line and the file path stay.
#[tokio::test]
async fn distillation_drops_repeated_interface_lines_and_keeps_entities() {
    let chrome = "You are signed in as gui and you have no new notifications";
    let sessions: Vec<SessionText> = (1..=4)
        .map(|id| SessionText {
            session_id: id,
            app: "Arc".into(),
            window: "GitHub".into(),
            time: format!("14:0{id}"),
            seconds: 300,
            documents: vec![(
                format!("14:0{id}"),
                if id == 1 {
                    format!("{chrome}\nMoved KAN-123 to review after editing src/auth/login.rs")
                } else {
                    format!("{chrome}\nRead the discussion about release number {id} with the team")
                },
            )],
        })
        .collect();
    let out = distill(&sessions, "=== HOUR ===", &NoEmbedder).await;
    assert!(!out.body.contains("no new notifications"), "{}", out.body);
    assert!(out.body.contains("KAN-123"));
    assert!(out.body.contains("src/auth/login.rs"));
}

/// Scenario "Hora atribuída a frente existente": the hour is appended to
/// "Corrigir bug de login" and the other workstreams do not change.
#[test]
fn an_hour_continuing_a_workstream_is_appended_to_it_only() {
    let prior = vec![
        WorkstreamDto {
            id: 7,
            title: "Corrigir bug de login".into(),
            summary: "Investigou o redirecionamento.".into(),
            minutes: 35,
            hours: vec![WorkstreamHourDto {
                hour: "2026-10-04T13".into(),
                minutes: 35,
                note: "Investigou".into(),
            }],
        },
        WorkstreamDto {
            id: 8,
            title: "Planejamento".into(),
            summary: "Revisou o quadro.".into(),
            minutes: 10,
            hours: Vec::new(),
        },
    ];
    let activities = assemble(
        &HourAnswer {
            summary: "s".into(),
            activities: vec![("Corrigiu o redirecionamento".into(), 1.0)],
        },
        40,
    );
    let plan = plan_fold(
        &prior,
        &activities,
        &[Placement {
            workstream_id: 7,
            title: "Corrigir bug de login".into(),
            summary: "Investigou e corrigiu o redirecionamento.".into(),
            activities: vec![1],
        }],
    );
    assert!(plan.creates.is_empty());
    assert_eq!(
        plan.appends,
        vec![Append {
            workstream_id: 7,
            minutes: 40,
            note: "Corrigiu o redirecionamento".into(),
            summary: Some("Investigou e corrigiu o redirecionamento.".into()),
        }]
    );
}

/// Scenario "Mac dormindo às 18:00": asleep from 17:30 to 19:00, the summary
/// is generated right after waking, and only once that day.
#[test]
fn the_daily_summary_runs_once_right_after_waking() {
    let zone = zone();
    let mut record: Option<RunRecord> = None;
    let mut generated = Vec::new();
    // The scheduler ticks every minute while awake; no ticks while asleep.
    let awake = (0..30)
        .map(|minute| local(4, 17, minute))
        .chain((0..120).map(|minute| local(4, 19, 0) + Duration::minutes(minute)));
    for tick in awake {
        let now = zone.instant(tick).unwrap();
        if auto_summary_due(tick, "18:00", record.as_ref(), now) {
            generated.push(tick);
            record = Some(RunRecord {
                attempts: 1,
                outcome: RunOutcome::Ok,
                next_attempt_at: None,
            });
        }
    }
    assert_eq!(generated, vec![local(4, 19, 0)]);
}

/// Scenario "Horário silencioso": quiet 22:00-08:00, ready at 23:00 →
/// delivered only at 08:00.
#[test]
fn a_summary_ready_in_quiet_hours_is_delivered_when_they_end() {
    let zone = zone();
    let settings = ActivityNotificationSettings {
        enabled: true,
        quiet_hours: QuietHours {
            enabled: true,
            start: "22:00".into(),
            end: "08:00".into(),
        },
    };
    let mut queue = NoticeQueue::default();
    let ready = zone.instant(local(4, 23, 0)).unwrap();
    queue.raise(
        NoticeKind::SummaryReady,
        "2026-10-04".into(),
        None,
        ready,
        &zone,
        &settings,
    );
    let mut delivered_at = None;
    let mut now = ready;
    while now < ready + Duration::hours(12) {
        if !queue.take_due(now, &zone, &settings).is_empty() {
            delivered_at = Some(now);
            break;
        }
        now += Duration::minutes(1);
    }
    assert_eq!(delivered_at.map(|at| zone.local(at)), Some(local(5, 8, 0)));
}
