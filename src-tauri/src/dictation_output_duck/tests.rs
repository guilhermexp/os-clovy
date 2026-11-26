use super::{Backend, Device, Executor, Snapshot, RESTORE_TIMEOUT};
use std::{
    collections::BTreeMap,
    sync::{mpsc, Arc, Mutex},
    thread,
    time::Duration,
};

const WAIT: Duration = Duration::from_secs(3);

struct FakeDevice {
    uid: String,
    channels: u32,
    volumes: BTreeMap<u32, f32>,
    mutes: BTreeMap<u32, bool>,
}

struct FakeHal {
    output: Option<u32>,
    devices: BTreeMap<u32, FakeDevice>,
    writes: Vec<(u32, u32, f32)>,
    warnings: Vec<String>,
    default_reads: usize,
    snapshot_reads: usize,
    fail_channel: Option<u32>,
    fail_restore_once: bool,
    fail_readback_once: bool,
    failed_readback_ready: bool,
    quantize: bool,
    manual_volume_after_write: Option<f32>,
    defer_write: bool,
    deferred_writes: Vec<(u32, u32, f32)>,
    block_write: Option<(mpsc::Sender<()>, mpsc::Receiver<()>)>,
    first_readback: Option<mpsc::Sender<f32>>,
    next_write: Option<mpsc::Sender<(u32, u32, f32)>>,
}

impl FakeHal {
    fn device(&mut self, id: u32, uid: &str, channels: u32, volumes: &[(u32, f32)]) {
        self.devices.insert(
            id,
            FakeDevice {
                uid: uid.into(),
                channels,
                volumes: volumes.iter().copied().collect(),
                mutes: BTreeMap::from([(0, false)]),
            },
        );
    }
}

#[derive(Clone)]
struct Hal(Arc<Mutex<FakeHal>>);

impl Hal {
    fn volume_at(&self, id: u32, channel: u32) -> f32 {
        self.0.lock().unwrap().devices[&id].volumes[&channel]
    }

    fn change(&self, update: impl FnOnce(&mut FakeHal)) {
        update(&mut self.0.lock().unwrap());
    }
}

impl Backend for Hal {
    fn default_output(&mut self) -> Result<Option<Device>, String> {
        let mut hal = self.0.lock().unwrap();
        hal.default_reads += 1;
        Ok(hal.output.and_then(|id| {
            hal.devices.get(&id).map(|device| Device {
                id,
                uid: device.uid.clone(),
            })
        }))
    }

    fn identity(&mut self, id: u32) -> Result<Option<Device>, String> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .devices
            .get(&id)
            .map(|device| Device {
                id,
                uid: device.uid.clone(),
            }))
    }

    fn snapshot(&mut self, device: &Device) -> Result<Option<Snapshot>, String> {
        let mut hal = self.0.lock().unwrap();
        hal.snapshot_reads += 1;
        let device = &hal.devices[&device.id];
        if device.volumes.is_empty() {
            return Ok(None);
        }
        Ok(Some(Snapshot {
            channels: device.channels,
            volumes: device
                .volumes
                .iter()
                .map(|(&ch, &value)| (ch, value))
                .collect(),
            mutes: device
                .mutes
                .iter()
                .map(|(&ch, &value)| (ch, value))
                .collect(),
        }))
    }

    fn volume(&mut self, device: &Device, channel: u32) -> Result<f32, String> {
        let mut hal = self.0.lock().unwrap();
        if channel == 0 && std::mem::take(&mut hal.failed_readback_ready) {
            return Err("readback failed".into());
        }
        let actual = hal.devices[&device.id].volumes[&channel];
        if !hal.writes.is_empty() {
            if let Some(observed) = hal.first_readback.take() {
                observed.send(actual).unwrap();
            }
        }
        Ok(actual)
    }

    fn mute(&mut self, device: &Device, channel: u32) -> Result<bool, String> {
        Ok(self.0.lock().unwrap().devices[&device.id].mutes[&channel])
    }

    fn set_volume(&mut self, device: &Device, channel: u32, volume: f32) -> Result<(), String> {
        let blocked = self.0.lock().unwrap().block_write.take();
        if let Some((started, resume)) = blocked {
            started.send(()).unwrap();
            resume.recv().unwrap();
        }
        let mut hal = self.0.lock().unwrap();
        if hal.fail_channel == Some(channel) {
            return Err("channel write failed".into());
        }
        if volume > 0.2 && std::mem::take(&mut hal.fail_restore_once) {
            return Err("restore failed".into());
        }
        let actual = hal.manual_volume_after_write.take().unwrap_or_else(|| {
            if hal.quantize {
                (volume * 10.0).round() / 10.0
            } else {
                volume
            }
        });
        if hal.defer_write {
            hal.deferred_writes.push((device.id, channel, actual));
        } else {
            hal.devices
                .get_mut(&device.id)
                .unwrap()
                .volumes
                .insert(channel, actual);
        }
        let write = (device.id, channel, volume);
        hal.writes.push(write);
        if let Some(next_write) = hal.next_write.take() {
            next_write.send(write).unwrap();
        }
        if std::mem::take(&mut hal.fail_readback_once) {
            hal.failed_readback_ready = true;
        }
        Ok(())
    }

    fn watch_topology(&mut self) -> Result<(), String> {
        Ok(())
    }

    fn watch_device(&mut self, _: &Device, _: &Snapshot) -> Result<(), String> {
        Ok(())
    }

    fn unwatch_device(&mut self, _: &Device) {}
}

fn fixture(enabled: bool, channels: u32, volumes: &[(u32, f32)]) -> (Executor, Hal) {
    let mut hal = FakeHal {
        output: Some(10),
        devices: BTreeMap::new(),
        writes: Vec::new(),
        warnings: Vec::new(),
        default_reads: 0,
        snapshot_reads: 0,
        fail_channel: None,
        fail_restore_once: false,
        fail_readback_once: false,
        failed_readback_ready: false,
        quantize: false,
        manual_volume_after_write: None,
        defer_write: false,
        deferred_writes: Vec::new(),
        block_write: None,
        first_readback: None,
        next_write: None,
    };
    hal.device(10, "speakers", channels, volumes);
    let hal = Hal(Arc::new(Mutex::new(hal)));
    let logs = hal.clone();
    let executor = Executor::spawn(enabled, hal.clone(), move |warning| {
        logs.change(|hal| hal.warnings.push(warning));
    })
    .unwrap();
    executor.helper_started(1);
    (executor, hal)
}

fn listen(executor: &Executor, take: &str) {
    executor.event(1, "listening_started", take);
}

fn sync(executor: &Executor) {
    assert!(executor.wait_until_reconciled().wait(WAIT));
}

fn assert_volume(hal: &Hal, id: u32, channel: u32, expected: f32) {
    let actual = hal.volume_at(id, channel);
    assert!(
        (actual - expected).abs() < 0.000_01,
        "volume was {actual}, expected {expected}"
    );
}

#[test]
fn disabled_take_and_mic_test_never_access_the_output() {
    let (executor, hal) = fixture(false, 2, &[(0, 0.5)]);
    listen(&executor, "while-off");
    sync(&executor);
    executor.set_enabled(true);
    listen(&executor, "while-off");
    executor.event(1, "recording_ready", "while-off");
    executor.event(1, "mic_test_started", "mic");
    executor.event(1, "mic_test_ready", "mic");
    sync(&executor);
    assert_eq!(hal.0.lock().unwrap().default_reads, 0);
    assert_volume(&hal, 10, 0, 0.5);

    listen(&executor, "next-take");
    sync(&executor);
    assert_volume(&hal, 10, 0, 0.1);
}

#[test]
fn master_is_relative_and_restored_at_finalizing_before_recording_ready() {
    let (executor, hal) = fixture(true, 2, &[(0, 0.5), (1, 0.3), (2, 0.7)]);
    listen(&executor, "take");
    sync(&executor);
    assert_volume(&hal, 10, 0, 0.1);
    assert_volume(&hal, 10, 1, 0.3);
    assert_volume(&hal, 10, 2, 0.7);

    executor.event(1, "finalizing_transcript", "take");
    sync(&executor);
    assert_volume(&hal, 10, 0, 0.5);
}

#[test]
fn physical_channels_preserve_balance_including_channel_three() {
    let (executor, hal) = fixture(true, 3, &[(1, 0.5), (2, 0.8), (3, 0.25)]);
    listen(&executor, "take");
    sync(&executor);
    for (channel, expected) in [(1, 0.1), (2, 0.16), (3, 0.05)] {
        assert_volume(&hal, 10, channel, expected);
    }
    executor.event(1, "recording_discarded", "take");
    sync(&executor);
    for (channel, expected) in [(1, 0.5), (2, 0.8), (3, 0.25)] {
        assert_volume(&hal, 10, channel, expected);
    }
}

#[test]
fn preexisting_mute_and_unsupported_outputs_are_left_alone() {
    for (volumes, muted) in [(&[(0, 0.5)][..], true), (&[][..], false)] {
        let (executor, hal) = fixture(true, 2, volumes);
        hal.change(|hal| {
            hal.devices.get_mut(&10).unwrap().mutes.insert(0, muted);
        });
        listen(&executor, "take");
        sync(&executor);
        assert!(hal.0.lock().unwrap().writes.is_empty());
    }
}

#[test]
fn partial_channel_failure_rolls_back_only_the_successful_write() {
    let (executor, hal) = fixture(true, 3, &[(1, 0.5), (2, 0.8), (3, 0.25)]);
    hal.change(|hal| hal.fail_channel = Some(2));
    listen(&executor, "take");
    sync(&executor);
    assert_eq!(hal.0.lock().unwrap().writes, [(10, 1, 0.1), (10, 1, 0.5)]);
}

#[test]
fn attenuation_receipt_distinguishes_exact_and_quantized_writes() {
    for quantized in [false, true] {
        for deferred in [false, true] {
            for stop_during_write in [false, true] {
                let case = format!("quantized={quantized}, deferred={deferred}, stop_during_write={stop_during_write}");
                let (executor, hal) = fixture(true, 2, &[(0, 0.7)]);
                let (started, entered) = mpsc::channel();
                let (resume, suspended) = mpsc::channel();
                let (report_readback, first_readback) = mpsc::channel();
                hal.change(|hal| {
                    hal.first_readback = Some(report_readback);
                    hal.quantize = quantized;
                    hal.defer_write = deferred;
                    hal.block_write = Some((started, suspended));
                });
                listen(&executor, "take");
                entered.recv_timeout(WAIT).unwrap();
                if stop_during_write {
                    executor.event(1, "finalizing_transcript", "take");
                }
                resume.send(()).unwrap();
                let observed = first_readback.recv_timeout(WAIT).unwrap();
                if deferred {
                    assert_eq!(observed, 0.7, "{case}");
                    apply_deferred_write(&hal);
                    executor.properties_changed();
                }
                executor.event(1, "recording_ready", "take");

                assert_eq!(
                    executor.wait_until_reconciled().wait(WAIT),
                    !quantized,
                    "{case}"
                );
                if quantized {
                    assert_volume(&hal, 10, 0, 0.1);
                    assert_one_attenuation_write(&hal, 0.14);
                    assert_warning(&hal, "attenuation could not be confirmed");
                } else {
                    assert_volume(&hal, 10, 0, 0.7);
                    let state = hal.0.lock().unwrap();
                    assert_eq!(state.writes.len(), 2, "{case}");
                    assert_eq!(state.writes[1], (10, 0, 0.7), "{case}");
                    assert!(state.warnings.is_empty(), "{case}");
                }
            }
        }
    }
}

#[test]
fn asynchronous_quantization_stays_unconfirmed_after_the_callback_is_processed() {
    let (executor, hal) = fixture(true, 2, &[(0, 0.7)]);
    hal.change(|hal| {
        hal.quantize = true;
        hal.defer_write = true;
    });
    listen(&executor, "take");
    sync(&executor);
    apply_deferred_write(&hal);
    executor.properties_changed();
    let observed = executor.wait_until_reconciled();

    // The HAL notified before capture stopped. Keep the ambiguous outcome
    // through the later recording-ready receipt, even after retiring the lease.
    let _ = observed.wait(WAIT);
    executor.event(1, "recording_ready", "take");
    assert!(!executor.wait_until_reconciled().wait(WAIT));
    assert_warning(&hal, "attenuation could not be confirmed");
    assert_volume(&hal, 10, 0, 0.1);
    assert_one_attenuation_write(&hal, 0.14);
}

fn apply_deferred_write(hal: &Hal) {
    hal.change(|hal| {
        hal.defer_write = false;
        for (id, channel, volume) in std::mem::take(&mut hal.deferred_writes) {
            hal.devices
                .get_mut(&id)
                .unwrap()
                .volumes
                .insert(channel, volume);
        }
    });
}

fn assert_warning(hal: &Hal, expected: &str) {
    let state = hal.0.lock().unwrap();
    assert!(
        state
            .warnings
            .iter()
            .any(|warning| warning.contains(expected)),
        "expected {expected:?}, got {:?}",
        state.warnings
    );
}

fn assert_one_attenuation_write(hal: &Hal, expected: f32) {
    let state = hal.0.lock().unwrap();
    assert_eq!(state.writes.len(), 1);
    let (device, element, target) = state.writes[0];
    assert_eq!((device, element), (10, 0));
    assert!((target - expected).abs() < f32::EPSILON);
}

fn start_take_during_previous_restore() -> (Executor, Hal) {
    let (executor, hal) = fixture(true, 2, &[(0, 0.5)]);
    listen(&executor, "a");
    sync(&executor);
    assert_volume(&hal, 10, 0, 0.1);

    hal.change(|hal| hal.defer_write = true);
    executor.event(1, "finalizing_transcript", "a");
    listen(&executor, "b");
    // B's barrier completes only after the executor processes its start.
    // A's restore is still pending and B must not snapshot A's reduced level.
    sync(&executor);
    assert_volume(&hal, 10, 0, 0.1);
    {
        let state = hal.0.lock().unwrap();
        assert_eq!(state.writes, [(10, 0, 0.1), (10, 0, 0.5)]);
        assert_eq!(state.deferred_writes, [(10, 0, 0.5)]);
        assert_eq!(state.snapshot_reads, 1);
    }
    (executor, hal)
}

#[test]
fn confirming_a_previous_restore_resumes_the_waiting_take_on_the_same_output() {
    let (executor, hal) = start_take_during_previous_restore();
    let (written, next_write) = mpsc::channel();
    hal.change(|hal| hal.next_write = Some(written));
    apply_deferred_write(&hal);
    executor.properties_changed();

    // Wait on the write itself so another executor event cannot wake B.
    assert_eq!(
        next_write
            .recv_timeout(WAIT)
            .expect("the restore callback must wake B"),
        (10, 0, 0.1)
    );
    assert_volume(&hal, 10, 0, 0.1);

    executor.event(1, "recording_ready", "b");
    sync(&executor);
    assert_volume(&hal, 10, 0, 0.5);
    let state = hal.0.lock().unwrap();
    assert_eq!(
        state.writes,
        [(10, 0, 0.1), (10, 0, 0.5), (10, 0, 0.1), (10, 0, 0.5)]
    );
    assert_eq!(state.snapshot_reads, 2);
    assert!(state.warnings.is_empty());
}

#[test]
fn a_waiting_take_cancelled_or_disabled_before_the_restore_callback_stays_idle() {
    for cancel in [true, false] {
        let (executor, hal) = start_take_during_previous_restore();
        if cancel {
            executor.event(1, "recording_discarded", "b");
        } else {
            executor.set_enabled(false);
            executor.set_enabled(true);
            listen(&executor, "b");
        }
        apply_deferred_write(&hal);
        executor.properties_changed();
        sync(&executor);
        assert_volume(&hal, 10, 0, 0.5);
        let state = hal.0.lock().unwrap();
        assert_eq!(state.writes, [(10, 0, 0.1), (10, 0, 0.5)]);
        assert_eq!(state.snapshot_reads, 1);
    }
}

#[test]
fn notifications_do_not_retry_a_resolved_route_after_override_or_unsupported_output() {
    for supported in [true, false] {
        let volumes: &[(u32, f32)] = if supported { &[(0, 0.5)] } else { &[] };
        let (executor, hal) = fixture(true, 2, volumes);
        listen(&executor, "take");
        sync(&executor);

        if supported {
            assert_volume(&hal, 10, 0, 0.1);
            hal.change(|hal| {
                hal.devices.get_mut(&10).unwrap().volumes.insert(0, 0.37);
            });
            executor.properties_changed();
            sync(&executor);
        }

        executor.properties_changed();
        executor.device_changed();
        sync(&executor);
        {
            let state = hal.0.lock().unwrap();
            assert_eq!(state.snapshot_reads, 1);
            assert_eq!(state.writes.len(), usize::from(supported));
        }
        if supported {
            executor.event(1, "recording_ready", "take");
            sync(&executor);
            assert_volume(&hal, 10, 0, 0.37);
            assert_one_attenuation_write(&hal, 0.1);
        }
    }
}

#[test]
fn a_new_take_can_confirm_its_own_restore_after_an_ambiguous_take() {
    let (executor, hal) = fixture(true, 2, &[(0, 0.5)]);
    hal.change(|hal| hal.manual_volume_after_write = Some(0.3));
    listen(&executor, "ambiguous");
    assert!(!executor.wait_until_reconciled().wait(WAIT));
    assert_one_attenuation_write(&hal, 0.1);
    executor.event(1, "recording_ready", "ambiguous");
    assert!(!executor.wait_until_reconciled().wait(WAIT));
    assert!(!executor.wait_until_reconciled().wait(WAIT));

    listen(&executor, "next");
    sync(&executor);
    assert_volume(&hal, 10, 0, 0.06);
    executor.event(1, "recording_ready", "next");
    sync(&executor);
    assert_volume(&hal, 10, 0, 0.3);

    assert!(!executor.shutdown().wait(WAIT));
    assert_warning(&hal, "before shutdown");
}

#[test]
fn a_restore_readback_after_the_receipt_deadline_cannot_confirm_that_receipt() {
    let (executor, hal) = fixture(true, 2, &[(0, 0.5)]);
    listen(&executor, "take");
    sync(&executor);
    let (started, entered) = mpsc::channel();
    let (resume, suspended) = mpsc::channel();
    hal.change(|hal| hal.block_write = Some((started, suspended)));
    executor.event(1, "recording_ready", "take");
    entered.recv_timeout(WAIT).unwrap();

    let receipt = executor.wait_until_reconciled();
    thread::sleep(RESTORE_TIMEOUT);
    resume.send(()).unwrap();
    assert!(!receipt.wait(WAIT));
    assert_volume(&hal, 10, 0, 0.5);
    assert_warning(&hal, "restoration was not confirmed before the deadline");
}

#[test]
fn failed_readback_reverts_when_the_requested_value_can_be_confirmed() {
    let (executor, hal) = fixture(true, 2, &[(0, 0.5)]);
    hal.change(|hal| hal.fail_readback_once = true);
    listen(&executor, "take");
    sync(&executor);
    assert_eq!(hal.0.lock().unwrap().writes, [(10, 0, 0.1), (10, 0, 0.5)]);
}

#[test]
fn late_hal_notification_reconciles_a_write_after_stop() {
    let (executor, hal) = fixture(true, 2, &[(0, 0.5)]);
    hal.change(|hal| hal.defer_write = true);
    listen(&executor, "take");
    sync(&executor);
    assert_volume(&hal, 10, 0, 0.5);
    assert_eq!(hal.0.lock().unwrap().writes, [(10, 0, 0.1)]);

    executor.event(1, "finalizing_transcript", "take");
    let completion = executor.wait_until_reconciled();
    hal.change(|hal| {
        hal.defer_write = false;
        for (id, channel, volume) in std::mem::take(&mut hal.deferred_writes) {
            hal.devices
                .get_mut(&id)
                .unwrap()
                .volumes
                .insert(channel, volume);
        }
    });
    executor.properties_changed();
    assert!(completion.wait(WAIT));
    assert_volume(&hal, 10, 0, 0.5);
}

#[test]
fn disconnect_waits_for_the_same_output_and_never_writes_to_a_fallback() {
    let (executor, hal) = fixture(true, 2, &[(0, 0.5)]);
    listen(&executor, "take");
    sync(&executor);
    let mut unplugged = None;
    hal.change(|hal| {
        unplugged = hal.devices.remove(&10);
        hal.device(11, "headphones", 2, &[(0, 0.75)]);
        hal.output = Some(11);
    });
    executor.event(1, "recording_discarded", "take");
    executor.device_changed();
    assert!(!executor
        .wait_until_reconciled()
        .wait(Duration::from_millis(50)));
    assert_volume(&hal, 11, 0, 0.75);
    hal.change(|hal| {
        hal.devices.insert(10, unplugged.take().unwrap());
    });
    executor.device_changed();
    sync(&executor);
    assert_volume(&hal, 10, 0, 0.5);
    assert_volume(&hal, 11, 0, 0.75);
}

#[test]
fn manual_increase_and_decrease_during_acquisition_are_not_adopted() {
    for manual in [0.3, 0.8] {
        let (executor, hal) = fixture(true, 2, &[(0, 0.5)]);
        hal.change(|hal| hal.manual_volume_after_write = Some(manual));
        listen(&executor, "take");
        let _ = executor.wait_until_reconciled().wait(WAIT);
        executor.event(1, "recording_discarded", "take");
        assert_volume(&hal, 10, 0, manual);
        let restored = executor.wait_until_reconciled().wait(WAIT);
        assert_volume(&hal, 10, 0, manual);
        assert!(!restored, "manual volume {manual} must remain unconfirmed");
        assert_one_attenuation_write(&hal, 0.1);
        assert_warning(&hal, "attenuation could not be confirmed");
    }
}

#[test]
fn restore_set_without_original_readback_has_a_negative_receipt() {
    let (executor, hal) = fixture(true, 2, &[(0, 0.5)]);
    listen(&executor, "take");
    sync(&executor);
    hal.change(|hal| hal.defer_write = true);

    executor.event(1, "recording_ready", "take");
    assert!(!executor.wait_until_reconciled().wait(WAIT));
    assert_volume(&hal, 10, 0, 0.1);
    assert_eq!(hal.0.lock().unwrap().writes, [(10, 0, 0.1), (10, 0, 0.5)]);
    assert_warning(&hal, "restoration was not confirmed before the deadline");
}

#[test]
fn a_divergent_restore_readback_does_not_confirm_shutdown() {
    let (executor, hal) = fixture(true, 2, &[(0, 0.5)]);
    listen(&executor, "take");
    sync(&executor);
    hal.change(|hal| hal.manual_volume_after_write = Some(0.35));
    assert!(!executor.shutdown().wait(WAIT));
    assert_volume(&hal, 10, 0, 0.35);
    assert_eq!(hal.0.lock().unwrap().writes, [(10, 0, 0.1), (10, 0, 0.5)]);
    assert_warning(&hal, "restoration readback could not be confirmed");
    assert_warning(&hal, "before shutdown");
}

#[test]
fn observed_manual_volume_or_mute_change_retires_ownership() {
    for mute in [false, true] {
        let (executor, hal) = fixture(true, 2, &[(0, 0.5)]);
        listen(&executor, "take");
        sync(&executor);
        assert_volume(&hal, 10, 0, 0.1);
        hal.change(|hal| {
            let device = hal.devices.get_mut(&10).unwrap();
            if mute {
                device.mutes.insert(0, true);
            } else {
                device.volumes.insert(0, 0.37);
            }
        });
        executor.properties_changed();
        sync(&executor);

        // Returning to the imposed snapshot does not give ownership back.
        hal.change(|hal| {
            let device = hal.devices.get_mut(&10).unwrap();
            device.mutes.insert(0, false);
            device.volumes.insert(0, 0.1);
        });
        executor.event(1, "recording_discarded", "take");
        sync(&executor);
        assert_volume(&hal, 10, 0, 0.1);
    }
}

#[test]
fn route_change_uses_its_own_snapshot_and_restores_the_old_device() {
    let (executor, hal) = fixture(true, 2, &[(0, 0.5)]);
    listen(&executor, "take");
    sync(&executor);
    hal.change(|hal| {
        hal.device(11, "headphones", 2, &[(0, 0.75)]);
        hal.output = Some(11);
    });
    executor.device_changed();
    sync(&executor);
    assert_volume(&hal, 10, 0, 0.5);
    assert_volume(&hal, 11, 0, 0.15);
    executor.event(1, "error", "take");
    sync(&executor);
    assert_volume(&hal, 11, 0, 0.75);
}

#[test]
fn recycled_id_cannot_receive_the_old_devices_restore() {
    let (executor, hal) = fixture(true, 2, &[(0, 0.5)]);
    listen(&executor, "take");
    sync(&executor);
    hal.change(|hal| hal.device(10, "usb", 2, &[(0, 0.1)]));
    executor.event(1, "error", "take");
    assert!(!executor.wait_until_reconciled().wait(WAIT));
    assert_warning(&hal, "output identity changed");
    assert_eq!(hal.0.lock().unwrap().writes, [(10, 0, 0.1)]);
}

#[test]
fn duplicate_start_and_stale_terminal_events_cannot_change_the_current_take() {
    let (executor, hal) = fixture(true, 2, &[(0, 0.5)]);
    listen(&executor, "old");
    sync(&executor);
    listen(&executor, "old");
    sync(&executor);
    assert_eq!(hal.0.lock().unwrap().writes, [(10, 0, 0.1)]);

    listen(&executor, "new");
    sync(&executor);
    executor.event(1, "error", "old");
    executor.event(1, "finalizing_transcript", "old");
    sync(&executor);
    assert_volume(&hal, 10, 0, 0.1);

    executor.event(1, "recording_ready", "new");
    listen(&executor, "new");
    sync(&executor);
    assert_volume(&hal, 10, 0, 0.5);
}

#[test]
fn only_the_live_helper_generation_has_authority() {
    let (executor, hal) = fixture(true, 2, &[(0, 0.5)]);
    listen(&executor, "old");
    sync(&executor);
    executor.helper_started(2);
    executor.event(2, "listening_started", "new");
    sync(&executor);

    executor.helper_exited(1);
    executor.event(1, "recording_discarded", "new");
    listen(&executor, "late-start");
    sync(&executor);
    assert_volume(&hal, 10, 0, 0.1);

    executor.helper_exited(2);
    sync(&executor);
    assert_volume(&hal, 10, 0, 0.5);
}

#[test]
fn stop_off_and_shutdown_invalidate_an_in_flight_write() {
    for action in ["stop", "off", "shutdown"] {
        let (executor, hal) = fixture(true, 2, &[(1, 0.5), (2, 0.8)]);
        let (started, entered) = mpsc::channel();
        let (resume, suspended) = mpsc::channel();
        hal.change(|hal| hal.block_write = Some((started, suspended)));
        listen(&executor, "take");
        entered.recv_timeout(WAIT).unwrap();

        let completion = match action {
            "stop" => {
                executor.event(1, "finalizing_transcript", "take");
                executor.wait_until_reconciled()
            }
            "off" => {
                executor.set_enabled(false);
                executor.set_enabled(true);
                listen(&executor, "take");
                executor.wait_until_reconciled()
            }
            _ => executor.shutdown(),
        };
        resume.send(()).unwrap();
        assert!(completion.wait(WAIT));
        assert_eq!(hal.0.lock().unwrap().writes, [(10, 1, 0.1), (10, 1, 0.5)]);
    }
}

#[test]
fn shutdown_retries_a_pending_restore_before_acknowledging() {
    let (executor, hal) = fixture(true, 2, &[(0, 0.5)]);
    listen(&executor, "take");
    sync(&executor);
    hal.change(|hal| hal.fail_restore_once = true);
    executor.event(1, "finalizing_transcript", "take");
    assert!(executor.shutdown().wait(WAIT));
    assert_volume(&hal, 10, 0, 0.5);
}
