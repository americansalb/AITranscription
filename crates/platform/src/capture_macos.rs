//! macOS capture: the window in front, read through the Accessibility API
//! into the shared schema.

use crate::ax::{self, AXUIElementRef};
use crate::{now_ms, PlatformError};
use core_graphics::display::CGDisplay;
use logic::schema::{App, Element, Limits, Snapshot};
use std::time::{Duration, Instant};

pub fn snapshot(limits: &Limits) -> Result<Snapshot, PlatformError> {
    if !ax::trusted() {
        return Err(PlatformError::NotPermitted);
    }
    let started = Instant::now();
    let system = ax::system_wide();

    // Ask the accessibility system which app is in front. No AppleScript, no
    // second permission, no round trip through another process.
    let app = ax::element_attr(system.element(), "AXFocusedApplication")
        .ok_or(PlatformError::NoForegroundWindow)?;
    let pid = ax::pid(app.element()).unwrap_or(0);
    if pid > 0 && pid as u32 == std::process::id() {
        return Err(PlatformError::OwnWindow);
    }
    let app_name = ax::string_attr(app.element(), "AXTitle");

    // Electron apps only expose their tree when asked. Harmless elsewhere.
    ax::set_true(app.element(), "AXManualAccessibility");

    let window = ax::element_attr(app.element(), "AXFocusedWindow")
        .ok_or_else(|| PlatformError::NoWindow { app: app_name.clone() })?;
    let window_title = ax::string_attr(window.element(), "AXTitle");

    let mut walker = Walker::new(limits, started);
    let mut elements = walker.children_of(window.element(), 0);
    if Snapshot::needs_web_flags(&elements) {
        // Chromium builds its web tree only once it believes a screen reader
        // is present. Raise our hand and look once more.
        ax::set_true(app.element(), "AXEnhancedUserInterface");
        walker = Walker::new(limits, started);
        elements = walker.children_of(window.element(), 0);
    }

    Ok(Snapshot {
        platform: "macos".to_string(),
        app: App { name: app_name, pid: pid.max(0) as u32 },
        window_title,
        scale: main_display_scale(),
        captured_at_ms: now_ms(),
        elapsed_ms: started.elapsed().as_millis() as u64,
        truncated: walker.truncated,
        element_count: Snapshot::count_elements(&elements),
        elements,
    })
}

pub fn focused_element() -> Result<Element, PlatformError> {
    if !ax::trusted() {
        return Err(PlatformError::NotPermitted);
    }
    let system = ax::system_wide();
    let focused = ax::element_attr(system.element(), "AXFocusedUIElement")
        .ok_or(PlatformError::NoForegroundWindow)?;
    if ax::pid(focused.element()).map(|p| p as u32) == Some(std::process::id()) {
        return Err(PlatformError::OwnWindow);
    }
    Ok(ax::read_element(focused.element(), 1))
}

/// Pixels per point on the main display, for mapping to screenshots.
fn main_display_scale() -> f64 {
    let display = CGDisplay::main();
    let points = display.bounds().size.height;
    let pixels = display.pixels_high() as f64;
    if points > 0.0 && pixels > 0.0 {
        pixels / points
    } else {
        1.0
    }
}

struct Walker<'a> {
    limits: &'a Limits,
    started: Instant,
    budget: Duration,
    count: u32,
    next_id: u32,
    truncated: bool,
}

impl<'a> Walker<'a> {
    fn new(limits: &'a Limits, started: Instant) -> Walker<'a> {
        Walker {
            limits,
            started,
            budget: Duration::from_millis(limits.budget_ms),
            count: 0,
            next_id: 0,
            truncated: false,
        }
    }

    fn children_of(&mut self, parent: AXUIElementRef, depth: u32) -> Vec<Element> {
        if depth >= self.limits.max_depth {
            self.truncated = true;
            return Vec::new();
        }
        let mut out = Vec::new();
        for child in ax::children(parent) {
            if self.count >= self.limits.max_elements || self.started.elapsed() > self.budget {
                self.truncated = true;
                break;
            }
            self.count += 1;
            self.next_id += 1;
            let mut element = ax::read_element(child.element(), self.next_id);
            element.children = self.children_of(child.element(), depth + 1);
            out.push(element);
        }
        out
    }
}
