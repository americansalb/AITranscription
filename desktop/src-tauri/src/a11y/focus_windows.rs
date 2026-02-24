//! Windows UIA focus tracking.
//!
//! Spawns a dedicated thread that polls UIA GetFocusedElement every 100ms,
//! extracts element name + role via NormalizedRole mapping, and emits
//! "speak-immediate" Tauri events for TTS announcements.
//! Deduplicates: only announces when the focused element changes.

use std::sync::atomic::{AtomicBool, Ordering};

static TRACKING_ACTIVE: AtomicBool = AtomicBool::new(false);

fn build_announcement(name: &str, role: &str, value: &str) -> String {
    let mut parts = Vec::new();
    if !name.is_empty() {
        parts.push(name.to_string());
    }
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
        let display_value = if value.chars().count() > 50 {
            let truncated: String = value.chars().take(50).collect();
            format!("{}...", truncated)
        } else {
            value.to_string()
        };
        parts.push(display_value);
    } else if role == "TextInput" {
        parts.push("empty".to_string());
    }
    parts.join(", ")
}

/// Start focus tracking on Windows via UIA polling.
pub fn start(app: tauri::AppHandle) {
    if TRACKING_ACTIVE.swap(true, Ordering::SeqCst) {
        return;
    }

    let handle = app.clone();

    std::thread::spawn(move || {
        #[cfg(target_os = "windows")]
        {
            use windows::Win32::System::Com::*;
            use windows::Win32::UI::Accessibility::*;
            use tauri::{Emitter, Manager};

            unsafe {
                if CoInitializeEx(Some(std::ptr::null()), COINIT_APARTMENTTHREADED).is_err() {
                    TRACKING_ACTIVE.store(false, Ordering::SeqCst);
                    return;
                }
            }

            let uia: IUIAutomation = match unsafe {
                CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)
            } {
                Ok(u) => u,
                Err(e) => {
                    eprintln!("[a11y/focus_windows] UIA create failed: {}", e);
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
        }

        TRACKING_ACTIVE.store(false, Ordering::SeqCst);
    });
}

/// Stop focus tracking.
pub fn stop() {
    TRACKING_ACTIVE.store(false, Ordering::SeqCst);
}

/// Returns true if focus tracking is active.
pub fn is_active() -> bool {
    TRACKING_ACTIVE.load(Ordering::SeqCst)
}
