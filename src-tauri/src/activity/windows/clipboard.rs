//! Clipboard monitoring on Windows with password manager concealment detection.

use std::sync::LazyLock;
use windows::core::w;
use windows::Win32::Foundation::{HGLOBAL, HWND};
use windows::Win32::System::DataExchange::{
    CloseClipboard, GetClipboardData, GetClipboardSequenceNumber,
    IsClipboardFormatAvailable, OpenClipboard, RegisterClipboardFormatW,
};
use windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};

const CF_UNICODETEXT: u32 = 13;

static FORMAT_EXCLUDE: LazyLock<u32> = LazyLock::new(|| unsafe {
    RegisterClipboardFormatW(w!("ExcludeClipboardContentFromMonitorProcessing"))
});

static FORMAT_HISTORY: LazyLock<u32> = LazyLock::new(|| unsafe {
    RegisterClipboardFormatW(w!("CanIncludeInClipboardHistory"))
});

struct ClipboardGuard;

impl Drop for ClipboardGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseClipboard();
        }
    }
}

/// Evaluates clipboard concealment markers and only calls `text_reader` if content is allowed.
/// If content is marked concealed or history is disabled, `text_reader` is NEVER called,
/// preventing sensitive passwords/tokens from ever being requested or read into memory.
pub(crate) fn read_clipboard_content_with<F>(
    exclude_available: bool,
    can_include_in_history: bool,
    mut text_reader: F,
) -> Option<String>
where
    F: FnMut() -> Option<String>,
{
    if exclude_available {
        return None;
    }
    if !can_include_in_history {
        return None;
    }
    let text = text_reader()?;
    if text.trim().is_empty() {
        None
    } else {
        Some(text)
    }
}

fn history_allows_text(_format_present: bool, value: Option<u32>) -> bool {
    value != Some(0)
}

/// Safely extracts a null-terminated UTF-16 string from raw memory bounded by `max_bytes`.
/// If no null terminator exists within `max_bytes`, returns None (avoids reading out of bounds).
pub(crate) fn parse_utf16_bounded(ptr: *const u16, max_bytes: usize) -> Option<String> {
    if ptr.is_null() || max_bytes < 2 {
        return None;
    }
    let max_units = max_bytes / 2;
    let slice = unsafe { std::slice::from_raw_parts(ptr, max_units) };
    let term_idx = slice.iter().position(|&c| c == 0)?;
    let text = String::from_utf16_lossy(&slice[..term_idx]);
    Some(text)
}

/// Safely parses a 32-bit DWORD from memory if `size >= 4`.
pub(crate) fn parse_history_dword_bounded(ptr: *const u8, size: usize) -> Option<u32> {
    if ptr.is_null() || size < 4 {
        return None;
    }
    let val = unsafe { std::ptr::read_unaligned(ptr as *const u32) };
    Some(val)
}

/// Pure sequence-state policy for tracking clipboard updates across ticks.
#[derive(Debug, Default)]
pub(crate) struct ClipboardSequenceState {
    last_completed_sequence: u32,
}

impl ClipboardSequenceState {
    pub(crate) fn new(initial_sequence: u32) -> Self {
        Self {
            last_completed_sequence: initial_sequence,
        }
    }

    /// Returns true if `sequence` represents new content eligible for inspection.
    pub(crate) fn is_eligible(&self, sequence: u32) -> bool {
        sequence != self.last_completed_sequence
    }

    /// Marks `sequence` as completed, suppressing it until the sequence advances.
    pub(crate) fn mark_completed(&mut self, sequence: u32) {
        self.last_completed_sequence = sequence;
    }
}

pub(crate) struct Clipboard {
    sequence_state: ClipboardSequenceState,
}

impl Default for Clipboard {
    fn default() -> Self {
        Self::new()
    }
}

impl Clipboard {
    pub(crate) fn new() -> Self {
        let initial = unsafe { GetClipboardSequenceNumber() };
        Self {
            sequence_state: ClipboardSequenceState::new(initial),
        }
    }

    /// Retrieves the general clipboard text if the sequence number has advanced
    /// since the previous inspection, unless the clipboard declares concealed or
    /// transient formats used by password managers.
    pub(crate) fn changed_text(&mut self) -> Option<String> {
        let current_seq = unsafe { GetClipboardSequenceNumber() };
        if !self.sequence_state.is_eligible(current_seq) {
            return None;
        }

        // If clipboard is busy/locked by another application, return None immediately.
        // Because mark_completed is NOT called, this sequence remains eligible on the next tick.
        if unsafe { OpenClipboard(None).is_err() } {
            return None;
        }
        let _guard = ClipboardGuard;

        let exclude_fmt = *FORMAT_EXCLUDE;
        let exclude_available =
            exclude_fmt != 0 && unsafe { IsClipboardFormatAvailable(exclude_fmt).is_ok() };

        let history_fmt = *FORMAT_HISTORY;
        let history_available =
            history_fmt != 0 && unsafe { IsClipboardFormatAvailable(history_fmt).is_ok() };
        let can_include_in_history =
            if history_available {
                if let Ok(handle) = unsafe { GetClipboardData(history_fmt) } {
                    if !handle.is_invalid() {
                        let hmem = HGLOBAL(handle.0);
                        let size = unsafe { GlobalSize(hmem) };
                        let ptr = unsafe { GlobalLock(hmem) };
                        if !ptr.is_null() {
                            let parsed = parse_history_dword_bounded(ptr as *const u8, size);
                            unsafe {
                                let _ = GlobalUnlock(hmem);
                            }
                            parsed
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            };

        // Pre-read concealment check: lazy reader MUST NOT be invoked when hidden
        let res = read_clipboard_content_with(exclude_available, history_allows_text(history_available, can_include_in_history), || {
            if unsafe { IsClipboardFormatAvailable(CF_UNICODETEXT).is_err() } {
                return None;
            }
            let handle = unsafe { GetClipboardData(CF_UNICODETEXT) }.ok()?;
            if handle.is_invalid() {
                return None;
            }
            let hmem = HGLOBAL(handle.0);
            let size_bytes = unsafe { GlobalSize(hmem) };
            let ptr = unsafe { GlobalLock(hmem) as *const u16 };
            if ptr.is_null() {
                return None;
            }
            let text = parse_utf16_bounded(ptr, size_bytes);
            unsafe {
                let _ = GlobalUnlock(hmem);
            }
            text
        });

        // Mark completed only after inspection decision is reached
        self.sequence_state.mark_completed(current_seq);

        res
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clipboard_pre_read_concealment_lazy_reader() {
        let mut reader_invoked = false;

        // 1. ExcludeClipboardContentFromMonitorProcessing present: text reader MUST NOT be invoked
        let res1 = read_clipboard_content_with(true, true, || {
            reader_invoked = true;
            Some("sensitive_password".to_string())
        });
        assert_eq!(res1, None);
        assert_eq!(
            reader_invoked, false,
            "Text reader closure MUST NOT be invoked when exclude format is present"
        );

        // 2. CanIncludeInClipboardHistory = 0: text reader MUST NOT be invoked
        let res2 = read_clipboard_content_with(false, false, || {
            reader_invoked = true;
            Some("one_time_token".to_string())
        });
        assert_eq!(res2, None);
        assert_eq!(
            reader_invoked, false,
            "Text reader closure MUST NOT be invoked when CanIncludeInClipboardHistory is 0"
        );

        // 3. Normal content: text reader IS invoked
        let res3 = read_clipboard_content_with(false, true, || {
            reader_invoked = true;
            Some("regular_text".to_string())
        });
        assert_eq!(res3, Some("regular_text".to_string()));
        assert_eq!(
            reader_invoked, true,
            "Text reader must be invoked for allowed content"
        );
    }

    #[test]
    fn unreadable_history_marker_never_requests_plaintext() {
        let result = read_clipboard_content_with(
            false,
            history_allows_text(true, None),
            || panic!("An unreadable password-manager marker must conceal plaintext"),
        );
        assert_eq!(result, None);
    }

    #[test]
    fn test_clipboard_bounded_utf16_unterminated_and_short_dword() {
        // 1. Unterminated UTF-16: no null terminator in allocation
        let unterminated: Vec<u16> = vec![b'H' as u16, b'i' as u16, b'!' as u16];
        let parsed = parse_utf16_bounded(unterminated.as_ptr(), unterminated.len() * 2);
        assert_eq!(
            parsed, None,
            "Unterminated UTF-16 allocation must be rejected (return None) to prevent reading out of bounds"
        );

        // 2. Properly terminated UTF-16
        let terminated: Vec<u16> = vec![b'H' as u16, b'i' as u16, 0, b'X' as u16];
        let parsed_ok = parse_utf16_bounded(terminated.as_ptr(), terminated.len() * 2);
        assert_eq!(parsed_ok, Some("Hi".to_string()));

        // 3. Short allocation for history DWORD: size < 4 bytes
        let short_dword: [u8; 3] = [1, 0, 0];
        let parsed_dword = parse_history_dword_bounded(short_dword.as_ptr(), short_dword.len());
        assert_eq!(
            parsed_dword, None,
            "History allocation smaller than 4 bytes must be rejected (return None)"
        );

        // 4. Valid 4-byte history DWORD
        let valid_dword: [u8; 4] = [1, 0, 0, 0];
        let parsed_valid = parse_history_dword_bounded(valid_dword.as_ptr(), valid_dword.len());
        assert_eq!(parsed_valid, Some(1));
    }

    #[test]
    fn test_clipboard_sequence_policy() {
        let mut policy = ClipboardSequenceState::new(100);

        // Same sequence as initial must be suppressed (ineligible)
        assert_eq!(
            policy.is_eligible(100),
            false,
            "Initial sequence number must be suppressed to avoid capturing pre-existing clipboard"
        );

        // Advanced sequence 101 is eligible
        assert_eq!(
            policy.is_eligible(101),
            true,
            "Advanced sequence number must be eligible for inspection"
        );


        // Successfully completing sequence 101 suppresses it on subsequent checks
        policy.mark_completed(101);
        assert_eq!(
            policy.is_eligible(101),
            false,
            "Completed sequence must be suppressed on subsequent ticks"
        );

        // Sequence rollover / wrap-around (e.g. u32::MAX -> 0) is eligible
        let rollover = ClipboardSequenceState::new(u32::MAX);
        assert_eq!(
            rollover.is_eligible(0),
            true,
            "Sequence rollover from u32::MAX to 0 must be eligible"
        );
    }
}
