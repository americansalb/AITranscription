// Collaborate v2 — P1 scope.
// Greenfield module per Ground Rule 1 in COLLABORATE_V2_SPEC.html §2.
// P1 surface: open/toggle/hide the standalone window + read .vaak/v2/seats.json for
// the static roster. No wire, no gating, no claim logic — those land in P2/P3.

use std::path::PathBuf;
use tauri::Manager;

fn v2_dir(project_dir: &str) -> PathBuf {
    PathBuf::from(project_dir).join(".vaak").join("v2")
}

/// PERF (human msg 569 "why is vaak so memory-intensive for a text UI"):
/// the collaborate-v2 window used to be declared in tauri.conf.json's eager
/// `app.windows` list, so Tauri spawned its full WebView2 (a whole Chromium
/// instance, ~150-300MB) at STARTUP even though it's an on-demand P1
/// experiment the user only opens via the launcher button. Removing it from
/// the eager list and creating it lazily here on first show drops that cost
/// from boot. The close→hide handler (previously attached in main.rs setup())
/// now lives at creation time, so closing still hides (persists, per
/// COLLABORATE_V2_SPEC §20) instead of destroying.
fn ensure_collaborate_v2_window(
    app: &tauri::AppHandle,
) -> Result<tauri::WebviewWindow, String> {
    if let Some(window) = app.get_webview_window("collaborate-v2") {
        return Ok(window);
    }
    let window = tauri::WebviewWindowBuilder::new(
        app,
        "collaborate-v2",
        tauri::WebviewUrl::App("index.html#/collaborate-v2".into()),
    )
    .title("Collaborate")
    .inner_size(1100.0, 720.0)
    .min_inner_size(800.0, 500.0)
    .resizable(true)
    .center()
    .decorations(true)
    .visible(false)
    .build()
    .map_err(|e| e.to_string())?;
    // Close → hide (don't destroy) so the launcher can reopen it and window
    // state persists within the session (spec §20). Mirrors the handler that
    // previously lived in main.rs setup() for the eagerly-created window.
    let clone = window.clone();
    window.on_window_event(move |event| {
        if let tauri::WindowEvent::CloseRequested { api, .. } = event {
            api.prevent_close();
            let _ = clone.hide();
        }
    });
    Ok(window)
}

#[tauri::command]
pub fn show_collaborate_v2_window(app: tauri::AppHandle) -> Result<(), String> {
    let window = ensure_collaborate_v2_window(&app)?;
    window.show().map_err(|e| e.to_string())?;
    window.set_focus().map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn toggle_collaborate_v2_window(app: tauri::AppHandle) -> Result<(), String> {
    let window = ensure_collaborate_v2_window(&app)?;
    match window.is_visible() {
        Ok(true) => {
            window.hide().map_err(|e| e.to_string())?;
        }
        _ => {
            window.show().map_err(|e| e.to_string())?;
            window.set_focus().map_err(|e| e.to_string())?;
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
