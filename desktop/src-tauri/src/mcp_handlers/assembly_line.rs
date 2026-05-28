//! Tauri-side implementation of the `assembly_line` MCP tool, migrated from
//! `bin/vaak-mcp.rs:handle_assembly_line`. The sidecar will POST
//! `127.0.0.1:7865/mcp/assembly_line` and this module owns the logic.
//!
//! Phase 1 of the hot-reload architecture per architect spec
//! `.vaak/design-notes/2026-05-28-hot-reload-architecture-spec.md` + human
//! msg 2415 directive. SHA-HR.1.2 lands the first helper move per the
//! incremental step plan (developer:0 msg 2421).
//!
//! Lock-residency audit per evil-arch msg 2434 F1 + developer:0 msg 2437 +
//! tester:0 msg 2439: all 5 helpers contain ZERO inner lock primitives. They
//! operate on `&mut serde_json::Value` references with file/board locks held
//! at the `do_protocol_mutate` wrapper layer. Relocating bodies preserves
//! cross-process locking semantics — no refactor needed.

/// Active-seat set sourced from sessions.json. Used by the JSON-Value
/// `protocol_normalize_in_place` mirror of `protocol::Protocol::normalize`
/// per evil-arch #978 + architect #979 ship-block fix (Slice 5 follow-on).
///
/// SHA-HR.1.2: moved from `bin/vaak-mcp.rs:3622` with the `read_sessions`
/// dependency inlined (the sidecar-local `read_sessions` reader isn't in a
/// shared module; rather than extract it as a prerequisite step, inline the
/// 4-line sessions.json read here — pure data-shape transform, no lock
/// concerns).
pub(crate) fn protocol_active_seats_set(
    project_dir: &str,
) -> std::collections::HashSet<String> {
    let mut set = std::collections::HashSet::new();
    let sessions_path = std::path::Path::new(project_dir)
        .join(".vaak")
        .join("sessions.json");
    let sessions: serde_json::Value = std::fs::read_to_string(&sessions_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| serde_json::json!({"bindings": []}));
    if let Some(bindings) = sessions.get("bindings").and_then(|b| b.as_array()) {
        for b in bindings {
            let role = b.get("role").and_then(|v| v.as_str()).unwrap_or("");
            let inst = b.get("instance").and_then(|v| v.as_u64()).unwrap_or(0);
            let status = b.get("status").and_then(|v| v.as_str()).unwrap_or("");
            if !role.is_empty() && status == "active" {
                set.insert(format!("{}:{}", role, inst));
            }
        }
    }
    set
}
