// Prevents additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio;

use audio::{AudioData, AudioDevice, AudioRecorder};
use enigo::{Enigo, Keyboard, Settings};
use parking_lot::Mutex;
use std::io::Read;
use std::thread;
use std::time::Duration;
use tauri::{
    image::Image,
    menu::{MenuBuilder, MenuItemBuilder},
    tray::TrayIconBuilder,
    Emitter, Manager,
};
use tiny_http::{Response, Server};

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
///
/// Note: Focus switching timing is handled by the JavaScript caller.
/// This function sends the paste keys with logging for debugging.
#[tauri::command]
fn simulate_paste() -> Result<(), String> {
    log_error("simulate_paste: called");

    let mut enigo = Enigo::new(&Settings::default()).map_err(|e| {
        log_error(&format!("simulate_paste: Failed to create Enigo: {}", e));
        e.to_string()
    })?;

    // Use the appropriate modifier key based on platform
    #[cfg(target_os = "macos")]
    {
        use enigo::Key;
        log_error("simulate_paste: macOS - waiting for focus");
        // Same timing as Windows for consistent cross-platform behavior
        // 150ms works better for Chrome and Electron-based apps
        thread::sleep(Duration::from_millis(150));
        log_error("simulate_paste: macOS - sending Cmd+V");
        enigo
            .key(Key::Meta, enigo::Direction::Press)
            .map_err(|e| {
                log_error(&format!("simulate_paste: Meta press failed: {}", e));
                e.to_string()
            })?;
        // Small delay between key press and V for reliable detection
        thread::sleep(Duration::from_millis(20));
        enigo
            .key(Key::Unicode('v'), enigo::Direction::Click)
            .map_err(|e| {
                log_error(&format!("simulate_paste: V click failed: {}", e));
                e.to_string()
            })?;
        thread::sleep(Duration::from_millis(20));
        enigo
            .key(Key::Meta, enigo::Direction::Release)
            .map_err(|e| {
                log_error(&format!("simulate_paste: Meta release failed: {}", e));
                e.to_string()
            })?;
        log_error("simulate_paste: macOS - Cmd+V sent successfully");
    }

    #[cfg(target_os = "windows")]
    {
        use enigo::Key;
        log_error("simulate_paste: Windows - waiting for focus");
        // Longer delay for Chrome and other electron-based apps
        // They need extra time to fully regain focus after window switch
        thread::sleep(Duration::from_millis(150));
        log_error("simulate_paste: Windows - sending Ctrl+V");
        enigo
            .key(Key::Control, enigo::Direction::Press)
            .map_err(|e| {
                log_error(&format!("simulate_paste: Ctrl press failed: {}", e));
                e.to_string()
            })?;
        // Small delay between key press and V to ensure reliable detection
        thread::sleep(Duration::from_millis(20));
        enigo
            .key(Key::Unicode('v'), enigo::Direction::Click)
            .map_err(|e| {
                log_error(&format!("simulate_paste: V click failed: {}", e));
                e.to_string()
            })?;
        thread::sleep(Duration::from_millis(20));
        enigo
            .key(Key::Control, enigo::Direction::Release)
            .map_err(|e| {
                log_error(&format!("simulate_paste: Ctrl release failed: {}", e));
                e.to_string()
            })?;
        log_error("simulate_paste: Windows - Ctrl+V sent successfully");
    }

    #[cfg(target_os = "linux")]
    {
        use enigo::Key;
        log_error("simulate_paste: Linux - waiting for focus");
        // Same timing as Windows/macOS for consistent cross-platform behavior
        thread::sleep(Duration::from_millis(150));
        log_error("simulate_paste: Linux - sending Ctrl+V");
        enigo
            .key(Key::Control, enigo::Direction::Press)
            .map_err(|e| {
                log_error(&format!("simulate_paste: Ctrl press failed: {}", e));
                e.to_string()
            })?;
        thread::sleep(Duration::from_millis(20));
        enigo
            .key(Key::Unicode('v'), enigo::Direction::Click)
            .map_err(|e| {
                log_error(&format!("simulate_paste: V click failed: {}", e));
                e.to_string()
            })?;
        thread::sleep(Duration::from_millis(20));
        enigo
            .key(Key::Control, enigo::Direction::Release)
            .map_err(|e| {
                log_error(&format!("simulate_paste: Ctrl release failed: {}", e));
                e.to_string()
            })?;
        log_error("simulate_paste: Linux - Ctrl+V sent successfully");
    }

    Ok(())
}

/// Type text directly using keyboard simulation
/// This is an alternative to paste for applications that don't support clipboard
#[tauri::command]
fn type_text(text: String) -> Result<(), String> {
    let mut enigo = Enigo::new(&Settings::default()).map_err(|e| {
        log_error(&format!("Failed to create Enigo for typing: {}", e));
        e.to_string()
    })?;

    enigo.text(&text).map_err(|e| {
        log_error(&format!("Failed to type text: {}", e));
        e.to_string()
    })?;

    Ok(())
}

/// Show the floating recording indicator overlay
#[tauri::command]
fn show_recording_overlay(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("recording-indicator") {
        let _ = window.show();
        let _ = window.set_focus();
    }
    Ok(())
}

/// Hide the floating recording indicator overlay
#[tauri::command]
fn hide_recording_overlay(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("recording-indicator") {
        let _ = window.hide();
    }
    Ok(())
}

/// Auto-configure Claude Code integration on startup
/// Creates MCP server script and config so Claude Code can speak through Scribe
fn setup_claude_code_integration() {
    use std::fs;
    use std::path::PathBuf;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    // Get home directory
    let home = match std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")) {
        Ok(h) => PathBuf::from(h),
        Err(_) => {
            log_error("Could not determine home directory for Claude Code setup");
            return;
        }
    };

    let claude_dir = home.join(".claude");
    let settings_path = claude_dir.join("settings.json");

    // Create .claude directory if it doesn't exist
    if !claude_dir.exists() {
        if let Err(e) = fs::create_dir_all(&claude_dir) {
            log_error(&format!("Failed to create .claude directory: {}", e));
            return;
        }
    }

    // Create the MCP speak script that Claude Code will use
    // This is a simple script that forwards speak requests to Scribe's local server
    let mcp_script_path = claude_dir.join("scribe-mcp.sh");
    let mcp_script_content = r#"#!/bin/bash
# Scribe MCP Server - Bridges Claude Code to Scribe for voice output
# Auto-generated by Scribe app - do not edit

# Simple MCP server that handles the speak tool
while IFS= read -r line; do
    # Check if this is a tool call for speak
    if echo "$line" | grep -q '"method".*"tools/call"'; then
        # Extract the text parameter
        text=$(echo "$line" | sed -n 's/.*"text"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p')
        if [ -n "$text" ]; then
            # Send to Scribe's local server
            curl -s -X POST "http://127.0.0.1:7865/speak" \
                -H "Content-Type: application/json" \
                -d "{\"text\": \"$text\"}" > /dev/null 2>&1
            # Return success response
            echo '{"jsonrpc":"2.0","id":1,"result":{"content":[{"type":"text","text":"Spoke the message"}]}}'
        fi
    elif echo "$line" | grep -q '"method".*"initialize"'; then
        # Return initialization response
        echo '{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2024-11-05","capabilities":{"tools":{}},"serverInfo":{"name":"scribe-speak","version":"1.0.0"}}}'
    elif echo "$line" | grep -q '"method".*"tools/list"'; then
        # Return tool list
        echo '{"jsonrpc":"2.0","id":1,"result":{"tools":[{"name":"speak","description":"Speak text aloud through Scribe. Use to announce what you are doing.","inputSchema":{"type":"object","properties":{"text":{"type":"string","description":"Text to speak"}},"required":["text"]}}]}}'
    fi
done
"#;

    // Write the script
    let script_needs_update = if mcp_script_path.exists() {
        // Check if script content has changed
        fs::read_to_string(&mcp_script_path)
            .map(|content| content != mcp_script_content)
            .unwrap_or(true)
    } else {
        true
    };

    if script_needs_update {
        if let Err(e) = fs::write(&mcp_script_path, mcp_script_content) {
            log_error(&format!("Failed to write MCP script: {}", e));
            return;
        }

        // Make executable on Unix
        #[cfg(unix)]
        {
            if let Err(e) = fs::set_permissions(&mcp_script_path, fs::Permissions::from_mode(0o755)) {
                log_error(&format!("Failed to make MCP script executable: {}", e));
            }
        }
        log_error("Created Claude Code MCP script");
    }

    // Read existing settings or create new
    let mut settings: serde_json::Value = if settings_path.exists() {
        match fs::read_to_string(&settings_path) {
            Ok(content) => serde_json::from_str(&content).unwrap_or(serde_json::json!({})),
            Err(_) => serde_json::json!({}),
        }
    } else {
        serde_json::json!({})
    };

    // Check if scribe MCP server is already configured with our script
    let mcp_configured = settings
        .get("mcpServers")
        .and_then(|s| s.get("scribe"))
        .and_then(|s| s.get("command"))
        .and_then(|c| c.as_str())
        .map(|cmd| cmd.contains("scribe-mcp.sh"))
        .unwrap_or(false);

    if !mcp_configured {
        // Add scribe MCP server configuration pointing to our script
        if settings.get("mcpServers").is_none() {
            settings["mcpServers"] = serde_json::json!({});
        }

        let script_path_str = mcp_script_path.to_string_lossy().to_string();
        settings["mcpServers"]["scribe"] = serde_json::json!({
            "command": script_path_str
        });

        // Write updated settings
        match serde_json::to_string_pretty(&settings) {
            Ok(json_str) => {
                if let Err(e) = fs::write(&settings_path, json_str) {
                    log_error(&format!("Failed to write Claude Code settings: {}", e));
                } else {
                    log_error("Configured Claude Code to use Scribe for voice");
                }
            }
            Err(e) => {
                log_error(&format!("Failed to serialize Claude Code settings: {}", e));
            }
        }
    }
}

/// Start local HTTP server for Claude Code speak integration
fn start_speak_server(app_handle: tauri::AppHandle) {
    thread::spawn(move || {
        let server = match Server::http("127.0.0.1:7865") {
            Ok(s) => {
                log_error("Speak server started on http://127.0.0.1:7865");
                s
            }
            Err(e) => {
                log_error(&format!("Failed to start speak server: {}", e));
                return;
            }
        };

        for mut request in server.incoming_requests() {
            // Only accept POST to /speak
            if request.method().as_str() != "POST" || request.url() != "/speak" {
                let response = Response::from_string("Not Found").with_status_code(404);
                let _ = request.respond(response);
                continue;
            }

            // Read the body
            let mut body = String::new();
            if let Err(e) = request.as_reader().read_to_string(&mut body) {
                log_error(&format!("Failed to read request body: {}", e));
                let response = Response::from_string("Bad Request").with_status_code(400);
                let _ = request.respond(response);
                continue;
            }

            // Parse JSON - expecting {"text": "..."}
            let text = if let Ok(json) = serde_json::from_str::<serde_json::Value>(&body) {
                json.get("text")
                    .and_then(|t| t.as_str())
                    .unwrap_or("")
                    .to_string()
            } else {
                // If not JSON, treat the whole body as text
                body.trim().to_string()
            };

            if text.is_empty() {
                let response = Response::from_string("No text provided").with_status_code(400);
                let _ = request.respond(response);
                continue;
            }

            // Emit event to frontend - emit to main window specifically
            if let Some(window) = app_handle.get_webview_window("main") {
                if let Err(e) = window.emit("speak", &text) {
                    log_error(&format!("Failed to emit speak event to window: {}", e));
                }
            } else {
                log_error("Main window not found for speak event");
            }

            // Respond with success
            let response = Response::from_string("{\"status\":\"ok\"}")
                .with_header(
                    tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..])
                        .unwrap(),
                )
                .with_header(
                    tiny_http::Header::from_bytes(
                        &b"Access-Control-Allow-Origin"[..],
                        &b"*"[..],
                    )
                    .unwrap(),
                );
            let _ = request.respond(response);
        }
    });
}

// Global audio recorder state
struct AudioRecorderState(Mutex<AudioRecorder>);

/// Start native audio recording
#[tauri::command]
fn start_recording(
    state: tauri::State<AudioRecorderState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let mut recorder = state.0.lock();
    recorder.set_app_handle(app);
    recorder.start()
}

/// Stop recording and return audio data
#[tauri::command]
fn stop_recording(state: tauri::State<AudioRecorderState>) -> Result<AudioData, String> {
    let mut recorder = state.0.lock();
    recorder.stop()
}

/// Cancel recording without returning data
#[tauri::command]
fn cancel_recording(state: tauri::State<AudioRecorderState>) -> Result<(), String> {
    let mut recorder = state.0.lock();
    recorder.cancel();
    Ok(())
}

/// List available audio input devices
#[tauri::command]
fn get_audio_devices(state: tauri::State<AudioRecorderState>) -> Result<Vec<AudioDevice>, String> {
    let recorder = state.0.lock();
    recorder.list_devices()
}

/// Check if currently recording
#[tauri::command]
fn check_recording(state: tauri::State<AudioRecorderState>) -> Result<bool, String> {
    let recorder = state.0.lock();
    Ok(recorder.is_recording())
}

/// Setup Claude Code integration by creating CLAUDE.md in user's home directory
/// This is called automatically on app startup
fn setup_claude_integration() {
    // Just call the update function with default "summary" mode
    update_claude_md_content("summary");
}

/// Update CLAUDE.md content based on voice output mode
/// Called from frontend when user changes the setting
#[tauri::command]
fn update_claude_md(mode: String) -> Result<(), String> {
    update_claude_md_content(&mode);
    Ok(())
}

/// Internal function to update CLAUDE.md content
fn update_claude_md_content(mode: &str) {
    let home_var = if cfg!(target_os = "windows") {
        "USERPROFILE"
    } else {
        "HOME"
    };

    if let Some(home) = std::env::var_os(home_var) {
        let claude_md_path = std::path::PathBuf::from(&home).join("CLAUDE.md");

        // If disabled, remove the file entirely
        if mode == "disabled" {
            match std::fs::remove_file(&claude_md_path) {
                Ok(_) => log_error("Removed CLAUDE.md (voice disabled)"),
                Err(_) => {} // File might not exist, that's fine
            }
            return;
        }

        let content = if mode == "full" {
            r#"# Claude Code Instructions

## Voice Output

Always use the Scribe speak integration to read responses aloud. Call it using curl:

```bash
curl -X POST http://127.0.0.1:7865/speak -H "Content-Type: application/json" -d '{"text": "YOUR MESSAGE HERE"}'
```

Use this for all conversational responses to the user. Read everything including code blocks - the user wants to hear the complete response.
"#
        } else {
            r#"# Claude Code Instructions

## Voice Output

Always use the Scribe speak integration to read responses aloud. Call it using curl:

```bash
curl -X POST http://127.0.0.1:7865/speak -H "Content-Type: application/json" -d '{"text": "YOUR MESSAGE HERE"}'
```

Use this for all conversational responses to the user. Do not read code aloud - only explanations, summaries, and direct responses.
"#
        };

        match std::fs::write(&claude_md_path, content) {
            Ok(_) => log_error(&format!("Updated CLAUDE.md with mode: {}", mode)),
            Err(e) => log_error(&format!("Failed to update CLAUDE.md: {}", e)),
        }
    }
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
        .manage(AudioRecorderState(Mutex::new(AudioRecorder::new())))
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_clipboard_manager::init())
        .setup(|app| {
            // Create tray menu
            let show_item = MenuItemBuilder::with_id("show", "Show Scribe").build(app)?;
            let devtools_item = MenuItemBuilder::with_id("devtools", "Open Dev Tools").build(app)?;
            let quit_item = MenuItemBuilder::with_id("quit", "Quit").build(app)?;
            let menu = MenuBuilder::new(app)
                .item(&show_item)
                .item(&devtools_item)
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
                                let _ = window.unminimize();
                                let _ = window.set_focus();
                            }
                        }
                        "devtools" => {
                            if let Some(window) = app.get_webview_window("main") {
                                window.open_devtools();
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
                            let _ = window.unminimize();
                            let _ = window.set_focus();
                        }
                    }
                })
                .build(app)?;

            // Open dev tools for debugging (in both dev and release builds)
            #[cfg(debug_assertions)]
            if let Some(window) = app.get_webview_window("main") {
                window.open_devtools();
            }

            // Set up Claude Code integration (auto-configure MCP)
            setup_claude_code_integration();

            // Start the speak server for Claude Code integration
            start_speak_server(app.handle().clone());

            // Setup Claude Code integration (creates CLAUDE.md if not exists)
            setup_claude_integration();

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            simulate_paste,
            type_text,
            set_recording_state,
            show_recording_overlay,
            hide_recording_overlay,
            start_recording,
            stop_recording,
            cancel_recording,
            get_audio_devices,
            check_recording,
            update_claude_md
        ]);

    match builder.run(tauri::generate_context!()) {
        Ok(_) => {}
        Err(e) => {
            let error_msg = format!("Failed to run Tauri application: {}", e);
            log_error(&error_msg);

            // Show native error dialog based on platform
            show_error_dialog(&format!(
                "Scribe failed to start:\n\n{}\n\nCheck ~/scribe-error.log for details.",
                e
            ));

            std::process::exit(1);
        }
    }
}

/// Show a native error dialog on all platforms
#[cfg(target_os = "windows")]
fn show_error_dialog(message: &str) {
    use std::ptr::null_mut;
    let wide_msg: Vec<u16> = message.encode_utf16().chain(std::iter::once(0)).collect();
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

#[cfg(target_os = "macos")]
fn show_error_dialog(message: &str) {
    use std::process::Command;
    // Use osascript to show a native macOS alert dialog
    let script = format!(
        r#"display dialog "{}" with title "Scribe Error" buttons {{"OK"}} default button "OK" with icon stop"#,
        message.replace("\"", "\\\"").replace("\n", "\\n")
    );
    let _ = Command::new("osascript").arg("-e").arg(&script).output();
}

#[cfg(target_os = "linux")]
fn show_error_dialog(message: &str) {
    use std::process::Command;
    // Try zenity first (GTK), then kdialog (KDE), then notify-send as fallback
    let zenity = Command::new("zenity")
        .args(["--error", "--title=Scribe Error", &format!("--text={}", message)])
        .output();

    if zenity.is_err() || !zenity.unwrap().status.success() {
        let kdialog = Command::new("kdialog")
            .args(["--error", message, "--title", "Scribe Error"])
            .output();

        if kdialog.is_err() || !kdialog.unwrap().status.success() {
            // Last resort: notification
            let _ = Command::new("notify-send")
                .args(["Scribe Error", message])
                .output();
        }
    }
}
