//! Private agent-browser client. The stdio pipes are inherited capabilities;
//! no command socket, CDP port, global CLI or user browser is involved.
use super::{
    policy::{validate_final_public_url, PolicyConfig, Resolver, SystemResolver},
    proxy::PinningProxy,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, Read, Write},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tokio::sync::{mpsc, oneshot};

const WIRE_LIMIT: u64 = 16 * 1024 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(45);

#[derive(Debug, Clone)]
pub struct EngineError {
    pub code: String,
    pub message: String,
}
impl EngineError {
    pub fn new(code: &str, message: &str) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}
type Result<T> = std::result::Result<T, EngineError>;
fn closed() -> EngineError {
    EngineError::new("browser_session_closed", "The browser session has ended.")
}
fn unknown() -> EngineError {
    EngineError::new(
        "browser_outcome_unknown",
        "The browser outcome is unknown. The operation may have happened; do not repeat it.",
    )
}
fn post_network_error(read_only: bool, error: EngineError) -> EngineError {
    if read_only {
        return error;
    }
    // validate_url normalizes every policy/resolver failure to this safe cause.
    // A successful mutation response precedes this check, so reporting refusal
    // would hide possible delivery and invite the caller to replay the effect.
    EngineError::new(
        "browser_outcome_unknown",
        "The browser operation may have happened, but its post-action network validation failed (browser_network_blocked). Its task has been stopped. Do not repeat it.",
    )
}
fn failed() -> EngineError {
    EngineError::new(
        "browser_start_failed",
        "Clovy could not start its browser. Prepare or reinstall the bundled browser resources.",
    )
}

#[derive(Clone)]
pub struct EngineConfig {
    pub engine: PathBuf,
    pub chrome: PathBuf,
    pub persistent_profile: PathBuf,
    pub temporary_root: PathBuf,
    pub persistent: bool,
    pub policy: PolicyConfig,
    pub resolver: Arc<dyn Resolver>,
}
impl EngineConfig {
    pub(crate) fn chrome_namespace(&self) -> Option<PathBuf> {
        self.engine.parent().map(|parent| parent.join("chrome"))
    }

    pub fn production(resources: PathBuf, data: PathBuf, persistent: bool) -> Self {
        let platform = if cfg!(target_os = "windows") {
            "win64"
        } else if cfg!(target_os = "macos") {
            if cfg!(target_arch = "aarch64") {
                "mac-arm64"
            } else {
                "mac-x64"
            }
        } else {
            "linux64"
        };
        let chrome = match platform {
            "win64" => "chrome/chrome-win64/chrome.exe",
            "mac-arm64" => "chrome/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing",
            "mac-x64" => "chrome/chrome-mac-x64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing",
            _ => "chrome/chrome-linux64/chrome",
        };
        let resources = resources.join(platform);
        Self {
            engine: resources.join(if cfg!(windows) {
                "clovy-agent-browser.exe"
            } else {
                "clovy-agent-browser"
            }),
            chrome: resources.join(chrome),
            persistent_profile: data.join("browser-profile"),
            temporary_root: data.join("browser-temporary"),
            persistent,
            policy: PolicyConfig::default(),
            resolver: Arc::new(SystemResolver),
        }
    }
}

struct Request {
    value: Value,
    reply: oneshot::Sender<Result<Value>>,
}
struct Process {
    child: Child,
    #[cfg(windows)]
    job: crate::process_group::windows_job::Job,
}
impl Process {
    fn stop(&mut self, grace: Duration) {
        #[cfg(unix)]
        unsafe {
            libc::kill(-(self.child.id() as i32), libc::SIGTERM);
        }
        #[cfg(windows)]
        self.job.terminate();
        let end = Instant::now() + grace.min(Duration::from_millis(250));
        while Instant::now() < end {
            if matches!(self.child.try_wait(), Ok(Some(_))) {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        // Always kill the group even if the leader has exited first.
        #[cfg(unix)]
        unsafe {
            libc::kill(-(self.child.id() as i32), libc::SIGKILL);
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
    fn tree_exited(&self) -> bool {
        #[cfg(windows)]
        {
            self.job.is_empty().unwrap_or(false)
        }
        #[cfg(unix)]
        {
            unix_group_exited(self.child.id() as i32)
        }
    }
}
struct Slot {
    cancelled: AtomicBool,
    suspended: AtomicBool,
    visible: AtomicBool,
    started: AtomicBool,
    sequence: AtomicU64,
    process: Mutex<Option<Process>>,
    sender: Mutex<Option<mpsc::UnboundedSender<Request>>>,
    proxy: Mutex<Option<PinningProxy>>,
    lease: Mutex<Option<File>>,
    profile: PathBuf,
    persistent: bool,
    last_used: Mutex<Instant>,
    /// Capture/dispatch and takeover share this lock. Stop never acquires it.
    operation: tokio::sync::Mutex<()>,
}
impl Slot {
    fn stop(&self) {
        self.stop_with_grace(Duration::from_millis(250));
    }
    fn stop_with_grace(&self, grace: Duration) {
        if self.cancelled.swap(true, Ordering::SeqCst) {
            return;
        }
        if let Some(sender) = self.sender.lock().unwrap().as_ref() {
            let (reply, _) = oneshot::channel();
            let _=sender.send(Request{value:json!({"id":self.sequence.fetch_add(1,Ordering::SeqCst),"action":"shutdown"}),reply});
        }
        let process = self.process.lock().unwrap().take();
        let lease = self.lease.lock().unwrap().take();
        if let Some(mut process) = process {
            process.stop(grace);
            if !process.tree_exited() {
                // Tree termination can be asynchronous. The reaper owns the
                // group/Job and lease until every descendant has exited, without
                // holding a mutex needed by Stop or another browser call.
                let profile = self.profile.clone();
                let persistent = self.persistent;
                std::thread::spawn(move || {
                    while !process.tree_exited() {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    drop(lease);
                    if !persistent {
                        let _ = fs::remove_dir_all(profile);
                    }
                });
                if let Some(proxy) = self.proxy.lock().unwrap().take() {
                    proxy.shutdown();
                }
                self.sender.lock().unwrap().take();
                return;
            }
        }
        self.sender.lock().unwrap().take();
        if let Some(proxy) = self.proxy.lock().unwrap().take() {
            proxy.shutdown();
        }
        // The lease is released only after the entire owned process group/job exits.
        drop(lease);
        if !self.persistent {
            let _ = fs::remove_dir_all(&self.profile);
        }
    }
}
impl Drop for Slot {
    fn drop(&mut self) {
        self.stop();
    }
}

pub struct AgentBrowserTransport {
    config: EngineConfig,
    slots: Mutex<HashMap<String, Arc<Slot>>>,
}
impl AgentBrowserTransport {
    pub fn new(config: EngineConfig) -> Arc<Self> {
        let transport = Arc::new(Self {
            config,
            slots: Mutex::new(HashMap::new()),
        });
        let weak = Arc::downgrade(&transport);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(30)).await;
                let Some(transport) = weak.upgrade() else {
                    break;
                };
                transport.reap_idle();
            }
        });
        transport
    }
    fn reap_idle(&self) {
        let expired: Vec<_> = self
            .slots
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, slot)| {
                !slot.persistent
                    && slot.last_used.lock().unwrap().elapsed() >= Duration::from_secs(300)
            })
            .map(|(id, _)| id.clone())
            .collect();
        for id in expired {
            self.terminate(&id);
        }
    }
    pub fn reserve(&self, id: &str) -> Result<()> {
        let mut slots = self.slots.lock().unwrap();
        slots.retain(|_, slot| !slot.cancelled.load(Ordering::SeqCst));
        if slots.contains_key(id) {
            return Err(closed());
        }
        if !self.config.persistent && slots.len() >= 2 {
            return Err(EngineError::new(
                "browser_session_limit",
                "Two routine browser sessions are already running.",
            ));
        }
        let profile = if self.config.persistent {
            self.config.persistent_profile.clone()
        } else {
            self.config
                .temporary_root
                .join(uuid::Uuid::new_v4().to_string())
        };
        fs::create_dir_all(&profile).map_err(|_| failed())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&profile, fs::Permissions::from_mode(0o700))
                .map_err(|_| failed())?;
        }
        let lease = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(profile.join(".clovy-lease"))
            .map_err(|_| failed())?;
        lock_profile(&lease).map_err(|_| {
            EngineError::new(
                "browser_profile_busy",
                "Clovy's browser profile is in use by another task.",
            )
        })?;
        slots.insert(
            id.into(),
            Arc::new(Slot {
                cancelled: AtomicBool::new(false),
                suspended: AtomicBool::new(false),
                visible: AtomicBool::new(false),
                started: AtomicBool::new(false),
                sequence: AtomicU64::new(1),
                process: Mutex::new(None),
                sender: Mutex::new(None),
                proxy: Mutex::new(None),
                lease: Mutex::new(Some(lease)),
                profile,
                persistent: self.config.persistent,
                last_used: Mutex::new(Instant::now()),
                operation: tokio::sync::Mutex::new(()),
            }),
        );
        Ok(())
    }
    fn slot(&self, id: &str) -> Result<Arc<Slot>> {
        self.slots
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .filter(|s| !s.cancelled.load(Ordering::SeqCst))
            .ok_or_else(closed)
    }
    pub fn terminate(&self, id: &str) {
        self.terminate_with_grace(id, Duration::from_millis(250));
    }
    pub fn terminate_with_grace(&self, id: &str, grace: Duration) {
        if let Some(slot) = self.slots.lock().unwrap().remove(id) {
            slot.stop_with_grace(grace);
        }
    }
    pub fn shutdown(&self) {
        let slots: Vec<_> = self
            .slots
            .lock()
            .unwrap()
            .drain()
            .map(|(_, slot)| slot)
            .collect();
        for slot in slots {
            slot.stop();
        }
    }
    pub fn state(&self, id: &str) -> Result<Value> {
        let slot = self.slot(id)?;
        Ok(
            json!({"sessionId":id,"mode":if slot.visible.load(Ordering::SeqCst) {"visible"} else {"background"},"suspended":slot.suspended.load(Ordering::SeqCst)}),
        )
    }
    async fn start(&self, slot: &Arc<Slot>, headed: bool) -> Result<Value> {
        if slot.cancelled.load(Ordering::SeqCst) {
            return Err(closed());
        }
        if slot.started.swap(true, Ordering::SeqCst) {
            return Err(closed());
        }
        if !self.config.engine.is_file() || !self.config.chrome.is_file() {
            return Err(failed());
        }
        verify_bundle(&self.config)?;
        let proxy = PinningProxy::start(self.config.resolver.clone(), self.config.policy.clone())
            .await
            .map_err(|_| failed())?;
        let port = proxy.port();
        {
            let mut guard = slot.proxy.lock().unwrap();
            if slot.cancelled.load(Ordering::SeqCst) {
                proxy.shutdown();
                return Err(closed());
            }
            *guard = Some(proxy);
        }
        let mut command = Command::new(&self.config.engine);
        command
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        // Keep the normal HOME/native credential store. None of upstream's
        // AGENT_BROWSER_* launch/attach/plugin/environment controls survive.
        for key in [
            "HOME",
            "USERPROFILE",
            "LOCALAPPDATA",
            "APPDATA",
            "SystemRoot",
            "WINDIR",
            "TEMP",
            "TMP",
            "DISPLAY",
            "XAUTHORITY",
            "DBUS_SESSION_BUS_ADDRESS",
            "XDG_RUNTIME_DIR",
        ] {
            if let Some(value) = std::env::var_os(key) {
                command.env(key, value);
            }
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x00000004 | 0x08000000);
        }
        // This mutex covers only spawn/publication, never CDP or wire I/O.
        let (stdin, stdout) = {
            let mut process_guard = slot.process.lock().unwrap();
            if slot.cancelled.load(Ordering::SeqCst) {
                return Err(closed());
            }
            #[cfg(windows)]
            let job = crate::process_group::windows_job::Job::new().map_err(|_| failed())?;
            let mut child = command.spawn().map_err(|_| failed())?;
            #[cfg(windows)]
            {
                use std::os::windows::io::AsRawHandle;
                if job
                    .assign(child.as_raw_handle())
                    .and_then(|_| {
                        crate::process_group::windows_job::resume_first_thread(child.id())
                    })
                    .is_err()
                {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(failed());
                }
            }
            let stdin = child.stdin.take().ok_or_else(failed)?;
            let stdout = child.stdout.take().ok_or_else(failed)?;
            *process_guard = Some(Process {
                child,
                #[cfg(windows)]
                job,
            });
            (stdin, stdout)
        };
        let (sender, mut receiver) = mpsc::unbounded_channel::<Request>();
        *slot.sender.lock().unwrap() = Some(sender);
        let token = format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
        let (ready_tx, ready_rx) = oneshot::channel();
        let weak = Arc::downgrade(slot);
        let persistent = slot.persistent;
        std::thread::spawn(move || {
            let mut stdin = stdin;
            let mut stdout = BufReader::new(stdout);
            let ready = write_line(&mut stdin, &json!({"protocol":1,"token":token}))
                .and_then(|_| read_line(&mut stdout));
            let ready = ready.and_then(|value| {
                if value["protocol"] == 1 && value["token"].as_str() == Some(&token) {
                    Ok(())
                } else {
                    Err(failed())
                }
            });
            let okay = ready.is_ok();
            let _ = ready_tx.send(ready);
            if okay {
                while let Some(request) = receiver.blocking_recv() {
                    let id = request.value["id"].clone();
                    let response = write_line(&mut stdin, &request.value).and_then(|_| read_line(&mut stdout)).and_then(|value| {
                        if value["id"] != id { return Err(unknown()); }
                        match value["delivery"].as_str() {
                            Some("unknown") => Err(unknown()),
                            Some("not_attempted" | "delivered") => {
                                if let Some(error) = value.get("error") {
                                    let code = safe_code(error["code"].as_str().unwrap_or("browser_failed"));
                                    // Error text is host-authored. Never forward page/Chrome errors.
                                    let message = if code == "browser_network_blocked" {
                                        "This page is outside the browser's public web scope. Open a public web page before continuing."
                                    } else if code == "browser_human_takeover_required" {
                                        if persistent {
                                            "First call request_clarification and keep it pending while the user takes over the dedicated browser and resumes control. Then list tabs and inspect a fresh snapshot; never repeat sensitive input."
                                        } else {
                                            "This routine uses an anonymous browser and cannot enter sensitive fields or use a login profile."
                                        }
                                    } else if code == "browser_screenshot_requires_visible" {
                                        if persistent {
                                            "A screenshot of a hidden tab is unavailable. Read a fresh snapshot instead. If an image is required, call request_clarification and keep it pending while the user takes over the dedicated browser and resumes control. Then list tabs and inspect a fresh snapshot."
                                        } else {
                                            "A screenshot of a hidden tab is unavailable. This routine can read a fresh snapshot but cannot request human takeover."
                                        }
                                    } else {
                                        "The browser operation did not complete. Inspect its state before continuing."
                                    };
                                    Err(EngineError::new(code, message))
                                } else { Ok(value.get("data").cloned().unwrap_or(Value::Null)) }
                            }, _ => Err(unknown())
                        }
                    });
                    let lost = response
                        .as_ref()
                        .err()
                        .is_some_and(|e| e.code == "browser_outcome_unknown");
                    let _ = request.reply.send(response);
                    if lost {
                        break;
                    }
                }
            }
            if let Some(slot) = weak.upgrade() {
                slot.stop();
            }
        });
        tokio::time::timeout(Duration::from_secs(10), ready_rx)
            .await
            .map_err(|_| failed())?
            .map_err(|_| failed())??;
        let data = self.request(slot, json!({"action":"start", "chromePath":self.config.chrome, "profilePath":slot.profile,
            "persistent":slot.persistent,"proxyPort":port,"headed":headed})).await?;
        slot.visible.store(headed, Ordering::SeqCst);
        Ok(data)
    }
    async fn request(&self, slot: &Arc<Slot>, mut value: Value) -> Result<Value> {
        if slot.cancelled.load(Ordering::SeqCst) {
            return Err(closed());
        }
        value["id"] = json!(slot.sequence.fetch_add(1, Ordering::SeqCst));
        let (reply, receiver) = oneshot::channel();
        slot.sender
            .lock()
            .unwrap()
            .as_ref()
            .ok_or_else(closed)?
            .send(Request { value, reply })
            .map_err(|_| closed())?;
        *slot.last_used.lock().unwrap() = Instant::now();
        match tokio::time::timeout(REQUEST_TIMEOUT, receiver).await {
            Ok(Ok(result)) => result,
            _ => {
                slot.stop();
                Err(unknown())
            }
        }
    }
    pub async fn execute(&self, id: &str, tool: &str, arguments: Value) -> Result<Value> {
        let slot = self.slot(id)?;
        let _operation = slot.operation.lock().await;
        if slot.suspended.load(Ordering::SeqCst) {
            return Err(EngineError::new(
                "browser_human_takeover",
                "Browser control is paused for you. Resume control explicitly to continue.",
            ));
        }
        if tool == "start_session" {
            return match self.start(&slot, false).await {
                Ok(value) => Ok(value),
                Err(error) => {
                    slot.stop();
                    Err(error)
                }
            };
        }
        let action = match tool {
            "open_tab" => "tab_open",
            "list_tabs" => "tab_list",
            "switch_tab" => "tab_switch",
            "close_tab" => "tab_close",
            "click" | "fill" | "press" => "guarded_action",
            "navigate" | "back" | "snapshot" | "screenshot" | "inspect_reference" => tool,
            _ => {
                return Err(EngineError::new(
                    "unknown_browser_tool",
                    "Unknown browser operation.",
                ))
            }
        };
        if let Some(raw) = arguments.get("url").and_then(Value::as_str) {
            self.validate_url(raw).await?;
        }
        let mut request = arguments;
        if let Some(object) = request.as_object_mut() {
            object.remove("session_id");
        }
        request["action"] = json!(action);
        if action == "guarded_action" {
            request["operation"] = json!(tool);
        }
        let data = self.request(&slot, request).await?;
        let read_only = matches!(
            tool,
            "snapshot" | "screenshot" | "list_tabs" | "inspect_reference"
        );
        if slot.suspended.load(Ordering::SeqCst) {
            if !read_only {
                // Takeover may have been requested while an operation was
                // already being delivered. A pause is not a refusal of that
                // effect; invalidate the task instead of inviting a replay.
                slot.stop();
                return Err(unknown());
            }
            return Err(EngineError::new(
                "browser_human_takeover",
                "Browser control is paused for you. Resume explicitly to continue.",
            ));
        }
        if matches!(tool, "snapshot" | "screenshot")
            && data.get("url").and_then(Value::as_str).is_none()
        {
            slot.stop();
            return Err(EngineError::new(
                "browser_artifact_invalid",
                "The browser capture has no verifiable page scope. Its task has been stopped.",
            ));
        }
        if let Some(raw) = data.get("url").and_then(Value::as_str) {
            if let Err(error) = self.validate_url(raw).await {
                slot.stop();
                return Err(post_network_error(read_only, error));
            }
        }
        if let Some(tabs) = data.get("tabs").and_then(Value::as_array) {
            for tab in tabs {
                if let Some(raw) = tab.get("url").and_then(Value::as_str) {
                    if let Err(error) = self.validate_url(raw).await {
                        slot.stop();
                        return Err(post_network_error(read_only, error));
                    }
                }
            }
        }
        Ok(data)
    }
    async fn validate_url(&self, raw: &str) -> Result<()> {
        if raw == "about:blank" {
            return Ok(());
        }
        validate_final_public_url(raw, self.config.resolver.as_ref(), &self.config.policy)
            .await
            .map_err(|_| {
                EngineError::new(
                    "browser_network_blocked",
                    "The browser destination is blocked by its network policy.",
                )
            })
    }
    /// Called only by the local human UI. No model tool exposes this method.
    pub async fn takeover(&self, id: &str) -> Result<Value> {
        let slot = self.slot(id)?;
        if !slot.persistent {
            return Err(EngineError::new(
                "browser_takeover_unavailable",
                "Routine browsers cannot use the dedicated login profile.",
            ));
        }
        slot.suspended.store(true, Ordering::SeqCst);
        let _operation = slot.operation.lock().await;
        // The private host closes/flushed Chrome and reopens the same profile
        // headed, in the same supervised process group/job. Old refs/tabs die.
        let data = self.request(&slot, json!({"action":"takeover"})).await?;
        slot.visible.store(true, Ordering::SeqCst);
        Ok(data)
    }
    pub async fn resume(&self, id: &str) -> Result<()> {
        let slot = self.slot(id)?;
        let _operation = slot.operation.lock().await;
        self.request(&slot, json!({"action":"resume"})).await?;
        slot.suspended.store(false, Ordering::SeqCst);
        Ok(())
    }
}
impl Drop for AgentBrowserTransport {
    fn drop(&mut self) {
        self.shutdown();
    }
}
fn write_line(writer: &mut impl Write, value: &Value) -> Result<()> {
    let bytes = serde_json::to_vec(value).map_err(|_| unknown())?;
    if bytes.len() as u64 > WIRE_LIMIT {
        return Err(EngineError::new(
            "browser_request_too_large",
            "The browser request is too large.",
        ));
    }
    writer
        .write_all(&bytes)
        .and_then(|_| writer.write_all(b"\n"))
        .and_then(|_| writer.flush())
        .map_err(|_| unknown())
}
fn read_line(reader: &mut impl BufRead) -> Result<Value> {
    let mut line = Vec::new();
    reader
        .take(WIRE_LIMIT + 1)
        .read_until(b'\n', &mut line)
        .map_err(|_| unknown())?;
    if line.is_empty() || line.len() as u64 > WIRE_LIMIT || line.last() != Some(&b'\n') {
        return Err(unknown());
    }
    serde_json::from_slice(&line).map_err(|_| unknown())
}

fn safe_code(code: &str) -> &'static str {
    match code {
        "stale_reference" => "stale_reference",
        "tab_gone" => "tab_gone",
        "sensitive_field" => "browser_human_takeover_required",
        "human_takeover" => "browser_human_takeover",
        "unsupported_key" => "browser_unsupported_key",
        "unsupported_operation" => "browser_unsupported_operation",
        "no_history" => "browser_no_history",
        "invalid_url" => "browser_network_blocked",
        "screenshot_hidden" => "browser_screenshot_requires_visible",
        _ => "browser_operation_failed",
    }
}

/// Provenance checks are shared by release packaging and runtime startup.
pub fn sha256_file(path: &std::path::Path) -> std::io::Result<String> {
    let mut file = File::open(path)?;
    let mut digest = Sha256::new();
    let mut bytes = [0u8; 64 * 1024];
    loop {
        let count = file.read(&mut bytes)?;
        if count == 0 {
            break;
        }
        digest.update(&bytes[..count]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn verify_bundle(config: &EngineConfig) -> Result<()> {
    let root = config.engine.parent().ok_or_else(failed)?;
    let metadata: Value =
        serde_json::from_slice(&fs::read(root.join("provenance.json")).map_err(|_| failed())?)
            .map_err(|_| failed())?;
    if metadata["source"]["commit"] != "5868f7c05020e047c00e68e4abf9253cce864f41"
        || metadata["protocol"] != "clovy-agent-browser/1"
        || metadata["chrome"]["version"] != "156.0.8078.80"
    {
        return Err(failed());
    }
    // A debug binary must also reject resources built from an older private
    // guard/wire revision of the same upstream commit, even outside the normal
    // dev preparation hook. Release signing then protects the matching tree.
    let inputs: [(&str, &[u8]); 6] = [
        (
            "0001-no-reference-recovery.patch",
            include_bytes!("../../../scripts/agent-browser/0001-no-reference-recovery.patch"),
        ),
        (
            "0002-private-host.patch",
            include_bytes!("../../../scripts/agent-browser/0002-private-host.patch"),
        ),
        (
            "private-host/clovy_private_main.rs",
            include_bytes!("../../../scripts/agent-browser/private-host/clovy_private_main.rs"),
        ),
        (
            "private-host/clovy_private_client.rs",
            include_bytes!("../../../scripts/agent-browser/private-host/clovy_private_client.rs"),
        ),
        (
            "private-host/clovy_private_launch.rs",
            include_bytes!("../../../scripts/agent-browser/private-host/clovy_private_launch.rs"),
        ),
        (
            "private-host/clovy_private_guard.js",
            include_bytes!("../../../scripts/agent-browser/private-host/clovy_private_guard.js"),
        ),
    ];
    for (name, bytes) in inputs {
        if metadata["source"]["inputSha256"][name].as_str()
            != Some(format!("{:x}", Sha256::digest(bytes)).as_str())
        {
            return Err(EngineError::new("browser_resource_modified","The private browser revision is stale. Prepare or reinstall its matching resources."));
        }
    }
    for path in [&config.engine, &config.chrome] {
        let relative = path
            .strip_prefix(root)
            .map_err(|_| failed())?
            .to_str()
            .ok_or_else(failed)?
            .replace('\\', "/");
        let expected = metadata["files"][&relative]["sha256"]
            .as_str()
            .ok_or_else(failed)?;
        if sha256_file(path).map_err(|_| failed())? != expected {
            return Err(EngineError::new("browser_resource_modified","Clovy's bundled browser resources changed. Prepare or reinstall them before using Browser use."));
        }
    }
    Ok(())
}

fn lock_profile(file: &File) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        // The descriptor stays open until all supervised processes exit.
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error())
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows::Win32::{
            Foundation::HANDLE,
            Storage::FileSystem::{LockFileEx, LOCKFILE_EXCLUSIVE_LOCK, LOCKFILE_FAIL_IMMEDIATELY},
            System::IO::OVERLAPPED,
        };
        let mut overlapped = OVERLAPPED::default();
        unsafe {
            LockFileEx(
                HANDLE(file.as_raw_handle()),
                LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY,
                None,
                u32::MAX,
                u32::MAX,
                &mut overlapped,
            )
        }
        .map_err(std::io::Error::other)
    }
}

#[cfg(unix)]
fn unix_group_exited(group: i32) -> bool {
    // Zombies have already exited and cannot use the profile. Waiting for an
    // unrelated init process to reap them would keep a lease forever on some
    // containers. Live descendants, including a stopped one, keep the lease.
    if unsafe { libc::kill(-group, 0) } < 0
        && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
    {
        return true;
    }
    #[cfg(target_os = "linux")]
    {
        let Ok(entries) = fs::read_dir("/proc") else {
            return false;
        };
        for entry in entries.flatten() {
            let Some(pid) = entry
                .file_name()
                .to_str()
                .and_then(|s| s.parse::<i32>().ok())
            else {
                continue;
            };
            let Ok(stat) = fs::read_to_string(entry.path().join("stat")) else {
                if unsafe { libc::getpgid(pid) } == group {
                    return false;
                }
                continue;
            };
            let Some(fields) = stat
                .rsplit_once(')')
                .map(|(_, tail)| tail.split_whitespace().collect::<Vec<_>>())
            else {
                continue;
            };
            if fields.get(2).and_then(|p| p.parse::<i32>().ok()) == Some(group)
                && !matches!(fields.first().copied(), Some("Z" | "X"))
            {
                return false;
            }
        }
        true
    }
    #[cfg(target_os = "macos")]
    {
        // libproc returns a PID count (not a byte count) for this wrapper.
        let mut pids = vec![0i32; 4096];
        let count = unsafe {
            browser_list_process_group(
                group,
                pids.as_mut_ptr().cast(),
                std::mem::size_of_val(pids.as_slice()) as i32,
            )
        };
        if count <= 0 || count as usize >= pids.len() {
            return false;
        }
        for pid in pids.into_iter().take(count as usize) {
            let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
            let size = std::mem::size_of_val(&info) as i32;
            let read = unsafe {
                browser_process_info(
                    pid,
                    libc::PROC_PIDTBSDINFO,
                    0,
                    std::ptr::from_mut(&mut info).cast(),
                    size,
                )
            };
            if read != size {
                if unsafe { libc::getpgid(pid) } == group {
                    return false;
                }
                continue;
            }
            if info.pbi_pgid == group as u32 && info.pbi_status != libc::SZOMB {
                return false;
            }
        }
        true
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        false
    }
}
#[cfg(target_os = "macos")]
#[link(name = "proc")]
extern "C" {
    #[link_name = "proc_listpgrppids"]
    fn browser_list_process_group(
        group: libc::pid_t,
        buffer: *mut libc::c_void,
        size: libc::c_int,
    ) -> libc::c_int;
    #[link_name = "proc_pidinfo"]
    fn browser_process_info(
        pid: libc::c_int,
        flavor: libc::c_int,
        arg: u64,
        buffer: *mut libc::c_void,
        size: libc::c_int,
    ) -> libc::c_int;
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;
    #[cfg(unix)]
    #[tokio::test]
    async fn process_group_is_reaped_before_releasing_its_authority() {
        use std::os::unix::process::CommandExt;
        let root = tempfile::tempdir().unwrap();
        let marker = root.path().join("child-pid");
        let mut command = Command::new("/bin/sh");
        command
            .args([
                "-c",
                "sleep 30 & printf '%s' \"$!\" > \"$1\"; wait",
                "fixture",
            ])
            .arg(&marker)
            .process_group(0)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let child = command.spawn().unwrap();
        let mut process = Process { child };
        for _ in 0..100 {
            if marker.is_file() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(marker.is_file());
        assert!(!process.tree_exited());
        process.stop(Duration::ZERO);
        for _ in 0..100 {
            if process.tree_exited() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("A live descendant survived the supervised group termination");
    }
    #[tokio::test]
    async fn persistent_profile_has_one_lease_across_transport_instances() {
        let root = tempfile::tempdir().unwrap();
        let config = EngineConfig::production(
            root.path().join("resources"),
            root.path().join("data"),
            true,
        );
        let first = AgentBrowserTransport::new(config.clone());
        let second = AgentBrowserTransport::new(config.clone());
        first.reserve("conversation-a").unwrap();
        assert_eq!(
            second.reserve("conversation-b").unwrap_err().code,
            "browser_profile_busy"
        );
        fs::write(
            config.persistent_profile.join("synthetic-login-state"),
            b"retained",
        )
        .unwrap();
        first.terminate("conversation-a");
        second.reserve("new-run").unwrap();
        assert_eq!(
            fs::read(config.persistent_profile.join("synthetic-login-state")).unwrap(),
            b"retained"
        );
        second.shutdown();
    }
    #[tokio::test]
    async fn anonymous_profiles_are_capped_reaped_and_deleted_without_touching_login_profile() {
        let root = tempfile::tempdir().unwrap();
        let config = EngineConfig::production(
            root.path().join("resources"),
            root.path().join("data"),
            false,
        );
        fs::create_dir_all(&config.persistent_profile).unwrap();
        fs::write(
            config.persistent_profile.join("synthetic-login"),
            b"preserve",
        )
        .unwrap();
        let transport = AgentBrowserTransport::new(config.clone());
        transport.reserve("one").unwrap();
        transport.reserve("two").unwrap();
        assert_eq!(
            transport.reserve("three").unwrap_err().code,
            "browser_session_limit"
        );
        let slot = transport.slot("one").unwrap();
        let profile = slot.profile.clone();
        *slot.last_used.lock().unwrap() = Instant::now() - Duration::from_secs(301);
        transport.reap_idle();
        assert!(!profile.exists());
        assert!(config.persistent_profile.join("synthetic-login").exists());
        transport.reserve("three").unwrap();
        transport.shutdown();
        assert_eq!(fs::read_dir(&config.temporary_root).unwrap().count(), 0);
    }
}
