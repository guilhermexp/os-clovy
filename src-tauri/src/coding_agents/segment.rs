//! Cuts one normalized session into blocks. A new block starts when:
//!
//! - more than [`IDLE_GAP`] passed since the previous timestamped record;
//! - the block is at least [`TIME_BOX`] long and the record is a human prompt
//!   (so a block always ends on a complete agent turn and the next opens on
//!   what the user asked);
//! - the previous record was an explicit exit (a resumed session is new work).
//!
//! Records without a timestamp join the open block when they are turns.
//! Blocks with no turns (only bookkeeping) are dropped. Segmentation depends
//! only on earlier records, so appending to a transcript never moves an
//! existing block's start, which is the block's identity in the store.

use chrono::{DateTime, Duration, SecondsFormat, Utc};

use super::record::{Record, RecordKind, Session};

pub const IDLE_GAP: Duration = Duration::hours(1);
pub const TIME_BOX: Duration = Duration::hours(1);
/// Idle time between two records counted as active, at most.
pub const ACTIVE_GAP_CAP: Duration = Duration::minutes(2);
/// Rendered transcript kept per block (70% head, 30% tail when longer).
pub const TRANSCRIPT_CAP_CHARS: usize = 100_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Segment<'a> {
    pub started_at: DateTime<Utc>,
    pub ended_at: DateTime<Utc>,
    pub cwd: Option<String>,
    pub turns: Vec<&'a Record>,
    pub active_seconds: i64,
    /// The agent exited after the block's last turn.
    pub exited: bool,
}

impl Segment<'_> {
    pub fn prompt_count(&self) -> usize {
        self.count(RecordKind::Prompt)
    }

    pub fn reply_count(&self) -> usize {
        self.count(RecordKind::Reply)
    }

    fn count(&self, kind: RecordKind) -> usize {
        self.turns.iter().filter(|turn| turn.kind == kind).count()
    }

    pub fn first_prompt(&self) -> Option<&str> {
        self.turns
            .iter()
            .find(|turn| turn.kind == RecordKind::Prompt)
            .map(|turn| turn.body.as_str())
    }
}

pub fn segment(session: &Session) -> Vec<Segment<'_>> {
    let mut segments: Vec<Segment<'_>> = Vec::new();
    let mut cwd = session.cwd.clone();
    let mut previous: Option<(DateTime<Utc>, RecordKind)> = None;

    for record in &session.records {
        if let Some(record_cwd) = &record.cwd {
            cwd = Some(record_cwd.clone());
        }
        let Some(at) = record.at else {
            if let Some(open) = segments.last_mut().filter(|_| record.is_turn()) {
                open.turns.push(record);
                open.exited = false;
            }
            continue;
        };
        let starts_block = match (segments.last(), previous) {
            (Some(open), Some((previous_at, previous_kind))) => {
                previous_kind == RecordKind::End
                    || at - previous_at > IDLE_GAP
                    || (record.kind == RecordKind::Prompt && at - open.started_at >= TIME_BOX)
            }
            _ => true,
        };
        if starts_block {
            segments.push(Segment {
                started_at: at,
                ended_at: at,
                cwd: cwd.clone(),
                turns: Vec::new(),
                active_seconds: 0,
                exited: false,
            });
        } else if let Some((previous_at, _)) = previous {
            let gap = (at - previous_at).clamp(Duration::zero(), ACTIVE_GAP_CAP);
            if let Some(open) = segments.last_mut() {
                open.active_seconds += gap.num_seconds();
            }
        }
        let Some(open) = segments.last_mut() else {
            continue;
        };
        open.ended_at = open.ended_at.max(at);
        open.cwd = cwd.clone();
        match record.kind {
            RecordKind::End => open.exited = true,
            RecordKind::Event => {}
            _ => {
                open.turns.push(record);
                open.exited = false;
            }
        }
        previous = Some((at, record.kind));
    }
    segments.retain(|segment| !segment.turns.is_empty());
    segments
}

/// The block as text for the summarizer: one line group per turn.
pub fn render(segment: &Segment<'_>, agent_label: &str) -> String {
    let text = segment
        .turns
        .iter()
        .map(|turn| {
            let role = match turn.kind {
                RecordKind::Prompt => "user",
                RecordKind::Tool => "tool",
                _ => agent_label,
            };
            match turn.at {
                Some(at) => format!(
                    "[{}] {role}: {}",
                    at.to_rfc3339_opts(SecondsFormat::Secs, true),
                    turn.body
                ),
                None => format!("{role}: {}", turn.body),
            }
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    cap_middle(&text, TRANSCRIPT_CAP_CHARS)
}

fn cap_middle(text: &str, max: usize) -> String {
    let total = text.chars().count();
    if total <= max {
        return text.to_string();
    }
    let head_len = max * 7 / 10;
    let tail_len = max - head_len;
    let head: String = text.chars().take(head_len).collect();
    let tail: String = text.chars().skip(total - tail_len).collect();
    format!(
        "{head}\n\n…[{} characters omitted]…\n\n{tail}",
        total - head_len - tail_len
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coding_agents::SourceId;
    use chrono::TimeZone;

    fn at(minute: i64) -> Option<DateTime<Utc>> {
        Some(Utc.with_ymd_and_hms(2026, 10, 4, 10, 0, 0).unwrap() + Duration::minutes(minute))
    }

    fn session(records: Vec<Record>) -> Session {
        Session {
            source: SourceId::ClaudeCode,
            id: "s".into(),
            title: None,
            cwd: Some("/repo".into()),
            records,
        }
    }

    #[test]
    fn idle_gap_over_an_hour_starts_a_block_even_without_a_prompt() {
        let session = session(vec![
            Record::new(at(0), RecordKind::Prompt, "a"),
            Record::new(at(5), RecordKind::Reply, "b"),
            Record::new(at(66), RecordKind::Reply, "late reply"),
        ]);
        let blocks = segment(&session);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[1].started_at, at(66).unwrap());
        assert_eq!(blocks[0].active_seconds, 120);
    }

    #[test]
    fn exactly_one_hour_of_silence_keeps_the_block() {
        let session = session(vec![
            Record::new(at(0), RecordKind::Prompt, "a"),
            Record::new(at(30), RecordKind::Reply, "b"),
            Record::new(at(60), RecordKind::Reply, "c"),
        ]);
        assert_eq!(segment(&session).len(), 1);
    }

    #[test]
    fn time_box_waits_for_a_prompt() {
        let mut records = vec![Record::new(at(0), RecordKind::Prompt, "start")];
        records.extend((1..=9).map(|step| Record::new(at(step * 10), RecordKind::Tool, "work")));
        let session = session(records);
        assert_eq!(
            segment(&session).len(),
            1,
            "an autonomous stretch past an hour stays in its block"
        );
    }

    #[test]
    fn exit_closes_the_block_and_a_resume_opens_another() {
        let session = session(vec![
            Record::new(at(0), RecordKind::Prompt, "a"),
            Record::new(at(1), RecordKind::Reply, "b"),
            Record::new(at(2), RecordKind::End, ""),
            Record::new(at(10), RecordKind::Prompt, "resumed"),
        ]);
        let blocks = segment(&session);
        assert_eq!(blocks.len(), 2);
        assert!(blocks[0].exited);
        assert!(!blocks[1].exited);
    }

    #[test]
    fn untimed_turns_join_the_open_block_and_turnless_blocks_drop() {
        let session = session(vec![
            Record::event(at(0)),
            Record::new(at(1), RecordKind::Prompt, "a"),
            Record::new(None, RecordKind::Reply, "b"),
            Record::event(at(200)),
        ]);
        let blocks = segment(&session);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].turns.len(), 2);
        assert_eq!(blocks[0].cwd.as_deref(), Some("/repo"));
    }

    #[test]
    fn render_caps_the_middle() {
        let text = "x".repeat(10);
        assert_eq!(cap_middle(&text, 20), text);
        let capped = cap_middle(&"y".repeat(200), 100);
        assert!(capped.starts_with(&"y".repeat(70)));
        assert!(capped.contains("[100 characters omitted]"));
    }
}
