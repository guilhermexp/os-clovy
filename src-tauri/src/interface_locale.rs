//! Interface language for the native chrome (app menu and menu bar tray).
//!
//! The webview owns the preference (`src/i18n/locale.ts`, localStorage) and
//! emits `clovy://interface-locale` with the chosen BCP 47 tag on startup and
//! on every change. Rust only mirrors it so the menus it builds read in the
//! same language; unknown tags fall back to English, matching the frontend.
//! This is independent of the dictation/transcription language.

use std::sync::atomic::{AtomicU8, Ordering};

pub const INTERFACE_LOCALE_EVENT: &str = "clovy://interface-locale";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiLocale {
    En,
    PtBr,
}

static CURRENT: AtomicU8 = AtomicU8::new(0);

impl UiLocale {
    /// Parse an event payload: a JSON string ("\"pt-BR\"") or a bare tag.
    pub fn from_payload(payload: &str) -> Self {
        let tag = serde_json::from_str::<String>(payload).unwrap_or_else(|_| payload.to_string());
        Self::from_tag(&tag)
    }

    pub fn from_tag(tag: &str) -> Self {
        let tag = tag.trim().replace('_', "-").to_ascii_lowercase();
        if tag == "pt" || tag.starts_with("pt-") {
            UiLocale::PtBr
        } else {
            UiLocale::En
        }
    }

    fn as_u8(self) -> u8 {
        match self {
            UiLocale::En => 0,
            UiLocale::PtBr => 1,
        }
    }

    fn from_u8(value: u8) -> Self {
        if value == 1 {
            UiLocale::PtBr
        } else {
            UiLocale::En
        }
    }
}

pub fn current() -> UiLocale {
    UiLocale::from_u8(CURRENT.load(Ordering::SeqCst))
}

/// Store the locale; returns true when it changed.
pub fn set_current(locale: UiLocale) -> bool {
    CURRENT.swap(locale.as_u8(), Ordering::SeqCst) != locale.as_u8()
}

/// Native menu copy. English strings are byte-identical to the pre-i18n
/// literals; Portuguese follows the repo copy rules (sentence case, plain
/// hyphens).
#[derive(Clone, Copy, Debug)]
pub struct MenuStrings {
    locale: UiLocale,
}

pub fn strings(locale: UiLocale) -> MenuStrings {
    MenuStrings { locale }
}

impl MenuStrings {
    fn pick(&self, en: &'static str, pt: &'static str) -> &'static str {
        match self.locale {
            UiLocale::En => en,
            UiLocale::PtBr => pt,
        }
    }

    // App menu.
    pub fn check_for_updates(&self) -> &'static str {
        self.pick("Check for Updates", "Procurar atualizações")
    }
    pub fn settings(&self) -> &'static str {
        self.pick("Settings...", "Configurações...")
    }
    pub fn file(&self) -> &'static str {
        self.pick("File", "Arquivo")
    }
    pub fn edit(&self) -> &'static str {
        self.pick("Edit", "Editar")
    }
    pub fn view(&self) -> &'static str {
        self.pick("View", "Visualizar")
    }
    pub fn window(&self) -> &'static str {
        self.pick("Window", "Janela")
    }
    pub fn help(&self) -> &'static str {
        self.pick("Help", "Ajuda")
    }
    pub fn close_tab(&self) -> &'static str {
        self.pick("Close tab", "Fechar aba")
    }
    pub fn close_window(&self) -> &'static str {
        self.pick("Close window", "Fechar janela")
    }

    /// Text for AppKit's predefined items. `None` keeps the platform default
    /// (English), which is exactly the pre-i18n behavior.
    pub fn predefined(&self, item: PredefinedLabel) -> Option<&'static str> {
        if self.locale == UiLocale::En {
            return None;
        }
        Some(match item {
            PredefinedLabel::Services => "Serviços",
            PredefinedLabel::Hide => "Ocultar Clovy",
            PredefinedLabel::HideOthers => "Ocultar outros",
            PredefinedLabel::ShowAll => "Mostrar tudo",
            PredefinedLabel::Quit => "Encerrar Clovy",
            PredefinedLabel::Undo => "Desfazer",
            PredefinedLabel::Redo => "Refazer",
            PredefinedLabel::Cut => "Recortar",
            PredefinedLabel::Copy => "Copiar",
            PredefinedLabel::Paste => "Colar",
            PredefinedLabel::SelectAll => "Selecionar tudo",
            PredefinedLabel::Fullscreen => "Tela cheia",
            PredefinedLabel::Minimize => "Minimizar",
            PredefinedLabel::Maximize => "Zoom",
        })
    }

    pub fn predefined_about(&self) -> Option<&'static str> {
        match self.locale {
            UiLocale::En => None,
            UiLocale::PtBr => Some("Sobre o Clovy"),
        }
    }

    // Menu bar tray.
    pub fn open_clovy(&self) -> &'static str {
        self.pick("Open Clovy", "Abrir o Clovy")
    }
    pub fn new_session(&self) -> &'static str {
        self.pick("New session...", "Nova sessão...")
    }
    pub fn hide_sessions_hud(&self) -> &'static str {
        self.pick("Hide sessions HUD", "Ocultar HUD de sessões")
    }
    pub fn show_sessions_hud(&self) -> &'static str {
        self.pick("Show sessions HUD", "Mostrar HUD de sessões")
    }
    pub fn quit_clovy(&self) -> &'static str {
        self.pick("Quit Clovy", "Encerrar Clovy")
    }
    pub fn untitled_session(&self) -> &'static str {
        self.pick("Untitled session", "Sessão sem título")
    }
    pub fn no_active_sessions(&self) -> &'static str {
        self.pick("No active sessions", "Nenhuma sessão ativa")
    }
    pub fn session_prefix_needs_approval(&self) -> &'static str {
        self.pick("Needs Approval - ", "Precisa de aprovação - ")
    }
    pub fn session_prefix_working(&self) -> &'static str {
        self.pick("Working - ", "Trabalhando - ")
    }
    pub fn activity(&self, recording: bool, dictating: bool) -> &'static str {
        match (recording, dictating) {
            (true, true) => self.pick("Recording and dictating - ", "Gravando e ditando - "),
            (true, false) => self.pick("Recording - ", "Gravando - "),
            (false, true) => self.pick("Dictating - ", "Ditando - "),
            (false, false) => "",
        }
    }

    pub fn sessions(&self, count: usize) -> String {
        match (self.locale, count) {
            (UiLocale::En, 1) => "1 session".to_string(),
            (UiLocale::En, n) => format!("{n} sessions"),
            (UiLocale::PtBr, 1) => "1 sessão".to_string(),
            (UiLocale::PtBr, n) => format!("{n} sessões"),
        }
    }

    pub fn needs_approval(&self, count: usize) -> &'static str {
        match (self.locale, count) {
            (UiLocale::En, 1) => "needs approval",
            (UiLocale::En, _) => "need approval",
            (UiLocale::PtBr, 1) => "precisa de aprovação",
            (UiLocale::PtBr, _) => "precisam de aprovação",
        }
    }

    pub fn working(&self, count: usize) -> String {
        let sessions = self.sessions(count);
        match self.locale {
            UiLocale::En => format!("{sessions} working"),
            UiLocale::PtBr => format!("{sessions} trabalhando"),
        }
    }

    pub fn last(&self, detail: &str) -> String {
        match self.locale {
            UiLocale::En => format!("Last: {detail}"),
            UiLocale::PtBr => format!("Última: {detail}"),
        }
    }

    pub fn readable_status(&self, status: &str) -> &'static str {
        match status {
            "received" => self.pick("Received", "Recebida"),
            "starting" => self.pick("Starting", "Iniciando"),
            "running" => self.pick("Working", "Trabalhando"),
            "waitingForUser" => self.pick("Needs Approval", "Precisa de aprovação"),
            "completed" => self.pick("Completed", "Concluída"),
            "failed" => self.pick("Failed", "Falhou"),
            "cancelled" => self.pick("Cancelled", "Cancelada"),
            _ => self.pick("Updated", "Atualizada"),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum PredefinedLabel {
    Services,
    Hide,
    HideOthers,
    ShowAll,
    Quit,
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    SelectAll,
    Fullscreen,
    Minimize,
    Maximize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_payloads_and_falls_back_to_english() {
        assert_eq!(UiLocale::from_payload("\"pt-BR\""), UiLocale::PtBr);
        assert_eq!(UiLocale::from_payload("pt_br"), UiLocale::PtBr);
        assert_eq!(UiLocale::from_payload("\"en\""), UiLocale::En);
        assert_eq!(UiLocale::from_payload("\"fr\""), UiLocale::En);
        assert_eq!(UiLocale::from_payload("not json"), UiLocale::En);
        assert_eq!(UiLocale::from_payload(""), UiLocale::En);
    }

    #[test]
    fn english_keeps_platform_defaults_for_predefined_items() {
        let en = strings(UiLocale::En);
        assert_eq!(en.predefined(PredefinedLabel::Quit), None);
        assert_eq!(en.settings(), "Settings...");
        let pt = strings(UiLocale::PtBr);
        assert_eq!(pt.predefined(PredefinedLabel::Quit), Some("Encerrar Clovy"));
        assert_eq!(pt.settings(), "Configurações...");
    }

    #[test]
    fn pluralizes_session_counts() {
        let pt = strings(UiLocale::PtBr);
        assert_eq!(pt.sessions(1), "1 sessão");
        assert_eq!(pt.sessions(3), "3 sessões");
        assert_eq!(pt.needs_approval(2), "precisam de aprovação");
        let en = strings(UiLocale::En);
        assert_eq!(en.working(2), "2 sessions working");
    }
}
