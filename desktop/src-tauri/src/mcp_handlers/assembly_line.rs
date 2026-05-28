//! Tauri-side implementation of the `assembly_line` MCP tool, migrated from
//! `bin/vaak-mcp.rs:handle_assembly_line`. The sidecar now POSTs
//! `127.0.0.1:7865/mcp/assembly_line` and this module owns the logic.
//!
//! Phase 1 of the hot-reload architecture per architect spec
//! `.vaak/design-notes/2026-05-28-hot-reload-architecture-spec.md` (commit
//! 184d10d) + human msg 2415 directive.
//!
//! Skeleton commit (developer:0 Phase 1 Step 1). The actual function moves
//! (apply_set_preset + seed_rotation_order_force + seed_rotation_order_if_empty
//! + protocol_normalize_in_place + protocol_active_seats_set) follow in
//! subsequent commits in this chain. Keeping skeleton + helpers + endpoint +
//! sidecar-forwarder as separate commits so dev-challenger:0 / evil-architect:0
//! can attack each step independently per architect spec encouragement.

// NOTE: function bodies land in step 2 of the phase-1 plan. This file currently
// declares the public surface and references the spec. Compilation green-
// lights the module skeleton; subsequent commits fill in implementations
// without touching the sidecar (still pointed at the old vaak-mcp.rs
// handler) until step 5 wires the HTTP forwarder.
