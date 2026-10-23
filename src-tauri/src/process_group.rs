//! The whole process tree of a spawned child.
//!
//! A child that forks helpers (dev servers, node workers, MCP servers) must
//! not leave them behind when it is cancelled or times out: killing the child
//! alone would orphan them and leave its pipes open.
//!
//! - Unix: the child is spawned with `process_group(0)`; signalling the group
//!   reaches every helper.
//! - Windows: there are no process groups. The child is placed in a Job
//!   Object right after it starts, its descendants join the job when they
//!   are created, and killing the tree terminates the job. The job is also
//!   closed with `KILL_ON_JOB_CLOSE`, so dropping the tree sweeps whatever is
//!   left (an npm `codex.cmd` runs `cmd.exe` -> `node` -> `codex.exe`).

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupSignal {
    Terminate,
    Kill,
}

/// The tree of the child with this pid, adopted right after spawn.
#[derive(Debug)]
pub struct ProcessTree {
    #[cfg(unix)]
    group: Option<u32>,
    #[cfg(windows)]
    job: Option<windows_job::Job>,
}

impl ProcessTree {
    /// Adopts the tree of a freshly spawned child (`child.id()`). On Unix the
    /// child must have been spawned with `process_group(0)`.
    pub fn adopt(pid: Option<u32>) -> Self {
        #[cfg(unix)]
        {
            Self { group: pid }
        }
        #[cfg(windows)]
        {
            Self {
                job: pid.and_then(windows_job::Job::for_process),
            }
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = pid;
            Self {}
        }
    }

    /// Spawns `command` and adopts its whole tree.
    pub fn spawn(
        command: &mut tokio::process::Command,
    ) -> std::io::Result<(tokio::process::Child, Self)> {
        Self::spawn_with(command, |_| {})
    }

    /// [`Self::spawn`], running `between` once the child exists and before
    /// its tree is adopted; tests widen that window with it.
    fn spawn_with(
        command: &mut tokio::process::Command,
        between: impl FnOnce(u32),
    ) -> std::io::Result<(tokio::process::Child, Self)> {
        #[cfg(unix)]
        command.process_group(0);
        let child = command.spawn()?;
        if let Some(pid) = child.id() {
            between(pid);
        }
        let tree = Self::adopt(child.id());
        Ok((child, tree))
    }

    /// Sends `signal` to the whole tree. Windows has no graceful signal for
    /// another process, so `Terminate` does nothing there and only `Kill`
    /// ends the tree.
    pub fn signal(&self, signal: GroupSignal) {
        #[cfg(unix)]
        signal_group(self.group, signal);
        #[cfg(windows)]
        if signal == GroupSignal::Kill {
            if let Some(job) = &self.job {
                job.terminate();
            }
        }
        #[cfg(not(any(unix, windows)))]
        let _ = signal;
    }

    /// Kills the whole tree.
    pub fn kill(&self) {
        self.signal(GroupSignal::Kill);
    }
}

/// Sends `signal` to the group led by `group` (the child's pid). Does nothing
/// for a missing id or for pids 0 and 1.
#[cfg(unix)]
fn signal_group(group: Option<u32>, signal: GroupSignal) {
    let Some(group) = group.and_then(|pid| i32::try_from(pid).ok()) else {
        return;
    };
    if group <= 1 {
        return;
    }
    let signal = match signal {
        GroupSignal::Terminate => libc::SIGTERM,
        GroupSignal::Kill => libc::SIGKILL,
    };
    // SAFETY: killpg only signals; the group id is the child's own pid because
    // it was spawned with `process_group(0)`, so this never reaches Clovy.
    unsafe {
        libc::killpg(group, signal);
    }
}

#[cfg(windows)]
mod windows_job {
    use windows::core::PCWSTR;
    use windows::Win32::{
        Foundation::{CloseHandle, HANDLE},
        System::{
            JobObjects::{
                AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
                SetInformationJobObject, TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
                JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            },
            Threading::{OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE},
        },
    };

    /// A Job Object handle that kills its processes when closed.
    #[derive(Debug)]
    pub struct Job(isize);

    // SAFETY: a job handle is a kernel handle usable from any thread.
    unsafe impl Send for Job {}
    unsafe impl Sync for Job {}

    impl Job {
        /// A new kill-on-close job holding the process `pid`, or `None` when
        /// Windows refuses (the tree then ends with the child only).
        pub fn for_process(pid: u32) -> Option<Self> {
            // SAFETY: every handle opened here is closed here or by `Drop`.
            unsafe {
                let job = CreateJobObjectW(None, PCWSTR::null()).ok()?;
                let job = Job(job.0 as isize);
                let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
                limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                SetInformationJobObject(
                    job.handle(),
                    JobObjectExtendedLimitInformation,
                    std::ptr::from_ref(&limits).cast(),
                    u32::try_from(std::mem::size_of_val(&limits)).ok()?,
                )
                .ok()?;
                let process =
                    OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, false, pid).ok()?;
                let assigned = AssignProcessToJobObject(job.handle(), process);
                let _ = CloseHandle(process);
                if let Err(error) = assigned {
                    tracing::warn!(%error, pid, "could not place the CLI in a job object");
                    return None;
                }
                Some(job)
            }
        }

        fn handle(&self) -> HANDLE {
            HANDLE(self.0 as *mut _)
        }

        pub fn terminate(&self) {
            // SAFETY: the handle is open for the lifetime of `self`.
            unsafe {
                let _ = TerminateJobObject(self.handle(), 1);
            }
        }
    }

    impl Drop for Job {
        fn drop(&mut self) {
            // SAFETY: closing the last handle kills what is left in the job
            // (KILL_ON_JOB_CLOSE); the handle is not used afterwards.
            unsafe {
                let _ = CloseHandle(self.handle());
            }
        }
    }
}

#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;
    use crate::llm::cmd_shim_tests::{
        kill_pid, npm_cmd_cli_with_child, process_alive, wait_for_pid,
    };
    use std::{process::Stdio, time::Duration};

    /// `cmd.exe` → `node` → child, where the child starts while Clovy is
    /// still between `spawn()` and placing the CLI in its job: killing the
    /// tree must still end it.
    #[tokio::test]
    async fn a_child_started_before_the_tree_is_adopted_dies_with_it() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("child.pid");
        let program = npm_cmd_cli_with_child(dir.path(), "codex", &pid_file, "started");
        let mut command = crate::hide_console::tokio_command(&program);
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        let window_pid = pid_file.clone();
        let (mut child, tree) = ProcessTree::spawn_with(&mut command, move |_| {
            // A wide window: a running CLI starts its child in it.
            let _ = wait_for_pid(&window_pid, Duration::from_secs(15));
        })
        .expect("spawn");
        let pid_path = pid_file.clone();
        let grandchild =
            tokio::task::spawn_blocking(move || wait_for_pid(&pid_path, Duration::from_secs(30)))
                .await
                .unwrap()
                .expect("child never started");
        tree.kill();
        let _ = child.wait().await;
        let alive = process_alive(grandchild);
        kill_pid(grandchild);
        assert!(!alive, "child {grandchild} escaped the job");
    }
}
