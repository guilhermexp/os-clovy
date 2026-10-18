//! The real icons of the apps in "Top apps": resolved on this Mac by bundle
//! id through Launch Services (`NSWorkspace`), drawn at 64x64 px, and returned
//! as a PNG data URL. Never by app name (two apps can share one) and never
//! over the network. Answers, including "no icon" for an app that is gone,
//! are cached per bundle id for the life of the process, so a render or a
//! timeline refresh never looks an app up again.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use serde::Deserialize;

use crate::domain::types::AppError;

/// Pixel size of the drawn icon (32 pt at 2x).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const ICON_PX: isize = 64;
/// A PNG above this size is refused rather than sent to the webview.
const MAX_PNG_BYTES: usize = 64 * 1024;
/// Distinct bundle ids remembered before the cache starts over.
const CACHE_LIMIT: usize = 256;

static CACHE: LazyLock<Mutex<HashMap<String, Option<String>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppIconRequest {
    pub bundle_id: String,
}

/// A bundle identifier as macOS writes them (reverse DNS); anything else is
/// not looked up.
fn plausible_bundle_id(bundle_id: &str) -> bool {
    !bundle_id.is_empty()
        && bundle_id.len() <= 255
        && bundle_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

/// The app's icon as a PNG data URL, or `None` when the app is not installed,
/// its icon cannot be drawn, or capture is unsupported on this platform.
pub fn app_icon_data_url(bundle_id: &str) -> Option<String> {
    if !plausible_bundle_id(bundle_id) {
        return None;
    }
    if let Some(cached) = CACHE.lock().ok()?.get(bundle_id) {
        return cached.clone();
    }
    let resolved = icon_png(bundle_id)
        .filter(|png| png.len() <= MAX_PNG_BYTES)
        .map(|png| {
            use base64::Engine;
            format!(
                "data:image/png;base64,{}",
                base64::engine::general_purpose::STANDARD.encode(png)
            )
        });
    if let Ok(mut cache) = CACHE.lock() {
        if cache.len() >= CACHE_LIMIT {
            cache.clear();
        }
        cache.insert(bundle_id.to_string(), resolved.clone());
    }
    resolved
}

#[tauri::command]
pub async fn activity_app_icon(request: AppIconRequest) -> Result<Option<String>, AppError> {
    tauri::async_runtime::spawn_blocking(move || app_icon_data_url(&request.bundle_id))
        .await
        .map_err(|error| AppError::new("activity_app_icon_failed", error.to_string()))
}

#[cfg(target_os = "macos")]
fn icon_png(bundle_id: &str) -> Option<Vec<u8>> {
    use objc2::rc::autoreleasepool;
    use objc2::runtime::AnyObject;
    use objc2::AllocAnyThread;
    use objc2_app_kit::{
        NSBitmapImageFileType, NSBitmapImageRep, NSBitmapImageRepPropertyKey,
        NSCompositingOperation, NSDeviceRGBColorSpace, NSGraphicsContext, NSImageInterpolation,
        NSWorkspace,
    };
    use objc2_foundation::{NSDictionary, NSPoint, NSRect, NSSize, NSString};

    autoreleasepool(|_| {
        let workspace = NSWorkspace::sharedWorkspace();
        let url =
            workspace.URLForApplicationWithBundleIdentifier(&NSString::from_str(bundle_id))?;
        let path = url.path()?;
        let icon = workspace.iconForFile(&path);
        // SAFETY: null planes make AppKit allocate the pixel buffer; the
        // colour space is AppKit's own constant.
        let bitmap = unsafe {
            NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
                NSBitmapImageRep::alloc(),
                std::ptr::null_mut(),
                ICON_PX,
                ICON_PX,
                8,
                4,
                true,
                false,
                NSDeviceRGBColorSpace,
                0,
                0,
            )
        }?;
        let context = NSGraphicsContext::graphicsContextWithBitmapImageRep(&bitmap)?;
        NSGraphicsContext::saveGraphicsState_class();
        NSGraphicsContext::setCurrentContext(Some(&context));
        context.setImageInterpolation(NSImageInterpolation::High);
        let side = ICON_PX as f64;
        icon.drawInRect_fromRect_operation_fraction(
            NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(side, side)),
            NSRect::ZERO,
            NSCompositingOperation::Copy,
            1.0,
        );
        context.flushGraphics();
        NSGraphicsContext::restoreGraphicsState_class();
        let properties = NSDictionary::<NSBitmapImageRepPropertyKey, AnyObject>::new();
        // SAFETY: an empty properties dictionary of the declared key type.
        let png = unsafe {
            bitmap.representationUsingType_properties(NSBitmapImageFileType::PNG, &properties)
        }?;
        Some(png.to_vec())
    })
}

#[cfg(not(target_os = "macos"))]
fn icon_png(_bundle_id: &str) -> Option<Vec<u8>> {
    None
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    fn png_of(data_url: &str) -> Vec<u8> {
        use base64::Engine;
        let payload = data_url
            .strip_prefix("data:image/png;base64,")
            .expect("a PNG data URL");
        base64::engine::general_purpose::STANDARD
            .decode(payload)
            .unwrap()
    }

    #[test]
    fn installed_apps_get_their_own_icon_at_the_bounded_size() {
        // Finder and Terminal ship with every macOS install.
        let finder = png_of(&app_icon_data_url("com.apple.finder").expect("Finder icon"));
        let terminal = png_of(&app_icon_data_url("com.apple.Terminal").expect("Terminal icon"));
        for png in [&finder, &terminal] {
            let size = imagesize::blob_size(png).unwrap();
            assert_eq!((size.width, size.height), (64, 64));
            assert!(png.len() <= MAX_PNG_BYTES);
        }
        assert_ne!(finder, terminal, "each app shows its own icon");
    }

    #[test]
    fn an_app_that_is_gone_or_a_malformed_id_has_no_icon() {
        assert_eq!(app_icon_data_url("com.example.clovy.never-installed"), None);
        assert_eq!(app_icon_data_url("../../Applications/Finder"), None);
        assert_eq!(app_icon_data_url(""), None);
    }
}

#[cfg(all(test, target_os = "windows"))]
mod windows_tests {
    use super::*;
    use base64::Engine;

    #[test]
    fn windows_executables_have_distinct_bounded_png_icons() {
        let icons: Vec<Vec<u8>> = ["cmd.exe", "explorer.exe"]
            .into_iter()
            .map(|executable| {
                let url = app_icon_data_url(executable).expect("installed Windows app icon");
                let payload = url.strip_prefix("data:image/png;base64,").expect("PNG URL");
                let png = base64::engine::general_purpose::STANDARD
                    .decode(payload)
                    .unwrap();
                let dimensions = imagesize::blob_size(&png).unwrap();
                assert_eq!((dimensions.width, dimensions.height), (64, 64));
                assert!(png.len() <= MAX_PNG_BYTES);
                png
            })
            .collect();
        assert_ne!(icons[0], icons[1], "each executable must show its own icon");
        assert_eq!(app_icon_data_url("../../Windows/explorer.exe"), None);
        assert_eq!(app_icon_data_url("clovy-never-installed.exe"), None);
    }
}
