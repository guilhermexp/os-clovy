//! The agent-blind shape every source normalizes its transcripts into. Only
//! a `Prompt` (a real human message) can open a time-boxed block; tool
//! results the agent logs as user messages are `Tool`.

use chrono::{DateTime, TimeZone, Utc};

use super::SourceId;

/// Tool results and tool inputs are capped so one file dump cannot dominate
/// a block's transcript.
pub const TOOL_TEXT_CAP: usize = 800;
pub const TOOL_INPUT_CAP: usize = 400;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecordKind {
    /// A message the user typed (non-empty text).
    Prompt,
    /// The agent's answer text (and the tools it decided to call).
    Reply,
    /// A tool call result or tool activity.
    Tool,
    /// Bookkeeping with only a timestamp (metadata, token counts).
    Event,
    /// An explicit end of the session (the agent exited).
    End,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Record {
    pub at: Option<DateTime<Utc>>,
    pub cwd: Option<String>,
    pub kind: RecordKind,
    pub body: String,
}

impl Record {
    pub fn new(at: Option<DateTime<Utc>>, kind: RecordKind, body: impl Into<String>) -> Self {
        Self {
            at,
            cwd: None,
            kind,
            body: body.into(),
        }
    }

    pub fn event(at: Option<DateTime<Utc>>) -> Self {
        Self::new(at, RecordKind::Event, String::new())
    }

    pub fn with_cwd(mut self, cwd: Option<String>) -> Self {
        self.cwd = cwd.filter(|cwd| !cwd.trim().is_empty());
        self
    }

    pub fn is_turn(&self) -> bool {
        matches!(
            self.kind,
            RecordKind::Prompt | RecordKind::Reply | RecordKind::Tool
        )
    }
}

/// One conversation of one agent, normalized.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Session {
    pub source: SourceId,
    pub id: String,
    pub title: Option<String>,
    /// Working directory known for the whole session (records may refine it).
    pub cwd: Option<String>,
    pub records: Vec<Record>,
}

impl Session {
    /// True when the first human prompt carries Clovy's authorship marker:
    /// the conversation is one of Clovy's own CLI calls, never user work.
    pub fn is_clovy_call(&self) -> bool {
        self.records
            .iter()
            .find(|record| record.kind == RecordKind::Prompt)
            .is_some_and(|prompt| prompt.body.contains(crate::llm::CLOVY_AUTHORSHIP_MARKER))
    }
}

pub fn parse_time(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value.trim())
        .ok()
        .map(|at| at.with_timezone(&Utc))
}

pub fn from_epoch_millis(millis: f64) -> Option<DateTime<Utc>> {
    if !millis.is_finite() || millis <= 0.0 {
        return None;
    }
    Utc.timestamp_millis_opt(millis as i64).single()
}

/// First `max` characters, with an ellipsis marker when cut.
pub fn cap(text: &str, max: usize) -> String {
    let trimmed = text.trim();
    match trimmed.char_indices().nth(max) {
        Some((end, _)) => format!("{}…[truncated]", &trimmed[..end]),
        None => trimmed.to_string(),
    }
}

/// Text inside `<tag>…</tag>`, if the tag is present.
pub fn inner_tag<'a>(text: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = text.find(&open)? + open.len();
    let end = text[start..]
        .find(&close)
        .map_or(text.len(), |end| start + end);
    Some(text[start..end].trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cap_marks_truncation_on_char_boundaries() {
        assert_eq!(cap("ação", 10), "ação");
        assert_eq!(cap("ação ok", 3), "açã…[truncated]");
    }

    #[test]
    fn inner_tag_extracts_wrapped_text() {
        assert_eq!(
            inner_tag("<USER_REQUEST>\nfix it\n</USER_REQUEST>", "USER_REQUEST"),
            Some("fix it")
        );
        assert_eq!(inner_tag("plain", "USER_REQUEST"), None);
    }
}
