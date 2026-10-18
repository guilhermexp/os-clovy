//! The real icons of "Top apps": resolved by bundle id through Launch Services
//! on macOS, or by executable/AUMID on Windows, drawn at 64x64 px, and returned
//! as PNG data URLs. Never by app name (two apps can share one) and never from
//! a remote icon service. Answers, including "no icon" for an app that is gone,
//! are cached per identity for the life of the process, so a render or a
//! timeline refresh never looks an app up again.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use serde::Deserialize;

use crate::domain::types::AppError;

/// Pixel size of the drawn icon (32 pt at 2x).
#[cfg_attr(not(any(target_os = "macos", windows)), allow(dead_code))]
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

/// An OS app identity, not an arbitrary path. Windows AUMIDs also contain `!`.
fn plausible_bundle_id(bundle_id: &str) -> bool {
    !bundle_id.is_empty()
        && bundle_id.len() <= 255
        && bundle_id
            .chars()
            .all(|c| {
                c.is_ascii_alphanumeric()
                    || matches!(c, '.' | '-' | '_')
                    || (cfg!(windows) && (c == '!' || c == ' ' || c.is_alphanumeric()))
            })
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

#[cfg(not(any(target_os = "macos", windows)))]
fn icon_png(_bundle_id: &str) -> Option<Vec<u8>> {
    None
}

#[cfg(windows)]
use windows_icon::icon_png;

#[cfg(windows)]
mod windows_icon {
    use std::os::windows::ffi::OsStrExt;

    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{HGLOBAL, RPC_E_CHANGED_MODE};
    use windows::Win32::Graphics::Imaging::{
        CLSID_WICImagingFactory, GUID_ContainerFormatPng, GUID_WICPixelFormat32bppBGRA,
        IWICImagingFactory, WICBitmapEncoderNoCache, WICBitmapInterpolationModeFant,
    };
    use windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES;
    use windows::Win32::System::Com::StructuredStorage::{CreateStreamOnHGlobal, IPropertyBag2};
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
        COINIT_MULTITHREADED, STATFLAG_NONAME, STATSTG, STREAM_SEEK_SET,
    };
    use windows::Win32::UI::Shell::{SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON};
    use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, HICON};

    use super::{ICON_PX, MAX_PNG_BYTES};
    use crate::activity::windows::window::executable_path;

    struct ComApartment(bool);

    impl ComApartment {
        fn enter() -> Option<Self> {
            let result = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
            if result.is_ok() {
                Some(Self(true))
            } else if result == RPC_E_CHANGED_MODE {
                // WIC also works in the caller's existing STA; we did not add a COM reference.
                Some(Self(false))
            } else {
                None
            }
        }
    }

    impl Drop for ComApartment {
        fn drop(&mut self) {
            if self.0 {
                unsafe { CoUninitialize() };
            }
        }
    }

    struct OwnedIcon(HICON);

    impl Drop for OwnedIcon {
        fn drop(&mut self) {
            let _ = unsafe { DestroyIcon(self.0) };
        }
    }

    pub(super) fn icon_png(identity: &str) -> Option<Vec<u8>> {
        let path = executable_path(identity)?;
        let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let _apartment = ComApartment::enter()?;
        let mut info = SHFILEINFOW::default();
        if unsafe {
            SHGetFileInfoW(
                PCWSTR(path.as_ptr()),
                FILE_FLAGS_AND_ATTRIBUTES(0),
                Some(&mut info),
                std::mem::size_of::<SHFILEINFOW>() as u32,
                SHGFI_ICON | SHGFI_LARGEICON,
            )
        } == 0
            || info.hIcon.0.is_null()
        {
            return None;
        }
        let icon = OwnedIcon(info.hIcon);
        let factory: IWICImagingFactory =
            unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER) }.ok()?;
        let bitmap = unsafe { factory.CreateBitmapFromHICON(icon.0) }.ok()?;
        let scaler = unsafe { factory.CreateBitmapScaler() }.ok()?;
        unsafe {
            scaler.Initialize(
                &bitmap,
                ICON_PX as u32,
                ICON_PX as u32,
                WICBitmapInterpolationModeFant,
            )
        }
        .ok()?;
        let stream = unsafe { CreateStreamOnHGlobal(HGLOBAL::default(), true) }.ok()?;
        let encoder =
            unsafe { factory.CreateEncoder(&GUID_ContainerFormatPng, std::ptr::null()) }.ok()?;
        unsafe { encoder.Initialize(&stream, WICBitmapEncoderNoCache) }.ok()?;
        let mut frame = None;
        unsafe { encoder.CreateNewFrame(&mut frame, std::ptr::null_mut()) }.ok()?;
        let frame = frame?;
        unsafe { frame.Initialize(None::<&IPropertyBag2>) }.ok()?;
        unsafe { frame.SetSize(ICON_PX as u32, ICON_PX as u32) }.ok()?;
        let mut pixel_format = GUID_WICPixelFormat32bppBGRA;
        unsafe { frame.SetPixelFormat(&mut pixel_format) }.ok()?;
        unsafe { frame.WriteSource(&scaler, std::ptr::null()) }.ok()?;
        unsafe { frame.Commit() }.ok()?;
        unsafe { encoder.Commit() }.ok()?;

        let mut stat = STATSTG::default();
        unsafe { stream.Stat(&mut stat, STATFLAG_NONAME) }.ok()?;
        let len = usize::try_from(stat.cbSize).ok()?;
        if len == 0 || len > MAX_PNG_BYTES {
            return None;
        }
        unsafe { stream.Seek(0, STREAM_SEEK_SET, None) }.ok()?;
        let mut png = Vec::<u8>::with_capacity(len);
        let mut read = 0;
        unsafe { stream.Read(png.as_mut_ptr().cast(), len as u32, Some(&mut read)) }
            .ok()
            .ok()?;
        if read as usize != len {
            return None;
        }
        // IStream initialized exactly `read` bytes in the reserved allocation above.
        unsafe { png.set_len(len) };
        Some(png)
    }
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
