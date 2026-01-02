// Prevents additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use enigo::{Enigo, Keyboard, Settings};
use std::thread;
use std::time::Duration;
use tauri::{
    image::Image,
    menu::{MenuBuilder, MenuItemBuilder},
    tray::TrayIconBuilder,
    Manager,
};

/// Helper to create an Image from PNG bytes
fn load_png_image(png_bytes: &[u8]) -> Result<Image<'static>, String> {
    // Decode PNG to RGBA
    let img = image::load_from_memory(png_bytes)
        .map_err(|e| format!("Failed to decode PNG: {}", e))?;
    let rgba = img.to_rgba8();
    let (width, height) = rgba.dimensions();
    Ok(Image::new_owned(rgba.into_raw(), width, height))
}

use std::fs::OpenOptions;
use std::io::Write;

/// Log errors to a file for debugging launch issues
fn log_error(message: &str) {
    // Get home directory based on platform
    let home_var = if cfg!(target_os = "windows") {
        "USERPROFILE"
    } else {
        "HOME"
    };

    if let Some(home) = std::env::var_os(home_var) {
        let log_path = std::path::PathBuf::from(home).join("scribe-error.log");
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&log_path) {
            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let _ = writeln!(file, "[{}] {}", timestamp, message);
        }
    }

    // Also print to stderr for debugging
    eprintln!("Scribe: {}", message);
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

/// Update tray icon to show recording state
#[tauri::command]
fn set_recording_state(app: tauri::AppHandle, recording: bool) -> Result<(), String> {
    if let Some(tray) = app.tray_by_id("main-tray") {
        let icon_bytes: &[u8] = if recording {
            include_bytes!("../icons/tray-recording.png")
        } else {
            include_bytes!("../icons/tray-idle.png")
        };

        if let Ok(icon) = load_png_image(icon_bytes) {
            let _ = tray.set_icon(Some(icon));
        }

        let tooltip = if recording {
            "Scribe - Recording..."
        } else {
            "Scribe - Ready"
        };
        let _ = tray.set_tooltip(Some(tooltip));
    }
    Ok(())
}

fn main() {
    // Log startup attempt for debugging
    log_error("Scribe starting...");

    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_clipboard_manager::init())
        .setup(|app| {
            // Create tray menu
            let show_item = MenuItemBuilder::with_id("show", "Show Scribe").build(app)?;
            let quit_item = MenuItemBuilder::with_id("quit", "Quit").build(app)?;
            let menu = MenuBuilder::new(app)
                .item(&show_item)
                .separator()
                .item(&quit_item)
                .build()?;

            // Create tray icon - use the app icon initially
            let icon = load_png_image(include_bytes!("../icons/tray-idle.png"))
                .unwrap_or_else(|_| {
                    // Fallback to 32x32 icon
                    load_png_image(include_bytes!("../icons/32x32.png"))
                        .expect("Failed to load tray icon")
                });

            let _tray = TrayIconBuilder::with_id("main-tray")
                .icon(icon)
                .tooltip("Scribe - Ready")
                .menu(&menu)
                .on_menu_event(move |app, event| {
                    match event.id().as_ref() {
                        "show" => {
                            if let Some(window) = app.get_webview_window("main") {
                                let _ = window.show();
                                let _ = window.set_focus();
                            }
                        }
                        "quit" => {
                            app.exit(0);
                        }
                        _ => {}
                    }
                })
                .on_tray_icon_event(|tray, event| {
                    if let tauri::tray::TrayIconEvent::Click { .. } = event {
                        let app = tray.app_handle();
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                })
                .build(app)?;

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![simulate_paste, type_text, set_recording_state]);

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
