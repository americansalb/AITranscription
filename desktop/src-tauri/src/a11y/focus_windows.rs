//! Windows UIA focus tracking.
//!
//! # Migration Status
//! STUB — awaiting port from `focus_tracker.rs`.
//!
//! The existing `focus_tracker.rs` continues to work in the meantime.
//! When ported, this module will:
//! 1. Spawn a dedicated thread that polls UIA GetFocusedElement every 100ms
//! 2. Extract element name + role via NormalizedRole mapping
//! 3. Emit "speak-immediate" Tauri events for TTS announcements
//! 4. Deduplicate: only announce when the focused element changes

use std::sync::atomic::{AtomicBool, Ordering};

static TRACKING_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Start focus tracking on Windows.
///
/// TODO: Port logic from focus_tracker.rs.
/// For now, delegates to the old module.
pub fn start(app: tauri::AppHandle) {
    crate::focus_tracker::start_focus_tracking(app);
    TRACKING_ACTIVE.store(true, Ordering::SeqCst);
}

/// Stop focus tracking.
pub fn stop() {
    crate::focus_tracker::stop_focus_tracking();
    TRACKING_ACTIVE.store(false, Ordering::SeqCst);
}

/// Returns true if focus tracking is active.
pub fn is_active() -> bool {
    TRACKING_ACTIVE.load(Ordering::SeqCst)
}
