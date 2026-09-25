//! Linux focus tracking (not available). See capture_linux.rs.

use crate::{FocusSink, PlatformError};

pub fn start(_sink: FocusSink) -> Result<(), PlatformError> {
    Err(PlatformError::NotSupported)
}

pub fn stop() {}

pub fn is_active() -> bool {
    false
}
