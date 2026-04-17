// Library target for vaak-desktop crate.
// Exposes shared modules so that binary targets (e.g., vaak-mcp sidecar)
// can import types and functions without duplicating code.

pub mod build_info;
pub mod collab;
