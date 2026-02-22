//! macOS Accessibility API tree capture → NormalizedTree.
//!
//! Uses AXUIElement API to capture the foreground window's accessibility tree.
//! Maps AX roles via `ax_role_to_normalized()` from types.rs.
//!
//! # Coordinate Convention
//! Apple docs indicate AXPosition uses top-left origin (screen coordinates).
//! No coordinate flip is applied. This MUST be verified on physical Mac hardware.
//! If AX actually uses bottom-left origin: `y = screen_height - ax_y - height`

use super::types::*;

#[cfg(target_os = "macos")]
mod ax {
    use core_foundation::array::CFArray;
    use core_foundation::base::{CFType, TCFType};
    use core_foundation::boolean::CFBoolean;
    use core_foundation::number::CFNumber;
    use core_foundation::string::{CFString, CFStringRef};
    use core_graphics::display::CGMainDisplayID;
    use std::ffi::c_void;
    use std::ptr;

    use super::*;

    // AX API FFI declarations
    type AXUIElementRef = *const c_void;
    type AXError = i32;
    const K_AX_ERROR_SUCCESS: AXError = 0;

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXUIElementCreateApplication(pid: i32) -> AXUIElementRef;
        fn AXUIElementCopyAttributeValue(
            element: AXUIElementRef,
            attribute: CFStringRef,
            value: *mut *const c_void,
        ) -> AXError;
    }

    extern "C" {
        fn CGDisplayPixelsHigh(display: u32) -> u64;
    }

    // Core Foundation memory management
    extern "C" {
        fn CFRelease(cf: *const c_void);
        fn CFRetain(cf: *const c_void) -> *const c_void;
    }

    // NSWorkspace FFI for frontmost app PID
    #[link(name = "AppKit", kind = "framework")]
    extern "C" {}

    fn get_frontmost_pid() -> Option<i32> {
        use std::process::Command;
        // Use osascript to get frontmost app PID — reliable and no extra deps
        let output = Command::new("osascript")
            .args(["-e", "tell application \"System Events\" to unix id of first process whose frontmost is true"])
            .output()
            .ok()?;
        let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
        s.parse::<i32>().ok()
    }

    fn get_ax_attribute(element: AXUIElementRef, attr: &str) -> Option<*const c_void> {
        let cf_attr = CFString::new(attr);
        let mut value: *const c_void = ptr::null();
        let err = unsafe {
            AXUIElementCopyAttributeValue(element, cf_attr.as_concrete_TypeRef(), &mut value)
        };
        if err == K_AX_ERROR_SUCCESS && !value.is_null() {
            Some(value)
        } else {
            None
        }
    }

    fn get_ax_string(element: AXUIElementRef, attr: &str) -> String {
        if let Some(val) = get_ax_attribute(element, attr) {
            let cf_str: CFString = unsafe { TCFType::wrap_under_get_rule(val as CFStringRef) };
            let result = cf_str.to_string();
            unsafe { CFRelease(val) };
            result
        } else {
            String::new()
        }
    }

    fn get_ax_bool(element: AXUIElementRef, attr: &str) -> Option<bool> {
        if let Some(val) = get_ax_attribute(element, attr) {
            let cf_bool: CFBoolean = unsafe { TCFType::wrap_under_get_rule(val as *const _) };
            let result = cf_bool == CFBoolean::true_value();
            unsafe { CFRelease(val) };
            Some(result)
        } else {
            None
        }
    }

    fn get_ax_position(element: AXUIElementRef) -> Option<(f64, f64)> {
        if let Some(val) = get_ax_attribute(element, "AXPosition") {
            let mut point = core_graphics::geometry::CGPoint::new(0.0, 0.0);
            let ok = unsafe {
                AXValueGetValue(
                    val as *const c_void,
                    1, // kAXValueCGPointType
                    &mut point as *mut _ as *mut c_void,
                )
            };
            unsafe { CFRelease(val) };
            if ok {
                Some((point.x, point.y))
            } else {
                None
            }
        } else {
            None
        }
    }

    fn get_ax_size(element: AXUIElementRef) -> Option<(f64, f64)> {
        if let Some(val) = get_ax_attribute(element, "AXSize") {
            let mut size = core_graphics::geometry::CGSize::new(0.0, 0.0);
            let ok = unsafe {
                AXValueGetValue(
                    val as *const c_void,
                    2, // kAXValueCGSizeType
                    &mut size as *mut _ as *mut c_void,
                )
            };
            unsafe { CFRelease(val) };
            if ok {
                Some((size.width, size.height))
            } else {
                None
            }
        } else {
            None
        }
    }

    extern "C" {
        fn AXValueGetValue(value: *const c_void, value_type: u32, out: *mut c_void) -> bool;
    }

    fn get_screen_height() -> i32 {
        unsafe { CGDisplayPixelsHigh(CGMainDisplayID()) as i32 }
    }

    fn get_ax_children(element: AXUIElementRef) -> Vec<AXUIElementRef> {
        if let Some(val) = get_ax_attribute(element, "AXChildren") {
            // val is a CFArrayRef of AXUIElementRefs
            let arr: CFArray<CFType> = unsafe { TCFType::wrap_under_get_rule(val as *const _) };
            let count = arr.len();
            let mut children = Vec::with_capacity(count as usize);
            for i in 0..count {
                let child = arr.get(i).unwrap();
                let child_ref = child.as_CFTypeRef() as AXUIElementRef;
                unsafe { CFRetain(child_ref) };
                children.push(child_ref);
            }
            unsafe { CFRelease(val) };
            children
        } else {
            Vec::new()
        }
    }

    extern "C" {
        fn CFGetTypeID(cf: *const c_void) -> u64;
        fn CFStringGetTypeID() -> u64;
        fn CFNumberGetTypeID() -> u64;
        fn CFBooleanGetTypeID() -> u64;
    }

    fn get_value_string(element: AXUIElementRef) -> Option<String> {
        if let Some(val) = get_ax_attribute(element, "AXValue") {
            let type_id = unsafe { CFGetTypeID(val) };
            let result = unsafe {
                if type_id == CFStringGetTypeID() {
                    let cf_str: CFString = TCFType::wrap_under_get_rule(val as CFStringRef);
                    let s = cf_str.to_string();
                    if s.is_empty() { None }
                    else if s.len() > 200 { Some(format!("{}...", &s[..200])) }
                    else { Some(s) }
                } else if type_id == CFNumberGetTypeID() {
                    let cf_num: CFNumber = TCFType::wrap_under_get_rule(val as *const _);
                    cf_num.to_f64().map(|n| format!("{}", n))
                } else if type_id == CFBooleanGetTypeID() {
                    let cf_bool: CFBoolean = TCFType::wrap_under_get_rule(val as *const _);
                    Some(if cf_bool == CFBoolean::true_value() { "true" } else { "false" }.to_string())
                } else {
                    None // Unknown CF type — skip
                }
            };
            unsafe { CFRelease(val) };
            result
        } else {
            None
        }
    }

    fn read_element(
        element: AXUIElementRef,
        depth: u32,
        id_counter: &mut u64,
        screen_height: i32,
    ) -> NormalizedElement {
        *id_counter += 1;
        let id = *id_counter;

        // Name: prefer AXTitle, fall back to AXDescription
        let title = get_ax_string(element, "AXTitle");
        let name = if !title.is_empty() {
            title
        } else {
            get_ax_string(element, "AXDescription")
        };

        // Role
        let role_str = get_ax_string(element, "AXRole");
        let subrole = get_ax_string(element, "AXSubrole");
        let role = if !subrole.is_empty() {
            // Try subrole first for more specific mapping
            let sr = ax_role_to_normalized(&subrole);
            if matches!(sr, NormalizedRole::Unknown(_)) {
                ax_role_to_normalized(&role_str)
            } else {
                sr
            }
        } else {
            ax_role_to_normalized(&role_str)
        };

        // Value
        let value = get_value_string(element);

        // Bounds — pass through directly (AXPosition reportedly uses top-left origin)
        // TODO: Verify on physical Mac. If AX uses bottom-left origin, apply:
        //   y = screen_height - y - height
        let _ = screen_height; // suppress unused warning until verification
        let bounds = match (get_ax_position(element), get_ax_size(element)) {
            (Some((x, y)), Some((w, h))) if w > 0.0 && h > 0.0 => {
                Some(Rect {
                    x: x as i32,
                    y: y as i32,
                    width: w as u32,
                    height: h as u32,
                })
            }
            _ => None,
        };

        // States
        let mut states = Vec::new();
        if let Some(enabled) = get_ax_bool(element, "AXEnabled") {
            if !enabled {
                states.push(ElementState::Disabled);
            }
        }
        if let Some(focused) = get_ax_bool(element, "AXFocused") {
            if focused {
                states.push(ElementState::Focused);
            }
        }
        if let Some(selected) = get_ax_bool(element, "AXSelected") {
            if selected {
                states.push(ElementState::Selected);
            }
        }
        if let Some(expanded) = get_ax_bool(element, "AXExpanded") {
            if expanded {
                states.push(ElementState::Expanded);
            } else {
                states.push(ElementState::Collapsed);
            }
        }

        // Shortcut
        let shortcut_str = get_ax_string(element, "AXMenuItemCmdChar");
        let shortcut = if !shortcut_str.is_empty() {
            let modifiers = get_ax_string(element, "AXMenuItemCmdModifiers");
            if !modifiers.is_empty() {
                Some(format!("{}+{}", modifiers, shortcut_str))
            } else {
                Some(format!("Cmd+{}", shortcut_str))
            }
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
            children_count: 0,
            children: Vec::new(),
        }
    }

    fn walk_tree(
        element: AXUIElementRef,
        depth: u32,
        max_depth: u32,
        count: &mut usize,
        max_elements: usize,
        id_counter: &mut u64,
        screen_height: i32,
    ) -> Vec<NormalizedElement> {
        if depth >= max_depth || *count >= max_elements {
            return Vec::new();
        }

        let children_refs = get_ax_children(element);
        let mut elements = Vec::new();

        for child_ref in &children_refs {
            if *count >= max_elements {
                break;
            }

            let mut el = read_element(*child_ref, depth, id_counter, screen_height);

            let dominated = el.name.is_empty()
                && el.value.is_none()
                && !el.role.is_interactive();

            el.children = walk_tree(
                *child_ref,
                depth + 1,
                max_depth,
                count,
                max_elements,
                id_counter,
                screen_height,
            );
            el.children_count = el.children.len() as u32;

            if !dominated || !el.children.is_empty() {
                *count += 1;
                elements.push(el);
            }
        }

        // Release retained child refs
        for child_ref in &children_refs {
            unsafe { CFRelease(*child_ref) };
        }

        elements
    }

    fn get_process_name_from_pid(pid: i32) -> String {
        use std::process::Command;
        Command::new("ps")
            .args(["-p", &pid.to_string(), "-o", "comm="])
            .output()
            .ok()
            .and_then(|o| {
                let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
                s.rsplit('/').next().map(|n| n.to_string())
            })
            .unwrap_or_default()
    }

    pub fn capture() -> Result<NormalizedTree, String> {
        let pid = get_frontmost_pid()
            .ok_or_else(|| "Could not determine frontmost application".to_string())?;

        let app_element = unsafe { AXUIElementCreateApplication(pid) };
        if app_element.is_null() {
            return Err("Failed to create AXUIElement for application".to_string());
        }

        // Get focused window — release app_element on failure
        let window = match get_ax_attribute(app_element, "AXFocusedWindow") {
            Some(w) => w,
            None => {
                unsafe { CFRelease(app_element) };
                return Err("No focused window found".to_string());
            }
        };

        let window_title = get_ax_string(window as AXUIElementRef, "AXTitle");
        let process_name = get_process_name_from_pid(pid);
        let screen_height = get_screen_height();

        let mut element_count = 0usize;
        let mut id_counter: u64 = 0;
        let elements = walk_tree(
            window as AXUIElementRef,
            0,
            8,
            &mut element_count,
            500,
            &mut id_counter,
            screen_height,
        );

        // Release AX refs
        unsafe {
            CFRelease(window);
            CFRelease(app_element);
        }

        Ok(NormalizedTree {
            window_title,
            process_name,
            platform: "macos".to_string(),
            element_count,
            elements,
        })
    }
}

/// Capture the accessibility tree from the foreground window on macOS.
#[cfg(target_os = "macos")]
pub fn capture() -> Result<NormalizedTree, String> {
    ax::capture()
}

#[cfg(not(target_os = "macos"))]
pub fn capture() -> Result<NormalizedTree, String> {
    Ok(NormalizedTree {
        window_title: String::new(),
        process_name: String::new(),
        platform: "macos".to_string(),
        element_count: 0,
        elements: Vec::new(),
    })
}
