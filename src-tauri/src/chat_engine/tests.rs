//! Integration tests of the CLI chat engine with fake CLIs (shell scripts in
//! a temporary directory that reproduce each CLI's measured stream format),
//! an in-memory agent database, and the sidecar's persistence path.

use super::cli::{self, EngineEvent, StreamTranslator};
use super::turn::{execute_turn, TurnContext};
use super::*;
use crate::agent_runtime::{AgentItemPayload, AgentRepository, AgentSafetyMode, MessagePayload};
use crate::mcp_server::McpLaunch;
use serde_json::{json, Value};
use sqlx_sqlite::SqlitePoolOptions;
use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Mutex as StdMutex;

/// Persists like the app does and records what would be emitted.
pub(super) struct TestSink {
    pub repository: AgentRepository,
    pub emitted: StdMutex<Vec<(String, Value)>>,
}

impl FrameSink for TestSink {
    async fn publish(&self, frame: RpcFrame) {
        let persisted = crate::agent_runtime::host::persist_runtime_event(&self.repository, &frame)
            .await
            .expect("event persists");
        if let Some(event) = persisted {
            self.emitted
                .lock()
                .unwrap()
                .push((event.method, event.data));
        }
    }
}

pub(super) struct Harness {
    pub repository: AgentRepository,
    pub session_id: String,
    pub workspace: tempfile::TempDir,
    pub bin: tempfile::TempDir,
    pub log: tempfile::TempDir,
}

impl Harness {
    pub async fn new(kind: CliKind) -> Self {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("memory database");
        crate::db::migrations::run_migrations(&pool)
            .await
            .expect("migrations");
        let repository = AgentRepository::new(pool);
        let workspace = tempfile::tempdir().unwrap();
        let session = repository
            .create_session(
                "CLI chat",
                &engine_model_id(kind),
                AgentSafetyMode::Sandboxed,
                workspace.path().to_str(),
            )
            .await
            .unwrap();
        Self {
            repository,
            session_id: session.id,
            workspace,
            bin: tempfile::tempdir().unwrap(),
            log: tempfile::tempdir().unwrap(),
        }
    }

    /// Writes an executable fake CLI. The prelude numbers each call and logs
    /// its argv (one per line) and stdin to `<log>/<n>.args` / `<n>.stdin`.
    pub fn fake_cli(&self, kind: CliKind, body: &str) -> PathBuf {
        let path = self.bin.path().join(kind.id());
        let script = format!(
            "#!/bin/sh\nLOG='{log}'\nn=$(( $(cat \"$LOG/count\" 2>/dev/null || echo 0) + 1 ))\necho $n > \"$LOG/count\"\nfor a in \"$@\"; do printf '%s\\n' \"$a\"; done > \"$LOG/$n.args\"\ncat > \"$LOG/$n.stdin\"\n{body}\n",
            log = self.log.path().display()
        );
        std::fs::write(&path, script).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    pub fn args(&self, call: usize) -> Vec<String> {
        std::fs::read_to_string(self.log.path().join(format!("{call}.args")))
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }

    pub fn stdin(&self, call: usize) -> String {
        std::fs::read_to_string(self.log.path().join(format!("{call}.stdin"))).unwrap_or_default()
    }

    pub fn context(
        &self,
        kind: CliKind,
        program: &Path,
        run_id: &str,
        input: &str,
        mcp: Option<McpLaunch>,
    ) -> TurnContext {
        TurnContext {
            session_id: self.session_id.clone(),
            run_id: run_id.to_string(),
            kind,
            program: program.to_path_buf(),
            env: BTreeMap::from([("PATH".to_string(), "/usr/bin:/bin".to_string())]),
            workspace: self.workspace.path().to_path_buf(),
            input: input.to_string(),
            mcp,
        }
    }

    /// What `start_agent_run` does before the turn: the run and the user
    /// message.
    pub async fn new_run(&self, kind: CliKind, input: &str) -> String {
        let run = self
            .repository
            .create_run(&self.session_id, &engine_model_id(kind), None)
            .await
            .unwrap();
        self.repository
            .append_item(
                &self.session_id,
                Some(&run.id),
                0,
                &AgentItemPayload::UserMessage(MessagePayload {
                    role: "user".into(),
                    content: input.to_string(),
                    attachments: Vec::new(),
                }),
                Some(&format!("user:{}", run.id)),
            )
            .await
            .unwrap();
        run.id
    }

    /// Runs one complete turn and returns its run id and emitted events.
    pub async fn turn(
        &self,
        kind: CliKind,
        program: &Path,
        input: &str,
        mcp: Option<McpLaunch>,
    ) -> (String, Vec<(String, Value)>) {
        let run_id = self.new_run(kind, input).await;
        let sink = TestSink {
            repository: self.repository.clone(),
            emitted: StdMutex::new(Vec::new()),
        };
        let (_cancel, receiver) = watch::channel(false);
        execute_turn(
            &self.repository,
            &sink,
            self.context(kind, program, &run_id, input, mcp),
            receiver,
        )
        .await;
        let emitted = sink.emitted.into_inner().unwrap();
        (run_id, emitted)
    }

    /// The session transcript as (kind, text) pairs, in display order.
    pub async fn transcript(&self) -> Vec<(String, String)> {
        self.repository
            .items(&self.session_id)
            .await
            .unwrap()
            .into_iter()
            .map(|item| {
                let kind = item.payload.kind().to_string();
                let text = match item.payload {
                    AgentItemPayload::UserMessage(message)
                    | AgentItemPayload::AssistantMessage(message) => message.content,
                    AgentItemPayload::Reasoning(text) => text.text,
                    AgentItemPayload::ToolCall(tool) | AgentItemPayload::ToolResult(tool) => {
                        format!(
                            "{}:{}",
                            tool.tool_name.unwrap_or_default(),
                            tool.status.unwrap_or_default()
                        )
                    }
                    AgentItemPayload::Error(value) => {
                        value["message"].as_str().unwrap_or_default().to_string()
                    }
                    other => format!("{other:?}"),
                };
                (kind, text)
            })
            .collect()
    }
}

/// A fake claude that requires `--resume claude-conv-1` after its first call
/// and answers "Answer <n>" with reasoning and one tool call.
pub(super) const FAKE_CLAUDE: &str = r#"
if [ "$n" -gt 1 ] && ! grep -qx -- 'claude-conv-1' "$LOG/$n.args"; then echo 'No conversation found to resume' >&2; exit 3; fi
echo '{"type":"system","subtype":"init","session_id":"claude-conv-1"}'
echo '{"type":"stream_event","event":{"type":"message_start"}}'
echo '{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"thinking it over"}}}'
echo '{"type":"stream_event","event":{"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"Answer "}}}'
echo '{"type":"stream_event","event":{"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"'"$n"'"}}}'
echo '{"type":"assistant","message":{"content":[{"type":"text","text":"Answer '"$n"'"}]}}'
echo '{"type":"assistant","message":{"content":[{"type":"tool_use","id":"tool-'"$n"'","name":"Bash","input":{"command":"ls"}}]}}'
echo '{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"tool-'"$n"'","content":"notes.txt","is_error":false}]}}'
echo '{"type":"result","subtype":"success","is_error":false,"result":"Answer '"$n"'","session_id":"claude-conv-1"}'
"#;

fn argument_after<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.iter()
        .position(|arg| arg == flag)
        .and_then(|index| args.get(index + 1))
        .map(String::as_str)
}

/// Fake stream for each CLI: answers "Answer <n>" and enforces the resume
/// argument from the second call on; `check` is a shell test on the args.
fn fake_two_turn_script(kind: CliKind) -> (&'static str, String) {
    match kind {
        CliKind::Claude => ("claude-conv-1", FAKE_CLAUDE.to_string()),
        CliKind::Codex => (
            "codex-thread-1",
            r#"
if [ "$n" -gt 1 ]; then grep -qx resume "$LOG/$n.args" && grep -qx codex-thread-1 "$LOG/$n.args" || exit 3; fi
echo '{"type":"thread.started","thread_id":"codex-thread-1"}'
echo '{"type":"turn.started"}'
echo '{"type":"item.completed","item":{"id":"item_0","type":"reasoning","text":"thinking it over"}}'
echo '{"type":"item.started","item":{"id":"item_1","type":"command_execution","command":"ls","aggregated_output":"","exit_code":null,"status":"in_progress"}}'
echo '{"type":"item.completed","item":{"id":"item_1","type":"command_execution","command":"ls","aggregated_output":"notes.txt","exit_code":0,"status":"completed"}}'
echo '{"type":"item.completed","item":{"id":"item_2","type":"agent_message","text":"Answer '"$n"'"}}'
echo '{"type":"turn.completed","usage":{"input_tokens":1,"output_tokens":1}}'
"#
            .to_string(),
        ),
        CliKind::Pi => (
            "",
            r#"
id=$(sed -n '/^--session-id$/{n;p;}' "$LOG/$n.args")
[ -n "$id" ] || exit 3
if [ "$n" -gt 1 ]; then [ "$id" = "$(cat "$LOG/pi.id")" ] || exit 4; fi
echo "$id" > "$LOG/pi.id"
echo '{"type":"session","version":3,"id":"'"$id"'"}'
echo '{"type":"agent_start"}'
echo '{"type":"message_start","message":{"role":"assistant","content":[]}}'
echo '{"type":"message_update","assistantMessageEvent":{"type":"thinking_delta","contentIndex":0,"delta":"thinking it over"}}'
echo '{"type":"message_update","assistantMessageEvent":{"type":"text_delta","contentIndex":1,"delta":"Answer '"$n"'"}}'
echo '{"type":"message_end","message":{"role":"assistant","stopReason":"toolUse","content":[{"type":"text","text":"Answer '"$n"'"}]}}'
echo '{"type":"tool_execution_start","toolCallId":"call-'"$n"'","toolName":"bash","args":{"command":"ls"}}'
echo '{"type":"tool_execution_end","toolCallId":"call-'"$n"'","toolName":"bash","result":{"content":[{"type":"text","text":"notes.txt"}]},"isError":false}'
echo '{"type":"message_start","message":{"role":"assistant","content":[]}}'
echo '{"type":"message_end","message":{"role":"assistant","stopReason":"stop","content":[]}}'
echo '{"type":"agent_end","messages":[],"willRetry":false}'
echo '{"type":"agent_settled"}'
"#
            .to_string(),
        ),
        CliKind::Agy => (
            "agy-conv-1",
            r#"
if [ "$n" -gt 1 ] && ! grep -qx agy-conv-1 "$LOG/$n.args"; then exit 3; fi
echo '{"event":"init","conversation_id":"agy-conv-1","init":{"cwd":"."}}'
echo '{"event":"step_update","step_update":{"conversation_id":"agy-conv-1","step_index":0,"state":"DONE","step_type":"user_input"}}'
echo '{"event":"step_update","step_update":{"step_index":1,"state":"ACTIVE","step_type":"tool","tool_name":"run_command","tool_info":{"name":"run_command","parameters":{"CommandLine":"ls"}}}}'
echo '{"event":"step_update","step_update":{"step_index":1,"state":"DONE","step_type":"tool","tool_name":"run_command","tool_info":{"name":"run_command","parameters":{"CommandLine":"ls"},"result":"notes.txt"}}}'
echo '{"event":"step_update","step_update":{"step_index":2,"state":"ACTIVE","step_type":"agent_response","thinking_delta":"thinking it over","text_delta":"Answer '"$n"'"}}'
echo '{"event":"result","result":{"conversation_id":"agy-conv-1","status":"SUCCESS","response":"Answer '"$n"'"}}'
"#
            .to_string(),
        ),
        CliKind::CursorAgent => (
            "cursor-chat-1",
            r#"
if [ "$n" -gt 1 ] && ! grep -qx cursor-chat-1 "$LOG/$n.args"; then exit 3; fi
echo '{"type":"system","subtype":"init","session_id":"cursor-chat-1"}'
echo '{"type":"thinking","subtype":"delta","text":"thinking it over","timestamp_ms":1}'
echo '{"type":"tool_call","subtype":"started","call_id":"call-'"$n"'","tool_call":{"shellToolCall":{"args":{"command":"ls"}}}}'
echo '{"type":"tool_call","subtype":"completed","call_id":"call-'"$n"'","tool_call":{"shellToolCall":{"result":{"success":{"stdout":"notes.txt"}}}}}'
echo '{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"Answer "}]},"timestamp_ms":2}'
echo '{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"'"$n"'"}]},"timestamp_ms":3}'
echo '{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"Answer '"$n"'"}]}}'
echo '{"type":"result","subtype":"success","is_error":false,"result":"Answer '"$n"'","session_id":"cursor-chat-1"}'
"#
            .to_string(),
        ),
        CliKind::Copilot => (
            "",
            r#"
id=$(sed -n '/^--session-id$/{n;p;}' "$LOG/$n.args")
[ -n "$id" ] || exit 3
if [ "$n" -gt 1 ]; then [ "$id" = "$(cat "$LOG/copilot.id")" ] || exit 4; fi
echo "$id" > "$LOG/copilot.id"
echo '{"type":"assistant.turn_start","data":{"turnId":"0"}}'
echo '{"type":"assistant.reasoning_delta","data":{"reasoningId":"r1","deltaContent":"thinking it over"}}'
echo '{"type":"tool.execution_start","data":{"toolCallId":"call-'"$n"'","toolName":"bash","arguments":{"command":"ls"}}}'
echo '{"type":"tool.execution_complete","data":{"toolCallId":"call-'"$n"'","success":true,"result":{"content":"notes.txt"}}}'
echo '{"type":"assistant.message_delta","data":{"messageId":"m'"$n"'","deltaContent":"Answer '"$n"'"}}'
echo '{"type":"assistant.message","data":{"messageId":"m'"$n"'","content":"Answer '"$n"'"}}'
echo '{"type":"assistant.turn_end","data":{"turnId":"0"}}'
echo '{"type":"result","exitCode":0}'
"#
            .to_string(),
        ),
    }
}

#[tokio::test]
async fn chat_engine_two_turns_resume_the_cli_conversation_for_every_cli() {
    for kind in CliKind::ALL {
        let harness = Harness::new(kind).await;
        let (conversation, body) = fake_two_turn_script(kind);
        let program = harness.fake_cli(kind, &body);

        let (first_run, first) = harness.turn(kind, &program, "First question", None).await;
        let (second_run, second) = harness.turn(kind, &program, "Second question", None).await;

        let methods = |events: &[(String, Value)]| {
            events
                .iter()
                .map(|(method, _)| method.clone())
                .collect::<Vec<_>>()
        };
        for events in [&first, &second] {
            let methods = methods(events);
            assert_eq!(
                methods.first().map(String::as_str),
                Some("run.started"),
                "{kind:?}"
            );
            assert_eq!(
                methods.last().map(String::as_str),
                Some("run.completed"),
                "{kind:?}: {events:?}"
            );
            for expected in [
                "reasoning.delta",
                "message.delta",
                "tool.started",
                "tool.completed",
                "message.completed",
            ] {
                assert!(
                    methods.iter().any(|method| method == expected),
                    "{kind:?} missing {expected}: {methods:?}"
                );
            }
        }
        // The second call resumed the first call's conversation.
        let second_args = harness.args(2);
        if conversation.is_empty() {
            let first_id = argument_after(&harness.args(1), "--session-id").map(str::to_string);
            assert!(first_id.is_some(), "{kind:?} chose a conversation id");
            assert_eq!(
                argument_after(&second_args, "--session-id").map(str::to_string),
                first_id,
                "{kind:?}"
            );
        } else {
            assert!(
                second_args.iter().any(|arg| arg == conversation),
                "{kind:?}: {second_args:?}"
            );
            assert!(
                !harness.args(1).iter().any(|arg| arg == conversation),
                "{kind:?} first turn starts fresh"
            );
        }
        // A resumed turn gets only the new message.
        let prompt_of = |call: usize| {
            let stdin = harness.stdin(call);
            if stdin.is_empty() {
                harness.args(call).join("\n")
            } else {
                stdin
            }
        };
        assert!(prompt_of(2).contains("Second question"), "{kind:?}");
        assert!(
            !prompt_of(2).contains("First question"),
            "{kind:?} resent history"
        );

        // Both turns are persisted, in order, as the sidecar's items.
        let transcript = harness.transcript().await;
        let messages = transcript
            .iter()
            .filter(|(kind, _)| kind == "user_message" || kind == "assistant_message")
            .map(|(_, text)| text.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            messages,
            ["First question", "Answer 1", "Second question", "Answer 2"],
            "{kind:?}: {transcript:?}"
        );
        assert!(
            transcript
                .iter()
                .any(|(kind, text)| kind == "reasoning" && text.contains("thinking it over")),
            "{kind:?}"
        );
        assert_eq!(
            transcript
                .iter()
                .filter(|(kind, text)| kind == "tool_result" && text.ends_with(":complete"))
                .count(),
            2,
            "{kind:?}: {transcript:?}"
        );
        for run in [&first_run, &second_run] {
            assert_eq!(
                harness.repository.get_run(run).await.unwrap().status,
                "completed",
                "{kind:?}"
            );
        }
    }
}

#[tokio::test]
async fn chat_engine_cancel_stops_the_cli_process_group_without_orphans() {
    let kind = CliKind::Claude;
    let harness = Harness::new(kind).await;
    let program = harness.fake_cli(
        kind,
        r#"
echo '{"type":"system","subtype":"init","session_id":"claude-conv-1"}'
echo '{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Working"}}}'
sleep 300 &
echo $! > "$LOG/helper.pid"
echo $$ > "$LOG/cli.pid"
sleep 300
"#,
    );
    let run_id = harness.new_run(kind, "Long task").await;
    let sink = TestSink {
        repository: harness.repository.clone(),
        emitted: StdMutex::new(Vec::new()),
    };
    let (cancel, receiver) = watch::channel(false);
    let context = harness.context(kind, &program, &run_id, "Long task", None);
    let pid_file = harness.log.path().join("cli.pid");
    let helper_file = harness.log.path().join("helper.pid");
    let canceller = async {
        for _ in 0..200 {
            if pid_file.exists() && helper_file.exists() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
        cancel.send(true).unwrap();
    };
    let started = std::time::Instant::now();
    tokio::join!(
        execute_turn(&harness.repository, &sink, context, receiver),
        canceller
    );
    assert!(started.elapsed() < std::time::Duration::from_secs(10));

    let read_pid = |path: &Path| {
        std::fs::read_to_string(path)
            .unwrap()
            .trim()
            .parse::<i32>()
            .unwrap()
    };
    for pid in [read_pid(&pid_file), read_pid(&helper_file)] {
        let mut alive = true;
        for _ in 0..100 {
            // SAFETY: signal 0 only checks that the process exists.
            alive = unsafe { libc::kill(pid, 0) } == 0;
            if !alive {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        assert!(!alive, "process {pid} survived the cancel");
    }
    let emitted = sink.emitted.into_inner().unwrap();
    assert_eq!(
        emitted.last().map(|(method, _)| method.as_str()),
        Some("run.cancelled")
    );
    assert_eq!(
        harness.repository.get_run(&run_id).await.unwrap().status,
        "cancelled"
    );
    // The partial answer stays in the session.
    let transcript = harness.transcript().await;
    assert!(transcript.contains(&("assistant_message".to_string(), "Working".to_string())));
}

#[tokio::test]
async fn chat_engine_message_sent_during_a_run_becomes_the_next_turn() {
    // While a CLI turn runs, the composer's message is not steered into it
    // (`steer_agent_run` answers `cli_engine`); it is sent when the turn
    // ends, as the next turn of the same CLI conversation.
    let kind = CliKind::Claude;
    let harness = Harness::new(kind).await;
    let program = harness.fake_cli(kind, FAKE_CLAUDE);
    let gate = harness.log.path().join("release");
    let slow = harness.fake_cli(
        kind,
        &format!(
            "while [ ! -f '{}' ]; do sleep 0.05; done\n{FAKE_CLAUDE}",
            gate.display()
        ),
    );
    assert_eq!(slow, program);
    let run_id = harness.new_run(kind, "First question").await;
    let sink = TestSink {
        repository: harness.repository.clone(),
        emitted: StdMutex::new(Vec::new()),
    };
    let (_cancel, receiver) = watch::channel(false);
    let first = execute_turn(
        &harness.repository,
        &sink,
        harness.context(kind, &program, &run_id, "First question", None),
        receiver,
    );
    let queued_while_running = async {
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        let run = harness.repository.get_run(&run_id).await.unwrap();
        assert_eq!(run.status, "running");
        assert!(
            cli_for_model(&run.model).is_some(),
            "the run refuses live steering"
        );
        std::fs::write(&gate, "").unwrap();
        "Follow-up sent during the run"
    };
    let ((), follow_up) = tokio::join!(first, queued_while_running);
    let (next_run, _) = harness.turn(kind, &program, follow_up, None).await;

    assert!(harness.args(2).iter().any(|arg| arg == "claude-conv-1"));
    assert_eq!(harness.stdin(2), follow_up);
    assert_eq!(
        harness.repository.get_run(&next_run).await.unwrap().status,
        "completed"
    );
    let users = harness
        .transcript()
        .await
        .into_iter()
        .filter(|(kind, _)| kind == "user_message")
        .map(|(_, text)| text)
        .collect::<Vec<_>>();
    assert_eq!(users, ["First question", follow_up]);
}

#[tokio::test]
async fn chat_engine_hands_clovy_mcp_server_only_to_clis_that_accept_it() {
    let launch = McpLaunch {
        command: PathBuf::from("/Applications/Clovy.app/Contents/Resources/native/bin/clovy-mcp"),
        args: vec!["--dir".into(), "/tmp/clovy data/mcp".into()],
    };
    // Each fake CLI reads the configuration it was given and reports it.
    let readers = [
        (
            CliKind::Claude,
            r#"cfg=$(sed -n '/^--mcp-config$/{n;p;}' "$LOG/$n.args"); cp "$cfg" "$LOG/mcp.json" 2>/dev/null
echo '{"type":"system","subtype":"init","session_id":"c"}'
echo '{"type":"result","subtype":"success","is_error":false,"result":""}'"#,
        ),
        (
            CliKind::Copilot,
            r#"cfg=$(sed -n 's/^--additional-mcp-config=@//p' "$LOG/$n.args"); cp "$cfg" "$LOG/mcp.json" 2>/dev/null
echo '{"type":"result","exitCode":0}'"#,
        ),
        (
            CliKind::Codex,
            r#"echo '{"type":"thread.started","thread_id":"t"}'
echo '{"type":"turn.completed"}'"#,
        ),
        (CliKind::Pi, r#"echo '{"type":"agent_end","messages":[]}'"#),
        (
            CliKind::Agy,
            r#"echo '{"event":"result","result":{"status":"SUCCESS"}}'"#,
        ),
        (
            CliKind::CursorAgent,
            r#"echo '{"type":"result","subtype":"success","is_error":false,"result":""}'"#,
        ),
    ];
    for (kind, body) in readers {
        let harness = Harness::new(kind).await;
        let program = harness.fake_cli(kind, body);
        let (run_id, _) = harness
            .turn(kind, &program, "Use Clovy's tools", Some(launch.clone()))
            .await;
        assert_eq!(
            harness.repository.get_run(&run_id).await.unwrap().status,
            "completed",
            "{kind:?}"
        );
        let args = harness.args(1);
        let config = std::fs::read_to_string(harness.log.path().join("mcp.json"))
            .ok()
            .and_then(|text| serde_json::from_str::<Value>(&text).ok());
        match kind {
            CliKind::Claude | CliKind::Copilot => {
                let config =
                    config.unwrap_or_else(|| panic!("{kind:?} received no MCP configuration"));
                let server = &config["mcpServers"]["clovy"];
                assert_eq!(
                    server["command"],
                    json!(launch.command.display().to_string()),
                    "{kind:?}"
                );
                assert_eq!(server["args"], json!(launch.args), "{kind:?}");
            }
            CliKind::Codex => {
                assert!(args.contains(&format!(
                    "mcp_servers.clovy.command=\"{}\"",
                    launch.command.display()
                )));
                assert!(args.contains(
                    &"mcp_servers.clovy.args=[\"--dir\",\"/tmp/clovy data/mcp\"]".to_string()
                ));
            }
            _ => {
                assert!(config.is_none(), "{kind:?}");
                assert!(
                    !args.iter().any(|arg| arg.contains("mcp")),
                    "{kind:?}: {args:?}"
                );
            }
        }
    }
    // What the session shows about Clovy's tools.
    assert_eq!(clovy_tools(CliKind::Claude, true), ClovyTools::Available);
    assert_eq!(clovy_tools(CliKind::Codex, false), ClovyTools::ServerOff);
    for kind in [CliKind::Pi, CliKind::Agy, CliKind::CursorAgent] {
        assert_eq!(clovy_tools(kind, true), ClovyTools::Unsupported, "{kind:?}");
    }
}

#[tokio::test]
async fn chat_engine_shows_actions_the_cli_refused_for_lack_of_approval() {
    let kind = CliKind::Claude;
    let harness = Harness::new(kind).await;
    // Measured from claude 2.1 in a non-interactive turn.
    let program = harness.fake_cli(
        kind,
        r#"
echo '{"type":"system","subtype":"init","session_id":"c"}'
echo '{"type":"assistant","message":{"content":[{"type":"tool_use","id":"w1","name":"Write","input":{"file_path":"/tmp/x"}}]}}'
echo '{"type":"user","message":{"content":[{"type":"tool_result","content":"Claude requested permissions to write to /tmp/x, but you haven'"'"'t granted it yet.","is_error":true,"tool_use_id":"w1"}]}}'
echo '{"type":"assistant","message":{"content":[{"type":"text","text":"The write was blocked."}]}}'
echo '{"type":"result","subtype":"success","is_error":false,"result":"The write was blocked.","permission_denials":[{"tool_name":"Write","tool_use_id":"w1"}]}'
"#,
    );
    let (_, emitted) = harness.turn(kind, &program, "Write a file", None).await;
    let failed = emitted
        .iter()
        .find(|(method, _)| method == "tool.failed")
        .map(|(_, data)| data.clone())
        .expect("the refused tool is shown as failed");
    assert_eq!(failed["needsApproval"], json!(true));
    assert!(failed["error"]
        .as_str()
        .unwrap()
        .contains("needs your approval"));
    assert_eq!(emitted.last().unwrap().0, "run.completed");
}

#[tokio::test]
async fn chat_engine_failures_end_the_run_with_the_cli_error() {
    let kind = CliKind::Codex;
    let harness = Harness::new(kind).await;
    let program = harness.fake_cli(
        kind,
        r#"
echo '{"type":"thread.started","thread_id":"t1"}'
echo '{"type":"error","message":"stream disconnected"}'
echo 'Not logged in. Run codex login.' >&2
exit 1
"#,
    );
    let (run_id, emitted) = harness.turn(kind, &program, "Hi", None).await;
    let (method, data) = emitted.last().unwrap();
    assert_eq!(method, "run.failed");
    assert_eq!(data["message"], json!("stream disconnected"));
    let run = harness.repository.get_run(&run_id).await.unwrap();
    assert_eq!(run.status, "failed");
    // The conversation id was saved, so a retry resumes it.
    let next = harness.fake_cli(
        kind,
        r#"grep -qx t1 "$LOG/$n.args" || exit 3
echo '{"type":"item.completed","item":{"id":"a","type":"agent_message","text":"Back"}}'
echo '{"type":"turn.completed"}'"#,
    );
    let (retry, _) = harness.turn(kind, &next, "Hi", None).await;
    assert_eq!(
        harness.repository.get_run(&retry).await.unwrap().status,
        "completed"
    );
}

#[tokio::test]
async fn chat_engine_new_cli_conversation_gets_earlier_messages_of_the_session() {
    let kind = CliKind::Claude;
    let harness = Harness::new(kind).await;
    // An earlier turn on the Clovy engine.
    let earlier = harness
        .repository
        .create_run(&harness.session_id, "open-software/auto", None)
        .await
        .unwrap();
    for (sequence, payload) in [
        (
            0,
            AgentItemPayload::UserMessage(MessagePayload {
                role: "user".into(),
                content: "My dog is Rex".into(),
                attachments: Vec::new(),
            }),
        ),
        (
            1,
            AgentItemPayload::AssistantMessage(MessagePayload {
                role: "assistant".into(),
                content: "Noted, Rex.".into(),
                attachments: Vec::new(),
            }),
        ),
    ] {
        harness
            .repository
            .append_item(
                &harness.session_id,
                Some(&earlier.id),
                sequence,
                &payload,
                None,
            )
            .await
            .unwrap();
    }
    let program = harness.fake_cli(kind, FAKE_CLAUDE);
    harness
        .turn(kind, &program, "What is my dog called?", None)
        .await;
    let prompt = harness.stdin(1);
    assert!(prompt.contains("User: My dog is Rex"), "{prompt}");
    assert!(prompt.contains("Assistant: Noted, Rex."), "{prompt}");
    assert!(prompt.ends_with("What is my dog called?"), "{prompt}");
    assert!(!harness.args(1).iter().any(|arg| arg == "--resume"));
}

#[test]
fn chat_engine_model_ids_round_trip() {
    for kind in CliKind::ALL {
        assert_eq!(cli_for_model(&engine_model_id(kind)), Some(kind));
    }
    assert_eq!(cli_for_model("__june_local_generation__:llama3"), None);
    assert_eq!(cli_for_model("__clovy_cli_engine__:unknown"), None);
    assert_eq!(cli_for_model("open-software/auto"), None);
}

#[test]
fn chat_engine_turns_never_pass_permission_bypass_flags() {
    let forbidden = [
        "--dangerously-skip-permissions",
        "--yolo",
        "--dangerously-bypass-approvals-and-sandbox",
        "--force",
        "-f",
        "--always-approve",
        "--allow-all",
        "--allow-all-tools",
        "--approve-mcps",
        "--permission-mode",
        "--allowedTools",
    ];
    let launch = McpLaunch {
        command: PathBuf::from("/bin/clovy-mcp"),
        args: vec![],
    };
    let scratch = tempfile::tempdir().unwrap();
    for kind in CliKind::ALL {
        for conversation in [None, Some("id-1")] {
            let invocation =
                cli::turn_invocation(kind, "hello", conversation, Some(&launch), scratch.path());
            for arg in &invocation.args {
                assert!(!forbidden.contains(&arg.as_str()), "{kind:?} passes {arg}");
            }
        }
    }
    // codex keeps its sandbox to the session workspace.
    let codex = cli::turn_invocation(CliKind::Codex, "hello", Some("t"), None, scratch.path());
    assert!(codex
        .args
        .contains(&"sandbox_mode=\"workspace-write\"".to_string()));
}

fn cursor_text(lines: &[&str]) -> (String, Vec<EngineEvent>) {
    let mut translator = StreamTranslator::new(CliKind::CursorAgent);
    let events = lines
        .iter()
        .flat_map(|line| translator.translate_line(line))
        .collect::<Vec<_>>();
    let text = events
        .iter()
        .filter_map(|event| match event {
            EngineEvent::TextDelta(text) | EngineEvent::Text(text) => Some(text.as_str()),
            _ => None,
        })
        .collect::<String>();
    (text, events)
}

#[test]
fn chat_engine_cursor_drops_the_whole_copy_of_a_streamed_segment() {
    // Shapes as captured from cursor-agent 2026.09 (`cursor-t2.jsonl`): the
    // whole copy before a tool call carries `model_call_id`, the one at the
    // end has no `timestamp_ms`.
    let (text, events) = cursor_text(&[
        r#"{"type":"assistant","message":{"content":[{"type":"text","text":"The"}]},"timestamp_ms":1}"#,
        r#"{"type":"assistant","message":{"content":[{"type":"text","text":" plan."}]},"timestamp_ms":2}"#,
        r#"{"type":"assistant","message":{"content":[{"type":"text","text":"The plan."}]},"model_call_id":"m-0","timestamp_ms":3}"#,
        r#"{"type":"tool_call","subtype":"started","call_id":"c1","tool_call":{"shellToolCall":{"args":{"command":"touch x"}}}}"#,
        r#"{"type":"tool_call","subtype":"completed","call_id":"c1","tool_call":{"shellToolCall":{"result":{"rejected":{"command":"touch x","reason":""}}}}}"#,
        r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Done"}]},"timestamp_ms":4}"#,
        r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Done"}]}}"#,
    ]);
    assert_eq!(text, "The plan.Done");
    assert!(events.iter().any(|event| matches!(
        event,
        EngineEvent::ToolFinished { needs_approval: true, name, .. } if name == "shell"
    )));
}

#[test]
fn chat_engine_cursor_one_delta_segment_before_a_tool_call_is_not_doubled() {
    let (text, _) = cursor_text(&[
        r#"{"type":"assistant","message":{"content":[{"type":"text","text":"OK"}]},"timestamp_ms":1}"#,
        r#"{"type":"assistant","message":{"content":[{"type":"text","text":"OK"}]},"model_call_id":"m-0","timestamp_ms":2}"#,
        r#"{"type":"tool_call","subtype":"started","call_id":"c1","tool_call":{"readToolCall":{"args":{"path":"a"}}}}"#,
        r#"{"type":"tool_call","subtype":"completed","call_id":"c1","tool_call":{"readToolCall":{"result":{"success":{"content":"a"}}}}}"#,
        r#"{"type":"assistant","message":{"content":[{"type":"text","text":"."}]},"timestamp_ms":3}"#,
        r#"{"type":"assistant","message":{"content":[{"type":"text","text":"."}]}}"#,
    ]);
    assert_eq!(text, "OK.");
    // Without streamed deltas the whole copies are the answer.
    let (text, _) = cursor_text(&[
        r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Plain answer"}]}}"#,
    ]);
    assert_eq!(text, "Plain answer");
}

/// A minimal OpenAI-compatible server that streams one tool call
/// (`search_june`) as server-sent events and records the request body.
async fn streaming_tool_call_server() -> (String, std::sync::Arc<StdMutex<Vec<Value>>>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let requests = std::sync::Arc::new(StdMutex::new(Vec::new()));
    let recorded = requests.clone();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let recorded = recorded.clone();
            tokio::spawn(async move {
                let mut buffer = Vec::new();
                let mut chunk = [0_u8; 8192];
                let body = loop {
                    let read = socket.read(&mut chunk).await.unwrap_or(0);
                    if read == 0 {
                        return;
                    }
                    buffer.extend_from_slice(&chunk[..read]);
                    let text = String::from_utf8_lossy(&buffer).to_string();
                    if let Some(split) = text.find("\r\n\r\n") {
                        let length = text[..split]
                            .lines()
                            .find_map(|line| {
                                let (name, value) = line.split_once(':')?;
                                name.eq_ignore_ascii_case("content-length")
                                    .then(|| value.trim().parse::<usize>().ok())?
                            })
                            .unwrap_or(0);
                        if buffer.len() >= split + 4 + length {
                            break buffer[split + 4..split + 4 + length].to_vec();
                        }
                    }
                };
                recorded
                    .lock()
                    .unwrap()
                    .push(serde_json::from_slice::<Value>(&body).unwrap_or(Value::Null));
                let _ = socket
                    .write_all(b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n")
                    .await;
                let chunks = [
                    json!({"choices":[{"index":0,"delta":{"role":"assistant","content":"Let me search. "}}]}),
                    json!({"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_1","type":"function","function":{"name":"search_june","arguments":""}}]}}]}),
                    json!({"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"query\":"}}]}}]}),
                    json!({"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"roadmap\"}"}}]}}]}),
                    json!({"choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}),
                ];
                for chunk in chunks {
                    let _ = socket
                        .write_all(format!("data: {chunk}\n\n").as_bytes())
                        .await;
                    let _ = socket.flush().await;
                    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                }
                let _ = socket.write_all(b"data: [DONE]\n\n").await;
            });
        }
    });
    (format!("http://{address}/v1"), requests)
}

#[tokio::test]
async fn chat_engine_registered_endpoint_serves_the_agent_with_streamed_tool_calls() {
    use crate::llm::registry::{LlmEndpointRecord, ProviderRef};
    let _lock = crate::providers::GLOBAL_SETTINGS_TEST_LOCK.lock().await;
    struct Restore;
    impl Drop for Restore {
        fn drop(&mut self) {
            crate::providers::replace_current_settings_for_tests(
                crate::providers::default_settings_for_tests(),
            );
        }
    }
    let _restore = Restore;
    let (base_url, requests) = streaming_tool_call_server().await;
    let mut settings = crate::providers::default_settings_for_tests();
    // Two endpoints; the chat selection is the other one, so the session's
    // tagged model alone must pick this endpoint.
    let endpoint = |id: &str, base_url: &str, model_id: &str| LlmEndpointRecord {
        id: id.into(),
        name: id.into(),
        base_url: base_url.into(),
        model_id: model_id.into(),
        has_api_key: false,
        structured_output: None,
        rpm_limit: None,
    };
    settings.llm_endpoints = vec![
        endpoint("other", "http://127.0.0.1:9/v1", "other-model"),
        endpoint("lab", &base_url, "tool-model"),
    ];
    settings.llm_usage.chat = ProviderRef::Endpoint { id: "other".into() };
    crate::llm::registry::normalize(&mut settings);
    crate::providers::replace_current_settings_for_tests(settings);

    let tools = json!([{ "type": "function", "function": { "name": "search_june", "description": "Search Clovy", "parameters": { "type": "object", "properties": { "query": { "type": "string" } } } } }]);
    let mut response = crate::clovy_api::proxy_agent_chat_completions(json!({
        "model": "__june_local_generation__:tool-model",
        "messages": [{ "role": "user", "content": "Find the roadmap note" }],
        "tools": tools,
        "stream": true,
    }))
    .await
    .expect("the endpoint answers");
    assert_eq!(response.status, 200);
    assert_eq!(response.route.provider.as_deref(), Some("local"));

    // Read it the way the host relays model streams to the sidecar.
    let mut received = Vec::new();
    let mut chunk_count = 0;
    while let Some(bytes) = response.chunk().await.unwrap() {
        chunk_count += 1;
        received.extend_from_slice(&bytes);
    }
    assert!(chunk_count > 1, "the answer arrived as a stream");
    let events = String::from_utf8(received)
        .unwrap()
        .lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .filter(|data| *data != "[DONE]")
        .map(|data| serde_json::from_str::<Value>(data).unwrap())
        .collect::<Vec<_>>();
    let text = events
        .iter()
        .filter_map(|event| event["choices"][0]["delta"]["content"].as_str())
        .collect::<String>();
    let tool_name = events.iter().find_map(|event| {
        event["choices"][0]["delta"]["tool_calls"][0]["function"]["name"].as_str()
    });
    let arguments = events
        .iter()
        .filter_map(|event| {
            event["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"].as_str()
        })
        .collect::<String>();
    assert_eq!(text, "Let me search. ");
    assert_eq!(tool_name, Some("search_june"));
    assert_eq!(
        serde_json::from_str::<Value>(&arguments).unwrap(),
        json!({ "query": "roadmap" })
    );

    let request = requests.lock().unwrap().first().cloned().unwrap();
    assert_eq!(
        request["model"],
        json!("tool-model"),
        "the endpoint's own model id"
    );
    assert_eq!(request["stream"], json!(true));
    assert_eq!(
        request["tools"][0]["function"]["name"],
        json!("search_june")
    );
}

#[tokio::test]
async fn chat_engine_endpoint_choice_keeps_its_identity_when_endpoints_share_a_model() {
    use crate::llm::registry::{LlmEndpointRecord, LlmRegistry, ProviderRef};
    let _lock = crate::providers::GLOBAL_SETTINGS_TEST_LOCK.lock().await;
    struct Restore;
    impl Drop for Restore {
        fn drop(&mut self) {
            crate::providers::replace_current_settings_for_tests(
                crate::providers::default_settings_for_tests(),
            );
        }
    }
    let _restore = Restore;
    let (first_url, first_requests) = streaming_tool_call_server().await;
    let (second_url, second_requests) = streaming_tool_call_server().await;
    let endpoint = |id: &str, base_url: &str| LlmEndpointRecord {
        id: id.into(),
        name: format!("Server {id}"),
        base_url: base_url.into(),
        model_id: "shared-model".into(),
        has_api_key: false,
        structured_output: None,
        rpm_limit: None,
    };
    let mut settings = crate::providers::default_settings_for_tests();
    settings.llm_endpoints = vec![
        endpoint("first", &first_url),
        endpoint("second", &second_url),
    ];
    // Chat is on the first endpoint, which serves the same model id.
    settings.llm_usage.chat = ProviderRef::Endpoint { id: "first".into() };
    crate::llm::registry::normalize(&mut settings);
    let registry = LlmRegistry::from(&settings);
    crate::providers::replace_current_settings_for_tests(settings);

    // The picker offers two distinct choices.
    let engines = endpoint_engines(&registry);
    assert_eq!(engines.len(), 2);
    assert_ne!(engines[0].option_id, engines[1].option_id);
    let second = engines
        .iter()
        .find(|engine| engine.id == "second")
        .unwrap()
        .option_id
        .clone();

    // A session on the second endpoint talks to the second server only.
    let mut response = crate::clovy_api::proxy_agent_chat_completions(json!({
        "model": second,
        "messages": [{ "role": "user", "content": "Hi" }],
        "stream": true,
    }))
    .await
    .expect("the chosen endpoint answers");
    while response.chunk().await.unwrap().is_some() {}
    assert_eq!(second_requests.lock().unwrap().len(), 1);
    assert!(first_requests.lock().unwrap().is_empty());
    assert_eq!(
        second_requests.lock().unwrap()[0]["model"],
        json!("shared-model")
    );

    // A removed endpoint is refused instead of silently using another one.
    let removed = crate::clovy_api::endpoint_option_id("shared-model", "gone");
    let error = crate::clovy_api::proxy_agent_chat_completions(json!({
        "model": removed,
        "messages": [{ "role": "user", "content": "Hi" }],
        "stream": true,
    }))
    .await
    .err()
    .expect("a missing endpoint is refused");
    assert_eq!(error.code, "local_model_unavailable");
    assert!(first_requests.lock().unwrap().is_empty());
}

#[tokio::test]
async fn chat_engine_app_shutdown_stops_and_awaits_running_cli_turns() {
    let kind = CliKind::Claude;
    let harness = Harness::new(kind).await;
    let program = harness.fake_cli(
        kind,
        r#"
echo '{"type":"system","subtype":"init","session_id":"claude-conv-1"}'
sleep 300 &
echo $! > "$LOG/helper.pid"
echo $$ > "$LOG/cli.pid"
sleep 300
"#,
    );
    let host = std::sync::Arc::new(ChatEngineHost::default());
    let run_id = harness.new_run(kind, "Long task").await;
    let ticket = host.register(&run_id).unwrap();
    let sink = std::sync::Arc::new(TestSink {
        repository: harness.repository.clone(),
        emitted: StdMutex::new(Vec::new()),
    });
    let task = {
        let repository = harness.repository.clone();
        let sink = sink.clone();
        let context = harness.context(kind, &program, &run_id, "Long task", None);
        tokio::spawn(async move {
            let TurnTicket {
                cancel,
                _done: done,
            } = ticket;
            execute_turn(&repository, sink.as_ref(), context, cancel).await;
            drop(done);
        })
    };
    let pid_file = harness.log.path().join("cli.pid");
    let helper_file = harness.log.path().join("helper.pid");
    for _ in 0..200 {
        if pid_file.exists() && helper_file.exists() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }

    host.shutdown().await;

    // Shutdown returned only after the turn ended and settled the run.
    assert_eq!(
        sink.emitted
            .lock()
            .unwrap()
            .last()
            .map(|(method, _)| method.clone()),
        Some("run.cancelled".to_string())
    );
    assert_eq!(
        harness.repository.get_run(&run_id).await.unwrap().status,
        "cancelled"
    );
    for path in [&pid_file, &helper_file] {
        assert_process_gone(read_pid(path)).await;
    }
    task.await.unwrap();
    // No new turn starts while the app quits.
    assert!(host.register("late-run").is_err());
}

fn read_pid(path: &Path) -> i32 {
    std::fs::read_to_string(path)
        .unwrap()
        .trim()
        .parse()
        .unwrap()
}

async fn assert_process_gone(pid: i32) {
    for _ in 0..100 {
        // SAFETY: signal 0 only checks that the process exists.
        if unsafe { libc::kill(pid, 0) } != 0 {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("process {pid} is still running");
}

#[tokio::test]
async fn chat_engine_cli_exit_ends_the_turn_even_if_a_helper_keeps_stdout_open() {
    let kind = CliKind::Claude;
    let harness = Harness::new(kind).await;
    // The helper inherits stdout and outlives the CLI.
    let program = harness.fake_cli(
        kind,
        r#"
echo '{"type":"system","subtype":"init","session_id":"claude-conv-1"}'
sleep 300 &
echo $! > "$LOG/helper.pid"
echo '{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"All done"}}}'
echo '{"type":"result","subtype":"success","is_error":false,"result":"All done"}'
exit 0
"#,
    );
    let started = std::time::Instant::now();
    let finished = tokio::time::timeout(
        std::time::Duration::from_secs(20),
        harness.turn(kind, &program, "Quick task", None),
    )
    .await;
    let (run_id, emitted) = finished.expect("the turn ends when the CLI exits");
    assert!(started.elapsed() < std::time::Duration::from_secs(10));
    assert_eq!(
        emitted.last().map(|(method, _)| method.as_str()),
        Some("run.completed")
    );
    assert_eq!(
        harness.repository.get_run(&run_id).await.unwrap().status,
        "completed"
    );
    assert!(harness
        .transcript()
        .await
        .contains(&("assistant_message".to_string(), "All done".to_string())));
    assert_process_gone(read_pid(&harness.log.path().join("helper.pid"))).await;
}

#[tokio::test]
async fn chat_engine_cli_environment_keeps_the_profile_but_not_clovy_credentials() {
    let kind = CliKind::Pi;
    let harness = Harness::new(kind).await;
    let program = harness.fake_cli(
        kind,
        r#"env > "$LOG/env"
echo '{"type":"agent_end","messages":[]}'"#,
    );
    let run_id = harness.new_run(kind, "Hi").await;
    let sink = TestSink {
        repository: harness.repository.clone(),
        emitted: StdMutex::new(Vec::new()),
    };
    let mut context = harness.context(kind, &program, &run_id, "Hi", None);
    for (name, value) in [
        ("HOME", "/Users/someone"),
        ("PI_CODING_AGENT_DIR", "/Users/someone/.pi/agent"),
        ("OPENROUTER_API_KEY", "user-own-key"),
        ("OS_CLOVY_LOCAL_DEV_BEARER_TOKEN", "clovy-dev-token"),
        ("OS_JUNE_LOCAL_DEV_BEARER_TOKEN", "june-dev-token"),
        ("OS_ACCOUNTS_CLIENT_ID", "clovy-client"),
        ("CLOVY_API_URL", "http://127.0.0.1:8787"),
        ("GOOGLE_OAUTH_CLIENT_SECRET", "clovy-oauth-secret"),
    ] {
        context.env.insert(name.to_string(), value.to_string());
    }
    let (_cancel, receiver) = watch::channel(false);
    execute_turn(&harness.repository, &sink, context, receiver).await;

    let env = std::fs::read_to_string(harness.log.path().join("env")).unwrap();
    for kept in [
        "HOME=/Users/someone",
        "PI_CODING_AGENT_DIR=/Users/someone/.pi/agent",
        "PATH=/usr/bin:/bin",
        "OPENROUTER_API_KEY=user-own-key",
    ] {
        assert!(env.lines().any(|line| line == kept), "missing {kept}");
    }
    for secret in [
        "clovy-dev-token",
        "june-dev-token",
        "clovy-client",
        "127.0.0.1:8787",
        "clovy-oauth-secret",
    ] {
        assert!(!env.contains(secret), "{secret} reached the CLI");
    }
}

#[tokio::test]
async fn chat_engine_cancel_during_launch_preparation_spawns_nothing() {
    let kind = CliKind::Claude;
    let harness = Harness::new(kind).await;
    let program = harness.fake_cli(kind, FAKE_CLAUDE);
    let host = ChatEngineHost::default();

    // The cancel arrives after the run is registered but before the turn
    // reaches its launch (while the message is saved or the CLI resolved).
    let run_id = harness.new_run(kind, "Never run").await;
    let ticket = host.register(&run_id).unwrap();
    assert!(host.cancel(&run_id), "the registered run is cancellable");
    let sink = TestSink {
        repository: harness.repository.clone(),
        emitted: StdMutex::new(Vec::new()),
    };
    let TurnTicket {
        cancel,
        _done: _completion,
    } = ticket;
    execute_turn(
        &harness.repository,
        &sink,
        harness.context(kind, &program, &run_id, "Never run", None),
        cancel,
    )
    .await;
    assert!(
        !harness.log.path().join("count").exists(),
        "the CLI was started after the cancel"
    );
    assert_eq!(
        sink.emitted
            .into_inner()
            .unwrap()
            .last()
            .map(|(method, _)| method.clone()),
        Some("run.cancelled".to_string())
    );
    assert_eq!(
        harness.repository.get_run(&run_id).await.unwrap().status,
        "cancelled"
    );

    // A run already settled by someone else is not started either.
    let settled = harness.new_run(kind, "Settled").await;
    harness
        .repository
        .update_run_status(&settled, "cancelled", None, None, None)
        .await
        .unwrap();
    let (_cancel, receiver) = watch::channel(false);
    execute_turn(
        &harness.repository,
        &TestSink {
            repository: harness.repository.clone(),
            emitted: StdMutex::new(Vec::new()),
        },
        harness.context(kind, &program, &settled, "Settled", None),
        receiver,
    )
    .await;
    assert!(!harness.log.path().join("count").exists());
    assert_eq!(
        harness.repository.get_run(&settled).await.unwrap().status,
        "cancelled"
    );
}

/// Real CLIs, two turns each in a temporary directory. Run apart with
/// `cargo test chat_engine_real -- --ignored --nocapture`; they use the
/// user's own CLI login and quota.
mod real {
    use super::*;

    async fn two_turns_on_real_cli(kind: CliKind) {
        let env = crate::llm::shell_env::login_env().await;
        let Some(program) = env.which(kind.id()) else {
            panic!("{} is not installed", kind.id());
        };
        let harness = Harness::new(kind).await;
        let turn = |input: &'static str| {
            let harness = &harness;
            let program = program.clone();
            let env = env.vars().clone();
            async move {
                let run_id = harness.new_run(kind, input).await;
                let sink = TestSink {
                    repository: harness.repository.clone(),
                    emitted: StdMutex::new(Vec::new()),
                };
                let (_cancel, receiver) = watch::channel(false);
                let mut context = harness.context(kind, &program, &run_id, input, None);
                context.env = env;
                execute_turn(&harness.repository, &sink, context, receiver).await;
                let emitted = sink.emitted.into_inner().unwrap();
                println!("{kind:?} turn {input:?}: {:?}", emitted.last());
                run_id
            }
        };
        let first = turn("Remember the code word PELICAN-42. Reply with only: stored").await;
        let second = turn("What was the code word? Reply with only the code word.").await;
        let transcript = harness.transcript().await;
        for (kind, text) in &transcript {
            println!("  {kind}: {}", text.chars().take(200).collect::<String>());
        }
        for run in [first, second] {
            assert_eq!(
                harness.repository.get_run(&run).await.unwrap().status,
                "completed"
            );
        }
        let answer = transcript
            .iter()
            .rev()
            .find(|(kind, _)| kind == "assistant_message")
            .map(|(_, text)| text.clone())
            .unwrap_or_default();
        assert!(
            answer.contains("PELICAN-42"),
            "{kind:?} lost the conversation: {answer}"
        );
    }

    #[tokio::test]
    #[ignore = "runs the installed claude CLI"]
    async fn chat_engine_real_claude_two_turns() {
        two_turns_on_real_cli(CliKind::Claude).await;
    }

    #[tokio::test]
    #[ignore = "runs the installed codex CLI"]
    async fn chat_engine_real_codex_two_turns() {
        two_turns_on_real_cli(CliKind::Codex).await;
    }

    #[tokio::test]
    #[ignore = "runs the installed pi CLI"]
    async fn chat_engine_real_pi_two_turns() {
        two_turns_on_real_cli(CliKind::Pi).await;
    }
}
