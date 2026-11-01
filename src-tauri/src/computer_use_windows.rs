//! Private inherited transport, authenticated Windows peers and job ownership.
//! Only task controllers enter a kill-on-close job. Application launch is
//! brokered by the existing interactive Explorer process outside that job.
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::{
    ffi::c_void,
    mem::{size_of, zeroed},
    os::windows::{ffi::OsStrExt, fs::OpenOptionsExt},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::process::{ChildStdin, ChildStdout, Command};
#[path = "computer_use_windows/process.rs"]
mod process;
use process::WindowsChild;
use windows::{
    core::{Interface, BOOL, BSTR, PCWSTR, PWSTR},
    Win32::{
        Foundation::{CloseHandle, FILETIME, HANDLE, HWND, LPARAM},
        Security::{
            GetSidSubAuthority, GetSidSubAuthorityCount, GetTokenInformation, TokenIntegrityLevel,
            WinTrust::*, TOKEN_MANDATORY_LABEL, TOKEN_QUERY,
        },
        System::{
            Com::{
                CoCreateInstance, CoInitializeEx, CoUninitialize, IServiceProvider,
                CLSCTX_LOCAL_SERVER, COINIT_APARTMENTTHREADED,
            },
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
                TH32CS_SNAPPROCESS,
            },
            JobObjects::{
                CreateJobObjectW, JobObjectBasicAccountingInformation,
                JobObjectExtendedLimitInformation, QueryInformationJobObject,
                SetInformationJobObject, TerminateJobObject,
                JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
                JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            },
            Registry::{
                RegGetValueW, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_EXPAND_SZ,
                RRF_RT_REG_SZ,
            },
            StationsAndDesktops::{
                CloseDesktop, GetProcessWindowStation, GetUserObjectInformationW, OpenInputDesktop,
                DESKTOP_ACCESS_FLAGS, DESKTOP_CONTROL_FLAGS, DESKTOP_HOOKCONTROL,
                DESKTOP_READOBJECTS, UOI_NAME,
            },
            Threading::{
                GetCurrentProcess, GetCurrentProcessId, GetProcessTimes, OpenProcess,
                OpenProcessToken, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
                PROCESS_QUERY_LIMITED_INFORMATION,
            },
            Variant::VARIANT,
        },
        UI::{
            Shell::{
                IShellBrowser, IShellDispatch2, IShellFolderViewDual, IShellWindows,
                SID_STopLevelBrowser, ShellWindows, SVGIO_BACKGROUND, SWC_DESKTOP,
                SWFO_NEEDDISPATCH,
            },
            WindowsAndMessaging::{
                EnumWindows, GetWindowThreadProcessId, IsIconic, IsWindow, SetForegroundWindow,
                ShowWindowAsync, SW_RESTORE, SW_SHOWNOACTIVATE,
            },
        },
    },
};

struct Handle(HANDLE);
unsafe impl Send for Handle {}
unsafe impl Sync for Handle {}
impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
}
#[derive(Clone)]
pub struct OwnedJob(Arc<Handle>);
impl OwnedJob {
    pub fn new() -> Result<Self> {
        unsafe {
            let job = Handle(CreateJobObjectW(None, PCWSTR::null())?);
            let info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION {
                BasicLimitInformation:
                    windows::Win32::System::JobObjects::JOBOBJECT_BASIC_LIMIT_INFORMATION {
                        LimitFlags: JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
                        ..zeroed()
                    },
                ..zeroed()
            };
            SetInformationJobObject(
                job.0,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const c_void,
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )?;
            // No BREAKAWAY_OK or SILENT_BREAKAWAY_OK: every controller and
            // overlay remains owned even if its parent exits unexpectedly.
            Ok(Self(Arc::new(job)))
        }
    }
    pub fn terminate(&self) {
        unsafe {
            let _ = TerminateJobObject(self.0 .0, 1);
        }
    }
    fn wait_empty(&self, timeout: Duration) {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            let mut info = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
            let result = unsafe {
                QueryInformationJobObject(
                    Some(self.0 .0),
                    JobObjectBasicAccountingInformation,
                    &mut info as *mut _ as *mut c_void,
                    size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                    None,
                )
            };
            if result.is_err() || info.ActiveProcesses == 0 || std::time::Instant::now() >= deadline
            {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
#[derive(Clone)]
pub struct JobCancel {
    job: OwnedJob,
    run_root: PathBuf,
}
impl JobCancel {
    pub fn cancel(&self) {
        crate::computer_use_cancel::cooperative_cancel(&self.run_root);
        self.job.terminate();
    }
}
pub(crate) fn spawn_in_job(command: Command, job: &OwnedJob) -> Result<WindowsChild> {
    process::spawn(command, job)
}

/// Deny writes and replacement from digest verification through process
/// creation/resume, binding the bytes actually loaded to the signed owner.
pub(crate) fn lock_image(path: &Path, expected: Option<&str>) -> Result<std::fs::File> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let expected = expected.context("The signed owner has no compiled executable digest")?;
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(windows::Win32::Storage::FileSystem::FILE_SHARE_READ.0)
        .open(path)?;
    let mut hash = Sha256::new();
    let mut bytes = [0u8; 64 * 1024];
    loop {
        let count = file.read(&mut bytes)?;
        if count == 0 {
            break;
        }
        hash.update(&bytes[..count]);
    }
    if format!("{:x}", hash.finalize()) != expected {
        bail!("Executable bytes do not match the signed owner's compiled digest");
    }
    Ok(file)
}

pub struct PrivateHelper {
    pub stdin: Option<ChildStdin>,
    pub stdout: Option<ChildStdout>,
    child: WindowsChild,
    _run_dir: tempfile::TempDir,
    job: OwnedJob,
    pid: u32,
}
impl PrivateHelper {
    pub async fn spawn(path: &Path, capability: &str) -> Result<Self> {
        if capability.len() < 32 {
            bail!("Missing private helper capability");
        }
        let _image_lock = lock_image(path, option_env!("CLOVY_COMPUTER_USE_HELPER_SHA256"))?;
        verify_signed_peer(path)?;
        readiness()?;
        let job = OwnedJob::new()?;
        let run_dir = tempfile::Builder::new()
            .prefix("clovy-desktop-run-")
            .tempdir()?;
        let input = process::listener(true)?;
        let output = process::listener(false)?;
        let mut command = Command::new(path);
        command
            .arg("mcp")
            .env("CLOVY_COMPUTER_USE_HELPER_CAPABILITY", capability)
            .env("CLOVY_COMPUTER_USE_RUN_DIR", run_dir.path())
            .env("CLOVY_COMPUTER_USE_INPUT_PIPE", &input.name)
            .env("CLOVY_COMPUTER_USE_OUTPUT_PIPE", &output.name)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null());
        struct StartupGuard(OwnedJob, bool);
        impl Drop for StartupGuard {
            fn drop(&mut self) {
                if self.1 {
                    self.0.terminate();
                }
            }
        }
        let mut startup = StartupGuard(job.clone(), true);
        let child = process::spawn_helper(command, &job)?;
        let pid = child.id().context("Missing helper process identity")?;
        let (input, output) = tokio::task::spawn_blocking(move || -> Result<_> {
            Ok((process::accept(input, pid)?, process::accept(output, pid)?))
        })
        .await??;
        let stdin = ChildStdin::from_std(std::process::ChildStdin::from(input))?;
        let stdout = ChildStdout::from_std(std::process::ChildStdout::from(output))?;
        startup.1 = false;
        Ok(Self {
            stdin: Some(stdin),
            stdout: Some(stdout),
            child,
            _run_dir: run_dir,
            job,
            pid,
        })
    }
    pub fn pid(&self) -> u32 {
        self.pid
    }
    pub fn cancel_handle(&self) -> JobCancel {
        JobCancel {
            job: self.job.clone(),
            run_root: self._run_dir.path().to_path_buf(),
        }
    }
    pub async fn stop(mut self) {
        self.cancel_handle().cancel();
        let _ = tokio::time::timeout(Duration::from_secs(2), self.child.wait()).await;
    }
}
impl Drop for PrivateHelper {
    fn drop(&mut self) {
        self.cancel_handle().cancel();
        // Wait for every controller to release its files before TempDir is
        // removed on Windows. This never waits for user applications.
        self.job.wait_empty(Duration::from_secs(2));
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcessIdentity {
    pub path: PathBuf,
    pub creation_time: u64,
}
pub fn connect_private_transport() -> Result<(ChildStdout, ChildStdin)> {
    process::connect_private_transport()
}
pub fn process_identity(pid: u32) -> Result<ProcessIdentity> {
    unsafe {
        let process = Handle(OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid)?);
        let mut path = vec![0u16; 32768];
        let mut len = path.len() as u32;
        QueryFullProcessImageNameW(
            process.0,
            PROCESS_NAME_WIN32,
            PWSTR(path.as_mut_ptr()),
            &mut len,
        )?;
        let mut created = FILETIME::default();
        let mut exited = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        GetProcessTimes(process.0, &mut created, &mut exited, &mut kernel, &mut user)?;
        Ok(ProcessIdentity {
            path: PathBuf::from(String::from_utf16(&path[..len as usize])?),
            creation_time: ((created.dwHighDateTime as u64) << 32) | created.dwLowDateTime as u64,
        })
    }
}
fn same_path(left: &Path, right: &Path) -> bool {
    match (std::fs::canonicalize(left), std::fs::canonicalize(right)) {
        (Ok(a), Ok(b)) => a
            .as_os_str()
            .to_string_lossy()
            .eq_ignore_ascii_case(&b.as_os_str().to_string_lossy()),
        _ => false,
    }
}
fn parent_pid(pid: u32) -> Result<u32> {
    unsafe {
        let snapshot = Handle(CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)?);
        let mut entry = PROCESSENTRY32W {
            dwSize: size_of::<PROCESSENTRY32W>() as u32,
            ..zeroed()
        };
        Process32FirstW(snapshot.0, &mut entry)?;
        loop {
            if entry.th32ProcessID == pid {
                return Ok(entry.th32ParentProcessID);
            }
            if Process32NextW(snapshot.0, &mut entry).is_err() {
                break;
            }
        }
    }
    bail!("Missing helper parent")
}
pub fn verify_parent() -> Result<()> {
    let this = std::env::current_exe()?;
    let parent = process_identity(parent_pid(unsafe { GetCurrentProcessId() })?)?;
    let packaged = this
        .ancestors()
        .skip(1)
        .map(|directory| directory.join("clovy.exe"))
        .find(|path| path.is_file());
    let expected = if packaged
        .as_ref()
        .is_some_and(|expected| same_path(&parent.path, expected))
    {
        packaged.expect("packaged path checked")
    } else if cfg!(debug_assertions) && development_main_path(&parent.path) {
        parent.path.clone()
    } else {
        bail!("Only the exact bundled Clovy process may launch this helper");
    };
    // Validate the still-live parent rather than trusting a PID in argv or env.
    if parent.creation_time >= process_identity(unsafe { GetCurrentProcessId() })?.creation_time {
        bail!("Invalid parent creation identity");
    }
    verify_same_signer(&expected, &this)?;
    readiness()
}
fn development_main_path(path: &Path) -> bool {
    let target = Path::new(env!("CARGO_MANIFEST_DIR")).join("target");
    let Ok(relative) = path.strip_prefix(&target) else {
        return false;
    };
    let components: Vec<_> = relative.components().collect();
    if !matches!(components.len(), 2 | 3)
        || components
            .iter()
            .any(|c| !matches!(c, std::path::Component::Normal(_)))
    {
        return false;
    }
    if components.len() == 3
        && !components[0].as_os_str().to_str().is_some_and(|triple| {
            triple.ends_with("-pc-windows-msvc") || triple.ends_with("-pc-windows-gnu")
        })
    {
        return false;
    }
    components.last().is_some_and(|name| {
        name.as_os_str()
            .to_string_lossy()
            .eq_ignore_ascii_case("clovy.exe")
    }) && path
        .parent()
        .is_some_and(|profile| profile.join(".cargo-lock").is_file())
}
pub fn verify_signed_peer(path: &Path) -> Result<()> {
    verify_same_signer(&std::env::current_exe()?, path)
}
pub fn verify_same_signer(a: &Path, b: &Path) -> Result<()> {
    if !a.is_absolute() || !b.is_absolute() {
        bail!("Executable identities must be absolute");
    }
    let left = verified_signer(a)?;
    let right = verified_signer(b)?;
    if left != right {
        bail!("The helper and Clovy have different Authenticode publishers");
    }
    Ok(())
}
fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}
fn verified_signer(path: &Path) -> Result<Vec<u8>> {
    let path = wide(path);
    let mut file = WINTRUST_FILE_INFO {
        cbStruct: size_of::<WINTRUST_FILE_INFO>() as u32,
        pcwszFilePath: PCWSTR(path.as_ptr()),
        ..Default::default()
    };
    let mut data = WINTRUST_DATA {
        cbStruct: size_of::<WINTRUST_DATA>() as u32,
        dwUIChoice: WTD_UI_NONE,
        fdwRevocationChecks: WTD_REVOKE_NONE,
        dwUnionChoice: WTD_CHOICE_FILE,
        Anonymous: WINTRUST_DATA_0 { pFile: &mut file },
        dwStateAction: WTD_STATEACTION_VERIFY,
        dwProvFlags: WTD_CACHE_ONLY_URL_RETRIEVAL | WTD_DISABLE_MD2_MD4,
        ..Default::default()
    };
    let mut action = WINTRUST_ACTION_GENERIC_VERIFY_V2;
    unsafe {
        let result = WinVerifyTrust(
            HWND::default(),
            &mut action,
            &mut data as *mut _ as *mut c_void,
        );
        let signer = if result == 0 {
            let provider = WTHelperProvDataFromStateData(data.hWVTStateData);
            let signer = if provider.is_null() {
                std::ptr::null_mut()
            } else {
                WTHelperGetProvSignerFromChain(provider, 0, false, 0)
            };
            let certificate = if signer.is_null() {
                std::ptr::null_mut()
            } else {
                WTHelperGetProvCertFromChain(signer, 0)
            };
            if certificate.is_null() || (*certificate).pCert.is_null() {
                None
            } else {
                let certificate = &*(*certificate).pCert;
                Some(
                    std::slice::from_raw_parts(
                        certificate.pbCertEncoded,
                        certificate.cbCertEncoded as usize,
                    )
                    .to_vec(),
                )
            }
        } else {
            None
        };
        data.dwStateAction = WTD_STATEACTION_CLOSE;
        let _ = WinVerifyTrust(
            HWND::default(),
            &mut action,
            &mut data as *mut _ as *mut c_void,
        );
        signer.context("Authenticode verification failed or signer is missing")
    }
}

/// Refuse locked/secure desktops and elevated helpers. We never request UAC,
/// switch desktops, alter integrity, or synthesize macOS permission concepts.
pub fn readiness() -> Result<()> {
    unsafe {
        let station = GetProcessWindowStation()?;
        let mut station_name = [0u16; 256];
        let mut station_needed = 0;
        GetUserObjectInformationW(
            HANDLE(station.0),
            UOI_NAME,
            Some(station_name.as_mut_ptr().cast()),
            (station_name.len() * 2) as u32,
            Some(&mut station_needed),
        )?;
        let station_end = station_name
            .iter()
            .position(|c| *c == 0)
            .unwrap_or(station_name.len());
        if !String::from_utf16_lossy(&station_name[..station_end]).eq_ignore_ascii_case("WinSta0") {
            bail!("Computer use requires the interactive user window station");
        }
        let desktop = OpenInputDesktop(
            DESKTOP_CONTROL_FLAGS(0),
            false,
            DESKTOP_ACCESS_FLAGS(DESKTOP_READOBJECTS.0 | DESKTOP_HOOKCONTROL.0),
        )?;
        let mut name = [0u16; 256];
        let mut needed = 0;
        let read = GetUserObjectInformationW(
            HANDLE(desktop.0),
            UOI_NAME,
            Some(name.as_mut_ptr().cast()),
            (name.len() * 2) as u32,
            Some(&mut needed),
        );
        let _ = CloseDesktop(desktop);
        read?;
        let end = name.iter().position(|c| *c == 0).unwrap_or(name.len());
        if !String::from_utf16_lossy(&name[..end]).eq_ignore_ascii_case("Default") {
            bail!("The interactive desktop is locked or secure");
        }
        let mut token = HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token)?;
        let token = Handle(token);
        let mut needed = 0;
        let _ = GetTokenInformation(token.0, TokenIntegrityLevel, None, 0, &mut needed);
        let mut bytes = vec![0usize; (needed as usize).div_ceil(size_of::<usize>())];
        GetTokenInformation(
            token.0,
            TokenIntegrityLevel,
            Some(bytes.as_mut_ptr().cast()),
            (bytes.len() * size_of::<usize>()) as u32,
            &mut needed,
        )?;
        let label = &*(bytes.as_ptr() as *const TOKEN_MANDATORY_LABEL);
        let count = *GetSidSubAuthorityCount(label.Label.Sid);
        if count == 0 || *GetSidSubAuthority(label.Label.Sid, (count - 1) as u32) >= 0x3000 {
            bail!("Computer use requires a non-elevated interactive process");
        }
    }
    Ok(())
}

#[derive(Debug)]
struct DesktopUnavailable(anyhow::Error);
impl std::fmt::Display for DesktopUnavailable {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.0, formatter)
    }
}
impl std::error::Error for DesktopUnavailable {}
fn require_desktop() -> Result<()> {
    readiness().map_err(|error| DesktopUnavailable(error).into())
}
fn desktop_unavailable(error: &anyhow::Error) -> bool {
    error.downcast_ref::<DesktopUnavailable>().is_some()
}

pub fn permission_result() -> Value {
    let ready = readiness();
    let accessible = ready.is_ok();
    json!({"content":[{"type":"text","text":match ready {Ok(())=>"The standard interactive Windows desktop is ready.".to_owned(),Err(error)=>error.to_string()}}],"structuredContent":{"accessibility":accessible,"screen_recording":accessible,"screen_recording_capturable":accessible,"interactive_desktop":accessible,"platform":"windows","code":if accessible{Value::Null}else{json!("computer_use_desktop_unavailable")},"disposition":{"delivery":"not_delivered","retry":"safe"},"source":{"attribution":"driver-process","pid":std::process::id(),"executable":std::env::current_exe().ok().map(|p|p.to_string_lossy().into_owned()).unwrap_or_default()}},"isError":!accessible})
}
pub fn focus_window_result(args: &Value) -> Value {
    let mut dispatched = false;
    let result = (|| -> Result<(u32, u64, bool, bool)> {
        require_desktop()?;
        let pid = u32::try_from(args["pid"].as_u64().context("Missing target PID")?)?;
        let id = args["window_id"]
            .as_u64()
            .context("Missing target window")?;
        let hwnd = HWND(usize::try_from(id)? as *mut c_void);
        let mut actual = 0;
        unsafe {
            if !IsWindow(Some(hwnd)).as_bool() {
                bail!("Target window disappeared");
            }
            GetWindowThreadProcessId(hwnd, Some(&mut actual));
        }
        if pid == 0 || pid != actual {
            bail!("Target window identity changed");
        }
        let expected = args["expected_process_instance"]
            .as_str()
            .context("Missing expected process generation")?;
        let process =
            unsafe { Handle(OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid)?) };
        let mut created = FILETIME::default();
        let mut exited = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        unsafe {
            GetProcessTimes(process.0, &mut created, &mut exited, &mut kernel, &mut user)?;
        }
        let ticks = ((created.dwHighDateTime as u64) << 32) | created.dwLowDateTime as u64;
        if expected
            != format!(
                "windows-proc-v1:{}:{}",
                ticks / 10_000_000,
                ticks % 10_000_000
            )
        {
            bail!("The authorized process generation changed");
        }
        unsafe {
            GetWindowThreadProcessId(hwnd, Some(&mut actual));
        }
        if actual != pid || !unsafe { IsWindow(Some(hwnd)) }.as_bool() {
            bail!("The authorized window identity changed");
        }
        require_desktop()?;
        if crate::computer_use_cancel::current_run_cancelled() {
            bail!("Computer use was stopped before window restoration");
        }
        let restored = unsafe { IsIconic(hwnd) }.as_bool()
            && unsafe { ShowWindowAsync(hwnd, SW_RESTORE) }.as_bool();
        dispatched |= restored;
        unsafe {
            GetWindowThreadProcessId(hwnd, Some(&mut actual));
        }
        if actual != pid || !unsafe { IsWindow(Some(hwnd)) }.as_bool() {
            bail!("The authorized window changed during restoration");
        }
        require_desktop()?;
        if crate::computer_use_cancel::current_run_cancelled() {
            bail!("Computer use was stopped before focusing the window");
        }
        let focused = unsafe { SetForegroundWindow(hwnd) }.as_bool();
        dispatched |= focused;
        Ok((pid, id, focused, restored))
    })();
    match result {
        Ok((pid, window_id, raised, restored)) => {
            json!({"content":[{"type":"text","text":if raised{"Focused the authorized window."}else{"Windows refused to focus the authorized window."}}],"structuredContent":{"pid":pid,"window_id":window_id,"raised":raised,"restored":restored,"disposition":{"delivery":if dispatched{"delivered_unverified"}else{"not_delivered"},"retry":if dispatched{"unsafe"}else{"safe"}}},"isError":!raised})
        }
        Err(error) => {
            json!({"content":[{"type":"text","text":error.to_string()}],"structuredContent":{"raised":false,"code":if desktop_unavailable(&error){json!("computer_use_desktop_unavailable")}else{Value::Null},"disposition":{"delivery":if dispatched{"delivered_unverified"}else{"not_delivered"},"retry":if dispatched{"unsafe"}else{"safe"}}},"isError":true})
        }
    }
}

/// Resolve an installed application before handing the launch to Explorer.
/// Arguments and arbitrary command lines are never accepted from the model.
fn resolve_app(name: &str) -> Result<PathBuf> {
    let name = name.trim();
    if name.is_empty()
        || name.len() > 200
        || name
            .chars()
            .any(|c| c == '\0' || c == '/' || c == '\\' || c == ':' || c == '"')
    {
        bail!("Use the installed application's display name");
    }
    let executable = if name.to_ascii_lowercase().ends_with(".exe") {
        name.to_owned()
    } else {
        format!("{name}.exe")
    };
    let key: Vec<u16> =
        format!("Software\\Microsoft\\Windows\\CurrentVersion\\App Paths\\{executable}")
            .encode_utf16()
            .chain(Some(0))
            .collect();
    for hive in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
        let mut value = vec![0u16; 32768];
        let mut bytes = (value.len() * 2) as u32;
        let result = unsafe {
            RegGetValueW(
                hive,
                PCWSTR(key.as_ptr()),
                PCWSTR::null(),
                RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ,
                None,
                Some(value.as_mut_ptr().cast()),
                Some(&mut bytes),
            )
        };
        if result.is_ok() {
            let end = value.iter().position(|v| *v == 0).unwrap_or(value.len());
            let path = PathBuf::from(String::from_utf16(&value[..end])?.trim_matches('"'));
            if path.is_absolute() && path.is_file() {
                return Ok(std::fs::canonicalize(path)?);
            }
        }
    }
    // GetSystemDirectoryW cannot be redirected by a model-supplied PATH or
    // environment variable. Bare names only search this fixed system folder.
    let mut system = [0u16; 32768];
    let length = unsafe {
        windows::Win32::System::SystemInformation::GetSystemDirectoryW(Some(&mut system))
    };
    if length > 0 && (length as usize) < system.len() {
        let path = PathBuf::from(String::from_utf16(&system[..length as usize])?).join(&executable);
        if path.is_file() {
            return Ok(std::fs::canonicalize(path)?);
        }
    }
    // Display names usually live in Start Menu shortcuts. Resolve links with
    // IShellLink rather than executing their names through a shell parser.
    let roots = [
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .map(|p| p.join("Microsoft/Windows/Start Menu/Programs")),
        std::env::var_os("PROGRAMDATA")
            .map(PathBuf::from)
            .map(|p| p.join("Microsoft/Windows/Start Menu/Programs")),
    ];
    for root in roots.into_iter().flatten() {
        let mut stack = vec![(root, 0usize)];
        let mut visited = 0;
        while let Some((directory, depth)) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(directory) else {
                continue;
            };
            for entry in entries.flatten() {
                visited += 1;
                if visited > 4096 {
                    break;
                }
                let path = entry.path();
                let metadata = entry.file_type()?;
                if metadata.is_symlink() {
                    continue;
                }
                if metadata.is_dir() && depth < 5 {
                    stack.push((path, depth + 1));
                    continue;
                }
                if path
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("lnk"))
                    && path
                        .file_stem()
                        .is_some_and(|stem| stem.to_string_lossy().eq_ignore_ascii_case(name))
                {
                    return resolve_shortcut(&path);
                }
            }
            if visited > 4096 {
                break;
            }
        }
    }
    bail!("No installed application matches that display name")
}
fn resolve_shortcut(path: &Path) -> Result<PathBuf> {
    use windows::Win32::{
        Storage::FileSystem::WIN32_FIND_DATAW,
        System::Com::{IPersistFile, CLSCTX_INPROC_SERVER, STGM_READ},
        UI::Shell::{IShellLinkW, ShellLink, SLGP_RAWPATH},
    };
    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
        let persist: IPersistFile = link.cast()?;
        let encoded = wide(path);
        persist.Load(PCWSTR(encoded.as_ptr()), STGM_READ)?;
        let mut target = [0u16; 32768];
        let mut data = WIN32_FIND_DATAW::default();
        link.GetPath(&mut target, &mut data, SLGP_RAWPATH.0 as u32)?;
        let end = target
            .iter()
            .position(|v| *v == 0)
            .context("Invalid shortcut path")?;
        let target = PathBuf::from(String::from_utf16(&target[..end])?);
        if !target.is_absolute() || !target.is_file() {
            bail!("The installed shortcut has no available executable");
        }
        let mut arguments = [0u16; 32768];
        link.GetArguments(&mut arguments)?;
        if arguments[0] != 0 {
            bail!(
                "This shortcut requires launch arguments; open its registered executable by name"
            );
        }
        Ok(std::fs::canonicalize(target)?)
    }
}
struct ComApartment;
impl ComApartment {
    fn enter() -> Result<Self> {
        unsafe {
            CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
        }
        Ok(Self)
    }
}
impl Drop for ComApartment {
    fn drop(&mut self) {
        unsafe {
            CoUninitialize();
        }
    }
}

pub async fn launch_app(name: &str) -> Result<Value> {
    let name = name.to_owned();
    tokio::task::spawn_blocking(move || {
        // Every ordinary Err from the worker precedes ShellExecute. Once a
        // dispatch is attempted the worker returns an explicit uncertain
        // receipt instead. A worker panic/join failure remains uncertain to
        // its caller, since it can occur on either side of dispatch.
        launch_app_blocking(&name).unwrap_or_else(|error|json!({"version":"2.5","ok":false,"command":"launch","error":{"code":if desktop_unavailable(&error){"computer_use_desktop_unavailable"}else{"computer_use_launch_unavailable"},"message":error.to_string(),"disposition":{"delivery":"not_delivered","retry":"safe"}}}))
    }).await.map_err(Into::into)
}
fn launch_app_blocking(name: &str) -> Result<Value> {
    require_desktop()?;
    let _apartment = ComApartment::enter()?;
    let path = resolve_app(name)?;
    let _launch_image_lock = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(windows::Win32::Storage::FileSystem::FILE_SHARE_READ.0)
        .open(&path)?;
    if !path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"))
    {
        bail!("The installed application target must be an executable, not a script or document");
    }
    let encoded = wide(&path);
    let mut binary_type = 0;
    unsafe {
        windows::Win32::Storage::FileSystem::GetBinaryTypeW(
            PCWSTR(encoded.as_ptr()),
            &mut binary_type,
        )?;
    }
    if !matches!(binary_type, 0 | 6) {
        bail!("The installed target is not a native Windows application");
    }
    if crate::computer_use_policy::blocked_app(name)
        || path
            .file_name()
            .and_then(|file| file.to_str())
            .is_none_or(crate::computer_use_policy::blocked_app)
    {
        return Ok(
            json!({"version":"2.5","ok":false,"command":"launch","error":{"code":"computer_use_target_blocked","message":"Clovy cannot open this protected application through Computer use.","disposition":{"delivery":"not_delivered","retry":"safe"}}}),
        );
    }
    if let Err(error) = verify_ordinary_run_level(&path) {
        return Ok(
            json!({"version":"2.5","ok":false,"command":"launch","error":{"code":"ACTION_NOT_SUPPORTED","message":format!("Clovy cannot automatically open an app without a declared standard-user execution level. Open it manually, then select its window. {error}"),"disposition":{"delivery":"not_delivered","retry":"safe"}}}),
        );
    }
    // Use the Shell.Application owned by the desktop Explorer process. A
    // local ShellExecute/CreateProcess call would place the user's new app in
    // the helper job and Stop would incorrectly close it.
    unsafe {
        let shell: IShellWindows = CoCreateInstance(&ShellWindows, None, CLSCTX_LOCAL_SERVER)?;
        let empty = VARIANT::default();
        let mut desktop_hwnd = 0;
        let desktop = shell.FindWindowSW(
            &empty,
            &empty,
            SWC_DESKTOP,
            &mut desktop_hwnd,
            SWFO_NEEDDISPATCH,
        )?;
        let services: IServiceProvider = desktop.cast()?;
        let browser: IShellBrowser = services.QueryService(&SID_STopLevelBrowser)?;
        let view = browser.QueryActiveShellView()?;
        let folder: IShellFolderViewDual = view.GetItemObject(SVGIO_BACKGROUND)?;
        let application: IShellDispatch2 = folder.Application()?.cast()?;
        let file = BSTR::from(path.as_os_str().to_string_lossy().as_ref());
        let operation = VARIANT::from("open");
        let show = VARIANT::from(SW_SHOWNOACTIVATE.0);
        require_desktop()?;
        if crate::computer_use_cancel::current_run_cancelled() {
            bail!("Computer use was stopped before application launch");
        }
        // Failure from this point cannot safely imply that no launch happened.
        if let Err(error) = application.ShellExecute(&file, &empty, &empty, &operation, &show) {
            return Ok(
                json!({"version":"2.5","ok":false,"command":"launch","error":{"code":"LAUNCH_UNCERTAIN","message":format!("Explorer did not confirm launch: {error}"),"disposition":{"delivery":"delivery_uncertain","retry":"unsafe"}}}),
            );
        }
    }
    let deadline = std::time::Instant::now() + Duration::from_secs(8);
    loop {
        if let Some(pid) = find_process_for_image(&path) {
            if let Ok(identity) = process_identity(pid) {
                let window = find_window(pid);
                return Ok(
                    json!({"version":"2.5","ok":true,"command":"launch","data":{"pid":pid,"app":name,"executable_path":identity.path,"process_creation_time":identity.creation_time,"window_id":window,"disposition":{"delivery":"delivered_unverified","retry":"unsafe"}}}),
                );
            }
        }
        if std::time::Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Ok(
        json!({"version":"2.5","ok":false,"command":"launch","error":{"code":"LAUNCH_UNCERTAIN","message":"Explorer accepted the launch but the exact application process was not observed.","disposition":{"delivery":"delivery_uncertain","retry":"unsafe"}}}),
    )
}
fn find_process_for_image(path: &Path) -> Option<u32> {
    unsafe {
        let snapshot = Handle(CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).ok()?);
        let mut entry = PROCESSENTRY32W {
            dwSize: size_of::<PROCESSENTRY32W>() as u32,
            ..zeroed()
        };
        Process32FirstW(snapshot.0, &mut entry).ok()?;
        loop {
            if process_identity(entry.th32ProcessID)
                .is_ok_and(|identity| same_path(&identity.path, path))
            {
                return Some(entry.th32ProcessID);
            }
            if Process32NextW(snapshot.0, &mut entry).is_err() {
                break;
            }
        }
    }
    None
}
fn find_window(pid: u32) -> Option<u64> {
    struct Search {
        pid: u32,
        window: Option<u64>,
    }
    unsafe extern "system" fn visit(hwnd: HWND, param: LPARAM) -> BOOL {
        let search = unsafe { &mut *(param.0 as *mut Search) };
        let mut actual = 0;
        unsafe {
            GetWindowThreadProcessId(hwnd, Some(&mut actual));
        }
        if actual == search.pid {
            search.window = Some(hwnd.0 as usize as u64);
            BOOL(0)
        } else {
            BOOL(1)
        }
    }
    let mut search = Search { pid, window: None };
    let _ = unsafe { EnumWindows(Some(visit), LPARAM(&mut search as *mut _ as isize)) };
    search.window
}

/// The QA bridge accepts only our signed synthetic target, never an arbitrary
/// same-publisher program copied into the fixture filename.
pub(crate) fn verify_fixture(path: &Path) -> Result<()> {
    use windows::Win32::Storage::FileSystem::{
        GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
    };
    let _image_lock = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(windows::Win32::Storage::FileSystem::FILE_SHARE_READ.0)
        .open(path)?;
    verify_signed_peer(path)?;
    let path = wide(path);
    let length = unsafe { GetFileVersionInfoSizeW(PCWSTR(path.as_ptr()), None) } as usize;
    if length == 0 || length > 2 * 1024 * 1024 {
        bail!("The synthetic fixture has no bounded version identity");
    }
    let mut data = vec![0u8; length];
    unsafe {
        GetFileVersionInfoW(
            PCWSTR(path.as_ptr()),
            None,
            length as u32,
            data.as_mut_ptr().cast(),
        )?;
    }
    let query = |name: &str| -> Result<(*mut c_void, usize)> {
        let encoded: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
        let mut output = std::ptr::null_mut();
        let mut count = 0;
        if !unsafe {
            VerQueryValueW(
                data.as_ptr().cast(),
                PCWSTR(encoded.as_ptr()),
                &mut output,
                &mut count,
            )
        }
        .as_bool()
        {
            bail!("Synthetic fixture version field is missing");
        }
        Ok((output, count as usize))
    };
    let bounded = |pointer: *mut c_void, bytes: usize| -> bool {
        let start = pointer as usize;
        let base = data.as_ptr() as usize;
        start >= base
            && start
                .checked_add(bytes)
                .is_some_and(|end| end <= base + data.len())
    };
    let (translations, count) = query(r"\VarFileInfo\Translation")?;
    if !(4..=256).contains(&count) || !count.is_multiple_of(4) || !bounded(translations, count) {
        bail!("Invalid synthetic fixture language identity");
    }
    const MARKER: &str = "Clovy Computer Use Synthetic Fixture";
    for index in (0..count).step_by(4) {
        let language = unsafe {
            std::ptr::read_unaligned((translations as *const u8).add(index).cast::<u16>())
        };
        let codepage = unsafe {
            std::ptr::read_unaligned((translations as *const u8).add(index + 2).cast::<u16>())
        };
        let mut matches = true;
        for field in ["ProductName", "FileDescription"] {
            let (text, count) = query(&format!(
                r"\StringFileInfo\{language:04x}{codepage:04x}\{field}"
            ))?;
            if count == 0 || count > 1024 || !bounded(text, count * 2) {
                bail!("Invalid counted synthetic fixture identity");
            }
            let characters: Vec<u16> = (0..count)
                .map(|offset| unsafe {
                    std::ptr::read_unaligned((text as *const u8).add(offset * 2).cast::<u16>())
                })
                .collect();
            if characters.last() != Some(&0)
                || characters[..count - 1].contains(&0)
                || String::from_utf16(&characters[..count - 1])? != MARKER
            {
                matches = false;
                break;
            }
        }
        if matches {
            return Ok(());
        }
    }
    bail!("The signed program is not Clovy's synthetic Computer use fixture")
}

fn verify_ordinary_run_level(path: &Path) -> Result<()> {
    use windows::Win32::System::{
        ApplicationInstallationAndServicing::{
            CreateActCtxW, QueryActCtxW, ReleaseActCtx, ACTCTXW, ACTCTX_RUN_LEVEL_AS_INVOKER,
            ACTIVATION_CONTEXT_RUN_LEVEL_INFORMATION,
        },
        SystemServices::RunlevelInformationInActivationContext,
        WindowsProgramming::ACTCTX_FLAG_RESOURCE_NAME_VALID,
    };
    struct Context(HANDLE);
    impl Drop for Context {
        fn drop(&mut self) {
            unsafe {
                ReleaseActCtx(self.0);
            }
        }
    }
    let source = wide(path);
    // MAKEINTRESOURCEW(1): Windows interprets the low word as the manifest
    // resource ordinal. This is a tagged integer and is never dereferenced.
    #[allow(clippy::manual_dangling_ptr)]
    let manifest_resource = PCWSTR(1usize as *const u16);
    let config = ACTCTXW {
        cbSize: size_of::<ACTCTXW>() as u32,
        dwFlags: ACTCTX_FLAG_RESOURCE_NAME_VALID,
        lpSource: PCWSTR(source.as_ptr()),
        lpResourceName: manifest_resource,
        ..Default::default()
    };
    let context = Context(unsafe { CreateActCtxW(&config)? });
    let mut level = ACTIVATION_CONTEXT_RUN_LEVEL_INFORMATION::default();
    let mut written = 0;
    unsafe {
        QueryActCtxW(
            0,
            context.0,
            None,
            RunlevelInformationInActivationContext.0 as u32,
            Some((&mut level as *mut ACTIVATION_CONTEXT_RUN_LEVEL_INFORMATION).cast()),
            size_of::<ACTIVATION_CONTEXT_RUN_LEVEL_INFORMATION>(),
            Some(&mut written),
        )?;
    }
    if written < size_of::<ACTIVATION_CONTEXT_RUN_LEVEL_INFORMATION>()
        || level.RunLevel != ACTCTX_RUN_LEVEL_AS_INVOKER
        || level.UiAccess != 0
    {
        bail!("The executable requests an unknown, elevated or UI-access execution level");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn process_identity_tracks_creation_and_image() {
        let pid = unsafe { GetCurrentProcessId() };
        let first = process_identity(pid).unwrap();
        assert!(first.creation_time > 0);
        assert!(same_path(&first.path, &std::env::current_exe().unwrap()));
        assert_eq!(first, process_identity(pid).unwrap());
    }
    #[test]
    fn untrusted_program_arguments_cannot_be_launch_names() {
        for name in [
            "cmd.exe /c anything",
            "../notepad.exe",
            "C:\\Windows\\notepad.exe",
            "",
            "a\0b",
        ] {
            assert!(resolve_app(name).is_err());
        }
    }
    #[tokio::test]
    async fn job_kills_suspended_assigned_descendants() {
        let job = OwnedJob::new().unwrap();
        let mut command = Command::new("C:\\Windows\\System32\\cmd.exe");
        command.args(["/d", "/c", "ping -n 60 127.0.0.1 > nul"]);
        let mut child = spawn_in_job(command, &job).unwrap();
        let parent = child.id().unwrap();
        let descendant = tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let found = unsafe {
                    let snapshot = Handle(CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).unwrap());
                    let mut entry = PROCESSENTRY32W {
                        dwSize: size_of::<PROCESSENTRY32W>() as u32,
                        ..zeroed()
                    };
                    Process32FirstW(snapshot.0, &mut entry).unwrap();
                    let mut found = None;
                    loop {
                        if entry.th32ParentProcessID == parent {
                            found = Some(Handle(
                                OpenProcess(
                                    windows::Win32::System::Threading::PROCESS_SYNCHRONIZE,
                                    false,
                                    entry.th32ProcessID,
                                )
                                .unwrap(),
                            ));
                            break;
                        }
                        if Process32NextW(snapshot.0, &mut entry).is_err() {
                            break;
                        }
                    }
                    found
                };
                if let Some(handle) = found {
                    break handle;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        job.terminate();
        assert_eq!(
            unsafe { windows::Win32::System::Threading::WaitForSingleObject(descendant.0, 2000) },
            windows::Win32::Foundation::WAIT_OBJECT_0
        );
        assert!(!tokio::time::timeout(Duration::from_secs(2), child.wait())
            .await
            .unwrap()
            .unwrap()
            .success());
    }
}
