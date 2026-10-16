//! The media group (parity slice 2.3): the artifact lands in the bound
//! session's workspace and in its artifact list, a video is followed within
//! the CLI's tool timeout and continued by job id, and unbound clients never
//! reach it. Clovy API is a fake: in process for the job logic, and a fake
//! HTTP server for the production client (in a child test process, since the
//! client reads its address and token from the environment).

use std::collections::VecDeque;
use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use futures_util::future::BoxFuture;
use serde_json::{json, Value};
use sqlx_sqlite::SqlitePool;

use super::media::{self, MediaApi, MediaTools, VideoPolling, GENERATE_IMAGE};
use super::{ActivityAccess, McpData, ToolCtx, ToolGroup, TurnBinding};
use crate::agent_runtime::{AgentRepository, AgentSafetyMode};
use crate::approval_broker::testing::TestBroker;
use crate::clovy_api::{GeneratedImageDto, VideoJobDto, VideoStatusDto};
use crate::domain::types::AppError;
use crate::llm::cli::CliKind;
use crate::providers::{EditImageRequest, GenerateImageRequest, GenerateVideoRequest};

const PNG_BYTES: &[u8] = b"\x89PNG\r\n\x1a\nfake image";

/// A fake Clovy API: records each request, answers images with fixed bytes
/// and video polls from a script, then done (or still rendering, `stuck`).
#[derive(Default)]
struct FakeClovyApi {
    calls: Mutex<Vec<String>>,
    statuses: Mutex<VecDeque<Result<VideoStatusDto, AppError>>>,
    /// With the script used up: still rendering (`true`) or done.
    stuck: Mutex<bool>,
    video_file: Option<std::path::PathBuf>,
}

impl FakeClovyApi {
    fn polling(video_file: &Path, statuses: Vec<Result<VideoStatusDto, AppError>>) -> Self {
        Self {
            statuses: Mutex::new(statuses.into()),
            video_file: Some(video_file.to_path_buf()),
            ..Self::default()
        }
    }

    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }

    fn image(&self) -> GeneratedImageDto {
        GeneratedImageDto {
            image_base64: BASE64.encode(PNG_BYTES),
            mime_type: "image/png".into(),
            model: "fixture-image".into(),
            provider: "fal".into(),
        }
    }

    fn completed(&self) -> VideoStatusDto {
        VideoStatusDto::Completed {
            path: self
                .video_file
                .as_ref()
                .expect("a video file")
                .to_string_lossy()
                .into_owned(),
            mime_type: "video/mp4".into(),
            size_bytes: 9,
            model: "fixture-video".into(),
        }
    }
}

fn processing() -> Result<VideoStatusDto, AppError> {
    Ok(VideoStatusDto::Processing {
        average_execution_ms: 60_000,
        execution_ms: 1_000,
    })
}

impl MediaApi for FakeClovyApi {
    fn generate_image(
        &self,
        request: GenerateImageRequest,
    ) -> BoxFuture<'_, Result<GeneratedImageDto, AppError>> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("image:{}", request.prompt));
        Box::pin(async move { Ok(self.image()) })
    }

    fn edit_image(
        &self,
        request: EditImageRequest,
    ) -> BoxFuture<'_, Result<GeneratedImageDto, AppError>> {
        self.calls.lock().unwrap().push(format!(
            "edit:{}:{}:{}",
            request.prompt,
            request.mime_type.unwrap_or_default(),
            request.image
        ));
        Box::pin(async move { Ok(self.image()) })
    }

    fn video_generate(
        &self,
        request: GenerateVideoRequest,
    ) -> BoxFuture<'_, Result<VideoJobDto, AppError>> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("video:{}", request.prompt));
        Box::pin(async move {
            Ok(VideoJobDto {
                job_id: "job-1".into(),
            })
        })
    }

    fn video_status(&self, job_id: String) -> BoxFuture<'_, Result<VideoStatusDto, AppError>> {
        self.calls.lock().unwrap().push(format!("poll:{job_id}"));
        Box::pin(async move {
            let next = self.statuses.lock().unwrap().pop_front();
            match next {
                Some(status) => status,
                None if *self.stuck.lock().unwrap() => processing(),
                None => Ok(self.completed()),
            }
        })
    }
}

/// A turn whose session workspace is a fresh folder.
struct Turn {
    broker: TestBroker,
    binding: TurnBinding,
    workspace: tempfile::TempDir,
}

impl Turn {
    async fn new(safety_mode: AgentSafetyMode) -> Self {
        let broker = TestBroker::new([]).await;
        let mut binding = broker.turn(CliKind::Codex, true).await;
        binding.safety_mode = safety_mode;
        let workspace = tempfile::tempdir().unwrap();
        sqlx::query::query("UPDATE agent_sessions SET workspace_path = ? WHERE id = ?")
            .bind(workspace.path().to_string_lossy().as_ref())
            .bind(&binding.session_id)
            .execute(broker.pool())
            .await
            .unwrap();
        Self {
            broker,
            binding,
            workspace,
        }
    }

    fn pool(&self) -> &SqlitePool {
        self.broker.pool()
    }

    fn data(&self) -> McpData {
        crate::mcp_server::tests::data(self.pool().clone(), ActivityAccess::Off)
    }

    async fn call(
        &self,
        api: &FakeClovyApi,
        polling: VideoPolling,
        name: &str,
        arguments: Value,
    ) -> Result<Value, AppError> {
        media::call(self.pool(), &self.binding, api, polling, name, &arguments).await
    }

    /// The session's artifacts, as `list_agent_artifacts` reads them.
    async fn artifacts(&self) -> Vec<crate::agent_runtime::AgentArtifactDto> {
        AgentRepository::new(self.pool().clone())
            .artifacts(&self.binding.session_id)
            .await
            .unwrap()
    }
}

fn quick(wait: Duration) -> VideoPolling {
    VideoPolling {
        interval: Duration::from_millis(5),
        wait: Some(wait),
    }
}

fn names(tools: &[Value]) -> Vec<&str> {
    tools
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect()
}

#[tokio::test]
async fn media_generate_image_lands_in_the_session_workspace_and_artifact_list() {
    let turn = Turn::new(AgentSafetyMode::Sandboxed).await;
    let api = FakeClovyApi::default();

    let result = turn
        .call(
            &api,
            quick(Duration::from_secs(5)),
            GENERATE_IMAGE,
            json!({ "prompt": "  a calm lake " }),
        )
        .await
        .unwrap();

    assert_eq!(api.calls(), ["image:a calm lake"]);
    assert_eq!(result["mediaType"], "image");
    assert_eq!(result["prompt"], "a calm lake");
    assert_eq!(result["mimeType"], "image/png");
    let path = Path::new(result["path"].as_str().unwrap());
    let artifacts = turn
        .workspace
        .path()
        .canonicalize()
        .unwrap()
        .join("artifacts");
    assert_eq!(path.parent().unwrap(), artifacts);
    assert_eq!(std::fs::read(path).unwrap(), PNG_BYTES);
    let listed = turn.artifacts().await;
    assert_eq!(listed.len(), 1);
    assert_eq!(Path::new(&listed[0].path), path);
    assert_eq!(
        listed[0].run_id.as_deref(),
        Some(turn.binding.run_id.as_str())
    );
    assert_eq!(listed[0].mime_type.as_deref(), Some("image/png"));
    assert_eq!(listed[0].action, "created");
    assert_eq!(listed[0].size_bytes, Some(PNG_BYTES.len() as i64));
}

#[tokio::test]
async fn media_tools_are_bound_only_listed_without_read_only_hint_and_refused_unbound() {
    let turn = Turn::new(AgentSafetyMode::Sandboxed).await;
    let data = turn.data();
    let unbound = ToolCtx::unbound(&data);
    let unbound_names = super::descriptors(&unbound);
    for name in ["generate_image", "edit_image", "generate_video"] {
        assert!(!names(&unbound_names).contains(&name), "{name} unbound");
        let refused = super::call(&unbound, name, &json!({ "prompt": "x" }))
            .await
            .unwrap_err();
        assert_eq!(refused.code, "turn_required");
    }
    let bound = ToolCtx {
        binding: Some(&turn.binding),
        ..unbound
    };
    let listed = MediaTools.descriptors(&bound);
    assert_eq!(
        names(&listed),
        ["generate_image", "edit_image", "generate_video"]
    );
    for tool in &listed {
        assert_eq!(tool["annotations"]["readOnlyHint"], false, "{tool}");
        assert_eq!(tool["annotations"]["openWorldHint"], true, "{tool}");
    }
    assert!(names(&super::descriptors(&bound)).contains(&"generate_video"));
    assert!(turn.artifacts().await.is_empty());
}

#[tokio::test]
async fn media_edit_image_reads_sources_in_the_workspace_and_only_there_when_sandboxed() {
    let outside = tempfile::tempdir().unwrap();
    let outside_source = outside.path().join("photo.png");
    std::fs::write(&outside_source, PNG_BYTES).unwrap();

    let turn = Turn::new(AgentSafetyMode::Sandboxed).await;
    std::fs::write(turn.workspace.path().join("photo.png"), PNG_BYTES).unwrap();
    let api = FakeClovyApi::default();
    let polling = quick(Duration::from_secs(5));
    let edited = turn
        .call(
            &api,
            polling,
            "edit_image",
            json!({ "sourcePath": "photo.png", "instruction": "make it blue" }),
        )
        .await
        .unwrap();
    assert_eq!(edited["mediaType"], "image");
    assert_eq!(
        api.calls(),
        [format!(
            "edit:make it blue:image/png:{}",
            BASE64.encode(PNG_BYTES)
        )]
    );
    let denied = turn
        .call(
            &api,
            polling,
            "edit_image",
            json!({ "sourcePath": outside_source.to_string_lossy(), "instruction": "blue" }),
        )
        .await
        .unwrap_err();
    assert_eq!(denied.code, "agent_path_denied");
    std::fs::write(turn.workspace.path().join("notes.txt"), "text").unwrap();
    let invalid = turn
        .call(
            &api,
            polling,
            "edit_image",
            json!({ "sourcePath": "notes.txt", "instruction": "blue" }),
        )
        .await
        .unwrap_err();
    assert_eq!(invalid.code, "image_source_invalid");
    assert_eq!(
        api.calls().len(),
        1,
        "refused sources never reach Clovy API"
    );
    assert_eq!(turn.artifacts().await.len(), 1);

    let unrestricted = Turn::new(AgentSafetyMode::Unrestricted).await;
    let api = FakeClovyApi::default();
    unrestricted
        .call(
            &api,
            polling,
            "edit_image",
            json!({ "sourcePath": outside_source.to_string_lossy(), "instruction": "blue" }),
        )
        .await
        .unwrap();
    assert_eq!(unrestricted.artifacts().await.len(), 1);
}

#[tokio::test]
async fn media_generate_video_polls_until_done_and_records_the_video() {
    let turn = Turn::new(AgentSafetyMode::Sandboxed).await;
    let downloads = tempfile::tempdir().unwrap();
    let video = downloads.path().join("generated-video.mp4");
    std::fs::write(&video, b"fake mp4!").unwrap();
    let api = FakeClovyApi::polling(&video, vec![processing(), processing()]);

    let result = turn
        .call(
            &api,
            quick(Duration::from_secs(5)),
            "generate_video",
            json!({ "prompt": "waves", "duration": "5", "aspectRatio": "16:9" }),
        )
        .await
        .unwrap();

    assert_eq!(
        api.calls(),
        ["video:waves", "poll:job-1", "poll:job-1", "poll:job-1"]
    );
    assert_eq!(result["mediaType"], "video");
    assert_eq!(result["mimeType"], "video/mp4");
    assert_eq!(result["prompt"], "waves");
    let path = Path::new(result["path"].as_str().unwrap());
    assert!(path.starts_with(
        turn.workspace
            .path()
            .canonicalize()
            .unwrap()
            .join("artifacts")
    ));
    assert_eq!(std::fs::read(path).unwrap(), b"fake mp4!");
    let listed = turn.artifacts().await;
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].mime_type.as_deref(), Some("video/mp4"));
}

#[tokio::test]
async fn media_generate_video_answers_processing_before_the_cli_timeout_and_continues_by_job_id() {
    let turn = Turn::new(AgentSafetyMode::Sandboxed).await;
    let downloads = tempfile::tempdir().unwrap();
    let video = downloads.path().join("generated-video.mp4");
    std::fs::write(&video, b"fake mp4!").unwrap();
    // Still rendering when this call's wait runs out.
    let api = FakeClovyApi::polling(&video, vec![]);
    *api.stuck.lock().unwrap() = true;
    let waiting = turn
        .call(
            &api,
            quick(Duration::from_millis(60)),
            "generate_video",
            json!({ "prompt": "waves" }),
        )
        .await
        .unwrap();
    assert_eq!(waiting["status"], "processing");
    assert_eq!(waiting["jobId"], "job-1");
    assert!(turn.artifacts().await.is_empty());

    // The model continues the same job: no second submit.
    *api.stuck.lock().unwrap() = false;
    let done = turn
        .call(
            &api,
            quick(Duration::from_secs(5)),
            "generate_video",
            json!({ "jobId": "job-1" }),
        )
        .await
        .unwrap();
    assert_eq!(done["mediaType"], "video");
    assert_eq!(done["prompt"], Value::Null);
    let calls = api.calls();
    assert_eq!(
        calls
            .iter()
            .filter(|call| call.starts_with("video:"))
            .count(),
        1
    );
    assert!(
        calls[1..].iter().all(|call| call == "poll:job-1"),
        "{calls:?}"
    );
    assert_eq!(turn.artifacts().await.len(), 1);

    // The production wait leaves the CLI a minute (spike S3).
    assert_eq!(
        crate::chat_engine::policy::CLI_TOOL_TIMEOUT - media::MCP_VIDEO_WAIT,
        Duration::from_secs(60)
    );
}

#[tokio::test]
async fn media_generate_video_stops_when_the_run_ends() {
    let turn = Turn::new(AgentSafetyMode::Sandboxed).await;
    let video = tempfile::NamedTempFile::new().unwrap();
    let api = FakeClovyApi::polling(video.path(), vec![]);
    *api.stuck.lock().unwrap() = true;
    let repository = AgentRepository::new(turn.pool().clone());
    let run_id = turn.binding.run_id.clone();
    let cancel = async {
        tokio::time::sleep(Duration::from_millis(50)).await;
        repository
            .update_run_status(&run_id, "cancelled", None, None, None)
            .await
            .unwrap();
    };
    let (result, ()) = tokio::join!(
        turn.call(
            &api,
            quick(Duration::from_secs(30)),
            "generate_video",
            json!({ "prompt": "waves" }),
        ),
        cancel
    );
    assert_eq!(result.unwrap_err().code, "agent_tool_cancelled");
    assert!(turn.artifacts().await.is_empty());
}

#[tokio::test]
async fn media_generate_video_waiting_for_the_fal_key_is_a_normal_result() {
    let turn = Turn::new(AgentSafetyMode::Sandboxed).await;
    let video = tempfile::NamedTempFile::new().unwrap();
    let api = FakeClovyApi::polling(
        video.path(),
        vec![Err(AppError::new("fal_api_key_rejected", "rejected"))],
    );
    let result = turn
        .call(
            &api,
            quick(Duration::from_secs(5)),
            "generate_video",
            json!({ "prompt": "waves" }),
        )
        .await
        .unwrap();
    assert_eq!(result["status"], "waitingForProviderKey");
    assert_eq!(result["provider"], "fal");
    assert_eq!(result["jobId"], "job-1");
    assert_eq!(result["reason"], "fal_api_key_rejected");
    assert!(turn.artifacts().await.is_empty());

    let failed = FakeClovyApi::polling(
        video.path(),
        vec![Ok(VideoStatusDto::Failed {
            reason: "content_rejected".into(),
        })],
    );
    let error = turn
        .call(
            &failed,
            quick(Duration::from_secs(5)),
            "generate_video",
            json!({ "prompt": "waves" }),
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, "video_generation_failed");
}

#[tokio::test]
async fn media_routine_session_refuses_tools_its_routine_does_not_enable() {
    let turn = Turn::new(AgentSafetyMode::Sandboxed).await;
    let now = "2026-10-15T00:00:00Z";
    sqlx::query::query("INSERT INTO routines (id, name, prompt, schedule, created_at, updated_at, metadata_json, tool_catalog_version) VALUES ('routine-1', 'Daily', 'Summarise', 'every 1d', ?, ?, '{\"enabledToolsets\":[\"web\"]}', 1)")
        .bind(now)
        .bind(now)
        .execute(turn.pool())
        .await
        .unwrap();
    sqlx::query::query("INSERT INTO routine_runs (id, routine_id, agent_session_id, claim_token, trigger_kind, status, model, safety_mode, created_at, updated_at) VALUES ('routine-run-1', 'routine-1', ?, 'claim-1', 'schedule', 'running', 'codex', 'sandboxed', ?, ?)")
        .bind(&turn.binding.session_id)
        .bind(now)
        .bind(now)
        .execute(turn.pool())
        .await
        .unwrap();
    let api = FakeClovyApi::default();
    let error = turn
        .call(
            &api,
            quick(Duration::from_secs(5)),
            GENERATE_IMAGE,
            json!({ "prompt": "x" }),
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, "routine_tool_not_enabled");
    assert!(api.calls().is_empty());
    assert!(turn.artifacts().await.is_empty());
}

/// The production client against a fake Clovy API over HTTP. Runs itself in
/// a child process with `CLOVY_API_URL` and a local-dev token, so the
/// environment of the other tests never changes.
#[tokio::test]
async fn media_generate_image_reaches_a_fake_clovy_api_over_http() {
    const CHILD: &str = "CLOVY_MEDIA_TOOLS_HTTP_CHILD";
    if std::env::var(CHILD).is_ok() {
        let turn = Turn::new(AgentSafetyMode::Sandboxed).await;
        let data = turn.data();
        let ctx = ToolCtx {
            binding: Some(&turn.binding),
            ..ToolCtx::unbound(&data)
        };
        let result = MediaTools
            .call(&ctx, GENERATE_IMAGE, &json!({ "prompt": "a lighthouse" }))
            .await
            .unwrap();
        assert_eq!(result["model"], "fixture-image");
        assert_eq!(
            std::fs::read(result["path"].as_str().unwrap()).unwrap(),
            PNG_BYTES
        );
        assert_eq!(turn.artifacts().await.len(), 1);
        return;
    }
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        let mut buffer = vec![0; 8192];
        // Head and JSON body (small) until the body parses.
        let body = loop {
            let read = socket.read(&mut buffer).await.unwrap();
            assert!(read > 0, "connection closed early");
            request.extend_from_slice(&buffer[..read]);
            let text = String::from_utf8_lossy(&request).into_owned();
            if let Some((_, body)) = text.split_once("\r\n\r\n") {
                if let Ok(body) = serde_json::from_str::<Value>(body) {
                    break (text, body);
                }
            }
        };
        let payload = json!({ "success": true, "data": {
            "imageBase64": BASE64.encode(PNG_BYTES), "mimeType": "image/png",
            "model": "fixture-image", "provider": "fal" } })
        .to_string();
        let response = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}", payload.len());
        socket.write_all(response.as_bytes()).await.unwrap();
        body
    });
    let output = tokio::task::spawn_blocking(move || {
        std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "mcp_server::tools::media_tests::media_generate_image_reaches_a_fake_clovy_api_over_http",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .env("CLOVY_API_URL", format!("http://{address}"))
            .env("OS_CLOVY_LOCAL_DEV", "1")
            .env("OS_CLOVY_LOCAL_DEV_BEARER_TOKEN", "media-test-token")
            .output()
            .unwrap()
    })
    .await
    .unwrap();
    if !output.status.success() {
        server.abort();
    }
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let (head, body) = server.await.unwrap();
    assert!(head.starts_with("POST /v1/image/generate "), "{head}");
    assert!(
        head.to_ascii_lowercase()
            .contains("authorization: bearer media-test-token"),
        "{head}"
    );
    assert_eq!(body["prompt"], "a lighthouse");
    assert!(body["requestId"].as_str().is_some_and(|id| !id.is_empty()));
}
