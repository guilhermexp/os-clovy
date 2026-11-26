use super::{
    policy::{Backend, Device, Snapshot},
    Executor, Signal,
};
use core_foundation::{
    base::TCFType,
    string::{CFString, CFStringRef},
};
use std::{
    ffi::c_void,
    io,
    mem::{offset_of, size_of, MaybeUninit},
    ptr,
    sync::{mpsc, OnceLock},
};

type ObjectId = u32;
type Status = i32;
type Listener = extern "C" fn(ObjectId, u32, *const Address, *mut c_void) -> Status;

const SYSTEM: ObjectId = 1;
const UNKNOWN: ObjectId = 0;
const GLOBAL: u32 = u32::from_be_bytes(*b"glob");
const OUTPUT: u32 = u32::from_be_bytes(*b"outp");
const DEFAULT_OUTPUT: u32 = u32::from_be_bytes(*b"dOut");
const DEVICES: u32 = u32::from_be_bytes(*b"dev#");
const UID: u32 = u32::from_be_bytes(*b"uid ");
const STREAM_CONFIGURATION: u32 = u32::from_be_bytes(*b"slay");
const VOLUME: u32 = u32::from_be_bytes(*b"volm");
const MUTE: u32 = u32::from_be_bytes(*b"mute");

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(C)]
struct Address {
    selector: u32,
    scope: u32,
    element: u32,
}

impl Address {
    const fn global(selector: u32) -> Self {
        Self {
            selector,
            scope: GLOBAL,
            element: 0,
        }
    }

    const fn output(selector: u32, channel: u32) -> Self {
        Self {
            selector,
            scope: OUTPUT,
            element: channel,
        }
    }
}

#[repr(C)]
struct AudioBuffer {
    channels: u32,
    byte_count: u32,
    data: *mut c_void,
}

#[repr(C)]
struct AudioBufferList {
    count: u32,
    buffers: [AudioBuffer; 1],
}

#[link(name = "CoreAudio", kind = "framework")]
extern "C" {
    fn AudioObjectHasProperty(id: ObjectId, address: *const Address) -> u8;
    fn AudioObjectIsPropertySettable(
        id: ObjectId,
        address: *const Address,
        settable: *mut u8,
    ) -> Status;
    fn AudioObjectGetPropertyDataSize(
        id: ObjectId,
        address: *const Address,
        qualifier_size: u32,
        qualifier: *const c_void,
        data_size: *mut u32,
    ) -> Status;
    fn AudioObjectGetPropertyData(
        id: ObjectId,
        address: *const Address,
        qualifier_size: u32,
        qualifier: *const c_void,
        data_size: *mut u32,
        data: *mut c_void,
    ) -> Status;
    fn AudioObjectSetPropertyData(
        id: ObjectId,
        address: *const Address,
        qualifier_size: u32,
        qualifier: *const c_void,
        data_size: u32,
        data: *const c_void,
    ) -> Status;
    fn AudioObjectAddPropertyListener(
        id: ObjectId,
        address: *const Address,
        listener: Listener,
        data: *mut c_void,
    ) -> Status;
    fn AudioObjectRemovePropertyListener(
        id: ObjectId,
        address: *const Address,
        listener: Listener,
        data: *mut c_void,
    ) -> Status;
}

// The app starts one executor. A process-lifetime sender keeps the callback
// context valid even if CoreAudio delivers a notification during removal.
static NOTIFICATIONS: OnceLock<mpsc::Sender<Signal>> = OnceLock::new();

extern "C" fn topology_changed(_: ObjectId, _: u32, _: *const Address, _: *mut c_void) -> Status {
    if let Some(wake) = NOTIFICATIONS.get() {
        let _ = wake.send(Signal::Topology);
    }
    0
}

extern "C" fn controls_changed(_: ObjectId, _: u32, _: *const Address, _: *mut c_void) -> Status {
    if let Some(wake) = NOTIFICATIONS.get() {
        let _ = wake.send(Signal::Properties);
    }
    0
}

pub(crate) fn start(enabled: bool) -> io::Result<Executor> {
    Executor::spawn_with(
        enabled,
        |wake| {
            let _ = NOTIFICATIONS.set(wake);
            Hal::default()
        },
        |error| tracing::warn!(%error, "dictation output volume"),
    )
}

#[derive(Default)]
struct Hal {
    listeners: Vec<Subscription>,
}

struct Subscription {
    id: ObjectId,
    address: Address,
    callback: Listener,
}

impl Subscription {
    fn add(id: ObjectId, address: Address, callback: Listener) -> Result<Self, String> {
        status("add output property listener", unsafe {
            AudioObjectAddPropertyListener(id, &address, callback, ptr::null_mut())
        })?;
        Ok(Self {
            id,
            address,
            callback,
        })
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        // A disconnected device may have removed these subscriptions already.
        let _ = unsafe {
            AudioObjectRemovePropertyListener(
                self.id,
                &self.address,
                self.callback,
                ptr::null_mut(),
            )
        };
    }
}

impl Backend for Hal {
    fn default_output(&mut self) -> Result<Option<Device>, String> {
        self.identity(get(SYSTEM, Address::global(DEFAULT_OUTPUT), 0_u32)?)
    }

    fn identity(&mut self, id: ObjectId) -> Result<Option<Device>, String> {
        let address = Address::global(UID);
        if id == UNKNOWN || !has(id, address) {
            return Ok(None);
        }
        let uid: CFStringRef = get(id, address, ptr::null())?;
        if uid.is_null() {
            return Err("CoreAudio returned a null output UID".into());
        }
        // The HAL hands the caller ownership of the UID CFString.
        let uid = unsafe { CFString::wrap_under_create_rule(uid) }.to_string();
        if uid.is_empty() {
            return Err("CoreAudio returned an empty output UID".into());
        }
        Ok(Some(Device { id, uid }))
    }

    fn snapshot(&mut self, device: &Device) -> Result<Option<Snapshot>, String> {
        let channels = output_channels(device.id)?;
        if channels == 0 {
            return Ok(None);
        }
        let volumes = if writable(device.id, Address::output(VOLUME, 0))? {
            vec![(0, self.volume(device, 0)?)]
        } else {
            let mut volumes = Vec::new();
            for channel in 1..=channels {
                if !writable(device.id, Address::output(VOLUME, channel))? {
                    // A partial set would change the relative channel levels.
                    return Ok(None);
                }
                volumes.push((channel, self.volume(device, channel)?));
            }
            volumes
        };
        let mut mutes = Vec::new();
        for channel in 0..=channels {
            if has(device.id, Address::output(MUTE, channel)) {
                mutes.push((channel, self.mute(device, channel)?));
            }
        }
        Ok(Some(Snapshot {
            volumes,
            mutes,
            channels,
        }))
    }

    fn volume(&mut self, device: &Device, channel: u32) -> Result<f32, String> {
        let scalar = get(device.id, Address::output(VOLUME, channel), 0_f32)?;
        if !(0.0..=1.0).contains(&scalar) {
            return Err("CoreAudio returned an invalid output volume".into());
        }
        Ok(scalar)
    }

    fn mute(&mut self, device: &Device, channel: u32) -> Result<bool, String> {
        Ok(get(device.id, Address::output(MUTE, channel), 0_u32)? != 0)
    }

    fn set_volume(&mut self, device: &Device, channel: u32, volume: f32) -> Result<(), String> {
        // The HAL has no compare-and-set. Reconfirm the UID as near the
        // property write as possible to avoid targeting a recycled object ID.
        if self.identity(device.id)?.as_ref() != Some(device) {
            return Err("output identity changed before the volume write".into());
        }
        status("set output volume", unsafe {
            AudioObjectSetPropertyData(
                device.id,
                &Address::output(VOLUME, channel),
                0,
                ptr::null(),
                size_of::<f32>() as u32,
                (&volume as *const f32).cast(),
            )
        })
    }

    fn watch_topology(&mut self) -> Result<(), String> {
        let default = Subscription::add(SYSTEM, Address::global(DEFAULT_OUTPUT), topology_changed)?;
        let devices = Subscription::add(SYSTEM, Address::global(DEVICES), topology_changed)?;
        self.listeners.extend([default, devices]);
        Ok(())
    }

    fn watch_device(&mut self, device: &Device, snapshot: &Snapshot) -> Result<(), String> {
        let addresses = snapshot
            .volumes
            .iter()
            .map(|&(channel, _)| Address::output(VOLUME, channel))
            .chain(
                snapshot
                    .mutes
                    .iter()
                    .map(|&(channel, _)| Address::output(MUTE, channel)),
            );
        let registrations = addresses
            .map(|address| Subscription::add(device.id, address, controls_changed))
            .collect::<Result<Vec<_>, _>>()?;
        self.listeners.extend(registrations);
        Ok(())
    }

    fn unwatch_device(&mut self, device: &Device) {
        self.listeners
            .retain(|registration| registration.id != device.id);
    }
}

fn has(id: ObjectId, address: Address) -> bool {
    unsafe { AudioObjectHasProperty(id, &address) != 0 }
}

fn writable(id: ObjectId, address: Address) -> Result<bool, String> {
    if !has(id, address) {
        return Ok(false);
    }
    let mut settable = 0;
    status("query output volume writability", unsafe {
        AudioObjectIsPropertySettable(id, &address, &mut settable)
    })?;
    Ok(settable != 0)
}

fn get<T: Copy>(id: ObjectId, address: Address, mut value: T) -> Result<T, String> {
    let expected = size_of::<T>() as u32;
    let mut size = expected;
    status("read output property", unsafe {
        AudioObjectGetPropertyData(
            id,
            &address,
            0,
            ptr::null(),
            &mut size,
            (&mut value as *mut T).cast(),
        )
    })?;
    if size != expected {
        return Err("CoreAudio returned an unexpected output property size".into());
    }
    Ok(value)
}

fn output_channels(id: ObjectId) -> Result<u32, String> {
    let address = Address::output(STREAM_CONFIGURATION, 0);
    let mut size = 0;
    status("size output stream configuration", unsafe {
        AudioObjectGetPropertyDataSize(id, &address, 0, ptr::null(), &mut size)
    })?;
    if size < size_of::<u32>() as u32 {
        return Err("CoreAudio returned a short output stream configuration".into());
    }
    // AudioBuffer-aligned storage for the variable-length AudioBufferList.
    let items = (size as usize).div_ceil(size_of::<AudioBuffer>());
    let mut buffers = Vec::<MaybeUninit<AudioBuffer>>::new();
    buffers
        .try_reserve_exact(items)
        .map_err(|error| error.to_string())?;
    buffers.resize_with(items, MaybeUninit::uninit);
    let list = buffers.as_mut_ptr().cast::<u8>();
    status("read output stream configuration", unsafe {
        AudioObjectGetPropertyData(id, &address, 0, ptr::null(), &mut size, list.cast())
    })?;
    if (size as usize) < size_of::<u32>() || size as usize > items * size_of::<AudioBuffer>() {
        return Err("CoreAudio returned an invalid output stream configuration size".into());
    }
    let count = unsafe { list.cast::<u32>().read() } as usize;
    if count == 0 {
        return Ok(0);
    }
    let offset = offset_of!(AudioBufferList, buffers);
    let available = (size as usize).saturating_sub(offset) / size_of::<AudioBuffer>();
    if count > available {
        return Err("CoreAudio returned a truncated output buffer list".into());
    }
    let data = unsafe { list.add(offset).cast::<AudioBuffer>() };
    (0..count).try_fold(0_u32, |channels, index| {
        let count = unsafe { ptr::addr_of!((*data.add(index)).channels).read() };
        channels
            .checked_add(count)
            .ok_or_else(|| "output channel count overflow".into())
    })
}

fn status(operation: &str, status: Status) -> Result<(), String> {
    if status == 0 {
        Ok(())
    } else {
        Err(format!("CoreAudio {operation} failed ({status})"))
    }
}
