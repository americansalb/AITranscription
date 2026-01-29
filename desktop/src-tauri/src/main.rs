// Prevents additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio;
mod database;
mod queue;

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

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

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

// ==================== Voice Settings Infrastructure ====================

/// Global project path for CLAUDE.md writes
/// This stores the project directory so we can write CLAUDE.md there
static PROJECT_PATH: std::sync::OnceLock<parking_lot::RwLock<Option<String>>> = std::sync::OnceLock::new();

/// Get the project path RwLock, initializing if needed
fn get_project_path_lock() -> &'static parking_lot::RwLock<Option<String>> {
    PROJECT_PATH.get_or_init(|| parking_lot::RwLock::new(None))
}

/// Voice settings structure
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
struct VoiceSettings {
    blind_mode: bool,  // true = describe visuals for blind users
    detail: u8,        // 1 = summary, 3 = balanced, 5 = developer
}

impl Default for VoiceSettings {
    fn default() -> Self {
        Self {
            blind_mode: false,
            detail: 3,
        }
    }
}

/// Get the path to the voice settings file
fn get_voice_settings_path() -> Option<PathBuf> {
    let home_var = if cfg!(target_os = "windows") {
        "APPDATA"
    } else {
        "HOME"
    };

    std::env::var_os(home_var).map(|home| {
        if cfg!(target_os = "windows") {
            PathBuf::from(home).join("Scribe").join("voice-settings.json")
        } else {
            PathBuf::from(home).join(".scribe").join("voice-settings.json")
        }
    })
}

/// Save voice settings to file
fn save_voice_settings(blind_mode: bool, detail: u8) -> Result<(), String> {
    let settings_path = get_voice_settings_path()
        .ok_or("Could not determine settings path")?;

    // Create parent directory if it doesn't exist
    if let Some(parent) = settings_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create settings directory: {}", e))?;
    }

    let settings = VoiceSettings {
        blind_mode,
        detail: detail.clamp(1, 5),
    };

    let json = serde_json::to_string_pretty(&settings)
        .map_err(|e| format!("Failed to serialize settings: {}", e))?;

    fs::write(&settings_path, json)
        .map_err(|e| format!("Failed to write settings file: {}", e))?;

    log_error(&format!("Saved voice settings: blind_mode={}, detail={}", blind_mode, detail));
    Ok(())
}

/// Load voice settings from file
fn load_voice_settings() -> VoiceSettings {
    let settings_path = match get_voice_settings_path() {
        Some(path) => path,
        None => return VoiceSettings::default(),
    };

    match fs::read_to_string(&settings_path) {
        Ok(contents) => {
            match serde_json::from_str::<VoiceSettings>(&contents) {
                Ok(settings) => settings,
                Err(e) => {
                    log_error(&format!("Failed to parse voice settings: {}, using defaults", e));
                    VoiceSettings::default()
                }
            }
        }
        Err(_) => {
            // File doesn't exist yet, use defaults
            VoiceSettings::default()
        }
    }
}

/// Generate instruction text based on blind mode and detail level
fn generate_instruction_text(blind_mode: bool, detail: u8) -> String {
    // Detail 1 = summary (simple), 5 = developer (technical)
    let detail_guidance = match detail {
        1 => "Keep explanations extremely brief - one sentence summaries only. Use simple, layperson terms.",
        2 => "Be concise - provide essential information without unnecessary detail. Minimize jargon.",
        3 => "Provide balanced detail - enough context to understand without overwhelming.",
        4 => "Be thorough - include context, rationale, and implications. Use technical terminology freely.",
        5 => "Provide exhaustive detail - comprehensive explanations including edge cases, patterns, and implementation specifics. Full technical depth.",
        _ => "Provide balanced detail.",
    };

    if blind_mode {
        format!(
            "User cannot see the screen. Describe ALL visual elements including: exact file paths, code structure with indentation levels, spatial positioning of UI elements, colors, borders, spacing measurements, hierarchical relationships, and how components are organized. Never assume they can see anything. {}",
            detail_guidance
        )
    } else {
        detail_guidance.to_string()
    }
}

/// Tauri command to save voice settings
#[tauri::command]
fn save_voice_settings_cmd(blind_mode: bool, detail: u8) -> Result<(), String> {
    save_voice_settings(blind_mode, detail)
}

/// Tauri command to set the project path for CLAUDE.md writes
/// This allows the frontend to specify where CLAUDE.md should be written
#[tauri::command]
fn set_project_path(path: String) -> Result<(), String> {
    let lock = get_project_path_lock();
    let mut guard = lock.write();

    // Validate the path exists
    let path_buf = std::path::PathBuf::from(&path);
    if !path_buf.exists() {
        log_error(&format!("set_project_path: Path does not exist: {}", path));
        return Err(format!("Path does not exist: {}", path));
    }

    log_error(&format!("set_project_path: Setting project path to: {}", path));
    *guard = Some(path);
    Ok(())
}

/// Tauri command to get the current project path (for debugging)
#[tauri::command]
fn get_project_path() -> Option<String> {
    let lock = get_project_path_lock();
    let guard = lock.read();
    guard.clone()
}

// ==================== End Voice Settings ====================

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
        // Just show - NEVER set_focus, or it steals focus from the user's app
        let _ = window.show();
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

/// Show the transcript window
#[tauri::command]
fn show_transcript_window(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("transcript") {
        let _ = window.show();
        let _ = window.set_focus();
    }
    Ok(())
}

/// Hide the transcript window
#[tauri::command]
fn hide_transcript_window(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("transcript") {
        let _ = window.hide();
    }
    Ok(())
}

/// Toggle transcript window visibility
#[tauri::command]
fn toggle_transcript_window(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("transcript") {
        match window.is_visible() {
            Ok(true) => { let _ = window.hide(); },
            _ => {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }
    }
    Ok(())
}

/// Get the path to the bundled scribe-mcp sidecar binary
fn get_sidecar_path() -> Option<std::path::PathBuf> {
    // Get the directory where the app is running from
    let exe_path = std::env::current_exe().ok()?;
    let exe_dir = exe_path.parent()?;

    // Determine platform-specific binary name
    #[cfg(target_os = "windows")]
    let binary_name = "scribe-mcp-x86_64-pc-windows-msvc.exe";

    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    let binary_name = "scribe-mcp-aarch64-apple-darwin";

    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    let binary_name = "scribe-mcp-x86_64-apple-darwin";

    #[cfg(target_os = "linux")]
    let binary_name = "scribe-mcp-x86_64-unknown-linux-gnu";

    // In development, the binary is in target/release or target/debug
    // In production, it's in the resources folder
    let possible_paths = vec![
        // Production: bundled with app
        exe_dir.join("binaries").join(binary_name),
        exe_dir.join("../Resources/binaries").join(binary_name), // macOS .app bundle
        // Development: in src-tauri/binaries
        exe_dir.join("../../src-tauri/binaries").join(binary_name),
        exe_dir.join("../../../src-tauri/binaries").join(binary_name),
    ];

    for path in possible_paths {
        if path.exists() {
            return Some(path.canonicalize().unwrap_or(path));
        }
    }

    None
}

/// Auto-configure Claude Code integration on startup
/// Creates .mcp.json config so Claude Code can speak through Scribe
fn setup_claude_code_integration() {
    use std::fs;
    use std::path::PathBuf;

    // Get home directory
    let home = match std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")) {
        Ok(h) => PathBuf::from(h),
        Err(_) => {
            log_error("Could not determine home directory for Claude Code setup");
            return;
        }
    };

    // Claude Code uses .claude.json in home directory for user-scoped MCP server config
    // (NOT .mcp.json - that's for project-scoped configs in the project root)
    let mcp_config_path = home.join(".claude.json");

    // Get the path to our bundled MCP sidecar
    let sidecar_path = match get_sidecar_path() {
        Some(path) => {
            log_error(&format!("Found scribe-mcp sidecar at: {:?}", path));
            path.to_string_lossy().to_string()
        }
        None => {
            // Fallback to scribe-speak if pip-installed (for backwards compatibility)
            log_error("Sidecar not found, falling back to scribe-speak in PATH");
            "scribe-speak".to_string()
        }
    };

    // Read existing config or create new
    let mut config: serde_json::Value = if mcp_config_path.exists() {
        match fs::read_to_string(&mcp_config_path) {
            Ok(content) => serde_json::from_str(&content).unwrap_or(serde_json::json!({})),
            Err(_) => serde_json::json!({}),
        }
    } else {
        serde_json::json!({})
    };

    // Check if scribe MCP server is already configured with THIS path
    let current_command = config
        .get("mcpServers")
        .and_then(|s| s.get("scribe"))
        .and_then(|s| s.get("command"))
        .and_then(|c| c.as_str())
        .unwrap_or("");

    let needs_update = current_command != sidecar_path;

    if needs_update {
        // Add/update scribe MCP server configuration
        if config.get("mcpServers").is_none() {
            config["mcpServers"] = serde_json::json!({});
        }

        config["mcpServers"]["scribe"] = serde_json::json!({
            "type": "stdio",
            "command": sidecar_path,
            "args": []
        });

        // Write updated config
        match serde_json::to_string_pretty(&config) {
            Ok(json_str) => {
                if let Err(e) = fs::write(&mcp_config_path, json_str) {
                    log_error(&format!("Failed to write .mcp.json: {}", e));
                } else {
                    log_error(&format!("Configured Claude Code MCP: {} in .mcp.json", sidecar_path));
                }
            }
            Err(e) => {
                log_error(&format!("Failed to serialize .mcp.json: {}", e));
            }
        }
    } else {
        log_error("Claude Code already configured for Scribe");
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
            let url = request.url().to_string();
            let method = request.method().as_str().to_string();

            // Handle heartbeat endpoint
            if method == "POST" && url == "/heartbeat" {
                let mut body = String::new();
                if request.as_reader().read_to_string(&mut body).is_ok() {
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&body) {
                        if let Some(session_id) = json.get("session_id").and_then(|s| s.as_str()) {
                            let timestamp = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .map(|d| d.as_millis() as u64)
                                .unwrap_or(0);

                            let payload = serde_json::json!({
                                "session_id": session_id,
                                "timestamp": timestamp
                            });

                            // Emit heartbeat event to frontend
                            if let Some(window) = app_handle.get_webview_window("main") {
                                let _ = window.emit("heartbeat", &payload);
                            }
                            if let Some(window) = app_handle.get_webview_window("transcript") {
                                let _ = window.emit("heartbeat", &payload);
                            }

                            let response = Response::from_string(r#"{"status":"ok"}"#)
                                .with_header(
                                    tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap()
                                );
                            let _ = request.respond(response);
                            continue;
                        }
                    }
                }
                let response = Response::from_string("Bad Request").with_status_code(400);
                let _ = request.respond(response);
                continue;
            }

            // Only accept POST to /speak
            if method != "POST" || url != "/speak" {
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

            // Parse JSON - expecting {"text": "...", "session_id": "..."}
            let (text, mut session_id) = if let Ok(json) = serde_json::from_str::<serde_json::Value>(&body) {
                let text = json.get("text")
                    .and_then(|t| t.as_str())
                    .unwrap_or("")
                    .to_string();
                let session_id = json.get("session_id")
                    .and_then(|s| s.as_str())
                    .map(|s| s.to_string());
                (text, session_id)
            } else {
                // If not JSON, treat the whole body as text
                (body.trim().to_string(), None)
            };

            if text.is_empty() {
                let response = Response::from_string("No text provided").with_status_code(400);
                let _ = request.respond(response);
                continue;
            }

            // Generate session_id if not provided
            if session_id.is_none() {
                use std::time::{SystemTime, UNIX_EPOCH};
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap();
                let timestamp = now.as_millis();
                let nanos = now.as_nanos();

                // Use nanoseconds as pseudo-random seed for suffix
                let random_suffix: String = (0..8)
                    .map(|i| {
                        let idx = ((nanos >> (i * 5)) % 36) as u8;
                        if idx < 10 {
                            (b'0' + idx) as char
                        } else {
                            (b'a' + (idx - 10)) as char
                        }
                    })
                    .collect();
                session_id = Some(format!("session-{}-{}", timestamp, random_suffix));
            }

            let session_id = session_id.unwrap();

            // Get current timestamp
            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);

            // Add to queue database
            let queue_item = match queue::add_queue_item(text.clone(), session_id.clone()) {
                Ok(item) => Some(item),
                Err(e) => {
                    log_error(&format!("Failed to add item to queue: {}", e));
                    None
                }
            };

            // Create payload with text, session_id, timestamp, and queue item info
            let payload = serde_json::json!({
                "text": text,
                "session_id": session_id,
                "timestamp": timestamp,
                "queue_item": queue_item
            });

            // Emit event to frontend - emit to main window
            log_error(&format!("Scribe: Emitting speak event to MAIN window - Session: {}, Text: {:.50}", session_id, text));
            if let Some(window) = app_handle.get_webview_window("main") {
                if let Err(e) = window.emit("speak", &payload) {
                    log_error(&format!("Failed to emit speak event to window: {}", e));
                }
            } else {
                log_error("Main window not found for speak event");
            }

            // Also emit to transcript window if it exists
            log_error(&format!("Scribe: Emitting speak event to TRANSCRIPT window - Session: {}", session_id));
            if let Some(window) = app_handle.get_webview_window("transcript") {
                let _ = window.emit("speak", &payload);
            }

            // Load voice settings and generate instructions
            let voice_settings = load_voice_settings();
            let instructions = generate_instruction_text(voice_settings.blind_mode, voice_settings.detail);

            // Respond with success, session_id, and instructions
            let response_body = serde_json::json!({
                "status": "ok",
                "session_id": session_id,
                "instructions": instructions
            }).to_string();
            let response = Response::from_string(response_body)
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
struct AudioRecorderState(pub Mutex<AudioRecorder>);

/// Start native audio recording
#[tauri::command]
fn start_recording(
    state: tauri::State<AudioRecorderState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let recorder = state.0.lock();
    recorder.start(Some(app))
}

/// Stop recording and return audio data
#[tauri::command]
fn stop_recording(state: tauri::State<AudioRecorderState>) -> Result<AudioData, String> {
    let recorder = state.0.lock();
    recorder.stop()
}

/// Cancel recording without returning data
#[tauri::command]
fn cancel_recording(state: tauri::State<AudioRecorderState>) -> Result<(), String> {
    let recorder = state.0.lock();
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
    // Just call the update function with default "summary" mode and detail level 3
    update_claude_md_content(true, false, 3);
}

/// Update CLAUDE.md content based on voice output mode
/// Called from frontend when user changes the setting
#[tauri::command]
fn update_claude_md(enabled: bool, blind_mode: bool, detail: u8) -> Result<(), String> {
    let detail_level = detail.clamp(1, 5); // Ensure 1-5 range
    update_claude_md_content(enabled, blind_mode, detail_level);
    Ok(())
}

/// Internal function to update CLAUDE.md content
/// Writes to BOTH the project directory (if set) and the home directory for maximum compatibility
fn update_claude_md_content(enabled: bool, blind_mode: bool, detail: u8) {
    let home_var = if cfg!(target_os = "windows") {
        "USERPROFILE"
    } else {
        "HOME"
    };

    let content = generate_voice_template(blind_mode, detail);
    let mode_str = if blind_mode { "blind" } else { "standard" };

    // Get the project path if set
    let project_path: Option<String> = {
        let lock = get_project_path_lock();
        let guard = lock.read();
        guard.clone()
    };

    // Write to project directory first (primary location for Claude Code)
    if let Some(ref proj_path) = project_path {
        let project_claude_md = std::path::PathBuf::from(proj_path).join("CLAUDE.md");

        if !enabled {
            match std::fs::remove_file(&project_claude_md) {
                Ok(_) => log_error(&format!("Removed CLAUDE.md from project dir (voice disabled): {:?}", project_claude_md)),
                Err(_) => {} // File might not exist, that's fine
            }
        } else {
            match std::fs::write(&project_claude_md, &content) {
                Ok(_) => log_error(&format!("Updated CLAUDE.md in project dir: {:?} (mode: {}, detail: {})", project_claude_md, mode_str, detail)),
                Err(e) => log_error(&format!("Failed to write CLAUDE.md to project dir {:?}: {}", project_claude_md, e)),
            }
        }
    } else {
        log_error("No project path set - CLAUDE.md will only be written to home directory");
    }

    // Also write to home directory (fallback location)
    if let Some(home) = std::env::var_os(home_var) {
        let home_claude_md = std::path::PathBuf::from(&home).join("CLAUDE.md");

        if !enabled {
            match std::fs::remove_file(&home_claude_md) {
                Ok(_) => log_error("Removed CLAUDE.md from home dir (voice disabled)"),
                Err(_) => {} // File might not exist, that's fine
            }
        } else {
            match std::fs::write(&home_claude_md, &content) {
                Ok(_) => log_error(&format!("Updated CLAUDE.md in home dir: {:?} (mode: {}, detail: {})", home_claude_md, mode_str, detail)),
                Err(e) => log_error(&format!("Failed to write CLAUDE.md to home dir: {}", e)),
            }
        }
    }
}

/// Generate voice template based on blind mode and detail level
fn generate_voice_template(blind_mode: bool, detail: u8) -> String {
    let base_instruction = r#"# Claude Code Instructions

## Voice Output

Always use the Scribe speak integration to read responses aloud.

**CRITICAL: Use the MCP `/speak` tool - NOT curl**

The `/speak` tool is available through the MCP server. It automatically manages stable session IDs based on your terminal process. Simply call the speak tool using MCP:

The session ID is handled automatically - all messages from this terminal will be grouped together in the same conversation.

**Session Management:**
- Each terminal window gets a unique session ID automatically (based on process ID)
- All Claude instances in the same terminal share the same session
- You don't need to track or pass session IDs manually
- NEVER use curl to call the speak endpoint directly

**How it works:**
- Session ID format: `{hostname}-{parent_process_id}`
- Same terminal = Same parent PID = Same session
- Different terminal = Different parent PID = Different session
"#;

    // Build the detail level scale explanation
    let detail_scale = format!(r#"
## Detail Level: {} out of 5

THE FULL SCALE (so you understand the range):
- Level 1 (Minimum): One sentence only. "I updated the login page."
- Level 2: 1-2 sentences. "I fixed the login button - the click handler was missing."
- Level 3 (Middle): Mention file names and explain why. "I modified LoginForm.tsx to fix the submit button by adding the missing onClick handler."
- Level 4: Include line numbers, technical details, and implications.
- Level 5 (Maximum): Full technical breakdown with architecture decisions, edge cases, all files touched, and implementation specifics.

YOU ARE AT LEVEL {}: {}
"#,
        detail,
        detail,
        match detail {
            1 => "This is the MINIMUM detail. Be as brief as humanly possible. One short sentence max. No technical terms. A child should understand it.",
            2 => "This is LOW detail. Keep it to 1-2 simple sentences. Mention what changed and why, nothing more.",
            3 => "This is MEDIUM detail. Include the file name, what you changed, and why. A few sentences is fine. Balance clarity with brevity.",
            4 => "This is HIGH detail. Be thorough. Include file names, line numbers, technical details, and explain the implications of your changes.",
            5 => "This is MAXIMUM detail. Give a comprehensive technical breakdown. Mention every file you touched, explain your architecture decisions, cover edge cases, and describe implementation specifics. Developers want the full picture.",
            _ => "This is MEDIUM detail. Include the file name, what you changed, and why.",
        }
    );

    let mode_instructions = if blind_mode {
        format!(r#"
{}
## Mode: Screen Reader

The user CANNOT see the screen. You MUST describe all visual information.

### ALWAYS do these things:
- Say the full file path when you modify a file
- Describe where UI elements are positioned (top-right, centered, below the header)
- Mention colors, sizes, and spacing when relevant
- Explain the visual hierarchy and structure of code
- Describe what's above, below, and beside changed elements

### NEVER do these things:
- Read code syntax character by character
- Assume the user can see anything on screen
- Skip describing the location of changes
- Use vague terms like "here" or "this" without context
"#, detail_scale)
    } else {
        format!(r#"
{}
## Mode: Standard

The user can see the screen. Focus on explaining what you did and why.

### ALWAYS do these things:
- Say the file name when you modify a file
- Explain what you changed and why
- Mention if you created new files or functions
- Summarize the purpose of bug fixes

### NEVER do these things:
- Read entire code blocks out loud
- Spell out syntax like brackets and semicolons
- Describe visual layouts in detail (user can see)
- Give lengthy explanations for simple changes
"#, detail_scale)
    };

    format!("{}{}", base_instruction, mode_instructions)
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

            // Initialize the queue database
            if let Err(e) = database::init_database() {
                log_error(&format!("Failed to initialize queue database: {}", e));
            }

            // Set up Claude Code integration (auto-configure MCP)
            setup_claude_code_integration();

            // Start the speak server for Claude Code integration
            start_speak_server(app.handle().clone());

            // Don't auto-setup Claude Code integration - let frontend control it
            // based on user's saved voice preference
            // Frontend will call update_claude_md() based on localStorage setting

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            simulate_paste,
            type_text,
            set_recording_state,
            show_recording_overlay,
            hide_recording_overlay,
            show_transcript_window,
            hide_transcript_window,
            toggle_transcript_window,
            start_recording,
            stop_recording,
            cancel_recording,
            get_audio_devices,
            check_recording,
            update_claude_md,
            save_voice_settings_cmd,
            set_project_path,
            get_project_path,
            // Queue commands
            queue::add_queue_item,
            queue::get_queue_items,
            queue::update_queue_item_status,
            queue::reorder_queue_item,
            queue::remove_queue_item,
            queue::clear_completed_items,
            queue::get_pending_count,
            queue::get_next_pending_item
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
