//! macOS media-output attenuation owned by the host for one dictation take.
//! The helper reader and settings setter publish intent. Only the executor calls HAL.

use std::{
    io,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

#[cfg(target_os = "macos")]
#[path = "dictation_output_duck/macos.rs"]
mod macos;
#[path = "dictation_output_duck/policy.rs"]
mod policy;
#[cfg(test)]
#[path = "dictation_output_duck/tests.rs"]
mod tests;

#[cfg(target_os = "macos")]
pub(crate) use macos::start;
use policy::{Backend, Policy};
#[cfg(test)]
use policy::{Device, Snapshot};

pub(crate) const RESTORE_TIMEOUT: Duration = Duration::from_millis(250);

#[derive(Clone)]
struct Take {
    generation: u64,
    id: String,
    epoch: u64,
    // Keep the outcome available to the take's receipt after retiring its lease.
    unconfirmed_restore: Arc<AtomicBool>,
}

impl PartialEq for Take {
    fn eq(&self, other: &Self) -> bool {
        self.generation == other.generation && self.id == other.id && self.epoch == other.epoch
    }
}

struct Intent {
    generation: u64,
    helper_live: bool,
    enabled: bool,
    epoch: u64,
    last_take: Option<Take>,
    active: Option<Take>,
    shutdown: bool,
}

enum Signal {
    Sync,
    Topology,
    Properties,
    Barrier(Barrier),
}

enum ReceiptScope {
    Take(Option<Take>),
    Shutdown,
}

struct Barrier {
    deadline: Instant,
    scope: ReceiptScope,
    reply: mpsc::SyncSender<bool>,
}

impl Barrier {
    fn is_shutdown(&self) -> bool {
        matches!(self.scope, ReceiptScope::Shutdown)
    }

    fn expire(&self, now: Instant, warn: &impl Fn(String)) -> bool {
        if now < self.deadline {
            return false;
        }
        warn("output volume restoration was not confirmed before the deadline".into());
        let _ = self.reply.send(false);
        true
    }
}

pub(crate) struct Completion(mpsc::Receiver<bool>);

impl Completion {
    pub(crate) fn wait(self, timeout: Duration) -> bool {
        self.0.recv_timeout(timeout).unwrap_or(false)
    }
}

pub(crate) struct Executor {
    intent: Arc<Mutex<Intent>>,
    wake: mpsc::Sender<Signal>,
}

impl Executor {
    #[cfg(test)]
    fn spawn(
        enabled: bool,
        backend: impl Backend + Send + 'static,
        warn: impl Fn(String) + Send + 'static,
    ) -> io::Result<Self> {
        Self::spawn_with(enabled, |_| backend, warn)
    }

    fn spawn_with<B: Backend + 'static>(
        enabled: bool,
        backend: impl FnOnce(mpsc::Sender<Signal>) -> B + Send + 'static,
        warn: impl Fn(String) + Send + 'static,
    ) -> io::Result<Self> {
        let (wake, events) = mpsc::channel();
        let intent = Arc::new(Mutex::new(Intent {
            generation: 0,
            helper_live: false,
            enabled,
            epoch: 0,
            last_take: None,
            active: None,
            shutdown: false,
        }));
        let worker_intent = intent.clone();
        let notifications = wake.clone();
        thread::Builder::new()
            .name("dictation-output".into())
            .spawn(move || run(backend(notifications), &worker_intent, events, warn))?;
        Ok(Self { intent, wake })
    }

    fn update(&self, update: impl FnOnce(&mut Intent) -> bool) {
        let mut intent = self.intent.lock().unwrap();
        if !intent.shutdown && update(&mut intent) {
            let _ = self.wake.send(Signal::Sync);
        }
    }

    pub(crate) fn helper_started(&self, generation: u64) {
        self.update(|intent| {
            if generation <= intent.generation {
                return false;
            }
            intent.generation = generation;
            intent.helper_live = true;
            intent.last_take = None;
            intent.active = None;
            true
        });
    }

    pub(crate) fn helper_exited(&self, generation: u64) {
        self.update(|intent| {
            if generation != intent.generation || !intent.helper_live {
                return false;
            }
            intent.helper_live = false;
            intent.active = None;
            true
        });
    }

    pub(crate) fn set_enabled(&self, enabled: bool) {
        self.update(|intent| {
            if enabled == intent.enabled {
                return false;
            }
            intent.enabled = enabled;
            intent.epoch += 1;
            if !enabled {
                intent.active = None;
            }
            true
        });
    }

    pub(crate) fn event(&self, generation: u64, kind: &str, id: &str) {
        let start = match kind {
            "listening_started" => true,
            "finalizing_transcript"
            | "recording_ready"
            | "recording_discarded"
            | "final_transcript"
            | "error" => false,
            _ => return,
        };
        self.update(|intent| {
            if !intent.helper_live || generation != intent.generation {
                return false;
            }
            if start {
                if intent.last_take.as_ref().is_some_and(|take| take.id == id) {
                    return false;
                }
                let take = Take {
                    generation,
                    id: id.to_owned(),
                    epoch: intent.epoch,
                    unconfirmed_restore: Arc::default(),
                };
                intent.active = intent.enabled.then(|| take.clone());
                intent.last_take = Some(take);
            } else if intent.active.as_ref().is_some_and(|take| take.id == id) {
                intent.active = None;
            } else {
                return false;
            }
            true
        });
    }

    pub(crate) fn wait_until_reconciled(&self) -> Completion {
        let take = self.intent.lock().unwrap().last_take.clone();
        self.completion(ReceiptScope::Take(take))
    }

    pub(crate) fn shutdown(&self) -> Completion {
        {
            let mut intent = self.intent.lock().unwrap();
            intent.shutdown = true;
            intent.active = None;
        }
        self.completion(ReceiptScope::Shutdown)
    }

    fn completion(&self, scope: ReceiptScope) -> Completion {
        let (reply, receipt) = mpsc::sync_channel(1);
        let _ = self.wake.send(Signal::Barrier(Barrier {
            deadline: Instant::now() + RESTORE_TIMEOUT,
            scope,
            reply,
        }));
        Completion(receipt)
    }

    #[cfg(test)]
    fn device_changed(&self) {
        self.wake.send(Signal::Topology).unwrap();
    }

    #[cfg(test)]
    fn properties_changed(&self) {
        self.wake.send(Signal::Properties).unwrap();
    }
}

fn run(
    mut backend: impl Backend,
    intent: &Mutex<Intent>,
    events: mpsc::Receiver<Signal>,
    warn: impl Fn(String),
) {
    let mut policy = Policy::default();
    let mut barriers = Vec::new();
    let mut shutdown = false;
    loop {
        let now = Instant::now();
        barriers.retain(|barrier: &Barrier| !barrier.expire(now, &warn));
        if shutdown && !barriers.iter().any(Barrier::is_shutdown) {
            break;
        }

        let signal = match barriers.iter().map(|barrier| barrier.deadline).min() {
            Some(deadline) => events.recv_timeout(deadline.saturating_duration_since(now)),
            None => events
                .recv()
                .map_err(|_| mpsc::RecvTimeoutError::Disconnected),
        };
        let signal = match signal {
            Ok(signal) => signal,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                policy.reconcile(&mut backend, None, |_| false, true, &warn);
                break;
            }
        };

        let retry = !matches!(signal, Signal::Properties);
        if let Signal::Barrier(barrier) = signal {
            shutdown |= barrier.is_shutdown();
            barriers.push(barrier);
        }

        let take = intent.lock().unwrap().active.clone();
        policy.reconcile(
            &mut backend,
            take,
            |take| intent.lock().unwrap().active.as_ref() == Some(take),
            retry,
            &warn,
        );

        let now = Instant::now();
        barriers.retain(|barrier| {
            if barrier.expire(now, &warn) {
                return false;
            }
            let (pending, unconfirmed) = match &barrier.scope {
                ReceiptScope::Take(Some(take)) => (
                    policy.has_pending_restore_for(take),
                    take.unconfirmed_restore.load(Ordering::Relaxed),
                ),
                ReceiptScope::Take(None) => (false, false),
                ReceiptScope::Shutdown => (
                    policy.has_pending_restore(),
                    policy.has_unconfirmed_restore(),
                ),
            };
            if pending {
                return true;
            }
            if unconfirmed && barrier.is_shutdown() {
                warn("output volume could not be confirmed restored before shutdown".into());
            }
            let _ = barrier.reply.send(!unconfirmed);
            false
        });
    }
    if policy.has_pending_restore() {
        warn("output volume could not be confirmed restored before shutdown".into());
    }
}
