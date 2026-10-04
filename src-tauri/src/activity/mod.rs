//! Activity capture: opt-in, text-only recording of what the user sees and
//! does on the Mac, stored in the encrypted `activity.sqlite3`. See
//! `docs/activity-capture.md` for the schema and the API later slices use.
//!
//! Layout: `engine` (2 s tick and privacy decisions), `platform` (OS seam,
//! `macos` implements it), `store` + `key` (SQLCipher database and its
//! Keychain key), `settings`, `filter`, `schedule`, `input`, `redact`.

pub mod engine;
pub mod filter;
pub mod input;
pub mod key;
#[cfg(target_os = "macos")]
pub mod macos;
pub mod platform;
pub mod redact;
pub mod schedule;
pub mod settings;
pub mod store;

use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::domain::types::AppError;
use engine::ActivityShared;
use key::ActivityKeyStore;
use platform::{ActivityPermission, PermissionSnapshot};
use schedule::{CaptureState, StoreStatus};
use settings::ActivitySettings;
use store::{ActivityStore, ACTIVITY_DB_FILE};

pub const ACTIVITY_STATE_EVENT: &str = "clovy://activity-state";
/// Rows written by the debug export.
const DEBUG_EXPORT_LIMIT: u32 = 500;

/// Everything the commands need once capture is supported on this platform.
pub struct ActivityRuntime {
    shared: Arc<ActivityShared>,
    keys: Arc<dyn ActivityKeyStore>,
    db_path: PathBuf,
    settings_path: PathBuf,
    data_dir: PathBuf,
}

/// Tauri-managed handle; `None` where capture is unsupported (Windows/Linux).
#[derive(Default)]
pub struct ActivityState(Option<ActivityRuntime>);

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityStatusDto {
    pub supported: bool,
    pub settings: ActivitySettings,
    pub state: CaptureState,
    pub permissions: PermissionSnapshot,
    pub manual_pause: bool,
    pub last_frame_at: Option<String>,
    pub debug_export_available: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityDebugExportDto {
    pub path: String,
    pub frames: usize,
    pub secondary_frames: usize,
    pub input_events: usize,
    pub pauses: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveActivitySettingsRequest {
    pub settings: ActivitySettings,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetActivityPausedRequest {
    pub paused: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestActivityPermissionRequest {
    pub permission: ActivityPermission,
}

fn status_of(runtime: Option<&ActivityRuntime>) -> ActivityStatusDto {
    match runtime {
        Some(runtime) => ActivityStatusDto {
            supported: true,
            settings: runtime.shared.settings(),
            state: runtime.shared.state(),
            permissions: runtime.shared.permissions(),
            manual_pause: runtime.shared.manual_pause(),
            last_frame_at: runtime.shared.last_frame_at(),
            debug_export_available: cfg!(debug_assertions),
        },
        None => ActivityStatusDto {
            supported: false,
            settings: ActivitySettings::default(),
            state: CaptureState::Off,
            permissions: PermissionSnapshot::UNSUPPORTED,
            manual_pause: false,
            last_frame_at: None,
            debug_export_available: false,
        },
    }
}

fn runtime<'a>(state: &'a State<'_, ActivityState>) -> Result<&'a ActivityRuntime, AppError> {
    state.0.as_ref().ok_or_else(|| {
        AppError::new(
            "activity_unsupported",
            "Activity capture is available on macOS only.",
        )
    })
}

/// Pushes the current status to the frontend and the menu bar.
pub fn publish(app: &AppHandle) {
    let state = app.state::<ActivityState>();
    let status = status_of(state.0.as_ref());
    crate::menu_bar::set_activity_state(app, &status.state, status.manual_pause);
    let _ = app.emit(ACTIVITY_STATE_EVENT, status);
}

/// Pause or resume from the menu bar (same path as the settings button).
pub fn set_manual_pause(app: &AppHandle, paused: bool) {
    if let Some(runtime) = app.state::<ActivityState>().0.as_ref() {
        runtime.shared.set_manual_pause(paused);
    }
    publish(app);
}

#[tauri::command]
pub fn activity_status(state: State<'_, ActivityState>) -> ActivityStatusDto {
    status_of(state.0.as_ref())
}

#[tauri::command]
pub fn activity_save_settings(
    app: AppHandle,
    state: State<'_, ActivityState>,
    request: SaveActivitySettingsRequest,
) -> Result<ActivityStatusDto, AppError> {
    let runtime = runtime(&state)?;
    let settings = request.settings.normalized();
    settings::save(&runtime.settings_path, &settings)
        .map_err(|error| AppError::new("activity_settings_save_failed", error.to_string()))?;
    if !settings.enabled {
        runtime.shared.set_manual_pause(false);
    }
    runtime.shared.set_settings(settings);
    publish(&app);
    Ok(status_of(Some(runtime)))
}

#[tauri::command]
pub fn activity_set_paused(
    app: AppHandle,
    state: State<'_, ActivityState>,
    request: SetActivityPausedRequest,
) -> Result<ActivityStatusDto, AppError> {
    let runtime = runtime(&state)?;
    runtime.shared.set_manual_pause(request.paused);
    publish(&app);
    Ok(status_of(Some(runtime)))
}

#[tauri::command]
pub fn activity_request_permission(
    state: State<'_, ActivityState>,
    request: RequestActivityPermissionRequest,
) -> Result<ActivityStatusDto, AppError> {
    let runtime = runtime(&state)?;
    runtime.shared.request_permission(request.permission);
    Ok(status_of(Some(runtime)))
}

/// User-confirmed: deletes the unreadable database and starts a new one with
/// a new key. The old data cannot be recovered without the old key.
#[tauri::command]
pub async fn activity_recreate_database(
    app: AppHandle,
    state: State<'_, ActivityState>,
) -> Result<ActivityStatusDto, AppError> {
    let runtime = runtime(&state)?;
    if let Some(store) = runtime.shared.take_store() {
        store.close().await;
    }
    match ActivityStore::recreate(&runtime.db_path, runtime.keys.as_ref()).await {
        Ok(store) => runtime
            .shared
            .replace_store(Some(store), StoreStatus::Ready),
        Err(error) => {
            runtime
                .shared
                .replace_store(None, StoreStatus::Failed(error.to_string()));
            return Err(AppError::new("activity_recreate_failed", error.to_string()));
        }
    }
    publish(&app);
    Ok(status_of(Some(runtime)))
}

/// Development builds only: writes the newest activity rows to a readable
/// JSON file in the (dev) data directory for verification. The key is never
/// part of the output.
#[tauri::command]
pub async fn activity_debug_export(
    state: State<'_, ActivityState>,
) -> Result<ActivityDebugExportDto, AppError> {
    if !cfg!(debug_assertions) {
        return Err(AppError::new(
            "activity_debug_export_unavailable",
            "The activity debug export exists only in development builds.",
        ));
    }
    let runtime = runtime(&state)?;
    let store = runtime.shared.store().ok_or_else(|| {
        AppError::new(
            "activity_database_closed",
            "The activity database is not open. Turn capture on first.",
        )
    })?;
    let export = store
        .debug_export(
            &runtime.data_dir.join("activity-debug-exports"),
            chrono::Utc::now(),
            DEBUG_EXPORT_LIMIT,
        )
        .await
        .map_err(|error| AppError::new("activity_debug_export_failed", error.to_string()))?;
    Ok(ActivityDebugExportDto {
        path: export.path.display().to_string(),
        frames: export.frames,
        secondary_frames: export.secondary_frames,
        input_events: export.input_events,
        pauses: export.pauses,
    })
}

/// Registers the managed state and, on macOS, starts the capture thread. The
/// thread ticks while capture is off too (only permissions are read then), so
/// the Activity tab shows live permission state.
pub fn setup(app: &mut tauri::App) {
    #[cfg(target_os = "macos")]
    {
        match start(app.handle()) {
            Ok(runtime) => {
                app.manage(ActivityState(Some(runtime)));
                return;
            }
            Err(error) => tracing::warn!(%error, "activity capture unavailable"),
        }
    }
    app.manage(ActivityState::default());
}

#[cfg(target_os = "macos")]
fn start(app: &AppHandle) -> Result<ActivityRuntime, tauri::Error> {
    let data_dir = crate::app_paths::app_data_dir(app)?;
    let config_dir = crate::app_paths::app_config_dir(app)?;
    let settings_path = settings::settings_path(&config_dir);
    let db_path = data_dir.join(ACTIVITY_DB_FILE);
    let shared = Arc::new(ActivityShared::new(settings::load(&settings_path)));
    let keys: Arc<dyn ActivityKeyStore> = Arc::new(key::KeychainKeyStore::for_current_build());

    let thread_shared = Arc::clone(&shared);
    let thread_keys = Arc::clone(&keys);
    let thread_db = db_path.clone();
    let handle = app.clone();
    std::thread::Builder::new()
        .name("clovy-activity-capture".into())
        .spawn(move || {
            let engine = engine::CaptureEngine::new(
                macos::MacActivityPlatform::new(),
                thread_shared,
                thread_keys,
                thread_db,
                Box::new(engine::SystemClock),
                std::process::id() as i32,
            );
            engine::run_capture_loop(engine, schedule::TICK, |report| {
                if report.state_changed || report.permissions_changed {
                    publish(&handle);
                }
            });
        })?;

    Ok(ActivityRuntime {
        shared,
        keys,
        db_path,
        settings_path,
        data_dir,
    })
}
