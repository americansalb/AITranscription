//! What the screen looks like to us, on every platform, in one shape.
//!
//! The platform crate fills this in from the macOS Accessibility API or from
//! Windows UI Automation. Everything above the platform crate speaks only
//! this schema.
//!
//! Two rules are built in rather than remembered:
//! - An element marked `secure` never carries a value. `Element::with_value`
//!   drops the value, and the renderer and the sentence builder never print
//!   one.
//! - Coordinates are the operating system's own input coordinates with a
//!   top-left origin, so a point in a snapshot can be clicked as it is.

use serde::{Deserialize, Serialize};

/// The kind of thing an element is. Small on purpose: this is the vocabulary
/// the app speaks, not a mirror of either platform's role list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Application,
    Window,
    Dialog,
    Sheet,
    Toolbar,
    MenuBar,
    Menu,
    MenuItem,
    Group,
    ScrollArea,
    WebArea,
    Document,
    Heading,
    Text,
    Link,
    Image,
    Button,
    Checkbox,
    Radio,
    ComboBox,
    Slider,
    Spinner,
    Tab,
    TextField,
    TextArea,
    List,
    ListItem,
    Table,
    Row,
    Cell,
    Tree,
    TreeItem,
    ProgressIndicator,
    Unknown,
}

impl Role {
    /// The word a person hears for this role, or empty when the role adds
    /// nothing worth saying aloud.
    pub fn spoken(self) -> &'static str {
        match self {
            Role::Window => "window",
            Role::Dialog => "dialog",
            Role::Sheet => "sheet",
            Role::Toolbar => "toolbar",
            Role::Menu => "menu",
            Role::MenuItem => "menu item",
            Role::Heading => "heading",
            Role::Link => "link",
            Role::Image => "image",
            Role::Button => "button",
            Role::Checkbox => "checkbox",
            Role::Radio => "radio button",
            Role::ComboBox => "combo box",
            Role::Slider => "slider",
            Role::Spinner => "spin button",
            Role::Tab => "tab",
            Role::TextField => "edit field",
            Role::TextArea => "text area",
            Role::Document => "document",
            Role::List => "list",
            Role::ListItem => "list item",
            Role::Table => "table",
            Role::Row => "row",
            Role::Cell => "cell",
            Role::Tree => "tree",
            Role::TreeItem => "tree item",
            Role::ProgressIndicator => "progress indicator",
            Role::Application
            | Role::MenuBar
            | Role::Group
            | Role::ScrollArea
            | Role::WebArea
            | Role::Text
            | Role::Unknown => "",
        }
    }

    /// The short label used in text meant for the model. Never empty.
    pub fn label(self) -> &'static str {
        match self {
            Role::Application => "application",
            Role::MenuBar => "menu bar",
            Role::Group => "group",
            Role::ScrollArea => "scroll area",
            Role::WebArea => "web area",
            Role::Text => "text",
            Role::Unknown => "element",
            other => other.spoken(),
        }
    }

    /// Something a person can click, press, type into, or choose.
    pub fn is_interactive(self) -> bool {
        matches!(
            self,
            Role::Link
                | Role::Button
                | Role::Checkbox
                | Role::Radio
                | Role::ComboBox
                | Role::Slider
                | Role::Spinner
                | Role::Tab
                | Role::TextField
                | Role::TextArea
                | Role::MenuItem
                | Role::ListItem
                | Role::TreeItem
        )
    }

    /// Something a person types into.
    pub fn is_text_entry(self) -> bool {
        matches!(self, Role::TextField | Role::TextArea | Role::ComboBox | Role::Document)
    }

    /// A container that carries no meaning of its own when it has no name.
    pub fn is_plain_container(self) -> bool {
        matches!(
            self,
            Role::Group | Role::ScrollArea | Role::Application | Role::Unknown | Role::WebArea
        )
    }

    /// Map a macOS Accessibility role and subrole. The subrole wins when it
    /// says something more specific, which is how tabs, switches, and
    /// dialogs are told apart on macOS.
    pub fn from_ax(role: &str, subrole: &str) -> Role {
        match subrole {
            "AXTabButton" => return Role::Tab,
            "AXSwitch" | "AXToggle" => return Role::Checkbox,
            "AXDialog" | "AXSystemDialog" => return Role::Dialog,
            "AXSearchField" => return Role::TextField,
            "AXOutlineRow" => return Role::TreeItem,
            "AXCloseButton" | "AXMinimizeButton" | "AXZoomButton" | "AXFullScreenButton" => {
                return Role::Button
            }
            _ => {}
        }
        match role {
            "AXApplication" => Role::Application,
            "AXWindow" => Role::Window,
            "AXSheet" => Role::Sheet,
            "AXDrawer" => Role::Group,
            "AXToolbar" => Role::Toolbar,
            "AXMenuBar" => Role::MenuBar,
            "AXMenu" => Role::Menu,
            "AXMenuBarItem" | "AXMenuItem" => Role::MenuItem,
            "AXGroup" | "AXSplitGroup" | "AXLayoutArea" | "AXLayoutItem" | "AXTabGroup"
            | "AXRadioGroup" | "AXGenericElement" => Role::Group,
            "AXScrollArea" => Role::ScrollArea,
            "AXWebArea" => Role::WebArea,
            "AXTextArea" => Role::TextArea,
            "AXHeading" => Role::Heading,
            "AXStaticText" => Role::Text,
            "AXLink" => Role::Link,
            "AXImage" => Role::Image,
            "AXButton" | "AXMenuButton" | "AXDisclosureTriangle" | "AXDockItem" => Role::Button,
            "AXCheckBox" => Role::Checkbox,
            "AXRadioButton" => Role::Radio,
            "AXComboBox" | "AXPopUpButton" => Role::ComboBox,
            "AXSlider" => Role::Slider,
            "AXIncrementor" => Role::Spinner,
            "AXTextField" | "AXSecureTextField" => Role::TextField,
            "AXList" => Role::List,
            "AXOutline" | "AXBrowser" => Role::Tree,
            "AXTable" | "AXGrid" => Role::Table,
            "AXRow" => Role::Row,
            "AXCell" => Role::Cell,
            "AXProgressIndicator" | "AXBusyIndicator" => Role::ProgressIndicator,
            _ => Role::Unknown,
        }
    }

    /// Map a Windows UI Automation control type id.
    pub fn from_uia(control_type_id: i32) -> Role {
        match control_type_id {
            50000 => Role::Button,
            50001 => Role::Group, // Calendar
            50002 => Role::Checkbox,
            50003 => Role::ComboBox,
            50004 => Role::TextField, // Edit
            50005 => Role::Link,
            50006 => Role::Image,
            50007 => Role::ListItem,
            50008 => Role::List,
            50009 => Role::Menu,
            50010 => Role::MenuBar,
            50011 => Role::MenuItem,
            50012 => Role::ProgressIndicator,
            50013 => Role::Radio,
            50014 => Role::Unknown, // ScrollBar
            50015 => Role::Slider,
            50016 => Role::Spinner,
            50017 => Role::Group, // StatusBar
            50018 => Role::Group, // Tab control, the container
            50019 => Role::Tab,   // TabItem
            50020 => Role::Text,
            50021 => Role::Toolbar,
            50022 => Role::Text, // ToolTip
            50023 => Role::Tree,
            50024 => Role::TreeItem,
            50025 => Role::Unknown, // Custom
            50026 => Role::Group,
            50027 => Role::Unknown, // Thumb
            50028 => Role::Table,   // DataGrid
            50029 => Role::Row,     // DataItem
            50030 => Role::Document,
            50031 => Role::Button, // SplitButton
            50032 => Role::Window,
            50033 => Role::Group, // Pane
            50034 => Role::Group, // Header
            50035 => Role::Cell,  // HeaderItem
            50036 => Role::Table,
            50037 => Role::Group,   // TitleBar
            50038 => Role::Unknown, // Separator
            50039 => Role::Group,   // SemanticZoom
            50040 => Role::Toolbar, // AppBar
            _ => Role::Unknown,
        }
    }
}

/// A rectangle in the operating system's input coordinates, top-left origin.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub fn center(&self) -> (f64, f64) {
        (self.x + self.width / 2.0, self.y + self.height / 2.0)
    }

    pub fn is_visible(&self) -> bool {
        self.width > 0.0 && self.height > 0.0
    }
}

fn is_false(b: &bool) -> bool {
    !*b
}

fn is_true(b: &bool) -> bool {
    *b
}

fn yes() -> bool {
    true
}

/// One thing on the screen.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Element {
    /// Unique within one snapshot, in capture order. Stable enough for
    /// "click the second one" within a conversation turn.
    pub id: u32,
    pub role: Role,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// Never present on a secure element.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    /// A password or other secret field. Its value is never captured.
    #[serde(default, skip_serializing_if = "is_false")]
    pub secure: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub focused: bool,
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub selected: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bounds: Option<Rect>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<Element>,
}

impl Element {
    pub fn new(id: u32, role: Role, name: impl Into<String>) -> Element {
        Element {
            id,
            role,
            name: name.into(),
            value: None,
            secure: false,
            focused: false,
            enabled: true,
            selected: false,
            bounds: None,
            children: Vec::new(),
        }
    }

    /// Sets the value, unless the element is secure, in which case the value
    /// is dropped. Empty values are dropped too.
    pub fn with_value(mut self, value: impl Into<String>) -> Element {
        let value = value.into();
        self.value = if self.secure || value.is_empty() { None } else { Some(value) };
        self
    }

    /// Marks the element secure and drops any value it carried.
    pub fn secure(mut self) -> Element {
        self.secure = true;
        self.value = None;
        self
    }

    pub fn with_children(mut self, children: Vec<Element>) -> Element {
        self.children = children;
        self
    }

    /// The text a person would call this element: its name, or for text and
    /// text entry elements, its content.
    pub fn display_text(&self) -> &str {
        if !self.name.is_empty() {
            &self.name
        } else if let Some(v) = &self.value {
            v
        } else {
            ""
        }
    }

    /// This element and everything under it, depth first.
    pub fn walk<'a>(&'a self, depth: usize, f: &mut dyn FnMut(&'a Element, usize)) {
        f(self, depth);
        for child in &self.children {
            child.walk(depth + 1, f);
        }
    }

    pub fn count(&self) -> u32 {
        1 + self.children.iter().map(Element::count).sum::<u32>()
    }
}

/// The program that owns the window in front.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct App {
    pub name: String,
    pub pid: u32,
}

/// Limits on a capture. A snapshot that hits one is marked truncated rather
/// than taking forever, because silence is the worst thing this app can do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Limits {
    pub max_depth: u32,
    pub max_elements: u32,
    pub budget_ms: u64,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits { max_depth: 14, max_elements: 1500, budget_ms: 1500 }
    }
}

/// Everything captured about the window in front at one moment.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub platform: String,
    pub app: App,
    pub window_title: String,
    /// Pixels per coordinate unit, for mapping to and from screenshots.
    pub scale: f64,
    pub captured_at_ms: u64,
    pub elapsed_ms: u64,
    pub truncated: bool,
    pub element_count: u32,
    pub elements: Vec<Element>,
}

impl Snapshot {
    pub fn walk<'a>(&'a self, f: &mut dyn FnMut(&'a Element, usize)) {
        for element in &self.elements {
            element.walk(0, f);
        }
    }

    pub fn count_elements(elements: &[Element]) -> u32 {
        elements.iter().map(Element::count).sum()
    }

    /// True when a web view came back with nothing under it. Chromium and
    /// Electron only expose web content once they believe an assistive
    /// technology is present, and this is how the platform crate knows to
    /// raise its hand and capture again.
    pub fn needs_web_flags(elements: &[Element]) -> bool {
        let mut found = false;
        for element in elements {
            element.walk(0, &mut |e, _| {
                if e.role == Role::WebArea && e.children.is_empty() {
                    found = true;
                }
            });
        }
        found
    }

    /// The focused element, if any, depth first.
    pub fn focused(&self) -> Option<&Element> {
        let mut hit = None;
        self.walk(&mut |e, _| {
            if hit.is_none() && e.focused {
                hit = Some(e);
            }
        });
        hit
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_secure_element_never_carries_a_value() {
        let e = Element::new(1, Role::TextField, "Password").secure().with_value("hunter2");
        assert!(e.secure);
        assert_eq!(e.value, None);
        let e = Element::new(1, Role::TextField, "Password").with_value("hunter2").secure();
        assert_eq!(e.value, None);
    }

    #[test]
    fn empty_values_are_dropped() {
        assert_eq!(Element::new(1, Role::TextField, "Search").with_value("").value, None);
    }

    #[test]
    fn every_uia_control_type_maps() {
        let intended_unknown = [50014, 50025, 50027, 50038];
        for id in 50000..=50040 {
            let role = Role::from_uia(id);
            if intended_unknown.contains(&id) {
                assert_eq!(role, Role::Unknown, "{id}");
            } else {
                assert_ne!(role, Role::Unknown, "control type {id} is unmapped");
            }
        }
        assert_eq!(Role::from_uia(0), Role::Unknown);
        assert_eq!(Role::from_uia(-1), Role::Unknown);
    }

    #[test]
    fn ax_roles_map_and_subroles_win() {
        assert_eq!(Role::from_ax("AXButton", ""), Role::Button);
        assert_eq!(Role::from_ax("AXRadioButton", "AXTabButton"), Role::Tab);
        assert_eq!(Role::from_ax("AXCheckBox", "AXSwitch"), Role::Checkbox);
        assert_eq!(Role::from_ax("AXWindow", "AXDialog"), Role::Dialog);
        assert_eq!(Role::from_ax("AXTextField", "AXSearchField"), Role::TextField);
        assert_eq!(Role::from_ax("AXSecureTextField", ""), Role::TextField);
        assert_eq!(Role::from_ax("AXStaticText", ""), Role::Text);
        assert_eq!(Role::from_ax("AXWebArea", ""), Role::WebArea);
        assert_eq!(Role::from_ax("AXPopUpButton", ""), Role::ComboBox);
        assert_eq!(Role::from_ax("AXRow", "AXOutlineRow"), Role::TreeItem);
        assert_eq!(Role::from_ax("AXNonsense", "AXMoreNonsense"), Role::Unknown);
        assert_eq!(Role::from_ax("", ""), Role::Unknown);
    }

    #[test]
    fn the_same_thing_gets_the_same_role_on_both_platforms() {
        assert_eq!(Role::from_ax("AXButton", ""), Role::from_uia(50000));
        assert_eq!(Role::from_ax("AXCheckBox", ""), Role::from_uia(50002));
        assert_eq!(Role::from_ax("AXTextField", ""), Role::from_uia(50004));
        assert_eq!(Role::from_ax("AXLink", ""), Role::from_uia(50005));
        assert_eq!(Role::from_ax("AXRadioButton", "AXTabButton"), Role::from_uia(50019));
        assert_eq!(Role::from_ax("AXMenuItem", ""), Role::from_uia(50011));
        assert_eq!(Role::from_ax("AXOutline", ""), Role::from_uia(50023));
    }

    #[test]
    fn spoken_words_are_lowercase_and_labels_never_empty() {
        let all = [
            Role::Application, Role::Window, Role::Dialog, Role::Sheet, Role::Toolbar,
            Role::MenuBar, Role::Menu, Role::MenuItem, Role::Group, Role::ScrollArea,
            Role::WebArea, Role::Document, Role::Heading, Role::Text, Role::Link, Role::Image,
            Role::Button, Role::Checkbox, Role::Radio, Role::ComboBox, Role::Slider,
            Role::Spinner, Role::Tab, Role::TextField, Role::TextArea, Role::List,
            Role::ListItem, Role::Table, Role::Row, Role::Cell, Role::Tree, Role::TreeItem,
            Role::ProgressIndicator, Role::Unknown,
        ];
        for role in all {
            assert_eq!(role.spoken(), role.spoken().to_lowercase());
            assert!(!role.label().is_empty(), "{role:?} has no label");
        }
    }

    #[test]
    fn rect_center_and_visibility() {
        let r = Rect { x: 10.0, y: 20.0, width: 100.0, height: 40.0 };
        assert_eq!(r.center(), (60.0, 40.0));
        assert!(r.is_visible());
        assert!(!Rect { x: 0.0, y: 0.0, width: 0.0, height: 5.0 }.is_visible());
    }

    #[test]
    fn needs_web_flags_only_for_an_empty_web_area() {
        let empty = vec![Element::new(1, Role::WebArea, "")];
        assert!(Snapshot::needs_web_flags(&empty));
        let full = vec![Element::new(1, Role::WebArea, "")
            .with_children(vec![Element::new(2, Role::Link, "Home")])];
        assert!(!Snapshot::needs_web_flags(&full));
        let none = vec![Element::new(1, Role::Group, "")];
        assert!(!Snapshot::needs_web_flags(&none));
    }

    #[test]
    fn walk_counts_and_finds_focus() {
        let tree = vec![Element::new(1, Role::Group, "").with_children(vec![
            Element::new(2, Role::Button, "Save"),
            Element {
                focused: true,
                ..Element::new(3, Role::TextField, "Subject")
            },
        ])];
        assert_eq!(Snapshot::count_elements(&tree), 3);
        let snap = Snapshot {
            platform: "test".into(),
            app: App { name: "Mail".into(), pid: 1 },
            window_title: "New Message".into(),
            scale: 2.0,
            captured_at_ms: 0,
            elapsed_ms: 0,
            truncated: false,
            element_count: 3,
            elements: tree,
        };
        assert_eq!(snap.focused().map(|e| e.id), Some(3));
    }
}
