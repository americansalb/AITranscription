//! Placeholder for native keyboard hook (currently unused).
//! The Tauri global shortcut plugin handles hotkey registration.

#[cfg(target_os = "windows")]
pub fn start_hook(_app: tauri::AppHandle, _hotkey: &str) {}

#[cfg(target_os = "windows")]
pub fn update_hotkey(_hotkey: &str) {}

#[cfg(not(target_os = "windows"))]
pub fn start_hook(_app: tauri::AppHandle, _hotkey: &str) {}

#[cfg(not(target_os = "windows"))]
pub fn update_hotkey(_hotkey: &str) {}
