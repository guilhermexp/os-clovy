//! The one path that shows Clovy's main window, and what closing it does.

/// What closing the main window does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloseAction {
    /// Hide it; the app keeps running in the tray / menu bar.
    Hide,
    /// Let the window be destroyed.
    Close,
}

/// What a close request on the main window does on this system.
pub fn close_action() -> CloseAction {
    if cfg!(target_os = "macos") {
        CloseAction::Hide
    } else {
        CloseAction::Close
    }
}

/// Where the main window lives; tests substitute it.
pub trait MainWindowSlot {
    type Window;
    fn existing(&self) -> Option<Self::Window>;
    /// Builds the window again from the app configuration.
    fn create(&self) -> Option<Self::Window>;
    /// Shows, restores, and focuses it.
    fn reveal(&self, window: &Self::Window);
}

/// Shows the main window. Returns whether a window was revealed.
pub fn show_with<S: MainWindowSlot>(slot: &S) -> bool {
    match slot.existing() {
        Some(window) => {
            slot.reveal(&window);
            true
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[derive(Default)]
    struct FakeSlot {
        window: RefCell<Option<u32>>,
        created: RefCell<u32>,
        revealed: RefCell<Vec<u32>>,
    }

    impl MainWindowSlot for FakeSlot {
        type Window = u32;
        fn existing(&self) -> Option<u32> {
            *self.window.borrow()
        }
        fn create(&self) -> Option<u32> {
            *self.created.borrow_mut() += 1;
            *self.window.borrow_mut() = Some(7);
            Some(7)
        }
        fn reveal(&self, window: &u32) {
            self.revealed.borrow_mut().push(*window);
        }
    }

    #[test]
    fn an_existing_window_is_revealed_not_rebuilt() {
        let slot = FakeSlot {
            window: RefCell::new(Some(3)),
            ..FakeSlot::default()
        };
        assert!(show_with(&slot));
        assert_eq!(*slot.created.borrow(), 0);
        assert_eq!(*slot.revealed.borrow(), [3]);
    }

    /// The tray, "Open Clovy", a second launch, notifications, and HUDs all
    /// end here; a window that no longer exists comes back.
    #[test]
    fn a_destroyed_window_is_rebuilt_and_revealed() {
        let slot = FakeSlot::default();
        assert!(show_with(&slot));
        assert_eq!(*slot.created.borrow(), 1);
        assert_eq!(*slot.revealed.borrow(), [7]);
    }

    #[test]
    fn closing_hides_the_window_on_every_system() {
        assert_eq!(close_action(), CloseAction::Hide);
    }

    /// Every caller goes through `main_window::show`; no module keeps its own
    /// copy that silently does nothing when the window is gone.
    #[test]
    fn no_other_module_shows_the_main_window_on_its_own() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut offenders = Vec::new();
        let mut stack = vec![root.clone()];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    stack.push(path);
                } else if path.extension().is_some_and(|ext| ext == "rs")
                    && !path.ends_with("main_window.rs")
                {
                    let source = std::fs::read_to_string(&path).unwrap();
                    if source.contains("fn show_main_window(") {
                        offenders.push(path.strip_prefix(&root).unwrap().display().to_string());
                    }
                }
            }
        }
        offenders.sort();
        assert!(offenders.is_empty(), "use main_window::show: {offenders:?}");
    }
}
