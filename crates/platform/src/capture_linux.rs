//! Linux accessibility tree capture (stub).
//!
//! A full implementation would use AT-SPI2 over D-Bus:
//! - org.a11y.atspi.Registry for registering as an AT client
//! - org.a11y.atspi.Accessible interface for element traversal
//! - GetChildAtIndex / GetChildren for tree walking
//!
//! This is not currently planned but the module structure is ready.

use super::types::*;

/// Stub — returns empty tree on Linux.
pub fn capture() -> Result<NormalizedTree, String> {
    Ok(NormalizedTree {
        window_title: String::new(),
        process_name: String::new(),
        platform: "linux".to_string(),
        element_count: 0,
        elements: Vec::new(),
    })
}
