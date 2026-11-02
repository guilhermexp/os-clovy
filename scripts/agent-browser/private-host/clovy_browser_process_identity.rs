//! Native process creation identity used by the browser host and supervisor.
use std::io;

pub(crate) fn process_birth(pid: u32) -> io::Result<u64> {
    if pid == 0 {
        return Err(io::Error::other("invalid process identity"));
    }
    #[cfg(target_os = "linux")]
    {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat"))?;
        let tail = stat
            .rsplit_once(')')
            .ok_or_else(|| io::Error::other("invalid process stat"))?
            .1;
        let birth = tail
            .split_whitespace()
            .nth(19)
            .and_then(|value| value.parse::<u64>().ok())
            .filter(|value| *value != 0)
            .ok_or_else(|| io::Error::other("invalid process birth"))?;
        Ok(birth)
    }
    #[cfg(target_os = "macos")]
    {
        let pid = i32::try_from(pid).map_err(|_| io::Error::other("invalid process identity"))?;
        let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::zeroed();
        let size = std::mem::size_of::<libc::proc_bsdinfo>() as i32;
        if unsafe {
            libc::proc_pidinfo(
                pid,
                libc::PROC_PIDTBSDINFO,
                0,
                info.as_mut_ptr().cast(),
                size,
            )
        } != size
        {
            return Err(io::Error::last_os_error());
        }
        let info = unsafe { info.assume_init() };
        if info.pbi_pid != pid as u32 || info.pbi_start_tvusec >= 1_000_000 {
            return Err(io::Error::other("invalid process birth"));
        }
        info.pbi_start_tvsec
            .checked_mul(1_000_000)
            .and_then(|v| v.checked_add(info.pbi_start_tvusec))
            .filter(|v| *v != 0)
            .ok_or_else(|| io::Error::other("invalid process birth"))
    }
    #[cfg(windows)]
    {
        #[repr(C)]
        #[derive(Default)]
        struct FileTime {
            low: u32,
            high: u32,
        }
        #[link(name = "kernel32")]
        extern "system" {
            fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut std::ffi::c_void;
            fn GetProcessTimes(
                process: *mut std::ffi::c_void,
                created: *mut FileTime,
                exited: *mut FileTime,
                kernel: *mut FileTime,
                user: *mut FileTime,
            ) -> i32;
            fn CloseHandle(handle: *mut std::ffi::c_void) -> i32;
        }
        let process = unsafe { OpenProcess(0x1000, 0, pid) };
        if process.is_null() {
            return Err(io::Error::last_os_error());
        }
        let (mut created, mut exited, mut kernel, mut user) = (
            FileTime::default(),
            FileTime::default(),
            FileTime::default(),
            FileTime::default(),
        );
        let ok =
            unsafe { GetProcessTimes(process, &mut created, &mut exited, &mut kernel, &mut user) };
        let error = if ok == 0 {
            Some(io::Error::last_os_error())
        } else {
            None
        };
        unsafe {
            CloseHandle(process);
        }
        if let Some(error) = error {
            return Err(error);
        }
        let birth = ((created.high as u64) << 32) | created.low as u64;
        if birth == 0 {
            Err(io::Error::other("invalid process birth"))
        } else {
            Ok(birth)
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    #[test]
    fn current_and_child_births_are_stable_for_live_processes() {
        let own = process_birth(std::process::id()).unwrap();
        assert_eq!(process_birth(std::process::id()).unwrap(), own);
        let mut child = std::process::Command::new("/bin/sleep")
            .arg("30")
            .spawn()
            .unwrap();
        let child_birth = process_birth(child.id()).unwrap();
        assert_eq!(process_birth(child.id()).unwrap(), child_birth);
        child.kill().unwrap();
        child.wait().unwrap();
    }
}
