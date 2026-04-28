// Protocol — unified per-section floor + consensus state machine.
//
// One file per section: `.vaak/sections/<section>/protocol.json` (default
// section: `.vaak/protocol.json`). Replaces the legacy split between
// `assembly.json` (floor rotation) and `discussion.json` (consensus rounds)
// per Assembly Line architecture v6 (`.vaak/al-architecture-diagram.md`
// §3 — Slice 1 scope).
//
// ============================================================
// Resilience-stack timer registry (per evil-arch #923 + dev-chall #917.1)
// ============================================================
// Five named constants drive different layers of the spec. They live where
// they are consumed — keep them decentralized but discoverable. Update both
// this block AND the mirror in vaak-mcp.rs whenever a constant moves.
//
//   floor.threshold_ms (per-section, default 60_000)
//                                       — protocol.rs::MIC_GRAB_THRESHOLD_MS
//                                         (mic freshness gate, spec §2)
//   SUPERVISOR_STALL_SECS = 90          — vaak-mcp.rs supervisor loop
//                                         (90s stall before pre-kill buzz,
//                                          spec §12.2 Layer 2)
//   PRE_KILL_GRACE_SECS = 5             — vaak-mcp.rs supervisor loop
//                                         (5s grace after buzz before
//                                          taskkill, spec §12.2)
//   KEEP_ALIVE_DEBOUNCE_MS ≈ 10_000     — composer (UI) keystroke heartbeat
//                                         (debounced fire to avoid storm,
//                                          spec §3.1)
//   MIC_AUTOROTATE_SECS = 600           — assembly_line auto-rotation
//                                         (10-min idle = grab, human #903)
//
// These are NOT collapsed into a single timer — see spec §2 ("Single
// threshold" applies to the freshness gate only) + memory entry on three-
// timestamps-three-consumers.
// ============================================================
//
// Slice 1 scope (this file):
//   - Schema struct + serde
//   - Path helpers
//   - Lazy read with migration from legacy files on first read
//   - Atomic-rename write via the existing collab::atomic_write
//   - Board.lock discipline reuses collab::with_board_lock
//
// Out of scope (later slices):
//   - MCP tools (`protocol_mutate` / `get_protocol`) — Slice 2
//   - Panel UI / composer flow                     — Slices 3 + 4
//   - Phase plan execution                         — Slice 5
//   - Legacy MCP-tool deprecation                  — Slice 6

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::collab::{atomic_write, get_active_section, iso_now, with_board_lock};

// Single-source threshold (mirrors spec §2 / vaak-mcp.rs MIC_GRAB_THRESHOLD_MS).
pub const MIC_GRAB_THRESHOLD_MS: u64 = 60_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Floor {
    pub mode: String,                       // none | reactive | round-robin | queue | free-grab
    #[serde(default)]
    pub current_speaker: Option<String>,
    #[serde(default)]
    pub queue: Vec<String>,
    #[serde(default)]
    pub rotation_order: Vec<String>,
    #[serde(default = "default_threshold_ms")]
    pub threshold_ms: u64,
    #[serde(default)]
    pub started_at: Option<String>,
}

fn default_threshold_ms() -> u64 { MIC_GRAB_THRESHOLD_MS }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusRound {
    #[serde(default)]
    pub topic: Option<String>,
    #[serde(default)]
    pub opened_at: Option<String>,
    #[serde(default)]
    pub opened_by: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Consensus {
    pub mode: String,                       // none | silence-consent | vote | tally
    #[serde(default)]
    pub round: Option<ConsensusRound>,
    #[serde(default)]
    pub phase: Option<String>,              // submitting | reviewing | closed
    #[serde(default)]
    pub submissions: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhaseOutcome {
    pub kind: String,                       // file_nonempty | vote_quorum | timer | manual
    #[serde(default)]
    pub target: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Phase {
    pub preset: String,
    #[serde(default)]
    pub duration_secs: u64,
    #[serde(default)]
    pub extension_secs: u64,
    pub outcome: PhaseOutcome,
    #[serde(default)]
    pub started_at: Option<String>,
    #[serde(default)]
    pub ended_at: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PhasePlan {
    #[serde(default)]
    pub phases: Vec<Phase>,
    #[serde(default)]
    pub current_phase_idx: usize,
    #[serde(default)]
    pub paused_at: Option<String>,
    #[serde(default)]
    pub paused_total_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scopes {
    #[serde(default = "default_floor_scope")]
    pub floor: String,                      // instance
    #[serde(default = "default_consensus_scope")]
    pub consensus: String,                  // role
}

fn default_floor_scope() -> String { "instance".to_string() }
fn default_consensus_scope() -> String { "role".to_string() }

impl Default for Scopes {
    fn default() -> Self {
        Self { floor: default_floor_scope(), consensus: default_consensus_scope() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Protocol {
    // Anchor for future on-disk schema migrations (evil-arch #923).
    // `rev` is per-write monotonic and doesn't survive structural changes —
    // schema_version does.
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub rev: u64,
    #[serde(default = "default_preset")]
    pub preset: String,
    pub floor: Floor,
    pub consensus: Consensus,
    #[serde(default)]
    pub phase_plan: PhasePlan,
    #[serde(default)]
    pub scopes: Scopes,
    #[serde(default)]
    pub last_writer_seat: Option<String>,
    #[serde(default)]
    pub last_writer_action: Option<String>,
    #[serde(default)]
    pub rev_at: Option<String>,
}

fn default_schema_version() -> u32 { 1 }
fn default_preset() -> String { "Debate".to_string() }

impl Protocol {
    pub fn fresh() -> Self {
        Self {
            schema_version: default_schema_version(),
            rev: 0,
            preset: default_preset(),
            floor: Floor {
                mode: "reactive".to_string(),
                current_speaker: None,
                queue: vec![],
                rotation_order: vec![],
                threshold_ms: MIC_GRAB_THRESHOLD_MS,
                started_at: Some(iso_now()),
            },
            consensus: Consensus {
                mode: "none".to_string(),
                round: None,
                phase: None,
                submissions: vec![],
            },
            phase_plan: PhasePlan {
                phases: vec![],
                current_phase_idx: 0,
                paused_at: None,
                paused_total_secs: 0,
            },
            scopes: Scopes::default(),
            last_writer_seat: None,
            last_writer_action: None,
            rev_at: None,
        }
    }

    /// Apply structural fixups required by spec §2.1 transition rules and
    /// §2.2 invariants. Three rules (Slice 5 — implements the body that
    /// architect left as no-op stub at 27f4eee):
    ///
    /// 1. `floor.mode == "free-grab"` → clear `floor.queue`. (Spec §2.2
    ///    edge case row 4: "any→free-grab dissolves the queue.")
    /// 2. `floor.current_speaker` references a seat not in `active_seats` →
    ///    clear current_speaker. Prevents the orphan-speaker invariant
    ///    violation when a held mic outlives its holder.
    /// 3. Queue entries that reference seats not in `active_seats` → prune.
    ///    Spec §2.2 invariant 2: every queue entry references a live seat.
    ///
    /// `active_seats` is the caller's view of which `role:instance` strings
    /// currently have an `active` binding in sessions.json. Pass an empty
    /// set to skip rules 2 + 3 (rule 1 still fires on free-grab).
    pub fn normalize(&mut self, active_seats: &std::collections::HashSet<String>) {
        // Rule 1: free-grab dissolves the queue.
        if self.floor.mode == "free-grab" {
            self.floor.queue.clear();
        }

        // Rule 2: orphan current_speaker → clear.
        if let Some(cs) = &self.floor.current_speaker {
            if !active_seats.is_empty() && !active_seats.contains(cs) {
                self.floor.current_speaker = None;
            }
        }

        // Rule 3: prune dead queue entries.
        if !active_seats.is_empty() {
            self.floor.queue.retain(|s| active_seats.contains(s));
        }
    }
}

// ==================== Path helpers ====================

pub fn protocol_path_for_section(dir: &str, section: &str) -> PathBuf {
    if section == "default" {
        Path::new(dir).join(".vaak").join("protocol.json")
    } else {
        Path::new(dir).join(".vaak").join("sections").join(section).join("protocol.json")
    }
}

pub fn active_protocol_path(dir: &str) -> PathBuf {
    protocol_path_for_section(dir, &get_active_section(dir))
}

fn legacy_assembly_path_for_section(dir: &str, section: &str) -> PathBuf {
    if section == "default" {
        Path::new(dir).join(".vaak").join("assembly.json")
    } else {
        Path::new(dir).join(".vaak").join("sections").join(section).join("assembly.json")
    }
}

fn legacy_discussion_path_for_section(dir: &str, section: &str) -> PathBuf {
    if section == "default" {
        Path::new(dir).join(".vaak").join("discussion.json")
    } else {
        Path::new(dir).join(".vaak").join("sections").join(section).join("discussion.json")
    }
}

fn legacy_archive_dir(dir: &str, section: &str) -> PathBuf {
    Path::new(dir).join(".vaak").join("legacy").join(section)
}

// ==================== Read / write / migrate ====================

/// Read protocol state for a section. On first read with no `protocol.json`,
/// synthesize from legacy `assembly.json` / `discussion.json` and archive
/// them (spec §3.3).
///
/// **Concurrency** (per evil-arch #929 round of review): the read-or-migrate
/// path acquires `board.lock` so two concurrent first-readers don't both
/// rename the same legacy files (second rename would silently no-op, then
/// the second migration writes empty defaults over the first's correct
/// state — silent data loss).
///
/// **Corrupted-file recovery** (same review): if `protocol.json` exists but
/// fails to parse, we DO NOT silently fall through to legacy migration —
/// legacy files have already been archived in a prior run, so re-migration
/// would produce empty defaults and overwrite the corrupted file with no
/// warning. Instead we eprintln the error and return `Protocol::fresh()`
/// without writing, leaving the bad file in place for human inspection.
pub fn read_protocol_for_section(dir: &str, section: &str) -> Protocol {
    let path = protocol_path_for_section(dir, section);

    // Fast path: file exists and parses cleanly. No lock needed for plain reads.
    if let Ok(content) = std::fs::read_to_string(&path) {
        match serde_json::from_str::<Protocol>(&content) {
            Ok(p) => return p,
            Err(e) => {
                eprintln!(
                    "[protocol] {} exists but failed to parse: {}. \
                     Returning fresh defaults; not overwriting on disk so \
                     the corrupted file is preserved for inspection.",
                    path.display(), e
                );
                return Protocol::fresh();
            }
        }
    }

    // No protocol.json — migrate under board.lock so concurrent first-readers
    // serialize. Inside the lock, re-check for protocol.json (a peer may have
    // migrated in the gap between our miss and the lock acquire); only one
    // caller actually performs the rename + write.
    let dir_owned = dir.to_string();
    let section_owned = section.to_string();
    let path_for_recheck = path.clone();
    let result: Result<Protocol, String> = with_board_lock(dir, move || {
        if let Ok(content) = std::fs::read_to_string(&path_for_recheck) {
            if let Ok(p) = serde_json::from_str::<Protocol>(&content) {
                return Ok(p);
            }
        }
        // Belt-and-suspenders ordering (dev-chall #930): synthesize the migrated
        // Protocol from legacy, then write protocol.json FIRST, archive legacy
        // ONLY on successful write. A crash between write and archive leaves a
        // valid protocol.json plus the legacy files (next read short-circuits
        // on the parsed protocol.json and skips archive — orphaned legacy is
        // benign vs. silent data loss). A crash before write leaves legacy
        // intact for retry. Lock + reorder = double safety per #931.
        let (migrated, did_migrate) =
            synthesize_from_legacy_for_section(&dir_owned, &section_owned);
        write_protocol_for_section_unlocked(&dir_owned, &section_owned, &migrated)?;
        if did_migrate {
            archive_legacy_for_section(&dir_owned, &section_owned);
        }
        Ok(migrated)
    });

    match result {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[protocol] migration under board.lock failed: {}. \
                       Returning fresh defaults (no disk write).", e);
            Protocol::fresh()
        }
    }
}

pub fn read_protocol(dir: &str) -> Protocol {
    read_protocol_for_section(dir, &get_active_section(dir))
}

/// Write protocol state, atomic-rename. **Caller must hold `board.lock`**;
/// writes outside the lock can race with a concurrent mutator.
pub fn write_protocol_for_section_unlocked(
    dir: &str,
    section: &str,
    state: &Protocol,
) -> Result<(), String> {
    let path = protocol_path_for_section(dir, section);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let json = serde_json::to_string_pretty(state)
        .map_err(|e| format!("Failed to serialize protocol: {}", e))?;
    atomic_write(&path, json.as_bytes())
        .map_err(|e| format!("Failed to write protocol.json: {}", e))
}

/// Build a Protocol from any legacy `assembly.json` / `discussion.json` present
/// in the section. Pure synthesis — does NOT touch disk. Caller is
/// responsible for writing protocol.json FIRST, then calling
/// `archive_legacy_for_section` only on successful write (dev-chall #930
/// belt-and-suspenders ordering).
///
/// Returns `(Protocol, did_migrate)` where `did_migrate=true` means at least
/// one legacy file was read and folded in.
fn synthesize_from_legacy_for_section(dir: &str, section: &str) -> (Protocol, bool) {
    let mut p = Protocol::fresh();

    let legacy_assembly = legacy_assembly_path_for_section(dir, section);
    let legacy_discussion = legacy_discussion_path_for_section(dir, section);

    let mut migrated_anything = false;

    if let Ok(content) = std::fs::read_to_string(&legacy_assembly) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&content) {
            let active = v.get("active").and_then(|a| a.as_bool()).unwrap_or(false);
            if active {
                p.preset = "Assembly Line".to_string();
                p.floor.mode = "round-robin".to_string();
                p.floor.current_speaker = v.get("current_speaker")
                    .and_then(|s| s.as_str())
                    .map(|s| s.to_string());
                p.floor.rotation_order = v.get("rotation_order")
                    .and_then(|r| r.as_array())
                    .map(|arr| arr.iter().filter_map(|x| x.as_str().map(String::from)).collect())
                    .unwrap_or_default();
                p.floor.started_at = v.get("started_at")
                    .and_then(|s| s.as_str())
                    .map(String::from)
                    .or_else(|| Some(iso_now()));
            } else {
                p.preset = "Default chat".to_string();
                p.floor.mode = "none".to_string();
            }
            migrated_anything = true;
        }
    }

    if let Ok(content) = std::fs::read_to_string(&legacy_discussion) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&content) {
            let mode = v.get("mode").and_then(|m| m.as_str()).unwrap_or("");
            if mode == "continuous" {
                p.consensus.mode = "tally".to_string();
            } else if !mode.is_empty() {
                p.consensus.mode = "tally".to_string();
            }
            if let Some(round) = v.get("round").or_else(|| v.get("current_round")) {
                p.consensus.round = Some(ConsensusRound {
                    topic: round.get("topic").and_then(|t| t.as_str()).map(String::from)
                        .or_else(|| v.get("topic").and_then(|t| t.as_str()).map(String::from)),
                    opened_at: round.get("opened_at").and_then(|t| t.as_str()).map(String::from),
                    opened_by: v.get("moderator").and_then(|t| t.as_str()).map(String::from),
                });
            }
            p.consensus.phase = v.get("phase").and_then(|s| s.as_str()).map(String::from);
            if let Some(arr) = v.get("submissions").and_then(|s| s.as_array()) {
                p.consensus.submissions = arr.clone();
            }
            migrated_anything = true;
        }
    }

    if migrated_anything {
        p.last_writer_seat = Some("system:migrate".to_string());
        p.last_writer_action = Some("migrate_from_legacy".to_string());
        p.rev_at = Some(iso_now());
    }

    (p, migrated_anything)
}

/// Move legacy `assembly.json` / `discussion.json` into `.vaak/legacy/<section>/`.
/// Caller MUST hold board.lock and have already written the new protocol.json
/// (dev-chall #930 ordering rule). Errors are logged, not returned — by this
/// point the new protocol.json is on disk, so a failed archive leaves the
/// system in a valid (just messy) state. Surface via eprintln so the human or
/// post-mortem reads it (tech-leader #931 D — no silent `let _ =` on disk ops).
fn archive_legacy_for_section(dir: &str, section: &str) {
    let archive_dir = legacy_archive_dir(dir, section);
    if let Err(e) = std::fs::create_dir_all(&archive_dir) {
        eprintln!(
            "[protocol] archive_legacy: create_dir_all({}) failed: {}",
            archive_dir.display(), e
        );
        return;
    }

    for src in [
        legacy_assembly_path_for_section(dir, section),
        legacy_discussion_path_for_section(dir, section),
    ] {
        if src.exists() {
            let file_name = match src.file_name() {
                Some(n) => n.to_owned(),
                None => continue,
            };
            let dest = archive_dir.join(file_name);
            if let Err(e) = std::fs::rename(&src, &dest) {
                eprintln!(
                    "[protocol] archive_legacy: rename({} -> {}) failed: {} \
                     (protocol.json already written; legacy file remains in \
                     original location for next-run retry)",
                    src.display(), dest.display(), e
                );
            }
        }
    }
}

// ============================================================
// Tests — golden migration fixtures + archive round-trip
// ============================================================
// Per tester #922 / tech-leader #925 hard requirements (a) + (c).
// (b) "any mutate sequence preserves invariants" lands with Slice 2's
// `protocol_mutate` API — no mutation surface to property-test today.

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    /// Per-test temp project dir under the OS tempdir, unique by test name.
    fn temp_project(test_name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("vaak-protocol-test-{}", test_name));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".vaak")).unwrap();
        dir
    }

    fn write_legacy_assembly(project: &Path, section: &str, content: &str) {
        let path = legacy_assembly_path_for_section(project.to_str().unwrap(), section);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, content).unwrap();
    }

    fn write_legacy_discussion(project: &Path, section: &str, content: &str) {
        let path = legacy_discussion_path_for_section(project.to_str().unwrap(), section);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, content).unwrap();
    }

    /// Fixture (a)-empty: section with no legacy files. Migration produces a
    /// fresh Protocol with the documented defaults.
    #[test]
    fn migration_empty_section_yields_fresh_defaults() {
        let project = temp_project("empty");
        let p = read_protocol_for_section(project.to_str().unwrap(), "default");
        assert_eq!(p.schema_version, 1);
        assert_eq!(p.preset, "Debate");
        assert_eq!(p.floor.mode, "reactive");
        assert_eq!(p.consensus.mode, "none");
        assert_eq!(p.rev, 0);
    }

    /// Fixture (a)-mid-rotation: legacy assembly.json with active=true and a
    /// rotation order. Migration must preserve current_speaker + rotation.
    #[test]
    fn migration_mid_rotation_preserves_speaker_and_order() {
        let project = temp_project("mid-rotation");
        write_legacy_assembly(&project, "default", r#"{
            "active": true,
            "current_speaker": "architect:0",
            "rotation_order": ["architect:0", "developer:0", "tester:0"],
            "started_at": "2026-04-28T18:00:00Z",
            "started_by": "human:0"
        }"#);
        let p = read_protocol_for_section(project.to_str().unwrap(), "default");
        assert_eq!(p.preset, "Assembly Line");
        assert_eq!(p.floor.mode, "round-robin");
        assert_eq!(p.floor.current_speaker.as_deref(), Some("architect:0"));
        assert_eq!(p.floor.rotation_order.len(), 3);
        assert_eq!(p.floor.started_at.as_deref(), Some("2026-04-28T18:00:00Z"));
        assert_eq!(p.last_writer_action.as_deref(), Some("migrate_from_legacy"));
    }

    /// Fixture (a)-mid-discussion: legacy discussion.json with active
    /// continuous round. Migration preserves topic + opener.
    #[test]
    fn migration_mid_discussion_preserves_round() {
        let project = temp_project("mid-discussion");
        write_legacy_discussion(&project, "default", r#"{
            "mode": "continuous",
            "topic": "ship 9faf275 to main?",
            "moderator": "moderator:0",
            "round": {
                "topic": "ship 9faf275 to main?",
                "opened_at": "2026-04-28T18:30:00Z"
            }
        }"#);
        let p = read_protocol_for_section(project.to_str().unwrap(), "default");
        assert_eq!(p.consensus.mode, "tally");
        let round = p.consensus.round.expect("round preserved");
        assert_eq!(round.topic.as_deref(), Some("ship 9faf275 to main?"));
        assert_eq!(round.opened_at.as_deref(), Some("2026-04-28T18:30:00Z"));
        assert_eq!(round.opened_by.as_deref(), Some("moderator:0"));
    }

    /// Fixture (a)-default-queue: legacy `assembly.json` has no `queue` field,
    /// so migrated `floor.queue` defaults to empty. Renamed from the prior
    /// over-promising name (dev-chall #930 nit — this test asserts the
    /// default, not orphan handling, which lands in Slice 5's normalize()).
    #[test]
    fn migration_queue_defaults_empty_when_legacy_omits_queue() {
        let project = temp_project("orphan-queue");
        write_legacy_assembly(&project, "default", r#"{
            "active": true,
            "current_speaker": "architect:0",
            "rotation_order": ["architect:0"]
        }"#);
        let p = read_protocol_for_section(project.to_str().unwrap(), "default");
        assert_eq!(p.floor.rotation_order, vec!["architect:0"]);
        // queue defaults to empty — legacy assembly.json has no queue field.
        // The "orphan" surface emerges in Slice 2 (mutate API) and is
        // resolved in Slice 5 (normalize). Slice 1 simply preserves shape.
        assert!(p.floor.queue.is_empty());
    }

    /// Fixture (c)-archive-round-trip: legacy files must move to
    /// `.vaak/legacy/<section>/` after migration. Original locations are
    /// emptied; archive contents byte-for-byte match the originals.
    #[test]
    fn archive_round_trip_legacy_byte_for_byte() {
        let project = temp_project("archive-rt");
        let assembly_payload = r#"{"active":false,"current_speaker":null,"rotation_order":[]}"#;
        let discussion_payload = r#"{"mode":"continuous","topic":"vote a","submissions":[]}"#;

        write_legacy_assembly(&project, "default", assembly_payload);
        write_legacy_discussion(&project, "default", discussion_payload);

        let _ = read_protocol_for_section(project.to_str().unwrap(), "default");

        let legacy_assembly = legacy_assembly_path_for_section(project.to_str().unwrap(), "default");
        let legacy_discussion = legacy_discussion_path_for_section(project.to_str().unwrap(), "default");
        assert!(!legacy_assembly.exists(), "original assembly.json must be moved");
        assert!(!legacy_discussion.exists(), "original discussion.json must be moved");

        let archive = legacy_archive_dir(project.to_str().unwrap(), "default");
        let archived_assembly = std::fs::read_to_string(archive.join("assembly.json")).unwrap();
        let archived_discussion = std::fs::read_to_string(archive.join("discussion.json")).unwrap();
        assert_eq!(archived_assembly, assembly_payload);
        assert_eq!(archived_discussion, discussion_payload);
    }

    /// Corrupted protocol.json (per evil-arch #929 review): must NOT
    /// silently re-migrate (legacy files already archived would yield
    /// fresh defaults overwriting the bad file). Must return fresh
    /// defaults WITHOUT writing — leaving the bad file in place.
    #[test]
    fn corrupted_protocol_json_does_not_silently_re_migrate() {
        let project = temp_project("corrupt");
        // Plant a corrupted protocol.json at the active path.
        let path = protocol_path_for_section(project.to_str().unwrap(), "default");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "this is not valid json {{{").unwrap();

        let p = read_protocol_for_section(project.to_str().unwrap(), "default");
        assert_eq!(p.preset, "Debate", "must return fresh defaults on parse failure");

        // Critical: the corrupted file must remain on disk for inspection.
        let still_corrupted = std::fs::read_to_string(&path).unwrap();
        assert_eq!(still_corrupted, "this is not valid json {{{",
            "corrupted file must NOT be overwritten");
    }

    // ============================================================
    // normalize() body — Slice 5 (architect left no-op stub at 27f4eee).
    // Three rules per spec §2.2 invariants 1+2 and edge case row 4.
    // ============================================================

    fn active_set(seats: &[&str]) -> std::collections::HashSet<String> {
        seats.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn normalize_free_grab_dissolves_queue() {
        let mut p = Protocol::fresh();
        p.floor.mode = "free-grab".to_string();
        p.floor.queue = vec!["dev:0".to_string(), "tester:0".to_string()];
        p.normalize(&active_set(&["dev:0", "tester:0"]));
        assert!(p.floor.queue.is_empty());
    }

    #[test]
    fn normalize_orphan_speaker_cleared() {
        let mut p = Protocol::fresh();
        p.floor.current_speaker = Some("ghost:0".to_string());
        p.normalize(&active_set(&["dev:0", "tester:0"]));
        assert!(p.floor.current_speaker.is_none());
    }

    #[test]
    fn normalize_keeps_active_speaker() {
        let mut p = Protocol::fresh();
        p.floor.current_speaker = Some("dev:0".to_string());
        p.normalize(&active_set(&["dev:0"]));
        assert_eq!(p.floor.current_speaker.as_deref(), Some("dev:0"));
    }

    #[test]
    fn normalize_prunes_dead_queue_entries() {
        let mut p = Protocol::fresh();
        p.floor.queue = vec!["dev:0".to_string(), "ghost:0".to_string(), "tester:0".to_string()];
        p.normalize(&active_set(&["dev:0", "tester:0"]));
        assert_eq!(p.floor.queue, vec!["dev:0".to_string(), "tester:0".to_string()]);
    }

    #[test]
    fn normalize_empty_active_set_skips_seat_rules() {
        // Rule 1 still fires on free-grab; rules 2+3 skipped (no signal).
        let mut p = Protocol::fresh();
        p.floor.mode = "reactive".to_string();
        p.floor.current_speaker = Some("ghost:0".to_string());
        p.floor.queue = vec!["ghost:0".to_string(), "dev:0".to_string()];
        p.normalize(&active_set(&[]));
        // Speaker stays (rule 2 skipped); queue stays (rule 3 skipped).
        assert_eq!(p.floor.current_speaker.as_deref(), Some("ghost:0"));
        assert_eq!(p.floor.queue, vec!["ghost:0".to_string(), "dev:0".to_string()]);
    }

    /// Idempotency: a second read after migration must NOT re-trigger
    /// migration (legacy archive directory is the canonical home, and
    /// protocol.json now exists). The second read should produce the same
    /// rev/preset and not change last_writer_*.
    #[test]
    fn second_read_is_idempotent() {
        let project = temp_project("idempotent");
        write_legacy_assembly(&project, "default", r#"{
            "active": true,
            "current_speaker": "developer:0",
            "rotation_order": ["developer:0", "architect:0"]
        }"#);
        let first = read_protocol_for_section(project.to_str().unwrap(), "default");
        let second = read_protocol_for_section(project.to_str().unwrap(), "default");
        assert_eq!(first.rev, second.rev);
        assert_eq!(first.preset, second.preset);
        assert_eq!(first.last_writer_action, second.last_writer_action);
        assert_eq!(first.floor.current_speaker, second.floor.current_speaker);
    }
}
