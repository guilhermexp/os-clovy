//! What keeps two writers apart: a lock per target shared by every launch,
//! and checks that the old app and stale lock holders are really gone.

use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::{fail, MigrationError};

/// `<target name><suffix>` beside the target.
pub(super) fn sibling(target: &Path, suffix: &str) -> PathBuf {
    let mut name = target.file_name().unwrap_or_default().to_os_string();
    name.push(suffix);
    target.with_file_name(name)
}

/// Held for the whole migration of one target: decision, cleanup of stale
/// staging, credentials and publication. Released when dropped; a crashed
/// holder leaves its pid behind and the next launch takes the lock over.
pub(super) struct MigrationLock {
    path: PathBuf,
}

impl MigrationLock {
    pub(super) fn acquire(target: &Path, wait: Duration) -> Result<Self, MigrationError> {
        let path = sibling(target, ".identity-migration.lock");
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| fail(format!("creating {}", parent.display()), e))?;
        }
        let deadline = Instant::now() + wait;
        loop {
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(mut file) => {
                    file.write_all(std::process::id().to_string().as_bytes())
                        .and_then(|()| file.sync_all())
                        .map_err(|e| fail(format!("writing {}", path.display()), e))?;
                    return Ok(Self { path });
                }
                Err(error) if error.kind() == ErrorKind::AlreadyExists => {
                    let holder = read_pid(&path);
                    match holder {
                        Some(pid) if is_live_launch(pid) => {
                            if Instant::now() >= deadline {
                                return Err(MigrationError {
                                    message: format!(
                                        "another Clovy launch (pid {pid}) is migrating {}; reopen Clovy when it finishes",
                                        target.display()
                                    ),
                                    blocked: true,
                                });
                            }
                            std::thread::sleep(Duration::from_millis(200));
                        }
                        // A dead holder, or a file that never got its pid:
                        // take it over, unless someone else did meanwhile.
                        _ => {
                            if read_pid(&path) == holder {
                                let _ = std::fs::remove_file(&path);
                            }
                        }
                    }
                }
                Err(error) => return Err(fail(format!("creating {}", path.display()), error)),
            }
        }
    }
}

impl Drop for MigrationLock {
    fn drop(&mut self) {
        if read_pid(&self.path) == Some(std::process::id()) {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

fn read_pid(path: &Path) -> Option<u32> {
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

/// The pid at the end of a staging name (`<target>.identity-migration-<pid>`,
/// optionally followed by `.scratch`).
pub(super) fn staging_owner(name: &str, prefix: &str) -> Option<u32> {
    let rest = name.strip_prefix(prefix)?;
    rest.split('.').next()?.parse().ok()
}

/// Whether `pid` is a running process.
pub(super) fn is_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        let Ok(pid) = libc::pid_t::try_from(pid) else {
            return false;
        };
        // SAFETY: signal 0 only checks that the process exists.
        let result = unsafe { libc::kill(pid, 0) };
        result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }
    #[cfg(windows)]
    {
        windows_image_of(pid).is_some()
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = pid;
        false
    }
}

/// Whether `pid` is alive and runs this very executable, so a reused pid of
/// an unrelated process never holds a lock forever.
fn is_live_launch(pid: u32) -> bool {
    let Some(own) = std::env::current_exe().ok().and_then(|path| {
        path.file_name()
            .map(|name| name.to_string_lossy().into_owned())
    }) else {
        return false;
    };
    #[cfg(unix)]
    {
        if !is_alive(pid) {
            return false;
        }
        let Ok(output) = std::process::Command::new("/bin/ps")
            .args(["-p", &pid.to_string(), "-o", "comm="])
            .output()
        else {
            return true;
        };
        let comm = String::from_utf8_lossy(&output.stdout);
        let comm = comm.trim();
        // `comm` is the executable path on macOS and may be cut short
        // elsewhere: a prefix match still tells the app from a stranger.
        let name = comm.rsplit('/').next().unwrap_or(comm);
        !name.is_empty() && own.starts_with(name)
    }
    #[cfg(windows)]
    {
        windows_image_of(pid).is_some_and(|image| image.eq_ignore_ascii_case(&own))
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (pid, own);
        false
    }
}

#[cfg(windows)]
fn windows_image_of(pid: u32) -> Option<String> {
    let output = std::process::Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    let line = text.lines().find(|line| line.starts_with('"'))?;
    let mut fields = line.split("\",\"");
    let image = fields.next()?.trim_start_matches('"').to_string();
    let listed_pid = fields.next()?.trim_matches('"');
    (listed_pid == pid.to_string()).then_some(image)
}

/// The executables of `names` that are running now. A check that cannot run
/// is an error: copying under a live writer is never assumed safe.
pub(super) fn running_executables(names: &[String]) -> Result<Vec<String>, MigrationError> {
    let mut running = Vec::new();
    for name in names {
        #[cfg(unix)]
        {
            if name.ends_with(".exe") {
                continue;
            }
            let status = std::process::Command::new("/usr/bin/pgrep")
                .args(["-x", name])
                .stdout(std::process::Stdio::null())
                .status()
                .map_err(|e| blocked_check(name, e))?;
            match status.code() {
                Some(0) => running.push(name.clone()),
                Some(1) => {}
                _ => return Err(blocked_check(name, format!("pgrep exited with {status}"))),
            }
        }
        #[cfg(windows)]
        {
            if !name.ends_with(".exe") {
                continue;
            }
            let output = std::process::Command::new("tasklist")
                .args(["/FI", &format!("IMAGENAME eq {name}"), "/FO", "CSV", "/NH"])
                .output()
                .map_err(|e| blocked_check(name, e))?;
            let text = String::from_utf8_lossy(&output.stdout).to_ascii_lowercase();
            if text.contains(&format!("\"{}\"", name.to_ascii_lowercase())) {
                running.push(name.clone());
            }
        }
    }
    Ok(running)
}

fn blocked_check(name: &str, error: impl std::fmt::Display) -> MigrationError {
    MigrationError {
        message: format!("could not check whether {name} is running: {error}"),
        blocked: true,
    }
}
