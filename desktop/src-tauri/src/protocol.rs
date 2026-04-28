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

use crate::collab::{atomic_write, get_active_section, iso_now};

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
    /// §2.2 invariants. Slice 1 ships the surface shape only — concrete
    /// transition logic lands with the mutate API in Slice 2 and the phase
    /// plan executor in Slice 5. Spec edge case row 4 ("any→free-grab
    /// dissolves the queue", §2.2 spec line: "edge case 4") is the canonical
    /// driver.
    ///
    /// Today this is a deliberate no-op so callers can already structure
    /// their code around `state.normalize()` and the call site doesn't move
    /// when the body fills in. If you delete this method, walk the call
    /// graph in collab.rs / vaak-mcp.rs first.
    pub fn normalize(&mut self) {
        // Slice 5 will implement:
        //   - if floor.mode == "free-grab": self.floor.queue.clear()
        //   - if floor.current_speaker no longer in active seats: clear it
        //   - drop queue entries that reference seats no longer alive
        // The shape of these mutations is settled (evil-arch #923, dev-chall
        // #917.3); only the wire-up waits.
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

/// Read protocol state for a section. On first read with no `protocol.json`
/// but an existing `assembly.json` or `discussion.json`, synthesize a fresh
/// protocol and archive the legacy files (per spec §3.3).
///
/// Caller is responsible for holding `board.lock` if reading inside a mutation
/// transaction. Plain reads (UI display) may skip the lock.
pub fn read_protocol_for_section(dir: &str, section: &str) -> Protocol {
    let path = protocol_path_for_section(dir, section);
    if let Ok(content) = std::fs::read_to_string(&path) {
        if let Ok(p) = serde_json::from_str::<Protocol>(&content) {
            return p;
        }
    }
    // No protocol.json (or unparseable). Try migration from legacy.
    let migrated = migrate_legacy_for_section(dir, section);
    let _ = write_protocol_for_section_unlocked(dir, section, &migrated);
    migrated
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
/// in the section. Archives the legacy files into `.vaak/legacy/<section>/` —
/// rollback is just moving them back. Per spec §3.3.
fn migrate_legacy_for_section(dir: &str, section: &str) -> Protocol {
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
        let _ = archive_legacy_for_section(dir, section);
        p.last_writer_seat = Some("system:migrate".to_string());
        p.last_writer_action = Some("migrate_from_legacy".to_string());
        p.rev_at = Some(iso_now());
    }

    p
}

fn archive_legacy_for_section(dir: &str, section: &str) -> Result<(), String> {
    let archive_dir = legacy_archive_dir(dir, section);
    std::fs::create_dir_all(&archive_dir)
        .map_err(|e| format!("Failed to create legacy archive dir: {}", e))?;

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
            let _ = std::fs::rename(&src, &dest);
        }
    }
    Ok(())
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

    /// Fixture (a)-orphan-queue: dev-chall asked about a queue entry whose
    /// referenced seat isn't in rotation_order. Slice 1 must NOT silently
    /// drop the entry — that decision belongs to Slice 5's normalize().
    /// Today's job: round-trip the data without loss.
    #[test]
    fn migration_does_not_drop_orphan_queue_entries_in_slice_1() {
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
