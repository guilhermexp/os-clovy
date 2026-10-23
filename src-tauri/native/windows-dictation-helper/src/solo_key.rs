//! Lone-key shortcuts on Windows, as pure state machines over the low-level
//! keyboard hook's key events (no Win32 here, so the rules are testable
//! anywhere with `rustc --test`).
//!
//! A lone modifier (Right Ctrl, Right Alt/AltGr, Right Shift) is not a
//! hotkey Windows can register, and it must not get in the way of normal
//! use: Right Ctrl + C still copies. It counts only when pressed and
//! released with no other key in between.

use std::collections::HashSet;

/// Virtual-key codes the rules need.
pub const VK_RSHIFT: u32 = 0xA1;
pub const VK_RCONTROL: u32 = 0xA3;
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

/// One decision of the hook about a lone-modifier shortcut, for the
/// dictation event log. Never names a key other than a modifier: anything
/// else is `"other"`, so the log holds no typed content.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    /// The modifier's DOM `code`, `"other"` or `"mouse"`.
    pub key: &'static str,
    /// `"down"`, `"up"` or `"click"`.
    pub edge: &'static str,
    pub injected: bool,
    /// The press went down with nothing else held.
    pub alone: bool,
    /// How long the shortcut key was held, on its release.
    pub held_ms: Option<u64>,
    pub decision: &'static str,
    pub reason: &'static str,
}

/// What one input meant for a shortcut, and why.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Step {
    pub event: Option<SoloEvent>,
    pub diagnostic: Option<Diagnostic>,
}

/// The DOM `code` of a modifier virtual key, `"other"` for any other key.
pub fn key_name(vk: u32) -> &'static str {
    match vk {
        0xA0 => "ShiftLeft",
        VK_RSHIFT => "ShiftRight",
        0xA2 => "ControlLeft",
        VK_RCONTROL => "ControlRight",
        0xA4 => "AltLeft",
        VK_RMENU => "AltRight",
        0x5B => "MetaLeft",
        0x5C => "MetaRight",
        _ => "other",
    }
}

/// Tracks one lone-modifier shortcut.
#[derive(Clone, Debug)]
pub struct SoloModifier {
    target: u32,
    /// Other keys currently down.
    others: HashSet<u32>,
    /// The target is down.
    held: bool,
    /// The current press went down with nothing else held.
    alone: bool,
    /// Another key joined the current lone press.
    chorded: bool,
    /// When the current press went down, in milliseconds.
    down_at: Option<u64>,
}

impl SoloModifier {
    pub fn new(target: u32) -> Self {
        Self {
            target,
            others: HashSet::new(),
            held: false,
            alone: false,
            chorded: false,
            down_at: None,
        }
    }

    pub fn target(&self) -> u32 {
        self.target
    }

    /// Feeds one key edge with its origin and time, and says why it decided.
    pub fn on_key_at(&mut self, vk: u32, is_down: bool, injected: bool, now_ms: u64) -> Step {
        let line = |edge, alone, held_ms, decision, reason| Diagnostic {
            key: key_name(vk),
            edge,
            injected,
            alone,
            held_ms,
            decision,
            reason,
        };
        if vk == self.target {
            if is_down {
                if self.held {
                    return Step::default(); // auto-repeat
                }
                let event = self.on_key(vk, true);
                self.down_at = Some(now_ms);
                let (decision, reason) = if self.alone {
                    ("down", "alone")
                } else {
                    ("ignored", "other_key_held")
                };
                return Step {
                    event,
                    diagnostic: Some(line("down", self.alone, None, decision, reason)),
                };
            }
            if !self.held {
                return Step {
                    event: None,
                    diagnostic: Some(line("up", false, None, "ignored", "up_without_down")),
                };
            }
            let (alone, chorded) = (self.alone, self.chorded);
            let held_ms = self.down_at.take().map(|at| now_ms.saturating_sub(at));
            let event = self.on_key(vk, false);
            let (decision, reason) = match (alone, chorded) {
                (false, _) => ("ignored", "not_alone"),
                (true, true) => ("up", "chorded"),
                (true, false) => ("up", "clean"),
            };
            return Step {
                event,
                diagnostic: Some(line("up", alone, held_ms, decision, reason)),
            };
        }
        let event = self.on_key(vk, is_down);
        let diagnostic = (event == Some(SoloEvent::Chord)).then(|| {
            let held_ms = self.down_at.map(|at| now_ms.saturating_sub(at));
            line("down", true, held_ms, "chord", "other_key")
        });
        Step { event, diagnostic }
    }

    /// Feeds one key edge (auto-repeat included); returns what it means for
    /// this shortcut.
    pub fn on_key(&mut self, vk: u32, is_down: bool) -> Option<SoloEvent> {
        if vk == self.target {
            if is_down {
                if self.held {
                    return None; // auto-repeat
                }
                self.held = true;
                self.alone = self.others.is_empty();
                self.chorded = false;
                return self.alone.then_some(SoloEvent::Down);
            }
            if !self.held {
                return None;
            }
            self.held = false;
            let alone = std::mem::replace(&mut self.alone, false);
            return alone.then_some(SoloEvent::Up {
                clean: !self.chorded,
            });
        }
        if is_down {
            let first = self.others.insert(vk);
            if first && self.held && self.alone && !self.chorded {
                self.chorded = true;
                return Some(SoloEvent::Chord);
            }
        } else {
            self.others.remove(&vk);
        }
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
pub struct CaptureTracker {
    /// The modifier pressed first while nothing else was down.
    candidate: Option<u32>,
    /// Keys currently down.
    down: HashSet<u32>,
    saw_key: bool,
}

impl CaptureTracker {
    /// Feeds one key edge. Returns a lone-modifier capture when the first
    /// modifier pressed alone is released with no other key in between.
    pub fn on_key(&mut self, vk: u32, is_down: bool, is_modifier: bool) -> Option<SoloCapture> {
        self.saw_key = true;
        if is_down {
            let first = self.down.insert(vk);
            if !first {
                return None; // auto-repeat
            }
            if is_modifier && self.down.len() == 1 {
                self.candidate = Some(vk);
            } else {
                self.candidate = None;
            }
            return None;
        }
        self.down.remove(&vk);
        if self.candidate != Some(vk) {
            return None;
        }
        self.candidate = None;
        Some(if is_right_modifier(vk) {
            SoloCapture::Modifier(vk)
        } else {
            SoloCapture::LeftModifier(vk)
        })
    }

    /// Whether any key event reached the hook during the capture.
    pub fn saw_any_key(&self) -> bool {
        self.saw_key
    }
}

fn is_right_modifier(vk: u32) -> bool {
    matches!(vk, VK_RCONTROL | VK_RMENU | VK_RSHIFT)
}

/// Lone keys: virtual key, DOM `code`, label.
const SOLO_KEYS: [(u32, &str, &str); 20] = [
    (VK_RCONTROL, "ControlRight", "Right Ctrl"),
    (VK_RMENU, "AltRight", "Right Alt"),
    (VK_RSHIFT, "ShiftRight", "Right Shift"),
    (0x7C, "F13", "F13"),
    (0x7D, "F14", "F14"),
    (0x7E, "F15", "F15"),
    (0x7F, "F16", "F16"),
    (0x80, "F17", "F17"),
    (0x81, "F18", "F18"),
    (0x82, "F19", "F19"),
    (0x83, "F20", "F20"),
    (0x84, "F21", "F21"),
    (0x85, "F22", "F22"),
    (0x86, "F23", "F23"),
    (0x87, "F24", "F24"),
    (0x13, "Pause", "Pause"),
    (0x91, "ScrollLock", "Scroll Lock"),
    (0x2D, "Insert", "Insert"),
    (0x2C, "PrintScreen", "Print Screen"),
    (0x5D, "ContextMenu", "Menu"),
];

/// The DOM `code` of a key that works as a shortcut on its own, and a label.
pub fn solo_code(vk: u32) -> Option<(&'static str, &'static str)> {
    SOLO_KEYS
        .iter()
        .find(|(key, _, _)| *key == vk)
        .map(|(_, code, label)| (*code, *label))
}

/// The virtual key of a lone-key `code`.
pub fn solo_vk(code: &str) -> Option<u32> {
    SOLO_KEYS
        .iter()
        .find(|(_, known, _)| *known == code)
        .map(|(vk, _, _)| *vk)
}

/// Whether `code` names a lone modifier (handled by the hook, not
/// `RegisterHotKey`).
pub fn is_solo_modifier_code(code: &str) -> bool {
    solo_vk(code).is_some_and(is_right_modifier)
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY_C: u32 = 0x43;
    const VK_LSHIFT: u32 = 0xA0;
    const VK_LCONTROL: u32 = 0xA2;

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

    fn diagnose(machine: &mut SoloModifier, events: &[(u32, bool, bool, u64)]) -> Vec<Diagnostic> {
        events
            .iter()
            .filter_map(|(vk, down, injected, at)| {
                machine.on_key_at(*vk, *down, *injected, *at).diagnostic
            })
            .collect()
    }

    #[test]
    fn every_decision_about_the_shortcut_key_is_a_diagnostic_line() {
        let mut machine = SoloModifier::new(VK_RCONTROL);
        let lines = diagnose(
            &mut machine,
            &[
                (VK_RCONTROL, true, false, 1_000),
                (VK_RCONTROL, false, false, 1_240),
            ],
        );
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].key, "ControlRight");
        assert_eq!(lines[0].edge, "down");
        assert!(lines[0].alone);
        assert!(!lines[0].injected);
        assert_eq!(lines[1].edge, "up");
        assert_eq!(lines[1].held_ms, Some(240));
        assert!(!lines[1].decision.is_empty() && !lines[1].reason.is_empty());
    }

    #[test]
    fn a_chord_is_logged_without_naming_the_other_key() {
        let mut machine = SoloModifier::new(VK_RCONTROL);
        let lines = diagnose(
            &mut machine,
            &[
                (VK_RCONTROL, true, false, 0),
                (KEY_C, true, false, 50),
                (KEY_C, false, false, 80),
                (VK_RCONTROL, false, false, 120),
            ],
        );
        assert_eq!(lines.len(), 3, "down, chord, up: {lines:?}");
        assert_eq!(lines[1].key, "other");
        assert_eq!(lines[1].decision, "chord");
        assert_eq!(lines[2].reason, "chorded");
        assert!(lines.iter().all(|line| line.key != "KeyC"));
    }

    #[test]
    fn typing_without_the_shortcut_key_logs_nothing() {
        let mut machine = SoloModifier::new(VK_RCONTROL);
        assert!(diagnose(
            &mut machine,
            &[(KEY_C, true, false, 0), (KEY_C, false, false, 10)]
        )
        .is_empty());
    }

    #[test]
    fn injected_and_not_alone_presses_say_so() {
        let mut machine = SoloModifier::new(VK_RCONTROL);
        let injected = diagnose(&mut machine, &[(VK_RCONTROL, true, true, 0)]);
        assert_eq!(injected.len(), 1);
        assert!(injected[0].injected);
        machine.on_key_at(VK_RCONTROL, false, true, 10);

        let held = diagnose(
            &mut machine,
            &[(VK_LSHIFT, true, false, 20), (VK_RCONTROL, true, false, 30)],
        );
        assert_eq!(held.len(), 1);
        assert!(!held[0].alone);
        assert_eq!(held[0].reason, "other_key_held");
    }

    #[test]
    fn modifier_names_are_codes_and_other_keys_are_anonymous() {
        assert_eq!(key_name(VK_LCONTROL), "ControlLeft");
        assert_eq!(key_name(VK_RMENU), "AltRight");
        assert_eq!(key_name(KEY_C), "other");
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
        assert_eq!(
            solo_code(VK_RCONTROL).map(|(_, label)| label),
            Some("Right Ctrl")
        );
        assert_eq!(solo_code(0x44), None, "D needs a modifier");
        assert_eq!(solo_code(VK_LCONTROL), None);
        assert!(is_solo_modifier_code("AltRight"));
        assert!(!is_solo_modifier_code("F13"));
    }
}
