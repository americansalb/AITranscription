//! Linux focus tracking (stub).
//!
//! A full implementation would use AT-SPI2 over D-Bus:
//! - Register for "object:state-changed:focused" events
//! - Read element name/role from the focused object
//! - Emit TTS announcements
//!
//! This is not currently planned but the module structure is ready.

use std::sync::atomic::{AtomicBool, Ordering};

static TRACKING_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Stub — focus tracking is not implemented on Linux.
pub fn start(_app: tauri::AppHandle) {
    eprintln!("[a11y/focus_linux] Focus tracking is not available on Linux");
}

/// No-op on Linux.
pub fn stop() {
    TRACKING_ACTIVE.store(false, Ordering::SeqCst);
}

/// Always returns false on Linux.
pub fn is_active() -> bool {
    false
}
