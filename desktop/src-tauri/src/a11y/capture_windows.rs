//! Windows UIA tree capture → NormalizedTree.
//!
//! Captures the UI Automation element tree from the foreground window, producing
//! a [`NormalizedTree`] with platform-agnostic roles via [`uia_control_type_to_role`].
//!
//! # Implementation Notes
//! - Uses `uia_control_type_to_role()` from `types.rs` for direct int → NormalizedRole mapping.
//! - Bounds from UIA are already in top-left origin — no coordinate flip needed.
//! - `accelerator_key` and `access_key` are combined into the single `shortcut` field.
//! - UIA states (enabled, offscreen) are mapped to `ElementState` enum values.
//! - Depth limit: 8, element cap: 500 (matching original uia_capture.rs).

use super::types::*;
use windows::Win32::UI::Accessibility::*;
use windows::Win32::System::Com::*;
use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

/// Capture the UIA tree from the foreground window and return as NormalizedTree.
pub fn capture() -> Result<NormalizedTree, String> {
    // Initialize COM on this thread
    unsafe {
        CoInitializeEx(Some(std::ptr::null()), COINIT_MULTITHREADED)
            .ok()
            .map_err(|e| format!("COM init failed: {}", e))?;
    }

    let result: Result<NormalizedTree, String> = (|| {
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
        let mut id_counter: u64 = 0;
        let elements = walk_tree(&walker, &root, 0, 8, &mut element_count, 500, &mut id_counter);

        Ok(NormalizedTree {
            window_title,
            process_name,
            platform: "windows".to_string(),
            element_count,
            elements,
        })
    })();

    unsafe { CoUninitialize() };
    result
}

fn walk_tree(
    walker: &IUIAutomationTreeWalker,
    parent: &IUIAutomationElement,
    depth: u32,
    max_depth: u32,
    count: &mut usize,
    max_elements: usize,
    id_counter: &mut u64,
) -> Vec<NormalizedElement> {
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

        let mut element = read_element(&current, depth, id_counter);

        let dominated = element.name.is_empty()
            && element.value.is_none()
            && !element.role.is_interactive();

        element.children = walk_tree(walker, &current, depth + 1, max_depth, count, max_elements, id_counter);
        element.children_count = element.children.len() as u32;

        if !dominated || !element.children.is_empty() {
            *count += 1;
            elements.push(element);
        }

        match unsafe { walker.GetNextSiblingElement(&current) } {
            Ok(next) => current = next,
            Err(_) => break,
        }
    }

    elements
}

#[allow(non_upper_case_globals)] // Windows SDK constants use mixed case
fn read_element(
    el: &IUIAutomationElement,
    depth: u32,
    id_counter: &mut u64,
) -> NormalizedElement {
    *id_counter += 1;
    let id = *id_counter;

    let name = unsafe { el.CurrentName().map(|s| s.to_string()).unwrap_or_default() };

    let control_type_id = unsafe {
        el.CurrentControlType().unwrap_or(UIA_CONTROLTYPE_ID(0))
    };
    let role = uia_control_type_to_role(control_type_id.0);

    let value = get_value(el);

    let bounds = unsafe {
        el.CurrentBoundingRectangle()
            .map(|r| {
                let w = r.right - r.left;
                let h = r.bottom - r.top;
                if w > 0 && h > 0 {
                    Some(Rect {
                        x: r.left,
                        y: r.top,
                        width: w as u32,
                        height: h as u32,
                    })
                } else {
                    None
                }
            })
            .unwrap_or(None)
    };

    let mut states = Vec::new();
    unsafe {
        if let Ok(enabled) = el.CurrentIsEnabled() {
            if !enabled.as_bool() {
                states.push(ElementState::Disabled);
            }
        }
        if let Ok(offscreen) = el.CurrentIsOffscreen() {
            if offscreen.as_bool() {
                states.push(ElementState::Offscreen);
            }
        }
        // Expand/collapse state (tree items, menus, combo boxes)
        if let Ok(pattern) = el.GetCurrentPatternAs::<IUIAutomationExpandCollapsePattern>(UIA_ExpandCollapsePatternId) {
            if let Ok(state) = pattern.CurrentExpandCollapseState() {
                match state {
                    ExpandCollapseState_Expanded => states.push(ElementState::Expanded),
                    ExpandCollapseState_Collapsed => states.push(ElementState::Collapsed),
                    _ => {}
                }
            }
        }
        // Toggle state → Checked/Unchecked/Indeterminate (checkboxes, toggle buttons)
        if let Ok(pattern) = el.GetCurrentPatternAs::<IUIAutomationTogglePattern>(UIA_TogglePatternId) {
            if let Ok(state) = pattern.CurrentToggleState() {
                match state {
                    ToggleState_On => states.push(ElementState::Checked),
                    ToggleState_Off => states.push(ElementState::Unchecked),
                    _ => states.push(ElementState::Indeterminate),
                }
            }
        }
        // Selection state (list items, tree items)
        if let Ok(pattern) = el.GetCurrentPatternAs::<IUIAutomationSelectionItemPattern>(UIA_SelectionItemPatternId) {
            if let Ok(selected) = pattern.CurrentIsSelected() {
                if selected.as_bool() {
                    states.push(ElementState::Selected);
                }
            }
        }
        // ReadOnly state from ValuePattern (text inputs, text areas)
        if let Ok(pattern) = el.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId) {
            if let Ok(readonly) = pattern.CurrentIsReadOnly() {
                if readonly.as_bool() {
                    states.push(ElementState::ReadOnly);
                }
            }
        }
    }

    // Combine accelerator_key and access_key into shortcut
    let accelerator_key = unsafe {
        el.CurrentAcceleratorKey().map(|s| s.to_string()).unwrap_or_default()
    };
    let access_key = unsafe {
        el.CurrentAccessKey().map(|s| s.to_string()).unwrap_or_default()
    };
    let shortcut = if !accelerator_key.is_empty() {
        Some(accelerator_key)
    } else if !access_key.is_empty() {
        Some(access_key)
    } else {
        None
    };

    NormalizedElement {
        id,
        name,
        role,
        value,
        bounds,
        states,
        shortcut,
        depth,
        children_count: 0, // Set after children are walked
        children: Vec::new(),
    }
}

#[allow(non_upper_case_globals)] // Windows SDK constants use mixed case (ToggleState_On, etc.)
fn get_value(el: &IUIAutomationElement) -> Option<String> {
    unsafe {
        // Value pattern (text inputs, etc.)
        if let Ok(pattern) = el.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId) {
            if let Ok(val) = pattern.CurrentValue() {
                let s = val.to_string();
                if !s.is_empty() {
                    if s.len() > 200 {
                        return Some(format!("{}...", &s[..200]));
                    }
                    return Some(s);
                }
            }
        }
        // Toggle pattern (checkboxes, toggles)
        if let Ok(pattern) = el.GetCurrentPatternAs::<IUIAutomationTogglePattern>(UIA_TogglePatternId) {
            if let Ok(state) = pattern.CurrentToggleState() {
                return Some(match state {
                    ToggleState_On => "checked".to_string(),
                    ToggleState_Off => "unchecked".to_string(),
                    _ => "indeterminate".to_string(),
                });
            }
        }
        // Selection pattern (lists, combo boxes)
        if let Ok(pattern) = el.GetCurrentPatternAs::<IUIAutomationSelectionPattern>(UIA_SelectionPatternId) {
            if let Ok(selection) = pattern.GetCurrentSelection() {
                if let Ok(len) = selection.Length() {
                    if len > 0 {
                        if let Ok(item) = selection.GetElement(0) {
                            if let Ok(name) = item.CurrentName() {
                                return Some(name.to_string());
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

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
                let success = QueryFullProcessImageNameW(
                    h,
                    PROCESS_NAME_WIN32,
                    windows::core::PWSTR(buf.as_mut_ptr()),
                    &mut size,
                );
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
