use crate::interface_locale::{self, MenuStrings};
use serde::Deserialize;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex,
};
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
    App, AppHandle, Emitter, Listener, Manager, Runtime,
};

const TRAY_ID: &str = "agent-menu-bar";

/// Set by `set_dictation_active` (called from the dictation seam) and read when
/// rendering the tray. Independent of the agent-session state so a dictation
/// edge can never clobber it.
static DICTATION_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Set by `set_meeting_recording_active` (called from the meeting-HUD capture
/// supervisor) — true while a note recording is actively capturing. Independent
/// of `DICTATION_ACTIVE`: both can be on at once (dictation during a recording),
/// and the red dot shows if either is.
static MEETING_RECORDING_ACTIVE: AtomicBool = AtomicBool::new(false);

/// The last agent-session state seen by the agent listener. The dictation
/// listener needs it to re-render the tooltip (which carries both the agent
/// status and the dictation indicator) without an agent payload of its own.
static LAST_AGENT_STATE: Mutex<Option<AgentMenuBarState>> = Mutex::new(None);

/// Last activity-capture state published by `crate::activity::publish`, with
/// whether the pause is the user's own (so the menu offers "Resume"). `None`
/// until the activity runtime starts (and always where it is unsupported), in
/// which case the menu shows no activity items.
static ACTIVITY_MENU_STATE: Mutex<Option<ActivityMenuState>> = Mutex::new(None);

#[derive(Clone, Debug)]
struct ActivityMenuState {
    state: crate::activity::schedule::CaptureState,
    manual_pause: bool,
}

/// The Clovy logo mark as a macOS template image (black glyph on transparent).
/// The menu bar must show the same mark as the app icon, but the app icon itself
/// can't be used
/// directly: template rendering keeps only the alpha channel, so the icon's
/// opaque squircle background becomes a solid blob instead of the glyph.
const TRAY_ICON_TEMPLATE_PNG: &[u8] = include_bytes!("../icons/tray-icon-template.png");
/// Shown while a dictation take runs: Clovy's mark plus a recording indicator.
/// Unlike the logo these are full-colour (NON-template) images, because macOS
/// flattens a template image to monochrome and would drop the red. That is why
/// there are two — one per menu-bar appearance — selected by `menu_bar_is_dark`:
/// a white mark for a dark bar, a black mark for a light one.
const TRAY_ICON_DICTATING_DARK_PNG: &[u8] = include_bytes!("../icons/tray-icon-dictating-dark.png");
const TRAY_ICON_DICTATING_LIGHT_PNG: &[u8] =
    include_bytes!("../icons/tray-icon-dictating-light.png");
const AGENT_MENU_BAR_STATE_EVENT: &str = "clovy:menu-bar:agent-state";
/// Carries the native dictation indicator (a bare `true`/`false`) from the
/// dictation seam to the tray, so all tray mutation stays inside this module.
const DICTATION_MENU_BAR_STATE_EVENT: &str = "clovy:menu-bar:dictation-state";
/// Carries the note-recording indicator (a bare `true`/`false`) from the capture
/// supervisor to the tray. Same rationale as the dictation event.
const MEETING_RECORDING_MENU_BAR_STATE_EVENT: &str = "clovy:menu-bar:recording-state";
const AGENT_MENU_BAR_NEW_SESSION_EVENT: &str = "clovy:menu-bar:new-agent-session";
const AGENT_MENU_BAR_OPEN_SESSION_EVENT: &str = "clovy:menu-bar:open-agent-session";
const AGENT_MENU_BAR_SET_AGENT_HUD_EVENT: &str = "clovy:menu-bar:set-agent-hud";
const AGENT_MENU_BAR_OPEN_SETTINGS_EVENT: &str = "clovy://open-settings";
/// Bare trigger: the activity state itself is stored in
/// `ACTIVITY_MENU_STATE`; the listener only rebuilds the menu.
const ACTIVITY_MENU_BAR_STATE_EVENT: &str = "clovy:menu-bar:activity-state";

const MENU_SHOW_ID: &str = "agent_menu_bar_show";
const MENU_SETTINGS_ID: &str = "agent_menu_bar_settings";
const MENU_NEW_SESSION_ID: &str = "agent_menu_bar_new_session";
const MENU_SHOW_AGENT_HUD_ID: &str = "agent_menu_bar_show_agent_hud";
const MENU_HIDE_AGENT_HUD_ID: &str = "agent_menu_bar_hide_agent_hud";
const MENU_QUIT_ID: &str = "agent_menu_bar_quit";
const MENU_STATUS_ID: &str = "agent_menu_bar_status";
const MENU_LAST_STATUS_ID: &str = "agent_menu_bar_last_status";
const MENU_ACTIVITY_STATUS_ID: &str = "agent_menu_bar_activity_status";
const MENU_ACTIVITY_TOGGLE_PAUSE_ID: &str = "agent_menu_bar_activity_toggle_pause";
const MENU_SESSION_ID_PREFIX: &str = "agent_menu_bar_session:";

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentMenuBarState {
    #[serde(default)]
    active_count: usize,
    #[serde(default)]
    needs_user_count: usize,
    #[serde(default)]
    sessions: Vec<AgentMenuBarSession>,
    #[serde(default = "default_agent_hud_enabled")]
    agent_hud_enabled: bool,
    #[serde(default)]
    last_status: Option<AgentMenuBarLastStatus>,
}

impl Default for AgentMenuBarState {
    fn default() -> Self {
        Self {
            active_count: 0,
            needs_user_count: 0,
            sessions: Vec::new(),
            agent_hud_enabled: default_agent_hud_enabled(),
            last_status: None,
        }
    }
}

fn default_agent_hud_enabled() -> bool {
    true
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentMenuBarSession {
    id: String,
    title: String,
    status: AgentMenuBarSessionStatus,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
enum AgentMenuBarSessionStatus {
    Idle,
    Running,
    WaitingForUser,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentMenuBarLastStatus {
    #[serde(default)]
    title: Option<String>,
    status: String,
    #[serde(default)]
    summary: Option<String>,
}

pub fn setup(app: &mut App) -> tauri::Result<()> {
    let initial_state = AgentMenuBarState::default();
    let initial_menu = build_menu(app, &initial_state)?;
    let mut tray_builder = TrayIconBuilder::with_id(TRAY_ID)
        .menu(&initial_menu)
        .tooltip(tray_tooltip(&initial_state, false, false))
        .show_menu_on_left_click(true)
        .on_menu_event(handle_menu_event);

    match tauri::image::Image::from_bytes(TRAY_ICON_TEMPLATE_PNG) {
        Ok(icon) => {
            tray_builder = tray_builder.icon(icon).icon_as_template(true);
        }
        // The embedded asset can only fail to decode if it was corrupted at
        // build time; a wrong-looking menu bar item beats a missing one.
        Err(_) => {
            if let Some(icon) = app.handle().default_window_icon().cloned() {
                tray_builder = tray_builder.icon(icon).icon_as_template(true);
            }
        }
    }

    let tray = tray_builder.build(app)?;
    update_tray(app.handle(), &tray, &initial_state);

    let handle = app.handle().clone();
    app.listen_any(AGENT_MENU_BAR_STATE_EVENT, move |event| {
        let Ok(state) = serde_json::from_str::<AgentMenuBarState>(event.payload()) else {
            return;
        };
        if let Ok(mut last) = LAST_AGENT_STATE.lock() {
            *last = Some(state.clone());
        }
        let Some(tray) = handle.tray_by_id(TRAY_ID) else {
            return;
        };
        if let Ok(menu) = build_menu(&handle, &state) {
            let _ = tray.set_menu(Some(menu));
        }
        update_tray(&handle, &tray, &state);
    });

    // Independent listeners for the recording indicators: they must never
    // rebuild the menu or touch agent-session state, only re-render the tray
    // (icon + tooltip) from the last-seen agent state plus their own flag.
    let handle = app.handle().clone();
    app.listen_any(DICTATION_MENU_BAR_STATE_EVENT, move |event| {
        DICTATION_ACTIVE.store(event.payload() == "true", Ordering::SeqCst);
        refresh_tray(&handle);
    });

    let handle = app.handle().clone();
    app.listen_any(MEETING_RECORDING_MENU_BAR_STATE_EVENT, move |event| {
        MEETING_RECORDING_ACTIVE.store(event.payload() == "true", Ordering::SeqCst);
        refresh_tray(&handle);
    });

    let handle = app.handle().clone();
    app.listen_any(ACTIVITY_MENU_BAR_STATE_EVENT, move |_| {
        relocalize(&handle);
    });

    Ok(())
}

/// Rebuilds the tray menu and tooltip in the current interface language from
/// the last-seen agent state. Called after the locale changes.
pub fn relocalize(app: &AppHandle) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    let state = LAST_AGENT_STATE
        .lock()
        .ok()
        .and_then(|last| last.clone())
        .unwrap_or_default();
    if let Ok(menu) = build_menu(app, &state) {
        let _ = tray.set_menu(Some(menu));
    }
    update_tray(app, &tray, &state);
}

/// Re-renders the tray from the current cached agent state and the recording
/// flags. Used by the dictation and recording listeners, which only change a
/// flag and never the menu or agent state.
fn refresh_tray(app: &AppHandle) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    let state = LAST_AGENT_STATE
        .lock()
        .ok()
        .and_then(|last| last.clone())
        .unwrap_or_default();
    update_tray(app, &tray, &state);
}

/// Emits the current dictation-take state toward the tray. Called from the
/// dictation seam (`dictation.rs`); routing through the event bus keeps every
/// tray mutation inside this module. Safe to call off the main thread.
pub fn set_dictation_active(app: &AppHandle, active: bool) {
    let _ = app.emit(DICTATION_MENU_BAR_STATE_EVENT, active);
}

/// Emits the current note-recording state toward the tray. Called from the
/// meeting-HUD capture supervisor (`meeting_hud.rs`). Same event-bus routing and
/// off-main-thread safety as `set_dictation_active`.
pub fn set_meeting_recording_active(app: &AppHandle, active: bool) {
    let _ = app.emit(MEETING_RECORDING_MENU_BAR_STATE_EVENT, active);
}

/// Records the activity-capture state for the tray and asks the tray to
/// rebuild its menu. Called from `crate::activity::publish`, on any thread.
pub fn set_activity_state(
    app: &AppHandle,
    state: &crate::activity::schedule::CaptureState,
    manual_pause: bool,
) {
    if let Ok(mut current) = ACTIVITY_MENU_STATE.lock() {
        *current = Some(ActivityMenuState {
            state: state.clone(),
            manual_pause,
        });
    }
    let _ = app.emit(ACTIVITY_MENU_BAR_STATE_EVENT, ());
}

fn handle_menu_event(app: &AppHandle, event: tauri::menu::MenuEvent) {
    let id = event.id().as_ref();
    if id == MENU_SHOW_ID {
        show_main_window(app);
        return;
    }
    if id == MENU_SETTINGS_ID {
        show_main_window(app);
        let _ = app.emit(AGENT_MENU_BAR_OPEN_SETTINGS_EVENT, ());
        return;
    }
    if id == MENU_NEW_SESSION_ID {
        show_main_window(app);
        let _ = app.emit(AGENT_MENU_BAR_NEW_SESSION_EVENT, ());
        return;
    }
    if id == MENU_SHOW_AGENT_HUD_ID {
        let _ = app.emit(AGENT_MENU_BAR_SET_AGENT_HUD_EVENT, true);
        return;
    }
    if id == MENU_HIDE_AGENT_HUD_ID {
        let _ = app.emit(AGENT_MENU_BAR_SET_AGENT_HUD_EVENT, false);
        return;
    }
    if id == MENU_ACTIVITY_TOGGLE_PAUSE_ID {
        let manual_pause = ACTIVITY_MENU_STATE
            .lock()
            .ok()
            .and_then(|current| current.as_ref().map(|activity| activity.manual_pause))
            .unwrap_or(false);
        crate::activity::set_manual_pause(app, !manual_pause);
        return;
    }
    if id == MENU_QUIT_ID {
        app.exit(0);
        return;
    }
    if let Some(session_id) = id.strip_prefix(MENU_SESSION_ID_PREFIX) {
        show_main_window(app);
        let _ = app.emit(AGENT_MENU_BAR_OPEN_SESSION_EVENT, session_id.to_string());
    }
}

fn build_menu<R, M>(manager: &M, state: &AgentMenuBarState) -> tauri::Result<Menu<R>>
where
    R: Runtime,
    M: Manager<R>,
{
    let menu = Menu::new(manager)?;
    let text = interface_locale::strings(interface_locale::current());

    let show_item =
        MenuItem::with_id(manager, MENU_SHOW_ID, text.open_clovy(), true, None::<&str>)?;
    let settings_item = MenuItem::with_id(
        manager,
        MENU_SETTINGS_ID,
        text.settings(),
        true,
        None::<&str>,
    )?;
    let new_session_item = MenuItem::with_id(
        manager,
        MENU_NEW_SESSION_ID,
        text.new_session(),
        true,
        None::<&str>,
    )?;
    let status_item = MenuItem::with_id(
        manager,
        MENU_STATUS_ID,
        escape_menu_text(status_label(&text, state)),
        false,
        None::<&str>,
    )?;

    menu.append(&show_item)?;
    let agent_hud_item = MenuItem::with_id(
        manager,
        if state.agent_hud_enabled {
            MENU_HIDE_AGENT_HUD_ID
        } else {
            MENU_SHOW_AGENT_HUD_ID
        },
        if state.agent_hud_enabled {
            text.hide_sessions_hud()
        } else {
            text.show_sessions_hud()
        },
        true,
        None::<&str>,
    )?;
    menu.append(&agent_hud_item)?;
    menu.append(&PredefinedMenuItem::separator(manager)?)?;
    menu.append(&status_item)?;
    menu.append(&new_session_item)?;

    if let Some(last_status) = state.last_status.as_ref() {
        let last_status_item = MenuItem::with_id(
            manager,
            MENU_LAST_STATUS_ID,
            escape_menu_text(last_status_label(&text, last_status)),
            false,
            None::<&str>,
        )?;
        menu.append(&last_status_item)?;
    }

    for session in &state.sessions {
        let session_item = MenuItem::with_id(
            manager,
            format!("{MENU_SESSION_ID_PREFIX}{}", session.id),
            escape_menu_text(session_label(&text, session)),
            true,
            None::<&str>,
        )?;
        menu.append(&session_item)?;
    }

    let activity = ACTIVITY_MENU_STATE
        .lock()
        .ok()
        .and_then(|current| current.clone());
    if let Some(activity) = activity {
        use crate::activity::schedule::CaptureState;
        menu.append(&PredefinedMenuItem::separator(manager)?)?;
        menu.append(&MenuItem::with_id(
            manager,
            MENU_ACTIVITY_STATUS_ID,
            escape_menu_text(text.activity_status(&activity.state)),
            false,
            None::<&str>,
        )?)?;
        let capturing = !matches!(activity.state, CaptureState::Off);
        if capturing {
            menu.append(&MenuItem::with_id(
                manager,
                MENU_ACTIVITY_TOGGLE_PAUSE_ID,
                if activity.manual_pause {
                    text.resume_capture()
                } else {
                    text.pause_capture()
                },
                true,
                None::<&str>,
            )?)?;
        }
    }

    menu.append(&PredefinedMenuItem::separator(manager)?)?;
    menu.append(&settings_item)?;
    menu.append(&PredefinedMenuItem::separator(manager)?)?;

    let quit_item =
        MenuItem::with_id(manager, MENU_QUIT_ID, text.quit_clovy(), true, None::<&str>)?;
    menu.append(&quit_item)?;

    Ok(menu)
}

fn update_tray<R: Runtime>(
    app: &AppHandle<R>,
    tray: &tauri::tray::TrayIcon<R>,
    state: &AgentMenuBarState,
) {
    let dictation_active = DICTATION_ACTIVE.load(Ordering::SeqCst);
    let recording_active = MEETING_RECORDING_ACTIVE.load(Ordering::SeqCst);
    // Keep the macOS menu extra compact and logo-only. Status details live in
    // the tooltip and dropdown menu; setting a title renders a wide text item
    // beside the icon in the menu bar.
    let _ = tray.set_title::<&str>(None);
    let _ = tray.set_tooltip(Some(tray_tooltip(
        state,
        dictation_active,
        recording_active,
    )));
    // The red recording dot shows while either activity is capturing audio.
    apply_tray_icon(app, tray, dictation_active || recording_active);
}

/// While recording or dictating, show the full-colour "≈ + red dot" mark (so the
/// dot stays red) matched to the menu-bar appearance; otherwise the adaptive
/// monochrome logo template. On a decode failure the current icon is left in
/// place — a stale-but-present icon beats a missing one.
fn apply_tray_icon<R: Runtime>(
    app: &AppHandle<R>,
    tray: &tauri::tray::TrayIcon<R>,
    show_recording_dot: bool,
) {
    let (bytes, is_template) = if show_recording_dot {
        let bytes = if menu_bar_is_dark(app) {
            TRAY_ICON_DICTATING_DARK_PNG
        } else {
            TRAY_ICON_DICTATING_LIGHT_PNG
        };
        (bytes, false)
    } else {
        (TRAY_ICON_TEMPLATE_PNG, true)
    };
    if let Ok(icon) = tauri::image::Image::from_bytes(bytes) {
        // Atomic on macOS (no icon+template flicker); falls back to set_icon
        // on other platforms.
        let _ = tray.set_icon_with_as_template(Some(icon), is_template);
    }
}

/// Whether the menu bar renders dark, so the dictating icon can pick the
/// matching full-colour variant. Follows the main window's effective appearance
/// (which tracks the system Light/Dark setting); defaults to dark if unknown.
fn menu_bar_is_dark<R: Runtime>(app: &AppHandle<R>) -> bool {
    app.get_webview_window("main")
        .and_then(|window| window.theme().ok())
        .map(|theme| theme == tauri::Theme::Dark)
        .unwrap_or(true)
}

fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn tray_tooltip(
    state: &AgentMenuBarState,
    dictation_active: bool,
    recording_active: bool,
) -> String {
    let text = interface_locale::strings(interface_locale::current());
    let status = status_label(&text, state);
    // Sentence case, plain hyphens — matches the existing tooltip and the repo
    // copy specs.
    let activity = text.activity(recording_active, dictation_active);
    format!("Clovy - {activity}{status}")
}

fn status_label(text: &MenuStrings, state: &AgentMenuBarState) -> String {
    if state.needs_user_count > 0 {
        let waiting = text.sessions(state.needs_user_count);
        let needs_approval = text.needs_approval(state.needs_user_count);
        if state.active_count > state.needs_user_count {
            let working_count = state.active_count - state.needs_user_count;
            return format!(
                "{waiting} {needs_approval}, {}",
                text.working(working_count)
            );
        }
        return format!("{waiting} {needs_approval}");
    }
    if state.active_count > 0 {
        return text.working(state.active_count);
    }
    text.no_active_sessions().to_string()
}

fn last_status_label(text: &MenuStrings, last_status: &AgentMenuBarLastStatus) -> String {
    let title = last_status
        .title
        .as_deref()
        .map(normalize_menu_text)
        .filter(|value| !value.is_empty());
    let summary = last_status
        .summary
        .as_deref()
        .map(normalize_menu_text)
        .filter(|value| !value.is_empty());
    let status = text.readable_status(&last_status.status);

    match (title, summary) {
        (Some(title), Some(summary)) => text.last(&format!("{title} - {summary}")),
        (Some(title), None) => text.last(&format!("{title} - {status}")),
        (None, Some(summary)) => text.last(&summary),
        (None, None) => text.last(status),
    }
}

fn session_label(text: &MenuStrings, session: &AgentMenuBarSession) -> String {
    let title = normalize_menu_text(&session.title);
    let title = if title.is_empty() {
        text.untitled_session().to_string()
    } else {
        title
    };
    let prefix = match session.status {
        AgentMenuBarSessionStatus::WaitingForUser => text.session_prefix_needs_approval(),
        AgentMenuBarSessionStatus::Running => text.session_prefix_working(),
        AgentMenuBarSessionStatus::Idle => "",
    };
    format!("{prefix}{title}")
}

fn normalize_menu_text(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn escape_menu_text(value: String) -> String {
    value.replace('&', "&&")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tooltip_reflects_recording_and_dictation() {
        let s = AgentMenuBarState::default();
        // (dictation, recording)
        assert_eq!(tray_tooltip(&s, false, false), "Clovy - No active sessions");
        assert_eq!(
            tray_tooltip(&s, true, false),
            "Clovy - Dictating - No active sessions"
        );
        assert_eq!(
            tray_tooltip(&s, false, true),
            "Clovy - Recording - No active sessions"
        );
        assert_eq!(
            tray_tooltip(&s, true, true),
            "Clovy - Recording and dictating - No active sessions"
        );
    }

    #[test]
    fn tray_labels_follow_the_interface_language() {
        use crate::interface_locale::{strings, UiLocale};
        let state = AgentMenuBarState {
            active_count: 3,
            needs_user_count: 1,
            ..AgentMenuBarState::default()
        };
        assert_eq!(
            status_label(&strings(UiLocale::En), &state),
            "1 session needs approval, 2 sessions working"
        );
        assert_eq!(
            status_label(&strings(UiLocale::PtBr), &state),
            "1 sessão precisa de aprovação, 2 sessões trabalhando"
        );
        assert_eq!(
            status_label(&strings(UiLocale::PtBr), &AgentMenuBarState::default()),
            "Nenhuma sessão ativa"
        );
        let session = AgentMenuBarSession {
            id: "s1".into(),
            title: "  ".into(),
            status: AgentMenuBarSessionStatus::WaitingForUser,
        };
        assert_eq!(
            session_label(&strings(UiLocale::PtBr), &session),
            "Precisa de aprovação - Sessão sem título"
        );
        assert_eq!(
            session_label(&strings(UiLocale::En), &session),
            "Needs Approval - Untitled session"
        );
        let last = AgentMenuBarLastStatus {
            title: None,
            status: "completed".into(),
            summary: None,
        };
        assert_eq!(
            last_status_label(&strings(UiLocale::PtBr), &last),
            "Última: Concluída"
        );
        assert_eq!(
            last_status_label(&strings(UiLocale::En), &last),
            "Last: Completed"
        );
    }

    #[test]
    fn logo_tray_icon_is_a_real_template_image() {
        let icon = tauri::image::Image::from_bytes(TRAY_ICON_TEMPLATE_PNG)
            .expect("embedded logo tray template PNG must decode");
        assert_eq!(icon.width(), icon.height(), "menu bar icon must be square");
        // macOS template rendering uses only the alpha channel: the mark must be
        // opaque and the background transparent, or the menu bar shows a solid
        // blob (the bug this asset exists to fix). Both must be present.
        let alphas: Vec<u8> = icon.rgba().chunks(4).map(|px| px[3]).collect();
        assert!(
            alphas.contains(&0),
            "template needs a transparent background"
        );
        assert!(alphas.contains(&255), "template needs an opaque mark");
        // Corners stay transparent — an opaque squircle background (the app
        // icon's shape) would fail here.
        let side = icon.width() as usize;
        for corner in [0, side - 1, side * (side - 1), side * side - 1] {
            assert_eq!(alphas[corner], 0, "corner pixels must be transparent");
        }
    }

    #[test]
    fn dictating_tray_icons_carry_a_red_recording_dot() {
        // These are deliberately full-colour (NON-template) so the dot renders
        // red; a template would flatten it to monochrome. One variant per
        // menu-bar appearance.
        for (name, bytes) in [
            ("dark", TRAY_ICON_DICTATING_DARK_PNG),
            ("light", TRAY_ICON_DICTATING_LIGHT_PNG),
        ] {
            let icon = tauri::image::Image::from_bytes(bytes)
                .unwrap_or_else(|_| panic!("embedded {name} dictating PNG must decode"));
            assert_eq!(icon.width(), icon.height(), "{name} icon must be square");
            let rgba = icon.rgba();
            let side = icon.width() as usize;
            // Corners transparent — the wave and dot sit inside the canvas.
            for corner in [0, side - 1, side * (side - 1), side * side - 1] {
                assert_eq!(rgba[corner * 4 + 3], 0, "{name} corner must be transparent");
            }
            // A clearly red, opaque pixel exists: the recording dot.
            let has_red = rgba
                .chunks(4)
                .any(|px| px[3] > 200 && px[0] > 200 && px[1] < 100 && px[2] < 100);
            assert!(has_red, "{name} icon must contain a red recording dot");
        }
    }
}
