//! Windows UI Automation tree capture module.
//!
//! Captures the UIA element tree from the foreground window, producing a JSON-serializable
//! structure with element names, roles, states, values, bounding rects, and keyboard shortcuts.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiaElement {
    pub name: String,
    pub role: String,
    pub value: String,
    pub states: Vec<String>,
    pub rect: [i32; 4], // [x, y, width, height]
    #[serde(skip_serializing_if = "String::is_empty")]
    pub accelerator_key: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub access_key: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<UiaElement>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiaTree {
    pub window_title: String,
    pub process_name: String,
    pub element_count: usize,
    pub elements: Vec<UiaElement>,
}

/// Capture the UI Automation tree from the foreground window.
#[cfg(target_os = "windows")]
pub fn capture_uia_tree() -> Result<UiaTree, String> {
    use windows::Win32::UI::Accessibility::*;
    use windows::Win32::System::Com::*;
    use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

    // Initialize COM on this thread
    unsafe {
        CoInitializeEx(Some(std::ptr::null()), COINIT_MULTITHREADED)
            .ok()
            .map_err(|e| format!("COM init failed: {}", e))?;
    }

    let result: Result<UiaTree, String> = (|| {
        let uia: IUIAutomation = unsafe {
            CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)
                .map_err(|e| format!("Failed to create UIA: {}", e))?
        };

        let hwnd = unsafe { GetForegroundWindow() };
        if hwnd.0 == std::ptr::null_mut() {
            return Err("No foreground window".to_string());
        }

        let root: IUIAutomationElement = unsafe {
            uia.ElementFromHandle(hwnd)
                .map_err(|e| format!("ElementFromHandle failed: {}", e))?
        };

        let window_title = unsafe {
            root.CurrentName()
                .map(|s| s.to_string())
                .unwrap_or_default()
        };

        let process_name = get_process_name(hwnd);

        let walker = unsafe {
            uia.ControlViewWalker()
                .map_err(|e| format!("ControlViewWalker failed: {}", e))?
        };

        let mut element_count = 0usize;
        let elements = walk_tree(&walker, &root, 0, 8, &mut element_count, 500);

        Ok(UiaTree {
            window_title,
            process_name,
            element_count,
            elements,
        })
    })();

    unsafe { CoUninitialize() };
    result
}

#[cfg(target_os = "windows")]
fn walk_tree(
    walker: &windows::Win32::UI::Accessibility::IUIAutomationTreeWalker,
    parent: &windows::Win32::UI::Accessibility::IUIAutomationElement,
    depth: u32,
    max_depth: u32,
    count: &mut usize,
    max_elements: usize,
) -> Vec<UiaElement> {
    if depth >= max_depth || *count >= max_elements {
        return Vec::new();
    }

    let mut elements = Vec::new();

    let first_child = unsafe { walker.GetFirstChildElement(parent) };
    let mut current = match first_child {
        Ok(el) => el,
        Err(_) => return elements,
    };

    loop {
        if *count >= max_elements {
            break;
        }

        let element = read_element(&current);

        let dominated = element.name.is_empty()
            && element.value.is_empty()
            && !is_interactive_role(&element.role);

        let mut el = element;
        el.children = walk_tree(walker, &current, depth + 1, max_depth, count, max_elements);

        if !dominated || !el.children.is_empty() {
            *count += 1;
            elements.push(el);
        }

        match unsafe { walker.GetNextSiblingElement(&current) } {
            Ok(next) => current = next,
            Err(_) => break,
        }
    }

    elements
}

#[cfg(target_os = "windows")]
fn read_element(el: &windows::Win32::UI::Accessibility::IUIAutomationElement) -> UiaElement {
    let name = unsafe { el.CurrentName().map(|s| s.to_string()).unwrap_or_default() };
    let control_type_id = unsafe { el.CurrentControlType().unwrap_or(windows::Win32::UI::Accessibility::UIA_CONTROLTYPE_ID(0)) };
    let role = control_type_to_string(control_type_id.0);
    let value = get_value(el);

    let rect = unsafe {
        el.CurrentBoundingRectangle()
            .map(|r| [r.left, r.top, r.right - r.left, r.bottom - r.top])
            .unwrap_or([0, 0, 0, 0])
    };

    let mut states = Vec::new();
    unsafe {
        if let Ok(enabled) = el.CurrentIsEnabled() {
            if !enabled.as_bool() {
                states.push("disabled".to_string());
            }
        }
        if let Ok(offscreen) = el.CurrentIsOffscreen() {
            if offscreen.as_bool() {
                states.push("offscreen".to_string());
            }
        }
    }

    let accelerator_key = unsafe {
        el.CurrentAcceleratorKey().map(|s| s.to_string()).unwrap_or_default()
    };
    let access_key = unsafe {
        el.CurrentAccessKey().map(|s| s.to_string()).unwrap_or_default()
    };

    UiaElement {
        name,
        role,
        value,
        states,
        rect,
        accelerator_key,
        access_key,
        children: Vec::new(),
    }
}

#[cfg(target_os = "windows")]
fn get_value(el: &windows::Win32::UI::Accessibility::IUIAutomationElement) -> String {
    use windows::Win32::UI::Accessibility::*;
    unsafe {
        if let Ok(pattern) = el.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId) {
            if let Ok(val) = pattern.CurrentValue() {
                let s = val.to_string();
                if !s.is_empty() {
                    if s.len() > 200 {
                        return format!("{}...", &s[..200]);
                    }
                    return s;
                }
            }
        }
        if let Ok(pattern) = el.GetCurrentPatternAs::<IUIAutomationTogglePattern>(UIA_TogglePatternId) {
            if let Ok(state) = pattern.CurrentToggleState() {
                return match state {
                    ToggleState_On => "checked".to_string(),
                    ToggleState_Off => "unchecked".to_string(),
                    _ => "indeterminate".to_string(),
                };
            }
        }
        if let Ok(pattern) = el.GetCurrentPatternAs::<IUIAutomationSelectionPattern>(UIA_SelectionPatternId) {
            if let Ok(selection) = pattern.GetCurrentSelection() {
                if let Ok(len) = selection.Length() {
                    if len > 0 {
                        if let Ok(item) = selection.GetElement(0) {
                            if let Ok(name) = item.CurrentName() {
                                return name.to_string();
                            }
                        }
                    }
                }
            }
        }
    }
    String::new()
}

fn is_interactive_role(role: &str) -> bool {
    matches!(
        role,
        "Button" | "Edit" | "ComboBox" | "CheckBox" | "RadioButton"
            | "Slider" | "Tab" | "TabItem" | "MenuItem" | "Link"
            | "ListItem" | "TreeItem" | "DataItem" | "SplitButton"
            | "SpinButton" | "ScrollBar"
    )
}

pub fn control_type_to_string(ct: i32) -> String {
    match ct {
        50000 => "Button",
        50001 => "Calendar",
        50002 => "CheckBox",
        50003 => "ComboBox",
        50004 => "Edit",
        50005 => "Hyperlink",
        50006 => "Image",
        50007 => "ListItem",
        50008 => "List",
        50009 => "Menu",
        50010 => "MenuBar",
        50011 => "MenuItem",
        50012 => "ProgressBar",
        50013 => "RadioButton",
        50014 => "ScrollBar",
        50015 => "Slider",
        50016 => "Spinner",
        50017 => "StatusBar",
        50018 => "Tab",
        50019 => "TabItem",
        50020 => "Text",
        50021 => "ToolBar",
        50022 => "ToolTip",
        50023 => "Tree",
        50024 => "TreeItem",
        50025 => "Custom",
        50026 => "Group",
        50027 => "Thumb",
        50028 => "DataGrid",
        50029 => "DataItem",
        50030 => "Document",
        50031 => "SplitButton",
        50032 => "Window",
        50033 => "Pane",
        50034 => "Header",
        50035 => "HeaderItem",
        50036 => "Table",
        50037 => "TitleBar",
        50038 => "Separator",
        50039 => "SemanticZoom",
        50040 => "AppBar",
        _ => "Unknown",
    }
    .to_string()
}

#[cfg(target_os = "windows")]
fn get_process_name(hwnd: windows::Win32::Foundation::HWND) -> String {
    use windows::Win32::System::Threading::*;
    use windows::Win32::Foundation::CloseHandle;

    unsafe {
        let mut pid = 0u32;
        windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return String::new();
        }

        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid);
        match handle {
            Ok(h) => {
                let mut buf = [0u16; 260];
                let mut size = buf.len() as u32;
                let success = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, windows::core::PWSTR(buf.as_mut_ptr()), &mut size);
                let _ = CloseHandle(h);
                if success.is_ok() {
                    let path = String::from_utf16_lossy(&buf[..size as usize]);
                    path.rsplit('\\').next().unwrap_or("").to_string()
                } else {
                    String::new()
                }
            }
            Err(_) => String::new(),
        }
    }
}

/// Format the UIA tree as compact text for inclusion in LLM prompts.
pub fn format_tree_for_prompt(tree: &UiaTree) -> String {
    let mut out = String::with_capacity(4096);
    out.push_str(&format!(
        "=== UI Automation Tree ===\nWindow: {} ({})\nElements: {}\n\n",
        tree.window_title, tree.process_name, tree.element_count
    ));

    for el in &tree.elements {
        format_element(&mut out, el, 0);
    }

    out
}

fn format_element(out: &mut String, el: &UiaElement, indent: usize) {
    let pad = "  ".repeat(indent);
    let mut desc = format!("{}{}", pad, el.role);

    if !el.name.is_empty() {
        desc.push_str(&format!(" '{}'", el.name));
    }
    if !el.value.is_empty() {
        desc.push_str(&format!(" = \"{}\"", el.value));
    }
    if !el.states.is_empty() {
        desc.push_str(&format!(" [{}]", el.states.join(", ")));
    }
    if is_interactive_role(&el.role) && el.rect[2] > 0 && el.rect[3] > 0 {
        let cx = el.rect[0] + el.rect[2] / 2;
        let cy = el.rect[1] + el.rect[3] / 2;
        desc.push_str(&format!(" @({},{})", cx, cy));
    }
    if !el.accelerator_key.is_empty() {
        desc.push_str(&format!(" [Shortcut: {}]", el.accelerator_key));
    }
    if !el.access_key.is_empty() {
        desc.push_str(&format!(" [AccessKey: {}]", el.access_key));
    }

    out.push_str(&desc);
    out.push('\n');

    for child in &el.children {
        format_element(out, child, indent + 1);
    }
}

#[cfg(not(target_os = "windows"))]
pub fn capture_uia_tree() -> Result<UiaTree, String> {
    Ok(UiaTree {
        window_title: String::new(),
        process_name: String::new(),
        element_count: 0,
        elements: Vec::new(),
    })
}
