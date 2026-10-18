//! Windows adapter for the shared text-only activity capture engine.

use std::path::Path;

use super::platform::{
    ActivityPermission, ActivityPlatform, PermissionSnapshot, PermissionState, RawInputEvent,
    WindowDescriptor,
};

#[derive(Default)]
pub struct WinActivityPlatform;

impl WinActivityPlatform {
    pub fn new() -> Self {
        Self
    }
}

impl ActivityPlatform for WinActivityPlatform {
    fn permissions(&self) -> PermissionSnapshot {
        PermissionSnapshot {
            accessibility: PermissionState::Granted,
            screen_recording: PermissionState::Granted,
            input_monitoring: PermissionState::Granted,
        }
    }

    fn request_permission(&self, _permission: ActivityPermission) {}

    fn focused_window(&mut self) -> Option<WindowDescriptor> {
        None
    }

    fn secondary_windows(&mut self) -> Vec<WindowDescriptor> {
        Vec::new()
    }

    fn accessibility_text(&mut self, _window: &WindowDescriptor) -> Option<String> {
        None
    }

    fn ocr_text(&mut self, _window: &WindowDescriptor) -> Option<String> {
        None
    }

    fn clipboard_text_if_changed(&mut self) -> Option<String> {
        None
    }

    fn set_input_capture(&mut self, _enabled: bool) {}

    fn drain_input_events(&mut self) -> Vec<RawInputEvent> {
        Vec::new()
    }

    fn free_disk_bytes(&self, _path: &Path) -> Option<u64> {
        None
    }
}
