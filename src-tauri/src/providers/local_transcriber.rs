//! Offline JSON-lines client for the pinned Swift helper.
use super::groq::{TimedTranscription, TranscriptSegment, TranscriptionProviderResult};
use super::transcription::{LocalModelId, LocalSelection};
use crate::domain::types::AppError;
use serde::Deserialize;
#[cfg(test)]
use std::sync::atomic::Ordering;
use std::sync::{Arc, OnceLock};
use std::{
    future::Future,
    path::{Path, PathBuf},
    pin::Pin,
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout},
    sync::{watch, Mutex},
};
pub const LOCAL_TRANSCRIBER_FAILED_MESSAGE: &str =
    "The local transcription model failed. Try again or reinstall it in Settings > Models.";
fn failed() -> AppError {
    AppError::new("local_transcriber_failed", LOCAL_TRANSCRIBER_FAILED_MESSAGE)
}
#[derive(Clone, Debug, Deserialize)]
pub struct Token {
    pub t: String,
    pub start_ms: i64,
    pub end_ms: i64,
}
#[derive(Clone, Debug, Deserialize)]
pub struct LocalTranscription {
    pub text: String,
    pub tokens: Vec<Token>,
    pub duration_ms: i64,
    pub language: Option<String>,
}
impl LocalTranscription {
    pub fn plain(self) -> Result<TranscriptionProviderResult, AppError> {
        if self.text.trim().is_empty() {
            return Err(AppError::new("no_speech", "No speech detected."));
        }
        Ok(TranscriptionProviderResult {
            text: self.text,
            language: self.language,
            provider: "local".into(),
        })
    }
    pub fn timed(self) -> Result<TimedTranscription, AppError> {
        self.clone().plain()?;
        let mut segments: Vec<TranscriptSegment> = Vec::new();
        for token in self.tokens {
            if token.start_ms < 0
                || token.end_ms < token.start_ms
                || token.end_ms > self.duration_ms
            {
                return Err(failed());
            }
            if segments
                .last()
                .is_some_and(|previous| token.start_ms < previous.start_ms)
            {
                return Err(failed());
            }
            let text = token.t.replace('▁', " ");
            if let Some(previous) = segments
                .last_mut()
                .filter(|previous| token.start_ms - previous.end_ms < 400)
            {
                previous.text.push_str(&text);
                previous.end_ms = previous.end_ms.max(token.end_ms);
            } else {
                segments.push(TranscriptSegment {
                    start_ms: token.start_ms,
                    end_ms: token.end_ms,
                    text,
                });
            }
        }
        for segment in &mut segments {
            segment.text = segment.text.trim().to_string();
        }
        segments.retain(|segment| !segment.text.is_empty());
        if segments.is_empty() {
            return Err(failed());
        }
        Ok(TimedTranscription {
            language: self.language,
            segments,
        })
    }
}
pub trait LocalTranscribe: Send + Sync {
    fn transcribe<'a>(
        &'a self,
        model: &'a LocalSelection,
        audio_path: &'a Path,
        language: Option<&'a str>,
    ) -> Pin<Box<dyn Future<Output = Result<LocalTranscription, AppError>> + Send + 'a>>;
}

struct Process {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
    model_id: LocalModelId,
    model_dir: PathBuf,
    request_id: u64,
}
impl Process {
    async fn request(
        &mut self,
        mut value: serde_json::Value,
    ) -> Result<serde_json::Value, AppError> {
        self.request_id += 1;
        value["request_id"] = self.request_id.into();
        value["model_id"] = self.model_id.as_str().into();
        let mut bytes = serde_json::to_vec(&value).map_err(|_| failed())?;
        bytes.push(b'\n');
        self.input.write_all(&bytes).await.map_err(|_| failed())?;
        self.input.flush().await.map_err(|_| failed())?;
        let mut response = Vec::new();
        loop {
            let buffer = self.output.fill_buf().await.map_err(|_| failed())?;
            if buffer.is_empty() {
                return Err(failed());
            }
            let length = buffer
                .iter()
                .position(|byte| *byte == b'\n')
                .map(|i| i + 1)
                .unwrap_or(buffer.len());
            if response.len() + length > 8 * 1024 * 1024 {
                return Err(failed());
            }
            let complete = buffer[length - 1] == b'\n';
            response.extend_from_slice(&buffer[..length]);
            self.output.consume(length);
            if complete {
                break;
            }
        }
        let value: serde_json::Value = serde_json::from_slice(&response).map_err(|_| failed())?;
        if value["ok"] != true
            || value["request_id"] != self.request_id
            || value["model_id"] != self.model_id.as_str()
        {
            return Err(failed());
        }
        Ok(value)
    }
    async fn stop(mut self) {
        let _ = self.child.kill().await;
        let _ = self.child.wait().await;
    }
}
pub struct LocalTranscriber {
    executable: PathBuf,
    timeout: Duration,
    process: Mutex<Option<Process>>,
    stop: watch::Sender<bool>,
    selected: std::sync::Mutex<Option<LocalModelId>>,
}

impl LocalTranscriber {
    fn new(executable: PathBuf, timeout: Duration) -> Self {
        Self {
            executable,
            timeout,
            process: Mutex::new(None),
            stop: watch::channel(false).0,
            selected: std::sync::Mutex::new(None),
        }
    }

    fn selected(&self) -> Option<LocalModelId> {
        *self
            .selected
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn select(&self, model: Option<LocalModelId>) {
        *self
            .selected
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = model;
    }

    async fn call(
        &self,
        model: &LocalSelection,
        request: Option<serde_json::Value>,
    ) -> Result<serde_json::Value, AppError> {
        let mut stopped = self.stop.subscribe();
        let mut slot = self.process.lock().await;
        if *stopped.borrow() {
            return Err(failed());
        }
        if request.is_none() && self.selected() != Some(model.id) {
            return Ok(serde_json::json!({"ok": true}));
        }

        // Own the process while awaiting: cancellation/drop cannot leave an
        // unread response in the pipe. A selection change never interrupts it.
        let mut process = slot.take();
        let operation = async {
            if process
                .as_ref()
                .is_some_and(|p| p.model_id != model.id || p.model_dir != model.directory)
            {
                if let Some(old) = process.take() {
                    old.stop().await;
                }
            }
            if process.is_none() {
                let mut child = crate::hide_console::tokio_command(&self.executable)
                    .stdin(std::process::Stdio::piped())
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::null())
                    .kill_on_drop(true)
                    .spawn()
                    .map_err(|_| failed())?;
                let input = child.stdin.take().ok_or_else(failed)?;
                let output = BufReader::new(child.stdout.take().ok_or_else(failed)?);
                process = Some(Process {
                    child,
                    input,
                    output,
                    model_id: model.id,
                    model_dir: model.directory.clone(),
                    request_id: 0,
                });
                process
                    .as_mut()
                    .ok_or_else(failed)?
                    .request(serde_json::json!({"op":"load","model_dir":model.directory}))
                    .await?;
            }
            match request {
                Some(request) => process.as_mut().ok_or_else(failed)?.request(request).await,
                None => Ok(serde_json::json!({"ok":true})),
            }
        };
        let result = tokio::select! {
            result = tokio::time::timeout(self.timeout, operation) => result.unwrap_or_else(|_| Err(failed())),
            _ = stopped.changed() => Err(failed()),
        };
        if result.is_ok() && self.selected() == Some(model.id) {
            *slot = process;
        } else if let Some(process) = process {
            process.stop().await;
        }
        result
    }

    async fn warm(&self, model: &LocalSelection) -> Result<(), AppError> {
        self.call(model, None).await.map(|_| ())
    }

    async fn shutdown(&self) {
        self.stop.send_replace(true);
        if let Some(process) = self.process.lock().await.take() {
            process.stop().await;
        }
    }

    async fn retire_unselected(&self) {
        let mut slot = self.process.lock().await;
        if slot
            .as_ref()
            .is_some_and(|process| self.selected() != Some(process.model_id))
        {
            if let Some(process) = slot.take() {
                process.stop().await;
            }
        }
    }

    #[cfg(target_os = "macos")]
    async fn unload(&self, id: LocalModelId, dir: &Path) {
        let mut slot = self.process.lock().await;
        if slot
            .as_ref()
            .is_some_and(|process| process.model_id == id && process.model_dir == dir)
        {
            if let Some(process) = slot.take() {
                process.stop().await;
            }
        }
    }
}
impl LocalTranscribe for LocalTranscriber {
    fn transcribe<'a>(
        &'a self,
        model: &'a LocalSelection,
        audio_path: &'a Path,
        language: Option<&'a str>,
    ) -> Pin<Box<dyn Future<Output = Result<LocalTranscription, AppError>> + Send + 'a>> {
        Box::pin(async move {
            let result = self.call(model, Some(serde_json::json!({"op":"transcribe","audio_path":audio_path,"language":language}))).await?;
            serde_json::from_value(result).map_err(|_| failed())
        })
    }
}
static CLIENT: OnceLock<Arc<LocalTranscriber>> = OnceLock::new();
pub fn setup(app: &mut tauri::App) {
    use tauri::Manager;
    if !super::local_transcription_supported() {
        return;
    }
    let relative = "Clovy Local Transcriber.app/Contents/MacOS/clovy-local-transcriber";
    let packaged = app
        .path()
        .resource_dir()
        .ok()
        .map(|path| path.join("native/bin").join(relative));
    let development = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|root| root.join(".tauri-helper").join(relative));
    let executable = if cfg!(debug_assertions) {
        development.filter(|path| path.is_file()).or(packaged)
    } else {
        packaged
    };
    if let Some(executable) = executable {
        let client = CLIENT
            .get_or_init(|| Arc::new(LocalTranscriber::new(executable, Duration::from_secs(120))));
        app.manage(client.clone());
        selection_changed();
    }
}
pub async fn transcribe(
    model: &LocalSelection,
    audio_path: &Path,
    language: Option<&str>,
) -> Result<LocalTranscription, AppError> {
    let result = async {
        #[cfg(test)]
        if let Ok(fake) = TEST_LOCAL.try_with(Arc::clone) {
            return fake.transcribe(model, audio_path, language).await;
        }
        CLIENT
            .get()
            .ok_or_else(failed)?
            .transcribe(model, audio_path, language)
            .await
    }
    .await;
    result.map_err(|mut error| {
        error.message = format!(
            "{} failed to transcribe. Try again or reinstall it in Settings > Models.",
            model.id.name()
        );
        error.details = Some(serde_json::json!({ "model": model.id.name() }));
        error
    })
}

pub fn selection_changed() {
    if let Some(client) = CLIENT.get() {
        let selected = {
            let mut slot = client
                .selected
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let settings = super::effective_current_settings();
            *slot = (settings.transcription_provider == super::PROVIDER_LOCAL)
                .then(|| LocalModelId::parse(&settings.transcription_model))
                .flatten();
            *slot
        };
        let client = client.clone();
        tauri::async_runtime::spawn(async move {
            client.retire_unselected().await;
            if let Some(model) =
                selected.and_then(|id| super::transcription::acquire_local(id).ok())
            {
                let _ = client.warm(&model).await;
            }
        });
    }
}

#[cfg(target_os = "macos")]
pub async fn unload(id: LocalModelId, dir: &Path) {
    if let Some(client) = CLIENT.get() {
        client.unload(id, dir).await;
    }
}

pub async fn shutdown() {
    if let Some(client) = CLIENT.get() {
        client.select(None);
        client.shutdown().await;
    }
}
#[cfg(test)]
tokio::task_local! { pub(crate) static TEST_LOCAL: Arc<dyn LocalTranscribe>; }

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    fn fake(body: &str, timeout: Duration) -> (tempfile::TempDir, LocalTranscriber) {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("helper");
        std::fs::write(
            &script,
            format!("#!/bin/sh\ncd '{}'\n{}\n", dir.path().display(), body),
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
        let helper = LocalTranscriber::new(script, timeout);
        helper.select(Some(LocalModelId::ParakeetUltra));
        (dir, helper)
    }
    const GOOD: &str = r#"echo $$ >> pids
while IFS= read -r line; do
 echo "$line" >> requests
 request_id=${line#*'"request_id":'}
 request_id=${request_id%%[!0-9]*}
 model_id=${line#*'"model_id":"'}
 model_id=${model_id%%\"*}
 case "$line" in
 *'"op":"load"'*) printf '{"ok":true,"request_id":%s,"model_id":"%s"}\n' "$request_id" "$model_id";;
 *) printf '{"ok":true,"request_id":%s,"model_id":"%s","text":"Olá mundo","tokens":[{"t":"▁Olá","start_ms":0,"end_ms":100},{"t":"▁mundo","start_ms":500,"end_ms":700}],"duration_ms":700,"language":"pt"}\n' "$request_id" "$model_id";;
 esac
done"#;
    #[tokio::test]
    async fn parakeet_review_disable_reaps_a_process_returned_by_an_overlapping_call() {
        let (_dir, helper) = fake(GOOD, Duration::from_secs(2));
        helper
            .warm(&LocalSelection::fixture(
                "/model",
                LocalModelId::ParakeetUltra,
            ))
            .await
            .unwrap();
        // A successful call owns the slot until it has restored its process.
        let mut slot = helper.process.lock().await;
        let process = slot.take();
        helper.select(None);
        *slot = process;
        drop(slot);
        helper.retire_unselected().await;
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if helper.process.lock().await.is_none() {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("Deselecting Local must clear a helper restored by an overlapping call");
    }

    #[tokio::test]
    async fn helper_reuses_load_serializes_and_shutdown_reaps() {
        let (dir, helper) = fake(GOOD, Duration::from_secs(2));
        helper
            .warm(&LocalSelection::fixture(
                "/model",
                LocalModelId::ParakeetUltra,
            ))
            .await
            .unwrap();
        let model = LocalSelection::fixture("/model", LocalModelId::ParakeetUltra);
        let (a, b) = tokio::join!(
            helper.transcribe(&model, Path::new("/take.m4a"), Some("pt")),
            helper.transcribe(&model, Path::new("/take.wav"), None)
        );
        assert_eq!(a.unwrap().plain().unwrap().text, "Olá mundo");
        assert_eq!(b.unwrap().timed().unwrap().segments.len(), 2);
        helper.shutdown().await;
        let requests = std::fs::read_to_string(dir.path().join("requests")).unwrap();
        assert_eq!(requests.lines().count(), 3);
        assert_eq!(requests.matches("\"op\":\"load\"").count(), 1);
        let pid: i32 = std::fs::read_to_string(dir.path().join("pids"))
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
        assert!(helper
            .transcribe(&model, Path::new("/late.wav"), None)
            .await
            .is_err());
        assert_eq!(
            std::fs::read_to_string(dir.path().join("pids"))
                .unwrap()
                .lines()
                .count(),
            1
        );
    }
    #[tokio::test]
    async fn helper_stall_exit_and_invalid_protocol_are_local_failures() {
        for body in [
            "exec sleep 30",
            "exit 7",
            "read line; echo nonsense",
            "read line; echo '{\"ok\":false,\"error\":\"broken\"}'",
        ] {
            let (_dir, helper) = fake(body, Duration::from_millis(60));
            let error = helper
                .warm(&LocalSelection::fixture(
                    "/model",
                    LocalModelId::ParakeetUltra,
                ))
                .await
                .unwrap_err();
            assert_eq!(error.code, "local_transcriber_failed");
            assert_eq!(error.message, LOCAL_TRANSCRIBER_FAILED_MESSAGE);
            helper.shutdown().await;
        }
    }
    #[test]
    fn token_segments_preserve_words_and_split_at_400_ms() {
        let local: LocalTranscription = serde_json::from_value(serde_json::json!({"text":"Olá mundo. Próximo", "tokens":[{"t":"▁Olá","start_ms":0,"end_ms":100},{"t":"▁mun","start_ms":120,"end_ms":200},{"t":"do.","start_ms":200,"end_ms":300},{"t":"▁Próximo","start_ms":700,"end_ms":900}], "duration_ms":900})).unwrap();
        let result = local.timed().unwrap();
        assert_eq!(
            result.segments,
            vec![
                TranscriptSegment {
                    start_ms: 0,
                    end_ms: 300,
                    text: "Olá mundo.".into()
                },
                TranscriptSegment {
                    start_ms: 700,
                    end_ms: 900,
                    text: "Próximo".into()
                }
            ]
        );
    }
    #[test]
    fn empty_transcription_and_invalid_times_fail() {
        let local = LocalTranscription {
            text: " ".into(),
            tokens: vec![],
            duration_ms: 0,
            language: None,
        };
        assert_eq!(local.clone().plain().unwrap_err().code, "no_speech");
        assert_eq!(local.timed().unwrap_err().code, "no_speech");
        let invalid = LocalTranscription {
            text: "word".into(),
            tokens: vec![Token {
                t: "word".into(),
                start_ms: 3,
                end_ms: 1,
            }],
            duration_ms: 4,
            language: None,
        };
        assert_eq!(
            invalid.timed().unwrap_err().code,
            "local_transcriber_failed"
        );
    }
}

#[cfg(test)]
#[derive(Default)]
pub(crate) struct FakeLocalTranscriber {
    pub calls: std::sync::Mutex<Vec<(PathBuf, Option<String>)>>,
    pub fail: bool,
}
#[cfg(test)]
impl LocalTranscribe for FakeLocalTranscriber {
    fn transcribe<'a>(
        &'a self,
        _model: &'a LocalSelection,
        audio_path: &'a Path,
        language: Option<&'a str>,
    ) -> Pin<Box<dyn Future<Output = Result<LocalTranscription, AppError>> + Send + 'a>> {
        Box::pin(async move {
            self.calls
                .lock()
                .unwrap()
                .push((audio_path.into(), language.map(str::to_string)));
            if self.fail {
                return Err(failed());
            }
            Ok(LocalTranscription {
                text: "Olá mundo".into(),
                tokens: (0..3)
                    .map(|i| Token {
                        t: format!("▁Turno {i}"),
                        start_ms: i * 1000,
                        end_ms: i * 1000 + 300,
                    })
                    .collect(),
                duration_ms: 2300,
                language: language.map(str::to_string),
            })
        })
    }
}

#[cfg(test)]
pub(crate) struct NetworkSentinel {
    pub origin: String,
    pub requests: Arc<std::sync::atomic::AtomicUsize>,
    task: tokio::task::JoinHandle<()>,
}
#[cfg(test)]
impl NetworkSentinel {
    pub async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count = requests.clone();
        let task = tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                count.fetch_add(1, Ordering::SeqCst);
                let _ = stream.write_all(b"HTTP/1.1 500 Unexpected\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
            }
        });
        Self {
            origin,
            requests,
            task,
        }
    }
    pub fn assert_unused(&self) {
        assert_eq!(
            self.requests.load(Ordering::SeqCst),
            0,
            "Local reached the network sentinel"
        );
    }
}
#[cfg(test)]
impl Drop for NetworkSentinel {
    fn drop(&mut self) {
        self.task.abort();
    }
}
