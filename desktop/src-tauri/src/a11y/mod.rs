//! Platform-agnostic accessibility module.
//!
//! Provides a unified interface for capturing UI element trees and tracking
//! keyboard focus across Windows (UIA), macOS (AX API), and Linux (stub).
//!
//! # Architecture
//! - `types.rs` — Normalized schema (NormalizedTree, NormalizedElement, NormalizedRole)
//! - `capture_*.rs` — Platform-specific tree capture implementations
//! - `focus_*.rs` — Platform-specific focus tracking implementations
//!
//! Each platform module implements the same function signatures. Dispatch is
//! compile-time via `#[cfg]` — no trait vtable overhead.

pub mod types;

pub use types::*;

// ---------------------------------------------------------------------------
// Tree capture — platform dispatch
// ---------------------------------------------------------------------------

#[cfg(target_os = "windows")]
mod capture_windows;

#[cfg(target_os = "macos")]
mod capture_macos;

#[cfg(target_os = "linux")]
mod capture_linux;

/// Capture the accessibility tree from the foreground window.
///
/// Returns a [`NormalizedTree`] with platform-agnostic element data.
/// All coordinates use top-left screen origin regardless of platform.
pub fn capture_tree() -> Result<NormalizedTree, String> {
    #[cfg(target_os = "windows")]
    {
        capture_windows::capture()
    }
    #[cfg(target_os = "macos")]
    {
        capture_macos::capture()
    }
    #[cfg(target_os = "linux")]
    {
        capture_linux::capture()
    }
}

// ---------------------------------------------------------------------------
// Focus tracking — platform dispatch
// ---------------------------------------------------------------------------

#[cfg(target_os = "windows")]
mod focus_windows;

#[cfg(target_os = "macos")]
mod focus_macos;

#[cfg(target_os = "linux")]
mod focus_linux;

/// Start tracking keyboard focus changes and emitting TTS announcements.
///
/// On Windows: polls UIA GetFocusedElement every 100ms on a dedicated thread.
/// On macOS: registers AXObserver for kAXFocusedUIElementChangedNotification
///           on a dedicated CFRunLoop thread.
/// On Linux: no-op (stub).
pub fn start_focus_tracking(app: tauri::AppHandle) {
    #[cfg(target_os = "windows")]
    {
        focus_windows::start(app);
    }
    #[cfg(target_os = "macos")]
    {
        focus_macos::start(app);
    }
    #[cfg(target_os = "linux")]
    {
        focus_linux::start(app);
    }
}

/// Stop focus tracking and clean up the observer thread.
pub fn stop_focus_tracking() {
    #[cfg(target_os = "windows")]
    {
        focus_windows::stop();
    }
    #[cfg(target_os = "macos")]
    {
        focus_macos::stop();
    }
    #[cfg(target_os = "linux")]
    {
        focus_linux::stop();
    }
}

/// Returns true if focus tracking is currently active.
pub fn is_focus_tracking_active() -> bool {
    #[cfg(target_os = "windows")]
    {
        focus_windows::is_active()
    }
    #[cfg(target_os = "macos")]
    {
        focus_macos::is_active()
    }
    #[cfg(target_os = "linux")]
    {
        focus_linux::is_active()
    }
}
