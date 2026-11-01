use std::sync::Mutex;
use std::time::Instant;

#[derive(Default)]
pub(super) struct PauseState {
    paused_at: Mutex<Option<Instant>>,
}

impl PauseState {
    pub(super) fn pause(&self, now: Instant) {
        let mut paused_at = self.paused_at.lock().unwrap_or_else(|e| e.into_inner());
        if paused_at.is_none() {
            *paused_at = Some(now);
        }
    }
    pub(super) fn resume(&self) {
        *self.paused_at.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
    pub(super) fn snapshot(&self) -> Option<Instant> {
        self.snapshot_with(|| {})
    }
    fn snapshot_with(&self, between: impl FnOnce()) -> Option<Instant> {
        let paused_at = *self.paused_at.lock().unwrap_or_else(|e| e.into_inner());
        between();
        paused_at
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::{Arc, Barrier},
        thread,
        time::Duration,
    };
    #[test]
    fn resume_during_capture_pause_cannot_change_the_observed_pause_time() {
        let state = Arc::new(PauseState::default());
        let paused_at = Instant::now() - Duration::from_secs(90);
        state.pause(paused_at);
        let observed = Arc::new(Barrier::new(2));
        let resumed = Arc::new(Barrier::new(2));
        let worker = {
            let (state, observed, resumed) = (state.clone(), observed.clone(), resumed.clone());
            thread::spawn(move || {
                state.snapshot_with(|| {
                    observed.wait();
                    resumed.wait();
                })
            })
        };
        observed.wait();
        state.resume();
        resumed.wait();
        let captured = worker.join().unwrap().unwrap();
        assert_eq!(captured, paused_at);
        let mut silence =
            super::super::active_silence::ActiveSilence::new(paused_at - Duration::from_secs(2));
        assert_eq!(silence.pause(captured, 100), 200);
        assert_eq!(silence.finish(Instant::now(), 100), 0);
        assert_eq!(state.snapshot(), None);
        state.pause(paused_at);
        state.pause(paused_at + Duration::from_secs(10));
        assert_eq!(state.snapshot(), Some(paused_at));
        state.resume();
        state.resume();
        assert_eq!(state.snapshot(), None);
    }
}
