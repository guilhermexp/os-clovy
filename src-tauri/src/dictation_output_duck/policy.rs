use super::Take;
use std::sync::atomic::Ordering;

const VOLUME_FACTOR: f32 = 0.2;
// Cover scalar float noise only. The HAL provides no attribution or device
// quantization contract that would let us treat a different value as ours.
const VOLUME_EPSILON: f32 = 0.000_01;
const DUCK_UNCONFIRMED: &str = "output volume attenuation could not be confirmed";
const RESTORE_UNCONFIRMED: &str = "output volume restoration readback could not be confirmed";
const OUTPUT_REPLACED: &str =
    "output identity changed before volume restoration could be confirmed";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Device {
    pub(super) id: u32,
    pub(super) uid: String,
}

pub(super) struct Snapshot {
    pub(super) volumes: Vec<(u32, f32)>,
    pub(super) mutes: Vec<(u32, bool)>,
    pub(super) channels: u32,
}

pub(super) trait Backend {
    fn default_output(&mut self) -> Result<Option<Device>, String>;
    fn identity(&mut self, id: u32) -> Result<Option<Device>, String>;
    fn snapshot(&mut self, device: &Device) -> Result<Option<Snapshot>, String>;
    fn volume(&mut self, device: &Device, channel: u32) -> Result<f32, String>;
    fn mute(&mut self, device: &Device, channel: u32) -> Result<bool, String>;
    fn set_volume(&mut self, device: &Device, channel: u32, volume: f32) -> Result<(), String>;
    fn watch_topology(&mut self) -> Result<(), String>;
    fn watch_device(&mut self, device: &Device, snapshot: &Snapshot) -> Result<(), String>;
    fn unwatch_device(&mut self, device: &Device);
}

fn same_volume(left: f32, right: f32) -> bool {
    (left - right).abs() <= VOLUME_EPSILON
}

#[derive(Clone, Copy)]
enum VolumeState {
    Original,
    Ducking(f32),
    Ducked(f32),
    Restoring(f32),
    RestoreFailed(f32),
}

struct Volume {
    channel: u32,
    original: f32,
    state: VolumeState,
}

struct Lease {
    take: Take,
    device: Device,
    volumes: Vec<Volume>,
    mutes: Vec<(u32, bool)>,
}

#[derive(PartialEq, Eq)]
enum Ownership {
    Held,
    Changed,
    Unavailable,
    Unconfirmed(&'static str),
}

enum Release {
    Finished,
    Pending,
    Unavailable,
    Unconfirmed(&'static str),
}

impl Ownership {
    fn release(&self) -> Option<Release> {
        match self {
            Self::Held => None,
            Self::Changed => Some(Release::Finished),
            Self::Unavailable => Some(Release::Unavailable),
            Self::Unconfirmed(reason) => Some(Release::Unconfirmed(reason)),
        }
    }
}

impl Volume {
    fn observe(&mut self, actual: f32) -> Ownership {
        let original = same_volume(actual, self.original);
        self.state = match self.state {
            VolumeState::Original if original => VolumeState::Original,
            VolumeState::Ducking(target) if same_volume(actual, target) => {
                VolumeState::Ducked(target)
            }
            VolumeState::Ducking(_) if original => self.state,
            VolumeState::Ducking(_) => return Ownership::Unconfirmed(DUCK_UNCONFIRMED),
            VolumeState::Ducked(value) | VolumeState::RestoreFailed(value)
                if same_volume(actual, value) =>
            {
                self.state
            }
            VolumeState::Restoring(_) if original => VolumeState::Original,
            VolumeState::Restoring(value) if same_volume(actual, value) => self.state,
            VolumeState::Restoring(_) => return Ownership::Unconfirmed(RESTORE_UNCONFIRMED),
            _ => return Ownership::Changed,
        };
        Ownership::Held
    }

    fn awaiting_readback(&self) -> Option<&'static str> {
        match self.state {
            VolumeState::Ducking(_) => Some(DUCK_UNCONFIRMED),
            VolumeState::Restoring(_) => Some(RESTORE_UNCONFIRMED),
            _ => None,
        }
    }
}

impl Lease {
    fn inspect(&mut self, backend: &mut impl Backend) -> Result<Ownership, String> {
        match backend.identity(self.device.id)? {
            Some(device) if device == self.device => {}
            Some(_) => return Ok(Ownership::Unconfirmed(OUTPUT_REPLACED)),
            None => return Ok(Ownership::Unavailable),
        }
        let awaiting = self.volumes.iter().find_map(Volume::awaiting_readback);
        let external_change = awaiting.map_or(Ownership::Changed, Ownership::Unconfirmed);
        if self.mute_changed(backend)? {
            return Ok(external_change);
        }
        // Scan all controls until a change is found so a confirmed write on
        // one channel does not give us authority over a different channel.
        for volume in &mut self.volumes {
            let actual = backend.volume(&self.device, volume.channel)?;
            match volume.observe(actual) {
                Ownership::Held => {}
                Ownership::Changed => return Ok(external_change),
                other => return Ok(other),
            }
        }
        Ok(Ownership::Held)
    }

    fn mute_changed(&self, backend: &mut impl Backend) -> Result<bool, String> {
        for &(channel, muted) in &self.mutes {
            if backend.mute(&self.device, channel)? != muted {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn has_writes(&self) -> bool {
        self.volumes
            .iter()
            .any(|volume| !matches!(volume.state, VolumeState::Original))
    }

    // None means capture ended or its route changed mid-acquisition.
    fn duck(
        &mut self,
        backend: &mut impl Backend,
        still_current: impl Fn() -> bool,
    ) -> Result<Option<Ownership>, String> {
        for index in 0..self.volumes.len() {
            if !still_current() || backend.default_output()?.as_ref() != Some(&self.device) {
                return Ok(None);
            }
            let ownership = self.inspect(backend)?;
            if ownership != Ownership::Held {
                return Ok(Some(ownership));
            }
            if !still_current() {
                return Ok(None);
            }

            let volume = &mut self.volumes[index];
            let target = volume.original * VOLUME_FACTOR;
            if same_volume(target, volume.original) {
                continue;
            }
            backend.set_volume(&self.device, volume.channel, target)?;
            // SET only starts a write. Both immediate and later readbacks go
            // through the same confirmation rules.
            volume.state = VolumeState::Ducking(target);
            let actual = backend.volume(&self.device, volume.channel)?;
            let ownership = volume.observe(actual);
            if ownership != Ownership::Held {
                return Ok(Some(ownership));
            }
        }
        Ok(still_current().then_some(Ownership::Held))
    }

    fn restore(&mut self, backend: &mut impl Backend, retry: bool) -> Result<Release, String> {
        // Check the whole volume/mute snapshot before each write. A user choice
        // anywhere on the output takes precedence over the whole snapshot.
        let mut failure = None;
        for index in 0..self.volumes.len() {
            if let Some(outcome) = self.inspect(backend)?.release() {
                return Ok(outcome);
            }
            let volume = &mut self.volumes[index];
            let imposed = match volume.state {
                VolumeState::Ducked(imposed) => imposed,
                VolumeState::RestoreFailed(imposed) if retry => imposed,
                _ => continue,
            };
            match backend.set_volume(&self.device, volume.channel, volume.original) {
                Ok(()) => volume.state = VolumeState::Restoring(imposed),
                Err(error) => {
                    volume.state = VolumeState::RestoreFailed(imposed);
                    failure = Some(error);
                }
            }
        }
        if let Some(error) = failure {
            return Err(error);
        }
        Ok(self.inspect(backend)?.release().unwrap_or_else(|| {
            if self.has_writes() {
                Release::Pending
            } else {
                Release::Finished
            }
        }))
    }
}

#[derive(Default)]
pub(super) struct Policy {
    topology_watched: bool,
    take: Option<Take>,
    route: Option<Device>,
    active: Option<Lease>,
    pending: Vec<Lease>,
    unconfirmed_restore: bool,
}

impl Policy {
    pub(super) fn reconcile(
        &mut self,
        backend: &mut impl Backend,
        take: Option<Take>,
        still_current: impl Fn(&Take) -> bool,
        retry: bool,
        warn: &impl Fn(String),
    ) {
        self.pending.retain_mut(|lease| {
            !restore(backend, lease, retry, &mut self.unconfirmed_restore, warn)
        });

        if take != self.take {
            self.release(backend, warn);
            self.take = take;
            self.route = None;
        }
        let Some(take) = &self.take else {
            return;
        };
        if !still_current(take) {
            return;
        }
        if !self.topology_watched {
            if let Err(error) = backend.watch_topology() {
                warn(error);
                return;
            }
            self.topology_watched = true;
        }
        let output = match backend.default_output() {
            Ok(output) => output,
            Err(error) => {
                warn(error);
                return;
            }
        };
        if self.route == output {
            if let Some(lease) = &mut self.active {
                match lease.inspect(backend) {
                    Ok(ownership) => {
                        if let Some(outcome) = ownership.release() {
                            if finish_release(
                                backend,
                                lease,
                                outcome,
                                &mut self.unconfirmed_restore,
                                warn,
                            ) {
                                self.active = None;
                            }
                        }
                    }
                    Err(error) => warn(error),
                }
            }
            return;
        }

        self.release(backend, warn);
        // Leave a deferred route open for the pending restore's callback.
        self.route =
            output.filter(|device| !self.pending.iter().any(|lease| &lease.device == device));
        let (Some(take), Some(device)) = (&self.take, &self.route) else {
            return;
        };
        if !still_current(take) {
            return;
        }
        match acquire(
            backend,
            device,
            take,
            || still_current(take),
            &mut self.unconfirmed_restore,
            warn,
        ) {
            Acquisition::Active(lease) => self.active = Some(lease),
            Acquisition::Pending(lease) => self.pending.push(lease),
            Acquisition::Skipped => {}
        }
    }

    fn release(&mut self, backend: &mut impl Backend, warn: &impl Fn(String)) {
        if let Some(mut lease) = self.active.take() {
            if !restore(
                backend,
                &mut lease,
                true,
                &mut self.unconfirmed_restore,
                warn,
            ) {
                self.pending.push(lease);
            }
        }
    }

    pub(super) fn has_pending_restore_for(&self, take: &Take) -> bool {
        self.pending.iter().any(|lease| &lease.take == take)
    }

    pub(super) fn has_pending_restore(&self) -> bool {
        !self.pending.is_empty()
    }

    pub(super) fn has_unconfirmed_restore(&self) -> bool {
        self.unconfirmed_restore
    }
}

enum Acquisition {
    Active(Lease),
    Pending(Lease),
    Skipped,
}

fn acquire(
    backend: &mut impl Backend,
    device: &Device,
    take: &Take,
    still_current: impl Fn() -> bool,
    unconfirmed_restore: &mut bool,
    warn: &impl Fn(String),
) -> Acquisition {
    let snapshot = match backend.snapshot(device) {
        Ok(Some(snapshot)) => snapshot,
        Ok(None) => return Acquisition::Skipped,
        Err(error) => {
            warn(error);
            return Acquisition::Skipped;
        }
    };
    let muted_channels = snapshot
        .mutes
        .iter()
        .filter(|&&(ch, muted)| ch != 0 && muted)
        .count();
    if snapshot.mutes.contains(&(0, true)) || muted_channels == snapshot.channels as usize {
        return Acquisition::Skipped;
    }
    let has_master = snapshot.volumes.iter().any(|&(ch, _)| ch == 0);
    if let Err(error) = backend.watch_device(device, &snapshot) {
        warn(error);
        return Acquisition::Skipped;
    }

    let mut lease = Lease {
        take: take.clone(),
        device: device.clone(),
        mutes: snapshot.mutes,
        volumes: snapshot
            .volumes
            .into_iter()
            .filter(|&(ch, _)| !has_master || ch == 0)
            .map(|(channel, original)| Volume {
                channel,
                original,
                state: VolumeState::Original,
            })
            .collect(),
    };
    let outcome = match lease.duck(backend, still_current) {
        Ok(Some(Ownership::Held)) if lease.has_writes() => return Acquisition::Active(lease),
        Ok(Some(ownership)) => ownership.release(),
        Err(error) => {
            warn(error);
            None
        }
        Ok(None) => None,
    };
    let finished = match outcome {
        Some(outcome) if lease.has_writes() => {
            finish_release(backend, &lease, outcome, unconfirmed_restore, warn)
        }
        _ => restore(backend, &mut lease, true, unconfirmed_restore, warn),
    };
    if finished {
        Acquisition::Skipped
    } else {
        Acquisition::Pending(lease)
    }
}

fn restore(
    backend: &mut impl Backend,
    lease: &mut Lease,
    retry: bool,
    unconfirmed_restore: &mut bool,
    warn: &impl Fn(String),
) -> bool {
    let outcome = if lease.has_writes() {
        match lease.restore(backend, retry) {
            Ok(outcome) => outcome,
            Err(error) => {
                warn(error);
                Release::Pending
            }
        }
    } else {
        Release::Finished
    };
    if retry && matches!(outcome, Release::Unavailable) {
        warn("output disconnected before restoration".into());
    }
    finish_release(backend, lease, outcome, unconfirmed_restore, warn)
}

fn finish_release(
    backend: &mut impl Backend,
    lease: &Lease,
    outcome: Release,
    unconfirmed_restore: &mut bool,
    warn: &impl Fn(String),
) -> bool {
    match outcome {
        Release::Pending | Release::Unavailable => return false,
        Release::Finished => {}
        Release::Unconfirmed(reason) => {
            lease
                .take
                .unconfirmed_restore
                .store(true, Ordering::Relaxed);
            *unconfirmed_restore = true;
            warn(reason.into());
        }
    }
    backend.unwatch_device(&lease.device);
    true
}
