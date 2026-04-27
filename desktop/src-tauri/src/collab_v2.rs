// Collaborate v2 — P1 scope.
// Greenfield module per Ground Rule 1 in COLLABORATE_V2_SPEC.html §2.
// P1 surface: open/toggle/hide the standalone window + read .vaak/v2/seats.json for
// the static roster. No wire, no gating, no claim logic — those land in P2/P3.

use std::path::PathBuf;
use tauri::Manager;

fn v2_dir(project_dir: &str) -> PathBuf {
    PathBuf::from(project_dir).join(".vaak").join("v2")
}

#[tauri::command]
pub fn show_collaborate_v2_window(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("collaborate-v2") {
        window.show().map_err(|e| e.to_string())?;
        window.set_focus().map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub fn toggle_collaborate_v2_window(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("collaborate-v2") {
        match window.is_visible() {
            Ok(true) => {
                window.hide().map_err(|e| e.to_string())?;
            }
            _ => {
                window.show().map_err(|e| e.to_string())?;
                window.set_focus().map_err(|e| e.to_string())?;
            }
        }
    }
    Ok(())
}

#[tauri::command]
pub fn hide_collaborate_v2_window(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("collaborate-v2") {
        window.hide().map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Read .vaak/v2/seats.json as a raw JSON value.
/// P1 returns `{"schema_version":1,"seats":[]}` when the file doesn't exist,
/// matching §18 P1 "static roster with static state" — an empty roster is a
/// valid shipping state for the shell.
#[tauri::command]
pub fn get_v2_seats(project_dir: String) -> Result<serde_json::Value, String> {
    let path = v2_dir(&project_dir).join("seats.json");
    match std::fs::read_to_string(&path) {
        Ok(content) => serde_json::from_str(&content)
            .map_err(|e| format!("Invalid .vaak/v2/seats.json: {}", e)),
        Err(_) => Ok(serde_json::json!({ "schema_version": 1, "seats": [] })),
    }
}
