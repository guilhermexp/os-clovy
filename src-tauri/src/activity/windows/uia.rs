//! Windows UI Automation (UIA) tree inspection, browser URL extraction,
//! and private window detection (D3).

use std::time::{Duration, Instant};

use crate::activity::filter::{has_private_window_marker, normalize_browser_url};
use crate::activity::platform::WindowDescriptor;

/// Description extracted from a window via UI Automation (URL and privacy state).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct UiaDescription {
    pub url: Option<String>,
    pub private: bool,
}

/// Abstract control types relevant for UI Automation extraction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UiaControlType {
    Window,
    Pane,
    Document,
    Edit,
    Text,
    Button,
    Toolbar,
    Other,
}

/// Minimal abstraction over a UI Automation element node.
///
/// Implemented by `ComUiaNode` on Windows (using lazy `IUIAutomationTreeWalker` navigation)
/// and by `FakeUiaNode` in unit tests. Traversal never materializes the whole tree in advance,
/// protecting password privacy, performance, and avoiding COM stalls.
pub(crate) trait UiaNode: Sized {
    fn control_type(&self) -> UiaControlType;
    fn is_password(&self) -> bool;
    fn name(&self) -> Option<String>;
    fn value(&self) -> Option<String>;
    fn text_content(&self, max_chars: usize) -> Option<String>;
    fn automation_id(&self) -> Option<String>;
    fn first_child(&self) -> Option<Self>;
    fn next_sibling(&self) -> Option<Self>;
}

/// The UI Automation adapter for the Windows capture thread.
pub(crate) struct Uia {
    #[cfg(target_os = "windows")]
    _automation: Option<()>, // Real IUIAutomation2 handle in 3.2 GREEN
}

impl Uia {
    /// Creates a new UIA instance on the capture thread.
    pub(crate) fn new() -> Option<Self> {
        // Stub implementation for 1.5; real COM MTA init in 3.2 GREEN
        Some(Self {
            #[cfg(target_os = "windows")]
            _automation: None,
        })
    }

    /// Enriches a browser window descriptor by extracting the active URL and
    /// detecting private/incognito browsing mode.
    pub(crate) fn describe(&mut self, window: &WindowDescriptor) -> UiaDescription {
        #[cfg(target_os = "windows")]
        {
            // Real Win32 ElementFromHandle + ComUiaNode in 3.2 GREEN
            let _ = window;
        }
        UiaDescription::default()
    }

    /// Extracts visible text from the window's UIA tree within budget constraints.
    pub(crate) fn text(&mut self, window: &WindowDescriptor) -> Option<String> {
        #[cfg(target_os = "windows")]
        {
            // Real Win32 ElementFromHandle + ComUiaNode in 3.2 GREEN
            let _ = window;
        }
        None
    }
}

/// Identifies whether a control is a browser address/location input field.
///
/// Requires control type `Edit` and matching name/automation_id.
/// Never matches `Document` text or arbitrary webpage form inputs.
pub(crate) fn is_address_bar_control<N: UiaNode>(_node: &N) -> bool {
    // Stub for RED state; reimplemented from scratch in 3.2 GREEN
    false
}

// ---------------------------------------------------------------------------
// Traversal algorithms over UiaNode
// ---------------------------------------------------------------------------

/// Extracts URL and privacy status from an abstract UIA tree node.
///
/// Budgets: 250ms deadline, 200 nodes, depth 8.
/// Rules:
/// - Skips password elements and their subtrees entirely (lazy first_child omission).
/// - Checks for private badges in toolbar / profile.
/// - Address URL must come from an address Edit control with ValuePattern/value.
/// - NEVER interprets Document control text as browser address URL.
/// - If private is detected, URL is cleared (`None`).
pub(crate) fn extract_uia_description<N: UiaNode>(
    _root: &N,
    _title: Option<&str>,
) -> UiaDescription {
    // Stub implementation for 1.5 / 3.1 RED; real algorithm in 3.2 GREEN
    UiaDescription::default()
}

/// Extracts visible text from an abstract UIA tree node within budgets.
///
/// Budgets: 250ms deadline, 3,000 nodes, depth 40, 20,000 characters.
/// Rules:
/// - Skips password elements and their subtrees entirely.
/// - Skips window chrome (buttons, menus, toolbars, scrollbars).
/// - Reads Document, Edit, and Text controls.
/// - Deduplicates adjacent chunks and caps at 20,000 characters.
pub(crate) fn extract_uia_text<N: UiaNode>(_root: &N) -> Option<String> {
    // Stub implementation for 1.5 / 3.1 RED; real algorithm in 3.2 GREEN
    None
}

// ---------------------------------------------------------------------------
// Test utilities and behavioral tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    /// Node representation for test trees, implementing lazy `UiaNode` traversal.
    #[derive(Clone)]
    struct FakeUiaNode {
        inner: Arc<FakeNodeInner>,
    }

    struct FakeNodeInner {
        control_type: UiaControlType,
        is_password: bool,
        name: Option<String>,
        value: Option<String>,
        text: Option<String>,
        automation_id: Option<String>,
        delay: Option<Duration>,
        first_child: Option<FakeUiaNode>,
        next_sibling: Option<FakeUiaNode>,
    }

    impl FakeUiaNode {
        fn leaf(
            control_type: UiaControlType,
            name: Option<&str>,
            value: Option<&str>,
            text: Option<&str>,
            automation_id: Option<&str>,
            is_password: bool,
            delay: Option<Duration>,
        ) -> Self {
            Self {
                inner: Arc::new(FakeNodeInner {
                    control_type,
                    is_password,
                    name: name.map(String::from),
                    value: value.map(String::from),
                    text: text.map(String::from),
                    automation_id: automation_id.map(String::from),
                    delay,
                    first_child: None,
                    next_sibling: None,
                }),
            }
        }

        fn with_children(
            control_type: UiaControlType,
            name: Option<&str>,
            is_password: bool,
            children: Vec<FakeUiaNode>,
        ) -> Self {
            let mut linked_children: Vec<FakeUiaNode> = Vec::new();
            // Link next_sibling backwards
            let mut next: Option<FakeUiaNode> = None;
            for child in children.into_iter().rev() {
                let mut node = child;
                if let Some(nxt) = next {
                    // Update next_sibling
                    let mut inner_clone = (*node.inner).clone_fields();
                    inner_clone.next_sibling = Some(nxt);
                    node = FakeUiaNode {
                        inner: Arc::new(inner_clone),
                    };
                }
                next = Some(node.clone());
                linked_children.push(node);
            }
            linked_children.reverse();

            let first_child = linked_children.into_iter().next();

            Self {
                inner: Arc::new(FakeNodeInner {
                    control_type,
                    is_password,
                    name: name.map(String::from),
                    value: None,
                    text: None,
                    automation_id: None,
                    delay: None,
                    first_child,
                    next_sibling: None,
                }),
            }
        }
    }

    impl FakeNodeInner {
        fn clone_fields(&self) -> Self {
            Self {
                control_type: self.control_type,
                is_password: self.is_password,
                name: self.name.clone(),
                value: self.value.clone(),
                text: self.text.clone(),
                automation_id: self.automation_id.clone(),
                delay: self.delay,
                first_child: self.first_child.clone(),
                next_sibling: self.next_sibling.clone(),
            }
        }
    }

    impl UiaNode for FakeUiaNode {
        fn control_type(&self) -> UiaControlType {
            if let Some(d) = self.inner.delay {
                std::thread::sleep(d);
            }
            self.inner.control_type
        }

        fn is_password(&self) -> bool {
            self.inner.is_password
        }

        fn name(&self) -> Option<String> {
            self.inner.name.clone()
        }

        fn value(&self) -> Option<String> {
            self.inner.value.clone()
        }

        fn text_content(&self, _max_chars: usize) -> Option<String> {
            self.inner.text.clone()
        }

        fn automation_id(&self) -> Option<String> {
            self.inner.automation_id.clone()
        }

        fn first_child(&self) -> Option<Self> {
            self.inner.first_child.clone()
        }

        fn next_sibling(&self) -> Option<Self> {
            self.inner.next_sibling.clone()
        }
    }

    #[test]
    fn test_uia_fake_tree_extracts_url_from_address_edit_control() {
        let address_bar = FakeUiaNode::leaf(
            UiaControlType::Edit,
            Some("Address and search bar"),
            Some("https://example.com/docs"),
            None,
            Some("view_1020"),
            false,
            None,
        );
        let toolbar = FakeUiaNode::with_children(
            UiaControlType::Toolbar,
            Some("Navigation"),
            false,
            vec![address_bar],
        );
        let document = FakeUiaNode::leaf(
            UiaControlType::Document,
            Some("Web content"),
            None,
            Some("Some document content"),
            None,
            false,
            None,
        );
        let tree = FakeUiaNode::with_children(
            UiaControlType::Window,
            Some("Example - Microsoft Edge"),
            false,
            vec![toolbar, document],
        );

        let desc = extract_uia_description(&tree, Some("Example - Microsoft Edge"));
        assert_eq!(desc.url.as_deref(), Some("https://example.com/docs"));
        assert!(!desc.private);
    }

    #[test]
    fn test_uia_fake_tree_never_extracts_url_from_document_text() {
        let document = FakeUiaNode::leaf(
            UiaControlType::Document,
            Some("Web content"),
            None,
            Some("Visit https://phishing-site.test for more info"),
            None,
            false,
            None,
        );
        let tree = FakeUiaNode::with_children(
            UiaControlType::Window,
            Some("Page - Google Chrome"),
            false,
            vec![document],
        );

        let desc = extract_uia_description(&tree, Some("Page - Google Chrome"));
        // Document content MUST NOT be extracted as browser URL!
        assert_eq!(desc.url, None);
    }

    #[test]
    fn test_uia_fake_tree_ignores_generic_page_edit_fields_containing_url() {
        // A web form textarea or input on a page that happens to contain a URL
        let form_input = FakeUiaNode::leaf(
            UiaControlType::Edit,
            Some("User Comment"),
            Some("https://user-comment-link.test"),
            None,
            Some("comment_box"),
            false,
            None,
        );
        let document = FakeUiaNode::with_children(
            UiaControlType::Document,
            Some("Web content"),
            false,
            vec![form_input],
        );
        let tree = FakeUiaNode::with_children(
            UiaControlType::Window,
            Some("Blog - Edge"),
            false,
            vec![document],
        );

        let desc = extract_uia_description(&tree, Some("Blog - Edge"));
        assert_eq!(desc.url, None, "Generic page edit fields must not be treated as address bar");
    }

    #[test]
    fn test_uia_fake_tree_detects_inprivate_badge_and_clears_url() {
        let inprivate_badge = FakeUiaNode::leaf(
            UiaControlType::Button,
            Some("InPrivate"),
            None,
            None,
            None,
            false,
            None,
        );
        let address_bar = FakeUiaNode::leaf(
            UiaControlType::Edit,
            Some("Address"),
            Some("https://sensitive-bank.test"),
            None,
            Some("addressEdit"),
            false,
            None,
        );
        let toolbar = FakeUiaNode::with_children(
            UiaControlType::Toolbar,
            Some("Toolbar"),
            false,
            vec![inprivate_badge, address_bar],
        );
        let tree = FakeUiaNode::with_children(
            UiaControlType::Window,
            Some("Edge"),
            false,
            vec![toolbar],
        );

        let desc = extract_uia_description(&tree, Some("Edge"));
        assert!(desc.private, "InPrivate badge must mark private window");
        assert_eq!(desc.url, None, "Private window URL must be cleared");
    }

    #[test]
    fn test_uia_fake_tree_extracts_visible_content_from_document_and_edit_controls() {
        let doc = FakeUiaNode::leaf(
            UiaControlType::Document,
            Some("Article"),
            None,
            Some("This is the main article content."),
            None,
            false,
            None,
        );
        let edit = FakeUiaNode::leaf(
            UiaControlType::Edit,
            Some("Notes"),
            Some("User draft notes"),
            None,
            None,
            false,
            None,
        );
        let tree = FakeUiaNode::with_children(
            UiaControlType::Window,
            Some("Editor"),
            false,
            vec![doc, edit],
        );

        let text = extract_uia_text(&tree).expect("visible text must be extracted");
        assert!(text.contains("This is the main article content."));
        assert!(text.contains("User draft notes"));
    }

    #[test]
    fn test_uia_fake_tree_skips_password_subtree_entirely() {
        let password_child = FakeUiaNode::leaf(
            UiaControlType::Text,
            None,
            None,
            Some("my_secret_pass_child"),
            None,
            false,
            None,
        );
        let password_field = FakeUiaNode::with_children(
            UiaControlType::Edit,
            Some("Password"),
            true, // is_password = true
            vec![password_child],
        );
        let username_field = FakeUiaNode::leaf(
            UiaControlType::Edit,
            Some("Username"),
            Some("alice"),
            None,
            None,
            false,
            None,
        );
        let form = FakeUiaNode::with_children(
            UiaControlType::Pane,
            Some("Login form"),
            false,
            vec![username_field, password_field],
        );
        let tree = FakeUiaNode::with_children(
            UiaControlType::Window,
            Some("Login"),
            false,
            vec![form],
        );

        let text = extract_uia_text(&tree).expect("visible non-password text must be extracted");
        assert!(text.contains("alice"), "public username must be extracted");
        assert!(!text.contains("Password"), "password field label/value must not be extracted");
        assert!(!text.contains("my_secret_pass_child"), "password subtree must be skipped");
    }

    #[test]
    fn test_uia_fake_tree_respects_node_budget() {
        let head = FakeUiaNode::leaf(
            UiaControlType::Text,
            None,
            None,
            Some("head_marker_public"),
            None,
            false,
            None,
        );
        // Thousands of empty / non-text group nodes (Pane) so that the 20k character cap
        // is NOT accidentally satisfied before 3000 nodes are traversed!
        let mut empty_panes = Vec::new();
        for _ in 0..3200 {
            empty_panes.push(FakeUiaNode::leaf(
                UiaControlType::Pane,
                None,
                None,
                None,
                None,
                false,
                None,
            ));
        }
        let tail = FakeUiaNode::leaf(
            UiaControlType::Text,
            None,
            None,
            Some("tail_sentinel_beyond_budget"),
            None,
            false,
            None,
        );

        let mut children = vec![head];
        children.extend(empty_panes);
        children.push(tail);

        let tree = FakeUiaNode::with_children(
            UiaControlType::Document,
            Some("Doc"),
            false,
            children,
        );

        let text = extract_uia_text(&tree).expect("head text must be extracted");
        assert!(text.contains("head_marker_public"), "head marker must be read within budget");
        assert!(
            !text.contains("tail_sentinel_beyond_budget"),
            "tail sentinel beyond 3000 nodes budget must not be processed"
        );
    }

    #[test]
    fn test_uia_fake_tree_respects_deadline_budget() {
        let head = FakeUiaNode::leaf(
            UiaControlType::Text,
            None,
            None,
            Some("fast_head_marker"),
            None,
            false,
            None,
        );
        let slow_node = FakeUiaNode::leaf(
            UiaControlType::Pane,
            Some("Slow"),
            None,
            None,
            None,
            false,
            Some(Duration::from_millis(300)),
        );
        let unreachable_tail = FakeUiaNode::leaf(
            UiaControlType::Text,
            None,
            None,
            Some("unreachable_tail_sentinel"),
            None,
            false,
            None,
        );
        let tree = FakeUiaNode::with_children(
            UiaControlType::Window,
            Some("Window"),
            false,
            vec![head, slow_node, unreachable_tail],
        );

        let start = Instant::now();
        let text = extract_uia_text(&tree).expect("fast head text must be extracted before deadline");
        let elapsed = start.elapsed();
        assert!(text.contains("fast_head_marker"), "fast head text must be extracted");
        assert!(
            !text.contains("unreachable_tail_sentinel"),
            "tail sentinel after deadline must not be reached"
        );
        assert!(elapsed < Duration::from_millis(700), "deadline must bound traversal");
    }

    #[test]
    fn test_uia_fake_tree_ignores_inpage_spoofed_address_and_private_markers_inside_document() {
        let spoofed_address = FakeUiaNode::leaf(
            UiaControlType::Edit,
            Some("Address and search bar"),
            Some("https://attacker.invalid"),
            None,
            Some("addressEdit"),
            false,
            None,
        );
        let inpage_private_text = FakeUiaNode::leaf(
            UiaControlType::Text,
            Some("InPrivate"),
            None,
            Some("This page mentions InPrivate browsing"),
            None,
            false,
            None,
        );
        let document = FakeUiaNode::with_children(
            UiaControlType::Document,
            Some("Web page content"),
            false,
            vec![spoofed_address, inpage_private_text],
        );

        let real_omnibox = FakeUiaNode::leaf(
            UiaControlType::Edit,
            Some("Address and search bar"),
            Some("https://example.org"),
            None,
            Some("addressEdit"),
            false,
            None,
        );
        let toolbar = FakeUiaNode::with_children(
            UiaControlType::Toolbar,
            Some("Browser chrome"),
            false,
            vec![real_omnibox],
        );

        let window = FakeUiaNode::with_children(
            UiaControlType::Window,
            Some("Edge"),
            false,
            vec![toolbar, document],
        );

        let desc = extract_uia_description(&window, Some("Edge"));
        assert_eq!(desc.url.as_deref(), Some("https://example.org"));
        assert!(!desc.private, "In-page text inside Document must not trigger private mode");
    }
}
