//! Windows: CLIs installed as `.cmd` launchers (npm shims, the Cursor agent
//! installer) run through `cmd.exe`, which cannot carry a multi-line or long
//! argument. The fake CLI is an npm shim ([`npm_cmd_cli`]).

use super::tests::{Harness, TestSink};
use super::turn::execute_turn;
use crate::llm::cli::CliKind;
use crate::llm::cmd_shim_tests::{npm_cmd_cli, read_record};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::Mutex as StdMutex;
use tokio::sync::watch;

fn fake_cmd_cli(harness: &Harness, kind: CliKind) -> PathBuf {
    npm_cmd_cli(
        harness.bin.path(),
        kind.id(),
        &harness.log.path().join("received.json"),
    )
}

/// Runs one turn with the test process's own environment (Node and
/// `cmd.exe` must resolve) and returns what the fake CLI received.
async fn received(harness: &Harness, kind: CliKind, program: &Path, input: &str) -> Value {
    let run_id = harness.new_run(kind, input).await;
    let sink = TestSink {
        repository: harness.repository.clone(),
        emitted: StdMutex::new(Vec::new()),
    };
    let mut context = harness.context(kind, program, &run_id, input, None);
    context.env = std::env::vars().collect();
    let (_cancel, receiver) = watch::channel(false);
    execute_turn(&harness.repository, &sink, context, receiver).await;
    let emitted = sink.emitted.into_inner().unwrap();
    read_record(&harness.log.path().join("received.json"))
        .unwrap_or_else(|| panic!("{} never ran; events: {emitted:?}", kind.id()))
}

/// The prompt as the CLI got it: wherever it went, argument or stdin.
fn prompt_seen(record: &Value) -> String {
    let mut seen = record["stdin"].as_str().unwrap_or_default().to_string();
    for arg in record["args"].as_array().unwrap() {
        seen.push_str(arg.as_str().unwrap());
        seen.push('\n');
    }
    seen
}

#[tokio::test]
async fn chat_engine_cmd_cli_gets_a_multiline_prompt_with_shell_characters_intact() {
    let input = "Resumo da reunião:\n- \"aspas\" & 'simples'\n- 100% de %PATH% ^ fim!\r\nÚltima linha > < |";
    for kind in [CliKind::CursorAgent, CliKind::Copilot] {
        let harness = Harness::new(kind).await;
        let program = fake_cmd_cli(&harness, kind);
        let record = received(&harness, kind, &program, input).await;
        assert!(
            prompt_seen(&record).contains(input),
            "{} got {record}",
            kind.id()
        );
    }
}

/// `cmd.exe` refuses a command line over 8191 characters.
#[tokio::test]
async fn chat_engine_cmd_cli_gets_a_prompt_longer_than_the_cmd_line_limit() {
    let input = "x".repeat(20_000);
    for kind in [CliKind::CursorAgent, CliKind::Copilot] {
        let harness = Harness::new(kind).await;
        let program = fake_cmd_cli(&harness, kind);
        let record = received(&harness, kind, &program, &input).await;
        assert!(prompt_seen(&record).contains(&input), "{}", kind.id());
    }
}
