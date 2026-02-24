// Prevents additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod a11y;
mod audio;
mod collab;
mod database;
mod keyboard_hook;
mod launcher;
mod queue;


use audio::{AudioData, AudioDevice, AudioRecorder};
use enigo::{Enigo, Keyboard, Mouse, Settings, Coordinate};
use parking_lot::Mutex;
use std::io::Read;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread;
use std::time::Duration;

/// Monotonically increasing ID for each Alt+A request. When a new request starts,
/// it bumps this counter. Running loops check if their ID still matches; if not, they abort.
static COMPUTER_USE_REQUEST_ID: AtomicU64 = AtomicU64::new(0);

/// Shutdown flag for the HTTP server thread. Set to true on app exit.
static HTTP_SERVER_SHUTDOWN: AtomicBool = AtomicBool::new(false);

/// Backend API base URL. Override via VAAK_BACKEND_URL env var; defaults to localhost:19836.
static BACKEND_URL: std::sync::OnceLock<String> = std::sync::OnceLock::new();

fn get_backend_url() -> &'static str {
    BACKEND_URL.get_or_init(|| {
        std::env::var("VAAK_BACKEND_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:19836".to_string())
    })
}

/// Speaker lock: only one session can speak at a time.
/// Stores (session_id, last_speak_timestamp_millis). Other sessions are silently
/// dropped until the active speaker has been quiet for SPEAKER_LOCK_TIMEOUT_MS.
static ACTIVE_SPEAKER: std::sync::OnceLock<Mutex<(Option<String>, u64)>> = std::sync::OnceLock::new();
const SPEAKER_LOCK_TIMEOUT_MS: u64 = 5000;

fn get_active_speaker() -> &'static Mutex<(Option<String>, u64)> {
    ACTIVE_SPEAKER.get_or_init(|| Mutex::new((None, 0)))
}

use tauri::{
    image::Image,
    menu::{MenuBuilder, MenuItemBuilder},
    tray::TrayIconBuilder,
    Emitter, Manager,
};
use tiny_http::{Response, Server};

/// Validate and canonicalize a project directory path.
/// Rejects path traversal attempts (e.g., `..`) and verifies the directory exists
/// and contains a `.vaak/` subdirectory (confirming it's a valid Vaak project).
fn validate_project_dir(dir: &str) -> Result<String, String> {
    let path = std::path::Path::new(dir);

    // Reject paths containing ".." components
    for component in path.components() {
        if let std::path::Component::ParentDir = component {
            return Err("Invalid project directory: path traversal not allowed".to_string());
        }
    }

    // Canonicalize to resolve symlinks and relative paths
    let canonical = path.canonicalize()
        .map_err(|e| format!("Invalid project directory '{}': {}", dir, e))?;

    // Must be a directory
    if !canonical.is_dir() {
        return Err(format!("Not a directory: {}", canonical.display()));
    }

    // Must contain .vaak/ subdirectory (valid project)
    let vaak_dir = canonical.join(".vaak");
    if !vaak_dir.is_dir() {
        return Err(format!("Not a Vaak project (no .vaak/ directory): {}", canonical.display()));
    }

    // Strip Windows extended-length path prefix (\\?\) that canonicalize() adds
    let s = canonical.to_string_lossy().to_string();
    Ok(s.strip_prefix("\\\\?\\").unwrap_or(&s).to_string())
}

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
        let log_path = std::path::PathBuf::from(home).join("vaak-error.log");
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&log_path) {
            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let _ = writeln!(file, "[{}] {}", timestamp, message);
        }
    }

    // Also print to stderr for debugging
    eprintln!("Vaak: {}", message);
}

// ==================== Voice Settings Infrastructure ====================

/// Global project path for settings
/// This stores the project directory
static PROJECT_PATH: std::sync::OnceLock<parking_lot::RwLock<Option<String>>> = std::sync::OnceLock::new();

/// Get the project path RwLock, initializing if needed
fn get_project_path_lock() -> &'static parking_lot::RwLock<Option<String>> {
    PROJECT_PATH.get_or_init(|| parking_lot::RwLock::new(None))
}

/// Voice settings structure
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
struct VoiceSettings {
    #[serde(default = "default_enabled")]
    enabled: bool,     // true = voice output active
    blind_mode: bool,  // true = describe visuals for blind users
    detail: u8,        // 1 = summary, 3 = balanced, 5 = developer
    #[serde(default)]
    auto_collab: bool, // true = agents autonomously handle team messages
    #[serde(default)]
    human_in_loop: bool, // true = human must approve plans and final sign-off
}

fn default_enabled() -> bool { true }

impl Default for VoiceSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            blind_mode: false,
            detail: 3,
            auto_collab: false,
            human_in_loop: false,
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
            PathBuf::from(home).join("Vaak").join("voice-settings.json")
        } else {
            PathBuf::from(home).join(".vaak").join("voice-settings.json")
        }
    })
}

/// Save voice settings to file
fn save_voice_settings(enabled: bool, blind_mode: bool, detail: u8) -> Result<(), String> {
    let settings_path = get_voice_settings_path()
        .ok_or("Could not determine settings path")?;

    // Create parent directory if it doesn't exist
    if let Some(parent) = settings_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create settings directory: {}", e))?;
    }

    // Preserve existing auto_collab value when saving voice settings
    let existing = load_voice_settings();

    let settings = VoiceSettings {
        enabled,
        blind_mode,
        detail: detail.clamp(1, 5),
        auto_collab: existing.auto_collab,
        human_in_loop: existing.human_in_loop,
    };

    let json = serde_json::to_string_pretty(&settings)
        .map_err(|e| format!("Failed to serialize settings: {}", e))?;

    fs::write(&settings_path, json)
        .map_err(|e| format!("Failed to write settings file: {}", e))?;

    log_error(&format!("Saved voice settings: enabled={}, blind_mode={}, detail={}", enabled, blind_mode, detail));
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
fn save_voice_settings_cmd(enabled: bool, blind_mode: bool, detail: u8) -> Result<(), String> {
    save_voice_settings(enabled, blind_mode, detail)
}

/// Sync auto_collab/human_in_loop to the active project.json so MCP agents can read them.
fn sync_collab_settings_to_project() {
    let watched_dir = {
        let guard = get_project_watched_dir().lock();
        match guard.as_ref() {
            Some(d) => d.clone(),
            None => return,
        }
    };

    let config_path = std::path::Path::new(&watched_dir).join(".vaak").join("project.json");
    let content = match std::fs::read_to_string(&config_path) {
        Ok(c) => c,
        Err(_) => return,
    };
    let mut config: serde_json::Value = match serde_json::from_str(&content) {
        Ok(c) => c,
        Err(_) => return,
    };

    let settings = load_voice_settings();
    if let Some(s) = config.get_mut("settings") {
        s["auto_collab"] = serde_json::Value::Bool(settings.auto_collab);
        s["human_in_loop"] = serde_json::Value::Bool(settings.human_in_loop);
    }

    if let Ok(pretty) = serde_json::to_string_pretty(&config) {
        let _ = collab::atomic_write(&config_path, pretty.as_bytes());
    }
}

/// Tauri command to toggle auto-collab mode
#[tauri::command]
fn set_auto_collab(enabled: bool) -> Result<(), String> {
    let settings_path = get_voice_settings_path()
        .ok_or("Could not determine settings path")?;

    let mut settings = load_voice_settings();
    settings.auto_collab = enabled;

    let json = serde_json::to_string_pretty(&settings)
        .map_err(|e| format!("Failed to serialize settings: {}", e))?;
    fs::write(&settings_path, json)
        .map_err(|e| format!("Failed to write settings file: {}", e))?;

    log_error(&format!("Vaak: auto_collab set to {}", enabled));
    sync_collab_settings_to_project();
    Ok(())
}

/// Tauri command to get auto-collab state
#[tauri::command]
fn get_auto_collab() -> bool {
    load_voice_settings().auto_collab
}

/// Tauri command to toggle human-in-loop mode
#[tauri::command]
fn set_human_in_loop(enabled: bool) -> Result<(), String> {
    let settings_path = get_voice_settings_path()
        .ok_or("Could not determine settings path")?;

    let mut settings = load_voice_settings();
    settings.human_in_loop = enabled;

    let json = serde_json::to_string_pretty(&settings)
        .map_err(|e| format!("Failed to serialize settings: {}", e))?;
    fs::write(&settings_path, json)
        .map_err(|e| format!("Failed to write settings file: {}", e))?;

    log_error(&format!("Vaak: human_in_loop set to {}", enabled));
    sync_collab_settings_to_project();
    Ok(())
}

/// Tauri command to get human-in-loop state
#[tauri::command]
fn get_human_in_loop() -> bool {
    load_voice_settings().human_in_loop
}

/// Tauri command to save screen reader settings (model, detail, focus, hotkey, voice)
#[tauri::command]
fn save_screen_reader_settings(model: String, detail: u8, focus: String, hotkey: String, voice: Option<String>) -> Result<(), String> {
    let home_var = if cfg!(target_os = "windows") { "APPDATA" } else { "HOME" };
    let settings_path = std::env::var_os(home_var)
        .map(|home| {
            if cfg!(target_os = "windows") {
                PathBuf::from(home).join("Vaak").join("screen-reader-settings.json")
            } else {
                PathBuf::from(home).join(".vaak").join("screen-reader-settings.json")
            }
        })
        .ok_or("Could not determine settings path")?;

    if let Some(parent) = settings_path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Failed to create dir: {}", e))?;
    }

    let settings = serde_json::json!({
        "model": model,
        "detail": detail.clamp(1, 5),
        "focus": focus,
        "hotkey": hotkey,
        "voice_id": voice.unwrap_or_else(|| "jiIkqWtTmS0GBz46iqA0".to_string())
    });

    let json = serde_json::to_string_pretty(&settings)
        .map_err(|e| format!("Failed to serialize: {}", e))?;
    fs::write(&settings_path, json)
        .map_err(|e| format!("Failed to write: {}", e))?;
    Ok(())
}

/// Tauri command to set the project path
/// This allows the frontend to specify the project directory
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

/// Track whether we've already shown the accessibility permission dialog
#[cfg(target_os = "macos")]
static ACCESSIBILITY_WARNED: AtomicBool = AtomicBool::new(false);

/// Cache for granted accessibility permission (once true, stays true for process lifetime)
#[cfg(target_os = "macos")]
static ACCESSIBILITY_GRANTED: AtomicBool = AtomicBool::new(false);

/// Cache for granted screen recording permission (once true, stays true for process lifetime)
#[cfg(target_os = "macos")]
static SCREEN_RECORDING_GRANTED: AtomicBool = AtomicBool::new(false);

/// Check if macOS Accessibility permission is granted.
/// Enigo silently fails without this — key simulation returns Ok but nothing happens.
/// On first failure, attempts to trigger the macOS permission prompt via AXIsProcessTrustedWithOptions.
/// Caches positive result to avoid repeated FFI calls after permission is granted.
#[cfg(target_os = "macos")]
fn check_accessibility_permission() -> Result<(), String> {
    // Fast path: already granted, skip FFI call
    if ACCESSIBILITY_GRANTED.load(Ordering::Relaxed) {
        return Ok(());
    }

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXIsProcessTrusted() -> u8;
    }

    let trusted = unsafe { AXIsProcessTrusted() };
    if trusted != 0 {
        ACCESSIBILITY_GRANTED.store(true, Ordering::Relaxed);
        return Ok(());
    }

    // Not yet granted — on first failure, trigger the system permission prompt
    if !ACCESSIBILITY_WARNED.swap(true, Ordering::Relaxed) {
        eprintln!("[accessibility] Permission not granted — triggering macOS prompt");
        // AXIsProcessTrustedWithOptions with kAXTrustedCheckOptionPrompt=true
        // triggers the macOS system dialog asking the user to grant permission
        #[link(name = "ApplicationServices", kind = "framework")]
        extern "C" {
            fn AXIsProcessTrustedWithOptions(options: *const std::ffi::c_void) -> u8;
        }
        #[link(name = "CoreFoundation", kind = "framework")]
        extern "C" {
            fn CFDictionaryCreate(
                allocator: *const std::ffi::c_void,
                keys: *const *const std::ffi::c_void,
                values: *const *const std::ffi::c_void,
                num: isize,
                key_callbacks: *const std::ffi::c_void,
                value_callbacks: *const std::ffi::c_void,
            ) -> *const std::ffi::c_void;
            fn CFRelease(cf: *const std::ffi::c_void);
            static kCFBooleanTrue: *const std::ffi::c_void;
            static kCFTypeDictionaryKeyCallBacks: u8;
            static kCFTypeDictionaryValueCallBacks: u8;
        }
        let key_str = b"AXTrustedCheckOptionPrompt\0";
        #[link(name = "CoreFoundation", kind = "framework")]
        extern "C" {
            fn CFStringCreateWithCString(
                alloc: *const std::ffi::c_void,
                c_str: *const u8,
                encoding: u32,
            ) -> *const std::ffi::c_void;
        }
        unsafe {
            let key = CFStringCreateWithCString(std::ptr::null(), key_str.as_ptr(), 0x08000100); // kCFStringEncodingUTF8
            let keys = [key];
            let values = [kCFBooleanTrue];
            let options = CFDictionaryCreate(
                std::ptr::null(),
                keys.as_ptr(),
                values.as_ptr(),
                1,
                &kCFTypeDictionaryKeyCallBacks as *const _ as *const std::ffi::c_void,
                &kCFTypeDictionaryValueCallBacks as *const _ as *const std::ffi::c_void,
            );
            let _ = AXIsProcessTrustedWithOptions(options);
            CFRelease(options);
            CFRelease(key);
        }
    }

    Err(
        "Accessibility permission not granted. Vaak needs Accessibility access to simulate \
         keyboard shortcuts. Go to System Settings > Privacy & Security > Accessibility \
         and enable Vaak. You may need to restart the app after granting permission."
            .to_string(),
    )
}

/// Check if macOS Screen Recording permission is granted.
/// On macOS 10.15+, screen capture returns blank images without this permission.
/// Caches positive result to avoid repeated FFI calls.
#[cfg(target_os = "macos")]
fn check_screen_recording_permission() -> Result<(), String> {
    // Fast path: already granted
    if SCREEN_RECORDING_GRANTED.load(Ordering::Relaxed) {
        return Ok(());
    }

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGPreflightScreenCaptureAccess() -> u8;
    }

    let granted = unsafe { CGPreflightScreenCaptureAccess() };
    if granted != 0 {
        SCREEN_RECORDING_GRANTED.store(true, Ordering::Relaxed);
        return Ok(());
    }

    // Not granted — request access (shows system dialog on first call)
    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGRequestScreenCaptureAccess() -> u8;
    }
    let requested = unsafe { CGRequestScreenCaptureAccess() };
    if requested != 0 {
        SCREEN_RECORDING_GRANTED.store(true, Ordering::Relaxed);
        return Ok(());
    }

    Err(
        "Screen Recording permission not granted. Vaak needs Screen Recording access to capture \
         screenshots. Go to System Settings > Privacy & Security > Screen Recording \
         and enable Vaak. You may need to restart the app after granting permission."
            .to_string(),
    )
}

/// Simulate a paste keyboard shortcut (Ctrl+V on Windows/Linux, Cmd+V on macOS)
///
/// Note: Focus switching timing is handled by the JavaScript caller.
/// This function sends the paste keys with logging for debugging.
#[tauri::command]
fn simulate_paste() -> Result<(), String> {
    log_error("simulate_paste: called");

    // On macOS, check Accessibility permission before attempting key simulation.
    // Without it, enigo silently succeeds but no keys are actually pressed.
    #[cfg(target_os = "macos")]
    check_accessibility_permission()?;

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
    // On macOS, check Accessibility permission before attempting key simulation.
    #[cfg(target_os = "macos")]
    check_accessibility_permission()?;

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

/// Show the screen reader settings window
#[tauri::command]
fn show_screen_reader_window(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("screen-reader") {
        let _ = window.show();
        let _ = window.set_focus();
    }
    Ok(())
}

/// Toggle screen reader settings window visibility
#[tauri::command]
fn toggle_screen_reader_window(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("screen-reader") {
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

/// Toggle queue window visibility
#[tauri::command]
fn toggle_queue_window(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("queue") {
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

/// Get the path to the bundled vaak-mcp sidecar binary
fn get_sidecar_path() -> Option<std::path::PathBuf> {
    // Get the directory where the app is running from
    let exe_path = std::env::current_exe().ok()?;
    let exe_dir = exe_path.parent()?;

    // Determine platform-specific binary name
    #[cfg(target_os = "windows")]
    let binary_name = "vaak-mcp-x86_64-pc-windows-msvc.exe";

    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    let binary_name = "vaak-mcp-aarch64-apple-darwin";

    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    let binary_name = "vaak-mcp-x86_64-apple-darwin";

    #[cfg(target_os = "linux")]
    let binary_name = "vaak-mcp-x86_64-unknown-linux-gnu";

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
/// Creates .mcp.json config so Claude Code can speak through Vaak
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
            log_error(&format!("Found vaak-mcp sidecar at: {:?}", path));
            // Strip Windows extended-length path prefix (\\?\) that canonicalize() adds
            let path_str = path.to_string_lossy().to_string();
            path_str.strip_prefix(r"\\?\").unwrap_or(&path_str).to_string()
        }
        None => {
            // Fallback to vaak-speak if pip-installed (for backwards compatibility)
            log_error("Sidecar not found, falling back to vaak-speak in PATH");
            "vaak-speak".to_string()
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

    // Check if vaak MCP server is already configured with THIS path
    let current_command = config
        .get("mcpServers")
        .and_then(|s| s.get("vaak"))
        .and_then(|s| s.get("command"))
        .and_then(|c| c.as_str())
        .unwrap_or("");

    let needs_update = current_command != sidecar_path;

    if needs_update {
        // Add/update vaak MCP server configuration
        if config.get("mcpServers").is_none() {
            config["mcpServers"] = serde_json::json!({});
        }

        config["mcpServers"]["vaak"] = serde_json::json!({
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
        log_error("Claude Code already configured for Vaak");
    }

    // --- Install UserPromptSubmit and Stop hooks using the sidecar binary ---
    // The sidecar supports --hook and --stop-hook flags.
    // We write .cmd/.sh wrappers to handle paths with spaces.
    let hooks_dir = home.join(".claude").join("hooks");
    if let Err(e) = fs::create_dir_all(&hooks_dir) {
        log_error(&format!("Failed to create hooks dir: {}", e));
        return;
    }

    let hook_command;
    let stop_hook_command;
    #[cfg(windows)]
    {
        // Write a .cmd wrapper for UserPromptSubmit hook
        let cmd_path = hooks_dir.join("vaak-hook.cmd");
        let cmd_content = format!("@echo off\n\"{}\" --hook\n", sidecar_path.replace('/', "\\"));
        if let Err(e) = fs::write(&cmd_path, &cmd_content) {
            log_error(&format!("Failed to write hook wrapper: {}", e));
            return;
        }
        hook_command = cmd_path.to_string_lossy().replace('\\', "/");

        // Write a .cmd wrapper for Stop hook
        let stop_cmd_path = hooks_dir.join("vaak-stop-hook.cmd");
        let stop_cmd_content = format!("@echo off\n\"{}\" --stop-hook\n", sidecar_path.replace('/', "\\"));
        if let Err(e) = fs::write(&stop_cmd_path, &stop_cmd_content) {
            log_error(&format!("Failed to write stop hook wrapper: {}", e));
            return;
        }
        stop_hook_command = stop_cmd_path.to_string_lossy().replace('\\', "/");
    }
    #[cfg(not(windows))]
    {
        // Write a .sh wrapper for UserPromptSubmit hook
        let sh_path = hooks_dir.join("vaak-hook.sh");
        let sh_content = format!("#!/bin/sh\n\"{}\" --hook\n", sidecar_path);
        if let Err(e) = fs::write(&sh_path, &sh_content) {
            log_error(&format!("Failed to write hook wrapper: {}", e));
            return;
        }
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&sh_path, fs::Permissions::from_mode(0o755));
        hook_command = sh_path.to_string_lossy().to_string();

        // Write a .sh wrapper for Stop hook
        let stop_sh_path = hooks_dir.join("vaak-stop-hook.sh");
        let stop_sh_content = format!("#!/bin/sh\n\"{}\" --stop-hook\n", sidecar_path);
        if let Err(e) = fs::write(&stop_sh_path, &stop_sh_content) {
            log_error(&format!("Failed to write stop hook wrapper: {}", e));
            return;
        }
        let _ = fs::set_permissions(&stop_sh_path, fs::Permissions::from_mode(0o755));
        stop_hook_command = stop_sh_path.to_string_lossy().to_string();
    }

    let settings_path = home.join(".claude").join("settings.json");
    let mut settings: serde_json::Value = if settings_path.exists() {
        match fs::read_to_string(&settings_path) {
            Ok(content) => serde_json::from_str(&content).unwrap_or(serde_json::json!({})),
            Err(_) => serde_json::json!({}),
        }
    } else {
        serde_json::json!({})
    };

    // Helper: check if a hook event already has the correct command configured
    let check_hook_configured = |settings: &serde_json::Value, event: &str, command: &str| -> bool {
        settings
            .get("hooks")
            .and_then(|h| h.get(event))
            .and_then(|arr| arr.as_array())
            .map(|arr| {
                arr.iter().any(|entry| {
                    entry.get("hooks")
                        .and_then(|h| h.as_array())
                        .map(|hooks| {
                            hooks.iter().any(|hook| {
                                hook.get("command")
                                    .and_then(|c| c.as_str())
                                    .map(|c| c == command || c.contains("vaak-hook") || c.contains("vaak-stop-hook"))
                                    .unwrap_or(false)
                            })
                        })
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false)
    };

    let prompt_hook_ok = check_hook_configured(&settings, "UserPromptSubmit", &hook_command);
    let stop_hook_ok = check_hook_configured(&settings, "Stop", &stop_hook_command);

    if !prompt_hook_ok || !stop_hook_ok {
        if settings.get("hooks").is_none() {
            settings["hooks"] = serde_json::json!({});
        }

        if !prompt_hook_ok {
            settings["hooks"]["UserPromptSubmit"] = serde_json::json!([
                {
                    "hooks": [
                        {
                            "type": "command",
                            "command": hook_command
                        }
                    ]
                }
            ]);
        }

        if !stop_hook_ok {
            settings["hooks"]["Stop"] = serde_json::json!([
                {
                    "hooks": [
                        {
                            "type": "command",
                            "command": stop_hook_command
                        }
                    ]
                }
            ]);
        }

        match serde_json::to_string_pretty(&settings) {
            Ok(json_str) => {
                if let Err(e) = fs::write(&settings_path, json_str) {
                    log_error(&format!("Failed to write settings.json: {}", e));
                } else {
                    if !prompt_hook_ok {
                        log_error(&format!("Configured UserPromptSubmit hook: {}", hook_command));
                    }
                    if !stop_hook_ok {
                        log_error(&format!("Configured Stop hook: {}", stop_hook_command));
                    }
                }
            }
            Err(e) => {
                log_error(&format!("Failed to serialize settings.json: {}", e));
            }
        }
    } else {
        log_error("Both hooks already configured");
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

        loop {
            // Check shutdown flag
            if HTTP_SERVER_SHUTDOWN.load(Ordering::Relaxed) {
                eprintln!("[speak-server] Shutdown signal received, exiting");
                break;
            }

            // Use recv_timeout so we can check the shutdown flag periodically
            let mut request = match server.recv_timeout(Duration::from_secs(1)) {
                Ok(Some(req)) => req,
                Ok(None) => continue, // timeout, loop to check shutdown flag
                Err(e) => {
                    eprintln!("[speak-server] recv error: {}", e);
                    continue;
                }
            };

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

            // ===== Project notify endpoint (fire-and-forget ping from MCP sidecar) =====
            if method == "POST" && url == "/collab/notify" {
                // Emit event to ALL windows so the collab tab re-reads project files
                if let Some(window) = app_handle.get_webview_window("main") {
                    let _ = window.emit("project-file-changed", serde_json::json!({}));
                }
                if let Some(window) = app_handle.get_webview_window("transcript") {
                    let _ = window.emit("project-file-changed", serde_json::json!({}));
                }
                let response = Response::from_string(r#"{"status":"ok"}"#)
                    .with_header(
                        tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap()
                    );
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

            // Parse JSON - expecting {"text": "...", "session_id": "...", "voice_id": "..."}
            let (text, mut session_id, voice_id) = if let Ok(json) = serde_json::from_str::<serde_json::Value>(&body) {
                let text = json.get("text")
                    .and_then(|t| t.as_str())
                    .unwrap_or("")
                    .to_string();
                let session_id = json.get("session_id")
                    .and_then(|s| s.as_str())
                    .map(|s| s.to_string());
                let voice_id = json.get("voice_id")
                    .and_then(|v| v.as_str())
                    .map(|v| v.to_string());
                (text, session_id, voice_id)
            } else {
                // If not JSON, treat the whole body as text
                (body.trim().to_string(), None, None)
            };

            if text.is_empty() {
                let response = Response::from_string("No text provided").with_status_code(400);
                let _ = request.respond(response);
                continue;
            }

            // Check if voice is enabled — if not, respond OK but don't queue or emit
            let voice_settings = load_voice_settings();
            if !voice_settings.enabled {
                let response_body = serde_json::json!({
                    "status": "ok",
                    "skipped": true,
                    "reason": "voice disabled"
                }).to_string();
                let response = Response::from_string(response_body)
                    .with_status_code(200)
                    .with_header(tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap());
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

            // ===== Speaker lock: only one session speaks at a time =====
            {
                let mut speaker = get_active_speaker().lock();
                let (ref active_id, ref last_ts) = *speaker;
                let elapsed = timestamp.saturating_sub(*last_ts);

                if let Some(ref locked_id) = active_id {
                    if locked_id != &session_id && elapsed < SPEAKER_LOCK_TIMEOUT_MS {
                        // Another session owns the speaker and hasn't timed out — silently drop
                        log_error(&format!(
                            "Vaak: Speaker locked by {} ({}ms ago). Dropping speak from {}",
                            locked_id, elapsed, session_id
                        ));
                        let response_body = serde_json::json!({
                            "status": "ok",
                            "skipped": true,
                            "reason": "another session is speaking"
                        }).to_string();
                        let response = Response::from_string(response_body)
                            .with_status_code(200)
                            .with_header(tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap());
                        let _ = request.respond(response);
                        continue;
                    }
                }

                // This session is now the active speaker
                *speaker = (Some(session_id.clone()), timestamp);
            }

            // Add to queue database
            let queue_item = match queue::add_queue_item(text.clone(), session_id.clone()) {
                Ok(item) => Some(item),
                Err(e) => {
                    log_error(&format!("Failed to add item to queue: {}", e));
                    None
                }
            };

            // Create payload with text, session_id, timestamp, queue item info, and optional voice override
            let payload = serde_json::json!({
                "text": text,
                "session_id": session_id,
                "timestamp": timestamp,
                "queue_item": queue_item,
                "voice_id": voice_id
            });

            // Emit event to frontend - emit to main window
            log_error(&format!("Vaak: Emitting speak event to MAIN window - Session: {}, Text: {:.50}", session_id, text));
            if let Some(window) = app_handle.get_webview_window("main") {
                if let Err(e) = window.emit("speak", &payload) {
                    log_error(&format!("Failed to emit speak event to window: {}", e));
                }
            } else {
                log_error("Main window not found for speak event");
            }

            // Also emit to transcript window if it exists
            log_error(&format!("Vaak: Emitting speak event to TRANSCRIPT window - Session: {}", session_id));
            if let Some(window) = app_handle.get_webview_window("transcript") {
                let _ = window.emit("speak-transcript", &payload);
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

/// Screen reader conversation state for Alt+A follow-up questions
struct ScreenReaderConversation {
    image_base64: Option<String>,
    messages: Vec<(String, String)>, // (role, content) pairs
    sr_settings: SRSettings,
}

impl Default for ScreenReaderConversation {
    fn default() -> Self {
        Self {
            image_base64: None,
            messages: Vec::new(),
            sr_settings: SRSettings::default(),
        }
    }
}

/// Tauri-managed conversation state
struct ScreenReaderConversationState(pub Mutex<ScreenReaderConversation>);

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


/// Core screen capture and description logic (used by both command and hotkey handler)
fn describe_screen_core(app: &tauri::AppHandle) -> Result<String, String> {
    use base64::Engine;
    use screenshots::Screen;

    // 1. Capture screenshot (primary screen)
    let screens = Screen::all().map_err(|e| format!("Failed to enumerate screens: {}", e))?;
    if screens.is_empty() {
        return Err("No screens found".to_string());
    }
    let screen = &screens[0];
    let image = screen.capture().map_err(|e| {
        // On macOS, a capture failure likely means Screen Recording permission is missing
        #[cfg(target_os = "macos")]
        {
            return format!(
                "Screen capture failed. You may need to grant Screen Recording permission: \
                 System Settings, Privacy and Security, Screen Recording, enable Vaak, then restart the app. \
                 Error: {}", e
            );
        }
        #[cfg(not(target_os = "macos"))]
        format!("Failed to capture screen: {}", e)
    })?;

    // 2. Resize if too large (keeps vision API happy and reduces payload)
    let (w, h) = image.dimensions();
    let max_width = 1920u32;
    let final_image: screenshots::image::DynamicImage = if w > max_width {
        let new_h = (h as f64 * max_width as f64 / w as f64) as u32;
        screenshots::image::DynamicImage::from(image).resize_exact(
            max_width, new_h,
            screenshots::image::imageops::FilterType::Triangle,
        )
    } else {
        screenshots::image::DynamicImage::from(image)
    };

    // 3. Encode to PNG then base64
    let mut png_bytes = Vec::new();
    {
        let mut cursor = std::io::Cursor::new(&mut png_bytes);
        final_image.write_to(&mut cursor, screenshots::image::ImageOutputFormat::Png).map_err(|e| format!("Failed to encode PNG: {}", e))?;
    }
    let image_base64 = base64::engine::general_purpose::STANDARD.encode(&png_bytes);

    // 3. Load screen reader settings from disk
    let sr_settings = load_sr_settings_from_disk();
    let voice_settings = load_voice_settings();

    // 3b. Capture accessibility tree (best-effort, don't fail if unavailable)
    let uia_tree_json = match a11y::capture_tree() {
        Ok(tree) => {
            let text = a11y::format_tree_for_prompt(&tree);
            log_error(&format!("A11y tree captured: {} elements", tree.element_count));
            Some(text)
        }
        Err(e) => {
            log_error(&format!("A11y capture failed (non-fatal): {}", e));
            None
        }
    };

    // 4. POST to backend /api/v1/describe-screen
    let client = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(60))
        .build();

    let mut body = serde_json::json!({
        "image_base64": image_base64,
        "blind_mode": voice_settings.blind_mode,
        "detail": sr_settings.detail,
        "model": sr_settings.model,
        "focus": sr_settings.focus,
    });
    if let Some(ref tree_text) = uia_tree_json {
        body["uia_tree"] = serde_json::Value::String(tree_text.clone());
    }

    let response = client.post(&format!("{}/api/v1/describe-screen", get_backend_url()))
        .set("Content-Type", "application/json")
        .send_string(&body.to_string())
        .map_err(|e| format!("Backend request failed: {}", e))?;

    let resp_body: String = response.into_string()
        .map_err(|e| format!("Failed to read response: {}", e))?;
    let resp_json: serde_json::Value = serde_json::from_str(&resp_body)
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    let description = resp_json.get("description")
        .and_then(|d| d.as_str())
        .unwrap_or("No description returned")
        .to_string();

    // Store screenshot and initial description in conversation state for Alt+A follow-ups
    {
        let conv_state = app.state::<ScreenReaderConversationState>();
        let mut conv = conv_state.0.lock();
        conv.image_base64 = Some(image_base64.clone());
        conv.messages.clear();
        conv.messages.push(("user".to_string(), "Describe what you see on this screen.".to_string()));
        conv.messages.push(("assistant".to_string(), description.clone()));
        conv.sr_settings = sr_settings.clone();
    }

    // 5. Emit speak event with screen reader voice
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    let payload = serde_json::json!({
        "text": description,
        "session_id": format!("screen-reader-{}", timestamp),
        "timestamp": timestamp,
        "voice_id": sr_settings.voice_id,
    });

    if let Some(window) = app.get_webview_window("main") {
        let _ = window.emit("speak", &payload);
    }

    Ok(description)
}

/// Capture the screen, send to vision API, speak the description, return it
#[tauri::command]
fn describe_screen(app: tauri::AppHandle) -> Result<String, String> {
    describe_screen_core(&app)
}

/// Capture the accessibility tree from the foreground window and return as JSON
#[tauri::command]
fn capture_uia_tree_cmd() -> Result<serde_json::Value, String> {
    let tree = a11y::capture_tree()?;
    serde_json::to_value(&tree).map_err(|e| format!("Serialize failed: {}", e))
}

/// Start/stop focus tracking for automatic announcements
#[tauri::command]
fn set_focus_tracking(app: tauri::AppHandle, enabled: bool) -> Result<(), String> {
    if enabled {
        a11y::start_focus_tracking(app);
    } else {
        a11y::stop_focus_tracking();
    }
    Ok(())
}

/// Load screen reader settings from disk (for describe_screen command)
fn load_sr_settings_from_disk() -> SRSettings {
    let home_var = if cfg!(target_os = "windows") { "APPDATA" } else { "HOME" };
    let path = std::env::var_os(home_var).map(|home| {
        if cfg!(target_os = "windows") {
            PathBuf::from(home).join("Vaak").join("screen-reader-settings.json")
        } else {
            PathBuf::from(home).join(".vaak").join("screen-reader-settings.json")
        }
    });

    match path {
        Some(p) => match fs::read_to_string(&p) {
            Ok(contents) => serde_json::from_str(&contents).unwrap_or_default(),
            Err(_) => SRSettings::default(),
        },
        None => SRSettings::default(),
    }
}

#[derive(serde::Deserialize, serde::Serialize, Clone, Debug)]
struct SRSettings {
    #[serde(default = "default_sr_model")]
    model: String,
    #[serde(default = "default_sr_detail")]
    detail: u8,
    #[serde(default = "default_sr_focus")]
    focus: String,
    #[serde(default = "default_sr_voice")]
    voice_id: String,
}

fn default_sr_model() -> String { "claude-3-5-haiku-20241022".to_string() }
fn default_sr_detail() -> u8 { 3 }
fn default_sr_focus() -> String { "general".to_string() }
fn default_sr_voice() -> String { "jiIkqWtTmS0GBz46iqA0".to_string() } // Ravi

impl Default for SRSettings {
    fn default() -> Self {
        Self {
            model: default_sr_model(),
            detail: default_sr_detail(),
            focus: default_sr_focus(),
            voice_id: default_sr_voice(),
        }
    }
}

/// Capture screenshot and return (base64_string, width, height)
fn capture_screenshot_base64_with_size(max_width: u32) -> Result<(String, u32, u32), String> {
    use base64::Engine;
    use screenshots::Screen;

    // On macOS, check Screen Recording permission before attempting capture
    #[cfg(target_os = "macos")]
    check_screen_recording_permission()?;

    let screens = Screen::all().map_err(|e| format!("Failed to enumerate screens: {}", e))?;
    if screens.is_empty() {
        return Err("No screens found".to_string());
    }
    let screen = &screens[0];
    let image = screen.capture().map_err(|e| {
        #[cfg(target_os = "macos")]
        {
            return format!(
                "Screen capture failed. You may need to grant Screen Recording permission: \
                 System Settings, Privacy and Security, Screen Recording, enable Vaak, then restart the app. \
                 Error: {}", e
            );
        }
        #[cfg(not(target_os = "macos"))]
        format!("Failed to capture screen: {}", e)
    })?;

    let (w, h) = image.dimensions();
    let final_image: screenshots::image::DynamicImage = if w > max_width {
        let new_h = (h as f64 * max_width as f64 / w as f64) as u32;
        screenshots::image::DynamicImage::from(image).resize_exact(
            max_width, new_h,
            screenshots::image::imageops::FilterType::Triangle,
        )
    } else {
        screenshots::image::DynamicImage::from(image)
    };

    let (final_w, final_h) = (final_image.width(), final_image.height());

    let mut png_bytes = Vec::new();
    {
        let mut cursor = std::io::Cursor::new(&mut png_bytes);
        final_image.write_to(&mut cursor, screenshots::image::ImageOutputFormat::Png)
            .map_err(|e| format!("Failed to encode PNG: {}", e))?;
    }
    let image_base64 = base64::engine::general_purpose::STANDARD.encode(&png_bytes);

    Ok((image_base64, final_w, final_h))
}

/// Capture screenshot at default resolution (1280px for computer use)
fn capture_screenshot_base64() -> Result<(String, u32, u32), String> {
    capture_screenshot_base64_with_size(1280)
}

/// Get the actual primary screen dimensions
fn get_screen_dimensions() -> (u32, u32) {
    use screenshots::Screen;
    match Screen::all() {
        Ok(screens) if !screens.is_empty() => {
            let s = &screens[0];
            let di = s.display_info;
            (di.width as u32, di.height as u32)
        }
        _ => (1920, 1080),
    }
}

/// Execute a computer use action from Anthropic's tool_use response
/// scale_x, scale_y: multiply Claude's coordinates by these to get real screen coords
fn execute_computer_action(input: &serde_json::Value, scale_x: f64, scale_y: f64) -> Result<(), String> {
    // On macOS, check Accessibility permission before attempting input simulation.
    #[cfg(target_os = "macos")]
    check_accessibility_permission()?;

    let mut enigo = Enigo::new(&Settings::default()).map_err(|e| format!("{}", e))?;

    let action = input.get("action").and_then(|a| a.as_str()).unwrap_or("");

    match action {
        "left_click" => {
            if let Some(coords) = input.get("coordinate").and_then(|c| c.as_array()) {
                let x = (coords.first().and_then(|v| v.as_f64()).unwrap_or(0.0) * scale_x) as i32;
                let y = (coords.get(1).and_then(|v| v.as_f64()).unwrap_or(0.0) * scale_y) as i32;
                enigo.move_mouse(x, y, Coordinate::Abs).map_err(|e| format!("{}", e))?;
                thread::sleep(Duration::from_millis(50));
                enigo.button(enigo::Button::Left, enigo::Direction::Click).map_err(|e| format!("{}", e))?;
            }
        }
        "right_click" => {
            if let Some(coords) = input.get("coordinate").and_then(|c| c.as_array()) {
                let x = (coords.first().and_then(|v| v.as_f64()).unwrap_or(0.0) * scale_x) as i32;
                let y = (coords.get(1).and_then(|v| v.as_f64()).unwrap_or(0.0) * scale_y) as i32;
                enigo.move_mouse(x, y, Coordinate::Abs).map_err(|e| format!("{}", e))?;
                thread::sleep(Duration::from_millis(50));
                enigo.button(enigo::Button::Right, enigo::Direction::Click).map_err(|e| format!("{}", e))?;
            }
        }
        "double_click" => {
            if let Some(coords) = input.get("coordinate").and_then(|c| c.as_array()) {
                let x = (coords.first().and_then(|v| v.as_f64()).unwrap_or(0.0) * scale_x) as i32;
                let y = (coords.get(1).and_then(|v| v.as_f64()).unwrap_or(0.0) * scale_y) as i32;
                enigo.move_mouse(x, y, Coordinate::Abs).map_err(|e| format!("{}", e))?;
                thread::sleep(Duration::from_millis(50));
                enigo.button(enigo::Button::Left, enigo::Direction::Click).map_err(|e| format!("{}", e))?;
                thread::sleep(Duration::from_millis(50));
                enigo.button(enigo::Button::Left, enigo::Direction::Click).map_err(|e| format!("{}", e))?;
            }
        }
        "middle_click" => {
            if let Some(coords) = input.get("coordinate").and_then(|c| c.as_array()) {
                let x = (coords.first().and_then(|v| v.as_f64()).unwrap_or(0.0) * scale_x) as i32;
                let y = (coords.get(1).and_then(|v| v.as_f64()).unwrap_or(0.0) * scale_y) as i32;
                enigo.move_mouse(x, y, Coordinate::Abs).map_err(|e| format!("{}", e))?;
                thread::sleep(Duration::from_millis(50));
                enigo.button(enigo::Button::Middle, enigo::Direction::Click).map_err(|e| format!("{}", e))?;
            }
        }
        "mouse_move" => {
            if let Some(coords) = input.get("coordinate").and_then(|c| c.as_array()) {
                let x = (coords.first().and_then(|v| v.as_f64()).unwrap_or(0.0) * scale_x) as i32;
                let y = (coords.get(1).and_then(|v| v.as_f64()).unwrap_or(0.0) * scale_y) as i32;
                enigo.move_mouse(x, y, Coordinate::Abs).map_err(|e| format!("{}", e))?;
            }
        }
        "type" => {
            let text = input.get("text").and_then(|t| t.as_str()).unwrap_or("");
            enigo.text(text).map_err(|e| format!("{}", e))?;
        }
        "key" => {
            let key_str = input.get("key").and_then(|k| k.as_str()).unwrap_or("");
            log_error(&format!("execute_computer_action: key input = {:?}", key_str));
            use enigo::Key;

            // Helper to map a single key name to enigo Key
            let map_key = |name: &str| -> Option<Key> {
                match name.to_lowercase().as_str() {
                    "return" | "enter" => Some(Key::Return),
                    "tab" => Some(Key::Tab),
                    "escape" | "esc" => Some(Key::Escape),
                    "backspace" => Some(Key::Backspace),
                    "delete" => Some(Key::Delete),
                    "space" | " " => Some(Key::Unicode(' ')),
                    "up" => Some(Key::UpArrow),
                    "down" => Some(Key::DownArrow),
                    "left" => Some(Key::LeftArrow),
                    "right" => Some(Key::RightArrow),
                    "home" => Some(Key::Home),
                    "end" => Some(Key::End),
                    "pageup" | "page_up" => Some(Key::PageUp),
                    "pagedown" | "page_down" => Some(Key::PageDown),
                    "f1" => Some(Key::F1),
                    "f2" => Some(Key::F2),
                    "f3" => Some(Key::F3),
                    "f4" => Some(Key::F4),
                    "f5" => Some(Key::F5),
                    "f6" => Some(Key::F6),
                    "f7" => Some(Key::F7),
                    "f8" => Some(Key::F8),
                    "f9" => Some(Key::F9),
                    "f10" => Some(Key::F10),
                    "f11" => Some(Key::F11),
                    "f12" => Some(Key::F12),
                    "alt" => Some(Key::Alt),
                    "ctrl" | "control" => Some(Key::Control),
                    "shift" => Some(Key::Shift),
                    "meta" | "super" | "win" | "command" | "cmd" => Some(Key::Meta),
                    "capslock" | "caps_lock" => Some(Key::CapsLock),
                    "insert" => {
                        #[cfg(target_os = "macos")]
                        { Some(Key::Other(0x72)) } // kVK_Help (Insert equivalent on Mac)
                        #[cfg(not(target_os = "macos"))]
                        { Some(Key::Other(0x2D)) } // VK_INSERT
                    },
                    "numlock" | "num_lock" => {
                        #[cfg(target_os = "macos")]
                        { Some(Key::Other(0x47)) } // kVK_ANSI_KeypadClear (NumLock equivalent)
                        #[cfg(not(target_os = "macos"))]
                        { Some(Key::Other(0x90)) } // VK_NUMLOCK
                    },
                    // These keys don't exist on macOS keyboards
                    "printscreen" | "print_screen" => {
                        #[cfg(target_os = "macos")]
                        { None }
                        #[cfg(not(target_os = "macos"))]
                        { Some(Key::Other(0x2C)) } // VK_SNAPSHOT
                    },
                    "scrolllock" | "scroll_lock" => {
                        #[cfg(target_os = "macos")]
                        { None }
                        #[cfg(not(target_os = "macos"))]
                        { Some(Key::Other(0x91)) } // VK_SCROLL
                    },
                    "pause" | "break" => {
                        #[cfg(target_os = "macos")]
                        { None }
                        #[cfg(not(target_os = "macos"))]
                        { Some(Key::Other(0x13)) } // VK_PAUSE
                    },
                    "contextmenu" | "apps" => {
                        #[cfg(target_os = "macos")]
                        { None }
                        #[cfg(not(target_os = "macos"))]
                        { Some(Key::Other(0x5D)) } // VK_APPS
                    },
                    other if other.len() == 1 => Some(Key::Unicode(other.chars().next().unwrap())),
                    _ => None,
                }
            };

            // Handle combo keys like "alt+F4", "ctrl+w", "ctrl+shift+s"
            let parts: Vec<&str> = key_str.split('+').collect();
            if parts.len() > 1 {
                // It's a combo — press modifiers, click last key, release modifiers
                let mut modifiers = Vec::new();
                for part in &parts[..parts.len()-1] {
                    if let Some(k) = map_key(part.trim()) {
                        enigo.key(k, enigo::Direction::Press).map_err(|e| format!("{}", e))?;
                        modifiers.push(k);
                        thread::sleep(Duration::from_millis(20));
                    }
                }
                // Click the final key
                if let Some(k) = map_key(parts.last().unwrap().trim()) {
                    enigo.key(k, enigo::Direction::Click).map_err(|e| format!("{}", e))?;
                }
                thread::sleep(Duration::from_millis(20));
                // Release modifiers in reverse order
                for k in modifiers.iter().rev() {
                    enigo.key(*k, enigo::Direction::Release).map_err(|e| format!("{}", e))?;
                }
            } else {
                // Single key
                if let Some(k) = map_key(key_str) {
                    enigo.key(k, enigo::Direction::Click).map_err(|e| format!("{}", e))?;
                } else if key_str.is_empty() {
                    log_error("execute_computer_action: empty key string, ignoring");
                } else {
                    return Err(format!("Unknown key: {}", key_str));
                }
            }
        }
        "scroll" => {
            if let Some(coords) = input.get("coordinate").and_then(|c| c.as_array()) {
                let x = (coords.first().and_then(|v| v.as_f64()).unwrap_or(0.0) * scale_x) as i32;
                let y = (coords.get(1).and_then(|v| v.as_f64()).unwrap_or(0.0) * scale_y) as i32;
                enigo.move_mouse(x, y, Coordinate::Abs).map_err(|e| format!("{}", e))?;
            }
            if let Some(delta) = input.get("delta").and_then(|d| d.as_array()) {
                let dx = delta.first().and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                let dy = delta.get(1).and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                log_error(&format!("scroll: raw delta=[{}, {}]", dx, dy));
                // Claude sends small delta values (3-5), but enigo scroll uses "clicks"
                // Multiply by 5 so scrolling is actually noticeable
                let amplified = if dy != 0 { dy * 5 } else { dx * 5 };
                let axis = if dy != 0 { enigo::Axis::Vertical } else { enigo::Axis::Horizontal };
                log_error(&format!("scroll: amplified={}, axis={:?}", amplified, if dy != 0 { "Vertical" } else { "Horizontal" }));
                enigo.scroll(amplified, axis).map_err(|e| format!("{}", e))?;
            }
        }
        "screenshot" => {
            // No-op — Rust takes screenshot automatically after each action
        }
        "wait" => {
            // Claude wants to wait for page to load
            let duration = input.get("duration").and_then(|d| d.as_u64()).unwrap_or(1000);
            let capped = duration.min(5000); // Cap at 5 seconds
            thread::sleep(Duration::from_millis(capped));
        }
        _ => return Err(format!("Unknown action: {}", action)),
    }

    // Small delay after action for UI to update
    thread::sleep(Duration::from_millis(200));
    Ok(())
}

/// Save voice settings when toggled from frontend (voice on/off, blind mode, detail)
#[tauri::command]
fn update_claude_md(enabled: bool, blind_mode: bool, detail: u8) -> Result<(), String> {
    save_voice_settings(enabled, blind_mode, detail)
}

/// Generate voice template based on blind mode and detail level
fn generate_voice_template(blind_mode: bool, detail: u8) -> String {
    let base_instruction = r#"# Claude Code Instructions

## Voice Output

Always use the Vaak speak integration to read responses aloud.

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

/// Update the native keyboard hook hotkey
#[tauri::command]
fn update_native_hotkey(hotkey: String) -> Result<(), String> {
    keyboard_hook::update_hotkey(&hotkey);
    Ok(())
}

/// Update tray icon, title, and tooltip to show recording/processing state.
/// `state` is one of: "idle", "recording", "processing", "success", "error"
/// On macOS, also sets tray title text (visible next to icon in menu bar)
/// and bounces the dock icon on recording start and processing complete.
#[tauri::command]
fn set_recording_state(app: tauri::AppHandle, recording: bool, state: Option<String>) -> Result<(), String> {
    let state_str = state.as_deref().unwrap_or(if recording { "recording" } else { "idle" });

    if let Some(tray) = app.tray_by_id("main-tray") {
        // Icon: recording uses red icon, everything else uses idle
        let icon_bytes: &[u8] = if state_str == "recording" {
            include_bytes!("../icons/tray-recording.png")
        } else if state_str == "processing" {
            include_bytes!("../icons/tray-recording.png") // keep red during processing
        } else {
            include_bytes!("../icons/tray-idle.png")
        };
        if let Ok(icon) = load_png_image(icon_bytes) {
            let _ = tray.set_icon(Some(icon));
        }

        // Tooltip: detailed state description
        let tooltip = match state_str {
            "recording" => "Vaak - Recording...",
            "processing" => "Vaak - Processing transcription...",
            "success" => "Vaak - Transcription complete",
            "error" => "Vaak - Transcription failed",
            _ => "Vaak - Ready",
        };
        let _ = tray.set_tooltip(Some(tooltip));

        // macOS: set tray title text (appears next to the tray icon in menu bar)
        #[cfg(target_os = "macos")]
        {
            let title: Option<&str> = match state_str {
                "recording" => Some("REC"),
                "processing" => Some("..."),
                _ => None, // clear title for idle/success/error
            };
            let _ = tray.set_title(title);
        }
    }

    // macOS: dock bounce on recording start and processing complete
    #[cfg(target_os = "macos")]
    {
        use tauri::UserAttentionType;
        if state_str == "recording" || state_str == "success" {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.request_user_attention(Some(UserAttentionType::Informational));
            }
        }
    }

    Ok(())
}

// ==================== Project File Watcher ====================

/// Shared state for the project file watcher
static PROJECT_WATCHED_DIR: std::sync::OnceLock<Mutex<Option<String>>> = std::sync::OnceLock::new();
static PROJECT_LAST_MTIMES: std::sync::OnceLock<Mutex<(Option<std::time::SystemTime>, Option<std::time::SystemTime>, Option<std::time::SystemTime>, Option<std::time::SystemTime>)>> = std::sync::OnceLock::new();

fn get_project_watched_dir() -> &'static Mutex<Option<String>> {
    PROJECT_WATCHED_DIR.get_or_init(|| Mutex::new(None))
}

fn get_project_last_mtimes() -> &'static Mutex<(Option<std::time::SystemTime>, Option<std::time::SystemTime>, Option<std::time::SystemTime>, Option<std::time::SystemTime>)> {
    PROJECT_LAST_MTIMES.get_or_init(|| Mutex::new((None, None, None, None)))
}

/// Tauri command: start watching a project directory for .vaak/ file changes
#[tauri::command]
fn watch_project_dir(dir: String) -> Result<serde_json::Value, String> {
    // Validate path to prevent traversal attacks
    let dir = validate_project_dir(&dir)?;

    // Determine the best directory to watch:
    // If the specified dir has an inactive/empty .vaak/ but a subdirectory has an active one, prefer the subdirectory.
    let effective_dir = find_best_vaak_dir(&dir);

    let vaak_dir = std::path::Path::new(&effective_dir).join(".vaak");

    // Compact board.jsonl on load: remove messages older than 7 days, keep at least 200
    match collab::compact_board(&effective_dir, 7, 200) {
        Ok((kept, removed)) => {
            if removed > 0 {
                eprintln!("[collab] Board compacted: kept {} messages, removed {} old messages", kept, removed);
            }
        }
        Err(e) => eprintln!("[collab] Board compaction failed (non-fatal): {}", e),
    }

    // Read and parse full project state
    let parsed = collab::parse_project_dir(&effective_dir);

    // Store mtimes for all watched files
    let project_mtime = vaak_dir.join("project.json").metadata().ok().and_then(|m| m.modified().ok());
    let sessions_mtime = vaak_dir.join("sessions.json").metadata().ok().and_then(|m| m.modified().ok());
    let board_mtime = collab::active_board_path(&effective_dir).metadata().ok().and_then(|m| m.modified().ok());
    let claims_mtime = vaak_dir.join("claims.json").metadata().ok().and_then(|m| m.modified().ok());
    *get_project_last_mtimes().lock() = (project_mtime, sessions_mtime, board_mtime, claims_mtime);

    // Store the effective dir (may differ from user's input if we found a better subdirectory)
    let effective_dir_for_response = effective_dir.clone();
    *get_project_watched_dir().lock() = Some(effective_dir);

    // Sync auto_collab/human_in_loop from voice settings to project.json on every project load
    sync_collab_settings_to_project();

    match parsed {
        Some(project) => {
            let mut val = serde_json::to_value(&project).unwrap_or(serde_json::json!(null));
            // Include effective_dir so the UI knows the actual watched path
            if let Some(obj) = val.as_object_mut() {
                obj.insert("effective_dir".to_string(), serde_json::json!(effective_dir_for_response));
            }
            Ok(val)
        }
        None => Ok(serde_json::json!(null)),
    }
}

/// Find the best .vaak/ directory to watch. If the given dir has an inactive project
/// but a subdirectory has an active one (with sessions/messages), prefer the subdirectory.
fn find_best_vaak_dir(dir: &str) -> String {
    let base = std::path::Path::new(dir);
    let vaak_dir = base.join(".vaak");

    // Check if the specified directory has an active .vaak/
    if vaak_dir.join("sessions.json").exists() {
        if let Ok(content) = std::fs::read_to_string(vaak_dir.join("sessions.json")) {
            if let Ok(sessions) = serde_json::from_str::<serde_json::Value>(&content) {
                let bindings = sessions.get("bindings")
                    .and_then(|b| b.as_array())
                    .map(|a| a.len())
                    .unwrap_or(0);
                if bindings > 0 {
                    // This dir has active sessions, use it
                    return dir.to_string();
                }
            }
        }
    }

    // No active sessions here — scan immediate subdirectories for .vaak/ with active sessions
    let mut best_dir = dir.to_string();
    let mut best_score: usize = 0; // sessions + messages

    if let Ok(entries) = std::fs::read_dir(base) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() { continue; }
            let sub_vaak = path.join(".vaak");
            if !sub_vaak.join("project.json").exists() { continue; }

            let mut score: usize = 0;

            // Count active sessions
            if let Ok(content) = std::fs::read_to_string(sub_vaak.join("sessions.json")) {
                if let Ok(sessions) = serde_json::from_str::<serde_json::Value>(&content) {
                    score += sessions.get("bindings")
                        .and_then(|b| b.as_array())
                        .map(|a| a.len())
                        .unwrap_or(0);
                }
            }

            // Count messages
            if let Ok(content) = std::fs::read_to_string(sub_vaak.join("board.jsonl")) {
                score += content.lines().filter(|l| !l.trim().is_empty()).count();
            }

            if score > best_score {
                best_score = score;
                best_dir = path.to_string_lossy().to_string();
            }
        }
    }

    best_dir
}

#[tauri::command]
fn initialize_project(dir: String, config: String) -> Result<(), String> {
    // Relaxed validation: no ".." components, must be an existing directory
    // (can't require .vaak/ since we're creating it)
    let path = std::path::Path::new(&dir);
    for component in path.components() {
        if let std::path::Component::ParentDir = component {
            return Err("Invalid directory: path traversal not allowed".to_string());
        }
    }
    let canonical = path.canonicalize()
        .map_err(|e| format!("Invalid directory '{}': {}", dir, e))?;
    if !canonical.is_dir() {
        return Err(format!("Not a directory: {}", canonical.display()));
    }
    // Strip Windows extended-length prefix (\\?\) to match validate_project_dir
    let raw = canonical.to_string_lossy().to_string();
    let dir = raw.strip_prefix("\\\\?\\").unwrap_or(&raw).to_string();

    let vaak_dir = std::path::Path::new(&dir).join(".vaak");
    let roles_dir = vaak_dir.join("roles");
    let last_seen_dir = vaak_dir.join("last-seen");

    // Create directories
    std::fs::create_dir_all(&roles_dir)
        .map_err(|e| format!("Failed to create .vaak/roles: {}", e))?;
    std::fs::create_dir_all(&last_seen_dir)
        .map_err(|e| format!("Failed to create .vaak/last-seen: {}", e))?;

    // Write project.json (pretty-printed)
    let parsed: serde_json::Value = serde_json::from_str(&config)
        .map_err(|e| format!("Invalid config JSON: {}", e))?;
    let pretty = serde_json::to_string_pretty(&parsed)
        .map_err(|e| format!("Failed to format config: {}", e))?;
    std::fs::write(vaak_dir.join("project.json"), pretty)
        .map_err(|e| format!("Failed to write project.json: {}", e))?;

    // Write empty sessions.json
    std::fs::write(vaak_dir.join("sessions.json"), "{\"bindings\":[]}")
        .map_err(|e| format!("Failed to write sessions.json: {}", e))?;

    // Create empty board.jsonl (default section uses root .vaak/ path)
    std::fs::write(vaak_dir.join("board.jsonl"), "")
        .map_err(|e| format!("Failed to write board.jsonl: {}", e))?;

    // Create sections directory for future use
    let _ = std::fs::create_dir_all(vaak_dir.join("sections"));

    // Write default role briefings for all four roles
    let briefings: &[(&str, &str)] = &[
        ("architect.md", "# Architect\n\nYou are the Architect on this project. You own the technical vision and ensure every piece of work aligns with it.\n\n## Core Responsibilities\n\n- **Maintain the Vision**: Keep a living document of the project's architecture, patterns, and design principles.\n- **Review for Consistency**: Review work for architectural coherence — consistent patterns, proper separation of concerns.\n- **Guide Technical Decisions**: Weigh in on library, pattern, and data structure choices.\n- **Prevent Drift**: Watch for shortcuts, tech debt, or deviations from established patterns.\n\n## Vision Document\n\nYou MUST maintain a file called `.vaak/vision.md` in the project root. Update it as the project evolves.\n\n## Communication\n\n- Use `project_send(to=\"manager\", type=\"review\", ...)` for architectural feedback\n- Use `project_send(to=\"developer\", type=\"directive\", ...)` to request changes\n- Use `project_send(to=\"all\", type=\"broadcast\", ...)` for architecture announcements\n- Use `project_check(0)` to see all messages\n\n## Workflow Types & Voting\n\nThe project runs under one of three workflow types: `full` (Full Review — complete onboarding + planning + full review pipeline), `quick` (Quick Feature — skip onboarding, abbreviated review cycle), `bugfix` (Bug Fix — focused diagnosis and fix, minimal review). No workflow is set by default.\n\nAny team member can propose a workflow change via voting. The human can override directly via the UI dropdown (bypasses voting).\n\nTo propose a change: `project_send(to=\"all\", type=\"vote\", subject=\"Workflow change: Quick Feature\", body=\"Reason...\", metadata={\"vote_type\": \"workflow_change\", \"proposed_value\": \"quick\", \"vote\": \"yes\"})`\n\nTo vote yes/no: `project_send(to=\"all\", type=\"vote\", subject=\"Re: Workflow change\", body=\"Agreed\", metadata={\"vote_type\": \"workflow_change\", \"in_reply_to\": <id>, \"vote\": \"yes\"})`\n\nMajority = floor(n/2) + 1 where n = active members + human.\n"),
        ("manager.md", "# Project Manager\n\nYou are the Project Manager. You coordinate the team and keep work flowing smoothly.\n\n## Core Responsibilities\n\n- **Break Down Work**: Split high-level goals into clear, actionable tasks.\n- **Assign Tasks**: Direct developers to specific work.\n- **Review Work**: Check completed work for correctness.\n- **Coordinate the Team**: Keep architect, developers, and testers aligned.\n\n## Communication\n\n- Use `project_send(to=\"developer\", type=\"directive\", ...)` to assign tasks\n- Use `project_send(to=\"architect\", type=\"question\", ...)` for architectural guidance\n- Use `project_send(to=\"tester\", type=\"directive\", ...)` to request testing\n- Use `project_send(to=\"all\", type=\"broadcast\", ...)` for team announcements\n- Use `project_check(0)` to see all messages\n\n## Workflow Types & Voting\n\nThe project runs under one of three workflow types: `full` (Full Review — complete onboarding + planning + full review pipeline), `quick` (Quick Feature — skip onboarding, abbreviated review cycle), `bugfix` (Bug Fix — focused diagnosis and fix, minimal review). No workflow is set by default.\n\nAny team member can propose a workflow change via voting. The human can override directly via the UI dropdown (bypasses voting).\n\nTo propose a change: `project_send(to=\"all\", type=\"vote\", subject=\"Workflow change: Quick Feature\", body=\"Reason...\", metadata={\"vote_type\": \"workflow_change\", \"proposed_value\": \"quick\", \"vote\": \"yes\"})`\n\nTo vote yes/no: `project_send(to=\"all\", type=\"vote\", subject=\"Re: Workflow change\", body=\"Agreed\", metadata={\"vote_type\": \"workflow_change\", \"in_reply_to\": <id>, \"vote\": \"yes\"})`\n\nMajority = floor(n/2) + 1 where n = active members + human.\n"),
        ("developer.md", "# Developer\n\nYou are a Developer on this project. You write the code that brings the project to life.\n\n## Core Responsibilities\n\n- **Implement Features**: Build what the manager assigns, following the architect's patterns.\n- **Fix Bugs**: Diagnose and resolve issues reported by the tester or manager.\n- **Write Clean Code**: Follow the project's established patterns and conventions.\n- **Report Progress**: Keep the team informed about your work and blockers.\n\n## Communication\n\n- Use `project_send(to=\"manager\", type=\"status\", ...)` to report progress\n- Use `project_send(to=\"manager\", type=\"question\", ...)` to ask for clarification\n- Use `project_send(to=\"manager\", type=\"handoff\", ...)` when work is complete\n- Use `project_send(to=\"architect\", type=\"question\", ...)` for architectural questions\n- Use `project_send(to=\"tester\", type=\"handoff\", ...)` to pass work for testing\n- Use `project_check(0)` to see all messages directed to you\n\n## Workflow Types & Voting\n\nThe project runs under one of three workflow types: `full` (Full Review — complete onboarding + planning + full review pipeline), `quick` (Quick Feature — skip onboarding, abbreviated review cycle), `bugfix` (Bug Fix — focused diagnosis and fix, minimal review). No workflow is set by default.\n\nAny team member can propose a workflow change via voting. The human can override directly via the UI dropdown (bypasses voting).\n\nTo propose a change: `project_send(to=\"all\", type=\"vote\", subject=\"Workflow change: Quick Feature\", body=\"Reason...\", metadata={\"vote_type\": \"workflow_change\", \"proposed_value\": \"quick\", \"vote\": \"yes\"})`\n\nTo vote yes/no: `project_send(to=\"all\", type=\"vote\", subject=\"Re: Workflow change\", body=\"Agreed\", metadata={\"vote_type\": \"workflow_change\", \"in_reply_to\": <id>, \"vote\": \"yes\"})`\n\nMajority = floor(n/2) + 1 where n = active members + human.\n"),
        ("tester.md", "# Tester\n\nYou are a Tester on this project. Your job is to validate implementations and catch bugs.\n\n## Core Responsibilities\n\n- **Write Tests**: Create unit, integration, and edge-case tests.\n- **Run the Test Suite**: Execute tests after changes to catch regressions.\n- **Explore Edge Cases**: Think adversarially about what inputs break things.\n- **Report Bugs**: Report issues clearly with reproduction steps.\n- **Validate Fixes**: Verify bug fixes actually work.\n\n## Communication\n\n- Use `project_send(to=\"developer\", type=\"review\", ...)` to report bugs\n- Use `project_send(to=\"manager\", type=\"status\", ...)` to report testing progress\n- Use `project_send(to=\"manager\", type=\"question\", ...)` to ask about expected behavior\n- Use `project_check(0)` to see all messages directed to you\n\n## Workflow Types & Voting\n\nThe project runs under one of three workflow types: `full` (Full Review — complete onboarding + planning + full review pipeline), `quick` (Quick Feature — skip onboarding, abbreviated review cycle), `bugfix` (Bug Fix — focused diagnosis and fix, minimal review). No workflow is set by default.\n\nAny team member can propose a workflow change via voting. The human can override directly via the UI dropdown (bypasses voting).\n\nTo propose a change: `project_send(to=\"all\", type=\"vote\", subject=\"Workflow change: Quick Feature\", body=\"Reason...\", metadata={\"vote_type\": \"workflow_change\", \"proposed_value\": \"quick\", \"vote\": \"yes\"})`\n\nTo vote yes/no: `project_send(to=\"all\", type=\"vote\", subject=\"Re: Workflow change\", body=\"Agreed\", metadata={\"vote_type\": \"workflow_change\", \"in_reply_to\": <id>, \"vote\": \"yes\"})`\n\nMajority = floor(n/2) + 1 where n = active members + human.\n"),
    ];

    for (filename, content) in briefings {
        std::fs::write(roles_dir.join(filename), content)
            .map_err(|e| format!("Failed to write {}: {}", filename, e))?;
    }

    Ok(())
}

/// Copy roles (briefings + project.json entries + role_groups) from one project to another.
#[tauri::command]
fn copy_project_roles(source_dir: String, dest_dir: String) -> Result<u32, String> {
    // Source must be a valid vaak project; dest needs only .vaak/ to exist
    let source = validate_project_dir(&source_dir)?;
    let dest_path = std::path::Path::new(&dest_dir);
    let dest_vaak = dest_path.join(".vaak");
    if !dest_vaak.is_dir() {
        return Err(format!("Destination has no .vaak/ directory: {}", dest_dir));
    }
    // Strip \\?\ from dest too
    let dest_canonical = dest_path.canonicalize()
        .map_err(|e| format!("Invalid dest directory: {}", e))?;
    let dest = {
        let s = dest_canonical.to_string_lossy().to_string();
        s.strip_prefix("\\\\?\\").unwrap_or(&s).to_string()
    };

    let source_vaak = std::path::Path::new(&source).join(".vaak");
    let dest_vaak = std::path::Path::new(&dest).join(".vaak");
    let mut copied: u32 = 0;

    // 1. Copy role briefing .md files from source/roles/ to dest/roles/
    let source_roles_dir = source_vaak.join("roles");
    let dest_roles_dir = dest_vaak.join("roles");
    let _ = std::fs::create_dir_all(&dest_roles_dir);
    if source_roles_dir.is_dir() {
        if let Ok(entries) = std::fs::read_dir(&source_roles_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name_str = name.to_string_lossy();
                if name_str.ends_with(".md") {
                    let dest_file = dest_roles_dir.join(&name);
                    if !dest_file.exists() {
                        if let Ok(content) = std::fs::read_to_string(entry.path()) {
                            if std::fs::write(&dest_file, &content).is_ok() {
                                copied += 1;
                            }
                        }
                    }
                }
            }
        }
    }

    // 2. Merge roles and role_groups from source project.json into dest project.json
    let source_config_path = source_vaak.join("project.json");
    let dest_config_path = dest_vaak.join("project.json");
    if source_config_path.exists() && dest_config_path.exists() {
        let source_json: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(&source_config_path)
                .map_err(|e| format!("Failed to read source project.json: {}", e))?
        ).map_err(|e| format!("Failed to parse source project.json: {}", e))?;

        let mut dest_json: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(&dest_config_path)
                .map_err(|e| format!("Failed to read dest project.json: {}", e))?
        ).map_err(|e| format!("Failed to parse dest project.json: {}", e))?;

        // Merge roles: copy entries from source that don't exist in dest
        if let Some(source_roles) = source_json.get("roles").and_then(|r| r.as_object()) {
            let dest_roles = dest_json.get_mut("roles")
                .and_then(|r| r.as_object_mut());
            if let Some(dest_roles) = dest_roles {
                for (slug, config) in source_roles {
                    if !dest_roles.contains_key(slug) {
                        dest_roles.insert(slug.clone(), config.clone());
                        copied += 1;
                    }
                }
            }
        }

        // Merge role_groups: copy groups from source that don't exist in dest (by slug)
        if let Some(source_groups) = source_json.get("role_groups").and_then(|g| g.as_array()) {
            let dest_groups = dest_json.get_mut("role_groups")
                .and_then(|g| g.as_array_mut());
            if let Some(dest_groups) = dest_groups {
                let existing_slugs: std::collections::HashSet<String> = dest_groups.iter()
                    .filter_map(|g| g.get("slug").and_then(|s| s.as_str()).map(|s| s.to_string()))
                    .collect();
                for group in source_groups {
                    if let Some(slug) = group.get("slug").and_then(|s| s.as_str()) {
                        if !existing_slugs.contains(slug) {
                            dest_groups.push(group.clone());
                            copied += 1;
                        }
                    }
                }
            } else {
                // dest has no role_groups array — create it
                dest_json["role_groups"] = serde_json::Value::Array(source_groups.clone());
                copied += source_groups.len() as u32;
            }
        }

        // Write back dest project.json
        let pretty = serde_json::to_string_pretty(&dest_json)
            .map_err(|e| format!("Failed to format dest project.json: {}", e))?;
        std::fs::write(&dest_config_path, pretty)
            .map_err(|e| format!("Failed to write dest project.json: {}", e))?;
    }

    Ok(copied)
}

#[tauri::command]
fn delete_message(dir: String, message_id: u64) -> Result<(), String> {
    let dir = validate_project_dir(&dir)?;
    let board_path = collab::active_board_path(&dir);
    let content = std::fs::read_to_string(&board_path)
        .map_err(|e| format!("Failed to read board.jsonl: {}", e))?;

    let filtered: Vec<String> = content
        .lines()
        .filter(|line| {
            if line.trim().is_empty() { return false; }
            match serde_json::from_str::<serde_json::Value>(line) {
                Ok(msg) => msg.get("id").and_then(|i| i.as_u64()) != Some(message_id),
                Err(_) => true, // keep unparseable lines
            }
        })
        .map(|l| l.to_string())
        .collect();

    let output = if filtered.is_empty() {
        String::new()
    } else {
        filtered.join("\n") + "\n"
    };

    collab::atomic_write(&board_path, output.as_bytes())
        .map_err(|e| format!("Failed to write board.jsonl: {}", e))?;
    notify_collab_change();
    Ok(())
}

#[tauri::command]
fn clear_all_messages(dir: String) -> Result<(), String> {
    let dir = validate_project_dir(&dir)?;
    let board_path = collab::active_board_path(&dir);
    collab::atomic_write(&board_path, b"")
        .map_err(|e| format!("Failed to truncate board.jsonl: {}", e))?;
    notify_collab_change();
    Ok(())
}

#[tauri::command]
fn set_message_retention(dir: String, days: u64) -> Result<(), String> {
    let dir = validate_project_dir(&dir)?;
    let config_path = std::path::Path::new(&dir).join(".vaak").join("project.json");
    let content = std::fs::read_to_string(&config_path)
        .map_err(|e| format!("Failed to read project.json: {}", e))?;
    let mut config: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse project.json: {}", e))?;

    if let Some(settings) = config.get_mut("settings") {
        settings["message_retention_days"] = serde_json::Value::Number(serde_json::Number::from(days));
    }

    // Update timestamp
    let now = {
        let dur = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        let secs = dur.as_secs();
        let hours = (secs % 86400) / 3600;
        let mins = (secs % 3600) / 60;
        let s = secs % 60;
        let days_total = secs / 86400;
        let mut y = 1970u64;
        let mut remaining = days_total;
        loop {
            let diy = if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 { 366 } else { 365 };
            if remaining < diy { break; }
            remaining -= diy;
            y += 1;
        }
        let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
        let month_days: [u64; 12] = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
        let mut m = 0u64;
        for i in 0..12 {
            if remaining < month_days[i] { break; }
            remaining -= month_days[i];
            m = i as u64 + 1;
        }
        format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", y, m + 1, remaining + 1, hours, mins, s)
    };
    config["updated_at"] = serde_json::Value::String(now);

    let pretty = serde_json::to_string_pretty(&config)
        .map_err(|e| format!("Failed to serialize config: {}", e))?;
    collab::atomic_write(&config_path, pretty.as_bytes())
        .map_err(|e| format!("Failed to write project.json: {}", e))?;
    notify_collab_change();
    Ok(())
}

#[tauri::command]
fn set_workflow_type(dir: String, workflow_type: Option<String>) -> Result<(), String> {
    let dir = validate_project_dir(&dir)?;
    // Validate workflow type
    if let Some(ref wt) = workflow_type {
        if wt != "full" && wt != "quick" && wt != "bugfix" {
            return Err(format!("Invalid workflow type '{}'. Must be 'full', 'quick', or 'bugfix'.", wt));
        }
    }

    let config_path = std::path::Path::new(&dir).join(".vaak").join("project.json");
    let content = std::fs::read_to_string(&config_path)
        .map_err(|e| format!("Failed to read project.json: {}", e))?;
    let mut config: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse project.json: {}", e))?;

    // Update settings.workflow_type
    if let Some(settings) = config.get_mut("settings") {
        match &workflow_type {
            Some(wt) => { settings["workflow_type"] = serde_json::Value::String(wt.clone()); }
            None => { settings.as_object_mut().map(|o| o.remove("workflow_type")); }
        }
    }

    // Update timestamp
    let now = {
        let dur = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        let secs = dur.as_secs();
        // Simple ISO timestamp
        let hours = (secs % 86400) / 3600;
        let mins = (secs % 3600) / 60;
        let s = secs % 60;
        let days = secs / 86400;
        let mut y = 1970u64;
        let mut remaining = days;
        loop {
            let diy = if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 { 366 } else { 365 };
            if remaining < diy { break; }
            remaining -= diy;
            y += 1;
        }
        let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
        let month_days: [u64; 12] = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
        let mut m = 0u64;
        for i in 0..12 {
            if remaining < month_days[i] { break; }
            remaining -= month_days[i];
            m = i as u64 + 1;
        }
        format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", y, m + 1, remaining + 1, hours, mins, s)
    };
    config["updated_at"] = serde_json::Value::String(now);

    let pretty = serde_json::to_string_pretty(&config)
        .map_err(|e| format!("Failed to serialize config: {}", e))?;
    collab::atomic_write(&config_path, pretty.as_bytes())
        .map_err(|e| format!("Failed to write project.json: {}", e))?;

    notify_collab_change();
    Ok(())
}

#[tauri::command]
fn set_discussion_mode(dir: String, discussion_mode: Option<String>) -> Result<(), String> {
    let dir = validate_project_dir(&dir)?;
    if let Some(ref dm) = discussion_mode {
        if dm != "open" && dm != "directed" {
            return Err(format!("Invalid communication mode '{}'. Must be 'open' or 'directed'. (Discussion formats like 'delphi'/'oxford' are set when starting a discussion, not here.)", dm));
        }
    }

    let config_path = std::path::Path::new(&dir).join(".vaak").join("project.json");
    let content = std::fs::read_to_string(&config_path)
        .map_err(|e| format!("Failed to read project.json: {}", e))?;
    let mut config: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse project.json: {}", e))?;

    if let Some(settings) = config.get_mut("settings") {
        match &discussion_mode {
            Some(dm) => { settings["discussion_mode"] = serde_json::Value::String(dm.clone()); }
            None => { settings.as_object_mut().map(|o| o.remove("discussion_mode")); }
        }
    }

    let now = {
        let dur = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        let secs = dur.as_secs();
        let hours = (secs % 86400) / 3600;
        let mins = (secs % 3600) / 60;
        let s = secs % 60;
        let days = secs / 86400;
        let mut y = 1970u64;
        let mut remaining = days;
        loop {
            let diy = if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 { 366 } else { 365 };
            if remaining < diy { break; }
            remaining -= diy;
            y += 1;
        }
        let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
        let month_days: [u64; 12] = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
        let mut m = 0u64;
        for i in 0..12 {
            if remaining < month_days[i] { break; }
            remaining -= month_days[i];
            m = i as u64 + 1;
        }
        format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", y, m + 1, remaining + 1, hours, mins, s)
    };
    config["updated_at"] = serde_json::Value::String(now);

    let pretty = serde_json::to_string_pretty(&config)
        .map_err(|e| format!("Failed to serialize config: {}", e))?;
    collab::atomic_write(&config_path, pretty.as_bytes())
        .map_err(|e| format!("Failed to write project.json: {}", e))?;

    notify_collab_change();
    Ok(())
}

/// Fire-and-forget notification to the local HTTP server (port 7865) so that
/// the MCP sidecar and frontend windows learn about board/discussion changes
/// made by Tauri commands. Without this, MCP waits up to 55s to discover changes.
fn notify_collab_change() {
    std::thread::spawn(|| {
        let _ = ureq::AgentBuilder::new()
            .timeout(std::time::Duration::from_secs(2))
            .build()
            .post("http://127.0.0.1:7865/collab/notify")
            .send_string("");
    });
}

// ==================== Discussion Control Commands ====================

#[tauri::command]
fn start_discussion(
    dir: String,
    mode: String,
    topic: String,
    moderator: Option<String>,
    participants: Vec<String>,
) -> Result<(), String> {
    let valid_modes = ["delphi", "oxford", "red_team", "continuous"];
    if !valid_modes.contains(&mode.as_str()) {
        return Err(format!("Invalid discussion mode '{}'. Must be one of: {}", mode, valid_modes.join(", ")));
    }
    if topic.trim().is_empty() {
        return Err("Topic cannot be empty.".to_string());
    }
    if participants.is_empty() {
        return Err("At least one participant is required.".to_string());
    }

    let now = iso_now();
    let is_continuous = mode == "continuous";

    // Continuous mode starts in "reviewing" phase with no rounds —
    // rounds are auto-created when developers post status messages.
    // "reviewing" = ready for next auto-trigger (consistent with post-close phase).
    let (initial_round, initial_phase, initial_rounds) = if is_continuous {
        (0, "reviewing".to_string(), Vec::new())
    } else {
        (1, "submitting".to_string(), vec![collab::DiscussionRound {
            number: 1,
            opened_at: now.clone(),
            closed_at: None,
            submissions: Vec::new(),
            aggregate_message_id: None,
            trigger_message_id: None,
            trigger_from: None,
            author: None,
            trigger_subject: None,
            auto_triggered: None,
            topic: None,
        }])
    };

    let mut settings = collab::DiscussionSettings::default();
    if is_continuous {
        settings.max_rounds = 999;
        settings.auto_close_timeout_seconds = 60;
    }

    let state = collab::DiscussionState {
        active: true,
        mode: Some(mode),
        topic,
        started_at: Some(now.clone()),
        moderator,
        participants,
        current_round: initial_round,
        phase: Some(initial_phase),
        paused_at: None,
        expire_at: None,
        previous_phase: None,
        rounds: initial_rounds,
        settings,
    };

    if !collab::write_discussion(&dir, &state) {
        return Err("Failed to write discussion.json".to_string());
    }

    // Post announcement to board.jsonl so agents see the discussion started
    let board_path = collab::active_board_path(&dir);
    let board_content = std::fs::read_to_string(&board_path).unwrap_or_default();
    let msg_id = board_content.lines().filter(|l| !l.trim().is_empty()).count() as u64 + 1;
    let mode_ref = state.mode.as_deref().unwrap_or("unknown");
    let topic_ref = &state.topic;
    let mod_ref = state.moderator.as_deref().unwrap_or("none");
    let parts_ref = state.participants.join(", ");
    let announcement_body = if is_continuous {
        format!("Continuous Review mode activated.\n\n**Topic:** {}\n**Moderator:** {}\n**Participants:** {}\n\nReview windows open automatically when developers post status updates. Respond with: agree / disagree: [reason] / alternative: [proposal]. Silence within the timeout = consent.",
            topic_ref, mod_ref, parts_ref)
    } else {
        format!("A {} discussion has been started.\n\n**Topic:** {}\n**Moderator:** {}\n**Participants:** {}\n**Round:** 1\n\nSubmit your position using type: submission, addressed to the moderator.",
            mode_ref, topic_ref, mod_ref, parts_ref)
    };
    let announcement = serde_json::json!({
        "id": msg_id,
        "from": "system",
        "to": "all",
        "type": "moderation",
        "timestamp": state.started_at,
        "subject": format!("{} discussion started: {}", mode_ref, topic_ref),
        "body": announcement_body,
        "metadata": {
            "discussion_action": "start",
            "mode": mode_ref,
            "round": 1
        }
    });
    if let Ok(line) = serde_json::to_string(&announcement) {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&board_path) {
            let _ = writeln!(f, "{}", line);
        }
    }

    notify_collab_change();
    Ok(())
}

#[tauri::command]
fn close_discussion_round(dir: String) -> Result<String, String> {
    let dir = validate_project_dir(&dir)?;
    // Wrap the entire read-modify-write in a single lock acquisition
    // to prevent the dual-writer race with MCP sidecar submissions.
    // Without this, MCP-written submissions could be wiped when we
    // read discussion.json, modify it, and write it back.
    let result = collab::with_board_lock(&dir, || {
        let mut state = collab::read_discussion(&dir);
        if !state.active {
            return Err("No active discussion.".to_string());
        }
        if state.phase.as_deref() != Some("submitting") {
            return Err(format!("Cannot close round — current phase is '{}'.", state.phase.as_deref().unwrap_or("none")));
        }

        let now = iso_now();
        let round_num = state.current_round;

        // Collect submission message IDs from current round
        let submission_ids: Vec<u64> = state.rounds.last()
            .map(|r| r.submissions.iter().map(|s| s.message_id).collect())
            .unwrap_or_default();

        // Read board.jsonl to find submission bodies
        let board_path = collab::active_board_path(&dir);
        let board_content = std::fs::read_to_string(&board_path).unwrap_or_default();
        let mut bodies: Vec<String> = Vec::new();

        if !submission_ids.is_empty() {
            // Use tracked submission IDs
            for line in board_content.lines() {
                if line.trim().is_empty() { continue; }
                if let Ok(msg) = serde_json::from_str::<serde_json::Value>(line) {
                    let id = msg.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
                    if submission_ids.contains(&id) {
                        let body = msg.get("body").and_then(|b| b.as_str()).unwrap_or("(empty)");
                        bodies.push(body.to_string());
                    }
                }
            }
        } else {
            // Fallback: scan board.jsonl for type="submission" messages in this round's time window
            let opened_at = state.rounds.last()
                .map(|r| r.opened_at.as_str())
                .unwrap_or("");
            for line in board_content.lines() {
                if line.trim().is_empty() { continue; }
                if let Ok(msg) = serde_json::from_str::<serde_json::Value>(line) {
                    let msg_type = msg.get("type").and_then(|t| t.as_str()).unwrap_or("");
                    let ts = msg.get("timestamp").and_then(|t| t.as_str()).unwrap_or("");
                    if msg_type == "submission" && ts >= opened_at {
                        let body = msg.get("body").and_then(|b| b.as_str()).unwrap_or("(empty)");
                        bodies.push(body.to_string());
                    }
                }
            }
        }

        let topic = &state.topic;
        let total = bodies.len();
        let is_continuous_close = state.mode.as_deref() == Some("continuous");

        let aggregate = if is_continuous_close {
            // Continuous mode: lightweight tally (not anonymized)
            if total == 0 {
                format!("## Continuous Review Round {} — APPROVED\n**Change:** {}\n**Result:** No objections (silence = consent)", round_num, topic)
            } else {
                let mut agree_count = 0usize;
                let mut disagree_list: Vec<String> = Vec::new();
                let mut alternative_list: Vec<String> = Vec::new();
                for body in &bodies {
                    let lower = body.trim().to_lowercase();
                    if lower.starts_with("agree") || lower == "lgtm" || lower == "approved" || lower == "+1"
                        || lower.starts_with("looks good") || lower.starts_with("makes sense")
                        || lower.starts_with("i'm fine with") || lower.starts_with("im fine with")
                        || lower.starts_with("no objection") || lower.starts_with("sounds good")
                        || lower.starts_with("i agree") || lower.starts_with("fine with")
                        || lower.starts_with("works for me") || lower.starts_with("ship it")
                        || lower.starts_with("no concerns") || lower.starts_with("all good")
                        || lower.starts_with("thumbs up") || lower.starts_with("go ahead")
                        || lower.starts_with("no issues") || lower.starts_with("acknowledged")
                    {
                        agree_count += 1;
                    } else if lower.starts_with("disagree") || lower.starts_with("object") || lower.starts_with("-1")
                        || lower.starts_with("block") || lower.starts_with("reject") || lower.starts_with("nack")
                    {
                        disagree_list.push(body.clone());
                    } else if lower.starts_with("alternative") || lower.starts_with("suggest") || lower.starts_with("instead") {
                        alternative_list.push(body.clone());
                    } else {
                        // H3 fix: Unclassified responses default to agree (silence = consent model)
                        // rather than disagree, which was incorrectly biasing toward rejection
                        agree_count += 1;
                    }
                }
                let verdict = if disagree_list.is_empty() && alternative_list.is_empty() { "APPROVED" } else { "DISPUTED" };
                let silent = state.participants.len().saturating_sub(total + 1);
                let mut result = format!(
                    "## Continuous Review Round {} — {}\n**Change:** {}\n**Result:** {} agree, {} disagree, {} alternatives, {} silent (= approve)\n",
                    round_num, verdict, topic, agree_count + silent, disagree_list.len(), alternative_list.len(), silent);
                if !disagree_list.is_empty() {
                    result.push_str("\n**Disagreements:**\n");
                    for (i, d) in disagree_list.iter().enumerate() { result.push_str(&format!("  {}. {}\n", i+1, d)); }
                }
                if !alternative_list.is_empty() {
                    result.push_str("\n**Alternatives:**\n");
                    for (i, a) in alternative_list.iter().enumerate() { result.push_str(&format!("  {}. {}\n", i+1, a)); }
                }
                result
            }
        } else {
            // Delphi/Oxford/Red Team: full anonymized aggregate with randomized order.
            // Uses UUID v4 (backed by OS entropy) for unpredictable shuffle seed.
            let mut rng = uuid::Uuid::new_v4().as_u128();
            for i in (1..bodies.len()).rev() {
                rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                let j = (rng as usize) % (i + 1);
                bodies.swap(i, j);
            }

            let format_name = state.mode.as_deref().map(|m| {
                let mut chars = m.chars();
                match chars.next() {
                    Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
                    None => "Discussion".to_string(),
                }
            }).unwrap_or_else(|| "Discussion".to_string());

            if total == 0 {
                format!("## {} Round {} Aggregate\nNo submissions received this round.", format_name, round_num)
            } else {
                let mut agg = format!(
                    "## {} Round {} Aggregate — {} submissions\n**Topic:** {}\n\n---\n\n",
                    format_name, round_num, total, topic
                );
                for (i, body) in bodies.iter().enumerate() {
                    agg.push_str(&format!("### Participant {}\n{}\n\n---\n\n", i + 1, body));
                }
                agg.push_str(&format!(
                    "*{} submissions collected. Order randomized. Identities anonymized.*", total
                ));
                agg
            }
        };

        // Write aggregate as moderation message to board.jsonl
        // (board.lock is already held by with_board_lock — write directly)
        let msg_id = board_content.lines().filter(|l| !l.trim().is_empty()).count() as u64 + 1;
        let aggregate_msg = serde_json::json!({
            "id": msg_id,
            "from": "system",
            "to": "all",
            "type": "moderation",
            "timestamp": now,
            "subject": format!("Round {} Aggregate", round_num),
            "body": aggregate,
            "metadata": {
                "discussion_action": "aggregate",
                "round": round_num
            }
        });
        let aggregate_line = serde_json::to_string(&aggregate_msg)
            .map_err(|e| format!("Failed to serialize aggregate: {}", e))?;

        {
            use std::io::Write;
            let mut file = std::fs::OpenOptions::new()
                .create(true).append(true).open(&board_path)
                .map_err(|e| format!("Failed to open board.jsonl: {}", e))?;
            writeln!(file, "{}", aggregate_line)
                .map_err(|e| format!("Failed to write aggregate: {}", e))?;
        }

        // Update discussion state and write atomically (lock already held)
        if let Some(round) = state.rounds.last_mut() {
            round.closed_at = Some(now.clone());
            round.aggregate_message_id = Some(msg_id);
        }
        state.phase = Some("reviewing".to_string());

        if !collab::write_discussion_unlocked(&dir, &state) {
            return Err("Failed to write discussion.json".to_string());
        }
        Ok(format!("Round {} closed. {} submissions aggregated.", round_num, total))
    });
    if result.is_ok() { notify_collab_change(); }
    result
}

#[tauri::command]
fn open_next_round(dir: String) -> Result<u32, String> {
    let dir = validate_project_dir(&dir)?;
    // Wrap in board lock to prevent race with MCP sidecar
    let result = collab::with_board_lock(&dir, || {
        let mut state = collab::read_discussion(&dir);
        if !state.active {
            return Err("No active discussion.".to_string());
        }

        let next_round = state.current_round + 1;
        if next_round > state.settings.max_rounds {
            return Err(format!("Max rounds ({}) reached.", state.settings.max_rounds));
        }

        let now = iso_now();
        state.current_round = next_round;
        state.phase = Some("submitting".to_string());
        state.rounds.push(collab::DiscussionRound {
            number: next_round,
            opened_at: now.clone(),
            closed_at: None,
            submissions: Vec::new(),
            aggregate_message_id: None,
            trigger_message_id: None,
            trigger_from: None,
            author: None,
            trigger_subject: None,
            auto_triggered: None,
            topic: None,
        });

        if !collab::write_discussion_unlocked(&dir, &state) {
            return Err("Failed to write discussion.json".to_string());
        }

        // Post round-open announcement to board.jsonl (lock already held)
        let board_path = collab::active_board_path(&dir);
        let board_content = std::fs::read_to_string(&board_path).unwrap_or_default();
        let msg_id = board_content.lines().filter(|l| !l.trim().is_empty()).count() as u64 + 1;
        let announcement = serde_json::json!({
            "id": msg_id,
            "from": "system",
            "to": "all",
            "type": "moderation",
            "timestamp": now,
            "subject": format!("Round {} opened", next_round),
            "body": format!("Round {} is now open for submissions. Review the previous aggregate and submit your revised position.", next_round),
            "metadata": {
                "discussion_action": "open_round",
                "round": next_round
            }
        });
        if let Ok(line) = serde_json::to_string(&announcement) {
            use std::io::Write;
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&board_path) {
                let _ = writeln!(f, "{}", line);
            }
        }

        Ok(next_round)
    });
    if result.is_ok() { notify_collab_change(); }
    result
}

#[tauri::command]
fn end_discussion(dir: String) -> Result<(), String> {
    let dir = validate_project_dir(&dir)?;
    // Wrap in board lock to prevent race with MCP sidecar
    let result = collab::with_board_lock(&dir, || {
        let mut state = collab::read_discussion(&dir);
        if !state.active {
            return Err("No active discussion.".to_string());
        }

        let now = iso_now();
        let topic = state.topic.clone();
        let round_num = state.current_round;

        state.active = false;
        state.phase = Some("complete".to_string());

        if !collab::write_discussion_unlocked(&dir, &state) {
            return Err("Failed to write discussion.json".to_string());
        }

        // Post end announcement to board.jsonl (lock already held)
        let board_path = collab::active_board_path(&dir);
        let board_content = std::fs::read_to_string(&board_path).unwrap_or_default();
        let msg_id = board_content.lines().filter(|l| !l.trim().is_empty()).count() as u64 + 1;
        let announcement = serde_json::json!({
            "id": msg_id,
            "from": "system",
            "to": "all",
            "type": "moderation",
            "timestamp": now,
            "subject": format!("Discussion ended: {}", topic),
            "body": format!("The discussion on \"{}\" has concluded after {} round(s).", topic, round_num),
            "metadata": {
                "discussion_action": "end",
                "final_round": round_num
            }
        });
        if let Ok(line) = serde_json::to_string(&announcement) {
            use std::io::Write;
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&board_path) {
                let _ = writeln!(f, "{}", line);
            }
        }

        Ok(())
    });
    if result.is_ok() { notify_collab_change(); }
    result
}

#[tauri::command]
fn get_discussion_state(dir: String) -> Result<serde_json::Value, String> {
    let dir = validate_project_dir(&dir)?;
    let state = collab::read_discussion(&dir);
    let mut val = serde_json::to_value(&state)
        .map_err(|e| format!("Failed to serialize discussion state: {}", e))?;

    // Enrich submissions from board.jsonl (source of truth) since MCP tracking can miss writes
    if state.active {
        let board_path = collab::active_board_path(&dir);
        if let Ok(board_content) = std::fs::read_to_string(&board_path) {
            let submissions: Vec<serde_json::Value> = board_content.lines()
                .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
                .filter(|msg| msg.get("type").and_then(|t| t.as_str()) == Some("submission"))
                .collect();

            if let Some(rounds) = val.get_mut("rounds").and_then(|r| r.as_array_mut()) {
                for round in rounds.iter_mut() {
                    let opened_at = round.get("opened_at").and_then(|t| t.as_str()).unwrap_or("");
                    let closed_at = round.get("closed_at").and_then(|t| t.as_str()).unwrap_or("");

                    // Find submissions that fall within this round's time window
                    let round_subs: Vec<serde_json::Value> = submissions.iter()
                        .filter(|msg| {
                            let ts = msg.get("timestamp").and_then(|t| t.as_str()).unwrap_or("");
                            ts >= opened_at && (closed_at.is_empty() || ts <= closed_at)
                        })
                        .map(|msg| {
                            serde_json::json!({
                                "from": msg.get("from").and_then(|f| f.as_str()).unwrap_or("unknown"),
                                "message_id": msg.get("id").and_then(|i| i.as_u64()).unwrap_or(0),
                                "submitted_at": msg.get("timestamp").and_then(|t| t.as_str()).unwrap_or("")
                            })
                        })
                        .collect();

                    // Only override if board has more submissions than discussion.json tracked
                    let existing_count = round.get("submissions")
                        .and_then(|s| s.as_array())
                        .map(|a| a.len())
                        .unwrap_or(0);
                    if round_subs.len() > existing_count {
                        round["submissions"] = serde_json::json!(round_subs);
                    }
                }
            }
        }
    }

    // Strip author-identifying fields from rounds to prevent metadata leak (matches vaak-mcp.rs).
    // trigger_from: direct author identity
    // trigger_message_id: indirect leak — client can look up the board message to find its `from` field
    // trigger_subject: probabilistic leak — specific subjects are attributable in small teams
    if let Some(rounds) = val.get_mut("rounds").and_then(|r| r.as_array_mut()) {
        for round in rounds.iter_mut() {
            if let Some(obj) = round.as_object_mut() {
                obj.remove("trigger_from");
                obj.remove("trigger_message_id");
                obj.remove("trigger_subject");
            }
        }
    }

    Ok(val)
}

#[tauri::command]
fn set_continuous_timeout(dir: String, timeout_seconds: u32) -> Result<(), String> {
    let dir = validate_project_dir(&dir)?;
    // Wrap in board lock to prevent race with MCP sidecar
    let result = collab::with_board_lock(&dir, || {
        let disc_path = collab::active_discussion_path(&dir);
        let content = std::fs::read_to_string(&disc_path)
            .map_err(|e| format!("Failed to read discussion.json: {}", e))?;
        let mut disc: serde_json::Value = serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse discussion.json: {}", e))?;

        if let Some(settings) = disc.get_mut("settings") {
            settings["auto_close_timeout_seconds"] = serde_json::json!(timeout_seconds);
        } else {
            disc["settings"] = serde_json::json!({
                "auto_close_timeout_seconds": timeout_seconds
            });
        }

        let disc_content = serde_json::to_string_pretty(&disc)
            .map_err(|e| format!("Failed to serialize: {}", e))?;
        collab::atomic_write(&disc_path, disc_content.as_bytes())
            .map_err(|e| format!("Failed to write discussion.json: {}", e))?;

        Ok(())
    });
    if result.is_ok() { notify_collab_change(); }
    result
}

/// Generate ISO 8601 UTC timestamp
fn iso_now() -> String {
    let dur = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs();
    let hours = (secs % 86400) / 3600;
    let mins = (secs % 3600) / 60;
    let s = secs % 60;
    let days = secs / 86400;
    let mut y = 1970u64;
    let mut remaining = days;
    loop {
        let diy = if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 { 366 } else { 365 };
        if remaining < diy { break; }
        remaining -= diy;
        y += 1;
    }
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let month_days: [u64; 12] = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut m = 0u64;
    for i in 0..12 {
        if remaining < month_days[i] { break; }
        remaining -= month_days[i];
        m = i as u64 + 1;
    }
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", y, m + 1, remaining + 1, hours, mins, s)
}

#[tauri::command]
fn send_team_message(dir: String, to: String, subject: String, body: String, msg_type: Option<String>, metadata: Option<serde_json::Value>) -> Result<u64, String> {
    let dir = validate_project_dir(&dir)?;
    let board_path = collab::active_board_path(&dir);

    // Read existing board to determine next message ID
    let existing = std::fs::read_to_string(&board_path).unwrap_or_default();
    let max_id: u64 = existing.lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter_map(|m| m.get("id").and_then(|i| i.as_u64()))
        .max()
        .unwrap_or(0);

    let msg_id = max_id + 1;

    // Generate UTC ISO timestamp without chrono dependency
    let now = {
        let dur = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        let secs = dur.as_secs();
        let hours = (secs % 86400) / 3600;
        let mins = (secs % 3600) / 60;
        let s = secs % 60;
        let days = secs / 86400;
        let mut y = 1970u64;
        let mut remaining = days;
        loop {
            let diy = if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 { 366 } else { 365 };
            if remaining < diy { break; }
            remaining -= diy;
            y += 1;
        }
        let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
        let month_days: [u64; 12] = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
        let mut m = 0u64;
        for md in &month_days {
            if remaining < *md { break; }
            remaining -= *md;
            m += 1;
        }
        format!("{}-{:02}-{:02}T{:02}:{:02}:{:02}Z", y, m + 1, remaining + 1, hours, mins, s)
    };

    let effective_type = msg_type.unwrap_or_else(|| "directive".to_string());
    let effective_metadata = metadata.unwrap_or(serde_json::json!({}));

    let message = serde_json::json!({
        "id": msg_id,
        "from": "human:0",
        "to": to,
        "type": effective_type,
        "timestamp": now,
        "subject": subject,
        "body": body,
        "metadata": effective_metadata
    });

    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&board_path)
        .map_err(|e| format!("Failed to open board.jsonl: {}", e))?;

    writeln!(file, "{}", message.to_string())
        .map_err(|e| format!("Failed to write message: {}", e))?;

    notify_collab_change();
    Ok(msg_id)
}

// ==================== Section Commands ====================

#[tauri::command]
fn create_section(dir: String, name: String) -> Result<collab::SectionInfo, String> {
    let dir = validate_project_dir(&dir)?;
    let result = collab::create_section(&dir, &name);
    if result.is_ok() { notify_collab_change(); }
    result
}

#[tauri::command]
fn switch_section(dir: String, slug: String) -> Result<(), String> {
    let dir = validate_project_dir(&dir)?;
    // Verify the section exists (either "default" or has a sections/{slug}/ directory)
    if slug != "default" {
        let sec_dir = std::path::Path::new(&dir).join(".vaak").join("sections").join(&slug);
        if !sec_dir.exists() {
            return Err(format!("Section '{}' does not exist", slug));
        }
    }
    let result = collab::set_active_section(&dir, &slug);
    if result.is_ok() { notify_collab_change(); }
    result
}

#[tauri::command]
fn list_sections(dir: String) -> Result<Vec<collab::SectionInfo>, String> {
    let dir = validate_project_dir(&dir)?;
    Ok(collab::list_sections(&dir))
}

#[tauri::command]
fn get_active_section(dir: String) -> Result<String, String> {
    let dir = validate_project_dir(&dir)?;
    Ok(collab::get_active_section(&dir))
}

// ==================== Roster Commands ====================

#[tauri::command]
fn roster_add_slot(project_dir: String, role: String, metadata: Option<serde_json::Value>) -> Result<collab::RosterSlot, String> {
    let project_dir = validate_project_dir(&project_dir)?;
    let result = collab::roster_add_slot(&project_dir, &role, metadata);
    if result.is_ok() { notify_collab_change(); }
    result
}

#[tauri::command]
fn roster_remove_slot(
    project_dir: String,
    role: String,
    instance: i32,
    launcher_state: tauri::State<'_, launcher::LauncherState>,
) -> Result<(), String> {
    // Best-effort kill the spawned terminal process before removing the slot
    launcher::kill_tracked_agent(&role, instance, &launcher_state);
    let result = collab::roster_remove_slot(&project_dir, &role, instance);
    if result.is_ok() { notify_collab_change(); }
    result
}

#[tauri::command]
fn roster_get(project_dir: String) -> Result<Vec<collab::RosterSlotWithStatus>, String> {
    let project_dir = validate_project_dir(&project_dir)?;
    collab::roster_get(&project_dir)
}

// ==================== Role CRUD Commands ====================

#[tauri::command]
fn create_role(
    project_dir: String,
    slug: String,
    title: String,
    description: String,
    permissions: Vec<String>,
    max_instances: u32,
    briefing: String,
    tags: Vec<String>,
    companions: Option<Vec<collab::CompanionConfig>>,
) -> Result<collab::RoleConfig, String> {
    // Auto-save to global templates happens inside collab::create_role
    let result = collab::create_role(&project_dir, &slug, &title, &description, permissions, max_instances, &briefing, tags, companions.unwrap_or_default());
    if result.is_ok() { notify_collab_change(); }
    result
}

#[tauri::command]
fn update_role(
    project_dir: String,
    slug: String,
    title: Option<String>,
    description: Option<String>,
    permissions: Option<Vec<String>>,
    max_instances: Option<u32>,
    briefing: Option<String>,
    tags: Option<Vec<String>>,
    companions: Option<Vec<collab::CompanionConfig>>,
) -> Result<collab::RoleConfig, String> {
    let result = collab::update_role(
        &project_dir,
        &slug,
        title.as_deref(),
        description.as_deref(),
        permissions,
        max_instances,
        briefing.as_deref(),
        tags,
        companions,
    )?;
    // Auto-update global template on edit (non-blocking)
    if let Err(e) = collab::save_role_as_global_template(&project_dir, &slug) {
        eprintln!("[main] Auto-update global template for '{}' failed: {}", slug, e);
    }
    notify_collab_change();
    Ok(result)
}

#[tauri::command]
fn delete_role(project_dir: String, slug: String) -> Result<(), String> {
    let project_dir = validate_project_dir(&project_dir)?;
    let result = collab::delete_role(&project_dir, &slug);
    if result.is_ok() { notify_collab_change(); }
    result
}

#[tauri::command]
fn save_role_group(project_dir: String, group: collab::RoleGroup) -> Result<collab::RoleGroup, String> {
    let project_dir = validate_project_dir(&project_dir)?;
    let result = collab::save_role_group(&project_dir, group);
    if result.is_ok() { notify_collab_change(); }
    result
}

#[tauri::command]
fn delete_role_group(project_dir: String, slug: String) -> Result<(), String> {
    let project_dir = validate_project_dir(&project_dir)?;
    let result = collab::delete_role_group(&project_dir, &slug);
    if result.is_ok() { notify_collab_change(); }
    result
}

#[tauri::command]
fn read_role_briefing(dir: String, role_slug: String) -> Result<String, String> {
    let dir = validate_project_dir(&dir)?;
    // Sanitize role_slug: reject path separators and traversal
    if role_slug.contains('/') || role_slug.contains('\\') || role_slug.contains("..") {
        return Err("Invalid role slug: path traversal not allowed".to_string());
    }
    let path = std::path::Path::new(&dir)
        .join(".vaak")
        .join("roles")
        .join(format!("{}.md", role_slug));
    std::fs::read_to_string(&path)
        .map_err(|e| format!("No briefing found for '{}': {}", role_slug, e))
}

// ==================== Global Role Template Commands ====================

#[tauri::command]
fn save_role_as_global_template(project_dir: String, slug: String) -> Result<(), String> {
    let project_dir = validate_project_dir(&project_dir)?;
    collab::save_role_as_global_template(&project_dir, &slug)
}

#[tauri::command]
fn list_global_role_templates() -> Result<serde_json::Value, String> {
    collab::list_global_role_templates()
}

#[tauri::command]
fn remove_global_role_template(slug: String) -> Result<(), String> {
    collab::remove_global_role_template(&slug)
}

#[tauri::command]
fn get_project_claims(dir: String) -> Result<serde_json::Value, String> {
    let dir = validate_project_dir(&dir)?;
    let vaak_dir = std::path::Path::new(&dir).join(".vaak");
    let claims_path = vaak_dir.join("claims.json");
    let content = match std::fs::read_to_string(&claims_path) {
        Ok(c) => c,
        Err(_) => return Ok(serde_json::json!({})),
    };
    let claims_map: std::collections::HashMap<String, serde_json::Value> = serde_json::from_str(&content)
        .map_err(|e| format!("Invalid claims.json: {}", e))?;

    // Cross-reference with sessions for staleness
    let sessions_path = vaak_dir.join("sessions.json");
    let config_path = vaak_dir.join("project.json");
    let bindings: Vec<serde_json::Value> = std::fs::read_to_string(&sessions_path)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.get("bindings")?.as_array().cloned())
        .unwrap_or_default();
    let timeout_secs = std::fs::read_to_string(&config_path)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.get("settings")?.get("heartbeat_timeout_seconds")?.as_u64())
        .unwrap_or(120);
    let gone_threshold = (timeout_secs as f64 * 2.5) as u64;
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let mut clean = serde_json::Map::new();
    for (key, val) in &claims_map {
        let session_id = val.get("session_id").and_then(|s| s.as_str()).unwrap_or("");
        let binding = bindings.iter().find(|b| {
            b.get("session_id").and_then(|s| s.as_str()) == Some(session_id)
        });
        let is_stale = match binding {
            None => true,
            Some(b) => {
                let hb = b.get("last_heartbeat").and_then(|h| h.as_str()).unwrap_or("");
                let age = collab::parse_iso_epoch_pub(hb)
                    .map(|hb_secs| now_secs.saturating_sub(hb_secs))
                    .unwrap_or(u64::MAX);
                age > gone_threshold
            }
        };
        if !is_stale {
            clean.insert(key.clone(), val.clone());
        }
    }

    Ok(serde_json::Value::Object(clean))
}

#[tauri::command]
fn claim_files(dir: String, role_instance: String, files: Vec<String>, description: String, session_id: String) -> Result<serde_json::Value, String> {
    let dir = validate_project_dir(&dir)?;
    let claims_path = std::path::Path::new(&dir).join(".vaak").join("claims.json");
    let lock_path = std::path::Path::new(&dir).join(".vaak").join("board.lock");

    std::fs::create_dir_all(std::path::Path::new(&dir).join(".vaak"))
        .map_err(|e| format!("Failed to create .vaak dir: {}", e))?;

    let lock_file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .open(&lock_path)
        .map_err(|e| format!("Failed to open lock file: {}", e))?;

    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{LockFileEx, UnlockFileEx, LOCKFILE_EXCLUSIVE_LOCK};
        use windows_sys::Win32::System::IO::OVERLAPPED;

        let handle = lock_file.as_raw_handle();
        let mut overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };
        let locked = unsafe {
            LockFileEx(handle as _, LOCKFILE_EXCLUSIVE_LOCK, 0, u32::MAX, u32::MAX, &mut overlapped)
        };
        if locked == 0 {
            return Err("Failed to acquire file lock".to_string());
        }

        let result = (|| {
            let content = std::fs::read_to_string(&claims_path).unwrap_or_else(|_| "{}".to_string());
            let mut claims: serde_json::Map<String, serde_json::Value> = serde_json::from_str(&content)
                .unwrap_or_default();

            // Check for conflicts
            let mut conflicts = Vec::new();
            for (key, val) in &claims {
                if key == &role_instance { continue; }
                let their_files: Vec<String> = val.get("files")
                    .and_then(|f| f.as_array())
                    .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
                    .unwrap_or_default();
                let overlapping: Vec<String> = files.iter()
                    .filter(|f| their_files.iter().any(|tf| f.starts_with(tf.as_str()) || tf.starts_with(f.as_str()) || *f == tf))
                    .cloned()
                    .collect();
                if !overlapping.is_empty() {
                    conflicts.push(serde_json::json!({
                        "claimant": key,
                        "overlapping_files": overlapping
                    }));
                }
            }

            let now = chrono_free_iso_now();
            claims.insert(role_instance.clone(), serde_json::json!({
                "files": files,
                "description": description,
                "claimed_at": now,
                "session_id": session_id
            }));

            let output = serde_json::to_string_pretty(&claims)
                .map_err(|e| format!("Serialize error: {}", e))?;
            collab::atomic_write(&claims_path, output.as_bytes())
                .map_err(|e| format!("Write error: {}", e))?;

            Ok(serde_json::json!({ "conflicts": conflicts }))
        })();

        unsafe { UnlockFileEx(handle as _, 0, u32::MAX, u32::MAX, &mut overlapped); }
        if result.is_ok() { notify_collab_change(); }
        result
    }

    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        let fd = lock_file.as_raw_fd();
        if unsafe { libc::flock(fd, libc::LOCK_EX) } != 0 {
            return Err("Failed to acquire file lock".to_string());
        }

        let result = (|| {
            let content = std::fs::read_to_string(&claims_path).unwrap_or_else(|_| "{}".to_string());
            let mut claims: serde_json::Map<String, serde_json::Value> = serde_json::from_str(&content)
                .unwrap_or_default();

            let mut conflicts = Vec::new();
            for (key, val) in &claims {
                if key == &role_instance { continue; }
                let their_files: Vec<String> = val.get("files")
                    .and_then(|f| f.as_array())
                    .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
                    .unwrap_or_default();
                let overlapping: Vec<String> = files.iter()
                    .filter(|f| their_files.iter().any(|tf| f.starts_with(tf.as_str()) || tf.starts_with(f.as_str()) || *f == tf))
                    .cloned()
                    .collect();
                if !overlapping.is_empty() {
                    conflicts.push(serde_json::json!({
                        "claimant": key,
                        "overlapping_files": overlapping
                    }));
                }
            }

            let now = chrono_free_iso_now();
            claims.insert(role_instance.clone(), serde_json::json!({
                "files": files,
                "description": description,
                "claimed_at": now,
                "session_id": session_id
            }));

            let output = serde_json::to_string_pretty(&claims)
                .map_err(|e| format!("Serialize error: {}", e))?;
            collab::atomic_write(&claims_path, output.as_bytes())
                .map_err(|e| format!("Write error: {}", e))?;

            Ok(serde_json::json!({ "conflicts": conflicts }))
        })();

        unsafe { libc::flock(fd, libc::LOCK_UN); }
        if result.is_ok() { notify_collab_change(); }
        result
    }
}

#[tauri::command]
fn release_claim(dir: String, role_instance: String) -> Result<(), String> {
    let dir = validate_project_dir(&dir)?;
    let claims_path = std::path::Path::new(&dir).join(".vaak").join("claims.json");
    let lock_path = std::path::Path::new(&dir).join(".vaak").join("board.lock");

    let lock_file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .open(&lock_path)
        .map_err(|e| format!("Failed to open lock file: {}", e))?;

    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{LockFileEx, UnlockFileEx, LOCKFILE_EXCLUSIVE_LOCK};
        use windows_sys::Win32::System::IO::OVERLAPPED;

        let handle = lock_file.as_raw_handle();
        let mut overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };
        let locked = unsafe {
            LockFileEx(handle as _, LOCKFILE_EXCLUSIVE_LOCK, 0, u32::MAX, u32::MAX, &mut overlapped)
        };
        if locked == 0 {
            return Err("Failed to acquire file lock".to_string());
        }

        let result = (|| {
            let content = std::fs::read_to_string(&claims_path).unwrap_or_else(|_| "{}".to_string());
            let mut claims: serde_json::Map<String, serde_json::Value> = serde_json::from_str(&content)
                .unwrap_or_default();
            claims.remove(&role_instance);
            let output = serde_json::to_string_pretty(&claims)
                .map_err(|e| format!("Serialize error: {}", e))?;
            collab::atomic_write(&claims_path, output.as_bytes())
                .map_err(|e| format!("Write error: {}", e))?;
            Ok(())
        })();

        unsafe { UnlockFileEx(handle as _, 0, u32::MAX, u32::MAX, &mut overlapped); }
        if result.is_ok() { notify_collab_change(); }
        result
    }

    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        let fd = lock_file.as_raw_fd();
        if unsafe { libc::flock(fd, libc::LOCK_EX) } != 0 {
            return Err("Failed to acquire file lock".to_string());
        }

        let result = (|| {
            let content = std::fs::read_to_string(&claims_path).unwrap_or_else(|_| "{}".to_string());
            let mut claims: serde_json::Map<String, serde_json::Value> = serde_json::from_str(&content)
                .unwrap_or_default();
            claims.remove(&role_instance);
            let output = serde_json::to_string_pretty(&claims)
                .map_err(|e| format!("Serialize error: {}", e))?;
            collab::atomic_write(&claims_path, output.as_bytes())
                .map_err(|e| format!("Write error: {}", e))?;
            Ok(())
        })();

        unsafe { libc::flock(fd, libc::LOCK_UN); }
        if result.is_ok() { notify_collab_change(); }
        result
    }
}

/// Generate ISO timestamp without chrono (same approach as vaak-mcp.rs)
fn chrono_free_iso_now() -> String {
    let dur = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs();
    let hours = (secs % 86400) / 3600;
    let mins = (secs % 3600) / 60;
    let s = secs % 60;
    let days = secs / 86400;
    let mut y = 1970u64;
    let mut remaining = days;
    loop {
        let diy = if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 { 366 } else { 365 };
        if remaining < diy { break; }
        remaining -= diy;
        y += 1;
    }
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let month_days: [u64; 12] = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut m = 0u64;
    for md in &month_days {
        if remaining < *md { break; }
        remaining -= *md;
        m += 1;
    }
    format!("{}-{:02}-{:02}T{:02}:{:02}:{:02}Z", y, m + 1, remaining + 1, hours, mins, s)
}

#[tauri::command]
fn stop_watching_project() -> Result<(), String> {
    *get_project_watched_dir().lock() = None;
    *get_project_last_mtimes().lock() = (None, None, None, None);
    Ok(())
}

/// Start a background thread that polls .vaak/ project files for changes (1-second interval)
fn start_project_watcher(app_handle: tauri::AppHandle) {
    std::thread::spawn(move || {
        let mut cleanup_counter: u32 = 0;
        loop {
            std::thread::sleep(std::time::Duration::from_secs(1));

            // Check shutdown flag (same pattern as HTTP server)
            if HTTP_SERVER_SHUTDOWN.load(Ordering::Relaxed) {
                eprintln!("[watcher] Shutdown flag set, exiting file watcher thread");
                break;
            }

            let dir = {
                let guard = get_project_watched_dir().lock();
                match guard.as_ref() {
                    Some(d) => d.clone(),
                    None => {
                        cleanup_counter = 0;
                        continue;
                    }
                }
            };

            // Run cleanup every 30 seconds to remove gone sessions
            cleanup_counter += 1;
            let mut did_cleanup = false;
            if cleanup_counter >= 30 {
                cleanup_counter = 0;
                did_cleanup = collab::cleanup_gone_sessions(&dir);
                if did_cleanup {
                    // Update stored mtime after cleanup write to avoid re-triggering
                    let vaak_dir = std::path::Path::new(&dir).join(".vaak");
                    let sessions_mtime = vaak_dir.join("sessions.json").metadata().ok().and_then(|m| m.modified().ok());
                    let mut mtimes = get_project_last_mtimes().lock();
                    mtimes.1 = sessions_mtime;
                }
            }

            let vaak_dir = std::path::Path::new(&dir).join(".vaak");
            let project_path = vaak_dir.join("project.json");
            let sessions_path = vaak_dir.join("sessions.json");
            let board_path = collab::active_board_path(&dir);
            let claims_path = vaak_dir.join("claims.json");

            let current_mtimes = (
                project_path.metadata().ok().and_then(|m| m.modified().ok()),
                sessions_path.metadata().ok().and_then(|m| m.modified().ok()),
                board_path.metadata().ok().and_then(|m| m.modified().ok()),
                claims_path.metadata().ok().and_then(|m| m.modified().ok()),
            );

            let last = { *get_project_last_mtimes().lock() };

            let changed = did_cleanup
                || current_mtimes.0 != last.0
                || current_mtimes.1 != last.1
                || current_mtimes.2 != last.2
                || current_mtimes.3 != last.3;

            if changed {
                *get_project_last_mtimes().lock() = current_mtimes;

                if let Some(parsed) = collab::parse_project_dir(&dir) {
                    if let Ok(json) = serde_json::to_value(&parsed) {
                        if let Some(window) = app_handle.get_webview_window("main") {
                            let _ = window.emit("project-update", &json);
                        }
                        if let Some(window) = app_handle.get_webview_window("transcript") {
                            let _ = window.emit("project-update", &json);
                        }
                    }
                }
            }
        }
    });
}

fn main() {
    // Log startup attempt for debugging
    log_error("Vaak starting...");

    let builder = tauri::Builder::default()
        .manage(AudioRecorderState(Mutex::new(AudioRecorder::new())))
        .manage(ScreenReaderConversationState(Mutex::new(ScreenReaderConversation::default())))
        .manage(launcher::LauncherState::default())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // Create tray menu
            let show_item = MenuItemBuilder::with_id("show", "Show Vaak").build(app)?;
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
                .tooltip("Vaak - Ready")
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

            // Start project file watcher (polls .vaak/ files every 1s)
            start_project_watcher(app.handle().clone());

            // Native keyboard hook placeholder (currently unused)
            keyboard_hook::start_hook(app.handle().clone(), "CommandOrControl+Shift+S");

            // Register Alt+R global hotkey for screen reader
            {
                use tauri_plugin_global_shortcut::GlobalShortcutExt;
                let app_handle = app.handle().clone();
                app.global_shortcut().on_shortcut("Alt+R", move |_app, _shortcut, event| {
                    if event.state == tauri_plugin_global_shortcut::ShortcutState::Pressed {
                        let handle = app_handle.clone();
                        // Stop any currently playing audio, then emit earcon and describe
                        if let Some(window) = handle.get_webview_window("main") {
                            let _ = window.emit("screen-reader-stop-speaking", ());
                            let _ = window.emit("screen-reader-start", ());
                        }
                        std::thread::spawn(move || {
                            match describe_screen_core(&handle) {
                                Ok(_desc) => {
                                    if let Some(window) = handle.get_webview_window("main") {
                                        let _ = window.emit("screen-reader-done", ());
                                    }
                                }
                                Err(e) => {
                                    log_error(&format!("Screen reader failed: {}", e));
                                    if let Some(window) = handle.get_webview_window("main") {
                                        let _ = window.emit("screen-reader-error", e);
                                    }
                                }
                            }
                        });
                    }
                }).map_err(|e| format!("Failed to register Alt+R hotkey: {}", e))?;
                log_error("[setup] Screen reader hotkey Alt+R registered");
            }

            // Register Alt+A global hotkey for screen reader follow-up questions (push-to-talk)
            {
                use tauri_plugin_global_shortcut::GlobalShortcutExt;
                let app_handle = app.handle().clone();
                app.global_shortcut().on_shortcut("Alt+A", move |_app, _shortcut, event| {
                    match event.state {
                        tauri_plugin_global_shortcut::ShortcutState::Pressed => {
                            // Start recording for follow-up question
                            let recorder_state = app_handle.state::<AudioRecorderState>();
                            let recorder = recorder_state.0.lock();
                            if recorder.is_recording() {
                                return; // Already recording, ignore
                            }

                            // Computer use takes its own screenshots, no need to require Alt+R first

                            // Cancel any running computer use loop IMMEDIATELY
                            COMPUTER_USE_REQUEST_ID.fetch_add(1, Ordering::SeqCst);

                            if let Some(window) = app_handle.get_webview_window("main") {
                                let _ = window.emit("screen-reader-stop-speaking", ());
                                let _ = window.emit("screen-reader-ask-start", ());
                            }

                            if let Err(e) = recorder.start(Some(app_handle.clone())) {
                                log_error(&format!("Alt+A: Failed to start recording: {}", e));
                            }
                        }
                        tauri_plugin_global_shortcut::ShortcutState::Released => {
                            // Stop recording and process the question
                            let handle = app_handle.clone();

                            let audio_data = {
                                let recorder_state = handle.state::<AudioRecorderState>();
                                let recorder = recorder_state.0.lock();
                                if !recorder.is_recording() {
                                    return; // Not recording, ignore
                                }
                                match recorder.stop() {
                                    Ok(data) => data,
                                    Err(e) => {
                                        log_error(&format!("Alt+A: Failed to stop recording: {}", e));
                                        if let Some(window) = handle.get_webview_window("main") {
                                            let _ = window.emit("screen-reader-ask-error", e);
                                        }
                                        return;
                                    }
                                }
                            };

                            // Ignore very short recordings (< 0.5s) to avoid accidental taps
                            if audio_data.duration_secs < 0.5 {
                                log_error("Alt+A: Recording too short, ignoring");
                                return;
                            }

                            std::thread::spawn(move || {
                                let client = ureq::AgentBuilder::new()
                                    .timeout(std::time::Duration::from_secs(45))
                                    .build();

                                // 1. Transcribe the audio
                                let transcribe_body = serde_json::json!({
                                    "audio_base64": audio_data.audio_base64,
                                });

                                let transcribe_resp = match client.post(&format!("{}/api/v1/transcribe-base64", get_backend_url()))
                                    .set("Content-Type", "application/json")
                                    .send_string(&transcribe_body.to_string())
                                {
                                    Ok(r) => r,
                                    Err(e) => {
                                        log_error(&format!("Alt+A: Transcription request failed: {}", e));
                                        if let Some(window) = handle.get_webview_window("main") {
                                            let _ = window.emit("screen-reader-ask-error", format!("{}", e));
                                        }
                                        return;
                                    }
                                };

                                let transcribe_text = match transcribe_resp.into_string() {
                                    Ok(body) => {
                                        match serde_json::from_str::<serde_json::Value>(&body) {
                                            Ok(json) => json.get("raw_text")
                                                .and_then(|t| t.as_str())
                                                .unwrap_or("")
                                                .to_string(),
                                            Err(_) => String::new(),
                                        }
                                    }
                                    Err(_) => String::new(),
                                };

                                if transcribe_text.trim().is_empty() {
                                    log_error("Alt+A: Empty transcription, ignoring");
                                    if let Some(window) = handle.get_webview_window("main") {
                                        let _ = window.emit("screen-reader-ask-error", "Could not understand audio");
                                    }
                                    return;
                                }

                                log_error(&format!("Alt+A: Transcribed question: {}", transcribe_text));

                                // Get voice_id from disk settings (user's actual preference)
                                let voice_id = {
                                    let disk_settings = load_sr_settings_from_disk();
                                    disk_settings.voice_id
                                };

                                // 2. Computer Use Loop
                                // Bump the request ID — any previous loop will see the mismatch and abort
                                let my_request_id = COMPUTER_USE_REQUEST_ID.fetch_add(1, Ordering::SeqCst) + 1;

                                // Take screenshot AFTER transcription so it's as fresh as possible
                                log_error("Alt+A: Taking fresh screenshot after transcription");
                                let (screenshot_b64, disp_w, disp_h) = match capture_screenshot_base64_with_size(1280) {
                                    Ok(s) => s,
                                    Err(e) => {
                                        log_error(&format!("Alt+A: Screenshot failed: {}", e));
                                        if let Some(window) = handle.get_webview_window("main") {
                                            let _ = window.emit("screen-reader-ask-error", format!("{}", e));
                                        }
                                        return;
                                    }
                                };

                                // Capture accessibility tree for initial message
                                let initial_uia = match a11y::capture_tree() {
                                    Ok(tree) => Some(a11y::format_tree_for_prompt(&tree)),
                                    Err(_) => None,
                                };
                                let user_text = if let Some(ref tree_text) = initial_uia {
                                    format!("{}\n\n{}", tree_text, transcribe_text)
                                } else {
                                    transcribe_text.clone()
                                };

                                let mut messages: Vec<serde_json::Value> = vec![
                                    serde_json::json!({
                                        "role": "user",
                                        "content": [
                                            {
                                                "type": "image",
                                                "source": {
                                                    "type": "base64",
                                                    "media_type": "image/png",
                                                    "data": screenshot_b64,
                                                }
                                            },
                                            {
                                                "type": "text",
                                                "text": user_text,
                                            }
                                        ]
                                    })
                                ];

                                let max_iterations = 10;
                                let mut final_text = String::new();
                                let mut consecutive_same_action = 0u32;
                                let mut last_action_str = String::new();

                                for iteration in 0..max_iterations {
                                    // Check if a newer request has started — if so, abort this loop
                                    if COMPUTER_USE_REQUEST_ID.load(Ordering::SeqCst) != my_request_id {
                                        log_error("Alt+A: Cancelled by newer request, aborting loop");
                                        return;
                                    }

                                    log_error(&format!("Alt+A: Computer use iteration {}", iteration));

                                    // Capture fresh accessibility tree for the system prompt
                                    let cu_uia_tree = match a11y::capture_tree() {
                                        Ok(tree) => Some(a11y::format_tree_for_prompt(&tree)),
                                        Err(_) => None,
                                    };
                                    let mut cu_body = serde_json::json!({
                                        "messages": messages,
                                        "display_width": disp_w,
                                        "display_height": disp_h,
                                    });
                                    if let Some(ref tree_text) = cu_uia_tree {
                                        cu_body["uia_tree"] = serde_json::Value::String(tree_text.clone());
                                    }

                                    let cu_resp = match client.post(&format!("{}/api/v1/computer-use", get_backend_url()))
                                        .set("Content-Type", "application/json")
                                        .send_string(&cu_body.to_string())
                                    {
                                        Ok(r) => r,
                                        Err(e) => {
                                            log_error(&format!("Alt+A: Computer use request failed: {}", e));
                                            if let Some(window) = handle.get_webview_window("main") {
                                                let _ = window.emit("screen-reader-ask-error", format!("{}", e));
                                            }
                                            return;
                                        }
                                    };

                                    let resp_json: serde_json::Value = match cu_resp.into_string() {
                                        Ok(body) => match serde_json::from_str(&body) {
                                            Ok(j) => j,
                                            Err(e) => {
                                                log_error(&format!("Alt+A: Failed to parse CU response: {}", e));
                                                return;
                                            }
                                        },
                                        Err(e) => {
                                            log_error(&format!("Alt+A: Failed to read CU response: {}", e));
                                            return;
                                        }
                                    };

                                    // Check cancellation again after HTTP call returned
                                    if COMPUTER_USE_REQUEST_ID.load(Ordering::SeqCst) != my_request_id {
                                        log_error("Alt+A: Cancelled after HTTP response, aborting");
                                        return;
                                    }

                                    let stop_reason = resp_json.get("stop_reason")
                                        .and_then(|s| s.as_str())
                                        .unwrap_or("end_turn");
                                    let content = resp_json.get("content")
                                        .and_then(|c| c.as_array())
                                        .cloned()
                                        .unwrap_or_default();

                                    // Speak text blocks immediately as they arrive
                                    for block in &content {
                                        if block.get("type").and_then(|t| t.as_str()) == Some("text") {
                                            if let Some(txt) = block.get("text").and_then(|t| t.as_str()) {
                                                if !txt.trim().is_empty() {
                                                    // Speak this text block right now
                                                    let ts = std::time::SystemTime::now()
                                                        .duration_since(std::time::UNIX_EPOCH)
                                                        .map(|d| d.as_millis() as u64)
                                                        .unwrap_or(0);
                                                    let speak_payload = serde_json::json!({
                                                        "text": txt,
                                                        "session_id": format!("screen-reader-cu-{}", ts),
                                                        "timestamp": ts,
                                                        "voice_id": voice_id,
                                                    });
                                                    if let Some(window) = handle.get_webview_window("main") {
                                                        let _ = window.emit("speak", &speak_payload);
                                                    }
                                                }
                                                if !final_text.is_empty() {
                                                    final_text.push(' ');
                                                }
                                                final_text.push_str(txt);
                                            }
                                        }
                                    }

                                    if stop_reason == "tool_use" {
                                        // Find tool_use blocks, execute them, and build tool_results
                                        // First, add the assistant message with all content blocks
                                        messages.push(serde_json::json!({
                                            "role": "assistant",
                                            "content": content,
                                        }));

                                        let mut tool_results: Vec<serde_json::Value> = Vec::new();

                                        for block in &content {
                                            if block.get("type").and_then(|t| t.as_str()) != Some("tool_use") {
                                                continue;
                                            }

                                            let tool_id = block.get("id").and_then(|i| i.as_str()).unwrap_or("");
                                            let input = block.get("input").unwrap_or(&serde_json::Value::Null);
                                            let action_name = input.get("action").and_then(|a| a.as_str()).unwrap_or("unknown");

                                            log_error(&format!("Alt+A: Executing action: {} (id={})", action_name, tool_id));

                                            // Detect if Claude is stuck repeating the same action
                                            let action_fingerprint = format!("{}-{}", action_name, input.to_string());
                                            if action_fingerprint == last_action_str {
                                                consecutive_same_action += 1;
                                            } else {
                                                consecutive_same_action = 0;
                                                last_action_str = action_fingerprint;
                                            }
                                            if consecutive_same_action >= 3 {
                                                log_error("Alt+A: Claude stuck repeating same action 3 times, breaking loop");
                                                let ts = std::time::SystemTime::now()
                                                    .duration_since(std::time::UNIX_EPOCH)
                                                    .map(|d| d.as_millis() as u64)
                                                    .unwrap_or(0);
                                                let speak_payload = serde_json::json!({
                                                    "text": "I wasn't able to complete that action after several attempts.",
                                                    "session_id": format!("screen-reader-cu-{}", ts),
                                                    "timestamp": ts,
                                                    "voice_id": voice_id,
                                                });
                                                if let Some(window) = handle.get_webview_window("main") {
                                                    let _ = window.emit("speak", &speak_payload);
                                                }
                                                break;
                                            }

                                            // Scale coordinates: Claude thinks screen is disp_w x disp_h
                                            // but actual screen may be larger
                                            let (real_w, real_h) = get_screen_dimensions();
                                            let sx = real_w as f64 / disp_w as f64;
                                            let sy = real_h as f64 / disp_h as f64;

                                            // Execute the action
                                            let exec_result = execute_computer_action(input, sx, sy);
                                            if let Err(ref e) = exec_result {
                                                log_error(&format!("Alt+A: Action failed: {}", e));
                                            }

                                            // Check cancellation before taking screenshot
                                            if COMPUTER_USE_REQUEST_ID.load(Ordering::SeqCst) != my_request_id {
                                                log_error("Alt+A: Cancelled after action, aborting");
                                                return;
                                            }

                                            // Take medium-res verification screenshot (1024px)
                                            // Balances token cost vs readability for finding buttons/text
                                            let screenshot_result = if action_name != "screenshot" {
                                                thread::sleep(Duration::from_millis(300));
                                                capture_screenshot_base64_with_size(1024)
                                            } else {
                                                capture_screenshot_base64_with_size(1024)
                                            };

                                            match screenshot_result {
                                                Ok((new_screenshot, _, _)) => {
                                                    // Capture fresh accessibility tree after action
                                                    let uia_text = match a11y::capture_tree() {
                                                        Ok(tree) => Some(a11y::format_tree_for_prompt(&tree)),
                                                        Err(_) => None,
                                                    };
                                                    let mut content_blocks = vec![
                                                        serde_json::json!({
                                                            "type": "image",
                                                            "source": {
                                                                "type": "base64",
                                                                "media_type": "image/png",
                                                                "data": new_screenshot,
                                                            }
                                                        }),
                                                    ];
                                                    if let Some(tree_text) = uia_text {
                                                        content_blocks.push(serde_json::json!({
                                                            "type": "text",
                                                            "text": tree_text,
                                                        }));
                                                    }
                                                    tool_results.push(serde_json::json!({
                                                        "type": "tool_result",
                                                        "tool_use_id": tool_id,
                                                        "content": content_blocks,
                                                    }));
                                                }
                                                Err(e) => {
                                                    log_error(&format!("Alt+A: Post-action screenshot failed: {}", e));
                                                    tool_results.push(serde_json::json!({
                                                        "type": "tool_result",
                                                        "tool_use_id": tool_id,
                                                        "content": format!("Action executed but screenshot failed: {}", e),
                                                        "is_error": true,
                                                    }));
                                                }
                                            }
                                        }

                                        // Add tool results as a user message
                                        messages.push(serde_json::json!({
                                            "role": "user",
                                            "content": tool_results,
                                        }));

                                    } else {
                                        // stop_reason is "end_turn" — we're done
                                        break;
                                    }
                                }

                                // Store in conversation history for context
                                {
                                    let conv_state = handle.state::<ScreenReaderConversationState>();
                                    let mut conv = conv_state.0.lock();
                                    conv.messages.push(("user".to_string(), transcribe_text.clone()));
                                    conv.messages.push(("assistant".to_string(), final_text.clone()));
                                }

                                // If no text was spoken during the loop, say a default
                                if final_text.trim().is_empty() {
                                    let ts = std::time::SystemTime::now()
                                        .duration_since(std::time::UNIX_EPOCH)
                                        .map(|d| d.as_millis() as u64)
                                        .unwrap_or(0);
                                    let payload = serde_json::json!({
                                        "text": "Done.",
                                        "session_id": format!("screen-reader-cu-{}", ts),
                                        "timestamp": ts,
                                        "voice_id": voice_id,
                                    });
                                    if let Some(window) = handle.get_webview_window("main") {
                                        let _ = window.emit("speak", &payload);
                                    }
                                }

                                if let Some(window) = handle.get_webview_window("main") {
                                    let _ = window.emit("screen-reader-ask-done", ());
                                }

                                log_error(&format!("Alt+A: Complete ({} chars spoken)", final_text.len()));
                            });
                        }
                    }
                }).map_err(|e| format!("Failed to register Alt+A hotkey: {}", e))?;
                log_error("[setup] Screen reader ask hotkey Alt+A registered");
            }

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
            toggle_queue_window,
            start_recording,
            stop_recording,
            cancel_recording,
            get_audio_devices,
            check_recording,
            describe_screen,
            update_claude_md,
            save_voice_settings_cmd,
            set_auto_collab,
            get_auto_collab,
            set_human_in_loop,
            get_human_in_loop,
            save_screen_reader_settings,
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
            queue::get_next_pending_item,
            update_native_hotkey,
            show_screen_reader_window,
            toggle_screen_reader_window,
            capture_uia_tree_cmd,
            set_focus_tracking,
            watch_project_dir,
            stop_watching_project,
            initialize_project,
            copy_project_roles,
            read_role_briefing,
            send_team_message,
            set_workflow_type,
            set_discussion_mode,
            start_discussion,
            close_discussion_round,
            open_next_round,
            end_discussion,
            get_discussion_state,
            set_continuous_timeout,
            delete_message,
            clear_all_messages,
            set_message_retention,
            get_project_claims,
            claim_files,
            release_claim,
            create_section,
            switch_section,
            list_sections,
            get_active_section,
            // Roster commands
            roster_add_slot,
            roster_remove_slot,
            roster_get,
            // Role CRUD commands
            create_role,
            update_role,
            delete_role,
            // Role group commands
            save_role_group,
            delete_role_group,
            // Global role template commands
            save_role_as_global_template,
            list_global_role_templates,
            remove_global_role_template,
            // Team launcher commands
            launcher::check_claude_installed,
            launcher::launch_team_member,
            launcher::launch_team,
            launcher::kill_team_member,
            launcher::kill_all_team_members,
            launcher::get_spawned_agents,
            launcher::get_role_companions,
            launcher::repopulate_spawned,
            launcher::focus_agent_window,
            launcher::buzz_agent_terminal,
            launcher::check_macos_permissions,
            launcher::open_macos_settings,
            launcher::open_terminal_in_dir,
        ]);

    match builder.build(tauri::generate_context!()) {
        Ok(app) => {
            app.run(|_app_handle, event| {
                // Signal HTTP server to shut down when the app exits
                if let tauri::RunEvent::Exit = event {
                    HTTP_SERVER_SHUTDOWN.store(true, Ordering::Relaxed);
                    eprintln!("[main] App exiting — HTTP server shutdown signaled");
                }
                // NOTE: Do NOT kill spawned Claude agents on app exit.
                // Terminal sessions must survive app restarts so agents keep their context.
                // Use kill_all_team_members for explicit cleanup.
            });
        }
        Err(e) => {
            let error_msg = format!("Failed to run Tauri application: {}", e);
            log_error(&error_msg);

            // Show native error dialog based on platform
            show_error_dialog(&format!(
                "Vaak failed to start:\n\n{}\n\nCheck ~/vaak-error.log for details.",
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
    let wide_title: Vec<u16> = "Vaak Error"
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
        r#"display dialog "{}" with title "Vaak Error" buttons {{"OK"}} default button "OK" with icon stop"#,
        message.replace("\"", "\\\"").replace("\n", "\\n")
    );
    let _ = Command::new("osascript").arg("-e").arg(&script).output();
}

#[cfg(target_os = "linux")]
fn show_error_dialog(message: &str) {
    use std::process::Command;
    // Try zenity first (GTK), then kdialog (KDE), then notify-send as fallback
    let zenity = Command::new("zenity")
        .args(["--error", "--title=Vaak Error", &format!("--text={}", message)])
        .output();

    if zenity.is_err() || !zenity.unwrap().status.success() {
        let kdialog = Command::new("kdialog")
            .args(["--error", message, "--title", "Vaak Error"])
            .output();

        if kdialog.is_err() || !kdialog.unwrap().status.success() {
            // Last resort: notification
            let _ = Command::new("notify-send")
                .args(["Vaak Error", message])
                .output();
        }
    }
}
