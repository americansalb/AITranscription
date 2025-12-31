// Prevents additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use enigo::{Enigo, Keyboard, Settings};
use std::thread;
use std::time::Duration;

/// Simulate a paste keyboard shortcut (Ctrl+V on Windows/Linux, Cmd+V on macOS)
#[tauri::command]
fn simulate_paste() -> Result<(), String> {
    // Small delay to let the app lose focus and target app gain it
    thread::sleep(Duration::from_millis(100));

    let mut enigo = Enigo::new(&Settings::default()).map_err(|e| e.to_string())?;

    // Use the appropriate modifier key based on platform
    #[cfg(target_os = "macos")]
    {
        use enigo::Key;
        enigo
            .key(Key::Meta, enigo::Direction::Press)
            .map_err(|e| e.to_string())?;
        enigo
            .key(Key::Unicode('v'), enigo::Direction::Click)
            .map_err(|e| e.to_string())?;
        enigo
            .key(Key::Meta, enigo::Direction::Release)
            .map_err(|e| e.to_string())?;
    }

    #[cfg(not(target_os = "macos"))]
    {
        use enigo::Key;
        enigo
            .key(Key::Control, enigo::Direction::Press)
            .map_err(|e| e.to_string())?;
        enigo
            .key(Key::Unicode('v'), enigo::Direction::Click)
            .map_err(|e| e.to_string())?;
        enigo
            .key(Key::Control, enigo::Direction::Release)
            .map_err(|e| e.to_string())?;
    }

    Ok(())
}

/// Type text directly using keyboard simulation
/// This is an alternative to paste for applications that don't support clipboard
#[tauri::command]
fn type_text(text: String) -> Result<(), String> {
    thread::sleep(Duration::from_millis(100));

    let mut enigo = Enigo::new(&Settings::default()).map_err(|e| e.to_string())?;
    enigo.text(&text).map_err(|e| e.to_string())?;

    Ok(())
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_clipboard_manager::init())
        .invoke_handler(tauri::generate_handler![simulate_paste, type_text])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
