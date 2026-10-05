//! What the day summary reads besides the timeline: meetings and notes the
//! user recorded in Clovy (main database) and coding-agent session blocks.

use chrono::{DateTime, Utc};
use futures_util::future::BoxFuture;
use serde::Serialize;
use sqlx::query::query;
use sqlx::row::Row;
use tauri::AppHandle;

use super::schedule::LocalZone;
use crate::activity::store::ActivityStore;
use crate::coding_agents::store::CodingAgentBlock;
use crate::coding_agents::CodingAgentsStatusDto;

const EXCERPT_CHARS: usize = 400;

/// One row of the notes query: note id, title, content, recording start and
/// end (none without a recording), and creation time.
pub type NoteRow = (
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    String,
);

/// A note recorded in Clovy that day: a meeting when it has a recording.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingNote {
    pub note_id: String,
    pub title: String,
    pub started_at: String,
    pub ended_at: String,
    /// Local "HH:MM".
    pub start_local: String,
    pub end_local: String,
    pub duration_ms: i64,
    pub recorded: bool,
    #[serde(skip)]
    pub excerpt: Option<String>,
}

impl MeetingNote {
    pub fn prompt_line(&self) -> String {
        let kind = if self.recorded { "meeting" } else { "note" };
        let title = if self.title.trim().is_empty() {
            "Untitled"
        } else {
            self.title.trim()
        };
        let mut line = format!(
            "{}-{} · {kind} \"{title}\" ({} min)",
            self.start_local,
            self.end_local,
            self.duration_ms / 60_000
        );
        if let Some(excerpt) = &self.excerpt {
            line.push_str(": ");
            line.push_str(excerpt);
        }
        line
    }
}

/// A coding-agent session block (coding-agent ingestion), as the day
/// summary uses it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodingBlock {
    pub source: String,
    pub project: Option<String>,
    pub title: Option<String>,
    pub started_at: String,
    pub ended_at: String,
    pub start_local: String,
    pub end_local: String,
    pub active_seconds: i64,
    #[serde(skip)]
    pub summary: Option<String>,
}

impl CodingBlock {
    pub fn prompt_line(&self) -> String {
        let mut line = format!(
            "{}-{} · {} · {} min",
            self.start_local,
            self.end_local,
            self.source,
            self.active_seconds / 60
        );
        if let Some(project) = &self.project {
            line.push_str(&format!(" · project {project}"));
        }
        if let Some(title) = &self.title {
            line.push_str(&format!(" · \"{title}\""));
        }
        if let Some(summary) = &self.summary {
            line.push_str(": ");
            line.push_str(summary);
        }
        line
    }
}

pub trait DaySources: Send + Sync {
    /// Notes recorded or created in `[from, to)`, oldest first.
    fn meetings<'a>(
        &'a self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
        zone: &'a dyn LocalZone,
    ) -> BoxFuture<'a, Vec<MeetingNote>>;

    /// Coding-agent blocks overlapping `[from, to)`, oldest first.
    fn coding_blocks<'a>(
        &'a self,
        store: &'a ActivityStore,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
        zone: &'a dyn LocalZone,
    ) -> BoxFuture<'a, Vec<CodingBlock>>;

    /// Whether coding-agent ingestion has read every enabled agent's
    /// transcripts up to `until` (true with every source off).
    fn coding_agents_read_until(&self, until: DateTime<Utc>) -> bool;
}

/// From the ingestion status: read past `until` when no source is on, when
/// the last scan finished at or after it, or when scanning fails (its error
/// shows in Settings; a broken source must not hold the summary forever).
pub fn ingestion_read_until(status: &CodingAgentsStatusDto, until: DateTime<Utc>) -> bool {
    if !status.supported || !status.sources.iter().any(|source| source.enabled) {
        return true;
    }
    if status.last_error.is_some() {
        return true;
    }
    status
        .last_scan_at
        .as_deref()
        .and_then(parse_time)
        .is_some_and(|at| at >= until)
}

pub fn excerpt(text: &str) -> Option<String> {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    (!collapsed.is_empty()).then(|| collapsed.chars().take(EXCERPT_CHARS).collect())
}

pub fn hhmm(zone: &dyn LocalZone, at: DateTime<Utc>) -> String {
    zone.local(at).format("%H:%M").to_string()
}

fn parse_time(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|at| at.with_timezone(&Utc))
}

/// Groups note rows (one per recording session, or one without a recording)
/// into notes: a meeting spans its first recording start to its last end; a
/// note without a recording sits at its creation time. A recording counts
/// only its part inside `[from, to)`, so a meeting across midnight splits its
/// duration between the two days.
pub fn meetings_from_rows(
    rows: Vec<NoteRow>,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    zone: &dyn LocalZone,
    now: DateTime<Utc>,
) -> Vec<MeetingNote> {
    let mut notes: Vec<MeetingNote> = Vec::new();
    for (note_id, title, content, started, ended, created) in rows {
        let (start, end, recorded) = match started.as_deref().and_then(parse_time) {
            Some(start) => {
                let end = ended
                    .as_deref()
                    .and_then(parse_time)
                    .unwrap_or(now)
                    .max(start);
                (start, end, true)
            }
            None => match parse_time(&created) {
                Some(at) => (at, at, false),
                None => continue,
            },
        };
        let overlaps = if recorded {
            start < to && (end > from || start >= from)
        } else {
            start >= from && start < to
        };
        let inside_ms = if recorded {
            (end.min(to) - start.max(from)).num_milliseconds().max(0)
        } else {
            0
        };
        if !overlaps {
            continue;
        }
        if let Some(existing) = notes.iter_mut().find(|note| note.note_id == note_id) {
            let existing_start = parse_time(&existing.started_at).unwrap_or(start);
            let existing_end = parse_time(&existing.ended_at).unwrap_or(end);
            if recorded {
                existing.duration_ms += inside_ms;
                existing.recorded = true;
            }
            let start = start.min(existing_start);
            let end = end.max(existing_end);
            existing.started_at = start.to_rfc3339();
            existing.ended_at = end.to_rfc3339();
            existing.start_local = hhmm(zone, start);
            existing.end_local = hhmm(zone, end);
            continue;
        }
        notes.push(MeetingNote {
            note_id,
            title,
            started_at: start.to_rfc3339(),
            ended_at: end.to_rfc3339(),
            start_local: hhmm(zone, start),
            end_local: hhmm(zone, end),
            duration_ms: inside_ms,
            recorded,
            excerpt: excerpt(&content),
        });
    }
    notes.sort_by(|a, b| a.started_at.cmp(&b.started_at));
    notes
}

/// The app's sources: the main database and the activity database.
pub struct AppSources {
    pub app: AppHandle,
}

impl DaySources for AppSources {
    fn meetings<'a>(
        &'a self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
        zone: &'a dyn LocalZone,
    ) -> BoxFuture<'a, Vec<MeetingNote>> {
        Box::pin(async move {
            let Ok(repositories) = crate::commands::repositories(&self.app).await else {
                return Vec::new();
            };
            let profile = crate::commands::active_profile(&self.app);
            let from_text = from.to_rfc3339();
            let to_text = to.to_rfc3339();
            let rows = query(
                "SELECT n.id, n.title, COALESCE(n.edited_content, n.generated_content, ''),
                        r.started_at, r.ended_at, n.created_at
                 FROM notes n LEFT JOIN recording_sessions r ON r.note_id = n.id
                 WHERE n.profile = ? AND (
                    (r.started_at IS NOT NULL
                      AND julianday(r.started_at) < julianday(?)
                      AND (r.ended_at IS NULL OR julianday(r.ended_at) > julianday(?)
                        OR julianday(r.started_at) >= julianday(?)))
                    OR (r.id IS NULL
                      AND julianday(n.created_at) >= julianday(?)
                      AND julianday(n.created_at) < julianday(?)))
                 ORDER BY COALESCE(r.started_at, n.created_at)",
            )
            .bind(profile)
            .bind(&to_text)
            .bind(&from_text)
            .bind(&from_text)
            .bind(&from_text)
            .bind(&to_text)
            .fetch_all(&repositories.pool)
            .await
            .unwrap_or_else(|error| {
                tracing::warn!(%error, "day summary: meetings could not be read");
                Vec::new()
            });
            let rows = rows
                .iter()
                .map(|row| {
                    (
                        row.get::<String, _>(0),
                        row.get::<String, _>(1),
                        row.get::<String, _>(2),
                        row.get::<Option<String>, _>(3),
                        row.get::<Option<String>, _>(4),
                        row.get::<String, _>(5),
                    )
                })
                .collect();
            meetings_from_rows(rows, from, to, zone, Utc::now())
        })
    }

    fn coding_blocks<'a>(
        &'a self,
        store: &'a ActivityStore,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
        zone: &'a dyn LocalZone,
    ) -> BoxFuture<'a, Vec<CodingBlock>> {
        Box::pin(coding_blocks_between(store, from, to, zone))
    }

    fn coding_agents_read_until(&self, until: DateTime<Utc>) -> bool {
        use tauri::Manager;
        self.app
            .try_state::<crate::coding_agents::CodingAgentsState>()
            .is_none_or(|state| {
                ingestion_read_until(&crate::coding_agents::coding_agents_status(state), until)
            })
    }
}

/// Coding-agent blocks from the activity database
/// (`ActivityStore::coding_agent_blocks_between`, docs/coding-agent-sessions.md).
pub(crate) async fn coding_blocks_between(
    store: &ActivityStore,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    zone: &dyn LocalZone,
) -> Vec<CodingBlock> {
    match store.coding_agent_blocks_between(from, to).await {
        Ok(blocks) => blocks_in_window(blocks, from, to, zone),
        Err(error) => {
            tracing::warn!(%error, "day intelligence: coding-agent blocks could not be read");
            Vec::new()
        }
    }
}

/// Stored blocks as the hour report and the day summary use them. A block
/// counts only inside `[from, to)` (one ending exactly at `from` is not in
/// it), and its active time is split by the share of the block inside, so a
/// block across two hours or two days is not counted twice. Text: the block's
/// summary once summarized, else its first prompt.
pub fn blocks_in_window(
    blocks: Vec<CodingAgentBlock>,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    zone: &dyn LocalZone,
) -> Vec<CodingBlock> {
    blocks
        .into_iter()
        .filter_map(|block| {
            let start = parse_time(&block.started_at)?;
            let end = parse_time(&block.ended_at)?.max(start);
            if start >= to || (end <= from && start < from) {
                return None;
            }
            let span_ms = (end - start).num_milliseconds();
            let inside_ms = (end.min(to) - start.max(from)).num_milliseconds().max(0);
            let active_seconds = if span_ms > 0 {
                ((block.active_seconds as i128 * inside_ms as i128 + span_ms as i128 / 2)
                    / span_ms as i128) as i64
            } else {
                block.active_seconds
            };
            Some(CodingBlock {
                source: block.source.display_name().to_string(),
                project: block.project,
                title: block.title,
                start_local: hhmm(zone, start),
                end_local: hhmm(zone, end),
                started_at: block.started_at,
                ended_at: block.ended_at,
                active_seconds,
                summary: block.summary.or(block.first_prompt),
            })
        })
        .collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::super::schedule::FixedZone;
    use super::*;

    /// Fixed meetings and blocks, filtered by overlap like the real sources.
    /// `ingestion_scanned_at`: the last coding-agent scan (`None`: up to date).
    #[derive(Default)]
    pub(crate) struct FixedSources {
        pub meetings: Vec<MeetingNote>,
        pub blocks: Vec<CodingBlock>,
        pub ingestion_scanned_at: std::sync::Mutex<Option<DateTime<Utc>>>,
    }

    fn overlaps(start: &str, end: &str, from: DateTime<Utc>, to: DateTime<Utc>) -> bool {
        let start = parse_time(start).unwrap();
        let end = parse_time(end).unwrap();
        start < to && (end > from || start >= from)
    }

    impl DaySources for FixedSources {
        fn meetings<'a>(
            &'a self,
            from: DateTime<Utc>,
            to: DateTime<Utc>,
            _zone: &'a dyn LocalZone,
        ) -> BoxFuture<'a, Vec<MeetingNote>> {
            let found = self
                .meetings
                .iter()
                .filter(|note| overlaps(&note.started_at, &note.ended_at, from, to))
                .cloned()
                .collect();
            Box::pin(async move { found })
        }

        fn coding_blocks<'a>(
            &'a self,
            _store: &'a ActivityStore,
            from: DateTime<Utc>,
            to: DateTime<Utc>,
            _zone: &'a dyn LocalZone,
        ) -> BoxFuture<'a, Vec<CodingBlock>> {
            let found = self
                .blocks
                .iter()
                .filter(|block| overlaps(&block.started_at, &block.ended_at, from, to))
                .cloned()
                .collect();
            Box::pin(async move { found })
        }

        fn coding_agents_read_until(&self, until: DateTime<Utc>) -> bool {
            crate::day_intelligence::lock(&self.ingestion_scanned_at)
                .is_none_or(|scanned| scanned >= until)
        }
    }

    #[test]
    fn recordings_of_one_note_merge_and_plain_notes_count_by_creation() {
        let zone = FixedZone(chrono::FixedOffset::west_opt(3 * 3600).unwrap());
        let from = parse_time("2026-10-04T03:00:00Z").unwrap();
        let to = parse_time("2026-10-05T03:00:00Z").unwrap();
        let now = parse_time("2026-10-04T23:00:00Z").unwrap();
        let rows = vec![
            (
                "n1".into(),
                "Weekly sync".into(),
                "## Decisions\n\nShip   the beta".into(),
                Some("2026-10-04T18:00:00.000Z".into()),
                Some("2026-10-04T18:20:00.000Z".into()),
                "2026-10-04T18:00:00.000Z".into(),
            ),
            (
                "n1".into(),
                "Weekly sync".into(),
                String::new(),
                Some("2026-10-04T18:25:00.000Z".into()),
                Some("2026-10-04T18:35:00.000Z".into()),
                "2026-10-04T18:00:00.000Z".into(),
            ),
            (
                "n2".into(),
                "Ideas".into(),
                "draft".into(),
                None,
                None,
                "2026-10-04T12:00:00Z".into(),
            ),
            (
                "n3".into(),
                "Yesterday".into(),
                String::new(),
                None,
                None,
                "2026-10-03T12:00:00Z".into(),
            ),
        ];
        let notes = meetings_from_rows(rows, from, to, &zone, now);
        assert_eq!(notes.len(), 2);
        assert_eq!(notes[0].title, "Ideas");
        assert!(!notes[0].recorded);
        let meeting = &notes[1];
        assert_eq!(
            (meeting.start_local.as_str(), meeting.end_local.as_str()),
            ("15:00", "15:35")
        );
        assert_eq!(meeting.duration_ms, 30 * 60_000);
        assert_eq!(
            meeting.excerpt.as_deref(),
            Some("## Decisions Ship the beta")
        );
        assert!(meeting
            .prompt_line()
            .contains("meeting \"Weekly sync\" (30 min)"));
    }

    #[test]
    fn a_meeting_across_midnight_splits_its_minutes_between_the_days() {
        let zone = FixedZone(chrono::FixedOffset::west_opt(3 * 3600).unwrap());
        let midnight = parse_time("2026-10-05T03:00:00Z").unwrap();
        let day = chrono::Duration::days(1);
        let now = parse_time("2026-10-05T12:00:00Z").unwrap();
        let row = |id: &str, start: &str, end: &str| {
            (
                id.to_string(),
                "Late call".to_string(),
                String::new(),
                Some(start.to_string()),
                Some(end.to_string()),
                start.to_string(),
            )
        };
        let rows = || {
            vec![
                row("n1", "2026-10-05T02:50:00Z", "2026-10-05T03:10:00Z"),
                row("n2", "2026-10-05T02:30:00Z", "2026-10-05T03:00:00Z"),
            ]
        };

        let before = meetings_from_rows(rows(), midnight - day, midnight, &zone, now);
        let after = meetings_from_rows(rows(), midnight, midnight + day, &zone, now);

        let minutes = |notes: &[MeetingNote]| {
            notes
                .iter()
                .map(|note| (note.note_id.clone(), note.duration_ms / 60_000))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            minutes(&before),
            vec![("n2".to_string(), 30), ("n1".to_string(), 10)]
        );
        // n2 ended exactly at midnight: it is not part of the next day.
        assert_eq!(minutes(&after), vec![("n1".to_string(), 10)]);
    }

    #[test]
    fn ingestion_holds_the_summary_until_a_scan_passes_the_last_hour() {
        use crate::coding_agents::{SourceId, SourceStatusDto};
        let until = parse_time("2026-10-04T21:00:00Z").unwrap();
        let status =
            |enabled: bool, scanned: Option<&str>, error: Option<&str>| CodingAgentsStatusDto {
                supported: true,
                sources: vec![SourceStatusDto {
                    id: SourceId::Codex,
                    name: "Codex",
                    enabled,
                    present: true,
                }],
                database_ready: true,
                last_scan_at: scanned.map(str::to_string),
                last_error: error.map(str::to_string),
            };
        assert!(ingestion_read_until(&status(false, None, None), until));
        assert!(!ingestion_read_until(&status(true, None, None), until));
        assert!(!ingestion_read_until(
            &status(true, Some("2026-10-04T20:59:00.000000Z"), None),
            until
        ));
        assert!(ingestion_read_until(
            &status(true, Some("2026-10-04T21:00:30.000000Z"), None),
            until
        ));
        assert!(ingestion_read_until(
            &status(
                true,
                Some("2026-10-04T20:00:00.000000Z"),
                Some("permission denied")
            ),
            until
        ));
    }
}
