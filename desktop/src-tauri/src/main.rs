// Prevents additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod a11y;
mod audio;
mod build_info;
mod collab;
mod database;
mod ethereal;
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

/// Return build identity for host + sidecar (+ ui injected by frontend).
/// Host info is compile-time baked via build.rs; sidecar info comes from
/// spawning `vaak-mcp --build-info` and parsing its stdout. Cached 30s.
#[tauri::command]
fn get_build_info() -> serde_json::Value {
    static CACHE: once_cell::sync::Lazy<Mutex<Option<(std::time::Instant, serde_json::Value)>>> =
        once_cell::sync::Lazy::new(|| Mutex::new(None));
    let mut guard = CACHE.lock();
    if let Some((when, v)) = guard.as_ref() {
        if when.elapsed() < std::time::Duration::from_secs(30) {
            return v.clone();
        }
    }
    let host = build_info::as_json();
    let sidecar = match get_sidecar_path() {
        Some(path) => probe_sidecar_build_info(&path),
        None => build_info_err("binary_missing", None),
    };
    let result = serde_json::json!({ "host": host, "sidecar": sidecar });
    *guard = Some((std::time::Instant::now(), result.clone()));
    result
}

/// Invalidate the cached build-info so the next call re-probes. Called on
/// manual SHA click, window focus, or after pr-sidecar-atomic-swap completes.
#[tauri::command]
fn invalidate_build_info_cache() {
    static CACHE: once_cell::sync::Lazy<Mutex<Option<(std::time::Instant, serde_json::Value)>>> =
        once_cell::sync::Lazy::new(|| Mutex::new(None));
    *CACHE.lock() = None;
}

fn build_info_err(kind: &str, detail: Option<String>) -> serde_json::Value {
    let mut v = serde_json::json!({ "error": kind, "sha": "unknown", "dirty": false });
    if let Some(d) = detail {
        let truncated: String = d.chars().take(512).collect();
        v["detail"] = serde_json::Value::String(truncated);
    }
    v
}

fn probe_sidecar_build_info(path: &std::path::Path) -> serde_json::Value {
    let (tx, rx) = std::sync::mpsc::channel();
    let path_owned = path.to_path_buf();
    std::thread::spawn(move || {
        let mut cmd = std::process::Command::new(&path_owned);
        cmd.arg("--build-info");
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW — suppress console flash on GUI-app spawn
        }
        let _ = tx.send(cmd.output());
    });
    match rx.recv_timeout(std::time::Duration::from_secs(2)) {
        Ok(Ok(output)) => {
            let stdout_str = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr_str = String::from_utf8_lossy(&output.stderr).to_string();
            if !output.status.success() {
                log_error(&format!("Vaak: vaak-mcp --build-info exit={:?} stdout={:?} stderr={:?}",
                    output.status.code(), stdout_str.chars().take(512).collect::<String>(), stderr_str));
                return build_info_err("probe_exited_nonzero", Some(format!("exit={:?}", output.status.code())));
            }
            match serde_json::from_str::<serde_json::Value>(stdout_str.trim()) {
                Ok(v) => v,
                Err(_) => {
                    log_error(&format!("Vaak: vaak-mcp --build-info malformed stdout={:?} stderr={:?}",
                        stdout_str.chars().take(512).collect::<String>(), stderr_str));
                    build_info_err("malformed_output", Some(stdout_str))
                }
            }
        }
        Ok(Err(e)) => build_info_err("probe_failed", Some(e.to_string())),
        Err(_) => build_info_err("probe_timeout", None),
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
    let home = match collab::vaak_home_dir() {
        Ok(h) => h,
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

/// DEPRECATED (pr-r2-data-fields): use `set_session_mode` instead. This
/// alias exists so frontend caches mid-migration keep working. Will be
/// removed in a future cleanup PR.
#[tauri::command]
fn set_discussion_mode(dir: String, discussion_mode: Option<String>) -> Result<(), String> {
    eprintln!("[deprecated] Tauri command `set_discussion_mode` is deprecated — use `set_session_mode` instead.");
    set_session_mode(dir, discussion_mode)
}

/// Toggle the dead-agent watchdog (pr-watchdog-opt-in). The backend reads
/// `settings.watchdog_respawn_dead_agents` from project.json per invocation
/// of `check_and_respawn_dead_agents` (see launcher.rs), so this command
/// takes effect immediately — no app restart required. The frontend
/// `setInterval` that gates on the same flag picks up the new value on
/// next render (dependent on `project` state being re-fetched after write,
/// which happens via `notify_collab_change`).
///
/// Per ux-engineer:0 msg 155: needed to wire the opt-in toggle UI. Uses
/// the same atomic-write + notify pattern as `set_workflow_type` so the
/// read/write contract is consistent across settings mutators.
#[tauri::command]
fn set_watchdog_respawn_enabled(dir: String, enabled: bool) -> Result<(), String> {
    let dir = validate_project_dir(&dir)?;
    let config_path = std::path::Path::new(&dir).join(".vaak").join("project.json");
    let content = std::fs::read_to_string(&config_path)
        .map_err(|e| format!("Failed to read project.json: {}", e))?;
    let mut config: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse project.json: {}", e))?;

    if let Some(settings) = config.get_mut("settings") {
        settings["watchdog_respawn_dead_agents"] = serde_json::Value::Bool(enabled);
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

#[tauri::command]
fn set_session_mode(dir: String, discussion_mode: Option<String>) -> Result<(), String> {
    let dir = validate_project_dir(&dir)?;
    if let Some(ref dm) = discussion_mode {
        if dm != "open" && dm != "directed" {
            return Err(format!("Invalid communication mode '{}'. Must be 'open' or 'directed'. (Session formats like 'delphi'/'oxford' are set when starting a session, not here.)", dm));
        }
    }

    let config_path = std::path::Path::new(&dir).join(".vaak").join("project.json");
    let content = std::fs::read_to_string(&config_path)
        .map_err(|e| format!("Failed to read project.json: {}", e))?;
    let mut config: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse project.json: {}", e))?;

    if let Some(settings) = config.get_mut("settings") {
        // pr-r2-data-fields: write the new field name AND remove the legacy one
        // so existing project.json files migrate on first set. Keeps the file
        // clean — no dual-key drift between session_mode and discussion_mode.
        if let Some(obj) = settings.as_object_mut() {
            obj.remove("discussion_mode");
        }
        match &discussion_mode {
            Some(dm) => { settings["session_mode"] = serde_json::Value::String(dm.clone()); }
            None => { settings.as_object_mut().map(|o| o.remove("session_mode")); }
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

#[tauri::command]
fn get_turn_state(dir: String) -> Result<serde_json::Value, String> {
    let dir = validate_project_dir(&dir)?;
    let turn_path = std::path::Path::new(&dir).join(".vaak").join("turn_state.json");
    match std::fs::read_to_string(&turn_path) {
        Ok(content) => serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse turn_state.json: {}", e)),
        Err(_) => Ok(serde_json::json!({"completed": true})),
    }
}

#[tauri::command]
fn set_work_mode(dir: String, work_mode: String) -> Result<(), String> {
    let dir = validate_project_dir(&dir)?;
    if work_mode != "simultaneous" && work_mode != "consecutive" {
        return Err(format!("Invalid work mode '{}'. Must be 'simultaneous' or 'consecutive'.", work_mode));
    }

    let config_path = std::path::Path::new(&dir).join(".vaak").join("project.json");
    let content = std::fs::read_to_string(&config_path)
        .map_err(|e| format!("Failed to read project.json: {}", e))?;
    let mut config: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse project.json: {}", e))?;

    if let Some(settings) = config.get_mut("settings") {
        settings["work_mode"] = serde_json::Value::String(work_mode.clone());
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

    // Clear stale turn state when switching to simultaneous
    if work_mode == "simultaneous" {
        let turn_state_path = std::path::Path::new(&dir).join(".vaak").join("turn_state.json");
        let _ = std::fs::remove_file(&turn_state_path);
    }

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

/// DEPRECATED (pr-r2-tauri-cmds): use `start_session` instead.
/// Kept for backward compat during the UX migration window. Will be
/// removed in a future cleanup PR after all callers flip to the new name.
#[tauri::command]
fn start_discussion(
    dir: String,
    mode: String,
    topic: String,
    moderator: Option<String>,
    participants: Vec<String>,
    rounds: Option<u32>,
    pipeline_mode: Option<String>,
) -> Result<(), String> {
    eprintln!("[deprecated] Tauri command `start_discussion` is deprecated — use `start_session` instead.");
    // DIAGNOSTIC: Write immediately at function entry to prove this code runs
    let diag_path = std::path::Path::new(&dir).join(".vaak").join("start_discussion_debug.log");
    let _ = std::fs::write(&diag_path, format!("ENTRY: mode={} topic={} dir={}\n", mode, topic, dir));
    eprintln!("[start_discussion] DIAGNOSTIC: function entered, mode={}, topic={}", mode, topic);

    let valid_modes = ["delphi", "oxford", "red_team", "continuous", "pipeline"];
    if !valid_modes.contains(&mode.as_str()) {
        return Err(format!("Invalid discussion mode '{}'. Must be one of: {}", mode, valid_modes.join(", ")));
    }
    if topic.trim().is_empty() {
        return Err("Topic cannot be empty.".to_string());
    }
    if participants.is_empty() {
        return Err("At least one participant is required.".to_string());
    }

    // Auto-detect moderator: if not explicitly provided, check if a "moderator"
    // role exists in the roster. Discussion-bound roles auto-start when discussions
    // begin, so the moderator may not have an active session yet.
    let moderator = moderator.or_else(|| {
        let project_path = std::path::Path::new(&dir).join(".vaak").join("project.json");
        let project_data = std::fs::read_to_string(&project_path).ok()?;
        let project: serde_json::Value = serde_json::from_str(&project_data).ok()?;
        // Check if "moderator" role exists in the roster
        let has_moderator = project.get("roles")
            .and_then(|r| r.get("moderator"))
            .is_some();
        if !has_moderator {
            return None;
        }
        // Check for an active session first
        let sessions_path = std::path::Path::new(&dir).join(".vaak").join("sessions.json");
        if let Some(sessions) = std::fs::read_to_string(&sessions_path).ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        {
            if let Some(instance) = sessions.get("bindings")
                .and_then(|b| b.as_array())
                .and_then(|bindings| {
                    bindings.iter().find(|b| {
                        let role = b.get("role").and_then(|r| r.as_str()).unwrap_or("");
                        let status = b.get("status").and_then(|s| s.as_str()).unwrap_or("");
                        role == "moderator" && (status == "active" || status == "idle")
                    })
                })
                .and_then(|b| b.get("instance").and_then(|i| i.as_u64()))
            {
                return Some(format!("moderator:{}", instance));
            }
        }
        // Role exists but no active session — use moderator:0 (will auto-start)
        Some("moderator:0".to_string())
    });

    // Validate: warn if moderator is vacant (but don't block — auto mode is the fallback)
    // This log helps diagnose "absent moderator" issues in discussions
    if moderator.is_none() {
        eprintln!("[start_discussion] WARNING: No moderator available. Discussion will run unmoderated.");
    }

    let now = iso_now();
    let is_continuous = mode == "continuous";
    let is_pipeline = mode == "pipeline";

    // Continuous mode starts in "reviewing" phase with no rounds —
    // rounds are auto-created when developers post status messages.
    // Pipeline mode starts at stage 0, phase "pipeline_active", no rounds.
    // "reviewing" = ready for next auto-trigger (consistent with post-close phase).
    let (initial_round, initial_phase, initial_rounds) = if is_continuous {
        (0, "reviewing".to_string(), Vec::new())
    } else if is_pipeline {
        (0, "pipeline_active".to_string(), Vec::new())
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
    } else if is_pipeline {
        // Use rounds parameter if provided, otherwise default to unlimited
        if let Some(r) = rounds {
            settings.max_rounds = r;
            settings.termination = Some(collab::TerminationStrategy::FixedRounds { rounds: r });
        } else {
            settings.max_rounds = 999;
            settings.termination = Some(collab::TerminationStrategy::Unlimited);
        }
        settings.auto_close_timeout_seconds = 30;
        settings.automation = Some(collab::AutomationLevel::Auto);
    }

    // For pipeline mode, use intelligent topic-based ordering (shared with MCP sidecar)
    let pipeline_order = if is_pipeline {
        Some(collab::build_pipeline_order(&dir, &topic, &participants))
    } else {
        None
    };

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
        audience_state: "listening".to_string(),
        audience_enabled: false,
        pipeline_order,
        pipeline_stage: if is_pipeline { Some(0) } else { None },
        pipeline_outputs: if is_pipeline { Some(Vec::new()) } else { None },
        oxford_teams: None,
        oxford_votes: None,
        oxford_motion: None,
        attack_chains: None,
        severity_summary: None,
        unaddressed_count: None,
        micro_rounds: None,
        decision_stream: None,
        pipeline_mode: if is_pipeline { Some(pipeline_mode.unwrap_or_else(|| "discussion".to_string())) } else { None },
        stagnant_rounds: 0,
    };

    // Step 1: Write discussion.json directly (no board lock needed — uses its own lock)
    collab::write_discussion(&dir, &state)
        .map_err(|e| format!("Failed to write discussion.json: {}", e))?;

    // Step 2: Post announcement to board file (inside file lock for atomic ID + write).
    // Diagnostic: log before attempting board lock
    {
        let diag_path = std::path::Path::new(&dir).join(".vaak").join("start_discussion_debug.log");
        let _ = std::fs::write(&diag_path, format!(
            "ENTRY: mode={} topic={} dir={}\nSTEP2: About to call with_board_lock\nactive_board_path: {:?}\nactive_lock_path: {:?}\n",
            state.mode.as_deref().unwrap_or("?"), state.topic, dir,
            collab::active_board_path(&dir), collab::active_lock_path(&dir)
        ));
    }
    {
        let mode_ref = state.mode.as_deref().unwrap_or("unknown");
        let topic_ref = state.topic.clone();
        let mod_ref = state.moderator.as_deref().unwrap_or("none").to_string();
        let parts_ref = state.participants.join(", ");
        let pipeline_order = state.pipeline_order.clone();
        let started_at = state.started_at.clone();
        let dir_clone = dir.clone();
        let dir_for_log = dir.clone();

        let board_result = collab::with_board_lock(&dir, move || {
            let board_path = collab::active_board_path(&dir_clone);
            let log_path = std::path::Path::new(&dir_clone).join(".vaak").join("start_discussion_debug.log");
            let mut log_entries = vec![format!("[{}] Board path: {}", started_at.as_deref().unwrap_or("?"), board_path.display())];

            let board_content = std::fs::read_to_string(&board_path).unwrap_or_default();
            let line_count = board_content.lines().filter(|l| !l.trim().is_empty()).count();
            log_entries.push(format!("Board has {} lines", line_count));

            let msg_id: u64 = board_content.lines()
                .filter(|l| !l.trim().is_empty())
                .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
                .filter_map(|m| m.get("id").and_then(|i| i.as_u64()))
                .max()
                .unwrap_or(0) + 1;
            log_entries.push(format!("Computed msg_id: {}", msg_id));

            let announcement_body = if mode_ref == "continuous" {
                format!("Continuous Review mode activated.\n\n**Topic:** {}\n**Moderator:** {}\n**Participants:** {}\n\nReview windows open automatically when developers post status updates. Respond with: agree / disagree: [reason] / alternative: [proposal]. Silence within the timeout = consent.",
                    topic_ref, mod_ref, parts_ref)
            } else if mode_ref == "pipeline" {
                let order_display = pipeline_order.as_ref().map(|o| o.iter().enumerate()
                    .map(|(i, a)| if i == 0 { format!("▶ {}", a) } else { a.clone() })
                    .collect::<Vec<_>>().join(" → ")).unwrap_or_default();
                format!("A pipeline discussion has been started.\n\n**Topic:** {}\n**Moderator:** {}\n**Pipeline Order:** {}\n\nAgents will process sequentially. Each agent sees all previous stage outputs.",
                    topic_ref, mod_ref, order_display)
            } else {
                format!("A {} discussion has been started.\n\n**Topic:** {}\n**Moderator:** {}\n**Participants:** {}\n**Round:** 1\n\nSubmit your position using type: submission, addressed to the moderator.",
                    mode_ref, topic_ref, mod_ref, parts_ref)
            };
            let announcement = serde_json::json!({
                "id": msg_id,
                "from": "system",
                "to": "all",
                "type": "moderation",
                "timestamp": started_at,
                "subject": format!("{} discussion started: {}", mode_ref, topic_ref),
                "body": announcement_body,
                "metadata": {
                    "discussion_action": "start",
                    "mode": mode_ref,
                    "round": 1
                }
            });
            let line = serde_json::to_string(&announcement)
                .map_err(|e| format!("Failed to serialize announcement: {}", e))?;
            log_entries.push(format!("Serialized OK, length: {}", line.len()));

            use std::io::Write;
            let mut f = std::fs::OpenOptions::new().create(true).append(true).open(&board_path)
                .map_err(|e| {
                    log_entries.push(format!("OPEN FAILED: {}", e));
                    let _ = std::fs::write(&log_path, log_entries.join("\n"));
                    format!("[start_discussion] Board open failed: {} (path: {})", e, board_path.display())
                })?;
            match writeln!(f, "{}", line) {
                Ok(()) => {
                    log_entries.push(format!("WRITE OK — id={}", msg_id));
                    let _ = std::fs::write(&log_path, log_entries.join("\n"));
                }
                Err(e) => {
                    log_entries.push(format!("WRITE FAILED: {}", e));
                    let _ = std::fs::write(&log_path, log_entries.join("\n"));
                    return Err(format!("[start_discussion] Board write failed: {}", e));
                }
            }
            eprintln!("[start_discussion] Board announcement written successfully (id={})", msg_id);
            Ok(())
        });

        // Diagnostic: log result of with_board_lock
        let diag_path2 = std::path::Path::new(&dir_for_log).join(".vaak").join("start_discussion_debug.log");
        match &board_result {
            Ok(()) => {
                eprintln!("[start_discussion] with_board_lock succeeded");
                let prev = std::fs::read_to_string(&diag_path2).unwrap_or_default();
                let _ = std::fs::write(&diag_path2, format!("{}\nBOARD_RESULT: OK — announcement written", prev));
            }
            Err(e) => {
                eprintln!("[start_discussion] with_board_lock FAILED: {}", e);
                let prev = std::fs::read_to_string(&diag_path2).unwrap_or_default();
                let _ = std::fs::write(&diag_path2, format!("{}\nBOARD_RESULT: FAILED — {}", prev, e));
            }
        }
        board_result.map_err(|e| format!("Board announcement failed: {}", e))?;
    }

    notify_collab_change();
    Ok(())
}

#[tauri::command]
fn close_discussion_round(dir: String) -> Result<String, String> {
    eprintln!("[deprecated] Tauri command `close_discussion_round` is deprecated — use `close_session_round` instead.");
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
        let msg_id: u64 = board_content.lines()
            .filter(|l| !l.trim().is_empty())
            .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
            .filter_map(|m| m.get("id").and_then(|i| i.as_u64()))
            .max()
            .unwrap_or(0) + 1;
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

        collab::write_discussion_unlocked(&dir, &state)
            .map_err(|e| format!("Failed to write discussion.json: {}", e))?;
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

        collab::write_discussion_unlocked(&dir, &state)
            .map_err(|e| format!("Failed to write discussion.json: {}", e))?;

        // Post round-open announcement to board.jsonl (lock already held)
        let board_path = collab::active_board_path(&dir);
        let board_content = std::fs::read_to_string(&board_path).unwrap_or_default();
        let msg_id: u64 = board_content.lines()
            .filter(|l| !l.trim().is_empty())
            .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
            .filter_map(|m| m.get("id").and_then(|i| i.as_u64()))
            .max()
            .unwrap_or(0) + 1;
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

/// Normalize a moderator-action reason: trim and apply a fallback default
/// when the input is empty or too short to be informative.
///
/// History: pr-reason-params (commit 0f0911c) initially returned `Err` for
/// inputs under 3 chars. Human msg 462 reported friction — required typed
/// reasons on every End click broke the hot-path UX. Architect msg 449
/// independently flagged the audit-null placeholder problem and recommended
/// auto-derived defaults rather than mandatory prompts (option 2).
///
/// Resolution (pr-reason-relax): soft contract — backend never rejects.
/// Caller-supplied reason is used when ≥3 chars after trim; otherwise a
/// default tagged with the action name flows through so the audit trail
/// stays populated without blocking the user. UI layers (e.g. EndSession-
/// ConfirmModal) may still enforce stricter prompts where audit value
/// matters most.
fn normalize_action_reason(reason: &str, action_default: &str) -> String {
    let trimmed = reason.trim();
    if trimmed.len() >= 3 {
        trimmed.to_string()
    } else {
        // Per architect msg 484 (pr-normalize-debug-warn): silent substitution
        // is the right behavior for human callers but masks broken agent
        // reason-builders in development. Warn in debug builds when the
        // caller passed a non-empty-but-too-short value (the empty case is
        // the expected default-flow path and stays silent).
        #[cfg(debug_assertions)]
        if !reason.is_empty() {
            eprintln!(
                "[normalize_action_reason] caller passed reason={:?} (trimmed len {}, below 3 char threshold) — substituting default {:?}",
                reason, trimmed.len(), action_default
            );
        }
        action_default.to_string()
    }
}

#[tauri::command]
fn end_discussion(dir: String, reason: Option<String>) -> Result<(), String> {
    eprintln!("[deprecated] Tauri command `end_discussion` is deprecated — use `end_session` instead.");
    let dir = validate_project_dir(&dir)?;
    let reason = normalize_action_reason(reason.as_deref().unwrap_or(""), "Ended by user");
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

        collab::write_discussion_unlocked(&dir, &state)
            .map_err(|e| format!("Could not save discussion state to disk: {}. The session may have ended despite this error — check .vaak/sections/<active>/discussion.json.", e))?;

        // Post end announcement to board.jsonl (lock already held)
        let board_path = collab::active_board_path(&dir);
        let board_content = std::fs::read_to_string(&board_path).unwrap_or_default();
        let msg_id: u64 = board_content.lines()
            .filter(|l| !l.trim().is_empty())
            .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
            .filter_map(|m| m.get("id").and_then(|i| i.as_u64()))
            .max()
            .unwrap_or(0) + 1;
        let announcement = serde_json::json!({
            "id": msg_id,
            "from": "system",
            "to": "all",
            "type": "moderation",
            "timestamp": now,
            "subject": format!("Discussion ended: {}", topic),
            "body": format!("The discussion on \"{}\" has concluded after {} round(s). Reason: {}", topic, round_num, reason),
            "metadata": {
                "discussion_action": "end",
                "final_round": round_num,
                "reason": reason
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

/// Explicitly designate a moderator for the active session.
///
/// Per architect msg 524 + tech-leader msg 540 (pr-moderator-set + cleanup,
/// addresses human msg 511 ask #3 "no clear way to designate a moderator").
/// Today moderator is implicit (whichever role claims first, or `null` for
/// auto). This command makes it explicit.
///
/// Validation:
///   1. session must be active
///   2. target role + instance must have an active session in sessions.json
///      (last_heartbeat within heartbeat_timeout_seconds)
///   3. authority: this is a `#[tauri::command]` callable only from the
///      Tauri webview (not exposed via MCP per platform-engineer msg 534
///      Finding 2 rebuttal). The caller therefore IS the human, by
///      construction — no per-call caller verification needed.
///
/// SAFETY GATE (vision § 11.14a, tech-leader msg 540): if this command is
/// ever exposed via the MCP sidecar (`handle_*` in vaak-mcp.rs), Option A
/// from architect msg 533 (Tauri-window-injection or split into separate
/// MCP/Tauri commands) MUST be implemented first. Otherwise an agent
/// claiming the `human` role gains pipeline-hijack capability.
///
/// On success: writes discussion.json.moderator = "<role>:<instance>" (string
/// format consistent with how `handle_discussion_control` reads it at
/// vaak-mcp.rs:2614). Posts a `moderation` board message announcing the
/// reassignment so the audit trail captures it.
#[tauri::command]
fn set_session_moderator(
    dir: String,
    role: String,
    instance: u32,
) -> Result<(), String> {
    let dir = validate_project_dir(&dir)?;
    let role = role.trim().to_string();

    if role.is_empty() {
        return Err("role must be non-empty".to_string());
    }

    let new_moderator_label = format!("{}:{}", role, instance);

    let result = collab::with_board_lock(&dir, || {
        let mut state = collab::read_discussion(&dir);
        if !state.active {
            return Err("No active session — cannot set moderator on a closed session.".to_string());
        }

        // Validate target has an active heartbeat
        let sessions_path = std::path::Path::new(&dir).join(".vaak").join("sessions.json");
        let sessions: serde_json::Value = std::fs::read_to_string(&sessions_path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or(serde_json::json!({"bindings": []}));
        let bindings = sessions.get("bindings")
            .and_then(|b| b.as_array())
            .cloned()
            .unwrap_or_default();

        let project_cfg_path = std::path::Path::new(&dir).join(".vaak").join("project.json");
        let project_cfg: serde_json::Value = std::fs::read_to_string(&project_cfg_path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or(serde_json::json!({}));
        let timeout_secs = project_cfg.get("settings")
            .and_then(|s| s.get("heartbeat_timeout_seconds"))
            .and_then(|t| t.as_u64())
            .unwrap_or(300);

        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let target_session = bindings.iter().find(|b| {
            let r = b.get("role").and_then(|x| x.as_str()).unwrap_or("");
            let i = b.get("instance").and_then(|x| x.as_u64()).unwrap_or(0);
            r == role && i as u32 == instance
        });

        let target_alive = match target_session.and_then(|s| s.get("last_heartbeat").and_then(|h| h.as_str())) {
            Some(hb_iso) => {
                match collab::parse_iso_epoch_pub(hb_iso) {
                    Some(hb_secs) => now_secs.saturating_sub(hb_secs) <= timeout_secs,
                    None => false,
                }
            }
            None => false,
        };

        if !target_alive {
            return Err(format!(
                "{} is not an active session (no recent heartbeat in last {}s) — cannot designate as moderator.",
                new_moderator_label, timeout_secs
            ));
        }

        // Authorization: command is Tauri-only (not MCP-exposed) per safety
        // gate in the doc comment above. Caller IS the human via construction;
        // no per-call check needed. Future MCP exposure must add Option A
        // discrimination first.

        let prior_moderator = state.moderator.clone().unwrap_or_default();
        state.moderator = Some(new_moderator_label.clone());

        collab::write_discussion_unlocked(&dir, &state)
            .map_err(|e| format!("Failed to write discussion.json: {}", e))?;

        // Post moderation announcement (lock already held → use append directly)
        let board_path = collab::active_board_path(&dir);
        let board_content = std::fs::read_to_string(&board_path).unwrap_or_default();
        let msg_id: u64 = board_content.lines()
            .filter(|l| !l.trim().is_empty())
            .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
            .filter_map(|m| m.get("id").and_then(|i| i.as_u64()))
            .max()
            .unwrap_or(0) + 1;
        let now_iso = iso_now();
        let announcement = serde_json::json!({
            "id": msg_id,
            "from": "system",
            "to": "all",
            "type": "moderation",
            "timestamp": now_iso,
            "subject": format!("Moderator set to {}", new_moderator_label),
            "body": format!(
                "Session moderator is now {} (was: {}). Set via Tauri UI (human).",
                new_moderator_label,
                if prior_moderator.is_empty() { "none" } else { prior_moderator.as_str() }
            ),
            "metadata": {
                "discussion_action": "set_moderator",
                "prior_moderator": prior_moderator,
                "new_moderator": new_moderator_label,
                "set_by": "tauri_ui"
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
fn pause_discussion(dir: String, reason: Option<String>) -> Result<(), String> {
    eprintln!("[deprecated] Tauri command `pause_discussion` is deprecated — use `pause_session` instead.");
    let dir = validate_project_dir(&dir)?;
    let reason = normalize_action_reason(reason.as_deref().unwrap_or(""), "Paused by user");
    let result = collab::with_board_lock(&dir, || {
        let mut state = collab::read_discussion(&dir);
        if !state.active {
            return Err("No active discussion.".to_string());
        }
        if state.paused_at.is_some() {
            return Err("Discussion is already paused.".to_string());
        }
        let now = iso_now();
        state.paused_at = Some(now.clone());
        collab::write_discussion_unlocked(&dir, &state)
            .map_err(|e| format!("Could not save discussion state to disk: {}. The pause may have applied despite this error — refresh the panel.", e))?;
        // Post pause announcement
        let board_path = collab::active_board_path(&dir);
        let board_content = std::fs::read_to_string(&board_path).unwrap_or_default();
        let msg_id: u64 = board_content.lines()
            .filter(|l| !l.trim().is_empty())
            .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
            .filter_map(|m| m.get("id").and_then(|i| i.as_u64()))
            .max()
            .unwrap_or(0) + 1;
        let announcement = serde_json::json!({
            "id": msg_id, "from": "system", "to": "all", "type": "system",
            "timestamp": now,
            "subject": "Discussion paused",
            "body": format!("The discussion has been paused by the human. Reason: {}", reason),
            "metadata": {"discussion_action": "pause", "reason": reason}
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
fn resume_discussion(dir: String, reason: Option<String>) -> Result<(), String> {
    eprintln!("[deprecated] Tauri command `resume_discussion` is deprecated — use `resume_session` instead.");
    let dir = validate_project_dir(&dir)?;
    let reason = normalize_action_reason(reason.as_deref().unwrap_or(""), "Resumed by user");
    let result = collab::with_board_lock(&dir, || {
        let mut state = collab::read_discussion(&dir);
        if !state.active {
            return Err("No active discussion.".to_string());
        }
        if state.paused_at.is_none() {
            return Err("Discussion is not paused.".to_string());
        }
        let now = iso_now();
        state.paused_at = None;
        collab::write_discussion_unlocked(&dir, &state)
            .map_err(|e| format!("Could not save discussion state to disk: {}. The resume may have applied despite this error — refresh the panel.", e))?;
        // Post resume announcement
        let board_path = collab::active_board_path(&dir);
        let board_content = std::fs::read_to_string(&board_path).unwrap_or_default();
        let msg_id: u64 = board_content.lines()
            .filter(|l| !l.trim().is_empty())
            .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
            .filter_map(|m| m.get("id").and_then(|i| i.as_u64()))
            .max()
            .unwrap_or(0) + 1;
        let announcement = serde_json::json!({
            "id": msg_id, "from": "system", "to": "all", "type": "system",
            "timestamp": now,
            "subject": "Discussion resumed",
            "body": format!("The discussion has been resumed by the human. Reason: {}", reason),
            "metadata": {"discussion_action": "resume", "reason": reason}
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
fn update_discussion_settings(dir: String, max_rounds: Option<u32>) -> Result<(), String> {
    eprintln!("[deprecated] Tauri command `update_discussion_settings` is deprecated — use `update_session_settings` instead.");
    let dir = validate_project_dir(&dir)?;
    let result = collab::with_board_lock(&dir, || {
        let mut state = collab::read_discussion(&dir);
        if !state.active {
            return Err("No active discussion.".to_string());
        }
        if let Some(rounds) = max_rounds {
            state.settings.max_rounds = rounds;
        }
        collab::write_discussion_unlocked(&dir, &state)
            .map_err(|e| format!("Failed to write discussion.json: {}", e))?;
        Ok(())
    });
    if result.is_ok() { notify_collab_change(); }
    result
}

#[tauri::command]
fn get_discussion_state(dir: String) -> Result<serde_json::Value, String> {
    eprintln!("[deprecated] Tauri command `get_discussion_state` is deprecated — use `get_session_state` instead.");
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

// ════════════════════════════════════════════════════════════════════
// pr-r2-tauri-cmds: Session-named aliases for the Discussion-named
// Tauri commands. Per architect msg 524 + tech-leader msg 540/550 +
// human msg 511 ask #2 ("It still is called a discussion").
//
// Strategy (split into 3 commits per dev-challenger msg 530 Finding 3):
//   1. THIS PR — register both old and new names; old logs deprecation
//   2. UX (separate PR) — flip frontend invoke sites to new names
//   3. Future cleanup PR — remove old aliases entirely
//
// Each new alias is a one-line delegation. Behavior is identical;
// only the externally-visible Tauri command name differs. UX can call
// either name during the migration window.
//
// See `set_session_moderator` (already shipped under the new name) for
// the eventual single-name target shape.
// ════════════════════════════════════════════════════════════════════

#[tauri::command]
fn start_session(
    dir: String,
    mode: String,
    topic: String,
    moderator: Option<String>,
    participants: Vec<String>,
    rounds: Option<u32>,
    pipeline_mode: Option<String>,
) -> Result<(), String> {
    start_discussion(dir, mode, topic, moderator, participants, rounds, pipeline_mode)
}

#[tauri::command]
fn close_session_round(dir: String) -> Result<String, String> {
    close_discussion_round(dir)
}

#[tauri::command]
fn end_session(dir: String, reason: Option<String>) -> Result<(), String> {
    end_discussion(dir, reason)
}

#[tauri::command]
fn pause_session(dir: String, reason: Option<String>) -> Result<(), String> {
    pause_discussion(dir, reason)
}

#[tauri::command]
fn resume_session(dir: String, reason: Option<String>) -> Result<(), String> {
    resume_discussion(dir, reason)
}

#[tauri::command]
fn update_session_settings(dir: String, max_rounds: Option<u32>) -> Result<(), String> {
    update_discussion_settings(dir, max_rounds)
}

#[tauri::command]
fn get_session_state(dir: String) -> Result<serde_json::Value, String> {
    get_discussion_state(dir)
}

// ════════════════════════════════════════════════════════════════════
// END pr-r2-tauri-cmds aliases
// ════════════════════════════════════════════════════════════════════

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

    let effective_type = msg_type.unwrap_or_else(|| "directive".to_string());
    let effective_metadata = metadata.unwrap_or(serde_json::json!({}));

    let msg_id = collab::with_board_lock(&dir, || {
        let board_path = collab::active_board_path(&dir);

        // Read existing board to determine next message ID (inside lock to prevent races)
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

        Ok(msg_id)
    })?;

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

    let home = collab::vaak_home_dir()?;
    let global_path = home.join(".vaak").join("role-groups.json");
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

// ==================== Ethereal Agent Commands ====================

/// Try to read ANTHROPIC_API_KEY from .env files (backend/.env, .env)
fn resolve_api_key_from_dotenv(project_dir: &str) -> Option<String> {
    let project = std::path::Path::new(project_dir);
    // Walk up to find the repo root (where backend/.env lives)
    let mut dir = project.to_path_buf();
    loop {
        // Check backend/.env
        let backend_env = dir.join("backend").join(".env");
        if let Some(key) = read_dotenv_key(&backend_env, "ANTHROPIC_API_KEY") {
            return Some(key);
        }
        // Check .env at root
        let root_env = dir.join(".env");
        if let Some(key) = read_dotenv_key(&root_env, "ANTHROPIC_API_KEY") {
            return Some(key);
        }
        if !dir.pop() {
            break;
        }
    }
    None
}

fn read_dotenv_key(path: &std::path::Path, key: &str) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    let prefix = format!("{}=", key);
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with(&prefix) {
            let val = trimmed[prefix.len()..].trim().trim_matches('"').trim_matches('\'');
            if !val.is_empty() {
                return Some(val.to_string());
            }
        }
    }
    None
}

// ==================== Role Designer (Anthropic API) ====================

const ROLE_DESIGNER_SYSTEM: &str = r#"You are a Role Designer — an expert at creating team roles for AI agent collaboration systems. Your job is to interview the user about the role they want to create, then produce a complete role configuration.

## How You Work

1. **Interview phase**: Ask the user 3-5 focused questions to understand the role they need. Ask one question at a time. Be conversational but efficient.
2. **Design phase**: When you have enough information, generate the complete role configuration.

## Interview Guidelines

- Start by asking what kind of work they need help with
- Ask about boundaries — what should this role NOT do?
- Ask about team fit — how does this relate to their existing roles?
- Ask about authority — should this role direct others, or be directed?
- Don't ask more than 5 questions total. If you have enough after 3, proceed to design.

## Available Capabilities (tags)

implementation, code-review, testing, architecture, moderation, security, compliance, analysis, coordination, red-team, documentation, debugging

## Available Permissions

broadcast, review, assign_tasks, status, question, handoff, moderation

## When You're Ready to Generate

When you have enough information, output a friendly summary of the role you'll create, followed by the configuration in this exact format:

|||ROLE_CONFIG|||
{
  "title": "Role Title",
  "slug": "role-slug",
  "description": "One-sentence description of this role",
  "tags": ["tag1", "tag2"],
  "permissions": ["perm1", "perm2"],
  "max_instances": 1,
  "briefing": "Full markdown briefing content..."
}
|||END_CONFIG|||

The briefing should be a complete markdown document with sections: Identity, Primary Function, Anti-patterns, Peer Relationships, Action Boundary, Onboarding.

## Important Rules

- The slug must be lowercase alphanumeric with hyphens only
- Always include "status" in permissions
- max_instances should be 1 for specialized roles, 2-3 for implementation roles
- The briefing should reference the specific team context the user described
- Be opinionated — recommend what you think is best
- If the user's request closely matches an existing role on their team, point that out
"#;

#[tauri::command]
fn design_role_turn(dir: String, messages: Vec<serde_json::Value>, api_key: String) -> Result<serde_json::Value, String> {
    let dir = validate_project_dir(&dir)?;
    let resolved_key = if !api_key.is_empty() {
        api_key
    } else if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
        key
    } else {
        resolve_api_key_from_dotenv(&dir)
            .ok_or_else(|| format!("No API key found. Set ANTHROPIC_API_KEY environment variable, enter it in Collab > Ethereal Settings, or add it to backend/.env (searched from: {})", dir))?
    };

    // Build team context from project.json
    let config_path = std::path::Path::new(&dir).join(".vaak").join("project.json");
    let team_context = if let Ok(content) = std::fs::read_to_string(&config_path) {
        if let Ok(config) = serde_json::from_str::<serde_json::Value>(&content) {
            if let Some(roles) = config.get("roles").and_then(|r| r.as_object()) {
                let mut ctx = String::from("\n\n## Current Team Context\n\nExisting roles:\n");
                for (slug, role) in roles {
                    let title = role.get("title").and_then(|t| t.as_str()).unwrap_or(slug);
                    let desc = role.get("description").and_then(|d| d.as_str()).unwrap_or("No description");
                    ctx.push_str(&format!("- **{}** ({}): {}\n", title, slug, desc));
                }
                ctx
            } else {
                String::new()
            }
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    let system_prompt = format!("{}{}", ROLE_DESIGNER_SYSTEM, team_context);

    // Build API messages
    let api_messages: Vec<serde_json::Value> = messages.iter()
        .filter(|m| {
            let role = m.get("role").and_then(|r| r.as_str()).unwrap_or("");
            role == "user" || role == "assistant"
        })
        .cloned()
        .collect();

    if api_messages.is_empty() {
        return Err("At least one message is required".to_string());
    }

    let body = serde_json::json!({
        "model": "claude-sonnet-4-5-20250929",
        "max_tokens": 4096,
        "system": system_prompt,
        "messages": api_messages,
    });

    let agent = ureq::AgentBuilder::new()
        .timeout_read(std::time::Duration::from_secs(90))
        .timeout_write(std::time::Duration::from_secs(30))
        .build();

    let resp = agent.post("https://api.anthropic.com/v1/messages")
        .set("x-api-key", &resolved_key)
        .set("anthropic-version", "2023-06-01")
        .set("content-type", "application/json")
        .send_string(&body.to_string())
        .map_err(|e| format!("Anthropic API error: {}", e))?;

    let resp_str = resp.into_string()
        .map_err(|e| format!("Failed to read API response: {}", e))?;
    let resp_body: serde_json::Value = serde_json::from_str(&resp_str)
        .map_err(|e| format!("Failed to parse API response: {}", e))?;

    let content = resp_body.get("content")
        .and_then(|c| c.as_array())
        .and_then(|arr| arr.first())
        .and_then(|block| block.get("text"))
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .to_string();

    // Parse role config from |||ROLE_CONFIG||| delimiters
    let role_config = {
        let start_marker = "|||ROLE_CONFIG|||";
        let end_marker = "|||END_CONFIG|||";
        if let (Some(start), Some(end)) = (content.find(start_marker), content.find(end_marker)) {
            let json_str = &content[start + start_marker.len()..end].trim();
            serde_json::from_str::<serde_json::Value>(json_str).ok()
        } else {
            None
        }
    };

    // Extract reply (content without the config block)
    let reply = if content.contains("|||ROLE_CONFIG|||") {
        let before = content.split("|||ROLE_CONFIG|||").next().unwrap_or("").trim();
        let after = content.split("|||END_CONFIG|||").last().unwrap_or("").trim();
        let combined = format!("{}\n{}", before, after).trim().to_string();
        if combined.is_empty() { content.clone() } else { combined }
    } else {
        content
    };

    Ok(serde_json::json!({
        "reply": reply,
        "role_config": role_config,
    }))
}

#[tauri::command]
fn start_ethereal_agent(
    dir: String,
    slug: String,
    api_key: String,
    groq_key: Option<String>,
    openai_key: Option<String>,
) -> Result<(), String> {
    let dir = validate_project_dir(&dir)?;

    // Resolve Anthropic key from parameter > env > dotenv
    let anthropic_key = if !api_key.is_empty() {
        Some(api_key)
    } else if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
        Some(key)
    } else {
        resolve_api_key_from_dotenv(&dir)
    };

    // Resolve Groq key from parameter > env
    let groq = groq_key
        .filter(|k| !k.is_empty())
        .or_else(|| std::env::var("GROQ_API_KEY").ok());

    // Resolve OpenAI key from parameter > env
    let openai = openai_key
        .filter(|k| !k.is_empty())
        .or_else(|| std::env::var("OPENAI_API_KEY").ok());

    let keys = ethereal::ProviderKeys {
        anthropic: anthropic_key,
        groq,
        openai,
    };

    let configs = ethereal::default_configs();
    let config = configs.into_iter()
        .find(|c| c.slug == slug)
        .ok_or_else(|| format!("Unknown ethereal agent: {}", slug))?;
    ethereal::start_agent_multi_provider(&dir, config, keys)
}

#[tauri::command]
fn check_anthropic_env_key() -> bool {
    std::env::var("ANTHROPIC_API_KEY").is_ok()
}

#[tauri::command]
fn stop_ethereal_agent(slug: String) -> Result<(), String> {
    ethereal::stop_agent(&slug)
}

#[tauri::command]
fn get_ethereal_statuses() -> Vec<ethereal::EtherealStatus> {
    ethereal::agent_statuses()
}

// ==================== Audience Pool Management ====================

/// Get the audiences directory path (~/.vaak/audiences/).
fn audiences_dir() -> Result<std::path::PathBuf, String> {
    Ok(collab::vaak_home_dir()?.join(".vaak").join("audiences"))
}

#[tauri::command]
fn list_audience_pools(project_dir: Option<String>) -> Result<Vec<serde_json::Value>, String> {
    let _ = project_dir; // Pools are global (~/.vaak/audiences/), project_dir reserved for future per-project pools
    let dir = audiences_dir()?;
    if !dir.exists() {
        return Ok(vec![]);
    }
    let mut pools = Vec::new();
    let entries = std::fs::read_dir(&dir).map_err(|e| format!("Read dir error: {}", e))?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        if let Ok(content) = std::fs::read_to_string(&path) {
            if let Ok(data) = serde_json::from_str::<serde_json::Value>(&content) {
                let personas = data.get("personas").and_then(|p| p.as_array()).map(|a| a.len()).unwrap_or(0);
                let providers: Vec<String> = data.get("personas")
                    .and_then(|p| p.as_array())
                    .map(|arr| {
                        let mut provs: Vec<String> = arr.iter()
                            .filter_map(|p| p.get("provider").and_then(|v| v.as_str()).map(|s| s.to_string()))
                            .collect::<std::collections::HashSet<_>>()
                            .into_iter()
                            .collect();
                        provs.sort();
                        provs
                    })
                    .unwrap_or_default();
                pools.push(serde_json::json!({
                    "id": data.get("id").and_then(|v| v.as_str()).unwrap_or(""),
                    "name": data.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                    "description": data.get("description").and_then(|v| v.as_str()).unwrap_or(""),
                    "builtin": data.get("builtin").and_then(|v| v.as_bool()).unwrap_or(false),
                    "persona_count": personas,
                    "member_count": personas, // backward compat
                    "providers": providers,
                }));
            }
        }
    }
    pools.sort_by(|a, b| {
        let a_name = a.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let b_name = b.get("name").and_then(|v| v.as_str()).unwrap_or("");
        a_name.cmp(b_name)
    });
    Ok(pools)
}

#[tauri::command]
fn get_audience_pool(pool_id: String, project_dir: Option<String>) -> Result<serde_json::Value, String> {
    let _ = project_dir;
    let pool_id = pool_id.to_ascii_lowercase();
    if pool_id.is_empty() || !pool_id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err("Invalid pool ID: must be lowercase alphanumeric with hyphens".to_string());
    }
    let path = audiences_dir()?.join(format!("{}.json", pool_id));
    let content = std::fs::read_to_string(&path)
        .map_err(|_| format!("Pool '{}' not found", pool_id))?;
    serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse pool: {}", e))
}

#[tauri::command]
fn save_audience_pool(pool_id: String, pool: String, project_dir: Option<String>) -> Result<(), String> {
    let _ = project_dir;
    let pool_id = pool_id.to_ascii_lowercase();
    if pool_id.is_empty() || !pool_id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err("Invalid pool ID: must be lowercase alphanumeric with hyphens".to_string());
    }
    // Parse the JSON string to validate it
    let pool_data: serde_json::Value = serde_json::from_str(&pool)
        .map_err(|e| format!("Invalid pool JSON: {}", e))?;
    let dir = audiences_dir()?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("Failed to create audiences dir: {}", e))?;
    let path = dir.join(format!("{}.json", pool_id));
    let content = serde_json::to_string_pretty(&pool_data)
        .map_err(|e| format!("Failed to serialize pool: {}", e))?;
    std::fs::write(&path, content)
        .map_err(|e| format!("Failed to write pool: {}", e))?;
    Ok(())
}

#[tauri::command]
fn delete_audience_pool(pool_id: String, project_dir: Option<String>) -> Result<bool, String> {
    let _ = project_dir;
    let pool_id = pool_id.to_ascii_lowercase();
    if pool_id.is_empty() || !pool_id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err("Invalid pool ID".to_string());
    }
    let path = audiences_dir()?.join(format!("{}.json", pool_id));
    if !path.exists() {
        return Ok(false);
    }
    // Refuse to delete builtin pools
    if let Ok(content) = std::fs::read_to_string(&path) {
        if let Ok(data) = serde_json::from_str::<serde_json::Value>(&content) {
            if data.get("builtin").and_then(|v| v.as_bool()).unwrap_or(false) {
                return Err("Cannot delete builtin pool".to_string());
            }
        }
    }
    std::fs::remove_file(&path).map_err(|e| format!("Failed to delete pool: {}", e))?;
    Ok(true)
}

const MAX_AUDIENCE_SIZE: usize = 10;

#[tauri::command]
fn set_audience_size(dir: String, size: usize) -> Result<(), String> {
    let dir = validate_project_dir(&dir)?;
    let capped = size.min(MAX_AUDIENCE_SIZE);
    let path = std::path::Path::new(&dir).join(".vaak").join("project.json");
    let content = std::fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read project.json: {}", e))?;
    let mut data: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse project.json: {}", e))?;

    // Ensure settings object exists
    if data.get("settings").is_none() {
        data["settings"] = serde_json::json!({});
    }
    data["settings"]["audience_size"] = serde_json::json!(capped);

    let output = serde_json::to_string_pretty(&data)
        .map_err(|e| format!("Failed to serialize: {}", e))?;
    collab::atomic_write(std::path::Path::new(&path), output.as_bytes())
        .map_err(|e| format!("Failed to write project.json: {}", e))?;
    Ok(())
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
            check_sidecar_status,
            get_build_info,
            invalidate_build_info_cache,
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
            set_watchdog_respawn_enabled,
            set_discussion_mode,
            set_session_mode,
            set_work_mode,
            get_turn_state,
            start_discussion,
            close_discussion_round,
            open_next_round,
            end_discussion,
            pause_discussion,
            resume_discussion,
            set_session_moderator,
            update_discussion_settings,
            get_discussion_state,
            set_continuous_timeout,
            // pr-r2-tauri-cmds: session-named aliases (UX migrating away from "discussion")
            start_session,
            close_session_round,
            end_session,
            pause_session,
            resume_session,
            update_session_settings,
            get_session_state,
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
            launcher::get_spawned_agents,
            launcher::get_role_companions,
            launcher::repopulate_spawned,
            launcher::relaunch_spawned,
            launcher::peek_spawned_manifest,
            launcher::discard_spawned_manifest,
            launcher::check_and_respawn_dead_agents,
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
            // Role designer command
            design_role_turn,
            // Ethereal agent commands
            start_ethereal_agent,
            stop_ethereal_agent,
            get_ethereal_statuses,
            check_anthropic_env_key,
            list_audience_pools,
            get_audience_pool,
            save_audience_pool,
            delete_audience_pool,
            set_audience_size,
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

    // ── pr-r2-tauri-cmds aliases ──────────────────────────────────────
    // Sanity check that each session-named alias delegates to its
    // discussion-named counterpart with the same Result. Uses an inactive
    // session fixture so the call returns Err quickly without touching
    // discussion state — the assertion is "alias and original behave the
    // same way" rather than exercising the underlying handler.

    fn fixture_inactive_for_alias_test(test_name: &str) -> std::path::PathBuf {
        let tmp = std::env::temp_dir().join(format!("vaak-test-r2-alias-{}-{}", test_name, std::process::id()));
        let vaak = tmp.join(".vaak");
        let _ = std::fs::create_dir_all(&vaak);
        std::fs::write(vaak.join("project.json"), r#"{"settings":{"heartbeat_timeout_seconds":3600}}"#).expect("project");
        std::fs::write(vaak.join("discussion.json"), r#"{"active":false,"mode":"pipeline"}"#).expect("disc");
        std::fs::write(vaak.join("sessions.json"), r#"{"bindings":[]}"#).expect("sess");
        std::fs::write(vaak.join("board.jsonl"), "").expect("board");
        tmp
    }

    #[test]
    fn end_session_alias_matches_end_discussion_behavior() {
        let tmp = fixture_inactive_for_alias_test("end");
        let dir = tmp.to_str().unwrap().to_string();
        let original = super::end_discussion(dir.clone(), Some("test reason 123".to_string()));
        let alias = super::end_session(dir.clone(), Some("test reason 123".to_string()));
        // Both must err with the same "No active discussion" message
        assert!(original.is_err() && alias.is_err());
        assert_eq!(original.unwrap_err(), alias.unwrap_err(),
            "end_session alias must produce identical error to end_discussion");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn pause_session_alias_matches_pause_discussion_behavior() {
        let tmp = fixture_inactive_for_alias_test("pause");
        let dir = tmp.to_str().unwrap().to_string();
        let original = super::pause_discussion(dir.clone(), Some("test".to_string()));
        let alias = super::pause_session(dir.clone(), Some("test".to_string()));
        assert!(original.is_err() && alias.is_err());
        assert_eq!(original.unwrap_err(), alias.unwrap_err());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn get_session_state_alias_matches_get_discussion_state_behavior() {
        let tmp = fixture_inactive_for_alias_test("get-state");
        let dir = tmp.to_str().unwrap().to_string();
        let original = super::get_discussion_state(dir.clone());
        let alias = super::get_session_state(dir.clone());
        // Both should succeed identically (returns the discussion.json content)
        assert!(original.is_ok() && alias.is_ok(),
            "get_session_state alias and get_discussion_state must both succeed");
        assert_eq!(original.unwrap(), alias.unwrap(),
            "get_session_state must return the same value as get_discussion_state");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    // ── set_session_moderator (pr-moderator-set) ──────────────────────
    // Per architect msg 524 testing spec: happy path + 3 validation failures.
    // TOCTOU coverage handled by the with_board_lock wrapper itself; these
    // tests exercise the predicate logic.

    /// Build a project fixture with the given moderator and a sessions.json
    /// containing the supplied bindings. Returns the temp path.
    fn fixture_set_moderator(test_name: &str, moderator: Option<&str>, bindings: &str) -> std::path::PathBuf {
        let tmp = std::env::temp_dir().join(format!("vaak-test-set-mod-{}-{}", test_name, std::process::id()));
        let vaak = tmp.join(".vaak");
        let _ = std::fs::create_dir_all(&vaak);
        std::fs::write(vaak.join("project.json"),
            r#"{"settings":{"heartbeat_timeout_seconds":3600},"name":"test"}"#)
            .expect("write project.json");
        let mod_field = match moderator {
            Some(m) => format!(r#""moderator":"{}","#, m),
            None => String::new(),
        };
        let disc = format!(r#"{{
            "active": true,
            "mode": "pipeline",
            {}
            "topic": "test"
        }}"#, mod_field);
        std::fs::write(vaak.join("discussion.json"), disc).expect("write discussion.json");
        std::fs::write(vaak.join("sessions.json"), format!(r#"{{"bindings":[{}]}}"#, bindings))
            .expect("write sessions.json");
        std::fs::write(vaak.join("board.jsonl"), "").expect("write board.jsonl");
        tmp
    }

    #[test]
    fn set_session_moderator_happy_path_human_assigns_active_role() {
        // Future heartbeat = always-alive (test runs in 2026, "2099" is far future)
        let tmp = fixture_set_moderator(
            "happy",
            None,
            r#"{"role":"developer","instance":0,"status":"active","last_heartbeat":"2099-01-01T00:00:00Z"}"#,
        );
        // Use the underlying impl directly so we don't need a Tauri runtime.
        // The Tauri command just delegates to this function body.
        let result = super::set_session_moderator(
            tmp.to_str().unwrap().to_string(),
            "developer".to_string(),
            0,
        );
        assert!(result.is_ok(), "tauri-only call should succeed, got {:?}", result);

        // Verify discussion.json got the new moderator
        let disc: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(tmp.join(".vaak").join("discussion.json")).expect("read")
        ).expect("parse");
        assert_eq!(
            disc.get("moderator").and_then(|m| m.as_str()),
            Some("developer:0")
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn set_session_moderator_rejects_stale_target() {
        let tmp = fixture_set_moderator(
            "stale-target",
            None,
            r#"{"role":"ghost","instance":0,"status":"active","last_heartbeat":"2020-01-01T00:00:00Z"}"#,
        );
        let result = super::set_session_moderator(
            tmp.to_str().unwrap().to_string(),
            "ghost".to_string(),
            0,
        );
        assert!(result.is_err(), "stale target must be rejected");
        let err = result.unwrap_err();
        assert!(
            err.contains("not an active session") || err.contains("no recent heartbeat"),
            "error must explain staleness, got: {}", err
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn set_session_moderator_rejects_unknown_role() {
        let tmp = fixture_set_moderator(
            "unknown",
            None,
            r#"{"role":"developer","instance":0,"status":"active","last_heartbeat":"2099-01-01T00:00:00Z"}"#,
        );
        let result = super::set_session_moderator(
            tmp.to_str().unwrap().to_string(),
            "phantom".to_string(),
            0,
        );
        assert!(result.is_err(), "unknown role must be rejected");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    // (Removed: set_session_moderator_rejects_non_authority_caller — the
    // command no longer takes a caller_role parameter. Authority is now
    // enforced structurally by Tauri-only exposure per safety gate above.)

    #[test]
    fn set_session_moderator_rejects_when_session_inactive() {
        let tmp = std::env::temp_dir().join(format!("vaak-test-set-mod-inactive-{}", std::process::id()));
        let vaak = tmp.join(".vaak");
        let _ = std::fs::create_dir_all(&vaak);
        std::fs::write(vaak.join("project.json"),
            r#"{"settings":{"heartbeat_timeout_seconds":3600}}"#).expect("project");
        std::fs::write(vaak.join("discussion.json"),
            r#"{"active":false,"mode":"pipeline"}"#).expect("disc");
        std::fs::write(vaak.join("sessions.json"),
            r#"{"bindings":[{"role":"developer","instance":0,"status":"active","last_heartbeat":"2099-01-01T00:00:00Z"}]}"#).expect("sess");
        std::fs::write(vaak.join("board.jsonl"), "").expect("board");

        let result = super::set_session_moderator(
            tmp.to_str().unwrap().to_string(),
            "developer".to_string(),
            0,
        );
        assert!(result.is_err(), "inactive session must reject moderator-set");
        assert!(
            result.unwrap_err().contains("No active session"),
            "error must explain inactive state"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    // ── write_discussion_unlocked error surface (pr-error-bubble) ──────

    #[test]
    fn write_discussion_unlocked_returns_error_detail_on_path_failure() {
        // Force atomic_write to fail by pointing at a project dir that doesn't
        // exist so active_discussion_path resolves to a missing parent. The
        // returned Err string must include both "write" (function-level) and
        // a path/OS error fragment (atomic_write detail) — this is what the
        // human will now see in the toast instead of "Failed to write".
        let bogus_dir = std::env::temp_dir()
            .join("vaak-test-error-bubble-nonexistent-99999")
            .join("nested")
            .join("never-created");
        let _ = std::fs::remove_dir_all(&bogus_dir); // ensure missing

        let state = collab::DiscussionState::default();
        let result = collab::write_discussion_unlocked(
            bogus_dir.to_str().unwrap(),
            &state,
        );

        assert!(result.is_err(), "expected Err on missing parent dir");
        let err = result.unwrap_err();
        assert!(
            err.starts_with("write "),
            "error must include the wrapped 'write <path>:' prefix, got: {}",
            err
        );
        // OS error message varies by platform but always contains some
        // diagnostic substring beyond the bare prefix
        assert!(
            err.len() > "write : ".len(),
            "error must include OS detail beyond the prefix, got: {}",
            err
        );
    }

    // ── normalize_action_reason (pr-reason-relax) ──────────────────────
    // Soft contract: backend never rejects on reason. Caller value used when
    // ≥3 chars after trim; otherwise the action default flows through. This
    // replaced validate_action_reason after human msg 462 reported the
    // hard-rejection broke the End-button hot path.

    #[test]
    fn normalize_action_reason_uses_caller_value_when_three_chars_or_more() {
        let result = normalize_action_reason("end", "default");
        assert_eq!(result, "end", "caller value at threshold passes through");
    }

    #[test]
    fn normalize_action_reason_trims_whitespace_before_length_check() {
        let result = normalize_action_reason("  Done with debate  ", "default");
        assert_eq!(result, "Done with debate", "returned reason is trimmed");
    }

    #[test]
    fn normalize_action_reason_falls_back_when_under_three_chars() {
        let result = normalize_action_reason("ab", "Ended by user");
        assert_eq!(result, "Ended by user", "2 chars triggers default");
    }

    #[test]
    fn normalize_action_reason_falls_back_for_only_whitespace() {
        let result = normalize_action_reason("   ", "Paused by user");
        assert_eq!(result, "Paused by user", "whitespace-only triggers default");
    }

    #[test]
    fn normalize_action_reason_falls_back_for_empty_string() {
        let result = normalize_action_reason("", "Resumed by user");
        assert_eq!(result, "Resumed by user", "empty triggers default");
    }

    #[test]
    fn normalize_action_reason_default_is_action_specific() {
        // Each command passes its own default so audit can distinguish them
        // even when the user supplied no reason.
        assert_eq!(
            normalize_action_reason("", "Ended by user"),
            "Ended by user"
        );
        assert_eq!(
            normalize_action_reason("", "Paused by user"),
            "Paused by user"
        );
        assert_eq!(
            normalize_action_reason("", "Resumed by user"),
            "Resumed by user"
        );
    }

    // ── validate_project_dir ───────────────────────────────────────────

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
        let board = std::fs::read_to_string(tmp.join(".vaak/board.jsonl")).unwrap();
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
