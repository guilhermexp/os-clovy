//! User-installed, pinned speech models. Transcription itself never downloads files.
use crate::{domain::types::AppError, providers::transcription::LocalModelId};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
};

#[cfg(test)]
tokio::task_local! {
    static TEST_BEFORE_FLUSH: (std::sync::Arc<tokio::sync::Notify>, std::sync::Arc<tokio::sync::Notify>);
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ModelState {
    Absent,
    Downloading { received: u64, total: u64 },
    Ready { bytes: u64 },
    Failed { reason: String },
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelFile {
    pub path: String,
    pub bytes: u64,
    pub sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<ModelFileSource>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelFileSource {
    pub repository: String,
    pub revision: String,
    pub path: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Manifest {
    pub repository: String,
    pub revision: String,
    pub total_bytes: u64,
    pub files: Vec<ModelFile>,
}

impl Manifest {
    fn validate(&self) -> bool {
        fn pinned(repository: &str, revision: &str) -> bool {
            repository
                .split_once('/')
                .is_some_and(|(owner, repo)| safe_component(owner) && safe_component(repo))
                && valid_hex(revision, 40)
        }
        let mut paths = std::collections::HashSet::new();
        !self.files.is_empty()
            && pinned(&self.repository, &self.revision)
            && self.files.iter().all(|file| {
                safe_file(&file.path)
                    && paths.insert(&file.path)
                    && file.bytes > 0
                    && valid_hex(&file.sha256, 64)
                    && file.source.as_ref().map_or(true, |source| {
                        pinned(&source.repository, &source.revision) && safe_file(&source.path)
                    })
            })
            && self
                .files
                .iter()
                .try_fold(0u64, |total, file| total.checked_add(file.bytes))
                == Some(self.total_bytes)
    }

    fn base_url(&self) -> String {
        format!(
            "https://huggingface.co/{}/resolve/{}",
            self.repository, self.revision
        )
    }
}

fn valid_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn safe_component(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
}

fn safe_file(value: &str) -> bool {
    value.split('/').all(safe_component) && !value.ends_with(".part") && value != "installed.json"
}

// A wakeable cancellation token backed by Tokio watch; no additional crate.
struct CancellationToken(tokio::sync::watch::Sender<bool>);
impl CancellationToken {
    fn new() -> Self {
        Self(tokio::sync::watch::channel(false).0)
    }
    fn cancel(&self) {
        self.0.send_replace(true);
    }
    fn is_cancelled(&self) -> bool {
        *self.0.borrow()
    }
    async fn cancelled(&self) {
        let mut receiver = self.0.subscribe();
        let _ = receiver.wait_for(|cancelled| *cancelled).await;
    }
}

pub struct TranscriptionModel {
    dir: PathBuf,
    manifest: Manifest,
    state: Mutex<ModelState>,
    installation: Mutex<Option<Installation>>,
    operation: tokio::sync::Mutex<()>,
    use_state: Mutex<ModelUseState>,
}

type InstallResult = Result<(), String>;

struct Installation {
    cancellation: Arc<CancellationToken>,
    result: tokio::sync::watch::Receiver<Option<InstallResult>>,
}

struct InstallOwner<'a>(&'a Mutex<Option<Installation>>);

impl Drop for InstallOwner<'_> {
    fn drop(&mut self) {
        *lock(self.0) = None;
    }
}

#[derive(Default)]
struct ModelUseState {
    readers: usize,
    removing: bool,
}

pub struct ModelUse(Arc<TranscriptionModel>);
impl Drop for ModelUse {
    fn drop(&mut self) {
        lock(&self.0.use_state).readers -= 1;
    }
}

pub struct RemovalGuard<'a>(&'a TranscriptionModel);
impl Drop for RemovalGuard<'_> {
    fn drop(&mut self) {
        lock(&self.0.use_state).removing = false;
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl TranscriptionModel {
    pub fn new(dir: PathBuf, manifest: Manifest) -> Self {
        // The receipt check is metadata-only, so persisted Local selection is usable
        // before the startup worker runs. Missing/stale receipts still need its hash check.
        let state = if marker_matches(&dir, &manifest) {
            ModelState::Ready {
                bytes: manifest.total_bytes,
            }
        } else {
            ModelState::Absent
        };
        Self {
            dir,
            manifest,
            state: Mutex::new(state),
            installation: Mutex::new(None),
            operation: tokio::sync::Mutex::new(()),
            use_state: Mutex::new(ModelUseState::default()),
        }
    }
    /// Startup registers immediately; disk verification runs away from the window thread.
    pub async fn refresh_from_disk(
        &self,
        progress: impl Fn(ModelState) + Send + Sync,
        on_ready: impl FnOnce() + Send,
    ) {
        let _operation = self.operation.lock().await;
        if lock(&self.use_state).removing {
            return;
        }
        let dir = self.dir.clone();
        let manifest = self.manifest.clone();
        let ready = tokio::task::spawn_blocking(move || installed(&dir, &manifest))
            .await
            .unwrap_or(false);
        if lock(&self.use_state).removing {
            return;
        }
        self.publish(
            if ready {
                ModelState::Ready {
                    bytes: self.manifest.total_bytes,
                }
            } else {
                ModelState::Absent
            },
            &progress,
        );
        if ready {
            on_ready();
        }
    }
    pub fn directory(&self) -> &std::path::Path {
        &self.dir
    }
    pub fn state(&self) -> ModelState {
        if lock(&self.use_state).removing {
            return ModelState::Absent;
        }
        let state = lock(&self.state).clone();
        if matches!(state, ModelState::Ready { .. }) && !marker_matches(&self.dir, &self.manifest) {
            return ModelState::Absent;
        }
        state
    }
    fn acquire(self: &Arc<Self>) -> Option<Arc<ModelUse>> {
        let mut state = lock(&self.use_state);
        if state.removing
            || !matches!(*lock(&self.state), ModelState::Ready { .. })
            || !marker_matches(&self.dir, &self.manifest)
        {
            return None;
        }
        state.readers += 1;
        Some(Arc::new(ModelUse(self.clone())))
    }
    pub fn begin_removal(&self) -> Result<RemovalGuard<'_>, AppError> {
        let mut state = lock(&self.use_state);
        if state.readers > 0 || state.removing {
            return Err(AppError::new(
                "local_model_in_use",
                "This model is in use. Try again after transcription finishes.",
            ));
        }
        state.removing = true;
        drop(state);
        self.cancel();
        Ok(RemovalGuard(self))
    }
    pub fn cancel(&self) {
        if let Some(installation) = lock(&self.installation).as_ref() {
            installation.cancellation.cancel();
        }
    }
    fn publish(&self, state: ModelState, progress: &impl Fn(ModelState)) {
        *lock(&self.state) = state.clone();
        progress(state);
    }
    pub async fn install(
        &self,
        base: &str,
        progress: impl Fn(ModelState) + Send + Sync,
    ) -> Result<(), String> {
        let (producer, mut result) = {
            let mut active = lock(&self.installation);
            if let Some(existing) = active.as_ref() {
                (None, existing.result.clone())
            } else {
                let token = Arc::new(CancellationToken::new());
                let (sender, receiver) = tokio::sync::watch::channel(None);
                *active = Some(Installation {
                    cancellation: Arc::clone(&token),
                    result: receiver.clone(),
                });
                (
                    Some((InstallOwner(&self.installation), token, sender)),
                    receiver,
                )
            }
        };
        let Some((_owner, token, sender)) = producer else {
            return result
                .wait_for(Option::is_some)
                .await
                .map_err(|_| "Installation stopped before completion.".to_string())?
                .as_ref()
                .expect("completed installation")
                .clone();
        };
        let _operation = self.operation.lock().await;
        if lock(&self.use_state).removing {
            token.cancel();
        }
        let result = self.install_owned(base, &progress, &token).await;
        sender.send_replace(Some(result.clone()));
        result
    }

    async fn install_owned(
        &self,
        base: &str,
        progress: &(impl Fn(ModelState) + Send + Sync),
        token: &CancellationToken,
    ) -> InstallResult {
        if token.is_cancelled() {
            return Ok(());
        }
        if matches!(self.state(), ModelState::Ready { .. }) {
            return Ok(());
        }
        self.publish(
            ModelState::Downloading {
                received: 0,
                total: self.manifest.total_bytes,
            },
            progress,
        );
        // Only network waits are interrupted. Dropping Tokio filesystem operations
        // can leave blocking writes running after the operation lock is released.
        let result = self.download(base, progress, token).await;
        let result = if token.is_cancelled() {
            Ok(false)
        } else {
            result.map(|()| true)
        };
        let final_state = match &result {
            Ok(true) if !token.is_cancelled() => ModelState::Ready {
                bytes: self.manifest.total_bytes,
            },
            Ok(_) => ModelState::Absent,
            Err(reason) => ModelState::Failed {
                reason: reason.clone(),
            },
        };
        if !matches!(final_state, ModelState::Ready { .. }) {
            if let Ok(path) = managed_path(&self.dir, "installed.json") {
                let _ = tokio::fs::remove_file(path).await;
            }
        }
        self.publish(final_state, progress);
        result.map(|_| ())
    }
    async fn download(
        &self,
        base: &str,
        progress: &impl Fn(ModelState),
        token: &CancellationToken,
    ) -> Result<(), String> {
        use futures_util::StreamExt;
        use sha2::{Digest, Sha256};
        use tokio::io::AsyncWriteExt;
        for file in &self.manifest.files {
            managed_path(&self.dir, &file.path)?;
            managed_path(&self.dir, &format!("{}.part", file.path))?;
        }
        managed_path(&self.dir, "installed.json")?;
        tokio::fs::create_dir_all(&self.dir)
            .await
            .map_err(|e| e.to_string())?;
        let client = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(20))
            .read_timeout(std::time::Duration::from_secs(60))
            .build()
            .map_err(|e| e.to_string())?;
        let mut completed = 0;
        for file in &self.manifest.files {
            if token.is_cancelled() {
                return Ok(());
            }
            let destination = managed_path(&self.dir, &file.path)?;
            if verified_during_install(&destination, file, token).await? {
                completed += file.bytes;
                self.publish(
                    ModelState::Downloading {
                        received: completed,
                        total: self.manifest.total_bytes,
                    },
                    progress,
                );
                continue;
            }
            let parent = destination.parent().ok_or("invalid model path")?;
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| e.to_string())?;
            let part_path = format!("{}.part", file.path);
            let part = managed_path(&self.dir, &part_path)?;
            let mut offset = tokio::fs::metadata(&part)
                .await
                .map(|m| m.len())
                .unwrap_or(0);
            // Cancellation can arrive after the final durable progress event,
            // before checksum verification/rename. Reuse only a verified part.
            if offset == file.bytes && verified_during_install(&part, file, token).await? {
                tokio::fs::rename(&part, &destination)
                    .await
                    .map_err(|e| e.to_string())?;
                completed += file.bytes;
                self.publish(
                    ModelState::Downloading {
                        received: completed,
                        total: self.manifest.total_bytes,
                    },
                    progress,
                );
                continue;
            }
            if offset >= file.bytes {
                offset = 0;
            }
            let url = file.source.as_ref().map_or_else(
                || format!("{}/{}", base.trim_end_matches('/'), file.path),
                |source| {
                    format!(
                        "https://huggingface.co/{}/resolve/{}/{}",
                        source.repository, source.revision, source.path
                    )
                },
            );
            let mut request = client
                .get(url)
                .header(reqwest::header::ACCEPT_ENCODING, "identity");
            if offset > 0 {
                request = request.header(reqwest::header::RANGE, format!("bytes={offset}-"));
            }
            let response = tokio::select! {
                biased;
                _ = token.cancelled() => return Ok(()),
                response = request.send() => response,
            }
            .and_then(reqwest::Response::error_for_status)
            .map_err(|_| format!("network: {}", file.path))?;
            if response.status() == reqwest::StatusCode::PARTIAL_CONTENT {
                let expected = format!("bytes {offset}-{}/{}", file.bytes - 1, file.bytes);
                if response
                    .headers()
                    .get(reqwest::header::CONTENT_RANGE)
                    .and_then(|v| v.to_str().ok())
                    != Some(expected.as_str())
                {
                    return Err(format!("range: {}", file.path));
                }
            } else {
                offset = 0;
            }
            let mut hash = Sha256::new();
            if offset > 0 {
                hash = hash_resume_prefix(&part, token).await?;
            }
            let mut options = tokio::fs::OpenOptions::new();
            #[cfg(unix)]
            options.custom_flags(libc::O_NOFOLLOW);
            let mut output = options
                .create(true)
                .write(true)
                .truncate(offset == 0)
                .append(offset > 0)
                .open(managed_path(&self.dir, &part_path)?)
                .await
                .map_err(|e| e.to_string())?;
            let mut received = offset;
            let mut last_progress = None;
            let mut stream = response.bytes_stream();
            loop {
                let chunk = tokio::select! {
                    biased;
                    _ = token.cancelled() => return Ok(()),
                    chunk = stream.next() => chunk,
                };
                let Some(chunk) = chunk else { break };
                let chunk = chunk.map_err(|_| format!("network: {}", file.path))?;
                received += chunk.len() as u64;
                if received > file.bytes {
                    return Err(format!("size: {}", file.path));
                }
                output.write_all(&chunk).await.map_err(|e| e.to_string())?;
                #[cfg(test)]
                if let Ok((entered, resume)) = TEST_BEFORE_FLUSH.try_with(Clone::clone) {
                    entered.notify_one();
                    resume.notified().await;
                }
                // Make progress a durable watermark: cancel/restart resumes these bytes.
                output.flush().await.map_err(|e| e.to_string())?;
                hash.update(&chunk);
                // Bound IPC/render work independently of the server's chunk size.
                // Always publish the first durable bytes and the completed file.
                let now = std::time::Instant::now();
                if received == file.bytes
                    || last_progress.map_or(true, |last| {
                        now.duration_since(last) >= std::time::Duration::from_millis(100)
                    })
                {
                    self.publish(
                        ModelState::Downloading {
                            received: completed + received,
                            total: self.manifest.total_bytes,
                        },
                        progress,
                    );
                    last_progress = Some(now);
                }
                if token.is_cancelled() {
                    return Ok(());
                }
            }
            output.sync_all().await.map_err(|e| e.to_string())?;
            drop(output);
            if received != file.bytes || format!("{:x}", hash.finalize()) != file.sha256 {
                let _ = tokio::fs::remove_file(&part).await;
                return Err(format!("checksum: {}", file.path));
            }
            tokio::fs::rename(
                managed_path(&self.dir, &part_path)?,
                managed_path(&self.dir, &file.path)?,
            )
            .await
            .map_err(|e| e.to_string())?;
            completed += file.bytes;
        }
        write_marker(&self.dir, &self.manifest).map_err(|e| e.to_string())
    }
    pub async fn remove_files(&self, _removal: &RemovalGuard<'_>) -> Result<(), String> {
        let _operation = self.operation.lock().await;
        let result = self.remove_owned_files().await;
        // The command reports removal errors separately. Keep an intact
        // installation ready if deletion was rejected before any files changed.
        *lock(&self.state) = if result.is_err() && marker_matches(&self.dir, &self.manifest) {
            ModelState::Ready {
                bytes: self.manifest.total_bytes,
            }
        } else {
            ModelState::Absent
        };
        result
    }

    async fn remove_owned_files(&self) -> Result<(), String> {
        let mut paths = vec![managed_path(&self.dir, "installed.json")?];
        for file in &self.manifest.files {
            paths.push(managed_path(&self.dir, &file.path)?);
            paths.push(managed_path(&self.dir, &format!("{}.part", file.path))?);
        }
        let mut dirs = std::collections::BTreeSet::new();
        for path in paths {
            match tokio::fs::remove_file(&path).await {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.to_string()),
            }
            dirs.extend(
                path.ancestors()
                    .skip(1)
                    .take_while(|dir| dir.starts_with(&self.dir))
                    .map(Path::to_owned),
            );
        }
        for dir in dirs.into_iter().rev() {
            // A nonempty directory may contain files outside the manifest.
            let _ = tokio::fs::remove_dir(dir).await;
        }
        Ok(())
    }

    #[cfg(test)]
    async fn remove(&self) -> Result<(), String> {
        let removal = self.begin_removal().map_err(|error| error.code)?;
        self.remove_files(&removal).await
    }
}

fn managed_path(dir: &Path, relative: &str) -> Result<PathBuf, String> {
    fn check(path: &Path) -> Result<(), String> {
        match std::fs::symlink_metadata(path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                Err("A local model path is a symbolic link.".into())
            }
            Ok(_) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.to_string()),
        }
    }
    if !relative.split('/').all(safe_component) {
        return Err("Invalid local model file path.".into());
    }
    check(dir.parent().ok_or("Invalid local model directory.")?)?;
    check(dir)?;
    let mut current = dir.to_owned();
    for component in relative.split('/') {
        current.push(component);
        check(&current)?;
    }
    Ok(current)
}

async fn hash_resume_prefix(
    path: &std::path::Path,
    token: &CancellationToken,
) -> Result<sha2::Sha256, String> {
    use sha2::Digest;
    use tokio::io::AsyncReadExt;
    if token.is_cancelled() {
        return Err("cancelled".into());
    }
    let mut input = tokio::fs::File::open(path)
        .await
        .map_err(|error| error.to_string())?;
    let mut hash = sha2::Sha256::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        let count = tokio::select! {
            biased;
            _ = token.cancelled() => return Err("cancelled".into()),
            read = input.read(&mut buffer) => read.map_err(|error| error.to_string())?,
        };
        if count == 0 {
            return Ok(hash);
        }
        hash.update(&buffer[..count]);
    }
}

async fn verified_during_install(
    path: &std::path::Path,
    file: &ModelFile,
    token: &CancellationToken,
) -> Result<bool, String> {
    use sha2::Digest;
    if !tokio::fs::symlink_metadata(path)
        .await
        .is_ok_and(|m| m.is_file() && m.len() == file.bytes)
    {
        return Ok(false);
    }
    let hash = hash_resume_prefix(path, token).await?;
    Ok(format!("{:x}", hash.finalize()) == file.sha256)
}

fn hash_prefix(path: &std::path::Path, hash: &mut sha2::Sha256) -> std::io::Result<()> {
    use sha2::Digest;
    use std::io::Read;
    let mut input = std::fs::File::open(path)?;
    let mut buffer = [0; 64 * 1024];
    loop {
        let count = input.read(&mut buffer)?;
        if count == 0 {
            return Ok(());
        }
        hash.update(&buffer[..count]);
    }
}
fn verified(path: &std::path::Path, file: &ModelFile) -> bool {
    use sha2::Digest;
    if !std::fs::symlink_metadata(path).is_ok_and(|m| m.is_file() && m.len() == file.bytes) {
        return false;
    }
    let mut hash = sha2::Sha256::new();
    hash_prefix(path, &mut hash).is_ok() && format!("{:x}", hash.finalize()) == file.sha256
}
fn write_marker(dir: &std::path::Path, manifest: &Manifest) -> std::io::Result<()> {
    use std::io::Write;
    let path = managed_path(dir, "installed.json").map_err(std::io::Error::other)?;
    let mut marker = tempfile::NamedTempFile::new_in(dir)?;
    serde_json::to_writer(&mut marker, manifest)?;
    marker.flush()?;
    marker.as_file().sync_all()?;
    marker.persist(path).map_err(|e| e.error)?;
    Ok(())
}
fn marker_matches(dir: &std::path::Path, manifest: &Manifest) -> bool {
    // D5 intentionally treats this marker as the receipt for verified installation bytes.
    // A matching pinned manifest and file sizes avoid rehashing 632 MB at each startup.
    // Missing receipts or size changes rehash below; same-size external edits are outside
    // this readiness check (see ADR-0074 and the change's design.md, D5).
    let marker = managed_path(dir, "installed.json")
        .ok()
        .and_then(|path| std::fs::read(path).ok())
        .and_then(|b| serde_json::from_slice::<Manifest>(&b).ok());
    marker.as_ref() == Some(manifest)
        && manifest.files.iter().all(|file| {
            managed_path(dir, &file.path).is_ok_and(|path| {
                std::fs::symlink_metadata(path).is_ok_and(|m| m.is_file() && m.len() == file.bytes)
            })
        })
}
fn installed(dir: &std::path::Path, manifest: &Manifest) -> bool {
    if marker_matches(dir, manifest) {
        return true;
    }
    if manifest
        .files
        .iter()
        .all(|file| managed_path(dir, &file.path).is_ok_and(|path| verified(&path, file)))
    {
        return write_marker(dir, manifest).is_ok();
    }
    false
}

pub const MODEL_STATE_EVENT: &str = "local-transcription-model-state";
static MODELS: OnceLock<std::collections::HashMap<LocalModelId, Arc<TranscriptionModel>>> =
    OnceLock::new();

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelStatus {
    model_id: &'static str,
    name: &'static str,
    minimum_macos: u32,
    compatible: bool,
    download_bytes: u64,
    installed_bytes: u64,
    status: ModelState,
}

fn pinned_model(id: LocalModelId) -> (&'static str, Manifest) {
    let (directory, json) = match id {
        LocalModelId::WhisperTurbo => (
            "whisper-large-v3-turbo-coreml",
            include_str!("../resources/local-transcriber-model-whisper.json"),
        ),
        LocalModelId::ParakeetUltra => (
            "parakeet-ultra-coreml",
            include_str!("../resources/local-transcriber-model.json"),
        ),
        LocalModelId::FluidSpeechPro => (
            "fluid-speech-pro-coreml",
            include_str!("../resources/local-transcriber-model-pro.json"),
        ),
        LocalModelId::Phonon25 => (
            "phonon-2.5-coreml",
            include_str!("../resources/local-transcriber-model-phonon.json"),
        ),
    };
    let manifest: Manifest = serde_json::from_str(json).expect("pinned model manifest");
    assert!(
        manifest.validate(),
        "invalid model manifest for {}",
        id.as_str()
    );
    (directory, manifest)
}

pub fn setup(app: &mut tauri::App) -> Result<(), tauri::Error> {
    use tauri::Emitter;
    if !crate::providers::local_transcription_supported() {
        return Ok(());
    }
    let root = crate::app_paths::app_data_dir(app.handle())?.join("models");
    let models = MODELS.get_or_init(|| {
        LocalModelId::ALL
            .into_iter()
            .map(|id| {
                let (directory, manifest) = pinned_model(id);
                (
                    id,
                    Arc::new(TranscriptionModel::new(root.join(directory), manifest)),
                )
            })
            .collect()
    });
    let app = app.handle().clone();
    tauri::async_runtime::spawn(async move {
        let settings = crate::providers::effective_current_settings();
        let selected = (settings.transcription_provider == crate::providers::PROVIDER_LOCAL)
            .then(|| LocalModelId::parse(&settings.transcription_model))
            .flatten();
        let ids = selected.into_iter().chain(
            LocalModelId::ALL
                .into_iter()
                .filter(|id| Some(*id) != selected),
        );
        for id in ids {
            let model = &models[&id];
            model
                .refresh_from_disk(
                    |state| {
                        let _ = app.emit(MODEL_STATE_EVENT, model.status(id, state));
                    },
                    crate::providers::local_transcriber::selection_changed,
                )
                .await;
        }
    });
    Ok(())
}

impl TranscriptionModel {
    pub fn status(&self, id: LocalModelId, status: ModelState) -> ModelStatus {
        use crate::providers::transcription::macos_major;
        let installed_bytes = if let ModelState::Downloading { received, .. } = status {
            received
        } else {
            self.installed_bytes()
        };
        ModelStatus {
            model_id: id.as_str(),
            name: id.name(),
            minimum_macos: id.minimum_macos(),
            compatible: id.compatible(macos_major()),
            download_bytes: self.manifest.total_bytes,
            installed_bytes,
            status,
        }
    }

    fn installed_bytes(&self) -> u64 {
        self.manifest
            .files
            .iter()
            .map(|file| {
                let bytes = |relative: &str| {
                    managed_path(&self.dir, relative)
                        .ok()
                        .and_then(|path| std::fs::symlink_metadata(path).ok())
                        .filter(std::fs::Metadata::is_file)
                        .map_or(0, |metadata| metadata.len())
                };
                bytes(&file.path) + bytes(&format!("{}.part", file.path))
            })
            .sum()
    }

    pub async fn install_pinned(
        &self,
        progress: impl Fn(ModelState) + Send + Sync,
    ) -> Result<(), String> {
        self.install(&self.manifest.base_url(), progress).await
    }
}

pub fn runtime(id: LocalModelId) -> Result<&'static Arc<TranscriptionModel>, AppError> {
    MODELS
        .get()
        .and_then(|models| models.get(&id))
        .ok_or_else(|| {
            AppError::new(
                "local_model_not_installed",
                "Install the local model in Settings > Models.",
            )
        })
}

pub fn snapshots() -> Vec<ModelStatus> {
    LocalModelId::ALL
        .into_iter()
        .filter_map(|id| {
            let model = runtime(id).ok()?;
            Some(model.status(id, model.state()))
        })
        .collect()
}

pub fn acquire(id: LocalModelId) -> Option<(PathBuf, Arc<ModelUse>)> {
    let model = runtime(id).ok()?;
    Some((model.dir.clone(), model.acquire()?))
}

#[cfg(test)]
#[path = "transcription_model_tests.rs"]
mod tests;
