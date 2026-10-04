//! Summaries for sealed blocks. Each block goes to the CLI of the agent that
//! did the work when it is installed (Claude Code sessions to `claude`, Codex
//! to `codex`, Copilot to `copilot`, Cursor to `cursor-agent`, Antigravity to
//! `agy`); otherwise to the activity provider chosen in Settings, Models.
//! With neither, nothing is called and the block stays sealed without a
//! summary until one becomes available. Calls go through `crate::llm`, which
//! stamps Clovy's authorship marker on the prompt, isolates the CLI, and runs
//! activity calls one at a time.

use std::collections::HashMap;

use chrono::{DateTime, Duration, SecondsFormat, Utc};
use futures_util::future::BoxFuture;

use super::settings::CodingAgentSources;
use super::store::PendingSummary;
use super::SourceId;
use crate::activity::store::{ActivityStore, StoreError};
use crate::interface_locale::UiLocale;
use crate::llm::cli::CliKind;
use crate::llm::{GenerateOutput, GenerateRequest, LlmError};

/// Attempts per block before it is left without a summary.
pub const MAX_ATTEMPTS: i64 = 3;
/// Wait after a failed attempt, multiplied by the attempt number.
pub const RETRY_AFTER: Duration = Duration::minutes(30);
/// Blocks summarized per drain; the loop comes back for the rest.
pub const DRAIN_BATCH: u32 = 4;
const SUMMARY_CAP_CHARS: usize = 1_200;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Summarizer {
    OwnCli(CliKind),
    ActivityProvider,
    Unavailable,
}

/// Own CLI first, then the activity provider, else nothing.
pub fn choose(source: SourceId, cli_installed: bool, activity_ready: bool) -> Summarizer {
    if cli_installed {
        Summarizer::OwnCli(source.own_cli())
    } else if activity_ready {
        Summarizer::ActivityProvider
    } else {
        Summarizer::Unavailable
    }
}

/// The outside world the drain needs; tests substitute it.
pub trait SummaryBackend: Send + Sync {
    fn cli_installed(&self, kind: CliKind) -> BoxFuture<'_, bool>;
    /// An activity provider is selected and supports JSON schema output (the
    /// same gate `llm::generate_for_activity` applies before any call).
    fn activity_ready(&self) -> bool;
    fn generate(
        &self,
        summarizer: Summarizer,
        request: GenerateRequest,
    ) -> BoxFuture<'_, Result<GenerateOutput, LlmError>>;
}

/// Production backend over `crate::llm`.
pub struct LlmBackend;

impl SummaryBackend for LlmBackend {
    fn cli_installed(&self, kind: CliKind) -> BoxFuture<'_, bool> {
        Box::pin(async move {
            crate::llm::shell_env::login_env()
                .await
                .which(kind.id())
                .is_some()
        })
    }

    fn activity_ready(&self) -> bool {
        use crate::llm::registry::{LlmUsage, ProviderRef};
        let registry = crate::llm::registry();
        let provider = registry.usage.get(LlmUsage::Activity);
        !matches!(provider, ProviderRef::None | ProviderRef::Clovy)
            && registry
                .level_of(provider)
                .is_some_and(crate::llm::StructuredOutputLevel::supports_json_schema)
    }

    fn generate(
        &self,
        summarizer: Summarizer,
        request: GenerateRequest,
    ) -> BoxFuture<'_, Result<GenerateOutput, LlmError>> {
        Box::pin(async move {
            match summarizer {
                Summarizer::OwnCli(kind) => {
                    crate::llm::generate_on_cli_for_activity(kind, request).await
                }
                Summarizer::ActivityProvider => crate::llm::generate_for_activity(request).await,
                Summarizer::Unavailable => Err(LlmError::ActivityProviderMissing),
            }
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DrainReport {
    pub summarized: usize,
    pub failed: usize,
    /// More blocks were due than one batch.
    pub more: bool,
}

/// Summarizes up to [`DRAIN_BATCH`] due blocks of the enabled sources that
/// started at or after `since`, newest first. Blocks of a source turned off,
/// or of an agent with no available summarizer, are not even read: they stay
/// sealed and nothing is called.
pub async fn drain(
    store: &ActivityStore,
    backend: &dyn SummaryBackend,
    enabled: &CodingAgentSources,
    locale: UiLocale,
    since: DateTime<Utc>,
    now: impl Fn() -> DateTime<Utc>,
) -> Result<DrainReport, StoreError> {
    let activity_ready = backend.activity_ready();
    let mut installed: HashMap<CliKind, bool> = HashMap::new();
    let mut summarizers = Vec::new();
    for source in SourceId::ALL {
        if !enabled.is_enabled(source) {
            continue;
        }
        let kind = source.own_cli();
        let cli_installed = match installed.get(&kind) {
            Some(known) => *known,
            None => {
                let found = backend.cli_installed(kind).await;
                installed.insert(kind, found);
                found
            }
        };
        let summarizer = choose(source, cli_installed, activity_ready);
        if summarizer != Summarizer::Unavailable {
            summarizers.push((source, summarizer));
        }
    }
    if summarizers.is_empty() {
        return Ok(DrainReport::default());
    }
    let sources: Vec<SourceId> = summarizers.iter().map(|(source, _)| *source).collect();
    let mut pending = store
        .coding_agent_blocks_to_summarize(&sources, since, now(), MAX_ATTEMPTS, DRAIN_BATCH + 1)
        .await?;
    let mut report = DrainReport {
        more: pending.len() > DRAIN_BATCH as usize,
        ..DrainReport::default()
    };
    pending.truncate(DRAIN_BATCH as usize);
    for item in pending {
        let Some((_, summarizer)) = summarizers
            .iter()
            .find(|(source, _)| *source == item.block.source)
        else {
            continue;
        };
        let result = backend.generate(*summarizer, request(&item, locale)).await;
        let finished = now();
        match result.map(|output| (clean(&output.text), output.provider)) {
            Ok((summary, provider)) if !summary.is_empty() => {
                store
                    .record_coding_agent_summary(item.block.id, &summary, &provider, finished)
                    .await?;
                report.summarized += 1;
            }
            outcome => {
                let error = match outcome {
                    Err(error) => crate::domain::types::AppError::from(error).message,
                    Ok(_) => "The summarizer returned an empty answer.".to_string(),
                };
                let retry_at = finished + RETRY_AFTER * (item.block.summary_attempts as i32 + 1);
                store
                    .record_coding_agent_summary_failure(item.block.id, &error, retry_at, finished)
                    .await?;
                report.failed += 1;
            }
        }
    }
    Ok(report)
}

pub fn request(item: &PendingSummary, locale: UiLocale) -> GenerateRequest {
    let block = &item.block;
    let language = match locale {
        UiLocale::PtBr => "Brazilian Portuguese",
        UiLocale::En => "English",
    };
    let system = format!(
        "You summarize one block of a person's work with a coding agent, for their own work log. \
         Write 1 to 3 plain sentences in {language} saying what they worked on and what came out of it \
         (changes made, findings, decisions, open problems). Be factual and specific: name files, \
         features, and errors. No preamble, no markdown, no lists, and do not mention the transcript."
    );
    let project = block
        .project
        .as_deref()
        .or(block.cwd.as_deref())
        .unwrap_or("unknown");
    let mut prompt = format!(
        "Agent: {}\nProject: {project}\n",
        block.source.display_name()
    );
    if let Some(title) = &block.title {
        prompt.push_str(&format!("Session title: {title}\n"));
    }
    prompt.push_str(&format!(
        "Time (UTC): {} to {}\n\nTranscript:\n{}",
        short_time(&block.started_at),
        short_time(&block.ended_at),
        item.transcript
    ));
    GenerateRequest {
        system: Some(system),
        prompt,
        schema: None,
        timeout: None,
    }
}

fn short_time(stored: &str) -> String {
    DateTime::parse_from_rfc3339(stored)
        .map(|at| at.to_rfc3339_opts(SecondsFormat::Secs, true))
        .unwrap_or_else(|_| stored.to_string())
}

/// Plain text: trimmed, code fences removed, capped.
pub fn clean(text: &str) -> String {
    let trimmed = text.trim();
    let unfenced = trimmed
        .strip_prefix("```")
        .and_then(|rest| rest.split_once('\n').map(|(_, body)| body))
        .and_then(|body| body.trim_end().strip_suffix("```"))
        .unwrap_or(trimmed)
        .trim();
    match unfenced.char_indices().nth(SUMMARY_CAP_CHARS) {
        Some((end, _)) => format!("{}…", unfenced[..end].trim_end()),
        None => unfenced.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::activity::key::MemoryKeyStore;
    use crate::activity::store::ACTIVITY_DB_FILE;
    use crate::coding_agents::store::{BlockState, NewBlock};
    use chrono::TimeZone;
    use std::sync::{Mutex, MutexGuard, PoisonError};
    use std::time::Duration as StdDuration;

    impl FakeBackend {
        fn calls(&self) -> MutexGuard<'_, Vec<(Summarizer, GenerateRequest)>> {
            self.calls.lock().unwrap_or_else(PoisonError::into_inner)
        }
    }

    #[derive(Default)]
    struct FakeBackend {
        installed: Vec<CliKind>,
        activity_ready: bool,
        fail: bool,
        calls: Mutex<Vec<(Summarizer, GenerateRequest)>>,
    }

    impl SummaryBackend for FakeBackend {
        fn cli_installed(&self, kind: CliKind) -> BoxFuture<'_, bool> {
            let found = self.installed.contains(&kind);
            Box::pin(async move { found })
        }

        fn activity_ready(&self) -> bool {
            self.activity_ready
        }

        fn generate(
            &self,
            summarizer: Summarizer,
            request: GenerateRequest,
        ) -> BoxFuture<'_, Result<GenerateOutput, LlmError>> {
            self.calls().push((summarizer, request));
            let fail = self.fail;
            Box::pin(async move {
                if fail {
                    return Err(LlmError::TimedOut);
                }
                let provider = match summarizer {
                    Summarizer::OwnCli(kind) => format!("cli:{}", kind.id()),
                    _ => "endpoint:local".to_string(),
                };
                Ok(GenerateOutput {
                    text: "```\nFixed the login bug in auth.ts.\n```".into(),
                    json: None,
                    provider,
                    latency: StdDuration::ZERO,
                })
            })
        }
    }

    fn at(minute: i64) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 4, 10, 0, 0).unwrap() + Duration::minutes(minute)
    }

    fn all() -> CodingAgentSources {
        CodingAgentSources {
            claude_code: true,
            codex: true,
            copilot_cli: true,
            copilot_vscode: true,
            cursor: true,
            cursor_cli: true,
            antigravity: true,
        }
    }

    async fn store_with_sealed(source: SourceId) -> (tempfile::TempDir, ActivityStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = ActivityStore::open(
            &dir.path().join(ACTIVITY_DB_FILE),
            &MemoryKeyStore::default(),
        )
        .await
        .unwrap();
        store
            .upsert_coding_agent_block(
                &NewBlock {
                    source,
                    session_id: "s1".into(),
                    started_at: at(0),
                    ended_at: at(30),
                    cwd: Some("/Users/me/clovy".into()),
                    project: Some("clovy".into()),
                    title: None,
                    first_prompt: Some("fix the login bug".into()),
                    prompt_count: 1,
                    reply_count: 1,
                    active_seconds: 300,
                    transcript: "user: fix the login bug".into(),
                    sealed: true,
                },
                at(31),
            )
            .await
            .unwrap();
        (dir, store)
    }

    #[test]
    fn own_cli_wins_then_activity_provider_then_nothing() {
        assert_eq!(
            choose(SourceId::Cursor, true, true),
            Summarizer::OwnCli(CliKind::CursorAgent)
        );
        assert_eq!(
            choose(SourceId::CopilotVscode, true, false),
            Summarizer::OwnCli(CliKind::Copilot)
        );
        assert_eq!(
            choose(SourceId::Codex, false, true),
            Summarizer::ActivityProvider
        );
        assert_eq!(
            choose(SourceId::Cursor, false, false),
            Summarizer::Unavailable
        );
    }

    #[tokio::test]
    async fn without_cli_or_provider_the_block_stays_sealed_and_nothing_is_called() {
        let (_dir, store) = store_with_sealed(SourceId::Cursor).await;
        let backend = FakeBackend {
            installed: vec![CliKind::Claude],
            ..FakeBackend::default()
        };
        let report = drain(&store, &backend, &all(), UiLocale::En, at(-60), || at(40))
            .await
            .unwrap();
        assert_eq!(report, DrainReport::default());
        assert!(backend.calls().is_empty(), "no call may be made");
        let blocks = store
            .coding_agent_blocks_between(at(0), at(1))
            .await
            .unwrap();
        assert_eq!(blocks[0].state, BlockState::Sealed);
        assert_eq!(blocks[0].summary, None);
        assert_eq!(blocks[0].summary_attempts, 0, "no attempt is spent");
    }

    #[tokio::test]
    async fn a_source_turned_off_is_not_summarized() {
        let (_dir, store) = store_with_sealed(SourceId::ClaudeCode).await;
        let backend = FakeBackend {
            installed: vec![CliKind::Claude],
            activity_ready: true,
            ..FakeBackend::default()
        };
        let only_codex = CodingAgentSources {
            codex: true,
            ..CodingAgentSources::default()
        };
        drain(&store, &backend, &only_codex, UiLocale::En, at(-60), || {
            at(40)
        })
        .await
        .unwrap();
        assert!(backend.calls().is_empty());
    }

    #[tokio::test]
    async fn the_agents_own_cli_writes_the_summary() {
        let (_dir, store) = store_with_sealed(SourceId::ClaudeCode).await;
        let backend = FakeBackend {
            installed: vec![CliKind::Claude],
            activity_ready: true,
            ..FakeBackend::default()
        };
        let report = drain(&store, &backend, &all(), UiLocale::PtBr, at(-60), || at(40))
            .await
            .unwrap();
        assert_eq!(report.summarized, 1);
        let (summarizer, sent) = backend.calls()[0].clone();
        assert_eq!(summarizer, Summarizer::OwnCli(CliKind::Claude));
        assert!(sent
            .system
            .as_deref()
            .unwrap()
            .contains("Brazilian Portuguese"));
        assert!(sent.prompt.contains("Project: clovy"));
        assert!(sent.prompt.ends_with("user: fix the login bug"));
        let blocks = store
            .coding_agent_blocks_between(at(0), at(1))
            .await
            .unwrap();
        assert_eq!(blocks[0].state, BlockState::Summarized);
        assert_eq!(
            blocks[0].summary.as_deref(),
            Some("Fixed the login bug in auth.ts.")
        );
        assert_eq!(blocks[0].summary_source.as_deref(), Some("cli:claude"));
    }

    #[tokio::test]
    async fn failures_back_off_and_stop_after_the_last_attempt() {
        let (_dir, store) = store_with_sealed(SourceId::Codex).await;
        let backend = FakeBackend {
            activity_ready: true,
            fail: true,
            ..FakeBackend::default()
        };
        let mut now = at(40);
        for attempt in 1..=MAX_ATTEMPTS {
            let report = drain(&store, &backend, &all(), UiLocale::En, at(-60), || now)
                .await
                .unwrap();
            assert_eq!(report.failed, 1, "attempt {attempt}");
            let early = drain(&store, &backend, &all(), UiLocale::En, at(-60), || {
                now + Duration::minutes(1)
            })
            .await
            .unwrap();
            assert_eq!(early.failed, 0, "retries wait for the backoff");
            now += RETRY_AFTER * attempt as i32;
        }
        let report = drain(&store, &backend, &all(), UiLocale::En, at(-60), || {
            now + Duration::days(1)
        })
        .await
        .unwrap();
        assert_eq!(report, DrainReport::default());
        assert_eq!(backend.calls()[0].0, Summarizer::ActivityProvider);
        let blocks = store
            .coding_agent_blocks_between(at(0), at(1))
            .await
            .unwrap();
        assert_eq!(blocks[0].summary_attempts, MAX_ATTEMPTS);
        assert_eq!(blocks[0].state, BlockState::Sealed);
        assert!(blocks[0].summary_error.is_some());
    }

    #[test]
    fn clean_strips_fences_and_caps() {
        assert_eq!(clean("  plain  "), "plain");
        assert_eq!(clean("```text\nfenced\n```"), "fenced");
        assert!(clean(&"a".repeat(2_000)).ends_with('…'));
    }
}
