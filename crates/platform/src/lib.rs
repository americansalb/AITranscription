//! The platform layer.
//!
//! Every operating-system call the product makes lives in this crate, behind
//! one interface with one implementation per platform. Nothing outside this
//! crate may name a platform API. That rule is what makes the second platform
//! a copy of an interface instead of an archaeology dig.
//!
//! Capabilities are reported, never assumed, and every error has a spoken
//! sentence, so the app can always say what is going on.

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
compile_error!("the platform crate supports macOS, Windows, and Linux only");

mod error;

#[cfg(target_os = "macos")]
mod ax;
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

pub use error::PlatformError;
pub use logic::schema::{App, Element, Limits, Rect, Role, Snapshot};

#[cfg(target_os = "macos")]
use capture_macos as capture;
#[cfg(target_os = "macos")]
use focus_macos as focus;
#[cfg(target_os = "windows")]
use capture_windows as capture;
#[cfg(target_os = "windows")]
use focus_windows as focus;
#[cfg(target_os = "linux")]
use capture_linux as capture;
#[cfg(target_os = "linux")]
use focus_linux as focus;

/// What this platform can do, from compile-time facts, so it is always
/// honest. It is what the app reads aloud when asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    let supported = cfg!(any(target_os = "macos", target_os = "windows"));
    Capabilities {
        platform: std::env::consts::OS,
        accessibility_tree: supported,
        focus_tracking: supported,
    }
}

/// Capture the window in front as one snapshot, within the given limits.
/// Secure fields never carry a value, and our own windows are refused.
pub fn snapshot(limits: &Limits) -> Result<Snapshot, PlatformError> {
    capture::snapshot(limits)
}

/// The element that has keyboard focus right now, without its children.
pub fn focused_element() -> Result<Element, PlatformError> {
    capture::focused_element()
}

/// One change of keyboard focus, with the sentence to speak already built.
#[derive(Debug, Clone, PartialEq)]
pub struct FocusEvent {
    pub element: Element,
    pub sentence: String,
    pub timestamp_ms: u64,
}

/// Where focus events go. Called on a background thread owned by this
/// crate, so it must be safe to send and share.
pub type FocusSink = Box<dyn Fn(FocusEvent) + Send + Sync + 'static>;

/// Start announcing keyboard focus changes. Each change with something worth
/// saying, and different from the last, is handed to `sink`.
pub fn start_focus_tracking(sink: FocusSink) -> Result<(), PlatformError> {
    focus::start(sink)
}

/// Stop focus tracking and let the background thread exit.
pub fn stop_focus_tracking() {
    focus::stop()
}

pub fn is_focus_tracking_active() -> bool {
    focus::is_active()
}

/// Milliseconds since the Unix epoch, or zero if the clock is unavailable.
pub(crate) fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capabilities_name_the_platform_we_compiled_for() {
        assert_eq!(capabilities().platform, std::env::consts::OS);
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
    fn linux_is_honest_about_having_nothing() {
        let caps = capabilities();
        assert!(!caps.accessibility_tree);
        assert!(!caps.focus_tracking);
        assert_eq!(snapshot(&Limits::default()), Err(PlatformError::NotSupported));
        assert_eq!(focused_element(), Err(PlatformError::NotSupported));
        assert_eq!(start_focus_tracking(Box::new(|_| {})), Err(PlatformError::NotSupported));
        assert!(!is_focus_tracking_active());
        stop_focus_tracking();
    }
}
