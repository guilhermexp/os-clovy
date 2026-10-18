//! Windows window identity and enumeration (D2, D6).

use std::path::{Path, PathBuf};

use crate::activity::platform::WindowDescriptor;

/// Returns the focused window descriptor on the main display, or `None` if
/// no foreign window is focused.
#[cfg(target_os = "windows")]
pub(crate) fn focused_window() -> Option<WindowDescriptor> {
    // Stub implementation for 1.5; real Win32 implementation in 2.2 GREEN
    None
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn focused_window() -> Option<WindowDescriptor> {
    None
}

/// Returns the topmost visible window on each secondary monitor.
#[cfg(target_os = "windows")]
pub(crate) fn secondary_windows() -> Vec<WindowDescriptor> {
    // Stub implementation for 1.5; real Win32 implementation in 4.2 / 2.2
    Vec::new()
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn secondary_windows() -> Vec<WindowDescriptor> {
    Vec::new()
}

/// Resolves the executable path on disk for a given `bundle_id` (executable name or AUMID).
///
/// Bounded cache of observed identities -> paths, with fallback to SearchPath / App Paths.
/// NEVER executes the file.
pub(crate) fn executable_path(_bundle_id: &str) -> Option<PathBuf> {
    // Stub implementation for 1.5; real implementation in 2.2 GREEN
    None
}

/// Normalizes executable path to a lowercase filename (e.g. `chrome.exe`).
pub(crate) fn normalize_executable_name(_path: &Path) -> String {
    // Stub implementation for 1.5; real implementation in 2.2 GREEN
    String::new()
}

/// Resolves `(app_name, bundle_id)` from observed process image path, FileDescription,
/// and optional AppUserModelID (AUMID).
///
/// Rules (D2):
/// - `bundle_id`: AUMID preferred for packaged apps (UWP/MSIX); lowercase executable name otherwise.
/// - `app_name`: `FileDescription` from executable version info; fallback to file stem without extension.
pub(crate) fn resolve_identity(
    _image_path: Option<&Path>,
    _file_description: Option<&str>,
    _aumid: Option<&str>,
) -> (String, Option<String>) {
    // Stub implementation for 1.5; real implementation in 2.2 GREEN
    (String::new(), None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_executable_name_handles_mixed_case_and_path() {
        let path1 = Path::new(r"C:\Program Files\Google\Chrome\Application\CHROME.EXE");
        assert_eq!(normalize_executable_name(path1), "chrome.exe");

        let path2 = Path::new(r"msedge.exe");
        assert_eq!(normalize_executable_name(path2), "msedge.exe");

        let path3 = Path::new(r"C:\Windows\System32\CMD.EXE");
        assert_eq!(normalize_executable_name(path3), "cmd.exe");

        let path4 = Path::new(r"C:\Program Files\Microsoft VS Code\Code.exe");
        assert_eq!(normalize_executable_name(path4), "code.exe");
    }

    #[test]
    fn test_resolve_identity_prefers_aumid_for_packaged_apps() {
        let image = Path::new(r"C:\Program Files\WindowsApps\Microsoft.WindowsTerminal_1.18.10301.0_x64__8wekyb3d8bbwe\WindowsTerminal.exe");
        let (app_name, bundle_id) = resolve_identity(
            Some(image),
            Some("Windows Terminal"),
            Some("Microsoft.WindowsTerminal_8wekyb3d8bbwe!App"),
        );
        assert_eq!(app_name, "Windows Terminal");
        assert_eq!(
            bundle_id.as_deref(),
            Some("Microsoft.WindowsTerminal_8wekyb3d8bbwe!App")
        );
    }

    #[test]
    fn test_resolve_identity_uses_lowercase_exe_when_no_aumid() {
        let image = Path::new(r"C:\Program Files\Microsoft\Edge\Application\msedge.exe");
        let (app_name, bundle_id) = resolve_identity(Some(image), Some("Microsoft Edge"), None);
        assert_eq!(app_name, "Microsoft Edge");
        assert_eq!(bundle_id.as_deref(), Some("msedge.exe"));
    }

    #[test]
    fn test_resolve_identity_falls_back_to_stem_when_file_description_empty_or_whitespace() {
        let image = Path::new(r"C:\Windows\System32\cmd.exe");
        let (app_name, bundle_id) = resolve_identity(Some(image), Some("   "), None);
        assert_eq!(app_name, "cmd");
        assert_eq!(bundle_id.as_deref(), Some("cmd.exe"));
    }

    #[test]
    fn test_resolve_identity_falls_back_to_stem_when_file_description_none() {
        let image = Path::new(r"C:\Tools\custom_utility.exe");
        let (app_name, bundle_id) = resolve_identity(Some(image), None, None);
        assert_eq!(app_name, "custom_utility");
        assert_eq!(bundle_id.as_deref(), Some("custom_utility.exe"));
    }

    #[test]
    fn test_executable_path_caching_and_lookup() {
        // Querying non-existent or invalid bundle_id returns None
        assert_eq!(executable_path("non_existent_app_12345.exe"), None);
        assert_eq!(executable_path("../../Windows/explorer.exe"), None);
        assert_eq!(executable_path(""), None);
    }
}
