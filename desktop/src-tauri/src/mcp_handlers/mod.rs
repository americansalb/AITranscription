//! MCP-tool handler modules — Phase 1 of the hot-reload architecture
//! (spec `.vaak/design-notes/2026-05-28-hot-reload-architecture-spec.md`,
//! commit 184d10d, per human msg 2415).
//!
//! Each handler in this module is the Tauri-side authoritative implementation
//! of an MCP tool that the sidecar (`bin/vaak-mcp.rs`) used to own. The
//! sidecar becomes a thin HTTP forwarder; behavior changes ship via Vaak
//! restart only, not via Claude Code window restart.
//!
//! Phase 1 (this commit chain): `assembly_line` migrates here.
//! Phase 2: all `currency_*` handlers.
//! Phase 3: `oxford_*` / `delphi_*` / `assembly_*` (the active-mode tools).
//! Phase 4: `project_send` + remaining core tools.
//! Phase 5: auto-handshake on Vaak restart.

pub mod assembly_line;
