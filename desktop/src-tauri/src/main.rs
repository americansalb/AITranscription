// Prevents additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// SHA-11.5 runtime fingerprint chain for main.rs (see launcher.rs convention).
// Note: commit SHA omitted from this fingerprint because main.rs is too
// frequently edited to keep a per-commit fingerprint per module. The
// SHA-X.Y feature label is sufficient for "is feature X loaded" verification.
// Grep: findstr /C:"VAAK_FP:SHA-11.1" target\debug\vaak-desktop.exe
#[used]
#[no_mangle]
pub static VAAK_FINGERPRINT_MAIN_SHA11_1: [u8; 42] =
    *b"VAAK_FP:SHA-11.1:main.rs:watchdog_cooldown";

#[used]
#[no_mangle]
pub static VAAK_FINGERPRINT_MAIN_SHA10_2: [u8; 51] =
    *b"VAAK_FP:SHA-10.2:main.rs:oxford_initiate_auto_phase";

mod a11y;
mod audio;
mod collab;
mod collab_v2;
// Phase 1 hot-reload architecture per `.vaak/design-notes/2026-05-28-hot-reload-architecture-spec.md`
// (architect commit 184d10d, human msg 2415). Each handler in this module is
// the Tauri-side authoritative implementation of an MCP tool the sidecar
// used to own; behavior changes ship via Vaak restart only.
mod mcp_handlers;
mod database;
mod keyboard_hook;
mod launcher;
mod protocol;
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

/// Read the persisted last-opened project path from
/// `%APPDATA%/com.scribe.app/last-project-v2.json`. Returns the path
/// string if the file exists and contains a `path` field; None otherwise.
/// Used by ProjectDirContext to auto-restore the project pointer when
/// localStorage is empty (e.g. after a webview-cache wipe per human msg 307).
#[tauri::command]
fn get_last_project_path() -> Option<String> {
    let appdata = std::env::var_os("APPDATA")?;
    let path = std::path::PathBuf::from(appdata)
        .join("com.scribe.app")
        .join("last-project-v2.json");
    let content = std::fs::read_to_string(&path).ok()?;
    let json: serde_json::Value = serde_json::from_str(&content).ok()?;
    json.get("path")?.as_str().map(|s| s.to_string())
}

/// Check if the vaak-mcp sidecar binary is accessible
#[tauri::command]
fn check_sidecar_status() -> serde_json::Value {
    match get_sidecar_path() {
        Some(path) => serde_json::json!({
            "found": true,
            "path": path.to_string_lossy().to_string()
        }),
        None => serde_json::json!({
            "found": false,
            "path": null
        }),
    }
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

// PERF (human msg 569/571 "why is vaak so memory-intensive / 19 windows"):
// these auxiliary windows used to be declared in tauri.conf.json's eager
// app.windows list, so Tauri spawned each one's full WebView2 (a whole
// Chromium engine, ~100MB+) at STARTUP even though the user only opens them
// on demand. They are now created LAZILY here on first show, replicating each
// window's original tauri.conf attributes exactly. The hide/toggle paths keep
// the window resident after first open (cheap re-show); only the first open
// pays creation. Nothing emits to these windows at startup, and the
// screen-reader settings window is NOT the live screen-reader path (Alt+A
// describe-screen + speak events are event-based, independent of this window),
// so lazy creation does not affect screen-reader functionality.

fn ensure_recording_overlay(app: &tauri::AppHandle) -> Result<tauri::WebviewWindow, String> {
    if let Some(w) = app.get_webview_window("recording-indicator") {
        return Ok(w);
    }
    tauri::WebviewWindowBuilder::new(
        app,
        "recording-indicator",
        tauri::WebviewUrl::App("index.html#/overlay".into()),
    )
    .title("")
    .inner_size(160.0, 48.0)
    .resizable(false)
    .decorations(false)
    .transparent(true)
    .always_on_top(true)
    .skip_taskbar(true)
    .focused(false)
    .visible(false)
    .build()
    .map_err(|e| e.to_string())
}

// NOTE: the `transcript` window is intentionally NOT lazy. It receives live
// emits while hidden — heartbeat, project-file-changed, speak-transcript,
// project-update (see emit sites below) — so it must stay resident to not drop
// transcription/TTS events. It remains in tauri.conf.json's eager window list.
// (Candidate for lazy conversion only after its hidden-window live-event
// dependencies are verified; not safe to defer blind.)

fn ensure_screen_reader_window(app: &tauri::AppHandle) -> Result<tauri::WebviewWindow, String> {
    if let Some(w) = app.get_webview_window("screen-reader") {
        return Ok(w);
    }
    tauri::WebviewWindowBuilder::new(
        app,
        "screen-reader",
        tauri::WebviewUrl::App("index.html#/screen-reader".into()),
    )
    .title("Vaak - Screen Reader")
    .inner_size(700.0, 600.0)
    .min_inner_size(500.0, 400.0)
    .center()
    .resizable(true)
    .decorations(true)
    .visible(false)
    .build()
    .map_err(|e| e.to_string())
}

fn ensure_queue_window(app: &tauri::AppHandle) -> Result<tauri::WebviewWindow, String> {
    if let Some(w) = app.get_webview_window("queue") {
        return Ok(w);
    }
    tauri::WebviewWindowBuilder::new(
        app,
        "queue",
        tauri::WebviewUrl::App("index.html#/queue".into()),
    )
    .title("Vaak - Voice Controls")
    .inner_size(420.0, 650.0)
    .min_inner_size(350.0, 400.0)
    .center()
    .resizable(true)
    .decorations(true)
    .visible(false)
    .build()
    .map_err(|e| e.to_string())
}

/// Show the floating recording indicator overlay
#[tauri::command]
fn show_recording_overlay(app: tauri::AppHandle) -> Result<(), String> {
    let window = ensure_recording_overlay(&app)?;
    // Just show - NEVER set_focus, or it steals focus from the user's app
    let _ = window.show();
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

/// Show the transcript window (eager — see ensure_* note above)
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
    let window = ensure_screen_reader_window(&app)?;
    let _ = window.show();
    let _ = window.set_focus();
    Ok(())
}

/// Toggle screen reader settings window visibility
#[tauri::command]
fn toggle_screen_reader_window(app: tauri::AppHandle) -> Result<(), String> {
    let window = ensure_screen_reader_window(&app)?;
    match window.is_visible() {
        Ok(true) => { let _ = window.hide(); },
        _ => {
            let _ = window.show();
            let _ = window.set_focus();
        }
    }
    Ok(())
}

/// Toggle transcript window visibility (eager — see ensure_* note above)
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
    let window = ensure_queue_window(&app)?;
    match window.is_visible() {
        Ok(true) => { let _ = window.hide(); },
        _ => {
            let _ = window.show();
            let _ = window.set_focus();
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
            log_error("WARNING: vaak-mcp sidecar binary not found! Claude Code collab will not work.");
            log_error("Expected locations checked: binaries/ dir, macOS Resources, dev paths");
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
        // Validate sidecar_path doesn't contain cmd injection characters
        if sidecar_path.chars().any(|c| matches!(c, '"' | '&' | '|' | '>' | '<' | '^')) {
            log_error(&format!("SECURITY: sidecar path contains dangerous characters, skipping hook setup: {}", sidecar_path));
            return;
        }

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
                                    .map(|c| c == command)
                                    .unwrap_or(false)
                            })
                        })
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false)
    };

    // Layer 3 keep-alive hook (vaak-mcp --keep-alive). Without this, the per-
    // seat session files (.vaak/sessions/<role>-<inst>.json) only get refreshed
    // when an MCP call goes through, so the supervisor's last_alive_at_ms goes
    // stale during long tool calls and the health pill's Layer 1 reads "0 of N
    // agents heartbeated recently". Installed as PreToolUse + PostToolUse so
    // every Claude tool call (not just MCP) keeps the seat alive.
    let keep_alive_command;
    #[cfg(windows)]
    {
        let ka_path = hooks_dir.join("vaak-keep-alive.cmd");
        let ka_content = format!("@echo off\n\"{}\" --keep-alive\n", sidecar_path.replace('/', "\\"));
        if let Err(e) = fs::write(&ka_path, &ka_content) {
            log_error(&format!("Failed to write keep-alive hook wrapper: {}", e));
            return;
        }
        keep_alive_command = ka_path.to_string_lossy().replace('\\', "/");
    }
    #[cfg(not(windows))]
    {
        let ka_sh_path = hooks_dir.join("vaak-keep-alive.sh");
        let ka_sh_content = format!("#!/bin/sh\n\"{}\" --keep-alive\n", sidecar_path);
        if let Err(e) = fs::write(&ka_sh_path, &ka_sh_content) {
            log_error(&format!("Failed to write keep-alive hook wrapper: {}", e));
            return;
        }
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&ka_sh_path, fs::Permissions::from_mode(0o755));
        keep_alive_command = ka_sh_path.to_string_lossy().to_string();
    }

    let prompt_hook_ok = check_hook_configured(&settings, "UserPromptSubmit", &hook_command);
    let stop_hook_ok = check_hook_configured(&settings, "Stop", &stop_hook_command);
    let pre_keep_alive_ok = check_hook_configured(&settings, "PreToolUse", &keep_alive_command);
    let post_keep_alive_ok = check_hook_configured(&settings, "PostToolUse", &keep_alive_command);

    if !prompt_hook_ok || !stop_hook_ok || !pre_keep_alive_ok || !post_keep_alive_ok {
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

        if !pre_keep_alive_ok {
            settings["hooks"]["PreToolUse"] = serde_json::json!([
                {
                    "hooks": [
                        {
                            "type": "command",
                            "command": keep_alive_command.clone()
                        }
                    ]
                }
            ]);
        }

        if !post_keep_alive_ok {
            settings["hooks"]["PostToolUse"] = serde_json::json!([
                {
                    "hooks": [
                        {
                            "type": "command",
                            "command": keep_alive_command.clone()
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
                    if !pre_keep_alive_ok {
                        log_error(&format!("Configured PreToolUse keep-alive: {}", keep_alive_command));
                    }
                    if !post_keep_alive_ok {
                        log_error(&format!("Configured PostToolUse keep-alive: {}", keep_alive_command));
                    }
                }
            }
            Err(e) => {
                log_error(&format!("Failed to serialize settings.json: {}", e));
            }
        }
    } else {
        log_error("All hooks (UserPromptSubmit, Stop, PreToolUse, PostToolUse) already configured");
    }
}

/// Start local HTTP server for Claude Code speak integration
/// SHA-HR.1.4.token — F9 fail-closed token file load+create+ACL per architect
/// msg 2470 + msg 2568 ruling. Returns the token string on success; Err
/// causes the `/mcp/*` endpoint to return 503 (fail-closed semantics).
///
/// On-disk layout: `.vaak/.mcp-proxy-token` contains 64 hex chars (32 random
/// bytes). Created lazily on first /mcp/* request once the Tauri-side
/// watched project_dir is known. ACL:
/// - Windows: `icacls /inheritance:r /grant:r %USERNAME%:F`
/// - Unix: `chmod 0600`
///
/// Fail-closed: if the file exists but readback fails OR the ACL set
/// command returns non-zero OR the file ends up world-readable, refuse to
/// return a token (caller returns 503). The endpoint will not authorize
/// any request until the operator (Tauri main) can read the token from
/// disk under restricted ACLs.
fn ensure_and_load_mcp_proxy_token(project_dir: &str) -> Result<String, String> {
    use std::io::Write;
    let token_path = std::path::Path::new(project_dir)
        .join(".vaak")
        .join(".mcp-proxy-token");

    // If file exists, read + return (assumes prior write set ACL).
    if let Ok(existing) = std::fs::read_to_string(&token_path) {
        let trimmed = existing.trim();
        if trimmed.len() == 64 && trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
            return Ok(trimmed.to_string());
        }
        // Malformed file — refuse to overwrite blind; require operator to delete.
        return Err(format!(
            "[ProxyTokenMalformed] {} exists but contents are not 64 hex chars; delete the file to regenerate",
            token_path.display()
        ));
    }

    // Generate 32 random bytes → 64 hex. Use two uuid::Uuid::new_v4 (16 bytes
    // each, cryptographically random per uuid v4) concatenated to get 32 bytes.
    let u1 = uuid::Uuid::new_v4();
    let u2 = uuid::Uuid::new_v4();
    let mut bytes = [0u8; 32];
    bytes[..16].copy_from_slice(u1.as_bytes());
    bytes[16..].copy_from_slice(u2.as_bytes());
    let hex: String = bytes
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect();

    // Ensure parent dir exists.
    if let Some(parent) = token_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    // Write atomically: tmp file + rename. (Atomic rename within same dir.)
    let tmp_path = token_path.with_extension("token.tmp");
    {
        let mut f = std::fs::File::create(&tmp_path)
            .map_err(|e| format!("[ProxyTokenWriteFailed] create tmp: {}", e))?;
        f.write_all(hex.as_bytes())
            .map_err(|e| format!("[ProxyTokenWriteFailed] write tmp: {}", e))?;
        f.sync_all().ok();
    }
    std::fs::rename(&tmp_path, &token_path)
        .map_err(|e| format!("[ProxyTokenWriteFailed] rename: {}", e))?;

    // Apply ACL. Fail-closed: if perm-set fails, delete the file and return
    // Err so the endpoint never authorizes a request with a world-readable
    // token.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o600);
        if let Err(e) = std::fs::set_permissions(&token_path, perms) {
            let _ = std::fs::remove_file(&token_path);
            return Err(format!("[ProxyTokenAclFailed] chmod 0600: {}", e));
        }
    }
    #[cfg(windows)]
    {
        // icacls inheritance:r + grant running user full control. If the
        // command fails OR the file is still world-readable, fail-closed
        // by removing the file.
        let username = std::env::var("USERNAME").unwrap_or_default();
        if username.is_empty() {
            let _ = std::fs::remove_file(&token_path);
            return Err("[ProxyTokenAclFailed] USERNAME env var missing".to_string());
        }
        let status = std::process::Command::new("icacls")
            .arg(&token_path)
            .arg("/inheritance:r")
            .arg("/grant:r")
            .arg(format!("{}:F", username))
            .output();
        match status {
            Ok(o) if o.status.success() => { /* perm set */ }
            Ok(o) => {
                let _ = std::fs::remove_file(&token_path);
                return Err(format!(
                    "[ProxyTokenAclFailed] icacls exit {}: {}",
                    o.status,
                    String::from_utf8_lossy(&o.stderr)
                ));
            }
            Err(e) => {
                let _ = std::fs::remove_file(&token_path);
                return Err(format!("[ProxyTokenAclFailed] icacls invoke: {}", e));
            }
        }
    }

    Ok(hex)
}

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
                // A0 (Wave 1 slowdown fix per architect msg 3651 + Ruling 6 v4):
                // mtime-tuple dedup. The MCP sidecar fires this endpoint on EVERY
                // board write (heartbeat × N agents + every board.jsonl append +
                // every claims.json update). With 7+ active agents that's the
                // dominant source of setProject re-renders. Mirror of the polling
                // watcher's dedup at line 5247 — only emit project-file-changed
                // when the 4-mtime tuple actually changed since last emit.
                //
                // Skip dedup if no dir watched (early in startup). Skip dedup
                // gracefully on metadata-read failure — emit (preserves old
                // fire-on-every-call behavior for safety) rather than swallow.
                let should_emit = {
                    let dir_opt = get_project_watched_dir().lock().clone();
                    if let Some(dir) = dir_opt {
                        let vaak_dir = std::path::Path::new(&dir).join(".vaak");
                        let project_path = vaak_dir.join("project.json");
                        let sessions_path = vaak_dir.join("sessions.json");
                        // Section-aware per dev-challenger:1 msg 4070 catch on dfbfc57:
                        // polling watcher (line 5290) uses active_board_path which
                        // resolves to .vaak/sections/<section>/board.jsonl for non-default
                        // sections. Hardcoding .vaak/board.jsonl here would miss real
                        // board writes when the active section is e.g. "5-12", causing
                        // A0 dedup to leak (tuple's board element never changes from
                        // active-section activity) and false-emit when default-section
                        // board.jsonl shifts independently.
                        let board_path = collab::active_board_path(&dir);
                        let claims_path = vaak_dir.join("claims.json");

                        let current_mtimes = (
                            project_path.metadata().ok().and_then(|m| m.modified().ok()),
                            sessions_path.metadata().ok().and_then(|m| m.modified().ok()),
                            board_path.metadata().ok().and_then(|m| m.modified().ok()),
                            claims_path.metadata().ok().and_then(|m| m.modified().ok()),
                        );

                        let mut last_lock = get_notify_last_mtimes().lock();
                        let changed = current_mtimes.0 != last_lock.0
                            || current_mtimes.1 != last_lock.1
                            || current_mtimes.2 != last_lock.2
                            || current_mtimes.3 != last_lock.3;

                        if changed {
                            *last_lock = current_mtimes;
                        }
                        changed
                    } else {
                        // No dir watched yet — fall through to emit (no dedup).
                        true
                    }
                };

                if should_emit {
                    // Emit event to ALL windows so the collab tab re-reads project files
                    if let Some(window) = app_handle.get_webview_window("main") {
                        let _ = window.emit("project-file-changed", serde_json::json!({}));
                    }
                    if let Some(window) = app_handle.get_webview_window("transcript") {
                        let _ = window.emit("project-file-changed", serde_json::json!({}));
                    }
                }
                let response = Response::from_string(r#"{"status":"ok"}"#)
                    .with_header(
                        tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap()
                    );
                let _ = request.respond(response);
                continue;
            }

            // SHA-HR.1.4.token — Phase 1 F9 fail-closed enforcement per architect msg 2568
            // ruling: token verification MUST be active at endpoint lifetime. Closes the
            // auth-off violation in 3588f70. Local-process spoof window (evil-arch msg 2564
            // PowerShell snippet) now blocked by 401 unless caller has read .vaak/.mcp-proxy-token.
            //
            // SHA-HR.1.4 — Phase 1 hot-reload architecture per
            // `.vaak/design-notes/2026-05-28-hot-reload-architecture-spec.md`
            // + human msg 2415. POST /mcp/assembly_line is the pilot endpoint
            // for the sidecar-to-Tauri HTTP proxy channel. SHA-HR.1.3 already
            // wired do_protocol_mutate_inner's set_preset arm to call the
            // moved mcp_handlers::assembly_line helpers; this endpoint exposes
            // that path over the existing tiny_http listener at port 7865.
            //
            // Envelope per architect msg 2426 Q4 + dev-challenger msg 2526 F2
            // resolution: {ok: bool, result: <opaque>, error: <string|null>}.
            // Sidecar parses ONLY top-level envelope, passes `result` opaque to
            // MCP caller. Hot-reload preserved for field renames inside result.
            //
            // Token ACL per architect msg 2470 F9 ruling — deferred to follow-on
            // commit (SHA-HR.1.4.token). Phase 1 ships AUTH-OFF on this endpoint
            // with a strong-warn comment so reviewers catch it. Single-tenant
            // localhost is the current threat model; tightening before
            // multi-tenant deployment is the gate.
            if method == "POST" && url == "/mcp/assembly_line" {
                // SHA-HR.1.4.token — F9 fail-closed: ensure token exists + ACL set,
                // then check X-Vaak-Token header before processing. Closes the
                // auth-off violation per architect msg 2568.
                // Read X-Vaak-Token header first so we can short-circuit 401 BEFORE
                // any body parsing.
                let provided_token: Option<String> = request
                    .headers()
                    .iter()
                    .find(|h| h.field.as_str().as_str().eq_ignore_ascii_case("X-Vaak-Token"))
                    .map(|h| h.value.as_str().to_string());

                // Resolve project_dir early (needed for token-file path)
                let pd_for_token = get_project_watched_dir().lock().clone();
                let pd_for_token = match pd_for_token {
                    Some(p) => p,
                    None => {
                        let resp = Response::from_string(
                            r#"{"ok":false,"result":null,"error":"[NoWatchedProject] Tauri app is not watching a project yet"}"#,
                        )
                        .with_status_code(409)
                        .with_header(
                            tiny_http::Header::from_bytes(
                                &b"Content-Type"[..],
                                &b"application/json"[..],
                            )
                            .unwrap(),
                        );
                        let _ = request.respond(resp);
                        continue;
                    }
                };

                let expected_token = match ensure_and_load_mcp_proxy_token(&pd_for_token) {
                    Ok(t) => t,
                    Err(e) => {
                        let resp_body = format!(
                            r#"{{"ok":false,"result":null,"error":"[ProxyTokenSetupFailed] {}"}}"#,
                            e.replace('"', "'")
                        );
                        let resp = Response::from_string(resp_body)
                            .with_status_code(503)
                            .with_header(
                                tiny_http::Header::from_bytes(
                                    &b"Content-Type"[..],
                                    &b"application/json"[..],
                                )
                                .unwrap(),
                            );
                        let _ = request.respond(resp);
                        continue;
                    }
                };
                match &provided_token {
                    Some(t) if t == &expected_token => { /* auth ok, fall through */ }
                    _ => {
                        let resp = Response::from_string(
                            r#"{"ok":false,"result":null,"error":"[Unauthorized] missing or invalid X-Vaak-Token header"}"#,
                        )
                        .with_status_code(401)
                        .with_header(
                            tiny_http::Header::from_bytes(
                                &b"Content-Type"[..],
                                &b"application/json"[..],
                            )
                            .unwrap(),
                        );
                        let _ = request.respond(resp);
                        continue;
                    }
                }

                // Read body
                let mut body = String::new();
                if request.as_reader().read_to_string(&mut body).is_err() {
                    let resp = Response::from_string(
                        r#"{"ok":false,"result":null,"error":"[BadRequest] cannot read body"}"#,
                    )
                    .with_status_code(400)
                    .with_header(
                        tiny_http::Header::from_bytes(
                            &b"Content-Type"[..],
                            &b"application/json"[..],
                        )
                        .unwrap(),
                    );
                    let _ = request.respond(resp);
                    continue;
                }

                // Parse JSON: {action, args, rev}
                let payload: serde_json::Value = match serde_json::from_str(&body) {
                    Ok(v) => v,
                    Err(e) => {
                        let resp_body = format!(
                            r#"{{"ok":false,"result":null,"error":"[BadJson] {}"}}"#,
                            e.to_string().replace('"', "'")
                        );
                        let resp = Response::from_string(resp_body)
                            .with_status_code(400)
                            .with_header(
                                tiny_http::Header::from_bytes(
                                    &b"Content-Type"[..],
                                    &b"application/json"[..],
                                )
                                .unwrap(),
                            );
                        let _ = request.respond(resp);
                        continue;
                    }
                };

                let action = payload
                    .get("action")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let args = payload
                    .get("args")
                    .cloned()
                    .unwrap_or_else(|| serde_json::json!({}));
                let rev = payload.get("rev").and_then(|v| v.as_u64());

                // project_dir already resolved as pd_for_token above for token check.
                let pd = pd_for_token;
                let section = collab::get_active_section(&pd);
                // Sidecar pilot: actor = "human" for the assembly_line action.
                // Phase 2 will pass real role:instance from the sidecar body.
                let actor = "human".to_string();

                let dispatch_result = do_protocol_mutate_inner(&pd, &actor, &section, &action, args, rev);

                // SHA-HR.1.6 — Empirical hot-reload canary. The `_hot_reload_phase`
                // sentinel field in the response envelope is present ONLY when the
                // request was dispatched through the Tauri-side mcp_handlers path
                // (not the sidecar's internal do_protocol_mutate fallback). For the
                // hot-reload acceptance per architect spec + tester msg 2522:
                //
                // 1. Change behavior here (e.g. flip "_hot_reload_phase" from 1 to 2)
                // 2. Restart Vaak only (NOT the sidecar / CC windows)
                // 3. Call assembly_line from a running CC session
                // 4. Observe the NEW sentinel value in the response
                //
                // If step 4 succeeds without rebuilding the sidecar, Phase 1's
                // hot-reload claim is empirically validated for the pilot tool.
                // Per architect msg 2419 spec acceptance criterion 5.
                let envelope = match dispatch_result {
                    Ok(result) => {
                        // Inject the canary sentinel into the result, then pass
                        // through opaque per Q4 envelope. The sidecar parses
                        // only the top-level {ok, result, error}; the sentinel
                        // rides inside result.
                        let mut result_with_sentinel = result;
                        if let Some(obj) = result_with_sentinel.as_object_mut() {
                            obj.insert(
                                "_hot_reload_phase".to_string(),
                                serde_json::json!(1),
                            );
                        }
                        serde_json::json!({
                            "ok": true,
                            "result": result_with_sentinel,
                            "error": serde_json::Value::Null
                        })
                    }
                    Err(err) => serde_json::json!({
                        "ok": false,
                        "result": serde_json::Value::Null,
                        "error": format!("[VaakAppError] {}", err)
                    }),
                };

                let resp = Response::from_string(envelope.to_string())
                    .with_header(
                        tiny_http::Header::from_bytes(
                            &b"Content-Type"[..],
                            &b"application/json"[..],
                        )
                        .unwrap(),
                    );
                let _ = request.respond(resp);
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
                    .unwrap_or_default();
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
            // CORS: only allow localhost origins (Tauri WebView + local dev)
            let origin = request.headers().iter()
                .find(|h| h.field.as_str().as_str().eq_ignore_ascii_case("origin"))
                .map(|h| h.value.as_str().to_string())
                .unwrap_or_default();
            let cors_origin = if origin.starts_with("http://localhost")
                || origin.starts_with("https://localhost")
                || origin.starts_with("http://127.0.0.1")
                || origin.starts_with("tauri://localhost")
            {
                origin.as_str()
            } else {
                "http://localhost"
            };
            let response = Response::from_string(response_body)
                .with_header(
                    tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..])
                        .unwrap(),
                )
                .with_header(
                    tiny_http::Header::from_bytes(
                        &b"Access-Control-Allow-Origin"[..],
                        cors_origin.as_bytes(),
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

        // Play native system sound as fallback when WebAudio is suspended (background window)
        // afplay is non-blocking and works even when the webview is not focused
        if state_str == "recording" {
            std::thread::spawn(|| {
                let _ = std::process::Command::new("afplay")
                    .args(["/System/Library/Sounds/Tink.aiff"])
                    .output();
            });
        } else if state_str == "success" {
            std::thread::spawn(|| {
                let _ = std::process::Command::new("afplay")
                    .args(["/System/Library/Sounds/Glass.aiff"])
                    .output();
            });
        } else if state_str == "error" {
            std::thread::spawn(|| {
                let _ = std::process::Command::new("afplay")
                    .args(["/System/Library/Sounds/Basso.aiff"])
                    .output();
            });
        }
    }

    Ok(())
}

// ==================== Project File Watcher ====================

/// Shared state for the project file watcher
static PROJECT_WATCHED_DIR: std::sync::OnceLock<Mutex<Option<String>>> = std::sync::OnceLock::new();
static PROJECT_LAST_MTIMES: std::sync::OnceLock<Mutex<(Option<std::time::SystemTime>, Option<std::time::SystemTime>, Option<std::time::SystemTime>, Option<std::time::SystemTime>)>> = std::sync::OnceLock::new();

/// Per-section protocol.json mtime tracking — separate from the four-tuple
/// above because protocol.json is section-scoped (different file per section)
/// and is written by the sidecar without going through Tauri's protocol_mutate
/// path. Without this watch, sidecar mutations (assembly auto-advance, yield,
/// etc.) silently update protocol.json on disk but never reach useProtocolState
/// — the UI's mic indicator becomes stale until the next Tauri-side mutation.
static PROTOCOL_LAST_MTIME: std::sync::OnceLock<Mutex<Option<(String, std::time::SystemTime)>>> = std::sync::OnceLock::new();

/// A0 (Wave 1 slowdown fix per architect msg 3651 + Ruling 6 v4):
/// dedup state for the /collab/notify HTTP endpoint. The MCP sidecar fires
/// POST /collab/notify on EVERY board write (sessions.json heartbeat × N
/// agents + board.jsonl appends + claims.json updates) with no dedup at
/// either end. With 7+ active agents this fires multiple times/second,
/// each triggering setProject on the React side → full CollabTab re-render.
/// Mirror of PROJECT_LAST_MTIMES (line 5247 polling watcher dedup) but
/// distinct mutex so the push-path and poll-path dedup don't interfere.
/// Reset implicitly on dir change because the four-tuple of mtimes from
/// a different dir is guaranteed to differ.
static NOTIFY_LAST_MTIMES: std::sync::OnceLock<Mutex<(Option<std::time::SystemTime>, Option<std::time::SystemTime>, Option<std::time::SystemTime>, Option<std::time::SystemTime>)>> = std::sync::OnceLock::new();

fn get_project_watched_dir() -> &'static Mutex<Option<String>> {
    PROJECT_WATCHED_DIR.get_or_init(|| Mutex::new(None))
}

fn get_project_last_mtimes() -> &'static Mutex<(Option<std::time::SystemTime>, Option<std::time::SystemTime>, Option<std::time::SystemTime>, Option<std::time::SystemTime>)> {
    PROJECT_LAST_MTIMES.get_or_init(|| Mutex::new((None, None, None, None)))
}

fn get_protocol_last_mtime() -> &'static Mutex<Option<(String, std::time::SystemTime)>> {
    PROTOCOL_LAST_MTIME.get_or_init(|| Mutex::new(None))
}

fn get_notify_last_mtimes() -> &'static Mutex<(Option<std::time::SystemTime>, Option<std::time::SystemTime>, Option<std::time::SystemTime>, Option<std::time::SystemTime>)> {
    NOTIFY_LAST_MTIMES.get_or_init(|| Mutex::new((None, None, None, None)))
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

    // Import any missing global role templates into this project
    match collab::grandfather_global_templates(&effective_dir) {
        Ok(n) if n > 0 => eprintln!("[main] Imported {} global role template(s) into project", n),
        Err(e) => eprintln!("[main] Global template import failed (non-fatal): {}", e),
        _ => {}
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

/// Toggle the currency economy on/off (human msg 1366). Writes
/// settings.currency_enabled to project.json; the sidecar's
/// record_currency_earn gate reads it to skip currency processing when off.
/// Default (absent) is treated as ON by readers — currency is opt-out.
#[tauri::command]
fn set_currency_enabled(dir: String, enabled: bool) -> Result<(), String> {
    let dir = validate_project_dir(&dir)?;
    let config_path = std::path::Path::new(&dir).join(".vaak").join("project.json");
    let content = std::fs::read_to_string(&config_path)
        .map_err(|e| format!("Failed to read project.json: {}", e))?;
    let mut config: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse project.json: {}", e))?;

    if config.get("settings").is_none() {
        config["settings"] = serde_json::json!({});
    }
    if let Some(settings) = config.get_mut("settings") {
        settings["currency_enabled"] = serde_json::Value::Bool(enabled);
    }

    let pretty = serde_json::to_string_pretty(&config)
        .map_err(|e| format!("Failed to serialize config: {}", e))?;
    collab::atomic_write(&config_path, pretty.as_bytes())
        .map_err(|e| format!("Failed to write project.json: {}", e))?;

    log_error(&format!("Vaak: currency_enabled set to {}", enabled));
    notify_collab_change();
    Ok(())
}

/// Dedicated channel for collab change notifications. Uses a single background
/// thread instead of spawning a new OS thread per call (25+ call sites).
/// Notifications are coalesced: rapid successive calls result in a single HTTP request.
static COLLAB_NOTIFY_TX: std::sync::OnceLock<std::sync::mpsc::Sender<()>> = std::sync::OnceLock::new();

fn init_collab_notifier() {
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    COLLAB_NOTIFY_TX.set(tx).ok();
    std::thread::Builder::new()
        .name("collab-notifier".into())
        .spawn(move || {
            let agent = ureq::AgentBuilder::new()
                .timeout(std::time::Duration::from_secs(2))
                .build();
            loop {
                // Block until a notification arrives
                match rx.recv() {
                    Ok(()) => {}
                    Err(_) => break, // Channel closed, exit thread
                }
                // Drain any queued notifications (coalesce rapid-fire calls)
                while rx.try_recv().is_ok() {}
                // Small delay to coalesce further
                std::thread::sleep(std::time::Duration::from_millis(50));
                while rx.try_recv().is_ok() {}
                // Send single HTTP notification
                let _ = agent.post("http://127.0.0.1:7865/collab/notify").send_string("");
            }
        })
        .ok();
}

/// Fire-and-forget notification to the local HTTP server (port 7865) so that
/// the MCP sidecar and frontend windows learn about board/discussion changes
/// made by Tauri commands. Without this, MCP waits up to 55s to discover changes.
fn notify_collab_change() {
    if let Some(tx) = COLLAB_NOTIFY_TX.get() {
        let _ = tx.send(());
    }
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
        format!("Continuous Review mode activated.\n\n**Topic:** {}\n**Moderator:** {}\n**Participants:** {}\n\nReview windows open automatically when developers post status updates. To be COUNTED, respond via project_send with type=\"submission\", body starting with: agree / neutral / disagree: [reason] / alternative: [proposal]. Other message types are NOT tallied. Silence within the timeout = consent.",
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

// ==================== Assembly Line UI commands ====================
// Tauri-side counterpart to the MCP `assembly_line` tool. Post-44a7123:
// `set_assembly_state` writes the legacy .vaak/[sections/<s>/]assembly.json
// AND mirrors into protocol.json via do_protocol_mutate_inner so the new
// ProtocolPanel surface stays in sync with the existing AL toggle button
// (closes the disconnect human #1122 flagged). Single source of truth is
// becoming protocol.json; legacy assembly.json kept for the one-release
// compat tail per spec §3.3.

#[tauri::command]
fn get_assembly_state(dir: String) -> Result<serde_json::Value, String> {
    let dir = validate_project_dir(&dir)?;
    // Human #1252 fix — single source of truth: project from protocol.json
    // (the new authoritative state) into the legacy assembly.json shape that
    // CollabTab's AL toggle button + assemblyState UI binding expect. No more
    // dual-source poll.
    let section = collab::get_active_section(&dir);
    let proto = protocol::read_protocol_for_section(&dir, &section);
    let active = proto.preset == "Assembly Line";
    Ok(serde_json::json!({
        "active": active,
        "current_speaker": proto.floor.current_speaker,
        "rotation_order": proto.floor.rotation_order,
        "started_at": proto.floor.started_at,
        "started_by": proto.last_writer_seat.unwrap_or_default(),
        "_via": "protocol.json"
    }))
}

#[tauri::command]
fn set_assembly_state(dir: String, action: String) -> Result<serde_json::Value, String> {
    let dir = validate_project_dir(&dir)?;
    // Human #1252 fix — true single source of truth. Writes ONLY to
    // protocol.json (full state: preset + floor.mode + rotation_order +
    // current_speaker), via direct `protocol::Protocol` mutation under
    // board.lock. Legacy `.vaak/assembly.json` is no longer written; the
    // project_send AL gate in vaak-mcp.rs will be migrated in the same push
    // so it reads protocol.json too.
    let section = collab::get_active_section(&dir);

    // Read active seats for rotation_order seeding (matches the prior
    // collab::set_assembly_v0 behavior).
    let active_seats: Vec<String> = {
        let sessions_path = std::path::Path::new(&dir).join(".vaak").join("sessions.json");
        let sessions: serde_json::Value = std::fs::read_to_string(&sessions_path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_else(|| serde_json::json!({"bindings": []}));
        let bindings = sessions.get("bindings").and_then(|b| b.as_array()).cloned().unwrap_or_default();
        bindings.iter()
            .filter(|b| b.get("status").and_then(|s| s.as_str()) == Some("active"))
            .filter_map(|b| {
                let role = b.get("role").and_then(|r| r.as_str())?;
                let inst = b.get("instance").and_then(|i| i.as_u64()).unwrap_or(0);
                Some(format!("{}:{}", role, inst))
            })
            .collect()
    };

    let lock_result: Result<Result<serde_json::Value, String>, String> = collab::with_board_lock(&dir, || {
        let mut proto = protocol::read_protocol_for_section(&dir, &section);
        match action.as_str() {
            "enable" => {
                proto.preset = "Assembly Line".to_string();
                proto.floor.mode = "round-robin".to_string();
                // Two-controls field MUST be set in lockstep with preset/mode.
                // This entry point mutates the Protocol struct directly (it does
                // NOT route through apply_set_preset, which is where the MCP
                // protocol_mutate path syncs assembly_active — vaak-mcp.rs:4726).
                // Omitting it here is the same desync apply_set_preset's own
                // sync was added to fix (vaak-mcp.rs:6498-6525): a UI surface
                // reading floor.assembly_active strictly (CollabTab.tsx:5840)
                // would show AL "off" while preset/mode say "on". Refactor-drift
                // sibling — keep both write paths writing the same field set.
                proto.floor.assembly_active = Some(true);
                proto.floor.rotation_order = active_seats.clone();
                proto.floor.current_speaker = active_seats.first().cloned();
                proto.floor.started_at = Some(collab::iso_now());
            }
            "disable" => {
                proto.preset = "Default chat".to_string();
                proto.floor.mode = "none".to_string();
                proto.floor.assembly_active = Some(false); // see "enable" note
                proto.floor.rotation_order = vec![];
                // current_speaker preserved per spec §2.2 normalize() — the
                // none-mode HOLDING semantics keep the speaker informational.
            }
            "get_state" => {
                // Read-only — return the current state without mutation.
                return Ok(Ok(serde_json::json!({
                    "active": proto.preset == "Assembly Line",
                    "current_speaker": proto.floor.current_speaker,
                    "rotation_order": proto.floor.rotation_order,
                    "started_at": proto.floor.started_at,
                    "_via": "protocol.json"
                })));
            }
            other => return Ok(Err(format!("[InvalidAction] unknown set_assembly_state action: {}", other))),
        }
        proto.rev += 1;
        proto.last_writer_seat = Some("human".to_string());
        proto.last_writer_action = Some(format!("set_assembly_state.{}", action));
        proto.rev_at = Some(collab::iso_now());
        if let Err(e) = protocol::write_protocol_for_section_unlocked(&dir, &section, &proto) {
            return Ok(Err(e));
        }
        Ok(Ok(serde_json::json!({
            "active": proto.preset == "Assembly Line",
            "current_speaker": proto.floor.current_speaker,
            "rotation_order": proto.floor.rotation_order,
            "started_at": proto.floor.started_at,
            "_via": "protocol.json"
        })))
    });
    match lock_result {
        Ok(inner) => inner,
        Err(e) => Err(e),
    }
}

// ==================== Protocol v6 — Slice 3 Tauri commands ====================
// Tauri-side counterpart to the MCP `get_protocol` / `protocol_mutate` tools
// (Slice 2, vaak-mcp.rs). Reads/writes the same .vaak/[sections/<s>/]protocol.json
// file. Used by useProtocolState React hook (Slice 3+4).
//
// Heartbeat snapshot is JOIN at read time (spec §3.1 perf rule) — joined
// here from sessions.json. Mirrors handle_get_protocol in vaak-mcp.rs.

#[tauri::command]
fn get_protocol_cmd(dir: String, section: Option<String>) -> Result<serde_json::Value, String> {
    let dir = validate_project_dir(&dir)?;
    let section_resolved = section.unwrap_or_else(|| collab::get_active_section(&dir));
    let protocol = protocol::read_protocol_for_section(&dir, &section_resolved);

    // Heartbeat snapshot from sessions.json (per spec §3.1 — heartbeat lives
    // at runtime, not in protocol state).
    let sessions_path = std::path::Path::new(&dir).join(".vaak").join("sessions.json");
    let sessions: serde_json::Value = std::fs::read_to_string(&sessions_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| serde_json::json!({"bindings": []}));

    let mut snapshot = serde_json::Map::new();
    if let Some(bindings) = sessions.get("bindings").and_then(|b| b.as_array()) {
        for b in bindings {
            let role = b.get("role").and_then(|v| v.as_str()).unwrap_or("");
            let inst = b.get("instance").and_then(|v| v.as_u64()).unwrap_or(0);
            if role.is_empty() {
                continue;
            }
            let label = format!("{}:{}", role, inst);
            let entry = serde_json::json!({
                "last_active_at_ms": b.get("last_active_at_ms").cloned().unwrap_or(serde_json::Value::Null),
                "last_drafting_at_ms": b.get("last_drafting_at_ms").cloned().unwrap_or(serde_json::Value::Null),
                "last_heartbeat": b.get("last_heartbeat").cloned().unwrap_or(serde_json::Value::Null),
                "connected": b.get("status").and_then(|s| s.as_str()).map(|s| s == "active").unwrap_or(false)
            });
            snapshot.insert(label, entry);
        }
    }

    Ok(serde_json::json!({
        "section": section_resolved,
        "protocol": protocol,
        "heartbeats": snapshot
    }))
}

/// Tauri-side mutate. Mirrors `handle_protocol_mutate` in vaak-mcp.rs but
/// runs in the desktop binary. Both processes coordinate via OS-level
/// board.lock — same JSON shape, same flock, no race.
///
/// On success, emits a `protocol_changed` window event so the useProtocolState
/// hook re-reads (spec §4 push-then-pull rule).
#[tauri::command]
fn protocol_mutate_cmd(
    window: tauri::Window,
    dir: String,
    action: String,
    args: Option<serde_json::Value>,
    rev: Option<u64>,
) -> Result<serde_json::Value, String> {
    let dir = validate_project_dir(&dir)?;
    let actor = "human".to_string(); // Tauri side = human-driven UI per spec §10 auth.
    let section = collab::get_active_section(&dir);
    let args_value = args.unwrap_or(serde_json::json!({}));

    let result = do_protocol_mutate_inner(&dir, &actor, &section, &action, args_value, rev)?;

    // Push event so all subscribed UIs re-read via get_protocol (spec §4.1
    // push-events-best-effort, get_protocol authoritative). Failure to emit
    // is non-fatal — the hook's StaleRev recovery (useProtocolState.ts) will
    // re-fetch on the next stale collision. Surface via eprintln per
    // tech-leader #959 nit (no silent `let _ =` on side-effect calls in new code).
    if let Err(e) = window.emit("protocol_changed", serde_json::json!({
        "section": section,
        "rev": result.get("rev").cloned().unwrap_or(serde_json::Value::Null)
    })) {
        eprintln!("[protocol] window.emit(protocol_changed) failed: {} — push best-effort, hook StaleRev recovery covers", e);
    }

    Ok(result)
}

/// Two-controls B.4.1 — list active seats for the moderator-picker dropdown
/// (Item 7 from UX-eng's spec). Reads `.vaak/sessions.json:bindings` and
/// returns active seats with their freshness signals so the UI can render a
/// "Set moderator" dropdown of currently-seated agents.
///
/// Tauri-only (not mirrored to vaak-mcp.rs per architect msg 1464 + platform-
/// engineer msg 1462). Agents don't need active-roster access from inside MCP
/// sessions; this is purely a frontend-facing list.
#[tauri::command]
fn list_active_seats_cmd(dir: String) -> Result<serde_json::Value, String> {
    let dir = validate_project_dir(&dir)?;
    let sessions_path = std::path::Path::new(&dir).join(".vaak").join("sessions.json");
    let sessions: serde_json::Value = match std::fs::read_to_string(&sessions_path) {
        Ok(s) => serde_json::from_str(&s).map_err(|e| format!("Failed to parse sessions.json: {}", e))?,
        Err(_) => serde_json::json!({"bindings": []}),
    };
    // Per human msg 4804 ("fix this active claims thing... make it non-negotiable") +
    // dev-challenger:0 msg 4778 two-class diagnosis: sessions.json:bindings:status
    // is a stored claim that doesn't reflect MCP-sidecar-death. Derive liveness from
    // per-seat .vaak/sessions/<role>-<inst>.json:last_alive_at_ms (written every
    // project_wait/project_send tick per vaak-mcp.rs:365 update_seat_alive_at_ms).
    // Threshold = collab::staleness_thresholds::ALIVE_STATE_STALE_MS (120s = 4× the
    // 30s heartbeat cadence); single source of truth shared with read_claims_filtered
    // + collab::read_claims_filtered + the watchdog at line 7395.
    const STALE_THRESHOLD_MS: u64 = collab::staleness_thresholds::ALIVE_STATE_STALE_MS;
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let sessions_dir = std::path::Path::new(&dir).join(".vaak").join("sessions");
    let mut seats: Vec<serde_json::Value> = Vec::new();
    if let Some(bindings) = sessions.get("bindings").and_then(|b| b.as_array()) {
        for b in bindings {
            let status = b.get("status").and_then(|s| s.as_str()).unwrap_or("");
            if status != "active" {
                continue;
            }
            let role = b.get("role").and_then(|s| s.as_str()).unwrap_or("");
            let instance = b.get("instance").and_then(|i| i.as_u64()).unwrap_or(0);
            if role.is_empty() {
                continue;
            }
            let label = format!("{}:{}", role, instance);
            let last_heartbeat = b.get("last_heartbeat").cloned().unwrap_or(serde_json::Value::Null);
            // Read per-seat liveness file and derive alive_state.
            let seat_file = sessions_dir.join(format!("{}-{}.json", role, instance));
            let last_alive_at_ms: u64 = std::fs::read_to_string(&seat_file)
                .ok()
                .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
                .and_then(|v| v.get("last_alive_at_ms").and_then(|m| m.as_u64()))
                .unwrap_or(0);
            let stale_ms = now_ms.saturating_sub(last_alive_at_ms);
            // alive_state: "active" if heartbeat <120s ago; "stale" if older;
            // "unknown" if last_alive_at_ms missing (just joined or pre-instrumentation).
            let alive_state = if last_alive_at_ms == 0 {
                "unknown"
            } else if stale_ms > STALE_THRESHOLD_MS {
                "stale"
            } else {
                "active"
            };
            seats.push(serde_json::json!({
                "role": role,
                "instance": instance,
                "label": label,
                "last_heartbeat": last_heartbeat,
                "last_alive_at_ms": last_alive_at_ms,
                "alive_state": alive_state,
                "stale_ms": stale_ms,
            }));
        }
    }
    // Sort by label (role:instance) alphabetically for stable dropdown order.
    seats.sort_by(|a, b| {
        a.get("label").and_then(|l| l.as_str()).unwrap_or("")
            .cmp(b.get("label").and_then(|l| l.as_str()).unwrap_or(""))
    });
    // Per human msg 1687: prepend the human seat so they can pick themselves
    // as moderator. The human is the desktop UI user, not an MCP-connected
    // agent — they don't have a sessions.json:bindings entry. Synthesized
    // here at the IPC layer so the frontend dropdown sees a human:0 option.
    // apply_set_moderator accepts any seat string (no roster validation), so
    // floor.moderator = "human:0" works at the protocol level. is_seat_exempt
    // exempts the human seat correctly when mic_passing_mode == "moderator".
    let mut with_human: Vec<serde_json::Value> = vec![serde_json::json!({
        "role": "human",
        "instance": 0,
        "label": "human:0",
        "last_heartbeat": serde_json::Value::Null,
        "last_alive_at_ms": 0,
        "alive_state": "human",
        "stale_ms": 0,
    })];
    with_human.extend(seats.into_iter());
    Ok(serde_json::json!({"seats": with_human}))
}

/// Currency UI (Phase 1 display) — frontend-facing balance read per human
/// msg 1300 ("you need it in the UI ... where are the coins"). Reads
/// .vaak/balances.json via the shared collab::currency module (rebuilds from
/// the ledger if the snapshot is missing). Returns, per active non-human seat:
/// settled balance, escrow held, timed-out flag, and the gold/silver/copper
/// display split. Seats with no ledger entry yet lazy-default to
/// STARTING_BALANCE_COPPER so coins render immediately, even before any
/// currency.jsonl activity. Tauri-only (frontend display); authoritative
/// currency writes happen in the sidecar.
#[tauri::command]
fn get_currency_balances_cmd(dir: String) -> Result<serde_json::Value, String> {
    let dir = validate_project_dir(&dir)?;
    // Snapshot is authoritative; rebuild from the ledger if it's missing.
    let snap = match collab::currency::read_balances_snapshot(&dir) {
        Ok(s) => {
            if s.seats.is_empty() && collab::currency::currency_jsonl_path(&dir).exists() {
                collab::currency::replay_balances_from_ledger(&dir).unwrap_or(s)
            } else {
                s
            }
        }
        Err(_) => collab::currency::BalancesSnapshot::default(),
    };
    let sessions_path = std::path::Path::new(&dir).join(".vaak").join("sessions.json");
    let sessions: serde_json::Value = std::fs::read_to_string(&sessions_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| serde_json::json!({ "bindings": [] }));
    let mut out: Vec<serde_json::Value> = Vec::new();
    if let Some(bindings) = sessions.get("bindings").and_then(|b| b.as_array()) {
        for b in bindings {
            if b.get("status").and_then(|s| s.as_str()) != Some("active") {
                continue;
            }
            let role = b.get("role").and_then(|s| s.as_str()).unwrap_or("");
            if role.is_empty() || role == "human" {
                continue;
            }
            let instance = b.get("instance").and_then(|i| i.as_u64()).unwrap_or(0);
            let label = format!("{}:{}", role, instance);
            let (balance, escrow_held, timed_out, initialized) = match snap.seats.get(&label) {
                Some(sb) => (sb.balance, sb.escrow_held, sb.timed_out, true),
                None => (collab::currency::STARTING_BALANCE_COPPER, 0, false, false),
            };
            let disp = collab::currency::copper_to_display(balance);
            out.push(serde_json::json!({
                "label": label,
                "role": role,
                "instance": instance,
                "balance_copper": balance,
                "escrow_held_copper": escrow_held,
                "timed_out": timed_out,
                "initialized": initialized,
                "display": { "gold": disp.gold, "silver": disp.silver, "copper": disp.copper },
            }));
        }
    }
    out.sort_by(|a, b| {
        a.get("label").and_then(|l| l.as_str()).unwrap_or("")
            .cmp(b.get("label").and_then(|l| l.as_str()).unwrap_or(""))
    });
    Ok(serde_json::json!({
        "turn_counter": snap.turn_counter,
        "seats": out,
    }))
}

/// Human msg 706 (2026-05-24) — Tauri-path Oxford debate initiate. The UI
/// calls this command directly from the Vaak webview to start an Oxford-style
/// debate. Caller hard-wired to "human:0" (only the human uses this surface).
/// Backend uses collab::oxford module's validate_initiate + write_active_oxford
/// helpers under with_oxford_lock — same atomicity as the MCP-tool path.
/// On success: appends Initiate OxfordEvent + writes active-oxford-debate.json.
/// Does NOT emit a board broadcast (the UI surfaces the debate via its own
/// polling); the MCP-tool path is the one that broadcasts.
#[tauri::command]
fn oxford_initiate_cmd(
    dir: String,
    moderator: String,
    side_a: Vec<String>,
    side_b: Vec<String>,
    premise: String,
    audience: Vec<String>,
    winning_side_reward_copper: Option<i64>,
) -> Result<serde_json::Value, String> {
    use collab::oxford::*;
    let dir = validate_project_dir(&dir)?;
    let caller = "human:0".to_string();

    // Build active_seats from sessions.json bindings (mirror of the MCP handler).
    let active_seats: Vec<String> = (|| -> Vec<String> {
        let sessions_path = std::path::Path::new(&dir).join(".vaak").join("sessions.json");
        let sessions: serde_json::Value = std::fs::read_to_string(&sessions_path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_else(|| serde_json::json!({"bindings": []}));
        let mut out = Vec::new();
        if let Some(bindings) = sessions.get("bindings").and_then(|b| b.as_array()) {
            for b in bindings {
                if b.get("status").and_then(|s| s.as_str()) != Some("active") { continue; }
                let role = b.get("role").and_then(|s| s.as_str()).unwrap_or("");
                if role.is_empty() || role == "human" { continue; }
                let instance = b.get("instance").and_then(|i| i.as_u64()).unwrap_or(0);
                out.push(format!("{}:{}", role, instance));
            }
        }
        out
    })();

    if let Some(r) = winning_side_reward_copper {
        if r < 0 {
            return Err("[OxfordInvalidReward] winning_side_reward_copper must be >= 0".to_string());
        }
    }
    let oxford_settings_default_reward = collab::currency::read_economy_settings(&dir).oxford_default_winning_reward_copper;
    validate_initiate(&caller, &moderator, &side_a, &side_b, &audience, &active_seats)?;

    // VAAK_FP:SHA-12.1:main.rs:oxford_initiate_liveness_gate
    // Per debate 10 post-mortem (human msg 1465, 2026-05-26): oxford_initiate
    // selected zombie seats (dev-challenger:0, evil-architect:0 — both
    // stale 11h+ at initiate time) as debaters. The UI moderator-picker
    // dropdown already exposes alive_state per seat (list_active_seats_cmd
    // above) but the backend did not enforce it. Gate here so backend
    // rejection matches what the picker shows greyed-out. PARITY: keep in
    // sync with vaak-mcp.rs:~9660 (MCP-path twin).
    let project_path = std::path::Path::new(&dir);
    for seat in std::iter::once(&moderator)
        .chain(side_a.iter())
        .chain(side_b.iter())
        .chain(audience.iter())
    {
        if !collab::is_seat_alive(project_path, seat) {
            return Err(format!(
                "[OxfordSeatNotAlive] {} heartbeat is stale (> {}s) — cannot initiate Oxford debate with a non-responsive seat. Pick a different seat or wait for the agent to reconnect.",
                seat,
                collab::staleness_thresholds::ALIVE_STATE_STALE_MS / 1000
            ));
        }
    }

    with_oxford_lock(&dir, || {
        if read_active_oxford(&dir)?.is_some() {
            return Err("[OxfordAlreadyActive]".to_string());
        }
        let debate_id = next_debate_id(&dir);
        let now = collab::iso_now();
        let reward = winning_side_reward_copper.unwrap_or(oxford_settings_default_reward);
        // SHA-10.2 (UI-path twin of vaak-mcp.rs:~9695): auto-declare
        // side_a[0] as opener and enter phase OpeningA. Mirrors the
        // sidecar-path auto-phase-entry per PARITY contract; both paths
        // must construct identical initial-state ActiveOxfordDebate to
        // avoid initiate-source-divergence (which is itself the SHA-5
        // class-of-bug evil-arch msg 1112 guard 1 warned about).
        //
        // PARITY: keep in sync with vaak-mcp.rs:~9695 (sidecar-path twin).
        // Per dev-challenger msg 1389 Flag 2 + architect msg 1391 LOCK:
        // both twin entry points MUST construct identical initial-state
        // ActiveOxfordDebate. Future SHA-10.x extensions adding fields
        // to ActiveOxfordDebate MUST update BOTH twin sites; extract-
        // helper-to-collab::oxford queued for SHA-2 hygiene to eliminate
        // this duplication structurally.
        let opener_seat = side_a.first().cloned();
        let (initial_phase, initial_speaker, initial_phase_started_at, initial_turn_history) =
            match opener_seat.as_ref() {
                Some(seat) => (
                    collab::oxford::OxfordPhase::OpeningA,
                    Some(seat.clone()),
                    Some(now.clone()),
                    vec![collab::oxford::OxfordTurn {
                        seat: seat.clone(),
                        started_at: now.clone(),
                        ended_at: None,
                        auto_opened: true,
                    }],
                ),
                None => (
                    collab::oxford::OxfordPhase::None,
                    None,
                    None,
                    Vec::new(),
                ),
            };
        let debate = ActiveOxfordDebate {
            debate_id,
            moderator: moderator.clone(),
            side_a: side_a.clone(),
            side_b: side_b.clone(),
            audience: audience.clone(),
            premise: premise.clone(),
            current_speaker: initial_speaker.clone(),
            started_at: now.clone(),
            turn_history: initial_turn_history,
            winning_side_reward_copper: reward,
            phase: initial_phase,
            phase_started_at: initial_phase_started_at,
            audience_question_queue: Vec::new(),
        };
        write_active_oxford(&dir, &debate)?;
        append_oxford_event(&dir, &OxfordEvent::Initiate {
            debate_id,
            timestamp: now.clone(),
            moderator: moderator.clone(),
            side_a: side_a.clone(),
            side_b: side_b.clone(),
            premise: premise.clone(),
            audience: audience.clone(),
            winning_side_reward_copper: reward,
        })?;
        // Per evil-arch msg 722 + tester msg 724 + architect msg 732: emit
        // board broadcast so agents discover the active debate by design,
        // not by gate-rejection error. Direct board.jsonl append under
        // with_board_lock; the architect-preferred indirect-via-HTTP path
        // is queued as a refactor — for now the locked direct write closes
        // the silent-initiate adversarial vector identified in msg 722.
        // SHA-5.1 (human msg 1165 debate 7 root cause): use active-section
        // path. Pre-5.1 hardcoded `.vaak/board.jsonl` which is the LEGACY
        // root board — agents on a non-default section (e.g. "5-24") watch
        // `.vaak/sections/<slug>/board.jsonl` and never saw the prompts.
        // collab::active_board_path resolves to the right path per section.
        let board_path = collab::active_board_path(&dir);
        let _ = collab::with_board_lock(&dir, || -> Result<(), String> {
            use std::io::Write;
            // Compute next msg id by scanning existing board for max id + 1.
            let max_id: u64 = std::fs::read_to_string(&board_path)
                .unwrap_or_default()
                .lines()
                .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
                .filter_map(|v| v.get("id").and_then(|i| i.as_u64()))
                .max()
                .unwrap_or(0);
            let msg = serde_json::json!({
                "id": max_id + 1,
                "from": "system",
                "to": "all",
                "type": "broadcast",
                "timestamp": collab::iso_now(),
                "subject": format!("[OxfordDebateInitiated] debate {} by human:0", debate_id),
                "body": format!(
                    "Oxford-style debate {} initiated by human:0 via UI.\nPremise: {}\nSide A: {}\nSide B: {}\nAudience: {}\nModerator: {} declares speakers via oxford_declare_speaker. Reward: {} copper from pool. Per spec §6.3 only the declared speaker can project_send during their turn; human:0 always bypasses.",
                    debate_id, premise,
                    side_a.join(", "), side_b.join(", "),
                    if audience.is_empty() { "(none)".to_string() } else { audience.join(", ") },
                    moderator, reward
                ),
                "metadata": {
                    "debate_id": debate_id,
                    "moderator": moderator.clone(),
                    "side_a": side_a.clone(),
                    "side_b": side_b.clone(),
                    "audience": audience.clone(),
                    "winning_side_reward_copper": reward,
                    "oxford_event": "initiate",
                    "initiated_via": "ui"
                }
            });
            if let Some(parent) = board_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let mut f = std::fs::OpenOptions::new()
                .create(true).append(true).open(&board_path)
                .map_err(|e| format!("board open append: {}", e))?;
            writeln!(f, "{}", msg.to_string()).map_err(|e| format!("board write: {}", e))?;
            // Commit 2d.4: ping each debater with a DIRECTED message so they
            // wake up and discover their assignment (closes code-interpreter
            // msg 894 TODO). Audience skipped (observers); moderator skipped
            // (they were selected by the human in the UI, presumably know).
            let mut next_id = max_id + 2;
            let make_ping = |id: u64, target: &str, side: &str| {
                serde_json::json!({
                    "id": id,
                    "from": "system",
                    "to": target,
                    "type": "directive",
                    "timestamp": collab::iso_now(),
                    "subject": format!("[OxfordDebateAssignment] debate {} — you are on {}", debate_id, side),
                    "body": format!(
                        "You have been selected for Oxford debate {} as a {} debater.\n\nPremise: {}\n\nModerator: {} will declare speakers via oxford_declare_speaker. Only the declared speaker can project_send during their turn — wait for the moderator to call on you. Winning side splits {} copper from the pool.\n\nPer spec §6.3, non-speaker debaters can use oxford_react for visual reactions (rate-limited).",
                        debate_id, side, premise, moderator, reward
                    ),
                    "metadata": {
                        "debate_id": debate_id,
                        "assigned_side": side,
                        "oxford_event": "debater_assigned",
                        "initiated_via": "ui"
                    }
                })
            };
            for seat in &side_a {
                let m = make_ping(next_id, seat, "side_a");
                writeln!(f, "{}", m.to_string()).map_err(|e| format!("board write: {}", e))?;
                next_id += 1;
            }
            for seat in &side_b {
                let m = make_ping(next_id, seat, "side_b");
                writeln!(f, "{}", m.to_string()).map_err(|e| format!("board write: {}", e))?;
                next_id += 1;
            }
            // SHA-10.2 (UI-path twin): emit [OxfordPhaseEntered] for
            // opening_a + reworded [OxfordModeratorPrompt] to reflect
            // auto-declare. Mirrors vaak-mcp.rs:9780+ exactly per PARITY
            // contract. Phase machine sets OpeningA + side_a[0] auto-
            // declared during the snapshot construction above; this block
            // notifies the team.
            let opener = opener_seat.clone().unwrap_or_default();
            if let Some(speaker) = opener_seat.as_ref() {
                let (soft, hard) = collab::oxford::OxfordPhase::OpeningA
                    .floors(&collab::oxford::OxfordPhaseConfig::default())
                    .unwrap_or((0, 0));
                let phase_evt = serde_json::json!({
                    "id": next_id,
                    "from": "system",
                    "to": "all",
                    "type": "broadcast",
                    "timestamp": collab::iso_now(),
                    "subject": format!(
                        "[OxfordPhaseEntered] debate {} — opening_a opened, speaker={}, soft={}s hard={}s",
                        debate_id, speaker, soft, hard
                    ),
                    "body": format!(
                        "Oxford debate {} entered phase opening_a. Speaker: {} (auto-declared as side_a[0]). Equal-time floors: {}s soft warning, {}s hard auto-yield. Time accounting per-side-total per dev-challenger msg 1353 watch-3 ruling — successive side_a debaters share this budget. SHA-10.3 will add automatic phase advancement; for now this phase persists until moderator calls oxford_advance_phase (SHA-10.3) or oxford_end.",
                        debate_id, speaker, soft, hard
                    ),
                    "metadata": {
                        "debate_id": debate_id,
                        "phase": "opening_a",
                        "speaker": speaker,
                        "soft_secs": soft,
                        "hard_secs": hard,
                        "auto_declared": true,
                        "oxford_event": "phase_entered",
                        "initiated_via": "ui"
                    }
                });
                writeln!(f, "{}", phase_evt.to_string()).map_err(|e| format!("board write: {}", e))?;
                next_id += 1;
            }
            let mod_prompt = serde_json::json!({
                "id": next_id,
                "from": "system",
                "to": moderator.clone(),
                "type": "directive",
                "timestamp": collab::iso_now(),
                "subject": format!("[OxfordModeratorPrompt] debate {} — opening_a auto-declared", debate_id),
                "body": format!(
                    "You are the moderator for Oxford debate {}.\n\nSHA-10.2 phase machine: opening_a phase auto-entered, side_a[0] ({}) is the declared opener. No immediate action required — observe their opening. You may call `oxford_advance_phase` (SHA-10.3) to advance to opening_b when ready, or `oxford_declare_speaker` to override the auto-declared speaker. The phase-advancement sweeper (SHA-10.3) will eventually fire on hard-floor expiry.",
                    debate_id, opener
                ),
                "metadata": {
                    "debate_id": debate_id,
                    "suggested_opener": opener,
                    "phase": "opening_a",
                    "oxford_event": "moderator_prompt",
                    "initiated_via": "ui"
                }
            });
            writeln!(f, "{}", mod_prompt.to_string()).map_err(|e| format!("board write: {}", e))?;
            Ok(())
        });
        Ok(serde_json::json!({
            "debate_id": debate_id,
            "moderator": moderator,
            "side_a": side_a,
            "side_b": side_b,
            "audience": audience,
            "premise": premise,
            "winning_side_reward_copper": reward,
            "started_at": now,
        }))
    })
}

/// Lightweight read-only accessor for the active Oxford debate snapshot.
/// Returns the JSON object if one exists, or null. Used by the CollabTab to
/// poll active state so the End Debate button can show/hide and the human
/// can see the current premise + moderator at-a-glance.
#[tauri::command]
fn read_active_oxford_cmd(dir: String) -> Result<serde_json::Value, String> {
    let dir = validate_project_dir(&dir)?;
    match collab::oxford::read_active_oxford(&dir)? {
        Some(d) => serde_json::to_value(&d).map_err(|e| format!("oxford serialize: {}", e)),
        None => Ok(serde_json::Value::Null),
    }
}

/// Human msg 870 (2026-05-25) — force-end the active Oxford debate from the UI.
/// The MCP handle_oxford_end requires caller == moderator; this Tauri command
/// is human-authority and bypasses that gate (treats every UI-initiated end as
/// outcome="abandoned" — no reward distribution, same shape the MCP path uses
/// when the moderator picks "abandoned"). Closes the human msg 870 UX gap:
/// previously the only end-path required the moderator to call MCP, leaving
/// the human stuck if the moderator was idle or unreachable.
#[tauri::command]
fn oxford_end_cmd(dir: String) -> Result<serde_json::Value, String> {
    use collab::oxford::*;
    let dir = validate_project_dir(&dir)?;

    with_oxford_lock(&dir, || {
        let debate = read_active_oxford(&dir)?
            .ok_or_else(|| "[NoActiveOxfordDebate]".to_string())?;
        let now = collab::iso_now();
        let debate_id = debate.debate_id;

        append_oxford_event(&dir, &OxfordEvent::Ended {
            debate_id,
            timestamp: now.clone(),
            outcome: "abandoned".to_string(),
            audience_tally_nonhuman: None,
            audience_human_vote: None,
            reward_distributed: None,
        })?;
        clear_active_oxford(&dir)?;

        // Board broadcast mirroring the initiate path.
        // SHA-5.1 (human msg 1165 root cause): use active-section path. Same
        // bug as oxford_initiate_cmd above — the [OxfordDebateEnded] broadcast
        // was landing in the legacy root board, invisible to agents on a
        // non-default section.
        let board_path = collab::active_board_path(&dir);
        let _ = collab::with_board_lock(&dir, || -> Result<(), String> {
            use std::io::Write;
            let max_id: u64 = std::fs::read_to_string(&board_path)
                .unwrap_or_default()
                .lines()
                .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
                .filter_map(|v| v.get("id").and_then(|i| i.as_u64()))
                .max()
                .unwrap_or(0);
            let msg = serde_json::json!({
                "id": max_id + 1,
                "from": "system",
                "to": "all",
                "type": "broadcast",
                "timestamp": now.clone(),
                "subject": format!("[OxfordDebateEnded] debate {} (force-abandoned by human:0)", debate_id),
                "body": format!(
                    "Oxford-style debate {} was force-ended by human:0 via the UI. Outcome: abandoned. No reward distribution. Moderator was {}.",
                    debate_id, debate.moderator
                ),
                "metadata": {
                    "debate_id": debate_id,
                    "outcome": "abandoned",
                    "ended_via": "ui_force",
                    "oxford_event": "ended"
                }
            });
            let mut f = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&board_path)
                .map_err(|e| format!("board open: {}", e))?;
            writeln!(f, "{}", serde_json::to_string(&msg).unwrap_or_default())
                .map_err(|e| format!("board write: {}", e))?;
            Ok(())
        });

        Ok(serde_json::json!({
            "debate_id": debate_id,
            "outcome": "abandoned",
            "ended_at": now,
        }))
    })
}

// ============================================================
// Phase D — Delphi Discussion Tauri commands (SHA-D10.2)
// Spec: .vaak/design-notes/2026-05-27-delphi-discussion-spec.md
// PARITY: twins for vaak-mcp.rs::handle_delphi_*. Per spec §5.6, every
// MCP handler MUST have a Tauri command. Caller is hard-wired to human:0
// (Tauri commands are human-driven UI calls).
// ============================================================

// SHA-D10.2 runtime fingerprint — Tauri-twin side.
#[used]
#[no_mangle]
pub static VAAK_FINGERPRINT_MAIN_SHA_D10_2: [u8; 43] =
    *b"VAAK_FP:SHA-D10.2:main.rs:delphi_tauri_twin";

/// Generate per-round unshuffle seed. Mirrors `delphi_make_unshuffle_seed`
/// in vaak-mcp.rs. PARITY: both must produce equivalent outputs given the
/// same (discussion_id, round) — but they DON'T need to match exactly
/// since seed is generated once per round and persisted.
fn delphi_make_unshuffle_seed_ui(discussion_id: u64, round: u32) -> String {
    use sha2::{Digest, Sha256};
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let pid = std::process::id();
    let mut hasher = Sha256::new();
    hasher.update(nanos.to_le_bytes());
    hasher.update(pid.to_le_bytes());
    hasher.update(discussion_id.to_le_bytes());
    hasher.update(round.to_le_bytes());
    hex::encode(hasher.finalize())
}

fn delphi_content_hash_ui(content: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    format!("sha256:{}", hex::encode(hasher.finalize()))
}

/// Append a system message to the active-section's board.jsonl. Caller
/// MUST hold the board lock (via `delphi_atomic_op`'s middle tier).
fn delphi_append_to_board(dir: &str, message: &serde_json::Value) -> Result<u64, String> {
    use std::io::Write;
    let board_path = collab::active_board_path(dir);
    let max_id: u64 = std::fs::read_to_string(&board_path)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter_map(|v| v.get("id").and_then(|i| i.as_u64()))
        .max()
        .unwrap_or(0);
    let new_id = max_id + 1;
    let mut owned = message.clone();
    if let Some(obj) = owned.as_object_mut() {
        obj.insert("id".to_string(), serde_json::json!(new_id));
    }
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&board_path)
        .map_err(|e| format!("delphi board open: {}", e))?;
    writeln!(f, "{}", serde_json::to_string(&owned).unwrap_or_default())
        .map_err(|e| format!("delphi board write: {}", e))?;
    Ok(new_id)
}

/// SHA-D10.2 — `delphi_initiate` Tauri command. Mirrors
/// `handle_delphi_initiate` in vaak-mcp.rs. Caller is `human:0`.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
fn delphi_initiate_cmd(
    dir: String,
    moderator: String,
    participants: Vec<String>,
    topic: String,
    audience: Vec<String>,
    max_rounds: Option<u32>,
    convergence_criterion: Option<String>,
    convergence_reward_copper: Option<i64>,
    submission_soft_floor_secs: Option<u32>,
    submission_hard_floor_secs: Option<u32>,
    review_floor_secs: Option<u32>,
    blind_gate_strict: Option<bool>,
) -> Result<serde_json::Value, String> {
    use collab::delphi::*;
    let dir = validate_project_dir(&dir)?;
    let caller = "human:0".to_string();

    // Build active_seats from sessions.json bindings.
    let active_seats: Vec<String> = (|| -> Vec<String> {
        let sessions_path = std::path::Path::new(&dir).join(".vaak").join("sessions.json");
        let sessions: serde_json::Value = std::fs::read_to_string(&sessions_path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_else(|| serde_json::json!({"bindings": []}));
        let mut out = Vec::new();
        if let Some(bindings) = sessions.get("bindings").and_then(|b| b.as_array()) {
            for b in bindings {
                if b.get("status").and_then(|s| s.as_str()) != Some("active") { continue; }
                let role = b.get("role").and_then(|s| s.as_str()).unwrap_or("");
                if role.is_empty() || role == "human" { continue; }
                let instance = b.get("instance").and_then(|i| i.as_u64()).unwrap_or(0);
                out.push(format!("{}:{}", role, instance));
            }
        }
        out
    })();

    let conv_reward = convergence_reward_copper.unwrap_or(DELPHI_DEFAULT_CONVERGENCE_REWARD_COPPER);
    validate_initiate(&caller, &moderator, &participants, &audience, &active_seats, conv_reward)?;

    // Liveness gate — non-human seats must have fresh heartbeats. PARITY with
    // vaak-mcp.rs:handle_delphi_initiate.
    let project_path = std::path::Path::new(&dir);
    for seat in std::iter::once(&moderator).chain(participants.iter()).chain(audience.iter()) {
        if seat.starts_with("human:") { continue; }
        if !collab::is_seat_alive(project_path, seat) {
            return Err(format!(
                "[DelphiSeatNotAlive] {} heartbeat is stale (> {}s) — cannot initiate Delphi with a non-responsive seat.",
                seat,
                collab::staleness_thresholds::ALIVE_STATE_STALE_MS / 1000
            ));
        }
    }

    let conv_criterion_v: ConvergenceMode = match convergence_criterion.as_deref().unwrap_or("moderator") {
        "moderator" => ConvergenceMode::Moderator,
        "max_rounds" => ConvergenceMode::MaxRounds,
        "hybrid" => ConvergenceMode::Hybrid,
        other => return Err(format!(
            "[DelphiInvalidConvergenceCriterion] '{}' — must be moderator | max_rounds | hybrid",
            other
        )),
    };

    let max_rounds_v = max_rounds.unwrap_or(DELPHI_DEFAULT_MAX_ROUNDS);
    let soft_floor = submission_soft_floor_secs.unwrap_or(DELPHI_SUBMISSION_SOFT_FLOOR_SECS);
    let hard_floor = submission_hard_floor_secs.unwrap_or(DELPHI_SUBMISSION_HARD_FLOOR_SECS);
    let review_floor = review_floor_secs.unwrap_or(DELPHI_REVIEW_FLOOR_SECS);
    let blind_strict = blind_gate_strict.unwrap_or(false);

    collab::delphi::delphi_atomic_op(&dir, || {
        if read_active_delphi(&dir)?.is_some() {
            return Err("[DelphiAlreadyActive]".to_string());
        }
        if collab::oxford::read_active_oxford(&dir)?.is_some() {
            return Err("[OxfordDebateActive] End the active Oxford debate before initiating Delphi (spec §6.8 asymmetry — Oxford has priority).".to_string());
        }
        // Auto-disable continuous-review mode if active.
        let disc_path = std::path::Path::new(&dir).join(".vaak").join("discussion.json");
        if disc_path.exists() {
            let prior_mode = std::fs::read_to_string(&disc_path).ok()
                .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
                .and_then(|v| v.get("mode").and_then(|m| m.as_str()).map(|s| s.to_string()));
            if let Some(prior) = prior_mode {
                if !prior.is_empty() && prior != "null" {
                    let disabled = serde_json::json!({ "mode": serde_json::Value::Null });
                    let _ = std::fs::write(&disc_path, serde_json::to_string_pretty(&disabled).unwrap_or_default());
                }
            }
        }

        let discussion_id = next_discussion_id(&dir);
        let now = collab::iso_now();

        let debate = ActiveDelphiDebate {
            discussion_id,
            moderator: moderator.clone(),
            participants: participants.clone(),
            audience: audience.clone(),
            topic: topic.clone(),
            max_rounds: max_rounds_v,
            convergence_criterion: conv_criterion_v,
            convergence_reward_copper: conv_reward,
            phase: DelphiPhase::Setup,
            current_round: 0,
            phase_started_at: Some(now.clone()),
            blind_gate_active: false,
            blind_gate_strict: blind_strict,
            submission_soft_floor_secs: soft_floor,
            submission_hard_floor_secs: hard_floor,
            review_floor_secs: review_floor,
            started_at: now.clone(),
            rounds: Vec::new(),
        };
        write_active_delphi(&dir, &debate)?;

        append_delphi_event(&dir, &DelphiEvent::Initiate {
            discussion_id,
            timestamp: now.clone(),
            moderator: moderator.clone(),
            participants: participants.clone(),
            audience: audience.clone(),
            topic: topic.clone(),
            max_rounds: max_rounds_v,
            convergence_criterion: conv_criterion_v,
            convergence_reward_copper: conv_reward,
            submission_soft_floor_secs: soft_floor,
            submission_hard_floor_secs: hard_floor,
            review_floor_secs: review_floor,
            blind_gate_strict: blind_strict,
        })?;

        let reward_descr = if conv_reward > 0 {
            format!("{} copper from pool", conv_reward)
        } else {
            "no reward".to_string()
        };
        let _ = delphi_append_to_board(&dir, &serde_json::json!({
            "from": "system", "to": "all", "type": "broadcast",
            "timestamp": now.clone(),
            "subject": format!("[DelphiDiscussionInitiated] discussion {} by {} (UI-initiated)", discussion_id, moderator),
            "body": format!(
                "Delphi discussion {} initiated by {} via the UI.\nTopic: {}\nParticipants: {}\nAudience: {}\nMax rounds: {}\nReward: {}\n\nModerator: call `delphi_open_round` (or use the UI panel) to open round 1.",
                discussion_id, moderator, topic,
                participants.join(", "),
                if audience.is_empty() { "(none)".to_string() } else { audience.join(", ") },
                max_rounds_v, reward_descr
            ),
            "metadata": {
                "discussion_id": discussion_id,
                "moderator": moderator,
                "participants": participants,
                "audience": audience,
                "topic": topic,
                "max_rounds": max_rounds_v,
                "convergence_reward_copper": conv_reward,
                "initiated_via": "ui",
                "delphi_event": "initiate"
            }
        }));
        for seat in participants.iter() {
            let _ = delphi_append_to_board(&dir, &serde_json::json!({
                "from": "system", "to": seat, "type": "directive",
                "timestamp": now.clone(),
                "subject": format!("[DelphiParticipantAssignment] discussion {} — you are a participant", discussion_id),
                "body": format!(
                    "You have been selected as a participant in Delphi discussion {}.\n\nTopic: {}\nModerator: {}\nMax rounds: {}\n\nWhen the moderator opens a round, submit via `delphi_submit(content=\"...\")`. You CANNOT broadcast to the board during submitting phase. After all participants submit, the moderator closes the round and posts the anonymized aggregate.\n\nIMPORTANT: your submission's anonymity ends when the discussion ends — the unshuffle map becomes public for audit.",
                    discussion_id, topic, moderator, max_rounds_v
                ),
                "metadata": {
                    "discussion_id": discussion_id,
                    "assigned_role": "participant",
                    "delphi_event": "participant_assigned"
                }
            }));
        }
        for seat in audience.iter() {
            let _ = delphi_append_to_board(&dir, &serde_json::json!({
                "from": "system", "to": seat, "type": "directive",
                "timestamp": now.clone(),
                "subject": format!("[DelphiAudienceAssignment] discussion {} — you are audience", discussion_id),
                "body": format!(
                    "You are in the audience for Delphi discussion {}.\n\nTopic: {}\nModerator: {}\n\nDuring rounds: see only submission count. At round close: see anonymized aggregate; post questions via `delphi_audience_question` (SHA-D10.5).",
                    discussion_id, topic, moderator
                ),
                "metadata": {
                    "discussion_id": discussion_id,
                    "assigned_role": "audience",
                    "delphi_event": "audience_assigned"
                }
            }));
        }

        Ok(serde_json::json!({
            "discussion_id": discussion_id,
            "moderator": moderator,
            "participants": participants,
            "audience": audience,
            "topic": topic,
            "max_rounds": max_rounds_v,
            "convergence_criterion": conv_criterion_v,
            "convergence_reward_copper": conv_reward,
            "phase": "setup",
            "submission_soft_floor_secs": soft_floor,
            "submission_hard_floor_secs": hard_floor,
            "review_floor_secs": review_floor,
            "blind_gate_strict": blind_strict,
            "started_at": now,
        }))
    })
}

/// SHA-D10.2 — `delphi_get_state` Tauri command. Read-only snapshot for
/// UI polling. Caller is `human:0` so visibility is `moderator_view` (full
/// state including unshuffle_map when requested + ended).
#[tauri::command]
fn delphi_get_state_cmd(
    dir: String,
    include_unshuffle: Option<bool>,
) -> Result<serde_json::Value, String> {
    use collab::delphi::*;
    let dir = validate_project_dir(&dir)?;
    let active = match read_active_delphi(&dir)? {
        Some(a) => a,
        None => return Ok(serde_json::json!({ "active": false })),
    };
    let include_unshuffle = include_unshuffle.unwrap_or(false);
    let ended = active.phase == DelphiPhase::Ended;
    let unshuffle_visible = include_unshuffle && (ended || true); // UI caller is human → full visibility

    let rounds_view: Vec<serde_json::Value> = active.rounds.iter().map(|r| {
        let submissions_view: Vec<serde_json::Value> = r.submissions.iter().map(|s| {
            serde_json::json!({
                "from": s.from,
                "anonymous_id": s.anonymous_id,
                "content": s.content,
                "content_hash": s.content_hash,
                "submitted_at": s.submitted_at,
                "revision_count": s.revision_hash_chain.len(),
            })
        }).collect();
        let unshuffle_map_view = if unshuffle_visible {
            serde_json::to_value(&r.unshuffle_map).unwrap_or(serde_json::json!({}))
        } else {
            serde_json::json!({})
        };
        serde_json::json!({
            "number": r.number,
            "opened_at": r.opened_at,
            "closed_at": r.closed_at,
            "prompt": r.prompt,
            "submissions": submissions_view,
            "submissions_count": r.submissions.len(),
            "unshuffle_map": unshuffle_map_view,
            "aggregate_message_id": r.aggregate_message_id,
            "non_submitters": r.non_submitters,
            "audience_questions": r.audience_questions,
            "unshuffle_seed": if unshuffle_visible {
                serde_json::Value::String(r.unshuffle_seed.clone())
            } else {
                serde_json::Value::Null
            },
        })
    }).collect();

    Ok(serde_json::json!({
        "active": true,
        "caller": "human:0",
        "caller_role": "moderator_view",
        "discussion_id": active.discussion_id,
        "moderator": active.moderator,
        "participants": active.participants,
        "audience": active.audience,
        "topic": active.topic,
        "max_rounds": active.max_rounds,
        "convergence_criterion": active.convergence_criterion,
        "convergence_reward_copper": active.convergence_reward_copper,
        "phase": active.phase,
        "current_round": active.current_round,
        "phase_started_at": active.phase_started_at,
        "blind_gate_active": active.blind_gate_active,
        "blind_gate_strict": active.blind_gate_strict,
        "submission_soft_floor_secs": active.submission_soft_floor_secs,
        "submission_hard_floor_secs": active.submission_hard_floor_secs,
        "review_floor_secs": active.review_floor_secs,
        "started_at": active.started_at,
        "rounds": rounds_view,
    }))
}

/// SHA-D10.2 — `delphi_open_round` Tauri command. Human-driven override
/// of the moderator role-gate (per spec §6.6.1 human-authority bypass).
#[tauri::command]
fn delphi_open_round_cmd(
    dir: String,
    round_prompt_override: Option<String>,
) -> Result<serde_json::Value, String> {
    use collab::delphi::*;
    let dir = validate_project_dir(&dir)?;

    collab::delphi::delphi_atomic_op(&dir, || {
        let mut active = read_active_delphi(&dir)?
            .ok_or_else(|| "[NoActiveDelphi]".to_string())?;
        // Human-authority bypass per §6.6.1 — UI caller doesn't need to be the moderator.
        if !matches!(active.phase, DelphiPhase::Setup | DelphiPhase::Opening | DelphiPhase::Reviewing) {
            return Err(format!(
                "[DelphiCannotOpenFromPhase] current phase is {:?} — open_round requires setup | opening | reviewing",
                active.phase
            ));
        }
        if active.current_round >= active.max_rounds {
            return Err("[DelphiMaxRoundsReached]".to_string());
        }

        let now = collab::iso_now();
        let new_round_number = active.current_round + 1;
        let prompt = round_prompt_override
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| {
                if new_round_number == 1 {
                    active.topic.clone()
                } else {
                    format!("Round {}: refine your position in light of the prior aggregate. Topic: {}", new_round_number, active.topic)
                }
            });
        let unshuffle_seed = delphi_make_unshuffle_seed_ui(active.discussion_id, new_round_number);

        active.current_round = new_round_number;
        active.phase = DelphiPhase::Submitting;
        active.phase_started_at = Some(now.clone());
        active.blind_gate_active = true;
        active.rounds.push(DelphiRound {
            number: new_round_number,
            opened_at: now.clone(),
            closed_at: None,
            prompt: prompt.clone(),
            submissions: Vec::new(),
            unshuffle_map: std::collections::BTreeMap::new(),
            unshuffle_seed: unshuffle_seed.clone(),
            aggregate_message_id: None,
            non_submitters: Vec::new(),
            audience_questions: Vec::new(),
        });
        write_active_delphi(&dir, &active)?;
        append_delphi_event(&dir, &DelphiEvent::RoundOpened {
            discussion_id: active.discussion_id,
            round: new_round_number,
            prompt: prompt.clone(),
            timestamp: now.clone(),
        })?;
        let _ = delphi_append_to_board(&dir, &serde_json::json!({
            "from": "system", "to": "all", "type": "broadcast",
            "timestamp": now.clone(),
            "subject": format!("[DelphiRoundOpened] discussion {} round {} — submissions open (UI-opened)", active.discussion_id, new_round_number),
            "body": format!(
                "Round {} is open on: '{}'.\n\nParticipants ({}): submit via `delphi_submit(content=\"...\")`. Submissions due within {}s soft / {}s hard.",
                new_round_number, prompt,
                active.participants.join(", "),
                active.submission_soft_floor_secs, active.submission_hard_floor_secs,
            ),
            "metadata": {
                "discussion_id": active.discussion_id,
                "round": new_round_number,
                "prompt": prompt,
                "phase": "submitting",
                "opened_via": "ui",
                "delphi_event": "round_opened"
            }
        }));
        for seat in active.participants.iter() {
            let _ = delphi_append_to_board(&dir, &serde_json::json!({
                "from": "system", "to": seat, "type": "directive",
                "timestamp": now.clone(),
                "subject": format!("[DelphiRoundPrompt] discussion {} round {} — submit your position", active.discussion_id, new_round_number),
                "body": format!(
                    "Round {} of Delphi discussion {} is open.\n\nPrompt: {}\n\nCall `delphi_submit(content=\"<your-position>\")`. {}s soft / {}s hard.",
                    new_round_number, active.discussion_id, prompt,
                    active.submission_soft_floor_secs, active.submission_hard_floor_secs
                ),
                "metadata": {
                    "discussion_id": active.discussion_id,
                    "round": new_round_number,
                    "delphi_event": "round_prompt"
                }
            }));
        }

        Ok(serde_json::json!({
            "discussion_id": active.discussion_id,
            "round": new_round_number,
            "prompt": prompt,
            "phase": "submitting",
            "unshuffle_seed": unshuffle_seed,
            "opened_at": now,
        }))
    })
}

/// SHA-D10.2 — `delphi_submit` Tauri command. Caller is `human:0` per spec
/// §6.6.1 — human may always submit (override blind gate for human).
#[tauri::command]
fn delphi_submit_cmd(
    dir: String,
    content: String,
) -> Result<serde_json::Value, String> {
    use collab::delphi::*;
    let dir = validate_project_dir(&dir)?;
    let caller = "human:0".to_string();

    if content.trim().is_empty() {
        return Err("[DelphiSubmitEmpty] content must be non-empty".to_string());
    }

    collab::delphi::delphi_atomic_op(&dir, || {
        let mut active = read_active_delphi(&dir)?
            .ok_or_else(|| "[NoActiveDelphi]".to_string())?;
        // Human-authority: allow even if caller not in participants (spec §6.6.1).
        let is_human = caller.starts_with("human:");
        if !is_human && !active.participants.iter().any(|p| p == &caller) {
            return Err("[DelphiNonParticipantCannotSubmit]".to_string());
        }
        if active.phase != DelphiPhase::Submitting {
            return Err(format!(
                "[DelphiCannotSubmitInPhase] current phase is {:?}",
                active.phase
            ));
        }

        let now = collab::iso_now();
        let content_hash = delphi_content_hash_ui(&content);
        let current_round_idx = active.rounds.len().saturating_sub(1);
        let round = active.rounds.get_mut(current_round_idx)
            .ok_or_else(|| "[DelphiInternalState] round index OOB".to_string())?;
        let existing_idx = round.submissions.iter().position(|s| s.from == caller);
        let revision_number = match existing_idx {
            Some(idx) => {
                let prior = round.submissions[idx].clone();
                let mut chain = prior.revision_hash_chain.clone();
                chain.push(prior.content_hash.clone());
                round.submissions[idx] = DelphiSubmission {
                    from: caller.clone(),
                    anonymous_id: None,
                    content: content.clone(),
                    content_hash: content_hash.clone(),
                    revision_hash_chain: chain.clone(),
                    submitted_at: now.clone(),
                };
                (chain.len() as u32) + 1
            }
            None => {
                round.submissions.push(DelphiSubmission {
                    from: caller.clone(),
                    anonymous_id: None,
                    content: content.clone(),
                    content_hash: content_hash.clone(),
                    revision_hash_chain: Vec::new(),
                    submitted_at: now.clone(),
                });
                1
            }
        };
        let total_submitted = round.submissions.len();
        let round_number = round.number;
        let total_participants = active.participants.len();
        let moderator = active.moderator.clone();
        let discussion_id = active.discussion_id;
        write_active_delphi(&dir, &active)?;
        append_delphi_event(&dir, &DelphiEvent::Submission {
            discussion_id,
            round: round_number,
            seat: caller.clone(),
            content_hash: content_hash.clone(),
            revision_number,
            timestamp: now.clone(),
        })?;
        let _ = delphi_append_to_board(&dir, &serde_json::json!({
            "from": "system", "to": moderator,
            "type": "status",
            "timestamp": now.clone(),
            "subject": format!("[DelphiSubmissionReceived] discussion {} round {} — {}/{}", discussion_id, round_number, total_submitted, total_participants),
            "body": format!(
                "Submission received from {} (revision {}) for discussion {} round {}. Total: {} of {}.",
                caller, revision_number, discussion_id, round_number, total_submitted, total_participants
            ),
            "metadata": {
                "discussion_id": discussion_id,
                "round": round_number,
                "submitted_seat": caller,
                "revision_number": revision_number,
                "submitted_count": total_submitted,
                "total_participants": total_participants,
                "delphi_event": "submission_received"
            }
        }));

        Ok(serde_json::json!({
            "discussion_id": discussion_id,
            "round": round_number,
            "from": caller,
            "content_hash": content_hash,
            "revision_number": revision_number,
            "submitted_count": total_submitted,
            "total_participants": total_participants,
            "submitted_at": now,
        }))
    })
}

/// SHA-D10.2 — `delphi_close_round` Tauri command. Human-authority bypass
/// of moderator gate per §6.6.1. SHA-D10.3 will add Fisher-Yates aggregate.
#[tauri::command]
fn delphi_close_round_cmd(dir: String) -> Result<serde_json::Value, String> {
    use collab::delphi::*;
    let dir = validate_project_dir(&dir)?;

    collab::delphi::delphi_atomic_op(&dir, || {
        let mut active = read_active_delphi(&dir)?
            .ok_or_else(|| "[NoActiveDelphi]".to_string())?;
        if active.phase != DelphiPhase::Submitting {
            return Err(format!(
                "[DelphiCannotCloseFromPhase] current phase is {:?}",
                active.phase
            ));
        }

        let now = collab::iso_now();
        let current_round_idx = active.rounds.len().saturating_sub(1);
        // SHA-D10.3 — build Fisher-Yates anonymized aggregate.
        let (round_number, submissions_count, non_submitters, unshuffle_seed, aggregate_markdown, unshuffle_map): (u32, usize, Vec<String>, String, String, std::collections::BTreeMap<String, String>) = {
            let round = active.rounds.get(current_round_idx)
                .ok_or_else(|| "[DelphiInternalState] round index OOB".to_string())?;
            let submitted_set: std::collections::HashSet<&String> =
                round.submissions.iter().map(|s| &s.from).collect();
            let non_subs: Vec<String> = active.participants.iter()
                .filter(|p| !submitted_set.contains(p))
                .cloned()
                .collect();
            let (md, map) = collab::delphi::build_aggregate(
                &active.topic,
                round.number,
                &round.submissions,
                &round.unshuffle_seed,
                active.participants.len(),
            );
            (round.number, round.submissions.len(), non_subs, round.unshuffle_seed.clone(), md, map)
        };
        let aggregate_msg_id = {
            let board_path = collab::active_board_path(&dir);
            std::fs::read_to_string(&board_path)
                .unwrap_or_default()
                .lines()
                .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
                .filter_map(|v| v.get("id").and_then(|i| i.as_u64()))
                .max()
                .unwrap_or(0) + 1
        };
        {
            let round = active.rounds.get_mut(current_round_idx)
                .ok_or_else(|| "[DelphiInternalState] round idx OOB (mut)".to_string())?;
            round.closed_at = Some(now.clone());
            round.non_submitters = non_submitters.clone();
            round.aggregate_message_id = Some(aggregate_msg_id);
            round.unshuffle_map = unshuffle_map.clone();
            // Stamp anonymous_id on each submission for get_state consistency.
            let mut label_for_seat: std::collections::HashMap<String, String> =
                std::collections::HashMap::new();
            for (label, seat) in &unshuffle_map {
                label_for_seat.insert(seat.clone(), label.clone());
            }
            for sub in round.submissions.iter_mut() {
                if let Some(label) = label_for_seat.get(&sub.from) {
                    sub.anonymous_id = Some(label.clone());
                }
            }
        }
        active.phase = DelphiPhase::Reviewing;
        active.phase_started_at = Some(now.clone());
        active.blind_gate_active = false;
        write_active_delphi(&dir, &active)?;
        append_delphi_event(&dir, &DelphiEvent::RoundClosed {
            discussion_id: active.discussion_id,
            round: round_number,
            aggregate_message_id: aggregate_msg_id,
            submissions_count: submissions_count as u32,
            non_submitters: non_submitters.clone(),
            unshuffle_seed: unshuffle_seed.clone(),
            timestamp: now.clone(),
        })?;
        let non_subs_line = if non_submitters.is_empty() {
            String::from("(none)")
        } else {
            non_submitters.join(", ")
        };
        let _ = delphi_append_to_board(&dir, &serde_json::json!({
            "from": "system", "to": "all", "type": "broadcast",
            "timestamp": now.clone(),
            "subject": format!("[DelphiRoundClosed] discussion {} round {} — anonymized aggregate ({} submissions, UI-closed)", active.discussion_id, round_number, submissions_count),
            "body": format!(
                "{}\n\n---\n\nNon-submitters: {}.\n\nRound {} closed by human:0 via UI. Phase advanced to `reviewing`. Unshuffle map remains moderator-visible until the discussion ends (then public per spec §5).",
                aggregate_markdown,
                non_subs_line,
                round_number,
            ),
            "metadata": {
                "discussion_id": active.discussion_id,
                "round": round_number,
                "submissions_count": submissions_count,
                "non_submitters": non_submitters,
                "phase": "reviewing",
                "closed_via": "ui",
                "delphi_event": "round_closed"
            }
        }));

        Ok(serde_json::json!({
            "discussion_id": active.discussion_id,
            "round": round_number,
            "submissions_count": submissions_count,
            "non_submitters": non_submitters,
            "phase": "reviewing",
            "aggregate_message_id": aggregate_msg_id,
            "closed_at": now,
        }))
    })
}

/// SHA-D10.5a — `delphi_end` Tauri command. Caller is human:0 per UI
/// convention; human-authority bypass per §6.6.1 covers the moderator
/// role gate. Convergence reward distribution from delphi_pool queued
/// for SHA-D10.5 proper.
#[tauri::command]
fn delphi_end_cmd(dir: String, outcome: String) -> Result<serde_json::Value, String> {
    use collab::delphi::*;
    let dir = validate_project_dir(&dir)?;

    let outcome_v: DelphiOutcome = match outcome.as_str() {
        "converged" => DelphiOutcome::Converged,
        "max_rounds_reached" => DelphiOutcome::MaxRoundsReached,
        "abandoned" => DelphiOutcome::Abandoned,
        "aborted_quorum_loss" => DelphiOutcome::AbortedQuorumLoss,
        "human_override" => DelphiOutcome::HumanOverride,
        "oxford_preemption" => DelphiOutcome::OxfordPreemption,
        other => return Err(format!(
            "[DelphiInvalidOutcome] '{}' — must be converged | max_rounds_reached | abandoned | aborted_quorum_loss | human_override | oxford_preemption",
            other
        )),
    };

    collab::delphi::delphi_atomic_op(&dir, || {
        let mut active = read_active_delphi(&dir)?
            .ok_or_else(|| "[NoActiveDelphi]".to_string())?;
        let now = collab::iso_now();
        let discussion_id = active.discussion_id;
        let rounds_completed = active.rounds.iter()
            .filter(|r| r.closed_at.is_some())
            .count() as u32;
        let reward_distributed = 0i64;
        let reward_recipients: Vec<String> = Vec::new();

        active.phase = DelphiPhase::Ended;
        active.phase_started_at = Some(now.clone());
        active.blind_gate_active = false;
        write_active_delphi(&dir, &active)?;

        append_delphi_event(&dir, &DelphiEvent::Ended {
            discussion_id,
            outcome: outcome_v,
            rounds_completed,
            convergence_reward_distributed_copper: reward_distributed,
            reward_recipients: reward_recipients.clone(),
            timestamp: now.clone(),
        })?;

        archive_active_delphi(&dir, discussion_id)?;

        let reward_line = if active.convergence_reward_copper > 0 {
            format!("Convergence reward distribution queued for SHA-D10.5; none paid in this commit. Configured: {} copper from pool.", active.convergence_reward_copper)
        } else {
            String::from("No convergence reward configured (zero default).")
        };
        let _ = delphi_append_to_board(&dir, &serde_json::json!({
            "from": "system", "to": "all", "type": "broadcast",
            "timestamp": now.clone(),
            "subject": format!("[DelphiDiscussionEnded] discussion {} — outcome={:?} (UI-ended)", discussion_id, outcome_v),
            "body": format!(
                "Delphi discussion {} ended by human:0 via UI.\nOutcome: {:?}\nRounds completed: {}\n{}\n\nUnshuffle map is now public via archive at `.vaak/delphi-completed/{}.json` per spec §5.",
                discussion_id, outcome_v, rounds_completed, reward_line, discussion_id,
            ),
            "metadata": {
                "discussion_id": discussion_id,
                "outcome": outcome,
                "rounds_completed": rounds_completed,
                "convergence_reward_distributed_copper": reward_distributed,
                "reward_recipients": reward_recipients,
                "ended_via": "ui",
                "delphi_event": "ended"
            }
        }));

        Ok(serde_json::json!({
            "discussion_id": discussion_id,
            "outcome": outcome,
            "rounds_completed": rounds_completed,
            "convergence_reward_distributed_copper": reward_distributed,
            "reward_recipients": reward_recipients,
            "ended_at": now,
            "archived_to": format!(".vaak/delphi-completed/{}.json", discussion_id),
        }))
    })
}

/// Human msg 657 (2026-05-24) — read economy settings as JSON for the UI.
/// Returns the current EconomySettings (file values + defaults for missing
/// fields), enabling the Settings page to populate inputs with the live
/// values. Read-only; no audit.
#[tauri::command]
fn read_economy_settings_cmd(dir: String) -> Result<serde_json::Value, String> {
    let dir = validate_project_dir(&dir)?;
    let settings = collab::currency::read_economy_settings(&dir);
    serde_json::to_value(&settings).map_err(|e| format!("economy serialize: {}", e))
}

/// Human msg 657 — write economy settings to .vaak/economy.json + emit an
/// audit ledger row per evil-arch msg 661 + tester msg 663 #2.
///
/// Inputs: the full settings JSON object (UI builds from read_economy_settings_cmd
/// + applies user edits). Caller is hard-wired to human:0 (only the Vaak UI
/// can call this; the MCP-side equivalent would require a separate sidecar
/// tool that this commit does not ship).
///
/// On every write:
///   1. Reads current settings (the previous values) for the diff.
///   2. Parses the incoming JSON into EconomySettings (rejects malformed
///      payloads with [EconomyTuneInvalidPayload]).
///   3. Atomic-writes the new settings to .vaak/economy.json.
///   4. Emits ONE ledger audit row per field that changed, with
///      txn_type:"economy_tune", action_kind:EconomyTune, amount=0, and
///      reason="<field>: <old> → <new>". Fields that didn't change emit
///      no row (keeps the ledger from bloating on no-op writes).
///   5. Returns {fields_changed: N, settings: <new>}.
#[tauri::command]
fn write_economy_settings_cmd(
    dir: String,
    settings: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let dir = validate_project_dir(&dir)?;
    let new_settings: collab::currency::EconomySettings = serde_json::from_value(settings)
        .map_err(|e| format!("[EconomyTuneInvalidPayload] {}", e))?;
    // Semantic validation per architect msg 701 + evil-arch msg 691 + tester
    // msg 693 consolidated list. Reject malformed-semantics settings before
    // writing so the UI surfaces inline error + no audit row pollution + no
    // economy.json mutation. Defense-in-depth: UI also pre-validates (cheap
    // user feedback) + this is the security gate (canonical).
    let s = &new_settings;
    let mut violations: Vec<String> = Vec::new();
    if s.interest_per_10_copper_held < 0 {
        violations.push("interest_per_10_copper_held must be >= 0".to_string());
    }
    if s.starting_balance_copper <= 0 {
        violations.push(format!(
            "starting_balance_copper ({}) must be > 0 (per evil-arch msg 707 + tester msg 709)",
            s.starting_balance_copper
        ));
    }
    if s.pass_escrow_ticks == 0 || s.speak_escrow_ticks == 0 || s.edit_escrow_ticks == 0 || s.test_escrow_ticks == 0 {
        violations.push(format!(
            "all escrow_ticks_* must be > 0 (pass={}, speak={}, edit={}, test={}) — zero ticks = immediate release breaks the time-lock contract",
            s.pass_escrow_ticks, s.speak_escrow_ticks, s.edit_escrow_ticks, s.test_escrow_ticks
        ));
    }
    // Oxford-debate constants (commit 2d)
    if s.oxford_default_winning_reward_copper < 0 {
        violations.push(format!(
            "oxford_default_winning_reward_copper ({}) must be >= 0 (0 = no-reward debate)",
            s.oxford_default_winning_reward_copper
        ));
    }
    if s.oxford_turn_hard_limit_secs < s.oxford_turn_soft_limit_secs {
        violations.push(format!(
            "oxford_turn_hard_limit_secs ({}) must be >= oxford_turn_soft_limit_secs ({})",
            s.oxford_turn_hard_limit_secs, s.oxford_turn_soft_limit_secs
        ));
    }
    if s.oxford_audience_vote_window_secs == 0 {
        violations.push("oxford_audience_vote_window_secs must be > 0".to_string());
    }
    if s.decay_floor_copper > s.starting_balance_copper {
        violations.push(format!(
            "decay_floor_copper ({}) must be <= starting_balance_copper ({})",
            s.decay_floor_copper, s.starting_balance_copper
        ));
    }
    if s.starting_balance_copper > 0 && s.objection_cost_copper > s.starting_balance_copper / 5 {
        violations.push(format!(
            "objection_cost_copper ({}) must be <= starting_balance_copper/5 ({})",
            s.objection_cost_copper, s.starting_balance_copper / 5
        ));
    }
    if s.deficit_cap_copper > 0 {
        violations.push(format!(
            "deficit_cap_copper ({}) must be <= 0 (negative threshold for timeout)",
            s.deficit_cap_copper
        ));
    }
    if s.bounty_claim_stake_percent > 100 {
        violations.push(format!(
            "bounty_claim_stake_percent ({}) must be in [0, 100]",
            s.bounty_claim_stake_percent
        ));
    }
    if s.bounty_abandon_loss_percent > 100 {
        violations.push(format!(
            "bounty_abandon_loss_percent ({}) must be in [0, 100]",
            s.bounty_abandon_loss_percent
        ));
    }
    if s.bounty_reject_loss_percent > 100 {
        violations.push(format!(
            "bounty_reject_loss_percent ({}) must be in [0, 100]",
            s.bounty_reject_loss_percent
        ));
    }
    if s.bounty_objection_clawback_percent > 100 {
        violations.push(format!(
            "bounty_objection_clawback_percent ({}) must be in [0, 100]",
            s.bounty_objection_clawback_percent
        ));
    }
    if s.clawback_percent > 100 {
        violations.push(format!(
            "clawback_percent ({}) must be in [0, 100]",
            s.clawback_percent
        ));
    }
    if !violations.is_empty() {
        return Err(format!("[EconomyTuneInvalidValue] {}", violations.join("; ")));
    }
    let old_settings = collab::currency::read_economy_settings(&dir);

    // Diff per field — compare via serde Value to enumerate every field generically.
    let old_v = serde_json::to_value(&old_settings).map_err(|e| format!("economy serialize old: {}", e))?;
    let new_v = serde_json::to_value(&new_settings).map_err(|e| format!("economy serialize new: {}", e))?;
    let mut changes: Vec<(String, serde_json::Value, serde_json::Value)> = Vec::new();
    if let (Some(o), Some(n)) = (old_v.as_object(), new_v.as_object()) {
        for (k, nv) in n {
            let ov = o.get(k).cloned().unwrap_or(serde_json::Value::Null);
            if &ov != nv {
                changes.push((k.clone(), ov, nv.clone()));
            }
        }
    }

    // Persist under currency lock + emit audit rows atomically.
    collab::with_currency_lock(&dir, || {
        collab::currency::write_economy_settings(&dir, &new_settings)?;
        if !changes.is_empty() {
            let mut snap = collab::currency::read_balances_snapshot(&dir)?;
            if snap.seats.is_empty() && collab::currency::currency_jsonl_path(&dir).exists() {
                snap = collab::currency::replay_balances_from_ledger(&dir)?;
            }
            let now = collab::iso_now();
            for (field, old_value, new_value) in &changes {
                let id = snap.next_txn_id;
                snap.next_txn_id += 1;
                let row = collab::currency::LedgerRow {
                    id,
                    txn_type: "economy_tune".to_string(),
                    // System-level audit: not attributed to a specific seat
                    // because the change affects every seat. The UI caller
                    // (always human:0) is implicit.
                    seat: "human:0".to_string(),
                    amount: 0,
                    reason: format!("economy_tune by human:0: {} = {} → {}", field, old_value, new_value),
                    ref_msg: None,
                    balance_after: 0,
                    escrow_id: None,
                    release_turn: None,
                    turn: Some(snap.turn_counter),
                    action_kind: Some(collab::currency::ActionKind::EconomyTune),
                    linked_edit_msg: None,
                    at: now.clone(),
                };
                collab::currency::append_currency_transaction(&dir, &row)?;
            }
            collab::currency::write_balances_snapshot(&dir, &snap)?;
        }
        Ok(())
    })?;

    Ok(serde_json::json!({
        "fields_changed": changes.len(),
        "settings": new_v,
        "changes": changes.iter().map(|(f, o, n)| serde_json::json!({"field": f, "from": o, "to": n})).collect::<Vec<_>>(),
    }))
}

/// Human msg 458 (2026-05-24) — Tauri-path human balance adjust. The UI calls
/// this command directly from within the Vaak webview (no MCP roundtrip). The
/// `caller` is always hard-wired to "human:0" because only the human is the
/// user of the Vaak UI; the sidecar's MCP variant (currency_human_adjust)
/// resolves the caller from the agent session and is therefore the path for
/// any future "make this also work for moderator role" relaxation. Both
/// paths share `collab::currency::apply_human_adjust` for the lock + ledger
/// + snapshot mutation; this command does NOT emit a board system message
/// (the UI already shows the change via balance polling).
#[tauri::command]
fn currency_human_adjust_cmd(
    dir: String,
    seat: String,
    amount_copper: i64,
    reason: String,
) -> Result<serde_json::Value, String> {
    let dir = validate_project_dir(&dir)?;
    collab::currency::apply_human_adjust(&dir, "human:0", &seat, amount_copper, &reason)
}

/// Phase B v1 B1c support (ui-arch msg 795 + dev:1 msg 806). Returns currency
/// ledger rows newer than a caller-provided txn_id cursor. The Visualization
/// tab polls this every 1-2s, renders new rows as floating popups near the
/// affected seat (e.g., "+10c speak" floats up from architect:0's avatar).
///
/// Implementation: read currency.jsonl, filter rows with id > since_txn_id,
/// return them as JSON. Cheap O(N) tail scan; current ledger is ~5K rows so
/// well within polling budget. If hot, future optimization is a tail index
/// (track last-N rows in memory) — but for v1, raw scan is fine.
///
/// Cap output at 200 rows to prevent UI-flood after long-disconnect
/// catch-up. Frontend may make multiple calls advancing the cursor to
/// fully sync.
#[tauri::command]
fn read_currency_events_stream(
    dir: String,
    since_txn_id: u64,
) -> Result<serde_json::Value, String> {
    let dir = validate_project_dir(&dir)?;
    let path = collab::currency::currency_jsonl_path(&dir);
    let content = std::fs::read_to_string(&path).unwrap_or_default();
    let mut rows: Vec<serde_json::Value> = Vec::new();
    let mut last_id: u64 = since_txn_id;
    for line in content.lines() {
        if line.trim().is_empty() { continue; }
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
            let id = v.get("id").and_then(|i| i.as_u64()).unwrap_or(0);
            if id > since_txn_id {
                if id > last_id { last_id = id; }
                rows.push(v);
                if rows.len() >= 200 { break; }
            }
        }
    }
    Ok(serde_json::json!({
        "rows": rows,
        "last_txn_id": last_id,
        "count": rows.len(),
        "more": rows.len() == 200, // signal UI to call again for additional rows
    }))
}

/// Phase 5 (Chitragupta) Surface 1 — the Flow Feed. Returns the last `count`
/// rows of currency.jsonl as raw JSON (newest at the end), plus the total row
/// count. Read-only; never writes. Human-readable formatting happens in the
/// frontend per the Phase 5 directive (backend stays a dumb reader).
#[tauri::command]
fn read_currency_feed_cmd(dir: String, count: usize) -> Result<serde_json::Value, String> {
    let dir = validate_project_dir(&dir)?;
    let path = collab::currency::currency_jsonl_path(&dir);
    let content = std::fs::read_to_string(&path).unwrap_or_default();
    let lines: Vec<&str> = content.lines().filter(|l| !l.trim().is_empty()).collect();
    let total = lines.len();
    let n = count.min(total);
    let rows: Vec<serde_json::Value> = lines[total - n..]
        .iter()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .collect();
    Ok(serde_json::json!({ "rows": rows, "total": total }))
}

/// Phase 5 (Chitragupta) Surface 2 — the Judge Seat. Returns every dispute row
/// from disputes.jsonl (append-only history; a dispute id can appear multiple
/// times as its state advances — frontend takes the latest per id) plus the
/// open_disputes.json snapshot so the UI knows which disputes are still open.
/// Read-only. The challenged-message text + dispute-message bodies come from
/// the board the frontend already watches; this command supplies the dispute
/// records + economics.
#[tauri::command]
fn read_disputes_cmd(dir: String) -> Result<serde_json::Value, String> {
    let dir = validate_project_dir(&dir)?;
    let dpath = collab::currency::disputes_jsonl_path(&dir);
    let dcontent = std::fs::read_to_string(&dpath).unwrap_or_default();
    let disputes: Vec<serde_json::Value> = dcontent
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .collect();
    let opath = collab::currency::open_disputes_json_path(&dir);
    let open: serde_json::Value = std::fs::read_to_string(&opath)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| serde_json::json!({ "open_by_target": {}, "open_by_challenger": {} }));
    Ok(serde_json::json!({ "disputes": disputes, "open": open }))
}

/// Phase 6 (c) — the Bounty Board. Returns every bounties.jsonl row (append-only
/// history; frontend collapses to latest-per-id) + the open_bounties.json
/// snapshot. Read-only; never writes.
#[tauri::command]
fn read_bounties_cmd(dir: String) -> Result<serde_json::Value, String> {
    let dir = validate_project_dir(&dir)?;
    let bpath = collab::currency::bounties_jsonl_path(&dir);
    let bcontent = std::fs::read_to_string(&bpath).unwrap_or_default();
    let bounties: Vec<serde_json::Value> = bcontent
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .collect();
    let opath = collab::currency::open_bounties_json_path(&dir);
    let open: serde_json::Value = std::fs::read_to_string(&opath)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| serde_json::json!({ "next_bounty_id": 0, "bounties": {} }));
    Ok(serde_json::json!({ "bounties": bounties, "open": open }))
}

/// Phase 7 (b) — read all end-of-session snapshots from .vaak/currency-history/
/// for the lifetime Scoreboard. Files are zero-padded (`<date>-NNN.json`) so a
/// filename sort is chronological. Returns the parsed snapshots in order.
/// Aggregation happens in the frontend (per directive). Read-only.
#[tauri::command]
fn read_currency_history_cmd(dir: String) -> Result<serde_json::Value, String> {
    let dir = validate_project_dir(&dir)?;
    let hist_dir = std::path::Path::new(&dir).join(".vaak").join("currency-history");
    let mut snapshots: Vec<serde_json::Value> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&hist_dir) {
        let mut files: Vec<std::path::PathBuf> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("json"))
            .collect();
        files.sort();
        for p in files {
            if let Ok(content) = std::fs::read_to_string(&p) {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&content) {
                    snapshots.push(v);
                }
            }
        }
    }
    Ok(serde_json::json!({ "snapshots": snapshots }))
}

/// Slice 9 health pill — read 4-layer resilience status.
/// Spec §12.4. Returns JSON with per-layer health + a roll-up status.
///
/// Layer 1 (process wrapper): per-seat sessions/<role>-<inst>.json has
/// recent last_alive_at_ms. We approximate by checking if any seat's
/// .json file has been touched in the last 2× SUPERVISE_HANG_THRESHOLD
/// (so we don't false-alarm during normal idle gaps).
///
/// Layer 2 (supervisor): .vaak/supervisor.pid exists AND the recorded
/// PID is alive.
///
/// Layer 3 (hooks): ~/.claude/settings.json contains a hooks block
/// referencing vaak-keep-alive (or equivalent).
///
/// Layer 4 (visual): always reported as installed if the panel is rendering.
#[tauri::command]
fn get_resilience_status(dir: String) -> Result<serde_json::Value, String> {
    let dir = validate_project_dir(&dir)?;
    let vaak_dir = std::path::Path::new(&dir).join(".vaak");

    // Layer 1: any seat session file with last_alive_at_ms within 180s.
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let layer1_threshold_ms: u64 = 180_000; // 2× supervisor's 90s
    let mut layer1_ok = false;
    let mut layer1_seats_alive = 0u32;
    let mut layer1_seats_total = 0u32;
    if let Ok(entries) = std::fs::read_dir(vaak_dir.join("sessions")) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("json") { continue; }
            if !path.is_file() { continue; }
            layer1_seats_total += 1;
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&content) {
                    let last_alive = v.get("last_alive_at_ms").and_then(|x| x.as_u64()).unwrap_or(0);
                    if last_alive > 0 && now_ms.saturating_sub(last_alive) < layer1_threshold_ms {
                        layer1_seats_alive += 1;
                        layer1_ok = true;
                    }
                }
            }
        }
    }

    // Layer 2: supervisor.pid exists + recorded PID alive.
    let supervisor_pid_path = vaak_dir.join("supervisor.pid");
    let layer2_ok = std::fs::read_to_string(&supervisor_pid_path)
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok())
        .map(|pid| {
            #[cfg(windows)]
            {
                use std::process::Command;
                Command::new("tasklist")
                    .args(["/FI", &format!("PID eq {}", pid), "/FO", "CSV", "/NH"])
                    .output()
                    .map(|o| {
                        let stdout = String::from_utf8_lossy(&o.stdout);
                        stdout.contains(&format!("\"{}\"", pid))
                    })
                    .unwrap_or(false)
            }
            #[cfg(not(windows))]
            {
                // Unix: kill -0 PID returns 0 if alive.
                use std::process::Command;
                Command::new("kill")
                    .args(["-0", &pid.to_string()])
                    .status()
                    .map(|s| s.success())
                    .unwrap_or(false)
            }
        })
        .unwrap_or(false);

    // Layer 3: ~/.claude/settings.json has hooks block referencing vaak.
    let layer3_ok = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .and_then(|h| {
            let settings_path = std::path::PathBuf::from(h).join(".claude").join("settings.json");
            std::fs::read_to_string(&settings_path).ok()
        })
        .map(|s| s.contains("vaak-keep-alive") || s.contains("vaak-mcp"))
        .unwrap_or(false);

    // Roll-up: count pillars healthy. 4/4 OK → green. 3/4 → warn. ≤2 → bad.
    let pillars_ok = (layer1_ok as u32) + (layer2_ok as u32) + (layer3_ok as u32) + 1; // L4 always 1
    let roll_up = match pillars_ok {
        4 => "green",
        3 => "warn",
        _ => "bad",
    };

    Ok(serde_json::json!({
        "roll_up": roll_up,
        "pillars_ok": pillars_ok,
        "layer1": {
            "ok": layer1_ok,
            "label": "Agents responding",
            "detail": format!("{} of {} agents have heartbeated recently", layer1_seats_alive, layer1_seats_total)
        },
        "layer2": {
            "ok": layer2_ok,
            "label": "Auto-recovery watchdog",
            "detail": if layer2_ok { "Running — will restart hung agents automatically".to_string() } else { "Not running — restart vaak to enable, or expand this layer for the manual command".to_string() }
        },
        "layer3": {
            "ok": layer3_ok,
            "label": "Activity heartbeats",
            "detail": if layer3_ok { "Installed — every agent action keeps the seat alive".to_string() } else { "Not installed — agents only refresh on MCP calls. Run `vaak-mcp.exe --install-hooks` once per machine.".to_string() }
        },
        "layer4": {
            "ok": true,
            "label": "Visual indicators",
            "detail": "This panel is rendering"
        }
    }))
}

/// Minimal ISO-8601 → epoch-seconds parser local to main.rs (mirrors
/// vaak-mcp.rs's same helper). Used by Slice 5 phase pause/resume
/// duration math.
fn parse_iso_to_epoch_secs_main(ts: &str) -> Option<u64> {
    let no_tz: &str = if let Some(idx) = ts.find('Z') {
        &ts[..idx]
    } else if let Some(idx) = ts.rfind(|c| c == '+' || c == '-').filter(|&i| i > 10) {
        &ts[..idx]
    } else { ts };
    let main = no_tz.split('.').next()?;
    let (date, time) = main.split_once('T')?;
    let mut date_parts = date.split('-');
    let y: i64 = date_parts.next()?.parse().ok()?;
    let mo: i64 = date_parts.next()?.parse().ok()?;
    let d: i64 = date_parts.next()?.parse().ok()?;
    let mut time_parts = time.split(':');
    let h: i64 = time_parts.next()?.parse().ok()?;
    let mi: i64 = time_parts.next()?.parse().ok()?;
    let s: i64 = time_parts.next()?.parse().ok()?;
    let y = if mo <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if mo > 2 { mo - 3 } else { mo + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    let secs = days * 86400 + h * 3600 + mi * 60 + s;
    if secs < 0 { None } else { Some(secs as u64) }
}

/// Two-controls v1 helpers (commit A.5 — mirror of vaak-mcp.rs's helpers
/// for the desktop UI's protocol_mutate_cmd path). Same semantics, same
/// validation. Suffixed `_main` to avoid collision if a future shared module
/// re-exports these.
fn sha256_hex_main(bytes: &[u8]) -> String {
    use sha2::{Sha256, Digest};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

fn caller_role_main(actor: &str) -> &str {
    actor.splitn(2, ':').next().unwrap_or(actor)
}

fn parse_scope_block_main(plan_content: &str) -> Option<Vec<String>> {
    let needle_open = "<!-- scope:";
    let pos = plan_content.find(needle_open)?;
    let after = &plan_content[pos + needle_open.len()..];
    let close = after.find("-->")?;
    let body = after[..close].trim();
    if body == "*" || body.is_empty() {
        Some(vec![])
    } else {
        Some(body.split_whitespace().map(String::from).collect())
    }
}

fn validate_plan_path_main(project_dir: &str, plan_path: &str) -> Result<std::path::PathBuf, String> {
    if !plan_path.ends_with(".md") {
        return Err(format!("[PlanPathNotMarkdown] plan_path must end in .md: {}", plan_path));
    }
    if plan_path.split(['/', '\\']).any(|seg| seg == "..") {
        return Err(format!("[PlanPathOutsideDesignNotes] plan_path contains '..' segments: {}", plan_path));
    }
    if std::path::Path::new(plan_path).is_absolute() {
        return Err(format!("[PlanPathOutsideDesignNotes] plan_path must be repo-relative under .vaak/design-notes/, not absolute: {}", plan_path));
    }
    let normalized = plan_path.replace('\\', "/");
    let starts_under_design = normalized.starts_with(".vaak/design-notes/");
    let has_separator = normalized.contains('/');
    if has_separator && !starts_under_design {
        return Err(format!("[PlanPathOutsideDesignNotes] plan_path must resolve under .vaak/design-notes/: {}", plan_path));
    }
    let base = std::path::Path::new(project_dir).join(".vaak").join("design-notes");
    let candidate = if starts_under_design {
        std::path::Path::new(project_dir).join(plan_path)
    } else {
        base.join(plan_path)
    };
    let canon_cand = candidate.canonicalize()
        .map_err(|_| format!("[PlanPathMissing] plan_path file does not exist or is not readable: {}", plan_path))?;
    let canon_base = base.canonicalize().unwrap_or(base.clone());
    if !canon_cand.starts_with(&canon_base) {
        return Err(format!("[PlanPathOutsideDesignNotes] plan_path resolves outside .vaak/design-notes/ after canonicalization: {}", plan_path));
    }
    let content = std::fs::read_to_string(&canon_cand)
        .map_err(|e| format!("[PlanPathMissing] cannot read plan_path: {}", e))?;
    if parse_scope_block_main(&content).is_none() {
        return Err("[PlanScopeBlockMissing] plan file lacks <!-- scope: path1 path2 -->. Use <!-- scope: * --> for unrestricted plans.".to_string());
    }
    Ok(canon_cand)
}

/// Inner of `protocol_mutate_cmd` — pure-input version that runs the same
/// CAS gate + dispatch as vaak-mcp.rs's `do_protocol_mutate`. Mirrored by
/// design (vaak-mcp and vaak-desktop are separate binaries with no shared
/// crate; both serialize to the same JSON shape via OS-level board.lock).
///
/// As of A.5: handles toggle_queue/yield/force_release/phase_plan ops PLUS
/// the 8 two-controls v1 actions (set_assembly, accept_plan, open_planning,
/// revise_plan, set_mic_passing, raise_hand, grant_mic, set_moderator).
/// Mirror parity with vaak-mcp.rs's apply_* functions.
fn do_protocol_mutate_inner(
    pd: &str,
    actor: &str,
    section: &str,
    action: &str,
    args: serde_json::Value,
    rev_in: Option<u64>,
) -> Result<serde_json::Value, String> {
    if action == "keep_alive" {
        return Err("[InvalidAction] keep_alive is composer-side via MCP, not a UI mutate".to_string());
    }
    let rev_in = rev_in.ok_or("[MissingRev] rev field is required for protocol_mutate (silent CAS bypass forbidden)")?;

    let result: Result<Result<serde_json::Value, String>, String> =
        collab::with_board_lock(pd, || {
            let mut current = protocol::read_protocol_for_section(pd, section);
            let current_rev = current.rev;
            if rev_in != current_rev {
                return Ok(Err(format!(
                    "[StaleRev] expected rev {} (caller passed), current rev is {}",
                    rev_in, current_rev
                )));
            }

            // Slice 3 UI-side actions are the subset the panel needs:
            // toggle_queue (raise/lower hand on own chip) + yield (current
            // speaker drops mic). Everything else flows through the MCP
            // tool path so authority lives in one place per slice.
            let dispatch_result: Result<(), String> = match action {
                "toggle_queue" => {
                    let seat = args.get("seat").and_then(|v| v.as_str())
                        .map(String::from)
                        .unwrap_or_else(|| actor.to_string());
                    if seat != actor {
                        Err(format!("[NotPermitted] toggle_queue is self-only; caller '{}' tried to toggle '{}'", actor, seat))
                    } else {
                        let already_in = current.floor.queue.iter().any(|s| s == &seat);
                        if already_in {
                            current.floor.queue.retain(|s| s != &seat);
                        } else {
                            current.floor.queue.push(seat);
                        }
                        Ok(())
                    }
                }
                "yield" => {
                    if current.floor.current_speaker.as_deref() != Some(actor) {
                        Err(format!("[NotPermitted] caller '{}' is not current_speaker (current: {:?})", actor, current.floor.current_speaker))
                    } else {
                        let target = args.get("target").and_then(|v| v.as_str()).map(String::from);
                        let new_speaker = match target {
                            Some(t) => Some(t),
                            None => current.floor.queue.first().cloned(),
                        };
                        if let Some(t) = &new_speaker {
                            current.floor.queue.retain(|s| s != t);
                        }
                        current.floor.current_speaker = new_speaker;
                        Ok(())
                    }
                }
                // Human force-release (V3 follow-up): clears current_speaker
                // without the freshness gate transfer_mic enforces. Audit
                // event posted to board so the action is visible to the team
                // (visibility is the safety mechanism — confirmations train
                // dismissal, evil-arch msg 171). Human-only — agents must
                // yield or wait for the watchdog.
                "force_release" => {
                    if actor != "human" {
                        Err(format!(
                            "[NotPermitted] force_release is human-only (caller: '{}'). Agents must yield or wait for the watchdog.",
                            actor
                        ))
                    } else if current.floor.current_speaker.is_none() {
                        Err("[NoOp] force_release called but current_speaker is already null".to_string())
                    } else {
                        let prior = current.floor.current_speaker.clone().unwrap_or_default();
                        current.floor.current_speaker = None;
                        // idle_secs from sessions.json:last_working_at — same
                        // signal as the watchdog's stall criterion. Lets
                        // observers distinguish kicked-while-working from
                        // kicked-while-stalled. -1 if last_working_at is
                        // missing or unparseable. Per evil-arch msg 180 #3.
                        let now_secs = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_secs())
                            .unwrap_or(0);
                        let idle_secs: i64 = {
                            let sessions_path = std::path::Path::new(pd).join(".vaak").join("sessions.json");
                            let sessions: serde_json::Value = std::fs::read_to_string(&sessions_path)
                                .ok()
                                .and_then(|s| serde_json::from_str(&s).ok())
                                .unwrap_or(serde_json::Value::Null);
                            let mut parts = prior.splitn(2, ':');
                            let pr_role = parts.next().unwrap_or("");
                            let pr_inst: u64 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
                            sessions
                                .get("bindings")
                                .and_then(|b| b.as_array())
                                .and_then(|arr| arr.iter().find(|b| {
                                    b.get("role").and_then(|r| r.as_str()) == Some(pr_role)
                                        && b.get("instance").and_then(|i| i.as_u64()).unwrap_or(0) == pr_inst
                                }))
                                .and_then(|b| b.get("last_working_at").and_then(|v| v.as_str()))
                                .and_then(collab::parse_iso_epoch_pub)
                                .map(|w| now_secs.saturating_sub(w) as i64)
                                .unwrap_or(-1)
                        };
                        // Append mic_released event to board.jsonl. Best-effort —
                        // the floor mutation above is the load-bearing change;
                        // event-append failure logs but doesn't roll back.
                        let board_path = collab::active_board_path(pd);
                        let count = std::fs::read_to_string(&board_path)
                            .unwrap_or_default()
                            .lines()
                            .filter(|l| !l.trim().is_empty())
                            .count();
                        let now_iso = collab::iso_now();
                        let event = serde_json::json!({
                            "id": (count + 1) as u64,
                            "from": "human",
                            "to": "all",
                            "type": "mic_released",
                            "timestamp": now_iso,
                            "subject": format!("[mic_released] {} — human_force_release", prior),
                            "body": format!(
                                "Human force-released the mic from {} (idle: {}s). Mic is now free — next sender will auto-grab.",
                                prior, idle_secs
                            ),
                            "metadata": {
                                "from_speaker": prior,
                                "reason": "human_force_release",
                                "idle_secs_at_release": idle_secs,
                            }
                        });
                        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&board_path) {
                            use std::io::Write;
                            let _ = writeln!(f, "{}", serde_json::to_string(&event).unwrap_or_default());
                        }
                        Ok(())
                    }
                }
                // Slice 5 phase actions (spec §7).
                "pause_plan" => {
                    if current.phase_plan.paused_at.is_some() {
                        Err("[InvalidArgs] pause_plan: plan is already paused".to_string())
                    } else {
                        current.phase_plan.paused_at = Some(collab::iso_now());
                        Ok(())
                    }
                }
                "resume_plan" => {
                    let paused_at = match &current.phase_plan.paused_at {
                        Some(s) => s.clone(),
                        None => return Ok(Err("[InvalidArgs] resume_plan: plan is not paused".to_string())),
                    };
                    let paused_secs = parse_iso_to_epoch_secs_main(&paused_at);
                    let now_secs = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();
                    let pause_duration = paused_secs.map(|s| now_secs.saturating_sub(s)).unwrap_or(0);
                    current.phase_plan.paused_total_secs += pause_duration;
                    current.phase_plan.paused_at = None;
                    Ok(())
                }
                "extend_phase" => {
                    let secs = args.get("secs").and_then(|v| v.as_u64());
                    let secs = match secs {
                        Some(s) => s,
                        None => return Ok(Err("[InvalidArgs] extend_phase requires args.secs (positive integer)".to_string())),
                    };
                    let cur_idx = current.phase_plan.current_phase_idx;
                    if cur_idx >= current.phase_plan.phases.len() {
                        return Ok(Err(format!("[InvalidArgs] extend_phase: current_phase_idx {} out of range", cur_idx)));
                    }
                    current.phase_plan.phases[cur_idx].extension_secs += secs;
                    Ok(())
                }
                "advance_phase" => {
                    let phases_len = current.phase_plan.phases.len();
                    if phases_len == 0 {
                        return Ok(Err("[InvalidArgs] advance_phase: no phase_plan set — call set_phase_plan first".to_string()));
                    }
                    let cur_idx = current.phase_plan.current_phase_idx;
                    if cur_idx >= phases_len {
                        return Ok(Err(format!("[InvalidArgs] advance_phase: already past last phase ({}/{})", cur_idx, phases_len)));
                    }
                    let now = collab::iso_now();
                    current.phase_plan.phases[cur_idx].ended_at = Some(now.clone());
                    let next_idx = cur_idx + 1;
                    if next_idx < phases_len {
                        let next_phase = &mut current.phase_plan.phases[next_idx];
                        if next_phase.started_at.is_none() {
                            next_phase.started_at = Some(now);
                        }
                    }
                    current.phase_plan.current_phase_idx = next_idx.min(phases_len);
                    Ok(())
                }
                "set_phase_plan" => {
                    // Slice 9 phase editor wires this UI-side. v0 permissive
                    // auth per spec §10 plan-author tier ("any seat" in v0;
                    // hard-gate to phase_plan_authors lands in v1).
                    let phases_val = args.get("phases").cloned().unwrap_or(serde_json::Value::Null);
                    let phases_arr = match phases_val.as_array() {
                        Some(a) => a.clone(),
                        None => return Ok(Err("[InvalidArgs] set_phase_plan requires args.phases (array)".to_string())),
                    };
                    if phases_arr.is_empty() {
                        return Ok(Err("[InvalidArgs] set_phase_plan: phases array must be non-empty".to_string()));
                    }
                    // Validate + deserialize each phase via the existing
                    // protocol::Phase struct. serde_json::from_value enforces
                    // schema (preset + outcome required).
                    let mut new_phases: Vec<protocol::Phase> = Vec::with_capacity(phases_arr.len());
                    for (i, p) in phases_arr.iter().enumerate() {
                        match serde_json::from_value::<protocol::Phase>(p.clone()) {
                            Ok(phase) => new_phases.push(phase),
                            Err(e) => return Ok(Err(format!("[InvalidArgs] phase[{}] schema error: {}", i, e))),
                        }
                    }
                    // Stamp started_at on phase[0] if not already set.
                    if let Some(first) = new_phases.first_mut() {
                        if first.started_at.is_none() {
                            first.started_at = Some(collab::iso_now());
                        }
                    }
                    current.phase_plan.phases = new_phases;
                    current.phase_plan.current_phase_idx = 0;
                    current.phase_plan.paused_at = None;
                    current.phase_plan.paused_total_secs = 0;
                    Ok(())
                }
                // Two-controls v1 (commit A.5 — mirror of vaak-mcp.rs's apply_*
                // functions for the desktop UI's protocol_mutate_cmd path).
                // Per main.rs:3534 "mirrored by design" comment, both binaries
                // must dispatch the same actions. Commit A added these to
                // vaak-mcp.rs only; the desktop UI's clicks were silently
                // failing here with [InvalidAction] until A.5.
                "set_assembly" => {
                    let active = args.get("active").and_then(|v| v.as_bool());
                    match active {
                        Some(a) => {
                            // Coordinate preset (matches apply_set_assembly).
                            // The v1.0.7 interim gate in apply_set_preset
                            // rejects direct cross-preset transitions, so
                            // route through "Default chat" first if needed.
                            let target_preset = if a { "Assembly Line" } else { "Default chat" };
                            current.preset = target_preset.to_string();
                            current.floor.mode = if a { "round-robin".to_string() } else { "none".to_string() };
                            current.floor.assembly_active = Some(a);
                            if !a {
                                current.floor.current_speaker = None;
                            }
                            Ok(())
                        }
                        None => Err("[InvalidArgs] set_assembly requires args.active (bool)".to_string()),
                    }
                }
                "accept_plan" => {
                    // moderator-authority Item 4 gate — moderator OR architect/manager/human.
                    // Closes evil-arch msg 1490 CRITICAL.
                    let role = caller_role_main(actor);
                    let is_moderator = current.floor.moderator.as_deref() == Some(actor);
                    let is_privileged = matches!(role, "architect" | "manager" | "human");
                    if !is_moderator && !is_privileged {
                        Err(format!(
                            "[AcceptPlanForbidden] caller '{}' (role '{}') may not call accept_plan — gated to current moderator OR architect/manager/human (evil-arch msg 1490 CRITICAL closure, moderator-authority Item 4).",
                            actor, role
                        ))
                    } else {
                        let plan_path = args.get("plan_path").and_then(|v| v.as_str());
                        match plan_path {
                            Some(p) => match validate_plan_path_main(pd, p) {
                                Ok(canon) => {
                                    let content = std::fs::read(&canon)
                                        .map_err(|e| format!("[PlanPathMissing] cannot read plan_path post-validation: {}", e))?;
                                    let hash = sha256_hex_main(&content);
                                    current.floor.phase = Some("execution".to_string());
                                    current.floor.plan_path = Some(p.to_string());
                                    current.floor.plan_hash = Some(hash);
                                    Ok(())
                                }
                                Err(e) => Err(e),
                            },
                            None => Err("[InvalidArgs] accept_plan requires args.plan_path (string)".to_string()),
                        }
                    }
                }
                "open_planning" => {
                    // moderator-authority Item 4 gate — same pattern as accept_plan.
                    let role = caller_role_main(actor);
                    let is_moderator = current.floor.moderator.as_deref() == Some(actor);
                    let is_privileged = matches!(role, "architect" | "manager" | "human");
                    if !is_moderator && !is_privileged {
                        Err(format!(
                            "[OpenPlanningForbidden] caller '{}' (role '{}') may not call open_planning — gated to current moderator OR architect/manager/human (evil-arch msg 1490 CRITICAL closure, moderator-authority Item 4).",
                            actor, role
                        ))
                    } else {
                        current.floor.phase = Some("planning".to_string());
                        current.floor.plan_path = None;
                        current.floor.plan_hash = None;
                        Ok(())
                    }
                }
                "open_execution" => {
                    // Free planning→execution toggle (human msg 577: switching to
                    // execution should NOT demand a typed plan_path). Mirror of
                    // open_planning — same moderator/privileged gate, no plan
                    // required. accept_plan stays as the OPTIONAL plan-pinned
                    // variant; this is the one-click default path. Clears any
                    // stale accepted-plan pins so a free toggle doesn't leave a
                    // dangling plan_hash (architect 584).
                    let role = caller_role_main(actor);
                    let is_moderator = current.floor.moderator.as_deref() == Some(actor);
                    let is_privileged = matches!(role, "architect" | "manager" | "human");
                    if !is_moderator && !is_privileged {
                        Err(format!(
                            "[OpenExecutionForbidden] caller '{}' (role '{}') may not call open_execution — gated to current moderator OR architect/manager/human (mirror of open_planning).",
                            actor, role
                        ))
                    } else {
                        current.floor.phase = Some("execution".to_string());
                        current.floor.plan_path = None;
                        current.floor.plan_hash = None;
                        Ok(())
                    }
                }
                "revise_plan" => {
                    let role = caller_role_main(actor);
                    if !matches!(role, "architect" | "manager" | "human") {
                        Err(format!(
                            "[RevisePlanForbidden] caller role '{}' may not call revise_plan — gated to architect/manager/human only.",
                            role
                        ))
                    } else {
                        let plan_path = args.get("plan_path").and_then(|v| v.as_str());
                        match plan_path {
                            Some(p) => match validate_plan_path_main(pd, p) {
                                Ok(canon) => {
                                    let content = std::fs::read(&canon)
                                        .map_err(|e| format!("[PlanPathMissing] cannot read plan_path post-validation: {}", e))?;
                                    let new_hash = sha256_hex_main(&content);
                                    current.floor.plan_path = Some(p.to_string());
                                    current.floor.plan_hash = Some(new_hash);
                                    Ok(())
                                }
                                Err(e) => Err(e),
                            },
                            None => Err("[InvalidArgs] revise_plan requires args.plan_path (string)".to_string()),
                        }
                    }
                }
                "set_mic_passing" => {
                    let mode = args.get("mode").and_then(|v| v.as_str());
                    match mode {
                        Some(m) if matches!(m, "rotation" | "hand_raise" | "moderator") => {
                            let prev = current.floor.mic_passing_mode.clone().unwrap_or_else(|| "rotation".to_string());
                            // Defer-silent: mid-turn mode change is no-op
                            // (matches vaak-mcp.rs apply_set_mic_passing).
                            if current.floor.current_speaker.is_some() && prev != m {
                                Ok(())
                            } else if prev == m {
                                // Idempotent no-op.
                                Ok(())
                            } else {
                                current.floor.mic_passing_mode = Some(m.to_string());
                                // Cascading state cleanup.
                                match m {
                                    "rotation" => {
                                        current.floor.hand_queue = Some(vec![]);
                                        current.floor.moderator = None;
                                    }
                                    "hand_raise" => {
                                        current.floor.moderator = None;
                                    }
                                    "moderator" => {
                                        current.floor.hand_queue = Some(vec![]);
                                    }
                                    _ => {}
                                }
                                Ok(())
                            }
                        }
                        Some(m) => Err(format!("[UnknownMicMechanism] mic_passing_mode must be rotation|hand_raise|moderator: {}", m)),
                        None => Err("[InvalidArgs] set_mic_passing requires args.mode (string)".to_string()),
                    }
                }
                "raise_hand" => {
                    let mode_str = current.floor.mic_passing_mode.clone().unwrap_or_else(|| "rotation".to_string());
                    if mode_str != "hand_raise" {
                        Err(format!("[NotPermitted] raise_hand requires mic_passing_mode == 'hand_raise' (current: {})", mode_str))
                    } else {
                        let mut queue = current.floor.hand_queue.clone().unwrap_or_default();
                        if !queue.contains(&actor.to_string()) {
                            queue.push(actor.to_string());
                        }
                        current.floor.hand_queue = Some(queue);
                        Ok(())
                    }
                }
                "grant_mic" => {
                    let target = args.get("target").and_then(|v| v.as_str());
                    let mode_str = current.floor.mic_passing_mode.clone().unwrap_or_else(|| "rotation".to_string());
                    match target {
                        Some(t) => {
                            if mode_str != "moderator" {
                                Err(format!("[NotPermitted] grant_mic requires mic_passing_mode == 'moderator' (current: {})", mode_str))
                            } else {
                                let mod_label = current.floor.moderator.clone();
                                if mod_label.as_deref() != Some(actor) {
                                    Err(format!("[NotPermitted] grant_mic restricted to moderator '{:?}' (caller: {})", mod_label, actor))
                                } else {
                                    current.floor.current_speaker = Some(t.to_string());
                                    Ok(())
                                }
                            }
                        }
                        None => Err("[InvalidArgs] grant_mic requires args.target (string)".to_string()),
                    }
                }
                "set_moderator" => {
                    let role = caller_role_main(actor);
                    if !matches!(role, "architect" | "manager" | "human") {
                        Err(format!(
                            "[SetModeratorForbidden] caller role '{}' may not call set_moderator — gated to architect/manager/human only.",
                            role
                        ))
                    } else {
                        let target = args.get("seat").and_then(|v| v.as_str());
                        match target {
                            Some(t) => {
                                current.floor.moderator = Some(t.to_string());
                                Ok(())
                            }
                            None => Err("[InvalidArgs] set_moderator requires args.seat (string)".to_string()),
                        }
                    }
                }
                // Collaborative-proposal-workflow v1 (spec 2026-05-15, Commit P + P.B).
                // Mirror of vaak-mcp.rs apply_propose_replanning per
                // feedback_mirror_binary_parity_audit. Same gate, same queue
                // append. No role gate — any active seat can propose.
                //
                // ts injection per spec v6 line 29 (dev-challenger msg 1939 #3
                // + architect msg 1944 fold): production callers (this Tauri
                // dispatch + the vaak-mcp.rs MCP dispatch) ignore any
                // caller-supplied args.ts and always fill with now(). The
                // explicit ts parameter on apply_propose_replanning exists
                // only for test-side deterministic seeding (R3 N≥4 FIFO
                // assertion at sub-millisecond resolution); production never
                // exposes the field to MCP/Tauri callers.
                "propose_replanning" => {
                    let phase = current.floor.phase.as_deref().unwrap_or("execution");
                    if phase != "execution" {
                        Err(format!(
                            "[ProposeReplanningPhaseInvalid] propose_replanning requires phase == 'execution' (current: {})",
                            phase
                        ))
                    } else {
                        let reason = args.get("reason").and_then(|v| v.as_str());
                        match reason {
                            Some(r) => {
                                let ts = std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .map(|d| d.as_millis() as i64)
                                    .unwrap_or(0);
                                current.floor.replanning_requests.push(
                                    protocol::ReplanningRequest {
                                        seat: actor.to_string(),
                                        reason: r.to_string(),
                                        ts,
                                    },
                                );
                                Ok(())
                            }
                            None => Err("[InvalidArgs] propose_replanning requires args.reason (string)".to_string()),
                        }
                    }
                }
                // Commit S — set_review_intensity (strict-turn-discipline +
                // review-intensity-slider spec 2026-05-15). Mirror of
                // vaak-mcp.rs apply_set_review_intensity. Role gate +
                // range 1-10 validation; writes to floor.review_intensity.
                "set_review_intensity" => {
                    let role = caller_role_main(actor);
                    let is_moderator =
                        current.floor.moderator.as_deref() == Some(actor);
                    let is_privileged =
                        matches!(role, "architect" | "manager" | "human");
                    if !is_moderator && !is_privileged {
                        Err(format!(
                            "[SetReviewIntensityForbidden] caller '{}' (role '{}') not moderator or privileged — gated to moderator OR architect/manager/human.",
                            actor, role
                        ))
                    } else {
                        let level = args.get("level").and_then(|v| v.as_u64());
                        match level {
                            Some(n) if (1..=10).contains(&n) => {
                                current.floor.review_intensity = n as u8;
                                Ok(())
                            }
                            Some(n) => Err(format!(
                                "[InvalidArgs] set_review_intensity level must be 1-10 (got {})",
                                n
                            )),
                            None => Err("[InvalidArgs] set_review_intensity requires args.level (integer 1-10)".to_string()),
                        }
                    }
                }
                // Commit Q — accept_replanning. Mirror of vaak-mcp.rs
                // apply_accept_replanning. Role gate identical to v1.X
                // open_planning / revise_plan: moderator OR
                // architect/manager/human. Atomic side effects per spec
                // line 51-53.
                "accept_replanning" => {
                    let role = caller_role_main(actor);
                    let is_moderator =
                        current.floor.moderator.as_deref() == Some(actor);
                    let is_privileged =
                        matches!(role, "architect" | "manager" | "human");
                    if !is_moderator && !is_privileged {
                        Err(format!(
                            "[AcceptReplanningForbidden] caller '{}' (role '{}') not moderator or privileged — gated to moderator OR architect/manager/human (spec §accept_replanning role gate).",
                            actor, role
                        ))
                    } else {
                        // Validate request_index if provided. Out-of-bounds
                        // rejects so the event payload's triggered_by lookup
                        // is consistent with the accept.
                        let idx_validation = args
                            .get("request_index")
                            .and_then(|v| v.as_u64())
                            .map(|idx| {
                                let queue_len = current.floor.replanning_requests.len();
                                if (idx as usize) >= queue_len {
                                    Err(format!(
                                        "[InvalidArgs] accept_replanning request_index {} out of bounds for queue of length {}",
                                        idx, queue_len
                                    ))
                                } else {
                                    Ok(())
                                }
                            });
                        if let Some(Err(e)) = idx_validation {
                            Err(e)
                        } else {
                            // Atomic side effects per spec line 51-53.
                            current.floor.phase = Some("planning".to_string());
                            current.floor.plan_path = None;
                            current.floor.plan_hash = None;
                            current.floor.replanning_requests = vec![];
                            Ok(())
                        }
                    }
                }
                // SHA-HR.1.3 — Phase 1 hot-reload architecture per
                // `.vaak/design-notes/2026-05-28-hot-reload-architecture-spec.md`
                // + human msg 2415. set_preset is the pilot action migrating from
                // the sidecar's do_protocol_mutate to the Tauri-side path.
                //
                // Round-trip per mutation per tester:0 msg 2522 vote + developer:0
                // msg 2515 lean (option a): serialize typed Protocol → JSON Value,
                // call moved helpers in mcp_handlers::assembly_line, deserialize
                // back. Preserves single-source-of-truth from SHA-HR.1.2 helper
                // moves; ~1ms overhead acceptable for tool-call cadence.
                //
                // Mirrors the sidecar's apply_set_preset dispatch chain at
                // vaak-mcp.rs:4059-4061 — apply_set_preset followed by
                // seed_rotation_order_force (set_preset/set_assembly only) +
                // protocol_normalize_in_place.
                "set_preset" => {
                    let mut json: serde_json::Value = serde_json::to_value(&current)
                        .map_err(|e| format!("[ProtocolSerialize] {}", e))?;
                    let active_seats =
                        crate::mcp_handlers::assembly_line::protocol_active_seats_set(pd);
                    match crate::mcp_handlers::assembly_line::apply_set_preset(&mut json, &args) {
                        Ok(()) => {
                            crate::mcp_handlers::assembly_line::seed_rotation_order_force(
                                &mut json,
                                &active_seats,
                            );
                            crate::mcp_handlers::assembly_line::protocol_normalize_in_place(
                                &mut json,
                                &active_seats,
                            );
                            match serde_json::from_value::<protocol::Protocol>(json) {
                                Ok(new_proto) => {
                                    current = new_proto;
                                    Ok(())
                                }
                                Err(e) => Err(format!("[ProtocolDeserialize] {}", e)),
                            }
                        }
                        Err(e) => Err(e),
                    }
                }
                other => Err(format!(
                    "[InvalidAction] UI dispatch handles toggle_queue/yield/pause_plan/resume_plan/extend_phase/advance_phase + two-controls v1 (set_assembly/accept_plan/open_planning/open_execution/revise_plan/set_mic_passing/raise_hand/grant_mic/set_moderator) + collaborative-proposal v1 (propose_replanning/accept_replanning) + SHA-HR.1.3 hot-reload (set_preset); '{}' must go through MCP protocol_mutate",
                    other
                )),
            };

            if let Err(e) = dispatch_result {
                return Ok(Err(e));
            }

            current.rev = current_rev + 1;
            current.last_writer_seat = Some(actor.to_string());
            current.last_writer_action = Some(action.to_string());
            current.rev_at = Some(collab::iso_now());

            if let Err(e) = protocol::write_protocol_for_section_unlocked(pd, section, &current) {
                return Ok(Err(e));
            }
            Ok(Ok(serde_json::to_value(&current).unwrap_or(serde_json::Value::Null)))
        });
    match result {
        Ok(inner) => inner,
        Err(e) => Err(e),
    }
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
    // DoS guard per evil-arch msg 4522 / dev-challenger msg 4516 flag on 171d832
    // (removed frontend 50KB cap). Without backend guard, multi-GB bodies would
    // fill disk via board.jsonl append. 10MB is well above any legitimate human-
    // composed message (entire books fit in ~3MB plain text) but well below
    // anything that would meaningfully impact disk in a single message.
    const MAX_MESSAGE_BODY_BYTES: usize = 10 * 1024 * 1024;
    if body.len() > MAX_MESSAGE_BODY_BYTES {
        return Err(format!(
            "Message body too large: {} bytes exceeds {} byte limit. Send as multiple smaller messages or attach as file (future feature).",
            body.len(), MAX_MESSAGE_BODY_BYTES
        ));
    }
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

// ==================== Decision Panel v1 commands ====================
//
// See collab.rs `Decision Panel v1` section for the wire-format contract and
// why these commands don't change the MCP sidecar or the project_send schema.

/// Read all resolution entries for the active section. The frontend joins
/// these against board.jsonl messages (filtered to type=question + to=human
/// + metadata.choices) to derive the pending-decisions list. Returned as a
/// flat Vec so the JS side can build whatever index it needs.
#[tauri::command]
fn list_decision_resolutions_cmd(dir: String) -> Result<Vec<collab::DecisionResolution>, String> {
    let dir = validate_project_dir(&dir)?;
    let map = collab::read_decision_resolutions(&dir);
    Ok(map.into_values().collect())
}

/// Resolve a pending decision. If `option_id` is provided, a type:"answer"
/// board message is appended (matches the existing inline QuestionCard
/// flow so the board scrollback shows the choice). If `other_text` is
/// provided, a type:"directive" board message ALSO fires with
/// metadata.in_reply_to set — flag #3 from msg 4784: human's Other text
/// becomes a new directive the team picks up on rotation.
///
/// Both message-appends and the decisions.jsonl resolution entry happen
/// inside one with_board_lock() to keep observers atomic.
#[tauri::command]
fn resolve_decision_cmd(
    dir: String,
    decision_id: u64,
    option_id: Option<String>,
    option_label: Option<String>,
    other_text: Option<String>,
) -> Result<(), String> {
    let dir = validate_project_dir(&dir)?;

    if option_id.is_none() && other_text.as_ref().map(|s| s.trim().is_empty()).unwrap_or(true) {
        return Err("Either option_id or non-empty other_text must be provided.".to_string());
    }

    let board_path = collab::active_board_path(&dir);
    let now = iso_now();

    collab::with_board_lock(&dir, || {
        // Determine next message id from current board state
        let existing = std::fs::read_to_string(&board_path).unwrap_or_default();
        let mut next_id: u64 = existing.lines()
            .filter(|l| !l.trim().is_empty())
            .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
            .filter_map(|m| m.get("id").and_then(|i| i.as_u64()))
            .max()
            .unwrap_or(0) + 1;

        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&board_path)
            .map_err(|e| format!("Failed to open board.jsonl: {}", e))?;

        // 1. Inline answer message (preserves existing QuestionCard "answered" semantics)
        if let Some(oid) = option_id.as_ref() {
            let label = option_label.clone().unwrap_or_else(|| oid.clone());
            let answer = serde_json::json!({
                "id": next_id,
                "from": "human:0",
                "to": "all",
                "type": "answer",
                "timestamp": now,
                "subject": format!("Re: #{}", decision_id),
                "body": label,
                "metadata": {
                    "in_reply_to": decision_id,
                    "choice_id": oid,
                }
            });
            writeln!(file, "{}", answer.to_string())
                .map_err(|e| format!("Failed to write answer message: {}", e))?;
            next_id += 1;
        }

        // 2. Other → directive emission (flag #3)
        if let Some(text) = other_text.as_ref() {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                let directive = serde_json::json!({
                    "id": next_id,
                    "from": "human:0",
                    "to": "all",
                    "type": "directive",
                    "timestamp": now,
                    "subject": format!("Re: #{}", decision_id),
                    "body": trimmed,
                    "metadata": {
                        "in_reply_to": decision_id,
                        "from_decision_panel_other": true,
                    }
                });
                writeln!(file, "{}", directive.to_string())
                    .map_err(|e| format!("Failed to write directive message: {}", e))?;
            }
        }

        // 3. Persist resolution log entry (decisions.jsonl)
        let r = collab::DecisionResolution {
            decision_id,
            kind: "resolve".to_string(),
            option_id: option_id.clone(),
            other_text: other_text.as_ref().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
            reason: None,
            at: now.clone(),
            by: "human:0".to_string(),
        };
        collab::append_decision_resolution(&dir, &r)?;

        Ok(())
    })?;

    notify_collab_change();
    Ok(())
}

/// Cancel a pending decision without firing a directive. Used by the panel's
/// kill icon (flag #4: author-cancel surface). The reason field also accepts
/// "stale_archive" for the 24h auto-archive path and "board_resolved" for the
/// "subsequent directive matches topic" path — both invoked by the frontend
/// when it detects the conditions.
#[tauri::command]
fn cancel_decision_cmd(
    dir: String,
    decision_id: u64,
    reason: Option<String>,
) -> Result<(), String> {
    let dir = validate_project_dir(&dir)?;
    let r = collab::DecisionResolution {
        decision_id,
        kind: "cancel".to_string(),
        option_id: None,
        other_text: None,
        reason: Some(reason.unwrap_or_else(|| "author_cancel".to_string())),
        at: iso_now(),
        by: "human:0".to_string(),
    };
    collab::with_board_lock(&dir, || {
        collab::append_decision_resolution(&dir, &r)
    })?;
    notify_collab_change();
    Ok(())
}

/// Commit D — DelegationEntry + parse_delegation_blocks helpers per
/// collaborative-proposal-workflow-spec-2026-05-15.md §Delegation-block
/// markup + §parse_delegation_blocks (lines 89-106 + 166).
///
/// Two parser surfaces per architect msg 1944 (folding dev-challenger msg
/// 1939 #2):
///   - parse_delegation_blocks_lenient: drops malformed silently for the
///     UI's Affordance B chart (forward-compat — partial plan-doc edits
///     mid-write shouldn't break the chart render)
///   - parse_delegation_blocks_strict: reports ParseError list for the
///     pre-commit hook's well-formedness gate (Commit H Python side has
///     its own parser; Rust strict variant exists for symmetric API +
///     for future Tauri-side hook integration)
///
/// Hand-rolled (no regex crate dependency) matching the non-greedy
/// `<!--\s*delegation:\s*(.+?)\s*-->` semantics per platform-eng msg 1916
/// #2. Hyphenated values in field bodies (`section=V.A-1`, `deadline=
/// after-pilot`, `owner=ui-architect:0`) are preserved because the close
/// is determined by `-->` lookahead, not by stopping at `-`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
struct DelegationEntry {
    owner: String,
    section: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    deadline: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    deps: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
enum ParseError {
    MissingOwner { raw_body: String },
    MissingSection { raw_body: String },
    DuplicateSection { section: String, owner: String },
}

/// Iterate `<!-- delegation: ... -->` blocks in `content`. Returns the
/// inner-body string for each block (the part between `delegation:` and
/// `-->`, trimmed). Closure-driven so both lenient + strict can reuse
/// the same scanning logic.
fn for_each_delegation_block<F: FnMut(&str)>(content: &str, mut f: F) {
    let open = "<!--";
    let needle = "delegation:";
    let close = "-->";
    let mut rest = content;
    while let Some(open_pos) = rest.find(open) {
        let after_open = &rest[open_pos + open.len()..];
        // Optional whitespace then "delegation:" — skip otherwise.
        let trimmed = after_open.trim_start();
        if !trimmed.starts_with(needle) {
            // Not a delegation block; advance past this `<!--` and keep
            // scanning. (Could be a scope:, delegation-target:, or any
            // unrelated HTML comment.)
            rest = after_open;
            continue;
        }
        let body_start_offset =
            (after_open.len() - trimmed.len()) + needle.len();
        let body_and_after = &after_open[body_start_offset..];
        let Some(close_pos) = body_and_after.find(close) else {
            // Unclosed comment — skip the rest of the buffer to avoid
            // infinite loop. v1 chooses to silently bail (malformed
            // plan doc; pre-commit hook will catch it via the v1.1
            // scope-block parser separately).
            break;
        };
        let body = body_and_after[..close_pos].trim();
        f(body);
        // Advance past the close marker.
        rest = &body_and_after[close_pos + close.len()..];
    }
}

/// Parse a single delegation-block body into a DelegationEntry. Returns
/// None on missing required fields (owner, section). Used by both
/// lenient and strict callers via different error paths.
fn parse_delegation_body(body: &str) -> Result<DelegationEntry, ParseError> {
    let mut owner: Option<String> = None;
    let mut section: Option<String> = None;
    let mut deadline: Option<String> = None;
    let mut deps: Vec<String> = vec![];
    for token in body.split_whitespace() {
        let Some(eq) = token.find('=') else { continue };
        let key = &token[..eq];
        let value = &token[eq + 1..];
        match key {
            "owner" => owner = Some(value.to_string()),
            "section" => section = Some(value.to_string()),
            "deadline" => deadline = Some(value.to_string()),
            "deps" => {
                deps = value
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
            }
            _ => {} // forward-compat: unknown keys silently dropped
        }
    }
    match (owner, section) {
        (Some(o), Some(s)) => Ok(DelegationEntry {
            owner: o,
            section: s,
            deadline,
            deps,
        }),
        (None, _) => Err(ParseError::MissingOwner {
            raw_body: body.to_string(),
        }),
        (Some(_), None) => Err(ParseError::MissingSection {
            raw_body: body.to_string(),
        }),
    }
}

/// Lenient parser per spec §parse_delegation_blocks (Affordance B chart
/// path). Drops malformed blocks silently — forward-compat for users
/// editing plan docs mid-write.
fn parse_delegation_blocks_lenient(content: &str) -> Vec<DelegationEntry> {
    let mut out = Vec::new();
    for_each_delegation_block(content, |body| {
        if let Ok(entry) = parse_delegation_body(body) {
            out.push(entry);
        }
    });
    out
}

/// Strict parser per architect msg 1944 + spec §Pre-commit hook
/// extension. Reports all errors (malformed blocks AND duplicate
/// section names from spec line 115 `[delegation_drift]`). Hook
/// callers branch on Err for non-zero exit.
fn parse_delegation_blocks_strict(
    content: &str,
) -> Result<Vec<DelegationEntry>, Vec<ParseError>> {
    let mut entries: Vec<DelegationEntry> = Vec::new();
    let mut errors: Vec<ParseError> = Vec::new();
    let mut seen_sections: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    for_each_delegation_block(content, |body| {
        match parse_delegation_body(body) {
            Ok(entry) => {
                if let Some(existing_owner) = seen_sections.get(&entry.section) {
                    errors.push(ParseError::DuplicateSection {
                        section: entry.section.clone(),
                        owner: existing_owner.clone(),
                    });
                } else {
                    seen_sections.insert(entry.section.clone(), entry.owner.clone());
                    entries.push(entry);
                }
            }
            Err(e) => errors.push(e),
        }
    });
    if errors.is_empty() {
        Ok(entries)
    } else {
        Err(errors)
    }
}

/// Tauri command for the Affordance B chart UI — reads a plan file and
/// returns the lenient-parsed delegation entries. Strict variant is
/// reserved for hook integration (Python hook has its own parser at v1;
/// Rust strict exists for symmetric API + future use).
#[tauri::command]
fn parse_delegation_blocks_cmd(plan_path: String) -> Result<Vec<DelegationEntry>, String> {
    let content = std::fs::read_to_string(&plan_path)
        .map_err(|e| format!("Failed to read plan file {}: {}", plan_path, e))?;
    Ok(parse_delegation_blocks_lenient(&content))
}

/// Commit Q.B — replanning_dismissed informational event emit per
/// collaborative-proposal-workflow-spec-2026-05-15.md §Affordance C line
/// 187. Non-state-mutating board record so the team's audit trail captures
/// the moderator's "I saw this and chose not to pivot" decisions. The
/// associated request STAYS in floor.replanning_requests — only the
/// moderator's private UI surfaces it as dismissed (localStorage continuity
/// in AssemblyControls.tsx). Distinct from accept_replanning which DOES
/// mutate state through protocol_mutate; dismiss is moderator UX, not a
/// state transition.
///
/// No mirror in vaak-mcp.rs: dismiss originates only in the Tauri UI
/// (moderator-side). AI agents have no dismiss path — they propose, the
/// moderator decides.
#[tauri::command]
fn emit_replanning_dismissed_cmd(
    dir: String,
    request_seat: String,
    request_reason: String,
    request_ts: i64,
    request_index: u64,
    moderator: String,
) -> Result<u64, String> {
    let dir = validate_project_dir(&dir)?;
    let board_path = collab::active_board_path(&dir);

    let existing = std::fs::read_to_string(&board_path).unwrap_or_default();
    let max_id: u64 = existing
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter_map(|m| m.get("id").and_then(|i| i.as_u64()))
        .max()
        .unwrap_or(0);
    let msg_id = max_id + 1;

    let now = collab::iso_now();

    let message = serde_json::json!({
        "id": msg_id,
        "from": "system",
        "to": "all",
        "type": "replanning_dismissed",
        "timestamp": now,
        "subject": format!("[replanning_dismissed] {} dismissed {}'s request", moderator, request_seat),
        "body": format!(
            "Moderator {} dismissed replanning request from {}: {}",
            moderator, request_seat, request_reason
        ),
        "metadata": {
            "moderator": moderator,
            "request_seat": request_seat,
            "request_reason": request_reason,
            "request_ts": request_ts,
            "request_index": request_index,
        }
    });

    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&board_path)
        .map_err(|e| format!("Failed to open board.jsonl: {}", e))?;

    writeln!(file, "{}", message)
        .map_err(|e| format!("Failed to write replanning_dismissed event: {}", e))?;

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
    stats: Option<collab::RoleStats>,
    avatar_url: Option<String>,
) -> Result<collab::RoleConfig, String> {
    // Auto-save to global templates happens inside collab::create_role
    let result = collab::create_role(&project_dir, &slug, &title, &description, permissions, max_instances, &briefing, tags, companions.unwrap_or_default(), stats, avatar_url);
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
    stats: Option<collab::RoleStats>,
    avatar_url: Option<String>,
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
        stats,
        avatar_url,
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
    if result.is_ok() {
        notify_collab_change();
        // Auto-save all role_groups to global ~/.vaak/role-groups.json
        if let Err(e) = sync_role_groups_to_global(&project_dir) {
            eprintln!("[main] Auto-sync role groups to global failed: {}", e);
        }
    }
    result
}

#[tauri::command]
fn delete_role_group(project_dir: String, slug: String) -> Result<(), String> {
    let project_dir = validate_project_dir(&project_dir)?;
    let result = collab::delete_role_group(&project_dir, &slug);
    if result.is_ok() {
        notify_collab_change();
        if let Err(e) = sync_role_groups_to_global(&project_dir) {
            eprintln!("[main] Auto-sync role groups to global failed: {}", e);
        }
    }
    result
}

/// Sync role_groups from project.json to ~/.vaak/role-groups.json
fn sync_role_groups_to_global(project_dir: &str) -> Result<(), String> {
    let config_path = std::path::Path::new(project_dir).join(".vaak").join("project.json");
    let content = std::fs::read_to_string(&config_path)
        .map_err(|e| format!("Failed to read project.json: {}", e))?;
    let config: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse project.json: {}", e))?;

    let groups = config.get("role_groups")
        .ok_or("No role_groups in project.json")?;

    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .ok_or("Cannot determine home directory")?;
    let global_path = std::path::Path::new(&home).join(".vaak").join("role-groups.json");
    let json = serde_json::to_string_pretty(groups)
        .map_err(|e| format!("Failed to serialize role groups: {}", e))?;
    std::fs::write(&global_path, json)
        .map_err(|e| format!("Failed to write global role-groups.json: {}", e))?;
    Ok(())
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
/// Layer 2: ensure the supervisor process is running for this project.
/// Reads .vaak/supervisor.pid; if absent or the recorded PID is dead, spawns
/// a fresh `vaak-mcp.exe --supervise <project_dir>` daemon. Idempotent so
/// it's safe to call from the watcher loop.
///
/// Without this, the supervisor only runs if the user manually launches it,
/// which the health pill correctly reports as "Not running — restart vaak to
/// enable" but tonight's bug is that nothing in the Vaak app actually starts
/// it on restart. Architect msg 119 placement: call from the watcher loop so
/// it spawns once per watched-project, not per-app-startup (which would
/// happen before any project is selected).
fn ensure_supervise_running(project_dir: &str) {
    use std::path::Path;
    let supervisor_pid_path = Path::new(project_dir).join(".vaak").join("supervisor.pid");
    let alive = std::fs::read_to_string(&supervisor_pid_path)
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok())
        .map(|pid| {
            #[cfg(windows)]
            {
                use std::process::Command;
                Command::new("tasklist")
                    .args(["/FI", &format!("PID eq {}", pid), "/FO", "CSV", "/NH"])
                    .output()
                    .map(|o| {
                        let stdout = String::from_utf8_lossy(&o.stdout);
                        stdout.contains(&format!("\"{}\"", pid))
                    })
                    .unwrap_or(false)
            }
            #[cfg(not(windows))]
            {
                use std::process::Command;
                Command::new("kill")
                    .args(["-0", &pid.to_string()])
                    .status()
                    .map(|s| s.success())
                    .unwrap_or(false)
            }
        })
        .unwrap_or(false);

    if alive {
        return;
    }

    let sidecar = match get_sidecar_path() {
        Some(p) => p.to_string_lossy().to_string().trim_start_matches(r"\\?\").to_string(),
        None => {
            log_error("[supervise] cannot auto-launch — vaak-mcp sidecar not found");
            return;
        }
    };
    log_error(&format!("[supervise] auto-launching for project: {}", project_dir));
    let spawn_result = std::process::Command::new(&sidecar)
        .args(["--supervise", project_dir])
        // Detach: don't inherit stdio, don't tie lifetime to parent.
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
    match spawn_result {
        Ok(child) => log_error(&format!("[supervise] spawned pid={}", child.id())),
        Err(e) => log_error(&format!("[supervise] spawn failed: {}", e)),
    }
}

fn start_project_watcher(app_handle: tauri::AppHandle) {
    std::thread::spawn(move || {
        let mut cleanup_counter: u32 = 0;
        // Start at the threshold so the first iteration with a watched dir
        // fires the supervise check immediately, not after a 10s delay.
        let mut supervise_check_counter: u32 = 10;
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
                        supervise_check_counter = 0;
                        continue;
                    }
                }
            };

            // Layer 2 auto-launch: check every ~10s whether the supervisor is
            // alive for the watched project, spawn if dead. Idempotent inside
            // ensure_supervise_running so repeated checks are cheap.
            supervise_check_counter += 1;
            if supervise_check_counter >= 10 {
                supervise_check_counter = 0;
                ensure_supervise_running(&dir);
            }

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

            // Watch protocol.json for the active section. Sidecar writes to
            // this file directly (assembly auto-advance, yield, etc.) without
            // going through protocol_mutate_cmd, so without this watch the UI
            // never sees those updates — that's the "top-right indicator
            // doesn't update" bug from board msg 74. Section-keyed so a
            // section switch triggers a re-emit (UI re-fetches for the new
            // section).
            let active_section = collab::get_active_section(&dir);
            let proto_path = protocol::protocol_path_for_section(&dir, &active_section);
            if let Some(proto_mtime) = proto_path.metadata().ok().and_then(|m| m.modified().ok()) {
                let mut last = get_protocol_last_mtime().lock();
                let proto_changed = match last.as_ref() {
                    Some((sec, mt)) => sec != &active_section || mt != &proto_mtime,
                    None => true,
                };
                if proto_changed {
                    *last = Some((active_section.clone(), proto_mtime));
                    drop(last);
                    if let Some(window) = app_handle.get_webview_window("main") {
                        let _ = window.emit("protocol_changed", serde_json::json!({
                            "section": active_section,
                            "source": "watcher",
                        }));
                    }
                }
            }

            // V3 Phase 3 (rule 4) — floor-time watchdog. If Assembly Line is
            // active and the current speaker has been idle (no working activity
            // in sessions.json) for more than 2× threshold_ms, release the mic
            // so the line doesn't wedge on a stalled seat. Spec rule 4 calls
            // for last_drafting_at_ms-aware auto-extend; this MINIMAL form uses
            // sessions.json:last_working_at as the proxy because it's already
            // updated by every keep-alive hook tick (PreToolUse + PostToolUse
            // installed in setup_claude_code_integration). 2× multiplier so a
            // freshly-grabbed mic isn't insta-released by lingering pre-mic
            // standby state.
            check_assembly_floor_watchdog(&dir, &active_section, &proto_path, &app_handle);

            // Two-controls v1 (consolidated finding #4 / spec §85-91): mode-
            // aware dead-seat handling for hand_raise and moderator mic_passing
            // modes. Independent of the current_speaker stall handler above;
            // these check the queued/designated seats whose stale heartbeats
            // would wedge the team.
            check_two_controls_dead_seats(&dir, &active_section, &proto_path, &app_handle);

            // SHA-CR.tauri-tick — per architect msg 2627 RULING 1 + tester msg
            // 2618 finding. Wall-clock backstop for continuous-review auto-close.
            // Sidecar's own opportunistic sweeper (vaak-mcp.rs:2018) only fires
            // when a sidecar is actively polling; silent rooms orphan the timer.
            // This 1s Tauri tick closes the gap structurally — independent of
            // sidecar liveness. Returns false fast when no continuous round is
            // active (typical case); the read+gate sequence is bounded by a
            // single board.lock acquire + small JSON read.
            collab::auto_close_timed_out_review_round(&dir);
        }
    });
}

/// V3 Phase 3.1 (architect msg 134) — absolute ceiling on how long any
/// single speaker can hold the mic regardless of working-activity. Without
/// this cap, a speaker stuck in a tool-calling loop (cargo build that never
/// finishes, polling for a never-firing condition, Read of an enormous file)
/// keeps last_working_at fresh and the watchdog never fires — infinite hold.
/// 5 minutes is the floor of "this is genuinely stuck, not just slow."
const ASSEMBLY_MAX_FLOOR_SECS: u64 = 300;

fn check_assembly_floor_watchdog(
    dir: &str,
    active_section: &str,
    proto_path: &std::path::Path,
    app_handle: &tauri::AppHandle,
) {
    let proto: serde_json::Value = match std::fs::read_to_string(proto_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
    {
        Some(p) => p,
        None => return,
    };
    if proto.get("preset").and_then(|p| p.as_str()) != Some("Assembly Line") {
        return;
    }
    let floor = match proto.get("floor") {
        Some(f) => f,
        None => return,
    };
    let speaker = match floor.get("current_speaker").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => return,
    };
    // moderator-authority Item 1 (spec line 28-29 / evil-arch msg 1601 #2):
    // skip FLOOR-time stall check when the current_speaker IS the designated
    // moderator. Moderator going silent on the floor is NORMAL — they're
    // managing, not participating. HEARTBEAT-stale check stays in
    // check_two_controls_dead_seats (separate function, fires on
    // sessions.json:last_alive_at_ms staleness > 120s) — that's the legit
    // auto-recovery path for a truly-dead moderator.
    let mic_passing_mode = floor
        .get("mic_passing_mode")
        .and_then(|v| v.as_str())
        .unwrap_or("rotation");
    let moderator = floor
        .get("moderator")
        .and_then(|v| v.as_str());
    if mic_passing_mode == "moderator" && moderator == Some(speaker.as_str()) {
        return;
    }
    let threshold_ms = floor
        .get("threshold_ms")
        .and_then(|v| v.as_u64())
        .unwrap_or(60_000);
    // v1.5.1 commit 2: dynamic stall threshold. If the current speaker
    // declared `expected_duration_secs` via mic_claim, the watchdog respects
    // that declaration up to the 600s hard cap (evil-architect msg 875). If
    // unset (back-compat for unclaimed mics during rollout), falls back to
    // a default that matches Layer 1's freshness window — 180s (was 120s).
    // feature/watchdog-rpc-liveness Finding 3 (dev:1 msg 1286 / dev-challenger
    // msg 1199): the prior 120s default created a 60s band where Layer 1 said
    // "alive" but the assembly watchdog said "stalled" — false floor_stall
    // releases on actively-working agents. Now both agree.
    let stall_threshold_secs = floor
        .get("expected_duration_secs")
        .and_then(|v| v.as_u64())
        .filter(|secs| (30..=600).contains(secs))
        .unwrap_or((threshold_ms / 1000) * 3);

    // Compare against sessions.json:last_working_at for the speaker.
    let sessions_path = std::path::Path::new(dir).join(".vaak").join("sessions.json");
    let sessions: serde_json::Value = match std::fs::read_to_string(&sessions_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
    {
        Some(s) => s,
        None => return,
    };
    let mut parts = speaker.splitn(2, ':');
    let speaker_role = parts.next().unwrap_or("");
    let speaker_inst: u64 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let last_working_iso = sessions
        .get("bindings")
        .and_then(|b| b.as_array())
        .and_then(|arr| {
            arr.iter().find(|b| {
                b.get("role").and_then(|r| r.as_str()) == Some(speaker_role)
                    && b.get("instance").and_then(|i| i.as_u64()).unwrap_or(0) == speaker_inst
            })
        })
        .and_then(|b| b.get("last_working_at").and_then(|v| v.as_str()))
        .map(String::from);
    let last_working_secs = match last_working_iso.as_ref().and_then(|s| collab::parse_iso_epoch_pub(s)) {
        Some(s) => s,
        None => return, // no working timestamp — don't release on missing data
    };
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let idle_secs = now_secs.saturating_sub(last_working_secs);

    // Also gate on rev_at — if the protocol has been mutated recently, the
    // mic may have just landed and the speaker hasn't had a full floor cycle
    // yet. Don't release a mic that hasn't held its floor at least once.
    let rev_at_secs = proto
        .get("rev_at")
        .and_then(|v| v.as_str())
        .and_then(collab::parse_iso_epoch_pub)
        .unwrap_or(0);
    let rev_age_secs = now_secs.saturating_sub(rev_at_secs);
    if rev_age_secs < stall_threshold_secs {
        return;
    }

    // V1.0.6 (human msg 567, 2026-05-13): watchdog must distinguish stalled
    // from working-but-mid-tool-call. The agent's per-seat session file
    // `.vaak/sessions/<role>-<n>.json` records `last_alive_at_ms` and bumps
    // it on every MCP tool call — distinct from `bindings[i].last_working_at`
    // (which only updates on activity-state transitions) and from `rev_at`
    // (which only updates on protocol mutations, so a long-running tool call
    // that doesn't mutate protocol.json leaves rev_at stale even while the
    // agent is actively working).
    //
    // Read the per-seat file and compute heartbeat freshness in ms. If the
    // speaker is heartbeating within the WORKING_HEARTBEAT_FRESH window,
    // suppress the max_floor_exceeded release — they're not stuck in a
    // tool-loop, they're working productively. floor_stall is unaffected
    // because that already gates on last_working_at which bumps via the
    // existing assembly send path.
    const WORKING_HEARTBEAT_FRESH_MS: u64 = 30_000;
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let seat_file = std::path::Path::new(dir)
        .join(".vaak")
        .join("sessions")
        .join(format!("{}-{}.json", speaker_role, speaker_inst));
    // VAAK_FP:Mit12:main.rs:watchdog_uses_last_active_not_last_alive
    // MW10 silent-team-death fix (dev-challenger:0 msg 1738/1771, human msg
    // 1770 escalation): the prior heartbeat_fresh check read last_alive_at_ms
    // which is updated on every project_wait KEEPALIVE — so a sidecar that
    // is alive-but-agent-loop-dead (Claude Code session silently dropped,
    // sidecar still firing MCP keepalives) showed heartbeat_fresh=true and
    // the watchdog suppressed the release. Result: silent-team-death every
    // few hours, human had to manually buzz to wake the team.
    //
    // Fix (Mit 1+2 partial per dev-challenger:0 msg 1738): prefer
    // last_active_at_ms (written by PreToolUse/PostToolUse hooks — real
    // agent work, not just MCP-loop keepalive ticks). Fall back to
    // last_alive_at_ms when last_active_at_ms is absent (legacy seats
    // pre-Mit12 ship) so backward-compat holds during the migration window.
    let seat_state: Option<serde_json::Value> = std::fs::read_to_string(&seat_file)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok());
    let speaker_active_ms = seat_state
        .as_ref()
        .and_then(|v| v.get("last_active_at_ms").and_then(|x| x.as_u64()))
        .unwrap_or(0);
    let speaker_alive_ms_fallback = seat_state
        .as_ref()
        .and_then(|v| v.get("last_alive_at_ms").and_then(|x| x.as_u64()))
        .unwrap_or(0);
    // Prefer agent-loop signal (last_active_at_ms) over MCP-loop signal
    // (last_alive_at_ms) — the former actually proves the agent is processing
    // work, the latter only proves the sidecar process is running.
    let speaker_alive_ms = if speaker_active_ms > 0 { speaker_active_ms } else { speaker_alive_ms_fallback };
    let heartbeat_age_ms = if speaker_alive_ms > 0 {
        now_ms.saturating_sub(speaker_alive_ms)
    } else {
        u64::MAX // no heartbeat data → treat as stale, fall through to release
    };
    let heartbeat_fresh = heartbeat_age_ms < WORKING_HEARTBEAT_FRESH_MS;

    // Two release reasons (V3 spec rule 4 + Phase 3.1 ceiling, v1.0.6 heartbeat gate).
    //   - max_floor: speaker has held the mic past the absolute ceiling AND
    //     their per-seat heartbeat is stale (last MCP tool call > 30s ago).
    //     If heartbeat is fresh, they're working through a long tool call;
    //     extend grace rather than kick.
    //   - stall: working-activity has been silent past 2× threshold (governed
    //     by last_working_at, unaffected by the new heartbeat gate).
    let (release_reason, release_detail) = if rev_age_secs > ASSEMBLY_MAX_FLOOR_SECS && !heartbeat_fresh {
        (
            "max_floor_exceeded",
            format!(
                "held mic {}s past absolute max floor of {}s with heartbeat stale {}ms — presumed stuck in a tool loop",
                rev_age_secs, ASSEMBLY_MAX_FLOOR_SECS, heartbeat_age_ms
            ),
        )
    } else if idle_secs > stall_threshold_secs && !heartbeat_fresh {
        // feature/watchdog-rpc-liveness (developer:1 msg 1286 Finding 2 +
        // dev:0 msg 1211 symmetry): apply the same heartbeat_fresh gate that
        // max_floor_exceeded uses. Closes the dual-tracker false-positive
        // class — agents doing silent tool-call work (Read/Edit/Bash via
        // PreToolUse/PostToolUse hooks + Signal A RPC heartbeat in vaak-mcp.rs)
        // bump last_alive_at_ms; floor_stall only fires when BOTH the
        // working-activity track AND the heartbeat track are stale.
        //
        // Commit T (strict-turn-discipline spec §Working-turn unbounded
        // mic-hold): suppress floor_stall when current_speaker holds a
        // working-turn AND review_intensity >= 5. Working agents on the
        // mic for long stretches (writing code, drafting prose) shouldn't
        // be auto-rotated; the unbounded hold expires only on explicit
        // yield_to or heartbeat-truly-stale (max_floor_exceeded branch
        // above still fires when heartbeat is also stale).
        let turn_type = proto
            .get("floor")
            .and_then(|f| f.get("turn_type"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let review_intensity = proto
            .get("floor")
            .and_then(|f| f.get("review_intensity"))
            .and_then(|v| v.as_u64())
            .unwrap_or(5) as u8;
        if should_suppress_floor_stall(turn_type, review_intensity) {
            return;
        }
        (
            "floor_stall",
            format!(
                "no working activity in {}s (stall threshold: {}s) AND heartbeat stale {}ms",
                idle_secs, stall_threshold_secs, heartbeat_age_ms
            ),
        )
    } else {
        return;
    };

    // Release: write protocol.json with current_speaker=null, bump rev, post
    // mic_released event to board.jsonl. Same direct-write pattern as the
    // auto-grab in handle_project_send (vaak-mcp.rs).
    let mut current = proto.clone();
    if let Some(floor_obj) = current.get_mut("floor").and_then(|f| f.as_object_mut()) {
        floor_obj.insert("current_speaker".to_string(), serde_json::Value::Null);
    }
    let cur_rev = current.get("rev").and_then(|v| v.as_u64()).unwrap_or(0);
    if let Some(rev_field) = current.get_mut("rev") {
        *rev_field = serde_json::json!(cur_rev + 1);
    }
    let now_iso = iso_now();
    if let Some(obj) = current.as_object_mut() {
        obj.insert("last_writer_seat".to_string(), serde_json::json!("watchdog"));
        obj.insert("last_writer_action".to_string(), serde_json::json!(release_reason));
        obj.insert("rev_at".to_string(), serde_json::json!(now_iso));
    }
    if let Err(e) = std::fs::write(
        proto_path,
        serde_json::to_string_pretty(&current).unwrap_or_default(),
    ) {
        log_error(&format!("[watchdog] protocol.json write failed: {}", e));
        return;
    }

    // Append mic_released event to board.jsonl so the team sees who lost it
    // and why. Best-effort — failure logs but doesn't roll back the release.
    let board_path = collab::active_board_path(dir);
    let count = std::fs::read_to_string(&board_path)
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .count();
    let event = serde_json::json!({
        "id": (count + 1) as u64,
        "from": "system",
        "to": "all",
        "type": "mic_released",
        "timestamp": now_iso,
        "subject": format!("[mic_released] {} — {}", speaker, release_reason),
        "body": format!(
            "Watchdog released the mic from {}: {}. Mic is now free — next sender will auto-grab.",
            speaker, release_detail
        ),
        "metadata": {
            "from_speaker": speaker,
            "idle_secs": idle_secs,
            "stall_threshold_secs": stall_threshold_secs,
            "max_floor_secs": ASSEMBLY_MAX_FLOOR_SECS,
            "rev_age_secs": rev_age_secs,
            "section": active_section,
            "reason": release_reason,
        }
    });
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&board_path) {
        use std::io::Write;
        let _ = writeln!(f, "{}", serde_json::to_string(&event).unwrap_or_default());
    }

    log_error(&format!(
        "[watchdog] released mic from {} — reason={}, idle={}s, rev_age={}s, section={}",
        speaker, release_reason, idle_secs, rev_age_secs, active_section
    ));

    // SHA-11.1: mark the released seat in sessions.json with a watchdog-
    // release timestamp. The auto-grab logic in vaak-mcp.rs reads this and
    // enforces a cooldown (default 600s) before the same seat is allowed
    // to grab the mic again. Closes the zombie-seat-grabs-its-own-mic loop
    // — ui-architect:0 burned 12+ watchdog cycles tonight, each followed
    // immediately by an auto-grab of itself. Cooldown forces the mic to
    // go to a different sender during the recovery window.
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    if let Some(mut sessions_val) = std::fs::read_to_string(&sessions_path)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
    {
        let mut wrote = false;
        if let Some(arr) = sessions_val.get_mut("bindings").and_then(|b| b.as_array_mut()) {
            for b in arr.iter_mut() {
                let matches = b.get("role").and_then(|r| r.as_str()) == Some(speaker_role)
                    && b.get("instance").and_then(|i| i.as_u64()).unwrap_or(0) == speaker_inst;
                if matches {
                    if let Some(obj) = b.as_object_mut() {
                        obj.insert(
                            "last_watchdog_release_at_ms".to_string(),
                            serde_json::json!(now_ms),
                        );
                        wrote = true;
                    }
                }
            }
        }
        if wrote {
            if let Err(e) = std::fs::write(
                &sessions_path,
                serde_json::to_string_pretty(&sessions_val).unwrap_or_default(),
            ) {
                log_error(&format!("[watchdog] sessions.json cooldown-stamp write failed: {}", e));
            }
        }
    }

    // Push protocol_changed so UI refreshes immediately rather than waiting
    // for the file-watch tick to notice.
    if let Some(window) = app_handle.get_webview_window("main") {
        let _ = window.emit(
            "protocol_changed",
            serde_json::json!({"section": active_section, "source": "watchdog"}),
        );
    }
}

/// Two-controls v1 dead-seat watchdog (consolidated finding #4 / spec §85-91).
/// Independent of the current_speaker stall path. Two responsibilities:
///
///   - **hand_raise mode:** if the head of `floor.hand_queue` has a stale
///     heartbeat (last_alive_at_ms > stall threshold), strip that seat from
///     the queue atomically and emit `hand_dequeued` so observers see the
///     reason. Without this, a disconnected raised-hand seat blocks all
///     subsequent grants.
///
///   - **moderator mode:** if `floor.moderator` has a stale heartbeat OR the
///     designated moderator seat is no longer active in sessions.json, auto-
///     promote `mic_passing_mode` to `rotation` with a `mic_mechanism_promoted`
///     event so the team isn't locked indefinitely waiting on a vacant
///     moderator. One-way for the current floor; human can re-set moderator
///     after the situation resolves (spec §93).
///
/// Both checks reuse the existing direct-write pattern in
/// check_assembly_floor_watchdog (read protocol.json, mutate, write, append
/// board event). Lives in the same poll loop so cadence is identical.
/// Commit T (strict-turn-discipline spec §Working-turn unbounded mic-hold).
/// Returns true when a current working-turn at review_intensity >= 5
/// should NOT have floor_stall fire against it. Pure helper for unit
/// test surface (T1/T2 fixtures).
fn should_suppress_floor_stall(turn_type: &str, review_intensity: u8) -> bool {
    turn_type == "working" && review_intensity >= 5
}

fn check_two_controls_dead_seats(
    dir: &str,
    active_section: &str,
    proto_path: &std::path::Path,
    app_handle: &tauri::AppHandle,
) {
    let proto: serde_json::Value = match std::fs::read_to_string(proto_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
    {
        Some(p) => p,
        None => return,
    };
    // Only run when assembly is active — in simultaneous mode there's no
    // mic-passing state to police.
    let assembly_active = proto
        .get("floor")
        .and_then(|f| f.get("assembly_active"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if !assembly_active {
        return;
    }
    let mode = proto
        .get("floor")
        .and_then(|f| f.get("mic_passing_mode"))
        .and_then(|v| v.as_str())
        .unwrap_or("rotation")
        .to_string();
    if mode == "rotation" {
        return; // current_speaker stall handler covers rotation.
    }

    // Two-source freshness threshold (matches check_assembly_floor_watchdog
    // semantics): consider stale if no last_alive heartbeat in the last
    // ALIVE_STATE_STALE_MS (120s). Single source of truth via collab module.
    const DEAD_SEAT_THRESHOLD_MS: u64 = collab::staleness_thresholds::ALIVE_STATE_STALE_MS;
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let seat_alive_ms = |seat_label: &str| -> u64 {
        let mut parts = seat_label.splitn(2, ':');
        let role = parts.next().unwrap_or("");
        let inst: u64 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
        let seat_file = std::path::Path::new(dir)
            .join(".vaak")
            .join("sessions")
            .join(format!("{}-{}.json", role, inst));
        std::fs::read_to_string(&seat_file)
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .and_then(|v| v.get("last_alive_at_ms").and_then(|x| x.as_u64()))
            .unwrap_or(0)
    };

    if mode == "hand_raise" {
        let head = proto
            .get("floor")
            .and_then(|f| f.get("hand_queue"))
            .and_then(|q| q.as_array())
            .and_then(|arr| arr.first())
            .and_then(|v| v.as_str())
            .map(String::from);
        let head = match head {
            Some(s) => s,
            None => return, // empty queue
        };
        let head_alive = seat_alive_ms(&head);
        let head_stale = head_alive == 0
            || now_ms.saturating_sub(head_alive) > DEAD_SEAT_THRESHOLD_MS;
        if !head_stale {
            return;
        }
        let mut current = proto.clone();
        if let Some(floor_obj) = current.get_mut("floor").and_then(|f| f.as_object_mut()) {
            if let Some(queue) = floor_obj
                .get_mut("hand_queue")
                .and_then(|q| q.as_array_mut())
            {
                queue.retain(|v| v.as_str() != Some(&head));
            }
        }
        write_protocol_emit_two_controls_event(
            dir,
            proto_path,
            active_section,
            &mut current,
            "hand_dequeued",
            serde_json::json!({
                "seat": head.clone(),
                "reason": "stale_heartbeat",
            }),
            app_handle,
        );
        return;
    }

    if mode == "moderator" {
        let moderator = proto
            .get("floor")
            .and_then(|f| f.get("moderator"))
            .and_then(|v| v.as_str())
            .map(String::from);
        let mod_label = match moderator {
            Some(s) => s,
            None => {
                // No moderator designated → auto-promote to rotation now.
                let mut current = proto.clone();
                if let Some(floor_obj) =
                    current.get_mut("floor").and_then(|f| f.as_object_mut())
                {
                    floor_obj.insert(
                        "mic_passing_mode".to_string(),
                        serde_json::json!("rotation"),
                    );
                }
                write_protocol_emit_two_controls_event(
                    dir,
                    proto_path,
                    active_section,
                    &mut current,
                    "mic_mechanism_promoted",
                    serde_json::json!({
                        "from": "moderator",
                        "to": "rotation",
                        "reason": "moderator_vacant",
                    }),
                    app_handle,
                );
                return;
            }
        };
        // Bug 2 fix (per tester msg 1742 + architect msg 1745, supersedes
        // 57251b1's human:0-only skip): widen the staleness-skip to ALL
        // moderators, not just the human. Original logic auto-promoted
        // mic_passing_mode away from "moderator" whenever the moderator
        // seat's last_alive_at_ms went stale (>120s) — but moderators are
        // EXPECTED to go silent during the work they're moderating
        // (managing the pipeline, not participating in it). AI moderators
        // were hitting the same auto-promote class as the human (which
        // 57251b1 skipped via mod_label == "human:0"). evil-arch as
        // moderator at human msg 1713 hit this exactly: system msg 1720
        // mic_mechanism_promoted=moderator_stale fired right after they
        // were set, locking them out of moderation.
        //
        // Trade-off: a truly-dead AI moderator no longer auto-recovers via
        // mode-promotion. Recovery path is: (a) human re-sets moderator,
        // OR (b) Layer 2 supervisor process-kill auto-restarts the dead
        // session, OR (c) the human flips mic_passing_mode back to rotation
        // manually. Per architect msg 1745 trade analysis, this is the
        // right call — the auto-promote was breaking legitimate moderation
        // more often than it was rescuing dead moderators.
        // Suppress unused-variable warning for mod_label since it's now only
        // used by the moderator-vacant branch above.
        let _ = mod_label;
        return;
    }
}

/// Shared write+emit helper for the two-controls dead-seat watchdog. Mirrors
/// the inline pattern in check_assembly_floor_watchdog: bump rev, stamp audit,
/// write protocol.json, append board event, push protocol_changed for UI.
fn write_protocol_emit_two_controls_event(
    dir: &str,
    proto_path: &std::path::Path,
    active_section: &str,
    current: &mut serde_json::Value,
    event_type: &str,
    extra_metadata: serde_json::Value,
    app_handle: &tauri::AppHandle,
) {
    let cur_rev = current.get("rev").and_then(|v| v.as_u64()).unwrap_or(0);
    if let Some(rev_field) = current.get_mut("rev") {
        *rev_field = serde_json::json!(cur_rev + 1);
    }
    let now_iso = iso_now();
    if let Some(obj) = current.as_object_mut() {
        obj.insert("last_writer_seat".to_string(), serde_json::json!("watchdog"));
        obj.insert(
            "last_writer_action".to_string(),
            serde_json::json!(event_type),
        );
        obj.insert("rev_at".to_string(), serde_json::json!(now_iso.clone()));
    }
    if let Err(e) = std::fs::write(
        proto_path,
        serde_json::to_string_pretty(&current).unwrap_or_default(),
    ) {
        log_error(&format!(
            "[two-controls-watchdog] protocol.json write failed: {}",
            e
        ));
        return;
    }

    let board_path = collab::active_board_path(dir);
    let count = std::fs::read_to_string(&board_path)
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .count();
    let mut metadata = serde_json::Map::new();
    if let Some(obj) = extra_metadata.as_object() {
        for (k, v) in obj {
            metadata.insert(k.clone(), v.clone());
        }
    }
    metadata.insert("section".into(), serde_json::json!(active_section));
    metadata.insert("ts".into(), serde_json::json!(now_iso.clone()));
    let event = serde_json::json!({
        "id": (count + 1) as u64,
        "from": "system",
        "to": "all",
        "type": event_type,
        "timestamp": now_iso,
        "subject": format!("[{}] system", event_type),
        "body": format!("two-controls watchdog emitted {}", event_type),
        "metadata": serde_json::Value::Object(metadata),
    });
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&board_path)
    {
        use std::io::Write;
        let _ = writeln!(f, "{}", serde_json::to_string(&event).unwrap_or_default());
    }

    if let Some(window) = app_handle.get_webview_window("main") {
        let _ = window.emit(
            "protocol_changed",
            serde_json::json!({"section": active_section, "source": "two-controls-watchdog"}),
        );
    }
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

            // Ensure voice-settings.json exists with defaults on first launch
            // (required for MCP hook to read auto_collab, blind_mode, etc.)
            if let Some(settings_path) = get_voice_settings_path() {
                if !settings_path.exists() {
                    log_error("[setup] voice-settings.json not found — creating with defaults");
                    let defaults = VoiceSettings::default();
                    if let Some(parent) = settings_path.parent() {
                        let _ = fs::create_dir_all(parent);
                    }
                    if let Ok(json) = serde_json::to_string_pretty(&defaults) {
                        if let Err(e) = fs::write(&settings_path, json) {
                            log_error(&format!("[setup] Failed to create voice-settings.json: {}", e));
                        }
                    }
                }
            }

            // Start the speak server for Claude Code integration
            start_speak_server(app.handle().clone());

            // Initialize collab change notifier (single thread, coalesced notifications)
            init_collab_notifier();

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

            // Collaborate v2: window is now created LAZILY on first show
            // (collab_v2.rs ensure_collaborate_v2_window) to avoid spawning its
            // WebView2 at startup (PERF, human msg 569). The close→hide handler
            // (spec §20: closing hides instead of destroying so the launcher can
            // reopen it) is attached at creation time there, so the prior
            // startup-time handler block here is no longer needed.

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_last_project_path,
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
            check_sidecar_status,
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
            get_assembly_state,
            set_assembly_state,
            // Protocol v6 (Slice 3+4) — read + UI-side mutate
            get_protocol_cmd,
            protocol_mutate_cmd,
            // Collaborative-proposal-workflow v1 (Commit Q.B) — moderator's
            // informational dismiss event for board audit trail.
            emit_replanning_dismissed_cmd,
            // Commit D — delegation-block parser for Affordance B chart.
            parse_delegation_blocks_cmd,
            // Slice 9 — health pill (resilience-stack JOIN)
            get_resilience_status,
            // Two-controls B.4.1 — active seats for moderator picker
            list_active_seats_cmd,
            get_currency_balances_cmd,
            currency_human_adjust_cmd,
            read_economy_settings_cmd,
            write_economy_settings_cmd,
            oxford_initiate_cmd,
            oxford_end_cmd,
            delphi_initiate_cmd,
            delphi_get_state_cmd,
            delphi_open_round_cmd,
            delphi_submit_cmd,
            delphi_close_round_cmd,
            delphi_end_cmd,
            read_active_oxford_cmd,
            read_currency_events_stream,
            read_currency_feed_cmd,
            read_disputes_cmd,
            read_bounties_cmd,
            read_currency_history_cmd,
            set_currency_enabled,
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
            launcher::check_anthropic_key,
            launcher::launch_team_member,
            launcher::launch_team,
            launcher::kill_team_member,
            launcher::kill_all_team_members,
            launcher::clear_ghost_seat_rows,
            launcher::get_spawned_agents,
            launcher::get_role_companions,
            launcher::repopulate_spawned,
            launcher::focus_agent_window,
            launcher::buzz_agent_terminal,
            launcher::check_macos_permissions,
            launcher::open_macos_settings,
            launcher::open_url_in_browser,
            launcher::open_terminal_in_dir,
            launcher::check_npm_installed,
            launcher::check_homebrew_installed,
            launcher::install_nodejs,
            launcher::install_claude_cli,
            // Decision Panel v1 — per the 6 adversarial flags (msgs 4784/4787/4789/4811)
            list_decision_resolutions_cmd,
            resolve_decision_cmd,
            cancel_decision_cmd,
            // Collaborate v2 commands (P1: standalone window + static roster)
            collab_v2::show_collaborate_v2_window,
            collab_v2::toggle_collaborate_v2_window,
            collab_v2::hide_collaborate_v2_window,
            collab_v2::get_v2_seats,
        ]);

    match builder.build(tauri::generate_context!()) {
        Ok(app) => {
            app.run(|_app_handle, event| {
                // Signal HTTP server to shut down when the app exits
                if let tauri::RunEvent::Exit = event {
                    HTTP_SERVER_SHUTDOWN.store(true, Ordering::Relaxed);
                    eprintln!("[main] App exiting — HTTP server shutdown signaled");
                    // Phase 7 (a) — end-of-session currency snapshot. RunEvent::Exit
                    // fires ONCE on full app exit (sidesteps the per-window
                    // CloseRequested multi-fire ambiguity tester:0 flagged). Writes
                    // one .vaak/currency-history/<date>-NNN.json for the active
                    // project. Best-effort: a failure must not block exit.
                    if let Some(dir) = get_last_project_path() {
                        if collab::currency::currency_jsonl_path(&dir).exists() {
                            match collab::with_currency_and_board_lock(&dir, || {
                                collab::currency::write_session_snapshot(&dir)
                            }) {
                                Ok(p) => eprintln!("[main] session snapshot written: {:?}", p),
                                Err(e) => eprintln!("[main] session snapshot error: {}", e),
                            }
                        }
                    }
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

    let zenity_ok = zenity.as_ref().map(|o| o.status.success()).unwrap_or(false);
    if !zenity_ok {
        let kdialog = Command::new("kdialog")
            .args(["--error", message, "--title", "Vaak Error"])
            .output();

        let kdialog_ok = kdialog.as_ref().map(|o| o.status.success()).unwrap_or(false);
        if !kdialog_ok {
            // Last resort: notification
            let _ = Command::new("notify-send")
                .args(["Vaak Error", message])
                .output();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ============================================================
    // Commit D — parse_delegation_blocks (R7 well-formedness + R8 vacant-
    // owner detection precondition). Spec lines 286-291 + 102 (regex
    // grammar). Hand-rolled parser matches non-greedy semantics for
    // hyphenated values (ui-architect:0, after-pilot, V.A-1, etc.).
    // ============================================================

    /// R7 — well-formed single block parses owner + section + optional
    /// fields.
    #[test]
    fn r7_lenient_parses_single_well_formed_block() {
        let plan = r#"# Plan
Some prose.
<!-- delegation: owner=architect:0 section=I deadline=execution-phase deps=II,III -->
More prose.
"#;
        let entries = parse_delegation_blocks_lenient(plan);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].owner, "architect:0");
        assert_eq!(entries[0].section, "I");
        assert_eq!(entries[0].deadline.as_deref(), Some("execution-phase"));
        assert_eq!(entries[0].deps, vec!["II", "III"]);
    }

    /// R7 — multiple blocks per file parse independently.
    #[test]
    fn r7_lenient_parses_multiple_blocks() {
        let plan = r#"
<!-- delegation: owner=architect:0 section=I -->
<!-- delegation: owner=ux-engineer:0 section=IV.A deps=II -->
<!-- delegation: owner=ui-architect:0 section=IV.B -->
"#;
        let entries = parse_delegation_blocks_lenient(plan);
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[1].owner, "ux-engineer:0");
        assert_eq!(entries[2].owner, "ui-architect:0");
    }

    /// R7 — platform-eng msg 1916 #2 hyphen-tolerance: section IDs and
    /// deadline values containing `-` must NOT prematurely close the
    /// match. Naive `[^-]+` pattern would break here.
    #[test]
    fn r7_lenient_handles_hyphenated_values() {
        let plan = r#"
<!-- delegation: owner=ui-architect:0 section=V.A-1 deadline=after-pilot deps=II-a,III-b -->
"#;
        let entries = parse_delegation_blocks_lenient(plan);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].owner, "ui-architect:0");
        assert_eq!(entries[0].section, "V.A-1");
        assert_eq!(entries[0].deadline.as_deref(), Some("after-pilot"));
        assert_eq!(entries[0].deps, vec!["II-a", "III-b"]);
    }

    /// R7 — lenient parser silently drops malformed blocks (missing
    /// owner) per spec §parse_delegation_blocks line 166.
    #[test]
    fn r7_lenient_drops_block_missing_owner() {
        let plan = r#"
<!-- delegation: owner=architect:0 section=I -->
<!-- delegation: section=II deadline=after-pilot -->
<!-- delegation: owner=tester:0 section=III -->
"#;
        let entries = parse_delegation_blocks_lenient(plan);
        // Three blocks total; middle one drops silently.
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].section, "I");
        assert_eq!(entries[1].section, "III");
    }

    /// R7 — lenient parser handles missing close marker gracefully (no
    /// infinite loop, no panic, just bails out at the malformed point).
    #[test]
    fn r7_lenient_bails_on_unclosed_comment() {
        let plan = r#"
<!-- delegation: owner=architect:0 section=I -->
<!-- delegation: owner=tester:0 section=II
"#;
        // Should parse the first block, then bail.
        let entries = parse_delegation_blocks_lenient(plan);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].section, "I");
    }

    /// R7 — non-delegation comments (scope:, delegation-target:) are
    /// skipped without confusing the scanner.
    #[test]
    fn r7_lenient_ignores_non_delegation_comments() {
        let plan = r#"
<!-- scope: src/foo.rs src/bar.rs -->
<!-- delegation: owner=architect:0 section=I -->
<!-- delegation-target: section=I -->
<!-- some other comment -->
<!-- delegation: owner=ux-engineer:0 section=II -->
"#;
        let entries = parse_delegation_blocks_lenient(plan);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].owner, "architect:0");
        assert_eq!(entries[1].owner, "ux-engineer:0");
    }

    /// R7 — strict parser returns ParseError list on malformed input.
    /// Distinct from lenient which drops silently.
    #[test]
    fn r7_strict_reports_missing_owner_error() {
        let plan = r#"
<!-- delegation: section=II deadline=after-pilot -->
"#;
        let result = parse_delegation_blocks_strict(plan);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert_eq!(errors.len(), 1);
        assert!(matches!(errors[0], ParseError::MissingOwner { .. }));
    }

    /// R7 — strict parser detects duplicate-section drift per spec
    /// line 115. Lenient parser does NOT (it just accepts both, last
    /// occurrence wins via insertion order).
    #[test]
    fn r7_strict_detects_duplicate_section_drift() {
        let plan = r#"
<!-- delegation: owner=architect:0 section=I -->
<!-- delegation: owner=ux-engineer:0 section=I -->
"#;
        let result = parse_delegation_blocks_strict(plan);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors.iter().any(|e| matches!(
            e,
            ParseError::DuplicateSection { section, .. } if section == "I"
        )));
    }

    /// R7 — strict parser returns Ok with entries when all blocks
    /// well-formed and sections unique.
    #[test]
    fn r7_strict_ok_on_well_formed_input() {
        let plan = r#"
<!-- delegation: owner=architect:0 section=I -->
<!-- delegation: owner=ux-engineer:0 section=IV.A deps=II -->
"#;
        let result = parse_delegation_blocks_strict(plan);
        assert!(result.is_ok());
        let entries = result.unwrap();
        assert_eq!(entries.len(), 2);
    }

    /// R8 — vacant-owner detection: the parser itself doesn't know
    /// which seats are active (that's the team_status integration at
    /// the consumer layer). This test verifies the parsed `owner`
    /// field is the raw seat slug that the Affordance B chart will
    /// cross-check against active_seats in the UI.
    #[test]
    fn r8_owner_field_preserves_raw_seat_slug() {
        let plan = r#"
<!-- delegation: owner=ghost-role:0 section=I -->
<!-- delegation: owner=architect:0 section=II -->
"#;
        let entries = parse_delegation_blocks_lenient(plan);
        assert_eq!(entries.len(), 2);
        // Affordance B chart layer will mark "ghost-role:0" as
        // (vacant) when it doesn't appear in active_seats. Parser's
        // job is to return the literal owner value.
        assert_eq!(entries[0].owner, "ghost-role:0");
        assert_eq!(entries[1].owner, "architect:0");
    }

    // ── validate_project_dir ───────────────────────────────────────────

    // ============================================================
    // Strict-turn-discipline (Commit T): working-turn unbounded mic-hold
    // T1 working-turn at intensity >= 5 → suppress floor_stall
    // T2 communication-turn (reviewing/passing/thinking) → still releases
    // ============================================================

    #[test]
    fn t1_working_turn_at_level_5_suppresses_floor_stall() {
        assert!(should_suppress_floor_stall("working", 5));
        assert!(should_suppress_floor_stall("working", 6));
        assert!(should_suppress_floor_stall("working", 10));
    }

    #[test]
    fn t1_working_turn_below_level_5_does_not_suppress() {
        assert!(!should_suppress_floor_stall("working", 4));
        assert!(!should_suppress_floor_stall("working", 1));
    }

    #[test]
    fn t2_communication_turns_never_suppress() {
        for turn_type in ["reviewing", "passing", "thinking", ""] {
            assert!(
                !should_suppress_floor_stall(turn_type, 10),
                "turn_type={} at level 10 should NOT suppress",
                turn_type
            );
        }
    }

    #[test]
    fn test_validate_project_dir_path_traversal_rejected() {
        let result = validate_project_dir("/tmp/../etc/passwd");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("path traversal"));
    }

    #[test]
    fn test_validate_project_dir_nonexistent() {
        let result = validate_project_dir("/tmp/nonexistent-vaak-proj-99999");
        assert!(result.is_err(), "non-existent directory should fail");
    }

    #[test]
    fn test_validate_project_dir_no_vaak_subdir() {
        let tmp = std::env::temp_dir().join("vaak-test-validate-no-vaak");
        let _ = std::fs::create_dir_all(&tmp);

        let result = validate_project_dir(tmp.to_str().unwrap());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Not a Vaak project"));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_validate_project_dir_valid() {
        let tmp = std::env::temp_dir().join("vaak-test-validate-ok");
        let _ = std::fs::create_dir_all(tmp.join(".vaak"));

        let result = validate_project_dir(tmp.to_str().unwrap());
        assert!(result.is_ok(), "valid project dir should succeed: {:?}", result);
        let canonical = result.unwrap();
        assert!(!canonical.starts_with("\\\\?\\"), "should strip Windows extended-length prefix");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    // ── initialize_project ─────────────────────────────────────────────

    #[test]
    fn test_initialize_project_creates_structure() {
        let tmp = std::env::temp_dir().join("vaak-test-init-struct");
        let _ = std::fs::remove_dir_all(&tmp); // clean slate
        std::fs::create_dir_all(&tmp).unwrap();

        let config = serde_json::json!({
            "project_id": "test-001",
            "name": "Test Project",
            "roles": {
                "architect": {"title": "Architect"},
                "developer": {"title": "Developer"}
            },
            "settings": {
                "heartbeat_timeout_seconds": 300
            }
        });

        let result = initialize_project(
            tmp.to_str().unwrap().to_string(),
            config.to_string(),
        );
        assert!(result.is_ok(), "initialize_project should succeed: {:?}", result);

        // Verify directory structure
        assert!(tmp.join(".vaak").is_dir(), ".vaak/ should exist");
        assert!(tmp.join(".vaak/roles").is_dir(), ".vaak/roles/ should exist");
        assert!(tmp.join(".vaak/last-seen").is_dir(), ".vaak/last-seen/ should exist");
        assert!(tmp.join(".vaak/sections").is_dir(), ".vaak/sections/ should exist");

        // Verify project.json was written with pretty formatting
        let pj = std::fs::read_to_string(tmp.join(".vaak/project.json")).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&pj).unwrap();
        assert_eq!(parsed["project_id"].as_str().unwrap(), "test-001");
        assert!(pj.contains('\n'), "project.json should be pretty-printed");

        // Verify sessions.json
        let sj = std::fs::read_to_string(tmp.join(".vaak/sessions.json")).unwrap();
        let sessions: serde_json::Value = serde_json::from_str(&sj).unwrap();
        assert!(sessions["bindings"].as_array().unwrap().is_empty());

        // Verify board.jsonl exists (empty)
        let board = std::fs::read_to_string(tmp.join(".vaak/board.jsonl")).unwrap(); // LINT_EXEMPT_BOARD_PATH: test_code — verifies default-section init writes legacy root path
        assert!(board.is_empty(), "board.jsonl should be empty initially");

        // Verify role briefings
        assert!(tmp.join(".vaak/roles/architect.md").exists());
        assert!(tmp.join(".vaak/roles/manager.md").exists());
        assert!(tmp.join(".vaak/roles/developer.md").exists());
        assert!(tmp.join(".vaak/roles/tester.md").exists());

        // Verify briefing content has reasonable structure
        let arch_brief = std::fs::read_to_string(tmp.join(".vaak/roles/architect.md")).unwrap();
        assert!(arch_brief.starts_with("# Architect"), "architect briefing should start with header");
        assert!(arch_brief.contains("project_send"), "briefing should mention project_send");
        assert!(arch_brief.contains("Workflow Types"), "briefing should contain workflow section");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    // ── set_assembly_state — floor.assembly_active sync (commit 9c8cd0a) ──

    /// Regression lock for the dual-write desync. set_assembly_state (the
    /// launch-row Start/End button's path) mutates the Protocol struct directly
    /// — it does NOT route through apply_set_preset, which is where the MCP
    /// protocol_mutate path syncs assembly_active. Before 9c8cd0a this entry
    /// point set preset + floor.mode but NEVER floor.assembly_active, so a UI
    /// surface reading floor.assembly_active strictly (CollabTab.tsx:5840)
    /// desynced from preset/mode. This locks the invariant
    /// assembly_active == (preset == "Assembly Line") on THIS write path.
    #[test]
    fn set_assembly_state_syncs_assembly_active() {
        let tmp = std::env::temp_dir().join("vaak-test-set-assembly-active");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        initialize_project(
            tmp.to_str().unwrap().to_string(),
            serde_json::json!({
                "project_id": "test-asm",
                "name": "Asm Test",
                "roles": { "developer": {"title": "Developer"} },
                "settings": { "heartbeat_timeout_seconds": 300 }
            })
            .to_string(),
        )
        .unwrap();
        let dir = tmp.to_str().unwrap().to_string();
        let section = collab::get_active_section(&dir);

        // enable → preset, floor.mode, AND assembly_active move together.
        set_assembly_state(dir.clone(), "enable".to_string()).unwrap();
        let proto = protocol::read_protocol_for_section(&dir, &section);
        assert_eq!(proto.preset, "Assembly Line");
        assert_eq!(proto.floor.mode, "round-robin");
        assert_eq!(
            proto.floor.assembly_active,
            Some(true),
            "enable must set floor.assembly_active=Some(true) — the desync 9c8cd0a fixed"
        );

        // disable → all three clear together.
        set_assembly_state(dir.clone(), "disable".to_string()).unwrap();
        let proto = protocol::read_protocol_for_section(&dir, &section);
        assert_eq!(proto.preset, "Default chat");
        assert_eq!(proto.floor.mode, "none");
        assert_eq!(
            proto.floor.assembly_active,
            Some(false),
            "disable must set floor.assembly_active=Some(false)"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_initialize_project_path_traversal_rejected() {
        let result = initialize_project(
            "/tmp/../etc".to_string(),
            "{}".to_string(),
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("path traversal"));
    }

    #[test]
    fn test_initialize_project_invalid_json() {
        let tmp = std::env::temp_dir().join("vaak-test-init-badjson");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let result = initialize_project(
            tmp.to_str().unwrap().to_string(),
            "not valid json!!!".to_string(),
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Invalid config JSON"));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_initialize_project_nonexistent_dir() {
        let result = initialize_project(
            "/tmp/nonexistent-vaak-init-99999".to_string(),
            "{}".to_string(),
        );
        assert!(result.is_err(), "non-existent dir should fail");
    }

    #[test]
    fn test_initialize_project_idempotent() {
        // Running initialize_project twice should succeed (overwrite existing files)
        let tmp = std::env::temp_dir().join("vaak-test-init-idempotent");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let config = serde_json::json!({"project_id": "v1", "name": "First"});
        initialize_project(tmp.to_str().unwrap().to_string(), config.to_string()).unwrap();

        let config2 = serde_json::json!({"project_id": "v2", "name": "Second"});
        let result = initialize_project(tmp.to_str().unwrap().to_string(), config2.to_string());
        assert!(result.is_ok(), "second init should succeed");

        // Verify the second config overwrote the first
        let pj = std::fs::read_to_string(tmp.join(".vaak/project.json")).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&pj).unwrap();
        assert_eq!(parsed["project_id"].as_str().unwrap(), "v2");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    // ── get_sidecar_path ───────────────────────────────────────────────

    #[test]
    fn test_get_sidecar_path_returns_option() {
        // We can't guarantee the sidecar exists in test environment,
        // but we can verify the function doesn't panic
        let result = get_sidecar_path();
        // In dev environment, sidecar usually exists; in CI it might not
        if let Some(path) = result {
            assert!(path.exists(), "if returned, sidecar path should exist");
            let name = path.file_name().unwrap().to_string_lossy();
            assert!(name.contains("vaak-mcp"), "sidecar should contain 'vaak-mcp' in name");
        }
        // If None, that's also valid — sidecar may not be built
    }
}
