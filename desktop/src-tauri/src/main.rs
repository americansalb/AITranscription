// Prevents additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use enigo::{Enigo, Keyboard, Settings};
use std::thread;
use std::time::Duration;

#[cfg(target_os = "windows")]
use std::fs::OpenOptions;
#[cfg(target_os = "windows")]
use std::io::Write;

/// Log errors to a file on Windows for debugging launch issues
#[cfg(target_os = "windows")]
fn log_error(message: &str) {
    if let Some(home) = std::env::var_os("USERPROFILE") {
        let log_path = std::path::PathBuf::from(home).join("scribe-error.log");
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&log_path) {
            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let _ = writeln!(file, "[{}] {}", timestamp, message);
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn log_error(message: &str) {
    eprintln!("Scribe Error: {}", message);
}

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
    // Log startup attempt for debugging
    log_error("Scribe starting...");

    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_clipboard_manager::init())
        .invoke_handler(tauri::generate_handler![simulate_paste, type_text]);

    match builder.run(tauri::generate_context!()) {
        Ok(_) => {}
        Err(e) => {
            let error_msg = format!("Failed to run Tauri application: {}", e);
            log_error(&error_msg);

            // On Windows, also show a message box so user sees the error
            #[cfg(target_os = "windows")]
            {
                use std::ptr::null_mut;
                let msg = format!(
                    "Scribe failed to start:\n\n{}\n\nCheck ~/scribe-error.log for details.",
                    e
                );
                let wide_msg: Vec<u16> = msg.encode_utf16().chain(std::iter::once(0)).collect();
                let wide_title: Vec<u16> = "Scribe Error"
                    .encode_utf16()
                    .chain(std::iter::once(0))
                    .collect();
                unsafe {
                    #[link(name = "user32")]
                    extern "system" {
                        fn MessageBoxW(
                            hwnd: *mut std::ffi::c_void,
                            text: *const u16,
                            caption: *const u16,
                            utype: u32,
                        ) -> i32;
                    }
                    MessageBoxW(null_mut(), wide_msg.as_ptr(), wide_title.as_ptr(), 0x10);
                }
            }

            std::process::exit(1);
        }
    }
}
