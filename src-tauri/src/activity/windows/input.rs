//! Passive, privacy-preserving input monitoring on Windows using Raw Input.

use chrono::{DateTime, Utc};
use crate::activity::platform::RawInputEvent;

/// Normalizes raw keyboard input flags into a Key event.
/// Stub for 1.5/RED. Real implementation in GREEN 4.2.
pub(crate) fn normalize_keyboard_input(_flags: u16, _at: DateTime<Utc>) -> Option<RawInputEvent> {
    None
}

/// Normalizes raw mouse button flags into a Click event.
/// Stub for 1.5/RED. Real implementation in GREEN 4.2.
pub(crate) fn normalize_mouse_input(_button_flags: u16, _at: DateTime<Utc>) -> Option<RawInputEvent> {
    None
}

/// Production buffer state used by the RawInput window callback.
/// Stub for 1.5/RED. Real implementation in GREEN 4.2.
#[derive(Debug, Default)]
pub(crate) struct InputBuffer;

impl InputBuffer {
    pub(crate) fn set_enabled(&mut self, _enabled: bool) {}

    pub(crate) fn record_event(&mut self, _event: RawInputEvent) {}

    pub(crate) fn drain(&mut self) -> Vec<RawInputEvent> {
        Vec::new()
    }
}

#[derive(Debug, Default)]
pub(crate) struct InputCapture {
    _buffer: InputBuffer,
}

impl InputCapture {
    pub(crate) fn set_enabled(&mut self, enabled: bool) {
        self._buffer.set_enabled(enabled);
    }

    pub(crate) fn drain(&mut self) -> Vec<RawInputEvent> {
        self._buffer.drain()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_raw_input_keyboard_normalization_and_privacy() {
        let now = Utc::now();

        // Key-down (RI_KEY_MAKE = 0, no RI_KEY_BREAK bit): MUST return Key with characters: None
        let down_event = normalize_keyboard_input(0, now);
        assert!(
            down_event.is_some(),
            "Key-down must produce a RawInputEvent::Key"
        );
        match down_event.unwrap() {
            RawInputEvent::Key { at, characters } => {
                assert_eq!(at, now);
                assert!(
                    characters.is_none(),
                    "Windows RawInput MUST NEVER record virtualkeys, scancodes or characters"
                );
            }
            _ => panic!("Expected RawInputEvent::Key"),
        }

        // Key-up (RI_KEY_BREAK = 1): MUST return None
        let up_event = normalize_keyboard_input(1, now);
        assert_eq!(
            up_event, None,
            "Key-up events must be discarded by normalizer"
        );
    }

    #[test]
    fn test_raw_input_mouse_normalization() {
        let now = Utc::now();

        // Mouse-down (RI_MOUSE_LEFT_BUTTON_DOWN = 1): MUST return Click with at
        let click_event = normalize_mouse_input(1, now);
        assert!(
            click_event.is_some(),
            "Mouse button down must produce a RawInputEvent::Click"
        );
        match click_event.unwrap() {
            RawInputEvent::Click { at } => {
                assert_eq!(at, now);
            }
            _ => panic!("Expected RawInputEvent::Click"),
        }

        // Mouse-up (RI_MOUSE_LEFT_BUTTON_UP = 2): MUST return None
        let up_event = normalize_mouse_input(2, now);
        assert_eq!(
            up_event, None,
            "Mouse button up events must be discarded by normalizer"
        );

        // Mouse move without buttons (0): MUST return None
        let move_event = normalize_mouse_input(0, now);
        assert_eq!(
            move_event, None,
            "Mouse movement without clicks must be discarded by normalizer"
        );
    }

    #[test]
    fn test_raw_input_buffer_lifecycle_and_discard() {
        let mut buffer = InputBuffer::default();
        buffer.set_enabled(true);

        let t1 = Utc::now();
        let event1 = RawInputEvent::Key {
            at: t1,
            characters: None,
        };
        buffer.record_event(event1);

        // Disabling without draining must discard event1
        buffer.set_enabled(false);

        // Re-enabling and recording event2
        buffer.set_enabled(true);
        let t2 = Utc::now();
        let event2 = RawInputEvent::Click { at: t2 };
        buffer.record_event(event2.clone());

        let drained = buffer.drain();
        assert_eq!(
            drained,
            vec![event2],
            "Drained events must match EXACTLY [ClickT2] recorded after re-enable"
        );
    }
}
