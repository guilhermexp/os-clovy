use crate::domain::types::AppError;
use std::sync::{Arc, Mutex};

pub(crate) type CaptureStartAuthorization = Arc<dyn Fn() -> Result<(), AppError> + Send + Sync>;

pub(crate) fn with_control_admission<T, R>(
    state: &Mutex<T>,
    authorization: Option<&CaptureStartAuthorization>,
    effect: impl FnOnce(&mut T) -> Result<R, AppError>,
) -> Result<R, AppError> {
    let mut state = state
        .lock()
        .map_err(|_| AppError::new("recording_lock_failed", "Recording state is unavailable."))?;
    if let Some(authorization) = authorization {
        authorization()?;
    }
    effect(&mut state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::turn_authority::TurnAuthority;

    #[test]
    fn revocation_during_lock_wait_prevents_the_control_effect() {
        let state = Arc::new(Mutex::new(false));
        let held = state.lock().unwrap();
        let authority = TurnAuthority::new();
        let check = authority.clone();
        let gate: CaptureStartAuthorization = Arc::new(move || check.check_live());
        let (started, waiting) = std::sync::mpsc::channel();
        let worker = {
            let state = state.clone();
            std::thread::spawn(move || {
                started.send(()).unwrap();
                with_control_admission(&state, Some(&gate), |paused| {
                    *paused = true;
                    Ok(())
                })
            })
        };
        waiting.recv().unwrap();
        authority.revoke();
        drop(held);
        assert_eq!(worker.join().unwrap().unwrap_err().code, "turn_expired");
        assert!(!*state.lock().unwrap());
        with_control_admission(&state, None, |paused| {
            *paused = true;
            Ok(())
        })
        .unwrap();
        assert!(*state.lock().unwrap());
    }
}
