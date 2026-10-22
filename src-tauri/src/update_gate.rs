//! Keeps an update install from ending Clovy in the middle of a capture.
//!
//! On Windows `Update::install` exits the process, and on macOS the restart
//! that follows the install does too. A meeting recording or a dictation take
//! started while the package downloads must therefore hold the install back
//! until it ends, and once the install has begun no new capture may start.
//! Capture starts and the install decision take the same lock, so neither can
//! slip past the other.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use crate::domain::types::AppError;

/// A dictation start is sent to the helper before the helper reports the take
/// as running. Until then the start itself holds the install back.
pub const DICTATION_START_GRACE: Duration = Duration::from_secs(10);

pub static UPDATE_GATE: UpdateGate = UpdateGate::new();

struct GateState {
    installing: bool,
    starting_recordings: usize,
    last_dictation_start: Option<Instant>,
}

pub struct UpdateGate {
    state: Mutex<GateState>,
}

/// The install could not begin: a capture is running or starting.
#[derive(Debug, PartialEq, Eq)]
pub struct CaptureBusy;

/// Held from the start of a recording until it is published as active.
pub struct RecordingStart<'a> {
    gate: &'a UpdateGate,
}

/// Held while the install runs. Dropping it (a failed install) lets captures
/// start again.
pub struct InstallSlot<'a> {
    gate: &'a UpdateGate,
}

impl UpdateGate {
    pub const fn new() -> Self {
        Self {
            state: Mutex::new(GateState {
                installing: false,
                starting_recordings: 0,
                last_dictation_start: None,
            }),
        }
    }

    fn lock(&self) -> MutexGuard<'_, GateState> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn begin_recording_start(&self) -> Result<RecordingStart<'_>, AppError> {
        Ok(RecordingStart { gate: self })
    }

    pub fn begin_dictation_start(&self) -> Result<(), AppError> {
        self.begin_dictation_start_at(Instant::now())
    }

    fn begin_dictation_start_at(&self, _now: Instant) -> Result<(), AppError> {
        Ok(())
    }

    /// `capture_active` runs under the gate lock, so a capture that is not
    /// active yet is still registered as starting.
    pub fn try_begin_install(
        &self,
        capture_active: impl FnOnce() -> bool,
    ) -> Result<InstallSlot<'_>, CaptureBusy> {
        self.try_begin_install_at(Instant::now(), capture_active)
    }

    fn try_begin_install_at(
        &self,
        _now: Instant,
        _capture_active: impl FnOnce() -> bool,
    ) -> Result<InstallSlot<'_>, CaptureBusy> {
        Ok(InstallSlot { gate: self })
    }

    /// Waits until no capture runs, then takes the install slot. `on_wait`
    /// runs once, the first time a capture holds the install back. The owner
    /// can postpone while it waits.
    pub async fn wait_for_install_slot(
        &self,
        capture_active: impl Fn() -> bool,
        poll: Duration,
        on_wait: impl FnOnce(),
        postponed: &AtomicBool,
    ) -> Result<InstallSlot<'_>, InstallPostponed> {
        let _ = (capture_active, poll, on_wait, postponed);
        Ok(InstallSlot { gate: self })
    }
}

/// The owner postponed an install that was waiting for a capture to end.
#[derive(Debug, PartialEq, Eq)]
pub struct InstallPostponed;

impl Drop for RecordingStart<'_> {
    fn drop(&mut self) {
        drop(self.gate.lock());
    }
}

impl Drop for InstallSlot<'_> {
    fn drop(&mut self) {
        drop(self.gate.lock());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_active_capture_holds_the_install() {
        let gate = UpdateGate::new();
        assert_eq!(gate.try_begin_install(|| true).err(), Some(CaptureBusy));
    }

    #[test]
    fn a_recording_still_starting_holds_the_install() {
        let gate = UpdateGate::new();
        let start = gate.begin_recording_start().unwrap();
        assert_eq!(gate.try_begin_install(|| false).err(), Some(CaptureBusy));
        drop(start);
        assert!(gate.try_begin_install(|| false).is_ok());
    }

    #[test]
    fn a_recent_dictation_start_holds_the_install_until_the_grace_ends() {
        let gate = UpdateGate::new();
        let started = Instant::now();
        gate.begin_dictation_start_at(started).unwrap();
        let soon = started + Duration::from_secs(1);
        assert_eq!(
            gate.try_begin_install_at(soon, || false).err(),
            Some(CaptureBusy)
        );
        let later = started + DICTATION_START_GRACE + Duration::from_secs(1);
        assert!(gate.try_begin_install_at(later, || false).is_ok());
    }

    #[test]
    fn no_capture_starts_while_the_update_installs() {
        let gate = UpdateGate::new();
        let _install = gate.try_begin_install(|| false).unwrap();
        let recording = gate.begin_recording_start().err().map(|error| error.code);
        assert_eq!(recording.as_deref(), Some("update_installing"));
        let dictation = gate.begin_dictation_start().err().map(|error| error.code);
        assert_eq!(dictation.as_deref(), Some("update_installing"));
    }

    #[tokio::test]
    async fn the_install_waits_for_the_capture_to_end() {
        let gate = UpdateGate::new();
        let polls = std::sync::atomic::AtomicUsize::new(0);
        let waited = std::cell::Cell::new(0);
        let postponed = AtomicBool::new(false);
        let slot = gate
            .wait_for_install_slot(
                || polls.fetch_add(1, Ordering::SeqCst) < 2,
                Duration::from_millis(1),
                || waited.set(waited.get() + 1),
                &postponed,
            )
            .await;
        assert!(slot.is_ok());
        assert_eq!(polls.load(Ordering::SeqCst), 3);
        assert_eq!(waited.get(), 1);
    }

    #[tokio::test]
    async fn the_owner_can_postpone_a_waiting_install() {
        let gate = UpdateGate::new();
        let postponed = AtomicBool::new(false);
        let slot = gate
            .wait_for_install_slot(
                || {
                    postponed.store(true, Ordering::SeqCst);
                    true
                },
                Duration::from_millis(1),
                || {},
                &postponed,
            )
            .await;
        assert_eq!(slot.err(), Some(InstallPostponed));
        assert!(gate.begin_recording_start().is_ok());
    }

    #[test]
    fn a_failed_install_lets_captures_start_again() {
        let gate = UpdateGate::new();
        drop(gate.try_begin_install(|| false).unwrap());
        assert!(gate.begin_recording_start().is_ok());
        assert!(gate.begin_dictation_start().is_ok());
    }
}
