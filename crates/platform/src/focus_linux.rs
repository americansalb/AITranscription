//! Linux focus tracking (stub).
//!
//! A full implementation would use AT-SPI2 over D-Bus: register for
//! object:state-changed:focused events and read the focused object's name and
//! role. Not planned; the interface is here so the crate compiles and reports
//! itself honestly.

use crate::announce::FocusSink;

/// Focus tracking is not available on Linux. The sink is dropped unused.
pub fn start(_sink: FocusSink) {
    eprintln!("[platform/focus_linux] Focus tracking is not available on Linux");
}

pub fn stop() {}

pub fn is_active() -> bool {
    false
}
