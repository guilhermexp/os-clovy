//! One scan: seal idle live blocks, then, for every enabled source only,
//! discover store entries changed inside the window, load the changed ones,
//! drop Clovy's own CLI conversations, segment, and upsert the blocks.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

use chrono::{DateTime, Utc};

use super::record::{cap, Session};
use super::segment::{render, segment, IDLE_GAP};
use super::settings::CodingAgentSources;
use super::sources::{self, Fingerprint, SourceRoots};
use super::store::NewBlock;
use super::SourceId;
use crate::activity::store::{ActivityStore, StoreError};

const FIRST_PROMPT_CAP: usize = 200;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScanReport {
    /// Store entries opened, per source (unchanged entries are not opened).
    pub files_read: BTreeMap<SourceId, usize>,
    pub sessions: usize,
    pub clovy_calls_skipped: usize,
    pub blocks_written: usize,
    pub blocks_sealed_idle: u64,
    pub load_errors: usize,
}

impl ScanReport {
    pub fn changed(&self) -> bool {
        self.blocks_written > 0 || self.blocks_sealed_idle > 0
    }
}

pub struct Scanner {
    roots: SourceRoots,
    /// Fingerprint of every entry loaded successfully, so unchanged files are
    /// not parsed again.
    seen: HashMap<PathBuf, Fingerprint>,
}

impl Scanner {
    pub fn new(roots: SourceRoots) -> Self {
        Self {
            roots,
            seen: HashMap::new(),
        }
    }

    pub fn roots(&self) -> &SourceRoots {
        &self.roots
    }

    /// `window_start` bounds history: entries last changed before it are not
    /// read, and blocks that ended before it are not stored.
    pub async fn scan(
        &mut self,
        store: &ActivityStore,
        enabled: &CodingAgentSources,
        now: DateTime<Utc>,
        window_start: DateTime<Utc>,
    ) -> Result<ScanReport, StoreError> {
        let mut report = ScanReport {
            blocks_sealed_idle: store
                .seal_idle_coding_agent_blocks(now - IDLE_GAP, now)
                .await?,
            ..ScanReport::default()
        };
        for source in SourceId::ALL {
            if !enabled.is_enabled(source) {
                continue;
            }
            for candidate in sources::discover(source, &self.roots, window_start.into()) {
                if self.seen.get(&candidate.path) == Some(&candidate.fingerprint) {
                    continue;
                }
                *report.files_read.entry(source).or_default() += 1;
                let sessions = match sources::load(&candidate, window_start).await {
                    Ok(sessions) => sessions,
                    Err(error) => {
                        tracing::debug!(%error, path = %candidate.path.display(), "coding agents: store not readable yet");
                        report.load_errors += 1;
                        continue;
                    }
                };
                for session in &sessions {
                    report.sessions += 1;
                    if session.is_clovy_call() {
                        report.clovy_calls_skipped += 1;
                        continue;
                    }
                    for block in blocks_for(session, now, window_start) {
                        if store.upsert_coding_agent_block(&block, now).await? {
                            report.blocks_written += 1;
                        }
                    }
                }
                self.seen.insert(candidate.path, candidate.fingerprint);
            }
        }
        Ok(report)
    }
}

/// The blocks of one session that ended inside the window. Every block but
/// the last is sealed; the last stays live until the agent exits or the
/// session has been idle for longer than [`IDLE_GAP`].
pub fn blocks_for(
    session: &Session,
    now: DateTime<Utc>,
    window_start: DateTime<Utc>,
) -> Vec<NewBlock> {
    if session.is_clovy_call() {
        return Vec::new();
    }
    let segments = segment(session);
    let last = segments.len().saturating_sub(1);
    segments
        .iter()
        .enumerate()
        .filter(|(_, segment)| segment.ended_at >= window_start)
        .map(|(index, segment)| {
            let sealed = index < last || segment.exited || now - segment.ended_at > IDLE_GAP;
            NewBlock {
                source: session.source,
                session_id: session.id.clone(),
                started_at: segment.started_at,
                ended_at: segment.ended_at,
                project: segment.cwd.as_deref().and_then(project_name),
                cwd: segment.cwd.clone(),
                title: session.title.clone(),
                first_prompt: segment
                    .first_prompt()
                    .map(|prompt| cap(prompt, FIRST_PROMPT_CAP)),
                prompt_count: segment.prompt_count() as i64,
                reply_count: segment.reply_count() as i64,
                active_seconds: segment.active_seconds,
                transcript: render(segment, session.source.display_name()),
                sealed,
            }
        })
        .collect()
}

/// The last path component of the working directory.
fn project_name(cwd: &str) -> Option<String> {
    std::path::Path::new(cwd.trim_end_matches('/'))
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| !name.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coding_agents::record::{Record, RecordKind};
    use chrono::{Duration, TimeZone};

    fn at(minute: i64) -> Option<DateTime<Utc>> {
        Some(Utc.with_ymd_and_hms(2026, 10, 4, 10, 0, 0).unwrap() + Duration::minutes(minute))
    }

    fn session(records: Vec<Record>) -> Session {
        Session {
            source: SourceId::Codex,
            id: "s".into(),
            title: Some("Tests".into()),
            cwd: None,
            records,
        }
    }

    #[test]
    fn last_block_stays_live_until_idle_or_exit() {
        let records = vec![
            Record::new(at(0), RecordKind::Prompt, "a").with_cwd(Some("/Users/me/clovy/".into())),
            Record::new(at(5), RecordKind::Reply, "b"),
            Record::new(at(120), RecordKind::Prompt, "c"),
            Record::new(at(125), RecordKind::Reply, "d"),
        ];
        let blocks = blocks_for(
            &session(records.clone()),
            at(150).unwrap(),
            at(-600).unwrap(),
        );
        assert_eq!(blocks.len(), 2);
        assert!(blocks[0].sealed);
        assert!(!blocks[1].sealed, "the last block is still growing");
        assert_eq!(blocks[0].project.as_deref(), Some("clovy"));
        assert_eq!(blocks[1].first_prompt.as_deref(), Some("c"));

        let idle = blocks_for(&session(records), at(186).unwrap(), at(-600).unwrap());
        assert!(idle[1].sealed, "idle for more than an hour seals it");
    }

    #[test]
    fn blocks_that_ended_before_the_window_are_not_stored() {
        let records = vec![
            Record::new(at(0), RecordKind::Prompt, "old"),
            Record::new(at(300), RecordKind::Prompt, "new"),
        ];
        let blocks = blocks_for(&session(records), at(310).unwrap(), at(200).unwrap());
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].first_prompt.as_deref(), Some("new"));
    }
}
