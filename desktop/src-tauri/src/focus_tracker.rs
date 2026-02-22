//! Focus tracking module — polls UIA focused element and
//! emits "speak-immediate" Tauri events for instant TTS announcements.
//! Zero API calls, pure local.

use std::sync::atomic::{AtomicBool, Ordering};

static TRACKING_ACTIVE: AtomicBool = AtomicBool::new(false);

#[cfg(target_os = "windows")]
pub fn start_focus_tracking(app: tauri::AppHandle) {
    use tauri::Manager;

    if TRACKING_ACTIVE.swap(true, Ordering::SeqCst) {
        return;
    }

    let handle = app.clone();

    std::thread::spawn(move || {
        use windows::Win32::System::Com::*;
        use windows::Win32::UI::Accessibility::*;
        use tauri::{Emitter, Manager};

        unsafe {
            if CoInitializeEx(Some(std::ptr::null()), COINIT_MULTITHREADED).is_err() {
                TRACKING_ACTIVE.store(false, Ordering::SeqCst);
                return;
            }
        }

        let uia: IUIAutomation = match unsafe {
            CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)
        } {
            Ok(u) => u,
            Err(e) => {
                eprintln!("Focus tracker: UIA create failed: {}", e);
                TRACKING_ACTIVE.store(false, Ordering::SeqCst);
                unsafe { CoUninitialize() };
                return;
            }
        };

        let mut last_announcement = String::new();

        while TRACKING_ACTIVE.load(Ordering::SeqCst) {
            if let Ok(focused) = unsafe { uia.GetFocusedElement() } {
                let name = unsafe {
                    focused.CurrentName().map(|s| s.to_string()).unwrap_or_default()
                };
                let control_type_id = unsafe {
                    focused.CurrentControlType().unwrap_or(UIA_CONTROLTYPE_ID(0))
                };
                let role = crate::a11y::types::uia_control_type_to_role(control_type_id.0)
                    .as_str()
                    .to_string();

                let value = unsafe {
                    focused.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId)
                        .ok()
                        .and_then(|p| p.CurrentValue().ok())
                        .map(|v| v.to_string())
                        .unwrap_or_default()
                };

                let announcement = build_announcement(&name, &role, &value);

                if !announcement.is_empty() && announcement != last_announcement {
                    last_announcement = announcement.clone();

                    let ts = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_millis() as u64)
                        .unwrap_or(0);

                    let payload = serde_json::json!({
                        "text": announcement,
                        "session_id": "focus-tracker",
                        "timestamp": ts,
                        "priority": "immediate",
                    });

                    if let Some(window) = handle.get_webview_window("main") {
                        let _ = window.emit("speak-immediate", &payload);
                    }
                }
            }

            std::thread::sleep(std::time::Duration::from_millis(100));
        }

        unsafe { CoUninitialize() };
        TRACKING_ACTIVE.store(false, Ordering::SeqCst);
    });
}

pub fn stop_focus_tracking() {
    TRACKING_ACTIVE.store(false, Ordering::SeqCst);
}

fn build_announcement(name: &str, role: &str, value: &str) -> String {
    let mut parts = Vec::new();

    if !name.is_empty() {
        parts.push(name.to_string());
    }

    // Role strings now come from NormalizedRole::as_str() — use normalized names
    let friendly_role = match role {
        "TextInput" => "edit field",
        "TextArea" => "text area",
        "Button" => "button",
        "Checkbox" => "checkbox",
        "RadioButton" => "radio button",
        "ComboBox" => "combo box",
        "Tab" => "tab",
        "TabItem" => "tab",
        "MenuItem" => "menu item",
        "Link" => "link",
        "ListItem" => "list item",
        "TreeItem" => "tree item",
        "Slider" => "slider",
        "Spinner" => "spin button",
        _ => "",
    };

    if !friendly_role.is_empty() {
        parts.push(friendly_role.to_string());
    }

    if !value.is_empty() {
        let display_value = if value.len() > 50 {
            format!("{}...", &value[..50])
        } else {
            value.to_string()
        };
        parts.push(display_value);
    } else if role == "TextInput" {
        parts.push("empty".to_string());
    }

    parts.join(", ")
}

#[cfg(not(target_os = "windows"))]
pub fn start_focus_tracking(_app: tauri::AppHandle) {
    eprintln!("Focus tracking is only available on Windows");
}
