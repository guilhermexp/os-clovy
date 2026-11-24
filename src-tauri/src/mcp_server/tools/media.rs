//! Image generation, image editing and video generation use the configured
//! fal account. Bound tools keep artifacts in the session workspace and link
//! them in `agent_artifacts`. The external catalog uses operation approvals
//! and stores results in a directory owned by the external connection.
//!
//! An image comes back as MCP content blocks (`protocol::content_value`): the
//! artifact (id, path, metadata) as text, never its base64, and the image as
//! an `image` block while it fits the 3 MiB block limit.

use std::collections::HashMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use futures_util::future::BoxFuture;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx_sqlite::SqlitePool;
use tauri::AppHandle;
use tokio::time::Instant;
use uuid::Uuid;

use super::computer_use::MAX_IMAGE_BASE64;
use super::{ToolCtx, ToolGroup, TurnBinding};
use crate::agent_sessions::{AgentRepository, AgentSafetyMode};
use crate::approval_broker::external::ExternalCall;
use crate::domain::types::AppError;
use crate::mcp_server::protocol;
use crate::mcp_server::turns::TURN_REQUIRED;
use crate::providers::fal::{GeneratedImageDto, VideoJobDto, VideoStatusDto};
use crate::providers::{EditImageRequest, GenerateImageRequest, GenerateVideoRequest};

pub const GENERATE_IMAGE: &str = "generate_image";
pub const EDIT_IMAGE: &str = "edit_image";
pub const GENERATE_VIDEO: &str = "generate_video";
const MEDIA_TOOLS: [&str; 3] = [GENERATE_IMAGE, EDIT_IMAGE, GENERATE_VIDEO];

/// Largest source image `edit_image` sends.
const MAX_EDIT_SOURCE_BYTES: usize = 20 * 1024 * 1024;
/// Time between two polls of a video job.
pub(crate) const VIDEO_POLL_INTERVAL: Duration = Duration::from_secs(2);
/// How long one MCP `generate_video` call follows its job before answering
/// `processing` with the job id (the model continues it with `jobId`): a
/// minute under the CLI's tool timeout (spike S3), so the CLI gets this
/// answer and never its own timeout.
pub const MCP_VIDEO_WAIT: Duration = Duration::from_secs(14 * 60);
const _: () =
    assert!(MCP_VIDEO_WAIT.as_secs() < crate::chat_engine::policy::CLI_TOOL_TIMEOUT.as_secs());

/// The tools of this file as a [`ToolGroup`].
pub struct MediaTools;

impl ToolGroup for MediaTools {
    fn owns(&self, name: &str) -> bool {
        MEDIA_TOOLS.contains(&name)
    }

    fn descriptors(&self, _ctx: &ToolCtx<'_>) -> Vec<Value> {
        descriptors()
    }

    fn call<'a>(
        &'a self,
        ctx: &'a ToolCtx<'a>,
        name: &'a str,
        arguments: &'a Value,
    ) -> BoxFuture<'a, Result<Value, AppError>> {
        Box::pin(async move {
            let binding = ctx.binding.ok_or_else(turn_required)?;
            let api = FalMedia { app: ctx.app };
            let polling = VideoPolling {
                interval: VIDEO_POLL_INTERVAL,
                wait: Some(MCP_VIDEO_WAIT),
            };
            call(&ctx.data.notes, binding, &api, polling, name, arguments).await
        })
    }
}

fn turn_required() -> AppError {
    AppError::new(
        TURN_REQUIRED,
        "Media tools require a bound Clovy run or an authorized external MCP connection.",
    )
}

/// Generates media for the bound session (it writes, so it is never
/// read-only, and it reaches a paid service).
fn media(name: &str, description: &str, input_schema: Value) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": input_schema,
        "annotations": {
            "readOnlyHint": false,
            "destructiveHint": false,
            "idempotentHint": false,
            "openWorldHint": true,
        },
    })
}

fn descriptors() -> Vec<Value> {
    vec![
        media(
            GENERATE_IMAGE,
            "Generate an image from a text description. The file is saved in the session's artifacts and shown in the conversation.",
            json!({ "type": "object", "properties": { "prompt": { "type": "string" } }, "required": ["prompt"], "additionalProperties": false }),
        ),
        media(
            EDIT_IMAGE,
            "Edit an image file in the Clovy session workspace (PNG, JPEG, WebP, or GIF, up to 20 MB). The result is saved in the session's artifacts and shown in the conversation.",
            json!({ "type": "object", "properties": { "sourcePath": { "type": "string", "description": "Path of the image, relative to the session workspace or absolute." }, "instruction": { "type": "string" } }, "required": ["sourcePath", "instruction"], "additionalProperties": false }),
        ),
        media(
            GENERATE_VIDEO,
            "Generate a short video from a text description and show it in the conversation. Pass prompt to start a new video. If an earlier call returned status processing or waitingForProviderKey, continue that same video by calling again with only its jobId (for waitingForProviderKey, once the user has saved a valid fal key); never start a new video for it.",
            json!({ "type": "object", "properties": { "prompt": { "type": "string" }, "duration": { "type": "string" }, "aspectRatio": { "type": "string" }, "audio": { "type": "boolean" }, "jobId": { "type": "string" } }, "required": [], "additionalProperties": false }),
        ),
    ]
}

/// The media operations used by MCP tools, with an injectable test adapter.
pub(crate) trait MediaApi: Send + Sync {
    fn generate_image(
        &self,
        request: GenerateImageRequest,
    ) -> BoxFuture<'_, Result<GeneratedImageDto, AppError>>;
    fn edit_image(
        &self,
        request: EditImageRequest,
        safe_mode: Option<bool>,
    ) -> BoxFuture<'_, Result<GeneratedImageDto, AppError>>;
    fn video_generate(
        &self,
        request: GenerateVideoRequest,
    ) -> BoxFuture<'_, Result<VideoJobDto, AppError>>;
    fn video_status(&self, job_id: String) -> BoxFuture<'_, Result<VideoStatusDto, AppError>>;

    fn video_status_in<'a>(
        &'a self,
        _directory: &'a Path,
        job_id: String,
    ) -> BoxFuture<'a, Result<VideoStatusDto, AppError>> {
        self.video_status(job_id)
    }
}

/// The direct fal provider commands shared with the media screens.
pub(crate) struct FalMedia<'a> {
    /// Where finished videos are written; `None` without a Tauri runtime.
    pub app: Option<&'a AppHandle>,
}

impl MediaApi for FalMedia<'_> {
    fn generate_image(
        &self,
        request: GenerateImageRequest,
    ) -> BoxFuture<'_, Result<GeneratedImageDto, AppError>> {
        Box::pin(crate::providers::generate_image(request))
    }

    fn edit_image(
        &self,
        request: EditImageRequest,
        safe_mode: Option<bool>,
    ) -> BoxFuture<'_, Result<GeneratedImageDto, AppError>> {
        Box::pin(crate::providers::edit_image_with_safe_mode(
            request, safe_mode,
        ))
    }

    fn video_generate(
        &self,
        request: GenerateVideoRequest,
    ) -> BoxFuture<'_, Result<VideoJobDto, AppError>> {
        Box::pin(crate::providers::video_generate(request))
    }

    fn video_status(&self, job_id: String) -> BoxFuture<'_, Result<VideoStatusDto, AppError>> {
        Box::pin(async move {
            let app = self.app.ok_or_else(|| {
                AppError::new(
                    "media_unavailable",
                    "Clovy cannot follow this video right now.",
                )
            })?;
            crate::providers::video_status(
                app.clone(),
                crate::providers::VideoStatusRequest { job_id },
            )
            .await
        })
    }

    fn video_status_in<'a>(
        &'a self,
        directory: &'a Path,
        job_id: String,
    ) -> BoxFuture<'a, Result<VideoStatusDto, AppError>> {
        Box::pin(crate::providers::fal::video_status_in(directory, job_id))
    }
}

/// External media uses the same provider and schemas, with source handles
/// owned by the relay, approval for cost and upload, and no session artifact.
pub(super) fn external_descriptors(ctx: &ToolCtx<'_>, access: &ExternalCall<'_>) -> Vec<Value> {
    if external_ready(ctx, access) {
        return external_tool_schemas();
    }
    Vec::new()
}

fn external_ready(ctx: &ToolCtx<'_>, access: &ExternalCall<'_>) -> bool {
    ctx.app.is_some() && access.recording_data().is_some() && crate::providers::fal_configured()
}

pub(super) fn external_tool_schemas() -> Vec<Value> {
    let mut tools = descriptors();
    for tool in &mut tools {
        match tool["name"].as_str() {
            Some(GENERATE_IMAGE) => {
                tool["description"] = json!("Generate an image with the configured fal account. Approval is required. The result includes a local artifact and an image block when it fits.");
            }
            Some(EDIT_IMAGE) => {
                tool["description"] = json!("Edit a Clovy image issued to this MCP connection. Use its artifactId as sourcePath. Approval is required to upload the selected image and generate a result.");
                tool["inputSchema"]["properties"]["sourcePath"]["description"] =
                    json!("The artifactId of an image generated through this connection.");
            }
            Some(GENERATE_VIDEO) => {
                tool["description"] = json!("Generate a video with the configured fal account. Approval is required. If the job is still rendering, resume in this connection with only the returned jobId.");
            }
            _ => {}
        }
    }
    tools.retain(|tool| {
        tool["name"] != GENERATE_VIDEO || crate::feature_flags::VIDEO_GENERATION_ENABLED
    });
    tools
}

pub(super) async fn call_external(
    ctx: &ToolCtx<'_>,
    access: &ExternalCall<'_>,
    name: &str,
    arguments: &Value,
) -> Result<Value, AppError> {
    if !external_ready(ctx, access) {
        return Err(media_unavailable());
    }
    let api = FalMedia { app: ctx.app };
    crate::providers::fal::with_external_credential(call_external_with(
        access,
        &api,
        VideoPolling {
            interval: VIDEO_POLL_INTERVAL,
            wait: Some(Duration::from_secs(25)),
        },
        name,
        arguments,
    ))
    .await
}

struct ExternalVideoJob {
    owner: MediaOwner,
    native_id: String,
    created: Instant,
    polling: std::sync::Arc<tokio::sync::Mutex<()>>,
}

type MediaOwner = (Uuid, u64, String);

struct EditableImage {
    owner: MediaOwner,
    digest: [u8; 32],
    created: Instant,
}

static EXTERNAL_JOBS: LazyLock<Mutex<HashMap<Uuid, ExternalVideoJob>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static EDITABLE_IMAGES: LazyLock<Mutex<HashMap<String, EditableImage>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
const JOB_TTL: Duration = Duration::from_secs(60 * 60);
const MAX_EXTERNAL_JOBS: usize = 128;
const MAX_EDITABLE_IMAGES: usize = 128;

fn media_unavailable() -> AppError {
    AppError::new(
        "media_unavailable",
        "The Clovy MCP media store is unavailable.",
    )
}

fn job_unavailable() -> AppError {
    AppError::new(
        "video_job_unavailable",
        "This connection has no such video job.",
    )
}

fn external_owner(access: &ExternalCall<'_>) -> MediaOwner {
    let (client, generation, profile) = access.ownership();
    (client, generation, profile.to_owned())
}

async fn media_directory(access: &ExternalCall<'_>, create: bool) -> Result<PathBuf, AppError> {
    let root = crate::mcp_server::channel::channel_dir(
        access.recording_data().ok_or_else(media_unavailable)?,
    )
    .canonicalize()
    .map_err(io_error)?;
    let (client, generation, profile) = access.ownership();
    let profile_key = format!("{:x}", Sha256::digest(profile.as_bytes()));
    let path = root.join(format!(
        "external-media-{}-{generation}-{profile_key}",
        client.simple()
    ));
    if create {
        access.check_live()?;
        match tokio::fs::create_dir(&path).await {
            Ok(()) => (),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => (),
            Err(error) => return Err(io_error(error)),
        }
    }
    let metadata = tokio::fs::symlink_metadata(&path).await.map_err(io_error)?;
    if !metadata.is_dir() || path.canonicalize().map_err(io_error)? != path {
        return Err(media_unavailable());
    }
    Ok(path)
}

pub(super) async fn call_external_with(
    access: &ExternalCall<'_>,
    api: &dyn MediaApi,
    polling: VideoPolling,
    name: &str,
    arguments: &Value,
) -> Result<Value, AppError> {
    access.check_live()?;
    let descriptor = external_tool_schemas()
        .into_iter()
        .find(|tool| tool["name"] == name)
        .ok_or_else(media_unavailable)?;
    super::argument_validation::validate(&descriptor["inputSchema"], arguments)
        .map_err(|error| AppError::new("agent_tool_arguments_invalid", error))?;
    let owner = external_owner(access);
    match name {
        GENERATE_IMAGE => {
            let prompt = required_string(arguments, "prompt")?.trim();
            if prompt.is_empty() {
                return Err(AppError::new("image_prompt_required", "Enter a prompt."));
            }
            let model = crate::providers::image_model();
            let safe_mode = crate::providers::image_safe_mode();
            let target = json!({"provider":"fal","model":model,"safeMode":safe_mode});
            let approval = access
                .approve(
                    name,
                    "Generate an image",
                    "Send this prompt to fal. The configured fal account may be charged.",
                    arguments,
                    Some(&target),
                )
                .await?;
            let directory = media_directory(access, true).await?;
            approval.check(&owner.2)?;
            check_image_safety_setting(safe_mode)?;
            let generated = wait_for_media(
                access,
                api.generate_image(GenerateImageRequest {
                    prompt: prompt.to_string(),
                    model: Some(model),
                    request_id: None,
                    safe_mode: Some(safe_mode),
                }),
            )
            .await?;
            store_external_image(access, &directory, prompt, generated).await
        }
        EDIT_IMAGE => {
            let id = required_string(arguments, "sourcePath")?;
            let instruction = required_string(arguments, "instruction")?.trim();
            if instruction.is_empty() {
                return Err(AppError::new(
                    "image_prompt_required",
                    "Enter an edit instruction.",
                ));
            }
            let directory = media_directory(access, false).await?;
            let digest = EDITABLE_IMAGES
                .lock()
                .map_err(|_| media_unavailable())?
                .get(id)
                .filter(|image| image.owner == owner)
                .map(|image| image.digest)
                .ok_or_else(image_source_invalid)?;
            let path = external_image_path(&directory, id).await?;
            let (image, mime_type) = read_edit_source(&path).await?;
            let current_digest: [u8; 32] = Sha256::digest(image.as_bytes()).into();
            if current_digest != digest {
                return Err(image_source_invalid());
            }
            let safe_mode = crate::providers::image_safe_mode();
            let target = json!({
                "provider":"fal", "artifactId":id, "mimeType":mime_type, "safeMode":safe_mode,
                "file":crate::filesystem::normalize_path_for_external_use(&path),
            });
            let approval = access.approve(
                name, "Edit a generated image",
                "Upload this generated image and its edit instruction to fal. The configured account may be charged.",
                arguments, Some(&target),
            ).await?;
            approval.check(&owner.2)?;
            check_image_safety_setting(safe_mode)?;
            let generated = wait_for_media(
                access,
                api.edit_image(
                    EditImageRequest {
                        image,
                        prompt: instruction.to_string(),
                        request_id: None,
                        mime_type: Some(mime_type.to_string()),
                        model: None,
                    },
                    Some(safe_mode),
                ),
            )
            .await?;
            store_external_image(access, &directory, instruction, generated).await
        }
        GENERATE_VIDEO => external_video(access, api, polling, arguments, owner).await,
        _ => Err(AppError::new("mcp_tool_unknown", "Unknown media tool.")),
    }
}

fn check_image_safety_setting(approved: bool) -> Result<(), AppError> {
    if crate::providers::image_safe_mode() != approved {
        return Err(AppError::new(
            "mcp_external_scope_changed",
            "The image safety setting changed while awaiting approval.",
        ));
    }
    Ok(())
}

async fn wait_for_media<T>(
    access: &ExternalCall<'_>,
    operation: impl Future<Output = Result<T, AppError>>,
) -> Result<T, AppError> {
    access.check_live()?;
    tokio::select! {
        biased;
        _ = access.cancelled() => {
            let mut error = AppError::new(
                "mcp_external_media_interrupted",
                "The media request ended after dispatch. Check the provider before issuing another generation.",
            );
            error.details = Some(json!({"delivery":"unknown","effect":"unknown","retrySafe":false}));
            Err(error)
        }
        outcome = operation => outcome,
    }
}

async fn external_image_path(directory: &Path, id: &str) -> Result<PathBuf, AppError> {
    let valid = id
        .strip_prefix("generated-image-")
        .and_then(|name| name.split_once('.'))
        .is_some_and(|(uuid, extension)| {
            uuid.len() == 32
                && uuid.bytes().all(|byte| byte.is_ascii_hexdigit())
                && matches!(extension, "png" | "jpg" | "webp" | "gif")
        });
    if !valid {
        return Err(image_source_invalid());
    }
    let path = directory.join(id);
    let metadata = tokio::fs::symlink_metadata(&path)
        .await
        .map_err(|_| image_source_invalid())?;
    if !metadata.is_file()
        || metadata.len() > MAX_EDIT_SOURCE_BYTES as u64
        || path.canonicalize().map_err(|_| image_source_invalid())? != path
    {
        return Err(image_source_invalid());
    }
    Ok(path)
}

fn image_source_invalid() -> AppError {
    AppError::new(
        "image_source_invalid",
        "Select an unchanged generated image from this MCP connection.",
    )
}

async fn store_external_image(
    access: &ExternalCall<'_>,
    directory: &Path,
    prompt: &str,
    generated: GeneratedImageDto,
) -> Result<Value, AppError> {
    use tokio::io::AsyncWriteExt;
    let bytes = BASE64
        .decode(&generated.image_base64)
        .map_err(|error| AppError::new("image_response_invalid", error.to_string()))?;
    let filename = image_filename(&generated.mime_type);
    let path = directory.join(&filename);
    access.check_live()?;
    let mut file = tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .await
        .map_err(io_error)?;
    if let Err(error) = file.write_all(&bytes).await {
        drop(file);
        let _ = tokio::fs::remove_file(&path).await;
        return Err(io_error(error));
    }
    let mut images = EDITABLE_IMAGES.lock().map_err(|_| media_unavailable())?;
    if images.len() >= MAX_EDITABLE_IMAGES {
        if let Some(key) = images
            .iter()
            .min_by_key(|(_, image)| image.created)
            .map(|(key, _)| key.clone())
        {
            images.remove(&key);
        }
    }
    images.insert(
        filename.clone(),
        EditableImage {
            owner: external_owner(access),
            digest: Sha256::digest(generated.image_base64.as_bytes()).into(),
            created: Instant::now(),
        },
    );
    Ok(image_blocks(&path, filename, prompt, generated))
}

async fn external_video(
    access: &ExternalCall<'_>,
    api: &dyn MediaApi,
    polling: VideoPolling,
    arguments: &Value,
    owner: MediaOwner,
) -> Result<Value, AppError> {
    let call = parse_video_call(arguments, None)?;
    let (job, native_id, directory, _permit) = match call {
        VideoCall::Create(mut request) => {
            let model = crate::providers::video_model();
            request.model = Some(model.clone());
            let target = json!({"provider":"fal","model":model});
            let approval = access.approve(
                GENERATE_VIDEO, "Generate a video",
                "Send this prompt and generation options to fal. The configured account may be charged.",
                arguments, Some(&target),
            ).await?;
            let directory = media_directory(access, true).await?;
            {
                let mut jobs = EXTERNAL_JOBS.lock().map_err(|_| media_unavailable())?;
                jobs.retain(|_, job| job.created.elapsed() < JOB_TTL);
                if jobs.len() >= MAX_EXTERNAL_JOBS {
                    return Err(media_unavailable());
                }
            }
            let job = Uuid::new_v4();
            request.request_id = Some(job.to_string());
            approval.check(&owner.2)?;
            let native_id = wait_for_media(access, api.video_generate(request))
                .await?
                .job_id;
            let polling = std::sync::Arc::new(tokio::sync::Mutex::new(()));
            let permit = polling.clone().lock_owned().await;
            EXTERNAL_JOBS
                .lock()
                .map_err(|_| media_unavailable())?
                .insert(
                    job,
                    ExternalVideoJob {
                        owner,
                        native_id: native_id.clone(),
                        created: Instant::now(),
                        polling,
                    },
                );
            (job, native_id, directory, permit)
        }
        VideoCall::Resume { job_id } => {
            if arguments.as_object().is_none_or(|args| args.len() != 1) {
                return Err(AppError::new(
                    "agent_tool_arguments_invalid",
                    "Resume a video with only its jobId.",
                ));
            }
            let job = Uuid::parse_str(&job_id).map_err(|_| job_unavailable())?;
            let (native_id, polling) = EXTERNAL_JOBS
                .lock()
                .map_err(|_| media_unavailable())?
                .get(&job)
                .filter(|existing| existing.owner == owner && existing.created.elapsed() < JOB_TTL)
                .map(|existing| (existing.native_id.clone(), existing.polling.clone()))
                .ok_or_else(job_unavailable)?;
            let permit = polling.try_lock_owned().map_err(|_| {
                AppError::new("video_job_busy", "This video job is already being checked.")
            })?;
            access.approve(
                GENERATE_VIDEO, "Finish the requested video",
                "Check the same fal job and save the finished video when available. No new video is submitted.",
                arguments, None,
            ).await?.check(&owner.2)?;
            let directory = media_directory(access, false).await?;
            (job, native_id, directory, permit)
        }
    };
    let (_, result) = wait_for_media(
        access,
        follow_video_call(
            VideoCall::Resume { job_id: native_id },
            |request| api.video_generate(request),
            |id| api.video_status_in(&directory, id),
            std::future::pending(),
            polling,
        ),
    )
    .await?;
    match result {
        VideoJobOutcome::Completed {
            path,
            mime_type,
            model,
        } => {
            let path = PathBuf::from(path).canonicalize().map_err(io_error)?;
            if path.parent() != Some(directory.as_path()) {
                return Err(media_unavailable());
            }
            EXTERNAL_JOBS
                .lock()
                .map_err(|_| media_unavailable())?
                .remove(&job);
            Ok(json!({
                "mediaType":"video", "status":"completed", "mimeType":mime_type, "model":model,
                "path":crate::filesystem::normalize_path_for_external_use(&path)
            }))
        }
        VideoJobOutcome::StillProcessing => Ok(json!({"status":"processing","jobId":job})),
        VideoJobOutcome::WaitingForProviderKey { reason } => Ok(json!({
            "status":"waitingForProviderKey","provider":"fal","jobId":job,"reason":reason,
            "message":waiting_for_provider_key_message(&reason)
        })),
    }
}

/// One media call for the bound session. `polling` bounds how long a video
/// call follows its job.
pub(crate) async fn call(
    pool: &SqlitePool,
    binding: &TurnBinding,
    api: &dyn MediaApi,
    polling: VideoPolling,
    name: &str,
    arguments: &Value,
) -> Result<Value, AppError> {
    // A routine session runs only the tools its routine enables, as in the
    // native engine.
    if !super::routine_tool_allowed(pool, binding, name).await? {
        return Err(AppError::new(
            "routine_tool_not_enabled",
            "This tool is not enabled for the routine.",
        ));
    }
    let workspace = session_workspace(pool, &binding.session_id).await?;
    let target = ArtifactTarget {
        pool,
        workspace: &workspace,
        session_id: &binding.session_id,
        run_id: &binding.run_id,
    };
    match name {
        GENERATE_IMAGE => {
            let prompt = required_string(arguments, "prompt")?.trim();
            let generated = api
                .generate_image(GenerateImageRequest {
                    prompt: prompt.to_string(),
                    model: None,
                    request_id: None,
                    safe_mode: None,
                })
                .await?;
            mcp_image_result(&target, prompt, generated).await
        }
        EDIT_IMAGE => {
            let requested = required_string(arguments, "sourcePath")?;
            let source = resolve_requested_path(&workspace, requested, true)?;
            if binding.safety_mode == AgentSafetyMode::Sandboxed
                && !source.starts_with(workspace.canonicalize().map_err(io_error)?)
            {
                return Err(AppError::new(
                    "agent_path_denied",
                    "Path is outside this session's workspace.",
                ));
            }
            let instruction = required_string(arguments, "instruction")?.trim();
            let (image, mime_type) = read_edit_source(&source).await?;
            let generated = api
                .edit_image(
                    EditImageRequest {
                        image,
                        prompt: instruction.to_string(),
                        request_id: None,
                        mime_type: Some(mime_type.to_string()),
                        model: None,
                    },
                    None,
                )
                .await?;
            mcp_image_result(&target, instruction, generated).await
        }
        GENERATE_VIDEO => {
            let call = parse_video_call(arguments, None)?;
            let prompt = call.prompt();
            let (job_id, outcome) = follow_video_call(
                call,
                |request| api.video_generate(request),
                |job_id| api.video_status(job_id),
                // The CLI's call outlives a cancelled turn (the server
                // answers in a detached task), so the run ending stops it.
                run_ended(pool, &binding.run_id, polling.interval),
                polling,
            )
            .await?;
            finish_video_call(&target, job_id, outcome, prompt, None).await
        }
        other => Err(AppError::new(
            "mcp_tool_unknown",
            format!("Unknown tool `{other}`."),
        )),
    }
}

/// Completes once the run has ended (completed, cancelled, failed, or
/// interrupted), checked every `interval`; a run that cannot be read counts
/// as ended.
async fn run_ended(pool: &SqlitePool, run_id: &str, interval: Duration) {
    let repository = AgentRepository::new(pool.clone());
    loop {
        tokio::time::sleep(interval).await;
        match repository.get_run(run_id).await {
            Ok(run)
                if !matches!(
                    run.status.as_str(),
                    "completed" | "cancelled" | "failed" | "interrupted"
                ) => {}
            _ => return,
        }
    }
}

/// The bound session's workspace (`agent_sessions.workspace_path`).
async fn session_workspace(pool: &SqlitePool, session_id: &str) -> Result<PathBuf, AppError> {
    let session = AgentRepository::new(pool.clone())
        .get_session(session_id)
        .await
        .map_err(|_| {
            AppError::new(
                "agent_session_not_found",
                "The chat session of this turn was not found.",
            )
        })?;
    session
        .workspace_path
        .filter(|path| !path.trim().is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| {
            AppError::new(
                "agent_workspace_unavailable",
                "This chat session has no workspace for media.",
            )
        })
}

// Shared with the native tools (`agent_runtime/tools.rs`).

/// Where a tool's artifact goes: a session workspace and its rows.
pub(crate) struct ArtifactTarget<'a> {
    pub pool: &'a SqlitePool,
    pub workspace: &'a Path,
    pub session_id: &'a str,
    pub run_id: &'a str,
}

/// One `generate_video` call: start a new video, or continue a job an
/// earlier call left processing or waiting for the user's fal key. A
/// continuation carries only the job id, so nothing in it can start another
/// generation.
pub(crate) enum VideoCall {
    Create(GenerateVideoRequest),
    Resume { job_id: String },
}

impl VideoCall {
    pub(crate) fn prompt(&self) -> Option<String> {
        match self {
            VideoCall::Create(request) => Some(request.prompt.clone()),
            VideoCall::Resume { .. } => None,
        }
    }
}

pub(crate) fn parse_video_call(
    arguments: &Value,
    call_id: Option<String>,
) -> Result<VideoCall, AppError> {
    if let Some(job_id) = optional_nonempty_string(arguments, "jobId")? {
        return Ok(VideoCall::Resume { job_id });
    }
    Ok(VideoCall::Create(GenerateVideoRequest {
        prompt: required_string(arguments, "prompt")?.trim().to_string(),
        model: None,
        request_id: call_id,
        duration: optional_nonempty_string(arguments, "duration")?,
        resolution: None,
        aspect_ratio: optional_nonempty_string(arguments, "aspectRatio")?,
        audio: arguments.get("audio").and_then(Value::as_bool),
    }))
}

/// Where following one video job ended, short of an error.
#[derive(Debug, PartialEq)]
pub(crate) enum VideoJobOutcome {
    Completed {
        path: String,
        mime_type: String,
        model: String,
    },
    /// The job stays on the server, but this desktop cannot reach it until the
    /// user saves a valid key from the fal account that started it.
    WaitingForProviderKey { reason: String },
    /// The job was still rendering when the call's wait ran out
    /// ([`VideoPolling::wait`]); the next call continues it by id.
    StillProcessing,
}

/// How a call follows its job.
#[derive(Clone, Copy, Debug)]
pub(crate) struct VideoPolling {
    pub interval: Duration,
    /// `None` follows the job until it ends; `Some` answers
    /// `StillProcessing` after this long.
    pub wait: Option<Duration>,
}

/// Runs one call. A new video sends its single submit and follows the job it
/// gets back; a continuation follows the named job with no submit. A failed
/// submit is an error, never a job. Returns the job id with the outcome so a
/// waiting result can name it. `cancelled` ends the call with
/// `agent_tool_cancelled` when it completes.
pub(crate) async fn follow_video_call<S, SFut, P, PFut, C>(
    call: VideoCall,
    submit: S,
    mut poll: P,
    cancelled: C,
    polling: VideoPolling,
) -> Result<(String, VideoJobOutcome), AppError>
where
    S: FnOnce(GenerateVideoRequest) -> SFut,
    SFut: Future<Output = Result<VideoJobDto, AppError>>,
    P: FnMut(String) -> PFut,
    PFut: Future<Output = Result<VideoStatusDto, AppError>>,
    C: Future<Output = ()>,
{
    let job_id = match call {
        VideoCall::Create(request) => submit(request).await?.job_id,
        VideoCall::Resume { job_id } => job_id,
    };
    tokio::pin!(cancelled);
    let deadline = polling.wait.map(|wait| tokio::time::Instant::now() + wait);
    let wait_over = async {
        match deadline {
            Some(deadline) => tokio::time::sleep_until(deadline).await,
            None => std::future::pending().await,
        }
    };
    tokio::pin!(wait_over);
    loop {
        let result = tokio::select! {
            result = poll(job_id.clone()) => result,
            _ = &mut cancelled => return Err(video_cancelled()),
            _ = &mut wait_over => return Ok((job_id, VideoJobOutcome::StillProcessing)),
        };
        match result {
            Ok(VideoStatusDto::Processing { .. }) => {
                tokio::select! {
                    _ = tokio::time::sleep(polling.interval) => {}
                    _ = &mut cancelled => return Err(video_cancelled()),
                    _ = &mut wait_over => return Ok((job_id, VideoJobOutcome::StillProcessing)),
                }
            }
            Ok(VideoStatusDto::Failed { reason }) => {
                return Err(AppError::new("video_generation_failed", reason));
            }
            Ok(VideoStatusDto::Completed {
                path,
                mime_type,
                model,
                ..
            }) => {
                let outcome = VideoJobOutcome::Completed {
                    path,
                    mime_type,
                    model,
                };
                return Ok((job_id, outcome));
            }
            Err(error) if video_poll_waits_for_provider_key(&error) => {
                let outcome = VideoJobOutcome::WaitingForProviderKey { reason: error.code };
                return Ok((job_id, outcome));
            }
            Err(error) => return Err(error),
        }
    }
}

/// The tool result for a followed job: the finished video copied into the
/// session's artifacts, or a normal (non-error) result naming the job to
/// continue.
pub(crate) async fn finish_video_call(
    target: &ArtifactTarget<'_>,
    job_id: String,
    outcome: VideoJobOutcome,
    prompt: Option<String>,
    request_id: Option<String>,
) -> Result<Value, AppError> {
    let (path, mime_type, model) = match outcome {
        VideoJobOutcome::Completed {
            path,
            mime_type,
            model,
        } => (path, mime_type, model),
        VideoJobOutcome::WaitingForProviderKey { reason } => {
            // A normal result, not a failure: the run goes on, and the model
            // keeps the job id to continue this same job once the key is fixed.
            let message = waiting_for_provider_key_message(&reason);
            return Ok(json!({
                "status": "waitingForProviderKey",
                "provider": crate::providers::BuiltinProvider::Fal.id(),
                "jobId": job_id,
                "reason": reason,
                "message": message,
            }));
        }
        VideoJobOutcome::StillProcessing => {
            return Ok(json!({
                "status": "processing",
                "jobId": job_id,
                "message": "The video is still rendering. Call generate_video again with only this jobId to keep waiting for it; do not start a new video.",
            }));
        }
    };
    let destination = artifact_path(
        target.workspace,
        &format!("generated-video-{}.mp4", uuid::Uuid::new_v4().simple()),
    )
    .await?;
    tokio::fs::copy(PathBuf::from(path), &destination)
        .await
        .map_err(io_error)?;
    record_artifact(target, &destination, "created", None, Some(&mime_type)).await?;
    Ok(json!({
        "mediaType": "video",
        "path": crate::filesystem::normalize_path_for_external_use(&destination),
        "mimeType": mime_type,
        "model": model,
        "prompt": prompt,
        "requestId": request_id,
        "name": destination.file_name().map(|name| name.to_string_lossy().into_owned())
    }))
}

pub(crate) fn video_cancelled() -> AppError {
    AppError::new("agent_tool_cancelled", "Video generation was cancelled.")
}

/// Poll failures that preserve the fal job until the user saves a valid key
/// from the account that started it (including a rotated key). An unreadable
/// local key also preserves the job without sending a request.
fn video_poll_waits_for_provider_key(error: &AppError) -> bool {
    matches!(
        error.code.as_str(),
        "fal_api_key_required"
            | "fal_api_key_rejected"
            | "fal_request_inaccessible"
            | "provider_api_key_unavailable"
    )
}

fn waiting_for_provider_key_message(reason: &str) -> &'static str {
    if reason == "provider_api_key_unavailable" {
        "Clovy could not read your saved fal key, so it could not check on this video. Save a valid key from the fal account that started it in Settings, then ask to continue this video."
    } else {
        "fal could not continue this video with the key Clovy has now. Save a valid key from the fal account that started it in Settings, then ask to continue this video."
    }
}

/// Writes a generated image into the session's artifacts: its path and
/// artifact id.
async fn store_generated_image(
    target: &ArtifactTarget<'_>,
    generated: &GeneratedImageDto,
) -> Result<(PathBuf, String), AppError> {
    let bytes = BASE64
        .decode(&generated.image_base64)
        .map_err(|error| AppError::new("image_response_invalid", error.to_string()))?;
    let path = artifact_path(target.workspace, &image_filename(&generated.mime_type)).await?;
    tokio::fs::write(&path, &bytes).await.map_err(io_error)?;
    let artifact_id =
        record_artifact(target, &path, "created", None, Some(&generated.mime_type)).await?;
    Ok((path, artifact_id))
}

fn image_filename(mime_type: &str) -> String {
    let extension = match mime_type {
        "image/jpeg" => "jpg",
        "image/webp" => "webp",
        "image/gif" => "gif",
        _ => "png",
    };
    format!("generated-image-{}.{extension}", Uuid::new_v4().simple())
}

/// The MCP image result: a text block with the artifact (id, path,
/// metadata; no base64), then the image as an MCP `image` block when its
/// base64 fits the content-block limit (`MAX_IMAGE_BASE64`); a larger image
/// stays in the artifact and the text says so.
async fn mcp_image_result(
    target: &ArtifactTarget<'_>,
    prompt: &str,
    generated: GeneratedImageDto,
) -> Result<Value, AppError> {
    let (path, artifact_id) = store_generated_image(target, &generated).await?;
    Ok(image_blocks(&path, artifact_id, prompt, generated))
}

fn image_blocks(
    path: &Path,
    artifact_id: String,
    prompt: &str,
    generated: GeneratedImageDto,
) -> Value {
    let inline = generated.image_base64.len() <= MAX_IMAGE_BASE64;
    let mut text = json!({
        "mediaType": "image",
        "artifactId": artifact_id,
        "path": crate::filesystem::normalize_path_for_external_use(path),
        "name": path.file_name().map(|name| name.to_string_lossy().into_owned()),
        "mimeType": generated.mime_type,
        "model": generated.model,
        "prompt": prompt,
        "imageAttached": inline,
    });
    if !inline {
        text["note"] = json!("The image is too large to attach; read it from path.");
    }
    let mut blocks = vec![protocol::text_block(text.to_string())];
    if inline {
        blocks.push(protocol::image_block(
            generated.image_base64,
            generated.mime_type,
        ));
    }
    protocol::content_value(blocks)
}

/// Reads a source image for `edit_image`: base64 and media type, refusing
/// files over 20 MB and anything but PNG, JPEG, WebP, or GIF.
pub(crate) async fn read_edit_source(source: &Path) -> Result<(String, &'static str), AppError> {
    use tokio::io::AsyncReadExt;
    let mut bytes = Vec::new();
    tokio::fs::File::open(source)
        .await
        .map_err(io_error)?
        .take(MAX_EDIT_SOURCE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .await
        .map_err(io_error)?;
    if bytes.len() > MAX_EDIT_SOURCE_BYTES {
        return Err(AppError::new(
            "image_source_too_large",
            "The source image exceeds the 20 MB edit limit.",
        ));
    }
    let mime_type = image_mime_type(source).ok_or_else(|| {
        AppError::new(
            "image_source_invalid",
            "The source must be a PNG, JPEG, WebP, or GIF image.",
        )
    })?;
    Ok((BASE64.encode(bytes), mime_type))
}

fn image_mime_type(path: &Path) -> Option<&'static str> {
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("png") => Some("image/png"),
        Some("jpg" | "jpeg") => Some("image/jpeg"),
        Some("webp") => Some("image/webp"),
        Some("gif") => Some("image/gif"),
        _ => None,
    }
}

/// `artifacts/<file_name>` in the workspace (canonical), with the folder
/// created.
async fn artifact_path(workspace: &Path, file_name: &str) -> Result<PathBuf, AppError> {
    let folder = workspace.join("artifacts");
    tokio::fs::create_dir_all(&folder).await.map_err(io_error)?;
    Ok(folder.canonicalize().map_err(io_error)?.join(file_name))
}

/// Records a file a tool created as one of the session's artifacts; the
/// artifact id.
pub(crate) async fn record_artifact(
    target: &ArtifactTarget<'_>,
    path: &Path,
    action: &str,
    original: Option<&Path>,
    mime_type: Option<&str>,
) -> Result<String, AppError> {
    let metadata = tokio::fs::metadata(path).await.map_err(io_error)?;
    let id = uuid::Uuid::new_v4().to_string();
    sqlx::query::query("INSERT INTO agent_artifacts(id, session_id, run_id, provenance, action, path, original_path, mime_type, size_bytes, available, created_at) VALUES (?, ?, ?, 'tool', ?, ?, ?, ?, ?, 1, ?)")
        .bind(&id).bind(target.session_id).bind(target.run_id)
        .bind(action).bind(path.to_string_lossy().as_ref()).bind(original.map(|value| value.to_string_lossy().into_owned()))
        .bind(mime_type).bind(metadata.len() as i64).bind(crate::db::repositories::timestamp()).execute(target.pool).await?;
    Ok(id)
}

/// A requested path made absolute against the workspace and canonical (a
/// path that does not exist yet resolves through its nearest existing
/// ancestor). Containment is the caller's check.
pub(crate) fn resolve_requested_path(
    workspace: &Path,
    requested: &str,
    must_exist: bool,
) -> Result<PathBuf, AppError> {
    let requested = Path::new(requested);
    let joined = if requested.is_absolute() {
        requested.to_path_buf()
    } else {
        workspace.join(requested)
    };
    if must_exist || joined.exists() {
        return joined.canonicalize().map_err(io_error);
    }
    let mut existing = joined.as_path();
    while !existing.exists() {
        existing = existing
            .parent()
            .ok_or_else(|| AppError::new("agent_path_invalid", "Path has no existing ancestor."))?;
    }
    let canonical_existing = existing.canonicalize().map_err(io_error)?;
    let suffix = joined
        .strip_prefix(existing)
        .map_err(|_| AppError::new("agent_path_invalid", "Path could not be resolved."))?;
    Ok(canonical_existing.join(suffix))
}

pub(crate) fn required_string<'a>(value: &'a Value, key: &str) -> Result<&'a str, AppError> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            AppError::new(
                "agent_tool_arguments_invalid",
                format!("{key} is required."),
            )
        })
}

/// An optional argument that, when present, is a non-empty string.
fn optional_nonempty_string(arguments: &Value, key: &str) -> Result<Option<String>, AppError> {
    let Some(value) = arguments.get(key) else {
        return Ok(None);
    };
    let invalid = || {
        AppError::new(
            "invalid_arguments",
            format!("{key} must be a non-empty string."),
        )
    };
    let value = value.as_str().ok_or_else(invalid)?.trim();
    if value.is_empty() {
        return Err(invalid());
    }
    Ok(Some(value.to_string()))
}

pub(crate) fn io_error(error: std::io::Error) -> AppError {
    AppError::new("agent_tool_io_failed", error.to_string())
}
