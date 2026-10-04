//! Isolated child-process runner for one-shot CLI calls.
//!
//! Every child runs in its own process group. On timeout the whole group is
//! killed (SIGKILL), and after a normal exit the group is swept too, so a CLI
//! that forks helpers (node workers, MCP servers) never leaves an orphan.
//! Output is capped so a runaway CLI cannot exhaust memory.

use std::{
    collections::BTreeMap,
    path::PathBuf,
    process::Stdio,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};

/// Upper bound kept from each of stdout and stderr (and from any output file
/// a CLI writes instead of stdout).
pub const MAX_CAPTURE_BYTES: usize = 8 * 1024 * 1024;
/// How long to keep draining pipes after the group is swept; only a helper
/// that escaped the process group could still hold a pipe open.
const PIPE_DRAIN_GRACE: Duration = Duration::from_millis(500);

type Captured = Arc<Mutex<Vec<u8>>>;
type Reader = (tokio::task::JoinHandle<()>, Captured);

#[derive(Clone, Debug)]
pub struct ProcessSpec {
    pub program: PathBuf,
    pub args: Vec<String>,
    /// The complete child environment (the parent's environment is cleared).
    pub env: BTreeMap<String, String>,
    pub cwd: PathBuf,
    pub stdin: Option<String>,
    pub timeout: Duration,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcessOutput {
    pub success: bool,
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProcessError {
    Spawn(String),
    Io(String),
    TimedOut,
}

pub async fn run(spec: ProcessSpec) -> Result<ProcessOutput, ProcessError> {
    let mut command = tokio::process::Command::new(&spec.program);
    command
        .args(&spec.args)
        .env_clear()
        .envs(&spec.env)
        .current_dir(&spec.cwd)
        .stdin(if spec.stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    command.process_group(0);

    let mut child = command
        .spawn()
        .map_err(|error| ProcessError::Spawn(error.to_string()))?;
    let group = child.id();

    if let (Some(input), Some(mut stdin)) = (spec.stdin, child.stdin.take()) {
        tokio::spawn(async move {
            // A CLI may exit before reading everything; a broken pipe is not
            // an error of ours.
            let _ = stdin.write_all(input.as_bytes()).await;
            let _ = stdin.shutdown().await;
        });
    }
    let stdout_task = child.stdout.take().map(spawn_reader);
    let stderr_task = child.stderr.take().map(spawn_reader);

    let status = match tokio::time::timeout(spec.timeout, child.wait()).await {
        Ok(Ok(status)) => status,
        Ok(Err(error)) => {
            kill_group(group);
            return Err(ProcessError::Io(error.to_string()));
        }
        Err(_) => {
            kill_group(group);
            let _ = child.kill().await;
            let _ = child.wait().await;
            for (task, _) in [stdout_task, stderr_task].into_iter().flatten() {
                task.abort();
            }
            return Err(ProcessError::TimedOut);
        }
    };
    // Sweep helpers the CLI left behind first: a helper that inherited stdout
    // would otherwise keep the pipe open and the answer would never reach EOF.
    kill_group(group);
    let stdout = collect(stdout_task).await;
    let stderr = collect(stderr_task).await;
    Ok(ProcessOutput {
        success: status.success(),
        code: status.code(),
        stdout,
        stderr,
    })
}

fn spawn_reader(pipe: impl AsyncRead + Unpin + Send + 'static) -> Reader {
    let captured: Captured = Arc::new(Mutex::new(Vec::new()));
    let task = tokio::spawn(read_capped(pipe, captured.clone()));
    (task, captured)
}

/// Waits briefly for EOF, then returns whatever was read; bytes already
/// captured are never discarded.
async fn collect(reader: Option<Reader>) -> String {
    let Some((mut task, captured)) = reader else {
        return String::new();
    };
    if tokio::time::timeout(PIPE_DRAIN_GRACE, &mut task)
        .await
        .is_err()
    {
        task.abort();
    }
    let bytes = captured
        .lock()
        .map(|bytes| bytes.clone())
        .unwrap_or_default();
    String::from_utf8_lossy(&bytes).into_owned()
}

async fn read_capped(mut pipe: impl AsyncRead + Unpin, captured: Captured) {
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        match pipe.read(&mut buffer).await {
            Ok(0) | Err(_) => break,
            Ok(read) => {
                let Ok(mut kept) = captured.lock() else {
                    break;
                };
                let room = MAX_CAPTURE_BYTES.saturating_sub(kept.len());
                kept.extend_from_slice(&buffer[..read.min(room)]);
            }
        }
    }
}

#[cfg(unix)]
fn kill_group(group: Option<u32>) {
    let Some(group) = group.and_then(|pid| i32::try_from(pid).ok()) else {
        return;
    };
    if group <= 1 {
        return;
    }
    // SAFETY: killpg only signals; the group id is the child's own pid because
    // it was spawned with `process_group(0)`, so this never reaches Clovy.
    unsafe {
        libc::killpg(group, libc::SIGKILL);
    }
}

#[cfg(not(unix))]
fn kill_group(_group: Option<u32>) {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[tokio::test]
    async fn llm_answer_survives_a_helper_that_keeps_stdout_open() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("helper.pid");
        // The CLI prints its answer, leaves a helper holding stdout, and exits.
        let script = format!(
            "sleep 60 &\necho $! > '{}'\necho 'final answer'\nexit 0",
            pid_file.display()
        );
        let started = Instant::now();
        let output = run(ProcessSpec {
            program: PathBuf::from("/bin/sh"),
            args: vec!["-c".to_string(), script],
            env: BTreeMap::from([("PATH".to_string(), "/usr/bin:/bin".to_string())]),
            cwd: dir.path().to_path_buf(),
            stdin: None,
            timeout: Duration::from_secs(10),
        })
        .await
        .unwrap();
        assert!(output.success);
        assert_eq!(output.stdout.trim(), "final answer");
        assert!(started.elapsed() < Duration::from_secs(5));
        let helper: i32 = std::fs::read_to_string(&pid_file)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        let mut alive = true;
        for _ in 0..50 {
            // SAFETY: signal 0 only checks for existence.
            alive = unsafe { libc::kill(helper, 0) } == 0;
            if !alive {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(!alive, "helper {helper} survived the call");
    }
}
