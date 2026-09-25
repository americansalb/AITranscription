//! Pure logic. No operating system calls, no network, no clock.
//!
//! Everything here is a function of its inputs, so every test in this crate
//! runs on Linux, and the Ubuntu runner catches regressions without a Mac.

pub mod announce;
pub mod describe;
pub mod schema;

/// The product's working name, read at compile time from the PRODUCT_NAME
/// file at the repository root.
///
/// That file and the readme heading are the only two places the name is
/// written. No crate, module, file, identifier, or message contains it, and
/// tests/product_name.rs fails if it ever leaks into one. Renaming the
/// product is editing that file: nothing in code changes.
pub const PRODUCT_NAME: &str = include_str!("../../../PRODUCT_NAME").trim_ascii();
