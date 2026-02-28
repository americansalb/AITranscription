//! Platform-agnostic accessibility tree types.
//!
//! This module defines the normalized schema that both Windows (UIA) and macOS (AX API)
//! implementations emit. Claude consumes this format for screen descriptions — it MUST
//! be structurally identical regardless of platform.
//!
//! # Coordinate Convention
//! All bounds use **top-left screen origin** (0,0 at top-left of primary monitor).
//! The macOS implementation MUST flip coordinates: `y = screen_height - ax_y - height`
//! because macOS AX API uses bottom-left origin (Cocoa coordinate system).

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Core tree types
// ---------------------------------------------------------------------------

/// The complete accessibility tree captured from the foreground window.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NormalizedTree {
    /// Title of the foreground window.
    pub window_title: String,
    /// Executable name of the owning process (e.g. "notepad.exe", "Safari").
    pub process_name: String,
    /// Platform that produced this tree: "windows", "macos", or "linux".
    pub platform: String,
    /// Total number of elements captured (may be less than tree size due to depth/count limits).
    pub element_count: usize,
    /// Top-level elements (children of the root window element).
    pub elements: Vec<NormalizedElement>,
}

/// A single UI element in the accessibility tree.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NormalizedElement {
    /// Monotonically increasing ID within this tree capture (starts at 1).
    pub id: u64,
    /// Display name / label of the element (e.g. "Submit", "Username", "File").
    pub name: String,
    /// Platform-agnostic role classification.
    pub role: NormalizedRole,
    /// Current value — text content for inputs, toggle state for checkboxes, selected item for lists.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    /// Bounding rectangle in screen coordinates (top-left origin).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bounds: Option<Rect>,
    /// Active states on this element.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub states: Vec<ElementState>,
    /// Keyboard accelerator (e.g. "Ctrl+S", "Cmd+C").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shortcut: Option<String>,
    /// Depth in the element tree (0 = direct child of window root).
    pub depth: u32,
    /// Number of direct children.
    pub children_count: u32,
    /// Child elements.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<NormalizedElement>,
}

/// Screen-coordinate rectangle. Origin is ALWAYS top-left of the screen.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

// ---------------------------------------------------------------------------
// Role enumeration — platform-agnostic
// ---------------------------------------------------------------------------

/// Normalized role that both Windows UIA and macOS AX map into.
///
/// The mapping tables are defined in [`uia_control_type_to_role`] (Windows)
/// and [`ax_role_to_normalized`] (macOS). Roles that don't have a direct
/// mapping use `Unknown(String)` to preserve the platform-specific name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NormalizedRole {
    // -- Containers --
    Window,
    Dialog,
    Pane,
    Group,
    Toolbar,
    StatusBar,
    MenuBar,
    TitleBar,

    // -- Navigation --
    Tab,
    TabItem,
    Menu,
    MenuItem,
    Link,

    // -- Input controls --
    Button,
    SplitButton,
    TextInput,
    TextArea,
    Checkbox,
    RadioButton,
    ComboBox,
    Slider,
    Spinner,
    ScrollBar,

    // -- Data display --
    List,
    ListItem,
    TreeView,
    TreeItem,
    Table,
    TableRow,
    TableCell,
    DataGrid,
    DataItem,
    Header,
    HeaderItem,

    // -- Content --
    Text,
    Heading,
    Label,
    Image,
    Document,
    ProgressBar,
    Tooltip,
    Separator,

    // -- Calendar / date --
    Calendar,

    // -- Platform-specific (preserved) --
    Thumb,
    SemanticZoom,
    AppBar,
    Custom,

    // -- Catch-all for unmapped roles --
    /// Preserves the original platform-specific role string when no mapping exists.
    Unknown(String),
}

// Custom Serialize: all variants serialize as a plain string.
// Known variants use PascalCase; Unknown serializes the inner string directly.
impl Serialize for NormalizedRole {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

// Custom Deserialize: parse a string, match known variants first, fall back to Unknown.
// This eliminates the ambiguity that #[serde(untagged)] caused — "Button" ALWAYS
// deserializes to NormalizedRole::Button, never to Unknown("Button").
impl<'de> Deserialize<'de> for NormalizedRole {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Ok(normalized_role_from_str(&s))
    }
}

/// Parse a string into a NormalizedRole, matching known variants exactly.
/// Unknown strings are wrapped in `NormalizedRole::Unknown`.
pub fn normalized_role_from_str(s: &str) -> NormalizedRole {
    match s {
        "Window" => NormalizedRole::Window,
        "Dialog" => NormalizedRole::Dialog,
        "Pane" => NormalizedRole::Pane,
        "Group" => NormalizedRole::Group,
        "Toolbar" => NormalizedRole::Toolbar,
        "StatusBar" => NormalizedRole::StatusBar,
        "MenuBar" => NormalizedRole::MenuBar,
        "TitleBar" => NormalizedRole::TitleBar,
        "Tab" => NormalizedRole::Tab,
        "TabItem" => NormalizedRole::TabItem,
        "Menu" => NormalizedRole::Menu,
        "MenuItem" => NormalizedRole::MenuItem,
        "Link" => NormalizedRole::Link,
        "Button" => NormalizedRole::Button,
        "SplitButton" => NormalizedRole::SplitButton,
        "TextInput" => NormalizedRole::TextInput,
        "TextArea" => NormalizedRole::TextArea,
        "Checkbox" => NormalizedRole::Checkbox,
        "RadioButton" => NormalizedRole::RadioButton,
        "ComboBox" => NormalizedRole::ComboBox,
        "Slider" => NormalizedRole::Slider,
        "Spinner" => NormalizedRole::Spinner,
        "ScrollBar" => NormalizedRole::ScrollBar,
        "List" => NormalizedRole::List,
        "ListItem" => NormalizedRole::ListItem,
        "TreeView" => NormalizedRole::TreeView,
        "TreeItem" => NormalizedRole::TreeItem,
        "Table" => NormalizedRole::Table,
        "TableRow" => NormalizedRole::TableRow,
        "TableCell" => NormalizedRole::TableCell,
        "DataGrid" => NormalizedRole::DataGrid,
        "DataItem" => NormalizedRole::DataItem,
        "Header" => NormalizedRole::Header,
        "HeaderItem" => NormalizedRole::HeaderItem,
        "Text" => NormalizedRole::Text,
        "Heading" => NormalizedRole::Heading,
        "Label" => NormalizedRole::Label,
        "Image" => NormalizedRole::Image,
        "Document" => NormalizedRole::Document,
        "ProgressBar" => NormalizedRole::ProgressBar,
        "Tooltip" => NormalizedRole::Tooltip,
        "Separator" => NormalizedRole::Separator,
        "Calendar" => NormalizedRole::Calendar,
        "Thumb" => NormalizedRole::Thumb,
        "SemanticZoom" => NormalizedRole::SemanticZoom,
        "AppBar" => NormalizedRole::AppBar,
        "Custom" => NormalizedRole::Custom,
        other => NormalizedRole::Unknown(other.to_string()),
    }
}

impl NormalizedRole {
    /// Returns the string representation of this role.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Window => "Window",
            Self::Dialog => "Dialog",
            Self::Pane => "Pane",
            Self::Group => "Group",
            Self::Toolbar => "Toolbar",
            Self::StatusBar => "StatusBar",
            Self::MenuBar => "MenuBar",
            Self::TitleBar => "TitleBar",
            Self::Tab => "Tab",
            Self::TabItem => "TabItem",
            Self::Menu => "Menu",
            Self::MenuItem => "MenuItem",
            Self::Link => "Link",
            Self::Button => "Button",
            Self::SplitButton => "SplitButton",
            Self::TextInput => "TextInput",
            Self::TextArea => "TextArea",
            Self::Checkbox => "Checkbox",
            Self::RadioButton => "RadioButton",
            Self::ComboBox => "ComboBox",
            Self::Slider => "Slider",
            Self::Spinner => "Spinner",
            Self::ScrollBar => "ScrollBar",
            Self::List => "List",
            Self::ListItem => "ListItem",
            Self::TreeView => "TreeView",
            Self::TreeItem => "TreeItem",
            Self::Table => "Table",
            Self::TableRow => "TableRow",
            Self::TableCell => "TableCell",
            Self::DataGrid => "DataGrid",
            Self::DataItem => "DataItem",
            Self::Header => "Header",
            Self::HeaderItem => "HeaderItem",
            Self::Text => "Text",
            Self::Heading => "Heading",
            Self::Label => "Label",
            Self::Image => "Image",
            Self::Document => "Document",
            Self::ProgressBar => "ProgressBar",
            Self::Tooltip => "Tooltip",
            Self::Separator => "Separator",
            Self::Calendar => "Calendar",
            Self::Thumb => "Thumb",
            Self::SemanticZoom => "SemanticZoom",
            Self::AppBar => "AppBar",
            Self::Custom => "Custom",
            Self::Unknown(s) => s.as_str(),
        }
    }

    /// Returns true if this role is typically interactive (clickable, editable, selectable).
    pub fn is_interactive(&self) -> bool {
        matches!(
            self,
            NormalizedRole::Button
                | NormalizedRole::SplitButton
                | NormalizedRole::TextInput
                | NormalizedRole::TextArea
                | NormalizedRole::Checkbox
                | NormalizedRole::RadioButton
                | NormalizedRole::ComboBox
                | NormalizedRole::Slider
                | NormalizedRole::Spinner
                | NormalizedRole::ScrollBar
                | NormalizedRole::TabItem
                | NormalizedRole::MenuItem
                | NormalizedRole::Link
                | NormalizedRole::ListItem
                | NormalizedRole::TreeItem
                | NormalizedRole::DataItem
        )
    }
}

// ---------------------------------------------------------------------------
// Element state enumeration
// ---------------------------------------------------------------------------

/// Typed element states. Platform implementations map their native states to these.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ElementState {
    Enabled,
    Disabled,
    Focused,
    Selected,
    Expanded,
    Collapsed,
    Checked,
    Unchecked,
    Indeterminate,
    ReadOnly,
    Offscreen,
    Pressed,
    Required,
    Invalid,
}

// ---------------------------------------------------------------------------
// Windows UIA control type → NormalizedRole mapping
// ---------------------------------------------------------------------------

/// Maps a Windows UIA control type ID (50000–50040) to a [`NormalizedRole`].
///
/// Reference: <https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-controltype-ids>
///
/// | UIA ID | UIA Name             | NormalizedRole  |
/// |--------|----------------------|-----------------|
/// | 50000  | Button               | Button          |
/// | 50001  | Calendar             | Calendar        |
/// | 50002  | CheckBox             | Checkbox        |
/// | 50003  | ComboBox             | ComboBox        |
/// | 50004  | Edit                 | TextInput       |
/// | 50005  | Hyperlink            | Link            |
/// | 50006  | Image                | Image           |
/// | 50007  | ListItem             | ListItem        |
/// | 50008  | List                 | List            |
/// | 50009  | Menu                 | Menu            |
/// | 50010  | MenuBar              | MenuBar         |
/// | 50011  | MenuItem             | MenuItem        |
/// | 50012  | ProgressBar          | ProgressBar     |
/// | 50013  | RadioButton          | RadioButton     |
/// | 50014  | ScrollBar            | ScrollBar       |
/// | 50015  | Slider               | Slider          |
/// | 50016  | Spinner              | Spinner         |
/// | 50017  | StatusBar            | StatusBar       |
/// | 50018  | Tab                  | Tab             |
/// | 50019  | TabItem              | TabItem         |
/// | 50020  | Text                 | Text            |
/// | 50021  | ToolBar              | Toolbar         |
/// | 50022  | ToolTip              | Tooltip         |
/// | 50023  | Tree                 | TreeView        |
/// | 50024  | TreeItem             | TreeItem        |
/// | 50025  | Custom               | Custom          |
/// | 50026  | Group                | Group           |
/// | 50027  | Thumb                | Thumb           |
/// | 50028  | DataGrid             | DataGrid        |
/// | 50029  | DataItem             | DataItem        |
/// | 50030  | Document             | Document        |
/// | 50031  | SplitButton          | SplitButton     |
/// | 50032  | Window               | Window          |
/// | 50033  | Pane                 | Pane            |
/// | 50034  | Header               | Header          |
/// | 50035  | HeaderItem           | HeaderItem      |
/// | 50036  | Table                | Table           |
/// | 50037  | TitleBar             | TitleBar        |
/// | 50038  | Separator            | Separator       |
/// | 50039  | SemanticZoom         | SemanticZoom    |
/// | 50040  | AppBar               | AppBar          |
pub fn uia_control_type_to_role(control_type_id: i32) -> NormalizedRole {
    match control_type_id {
        50000 => NormalizedRole::Button,
        50001 => NormalizedRole::Calendar,
        50002 => NormalizedRole::Checkbox,
        50003 => NormalizedRole::ComboBox,
        50004 => NormalizedRole::TextInput,
        50005 => NormalizedRole::Link,
        50006 => NormalizedRole::Image,
        50007 => NormalizedRole::ListItem,
        50008 => NormalizedRole::List,
        50009 => NormalizedRole::Menu,
        50010 => NormalizedRole::MenuBar,
        50011 => NormalizedRole::MenuItem,
        50012 => NormalizedRole::ProgressBar,
        50013 => NormalizedRole::RadioButton,
        50014 => NormalizedRole::ScrollBar,
        50015 => NormalizedRole::Slider,
        50016 => NormalizedRole::Spinner,
        50017 => NormalizedRole::StatusBar,
        50018 => NormalizedRole::Tab,
        50019 => NormalizedRole::TabItem,
        50020 => NormalizedRole::Text,
        50021 => NormalizedRole::Toolbar,
        50022 => NormalizedRole::Tooltip,
        50023 => NormalizedRole::TreeView,
        50024 => NormalizedRole::TreeItem,
        50025 => NormalizedRole::Custom,
        50026 => NormalizedRole::Group,
        50027 => NormalizedRole::Thumb,
        50028 => NormalizedRole::DataGrid,
        50029 => NormalizedRole::DataItem,
        50030 => NormalizedRole::Document,
        50031 => NormalizedRole::SplitButton,
        50032 => NormalizedRole::Window,
        50033 => NormalizedRole::Pane,
        50034 => NormalizedRole::Header,
        50035 => NormalizedRole::HeaderItem,
        50036 => NormalizedRole::Table,
        50037 => NormalizedRole::TitleBar,
        50038 => NormalizedRole::Separator,
        50039 => NormalizedRole::SemanticZoom,
        50040 => NormalizedRole::AppBar,
        other => NormalizedRole::Unknown(format!("UIA_{}", other)),
    }
}

// ---------------------------------------------------------------------------
// macOS AX role → NormalizedRole mapping
// ---------------------------------------------------------------------------

/// Maps a macOS Accessibility API role string to a [`NormalizedRole`].
///
/// Reference: <https://developer.apple.com/documentation/appkit/nsaccessibility/role>
///
/// | AX Role                    | NormalizedRole  | Notes                                    |
/// |----------------------------|-----------------|------------------------------------------|
/// | AXWindow                   | Window          |                                          |
/// | AXSheet                    | Dialog          | macOS modal sheet                        |
/// | AXDialog                   | Dialog          |                                          |
/// | AXDrawer                   | Pane            | macOS drawer panel                       |
/// | AXGroup                    | Group           |                                          |
/// | AXGrowArea                 | Group           | Window resize handle                     |
/// | AXToolbar                  | Toolbar         |                                          |
/// | AXStatusBar                | StatusBar       | Rarely used on macOS                     |
/// | AXMenuBar                  | MenuBar         |                                          |
/// | AXMenuBarItem              | MenuItem        | Top-level menu bar item                  |
/// | AXMenu                     | Menu            |                                          |
/// | AXMenuItem                 | MenuItem        |                                          |
/// | AXSplitGroup               | Group           | Split view container                     |
/// | AXSplitter                 | Separator       | Draggable split divider                  |
/// | AXTabGroup                 | Tab             |                                          |
/// | AXTab                      | TabItem         | Individual tab in tab group              |
/// | AXButton                   | Button          |                                          |
/// | AXRadioButton              | RadioButton     |                                          |
/// | AXRadioGroup               | Group           | Container for radio buttons              |
/// | AXCheckBox                 | Checkbox        |                                          |
/// | AXPopUpButton              | ComboBox        | macOS popup button ≈ dropdown            |
/// | AXMenuButton               | Button          | Button that opens a menu                 |
/// | AXDisclosureTriangle       | Button          | Expand/collapse triangle                 |
/// | AXTextField                | TextInput       |                                          |
/// | AXTextArea                 | TextArea        |                                          |
/// | AXSearchField              | TextInput       | Spotlight-style search field             |
/// | AXSecureTextField          | TextInput       | Password field                           |
/// | AXStaticText               | Text            |                                          |
/// | AXHeading                  | Heading         | Web heading (H1-H6)                     |
/// | AXLink                     | Link            |                                          |
/// | AXImage                    | Image           |                                          |
/// | AXSlider                   | Slider          |                                          |
/// | AXIncrementor              | Spinner         | macOS stepper control                    |
/// | AXProgressIndicator        | ProgressBar     |                                          |
/// | AXBusyIndicator            | ProgressBar     | Indeterminate spinner                    |
/// | AXRelevanceIndicator       | ProgressBar     | Relevance bar                            |
/// | AXScrollArea               | Pane            | Scrollable container                     |
/// | AXScrollBar                | ScrollBar       |                                          |
/// | AXList                     | List            |                                          |
/// | AXOutline                  | TreeView        | macOS outline view ≈ tree view           |
/// | AXOutlineRow               | TreeItem        | Row in outline view                      |
/// | AXBrowser                  | TreeView        | Column browser (Finder column view)      |
/// | AXTable                    | Table           |                                          |
/// | AXRow                      | TableRow        |                                          |
/// | AXColumn                   | TableCell       | Column in table (closest mapping)        |
/// | AXCell                     | TableCell       |                                          |
/// | AXGrid                     | DataGrid        |                                          |
/// | AXComboBox                 | ComboBox        |                                          |
/// | AXColorWell                | Button          | Color picker button                      |
/// | AXDateField                | TextInput       | Date input field                         |
/// | AXHelpTag                  | Tooltip         | macOS help tag                           |
/// | AXLevelIndicator           | ProgressBar     | Rating/level indicator                   |
/// | AXMatte                    | Group           | Matte/backdrop container                 |
/// | AXRuler                    | Group           | Ruler control                            |
/// | AXRulerMarker              | Thumb           | Marker on ruler                          |
/// | AXValueIndicator           | Thumb           | Current value indicator on slider        |
/// | AXToolbarItem              | Button          | Individual toolbar button                |
/// | AXLayoutArea               | Group           | Layout container                         |
/// | AXLayoutItem               | Group           | Item in layout area                      |
/// | AXHandle                   | Thumb           | Draggable handle                         |
/// | AXSortButton               | Button          | Table column sort button                 |
/// | AXSaveRecentDocument       | Button          | Recent document save button              |
/// | AXWebArea                  | Document        | Web content area (WebKit)                |
/// | AXUnknown                  | Unknown         |                                          |
///
/// Subroles (used to refine mapping when available):
/// | AX Subrole                 | Overrides to    | Notes                                    |
/// |----------------------------|-----------------|------------------------------------------|
/// | AXCloseButton              | Button          |                                          |
/// | AXMinimizeButton           | Button          |                                          |
/// | AXZoomButton               | Button          |                                          |
/// | AXFullScreenButton         | Button          |                                          |
/// | AXSearchField              | TextInput       |                                          |
/// | AXSecureTextField          | TextInput       |                                          |
/// | AXDialog                   | Dialog          |                                          |
/// | AXSystemDialog             | Dialog          |                                          |
/// | AXFloatingWindow           | Window          |                                          |
/// | AXStandardWindow           | Window          |                                          |
pub fn ax_role_to_normalized(role: &str) -> NormalizedRole {
    match role {
        // Containers
        "AXWindow" | "AXFloatingWindow" | "AXStandardWindow" => NormalizedRole::Window,
        "AXSheet" | "AXDialog" | "AXSystemDialog" => NormalizedRole::Dialog,
        "AXDrawer" | "AXScrollArea" => NormalizedRole::Pane,
        "AXGroup" | "AXSplitGroup" | "AXRadioGroup" | "AXMatte"
        | "AXRuler" | "AXLayoutArea" | "AXLayoutItem" | "AXGrowArea" => NormalizedRole::Group,
        "AXToolbar" => NormalizedRole::Toolbar,
        "AXStatusBar" => NormalizedRole::StatusBar,
        "AXMenuBar" => NormalizedRole::MenuBar,
        "AXTitleBar" => NormalizedRole::TitleBar,

        // Navigation
        "AXTabGroup" => NormalizedRole::Tab,
        "AXTab" => NormalizedRole::TabItem,
        "AXMenu" => NormalizedRole::Menu,
        "AXMenuItem" | "AXMenuBarItem" => NormalizedRole::MenuItem,
        "AXLink" => NormalizedRole::Link,

        // Input controls
        "AXButton" | "AXDisclosureTriangle" | "AXMenuButton" | "AXColorWell"
        | "AXToolbarItem" | "AXSortButton" | "AXSaveRecentDocument"
        | "AXCloseButton" | "AXMinimizeButton" | "AXZoomButton"
        | "AXFullScreenButton" => NormalizedRole::Button,
        "AXTextField" | "AXSearchField" | "AXSecureTextField"
        | "AXDateField" => NormalizedRole::TextInput,
        "AXTextArea" => NormalizedRole::TextArea,
        "AXCheckBox" => NormalizedRole::Checkbox,
        "AXRadioButton" => NormalizedRole::RadioButton,
        "AXPopUpButton" | "AXComboBox" => NormalizedRole::ComboBox,
        "AXSlider" => NormalizedRole::Slider,
        "AXIncrementor" => NormalizedRole::Spinner,
        "AXScrollBar" => NormalizedRole::ScrollBar,

        // Data display
        "AXList" => NormalizedRole::List,
        "AXOutline" | "AXBrowser" => NormalizedRole::TreeView,
        "AXOutlineRow" => NormalizedRole::TreeItem,
        "AXTable" => NormalizedRole::Table,
        "AXRow" => NormalizedRole::TableRow,
        "AXColumn" | "AXCell" => NormalizedRole::TableCell,
        "AXGrid" => NormalizedRole::DataGrid,
        "AXHeader" => NormalizedRole::Header,
        "AXHeaderItem" => NormalizedRole::HeaderItem,

        // Content
        "AXStaticText" => NormalizedRole::Text,
        "AXHeading" => NormalizedRole::Heading,
        "AXImage" => NormalizedRole::Image,
        "AXWebArea" => NormalizedRole::Document,
        "AXProgressIndicator" | "AXBusyIndicator"
        | "AXRelevanceIndicator" | "AXLevelIndicator" => NormalizedRole::ProgressBar,
        "AXHelpTag" => NormalizedRole::Tooltip,
        "AXSplitter" => NormalizedRole::Separator,

        // Thumb-like
        "AXRulerMarker" | "AXValueIndicator" | "AXHandle" => NormalizedRole::Thumb,

        // Calendar
        "AXCalendar" => NormalizedRole::Calendar,

        // Catch-all
        "AXUnknown" => NormalizedRole::Unknown("AXUnknown".to_string()),
        other => NormalizedRole::Unknown(other.to_string()),
    }
}

// ---------------------------------------------------------------------------
// Prompt formatting (platform-agnostic)
// ---------------------------------------------------------------------------

/// Format a [`NormalizedTree`] as compact text suitable for LLM prompts.
///
/// Output format:
/// ```text
/// === Accessibility Tree ===
/// Window: Notepad (notepad.exe) [windows]
/// Elements: 42
///
/// Button 'Save' @(150,200) [Shortcut: Ctrl+S]
///   Text 'Save'
/// TextInput 'Username' = "john" [focused]
/// ```
pub fn format_tree_for_prompt(tree: &NormalizedTree) -> String {
    let mut out = String::with_capacity(4096);
    out.push_str(&format!(
        "=== Accessibility Tree ===\nWindow: {} ({}) [{}]\nElements: {}\n\n",
        tree.window_title, tree.process_name, tree.platform, tree.element_count
    ));

    for el in &tree.elements {
        format_element(&mut out, el, 0);
    }

    out
}

fn format_element(out: &mut String, el: &NormalizedElement, indent: usize) {
    let pad = "  ".repeat(indent);
    let role_str = format!("{:?}", el.role);
    let mut desc = format!("{}{}", pad, role_str);

    if !el.name.is_empty() {
        desc.push_str(&format!(" '{}'", el.name));
    }
    if let Some(ref value) = el.value {
        desc.push_str(&format!(" = \"{}\"", value));
    }

    // States
    let state_strs: Vec<String> = el.states.iter().map(|s| format!("{:?}", s).to_lowercase()).collect();
    if !state_strs.is_empty() {
        desc.push_str(&format!(" [{}]", state_strs.join(", ")));
    }

    // Click target for interactive elements
    if el.role.is_interactive() {
        if let Some(ref bounds) = el.bounds {
            if bounds.width > 0 && bounds.height > 0 {
                let cx = bounds.x + (bounds.width as i32) / 2;
                let cy = bounds.y + (bounds.height as i32) / 2;
                desc.push_str(&format!(" @({},{})", cx, cy));
            }
        }
    }

    if let Some(ref shortcut) = el.shortcut {
        desc.push_str(&format!(" [Shortcut: {}]", shortcut));
    }

    out.push_str(&desc);
    out.push('\n');

    for child in &el.children {
        format_element(out, child, indent + 1);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_uia_mapping_coverage_all_41_types() {
        // All documented UIA control type IDs (50000–50040 inclusive)
        for id in 50000..=50040 {
            let role = uia_control_type_to_role(id);
            assert!(
                !matches!(role, NormalizedRole::Unknown(_)),
                "UIA control type {} should have an explicit mapping, got Unknown",
                id
            );
        }
    }

    #[test]
    fn test_uia_unknown_for_invalid_ids() {
        let role = uia_control_type_to_role(99999);
        assert!(matches!(role, NormalizedRole::Unknown(_)));
        if let NormalizedRole::Unknown(s) = role {
            assert_eq!(s, "UIA_99999");
        }
    }

    #[test]
    fn test_ax_mapping_core_roles() {
        // Verify the most common macOS AX roles map correctly
        let mappings = vec![
            ("AXWindow", NormalizedRole::Window),
            ("AXButton", NormalizedRole::Button),
            ("AXTextField", NormalizedRole::TextInput),
            ("AXTextArea", NormalizedRole::TextArea),
            ("AXCheckBox", NormalizedRole::Checkbox),
            ("AXRadioButton", NormalizedRole::RadioButton),
            ("AXPopUpButton", NormalizedRole::ComboBox),
            ("AXComboBox", NormalizedRole::ComboBox),
            ("AXSlider", NormalizedRole::Slider),
            ("AXList", NormalizedRole::List),
            ("AXOutline", NormalizedRole::TreeView),
            ("AXTable", NormalizedRole::Table),
            ("AXTabGroup", NormalizedRole::Tab),
            ("AXMenu", NormalizedRole::Menu),
            ("AXMenuItem", NormalizedRole::MenuItem),
            ("AXLink", NormalizedRole::Link),
            ("AXStaticText", NormalizedRole::Text),
            ("AXHeading", NormalizedRole::Heading),
            ("AXImage", NormalizedRole::Image),
            ("AXWebArea", NormalizedRole::Document),
            ("AXProgressIndicator", NormalizedRole::ProgressBar),
            ("AXScrollBar", NormalizedRole::ScrollBar),
            ("AXToolbar", NormalizedRole::Toolbar),
            ("AXStatusBar", NormalizedRole::StatusBar),
            ("AXMenuBar", NormalizedRole::MenuBar),
            ("AXSheet", NormalizedRole::Dialog),
            ("AXDialog", NormalizedRole::Dialog),
            ("AXGroup", NormalizedRole::Group),
            ("AXDisclosureTriangle", NormalizedRole::Button),
            ("AXSearchField", NormalizedRole::TextInput),
            ("AXSecureTextField", NormalizedRole::TextInput),
            ("AXIncrementor", NormalizedRole::Spinner),
        ];

        for (ax_role, expected) in mappings {
            let result = ax_role_to_normalized(ax_role);
            assert_eq!(
                result, expected,
                "AX role '{}' should map to {:?}, got {:?}",
                ax_role, expected, result
            );
        }
    }

    #[test]
    fn test_ax_unknown_for_unmapped_roles() {
        let role = ax_role_to_normalized("AXCustomWidget");
        assert!(matches!(role, NormalizedRole::Unknown(_)));
        if let NormalizedRole::Unknown(s) = role {
            assert_eq!(s, "AXCustomWidget");
        }
    }

    #[test]
    fn test_interactive_roles() {
        assert!(NormalizedRole::Button.is_interactive());
        assert!(NormalizedRole::TextInput.is_interactive());
        assert!(NormalizedRole::Checkbox.is_interactive());
        assert!(NormalizedRole::Link.is_interactive());
        assert!(NormalizedRole::ListItem.is_interactive());

        assert!(!NormalizedRole::Window.is_interactive());
        assert!(!NormalizedRole::Group.is_interactive());
        assert!(!NormalizedRole::Text.is_interactive());
        assert!(!NormalizedRole::Image.is_interactive());
        assert!(!NormalizedRole::Separator.is_interactive());
    }

    #[test]
    fn test_rect_invariants() {
        let rect = Rect { x: 100, y: 200, width: 50, height: 30 };
        assert!(rect.x >= 0, "x should be non-negative for normal screen coordinates");
        assert!(rect.y >= 0, "y should be non-negative for top-left origin");
        assert!(rect.width > 0);
        assert!(rect.height > 0);
    }

    #[test]
    fn test_normalized_tree_serialization() {
        let tree = NormalizedTree {
            window_title: "Test Window".to_string(),
            process_name: "test.exe".to_string(),
            platform: "windows".to_string(),
            element_count: 1,
            elements: vec![NormalizedElement {
                id: 1,
                name: "OK".to_string(),
                role: NormalizedRole::Button,
                value: None,
                bounds: Some(Rect { x: 10, y: 20, width: 80, height: 30 }),
                states: vec![ElementState::Enabled, ElementState::Focused],
                shortcut: Some("Enter".to_string()),
                depth: 0,
                children_count: 0,
                children: Vec::new(),
            }],
        };

        let json = serde_json::to_string(&tree).expect("serialization should succeed");
        assert!(json.contains("\"window_title\":\"Test Window\""));
        assert!(json.contains("\"Button\""));
        assert!(json.contains("\"enabled\""));
        assert!(json.contains("\"focused\""));

        // Round-trip
        let deserialized: NormalizedTree = serde_json::from_str(&json).expect("deserialization should succeed");
        assert_eq!(deserialized.window_title, "Test Window");
        assert_eq!(deserialized.elements.len(), 1);
        assert_eq!(deserialized.elements[0].role, NormalizedRole::Button);
    }

    #[test]
    fn test_format_tree_for_prompt() {
        let tree = NormalizedTree {
            window_title: "Settings".to_string(),
            process_name: "app.exe".to_string(),
            platform: "windows".to_string(),
            element_count: 2,
            elements: vec![
                NormalizedElement {
                    id: 1,
                    name: "Username".to_string(),
                    role: NormalizedRole::TextInput,
                    value: Some("john".to_string()),
                    bounds: Some(Rect { x: 100, y: 50, width: 200, height: 24 }),
                    states: vec![ElementState::Focused],
                    shortcut: None,
                    depth: 0,
                    children_count: 0,
                    children: Vec::new(),
                },
                NormalizedElement {
                    id: 2,
                    name: "Save".to_string(),
                    role: NormalizedRole::Button,
                    value: None,
                    bounds: Some(Rect { x: 100, y: 80, width: 80, height: 30 }),
                    states: vec![ElementState::Enabled],
                    shortcut: Some("Ctrl+S".to_string()),
                    depth: 0,
                    children_count: 0,
                    children: Vec::new(),
                },
            ],
        };

        let prompt = format_tree_for_prompt(&tree);
        assert!(prompt.contains("=== Accessibility Tree ==="));
        assert!(prompt.contains("Settings (app.exe) [windows]"));
        assert!(prompt.contains("TextInput 'Username' = \"john\""));
        assert!(prompt.contains("Button 'Save'"));
        assert!(prompt.contains("@(140,95)")); // center of (100,80,80,30)
        assert!(prompt.contains("[Shortcut: Ctrl+S]"));
    }

    #[test]
    fn test_coordinate_flip_helper() {
        // Verify the formula: y_normalized = screen_height - ax_y - element_height
        // macOS AX: element at bottom-left-origin y=100, height=30, screen_height=900
        let screen_height: i32 = 900;
        let ax_y: i32 = 100;  // bottom-left origin (100px from bottom)
        let element_height: u32 = 30;

        let y_normalized = screen_height - ax_y - element_height as i32;
        // Expected: 900 - 100 - 30 = 770 (770px from top)
        assert_eq!(y_normalized, 770);
        assert!(y_normalized >= 0, "Normalized y must be non-negative");
    }

    // -----------------------------------------------------------------------
    // Additional contract tests (added by Tester)
    // -----------------------------------------------------------------------

    #[test]
    fn test_ax_mapping_exhaustive_all_documented_roles() {
        // Every AX role from the doc table MUST map to a known NormalizedRole
        // (not Unknown), ensuring macOS produces meaningful data for Claude.
        let all_documented = vec![
            "AXWindow", "AXSheet", "AXDialog", "AXDrawer", "AXGroup",
            "AXGrowArea", "AXToolbar", "AXStatusBar", "AXMenuBar",
            "AXMenuBarItem", "AXMenu", "AXMenuItem", "AXSplitGroup",
            "AXSplitter", "AXTabGroup", "AXTab", "AXButton",
            "AXRadioButton", "AXRadioGroup", "AXCheckBox", "AXPopUpButton",
            "AXMenuButton", "AXDisclosureTriangle", "AXTextField",
            "AXTextArea", "AXSearchField", "AXSecureTextField",
            "AXStaticText", "AXHeading", "AXLink", "AXImage", "AXSlider",
            "AXIncrementor", "AXProgressIndicator", "AXBusyIndicator",
            "AXRelevanceIndicator", "AXScrollArea", "AXScrollBar",
            "AXList", "AXOutline", "AXOutlineRow", "AXBrowser", "AXTable",
            "AXRow", "AXColumn", "AXCell", "AXGrid", "AXComboBox",
            "AXColorWell", "AXDateField", "AXHelpTag", "AXLevelIndicator",
            "AXMatte", "AXRuler", "AXRulerMarker", "AXValueIndicator",
            "AXToolbarItem", "AXLayoutArea", "AXLayoutItem", "AXHandle",
            "AXSortButton", "AXSaveRecentDocument", "AXWebArea",
            // Subroles that also have explicit mappings
            "AXCloseButton", "AXMinimizeButton", "AXZoomButton",
            "AXFullScreenButton", "AXFloatingWindow", "AXStandardWindow",
            "AXSystemDialog", "AXTitleBar", "AXCalendar",
        ];

        for role in &all_documented {
            let result = ax_role_to_normalized(role);
            assert!(
                !matches!(result, NormalizedRole::Unknown(_)),
                "AX role '{}' should have an explicit mapping, got {:?}",
                role, result
            );
        }
    }

    #[test]
    fn test_coordinate_flip_edge_cases() {
        // Element at the very top of screen (AX y = screen_height - height)
        let screen_h: i32 = 1080;
        let ax_y_top = screen_h - 30;  // AX bottom-left y for element at top
        let y_top = screen_h - ax_y_top - 30;
        assert_eq!(y_top, 0, "Element at screen top should normalize to y=0");

        // Element at the very bottom of screen (AX y = 0)
        let ax_y_bottom: i32 = 0;
        let y_bottom = screen_h - ax_y_bottom - 30;
        assert_eq!(y_bottom, 1050, "Element at screen bottom should normalize to screen_h - height");

        // Retina display (2x resolution)
        let retina_h: i32 = 2160;
        let ax_y_mid: i32 = 1080;
        let y_mid = retina_h - ax_y_mid - 100;
        assert_eq!(y_mid, 980);
        assert!(y_mid >= 0 && y_mid < retina_h);

        // 4K display
        let ultra_h: i32 = 2160;
        let ax_y_4k: i32 = 500;
        let y_4k = ultra_h - ax_y_4k - 50;
        assert_eq!(y_4k, 1610);
        assert!(y_4k >= 0);
    }

    #[test]
    fn test_serialization_empty_tree() {
        let tree = NormalizedTree {
            window_title: String::new(),
            process_name: String::new(),
            platform: "macos".to_string(),
            element_count: 0,
            elements: Vec::new(),
        };

        let json = serde_json::to_string(&tree).expect("empty tree should serialize");
        let rt: NormalizedTree = serde_json::from_str(&json).expect("should deserialize");
        assert_eq!(rt.platform, "macos");
        assert_eq!(rt.elements.len(), 0);
        assert_eq!(rt.element_count, 0);
    }

    #[test]
    fn test_serialization_nested_children() {
        let tree = NormalizedTree {
            window_title: "App".to_string(),
            process_name: "app".to_string(),
            platform: "windows".to_string(),
            element_count: 3,
            elements: vec![NormalizedElement {
                id: 1,
                name: "Panel".to_string(),
                role: NormalizedRole::Group,
                value: None,
                bounds: Some(Rect { x: 0, y: 0, width: 800, height: 600 }),
                states: vec![],
                shortcut: None,
                depth: 0,
                children_count: 2,
                children: vec![
                    NormalizedElement {
                        id: 2,
                        name: "Label".to_string(),
                        role: NormalizedRole::Text,
                        value: Some("Hello".to_string()),
                        bounds: Some(Rect { x: 10, y: 10, width: 100, height: 20 }),
                        states: vec![],
                        shortcut: None,
                        depth: 1,
                        children_count: 0,
                        children: Vec::new(),
                    },
                    NormalizedElement {
                        id: 3,
                        name: "OK".to_string(),
                        role: NormalizedRole::Button,
                        value: None,
                        bounds: Some(Rect { x: 10, y: 40, width: 80, height: 30 }),
                        states: vec![ElementState::Enabled, ElementState::Focused],
                        shortcut: Some("Enter".to_string()),
                        depth: 1,
                        children_count: 0,
                        children: Vec::new(),
                    },
                ],
            }],
        };

        let json = serde_json::to_string(&tree).expect("nested tree should serialize");
        let rt: NormalizedTree = serde_json::from_str(&json).expect("should round-trip");
        assert_eq!(rt.elements.len(), 1);
        assert_eq!(rt.elements[0].children.len(), 2);
        assert_eq!(rt.elements[0].children[0].role, NormalizedRole::Text);
        assert_eq!(rt.elements[0].children[1].role, NormalizedRole::Button);
        assert_eq!(rt.elements[0].children[1].states.len(), 2);
    }

    #[test]
    fn test_serialization_unknown_role_roundtrip() {
        let el = NormalizedElement {
            id: 1,
            name: "Custom".to_string(),
            role: NormalizedRole::Unknown("AXSpecialWidget".to_string()),
            value: None,
            bounds: None,
            states: vec![],
            shortcut: None,
            depth: 0,
            children_count: 0,
            children: Vec::new(),
        };

        let json = serde_json::to_string(&el).expect("Unknown role should serialize");
        // Unknown uses #[serde(untagged)] — serializes as bare string
        assert!(json.contains("AXSpecialWidget"), "Unknown role string must survive serialization");
        let rt: NormalizedElement = serde_json::from_str(&json).expect("should deserialize");
        assert_eq!(rt.role, NormalizedRole::Unknown("AXSpecialWidget".to_string()));
    }

    #[test]
    fn test_all_element_states_serialize() {
        let all_states = vec![
            ElementState::Enabled,
            ElementState::Disabled,
            ElementState::Focused,
            ElementState::Selected,
            ElementState::Expanded,
            ElementState::Collapsed,
            ElementState::Checked,
            ElementState::Unchecked,
            ElementState::Indeterminate,
            ElementState::ReadOnly,
            ElementState::Offscreen,
            ElementState::Pressed,
            ElementState::Required,
            ElementState::Invalid,
        ];

        let el = NormalizedElement {
            id: 1,
            name: "Test".to_string(),
            role: NormalizedRole::Button,
            value: None,
            bounds: None,
            states: all_states.clone(),
            shortcut: None,
            depth: 0,
            children_count: 0,
            children: Vec::new(),
        };

        let json = serde_json::to_string(&el).expect("all states should serialize");
        let rt: NormalizedElement = serde_json::from_str(&json).expect("should round-trip");
        assert_eq!(rt.states.len(), 14, "All 14 ElementState variants must survive round-trip");

        // Verify serde renames (lowercase)
        assert!(json.contains("\"enabled\""), "Enabled should serialize as lowercase");
        assert!(json.contains("\"disabled\""));
        assert!(json.contains("\"indeterminate\""));
        assert!(json.contains("\"readonly\""));
        assert!(json.contains("\"offscreen\""));
    }

    #[test]
    fn test_format_prompt_nested_indentation() {
        let tree = NormalizedTree {
            window_title: "Test".to_string(),
            process_name: "test".to_string(),
            platform: "macos".to_string(),
            element_count: 3,
            elements: vec![NormalizedElement {
                id: 1,
                name: "Form".to_string(),
                role: NormalizedRole::Group,
                value: None,
                bounds: None,
                states: vec![],
                shortcut: None,
                depth: 0,
                children_count: 1,
                children: vec![NormalizedElement {
                    id: 2,
                    name: "Submit".to_string(),
                    role: NormalizedRole::Button,
                    value: None,
                    bounds: Some(Rect { x: 50, y: 100, width: 100, height: 30 }),
                    states: vec![],
                    shortcut: None,
                    depth: 1,
                    children_count: 0,
                    children: Vec::new(),
                }],
            }],
        };

        let prompt = format_tree_for_prompt(&tree);
        // Parent at indent 0, child at indent 1 (2 spaces)
        assert!(prompt.contains("Group 'Form'"), "Parent should be at top level");
        assert!(prompt.contains("  Button 'Submit'"), "Child should be indented with 2 spaces");
        assert!(prompt.contains("@(100,115)"), "Interactive child should show click target");
    }

    #[test]
    fn test_format_prompt_non_interactive_no_coords() {
        let tree = NormalizedTree {
            window_title: "T".to_string(),
            process_name: "t".to_string(),
            platform: "windows".to_string(),
            element_count: 1,
            elements: vec![NormalizedElement {
                id: 1,
                name: "Label".to_string(),
                role: NormalizedRole::Text,
                value: Some("Hello world".to_string()),
                bounds: Some(Rect { x: 50, y: 50, width: 200, height: 20 }),
                states: vec![],
                shortcut: None,
                depth: 0,
                children_count: 0,
                children: Vec::new(),
            }],
        };

        let prompt = format_tree_for_prompt(&tree);
        // Text is NOT interactive — should NOT have @(x,y) coordinates
        assert!(!prompt.contains("@("), "Non-interactive elements should not show click coordinates");
        assert!(prompt.contains("Text 'Label' = \"Hello world\""));
    }

    #[test]
    fn test_uia_and_ax_produce_same_role_for_equivalent_elements() {
        // Verify that equivalent UI concepts map to the same NormalizedRole
        // on both platforms — this is the core parity guarantee.
        let parity_pairs: Vec<(i32, &str, NormalizedRole)> = vec![
            (50000, "AXButton", NormalizedRole::Button),
            (50002, "AXCheckBox", NormalizedRole::Checkbox),
            (50003, "AXComboBox", NormalizedRole::ComboBox),
            (50004, "AXTextField", NormalizedRole::TextInput),
            (50005, "AXLink", NormalizedRole::Link),
            (50006, "AXImage", NormalizedRole::Image),
            // Note: UIA 50007 = ListItem, but macOS has no direct AXListItem role.
            // AXOutlineRow maps to TreeItem. These roles diverge by design.
            (50008, "AXList", NormalizedRole::List),
            (50009, "AXMenu", NormalizedRole::Menu),
            (50010, "AXMenuBar", NormalizedRole::MenuBar),
            (50011, "AXMenuItem", NormalizedRole::MenuItem),
            (50012, "AXProgressIndicator", NormalizedRole::ProgressBar),
            (50013, "AXRadioButton", NormalizedRole::RadioButton),
            (50014, "AXScrollBar", NormalizedRole::ScrollBar),
            (50015, "AXSlider", NormalizedRole::Slider),
            (50017, "AXStatusBar", NormalizedRole::StatusBar),
            (50018, "AXTabGroup", NormalizedRole::Tab),
            (50019, "AXTab", NormalizedRole::TabItem),
            (50020, "AXStaticText", NormalizedRole::Text),
            (50021, "AXToolbar", NormalizedRole::Toolbar),
            (50022, "AXHelpTag", NormalizedRole::Tooltip),
            (50023, "AXOutline", NormalizedRole::TreeView),
            (50024, "AXOutlineRow", NormalizedRole::TreeItem),
            (50026, "AXGroup", NormalizedRole::Group),
            (50036, "AXTable", NormalizedRole::Table),
            (50032, "AXWindow", NormalizedRole::Window),
        ];

        for (uia_id, ax_role, expected) in parity_pairs {
            let uia_result = uia_control_type_to_role(uia_id);
            let ax_result = ax_role_to_normalized(ax_role);

            assert_eq!(
                uia_result, expected,
                "UIA {} should map to {:?}", uia_id, expected
            );
            assert_eq!(
                ax_result, expected,
                "AX '{}' should map to {:?}", ax_role, expected
            );
        }
    }

    #[test]
    fn test_skip_serializing_optional_fields() {
        // When value, bounds, shortcut are None and states/children are empty,
        // they should be omitted from JSON (not null).
        let el = NormalizedElement {
            id: 1,
            name: "Minimal".to_string(),
            role: NormalizedRole::Label,
            value: None,
            bounds: None,
            states: vec![],
            shortcut: None,
            depth: 0,
            children_count: 0,
            children: Vec::new(),
        };

        let json = serde_json::to_string(&el).expect("minimal element should serialize");
        assert!(!json.contains("\"value\""), "None value should be omitted");
        assert!(!json.contains("\"bounds\""), "None bounds should be omitted");
        assert!(!json.contains("\"shortcut\""), "None shortcut should be omitted");
        assert!(!json.contains("\"states\""), "Empty states should be omitted");
        assert!(!json.contains("\"children\""), "Empty children should be omitted");
        // Required fields must be present
        assert!(json.contains("\"id\""));
        assert!(json.contains("\"name\""));
        assert!(json.contains("\"role\""));
        assert!(json.contains("\"depth\""));
        assert!(json.contains("\"children_count\""));
    }
}
