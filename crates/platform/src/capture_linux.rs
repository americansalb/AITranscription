//! Linux capture (not available).
//!
//! A real implementation would use AT-SPI2 over D-Bus. Not planned. The
//! functions exist so the crate compiles everywhere and says so honestly.

use crate::PlatformError;
use logic::schema::{Element, Limits, Snapshot};

pub fn snapshot(_limits: &Limits) -> Result<Snapshot, PlatformError> {
    Err(PlatformError::NotSupported)
}

pub fn focused_element() -> Result<Element, PlatformError> {
    Err(PlatformError::NotSupported)
}
