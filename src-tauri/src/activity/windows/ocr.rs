//! In-memory window screenshot capture and OCR text recognition on Windows.

use crate::activity::platform::WindowDescriptor;

/// Stub for 1.5 skeleton. Real implementation written in GREEN 3.4.
pub(crate) fn window_text(_window: &WindowDescriptor) -> Option<String> {
    None
}

/// Stub for 1.5 skeleton. Real implementation written in GREEN 3.4.
pub(crate) fn recognize_image_bytes(_bytes: &[u8]) -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_windows_ocr_fixture_recognition() {
        let fixture_bytes = include_bytes!("fixtures/ocr-fixture.png");
        let recognized = recognize_image_bytes(fixture_bytes);
        assert!(
            recognized.is_some(),
            "Windows.Media.Ocr must recognize the test fixture image"
        );
        let text = recognized.unwrap();
        assert!(
            text.contains("CLOVY") && text.contains("WINDOWS") && text.contains("ACTIVITY"),
            "Recognized text must contain 'CLOVY WINDOWS ACTIVITY', got: {:?}",
            text
        );
    }
}
