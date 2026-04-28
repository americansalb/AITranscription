// Protocol — unified per-section floor + consensus state machine.
//
// One file per section: `.vaak/sections/<section>/protocol.json` (default
// section: `.vaak/protocol.json`). Replaces the legacy split between
// `assembly.json` (floor rotation) and `discussion.json` (consensus rounds)
// per Assembly Line architecture v6 (`.vaak/al-architecture-diagram.md`
// §3 — Slice 1 scope).
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

#[derive(Debug, Clone, Serialize, Deserialize)]
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

fn default_preset() -> String { "Debate".to_string() }

impl Protocol {
    pub fn fresh() -> Self {
        Self {
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
