//! Lone-key shortcuts on Windows, as pure state machines over the low-level
//! keyboard hook's key events (no Win32 here, so the rules are testable
//! anywhere with `rustc --test`).
//!
//! A lone modifier (Right Ctrl, Right Alt/AltGr, Right Shift) is not a
//! hotkey Windows can register, and it must not get in the way of normal
//! use: Right Ctrl + C still copies. It counts only when pressed and
//! released with no other key in between.

/// Virtual-key codes the rules need.
pub const VK_LSHIFT: u32 = 0xA0;
pub const VK_RSHIFT: u32 = 0xA1;
pub const VK_LCONTROL: u32 = 0xA2;
pub const VK_RCONTROL: u32 = 0xA3;
pub const VK_LMENU: u32 = 0xA4;
pub const VK_RMENU: u32 = 0xA5;

/// What a lone-modifier shortcut saw.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SoloEvent {
    /// The key went down with nothing else held.
    Down,
    /// Another key went down while it was held: not a lone press.
    Chord,
    /// It went up; `clean` when no other key joined since it went down.
    Up { clean: bool },
}

/// Tracks one lone-modifier shortcut.
#[derive(Clone, Debug)]
pub struct SoloModifier {
    target: u32,
}

impl SoloModifier {
    pub fn new(target: u32) -> Self {
        Self { target }
    }

    pub fn target(&self) -> u32 {
        self.target
    }

    /// Feeds one key edge (auto-repeat included); returns what it means for
    /// this shortcut.
    pub fn on_key(&mut self, _vk: u32, _is_down: bool) -> Option<SoloEvent> {
        None
    }
}

/// What a finished capture that never formed a chord recorded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SoloCapture {
    /// A right-hand modifier pressed and released alone.
    Modifier(u32),
    /// A left-hand modifier pressed and released alone: refused, since the
    /// left keys are in constant use.
    LeftModifier(u32),
}

/// Watches a shortcut capture for a lone modifier and for whether any key
/// reached Windows at all.
#[derive(Clone, Debug, Default)]
pub struct CaptureTracker;

impl CaptureTracker {
    pub fn on_key(&mut self, _vk: u32, _is_down: bool, _is_modifier: bool) -> Option<SoloCapture> {
        None
    }

    /// Whether any key event reached the hook during the capture.
    pub fn saw_any_key(&self) -> bool {
        false
    }
}

/// The DOM `code` of a key that works as a shortcut on its own, and a label.
pub fn solo_code(vk: u32) -> Option<(&'static str, &'static str)> {
    let _ = vk;
    None
}

/// The virtual key of a lone-key `code`.
pub fn solo_vk(code: &str) -> Option<u32> {
    let _ = code;
    None
}

/// Whether `code` names a lone modifier (handled by the hook, not
/// `RegisterHotKey`).
pub fn is_solo_modifier_code(code: &str) -> bool {
    let _ = code;
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY_C: u32 = 0x43;

    fn feed(machine: &mut SoloModifier, events: &[(u32, bool)]) -> Vec<SoloEvent> {
        events
            .iter()
            .filter_map(|(vk, down)| machine.on_key(*vk, *down))
            .collect()
    }

    #[test]
    fn right_ctrl_alone_goes_down_and_up_clean() {
        let mut machine = SoloModifier::new(VK_RCONTROL);
        assert_eq!(
            feed(&mut machine, &[(VK_RCONTROL, true), (VK_RCONTROL, false)]),
            [SoloEvent::Down, SoloEvent::Up { clean: true }]
        );
    }

    #[test]
    fn right_ctrl_with_c_is_a_chord_not_a_shortcut() {
        let mut machine = SoloModifier::new(VK_RCONTROL);
        assert_eq!(
            feed(
                &mut machine,
                &[
                    (VK_RCONTROL, true),
                    (KEY_C, true),
                    (KEY_C, false),
                    (VK_RCONTROL, false)
                ]
            ),
            [
                SoloEvent::Down,
                SoloEvent::Chord,
                SoloEvent::Up { clean: false }
            ]
        );
    }

    #[test]
    fn auto_repeat_of_the_held_key_is_ignored() {
        let mut machine = SoloModifier::new(VK_RCONTROL);
        assert_eq!(
            feed(
                &mut machine,
                &[
                    (VK_RCONTROL, true),
                    (VK_RCONTROL, true),
                    (VK_RCONTROL, true),
                    (VK_RCONTROL, false)
                ]
            ),
            [SoloEvent::Down, SoloEvent::Up { clean: true }]
        );
    }

    #[test]
    fn a_key_already_held_makes_it_a_chord() {
        let mut machine = SoloModifier::new(VK_RCONTROL);
        assert_eq!(
            feed(
                &mut machine,
                &[
                    (VK_LSHIFT, true),
                    (VK_RCONTROL, true),
                    (VK_RCONTROL, false),
                    (VK_LSHIFT, false)
                ]
            ),
            Vec::<SoloEvent>::new()
        );
        // Released everything: a later lone press counts again.
        assert_eq!(
            feed(&mut machine, &[(VK_RCONTROL, true), (VK_RCONTROL, false)]),
            [SoloEvent::Down, SoloEvent::Up { clean: true }]
        );
    }

    #[test]
    fn other_keys_alone_mean_nothing() {
        let mut machine = SoloModifier::new(VK_RMENU);
        assert!(feed(&mut machine, &[(KEY_C, true), (KEY_C, false)]).is_empty());
    }

    #[test]
    fn capture_records_a_lone_right_modifier() {
        let mut capture = CaptureTracker::default();
        assert!(!capture.saw_any_key());
        assert_eq!(capture.on_key(VK_RCONTROL, true, true), None);
        assert_eq!(
            capture.on_key(VK_RCONTROL, false, true),
            Some(SoloCapture::Modifier(VK_RCONTROL))
        );
        assert!(capture.saw_any_key());
    }

    #[test]
    fn capture_refuses_a_lone_left_modifier() {
        let mut capture = CaptureTracker::default();
        capture.on_key(VK_LCONTROL, true, true);
        assert_eq!(
            capture.on_key(VK_LCONTROL, false, true),
            Some(SoloCapture::LeftModifier(VK_LCONTROL))
        );
    }

    #[test]
    fn capture_of_a_chord_is_not_a_lone_modifier() {
        let mut capture = CaptureTracker::default();
        capture.on_key(VK_RCONTROL, true, true);
        capture.on_key(KEY_C, true, false);
        capture.on_key(KEY_C, false, false);
        assert_eq!(capture.on_key(VK_RCONTROL, false, true), None);
    }

    #[test]
    fn lone_keys_round_trip() {
        for (vk, code) in [
            (VK_RCONTROL, "ControlRight"),
            (VK_RMENU, "AltRight"),
            (VK_RSHIFT, "ShiftRight"),
            (0x7C, "F13"),
            (0x87, "F24"),
            (0x13, "Pause"),
            (0x91, "ScrollLock"),
            (0x2D, "Insert"),
        ] {
            assert_eq!(solo_code(vk).map(|(code, _)| code), Some(code));
            assert_eq!(solo_vk(code), Some(vk), "{code}");
        }
        assert_eq!(solo_code(VK_RCONTROL).map(|(_, label)| label), Some("Right Ctrl"));
        assert_eq!(solo_code(0x44), None, "D needs a modifier");
        assert_eq!(solo_code(VK_LCONTROL), None);
        assert!(is_solo_modifier_code("AltRight"));
        assert!(!is_solo_modifier_code("F13"));
    }
}
