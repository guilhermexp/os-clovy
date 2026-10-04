//! Accessibility (AX) tree inspection, focused window discovery, browser URL
//! extraction, and private window detection.

use std::collections::{HashSet, VecDeque};
use std::time::{Duration, Instant};

use core_foundation::base::{CFGetTypeID, CFRelease, CFTypeRef, TCFType};
use core_foundation::string::{CFString, CFStringRef};
use core_foundation::url::CFURLRef;
use objc2_app_kit::NSWorkspace;

use crate::activity::macos::screens::{active_displays, display_id_for_rect, main_display_id};
use crate::activity::platform::WindowDescriptor;
use platform_macos::ax::bindings::{
    ax_get_window_id, copy_ax_windows, copy_children, copy_string_attr, element_screen_rect,
    enable_chromium_accessibility, kAXErrorSuccess, AXUIElementCopyAttributeValue,
    AXUIElementCreateApplication, AXUIElementRef,
};

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFURLGetString(anURL: CFURLRef) -> CFStringRef;
    fn CFURLGetTypeID() -> core_foundation::base::CFTypeID;
}

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXUIElementSetMessagingTimeout(element: AXUIElementRef, timeout_in_seconds: f32) -> i32;
}

/// Per-call AX timeout. The system default is 6 s, so one hung app could
/// stall a 2 s capture tick for minutes across a tree walk.
const AX_MESSAGING_TIMEOUT_SECS: f32 = 0.25;

/// Bounds every later AX request made through `element`. Local call, no IPC.
pub(crate) fn bound_messaging(element: AXUIElementRef) {
    if !element.is_null() {
        unsafe {
            AXUIElementSetMessagingTimeout(element, AX_MESSAGING_TIMEOUT_SECS);
        }
    }
}

/// RAII wrapper for `AXUIElementRef` ensuring `CFRelease` is called on drop.
#[derive(Debug)]
pub struct AutoAxElement(pub AXUIElementRef);

impl Drop for AutoAxElement {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                CFRelease(self.0 as CFTypeRef);
            }
        }
    }
}

/// Normalizes browser URL strings from AXURL attributes or address fields.
pub fn normalize_browser_url(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    // Supported schemes
    let allowed_schemes = [
        "http://",
        "https://",
        "file://",
        "about:",
        "chrome://",
        "chrome-extension://",
        "edge://",
        "brave://",
        "arc://",
        "safari://",
    ];

    for scheme in &allowed_schemes {
        if trimmed.starts_with(scheme) {
            return Some(trimmed.to_string());
        }
    }

    // Domain-like pattern without scheme: e.g. "github.com/repo" or "news.ycombinator.com"
    if !trimmed.contains(' ') && (trimmed.contains('.') || trimmed.starts_with("localhost")) {
        return Some(format!("https://{trimmed}"));
    }

    None
}

/// Checks window title for private/incognito browsing indicators across
/// Chrome-family, Safari, and other browsers in multiple locales (en, pt-BR, es, fr, de).
pub fn has_private_window_marker(title: &str) -> bool {
    let lower = title.to_lowercase();
    let markers = [
        "incognito",
        "inprivate",
        "private browsing",
        "(private)",
        "[private]",
        "navegação privada",
        "navegacao privada",
        "janela privada",
        "aba privada",
        "guia privada",
        "anônima",
        "anonima",
        "janela anônima",
        "janela anonima",
        "guia anônima",
        "guia anonima",
        "navegação anônima",
        "navegacao anonima",
        "incógnito",
        "navegación privada",
        "navegacion privada",
        "ventana privada",
        "navigation privée",
        "navigation privee",
        "fenêtre privée",
        "fenetre privee",
        "privates surfen",
        "inkognito",
    ];

    markers.iter().any(|&m| lower.contains(m))
}

/// Determines whether a window is an incognito or private browsing window.
///
/// Evaluates:
/// 1. Window title marker keywords (en, pt-BR, es, fr, de).
/// 2. AX element markers (e.g. Incognito profile badge, Safari Private Browsing indicator).
pub fn is_private_window(
    app_name: &str,
    bundle_id: Option<&str>,
    window_title: Option<&str>,
    win_elem: Option<AXUIElementRef>,
) -> bool {
    let is_browser = {
        let lower_app = app_name.to_lowercase();
        let b_id = bundle_id.unwrap_or_default().to_lowercase();
        lower_app.contains("chrome")
            || lower_app.contains("safari")
            || lower_app.contains("brave")
            || lower_app.contains("edge")
            || lower_app.contains("arc")
            || lower_app.contains("opera")
            || b_id.contains("chrome")
            || b_id.contains("safari")
            || b_id.contains("brave")
            || b_id.contains("edge")
            || b_id.contains("arc")
    };

    if !is_browser {
        return false;
    }

    if let Some(title) = window_title {
        if has_private_window_marker(title) {
            return true;
        }
    }

    let Some(win) = win_elem else {
        return false;
    };

    // Shallow search in toolbar/groups for private badge / indicator (budget: 120 nodes, depth: 5)
    let mut queue = VecDeque::new();
    queue.push_back((win, 0usize));
    let mut visited = 0usize;

    while let Some((elem, depth)) = queue.pop_front() {
        bound_messaging(elem);
        visited += 1;
        if visited > 120 {
            break;
        }

        let role = unsafe { copy_string_attr(elem, "AXRole") }.unwrap_or_default();
        if role == "AXSecureTextField" {
            continue;
        }

        if let Some(t) = unsafe { copy_string_attr(elem, "AXTitle") } {
            if has_private_window_marker(&t) {
                return true;
            }
        }
        if let Some(d) = unsafe { copy_string_attr(elem, "AXDescription") } {
            if has_private_window_marker(&d) {
                return true;
            }
        }

        if depth < 5 {
            let children = unsafe { copy_children(elem) };
            for child in children {
                // Ensure child will be released when popped or discarded
                queue.push_back((child, depth + 1));
            }
        }

        if elem != win {
            unsafe {
                CFRelease(elem as CFTypeRef);
            }
        }
    }

    // Release any remaining unvisited items in queue
    while let Some((elem, _)) = queue.pop_front() {
        if elem != win {
            unsafe {
                CFRelease(elem as CFTypeRef);
            }
        }
    }

    false
}

/// Finds the browser URL by searching for `AXWebArea` under `win_elem`, then
/// reading its `AXURL` attribute. Falls back to an address field if cheap.
pub fn resolve_browser_url(win_elem: AXUIElementRef) -> Option<String> {
    if win_elem.is_null() {
        return None;
    }

    let mut queue = VecDeque::new();
    queue.push_back((win_elem, 0usize));
    let mut visited = 0usize;
    let mut fallback_url = None;

    while let Some((elem, depth)) = queue.pop_front() {
        bound_messaging(elem);
        visited += 1;
        if visited > 200 {
            break;
        }

        let role = unsafe { copy_string_attr(elem, "AXRole") }.unwrap_or_default();

        if role == "AXWebArea" {
            let url_attr = CFString::new("AXURL");
            let mut value: CFTypeRef = std::ptr::null();
            let err = unsafe {
                AXUIElementCopyAttributeValue(elem, url_attr.as_concrete_TypeRef(), &mut value)
            };
            if err == kAXErrorSuccess && !value.is_null() {
                let cf_type = unsafe { CFGetTypeID(value) };
                let extracted = if cf_type == unsafe { CFURLGetTypeID() } {
                    let str_ref = unsafe { CFURLGetString(value as CFURLRef) };
                    if !str_ref.is_null() {
                        let cf_str = unsafe { CFString::wrap_under_get_rule(str_ref) };
                        Some(cf_str.to_string())
                    } else {
                        None
                    }
                } else if cf_type == CFString::type_id() {
                    let cf_str = unsafe { CFString::wrap_under_get_rule(value as CFStringRef) };
                    Some(cf_str.to_string())
                } else {
                    None
                };
                unsafe {
                    CFRelease(value);
                }

                if let Some(raw) = extracted {
                    if let Some(norm) = normalize_browser_url(&raw) {
                        // Cleanup remaining items
                        if elem != win_elem {
                            unsafe {
                                CFRelease(elem as CFTypeRef);
                            }
                        }
                        while let Some((e, _)) = queue.pop_front() {
                            if e != win_elem {
                                unsafe {
                                    CFRelease(e as CFTypeRef);
                                }
                            }
                        }
                        return Some(norm);
                    }
                }
            }
        }

        // Cheap address field fallback (e.g. AXTextField with identifier or value looking like URL)
        if fallback_url.is_none() && (role == "AXTextField" || role == "AXComboBox") {
            let desc = unsafe { copy_string_attr(elem, "AXDescription") }.unwrap_or_default();
            let val = unsafe { copy_string_attr(elem, "AXValue") }.unwrap_or_default();
            if (desc.to_lowercase().contains("address") || desc.to_lowercase().contains("search"))
                && !val.is_empty()
            {
                if let Some(norm) = normalize_browser_url(&val) {
                    fallback_url = Some(norm);
                }
            }
        }

        if depth < 10 {
            let children = unsafe { copy_children(elem) };
            for child in children {
                queue.push_back((child, depth + 1));
            }
        }

        if elem != win_elem {
            unsafe {
                CFRelease(elem as CFTypeRef);
            }
        }
    }

    while let Some((elem, _)) = queue.pop_front() {
        if elem != win_elem {
            unsafe {
                CFRelease(elem as CFTypeRef);
            }
        }
    }

    fallback_url
}

/// Deduplicates adjacent repeated strings and joins them up to `max_chars`.
pub fn dedupe_and_cap_text(chunks: &[String], max_chars: usize) -> String {
    let mut result = String::new();
    let mut last_chunk = "";

    for chunk in chunks {
        let trimmed = chunk.trim();
        if trimmed.is_empty() || trimmed == last_chunk {
            continue;
        }

        if !result.is_empty() {
            if result.len() + 1 >= max_chars {
                break;
            }
            result.push('\n');
        }

        let remaining = max_chars.saturating_sub(result.len());
        if trimmed.len() <= remaining {
            result.push_str(trimmed);
            last_chunk = trimmed;
        } else {
            // Cap to remaining limit
            let slice: String = trimmed.chars().take(remaining).collect();
            result.push_str(&slice);
            break;
        }
    }

    result
}

/// Retrieves the descriptor of the focused window of the frontmost application.
pub fn focused_window(chromium_settled_pids: &mut HashSet<i32>) -> Option<WindowDescriptor> {
    let workspace = NSWorkspace::sharedWorkspace();
    let front_app = workspace.frontmostApplication()?;
    let pid = front_app.processIdentifier();
    let app_name = front_app
        .localizedName()
        .map(|s| s.to_string())
        .unwrap_or_default();
    let bundle_id = front_app.bundleIdentifier().map(|s| s.to_string());

    // Clovy's own windows are always excluded by the engine. Querying our own
    // accessibility tree from this thread would round-trip through the main
    // thread for nothing, so describe the app without touching AX.
    if pid == std::process::id() as i32 {
        return Some(WindowDescriptor {
            pid,
            app_name,
            bundle_id,
            ..WindowDescriptor::default()
        });
    }

    let app_elem = unsafe { AXUIElementCreateApplication(pid) };
    if app_elem.is_null() {
        return None;
    }
    bound_messaging(app_elem);
    let auto_app = AutoAxElement(app_elem);

    if !chromium_settled_pids.contains(&pid) {
        chromium_settled_pids.insert(pid);
        let settled = unsafe { enable_chromium_accessibility(app_elem) };
        if settled {
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    // Query AXFocusedWindow
    let focused_attr = CFString::new("AXFocusedWindow");
    let mut win_ref: CFTypeRef = std::ptr::null();
    let err = unsafe {
        AXUIElementCopyAttributeValue(app_elem, focused_attr.as_concrete_TypeRef(), &mut win_ref)
    };

    let target_win: Option<AutoAxElement> = if err == kAXErrorSuccess && !win_ref.is_null() {
        Some(AutoAxElement(win_ref as AXUIElementRef))
    } else {
        // Fallback to first window in AXWindows
        let windows = unsafe { copy_ax_windows(app_elem) };
        windows.into_iter().next().map(AutoAxElement)
    };

    let Some(win) = target_win else {
        drop(auto_app);
        return None;
    };

    let window_title = unsafe { copy_string_attr(win.0, "AXTitle") };
    let mut window_id = unsafe { ax_get_window_id(win.0) };

    // Fallback: match window_id via CG visible windows for this pid
    if window_id.is_none() {
        let visible = platform_macos::windows::visible_windows();
        if let Some(matching) = visible
            .iter()
            .find(|w| w.pid == pid && w.layer == 0 && w.is_on_screen)
        {
            window_id = Some(matching.window_id);
        }
    }

    // Display ID via window bounds vs displays
    let display_id = if let Some(rect) = unsafe { element_screen_rect(win.0) } {
        Some(display_id_for_rect(rect))
    } else {
        active_displays()
            .first()
            .copied()
            .or_else(|| Some(main_display_id()))
    };

    let browser_url = resolve_browser_url(win.0);
    let private_window = is_private_window(
        &app_name,
        bundle_id.as_deref(),
        window_title.as_deref(),
        Some(win.0),
    );

    drop(win);
    drop(auto_app);

    Some(WindowDescriptor {
        pid,
        app_name,
        bundle_id,
        window_title,
        browser_url,
        private_window,
        window_id,
        display_id,
    })
}

/// Performs a bounded walk of `window`'s AX tree extracting visible text.
///
/// Limits:
/// - Node budget: ~3,000 nodes
/// - Depth budget: ~40 levels
/// - Wall budget: ~300 ms
/// - Deduplicates adjacent repeats
/// - Caps output at ~20,000 characters
pub fn accessibility_text(window: &WindowDescriptor) -> Option<String> {
    let pid = window.pid;
    let app_elem = unsafe { AXUIElementCreateApplication(pid) };
    if app_elem.is_null() {
        return None;
    }
    bound_messaging(app_elem);
    let auto_app = AutoAxElement(app_elem);

    // Find the AX window matching window.window_id
    let ax_windows = unsafe { copy_ax_windows(app_elem) };
    let matching_win = if let Some(target_wid) = window.window_id {
        ax_windows
            .into_iter()
            .map(AutoAxElement)
            .find(|w| unsafe { ax_get_window_id(w.0) } == Some(target_wid))
    } else {
        ax_windows.into_iter().next().map(AutoAxElement)
    };

    let Some(win) = matching_win else {
        drop(auto_app);
        return None;
    };

    let start_time = Instant::now();
    let deadline = start_time + Duration::from_millis(300);

    let mut queue = VecDeque::new();
    queue.push_back((win.0, 0usize));

    let mut visited_nodes = 0usize;
    let mut chunks: Vec<String> = Vec::new();
    let mut total_chars = 0usize;

    const MAX_NODES: usize = 3000;
    const MAX_DEPTH: usize = 40;
    const MAX_CHARS: usize = 20_000;

    while let Some((elem, depth)) = queue.pop_front() {
        bound_messaging(elem);
        visited_nodes += 1;
        if visited_nodes >= MAX_NODES || Instant::now() >= deadline || total_chars >= MAX_CHARS {
            if elem != win.0 {
                unsafe {
                    CFRelease(elem as CFTypeRef);
                }
            }
            break;
        }

        let role = unsafe { copy_string_attr(elem, "AXRole") }.unwrap_or_default();
        let subrole = unsafe { copy_string_attr(elem, "AXSubrole") }.unwrap_or_default();

        // Privacy: unconditionally skip secure text fields and their children
        if role == "AXSecureTextField" || subrole == "AXSecureTextField" {
            if elem != win.0 {
                unsafe {
                    CFRelease(elem as CFTypeRef);
                }
            }
            continue;
        }

        // Skip non-text structural controls
        let is_pure_noise = role == "AXScrollBar" || role == "AXSplitter" || role == "AXRuler";
        if !is_pure_noise {
            // Extract text from AXValue, AXTitle, AXDescription
            let val = unsafe { copy_string_attr(elem, "AXValue") };
            let title = unsafe { copy_string_attr(elem, "AXTitle") };
            let desc = unsafe { copy_string_attr(elem, "AXDescription") };

            for t in [val, title, desc].into_iter().flatten() {
                let tr = t.trim();
                if !tr.is_empty() {
                    total_chars += tr.len() + 1;
                    chunks.push(tr.to_string());
                }
            }
        }

        if depth < MAX_DEPTH {
            let children = unsafe { copy_children(elem) };
            for child in children {
                queue.push_back((child, depth + 1));
            }
        }

        if elem != win.0 {
            unsafe {
                CFRelease(elem as CFTypeRef);
            }
        }
    }

    // Clean up any remaining allocated elements in queue
    while let Some((elem, _)) = queue.pop_front() {
        if elem != win.0 {
            unsafe {
                CFRelease(elem as CFTypeRef);
            }
        }
    }

    drop(win);
    drop(auto_app);

    if chunks.is_empty() {
        None
    } else {
        Some(dedupe_and_cap_text(&chunks, MAX_CHARS))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_browser_url() {
        assert_eq!(
            normalize_browser_url("https://example.com/page"),
            Some("https://example.com/page".to_string())
        );
        assert_eq!(
            normalize_browser_url("http://localhost:3000/app"),
            Some("http://localhost:3000/app".to_string())
        );
        assert_eq!(
            normalize_browser_url("about:blank"),
            Some("about:blank".to_string())
        );
        assert_eq!(
            normalize_browser_url("github.com/rust-lang"),
            Some("https://github.com/rust-lang".to_string())
        );
        assert_eq!(normalize_browser_url("   "), None);
        assert_eq!(normalize_browser_url("not a url at all with spaces"), None);
    }

    #[test]
    fn test_private_window_title_markers() {
        // English
        assert!(has_private_window_marker(
            "New Tab - Google Chrome (Incognito)"
        ));
        assert!(has_private_window_marker("Private Browsing - Safari"));
        assert!(has_private_window_marker("Example Domain - InPrivate"));

        // Portuguese (pt-BR)
        assert!(has_private_window_marker(
            "Nova guia - Google Chrome (Anônima)"
        ));
        assert!(has_private_window_marker("Nova janela anônima"));
        assert!(has_private_window_marker("Safari — Navegação Privada"));
        assert!(has_private_window_marker("Janela Privada"));

        // Spanish / French / German
        assert!(has_private_window_marker("Ventana de incógnito"));
        assert!(has_private_window_marker("Navigation privée"));
        assert!(has_private_window_marker("Privates Surfen"));

        // Normal windows
        assert!(!has_private_window_marker("Google Chrome"));
        assert!(!has_private_window_marker("Inbox (3) - Mail"));
        assert!(!has_private_window_marker("Settings - Clovy"));
    }

    #[test]
    fn test_dedupe_and_cap_text() {
        let chunks = vec![
            "Header".to_string(),
            "Header".to_string(), // duplicate
            "Body paragraph".to_string(),
            "Body paragraph".to_string(), // duplicate
            "Footer".to_string(),
        ];
        let deduped = dedupe_and_cap_text(&chunks, 1000);
        assert_eq!(deduped, "Header\nBody paragraph\nFooter");

        // Test capping
        let capped = dedupe_and_cap_text(&chunks, 10);
        assert!(capped.len() <= 10);
    }

    #[test]
    #[ignore = "requires live Accessibility TCC grant"]
    fn test_live_focused_window_and_ax_walk() {
        let mut settled = HashSet::new();
        let win = focused_window(&mut settled);
        if let Some(w) = win {
            let text = accessibility_text(&w);
            assert!(text.is_some());
        }
    }
}
