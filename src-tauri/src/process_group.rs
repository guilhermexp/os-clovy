//! Signals for the process group of a child spawned with `process_group(0)`.
//!
//! A child that forks helpers (dev servers, node workers, MCP servers) leaves
//! them in its own group; signalling the group reaches them too, where killing
//! the child alone would orphan them and leave its pipes open.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupSignal {
    Terminate,
    Kill,
}

/// Sends `signal` to the group led by `group` (the child's pid). Does nothing
/// for a missing id or for pids 0 and 1.
#[cfg(unix)]
pub fn signal_group(group: Option<u32>, signal: GroupSignal) {
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

#[cfg(not(unix))]
pub fn signal_group(_group: Option<u32>, _signal: GroupSignal) {}

/// SIGKILL to the whole group.
pub fn kill_group(group: Option<u32>) {
    signal_group(group, GroupSignal::Kill);
}
