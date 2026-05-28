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

/// SHA-HR.1.2b — moved from `bin/vaak-mcp.rs:3655` (seed_rotation_order_if_empty).
///
/// Seed `floor.rotation_order` from `active_seats` when the floor is
/// rotation-driven (`mode == "round-robin"`) AND the existing rotation_order
/// is empty. Also seeds `floor.current_speaker` to `rotation_order[0]` when
/// currently null/empty so the freshly-enabled assembly has a first speaker.
///
/// Idempotent and conservative: never overwrites a non-empty rotation_order
/// (an explicit moderator-set order survives a subsequent set_preset
/// re-invocation), and never touches non-round-robin floors. Empty
/// `active_seats` → no-op.
pub(crate) fn seed_rotation_order_if_empty(
    state: &mut serde_json::Value,
    active_seats: &std::collections::HashSet<String>,
) {
    let floor_mode = state
        .get("floor")
        .and_then(|f| f.get("mode"))
        .and_then(|m| m.as_str())
        .unwrap_or("");
    if floor_mode != "round-robin" {
        return;
    }
    if active_seats.is_empty() {
        return;
    }
    let floor = match state.get_mut("floor").and_then(|f| f.as_object_mut()) {
        Some(f) => f,
        None => return,
    };
    let rot_empty = floor
        .get("rotation_order")
        .and_then(|v| v.as_array())
        .map(|a| a.is_empty())
        .unwrap_or(true);
    if !rot_empty {
        return;
    }
    let mut seats: Vec<String> = active_seats.iter().cloned().collect();
    seats.sort();
    let arr: Vec<serde_json::Value> = seats
        .iter()
        .map(|s| serde_json::Value::String(s.clone()))
        .collect();
    floor.insert(
        "rotation_order".to_string(),
        serde_json::Value::Array(arr.clone()),
    );
    let cs_empty = floor
        .get("current_speaker")
        .map(|v| v.is_null() || v.as_str().map(|s| s.is_empty()).unwrap_or(false))
        .unwrap_or(true);
    if cs_empty {
        if let Some(first) = arr.first() {
            floor.insert("current_speaker".to_string(), first.clone());
        }
    }
}

/// SHA-HR.1.2b — moved from `bin/vaak-mcp.rs:3727` (seed_rotation_order_force).
///
/// SHA-13.4 (architect msg 2330, evil-arch msg 2328 empirical): ALWAYS
/// overwrite `floor.rotation_order` from `active_seats` + stamp
/// `floor.started_at = now`, regardless of prior rotation_order state.
/// Used ONLY when the caller wants to force a re-seed (set_preset /
/// set_assembly dispatch). Other call sites (defensive heal in
/// handle_project_status) keep `seed_rotation_order_if_empty` to preserve
/// moderator-customized orders.
///
/// `utc_now_iso` from sidecar is sidecar-local — using `crate::collab::iso_now`
/// which is the equivalent shared helper main.rs already consumes.
pub(crate) fn seed_rotation_order_force(
    state: &mut serde_json::Value,
    active_seats: &std::collections::HashSet<String>,
) {
    let floor_mode = state
        .get("floor")
        .and_then(|f| f.get("mode"))
        .and_then(|m| m.as_str())
        .unwrap_or("");
    if floor_mode != "round-robin" {
        return;
    }
    if active_seats.is_empty() {
        return;
    }
    let floor = match state.get_mut("floor").and_then(|f| f.as_object_mut()) {
        Some(f) => f,
        None => return,
    };
    let mut seats: Vec<String> = active_seats.iter().cloned().collect();
    seats.sort();
    let arr: Vec<serde_json::Value> = seats
        .iter()
        .map(|s| serde_json::Value::String(s.clone()))
        .collect();
    floor.insert(
        "rotation_order".to_string(),
        serde_json::Value::Array(arr.clone()),
    );
    floor.insert(
        "started_at".to_string(),
        serde_json::json!(crate::collab::iso_now()),
    );
    let cs = floor
        .get("current_speaker")
        .and_then(|v| v.as_str())
        .map(String::from);
    let cs_valid = cs
        .as_ref()
        .map(|c| seats.iter().any(|s| s == c))
        .unwrap_or(false);
    if !cs_valid {
        if let Some(first) = arr.first() {
            floor.insert("current_speaker".to_string(), first.clone());
        }
    }
}

/// SHA-HR.1.2c — moved from `bin/vaak-mcp.rs:3789` (protocol_normalize_in_place).
///
/// JSON-Value mirror of `protocol::Protocol::normalize`. Three ratified rules
/// per spec §2.2 + evil-arch #923 + #954:
///   1. floor.mode == "free-grab" → clear floor.queue
///   2. orphan current_speaker (not in active_seats) → clear
///   3. prune dead queue entries
/// Empty `active_seats` → skip rules 2+3 (rule 1 still fires).
pub(crate) fn protocol_normalize_in_place(
    state: &mut serde_json::Value,
    active_seats: &std::collections::HashSet<String>,
) {
    let floor_mode = state
        .get("floor")
        .and_then(|f| f.get("mode"))
        .and_then(|v| v.as_str())
        .unwrap_or("none")
        .to_string();
    if let Some(floor) = state.get_mut("floor").and_then(|f| f.as_object_mut()) {
        if floor_mode == "free-grab" {
            floor.insert("queue".to_string(), serde_json::Value::Array(vec![]));
        }
        let cs = floor
            .get("current_speaker")
            .and_then(|v| v.as_str())
            .map(String::from);
        if let Some(cs_str) = &cs {
            if !active_seats.is_empty() && !active_seats.contains(cs_str) {
                floor.insert("current_speaker".to_string(), serde_json::Value::Null);
            }
        }
        if !active_seats.is_empty() {
            if let Some(arr) = floor
                .get_mut("queue")
                .and_then(|q| q.as_array_mut())
            {
                arr.retain(|v| {
                    v.as_str()
                        .map(|s| active_seats.contains(s))
                        .unwrap_or(false)
                });
            }
        }
    }
}

/// SHA-HR.1.2d — moved from `bin/vaak-mcp.rs:4194` (apply_set_preset).
///
/// The dispatcher arm called from `do_protocol_mutate_inner` for the
/// `"set_preset"` action. Writes preset / floor.mode / floor.assembly_active /
/// consensus.mode, with cross-mode exclusivity gates enforcing:
/// 1. V3 spec rule 10 (Assembly Line ↔ discussion preset mutex)
/// 2. V1.0.7 interim gate (cross-transitions must route through Default chat)
///
/// String constants `PRESET_*` from the sidecar are replaced with
/// `crate::protocol::Preset::Variant.as_wire_str()` which is the same
/// "Default chat" / "Assembly Line" / "Delphi" / "Oxford" / "Continuous Review"
/// wire strings via the typed `Preset` enum's `#[serde(rename = ...)]`.
pub(crate) fn apply_set_preset(
    state: &mut serde_json::Value,
    args: &serde_json::Value,
) -> Result<(), String> {
    use crate::protocol::Preset;

    let name = args
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or("[InvalidArgs] set_preset requires args.name (string)")?;
    let prev_preset = state
        .get("preset")
        .and_then(|p| p.as_str())
        .unwrap_or("");
    let is_discussion = |p: &str| {
        p == Preset::ContinuousReview.as_wire_str()
            || p == Preset::Delphi.as_wire_str()
            || p == Preset::Oxford.as_wire_str()
    };
    if name == Preset::AssemblyLine.as_wire_str() && is_discussion(prev_preset) {
        return Err(format!(
            "[ConflictWithDiscussion] cannot set preset to '{}' while a discussion preset ('{}') is active. \
             Set preset to '{}' first, then enable {}.",
            Preset::AssemblyLine.as_wire_str(),
            prev_preset,
            Preset::DefaultChat.as_wire_str(),
            Preset::AssemblyLine.as_wire_str()
        ));
    }
    if is_discussion(name) && prev_preset == Preset::AssemblyLine.as_wire_str() {
        return Err(format!(
            "[ConflictWithAssembly] cannot set preset to '{}' while Assembly Line is active. \
             Disable Assembly Line first (set preset to 'Default chat'), then start the discussion.",
            name
        ));
    }

    // V1.0.7 cross-transition gate + cold-open carve-out (per SHA-V107.fix-1
    // commit 177669b).
    if !prev_preset.is_empty()
        && prev_preset != Preset::DefaultChat.as_wire_str()
        && name != Preset::DefaultChat.as_wire_str()
        && prev_preset != name
    {
        return Err(format!(
            "[ConflictWithActivePreset] cannot transition preset directly from '{}' to '{}' — \
             route through '{}' first to avoid floor.mode + rotation_order drift while the prior \
             mode's state is still live. v1.0.7 interim gate per multi-writer audit Instance 4.",
            prev_preset,
            name,
            Preset::DefaultChat.as_wire_str()
        ));
    }

    // V1.5.0 commit 2/6: typed Preset enum dispatch.
    let preset: Preset = match serde_json::from_value(serde_json::Value::String(name.to_string())) {
        Ok(p) => p,
        Err(_) => {
            return Err(format!(
                "[InvalidArgs] unknown preset '{}' — see spec §6 matrix for valid names",
                name
            ));
        }
    };
    let (floor_mode, consensus_mode) = match preset {
        Preset::DefaultChat => ("none", "none"),
        Preset::Debate => ("reactive", "none"),
        Preset::AssemblyLine => ("round-robin", "none"),
        Preset::TownHall => ("queue", "none"),
        Preset::Brainstorm => ("free-grab", "none"),
        Preset::ContinuousReview => ("free-grab", "tally"),
        Preset::Delphi => ("round-robin", "vote"),
        Preset::Oxford => ("queue", "vote"),
    };
    if let Some(obj) = state.as_object_mut() {
        obj.insert("preset".to_string(), serde_json::json!(preset.as_wire_str()));
    }
    let is_assembly_preset = matches!(preset, Preset::AssemblyLine);
    if let Some(floor) = state.get_mut("floor").and_then(|f| f.as_object_mut()) {
        floor.insert("mode".to_string(), serde_json::json!(floor_mode));
        floor.insert(
            "assembly_active".to_string(),
            serde_json::json!(is_assembly_preset),
        );
        if !is_assembly_preset {
            floor.insert("current_speaker".to_string(), serde_json::Value::Null);
            floor.insert("moderator".to_string(), serde_json::Value::Null);
        }
    }
    if let Some(cons) = state.get_mut("consensus").and_then(|c| c.as_object_mut()) {
        cons.insert("mode".to_string(), serde_json::json!(consensus_mode));
    }
    Ok(())
}
