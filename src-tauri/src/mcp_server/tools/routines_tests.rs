use super::{call, descriptors, ActivityAccess, McpData, ToolCtx};
use crate::approval_broker::testing::{Scripted, TestBroker};
use crate::llm::cli::CliKind;
use serde_json::json;

#[tokio::test]
async fn routines_crud_approved_and_persisted_for_both_clis() {
    for cli in [CliKind::Claude, CliKind::Codex] {
        let broker = TestBroker::new(std::iter::repeat_n(Scripted::Approve, 5)).await;
        let binding = broker.turn(cli, true).await;
        let data = McpData {
            notes: broker.pool().clone(),
            profile: "default".into(),
            memory_enabled: true,
            activity: ActivityAccess::Off,
            now: chrono::Utc::now(),
        };
        let ctx = ToolCtx {
            data: &data,
            app: None,
            binding: Some(&binding),
            broker: Some(broker.broker()),
        };
        let created = call(&ctx, "create_routine", &json!({"prompt":"Summarize yesterday", "schedule":"every 1d", "safetyMode":"sandboxed"})).await.unwrap();
        let id = created["id"].as_str().unwrap();
        let args = json!({"routineId":id});
        call(
            &ctx,
            "update_routine",
            &json!({"routineId":id,"name":"Daily recap"}),
        )
        .await
        .unwrap();
        assert_eq!(
            call(&ctx, "pause_routine", &args).await.unwrap()["state"],
            "paused"
        );
        assert_eq!(
            call(&ctx, "resume_routine", &args).await.unwrap()["state"],
            "scheduled"
        );
        let listed = call(&ctx, "list_routines", &json!({})).await.unwrap();
        assert_eq!(listed[0]["name"], "Daily recap");
        assert_eq!(listed[0]["id"], id);
        call(&ctx, "delete_routine", &args).await.unwrap();
        assert_eq!(
            call(&ctx, "list_routines", &json!({})).await.unwrap(),
            json!([])
        );
        assert_eq!(broker.requests().len(), 5);
    }
}

#[tokio::test]
async fn routines_denied_unattended_unbound_and_unavailable_leave_database_unchanged() {
    let broker = TestBroker::new(std::iter::repeat_n(Scripted::Deny, 5)).await;
    let binding = broker.turn(CliKind::Claude, true).await;
    let routine = crate::routines::create(
        broker.pool(),
        serde_json::from_value(json!({"prompt":"Read notes", "schedule":"every 1d"})).unwrap(),
    )
    .await
    .unwrap();
    let data = McpData {
        notes: broker.pool().clone(),
        profile: "default".into(),
        memory_enabled: true,
        activity: ActivityAccess::Off,
        now: chrono::Utc::now(),
    };
    let ctx = ToolCtx {
        data: &data,
        app: None,
        binding: Some(&binding),
        broker: Some(broker.broker()),
    };
    let create_args = json!({"name":"Changed", "prompt":"Changed", "schedule":"every 2d","safetyMode":"sandboxed"});
    let unattended = broker.turn(CliKind::Codex, false).await;
    for name in [
        "create_routine",
        "update_routine",
        "pause_routine",
        "resume_routine",
        "delete_routine",
    ] {
        let args = if name == "create_routine" {
            create_args.clone()
        } else if name == "update_routine" {
            json!({"routineId":routine.id,"name":"Changed","prompt":"Changed","schedule":"every 2d","safetyMode":"sandboxed"})
        } else {
            json!({"routineId":routine.id})
        };
        assert_eq!(
            call(&ctx, name, &args).await.unwrap_err().code,
            "user_denied"
        );
        assert_eq!(
            call(
                &ToolCtx {
                    binding: Some(&unattended),
                    ..ctx
                },
                name,
                &args
            )
            .await
            .unwrap_err()
            .code,
            "routine_tool_not_enabled"
        );
        assert_eq!(
            call(&ToolCtx::unbound(&data), name, &args)
                .await
                .unwrap_err()
                .code,
            "turn_required"
        );
        assert_eq!(
            call(
                &ToolCtx {
                    broker: None,
                    ..ctx
                },
                name,
                &args
            )
            .await
            .unwrap_err()
            .code,
            "approval_unavailable"
        );
    }
    assert_eq!(
        crate::routines::get(broker.pool(), &routine.id)
            .await
            .unwrap()
            .name,
        routine.name
    );
    assert_eq!(crate::routines::list(broker.pool()).await.unwrap().len(), 1);
    assert_eq!(broker.requests().len(), 5);
    for name in [
        "list_routines",
        "create_routine",
        "update_routine",
        "pause_routine",
        "resume_routine",
        "delete_routine",
    ] {
        assert!(!descriptors(&ToolCtx::unbound(&data))
            .await
            .iter()
            .any(|tool| tool["name"] == name));
        let tools = descriptors(&ctx).await;
        let tool = tools.iter().find(|tool| tool["name"] == name).unwrap();
        assert_eq!(tool["annotations"]["readOnlyHint"], name == "list_routines");
    }
}

#[tokio::test]
async fn routine_history_pages_and_claim_reports_busy_or_blocked() {
    let broker = TestBroker::new([]).await;
    let binding = broker.turn(CliKind::Claude, true).await;
    let routine = crate::routines::create(broker.pool(), serde_json::from_value(json!({"prompt":"Read notes","schedule":"every 1d","timezone":"America/Sao_Paulo","enabledToolsets":["skills"]})).unwrap()).await.unwrap();
    let claim = crate::routines::claim_for_test(broker.pool(), &routine.id, "manual", false)
        .await
        .unwrap()
        .unwrap();
    assert!(
        crate::routines::claim_for_test(broker.pool(), &routine.id, "manual", false)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        crate::routines::trigger_blocked_error(broker.pool(), &routine.id)
            .await
            .unwrap()
            .code,
        "routine_busy"
    );
    let data = McpData {
        notes: broker.pool().clone(),
        profile: "default".into(),
        memory_enabled: true,
        activity: ActivityAccess::Off,
        now: chrono::Utc::now(),
    };
    let ctx = ToolCtx {
        binding: Some(&binding),
        ..ToolCtx::unbound(&data)
    };
    let page = call(
        &ctx,
        "list_routine_runs",
        &json!({"routineId":routine.id,"limit":1}),
    )
    .await
    .unwrap();
    assert_eq!(page["items"][0]["id"], claim.routine_run_id);
    assert_eq!(page["hasMore"], false);
    let page = call(
        &ctx,
        "list_routine_runs",
        &json!({"routineId":routine.id,"offset":1}),
    )
    .await
    .unwrap();
    assert_eq!(page["items"], json!([]));
    let paused = crate::routines::create(
        broker.pool(),
        serde_json::from_value(json!({"prompt":"Paused","schedule":"every 1d","state":"paused"}))
            .unwrap(),
    )
    .await
    .unwrap();
    assert_eq!(
        crate::routines::trigger_blocked_error(broker.pool(), &paused.id)
            .await
            .unwrap()
            .code,
        "routine_blocked"
    );
    assert_eq!(crate::routines::create(broker.pool(),serde_json::from_value(json!({"prompt":"Paused invalid timezone","schedule":"every 1d","state":"paused","timezone":"invalid"})).unwrap()).await.unwrap_err().code,"routine_timezone_invalid");
}

#[tokio::test]
async fn stopped_turn_cannot_mutate_routines_or_admit_a_claim() {
    let broker = TestBroker::new([]).await;
    let binding = broker.turn(CliKind::Claude, true).await;
    let routine = crate::routines::create(
        broker.pool(),
        serde_json::from_value(json!({"prompt":"Keep","schedule":"every 1d"})).unwrap(),
    )
    .await
    .unwrap();
    sqlx::query::query("UPDATE agent_runs SET status='cancelled' WHERE id=?")
        .bind(&binding.run_id)
        .execute(broker.pool())
        .await
        .unwrap();
    let turn = Some((
        "default",
        crate::db::repositories::McpMutation::Turn(
            &binding.session_id,
            &binding.run_id,
            &binding.authority,
        ),
    ));
    assert_eq!(
        crate::routines::create_authorized(
            broker.pool(),
            serde_json::from_value(json!({"prompt":"Denied","schedule":"every 1d"})).unwrap(),
            turn
        )
        .await
        .unwrap_err()
        .code,
        "turn_expired"
    );
    assert_eq!(
        crate::routines::update_authorized(
            broker.pool(),
            serde_json::from_value(json!({"routineId":routine.id,"name":"Denied"})).unwrap(),
            turn
        )
        .await
        .unwrap_err()
        .code,
        "turn_expired"
    );
    assert_eq!(
        crate::routines::pause_authorized(broker.pool(), &routine.id, turn)
            .await
            .unwrap_err()
            .code,
        "turn_expired"
    );
    assert_eq!(
        crate::routines::resume_authorized(broker.pool(), &routine.id, turn)
            .await
            .unwrap_err()
            .code,
        "turn_expired"
    );
    assert_eq!(
        crate::routines::delete_authorized(broker.pool(), &routine.id, turn)
            .await
            .unwrap_err()
            .code,
        "turn_expired"
    );
    assert_eq!(
        crate::routines::list(broker.pool()).await.unwrap(),
        vec![routine]
    );
}

#[cfg(any(unix, windows))]
mod external_routines_and_skills {
    use super::*;
    use crate::approval_broker::external::ExternalApprovalBroker;
    use crate::mcp_server::channel;
    use serde_json::Value;
    use std::{path::PathBuf, sync::Arc, time::Duration};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    struct FixtureRuntime {
        environment: Arc<crate::llm::shell_env::LoginEnv>,
        launched: std::sync::atomic::AtomicUsize,
    }

    impl super::super::routines::RoutineRunPort for FixtureRuntime {
        fn environment(
            &self,
        ) -> futures_util::future::BoxFuture<'_, Arc<crate::llm::shell_env::LoginEnv>> {
            Box::pin(async { self.environment.clone() })
        }

        fn launch<'a>(
            &'a self,
            pool: &'a sqlx_sqlite::SqlitePool,
            claim: crate::routines::Claim,
            mutation: crate::routines::RoutineMutation<'a>,
        ) -> futures_util::future::BoxFuture<
            'a,
            Result<crate::routines::AgentRoutineRunDto, crate::domain::types::AppError>,
        > {
            Box::pin(async move {
                let (profile, authority) = mutation.expect("host admission");
                authority.before_commit(profile)?;
                self.launched
                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                crate::routines::list_runs(pool, Some(&claim.routine_id))
                    .await?
                    .into_iter()
                    .find(|run| run.id == claim.routine_run_id)
                    .ok_or_else(|| {
                        crate::domain::types::AppError::new(
                            "routine_run_missing",
                            "No durable fixture claim.",
                        )
                    })
            })
        }
    }

    struct Fixture {
        _directory: tempfile::TempDir,
        roots: Vec<PathBuf>,
        fixture_db: TestBroker,
        client: BufReader<channel::TestStream>,
        approvals: Arc<ExternalApprovalBroker>,
        runtime: Arc<FixtureRuntime>,
        listener: channel::Listener,
        next_id: i64,
    }

    impl Fixture {
        async fn new() -> Self {
            let fixture_db = TestBroker::new([]).await;
            let approvals = Arc::new(ExternalApprovalBroker::new(
                || Some("default".into()),
                || {},
                Duration::from_secs(30),
            ));
            let directory = tempfile::tempdir().unwrap();
            let roots = vec![directory.path().join("skills")];
            std::fs::create_dir_all(roots[0].join("example")).unwrap();
            std::fs::write(roots[0].join("example/SKILL.md"), "Original instructions").unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let executable = directory.path().join("claude");
                std::fs::write(&executable, "synthetic CLI name only").unwrap();
                std::fs::set_permissions(executable, std::fs::Permissions::from_mode(0o700))
                    .unwrap();
            }
            #[cfg(windows)]
            std::fs::write(
                directory.path().join("claude.exe"),
                "synthetic CLI name only",
            )
            .unwrap();
            let runtime = Arc::new(FixtureRuntime {
                environment: Arc::new(crate::llm::shell_env::LoginEnv::from_vars(
                    [
                        (
                            "PATH".to_owned(),
                            directory.path().to_string_lossy().into_owned(),
                        ),
                        ("PATHEXT".to_owned(), ".EXE".to_owned()),
                    ]
                    .into_iter()
                    .collect(),
                )),
                launched: std::sync::atomic::AtomicUsize::new(0),
            });
            let server_runtime = runtime.clone();
            let server_db = fixture_db.pool().clone();
            let server_approvals = Arc::clone(&approvals);
            let server_roots = roots.clone();
            let handler: channel::AuthenticatedHandler = Arc::new(move |message, connection| {
                let notes = server_db.clone();
                let approvals = Arc::clone(&server_approvals);
                let roots = server_roots.clone();
                let runtime = server_runtime.clone();
                Box::pin(async move {
                    let channel::Connection::External(connection) = connection else {
                        panic!("fixture has no agent turns");
                    };
                    let data = McpData {
                        notes,
                        profile: "default".into(),
                        memory_enabled: false,
                        activity: ActivityAccess::Off,
                        now: chrono::Utc::now(),
                    };
                    let ctx = ToolCtx::unbound(&data);
                    let scope = approvals.scope(&connection, &data.profile, None);
                    let tool = message["params"]["name"].as_str().unwrap();
                    let args = &message["params"]["arguments"];
                    let result = if tool.ends_with("skill") || tool == "list_skill_settings" {
                        super::super::vault_skills::call_scoped(
                            &ctx,
                            Some(&scope),
                            tool,
                            args,
                            &roots,
                        )
                        .await
                    } else {
                        super::super::routines::dispatch_with_runner(
                            &ctx,
                            Some(&scope),
                            tool,
                            args,
                            Some(runtime.as_ref()),
                        )
                        .await
                    };
                    Some(match result {
                        Ok(value) => json!({"jsonrpc":"2.0","id":message["id"],"result":value}),
                        Err(error) => {
                            json!({"jsonrpc":"2.0","id":message["id"],"error":error.code})
                        }
                    })
                })
            });
            let directory_path = directory.path().join("mcp");
            let listener = channel::Listener::start(&directory_path, handler)
                .await
                .unwrap();
            let mut client = BufReader::new(channel::test_stream(&directory_path).await);
            let secret = channel::read_secret(&directory_path).unwrap();
            let hello = format!("{}\n", channel::handshake(&secret));
            client.get_mut().write_all(hello.as_bytes()).await.unwrap();
            let mut ack = String::new();
            client.read_line(&mut ack).await.unwrap();
            assert_eq!(serde_json::from_str::<Value>(&ack).unwrap()["ok"], true);
            Self {
                _directory: directory,
                roots,
                fixture_db,
                client,
                approvals,
                runtime,
                listener,
                next_id: 0,
            }
        }

        async fn request(&mut self, name: &str, arguments: Value) {
            self.next_id += 1;
            let message = json!({"jsonrpc":"2.0","id":self.next_id,"method":"tools/call","params":{"name":name,"arguments":arguments}});
            self.client
                .get_mut()
                .write_all(format!("{message}\n").as_bytes())
                .await
                .unwrap();
        }

        async fn response(&mut self) -> Value {
            let mut line = String::new();
            tokio::time::timeout(Duration::from_secs(5), self.client.read_line(&mut line))
                .await
                .unwrap()
                .unwrap();
            let response = serde_json::from_str::<Value>(&line).unwrap();
            assert_eq!(response["id"], self.next_id);
            response
        }

        async fn pending(&self) -> Value {
            tokio::time::timeout(Duration::from_secs(5), async {
                loop {
                    if let Some(item) = self.approvals.pending().first() {
                        return serde_json::to_value(item).unwrap();
                    }
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            })
            .await
            .unwrap()
        }

        fn decide(&self, ticket: &Value, approve: bool) {
            self.approvals
                .decide(ticket["id"].as_str().unwrap(), approve)
                .unwrap();
        }

        async fn stop(self) {
            drop(self.client);
            self.listener.stop_and_wait().await;
        }
    }

    #[tokio::test]
    async fn external_routines_and_skills_run_only_the_approved_routine_mutation() {
        let mut fixture = Fixture::new().await;
        let args = json!({"name":"Fixture routine","prompt":"Summarize synthetic notes","schedule":"every 1d","safetyMode":"sandboxed","state":"paused"});
        fixture.request("create_routine", args.clone()).await;
        let ticket = fixture.pending().await;
        assert_eq!(ticket["tool"], "create_routine");
        assert!(crate::routines::list(fixture.fixture_db.pool())
            .await
            .unwrap()
            .is_empty());
        fixture.decide(&ticket, false);
        assert_eq!(fixture.response().await["error"], "user_denied");
        assert!(crate::routines::list(fixture.fixture_db.pool())
            .await
            .unwrap()
            .is_empty());

        fixture.request("create_routine", args).await;
        let ticket = fixture.pending().await;
        fixture.decide(&ticket, true);
        let created = fixture.response().await["result"].clone();
        let id = created["id"].as_str().unwrap();
        assert_eq!(created["state"], "paused");
        assert_eq!(
            crate::routines::list(fixture.fixture_db.pool())
                .await
                .unwrap()
                .len(),
            1
        );

        fixture
            .request(
                "update_routine",
                json!({"routineId":id,"name":"Stale edit"}),
            )
            .await;
        let ticket = fixture.pending().await;
        crate::routines::update(
            fixture.fixture_db.pool(),
            serde_json::from_value(json!({"routineId":id,"name":"Another edit"})).unwrap(),
        )
        .await
        .unwrap();
        fixture.decide(&ticket, true);
        assert_eq!(fixture.response().await["error"], "routine_conflict");
        assert_eq!(
            crate::routines::get(fixture.fixture_db.pool(), id)
                .await
                .unwrap()
                .name,
            "Another edit"
        );

        fixture
            .request("resume_routine", json!({"routineId":id}))
            .await;
        let ticket = fixture.pending().await;
        assert_eq!(
            crate::routines::get(fixture.fixture_db.pool(), id)
                .await
                .unwrap()
                .state,
            "paused"
        );
        fixture.decide(&ticket, true);
        assert_eq!(fixture.response().await["result"]["state"], "scheduled");
        fixture.stop().await;
    }

    #[tokio::test]
    async fn external_routines_and_skills_claims_once_and_keeps_unrestricted_unavailable() {
        use std::sync::atomic::Ordering::SeqCst;
        let mut fixture = Fixture::new().await;
        let pool = fixture.fixture_db.pool().clone();
        let routine = crate::routines::create(
            &pool,
            serde_json::from_value(json!({"name":"External routine","prompt":"Summarize fixture","schedule":"every 1d","model":"__clovy_cli_engine__:claude"})).unwrap(),
        ).await.unwrap();
        let args = json!({"routineId":routine.id});
        fixture.request("run_routine", args.clone()).await;
        let ticket = fixture.pending().await;
        assert!(ticket["preview"]
            .as_str()
            .unwrap()
            .contains("Summarize fixture"));
        assert_eq!(fixture.runtime.launched.load(SeqCst), 0);
        assert!(crate::routines::list_runs(&pool, Some(&routine.id))
            .await
            .unwrap()
            .is_empty());
        fixture.decide(&ticket, false);
        assert_eq!(fixture.response().await["error"], "user_denied");

        fixture.request("run_routine", args.clone()).await;
        let ticket = fixture.pending().await;
        fixture.decide(&ticket, true);
        let response = fixture.response().await;
        assert_eq!(response["result"]["status"], "queued");
        assert!(response["result"].get("agentSessionId").is_none());
        assert_eq!(fixture.runtime.launched.load(SeqCst), 1);
        let run = crate::routines::list_runs(&pool, Some(&routine.id))
            .await
            .unwrap();
        assert_eq!(run.len(), 1);
        assert_eq!(run[0].id, response["result"]["id"].as_str().unwrap());

        fixture.request("run_routine", args).await;
        let ticket = fixture.pending().await;
        fixture.decide(&ticket, true);
        assert_eq!(fixture.response().await["error"], "routine_busy");
        assert_eq!(fixture.runtime.launched.load(SeqCst), 1);

        let unrestricted = crate::routines::create(
            &pool,
            serde_json::from_value(json!({"name":"Native unrestricted routine","prompt":"Read fixture","schedule":"every 1d","safetyMode":"unrestricted"})).unwrap(),
        ).await.unwrap();
        fixture
            .request("run_routine", json!({"routineId":unrestricted.id}))
            .await;
        assert_eq!(
            fixture.response().await["error"],
            "routine_external_safety_mode"
        );
        assert!(fixture.approvals.pending().is_empty());
        assert_eq!(fixture.runtime.launched.load(SeqCst), 1);
        fixture.stop().await;
    }

    #[tokio::test]
    async fn external_routines_and_skills_rechecks_managed_file_and_settings() {
        let mut fixture = Fixture::new().await;
        let file = fixture.roots[0].join("example/SKILL.md");
        let args = json!({"name":"example","content":"Revised instructions"});
        fixture.request("update_skill", args.clone()).await;
        let ticket = fixture.pending().await;
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            "Original instructions"
        );
        fixture.decide(&ticket, false);
        assert_eq!(fixture.response().await["error"], "user_denied");
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            "Original instructions"
        );

        fixture.request("update_skill", args.clone()).await;
        let ticket = fixture.pending().await;
        std::fs::write(&file, "Other edit").unwrap();
        fixture.decide(&ticket, true);
        assert_eq!(fixture.response().await["error"], "agent_skill_conflict");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "Other edit");

        fixture.request("update_skill", args).await;
        let ticket = fixture.pending().await;
        fixture.decide(&ticket, true);
        let updated = fixture.response().await;
        assert_eq!(updated["result"]["id"], "example");
        assert!(updated["result"].get("root").is_none());
        assert!(std::fs::read_to_string(&file)
            .unwrap()
            .contains("Revised instructions"));
        fixture
            .request("disable_skill", json!({"name":"example"}))
            .await;
        let ticket = fixture.pending().await;
        assert!(
            crate::agent_sessions::AgentRepository::new(fixture.fixture_db.pool().clone())
                .skills()
                .await
                .unwrap()
                .is_empty()
        );
        fixture.decide(&ticket, true);
        assert_eq!(fixture.response().await["result"]["enabled"], false);
        assert!(
            !crate::agent_sessions::AgentRepository::new(fixture.fixture_db.pool().clone())
                .skills()
                .await
                .unwrap()[0]
                .enabled
        );
        fixture.stop().await;
    }
}
