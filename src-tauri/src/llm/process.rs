//! Isolated child-process runner for one-shot CLI calls.
//!
//! Every child runs in its own process group. On timeout the whole group is
//! killed (SIGKILL), and after a normal exit the group is swept too, so a CLI
//! that forks helpers (node workers, MCP servers) never leaves an orphan.
//! Output is capped so a runaway CLI cannot exhaust memory.

use std::{collections::BTreeMap, path::PathBuf, process::Stdio, time::Duration};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};

/// Upper bound kept from each of stdout and stderr.
const MAX_CAPTURE_BYTES: usize = 8 * 1024 * 1024;
/// How long to keep draining pipes after the main child exits; a grandchild
/// that inherited the pipe would otherwise hold the reader open forever.
const PIPE_DRAIN_GRACE: Duration = Duration::from_millis(500);

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
    let stdout_task = child
        .stdout
        .take()
        .map(|pipe| tokio::spawn(read_capped(pipe)));
    let stderr_task = child
        .stderr
        .take()
        .map(|pipe| tokio::spawn(read_capped(pipe)));

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
            for task in [stdout_task, stderr_task].into_iter().flatten() {
                task.abort();
            }
            return Err(ProcessError::TimedOut);
        }
    };
    let stdout = collect(stdout_task).await;
    // Sweep helpers the CLI may have left behind before reading stderr, so a
    // lingering grandchild cannot hold the pipe open.
    kill_group(group);
    let stderr = collect(stderr_task).await;
    Ok(ProcessOutput {
        success: status.success(),
        code: status.code(),
        stdout,
        stderr,
    })
}

async fn collect(task: Option<tokio::task::JoinHandle<Vec<u8>>>) -> String {
    let Some(mut task) = task else {
        return String::new();
    };
    let bytes = match tokio::time::timeout(PIPE_DRAIN_GRACE, &mut task).await {
        Ok(Ok(bytes)) => bytes,
        Ok(Err(_)) => Vec::new(),
        Err(_) => {
            task.abort();
            Vec::new()
        }
    };
    String::from_utf8_lossy(&bytes).into_owned()
}

async fn read_capped(mut pipe: impl AsyncRead + Unpin) -> Vec<u8> {
    let mut kept = Vec::new();
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        match pipe.read(&mut buffer).await {
            Ok(0) | Err(_) => break,
            Ok(read) => {
                let room = MAX_CAPTURE_BYTES.saturating_sub(kept.len());
                kept.extend_from_slice(&buffer[..read.min(room)]);
            }
        }
    }
    kept
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
