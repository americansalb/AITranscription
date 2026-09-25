//! The platform layer.
//!
//! Every operating-system call the product makes lives in this crate, behind
//! one interface with one implementation per platform. Nothing outside this
//! crate may name a platform API. That rule is what makes the second platform
//! a copy of an interface instead of an archaeology dig.
//!
//! Capabilities are reported, never assumed. A platform that cannot do
//! something says so, so the app can say so aloud.

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
compile_error!("liberty-platform supports macOS, Windows, and Linux only");

pub mod announce;
pub mod types;

#[cfg(target_os = "macos")]
mod capture_macos;
#[cfg(target_os = "macos")]
mod focus_macos;

#[cfg(target_os = "windows")]
mod capture_windows;
#[cfg(target_os = "windows")]
mod focus_windows;

#[cfg(target_os = "linux")]
mod capture_linux;
#[cfg(target_os = "linux")]
mod focus_linux;

pub use announce::{FocusEvent, FocusSink};
pub use types::*;

use serde::{Deserialize, Serialize};

/// What this platform can do. Reported from compile-time facts, so it is
/// always honest, and it is what the app reads aloud when asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capabilities {
    pub platform: &'static str,
    pub accessibility_tree: bool,
    pub focus_tracking: bool,
}

impl Capabilities {
    /// One sentence a person can hear instead of reading a struct.
    pub fn spoken(&self) -> String {
        let yes_no = |b: bool| if b { "yes" } else { "no" };
        format!(
            "Platform {}. Accessibility tree: {}. Focus tracking: {}.",
            self.platform,
            yes_no(self.accessibility_tree),
            yes_no(self.focus_tracking),
        )
    }
}

pub fn capabilities() -> Capabilities {
    #[cfg(target_os = "macos")]
    {
        Capabilities { platform: "macos", accessibility_tree: true, focus_tracking: true }
    }
    #[cfg(target_os = "windows")]
    {
        Capabilities { platform: "windows", accessibility_tree: true, focus_tracking: true }
    }
    #[cfg(target_os = "linux")]
    {
        Capabilities { platform: "linux", accessibility_tree: false, focus_tracking: false }
    }
}

/// Capture the accessibility tree of the foreground window, normalized to one
/// schema. Coordinates use a top-left screen origin on every platform.
pub fn capture_tree() -> Result<NormalizedTree, String> {
    #[cfg(target_os = "macos")]
    {
        capture_macos::capture()
    }
    #[cfg(target_os = "windows")]
    {
        capture_windows::capture()
    }
    #[cfg(target_os = "linux")]
    {
        capture_linux::capture()
    }
}

/// Start announcing keyboard focus changes. Each change that produces a
/// non-empty, non-duplicate announcement is handed to `sink`. The sink is
/// called on a background thread owned by this crate.
pub fn start_focus_tracking(sink: FocusSink) {
    #[cfg(target_os = "macos")]
    {
        focus_macos::start(sink)
    }
    #[cfg(target_os = "windows")]
    {
        focus_windows::start(sink)
    }
    #[cfg(target_os = "linux")]
    {
        focus_linux::start(sink)
    }
}

/// Stop focus tracking and let the background thread exit.
pub fn stop_focus_tracking() {
    #[cfg(target_os = "macos")]
    {
        focus_macos::stop()
    }
    #[cfg(target_os = "windows")]
    {
        focus_windows::stop()
    }
    #[cfg(target_os = "linux")]
    {
        focus_linux::stop()
    }
}

pub fn is_focus_tracking_active() -> bool {
    #[cfg(target_os = "macos")]
    {
        focus_macos::is_active()
    }
    #[cfg(target_os = "windows")]
    {
        focus_windows::is_active()
    }
    #[cfg(target_os = "linux")]
    {
        focus_linux::is_active()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capabilities_name_the_platform_we_compiled_for() {
        let caps = capabilities();
        assert_eq!(caps.platform, std::env::consts::OS);
    }

    #[test]
    fn capabilities_are_speakable() {
        let text = capabilities().spoken();
        assert!(text.starts_with("Platform "));
        assert!(text.contains("Accessibility tree: "));
        assert!(text.contains("Focus tracking: "));
    }

    #[test]
    fn focus_tracking_is_off_until_started() {
        assert!(!is_focus_tracking_active());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_reports_no_capabilities_and_captures_an_empty_tree() {
        let caps = capabilities();
        assert!(!caps.accessibility_tree);
        assert!(!caps.focus_tracking);
        let tree = capture_tree().expect("the linux stub never fails");
        assert_eq!(tree.platform, "linux");
        assert_eq!(tree.element_count, 0);
        assert!(tree.elements.is_empty());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_focus_tracking_never_activates() {
        start_focus_tracking(Box::new(|_event| {}));
        assert!(!is_focus_tracking_active());
        stop_focus_tracking();
    }
}
