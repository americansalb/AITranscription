#![recursion_limit = "256"]
//! Vaak MCP Server - Bridges Claude Code to Vaak for voice output
//!
//! This is a minimal MCP (Model Context Protocol) server that provides a `speak` tool
//! for Claude Code to send text-to-speech requests to the Vaak desktop app.
//!
//! ============================================================
//! Resilience-stack timer registry (mirror — keep in sync with
//! protocol.rs and collab.rs)
//! ============================================================
//! Per evil-arch #923 + dev-chall #917.1, the AL vision intentionally
//! keeps timers decentralized at their consumers — only when consumers
//! can find each other does decentralization work.
//!
//!   floor.threshold_ms (per-section, default 60_000)
//!                                       — protocol.rs::MIC_GRAB_THRESHOLD_MS
//!                                         (mic freshness gate, spec §2)
//!   SUPERVISOR_STALL_SECS = 90          — vaak-mcp.rs supervisor loop
//!                                         (90s stall before pre-kill buzz)
//!   PRE_KILL_GRACE_SECS = 5             — vaak-mcp.rs supervisor loop
//!                                         (5s grace before taskkill)
//!   KEEP_ALIVE_DEBOUNCE_MS ≈ 10_000     — composer (UI) keystroke heartbeat
//!   MIC_AUTOROTATE_SECS = 600           — assembly_line auto-rotation
//!                                         (10-min idle = grab, human #903)
//!
//! Spec: .vaak/al-architecture-diagram.md §2 (single threshold for the
//! freshness gate only) + §12 (resilience layers).
//! ============================================================
//!
//! Session ID is determined using a priority chain of methods for redundancy:
//! 1. CLAUDE_SESSION_ID env var (explicit override)
//! 2. WT_SESSION env var (Windows Terminal GUID)
//! 3. ITERM_SESSION_ID env var (iTerm2 on macOS)
//! 4. TERM_SESSION_ID env var (Terminal.app and others)
//! 5. Console window handle (Windows) or TTY path (Unix)
//! 6. Fallback hash of hostname + parent PID + working directory

use std::io::{self, BufRead, Write};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

// Shared with main.rs's collab module: Assembly Line mutation lives in ONE place
// per architect #156 (two front doors, one impl). Imported via #[path] because
// vaak-mcp.rs is a separate binary crate from the desktop app and does not share
// a lib.rs with main.rs.
// Renamed from `collab_shared` → `collab` so protocol.rs's
// `use crate::collab::{...}` resolves when protocol.rs is included into
// this binary via the #[path] block below. v1.5.0 commit 2/6 needs the
// Preset enum from protocol.rs; that file shares helpers with collab.rs.
#[path = "../collab.rs"]
mod collab;

// v1.5.0 commit 2/6 (architect msg 644 + tech-leader msg 674): the Preset
// enum lives in protocol.rs (commit 1 1cd488d). Imported via the same
// #[path] pattern. Inner module name differs from the file name to avoid
// the `protocol_slice2_tests` mod below shadowing the import.
#[path = "../protocol.rs"]
mod protocol_module;
use protocol_module::Preset;

/// Atomic file write: write to .tmp, fsync, rename. Prevents partial writes on macOS.
fn atomic_write(path: &Path, content: &[u8]) -> Result<(), String> {
    let tmp_path = path.with_extension("tmp");
    std::fs::write(&tmp_path, content)
        .map_err(|e| format!("Failed to write temp file {}: {}", tmp_path.display(), e))?;
    if let Ok(f) = std::fs::File::open(&tmp_path) {
        let _ = f.sync_all();
    }
    std::fs::rename(&tmp_path, path)
        .map_err(|e| format!("Failed to rename {} -> {}: {}", tmp_path.display(), path.display(), e))?;
    Ok(())
}

/// Backend API base URL. Override via VAAK_BACKEND_URL env var; defaults to localhost:19836.
fn get_backend_url() -> String {
    std::env::var("VAAK_BACKEND_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:19836".to_string())
}

// ==================== Project-based Collaboration Protocol ====================

/// Active project state for this MCP sidecar process.
/// Set on project_join, read by project_send/project_check/etc.
static ACTIVE_PROJECT: Mutex<Option<ActiveProjectState>> = Mutex::new(None);

#[derive(Clone)]
struct ActiveProjectState {
    project_dir: String,
    role: String,
    instance: u32,
    session_id: String,
}

/// Get the active project state, attempting auto-rejoin if the in-memory state was lost.
/// Looks up the current session's binding in sessions.json and re-joins if found.
fn get_or_rejoin_state() -> Result<ActiveProjectState, String> {
    // Fast path: state is already in memory
    {
        let guard = ACTIVE_PROJECT.lock().map_err(|_| "Lock poisoned")?;
        if let Some(state) = guard.as_ref() {
            return Ok(state.clone());
        }
    }

    // Slow path: state lost, attempt auto-rejoin from sessions.json
    eprintln!("[vaak-mcp] Session state lost — attempting auto-rejoin from sessions.json");

    let project_dir = find_project_root()
        .ok_or("Not in a project. Call project_join first.")?;
    let session_id = read_cached_session_id().unwrap_or_else(get_session_id);

    // Read sessions.json to find our binding
    let sessions = read_sessions(&project_dir);
    let bindings = sessions.get("bindings").and_then(|b| b.as_array())
        .ok_or("Not in a project. Call project_join first.")?;

    let binding = bindings.iter().find(|b| {
        b.get("session_id").and_then(|s| s.as_str()) == Some(&session_id)
        && b.get("status").and_then(|s| s.as_str()) == Some("active")
    }).ok_or("Not in a project. Call project_join first.")?;

    let role = binding.get("role").and_then(|r| r.as_str())
        .ok_or("Not in a project. Call project_join first.")?;

    eprintln!("[vaak-mcp] Found binding for session {} as role '{}' — re-joining", session_id, role);

    // Re-join using the existing binding info
    match handle_project_join(role, &project_dir, &session_id, None) {
        Ok(_) => {
            eprintln!("[vaak-mcp] Auto-rejoin successful");
            // Now read the restored state
            let guard = ACTIVE_PROJECT.lock().map_err(|_| "Lock poisoned")?;
            guard.as_ref().ok_or("Auto-rejoin failed to restore state".to_string()).cloned()
        }
        Err(e) => {
            eprintln!("[vaak-mcp] Auto-rejoin failed: {}", e);
            Err(format!("Not in a project. Auto-rejoin failed: {}", e))
        }
    }
}

fn vaak_dir(project_dir: &str) -> PathBuf {
    Path::new(project_dir).join(".vaak")
}

fn project_json_path(project_dir: &str) -> PathBuf {
    vaak_dir(project_dir).join("project.json")
}

fn sessions_json_path(project_dir: &str) -> PathBuf {
    vaak_dir(project_dir).join("sessions.json")
}

/// Get the active section slug. Checks per-session binding in sessions.json first,
/// then falls back to project.json active_section, then "default".
fn get_active_section(project_dir: &str) -> String {
    // Try per-session active_section from the current session's binding
    if let Ok(guard) = ACTIVE_PROJECT.lock() {
        if let Some(ref s) = *guard {
            let session_id = &s.session_id;
            if let Ok(sessions_str) = std::fs::read_to_string(sessions_json_path(project_dir)) {
                if let Ok(sessions) = serde_json::from_str::<serde_json::Value>(&sessions_str) {
                    if let Some(bindings) = sessions.get("bindings").and_then(|b| b.as_array()) {
                        for b in bindings {
                            if b.get("session_id").and_then(|s| s.as_str()) == Some(session_id) {
                                if let Some(section) = b.get("active_section").and_then(|s| s.as_str()) {
                                    return section.to_string();
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Fall back to project.json active_section
    std::fs::read_to_string(project_json_path(project_dir))
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.get("active_section")?.as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "default".to_string())
}

/// Returns the board.jsonl path for the active section.
/// "default" section uses legacy flat .vaak/board.jsonl for backward compatibility.
/// Non-default sections use .vaak/sections/{slug}/board.jsonl.
/// Matches collab.rs board_path_for_section().
fn board_jsonl_path(project_dir: &str) -> PathBuf {
    let section = get_active_section(project_dir);
    if section == "default" {
        vaak_dir(project_dir).join("board.jsonl")
    } else {
        vaak_dir(project_dir).join("sections").join(section).join("board.jsonl")
    }
}

fn role_briefing_path(project_dir: &str, role: &str) -> PathBuf {
    vaak_dir(project_dir).join("roles").join(format!("{}.md", role))
}

/// Character/stats Phase 1 dynamic injection — reads stats from project.json
/// and prepends a cognitive-budget framing block to the briefing.
///
/// Per human msg 3254 + spec at
/// .vaak/design-notes/character-stats-system-2026-05-16.md:
/// - NO literal stat numbers in output text (recursion prevention)
/// - 4-band classification: ≥9 / 7-8 / 5-6 / ≤4 with distinct framing
/// - Anti-deference-on-verification clause baked in
/// - Specialist lookup picks highest-stat peer in each low-stat dimension
///
/// Mirror of TS generateStatFraming in briefingGenerator.ts. Same shape.
/// Returns briefing_raw unchanged when the role has no stats (legacy roles).
fn inject_stat_framing(project_dir: &str, role: &str, briefing_raw: &str) -> String {
    let config = match read_project_config(project_dir) {
        Ok(c) => c,
        Err(_) => return briefing_raw.to_string(),
    };
    let roles_obj = match config.get("roles").and_then(|r| r.as_object()) {
        Some(o) => o,
        None => return briefing_raw.to_string(),
    };
    let my_role_obj = match roles_obj.get(role) {
        Some(r) => r,
        None => return briefing_raw.to_string(),
    };
    let stats = match my_role_obj.get("stats").and_then(|s| s.as_object()) {
        Some(s) => s,
        None => return briefing_raw.to_string(), // legacy role without stats
    };

    let title = my_role_obj
        .get("title")
        .and_then(|t| t.as_str())
        .unwrap_or(role);

    // Stat dimensions in display order
    let dimensions: &[(&str, &str)] = &[
        ("td", "Technical Depth"),
        ("ar", "Adversarial Rigor"),
        ("cp", "Communication Precision"),
        ("do", "Domain Ownership"),
        ("pd", "Process Discipline"),
        ("ja", "Judgment Under Ambiguity"),
    ];

    // Find specialist for a given dim by max stat among OTHER roles
    // (alphabetical slug tie-break). Returns peer's title or fallback string.
    let find_specialist = |dim_key: &str| -> String {
        let mut best: Option<(&String, &serde_json::Value, u64)> = None;
        for (slug, role_obj) in roles_obj.iter() {
            if slug == role {
                continue; // skip self per spec §5 self-reference avoidance
            }
            let v = role_obj
                .get("stats")
                .and_then(|s| s.as_object())
                .and_then(|s| s.get(dim_key))
                .and_then(|n| n.as_u64());
            if let Some(val) = v {
                let take = match best {
                    None => true,
                    Some((bs, _, bv)) => val > bv || (val == bv && slug < bs),
                };
                if take {
                    best = Some((slug, role_obj, val));
                }
            }
        }
        match best {
            Some((_, role_obj, _)) => role_obj
                .get("title")
                .and_then(|t| t.as_str())
                .unwrap_or("the team's specialist")
                .to_string(),
            None => "the team's specialist".to_string(),
        }
    };

    let mut lines: Vec<String> = Vec::new();
    for (key, label) in dimensions {
        let v = stats.get(*key).and_then(|n| n.as_u64()).unwrap_or(5);
        let line = if v >= 9 {
            format!("- You're the team's strongest voice on {}. Lead here.", label)
        } else if v >= 7 {
            format!("- Strong on {}. Engage when needed.", label)
        } else if v >= 5 {
            let target = find_specialist(key);
            format!(
                "- {} isn't your primary focus. When complex {} decisions arise, flag them for {}. Your cognitive budget is better spent on your strongest dimensions.",
                label, label, target
            )
        } else {
            let target = find_specialist(key);
            format!(
                "- {} is explicitly outside your scope. Always defer to {}.",
                label, target
            )
        };
        lines.push(line);
    }

    let block = format!(
        "## 0. Your Cognitive Budget\n\n\
         You're playing the role of {}. You have a limited cognitive budget; spend it where you're the team's strongest voice. Below: what to lead on, what to flag for specialists.\n\n\
         {}\n\n\
         **Verification responsibility preserved:** your stat profile biases your cognitive budget toward your 9s and 10s, but does NOT exempt you from verification responsibilities. If a peer specialist's output crosses your read path, you still independently verify what crosses your lane — multi-verifier coverage is a safety net, not redundant overhead.\n\n---\n\n",
        title,
        lines.join("\n")
    );

    format!("{}{}", block, briefing_raw)
}

/// Path to the per-session last-seen-id tracker file.
///
/// Keyed on (session_id, section) because board.jsonl is itself section-scoped
/// (see board_jsonl_path). If we only keyed on session_id, the last_seen_id
/// from the most-recently-active section would silence the agent in any other
/// section whose max id is below that carried value — agents go quiet on
/// section switches and stay quiet until the new section's id catches up.
fn last_seen_path(project_dir: &str, session_id: &str, section: &str) -> PathBuf {
    let safe_id = session_id.replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "_");
    let safe_section = section.replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "_");
    vaak_dir(project_dir).join("last-seen").join(format!("{}__{}.json", safe_id, safe_section))
}

/// Legacy session-only last-seen path used before the section-key fix.
/// Only consulted as a one-shot read fallback by read_last_seen_id; new writes
/// always go to the section-scoped path. The legacy file is left in place so a
/// downgrade doesn't lose state — it idles out naturally.
fn legacy_last_seen_path(project_dir: &str, session_id: &str) -> PathBuf {
    let safe_id = session_id.replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "_");
    vaak_dir(project_dir).join("last-seen").join(format!("{}.json", safe_id))
}

/// Read the stored last_seen_id for (session_id, section).
/// Falls back to the legacy session-only file when the section-scoped one is
/// missing, so agents upgrading mid-session don't re-process old messages.
fn read_last_seen_id(project_dir: &str, session_id: &str, section: &str) -> u64 {
    let new_path = last_seen_path(project_dir, session_id, section);
    if let Ok(s) = std::fs::read_to_string(&new_path) {
        if let Some(id) = serde_json::from_str::<serde_json::Value>(&s)
            .ok()
            .and_then(|j| j.get("last_seen_id")?.as_u64())
        {
            return id;
        }
    }
    let legacy = legacy_last_seen_path(project_dir, session_id);
    std::fs::read_to_string(&legacy)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|j| j.get("last_seen_id")?.as_u64())
        .unwrap_or(0)
}

/// Touch this seat's per-seat session file with a fresh last_alive_at_ms.
/// Mirrors the body of run_keep_alive's stamping logic, callable from any
/// non-hook context that needs to keep the seat observably alive (notably
/// project_wait's poll loop, where pure-standby seats wouldn't otherwise
/// fire the PreToolUse/PostToolUse hooks). Fail-open on every error — must
/// never block the calling tool.
fn update_seat_alive_at_ms(project_dir: &str, role: &str, instance: u32) {
    let sessions_dir = std::path::Path::new(project_dir).join(".vaak").join("sessions");
    if !sessions_dir.exists() {
        return;
    }
    let seat_file = sessions_dir.join(format!("{}-{}.json", role, instance));
    let mut state: serde_json::Value = std::fs::read_to_string(&seat_file)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    if let Some(obj) = state.as_object_mut() {
        obj.insert("last_alive_at_ms".to_string(), serde_json::json!(now_ms));
        // Deliberately do NOT update last_active_at_ms — pure project_wait is
        // standby, not "active" work. last_active_at_ms is reserved for the
        // keep-alive hook (PreToolUse/PostToolUse) so the watchdog's stall
        // criterion stays meaningful.
        if let Ok(serialized) = serde_json::to_string_pretty(&state) {
            let _ = atomic_write(&seat_file, serialized.as_bytes());
        }
    }
}

fn send_tracker_path(project_dir: &str, session_id: &str) -> PathBuf {
    let safe_id = session_id.replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "_");
    let dir = vaak_dir(project_dir).join("last-send");
    let _ = std::fs::create_dir_all(&dir);
    dir.join(format!("{}.txt", safe_id))
}

fn read_send_tracker(project_dir: &str, session_id: &str) -> u64 {
    let path = send_tracker_path(project_dir, session_id);
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or(0)
}

fn write_send_tracker(project_dir: &str, session_id: &str, count: u64) {
    let path = send_tracker_path(project_dir, session_id);
    let _ = std::fs::write(&path, count.to_string());
}

// ==================== Section Management ====================

/// Slugify a section name: lowercase, replace non-alphanumeric with hyphens, collapse multiples.
fn slugify(name: &str) -> String {
    let slug: String = name.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect();
    let mut result = String::new();
    let mut last_was_hyphen = false;
    for c in slug.chars() {
        if c == '-' {
            if !last_was_hyphen && !result.is_empty() {
                result.push(c);
                last_was_hyphen = true;
            }
        } else {
            result.push(c);
            last_was_hyphen = false;
        }
    }
    result.trim_end_matches('-').to_string()
}

/// Ensure the sections/ directory exists. Does NOT move flat files — "default" section
/// always uses flat .vaak/board.jsonl and .vaak/discussion.json for backward compatibility.
/// Only creates the sections/ directory and ensures project.json is ready for sections.
fn ensure_sections_layout(project_dir: &str) -> Result<(), String> {
    let vaak = vaak_dir(project_dir);
    let sections_dir = vaak.join("sections");
    if sections_dir.exists() {
        return Ok(());
    }

    eprintln!("[sections] Initializing sections/ directory");
    std::fs::create_dir_all(&sections_dir)
        .map_err(|e| format!("Failed to create sections/: {}", e))?;

    // Ensure project.json has active_section set
    let config_path = project_json_path(project_dir);
    let mut config: serde_json::Value = std::fs::read_to_string(&config_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(serde_json::json!({}));

    if config.get("active_section").is_none() {
        config["active_section"] = serde_json::json!("default");
    }

    let content = serde_json::to_string_pretty(&config)
        .map_err(|e| format!("Failed to serialize project.json: {}", e))?;
    atomic_write(&config_path, content.as_bytes())
        .map_err(|e| format!("Failed to write project.json: {}", e))?;

    eprintln!("[sections] Migration complete");
    Ok(())
}

/// Create a new section. Returns the slug.
fn handle_create_section(project_dir: &str, name: &str) -> Result<serde_json::Value, String> {
    ensure_sections_layout(project_dir)?;

    let slug = slugify(name);
    if slug.is_empty() {
        return Err("Section name must contain at least one alphanumeric character".to_string());
    }

    let section_dir = vaak_dir(project_dir).join("sections").join(&slug);
    if section_dir.exists() {
        return Err(format!("Section '{}' already exists", slug));
    }

    std::fs::create_dir_all(&section_dir)
        .map_err(|e| format!("Failed to create section directory: {}", e))?;
    std::fs::write(section_dir.join("board.jsonl"), "")
        .map_err(|e| format!("Failed to create board.jsonl: {}", e))?;

    // Add to sections manifest in project.json
    let config_path = project_json_path(project_dir);
    let mut config: serde_json::Value = std::fs::read_to_string(&config_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(serde_json::json!({}));

    let new_entry = serde_json::json!({
        "slug": slug,
        "name": name,
        "created_at": utc_now_iso()
    });
    if let Some(arr) = config.get_mut("sections").and_then(|s| s.as_array_mut()) {
        arr.push(new_entry);
    } else {
        config["sections"] = serde_json::json!([new_entry]);
    }

    let content = serde_json::to_string_pretty(&config)
        .map_err(|e| format!("Failed to serialize project.json: {}", e))?;
    atomic_write(&config_path, content.as_bytes())
        .map_err(|e| format!("Failed to write project.json: {}", e))?;

    eprintln!("[sections] Created section '{}' (slug: {})", name, slug);

    Ok(serde_json::json!({
        "status": "created",
        "slug": slug,
        "name": name
    }))
}

/// Switch active section for the current session.
fn handle_switch_section(project_dir: &str, slug: &str) -> Result<serde_json::Value, String> {
    // "default" uses legacy root .vaak/ paths — no sections/default/ dir needed
    if slug != "default" {
        let section_dir = vaak_dir(project_dir).join("sections").join(slug);
        if !section_dir.exists() {
            return Err(format!("Section '{}' does not exist", slug));
        }
    }

    let session_id = ACTIVE_PROJECT.lock().ok()
        .and_then(|guard| guard.as_ref().map(|s| s.session_id.clone()))
        .unwrap_or_default();

    // Update per-session active_section in sessions.json (NOT global project.json)
    // Each agent stays in their own section — no global switching
    let sessions_path = sessions_json_path(project_dir);
    let mut sessions: serde_json::Value = std::fs::read_to_string(&sessions_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(serde_json::json!({"bindings": []}));

    if let Some(bindings) = sessions.get_mut("bindings").and_then(|b| b.as_array_mut()) {
        for b in bindings.iter_mut() {
            if b.get("session_id").and_then(|s| s.as_str()) == Some(&session_id) {
                b["active_section"] = serde_json::json!(slug);
            }
        }
    }

    // Per-agent section isolation: only update the per-session binding, NOT the global
    // project.json active_section. The UI (Tauri/collab.rs) updates the global when
    // the human clicks a section tab. This prevents one agent's switch from moving all others.
    let sessions_content = serde_json::to_string_pretty(&sessions)
        .map_err(|e| format!("Failed to serialize sessions.json: {}", e))?;
    atomic_write(&sessions_path, sessions_content.as_bytes())
        .map_err(|e| format!("Failed to write sessions.json: {}", e))?;

    eprintln!("[sections] Switched to section '{}'", slug);

    Ok(serde_json::json!({
        "status": "switched",
        "active_section": slug
    }))
}

/// List all sections with message counts and last activity.
fn handle_list_sections(project_dir: &str) -> Result<serde_json::Value, String> {
    let config: serde_json::Value = std::fs::read_to_string(project_json_path(project_dir))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(serde_json::json!({}));

    let active_section = config.get("active_section")
        .and_then(|s| s.as_str())
        .unwrap_or("default");

    let sections = config.get("sections")
        .and_then(|s| s.as_array())
        .cloned()
        .unwrap_or_default();

    // Always include "default" section if not in manifest
    let has_default = sections.iter().any(|s| s.get("slug").and_then(|s| s.as_str()) == Some("default"));
    let all_sections = if has_default {
        sections.clone()
    } else {
        let mut v = vec![serde_json::json!({"slug": "default", "name": "Default", "created_at": ""})];
        v.extend(sections.iter().cloned());
        v
    };

    let mut result = Vec::new();
    for section in &all_sections {
        let slug = section.get("slug").and_then(|s| s.as_str()).unwrap_or("unknown");
        let name = section.get("name").and_then(|s| s.as_str()).unwrap_or(slug);
        let created_at = section.get("created_at").and_then(|s| s.as_str()).unwrap_or("");

        // "default" section uses flat path, others use sections/{slug}/
        let board_path = if slug == "default" {
            vaak_dir(project_dir).join("board.jsonl")
        } else {
            vaak_dir(project_dir).join("sections").join(slug).join("board.jsonl")
        };
        let message_count = std::fs::read_to_string(&board_path)
            .unwrap_or_default()
            .lines()
            .filter(|l| !l.trim().is_empty())
            .count();

        let last_activity = std::fs::read_to_string(&board_path)
            .unwrap_or_default()
            .lines()
            .filter(|l| !l.trim().is_empty())
            .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
            .last()
            .and_then(|m| m.get("timestamp").and_then(|t| t.as_str()).map(|s| s.to_string()));

        result.push(serde_json::json!({
            "slug": slug,
            "name": name,
            "created_at": created_at,
            "message_count": message_count,
            "last_activity": last_activity,
            "is_active": slug == active_section
        }));
    }

    Ok(serde_json::json!({
        "sections": result,
        "active_section": active_section
    }))
}

/// Get current ISO timestamp without chrono dependency
fn utc_now_iso() -> String {
    let dur = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs();
    let hours = (secs % 86400) / 3600;
    let mins = (secs % 3600) / 60;
    let s = secs % 60;
    let days = secs / 86400;
    let mut y = 1970u64;
    let mut remaining = days;
    loop {
        let diy = if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 { 366 } else { 365 };
        if remaining < diy { break; }
        remaining -= diy;
        y += 1;
    }
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let month_days: [u64; 12] = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut m = 0u64;
    for md in &month_days {
        if remaining < *md { break; }
        remaining -= *md;
        m += 1;
    }
    format!("{}-{:02}-{:02}T{:02}:{:02}:{:02}Z", y, m + 1, remaining + 1, hours, mins, s)
}

/// Parse an ISO 8601 timestamp to seconds since epoch.
/// Handles: "2026-02-05T04:11:10Z", "2026-02-05T04:11:10.123Z",
///          "2026-02-05T04:11:10+00:00", "2026-02-05T04:11:10.123+00:00"
fn parse_iso_to_epoch_secs(iso: &str) -> Option<u64> {
    // Strip timezone suffix: Z, +00:00, -05:00, etc.
    let iso_clean = iso.trim_end_matches('Z');
    // Handle +HH:MM or -HH:MM offset — just strip it (treat as UTC for simplicity)
    let iso_clean = if let Some(plus_pos) = iso_clean.rfind('+') {
        if plus_pos > 10 { &iso_clean[..plus_pos] } else { iso_clean }
    } else if let Some(minus_pos) = iso_clean.rfind('-') {
        // Only strip if the minus is in the time part (after T), not the date part
        if minus_pos > 10 { &iso_clean[..minus_pos] } else { iso_clean }
    } else {
        iso_clean
    };

    let (date_part, time_part) = iso_clean.split_once('T')?;
    let date_parts: Vec<&str> = date_part.split('-').collect();
    let time_parts: Vec<&str> = time_part.split(':').collect();
    if date_parts.len() != 3 || time_parts.len() < 3 { return None; }

    let year: u64 = date_parts[0].parse().ok()?;
    let month: u64 = date_parts[1].parse().ok()?;
    let day: u64 = date_parts[2].parse().ok()?;
    let hour: u64 = time_parts[0].parse().ok()?;
    let min: u64 = time_parts[1].parse().ok()?;
    // Handle fractional seconds (e.g., "10.123") — parse as float and truncate
    let sec: u64 = time_parts[2].split('.').next()?.parse().ok()?;

    // Count days from 1970 to the given date
    let mut total_days: u64 = 0;
    for y in 1970..year {
        let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
        total_days += if leap { 366 } else { 365 };
    }
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let month_days: [u64; 12] = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    for m in 0..(month.saturating_sub(1) as usize) {
        total_days += month_days.get(m).copied().unwrap_or(30);
    }
    total_days += day.saturating_sub(1);

    Some(total_days * 86400 + hour * 3600 + min * 60 + sec)
}

/// Execute a closure while holding an exclusive file lock on board.lock.
/// Lock path is section-aware: uses sections/{slug}/board.lock when sections/ exists.
/// On Windows uses LockFileEx, on Unix uses flock.
fn with_file_lock<F, R>(project_dir: &str, f: F) -> Result<R, String>
where
    F: FnOnce() -> Result<R, String>,
{
    let dir = vaak_dir(project_dir);
    std::fs::create_dir_all(&dir).map_err(|e| format!("Failed to create .vaak dir: {}", e))?;

    // Per-section lock: "default" section uses legacy .vaak/board.lock,
    // other sections use sections/{slug}/board.lock
    let section = get_active_section(project_dir);
    let lock_path = if section == "default" {
        dir.join("board.lock")
    } else {
        let section_dir = dir.join("sections").join(&section);
        std::fs::create_dir_all(&section_dir).map_err(|e| format!("Failed to create section dir: {}", e))?;
        section_dir.join("board.lock")
    };
    let lock_file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .open(&lock_path)
        .map_err(|e| format!("Failed to open lock file: {}", e))?;

    const LOCK_TIMEOUT_MS: u64 = 10_000; // 10 seconds max wait
    const LOCK_RETRY_MS: u64 = 50;       // retry interval

    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{LockFileEx, UnlockFileEx, LOCKFILE_EXCLUSIVE_LOCK, LOCKFILE_FAIL_IMMEDIATELY};
        use windows_sys::Win32::System::IO::OVERLAPPED;

        let handle = lock_file.as_raw_handle();
        let mut overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };

        // Try non-blocking first
        let locked = unsafe {
            LockFileEx(handle as _, LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY, 0, u32::MAX, u32::MAX, &mut overlapped)
        };

        if locked == 0 {
            // Lock held by another process — retry with timeout
            let start = std::time::Instant::now();
            loop {
                std::thread::sleep(std::time::Duration::from_millis(LOCK_RETRY_MS));
                let retry = unsafe {
                    LockFileEx(handle as _, LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY, 0, u32::MAX, u32::MAX, &mut overlapped)
                };
                if retry != 0 { break; }
                if start.elapsed().as_millis() as u64 > LOCK_TIMEOUT_MS {
                    return Err(format!(
                        "board.lock held for >{}s — possible stale lock. Lock file: {}",
                        LOCK_TIMEOUT_MS / 1000, lock_path.display()
                    ));
                }
            }
        }

        let result = f();

        unsafe {
            UnlockFileEx(handle as _, 0, u32::MAX, u32::MAX, &mut overlapped);
        }
        result
    }

    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        let fd = lock_file.as_raw_fd();

        // Try non-blocking first
        if unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            // Lock held — retry with timeout
            let start = std::time::Instant::now();
            loop {
                std::thread::sleep(std::time::Duration::from_millis(LOCK_RETRY_MS));
                if unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) } == 0 { break; }
                if start.elapsed().as_millis() as u64 > LOCK_TIMEOUT_MS {
                    return Err(format!(
                        "board.lock held for >{}s — possible stale lock. Lock file: {}",
                        LOCK_TIMEOUT_MS / 1000, lock_path.display()
                    ));
                }
            }
        }

        let result = f();
        unsafe { libc::flock(fd, libc::LOCK_UN); }
        result
    }
}

/// Execute a closure while holding an exclusive file lock on discussion.lock.
/// Prevents race conditions when multiple agents read-modify-write discussion.json.
fn with_discussion_lock<F, R>(project_dir: &str, f: F) -> Result<R, String>
where
    F: FnOnce() -> Result<R, String>,
{
    let dir = vaak_dir(project_dir);
    std::fs::create_dir_all(&dir).map_err(|e| format!("Failed to create .vaak dir: {}", e))?;

    // Discussion lock is section-aware, same as board lock
    let section = get_active_section(project_dir);
    let lock_path = if section == "default" {
        dir.join("discussion.lock")
    } else {
        let section_dir = dir.join("sections").join(&section);
        std::fs::create_dir_all(&section_dir).map_err(|e| format!("Failed to create section dir: {}", e))?;
        section_dir.join("discussion.lock")
    };
    let lock_file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .open(&lock_path)
        .map_err(|e| format!("Failed to open discussion lock file: {}", e))?;

    const LOCK_TIMEOUT_MS: u64 = 10_000; // 10 seconds max wait
    const LOCK_RETRY_MS: u64 = 50;       // retry interval

    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{LockFileEx, UnlockFileEx, LOCKFILE_EXCLUSIVE_LOCK, LOCKFILE_FAIL_IMMEDIATELY};
        use windows_sys::Win32::System::IO::OVERLAPPED;

        let handle = lock_file.as_raw_handle();
        let mut overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };

        // Try non-blocking first
        let locked = unsafe {
            LockFileEx(handle as _, LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY, 0, u32::MAX, u32::MAX, &mut overlapped)
        };

        if locked == 0 {
            // Lock held by another process — retry with timeout
            let start = std::time::Instant::now();
            loop {
                std::thread::sleep(std::time::Duration::from_millis(LOCK_RETRY_MS));
                let retry = unsafe {
                    LockFileEx(handle as _, LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY, 0, u32::MAX, u32::MAX, &mut overlapped)
                };
                if retry != 0 { break; }
                if start.elapsed().as_millis() as u64 > LOCK_TIMEOUT_MS {
                    return Err(format!(
                        "discussion.lock held for >{}s — possible stale lock. Lock file: {}",
                        LOCK_TIMEOUT_MS / 1000, lock_path.display()
                    ));
                }
            }
        }

        let result = f();

        unsafe {
            UnlockFileEx(handle as _, 0, u32::MAX, u32::MAX, &mut overlapped);
        }
        result
    }

    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        let fd = lock_file.as_raw_fd();

        // Try non-blocking first
        if unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            // Lock held — retry with timeout
            let start = std::time::Instant::now();
            loop {
                std::thread::sleep(std::time::Duration::from_millis(LOCK_RETRY_MS));
                if unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) } == 0 { break; }
                if start.elapsed().as_millis() as u64 > LOCK_TIMEOUT_MS {
                    return Err(format!(
                        "discussion.lock held for >{}s — possible stale lock. Lock file: {}",
                        LOCK_TIMEOUT_MS / 1000, lock_path.display()
                    ));
                }
            }
        }

        let result = f();
        unsafe { libc::flock(fd, libc::LOCK_UN); }
        result
    }
}

/// Fire-and-forget notification to the desktop app that project files changed.
fn notify_desktop() {
    let _ = std::thread::spawn(|| {
        let client = ureq::AgentBuilder::new()
            .timeout(std::time::Duration::from_millis(500))
            .build();
        let _ = client.post("http://127.0.0.1:7865/collab/notify")
            .set("Content-Type", "application/json")
            .send_string("{}");
    });
}

// ==================== JSONL Helper Functions ====================

/// Read all messages from board.jsonl
fn read_board(project_dir: &str) -> Vec<serde_json::Value> {
    let path = board_jsonl_path(project_dir);
    std::fs::read_to_string(&path)
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

/// Read board messages filtered by message_retention_days from project settings.
/// Value 0 = keep all. Messages with unparseable timestamps are kept.
fn read_board_filtered(project_dir: &str) -> Vec<serde_json::Value> {
    let all = read_board(project_dir);
    let retention_days = read_project_config(project_dir)
        .ok()
        .and_then(|c| c.get("settings")?.get("message_retention_days")?.as_u64())
        .unwrap_or(7);
    if retention_days == 0 {
        return all;
    }
    let max_age_secs = retention_days * 86400;
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    all.into_iter()
        .filter(|msg| {
            match msg.get("timestamp").and_then(|t| t.as_str()).and_then(parse_iso_to_epoch_secs) {
                Some(msg_secs) => now_secs.saturating_sub(msg_secs) <= max_age_secs,
                None => true,
            }
        })
        .collect()
}

/// Get the next message ID (count of existing messages + 1)
fn next_message_id(project_dir: &str) -> u64 {
    let path = board_jsonl_path(project_dir);
    let count = std::fs::read_to_string(&path)
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .count();
    (count + 1) as u64
}

/// Append a message to board.jsonl (caller must hold file lock)
fn append_to_board(project_dir: &str, message: &serde_json::Value) -> Result<(), String> {
    let path = board_jsonl_path(project_dir);
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| format!("Failed to open board.jsonl: {}", e))?;
    let line = serde_json::to_string(message)
        .map_err(|e| format!("Failed to serialize message: {}", e))?;
    writeln!(file, "{}", line)
        .map_err(|e| format!("Failed to write to board.jsonl: {}", e))?;
    Ok(())
}

/// Mark a seat as intentionally-left so the launcher's repopulate_spawned skips
/// auto-respawning it on the next vaak start.
///
/// Why this file (not spawned.json directly): launcher.rs writes spawned.json
/// without a file lock; cross-process writes from vaak-mcp would race. This
/// sentinel file is single-writer (vaak-mcp only) until launcher reads + deletes
/// it on next startup. Closes the gap evil-arch raised at #710(2) — auto-respawn
/// without this would resurrect kicked / project_leave'd roles after every
/// vaak restart.
///
/// Format: append-only JSONL — `{"role":"developer","instance":0,"reason":"left","ts":"..."}`.
/// launcher.rs reads, applies as a skip-list, then deletes the file.
fn mark_seat_intentionally_left(project_dir: &str, role: &str, instance: u32, reason: &str) {
    let path = std::path::Path::new(project_dir)
        .join(".vaak")
        .join("intentionally_left.jsonl");
    let entry = serde_json::json!({
        "role": role,
        "instance": instance,
        "reason": reason,
        "ts": utc_now_iso(),
    });
    if let Ok(line) = serde_json::to_string(&entry) {
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .and_then(|mut f| {
                use std::io::Write;
                writeln!(f, "{}", line)
            });
    }
}

/// Read sessions.json
fn read_sessions(project_dir: &str) -> serde_json::Value {
    let path = sessions_json_path(project_dir);
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(serde_json::json!({"bindings": []}))
}

/// Write sessions.json
fn write_sessions(project_dir: &str, sessions: &serde_json::Value) -> Result<(), String> {
    let path = sessions_json_path(project_dir);
    let content = serde_json::to_string_pretty(sessions)
        .map_err(|e| format!("Failed to serialize sessions: {}", e))?;
    atomic_write(&path, content.as_bytes())
        .map_err(|e| format!("Failed to write sessions.json: {}", e))?;
    Ok(())
}

/// Update this session's last_heartbeat in sessions.json so the desktop app
/// can accurately detect active vs idle vs gone sessions.
fn update_session_heartbeat_in_file() {
    let state = match ACTIVE_PROJECT.lock() {
        Ok(guard) => match guard.as_ref() {
            Some(s) => s.clone(),
            None => return,
        },
        Err(_) => return,
    };

    let _ = with_file_lock(&state.project_dir, || {
        let mut sessions = read_sessions(&state.project_dir);
        let now = utc_now_iso();
        if let Some(bindings) = sessions.get_mut("bindings").and_then(|b| b.as_array_mut()) {
            let mut found = false;
            for binding in bindings.iter_mut() {
                if binding.get("session_id").and_then(|s| s.as_str()) == Some(&state.session_id) {
                    binding["last_heartbeat"] = serde_json::json!(now);
                    found = true;
                }
            }
            // If binding was removed, check if it was revoked vs. stale-swept
            if !found {
                // Check if there's a revoked binding for us
                let was_revoked = bindings.iter().any(|b| {
                    b.get("session_id").and_then(|s| s.as_str()) == Some(&state.session_id)
                    && b.get("status").and_then(|s| s.as_str()) == Some("revoked")
                });
                if was_revoked {
                    eprintln!("[vaak-mcp] Session was revoked — not re-registering");
                } else {
                    // Binding was removed by another agent's stale sweep — re-register
                    eprintln!("[vaak-mcp] Session binding lost (stale sweep?) — re-registering");
                    bindings.push(serde_json::json!({
                        "session_id": state.session_id,
                        "role": state.role,
                        "instance": state.instance,
                        "status": "active",
                        "activity": "working",
                        "claimed_at": now,
                        "last_heartbeat": now
                    }));
                    found = true;
                }
            }

            // If roster exists, check if this session's slot was removed from roster
            if found {
                if let Ok(cfg) = read_project_config(&state.project_dir) {
                    if let Some(roster) = cfg.get("roster").and_then(|r| r.as_array()) {
                        let has_slot = roster.iter().any(|s| {
                            s.get("role").and_then(|r| r.as_str()) == Some(&state.role)
                                && s.get("instance").and_then(|i| i.as_u64()) == Some(state.instance as u64)
                        });
                        if !has_slot {
                            // Roster slot was removed — revoke this session
                            bindings.retain(|b| {
                                b.get("session_id").and_then(|s| s.as_str()) != Some(&state.session_id)
                            });
                            eprintln!("[vaak-mcp] Roster slot removed for {}:{} — session revoked",
                                state.role, state.instance);
                        }
                    }
                }
            }
        }
        write_sessions(&state.project_dir, &sessions)?;
        Ok(())
    });
}

/// Check if this session's binding has been revoked (removed from sessions.json)
fn is_session_revoked(project_dir: &str, session_id: &str) -> bool {
    let sessions = read_sessions(project_dir);
    if let Some(bindings) = sessions.get("bindings").and_then(|b| b.as_array()) {
        !bindings.iter().any(|b| {
            b.get("session_id").and_then(|s| s.as_str()) == Some(session_id)
        })
    } else {
        false // Can't read sessions, assume not revoked
    }
}

/// Append a sidecar lifecycle event to `.vaak/sidecar-events.jsonl` for post-mortem
/// diagnosis of "vaak reset killed my agents" complaints. Best-effort, fail-silent.
/// Captures stdin errors, heartbeat failures, and main-loop exit reasons so the team
/// can confirm/reject hypothesis (1) (Tauri restart severs MCP heartbeat path) without
/// guessing.
fn log_sidecar_event(event_type: &str, details: serde_json::Value) {
    let state = match ACTIVE_PROJECT.lock() {
        Ok(g) => g.as_ref().cloned(),
        Err(_) => None,
    };
    let (project_dir, session_id) = match state {
        Some(s) => (s.project_dir, s.session_id),
        None => (String::new(), String::new()),
    };
    if project_dir.is_empty() { return; }

    let log_path = std::path::Path::new(&project_dir).join(".vaak").join("sidecar-events.jsonl");
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let entry = serde_json::json!({
        "ts_ms": now_ms,
        "session_id": session_id,
        "pid": std::process::id(),
        "event": event_type,
        "details": details,
    });
    let line = format!("{}\n", entry.to_string());
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&log_path) {
        let _ = f.write_all(line.as_bytes());
    }
}

/// Update the activity state of this session in sessions.json ("working", "standby", "idle")
fn update_session_activity(activity: &str) {
    let state = match ACTIVE_PROJECT.lock() {
        Ok(guard) => match guard.as_ref() {
            Some(s) => s.clone(),
            None => return,
        },
        Err(_) => return,
    };

    let _ = with_file_lock(&state.project_dir, || {
        let mut sessions = read_sessions(&state.project_dir);
        if let Some(bindings) = sessions.get_mut("bindings").and_then(|b| b.as_array_mut()) {
            for binding in bindings.iter_mut() {
                if binding.get("session_id").and_then(|s| s.as_str()) == Some(&state.session_id) {
                    binding["activity"] = serde_json::json!(activity);
                    // When disconnecting, also mark status so team_status doesn't count ghosts
                    if activity == "disconnected" {
                        binding["status"] = serde_json::json!("disconnected");
                    }
                    // Track when the session last entered "working" state so the UI
                    // can show a minimum display duration (avoids flicker from brief work)
                    if activity == "working" {
                        binding["last_working_at"] = serde_json::json!(utc_now_iso());
                    }
                }
            }
        }
        write_sessions(&state.project_dir, &sessions)?;
        Ok(())
    });
}

/// Read project.json
fn read_project_config(project_dir: &str) -> Result<serde_json::Value, String> {
    let path = project_json_path(project_dir);
    let content = std::fs::read_to_string(&path)
        .map_err(|e| format!("No .vaak/project.json found: {}", e))?;
    serde_json::from_str(&content)
        .map_err(|e| format!("Invalid project.json: {}", e))
}

// ==================== Claims Helper Functions ====================

fn claims_json_path(project_dir: &str) -> PathBuf {
    vaak_dir(project_dir).join("claims.json")
}

fn read_claims(project_dir: &str) -> serde_json::Value {
    let path = claims_json_path(project_dir);
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(serde_json::json!({}))
}

fn write_claims(project_dir: &str, claims: &serde_json::Value) -> Result<(), String> {
    let path = claims_json_path(project_dir);
    let content = serde_json::to_string_pretty(claims)
        .map_err(|e| format!("Failed to serialize claims: {}", e))?;
    atomic_write(&path, content.as_bytes())
        .map_err(|e| format!("Failed to write claims.json: {}", e))?;
    Ok(())
}

/// Read claims, removing stale entries by cross-referencing with sessions.json heartbeats.
///
/// Sister-fix to active-claims-v1 c4e31c1 per evil-architect:0 msg 5068 F-EA-CA-3:
/// SURVIVING claims now carry an `alive_state` field ("active" | "stale" | "unknown")
/// derived from `.vaak/sessions/<role>-<inst>.json:last_alive_at_ms` so MCP-side
/// callers (agent claim queries, conflict-detection, claim-injection in handle_
/// project_check) speak the same liveness language as the UI's read_claims_filtered
/// in collab.rs:441. Threshold mirrors collab::staleness_thresholds::ALIVE_STATE_
/// STALE_MS — duplicated here as a literal because the sidecar binary cannot
/// reach into the desktop bin's crate. Future architectural close moves this
/// into a shared lib module (forward-flag from evil-arch msg 5068 Path B).
fn read_claims_filtered(project_dir: &str) -> serde_json::Value {
    /// Mirror of collab::staleness_thresholds::ALIVE_STATE_STALE_MS. MUST be
    /// kept in sync — if you change one, change both. Future Path B refactor
    /// puts both behind a shared module so this comment becomes obsolete.
    const ALIVE_STATE_STALE_MS: u64 = 120_000;

    let claims = read_claims(project_dir);
    let claims_obj = match claims.as_object() {
        Some(o) => o,
        None => return serde_json::json!({}),
    };
    if claims_obj.is_empty() {
        return claims;
    }

    let sessions = read_sessions(project_dir);
    let bindings = sessions.get("bindings").and_then(|b| b.as_array()).cloned().unwrap_or_default();

    let config = read_project_config(project_dir).unwrap_or(serde_json::json!({}));
    let timeout_secs = config.get("settings")
        .and_then(|s| s.get("heartbeat_timeout_seconds"))
        .and_then(|t| t.as_u64())
        .unwrap_or(120);
    let gone_threshold = (timeout_secs as f64 * 2.5) as u64;
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let now_ms = now_secs.saturating_mul(1000);
    let sessions_dir = std::path::Path::new(project_dir).join(".vaak").join("sessions");

    let mut clean = serde_json::Map::new();
    let mut any_removed = false;

    for (key, val) in claims_obj {
        let session_id = val.get("session_id").and_then(|s| s.as_str()).unwrap_or("");
        let binding = bindings.iter().find(|b| {
            b.get("session_id").and_then(|s| s.as_str()) == Some(session_id)
        });
        let is_gone = match binding {
            None => true,
            Some(b) => {
                let hb = b.get("last_heartbeat").and_then(|h| h.as_str()).unwrap_or("");
                let age = parse_iso_to_epoch_secs(hb)
                    .map(|hb_secs| now_secs.saturating_sub(hb_secs))
                    .unwrap_or(u64::MAX);
                age > gone_threshold
            }
        };
        if is_gone {
            any_removed = true;
            continue;
        }

        // Surviving claim — derive alive_state from per-seat keepalive file.
        // Matches collab.rs:read_claims_filtered logic so MCP and UI agree.
        let alive_state: String = (|| {
            let (role, instance) = key.split_once(':')?;
            let seat_file = sessions_dir.join(format!("{}-{}.json", role, instance));
            let raw = std::fs::read_to_string(&seat_file).ok()?;
            let parsed: serde_json::Value = serde_json::from_str(&raw).ok()?;
            let last_alive_at_ms = parsed.get("last_alive_at_ms").and_then(|m| m.as_u64()).unwrap_or(0);
            if last_alive_at_ms == 0 {
                return Some("unknown".to_string());
            }
            let stale_ms = now_ms.saturating_sub(last_alive_at_ms);
            if stale_ms > ALIVE_STATE_STALE_MS {
                Some("stale".to_string())
            } else {
                Some("active".to_string())
            }
        })().unwrap_or_else(|| "unknown".to_string());

        // Inject alive_state into the cloned claim object. Additive-only —
        // existing callers that iterate `files`/`description`/`claimed_at` ignore
        // the new field, so back-compat is preserved.
        let mut enriched = val.clone();
        if let Some(obj) = enriched.as_object_mut() {
            obj.insert("alive_state".to_string(), serde_json::Value::String(alive_state));
        }
        clean.insert(key.clone(), enriched);
    }

    let result = serde_json::Value::Object(clean);
    if any_removed {
        // Persist the SOURCE claim payload (without the derived alive_state
        // field) — alive_state is computed on every read, not stored.
        let mut to_persist = serde_json::Map::new();
        if let Some(obj) = result.as_object() {
            for (k, v) in obj {
                let mut stripped = v.clone();
                if let Some(m) = stripped.as_object_mut() {
                    m.remove("alive_state");
                }
                to_persist.insert(k.clone(), stripped);
            }
        }
        let _ = write_claims(project_dir, &serde_json::Value::Object(to_persist));
    }
    result
}

/// Handle project_claim: claim files for this session
fn handle_project_claim(files: Vec<String>, description: &str) -> Result<serde_json::Value, String> {
    let state = get_or_rejoin_state()?;

    let my_key = format!("{}:{}", state.role, state.instance);

    let result = with_file_lock(&state.project_dir, || {
        let claims = read_claims_filtered(&state.project_dir);
        let claims_obj = claims.as_object().cloned().unwrap_or_default();

        // Check for overlaps with other claimants
        let mut conflicts: Vec<serde_json::Value> = Vec::new();
        for (key, val) in &claims_obj {
            if key == &my_key { continue; }
            let their_files: Vec<String> = val.get("files")
                .and_then(|f| f.as_array())
                .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
                .unwrap_or_default();

            let overlapping: Vec<String> = files.iter()
                .filter(|f| {
                    their_files.iter().any(|tf| {
                        f.starts_with(tf.as_str()) || tf.starts_with(f.as_str()) || *f == tf
                    })
                })
                .cloned()
                .collect();

            if !overlapping.is_empty() {
                let desc = val.get("description").and_then(|d| d.as_str()).unwrap_or("");
                conflicts.push(serde_json::json!({
                    "claimant": key,
                    "overlapping_files": overlapping,
                    "their_description": desc
                }));
            }
        }

        // Write our claim
        let mut updated = claims_obj.clone();
        updated.insert(my_key.clone(), serde_json::json!({
            "files": files,
            "description": description,
            "claimed_at": utc_now_iso(),
            "session_id": state.session_id
        }));
        write_claims(&state.project_dir, &serde_json::Value::Object(updated.into_iter().collect()))?;

        Ok(conflicts)
    })?;

    update_session_heartbeat_in_file();
    notify_desktop();

    let mut response = serde_json::json!({
        "status": "claimed",
        "claimant": my_key,
        "files": files,
        "description": description
    });

    if !result.is_empty() {
        let conflict_summary: Vec<String> = result.iter()
            .map(|c| {
                let who = c.get("claimant").and_then(|w| w.as_str()).unwrap_or("?");
                let overlap = c.get("overlapping_files").and_then(|o| o.as_array())
                    .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>().join(", "))
                    .unwrap_or_default();
                format!("{} already claims: {}", who, overlap)
            })
            .collect();
        response["conflicts"] = serde_json::json!(result);
        response["warning"] = serde_json::json!(format!("⚠️ CONFLICT: {}", conflict_summary.join("; ")));
    }

    Ok(response)
}

/// Handle project_release: release this session's file claim
fn handle_project_release() -> Result<serde_json::Value, String> {
    let state = get_or_rejoin_state()?;

    let my_key = format!("{}:{}", state.role, state.instance);

    with_file_lock(&state.project_dir, || {
        let claims = read_claims(&state.project_dir);
        let mut claims_obj = claims.as_object().cloned().unwrap_or_default();
        claims_obj.remove(&my_key);
        write_claims(&state.project_dir, &serde_json::Value::Object(claims_obj.into_iter().collect()))?;
        Ok(())
    })?;

    notify_desktop();

    Ok(serde_json::json!({
        "status": "released",
        "claimant": my_key
    }))
}

/// Handle project_claims: return all active claims (read-only)
fn handle_project_claims() -> Result<serde_json::Value, String> {
    let state = get_or_rejoin_state()?;

    let claims = read_claims_filtered(&state.project_dir);

    Ok(serde_json::json!({
        "claims": claims
    }))
}

// ==================== Discussion Control ====================

/// Returns the discussion.json path for the active section.
/// "default" section uses legacy flat .vaak/discussion.json for backward compatibility.
/// Non-default sections use .vaak/sections/{slug}/discussion.json.
/// Matches collab.rs discussion_path_for_section().
fn discussion_json_path(project_dir: &str) -> PathBuf {
    let section = get_active_section(project_dir);
    if section == "default" {
        vaak_dir(project_dir).join("discussion.json")
    } else {
        vaak_dir(project_dir).join("sections").join(section).join("discussion.json")
    }
}

fn read_discussion_state(project_dir: &str) -> serde_json::Value {
    std::fs::read_to_string(discussion_json_path(project_dir))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(serde_json::json!({
            "active": false,
            "mode": null,
            "topic": "",
            "started_at": null,
            "moderator": null,
            "participants": [],
            "current_round": 0,
            "phase": null,
            "paused_at": null,
            "expire_at": null,
            "previous_phase": null,
            "rounds": [],
            "settings": {
                "max_rounds": 10,
                "timeout_minutes": 15,
                "expire_paused_after_minutes": 60
            }
        }))
}

/// Write discussion state WITHOUT acquiring the lock.
/// Use this when the caller already holds with_discussion_lock.
fn write_discussion_state_unlocked(project_dir: &str, state: &serde_json::Value) -> Result<(), String> {
    let path = discussion_json_path(project_dir);
    let content = serde_json::to_string_pretty(state)
        .map_err(|e| format!("Failed to serialize discussion state: {}", e))?;
    atomic_write(&path, content.as_bytes())
        .map_err(|e| format!("Failed to write discussion.json: {}", e))
}

/// Write discussion state WITH file locking.
/// Acquires with_discussion_lock to prevent concurrent write corruption.
/// Do NOT call from within with_discussion_lock (use write_discussion_state_unlocked instead).
fn write_discussion_state(project_dir: &str, state: &serde_json::Value) -> Result<(), String> {
    let content = serde_json::to_string_pretty(state)
        .map_err(|e| format!("Failed to serialize discussion state: {}", e))?;
    let pd = project_dir.to_string();
    with_discussion_lock(project_dir, move || {
        let path = discussion_json_path(&pd);
        atomic_write(&path, content.as_bytes())
            .map_err(|e| format!("Failed to write discussion.json: {}", e))
    })
}

/// Generate anonymized aggregate from submissions in the current round.
/// Collects submission messages from board.jsonl, strips identity, randomizes order.
/// For Oxford with teams: groups submissions by team (FOR/AGAINST) instead of randomizing.
fn generate_aggregate(project_dir: &str, discussion: &serde_json::Value) -> Result<String, String> {
    let rounds = discussion.get("rounds").and_then(|r| r.as_array())
        .ok_or("No rounds in discussion state")?;
    let current_round = rounds.last().ok_or("No current round")?;
    let submissions = current_round.get("submissions").and_then(|s| s.as_array());
    let tracked_ids: Vec<u64> = submissions.map(|subs| {
        subs.iter().filter_map(|s| s.get("message_id").and_then(|id| id.as_u64())).collect()
    }).unwrap_or_default();

    // Read all board messages
    let all_messages = read_board(project_dir);

    // Extract submissions as (from, body) tuples
    let mut entries: Vec<(String, String)> = Vec::new();
    if !tracked_ids.is_empty() {
        for msg in &all_messages {
            let id = msg.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
            if tracked_ids.contains(&id) {
                let from = msg.get("from").and_then(|f| f.as_str()).unwrap_or("unknown");
                let body = msg.get("body").and_then(|b| b.as_str()).unwrap_or("(empty)");
                entries.push((from.to_string(), body.to_string()));
            }
        }
    } else {
        // Fallback: find type="submission" messages within this round's time window
        let opened_at = current_round.get("opened_at").and_then(|t| t.as_str()).unwrap_or("");
        let closed_at = current_round.get("closed_at").and_then(|t| t.as_str()).unwrap_or("");
        for msg in &all_messages {
            let msg_type = msg.get("type").and_then(|t| t.as_str()).unwrap_or("");
            let ts = msg.get("timestamp").and_then(|t| t.as_str()).unwrap_or("");
            if msg_type == "submission" && ts >= opened_at && (closed_at.is_empty() || ts <= closed_at) {
                let from = msg.get("from").and_then(|f| f.as_str()).unwrap_or("unknown");
                let body = msg.get("body").and_then(|b| b.as_str()).unwrap_or("(empty)");
                entries.push((from.to_string(), body.to_string()));
            }
        }
    }

    if entries.is_empty() {
        return Ok("No submissions received this round.".to_string());
    }

    // Build aggregate text
    let round_num = current_round.get("number").and_then(|n| n.as_u64()).unwrap_or(0);
    let topic = discussion.get("topic").and_then(|t| t.as_str()).unwrap_or("(no topic)");
    let disc_mode = discussion.get("mode").and_then(|m| m.as_str()).unwrap_or("discussion");
    let format_name = {
        let mut chars = disc_mode.chars();
        match chars.next() {
            Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
            None => "Discussion".to_string(),
        }
    };
    let total = entries.len();

    // Check if Oxford with teams set — group by team instead of randomizing
    let teams = discussion.get("teams");
    let has_teams = disc_mode == "oxford" && teams.map(|t| !t.is_null()).unwrap_or(false);

    let mut aggregate = format!(
        "## {} Round {} Aggregate — {} submissions\n**Topic:** {}\n\n---\n\n",
        format_name, round_num, total, topic
    );

    if has_teams {
        let teams_obj = teams.unwrap();
        let team_for: Vec<String> = teams_obj.get("for")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
            .unwrap_or_default();
        let team_against: Vec<String> = teams_obj.get("against")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
            .unwrap_or_default();

        // Group submissions by team
        let mut for_subs: Vec<&str> = Vec::new();
        let mut against_subs: Vec<&str> = Vec::new();
        let mut unassigned_subs: Vec<&str> = Vec::new();

        for (from, body) in &entries {
            if team_for.contains(from) {
                for_subs.push(body);
            } else if team_against.contains(from) {
                against_subs.push(body);
            } else {
                unassigned_subs.push(body);
            }
        }

        if !for_subs.is_empty() {
            aggregate.push_str("## TEAM FOR\n\n");
            for (i, body) in for_subs.iter().enumerate() {
                aggregate.push_str(&format!("### FOR — Submission {}\n{}\n\n---\n\n", i + 1, body));
            }
        }
        if !against_subs.is_empty() {
            aggregate.push_str("## TEAM AGAINST\n\n");
            for (i, body) in against_subs.iter().enumerate() {
                aggregate.push_str(&format!("### AGAINST — Submission {}\n{}\n\n---\n\n", i + 1, body));
            }
        }
        if !unassigned_subs.is_empty() {
            aggregate.push_str("## UNASSIGNED\n\n");
            for (i, body) in unassigned_subs.iter().enumerate() {
                aggregate.push_str(&format!("### Unassigned — Submission {}\n{}\n\n---\n\n", i + 1, body));
            }
        }

        aggregate.push_str(&format!(
            "*{} submissions collected. Grouped by team assignment. Identities anonymized within teams.*",
            total
        ));
    } else {
        // Standard: randomize order using Fisher-Yates shuffle with cryptographic seed.
        // Uses UUID v4 (backed by getrandom/OS entropy) instead of predictable nanosecond timestamp.
        let mut bodies: Vec<&str> = entries.iter().map(|(_, b)| b.as_str()).collect();
        let mut rng_state = uuid::Uuid::new_v4().as_u128();
        for i in (1..bodies.len()).rev() {
            rng_state = rng_state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let j = (rng_state as usize) % (i + 1);
            bodies.swap(i, j);
        }

        for (i, body) in bodies.iter().enumerate() {
            aggregate.push_str(&format!("### Participant {}\n{}\n\n---\n\n", i + 1, body));
        }

        aggregate.push_str(&format!(
            "*{} submissions collected. Order randomized. Identities anonymized.*",
            total
        ));
    }

    Ok(aggregate)
}

/// Generate a lightweight tally-based aggregate for continuous review mode.
/// Instead of anonymized full text, produces: "X agree, Y disagree (reasons), Z alternatives"
fn generate_mini_aggregate(project_dir: &str, discussion: &serde_json::Value) -> Result<String, String> {
    let rounds = discussion.get("rounds").and_then(|r| r.as_array())
        .ok_or("No rounds in discussion state")?;
    let current_round = rounds.last().ok_or("No current round")?;
    let submissions = current_round.get("submissions").and_then(|s| s.as_array())
        .ok_or("No submissions in current round")?;
    let round_topic = current_round.get("topic").and_then(|t| t.as_str()).unwrap_or("(no topic)");
    let round_num = current_round.get("number").and_then(|n| n.as_u64()).unwrap_or(0);

    if submissions.is_empty() {
        return Ok(format!("**Review #{} — No responses** (silence = consent)\nChange: {}", round_num, round_topic));
    }

    // Collect submission message IDs
    let msg_ids: Vec<u64> = submissions.iter()
        .filter_map(|s| s.get("message_id").and_then(|id| id.as_u64()))
        .collect();

    let all_messages = read_board(project_dir);

    let mut agree_count = 0u32;
    let mut neutral_count = 0u32;
    let mut disagree_reasons: Vec<String> = Vec::new();
    let mut alternatives: Vec<String> = Vec::new();

    for msg in &all_messages {
        let id = msg.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
        if !msg_ids.contains(&id) { continue; }

        let body = msg.get("body").and_then(|b| b.as_str()).unwrap_or("").trim().to_lowercase();

        if body.starts_with("agree") || body == "lgtm" || body == "approved" || body == "+1"
            || body.starts_with("looks good") || body.starts_with("makes sense")
            || body.starts_with("i'm fine with") || body.starts_with("im fine with")
            || body.starts_with("no objection") || body.starts_with("sounds good")
            || body.starts_with("i agree") || body.starts_with("fine with")
            || body.starts_with("works for me") || body.starts_with("ship it")
            || body.starts_with("no concerns") || body.starts_with("all good")
            || body.starts_with("thumbs up") || body.starts_with("go ahead")
            || body.starts_with("no issues") || body.starts_with("acknowledged")
        {
            agree_count += 1;
        } else if body.starts_with("disagree") || body.starts_with("object") || body.starts_with("-1")
            || body.starts_with("block") || body.starts_with("reject") || body.starts_with("nack")
        {
            let reason = msg.get("body").and_then(|b| b.as_str()).unwrap_or("(no reason)").to_string();
            disagree_reasons.push(reason);
        } else if body.starts_with("alternative") || body.starts_with("suggest") || body.starts_with("instead") {
            let proposal = msg.get("body").and_then(|b| b.as_str()).unwrap_or("(no proposal)").to_string();
            alternatives.push(proposal);
        } else if body.starts_with("neutral") || body.starts_with("no opinion") || body.starts_with("abstain")
            || body.starts_with("n/a") || body.starts_with("no comment") || body.starts_with("pass")
            || body.starts_with("defer") || body.starts_with("no preference")
        {
            neutral_count += 1;
        } else {
            // Unclassified responses count as neutral — not agree.
            // Long/ambiguous messages shouldn't inflate the agree count.
            // Silence (no response at all) = consent; a response that can't be
            // classified is a distinct signal that should not be conflated with agreement.
            neutral_count += 1;
        }
    }

    let total = submissions.len();
    let consensus = if disagree_reasons.is_empty() && alternatives.is_empty() {
        if agree_count > 0 || total == 0 {
            "APPROVED"
        } else {
            // All responses were neutral/unclassified — not a clear approval
            "NOTED"
        }
    } else if disagree_reasons.len() > agree_count as usize {
        "CONTESTED"
    } else {
        "MIXED"
    };

    let mut result = format!(
        "**Review #{} — {} ({}/{} responded)**\nChange: {}\n\n",
        round_num, consensus, total, total, round_topic
    );

    result.push_str(&format!("- {} agree\n", agree_count));
    if neutral_count > 0 {
        result.push_str(&format!("- {} neutral\n", neutral_count));
    }
    if !disagree_reasons.is_empty() {
        result.push_str(&format!("- {} disagree:\n", disagree_reasons.len()));
        for (i, reason) in disagree_reasons.iter().enumerate() {
            result.push_str(&format!("  {}. {}\n", i + 1, reason));
        }
    }
    if !alternatives.is_empty() {
        result.push_str(&format!("- {} alternatives:\n", alternatives.len()));
        for (i, alt) in alternatives.iter().enumerate() {
            result.push_str(&format!("  {}. {}\n", i + 1, alt));
        }
    }

    Ok(result)
}

/// Auto-create a micro-round in continuous mode when a developer posts a status message.
/// Returns the new round number, or None if no round was created.
fn auto_create_continuous_round(project_dir: &str, status_msg_subject: &str, status_msg_body: &str, author: &str, msg_id: u64) -> Option<u32> {
    // Wrap in discussion lock to prevent race condition when two status messages arrive simultaneously
    let subject = status_msg_subject.to_string();
    let body = status_msg_body.to_string();
    let author_owned = author.to_string();
    let pd = project_dir.to_string();

    let result = with_discussion_lock(project_dir, move || {
        let disc = read_discussion_state(&pd);
        let is_active = disc.get("active").and_then(|v| v.as_bool()).unwrap_or(false);
        let mode = disc.get("mode").and_then(|v| v.as_str()).unwrap_or("");

        if !is_active || mode != "continuous" {
            return Ok(None);
        }

        // Don't create rounds for the moderator's own moderation messages
        let moderator = disc.get("moderator").and_then(|v| v.as_str()).unwrap_or("");
        if author_owned == moderator {
            return Ok(None);
        }

        // Close any open round that's timed out before creating a new one
        let _ = auto_close_timed_out_round_inner(&pd);

        let now = utc_now_iso();
        let mut updated = disc.clone();

        // Check if there's already an open round — don't create a new one
        let current_phase = updated.get("phase").and_then(|v| v.as_str()).unwrap_or("");
        if current_phase == "submitting" {
            return Ok(None);
        }

        let current_round = updated.get("current_round").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        let next_round = current_round + 1;

        // Build round topic from the status message
        let topic = if !subject.is_empty() {
            subject.clone()
        } else if body.len() > 200 {
            format!("{}...", &body[..200])
        } else {
            body.clone()
        };

        updated["current_round"] = serde_json::json!(next_round);
        updated["phase"] = serde_json::json!("submitting");

        if let Some(rounds) = updated.get_mut("rounds").and_then(|r| r.as_array_mut()) {
            rounds.push(serde_json::json!({
                "number": next_round,
                "opened_at": now,
                "closed_at": null,
                "submissions": [],
                "aggregate_message_id": null,
                "auto_triggered": true,
                "topic": topic,
                "trigger_from": author_owned,
                "trigger_subject": topic,
                "trigger_message_id": msg_id
            }));
        }

        let _ = write_discussion_state_unlocked(&pd, &updated);

        // Post review window notification
        let timeout = updated.get("settings")
            .and_then(|s| s.get("auto_close_timeout_seconds"))
            .and_then(|t| t.as_u64())
            .unwrap_or(60);

        let board_msg_id = next_message_id(&pd);
        let notification = serde_json::json!({
            "id": board_msg_id,
            "from": "system",
            "to": "all",
            "type": "moderation",
            "timestamp": now,
            "subject": format!("Review #{}: {}", next_round, if topic.len() > 80 { &topic[..80] } else { &topic }),
            "body": format!("**REVIEW WINDOW OPEN** ({}s)\n{} reported: {}\n\nRespond with: agree / neutral / disagree: [reason] / alternative: [proposal]\nSilence within {}s = consent.", timeout, author_owned, topic, timeout),
            "metadata": {
                "discussion_action": "auto_round",
                "round": next_round,
                "author": author_owned,
                "timeout_seconds": timeout
            }
        });
        let _ = append_to_board(&pd, &notification);

        Ok(Some(next_round))
    });

    result.ok().flatten()
}

/// Inner implementation of auto-close (called within discussion lock).
fn auto_close_timed_out_round_inner(project_dir: &str) -> bool {
    let disc = read_discussion_state(project_dir);
    let is_active = disc.get("active").and_then(|v| v.as_bool()).unwrap_or(false);
    let mode = disc.get("mode").and_then(|v| v.as_str()).unwrap_or("");
    let phase = disc.get("phase").and_then(|v| v.as_str()).unwrap_or("");

    if !is_active || mode != "continuous" || phase != "submitting" {
        return false;
    }

    let timeout_secs = disc.get("settings")
        .and_then(|s| s.get("auto_close_timeout_seconds"))
        .and_then(|t| t.as_u64())
        .unwrap_or(60);

    // Check if the current round opened_at + timeout has passed
    let rounds = match disc.get("rounds").and_then(|r| r.as_array()) {
        Some(r) => r,
        None => return false,
    };
    let current_round = match rounds.last() {
        Some(r) => r,
        None => return false,
    };
    let opened_at = match current_round.get("opened_at").and_then(|t| t.as_str()).and_then(parse_iso_to_epoch_secs) {
        Some(t) => t,
        None => return false,
    };

    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    if now_secs.saturating_sub(opened_at) < timeout_secs {
        return false; // Not timed out yet
    }

    // Timed out — auto-close and generate mini-aggregate
    let aggregate_text = generate_mini_aggregate(project_dir, &disc).unwrap_or_else(|_| "Auto-close: no aggregate generated.".to_string());

    let now = utc_now_iso();
    let round_num = current_round.get("number").and_then(|n| n.as_u64()).unwrap_or(0);

    // Post mini-aggregate
    let msg_id = next_message_id(project_dir);
    let aggregate_msg = serde_json::json!({
        "id": msg_id,
        "from": "system",
        "to": "all",
        "type": "moderation",
        "timestamp": now,
        "subject": format!("Review #{} closed", round_num),
        "body": aggregate_text,
        "metadata": {
            "discussion_action": "auto_aggregate",
            "round": round_num
        }
    });
    let _ = append_to_board(project_dir, &aggregate_msg);

    // Update discussion state
    let mut updated = disc.clone();
    if let Some(rounds) = updated.get_mut("rounds").and_then(|r| r.as_array_mut()) {
        if let Some(last) = rounds.last_mut() {
            last["closed_at"] = serde_json::json!(now);
            last["aggregate_message_id"] = serde_json::json!(msg_id);
        }
    }
    // In continuous mode, phase goes back to "reviewing" (ready for next auto-trigger)
    updated["phase"] = serde_json::json!("reviewing");
    let _ = write_discussion_state_unlocked(project_dir, &updated);

    true
}

/// Check if the current round has timed out. Acquires discussion lock.
fn auto_close_timed_out_round(project_dir: &str) -> bool {
    let pd = project_dir.to_string();
    with_discussion_lock(project_dir, move || {
        Ok(auto_close_timed_out_round_inner(&pd))
    }).unwrap_or(false)
}

/// Check if quorum is reached for the current continuous review round.
/// Quorum = all non-author participants have submitted.
fn check_continuous_quorum(project_dir: &str) -> bool {
    let disc = read_discussion_state(project_dir);
    let is_active = disc.get("active").and_then(|v| v.as_bool()).unwrap_or(false);
    let mode = disc.get("mode").and_then(|v| v.as_str()).unwrap_or("");
    let phase = disc.get("phase").and_then(|v| v.as_str()).unwrap_or("");

    if !is_active || mode != "continuous" || phase != "submitting" {
        return false;
    }

    let rounds = match disc.get("rounds").and_then(|r| r.as_array()) {
        Some(r) => r,
        None => return false,
    };
    let current_round = match rounds.last() {
        Some(r) => r,
        None => return false,
    };

    let author = current_round.get("trigger_from").and_then(|a| a.as_str()).unwrap_or("");
    let participants = disc.get("participants").and_then(|p| p.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>())
        .unwrap_or_default();
    let submissions = current_round.get("submissions").and_then(|s| s.as_array());
    let submitted: Vec<&str> = submissions
        .map(|subs| subs.iter().filter_map(|s| s.get("from").and_then(|f| f.as_str())).collect())
        .unwrap_or_default();

    // Non-author participants who haven't submitted
    let pending: Vec<&&str> = participants.iter()
        .filter(|p| **p != author && !submitted.contains(p))
        .collect();

    pending.is_empty() && !submitted.is_empty()
}

// ==================== Audience Vote Tool ====================

/// Vote history directory — stored per-project in .vaak/audience-history/
fn audience_history_dir(project_dir: &str) -> PathBuf {
    vaak_dir(project_dir).join("audience-history")
}

/// Call the backend audience vote API and post results to the collab board.
fn handle_audience_vote(topic: &str, arguments: &str, phase: &str, pool: Option<&str>) -> Result<serde_json::Value, String> {
    let state = get_or_rejoin_state()?;

    // Map phase names: MCP uses pre_vote/post_vote, backend uses pre/post
    let backend_phase = match phase {
        "pre_vote" | "pre" => "pre",
        _ => "post",
    };

    // Build request body for the backend API
    let mut request_body = serde_json::json!({
        "topic": topic,
        "arguments": arguments,
        "phase": backend_phase
    });
    if let Some(pool_id) = pool {
        request_body["pool"] = serde_json::json!(pool_id);
    }

    // Call the backend API for audience voting
    eprintln!("[audience_vote] Calling backend: topic='{}', phase={}, pool={:?}",
        &topic[..topic.len().min(80)], backend_phase, pool);

    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(120)) // 27 parallel LLM calls can take time
        .build();

    let resp = agent.post(&format!("{}/api/v1/audience/vote", get_backend_url()))
        .set("Content-Type", "application/json")
        .send_string(&request_body.to_string())
        .map_err(|e| format!("Backend API call failed: {}. Is the backend running at {}?", e, get_backend_url()))?;

    let resp_str = resp.into_string()
        .map_err(|e| format!("Failed to read backend response: {}", e))?;
    let resp_body: serde_json::Value = serde_json::from_str(&resp_str)
        .map_err(|e| format!("Failed to parse backend response: {}", e))?;

    // Check for error in response
    if let Some(err) = resp_body["error"].as_str() {
        if resp_body["votes"].as_array().map(|a| a.is_empty()).unwrap_or(true) {
            return Err(format!("Audience vote error: {}", err));
        }
        // Partial results — continue with what we have
        eprintln!("[audience_vote] Partial results (some providers failed): {}", err);
    }

    // Extract tally for the board message
    let tally = resp_body.get("tally").cloned().unwrap_or(serde_json::json!({}));
    let for_count = tally["FOR"].as_u64().unwrap_or(0);
    let against_count = tally["AGAINST"].as_u64().unwrap_or(0);
    let abstain_count = tally["ABSTAIN"].as_u64().unwrap_or(0);
    let error_count = tally["ERROR"].as_u64().unwrap_or(0);
    let total = resp_body["total_voters"].as_u64().unwrap_or(0);
    let latency = resp_body["total_latency_ms"].as_u64().unwrap_or(0);
    let pool_name = resp_body["pool_name"].as_str().unwrap_or("unknown");
    let pool_id = resp_body["pool"].as_str().unwrap_or("general");

    // Build per-provider breakdown
    let mut provider_breakdown = String::new();
    if let Some(by_prov) = resp_body["tally_by_provider"].as_object() {
        for (prov, counts) in by_prov {
            let pf = counts["FOR"].as_u64().unwrap_or(0);
            let pa = counts["AGAINST"].as_u64().unwrap_or(0);
            provider_breakdown.push_str(&format!("\n  - {}: FOR {}, AGAINST {}", prov, pf, pa));
        }
    }

    // Collect notable rationales (up to 3, one per provider if possible)
    let mut notable_rationales = String::new();
    if let Some(votes) = resp_body["votes"].as_array() {
        let mut seen_providers = std::collections::HashSet::new();
        let mut count = 0;
        for vote in votes {
            if count >= 3 { break; }
            let provider = vote["provider"].as_str().unwrap_or("");
            let vote_val = vote["vote"].as_str().unwrap_or("");
            if vote_val == "ERROR" { continue; }
            if seen_providers.contains(provider) { continue; }
            seen_providers.insert(provider.to_string());
            let persona = vote["persona"].as_str().unwrap_or("Anonymous");
            let rationale = vote["rationale"].as_str().unwrap_or("");
            notable_rationales.push_str(&format!(
                "\n> **{} ({}/{}):** {}", persona, vote_val, provider, rationale
            ));
            count += 1;
        }
    }

    // Format the board message body
    let phase_label = if backend_phase == "pre" { "Pre-Vote" } else { "Post-Vote" };
    let error_note = if error_count > 0 {
        format!(" ({} provider errors)", error_count)
    } else {
        String::new()
    };

    let board_body = format!(
        "## Audience {} Results\n\
         **Topic:** {}\n\
         **Pool:** {} ({})\n\
         **Total voters:** {}{}\n\n\
         ### Tally\n\
         - **FOR:** {}\n\
         - **AGAINST:** {}\n\
         - **ABSTAIN:** {}\n\n\
         ### By Provider{}\n\n\
         ### Notable Rationales{}\n\n\
         *Completed in {}ms*",
        phase_label, topic, pool_name, pool_id, total, error_note,
        for_count, against_count, abstain_count,
        provider_breakdown,
        notable_rationales,
        latency
    );

    // Post results to the collab board as a broadcast from "audience:0"
    let board_result = with_file_lock(&state.project_dir, || {
        let msg_id = next_message_id(&state.project_dir);
        let message = serde_json::json!({
            "id": msg_id,
            "from": "audience:0",
            "to": "all",
            "type": "broadcast",
            "timestamp": utc_now_iso(),
            "subject": format!("Audience {} — {}", phase_label, &topic[..topic.len().min(60)]),
            "body": board_body,
            "metadata": {
                "audience_vote": true,
                "phase": backend_phase,
                "pool": pool_id,
                "tally": tally,
                "total_voters": total,
                "total_latency_ms": latency,
                "votes": resp_body.get("votes").cloned().unwrap_or(serde_json::json!([]))
            }
        });
        append_to_board(&state.project_dir, &message)?;
        Ok(msg_id)
    });

    // Save to vote history for longitudinal tracking
    let _ = save_vote_history(&state.project_dir, topic, backend_phase, pool_id, &resp_body);

    // Notify desktop app
    notify_desktop();

    match board_result {
        Ok(msg_id) => {
            let invoker = format!("{}:{}", state.role, state.instance);
            Ok(serde_json::json!({
                "status": "posted",
                "message_id": msg_id,
                "invoked_by": invoker,
                "phase": backend_phase,
                "pool": pool_id,
                "tally": {
                    "FOR": for_count,
                    "AGAINST": against_count,
                    "ABSTAIN": abstain_count,
                    "ERROR": error_count
                },
                "total_voters": total,
                "note": "Full results posted to the collab board as a broadcast. All team members will see them."
            }))
        }
        Err(e) => Err(format!("Vote collected but failed to post to board: {}", e))
    }
}

/// Save vote results to the per-project history directory for longitudinal analysis.
fn save_vote_history(project_dir: &str, topic: &str, phase: &str, pool: &str, results: &serde_json::Value) -> Result<(), String> {
    let dir = audience_history_dir(project_dir);
    std::fs::create_dir_all(&dir).map_err(|e| format!("Failed to create history dir: {}", e))?;

    let history_path = dir.join("votes.jsonl");
    let entry = serde_json::json!({
        "timestamp": utc_now_iso(),
        "topic": topic,
        "phase": phase,
        "pool": pool,
        "tally": results.get("tally"),
        "tally_by_provider": results.get("tally_by_provider"),
        "total_voters": results.get("total_voters"),
        "total_latency_ms": results.get("total_latency_ms"),
    });

    let line = serde_json::to_string(&entry).map_err(|e| format!("Serialize error: {}", e))?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&history_path)
        .map_err(|e| format!("Failed to open votes.jsonl: {}", e))?;
    writeln!(file, "{}", line).map_err(|e| format!("Write error: {}", e))?;
    Ok(())
}

/// Retrieve historical audience vote data for a given topic.
fn handle_audience_history(topic: &str, pool: Option<&str>) -> Result<serde_json::Value, String> {
    let state = get_or_rejoin_state()?;

    let history_path = audience_history_dir(&state.project_dir).join("votes.jsonl");
    let content = std::fs::read_to_string(&history_path)
        .unwrap_or_default();

    if content.trim().is_empty() {
        return Ok(serde_json::json!({
            "matches": [],
            "message": "No audience vote history found for this project."
        }));
    }

    let topic_lower = topic.to_lowercase();
    let pool_owned = pool.map(|p| p.to_string());
    let matches: Vec<serde_json::Value> = content.lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|entry| {
            let entry_topic = entry["topic"].as_str().unwrap_or("");
            let topic_matches = entry_topic.to_lowercase().contains(&topic_lower);
            let pool_matches = match &pool_owned {
                Some(p) => entry["pool"].as_str() == Some(p.as_str()),
                None => true,
            };
            topic_matches && pool_matches
        })
        .collect();

    if matches.is_empty() {
        return Ok(serde_json::json!({
            "matches": [],
            "message": format!("No vote history found matching topic '{}'", topic)
        }));
    }

    // Compute opinion shift if we have both pre and post votes for same topic
    let mut opinion_shift = serde_json::json!(null);
    let pre_votes: Vec<&serde_json::Value> = matches.iter()
        .filter(|m| m["phase"].as_str() == Some("pre"))
        .collect();
    let post_votes: Vec<&serde_json::Value> = matches.iter()
        .filter(|m| m["phase"].as_str() == Some("post"))
        .collect();

    if let (Some(pre), Some(post)) = (pre_votes.last(), post_votes.last()) {
        let pre_tally = pre.get("tally").cloned().unwrap_or(serde_json::json!({}));
        let post_tally = post.get("tally").cloned().unwrap_or(serde_json::json!({}));
        let pre_for = pre_tally["FOR"].as_i64().unwrap_or(0);
        let pre_against = pre_tally["AGAINST"].as_i64().unwrap_or(0);
        let post_for = post_tally["FOR"].as_i64().unwrap_or(0);
        let post_against = post_tally["AGAINST"].as_i64().unwrap_or(0);

        opinion_shift = serde_json::json!({
            "pre_vote": { "FOR": pre_for, "AGAINST": pre_against },
            "post_vote": { "FOR": post_for, "AGAINST": post_against },
            "delta_for": post_for - pre_for,
            "delta_against": post_against - pre_against,
            "shifted": pre_for != post_for || pre_against != post_against
        });
    }

    Ok(serde_json::json!({
        "matches": matches,
        "total_records": matches.len(),
        "opinion_shift": opinion_shift
    }))
}

fn handle_discussion_control(action: &str, mode: Option<&str>, topic: Option<&str>, participants: Option<Vec<String>>, teams: Option<serde_json::Value>) -> Result<serde_json::Value, String> {
    // Slice 6 deprecation warning + thin-wrap split (per #993/#994/#995/#996
    // SHIP vote). discussion_control's many sub-actions are NOT all
    // mechanically translatable to protocol_mutate — Oxford team
    // assignment, Delphi gate, etc. live outside the consensus model.
    // For Slice 6 closer, we mirror the start_discussion(continuous) +
    // close_round flows into protocol.json so the §10 single-source-of-
    // truth invariant holds for the common case. Other actions (Oxford
    // set_teams, Delphi state machine) keep their legacy paths until the
    // post-release-tail decom round.
    eprintln!(
        "[deprecated] discussion_control MCP tool ('{}') — migrate continuous-review callers to protocol_mutate(open_round/submit/close_round). Oxford/Delphi flows pending Slice 7 wider mapping.",
        action
    );

    let state = get_or_rejoin_state()?;

    let my_label = format!("{}:{}", state.role, state.instance);

    // Slice 6 closer (architect #998 PURE thin-wrap): for the continuous
    // mode + close_round flows, route to protocol_mutate ONLY — no
    // .vaak/discussion.json write. Other modes (delphi, oxford, red_team)
    // keep their legacy state machine because their semantics (Delphi
    // blind-submit gate, Oxford team assignment) exceed the consensus
    // model's surface; Slice 7 owns wider mapping.
    if action == "start_discussion" {
        // Slice 7: route delphi/oxford/red_team/continuous through
        // protocol_mutate(open_round) with mode + optional teams +
        // blind_submit_gate args. Closes COVERAGE_GAPS.md Gap A.
        let mode_val = mode.ok_or("[InvalidArgs] start_discussion requires mode")?;
        let topic_str = topic.ok_or("[InvalidArgs] start_discussion requires topic")?;
        let pd = state.project_dir.clone();
        let actor = my_label.clone();
        let section = get_active_section(&pd);
        let cur_proto = read_protocol_for_section_value(&pd, &section);
        let cur_rev = cur_proto.get("rev").and_then(|v| v.as_u64()).unwrap_or(0);

        // Map legacy mode → protocol consensus mode per spec §6 matrix.
        let (consensus_mode, blind_gate) = match mode_val {
            "continuous" => ("tally", false),
            "delphi"     => ("vote", true),  // blind-submit gate semantics
            "oxford"     => ("vote", false),
            "red_team"   => ("vote", false),
            other => return Err(format!("[InvalidArgs] start_discussion mode must be continuous|delphi|oxford|red_team (got '{}')", other)),
        };

        // Build args including teams (Oxford) if provided.
        let mut open_args = serde_json::json!({
            "topic": topic_str,
            "mode": consensus_mode
        });
        if blind_gate {
            open_args["blind_submit_gate"] = serde_json::json!(true);
        }
        if let Some(t) = &teams {
            // Only valid for oxford per spec, but pass through if provided
            // (open_round validates shape; non-oxford callers won't supply).
            open_args["teams"] = t.clone();
        }

        let new_state = do_protocol_mutate(
            &pd,
            &actor,
            &section,
            "open_round",
            open_args,
            Some(cur_rev),
        )?;
        // Project to legacy shape so old callers' result-handling code
        // keeps working through the compat tail.
        return Ok(serde_json::json!({
            "active": true,
            "mode": mode_val,
            "topic": topic_str,
            "moderator": new_state.get("consensus").and_then(|c| c.get("round")).and_then(|r| r.get("opened_by")).cloned().unwrap_or(serde_json::Value::Null),
            "teams": new_state.get("consensus").and_then(|c| c.get("round")).and_then(|r| r.get("teams")).cloned().unwrap_or(serde_json::Value::Null),
            "rounds": [{
                "topic": topic_str,
                "opened_at": new_state.get("consensus").and_then(|c| c.get("round")).and_then(|r| r.get("opened_at")).cloned().unwrap_or(serde_json::Value::Null),
                "submissions": []
            }],
            "_via": "protocol.json"
        }));
    }
    let _ = participants; // not used by Slice 7 thin-wrap
    if action == "close_round" {
        let pd = state.project_dir.clone();
        let actor = my_label.clone();
        let section = get_active_section(&pd);
        let cur_proto = read_protocol_for_section_value(&pd, &section);
        let cur_rev = cur_proto.get("rev").and_then(|v| v.as_u64()).unwrap_or(0);
        // Check if a round is open in protocol.json. If yes, route through
        // protocol_mutate ONLY. If no, fall through to legacy (for sections
        // where the round was opened via delphi/oxford which aren't
        // mirrored into protocol.json yet).
        let phase = cur_proto.get("consensus")
            .and_then(|c| c.get("phase"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if phase == "submitting" || phase == "reviewing" {
            let new_state = do_protocol_mutate(
                &pd,
                &actor,
                &section,
                "close_round",
                serde_json::json!({}),
                Some(cur_rev),
            )?;
            return Ok(serde_json::json!({
                "active": false,
                "closed_at": new_state.get("consensus").and_then(|c| c.get("round")).and_then(|r| r.get("opened_at")).cloned().unwrap_or(serde_json::Value::Null),
                "_via": "protocol.json"
            }));
        }
        // Else: protocol.json had no open round — legacy delphi/oxford
        // state machine handles via match arm below (unchanged path).
    }

    match action {
        "start_discussion" => {
            let mode = mode.ok_or("mode is required for start_discussion")?;
            let topic = topic.ok_or("topic is required for start_discussion")?;

            // Validate mode — only discussion formats, not communication modes
            if !["delphi", "oxford", "red_team", "continuous"].contains(&mode) {
                return Err(format!("Invalid discussion format '{}'. Must be: delphi, oxford, red_team, continuous. (Communication modes 'open'/'directed' are set separately via set_discussion_mode.)", mode));
            }

            // Check no active discussion
            let existing = read_discussion_state(&state.project_dir);
            if existing.get("active").and_then(|v| v.as_bool()).unwrap_or(false) {
                return Err("A discussion is already active. End it first.".to_string());
            }

            // Determine participants: explicit list or all active sessions
            let participant_list = if let Some(p) = participants {
                p
            } else {
                let sessions = read_sessions(&state.project_dir);
                sessions.get("bindings")
                    .and_then(|b| b.as_array())
                    .map(|bindings| {
                        bindings.iter()
                            .filter(|b| {
                                let status = b.get("status").and_then(|s| s.as_str()).unwrap_or("");
                                status == "active" || status == "idle"
                            })
                            .filter_map(|b| {
                                let role = b.get("role").and_then(|r| r.as_str())?;
                                let instance = b.get("instance").and_then(|i| i.as_u64())?;
                                Some(format!("{}:{}", role, instance))
                            })
                            .collect()
                    })
                    .unwrap_or_default()
            };

            let now = utc_now_iso();

            // Continuous mode starts in "reviewing" phase with no rounds —
            // rounds are auto-created when developers post status messages.
            // "reviewing" = ready for next auto-trigger (consistent with post-close phase).
            // Other modes (delphi/oxford/red_team) start with round 1 open.
            let (initial_round, initial_phase, initial_rounds) = if mode == "continuous" {
                (0u64, "reviewing", serde_json::json!([]))
            } else if mode == "delphi" {
                // Delphi starts in "preparing" phase — broadcasts are immediately blocked
                // to prevent context leaking before blind submissions begin.
                // Moderator must call open_next_round to transition to "submitting" (round 1).
                (0u64, "preparing", serde_json::json!([]))
            } else {
                (1u64, "submitting", serde_json::json!([{
                    "number": 1,
                    "opened_at": now,
                    "closed_at": null,
                    "submissions": [],
                    "aggregate_message_id": null
                }]))
            };

            let new_state = serde_json::json!({
                "active": true,
                "mode": mode,
                "topic": topic,
                "started_at": now,
                "moderator": my_label,
                "participants": participant_list,
                "teams": null,
                "current_round": initial_round,
                "phase": initial_phase,
                "paused_at": null,
                "expire_at": null,
                "previous_phase": null,
                "rounds": initial_rounds,
                "settings": {
                    "max_rounds": if mode == "continuous" { 999 } else { 10 },
                    "timeout_minutes": 15,
                    "expire_paused_after_minutes": 60,
                    "auto_close_timeout_seconds": if mode == "continuous" { 60 } else { 0 }
                }
            });

            with_file_lock(&state.project_dir, || {
                write_discussion_state(&state.project_dir, &new_state)?;

                // NOTE: We do NOT update project.json discussion_mode here.
                // Communication mode (directed/open) and discussion format (delphi/oxford)
                // are orthogonal concepts. project.json stores the communication mode.
                // discussion.json stores the discussion format. They operate independently.

                // Post announcement to board
                let msg_id = next_message_id(&state.project_dir);
                let announcement_body = if mode == "continuous" {
                    format!("Continuous Review mode activated.\n\n**Topic:** {}\n**Moderator:** {}\n**Participants:** {}\n\nReview windows open automatically when developers post status updates. Respond with: agree / neutral / disagree: [reason] / alternative: [proposal]. Silence within the timeout = consent.",
                        topic, my_label, participant_list.join(", "))
                } else if mode == "delphi" {
                    format!("A Delphi discussion is being prepared.\n\n**Topic:** {}\n**Moderator:** {}\n**Participants:** {}\n**Phase:** Preparing (broadcasts locked)\n\nAll broadcasts to \"all\" are now blocked to protect blind submission integrity. The moderator will coordinate privately via directed messages, then open Round 1 when ready. Do NOT share reference material publicly.",
                        topic, my_label, participant_list.join(", "))
                } else {
                    format!("A {} discussion has been started.\n\n**Topic:** {}\n**Moderator:** {}\n**Participants:** {}\n**Round:** 1\n\nSubmit your position using type: submission, addressed to the moderator.",
                        mode, topic, my_label, participant_list.join(", "))
                };
                let announcement = serde_json::json!({
                    "id": msg_id,
                    "from": my_label,
                    "to": "all",
                    "type": "moderation",
                    "timestamp": now,
                    "subject": format!("{} discussion started: {}", mode, topic),
                    "body": announcement_body,
                    "metadata": {
                        "discussion_action": "start",
                        "mode": mode,
                        "round": initial_round
                    }
                });
                append_to_board(&state.project_dir, &announcement)?;
                Ok(())
            })?;

            notify_desktop();
            Ok(serde_json::json!({
                "status": "started",
                "mode": mode,
                "topic": topic,
                "phase": initial_phase,
                "round": initial_round,
                "participants": participant_list,
                "moderator": my_label
            }))
        }

        "close_round" => {
            let discussion = read_discussion_state(&state.project_dir);
            if !discussion.get("active").and_then(|v| v.as_bool()).unwrap_or(false) {
                return Err("No active discussion".to_string());
            }
            let phase = discussion.get("phase").and_then(|v| v.as_str()).unwrap_or("");
            if phase != "submitting" {
                return Err(format!("Cannot close round: phase is '{}', expected 'submitting'", phase));
            }

            // Use lightweight tally for continuous mode, full anonymized aggregate for others
            let disc_mode_str = discussion.get("mode").and_then(|v| v.as_str()).unwrap_or("");
            let aggregate_text = if disc_mode_str == "continuous" {
                generate_mini_aggregate(&state.project_dir, &discussion)?
            } else {
                generate_aggregate(&state.project_dir, &discussion)?
            };

            let now = utc_now_iso();
            let round_num = discussion.get("current_round").and_then(|v| v.as_u64()).unwrap_or(1);

            with_file_lock(&state.project_dir, || {
                // Post aggregate as moderation message
                let msg_id = next_message_id(&state.project_dir);
                let aggregate_msg = serde_json::json!({
                    "id": msg_id,
                    "from": my_label,
                    "to": "all",
                    "type": "moderation",
                    "timestamp": now,
                    "subject": format!("Round {} Aggregate", round_num),
                    "body": aggregate_text,
                    "metadata": {
                        "discussion_action": "aggregate",
                        "round": round_num
                    }
                });
                append_to_board(&state.project_dir, &aggregate_msg)?;

                // Update discussion state
                let mut updated = discussion.clone();
                updated["phase"] = serde_json::json!("reviewing");
                if let Some(rounds) = updated.get_mut("rounds").and_then(|r| r.as_array_mut()) {
                    if let Some(last) = rounds.last_mut() {
                        last["closed_at"] = serde_json::json!(now);
                        last["aggregate_message_id"] = serde_json::json!(msg_id);
                    }
                }
                write_discussion_state(&state.project_dir, &updated)?;
                Ok(())
            })?;

            notify_desktop();
            Ok(serde_json::json!({
                "status": "round_closed",
                "round": round_num,
                "phase": "reviewing",
                "aggregate_posted": true
            }))
        }

        "open_next_round" => {
            let discussion = read_discussion_state(&state.project_dir);
            if !discussion.get("active").and_then(|v| v.as_bool()).unwrap_or(false) {
                return Err("No active discussion".to_string());
            }
            let phase = discussion.get("phase").and_then(|v| v.as_str()).unwrap_or("");
            // Accept "reviewing" (normal between-rounds) or "preparing" (Delphi pre-round-1)
            if phase != "reviewing" && phase != "preparing" {
                return Err(format!("Cannot open next round: phase is '{}', expected 'reviewing' or 'preparing'", phase));
            }

            let current = discussion.get("current_round").and_then(|v| v.as_u64()).unwrap_or(0);
            let max_rounds = discussion.get("settings")
                .and_then(|s| s.get("max_rounds"))
                .and_then(|m| m.as_u64())
                .unwrap_or(10);
            let next_round = current + 1;
            if next_round > max_rounds {
                return Err(format!("Max rounds ({}) reached. End the discussion.", max_rounds));
            }

            let now = utc_now_iso();
            let is_first_round = phase == "preparing";

            with_file_lock(&state.project_dir, || {
                let mut updated = discussion.clone();
                updated["current_round"] = serde_json::json!(next_round);
                updated["phase"] = serde_json::json!("submitting");

                if let Some(rounds) = updated.get_mut("rounds").and_then(|r| r.as_array_mut()) {
                    rounds.push(serde_json::json!({
                        "number": next_round,
                        "opened_at": now,
                        "closed_at": null,
                        "submissions": [],
                        "aggregate_message_id": null
                    }));
                }
                write_discussion_state(&state.project_dir, &updated)?;

                // Post round open announcement
                let msg_id = next_message_id(&state.project_dir);
                let body_text = if is_first_round {
                    format!("Round 1 is now open for blind submissions. Submit your position using type: \"submission\" addressed to the moderator. Do NOT share your position publicly — all broadcasts remain blocked.")
                } else {
                    format!("Round {} is now open for submissions. Review the previous aggregate and submit your revised position.", next_round)
                };
                let announcement = serde_json::json!({
                    "id": msg_id,
                    "from": my_label,
                    "to": "all",
                    "type": "moderation",
                    "timestamp": now,
                    "subject": format!("Round {} opened", next_round),
                    "body": body_text,
                    "metadata": {
                        "discussion_action": "open_round",
                        "round": next_round
                    }
                });
                append_to_board(&state.project_dir, &announcement)?;
                Ok(())
            })?;

            notify_desktop();
            Ok(serde_json::json!({
                "status": "round_opened",
                "round": next_round,
                "phase": "submitting"
            }))
        }

        "end_discussion" => {
            let discussion = read_discussion_state(&state.project_dir);
            if !discussion.get("active").and_then(|v| v.as_bool()).unwrap_or(false) {
                return Err("No active discussion to end".to_string());
            }

            let now = utc_now_iso();
            let round_num = discussion.get("current_round").and_then(|v| v.as_u64()).unwrap_or(0);
            let topic = discussion.get("topic").and_then(|t| t.as_str()).unwrap_or("");

            with_file_lock(&state.project_dir, || {
                let mut updated = discussion.clone();
                updated["active"] = serde_json::json!(false);
                updated["phase"] = serde_json::json!("complete");
                write_discussion_state(&state.project_dir, &updated)?;

                // Post end announcement
                let msg_id = next_message_id(&state.project_dir);
                let announcement = serde_json::json!({
                    "id": msg_id,
                    "from": my_label,
                    "to": "all",
                    "type": "moderation",
                    "timestamp": now,
                    "subject": format!("Discussion ended: {}", topic),
                    "body": format!("The discussion on \"{}\" has concluded after {} round(s).", topic, round_num),
                    "metadata": {
                        "discussion_action": "end",
                        "final_round": round_num
                    }
                });
                append_to_board(&state.project_dir, &announcement)?;
                Ok(())
            })?;

            notify_desktop();
            Ok(serde_json::json!({
                "status": "ended",
                "topic": topic,
                "final_round": round_num
            }))
        }

        "set_teams" => {
            let teams_val = teams.ok_or("teams parameter is required for set_teams")?;

            // Validate discussion is active and Oxford
            let disc = read_discussion_state(&state.project_dir);
            if !disc.get("active").and_then(|v| v.as_bool()).unwrap_or(false) {
                return Err("No active discussion. Start one first.".to_string());
            }
            let disc_mode = disc.get("mode").and_then(|v| v.as_str()).unwrap_or("");
            if disc_mode != "oxford" {
                return Err(format!("set_teams is only valid for Oxford debates (current mode: {})", disc_mode));
            }

            // Only the moderator can set teams
            let moderator = disc.get("moderator").and_then(|v| v.as_str()).unwrap_or("");
            if my_label != moderator {
                return Err(format!("Only the moderator ({}) can set teams", moderator));
            }

            // Validate teams structure: must have "for" and "against" arrays
            let team_for = teams_val.get("for").and_then(|v| v.as_array())
                .ok_or("teams must have a 'for' array")?;
            let team_against = teams_val.get("against").and_then(|v| v.as_array())
                .ok_or("teams must have an 'against' array")?;

            // Validate all listed participants exist in the discussion
            let disc_participants: Vec<String> = disc.get("participants")
                .and_then(|p| p.as_array())
                .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
                .unwrap_or_default();

            for member in team_for.iter().chain(team_against.iter()) {
                if let Some(m) = member.as_str() {
                    if !disc_participants.contains(&m.to_string()) {
                        eprintln!("[set_teams] WARNING: {} is not in participants list", m);
                    }
                }
            }

            // Write teams to discussion state
            let mut updated = disc.clone();
            updated["teams"] = teams_val.clone();
            with_file_lock(&state.project_dir, || {
                write_discussion_state(&state.project_dir, &updated)
            })?;

            eprintln!("[set_teams] Teams set: FOR={:?}, AGAINST={:?}", team_for, team_against);

            Ok(serde_json::json!({
                "status": "teams_set",
                "for": team_for,
                "against": team_against
            }))
        }

        "get_state" => {
            let mut discussion = read_discussion_state(&state.project_dir);
            // Strip author-identifying fields from rounds to prevent metadata leak.
            // trigger_from: direct author identity
            // trigger_message_id: indirect leak — client can look up the board message to find its `from` field
            // trigger_subject: probabilistic leak — specific subjects are attributable in small teams
            if let Some(rounds) = discussion.get_mut("rounds").and_then(|r| r.as_array_mut()) {
                for round in rounds.iter_mut() {
                    if let Some(obj) = round.as_object_mut() {
                        obj.remove("trigger_from");
                        obj.remove("trigger_message_id");
                        obj.remove("trigger_subject");
                    }
                }
            }
            Ok(discussion)
        }

        _ => Err(format!("Unknown discussion action: '{}'. Valid: start_discussion, close_round, open_next_round, end_discussion, get_state, set_teams", action))
    }
}

// ==================== Assembly Line ====================
// Minimum mic-control mechanism per human directives (assembly-line thread).
// Two top-level modes: simultaneous (default; free-broadcast) ↔ assembly_line.
// State is section-scoped; mirrors discussion.json layout.
//
// SCOPE BOUNDARIES (v0):
//   - V1-only. project_send_v2 path stays untouched per Ground Rule (don't break v1, don't touch v2).
//   - Gate applies ONLY to handle_project_send. Floor-consuming "speech" tools.
//     project_join (announce), project_buzz (wake), audience_vote (audience event),
//     and the assembly_line tool itself are exempt by design — they are system
//     events, not speech.
//   - No human-only enforcement (MCP is not authenticated). Any seat can call
//     assembly_line(enable|disable). Trust the human via UI, not the wire.
//
// LOCK DISCIPLINE:
//   - assembly state writes happen ONLY inside `with_file_lock` (which holds board.lock).
//     One critical section per send: read assembly → gate → append → advance → release.
//   - assembly.json has NO standalone lock file. Gate-check + append + advance share
//     the existing board.lock acquire — atomic, no TOCTOU, no second lock to order.
//
// FAILURE MODES (deferred to v1 follow-up):
//   - Stuck mic if speaker crashes/AFKs: human toggles off (v0 escape hatch).
//   - All seats die: same.
//   - New seats joining mid-rotation: rotation_order is locked at enable-time and
//     never re-seeded mid-session. To include a freshly-joined seat, the human
//     must disable + re-enable. (No `round` counter exists in v0; the spec's
//     "next round boundary" semantics are deferred along with idle-skip.)

/// Returns the assembly.json path for the active section.
/// "default" section uses flat .vaak/assembly.json; non-default uses .vaak/sections/{slug}/assembly.json.
fn assembly_json_path(project_dir: &str) -> PathBuf {
    let section = get_active_section(project_dir);
    if section == "default" {
        vaak_dir(project_dir).join("assembly.json")
    } else {
        vaak_dir(project_dir).join("sections").join(section).join("assembly.json")
    }
}

/// Read the assembly-state view, projected from protocol.json.
///
/// v1.0.3 migration (dev-challenger msg 414, 2026-05-13): Slice 6 removed the
/// legacy `.vaak/sections/<section>/assembly.json` writer but left this
/// reader pointing at the now-absent file. Three callers — handle_project_join
/// (append-on-join), handle_project_status (acceptance surface from
/// commit 1c26267), handle_project_leave (rule 3a gate from commit e582e6e)
/// — silently no-op'd against the default `{active: false, …}` return for
/// the entire time between Slice 6 closer and 2026-05-13. Migrating the
/// reader to project protocol.json into the legacy shape revives all three
/// callers without touching their call sites.
///
/// The projection: `preset == "Assembly Line"` → `active: true`; floor
/// fields read directly. `started_by` has no protocol.json equivalent
/// (legacy-only field) and is reported `null` — no current reader cares.
fn read_assembly_state(project_dir: &str) -> serde_json::Value {
    let section = get_active_section(project_dir);
    let proto = read_protocol_for_section_value(project_dir, &section);
    let preset = proto.get("preset").and_then(|p| p.as_str()).unwrap_or("");
    let active = preset == PRESET_ASSEMBLY_LINE;
    serde_json::json!({
        "active": active,
        "current_speaker": proto
            .get("floor")
            .and_then(|f| f.get("current_speaker"))
            .cloned()
            .unwrap_or(serde_json::Value::Null),
        "rotation_order": proto
            .get("floor")
            .and_then(|f| f.get("rotation_order"))
            .cloned()
            .unwrap_or(serde_json::json!([])),
        "started_at": proto
            .get("floor")
            .and_then(|f| f.get("started_at"))
            .cloned()
            .unwrap_or(serde_json::Value::Null),
        "started_by": serde_json::Value::Null,
    })
}

/// Write the assembly-state view back into protocol.json. Caller must hold
/// with_file_lock (board.lock). Pre-v1.0.3 this wrote to the legacy
/// assembly.json path which no longer has any readers; the migration
/// targets protocol.json's `floor.current_speaker` and
/// `floor.rotation_order` so the existing handle_project_join append at
/// line ~5709 actually persists.
fn write_assembly_state_unlocked(project_dir: &str, state: &serde_json::Value) -> Result<(), String> {
    let section = get_active_section(project_dir);
    let mut proto = read_protocol_for_section_value(project_dir, &section);

    // Only update fields the caller supplied; leave everything else
    // (rev, consensus, phase_plan, preset, mode, threshold_ms, etc.)
    // untouched so this targeted write doesn't clobber other state.
    if let Some(floor) = proto.get_mut("floor").and_then(|f| f.as_object_mut()) {
        if let Some(cs) = state.get("current_speaker") {
            floor.insert("current_speaker".to_string(), cs.clone());
        }
        if let Some(order) = state.get("rotation_order") {
            floor.insert("rotation_order".to_string(), order.clone());
        }
        if let Some(started) = state.get("started_at") {
            floor.insert("started_at".to_string(), started.clone());
        }
    }

    // Bump rev so any CAS-style reader sees a fresh state and audit-track
    // who/what/when, matching the pattern used by the auto-advance block.
    let cur_rev = proto.get("rev").and_then(|v| v.as_u64()).unwrap_or(0);
    if let Some(rev_field) = proto.get_mut("rev") {
        *rev_field = serde_json::json!(cur_rev + 1);
    }
    if let Some(obj) = proto.as_object_mut() {
        obj.insert(
            "last_writer_action".to_string(),
            serde_json::json!("assembly_state_write"),
        );
        obj.insert(
            "rev_at".to_string(),
            serde_json::json!(utc_now_iso()),
        );
    }

    write_protocol_for_section_value(project_dir, &section, &proto)
}

/// Heartbeat freshness threshold for assembly seat eligibility, in seconds.
/// Spec V3 rule 5: bindings whose last_heartbeat is older than this are treated
/// as zombies and excluded from rotation_order at seed time. Tonight's stuck-mic
/// failure was a status="active" binding whose process had died — its heartbeat
/// stopped updating but no one cleared the binding. Heartbeat freshness is the
/// observable signal that the seat is genuinely reachable.
const ASSEMBLY_SEAT_FRESHNESS_SECS: u64 = 90;

/// Default floor-time advertised in mic-arrival messages (V3 spec rule 4).
/// The watchdog isn't enforced yet — Phase 3 ships it. This constant is what
/// the [YOUR TURN] body tells the new speaker to expect, so when Phase 3
/// lands the advertised floor matches the actual auto-yield boundary.
const ASSEMBLY_FLOOR_DEFAULT_SECS: u64 = 60;

/// V1.5 inaugural pattern-(c) work (dev-challenger msg 441, architect msg 443).
/// Preset names previously appeared as raw string literals at ~25+ sites; a
/// rename, case shift, or i18n shift in any one site would silently drift the
/// others. Centralizing to a single source of truth means a rename becomes
/// one constant edit + N compile errors at all touch points, not a
/// 25-site grep that misses corners.
///
/// Wire strings preserved exactly so existing on-disk protocol.json files
/// continue to parse. Test fixtures intentionally keep raw strings — they
/// exercise the wire-string interface and should fail loudly if a rename
/// breaks that contract.
const PRESET_DEFAULT_CHAT: &str = "Default chat";
const PRESET_DEBATE: &str = "Debate";
const PRESET_ASSEMBLY_LINE: &str = "Assembly Line";
const PRESET_TOWN_HALL: &str = "Town hall";
const PRESET_BRAINSTORM: &str = "Brainstorm";
const PRESET_CONTINUOUS_REVIEW: &str = "Continuous Review";
const PRESET_DELPHI: &str = "Delphi";
const PRESET_OXFORD: &str = "Oxford";

/// List active+idle session seats as "role:instance" strings, in roster order.
/// Used to seed rotation_order on enable and to find the next live speaker.
/// Filters bindings with stale heartbeats (>ASSEMBLY_SEAT_FRESHNESS_SECS) so a
/// dead binding doesn't end up holding the mic — V3 spec rule 5.
fn active_assembly_seats(project_dir: &str) -> Vec<String> {
    let sessions = read_sessions(project_dir);
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    sessions.get("bindings")
        .and_then(|b| b.as_array())
        .map(|bindings| {
            bindings.iter()
                .filter(|b| {
                    let status = b.get("status").and_then(|s| s.as_str()).unwrap_or("");
                    status == "active" || status == "idle"
                })
                .filter(|b| {
                    let hb = b.get("last_heartbeat").and_then(|v| v.as_str()).unwrap_or("");
                    match parse_iso_to_epoch_secs(hb) {
                        Some(hb_secs) => now_secs.saturating_sub(hb_secs) <= ASSEMBLY_SEAT_FRESHNESS_SECS,
                        None => false,
                    }
                })
                .filter_map(|b| {
                    let role = b.get("role").and_then(|r| r.as_str())?;
                    let instance = b.get("instance").and_then(|i| i.as_u64())?;
                    Some(format!("{}:{}", role, instance))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// V3 Phase 2.5 — resolve a yield_to.target to a concrete live seat label.
/// Returns None if the target is "human" (caller falls back to round-robin
/// because humans aren't in rotation_order), if the named role has no
/// fresh-heartbeat instance, or if an explicit "role:N" target is offline.
///
/// Resolution order:
/// - "human" → None (round-robin fallback)
/// - "role:N" → Some(target) if active_assembly_seats contains it; None otherwise
/// - "role" → freshest-heartbeat active instance, ties broken by lowest instance
///
/// Caller (handle_project_send auto-advance) prefers this over
/// next_assembly_speaker so the mic actually lands where the speaker yielded
/// to it — closing the "mic on wrong person" complaint that round-robin alone
/// cannot solve (architect msg 107 gap fix).
fn resolve_yield_target(project_dir: &str, target: &str) -> Option<String> {
    if target.is_empty() || target == "human" {
        return None;
    }
    let active: std::collections::HashSet<String> =
        active_assembly_seats(project_dir).into_iter().collect();
    if target.contains(':') {
        if active.contains(target) {
            return Some(target.to_string());
        }
        return None;
    }
    let sessions = read_sessions(project_dir);
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let mut candidates: Vec<(u64, u64, String)> = sessions
        .get("bindings")
        .and_then(|b| b.as_array())
        .map(|bindings| {
            bindings
                .iter()
                .filter(|b| b.get("role").and_then(|r| r.as_str()) == Some(target))
                .filter(|b| {
                    let st = b.get("status").and_then(|s| s.as_str()).unwrap_or("");
                    st == "active" || st == "idle"
                })
                .filter_map(|b| {
                    let inst = b.get("instance").and_then(|i| i.as_u64())?;
                    let hb = b.get("last_heartbeat").and_then(|v| v.as_str())?;
                    let hb_secs = parse_iso_to_epoch_secs(hb)?;
                    let age = now_secs.saturating_sub(hb_secs);
                    if age > ASSEMBLY_SEAT_FRESHNESS_SECS {
                        return None;
                    }
                    let label = format!("{}:{}", target, inst);
                    if !active.contains(&label) {
                        return None;
                    }
                    Some((age, inst, label))
                })
                .collect()
        })
        .unwrap_or_default();
    // Sort by age (newest heartbeat first), then by instance (lowest first).
    candidates.sort_by_key(|(age, inst, _)| (*age, *inst));
    candidates.into_iter().next().map(|(_, _, label)| label)
}

/// Given the current assembly state and the seat that just sent, return the next
/// speaker in rotation_order — skipping seats that are no longer active/idle.
/// Returns None if no live seat is found (caller leaves current_speaker as-is).
fn next_assembly_speaker(asm: &serde_json::Value, project_dir: &str, just_sent: &str) -> Option<String> {
    let order: Vec<String> = asm.get("rotation_order")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default();
    if order.is_empty() {
        return None;
    }
    let live: std::collections::HashSet<String> = active_assembly_seats(project_dir).into_iter().collect();

    // moderator-authority Item 1+2 (spec line 31-32 + line 38-46):
    // when mic_passing_mode == "moderator" AND a moderator is set, the moderator
    // seat is EXEMPT from rotation — they manage the pipeline rather than
    // participate. Read mic_passing_mode + moderator from current protocol.json
    // (not from `asm` which is a synthesized subset). Same derived semantics
    // as is_seat_exempt() helper.
    let section = get_active_section(project_dir);
    let proto = read_protocol_for_section_value(project_dir, &section);
    let exempt_moderator: Option<String> = {
        let floor = proto.get("floor");
        let mic_mode = floor
            .and_then(|f| f.get("mic_passing_mode"))
            .and_then(|v| v.as_str())
            .unwrap_or("rotation");
        if mic_mode == "moderator" {
            floor
                .and_then(|f| f.get("moderator"))
                .and_then(|v| v.as_str())
                .map(String::from)
        } else {
            None
        }
    };

    // Find the sender's index, then walk forward (with wrap) until we hit a
    // live seat that is NOT the exempt moderator.
    let start = order.iter().position(|s| s == just_sent).unwrap_or(0);
    for offset in 1..=order.len() {
        let candidate = &order[(start + offset) % order.len()];
        if live.contains(candidate) && exempt_moderator.as_deref() != Some(candidate.as_str()) {
            return Some(candidate.clone());
        }
    }
    // No live non-exempt seat anywhere in rotation — degenerate; mic stays with sender.
    None
}

/// Handle the `assembly_line` MCP tool. Auto-advance only — no manual pass_mic
/// (architect ruling: MCP is not authenticated, so a "human-only" override would
/// be a lie. Toggle off is the v0 escape hatch). Actions:
/// - "enable": activate, seed rotation_order from active sessions, current_speaker = order[0]
/// - "disable": deactivate, clear state
/// - "get_state": read-only inspect
///
/// The enable/disable mutation calls into `collab_shared::set_assembly_v0` so the
/// Tauri command path and the MCP path share ONE implementation per #156.
/// Posting the moderation board event stays here because it depends on
/// `next_message_id` and `append_to_board`, which are MCP-binary helpers.
fn handle_assembly_line(action: &str) -> Result<serde_json::Value, String> {
    // Slice 6 deprecation — `assembly_line` is now a PURE thin wrapper
    // (architect #998 + dev-chall #999): the legacy assembly.json write
    // is REMOVED. Spec §3.3 verbatim: "calling into protocol_mutate
    // underneath." The tool name + signature stay live for one release
    // (so old callers don't get tool-not-found), but persistence is
    // entirely protocol.json. Strict removal of the entry point is
    // the release after.
    eprintln!(
        "[deprecated:vision-tail] assembly_line MCP tool — migrate callers to protocol_mutate(set_preset, \"Assembly Line\"|\"Default chat\"). The legacy .vaak/assembly.json write was REMOVED in Slice 6 closer; this tool now writes ONLY to .vaak/protocol.json. Get-state projects from protocol.json. Removal of the entry point is next release."
    );

    let state = get_or_rejoin_state()?;
    let pd = state.project_dir.clone();
    let actor = format!("{}:{}", state.role, state.instance);
    let section = get_active_section(&pd);

    if action == "get_state" {
        // Project protocol.json into the legacy shape — single source of
        // truth, no fallback to .vaak/assembly.json (which we no longer
        // write to).
        let proto = read_protocol_for_section_value(&pd, &section);
        let preset = proto.get("preset").and_then(|p| p.as_str()).unwrap_or("");
        let active = preset == PRESET_ASSEMBLY_LINE;
        return Ok(serde_json::json!({
            "active": active,
            "current_speaker": proto.get("floor").and_then(|f| f.get("current_speaker")).cloned().unwrap_or(serde_json::Value::Null),
            "rotation_order": proto.get("floor").and_then(|f| f.get("rotation_order")).cloned().unwrap_or(serde_json::json!([])),
            "started_at": proto.get("floor").and_then(|f| f.get("started_at")).cloned().unwrap_or(serde_json::Value::Null),
            "_via": "protocol.json"
        }));
    }

    // Pure thin-wrap: route enable/disable through protocol_mutate ONLY
    // (spec §6 matrix: Assembly Line preset = round-robin floor; Default
    // chat = none).
    let preset_name = match action {
        "enable" => PRESET_ASSEMBLY_LINE,
        "disable" => PRESET_DEFAULT_CHAT,
        other => return Err(format!("[InvalidAction] assembly_line action '{}' must be enable|disable|get_state", other)),
    };
    // CAS read current rev from protocol.json.
    let cur_proto = read_protocol_for_section_value(&pd, &section);
    let cur_rev = cur_proto.get("rev").and_then(|v| v.as_u64()).unwrap_or(0);
    let new_state = do_protocol_mutate(
        &pd,
        &actor,
        &section,
        "set_preset",
        serde_json::json!({"name": preset_name}),
        Some(cur_rev),
    )?;
    // Project the new protocol.json state back into the legacy shape so
    // old callers' result-handling code keeps working through the
    // compat tail.
    let new_state = serde_json::json!({
        "active": preset_name == PRESET_ASSEMBLY_LINE,
        "current_speaker": new_state.get("floor").and_then(|f| f.get("current_speaker")).cloned().unwrap_or(serde_json::Value::Null),
        "rotation_order": new_state.get("floor").and_then(|f| f.get("rotation_order")).cloned().unwrap_or(serde_json::json!([])),
        "started_at": new_state.get("floor").and_then(|f| f.get("started_at")).cloned().unwrap_or(serde_json::Value::Null),
        "_via": "protocol.json"
    });

    // Post a system event to the board so sessions in project_wait wake up.
    // Acquire with_file_lock here for the next_message_id + append. Lock ordering:
    // collab_shared::set_assembly_v0 already released board.lock above; this
    // re-acquires it independently. The two acquires are sequential, not nested.
    let _ = with_file_lock(&pd, || {
        let msg_id = next_message_id(&pd);
        let body = match action {
            "enable" => format!("Assembly Line ENABLED by {}. Order: {}. Current speaker: {}.",
                actor,
                new_state.get("rotation_order").and_then(|v| v.as_array())
                    .map(|a| a.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>().join(" → "))
                    .unwrap_or_default(),
                new_state.get("current_speaker").and_then(|v| v.as_str()).unwrap_or("(none)")),
            "disable" => format!("Assembly Line DISABLED by {} — back to simultaneous.", actor),
            _ => String::new(),
        };
        let event = serde_json::json!({
            "id": msg_id,
            "from": actor,
            "to": "all",
            "type": "moderation",
            "timestamp": utc_now_iso(),
            "subject": format!("Assembly Line: {}", action),
            "body": body,
            "metadata": {
                "assembly_action": action,
                "current_speaker": new_state.get("current_speaker").cloned().unwrap_or(serde_json::Value::Null)
            }
        });
        append_to_board(&pd, &event)
    });

    // First-speaker [YOUR TURN] on enable (human msg 327 finding, 2026-05-13).
    // The moderation broadcast above goes to "all" — visible but undirected.
    // Without a directed mic_landed event, the first speaker has no clear
    // signal it's their turn (they have to read the moderation prose and
    // infer). The auto-advance block in project_send already posts a
    // directed mic_landed on every rotation — this brings parity for the
    // very first turn after enable.
    if action == "enable" {
        if let Some(first_speaker) = new_state
            .get("current_speaker")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
        {
            let first_speaker = first_speaker.to_string();
            let _ = with_file_lock(&pd, || {
                let mic_msg_id = next_message_id(&pd);
                let rotation_line = {
                    let order: Vec<String> = new_state
                        .get("rotation_order")
                        .and_then(|v| v.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                                .collect()
                        })
                        .unwrap_or_default();
                    if order.is_empty() {
                        String::new()
                    } else {
                        let parts: Vec<String> = order
                            .iter()
                            .map(|seat| {
                                if seat == &first_speaker {
                                    format!("{}(YOU)", seat)
                                } else {
                                    seat.clone()
                                }
                            })
                            .collect();
                        format!("\nRotation: {}", parts.join(" → "))
                    }
                };
                let mic_event = serde_json::json!({
                    "id": mic_msg_id,
                    "from": "system",
                    "to": first_speaker.clone(),
                    "type": "mic_landed",
                    "timestamp": utc_now_iso(),
                    "subject": format!("[YOUR TURN] {}", first_speaker),
                    "body": format!(
                        "[YOUR TURN] Assembly Line just enabled by {}. Floor: {}s. You are the first speaker.{}",
                        actor, ASSEMBLY_FLOOR_DEFAULT_SECS, rotation_line
                    ),
                    "metadata": {
                        "ask": "Open the assembly — first turn is yours.",
                        "expected_output": "first round contribution",
                        "floor_time_seconds": ASSEMBLY_FLOOR_DEFAULT_SECS,
                        "triggered_by": actor.clone(),
                        "assembly_action": "enable",
                        "rotation": rotation_line.trim_start_matches("\nRotation: "),
                    }
                });
                append_to_board(&pd, &mic_event)
            });
        }
    }

    notify_desktop();
    Ok(new_state)
}

// ============================================================
// Protocol v6 — Slice 2: protocol_mutate + get_protocol MCP tools
// ============================================================
// Per-section unified floor + consensus state. Mirrors the schema in
// desktop/src-tauri/src/protocol.rs (Slice 1, architect 27f4eee).
// vaak-mcp.exe and vaak-desktop are separate binaries with no shared crate;
// both serialize to the same JSON shape and coordinate via OS-level
// board.lock flock (with_file_lock here, with_board_lock in the desktop crate).
//
// Resilience-stack timer registry mirror (canonical: protocol.rs top-of-file):
//   floor.threshold_ms (per-section, default 60_000) — mic freshness gate (spec §2)
//   SUPERVISOR_STALL_SECS = 90                       — vaak-mcp.rs run_supervise
//   PRE_KILL_GRACE_SECS   = 5                        — vaak-mcp.rs run_supervise
//   KEEP_ALIVE_DEBOUNCE_MS ≈ 10_000                  — composer (UI) keystroke
//   MIC_AUTOROTATE_SECS   = 600                      — assembly_line auto-rotate (#903)

const PROTOCOL_DEFAULT_THRESHOLD_MS: u64 = 60_000;
const KEEP_ALIVE_SERVER_DEBOUNCE_MS: u64 = 5_000;

fn protocol_path_for_section(project_dir: &str, section: &str) -> PathBuf {
    if section == "default" {
        Path::new(project_dir).join(".vaak").join("protocol.json")
    } else {
        Path::new(project_dir)
            .join(".vaak")
            .join("sections")
            .join(section)
            .join("protocol.json")
    }
}

fn protocol_fresh_value() -> serde_json::Value {
    serde_json::json!({
        "schema_version": 1,
        "rev": 0,
        "preset": "Debate",
        "floor": {
            "mode": "reactive",
            "current_speaker": null,
            "queue": [],
            "rotation_order": [],
            "threshold_ms": PROTOCOL_DEFAULT_THRESHOLD_MS,
            "started_at": utc_now_iso(),
            // Two-controls v1 fields (spec 2026-05-14, items 1-9):
            "assembly_active": false,        // Control A — coordinated with preset == "Assembly Line"
            "phase": "execution",            // Control B — back-compat default. TODO(consolidated-findings #18): explicit v1.X migration question — back-compat vs forced-accept_plan for new sections.
            "mic_passing_mode": "rotation",  // Only used when assembly_active is true
            "moderator": null,               // seat slug if mic_passing_mode == "moderator"
            "hand_queue": [],                // seat slugs in raise-hand order
            "plan_path": null,               // accepted plan when phase == "execution"
            "plan_hash": null,               // SHA-256 of plan_path file at accept_plan time
            "replanning_requests": [],       // Collaborative-proposal-workflow v1 — multi-writer queue
            "review_intensity": 5            // Strict-turn-discipline slider 1-10; default 5 preserves pre-spec behavior
        },
        "consensus": {
            "mode": "none",
            "round": null,
            "phase": null,
            "submissions": []
        },
        "phase_plan": {
            "phases": [],
            "current_phase_idx": 0,
            "paused_at": null,
            "paused_total_secs": 0
        },
        "scopes": { "floor": "instance", "consensus": "role" },
        "last_writer_seat": null,
        "last_writer_action": null,
        "rev_at": null
    })
}

/// Read protocol.json for a section. Plain fs::read — caller may hold
/// board.lock or not. Missing file or parse error → fresh defaults.
///
/// **NOT migration-aware** — see `protocol_legacy_files_exist` /
/// `vaak_mcp_migrate_legacy_unlocked` for the pre-migrate hook used by the
/// mutate path (evil-arch #939 concern 1: vaak-mcp must not write fresh
/// defaults on top of un-migrated legacy state, leaving legacy dangling
/// forever). For pure reads (get_protocol), missing-file → fresh is fine —
/// no disk write happens.
fn read_protocol_for_section_value(project_dir: &str, section: &str) -> serde_json::Value {
    let path = protocol_path_for_section(project_dir, section);
    match std::fs::read_to_string(&path) {
        Ok(content) => serde_json::from_str::<serde_json::Value>(&content)
            .unwrap_or_else(|e| {
                eprintln!(
                    "[protocol_mcp] {} exists but failed to parse: {}. \
                     Returning fresh defaults; not overwriting on disk.",
                    path.display(), e
                );
                protocol_fresh_value()
            }),
        Err(_) => protocol_fresh_value(),
    }
}

fn legacy_assembly_path_for_section(project_dir: &str, section: &str) -> PathBuf {
    if section == "default" {
        Path::new(project_dir).join(".vaak").join("assembly.json")
    } else {
        Path::new(project_dir).join(".vaak").join("sections").join(section).join("assembly.json")
    }
}

fn legacy_discussion_path_for_section(project_dir: &str, section: &str) -> PathBuf {
    if section == "default" {
        Path::new(project_dir).join(".vaak").join("discussion.json")
    } else {
        Path::new(project_dir).join(".vaak").join("sections").join(section).join("discussion.json")
    }
}

fn legacy_archive_dir(project_dir: &str, section: &str) -> PathBuf {
    Path::new(project_dir).join(".vaak").join("legacy").join(section)
}

fn protocol_legacy_files_exist(project_dir: &str, section: &str) -> bool {
    legacy_assembly_path_for_section(project_dir, section).exists()
        || legacy_discussion_path_for_section(project_dir, section).exists()
}

/// In-process legacy → protocol synthesis (mirror of desktop crate's
/// `synthesize_from_legacy_for_section` — kept in sync with that file's
/// migration semantics). Caller MUST hold board.lock and MUST write
/// protocol.json BEFORE calling `archive_legacy_in_process` (dev-chall
/// #930 ordering rule).
fn synthesize_protocol_from_legacy(project_dir: &str, section: &str) -> (serde_json::Value, bool) {
    let mut p = protocol_fresh_value();
    let mut migrated_anything = false;

    let legacy_assembly = legacy_assembly_path_for_section(project_dir, section);
    let legacy_discussion = legacy_discussion_path_for_section(project_dir, section);

    if let Ok(content) = std::fs::read_to_string(&legacy_assembly) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&content) {
            let active = v.get("active").and_then(|a| a.as_bool()).unwrap_or(false);
            if active {
                if let Some(obj) = p.as_object_mut() {
                    obj.insert("preset".to_string(), serde_json::json!(PRESET_ASSEMBLY_LINE));
                }
                if let Some(floor) = p.get_mut("floor").and_then(|f| f.as_object_mut()) {
                    floor.insert("mode".to_string(), serde_json::json!("round-robin"));
                    if let Some(cs) = v.get("current_speaker").and_then(|s| s.as_str()) {
                        floor.insert("current_speaker".to_string(), serde_json::json!(cs));
                    }
                    if let Some(arr) = v.get("rotation_order").and_then(|r| r.as_array()) {
                        floor.insert("rotation_order".to_string(), serde_json::Value::Array(arr.clone()));
                    }
                    if let Some(sa) = v.get("started_at").and_then(|s| s.as_str()) {
                        floor.insert("started_at".to_string(), serde_json::json!(sa));
                    }
                }
            } else {
                if let Some(obj) = p.as_object_mut() {
                    obj.insert("preset".to_string(), serde_json::json!("Default chat"));
                }
                if let Some(floor) = p.get_mut("floor").and_then(|f| f.as_object_mut()) {
                    floor.insert("mode".to_string(), serde_json::json!("none"));
                }
            }
            migrated_anything = true;
        }
    }

    if let Ok(content) = std::fs::read_to_string(&legacy_discussion) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&content) {
            let mode = v.get("mode").and_then(|m| m.as_str()).unwrap_or("");
            if !mode.is_empty() {
                if let Some(cons) = p.get_mut("consensus").and_then(|c| c.as_object_mut()) {
                    cons.insert("mode".to_string(), serde_json::json!("tally"));
                    if let Some(round) = v.get("round").or_else(|| v.get("current_round")) {
                        let topic = round.get("topic").and_then(|t| t.as_str())
                            .or_else(|| v.get("topic").and_then(|t| t.as_str()));
                        let opened_at = round.get("opened_at").and_then(|t| t.as_str());
                        let opened_by = v.get("moderator").and_then(|t| t.as_str());
                        cons.insert("round".to_string(), serde_json::json!({
                            "topic": topic, "opened_at": opened_at, "opened_by": opened_by
                        }));
                    }
                    if let Some(phase) = v.get("phase").and_then(|s| s.as_str()) {
                        cons.insert("phase".to_string(), serde_json::json!(phase));
                    }
                    if let Some(arr) = v.get("submissions").and_then(|s| s.as_array()) {
                        cons.insert("submissions".to_string(), serde_json::Value::Array(arr.clone()));
                    }
                }
            }
            migrated_anything = true;
        }
    }

    if migrated_anything {
        if let Some(obj) = p.as_object_mut() {
            obj.insert("last_writer_seat".to_string(), serde_json::json!("system:migrate"));
            obj.insert("last_writer_action".to_string(), serde_json::json!("migrate_from_legacy"));
            obj.insert("rev_at".to_string(), serde_json::json!(utc_now_iso()));
        }
    }

    (p, migrated_anything)
}

/// Move legacy files to .vaak/legacy/<section>/. Caller MUST have already
/// written protocol.json and MUST hold board.lock. Errors are logged
/// (tech-leader #931 D — surface or log).
fn archive_legacy_in_process(project_dir: &str, section: &str) {
    let archive_dir = legacy_archive_dir(project_dir, section);
    if let Err(e) = std::fs::create_dir_all(&archive_dir) {
        eprintln!(
            "[protocol_mcp] archive_legacy: create_dir_all({}) failed: {}",
            archive_dir.display(), e
        );
        return;
    }
    for src in [
        legacy_assembly_path_for_section(project_dir, section),
        legacy_discussion_path_for_section(project_dir, section),
    ] {
        if src.exists() {
            let file_name = match src.file_name() {
                Some(n) => n.to_owned(),
                None => continue,
            };
            let dest = archive_dir.join(file_name);
            if let Err(e) = std::fs::rename(&src, &dest) {
                eprintln!(
                    "[protocol_mcp] archive_legacy: rename({} -> {}) failed: {} \
                     (protocol.json already written; legacy file remains for retry)",
                    src.display(), dest.display(), e
                );
            }
        }
    }
}

/// Write protocol.json atomically. Caller MUST hold board.lock (we re-use
/// `with_file_lock` in `handle_protocol_mutate` for this purpose).
fn write_protocol_for_section_value(
    project_dir: &str,
    section: &str,
    value: &serde_json::Value,
) -> Result<(), String> {
    let path = protocol_path_for_section(project_dir, section);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let json = serde_json::to_string_pretty(value)
        .map_err(|e| format!("Failed to serialize protocol: {}", e))?;
    atomic_write(&path, json.as_bytes())
}

/// Spec §2 two-source freshness rule: stuck = both `last_active_at_ms`
/// AND `last_drafting_at_ms` stale past `threshold_ms`. Reads sessions.json
/// directly — heartbeats live at runtime, not in protocol.json (spec §3.1).
/// Missing seat / missing fields → treated as stuck (no presence signal).
fn protocol_is_seat_stuck(project_dir: &str, seat_label: &str, threshold_ms: u64) -> bool {
    let parts: Vec<&str> = seat_label.splitn(2, ':').collect();
    let role = parts.get(0).copied().unwrap_or("");
    let inst: u64 = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
    let sessions = read_sessions(project_dir);
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let bindings = match sessions.get("bindings").and_then(|b| b.as_array()) {
        Some(b) => b,
        None => return true,
    };
    for b in bindings {
        let b_role = b.get("role").and_then(|v| v.as_str()).unwrap_or("");
        let b_inst = b.get("instance").and_then(|v| v.as_u64()).unwrap_or(0);
        if b_role != role || b_inst != inst {
            continue;
        }
        let last_active = b.get("last_active_at_ms").and_then(|v| v.as_u64())
            .or_else(|| b.get("last_heartbeat").and_then(|v| v.as_str())
                .and_then(parse_iso_to_epoch_secs).map(|s| s * 1000))
            .unwrap_or(0);
        let last_drafting = b
            .get("last_drafting_at_ms")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let active_stale = now_ms.saturating_sub(last_active) > threshold_ms;
        let drafting_stale = now_ms.saturating_sub(last_drafting) > threshold_ms;
        return active_stale && drafting_stale;
    }
    true
}

/// Active-seat set sourced from sessions.json. Used by the JSON-Value
/// `protocol_normalize_in_place` mirror of `protocol::Protocol::normalize`
/// per evil-arch #978 + architect #979 ship-block fix (Slice 5 follow-on).
fn protocol_active_seats_set(project_dir: &str) -> std::collections::HashSet<String> {
    let mut set = std::collections::HashSet::new();
    let sessions = read_sessions(project_dir);
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

/// Seed `floor.rotation_order` from `active_seats` when the floor is
/// rotation-driven (`mode == "round-robin"`) AND the existing rotation_order
/// is empty. Also seeds `floor.current_speaker` to `rotation_order[0]` when
/// currently null/empty so the freshly-enabled assembly has a first speaker.
///
/// Idempotent and conservative: never overwrites a non-empty rotation_order
/// (an explicit moderator-set order survives a subsequent set_preset
/// re-invocation), and never touches non-round-robin floors (queue / free-grab
/// / reactive / none use other authorities). Empty `active_seats` → no-op.
///
/// Bug fix — human msg 23, 2026-05-22 ("I turned on assembly line but i dont
/// see structure thats a UI issue"). Root cause: Slice 6 thin-wrapped
/// `handle_assembly_line` through `protocol_mutate(set_preset)` and lost the
/// legacy seed-on-enable behavior; `apply_set_preset` writes preset / mode /
/// consensus but never seeded rotation_order, so the UI rotation strip had
/// nothing to render and `al_auto_grab` ran without a canonical order.
/// Multi-writer / refactor-drift class.
fn seed_rotation_order_if_empty(
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

/// JSON-Value mirror of `protocol::Protocol::normalize` from protocol.rs.
/// Three ratified rules per spec §2.2 + evil-arch #923 + #954:
///   1. floor.mode == "free-grab" → clear floor.queue
///   2. orphan current_speaker (not in active_seats) → clear
///   3. prune dead queue entries
/// Empty `active_seats` → skip rules 2+3 (rule 1 still fires).
fn protocol_normalize_in_place(state: &mut serde_json::Value, active_seats: &std::collections::HashSet<String>) {
    let floor_mode = state.get("floor")
        .and_then(|f| f.get("mode"))
        .and_then(|v| v.as_str())
        .unwrap_or("none")
        .to_string();
    if let Some(floor) = state.get_mut("floor").and_then(|f| f.as_object_mut()) {
        // Rule 1: free-grab dissolves the queue.
        if floor_mode == "free-grab" {
            floor.insert("queue".to_string(), serde_json::Value::Array(vec![]));
        }
        // Rule 2: orphan current_speaker → clear.
        let cs = floor.get("current_speaker").and_then(|v| v.as_str()).map(String::from);
        if let Some(cs_str) = &cs {
            if !active_seats.is_empty() && !active_seats.contains(cs_str) {
                floor.insert("current_speaker".to_string(), serde_json::Value::Null);
            }
        }
        // Rule 3: prune dead queue entries.
        if !active_seats.is_empty() {
            if let Some(arr) = floor.get_mut("queue").and_then(|q| q.as_array_mut()) {
                arr.retain(|v| v.as_str().map(|s| active_seats.contains(s)).unwrap_or(false));
            }
        }
    }
}

fn protocol_seat_exists_active(project_dir: &str, seat_label: &str) -> bool {
    let parts: Vec<&str> = seat_label.splitn(2, ':').collect();
    let role = parts.get(0).copied().unwrap_or("");
    let inst: u64 = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
    let sessions = read_sessions(project_dir);
    if let Some(bindings) = sessions.get("bindings").and_then(|b| b.as_array()) {
        for b in bindings {
            let b_role = b.get("role").and_then(|v| v.as_str()).unwrap_or("");
            let b_inst = b.get("instance").and_then(|v| v.as_u64()).unwrap_or(0);
            let b_status = b.get("status").and_then(|v| v.as_str()).unwrap_or("");
            if b_role == role && b_inst == inst && b_status == "active" {
                return true;
            }
        }
    }
    false
}

/// MCP tool — `get_protocol(section?)`. Returns full protocol.json plus a
/// heartbeat snapshot {seat: {last_active_at_ms, last_drafting_at_ms,
/// connected}}. Heartbeat is JOIN at read time (spec §3.1 perf rule —
/// per-call disk write to protocol.json would be an order-of-magnitude
/// regression vs. today's sessions.json-as-runtime model).
fn handle_get_protocol(section: Option<&str>) -> Result<serde_json::Value, String> {
    let state = get_or_rejoin_state()?;
    let pd = state.project_dir.clone();
    let section_resolved = section
        .map(String::from)
        .unwrap_or_else(|| get_active_section(&pd));

    // Slice 6 auto-advance check: under board.lock, read protocol, evaluate
    // current phase's outcome predicate. If met, advance + bump rev + write.
    // Piggybacks on every UI poll so we don't need a separate scheduler
    // thread — at the cost of one disk write per get_protocol IF at a phase
    // boundary (rare).
    let _ = with_file_lock(&pd, || {
        let mut current = read_protocol_for_section_value(&pd, &section_resolved);
        if auto_advance_if_outcome_met(&mut current, &pd) {
            let cur_rev = current.get("rev").and_then(|v| v.as_u64()).unwrap_or(0);
            if let Some(rev_field) = current.get_mut("rev") {
                *rev_field = serde_json::json!(cur_rev + 1);
            }
            if let Some(obj) = current.as_object_mut() {
                obj.insert("last_writer_seat".to_string(), serde_json::json!("system:auto_advance"));
                obj.insert("last_writer_action".to_string(), serde_json::json!("auto_advance"));
                obj.insert("rev_at".to_string(), serde_json::json!(utc_now_iso()));
            }
            if let Err(e) = write_protocol_for_section_value(&pd, &section_resolved, &current) {
                eprintln!("[protocol_mcp] auto_advance write failed: {} — phase boundary not persisted, will retry on next poll", e);
            }
        }
        Ok(())
    });

    let protocol = read_protocol_for_section_value(&pd, &section_resolved);

    let sessions = read_sessions(&pd);
    let mut snapshot = serde_json::Map::new();
    if let Some(bindings) = sessions.get("bindings").and_then(|b| b.as_array()) {
        for b in bindings {
            let role = b.get("role").and_then(|v| v.as_str()).unwrap_or("");
            let inst = b.get("instance").and_then(|v| v.as_u64()).unwrap_or(0);
            if role.is_empty() {
                continue;
            }
            let label = format!("{}:{}", role, inst);
            let entry = serde_json::json!({
                "last_active_at_ms": b.get("last_active_at_ms").cloned().unwrap_or(serde_json::Value::Null),
                "last_drafting_at_ms": b.get("last_drafting_at_ms").cloned().unwrap_or(serde_json::Value::Null),
                "last_heartbeat": b.get("last_heartbeat").cloned().unwrap_or(serde_json::Value::Null),
                "connected": b.get("status").and_then(|s| s.as_str()).map(|s| s == "active").unwrap_or(false)
            });
            snapshot.insert(label, entry);
        }
    }

    Ok(serde_json::json!({
        "section": section_resolved,
        "protocol": protocol,
        "heartbeats": snapshot
    }))
}

/// MCP tool — `protocol_mutate(action, args?, rev)`. Single dispatch entry for
/// floor + consensus mutations. Acquires board.lock for the entire CAS+apply+
/// write sequence so that no observer sees a state where a mutation half-
/// landed (spec §10 atomicity rule).
///
/// Error envelope (per dev #927 vote 3): error string is prefixed with
/// `[Code]` so callers can branch:
///   [StaleRev]            CAS failure (`current.rev` ≠ `rev_in`)
///   [MissingRev]          rev field omitted (silent CAS bypass forbidden)
///   [InvalidAction]       action name not in dispatch table
///   [InvalidArgs]         message
///   [NotPermitted]        auth gate or self/other constraint
///   [SeatNotFound]        target not in active sessions.json
///   [StuckGateNotPassed]  current speaker is fresh; `transfer_mic` blocked
///   [Slice5Unimplemented] phase action stub
///   [Slice6Unimplemented] consensus action stub
fn handle_protocol_mutate(
    action: &str,
    args: serde_json::Value,
    rev_in: Option<u64>,
) -> Result<serde_json::Value, String> {
    let state = get_or_rejoin_state()?;
    let pd = state.project_dir.clone();
    let actor = format!("{}:{}", state.role, state.instance);
    let section = get_active_section(&pd);
    // Phase 2 (c) — reinstate_agent is special-cased OUTSIDE do_protocol_mutate:
    // it resets currency balance (needs the currency lock OUTER) + re-adds the
    // seat to rotation_order (board lock INNER). do_protocol_mutate only holds
    // the board lock, so nesting currency inside it would violate the
    // currency-outer ordering. Human-only; no CAS (rare, no race).
    if action == "reinstate_agent" {
        return apply_reinstate_agent(&pd, &actor, &section, &args);
    }
    do_protocol_mutate(&pd, &actor, &section, action, args, rev_in)
}

/// Phase 2 (c) — human reinstates a timed-out (or any) seat. Sets balance to 0
/// (NOT 10000 — they restart from nothing per directive), clears timed_out +
/// escrow + system-dispute ban, re-adds to rotation_order if assembly is active.
fn apply_reinstate_agent(pd: &str, actor: &str, section: &str, args: &serde_json::Value) -> Result<serde_json::Value, String> {
    use collab::currency::*;
    if !actor.starts_with("human:") {
        return Err("[Reinstate] human-only action — only the human can reinstate a seat.".to_string());
    }
    let seat = args.get("seat").and_then(|v| v.as_str())
        .ok_or("[Reinstate] requires args.seat (\"role:N\")")?.to_string();
    collab::with_currency_and_board_lock(pd, || {
        let mut snap = read_balances_snapshot(pd)?;
        if snap.seats.is_empty() && currency_jsonl_path(pd).exists() { snap = replay_balances_from_ledger(pd)?; }
        {
            let e = snap.seats.entry(seat.clone()).or_default();
            e.balance = 0;
            e.timed_out = false;
            e.escrow_items.clear();
            e.escrow_held = 0;
            e.system_dispute_ban_until = None;
        }
        let now = collab::iso_now();
        let id = snap.next_txn_id; snap.next_txn_id += 1;
        append_currency_transaction(pd, &LedgerRow {
            id, txn_type: "reinstate".to_string(), seat: seat.clone(), amount: 0,
            reason: format!("reinstated by {} — balance reset to 0", actor),
            ref_msg: None, balance_after: 0, escrow_id: None, release_turn: None,
            turn: Some(snap.turn_counter), action_kind: None, linked_edit_msg: None, at: now.clone(),
        })?;
        write_balances_snapshot(pd, &snap)?;

        // Re-add to rotation_order if assembly is active (best-effort; protocol
        // read/write under the board lock that's the inner half of this lock).
        let mut proto = read_protocol_for_section_value(pd, section);
        let asm_active = proto.get("preset").and_then(|p| p.as_str()) == Some(PRESET_ASSEMBLY_LINE);
        if asm_active {
            if let Some(floor) = proto.get_mut("floor").and_then(|f| f.as_object_mut()) {
                let ro = floor.entry("rotation_order".to_string()).or_insert_with(|| serde_json::json!([]));
                if let Some(arr) = ro.as_array_mut() {
                    if !arr.iter().any(|v| v.as_str() == Some(seat.as_str())) {
                        arr.push(serde_json::json!(seat));
                    }
                }
            }
            let cur_rev = proto.get("rev").and_then(|v| v.as_u64()).unwrap_or(0);
            if let Some(obj) = proto.as_object_mut() {
                obj.insert("rev".to_string(), serde_json::json!(cur_rev + 1));
                obj.insert("last_writer_seat".to_string(), serde_json::json!(actor));
                obj.insert("last_writer_action".to_string(), serde_json::json!("reinstate_agent"));
                obj.insert("rev_at".to_string(), serde_json::json!(utc_now_iso()));
            }
            let _ = write_protocol_for_section_value(pd, section, &proto);
        }

        let msg_id = next_message_id(pd);
        let _ = append_to_board(pd, &serde_json::json!({
            "id": msg_id, "from": "system", "to": "all", "type": "broadcast",
            "timestamp": utc_now_iso(),
            "subject": format!("[Reinstated] {} reinstated by {}", seat, actor),
            "body": format!("[Reinstated] {} reinstated by {}. Balance reset to 0 (fresh start, not 10000); timed-out + escrow + system-dispute ban cleared.{}",
                seat, actor, if asm_active { " Re-added to rotation." } else { "" }),
            "metadata": { "reinstated_seat": seat, "by": actor }
        }));
        Ok(serde_json::json!({ "reinstated": seat, "balance": 0, "by": actor }))
    })
}

/// Inner of `handle_protocol_mutate` — pure-input version used by tests
/// (project_dir, actor, section all explicit, no `get_or_rejoin_state`
/// dependency). Closes the CAS-gate behavioral coverage gap that
/// tech-leader #941.5 + dev-chall #940.4 flagged before Slice 3 forks.
fn do_protocol_mutate(
    pd: &str,
    actor: &str,
    section: &str,
    action: &str,
    args: serde_json::Value,
    rev_in: Option<u64>,
) -> Result<serde_json::Value, String> {
    // keep_alive bypasses CAS + protocol.rev bump — it writes to sessions.json
    // (spec §3.1 perf rule, dev #920). Every non-keep_alive action goes
    // through the rev gate.
    if action == "keep_alive" {
        return apply_keep_alive(pd, actor);
    }

    let rev_in =
        rev_in.ok_or("[MissingRev] rev field is required for protocol_mutate (silent CAS bypass forbidden — dev #927 vote 3)")?;

    let result: Result<Result<serde_json::Value, String>, String> =
        with_file_lock(pd, || {
            // Pre-migration check (evil-arch #939 concern 1): if protocol.json
            // is missing AND legacy files exist, vaak-mcp must NOT write fresh
            // defaults — that would leave legacy dangling on disk forever.
            // Synthesize from legacy under our lock, write protocol.json FIRST
            // (dev-chall #930 ordering), then archive legacy. Mirrors the
            // desktop crate's `read_protocol_for_section` migration path.
            let path = protocol_path_for_section(pd, section);
            if !path.exists() && protocol_legacy_files_exist(pd, section) {
                let (synth, did_migrate) = synthesize_protocol_from_legacy(pd, section);
                if let Err(e) = write_protocol_for_section_value(pd, section, &synth) {
                    return Ok(Err(format!(
                        "[InternalError] pre-migration synth write failed: {}",
                        e
                    )));
                }
                if did_migrate {
                    archive_legacy_in_process(pd, section);
                }
                // Migration produced rev=0; if caller passed rev=0 they can
                // proceed against the synthesized state. Otherwise it's a
                // stale-rev error (caller's expectation didn't match).
            }

            let mut current = read_protocol_for_section_value(pd, section);
            let current_rev = current.get("rev").and_then(|v| v.as_u64()).unwrap_or(0);
            if rev_in != current_rev {
                return Ok(Err(format!(
                    "[StaleRev] expected rev {} (caller passed), current rev is {}",
                    rev_in, current_rev
                )));
            }

            // Capture pre-state for two-controls event-emission (finding #3
            // observability invariant). Cheap clone before apply_* mutates.
            let pre_state = current.clone();

            let dispatch_result = match action {
                "set_preset" => apply_set_preset(&mut current, &args),
                "transfer_mic" => apply_transfer_mic(&mut current, &args, actor, pd),
                "yield" => apply_yield(&mut current, &args, actor),
                "force_release" => apply_force_release(&mut current, actor, pd),
                "toggle_queue" => apply_toggle_queue(&mut current, &args, actor),
                "mic_claim" => apply_mic_claim(&mut current, &args, actor),
                "set_phase_plan" => apply_set_phase_plan(&mut current, &args),
                "advance_phase" => apply_advance_phase(&mut current, pd),
                "pause_plan" => apply_pause_plan(&mut current),
                "resume_plan" => apply_resume_plan(&mut current),
                "extend_phase" => apply_extend_phase(&mut current, &args),
                "open_round" => apply_open_round(&mut current, &args, actor),
                "submit" => apply_submit(&mut current, &args, actor),
                "close_round" => apply_close_round(&mut current, &args, actor),
                // Two-controls v1 (spec 2026-05-14, items 1-9):
                "set_assembly" => apply_set_assembly(&mut current, &args),
                "accept_plan" => apply_accept_plan(&mut current, &args, actor, pd),
                "open_planning" => apply_open_planning(&mut current, actor),
                "revise_plan" => apply_revise_plan(&mut current, &args, actor, pd),
                "set_mic_passing" => match apply_set_mic_passing(&mut current, &args) {
                    Ok(MutateOutcome::Applied) => Ok(()),
                    Ok(MutateOutcome::NoOp) => {
                        // Defer-silent / idempotent — skip rev bump + emit
                        // (tester msg 1111 T-FINDING). Return early via a
                        // pre-applied current state with no mutations carried.
                        return Ok(Ok(pre_state));
                    }
                    Err(e) => Err(e),
                },
                "raise_hand" => apply_raise_hand(&mut current, actor),
                "grant_mic" => apply_grant_mic(&mut current, &args, actor),
                "set_moderator" => apply_set_moderator(&mut current, &args, actor),
                // Fix-A2 — typed reorder-only mutation per
                // .vaak/design-notes/fix-a2-set-rotation-order-spec-2026-05-22.md.
                // Pre-normalize ordering doesn't matter for this arm since the
                // mutation produces a permutation of active_seats — normalize's
                // rule #2 (orphan current_speaker) can never trip.
                "set_rotation_order" => {
                    apply_set_rotation_order(&mut current, &args, actor, pd)
                }
                // Collaborative-proposal-workflow v1 (spec 2026-05-15, Commit P + P.B).
                // ts injection: production caller passes None per spec v6 line 29;
                // tests call apply_propose_replanning directly with Some(...).
                "propose_replanning" => {
                    let reason = args.get("reason").and_then(|v| v.as_str());
                    match reason {
                        Some(r) => apply_propose_replanning(&mut current, actor, r, None),
                        None => Err("[InvalidArgs] propose_replanning requires args.reason (string)".to_string()),
                    }
                }
                // Commit Q — accept_replanning. Role gate per spec §accept_replanning.
                // request_index is optional; out-of-bounds rejects inside apply.
                "accept_replanning" => apply_accept_replanning(&mut current, &args, actor),
                // Commit S — strict-turn-discipline review-intensity slider.
                // Role gate (moderator/architect/manager/human) + range 1-10.
                "set_review_intensity" => apply_set_review_intensity(&mut current, &args, actor),
                other => Err(format!("[InvalidAction] no such action: {}", other)),
            };

            if let Err(e) = dispatch_result {
                return Ok(Err(e));
            }

            // Normalize after apply (evil-arch #978 + architect #979 ship-
            // block fix): three ratified rules of spec §2.2 only fire if
            // we invoke them here. Rules:
            //   1. free-grab → clear queue (Brainstorm/Continuous Review preset
            //      transitions need this, otherwise Town Hall queue persists)
            //   2. orphan speaker (not in active_seats) → clear current_speaker
            //   3. dead seats in queue → prune
            // active_seats sourced from sessions.json under our lock so the
            // pruning is consistent with whatever the supervisor saw last.
            let active_seats = protocol_active_seats_set(pd);
            // Post-set_preset seeding (bug fix, human msg 23 on 2026-05-22:
            // "I turned on assembly line but i dont see structure thats a UI
            // issue"). The Slice 6 deprecation thin-wrapped handle_assembly_line
            // through set_preset and lost the legacy seed-on-enable behavior —
            // apply_set_preset writes preset/mode/consensus but never seeds
            // floor.rotation_order, so the UI rotation strip has nothing to
            // render. Restore the contract here, BEFORE normalize so the freshly
            // seeded current_speaker isn't orphan-cleared on the same call.
            // Multi-writer/refactor-drift class.
            //
            // set_assembly is included because apply_set_assembly internally
            // calls apply_set_preset (vaak-mcp.rs:4941) and would otherwise
            // hit the same un-seeded rotation_order outcome — found during
            // Fix-A1 audit, human msg 88. Any future arm that wraps
            // apply_set_preset must be added here (or, better, the seed
            // helper should move into apply_set_preset's body via a project_dir
            // parameter — deferred to Fix-A2's typed-action refactor).
            if action == "set_preset" || action == "set_assembly" {
                seed_rotation_order_if_empty(&mut current, &active_seats);
            }
            protocol_normalize_in_place(&mut current, &active_seats);

            // CAS bump + audit — only after successful apply + normalize.
            if let Some(rev_field) = current.get_mut("rev") {
                *rev_field = serde_json::json!(current_rev + 1);
            }
            if let Some(obj) = current.as_object_mut() {
                obj.insert("last_writer_seat".to_string(), serde_json::json!(actor.to_string()));
                obj.insert("last_writer_action".to_string(), serde_json::json!(action));
                obj.insert("rev_at".to_string(), serde_json::json!(utc_now_iso()));
            }

            if let Err(e) = write_protocol_for_section_value(pd, section, &current) {
                return Ok(Err(e));
            }

            // Two-controls observability events (finding #3, A12). Inside the
            // lock so observers see protocol.json + board event consistently.
            emit_two_controls_event(pd, actor, action, &args, &pre_state, &current);

            Ok(Ok(current))
        });

    // Unwrap nested Result: outer Err = lock failure; inner Err = mutation failure.
    match result {
        Ok(inner) => inner,
        Err(e) => Err(e),
    }
}

fn apply_set_preset(
    state: &mut serde_json::Value,
    args: &serde_json::Value,
) -> Result<(), String> {
    let name = args
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or("[InvalidArgs] set_preset requires args.name (string)")?;
    // V3 spec rule 10: assembly mode and discussion presets are mutually
    // exclusive. Reject any transition INTO Assembly Line from a discussion
    // preset (and vice versa) instead of arbitrating precedence at runtime —
    // that's the class of bug we already lived through with two systems
    // owning the same state. The state.preset check uses the pre-mutation
    // value (we haven't written the new preset yet).
    let prev_preset = state
        .get("preset")
        .and_then(|p| p.as_str())
        .unwrap_or("");
    let is_discussion =
        |p: &str| matches!(p, PRESET_CONTINUOUS_REVIEW | PRESET_DELPHI | PRESET_OXFORD);
    if name == PRESET_ASSEMBLY_LINE && is_discussion(prev_preset) {
        return Err(format!(
            "[ConflictWithDiscussion] cannot set preset to '{}' while a discussion preset ('{}') is active. \
             Set preset to '{}' first, then enable {}.",
            PRESET_ASSEMBLY_LINE, prev_preset, PRESET_DEFAULT_CHAT, PRESET_ASSEMBLY_LINE
        ));
    }
    if is_discussion(name) && prev_preset == PRESET_ASSEMBLY_LINE {
        return Err(format!(
            "[ConflictWithAssembly] cannot set preset to '{}' while Assembly Line is active. \
             Disable Assembly Line first (set preset to 'Default chat'), then start the discussion.",
            name
        ));
    }

    // V1.0.7 (Instance 4 interim gate, dev-challenger msg 619, architect msg 622):
    // multi-writer audit's preset+floor.mode coordination class has no typed
    // enforcement yet (pattern-(c) Preset enum is the v1.5.0 work in flight).
    // Until that lands, reject ALL non-Default-chat cross-transitions: a
    // preset change is allowed only if it goes THROUGH Default chat in
    // either direction. Closes the live risk that an active assembly or
    // discussion gets silently overwritten by another non-Default mode while
    // floor.mode + rotation_order + discussion state drift independently.
    //
    // Coarse — superset of the existing Assembly Line ↔ discussion mutex
    // above (which catches the most dangerous pairs explicitly). When
    // v1.5.0 typed enforcement ships, this gate becomes redundant and can
    // be removed.
    if prev_preset != PRESET_DEFAULT_CHAT
        && name != PRESET_DEFAULT_CHAT
        && prev_preset != name
    {
        return Err(format!(
            "[ConflictWithActivePreset] cannot transition preset directly from '{}' to '{}' — \
             route through '{}' first to avoid floor.mode + rotation_order drift while the prior \
             mode's state is still live. v1.0.7 interim gate per multi-writer audit Instance 4.",
            prev_preset, name, PRESET_DEFAULT_CHAT
        ));
    }

    // V1.5.0 commit 2/6 — migrate apply_set_preset matrix to the typed
    // Preset enum (commit 1: 1cd488d). Wire string → Preset variant via
    // serde; matrix dispatch via exhaustive match on the variant. Compiler
    // now guarantees every variant is handled at this site; the wildcard
    // arm is reachable only when the JSON wire is unrecognized (strict
    // failure mode per dev-challenger Finding 2).
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
    // Sync the two-controls v1 derived fields when crossing the assembly
    // boundary (bug fix — human msg 261 on 2026-05-22: "you turned off
    // assembly mode but i still see it as on... If assembly mode is not on
    // why would review intesnity be relevant"). Tech-leader msg 244 had
    // called protocol_mutate(set_preset, "Default chat") directly — that
    // wrote preset/mode/consensus but left floor.assembly_active=true,
    // floor.current_speaker, and floor.moderator pointing at the prior
    // assembly state. UI renders off assembly_active and showed AL as
    // still on. Same shape as the rotation_order seed bug Fix-B1 closed:
    // apply_set_preset historically wrote a partial subset of derived
    // fields. apply_set_assembly already does this sync — preserve that
    // contract here so the direct set_preset path is symmetric.
    //
    // Refactor-drift class: any caller wrapping apply_set_preset (e.g.
    // apply_set_assembly at line 4949 + the dispatcher arm) now gets
    // consistent floor state without duplicating sync code.
    let is_assembly_preset = matches!(preset, Preset::AssemblyLine);
    if let Some(floor) = state.get_mut("floor").and_then(|f| f.as_object_mut()) {
        floor.insert("mode".to_string(), serde_json::json!(floor_mode));
        floor.insert(
            "assembly_active".to_string(),
            serde_json::json!(is_assembly_preset),
        );
        if !is_assembly_preset {
            // Leaving the assembly family — clear stale speaker + moderator
            // so the UI doesn't render mic-holder / ★ on cards whose
            // authority just evaporated. Rotation_order is preserved (it's
            // just data; a future enable can reseed via the dispatcher
            // hook, and an explicit set_rotation_order can pre-populate
            // before re-enable).
            floor.insert(
                "current_speaker".to_string(),
                serde_json::Value::Null,
            );
            floor.insert("moderator".to_string(), serde_json::Value::Null);
        }
    }
    if let Some(cons) = state.get_mut("consensus").and_then(|c| c.as_object_mut()) {
        cons.insert("mode".to_string(), serde_json::json!(consensus_mode));
    }
    Ok(())
}

/// transfer_mic — caller MUST NOT equal current_speaker; freshness gate must
/// pass (current speaker is STUCK) UNLESS current_speaker is None (cold-open
/// IDLE). Per spec §2 + §10 + dev-chall #924 §2.1 lock.
fn apply_transfer_mic(
    state: &mut serde_json::Value,
    args: &serde_json::Value,
    actor: &str,
    project_dir: &str,
) -> Result<(), String> {
    let target = args
        .get("target")
        .and_then(|v| v.as_str())
        .ok_or("[InvalidArgs] transfer_mic requires args.target (seat label like 'role:0')")?
        .to_string();
    let current_speaker = state
        .get("floor")
        .and_then(|f| f.get("current_speaker"))
        .and_then(|v| v.as_str())
        .map(String::from);
    let threshold_ms = state
        .get("floor")
        .and_then(|f| f.get("threshold_ms"))
        .and_then(|v| v.as_u64())
        .unwrap_or(PROTOCOL_DEFAULT_THRESHOLD_MS);

    if current_speaker.as_deref() == Some(actor) {
        return Err("[NotPermitted] caller IS current_speaker — use yield, not transfer_mic".to_string());
    }
    if let Some(cs) = &current_speaker {
        if !protocol_is_seat_stuck(project_dir, cs, threshold_ms) {
            return Err(format!(
                "[StuckGateNotPassed] current speaker '{}' is fresh (within {}ms freshness threshold). Wait for STUCK or address them via metadata.mic_to in a message.",
                cs, threshold_ms
            ));
        }
    }
    if !protocol_seat_exists_active(project_dir, &target) {
        return Err(format!(
            "[SeatNotFound] target '{}' not in active sessions.json bindings",
            target
        ));
    }

    if let Some(floor) = state.get_mut("floor").and_then(|f| f.as_object_mut()) {
        floor.insert("current_speaker".to_string(), serde_json::json!(target));
        if let Some(queue) = floor.get_mut("queue").and_then(|q| q.as_array_mut()) {
            queue.retain(|v| v.as_str() != Some(&target));
        }
    }
    Ok(())
}

/// yield — caller MUST equal current_speaker. Optional `args.target` hands
/// directly to a specific seat (skipping queue head). Default: pop queue head.
/// If queue empty and no target → current_speaker becomes None (IDLE).
fn apply_yield(
    state: &mut serde_json::Value,
    args: &serde_json::Value,
    actor: &str,
) -> Result<(), String> {
    let current_speaker = state
        .get("floor")
        .and_then(|f| f.get("current_speaker"))
        .and_then(|v| v.as_str())
        .map(String::from);
    if current_speaker.as_deref() != Some(actor) {
        return Err(format!(
            "[NotPermitted] caller '{}' is not current_speaker (current: {:?}) — only the speaker may yield",
            actor, current_speaker
        ));
    }
    let target = args.get("target").and_then(|v| v.as_str()).map(String::from);
    let new_speaker: Option<String> = match target {
        Some(t) => Some(t),
        None => state
            .get("floor")
            .and_then(|f| f.get("queue"))
            .and_then(|q| q.as_array())
            .and_then(|arr| arr.first())
            .and_then(|v| v.as_str())
            .map(String::from),
    };
    if let Some(floor) = state.get_mut("floor").and_then(|f| f.as_object_mut()) {
        floor.insert(
            "current_speaker".to_string(),
            new_speaker
                .as_ref()
                .map(|s| serde_json::json!(s))
                .unwrap_or(serde_json::Value::Null),
        );
        if let Some(target_seat) = &new_speaker {
            if let Some(queue) = floor.get_mut("queue").and_then(|q| q.as_array_mut()) {
                queue.retain(|v| v.as_str() != Some(target_seat));
            }
        }
    }
    Ok(())
}

/// force_release — human-only (or system) action that clears current_speaker
/// without the freshness gate that transfer_mic enforces. Posts a mic_released
/// board event with from="human" so the action is visible to the team and to
/// the human's later self. Visibility is the safety mechanism — confirmations
/// train dismissal (evil-arch msg 171), audit events deter reckless use.
///
/// Caller must be "human" — agents cannot force-release each other; they yield
/// or wait for the watchdog. Returns Err if invoked by an agent.
fn apply_force_release(
    state: &mut serde_json::Value,
    actor: &str,
    project_dir: &str,
) -> Result<(), String> {
    if actor != "human" {
        return Err(format!(
            "[NotPermitted] force_release is human-only (caller: '{}'). Agents must yield or wait for the watchdog.",
            actor
        ));
    }
    let prior_speaker = state
        .get("floor")
        .and_then(|f| f.get("current_speaker"))
        .and_then(|v| v.as_str())
        .map(String::from);
    if prior_speaker.is_none() {
        return Err("[NoOp] force_release called but current_speaker is already null".to_string());
    }
    if let Some(floor) = state.get_mut("floor").and_then(|f| f.as_object_mut()) {
        floor.insert("current_speaker".to_string(), serde_json::Value::Null);
    }

    // Audit event: append mic_released to board.jsonl so the team sees who
    // got yanked and by whom. Best-effort — failure logs but the release
    // still applies (the floor mutation is the load-bearing change).
    // Includes idle_secs from sessions.json:last_working_at so observers can
    // distinguish "kicked an actively-working speaker (suspicious)" from
    // "kicked a long-stalled speaker (just routine cleanup)" — evil-arch
    // msg 180 #3.
    let prior = prior_speaker.unwrap_or_default();
    let now = utc_now_iso();
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let idle_secs: i64 = {
        let sessions = read_sessions(project_dir);
        let mut parts = prior.splitn(2, ':');
        let pr_role = parts.next().unwrap_or("");
        let pr_inst: u64 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
        sessions
            .get("bindings")
            .and_then(|b| b.as_array())
            .and_then(|arr| arr.iter().find(|b| {
                b.get("role").and_then(|r| r.as_str()) == Some(pr_role)
                    && b.get("instance").and_then(|i| i.as_u64()).unwrap_or(0) == pr_inst
            }))
            .and_then(|b| b.get("last_working_at").and_then(|v| v.as_str()))
            .and_then(parse_iso_to_epoch_secs)
            .map(|w| now_secs.saturating_sub(w) as i64)
            .unwrap_or(-1)
    };
    let board_path = board_jsonl_path(project_dir);
    let count = std::fs::read_to_string(&board_path)
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .count();
    let event = serde_json::json!({
        "id": (count + 1) as u64,
        "from": "human",
        "to": "all",
        "type": "mic_released",
        "timestamp": now,
        "subject": format!("[mic_released] {} — human_force_release", prior),
        "body": format!(
            "Human force-released the mic from {} (idle: {}s). Mic is now free — next sender will auto-grab.",
            prior, idle_secs
        ),
        "metadata": {
            "from_speaker": prior,
            "reason": "human_force_release",
            "idle_secs_at_release": idle_secs,
        }
    });
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&board_path) {
        use std::io::Write;
        let _ = writeln!(f, "{}", serde_json::to_string(&event).unwrap_or_default());
    }
    Ok(())
}

/// v1.5.1 commit 2: mic_claim — caller declares their turn semantics on
/// the floor. Writes `turn_type` (working|reviewing|passing|thinking) and
/// `expected_duration_secs` (30-600, hard-capped per evil-arch msg 875) to
/// `floor`. Caller MUST be current_speaker. The watchdog at main.rs:4665
/// reads `expected_duration_secs` to compute the dynamic stall threshold
/// instead of the hard-coded 120s (v1.5.1 change #2). Unknown turn_type
/// strings reject strictly per v1.5.0 Preset enum precedent.
fn apply_mic_claim(
    state: &mut serde_json::Value,
    args: &serde_json::Value,
    actor: &str,
) -> Result<(), String> {
    let current_speaker = state
        .get("floor")
        .and_then(|f| f.get("current_speaker"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if current_speaker != actor {
        return Err(format!(
            "[NotSpeaker] mic_claim caller '{}' is not current_speaker '{}'. Only the active speaker can declare turn semantics.",
            actor, current_speaker
        ));
    }
    let turn_type = args
        .get("turn_type")
        .and_then(|v| v.as_str())
        .unwrap_or("working");
    if !matches!(turn_type, "working" | "reviewing" | "passing" | "thinking") {
        return Err(format!(
            "[UnknownTurnType] turn_type='{}' not in working|reviewing|passing|thinking. Strict deserialization per v1.5.0 Preset enum precedent.",
            turn_type
        ));
    }
    // Commit M — v1.1 §A2 planning_blocks_working gate per architect msg 2020
    // + tester msg 2011 R5.B investigation. v1.1 spec promised this but
    // implementation never delivered; collab-proposal-workflow-spec-2026-
    // 05-15.md §W2 (line 196) restates the rule. Discussion turn-types
    // (reviewing/passing/thinking) stay allowed in planning — planning IS
    // the discussion phase. Only `working` (code-writing posture) is gated.
    let phase = state
        .get("floor")
        .and_then(|f| f.get("phase"))
        .and_then(|v| v.as_str())
        .unwrap_or("execution");
    if turn_type == "working" && phase == "planning" {
        return Err(format!(
            "[planning_blocks_working] cannot claim turn_type='working' during planning phase (current phase: '{}'). Planning-phase contributions should claim reviewing/thinking/passing instead.",
            phase
        ));
    }
    let expected_duration_secs = args
        .get("expected_duration_secs")
        .and_then(|v| v.as_u64())
        .unwrap_or(match turn_type {
            // Defaults set inside the 30-600 bounds so a no-arg claim
            // of any turn_type succeeds. Architect msg 898 sets working
            // default to 300 (between routine review at 120s and the
            // 600s cap) — forces explicit declaration for genuinely
            // long working turns rather than silently using the ceiling.
            "working" => 300,
            "reviewing" => 120,
            "passing" => 30,
            "thinking" => 300,
            _ => 120,
        });
    if !(30..=600).contains(&expected_duration_secs) {
        return Err(format!(
            "[ClaimOutOfBounds] expected_duration_secs={} out of range [30, 600]. Hard-cap per evil-architect msg 875 to close dodge vector.",
            expected_duration_secs
        ));
    }
    if let Some(floor) = state.get_mut("floor").and_then(|f| f.as_object_mut()) {
        floor.insert("turn_type".to_string(), serde_json::json!(turn_type));
        floor.insert(
            "expected_duration_secs".to_string(),
            serde_json::json!(expected_duration_secs),
        );
        floor.insert("claimed_at".to_string(), serde_json::json!(utc_now_iso()));
    }
    Ok(())
}

/// toggle_queue — self-only (caller toggles their OWN seat in/out of the
/// queue). `args.seat` MAY be omitted (defaults to caller); if provided, must
/// equal caller. Per spec §10 auth gate (Self-floor tier).
fn apply_toggle_queue(
    state: &mut serde_json::Value,
    args: &serde_json::Value,
    actor: &str,
) -> Result<(), String> {
    let seat = args
        .get("seat")
        .and_then(|v| v.as_str())
        .map(String::from)
        .unwrap_or_else(|| actor.to_string());
    if seat != actor {
        return Err(format!(
            "[NotPermitted] toggle_queue is self-only; caller '{}' tried to toggle '{}' (Self-floor tier — spec §10)",
            actor, seat
        ));
    }
    if let Some(floor) = state.get_mut("floor").and_then(|f| f.as_object_mut()) {
        let queue_owned: Vec<serde_json::Value> = floor
            .get("queue")
            .and_then(|q| q.as_array())
            .cloned()
            .unwrap_or_default();
        let already_in = queue_owned.iter().any(|v| v.as_str() == Some(&seat));
        let mut new_queue = queue_owned;
        if already_in {
            new_queue.retain(|v| v.as_str() != Some(&seat));
        } else {
            new_queue.push(serde_json::json!(seat));
        }
        floor.insert("queue".to_string(), serde_json::Value::Array(new_queue));
    }
    Ok(())
}

/// keep_alive — composer-typing heartbeat. Writes `last_drafting_at_ms` to
/// sessions.json (NOT protocol.json — spec §3.1 perf rule). Server-side
/// debounce: if prior stamp is within KEEP_ALIVE_SERVER_DEBOUNCE_MS (5s),
/// no-op (returns `debounced: true`). Caller-side debounce is also recommended
/// (spec § 3.1 ≤1 ping per 5–10s) but server-side is the safety net.
fn apply_keep_alive(project_dir: &str, actor: &str) -> Result<serde_json::Value, String> {
    let parts: Vec<&str> = actor.splitn(2, ':').collect();
    let role = parts.get(0).copied().unwrap_or("").to_string();
    let inst: u64 = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    let result: Result<Result<serde_json::Value, String>, String> =
        with_file_lock(project_dir, || {
            let mut sessions = read_sessions(project_dir);
            let mut updated = false;
            if let Some(bindings) =
                sessions.get_mut("bindings").and_then(|b| b.as_array_mut())
            {
                for b in bindings.iter_mut() {
                    let b_role = b.get("role").and_then(|v| v.as_str()).unwrap_or("");
                    let b_inst = b.get("instance").and_then(|v| v.as_u64()).unwrap_or(0);
                    if b_role == role && b_inst == inst {
                        let prior = b
                            .get("last_drafting_at_ms")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0);
                        if now_ms.saturating_sub(prior) >= KEEP_ALIVE_SERVER_DEBOUNCE_MS {
                            if let Some(obj) = b.as_object_mut() {
                                obj.insert(
                                    "last_drafting_at_ms".to_string(),
                                    serde_json::json!(now_ms),
                                );
                                updated = true;
                            }
                        }
                        break;
                    }
                }
            }
            if updated {
                // Surface write errors loudly (evil-arch #939 concern 3 —
                // tech-leader #931 D forbids silent `let _ =` on disk ops in
                // new code). keep_alive failure is non-fatal (caller can
                // retry on next keystroke), but visibility is required.
                if let Err(e) = write_sessions(project_dir, &sessions) {
                    eprintln!(
                        "[protocol_mcp] apply_keep_alive: write_sessions failed for {}: {}",
                        actor, e
                    );
                    return Ok(Err(format!("[InternalError] keep_alive write_sessions failed: {}", e)));
                }
            }
            Ok(Ok(serde_json::json!({
                "ok": true,
                "debounced": !updated,
                "last_drafting_at_ms": now_ms
            })))
        });
    match result {
        Ok(inner) => inner,
        Err(e) => Err(e),
    }
}

/// Apply `metadata.mic_to` transfer atomically. Caller MUST hold board.lock
/// (project_send already does, before the board append). Reads protocol.json,
/// applies transfer per spec §2.2 row 2 + §10 atomicity, writes protocol.json.
/// Returns Err iff the disk read/write fails — semantic refusals (mode doesn't
/// permit, caller not authorized, target unauthorized) are silent successes
/// (the floor stays put per spec). Vacant target falls through to queue head
/// per spec §2.2 row 2 (dev-chall #940.5 fix).
fn apply_protocol_mic_to_transfer(
    project_dir: &str,
    section: &str,
    from_label: &str,
    requested_target: &str,
) -> Result<(), String> {
    let mut current = read_protocol_for_section_value(project_dir, section);
    let floor_mode = current
        .get("floor")
        .and_then(|f| f.get("mode"))
        .and_then(|v| v.as_str())
        .unwrap_or("none")
        .to_string();
    let permits_transfer = matches!(
        floor_mode.as_str(),
        "reactive" | "queue" | "free-grab"
    );
    if !permits_transfer {
        return Ok(()); // mode prohibits transfers — silent success (floor stays)
    }

    let cur_speaker = current
        .get("floor")
        .and_then(|f| f.get("current_speaker"))
        .and_then(|v| v.as_str())
        .map(String::from);
    let caller_authorized =
        cur_speaker.is_none() || cur_speaker.as_deref() == Some(from_label);
    if !caller_authorized {
        return Ok(()); // caller not speaker — silent success
    }

    // Resolve target per spec §2.2 row 2: requested seat is vacant →
    // fall through to queue head; queue empty → floor goes idle.
    let resolved_target: Option<String> = if protocol_seat_exists_active(project_dir, requested_target) {
        Some(requested_target.to_string())
    } else {
        // Vacant target — fall through to queue head (spec §2.2 row 2).
        current
            .get("floor")
            .and_then(|f| f.get("queue"))
            .and_then(|q| q.as_array())
            .and_then(|arr| arr.first())
            .and_then(|v| v.as_str())
            .map(String::from)
    };

    // Apply transfer (or fall-through-to-idle if neither target nor queue head).
    if let Some(floor) = current.get_mut("floor").and_then(|f| f.as_object_mut()) {
        floor.insert(
            "current_speaker".to_string(),
            resolved_target
                .as_ref()
                .map(|s| serde_json::json!(s))
                .unwrap_or(serde_json::Value::Null),
        );
        if let Some(rt) = &resolved_target {
            if let Some(queue) = floor.get_mut("queue").and_then(|q| q.as_array_mut()) {
                queue.retain(|v| v.as_str() != Some(rt));
            }
        }
    }

    let cur_rev = current.get("rev").and_then(|v| v.as_u64()).unwrap_or(0);
    if let Some(rev_field) = current.get_mut("rev") {
        *rev_field = serde_json::json!(cur_rev + 1);
    }
    if let Some(obj) = current.as_object_mut() {
        obj.insert(
            "last_writer_seat".to_string(),
            serde_json::json!(from_label),
        );
        let action_label = if resolved_target.as_deref() == Some(requested_target) {
            "project_send_mic_to"
        } else if resolved_target.is_some() {
            "project_send_mic_to_fallthrough_queue_head"
        } else {
            "project_send_mic_to_fallthrough_idle"
        };
        obj.insert(
            "last_writer_action".to_string(),
            serde_json::json!(action_label),
        );
        obj.insert(
            "rev_at".to_string(),
            serde_json::json!(utc_now_iso()),
        );
    }

    write_protocol_for_section_value(project_dir, section, &current)
}

// ============================================================
// Slice 5 — phase plan executor (spec §7 + §3.3 outcome predicates).
// Five action implementations + the four outcome evaluators they drive.
// ============================================================

fn apply_set_phase_plan(state: &mut serde_json::Value, args: &serde_json::Value) -> Result<(), String> {
    let phases = args.get("phases")
        .and_then(|v| v.as_array())
        .ok_or("[InvalidArgs] set_phase_plan requires args.phases (array of phase objects)")?
        .clone();
    if phases.is_empty() {
        return Err("[InvalidArgs] set_phase_plan: phases array must be non-empty".to_string());
    }
    // Validate each phase has at minimum a preset and an outcome.
    for (i, p) in phases.iter().enumerate() {
        let preset = p.get("preset").and_then(|v| v.as_str());
        let outcome = p.get("outcome");
        if preset.is_none() {
            return Err(format!("[InvalidArgs] phase[{}] missing 'preset'", i));
        }
        if outcome.is_none() {
            return Err(format!("[InvalidArgs] phase[{}] missing 'outcome'", i));
        }
    }
    // Stamp started_at on the first phase if not already set.
    let mut phases_owned = phases;
    if let Some(first) = phases_owned.first_mut() {
        if first.get("started_at").is_none() || first.get("started_at").map(|v| v.is_null()).unwrap_or(true) {
            if let Some(obj) = first.as_object_mut() {
                obj.insert("started_at".to_string(), serde_json::json!(utc_now_iso()));
            }
        }
    }
    if let Some(plan) = state.get_mut("phase_plan").and_then(|p| p.as_object_mut()) {
        plan.insert("phases".to_string(), serde_json::Value::Array(phases_owned));
        plan.insert("current_phase_idx".to_string(), serde_json::json!(0));
        plan.insert("paused_at".to_string(), serde_json::Value::Null);
        plan.insert("paused_total_secs".to_string(), serde_json::json!(0));
    }
    Ok(())
}

/// Force-advance to the next phase regardless of outcome predicate. Stamps
/// `ended_at` on the current phase + `started_at` on the next. If the
/// current phase is the last, ends the plan (current_phase_idx unchanged
/// but the predicate evaluator returns "complete").
fn apply_advance_phase(state: &mut serde_json::Value, _project_dir: &str) -> Result<(), String> {
    let plan = state.get_mut("phase_plan").and_then(|p| p.as_object_mut())
        .ok_or("[InternalError] phase_plan field missing")?;
    let phases_len = plan.get("phases").and_then(|p| p.as_array()).map(|a| a.len()).unwrap_or(0);
    if phases_len == 0 {
        return Err("[InvalidArgs] advance_phase: no phase_plan set — call set_phase_plan first".to_string());
    }
    let cur_idx = plan.get("current_phase_idx").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
    if cur_idx >= phases_len {
        return Err(format!("[InvalidArgs] advance_phase: already past last phase ({}/{})", cur_idx, phases_len));
    }
    let now = utc_now_iso();
    if let Some(phases) = plan.get_mut("phases").and_then(|p| p.as_array_mut()) {
        if let Some(cur_phase) = phases.get_mut(cur_idx).and_then(|p| p.as_object_mut()) {
            cur_phase.insert("ended_at".to_string(), serde_json::json!(now));
        }
        let next_idx = cur_idx + 1;
        if next_idx < phases_len {
            if let Some(next_phase) = phases.get_mut(next_idx).and_then(|p| p.as_object_mut()) {
                if next_phase.get("started_at").map(|v| v.is_null()).unwrap_or(true) {
                    next_phase.insert("started_at".to_string(), serde_json::json!(now));
                }
            }
        }
    }
    let next_idx = (cur_idx + 1).min(phases_len);
    plan.insert("current_phase_idx".to_string(), serde_json::json!(next_idx));
    Ok(())
}

fn apply_pause_plan(state: &mut serde_json::Value) -> Result<(), String> {
    let plan = state.get_mut("phase_plan").and_then(|p| p.as_object_mut())
        .ok_or("[InternalError] phase_plan field missing")?;
    if !plan.get("paused_at").map(|v| v.is_null()).unwrap_or(true) {
        return Err("[InvalidArgs] pause_plan: plan is already paused — call resume_plan first".to_string());
    }
    plan.insert("paused_at".to_string(), serde_json::json!(utc_now_iso()));
    Ok(())
}

fn apply_resume_plan(state: &mut serde_json::Value) -> Result<(), String> {
    let plan = state.get_mut("phase_plan").and_then(|p| p.as_object_mut())
        .ok_or("[InternalError] phase_plan field missing")?;
    let paused_at = plan.get("paused_at").and_then(|v| v.as_str()).map(String::from);
    let paused_at = match paused_at {
        Some(s) => s,
        None => return Err("[InvalidArgs] resume_plan: plan is not paused".to_string()),
    };
    // Add elapsed-while-paused to paused_total_secs accumulator (spec §3
    // schema field — outcome timer compares wall-clock minus this).
    let paused_secs = parse_iso_to_epoch_secs(&paused_at);
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let prev_total = plan.get("paused_total_secs").and_then(|v| v.as_u64()).unwrap_or(0);
    let pause_duration = paused_secs.map(|s| now_secs.saturating_sub(s)).unwrap_or(0);
    plan.insert("paused_total_secs".to_string(), serde_json::json!(prev_total + pause_duration));
    plan.insert("paused_at".to_string(), serde_json::Value::Null);
    Ok(())
}

fn apply_extend_phase(state: &mut serde_json::Value, args: &serde_json::Value) -> Result<(), String> {
    let secs = args.get("secs").and_then(|v| v.as_u64())
        .ok_or("[InvalidArgs] extend_phase requires args.secs (positive integer)")?;
    let plan = state.get_mut("phase_plan").and_then(|p| p.as_object_mut())
        .ok_or("[InternalError] phase_plan field missing")?;
    let cur_idx = plan.get("current_phase_idx").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
    let phases = plan.get_mut("phases").and_then(|p| p.as_array_mut())
        .ok_or("[InternalError] phase_plan.phases not an array")?;
    let phase = phases.get_mut(cur_idx).and_then(|p| p.as_object_mut())
        .ok_or(format!("[InvalidArgs] extend_phase: current_phase_idx {} out of range", cur_idx))?;
    let prev_ext = phase.get("extension_secs").and_then(|v| v.as_u64()).unwrap_or(0);
    phase.insert("extension_secs".to_string(), serde_json::json!(prev_ext + secs));
    Ok(())
}

/// Evaluate the outcome predicate for a phase. Returns true if the phase
/// is "done." Spec §3.3 + §7 outcome table:
///   - file_nonempty: target path exists and size > 0
///   - timer: now > started_at + duration_secs + extension_secs - paused_total_secs
///   - manual: always false (only advance_phase fires, predicate never true on its own)
///   - vote_quorum: deferred to Slice 6 consensus (returns false until then)
#[allow(dead_code)] // Used by Slice 5's auto-advance loop (lands at scheduler in subsequent commit).
fn evaluate_phase_outcome(
    phase: &serde_json::Value,
    project_dir: &str,
    paused_total_secs: u64,
) -> bool {
    let outcome = match phase.get("outcome") {
        Some(o) => o,
        None => return false,
    };
    let kind = outcome.get("kind").and_then(|v| v.as_str()).unwrap_or("");
    match kind {
        "file_nonempty" => {
            let target = match outcome.get("target").and_then(|v| v.as_str()) {
                Some(t) => t,
                None => return false,
            };
            let abs = if std::path::Path::new(target).is_absolute() {
                target.to_string()
            } else {
                format!("{}/{}", project_dir, target)
            };
            std::fs::metadata(&abs).map(|m| m.len() > 0).unwrap_or(false)
        }
        "timer" => {
            let started = match phase.get("started_at").and_then(|v| v.as_str()) {
                Some(s) => s,
                None => return false,
            };
            let started_secs = match parse_iso_to_epoch_secs(started) {
                Some(s) => s,
                None => return false,
            };
            let duration = phase.get("duration_secs").and_then(|v| v.as_u64()).unwrap_or(0);
            let extension = phase.get("extension_secs").and_then(|v| v.as_u64()).unwrap_or(0);
            if duration == 0 { return false; } // duration=0 means manual outcome
            let now_secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let elapsed = now_secs.saturating_sub(started_secs).saturating_sub(paused_total_secs);
            elapsed >= duration + extension
        }
        "manual" => false, // only advance_phase fires
        "vote_quorum" => false, // Slice 6 owns consensus rounds
        _ => false,
    }
}

// ============================================================
// Slice 6 — consensus actions (spec §3 + §10 + §6 matrix).
// Three actions: open_round / submit / close_round. Per spec §10
// "Round-scoped" auth tier: open_round any seat, submit any
// participant, close_round opener OR plan-author.
// ============================================================

fn apply_open_round(state: &mut serde_json::Value, args: &serde_json::Value, actor: &str) -> Result<(), String> {
    let topic = args.get("topic").and_then(|v| v.as_str())
        .ok_or("[InvalidArgs] open_round requires args.topic (string)")?;
    let mode = args.get("mode").and_then(|v| v.as_str()).unwrap_or("tally");
    if !matches!(mode, "tally" | "vote" | "silence-consent") {
        return Err(format!("[InvalidArgs] open_round.mode must be tally|vote|silence-consent (got '{}')", mode));
    }
    // Refuse if a round is already open in submitting/reviewing phase.
    if let Some(phase) = state.get("consensus").and_then(|c| c.get("phase")).and_then(|p| p.as_str()) {
        if phase == "submitting" || phase == "reviewing" {
            return Err(format!("[NotPermitted] open_round: a round is already open (phase: {})", phase));
        }
    }
    // Slice 7: Oxford preset uses args.teams = {for: [], against: []} for
    // team assignment (spec §6 matrix). Validate shape if present.
    let teams = args.get("teams").cloned();
    if let Some(t) = &teams {
        if !t.is_object() {
            return Err("[InvalidArgs] open_round.teams must be an object {for: [seats], against: [seats]} when present".to_string());
        }
        for key in ["for", "against"] {
            if let Some(arr) = t.get(key) {
                if !arr.is_array() {
                    return Err(format!("[InvalidArgs] open_round.teams.{} must be an array of seat labels", key));
                }
            }
        }
    }
    // Slice 7: Delphi preset uses args.blind_submit_gate=true to indicate
    // the round operates under blind-submission semantics (no broadcasts
    // to "all" allowed during submitting; only directed-to-moderator).
    let blind_gate = args.get("blind_submit_gate").and_then(|v| v.as_bool()).unwrap_or(false);

    if let Some(cons) = state.get_mut("consensus").and_then(|c| c.as_object_mut()) {
        cons.insert("mode".to_string(), serde_json::json!(mode));
        let mut round = serde_json::json!({
            "topic": topic,
            "opened_at": utc_now_iso(),
            "opened_by": actor
        });
        if let Some(t) = teams {
            round.as_object_mut().unwrap().insert("teams".to_string(), t);
        }
        if blind_gate {
            round.as_object_mut().unwrap().insert("blind_submit_gate".to_string(), serde_json::json!(true));
        }
        cons.insert("round".to_string(), round);
        cons.insert("phase".to_string(), serde_json::json!("submitting"));
        cons.insert("submissions".to_string(), serde_json::json!([]));
    }
    Ok(())
}

fn apply_submit(state: &mut serde_json::Value, args: &serde_json::Value, actor: &str) -> Result<(), String> {
    let body = args.get("body")
        .ok_or("[InvalidArgs] submit requires args.body (any JSON)")?
        .clone();
    let phase = state.get("consensus").and_then(|c| c.get("phase")).and_then(|p| p.as_str()).unwrap_or("");
    if phase != "submitting" {
        return Err(format!("[NotPermitted] submit: round phase must be 'submitting' (got '{}')", phase));
    }
    // Per-role scope (spec §3 scopes.consensus = "role"): replace prior
    // submission by same role to honor "one vote per role" invariant.
    let role: String = actor.split(':').next().unwrap_or(actor).to_string();
    if let Some(cons) = state.get_mut("consensus").and_then(|c| c.as_object_mut()) {
        let submissions = cons.entry("submissions").or_insert_with(|| serde_json::json!([]));
        if let Some(arr) = submissions.as_array_mut() {
            arr.retain(|s| {
                s.get("from").and_then(|v| v.as_str())
                    .map(|f| f.split(':').next().unwrap_or(f) != role)
                    .unwrap_or(true)
            });
            arr.push(serde_json::json!({
                "from": actor,
                "role": role,
                "body": body,
                "submitted_at": utc_now_iso()
            }));
        }
    }
    Ok(())
}

fn apply_close_round(state: &mut serde_json::Value, _args: &serde_json::Value, actor: &str) -> Result<(), String> {
    // Auth: opener OR plan-author (v0 permissive — any seat may close in
    // v0 per spec §10 v0/v1 split, audit row stamps the actor).
    let opened_by = state.get("consensus")
        .and_then(|c| c.get("round"))
        .and_then(|r| r.get("opened_by"))
        .and_then(|v| v.as_str())
        .map(String::from);
    let _ = opened_by; // v0 permissive; v1 will gate on (opener OR plan-author set)
    let _ = actor;

    let phase = state.get("consensus").and_then(|c| c.get("phase")).and_then(|p| p.as_str()).unwrap_or("");
    if phase == "closed" {
        return Err("[NotPermitted] close_round: round is already closed".to_string());
    }
    if phase == "" {
        return Err("[InvalidArgs] close_round: no round is open".to_string());
    }
    if let Some(cons) = state.get_mut("consensus").and_then(|c| c.as_object_mut()) {
        cons.insert("phase".to_string(), serde_json::json!("closed"));
    }
    Ok(())
}

/// Auto-advance check: if the current phase's outcome predicate evaluates
/// true, fire advance_phase. Called from get_protocol read path so every
/// UI poll opportunistically checks. Idempotent — if predicate is true and
/// we're already past last phase, no-op.
fn auto_advance_if_outcome_met(state: &mut serde_json::Value, project_dir: &str) -> bool {
    let phases = match state.get("phase_plan").and_then(|p| p.get("phases")).and_then(|p| p.as_array()) {
        Some(p) if !p.is_empty() => p.clone(),
        _ => return false,
    };
    let cur_idx = state.get("phase_plan").and_then(|p| p.get("current_phase_idx"))
        .and_then(|v| v.as_u64()).unwrap_or(0) as usize;
    if cur_idx >= phases.len() { return false; }

    // Skip auto-advance if plan is paused.
    let paused = state.get("phase_plan")
        .and_then(|p| p.get("paused_at"))
        .map(|v| !v.is_null())
        .unwrap_or(false);
    if paused { return false; }

    let paused_total = state.get("phase_plan")
        .and_then(|p| p.get("paused_total_secs"))
        .and_then(|v| v.as_u64()).unwrap_or(0);

    let cur_phase = &phases[cur_idx];
    if !evaluate_phase_outcome(cur_phase, project_dir, paused_total) {
        return false;
    }

    // Fire advance_phase inline (don't recurse through dispatch — we're
    // already inside a get_protocol path that holds no lock).
    let _ = apply_advance_phase(state, project_dir);
    true
}

// ============================================================
// Two-controls v1 (spec 2026-05-14, items 1-9 from consolidated
// findings list). Adds 8 new protocol_mutate actions:
//   set_assembly, accept_plan, open_planning, revise_plan,
//   set_mic_passing, raise_hand, grant_mic, set_moderator
// + plan_path validation, scope-block parsing, role-gate for revise_plan.
// ============================================================

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Sha256, Digest};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

fn caller_role(actor: &str) -> &str {
    actor.splitn(2, ':').next().unwrap_or(actor)
}

/// Parse `<!-- scope: ... -->` block. Returns Some(vec) on found,
/// None on absent. `*` body returns Some(empty vec) for unrestricted.
fn parse_scope_block(plan_content: &str) -> Option<Vec<String>> {
    let needle_open = "<!-- scope:";
    let pos = plan_content.find(needle_open)?;
    let after = &plan_content[pos + needle_open.len()..];
    let close = after.find("-->")?;
    let body = after[..close].trim();
    if body == "*" {
        Some(vec![])
    } else if body.is_empty() {
        Some(vec![])
    } else {
        Some(body.split_whitespace().map(String::from).collect())
    }
}

/// Validate plan_path per spec § plan_path validation.
/// Returns canonical absolute path on success.
///
/// Per dev-challenger msg 1115 F5 + evil-arch msg 1117 F5: pre-resolution
/// outside-check fires BEFORE canonicalize so the [PlanPathOutsideDesignNotes]
/// variant is reachable for paths the resolver wouldn't naturally land
/// outside design-notes. Specifically: absolute paths, paths starting with
/// repo-relative segments not under .vaak/design-notes/, and paths with `..`
/// segments. Without this, [PlanPathMissing] swallows what should be
/// [PlanPathOutsideDesignNotes] for any valid-on-disk file outside the
/// allowlist.
fn validate_plan_path(project_dir: &str, plan_path: &str) -> Result<PathBuf, String> {
    if !plan_path.ends_with(".md") {
        return Err(format!(
            "[PlanPathNotMarkdown] plan_path must end in .md: {}",
            plan_path
        ));
    }
    // Pre-resolution outside-checks (run BEFORE canonicalize):
    if plan_path.split(['/', '\\']).any(|seg| seg == "..") {
        return Err(format!(
            "[PlanPathOutsideDesignNotes] plan_path contains '..' segments: {}",
            plan_path
        ));
    }
    if Path::new(plan_path).is_absolute() {
        return Err(format!(
            "[PlanPathOutsideDesignNotes] plan_path must be repo-relative under .vaak/design-notes/, not absolute: {}",
            plan_path
        ));
    }
    let normalized_prefix = plan_path.replace('\\', "/");
    let starts_under_design = normalized_prefix.starts_with(".vaak/design-notes/");
    let has_repo_path_separator = normalized_prefix.contains('/');
    if has_repo_path_separator && !starts_under_design {
        // Repo-relative path with separators that doesn't start under
        // .vaak/design-notes/ → declared outside. Plain filename without a
        // separator is fine — resolver will join with the design-notes base.
        return Err(format!(
            "[PlanPathOutsideDesignNotes] plan_path must resolve under .vaak/design-notes/: {}",
            plan_path
        ));
    }

    let base = Path::new(project_dir).join(".vaak").join("design-notes");
    let candidate = if starts_under_design {
        Path::new(project_dir).join(plan_path)
    } else {
        base.join(plan_path)
    };
    let canon_cand = candidate.canonicalize().map_err(|_| {
        format!("[PlanPathMissing] plan_path file does not exist or is not readable: {}", plan_path)
    })?;
    let canon_base = base.canonicalize().unwrap_or(base.clone());
    // Defense-in-depth: even after the pre-resolution checks, refuse
    // anything that canonicalizes outside the allowlist (symlinks etc.).
    if !canon_cand.starts_with(&canon_base) {
        return Err(format!(
            "[PlanPathOutsideDesignNotes] plan_path resolves outside .vaak/design-notes/ after canonicalization: {}",
            plan_path
        ));
    }
    let content = std::fs::read_to_string(&canon_cand)
        .map_err(|e| format!("[PlanPathMissing] cannot read plan_path: {}", e))?;
    if parse_scope_block(&content).is_none() {
        return Err(
            "[PlanScopeBlockMissing] plan file lacks <!-- scope: path1 path2 -->. \
             Use <!-- scope: * --> for unrestricted plans."
                .to_string(),
        );
    }
    Ok(canon_cand)
}

fn apply_set_assembly(
    state: &mut serde_json::Value,
    args: &serde_json::Value,
) -> Result<(), String> {
    let active = args
        .get("active")
        .and_then(|v| v.as_bool())
        .ok_or("[InvalidArgs] set_assembly requires args.active (bool)")?;
    // Coordinate with existing preset model: assembly_active==true ⇒ preset=Assembly Line.
    let target_preset = if active {
        PRESET_ASSEMBLY_LINE
    } else {
        PRESET_DEFAULT_CHAT
    };
    apply_set_preset(state, &serde_json::json!({ "name": target_preset }))?;
    if let Some(floor) = state.get_mut("floor").and_then(|f| f.as_object_mut()) {
        floor.insert("assembly_active".to_string(), serde_json::json!(active));
        if !active {
            floor.insert("current_speaker".to_string(), serde_json::Value::Null);
        }
    }
    Ok(())
}

/// is_seat_exempt — derived helper per moderator-authority spec line 25.
/// A seat is "exempt" (out of the pipeline) when assembly is in moderator
/// mic-passing mode AND the seat IS the designated moderator. No new Floor
/// field — computed inline from existing `mic_passing_mode` + `moderator`.
fn is_seat_exempt(state: &serde_json::Value, seat: &str) -> bool {
    let floor = match state.get("floor") {
        Some(f) => f,
        None => return false,
    };
    let mode = floor
        .get("mic_passing_mode")
        .and_then(|v| v.as_str())
        .unwrap_or("rotation");
    if mode != "moderator" {
        return false;
    }
    floor
        .get("moderator")
        .and_then(|v| v.as_str())
        .map(|m| m == seat)
        .unwrap_or(false)
}

/// accept_plan — gated to moderator OR architect/manager/human per
/// feature/moderator-authority spec Item 4 (closes evil-arch msg 1490
/// CRITICAL phase-gate gap). Pre-A.5.2: this function had no gate; any
/// agent could flip phase via MCP, voiding plan_hash and bypassing the
/// destructive-confirm modal entirely.
fn apply_accept_plan(
    state: &mut serde_json::Value,
    args: &serde_json::Value,
    actor: &str,
    project_dir: &str,
) -> Result<(), String> {
    let role = caller_role(actor);
    let is_moderator = state
        .get("floor")
        .and_then(|f| f.get("moderator"))
        .and_then(|v| v.as_str())
        == Some(actor);
    let is_privileged = matches!(role, "architect" | "manager" | "human");
    if !is_moderator && !is_privileged {
        return Err(format!(
            "[AcceptPlanForbidden] caller '{}' (role '{}') may not call accept_plan — gated to current moderator OR architect/manager/human (evil-arch msg 1490 CRITICAL closure, moderator-authority Item 4).",
            actor, role
        ));
    }
    let plan_path = args
        .get("plan_path")
        .and_then(|v| v.as_str())
        .ok_or("[InvalidArgs] accept_plan requires args.plan_path (string)")?;
    let canon = validate_plan_path(project_dir, plan_path)?;
    let content = std::fs::read(&canon).map_err(|e| {
        format!("[PlanPathMissing] cannot read plan_path post-validation: {}", e)
    })?;
    let hash = sha256_hex(&content);
    if let Some(floor) = state.get_mut("floor").and_then(|f| f.as_object_mut()) {
        floor.insert("phase".to_string(), serde_json::json!("execution"));
        floor.insert("plan_path".to_string(), serde_json::json!(plan_path));
        floor.insert("plan_hash".to_string(), serde_json::json!(hash));
    }
    Ok(())
}

/// open_planning — same gate pattern as apply_accept_plan per
/// feature/moderator-authority spec Item 4.
fn apply_open_planning(state: &mut serde_json::Value, actor: &str) -> Result<(), String> {
    let role = caller_role(actor);
    let is_moderator = state
        .get("floor")
        .and_then(|f| f.get("moderator"))
        .and_then(|v| v.as_str())
        == Some(actor);
    let is_privileged = matches!(role, "architect" | "manager" | "human");
    if !is_moderator && !is_privileged {
        return Err(format!(
            "[OpenPlanningForbidden] caller '{}' (role '{}') may not call open_planning — gated to current moderator OR architect/manager/human (evil-arch msg 1490 CRITICAL closure, moderator-authority Item 4).",
            actor, role
        ));
    }
    if let Some(floor) = state.get_mut("floor").and_then(|f| f.as_object_mut()) {
        floor.insert("phase".to_string(), serde_json::json!("planning"));
        floor.insert("plan_path".to_string(), serde_json::Value::Null);
        floor.insert("plan_hash".to_string(), serde_json::Value::Null);
    }
    Ok(())
}

/// revise_plan — gated to architect/manager/human only per spec §64 / finding #1.
/// Without this gate the entire pre-commit-hook structural floor is bypassable
/// by any single rogue or confused agent (evil-arch msg 988 CRITICAL).
fn apply_revise_plan(
    state: &mut serde_json::Value,
    args: &serde_json::Value,
    actor: &str,
    project_dir: &str,
) -> Result<(), String> {
    let role = caller_role(actor);
    if !matches!(role, "architect" | "manager" | "human") {
        return Err(format!(
            "[RevisePlanForbidden] caller role '{}' may not call revise_plan — gated to architect/manager/human only (spec §64, finding #1).",
            role
        ));
    }
    let plan_path = args
        .get("plan_path")
        .and_then(|v| v.as_str())
        .ok_or("[InvalidArgs] revise_plan requires args.plan_path (string)")?;
    let canon = validate_plan_path(project_dir, plan_path)?;
    let content = std::fs::read(&canon).map_err(|e| {
        format!("[PlanPathMissing] cannot read plan_path post-validation: {}", e)
    })?;
    let new_hash = sha256_hex(&content);
    if let Some(floor) = state.get_mut("floor").and_then(|f| f.as_object_mut()) {
        floor.insert("plan_path".to_string(), serde_json::json!(plan_path));
        floor.insert("plan_hash".to_string(), serde_json::json!(new_hash));
    }
    Ok(())
}

/// set_mic_passing — returns `MutateOutcome::NoOp` when defer-silent fires
/// (mid-turn mode change with current_speaker set). Dispatch checks the
/// outcome and skips normalize/rev/write/emit on NoOp so observers don't see
/// phantom mic_passing_mode_changed events with old==new + spurious rev bumps
/// (tester msg 1111 T-FINDING).
fn apply_set_mic_passing(
    state: &mut serde_json::Value,
    args: &serde_json::Value,
) -> Result<MutateOutcome, String> {
    let mode = args
        .get("mode")
        .and_then(|v| v.as_str())
        .ok_or("[InvalidArgs] set_mic_passing requires args.mode (string)")?;
    if !matches!(mode, "rotation" | "hand_raise" | "moderator") {
        return Err(format!(
            "[UnknownMicMechanism] mic_passing_mode must be rotation|hand_raise|moderator: {}",
            mode
        ));
    }
    let prev = state
        .get("floor")
        .and_then(|f| f.get("mic_passing_mode"))
        .and_then(|v| v.as_str())
        .unwrap_or("rotation")
        .to_string();
    let current_speaker = state
        .get("floor")
        .and_then(|f| f.get("current_speaker"))
        .and_then(|v| v.as_str())
        .map(String::from);

    // Mid-turn semantics per architect msg 1070: defer-silent. If a current_speaker
    // exists AND mode is changing, this is a no-op (caller must retry after yield).
    // TODO(consolidated-findings #13): surface pending mechanism via
    // floor.mic_mechanism_pending field in status strip when item 13 is taken up.
    if current_speaker.is_some() && prev != mode {
        return Ok(MutateOutcome::NoOp);
    }

    // No-op also when caller specified the same mode that's already active —
    // avoids spurious old==new event emission on idempotent calls.
    if prev == mode {
        return Ok(MutateOutcome::NoOp);
    }

    if let Some(floor) = state.get_mut("floor").and_then(|f| f.as_object_mut()) {
        floor.insert("mic_passing_mode".to_string(), serde_json::json!(mode));
        // Cascading state cleanup so mode-switches don't leak stale state
        // (rotation→moderator clears hand_queue; moderator→rotation nulls moderator; etc.).
        match mode {
            "rotation" => {
                floor.insert(
                    "hand_queue".to_string(),
                    serde_json::Value::Array(vec![]),
                );
                floor.insert("moderator".to_string(), serde_json::Value::Null);
            }
            "hand_raise" => {
                floor.insert("moderator".to_string(), serde_json::Value::Null);
            }
            "moderator" => {
                floor.insert(
                    "hand_queue".to_string(),
                    serde_json::Value::Array(vec![]),
                );
            }
            _ => {}
        }
    }
    Ok(MutateOutcome::Applied)
}

/// Outcome variant for apply_* functions that may legitimately no-op (e.g.
/// defer-silent set_mic_passing per architect msg 1070 (b)). Dispatch reads
/// this to decide whether to fire post-mutation side effects (normalize, rev
/// bump, write, board event). NoOp = leave protocol.json + board untouched.
#[derive(Debug, PartialEq, Eq)]
enum MutateOutcome {
    Applied,
    NoOp,
}

fn apply_raise_hand(state: &mut serde_json::Value, actor: &str) -> Result<(), String> {
    let mode = state
        .get("floor")
        .and_then(|f| f.get("mic_passing_mode"))
        .and_then(|v| v.as_str())
        .unwrap_or("rotation");
    if mode != "hand_raise" {
        return Err(format!(
            "[NotPermitted] raise_hand requires mic_passing_mode == 'hand_raise' (current: {})",
            mode
        ));
    }
    if let Some(floor) = state.get_mut("floor").and_then(|f| f.as_object_mut()) {
        if let Some(queue) = floor.get_mut("hand_queue").and_then(|q| q.as_array_mut()) {
            // Idempotent: don't re-add if already queued.
            if !queue.iter().any(|v| v.as_str() == Some(actor)) {
                queue.push(serde_json::json!(actor));
            }
        }
    }
    Ok(())
}

fn apply_grant_mic(
    state: &mut serde_json::Value,
    args: &serde_json::Value,
    actor: &str,
) -> Result<(), String> {
    let target = args
        .get("target")
        .and_then(|v| v.as_str())
        .ok_or("[InvalidArgs] grant_mic requires args.target (string)")?;
    let mode = state
        .get("floor")
        .and_then(|f| f.get("mic_passing_mode"))
        .and_then(|v| v.as_str())
        .unwrap_or("rotation");
    if mode != "moderator" {
        return Err(format!(
            "[NotPermitted] grant_mic requires mic_passing_mode == 'moderator' (current: {})",
            mode
        ));
    }
    let moderator = state
        .get("floor")
        .and_then(|f| f.get("moderator"))
        .and_then(|v| v.as_str())
        .map(String::from);
    if moderator.as_deref() != Some(actor) {
        return Err(format!(
            "[NotPermitted] grant_mic restricted to moderator '{:?}' (caller: {})",
            moderator, actor
        ));
    }
    if let Some(floor) = state.get_mut("floor").and_then(|f| f.as_object_mut()) {
        floor.insert("current_speaker".to_string(), serde_json::json!(target));
    }
    Ok(())
}

/// set_rotation_order — Fix-A2 typed mutation per spec at
/// `.vaak/design-notes/fix-a2-set-rotation-order-spec-2026-05-22.md`.
///
/// REORDER-ONLY contract: `args.rotation_order` must be a permutation of
/// `active_seats - kicked` (= the active-seat set returned by
/// `protocol_active_seats_set`). Membership is fixed; only order may change.
/// Removals route through the existing typed `project_kick` mutation —
/// allowing omission here would reopen the v1.0-corrected structural-
/// exclusion bug class (commits 453228c / e582e6e / 1c26267 / 7895a03,
/// 2026-05-13).
///
/// Authorization: `moderator | human | manager:* | tech-leader:*`.
/// tech-leader is included per `[[project_tech_leader_is_manager_consultant]]`
/// + Instance #11 inline close (evil-arch msg 119 + tech-leader msg 121 +
/// architect msg 141 ruling — no defer-to-follow-up plan).
///
/// `active_seats` is read inside this function (under the dispatcher's
/// `with_file_lock`) per dev-challenger:0 msg 129 flag #6 — reading
/// pre-lock and passing in can yield a stale list.
///
/// CAS + rev bump + audit stamps + atomic_write + board event are handled
/// by the dispatcher wrapper, consistent with every other apply_* arm.
fn apply_set_rotation_order(
    state: &mut serde_json::Value,
    args: &serde_json::Value,
    actor: &str,
    project_dir: &str,
) -> Result<(), String> {
    // 1. Auth gate. moderator field on floor, OR special human seat string,
    //    OR caller role is manager/tech-leader. Per dev-challenger:0 msg 129
    //    flag #4: actor is always `role:instance`, so the bare "manager"
    //    branch never matches; use prefix checks. The `actor == "human"`
    //    branch DOES match because the human seat string is literally
    //    "human" (no instance suffix).
    let moderator = state
        .get("floor")
        .and_then(|f| f.get("moderator"))
        .and_then(|v| v.as_str());
    let is_moderator = moderator.map(|m| m == actor).unwrap_or(false);
    let is_authorized = actor == "human"
        || actor.starts_with("manager:")
        || actor.starts_with("tech-leader:")
        || is_moderator;
    if !is_authorized {
        return Err(format!(
            "[Unauthorized] set_rotation_order requires moderator | human | manager | tech-leader; caller={}",
            actor
        ));
    }

    // 2. Args present + array shape.
    let arr = args
        .get("rotation_order")
        .and_then(|v| v.as_array())
        .ok_or("[InvalidArgs] set_rotation_order requires args.rotation_order (array of role:instance strings)")?;

    // 3. Per-entry validation: shape, duplicate, active-membership.
    //    Shape regex `^[a-z0-9-]+:[0-9]+$` validated inline (no `regex`
    //    crate dependency). Acceptable chars: ASCII lowercase letter,
    //    ASCII digit, hyphen — split on FIRST ':' into role + instance;
    //    instance must be all digits and non-empty.
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let active_seats = protocol_active_seats_set(project_dir);
    for v in arr {
        let s = v
            .as_str()
            .ok_or("[InvalidArgs] every rotation_order entry must be a string")?;
        if !is_valid_seat_label(s) {
            return Err(format!(
                "[InvalidArgs] '{}' not in role:instance form (expected lowercase-letters/digits/hyphens, ':', digits)",
                s
            ));
        }
        if !seen.insert(s.to_string()) {
            return Err(format!("[InvalidArgs] duplicate entry '{}' in rotation_order", s));
        }
        if !active_seats.contains(s) {
            return Err(format!("[InvalidArgs] '{}' not an active seat", s));
        }
    }

    // 4. REORDER-ONLY membership check: every active seat must appear in
    //    args.rotation_order. Subset-only would reopen the exclusion class.
    let mut missing: Vec<&String> = active_seats
        .iter()
        .filter(|a| !seen.contains(a.as_str()))
        .collect();
    if !missing.is_empty() {
        missing.sort();
        return Err(format!(
            "[InvalidArgs] set_rotation_order must include every active seat (reorder-only). Missing: {:?}. Use project_kick to remove a seat first.",
            missing
        ));
    }

    // 5. Whole-field replacement preserving siblings.
    let new_arr: Vec<serde_json::Value> = arr.iter().cloned().collect();
    if let Some(floor) = state
        .get_mut("floor")
        .and_then(|f| f.as_object_mut())
    {
        floor.insert(
            "rotation_order".to_string(),
            serde_json::Value::Array(new_arr),
        );
    }
    Ok(())
}

/// Inline check for `^[a-z0-9-]+:[0-9]+$` without pulling in the `regex`
/// crate. Used by `apply_set_rotation_order` arg validation; could fold
/// into a shared helper if/when other arms validate seat labels.
fn is_valid_seat_label(s: &str) -> bool {
    let mut parts = s.splitn(2, ':');
    let role = match parts.next() {
        Some(r) if !r.is_empty() => r,
        _ => return false,
    };
    let instance = match parts.next() {
        Some(i) if !i.is_empty() => i,
        _ => return false,
    };
    if !role
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return false;
    }
    if !instance.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    true
}

/// set_moderator — gated to architect/manager/human per dev-challenger msg 1115
/// F2 + evil-arch msg 1117 CRITICAL upgrade. Same structural-floor parity as
/// apply_revise_plan: without the gate, any agent can self-elect moderator and
/// then call set_mic_passing(moderator) to wedge mic-passing in two calls.
/// Item 14's deferred question is "what authority gate?" (human-only via UI vs
/// vote vs delegation) — NOT "no gate at all." Server-side floor is required.
fn apply_set_moderator(
    state: &mut serde_json::Value,
    args: &serde_json::Value,
    actor: &str,
) -> Result<(), String> {
    let role = caller_role(actor);
    if !matches!(role, "architect" | "manager" | "human") {
        return Err(format!(
            "[SetModeratorForbidden] caller role '{}' may not call set_moderator — gated to architect/manager/human only (dev-challenger msg 1115 F2 + evil-arch msg 1117 CRITICAL).",
            role
        ));
    }
    let target = args
        .get("seat")
        .and_then(|v| v.as_str())
        .ok_or("[InvalidArgs] set_moderator requires args.seat (string)")?;
    if let Some(floor) = state.get_mut("floor").and_then(|f| f.as_object_mut()) {
        floor.insert("moderator".to_string(), serde_json::json!(target));
    }
    Ok(())
}

/// propose_replanning — Collaborative-proposal-workflow v1 spec Commit P.
/// Any active seat in the rotation can propose a replanning flip while phase
/// is "execution". Server-side gate: phase must be "execution"; otherwise
/// reject with [ProposeReplanningPhaseInvalid]. No role gate (per spec — any
/// seat that hits a plan gap can surface it; the moderator decides whether
/// to pivot via accept_replanning). Appends {seat, reason, ts} to the
/// floor.replanning_requests multi-writer queue. The outer `with_file_lock`
/// in `do_protocol_mutate` serializes appends so N simultaneous proposals
/// land FIFO without erasing each other (R3 invariant).
///
/// ts injection per spec v6 line 29 (dev-challenger msg 1939 #3 +
/// architect msg 1944 fold): the `ts: Option<i64>` parameter lets test
/// code seed deterministic timestamps so R3's N≥4-FIFO assertion can
/// discriminate FIFO from random-but-serialized at sub-millisecond
/// resolution. Production callers (the protocol_mutate dispatcher below)
/// pass None and the server fills with now(). Avoids #[cfg(test)] clock-
/// override per architect's "option a, not cfg(test)" call.
fn apply_propose_replanning(
    state: &mut serde_json::Value,
    actor: &str,
    reason: &str,
    ts: Option<i64>,
) -> Result<(), String> {
    let phase = state
        .get("floor")
        .and_then(|f| f.get("phase"))
        .and_then(|v| v.as_str())
        .unwrap_or("execution");
    if phase != "execution" {
        return Err(format!(
            "[ProposeReplanningPhaseInvalid] propose_replanning requires phase == 'execution' (current: {})",
            phase
        ));
    }
    let ts = ts.unwrap_or_else(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0)
    });
    if let Some(floor) = state.get_mut("floor").and_then(|f| f.as_object_mut()) {
        let queue = floor
            .entry("replanning_requests".to_string())
            .or_insert_with(|| serde_json::Value::Array(vec![]));
        if let Some(arr) = queue.as_array_mut() {
            arr.push(serde_json::json!({
                "seat": actor,
                "reason": reason,
                "ts": ts,
            }));
        }
    }
    Ok(())
}

/// set_review_intensity — Strict-turn-discipline + review-intensity-slider
/// spec Commit S. Moderator-set per-task discipline level 1-10. Higher
/// levels activate stricter rules (auto-claim, read-embargo, yield-only).
/// Default 5 preserves current behavior.
///
/// Gate: moderator OR architect/manager/human per v1.X §Item 4 phase-flip
/// predicate. Other callers reject with [SetReviewIntensityForbidden].
/// Validates 1 <= level <= 10 else [InvalidArgs].
fn apply_set_review_intensity(
    state: &mut serde_json::Value,
    args: &serde_json::Value,
    actor: &str,
) -> Result<(), String> {
    let role = caller_role(actor);
    let is_moderator = state
        .get("floor")
        .and_then(|f| f.get("moderator"))
        .and_then(|v| v.as_str())
        == Some(actor);
    let is_privileged = matches!(role, "architect" | "manager" | "human");
    if !is_moderator && !is_privileged {
        return Err(format!(
            "[SetReviewIntensityForbidden] caller '{}' (role '{}') not moderator or privileged — gated to moderator OR architect/manager/human (spec §set_review_intensity role gate).",
            actor, role
        ));
    }
    let level = args
        .get("level")
        .and_then(|v| v.as_u64())
        .ok_or("[InvalidArgs] set_review_intensity requires args.level (integer 1-10)")?;
    if !(1..=10).contains(&level) {
        return Err(format!(
            "[InvalidArgs] set_review_intensity level must be 1-10 (got {})",
            level
        ));
    }
    if let Some(floor) = state.get_mut("floor").and_then(|f| f.as_object_mut()) {
        floor.insert("review_intensity".to_string(), serde_json::json!(level));
    }
    Ok(())
}

/// accept_replanning — Collaborative-proposal-workflow v1 spec Commit Q.
/// School-of-fish pivot: moderator (or architect/manager/human) drains the
/// replanning_requests queue and flips phase back to planning atomically.
/// Mirrors v1.X moderator-authority §Item 4 phase-flip predicate. Side
/// effects within the single CAS write:
///   1. phase → "planning"
///   2. plan_path/plan_hash → null (replanning means prior plan insufficient)
///   3. replanning_requests → [] (queue drained)
/// triggered_by (event payload) derives from args.request_index against the
/// pre-state queue and is computed in emit_two_controls_event after this
/// apply runs — the function itself returns Result<(), String> with no
/// extra return channel needed.
fn apply_accept_replanning(
    state: &mut serde_json::Value,
    args: &serde_json::Value,
    actor: &str,
) -> Result<(), String> {
    let role = caller_role(actor);
    let is_moderator = state
        .get("floor")
        .and_then(|f| f.get("moderator"))
        .and_then(|v| v.as_str())
        == Some(actor);
    let is_privileged = matches!(role, "architect" | "manager" | "human");
    if !is_moderator && !is_privileged {
        return Err(format!(
            "[AcceptReplanningForbidden] caller '{}' (role '{}') not moderator or privileged — gated to moderator OR architect/manager/human (spec §accept_replanning role gate).",
            actor, role
        ));
    }
    // Validate request_index if provided — out-of-bounds rejects so the
    // event payload's triggered_by lookup is consistent with the accept.
    if let Some(idx) = args.get("request_index").and_then(|v| v.as_u64()) {
        let queue_len = state
            .get("floor")
            .and_then(|f| f.get("replanning_requests"))
            .and_then(|q| q.as_array())
            .map(|a| a.len())
            .unwrap_or(0);
        if (idx as usize) >= queue_len {
            return Err(format!(
                "[InvalidArgs] accept_replanning request_index {} out of bounds for queue of length {}",
                idx, queue_len
            ));
        }
    }
    if let Some(floor) = state.get_mut("floor").and_then(|f| f.as_object_mut()) {
        // Atomic side effects per spec line 51-53 — all four state writes
        // land in the SAME CAS-gated protocol.json write (outer
        // with_file_lock in do_protocol_mutate). Observers at T+ε see all
        // four together or none of them (school-of-fish W1).
        floor.insert("phase".to_string(), serde_json::json!("planning"));
        floor.insert("plan_path".to_string(), serde_json::Value::Null);
        floor.insert("plan_hash".to_string(), serde_json::Value::Null);
        floor.insert(
            "replanning_requests".to_string(),
            serde_json::Value::Array(vec![]),
        );
    }
    Ok(())
}

/// Emit a board event after a successful protocol_mutate. Captures pre/post
/// state diff for the spec's 9-event observability invariant (finding #3).
///
/// args is included (Commit Q per collaborative-proposal-workflow-spec-2026-
/// 05-15.md §accept_replanning step 4) so accept_replanning can derive its
/// `triggered_by` payload field from `args.request_index` against the
/// pre-state queue. Existing actions ignore args; only the new ones read it.
fn emit_two_controls_event(
    project_dir: &str,
    actor: &str,
    action: &str,
    args: &serde_json::Value,
    pre: &serde_json::Value,
    post: &serde_json::Value,
) {
    let event_type = match action {
        "set_assembly" => "assembly_toggled",
        // accept_replanning emits phase_toggled (extended payload — see match
        // arm below for the reason+triggered_by fields). Existing v1.1
        // consumers parse phase_toggled and ignore unknown fields.
        "accept_plan" | "open_planning" | "accept_replanning" => "phase_toggled",
        "revise_plan" => "plan_revised",
        "set_mic_passing" => "mic_passing_mode_changed",
        "raise_hand" => "hand_raised",
        "grant_mic" => "mic_granted",
        "set_moderator" => "moderator_set",
        "propose_replanning" => "replanning_proposed",
        "set_review_intensity" => "review_intensity_changed",
        _ => return,
    };
    let pre_floor = pre.get("floor").cloned().unwrap_or(serde_json::Value::Null);
    let post_floor = post.get("floor").cloned().unwrap_or(serde_json::Value::Null);
    let mut payload = serde_json::Map::new();
    let pre_get = |k: &str| pre_floor.get(k).cloned().unwrap_or(serde_json::Value::Null);
    let post_get = |k: &str| post_floor.get(k).cloned().unwrap_or(serde_json::Value::Null);
    match action {
        "set_assembly" => {
            payload.insert("old".into(), pre_get("assembly_active"));
            payload.insert("new".into(), post_get("assembly_active"));
        }
        "accept_plan" | "open_planning" => {
            payload.insert("old".into(), pre_get("phase"));
            payload.insert("new".into(), post_get("phase"));
            payload.insert("plan_path".into(), post_get("plan_path"));
            payload.insert("plan_hash".into(), post_get("plan_hash"));
        }
        "revise_plan" => {
            payload.insert("old_hash".into(), pre_get("plan_hash"));
            payload.insert("new_hash".into(), post_get("plan_hash"));
            payload.insert("plan_path".into(), post_get("plan_path"));
        }
        "set_mic_passing" => {
            payload.insert("old".into(), pre_get("mic_passing_mode"));
            payload.insert("new".into(), post_get("mic_passing_mode"));
        }
        "raise_hand" => {
            payload.insert("seat".into(), serde_json::json!(actor));
            payload.insert("queue".into(), post_get("hand_queue"));
        }
        "grant_mic" => {
            payload.insert("moderator".into(), pre_get("moderator"));
            payload.insert("target".into(), post_get("current_speaker"));
        }
        "set_moderator" => {
            payload.insert("old".into(), pre_get("moderator"));
            payload.insert("new".into(), post_get("moderator"));
        }
        "propose_replanning" => {
            // Emit the just-appended request as the event payload (the LAST
            // entry of the post-state queue is what this caller pushed). The
            // outer with_file_lock guarantees no concurrent push lands
            // between apply_propose_replanning and this read.
            let last_request = post_get("replanning_requests")
                .as_array()
                .and_then(|a| a.last().cloned())
                .unwrap_or(serde_json::Value::Null);
            payload.insert("seat".into(), serde_json::json!(actor));
            payload.insert("request".into(), last_request);
            payload.insert(
                "queue_depth".into(),
                serde_json::json!(
                    post_get("replanning_requests")
                        .as_array()
                        .map(|a| a.len())
                        .unwrap_or(0)
                ),
            );
        }
        "set_review_intensity" => {
            payload.insert("old".into(), pre_get("review_intensity"));
            payload.insert("new".into(), post_get("review_intensity"));
        }
        "accept_replanning" => {
            // Extends v1.1 phase_toggled payload with reason + triggered_by
            // per spec §accept_replanning step 4. Existing v1.1 consumers
            // parsing the 4 base fields (old/new/plan_path/plan_hash) keep
            // working — the additions are post-pop optional fields.
            payload.insert("old".into(), pre_get("phase"));
            payload.insert("new".into(), post_get("phase"));
            payload.insert("plan_path".into(), post_get("plan_path"));
            payload.insert("plan_hash".into(), post_get("plan_hash"));
            payload.insert(
                "reason".into(),
                serde_json::json!(format!("replanning_accepted_by:{}", actor)),
            );
            // triggered_by — only present when args.request_index pins a
            // specific request in the pre-state queue. Absent (no insert)
            // when the moderator accepts the queue as a whole.
            if let Some(idx) = args.get("request_index").and_then(|v| v.as_u64()) {
                if let Some(req) = pre_get("replanning_requests")
                    .as_array()
                    .and_then(|a| a.get(idx as usize))
                {
                    if let Some(seat) = req.get("seat").and_then(|s| s.as_str()) {
                        payload.insert("triggered_by".into(), serde_json::json!(seat));
                    }
                }
            }
        }
        _ => return,
    }
    payload.insert("ts".into(), serde_json::json!(utc_now_iso()));

    let msg_id = next_message_id(project_dir);
    let event = serde_json::json!({
        "id": msg_id,
        "from": "system",
        "to": "all",
        "type": event_type,
        "timestamp": utc_now_iso(),
        "subject": format!("[{}] {}", event_type, actor),
        "body": format!("{} fired by {}", event_type, actor),
        "metadata": serde_json::Value::Object(payload),
    });
    let _ = append_to_board(project_dir, &event);
}

// ============================================================
// Slice 2 tests — apply_* layer (pure JSON-state mutations, no I/O)
// ============================================================
// Per dev #927 plan (a) CAS gate, (b) action smokes. (c) backward-compat,
// (d) cold-open race, and (e) invariant property test ride along after
// the legacy wrappers + integration paths are wired.

#[cfg(test)]
mod protocol_slice2_tests {
    use super::*;

    fn fresh_state() -> serde_json::Value {
        protocol_fresh_value()
    }

    /// fresh_state() returns preset="Debate" by default. The v1.0.7 cross-
    /// transition gate (vaak-mcp.rs:3974) blocks Debate → any-non-default
    /// preset in a single apply_set_preset call. Tests that need to land at
    /// AssemblyLine / Brainstorm / Delphi etc. must first transition through
    /// "Default chat". This helper does that in one step, mirroring how
    /// real callers (do_protocol_mutate) sequence the two-step transition.
    fn fresh_state_at_default_chat() -> serde_json::Value {
        let mut s = protocol_fresh_value();
        apply_set_preset(&mut s, &serde_json::json!({"name": "Default chat"})).unwrap();
        s
    }

    /// (a) CAS — apply layer accepts and bumps. (Full rev gate is exercised
    /// in handle_protocol_mutate; here we assert apply_set_preset is idempotent
    /// in input shape and writes the matrix-mapped modes.)
    #[test]
    fn apply_set_preset_assembly_line_maps_to_round_robin_none() {
        let mut s = fresh_state();
        apply_set_preset(&mut s, &serde_json::json!({"name": "Assembly Line"})).unwrap();
        assert_eq!(s["preset"], "Assembly Line");
        assert_eq!(s["floor"]["mode"], "round-robin");
        assert_eq!(s["consensus"]["mode"], "none");
    }

    #[test]
    fn apply_set_preset_continuous_review_maps_to_free_grab_tally() {
        let mut s = fresh_state();
        apply_set_preset(&mut s, &serde_json::json!({"name": "Continuous Review"})).unwrap();
        assert_eq!(s["floor"]["mode"], "free-grab");
        assert_eq!(s["consensus"]["mode"], "tally");
    }

    #[test]
    fn apply_set_preset_unknown_returns_invalid_args() {
        let mut s = fresh_state();
        let err = apply_set_preset(&mut s, &serde_json::json!({"name": "Nonexistent"}))
            .unwrap_err();
        assert!(err.starts_with("[InvalidArgs]"), "got: {}", err);
    }

    #[test]
    fn apply_set_preset_missing_name_returns_invalid_args() {
        let mut s = fresh_state();
        let err = apply_set_preset(&mut s, &serde_json::json!({})).unwrap_err();
        assert!(err.starts_with("[InvalidArgs]"), "got: {}", err);
    }

    /// Regression — human msg 261 on 2026-05-22 ("you turned off assembly
    /// mode but i still see it as on... If assembly mode is not on why
    /// would review intesnity be relevant"). Tech-leader msg 244 had
    /// disabled assembly via `protocol_mutate(set_preset, "Default chat")`
    /// directly — that wrote preset/mode/consensus but left
    /// floor.assembly_active=true, floor.current_speaker stale, and
    /// floor.moderator stale. UI rendered as "AL on" because it reads
    /// assembly_active. apply_set_preset now syncs assembly_active to the
    /// new preset's family + clears current_speaker + moderator when
    /// leaving the assembly family.
    #[test]
    fn apply_set_preset_default_chat_from_assembly_clears_derived_fields() {
        let mut s = fresh_state_at_default_chat();
        // Bring state to an assembly-active configuration first.
        apply_set_preset(&mut s, &serde_json::json!({"name": "Assembly Line"})).unwrap();
        if let Some(floor) = s.get_mut("floor").and_then(|f| f.as_object_mut()) {
            floor.insert("current_speaker".to_string(), serde_json::json!("architect:0"));
            floor.insert("moderator".to_string(), serde_json::json!("tech-leader:0"));
        }
        assert_eq!(s["floor"]["assembly_active"], true);
        assert_eq!(s["floor"]["current_speaker"], "architect:0");
        assert_eq!(s["floor"]["moderator"], "tech-leader:0");

        // Now disable via direct set_preset, not set_assembly.
        apply_set_preset(&mut s, &serde_json::json!({"name": "Default chat"})).unwrap();

        assert_eq!(s["preset"], "Default chat");
        assert_eq!(s["floor"]["mode"], "none");
        assert_eq!(s["floor"]["assembly_active"], false);
        assert!(s["floor"]["current_speaker"].is_null());
        assert!(s["floor"]["moderator"].is_null());
        // rotation_order preserved (just data; a future enable can reseed).
    }

    /// Sibling — entering the assembly family via set_preset sets
    /// assembly_active=true so apply_set_assembly's overwrite is consistent
    /// (apply_set_assembly calls apply_set_preset internally).
    #[test]
    fn apply_set_preset_assembly_line_sets_assembly_active_true() {
        let mut s = fresh_state_at_default_chat();
        apply_set_preset(&mut s, &serde_json::json!({"name": "Assembly Line"})).unwrap();
        assert_eq!(s["floor"]["assembly_active"], true);
    }

    /// Regression — human msg 23 on 2026-05-22: enabling Assembly Line via
    /// `set_preset` left `floor.rotation_order = []`, so the UI had no
    /// structure to render and `al_auto_grab` chose seats without a canonical
    /// order. seed_rotation_order_if_empty restores the lost seed-on-enable
    /// behavior the Slice 6 deprecation refactor dropped.
    #[test]
    fn seed_rotation_order_seeds_when_empty_and_round_robin() {
        let mut s = fresh_state_at_default_chat();
        apply_set_preset(&mut s, &serde_json::json!({"name": "Assembly Line"})).unwrap();
        let seats: std::collections::HashSet<String> = [
            "architect:0".to_string(),
            "developer:1".to_string(),
            "tester:0".to_string(),
        ]
        .into_iter()
        .collect();
        seed_rotation_order_if_empty(&mut s, &seats);
        let order: Vec<String> = s["floor"]["rotation_order"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        assert_eq!(order, vec!["architect:0", "developer:1", "tester:0"]);
        assert_eq!(s["floor"]["current_speaker"], "architect:0");
    }

    /// Conservative — never overwrite an explicit non-empty rotation_order.
    /// Moderator-set order survives a subsequent set_preset re-invocation.
    #[test]
    fn seed_rotation_order_preserves_explicit_order() {
        let mut s = fresh_state_at_default_chat();
        apply_set_preset(&mut s, &serde_json::json!({"name": "Assembly Line"})).unwrap();
        s["floor"]["rotation_order"] = serde_json::json!(["existing:0", "other:0"]);
        s["floor"]["current_speaker"] = serde_json::json!("existing:0");
        let seats: std::collections::HashSet<String> = ["unrelated:0".to_string()]
            .into_iter()
            .collect();
        seed_rotation_order_if_empty(&mut s, &seats);
        let order: Vec<String> = s["floor"]["rotation_order"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        assert_eq!(order, vec!["existing:0", "other:0"]);
        assert_eq!(s["floor"]["current_speaker"], "existing:0");
    }

    /// Scoped — only round-robin floors (AssemblyLine, Delphi) own rotation_order
    /// semantically. Brainstorm (free-grab) must not get seeded with stale seats.
    #[test]
    fn seed_rotation_order_skips_non_round_robin_floors() {
        let mut s = fresh_state_at_default_chat();
        apply_set_preset(&mut s, &serde_json::json!({"name": "Brainstorm"})).unwrap();
        let seats: std::collections::HashSet<String> = ["a:0".to_string()].into_iter().collect();
        seed_rotation_order_if_empty(&mut s, &seats);
        assert!(s["floor"]["rotation_order"].as_array().unwrap().is_empty());
        assert!(s["floor"]["current_speaker"].is_null());
    }

    /// Empty active_seats is a no-op (degenerate seeding would write `[]`
    /// over `[]` and could orphan-clear current_speaker on subsequent
    /// normalize calls — keep this branch silent).
    #[test]
    fn seed_rotation_order_no_op_on_empty_active_seats() {
        let mut s = fresh_state_at_default_chat();
        apply_set_preset(&mut s, &serde_json::json!({"name": "Assembly Line"})).unwrap();
        let seats: std::collections::HashSet<String> = std::collections::HashSet::new();
        seed_rotation_order_if_empty(&mut s, &seats);
        assert!(s["floor"]["rotation_order"].as_array().unwrap().is_empty());
    }

    /// Fix-A1 audit follow-up — `apply_set_assembly(active=true)` wraps
    /// `apply_set_preset(AssemblyLine)` internally (vaak-mcp.rs:4941). The
    /// dispatch hook in `do_protocol_mutate` must therefore fire the seeder
    /// for `action == "set_assembly"` too, not just `"set_preset"`. This
    /// test asserts the helper produces the correct outcome on the state
    /// `apply_set_assembly` leaves behind — and stands as the unit-level
    /// witness that the dispatch hook's guard is broad enough.
    #[test]
    fn seed_rotation_order_seeds_after_apply_set_assembly_active() {
        let mut s = fresh_state_at_default_chat();
        apply_set_assembly(&mut s, &serde_json::json!({"active": true})).unwrap();
        assert_eq!(s["preset"], "Assembly Line");
        assert_eq!(s["floor"]["mode"], "round-robin");
        assert_eq!(s["floor"]["assembly_active"], true);
        let seats: std::collections::HashSet<String> = [
            "architect:0".to_string(),
            "developer:1".to_string(),
        ]
        .into_iter()
        .collect();
        seed_rotation_order_if_empty(&mut s, &seats);
        let order: Vec<String> = s["floor"]["rotation_order"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        assert_eq!(order, vec!["architect:0", "developer:1"]);
        assert_eq!(s["floor"]["current_speaker"], "architect:0");
    }

    /// Commit C (2026-05-24) — handle_project_status defensive heal regression.
    /// Pins the heal's preconditions + helper invocation. Simulates a section
    /// that was persisted with assembly preset on but rotation_order cleared
    /// (the bug per tester msg 34 §"Acceptance criteria for any fix" #1).
    /// Mirrors the heal block at handle_project_status: predicate fires, helper
    /// seeds rotation_order from N active seats, current_speaker is anchored to
    /// the first seat. Regresses if either the predicate name (preset / mode /
    /// rotation_order key) or the helper's behavior drifts.
    #[test]
    fn commit_c_project_status_heal_seeds_when_assembly_on_and_order_empty() {
        let mut proto = fresh_state_at_default_chat();
        // Bring the section to the bug state: preset=Assembly Line,
        // mode=round-robin, but rotation_order=[] (post-enable, pre-seed
        // fixture — what a section persisted from a prior session looks
        // like at first read).
        apply_set_preset(&mut proto, &serde_json::json!({"name": "Assembly Line"})).unwrap();
        proto["floor"]["rotation_order"] = serde_json::json!([]);
        proto["floor"]["current_speaker"] = serde_json::Value::Null;

        // Mirror the heal's predicate exactly (PRESET_ASSEMBLY_LINE constant +
        // rotation_order array emptiness). If either side renames or moves,
        // this test will catch the drift before runtime does.
        let assembly_on =
            proto.get("preset").and_then(|p| p.as_str()) == Some(PRESET_ASSEMBLY_LINE);
        let order_empty = proto["floor"]["rotation_order"]
            .as_array()
            .map(|a| a.is_empty())
            .unwrap_or(true);
        assert!(assembly_on, "preset wire string must match PRESET_ASSEMBLY_LINE");
        assert!(order_empty, "fixture must start with empty rotation_order");

        let seats: std::collections::HashSet<String> = [
            "architect:0".to_string(),
            "developer:1".to_string(),
            "tester:0".to_string(),
        ]
        .into_iter()
        .collect();

        if assembly_on && order_empty {
            seed_rotation_order_if_empty(&mut proto, &seats);
        }

        let order: Vec<String> = proto["floor"]["rotation_order"]
            .as_array()
            .expect("rotation_order should be an array after heal")
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        assert_eq!(order, vec!["architect:0", "developer:1", "tester:0"]);
        assert_eq!(proto["floor"]["current_speaker"], "architect:0");
    }

    /// Commit C — heal predicate must NOT fire when rotation_order is already
    /// non-empty (idempotency / no-clobber). A heal that ran every project_status
    /// call and overwrote an explicit moderator-set order would regress
    /// `apply_set_rotation_order`'s contract.
    #[test]
    fn commit_c_project_status_heal_skips_when_order_non_empty() {
        let mut proto = fresh_state_at_default_chat();
        apply_set_preset(&mut proto, &serde_json::json!({"name": "Assembly Line"})).unwrap();
        // Explicit moderator-set order — heal must leave it alone.
        proto["floor"]["rotation_order"] =
            serde_json::json!(["explicit:0", "moderator-set:1"]);
        proto["floor"]["current_speaker"] = serde_json::json!("explicit:0");

        let order_empty = proto["floor"]["rotation_order"]
            .as_array()
            .map(|a| a.is_empty())
            .unwrap_or(true);
        assert!(!order_empty, "predicate must read order_empty=false here");

        let unrelated_seats: std::collections::HashSet<String> =
            ["unrelated:7".to_string()].into_iter().collect();
        if order_empty {
            seed_rotation_order_if_empty(&mut proto, &unrelated_seats);
        }

        let order: Vec<String> = proto["floor"]["rotation_order"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        assert_eq!(order, vec!["explicit:0", "moderator-set:1"]);
        assert_eq!(proto["floor"]["current_speaker"], "explicit:0");
    }

    // ===================================================================
    // Fix-A2 — apply_set_rotation_order tests per spec at
    // .vaak/design-notes/fix-a2-set-rotation-order-spec-2026-05-22.md
    // §Test surface. Tests use a temporary project directory so the
    // active_seats read (under-lock per dev-challenger:0 msg 129 flag #6)
    // resolves against a controlled sessions.json fixture, NOT the live
    // .vaak/sessions.json. The fixture writes 3 seats: architect:0,
    // developer:1, tester:0 — same shape as today's roster but small
    // enough to make assertions readable.
    // ===================================================================

    /// Build a temp dir with `.vaak/sessions.json` containing the named
    /// active bindings. Returns the `tempfile::TempDir` guard so the caller
    /// can pass it to apply_set_rotation_order; drop cleans up.
    ///
    /// Per evil-arch msg 164 Flag 2 + tech-leader msg 183 ruling: migrated
    /// off the prior `std::env::temp_dir() + per-test-name-suffix` pattern
    /// (which had no compile-time guard against name collisions across
    /// parallel `cargo test` workers). `tempfile::tempdir()` guarantees a
    /// unique per-call directory with RAII cleanup.
    fn temp_project_with_seats(seats: &[(&str, u64)]) -> tempfile::TempDir {
        let td = tempfile::tempdir().expect("tempdir");
        let vaak = td.path().join(".vaak");
        std::fs::create_dir_all(&vaak).unwrap();
        let bindings: Vec<serde_json::Value> = seats
            .iter()
            .map(|(role, inst)| {
                serde_json::json!({
                    "role": role,
                    "instance": inst,
                    "session_id": format!("test-{}-{}", role, inst),
                    "status": "active",
                    "last_heartbeat": "2026-05-22T17:00:00Z"
                })
            })
            .collect();
        let sessions = serde_json::json!({ "bindings": bindings });
        std::fs::write(
            vaak.join("sessions.json"),
            serde_json::to_string_pretty(&sessions).unwrap(),
        )
        .unwrap();
        td
    }

    fn three_seat_fixture() -> tempfile::TempDir {
        temp_project_with_seats(&[("architect", 0), ("developer", 1), ("tester", 0)])
    }

    fn assembly_state_with_moderator(moderator: &str, rotation: Vec<&str>) -> serde_json::Value {
        let mut s = fresh_state_at_default_chat();
        apply_set_preset(&mut s, &serde_json::json!({"name": "Assembly Line"})).unwrap();
        if let Some(floor) = s.get_mut("floor").and_then(|f| f.as_object_mut()) {
            floor.insert(
                "moderator".to_string(),
                serde_json::json!(moderator),
            );
            let rot: Vec<serde_json::Value> =
                rotation.iter().map(|x| serde_json::json!(x)).collect();
            floor.insert(
                "rotation_order".to_string(),
                serde_json::Value::Array(rot),
            );
        }
        s
    }

    #[test]
    fn set_rotation_order_replaces_array_under_cas() {
        let td = three_seat_fixture();
        let mut s = assembly_state_with_moderator(
            "tech-leader:0",
            vec!["architect:0", "developer:1", "tester:0"],
        );
        apply_set_rotation_order(
            &mut s,
            &serde_json::json!({"rotation_order": ["tester:0", "architect:0", "developer:1"]}),
            "tech-leader:0",
            td.path().to_str().unwrap(),
        )
        .unwrap();
        let order: Vec<String> = s["floor"]["rotation_order"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        assert_eq!(order, vec!["tester:0", "architect:0", "developer:1"]);
        // Sibling fields preserved
        assert_eq!(s["floor"]["moderator"], "tech-leader:0");
        assert_eq!(s["floor"]["mode"], "round-robin");
    }

    #[test]
    fn set_rotation_order_rejects_unauthorized_caller() {
        let td = three_seat_fixture();
        let mut s = assembly_state_with_moderator(
            "tech-leader:0",
            vec!["architect:0", "developer:1", "tester:0"],
        );
        let err = apply_set_rotation_order(
            &mut s,
            &serde_json::json!({"rotation_order": ["tester:0", "architect:0", "developer:1"]}),
            "developer:1",
            td.path().to_str().unwrap(),
        )
        .unwrap_err();
        assert!(
            err.starts_with("[Unauthorized]"),
            "expected [Unauthorized], got: {}",
            err
        );
    }

    #[test]
    fn set_rotation_order_accepts_human_caller() {
        let td = three_seat_fixture();
        let mut s = assembly_state_with_moderator(
            "tech-leader:0",
            vec!["architect:0", "developer:1", "tester:0"],
        );
        apply_set_rotation_order(
            &mut s,
            &serde_json::json!({"rotation_order": ["tester:0", "architect:0", "developer:1"]}),
            "human",
            td.path().to_str().unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn set_rotation_order_accepts_tech_leader_caller() {
        let td = three_seat_fixture();
        let mut s = assembly_state_with_moderator(
            "architect:0",
            vec!["architect:0", "developer:1", "tester:0"],
        );
        apply_set_rotation_order(
            &mut s,
            &serde_json::json!({"rotation_order": ["tester:0", "architect:0", "developer:1"]}),
            "tech-leader:0",
            td.path().to_str().unwrap(),
        )
        .unwrap();
    }

    /// Per evil-architect:0 msg 148 Flag 4 + msg 164 Flag 1 — Fix-A2
    /// §Authorization gates on `floor.moderator == actor` independent of role
    /// slug. A non-privileged role (e.g. developer) that has been set as
    /// floor.moderator must pass the gate. Regression bar for the moderator-
    /// bypass authorization path.
    #[test]
    fn set_rotation_order_accepts_caller_matching_floor_moderator() {
        let td = three_seat_fixture();
        // Set floor.moderator = developer:1 (NOT in the privileged role slugs
        // human/manager/tech-leader). Authorization must still accept this
        // caller via the moderator-equality branch.
        let mut s = assembly_state_with_moderator(
            "developer:1",
            vec!["architect:0", "developer:1", "tester:0"],
        );
        apply_set_rotation_order(
            &mut s,
            &serde_json::json!({"rotation_order": ["tester:0", "architect:0", "developer:1"]}),
            "developer:1",
            td.path().to_str().unwrap(),
        )
        .unwrap();
        let order: Vec<String> = s["floor"]["rotation_order"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        assert_eq!(order, vec!["tester:0", "architect:0", "developer:1"]);
    }

    #[test]
    fn set_rotation_order_rejects_duplicates_in_args() {
        let td = three_seat_fixture();
        let mut s = assembly_state_with_moderator(
            "tech-leader:0",
            vec!["architect:0", "developer:1", "tester:0"],
        );
        let err = apply_set_rotation_order(
            &mut s,
            &serde_json::json!({"rotation_order": ["architect:0", "developer:1", "architect:0"]}),
            "human",
            td.path().to_str().unwrap(),
        )
        .unwrap_err();
        assert!(err.starts_with("[InvalidArgs]"), "got: {}", err);
        assert!(err.contains("duplicate"), "got: {}", err);
    }

    #[test]
    fn set_rotation_order_rejects_invalid_seat_format() {
        let td = three_seat_fixture();
        let mut s = assembly_state_with_moderator(
            "tech-leader:0",
            vec!["architect:0", "developer:1", "tester:0"],
        );
        let err = apply_set_rotation_order(
            &mut s,
            &serde_json::json!({"rotation_order": ["Architect:0", "developer:1", "tester:0"]}),
            "human",
            td.path().to_str().unwrap(),
        )
        .unwrap_err();
        assert!(err.starts_with("[InvalidArgs]"), "got: {}", err);
        assert!(
            err.contains("role:instance form"),
            "expected shape error, got: {}",
            err
        );
    }

    #[test]
    fn set_rotation_order_rejects_non_active_seat() {
        let td = three_seat_fixture();
        let mut s = assembly_state_with_moderator(
            "tech-leader:0",
            vec!["architect:0", "developer:1", "tester:0"],
        );
        let err = apply_set_rotation_order(
            &mut s,
            &serde_json::json!({"rotation_order": ["architect:0", "developer:1", "ghost:99"]}),
            "human",
            td.path().to_str().unwrap(),
        )
        .unwrap_err();
        assert!(err.starts_with("[InvalidArgs]"), "got: {}", err);
        assert!(err.contains("not an active seat"), "got: {}", err);
    }

    #[test]
    fn set_rotation_order_rejects_empty_array() {
        let td = three_seat_fixture();
        let mut s = assembly_state_with_moderator(
            "tech-leader:0",
            vec!["architect:0", "developer:1", "tester:0"],
        );
        // Per spec §5 reorder-only ruling + rejection case #1 (corrected from
        // prior false-permit test name per dev-challenger:0 msg 129 flag #2):
        // empty array with active seats present must reject as proper-subset.
        let err = apply_set_rotation_order(
            &mut s,
            &serde_json::json!({"rotation_order": []}),
            "human",
            td.path().to_str().unwrap(),
        )
        .unwrap_err();
        assert!(err.starts_with("[InvalidArgs]"), "got: {}", err);
        assert!(err.contains("reorder-only"), "got: {}", err);
    }

    #[test]
    fn set_rotation_order_rejects_proper_subset() {
        let td = three_seat_fixture();
        let mut s = assembly_state_with_moderator(
            "tech-leader:0",
            vec!["architect:0", "developer:1", "tester:0"],
        );
        // Missing tester:0 from the array → proper subset → reject + name the
        // missing seats. Per dev-challenger:0 msg 129 flag #3 + tester:0
        // msg 123 explicit rejection case #4.
        let err = apply_set_rotation_order(
            &mut s,
            &serde_json::json!({"rotation_order": ["architect:0", "developer:1"]}),
            "human",
            td.path().to_str().unwrap(),
        )
        .unwrap_err();
        assert!(err.starts_with("[InvalidArgs]"), "got: {}", err);
        assert!(err.contains("reorder-only"), "got: {}", err);
        assert!(
            err.contains("tester:0"),
            "expected missing seat name in error, got: {}",
            err
        );
    }

    #[test]
    fn set_rotation_order_rejects_missing_args() {
        let td = three_seat_fixture();
        let mut s = assembly_state_with_moderator(
            "tech-leader:0",
            vec!["architect:0", "developer:1", "tester:0"],
        );
        let err = apply_set_rotation_order(
            &mut s,
            &serde_json::json!({}),
            "human",
            td.path().to_str().unwrap(),
        )
        .unwrap_err();
        assert!(err.starts_with("[InvalidArgs]"), "got: {}", err);
        assert!(err.contains("requires args.rotation_order"), "got: {}", err);
    }

    #[test]
    fn is_valid_seat_label_accepts_canonical_forms() {
        assert!(is_valid_seat_label("architect:0"));
        assert!(is_valid_seat_label("developer:1"));
        assert!(is_valid_seat_label("ui-architect:1"));
        assert!(is_valid_seat_label("dev-challenger:0"));
        assert!(is_valid_seat_label("med-presenter:42"));
    }

    #[test]
    fn is_valid_seat_label_rejects_malformed() {
        assert!(!is_valid_seat_label(""));
        assert!(!is_valid_seat_label("architect"));
        assert!(!is_valid_seat_label(":0"));
        assert!(!is_valid_seat_label("architect:"));
        assert!(!is_valid_seat_label("Architect:0"));         // uppercase
        assert!(!is_valid_seat_label("architect:abc"));       // non-digit instance
        assert!(!is_valid_seat_label("architect:0:extra"));   // extra colon — splitn(2) keeps it in instance, which fails digit check
        assert!(!is_valid_seat_label("arch_itect:0"));        // underscore not allowed
        assert!(!is_valid_seat_label("arch itect:0"));        // space not allowed
        assert!(!is_valid_seat_label("human"));               // human seat is its own special case, not a role:instance form
    }

    /// Sibling — `apply_set_assembly(active=false)` routes through
    /// `apply_set_preset(DefaultChat)` → mode=none. The seeder must be a
    /// no-op on the resulting state regardless of whether the dispatcher
    /// invokes it (defensive: the guard fires; the helper short-circuits
    /// on non-round-robin floor).
    #[test]
    fn seed_rotation_order_no_op_after_apply_set_assembly_inactive() {
        let mut s = fresh_state();
        apply_set_assembly(&mut s, &serde_json::json!({"active": false})).unwrap();
        assert_eq!(s["preset"], "Default chat");
        assert_eq!(s["floor"]["mode"], "none");
        assert_eq!(s["floor"]["assembly_active"], false);
        let seats: std::collections::HashSet<String> = [
            "architect:0".to_string(),
            "developer:1".to_string(),
        ]
        .into_iter()
        .collect();
        seed_rotation_order_if_empty(&mut s, &seats);
        assert!(s["floor"]["rotation_order"].as_array().unwrap().is_empty());
    }

    /// (b) yield — non-speaker fails NotPermitted; speaker yielding to None
    /// with empty queue clears current_speaker.
    #[test]
    fn apply_yield_by_non_speaker_fails_not_permitted() {
        let mut s = fresh_state();
        s["floor"]["current_speaker"] = serde_json::json!("architect:0");
        let err = apply_yield(&mut s, &serde_json::json!({}), "developer:0").unwrap_err();
        assert!(err.starts_with("[NotPermitted]"), "got: {}", err);
    }

    #[test]
    fn apply_yield_speaker_no_target_no_queue_clears_speaker() {
        let mut s = fresh_state();
        s["floor"]["current_speaker"] = serde_json::json!("architect:0");
        apply_yield(&mut s, &serde_json::json!({}), "architect:0").unwrap();
        assert!(s["floor"]["current_speaker"].is_null());
    }

    #[test]
    fn apply_yield_speaker_pops_queue_head() {
        let mut s = fresh_state();
        s["floor"]["current_speaker"] = serde_json::json!("architect:0");
        s["floor"]["queue"] = serde_json::json!(["developer:0", "tester:0"]);
        apply_yield(&mut s, &serde_json::json!({}), "architect:0").unwrap();
        assert_eq!(s["floor"]["current_speaker"], "developer:0");
        // popped head should be removed from queue
        assert_eq!(s["floor"]["queue"], serde_json::json!(["tester:0"]));
    }

    /// (b) toggle_queue — self-only; toggles in then out.
    #[test]
    fn apply_toggle_queue_other_seat_fails_not_permitted() {
        let mut s = fresh_state();
        let err = apply_toggle_queue(
            &mut s,
            &serde_json::json!({"seat": "architect:0"}),
            "developer:0",
        )
        .unwrap_err();
        assert!(err.starts_with("[NotPermitted]"), "got: {}", err);
    }

    #[test]
    fn apply_toggle_queue_self_adds_then_removes() {
        let mut s = fresh_state();
        apply_toggle_queue(&mut s, &serde_json::json!({}), "developer:0").unwrap();
        assert_eq!(s["floor"]["queue"], serde_json::json!(["developer:0"]));
        apply_toggle_queue(&mut s, &serde_json::json!({}), "developer:0").unwrap();
        assert_eq!(s["floor"]["queue"], serde_json::json!([]));
    }

    /// (b) transfer_mic — caller==current_speaker fails (yield is the path).
    #[test]
    fn apply_transfer_mic_self_fails_not_permitted() {
        let mut s = fresh_state();
        s["floor"]["current_speaker"] = serde_json::json!("architect:0");
        // We don't reach the project_dir-dependent branch because the
        // self-target check fires first. project_dir doesn't matter here.
        let err = apply_transfer_mic(
            &mut s,
            &serde_json::json!({"target": "developer:0"}),
            "architect:0",
            "/nonexistent",
        )
        .unwrap_err();
        assert!(err.starts_with("[NotPermitted]"), "got: {}", err);
    }

    /// transfer_mic missing args.target → InvalidArgs (early return before
    /// any project_dir read).
    #[test]
    fn apply_transfer_mic_missing_target_invalid_args() {
        let mut s = fresh_state();
        let err = apply_transfer_mic(&mut s, &serde_json::json!({}), "architect:0", "/none")
            .unwrap_err();
        assert!(err.starts_with("[InvalidArgs]"), "got: {}", err);
    }

    // ============================================================
    // §10 atomicity behavioral tests (dev-chall #940.4 + tech-leader #941.5)
    // Run against apply_protocol_mic_to_transfer directly — the function the
    // project_send hook delegates to. Doesn't require get_or_rejoin_state
    // since this layer is pure (project_dir, section, from_label, target).
    // ============================================================

    fn temp_project_with_protocol(
        test_name: &str,
        protocol: serde_json::Value,
        sessions: serde_json::Value,
    ) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("vaak-mcp-protocol-{}", test_name));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".vaak")).unwrap();
        std::fs::write(
            dir.join(".vaak").join("protocol.json"),
            serde_json::to_string_pretty(&protocol).unwrap(),
        )
        .unwrap();
        std::fs::write(
            dir.join(".vaak").join("sessions.json"),
            serde_json::to_string_pretty(&sessions).unwrap(),
        )
        .unwrap();
        dir
    }

    fn read_protocol_back(dir: &std::path::Path) -> serde_json::Value {
        let s = std::fs::read_to_string(dir.join(".vaak").join("protocol.json")).unwrap();
        serde_json::from_str(&s).unwrap()
    }

    /// (d) §10 atomicity happy path: transfer commits → protocol.rev bumps,
    /// current_speaker updates, audit fields stamped. Same-lock-window
    /// invariant: this function is what runs INSIDE project_send's lock.
    #[test]
    fn protocol_mic_to_transfer_happy_path() {
        let dir = temp_project_with_protocol(
            "mic-to-happy",
            serde_json::json!({
                "schema_version": 1, "rev": 5, "preset": "Debate",
                "floor": {"mode": "reactive", "current_speaker": "architect:0", "queue": [], "rotation_order": [], "threshold_ms": 60000, "started_at": null},
                "consensus": {"mode": "none", "round": null, "phase": null, "submissions": []},
                "phase_plan": {"phases": [], "current_phase_idx": 0, "paused_at": null, "paused_total_secs": 0},
                "scopes": {"floor": "instance", "consensus": "role"},
                "last_writer_seat": null, "last_writer_action": null, "rev_at": null
            }),
            serde_json::json!({
                "bindings": [
                    {"role": "architect", "instance": 0, "status": "active", "last_active_at_ms": 9999999999u64, "last_drafting_at_ms": 0},
                    {"role": "developer", "instance": 0, "status": "active", "last_active_at_ms": 9999999999u64, "last_drafting_at_ms": 0}
                ]
            }),
        );

        apply_protocol_mic_to_transfer(
            dir.to_str().unwrap(),
            "default",
            "architect:0",
            "developer:0",
        )
        .unwrap();

        let after = read_protocol_back(&dir);
        assert_eq!(after["rev"], 6);
        assert_eq!(after["floor"]["current_speaker"], "developer:0");
        assert_eq!(after["last_writer_seat"], "architect:0");
        assert_eq!(after["last_writer_action"], "project_send_mic_to");
    }

    /// Vacant target → fall through to queue head (spec §2.2 row 2 +
    /// dev-chall #940.5).
    #[test]
    fn protocol_mic_to_vacant_falls_through_to_queue_head() {
        let dir = temp_project_with_protocol(
            "mic-to-vacant-queue",
            serde_json::json!({
                "schema_version": 1, "rev": 1, "preset": "Debate",
                "floor": {"mode": "reactive", "current_speaker": "architect:0", "queue": ["tester:0", "developer:0"], "rotation_order": [], "threshold_ms": 60000, "started_at": null},
                "consensus": {"mode": "none", "round": null, "phase": null, "submissions": []},
                "phase_plan": {"phases": [], "current_phase_idx": 0, "paused_at": null, "paused_total_secs": 0},
                "scopes": {"floor": "instance", "consensus": "role"},
                "last_writer_seat": null, "last_writer_action": null, "rev_at": null
            }),
            serde_json::json!({
                "bindings": [
                    {"role": "architect", "instance": 0, "status": "active"},
                    {"role": "tester", "instance": 0, "status": "active"},
                    {"role": "developer", "instance": 0, "status": "active"}
                    // manager is NOT in bindings — vacant
                ]
            }),
        );

        // Speaker addresses vacant manager:0 — should fall through to queue head tester:0.
        apply_protocol_mic_to_transfer(
            dir.to_str().unwrap(),
            "default",
            "architect:0",
            "manager:0",
        )
        .unwrap();

        let after = read_protocol_back(&dir);
        assert_eq!(after["floor"]["current_speaker"], "tester:0");
        assert_eq!(after["last_writer_action"], "project_send_mic_to_fallthrough_queue_head");
        // Queue head consumed.
        assert_eq!(after["floor"]["queue"], serde_json::json!(["developer:0"]));
    }

    /// Vacant target + empty queue → floor goes idle (current_speaker = null).
    #[test]
    fn protocol_mic_to_vacant_empty_queue_falls_through_to_idle() {
        let dir = temp_project_with_protocol(
            "mic-to-vacant-idle",
            serde_json::json!({
                "schema_version": 1, "rev": 0, "preset": "Debate",
                "floor": {"mode": "reactive", "current_speaker": "architect:0", "queue": [], "rotation_order": [], "threshold_ms": 60000, "started_at": null},
                "consensus": {"mode": "none", "round": null, "phase": null, "submissions": []},
                "phase_plan": {"phases": [], "current_phase_idx": 0, "paused_at": null, "paused_total_secs": 0},
                "scopes": {"floor": "instance", "consensus": "role"},
                "last_writer_seat": null, "last_writer_action": null, "rev_at": null
            }),
            serde_json::json!({
                "bindings": [{"role": "architect", "instance": 0, "status": "active"}]
            }),
        );

        apply_protocol_mic_to_transfer(
            dir.to_str().unwrap(),
            "default",
            "architect:0",
            "manager:0",
        )
        .unwrap();

        let after = read_protocol_back(&dir);
        assert!(after["floor"]["current_speaker"].is_null());
        assert_eq!(after["last_writer_action"], "project_send_mic_to_fallthrough_idle");
    }

    /// Mode `none` blocks transfer — silent success, floor stays.
    #[test]
    fn protocol_mic_to_mode_none_does_not_transfer() {
        let dir = temp_project_with_protocol(
            "mic-to-mode-none",
            serde_json::json!({
                "schema_version": 1, "rev": 0, "preset": "Default chat",
                "floor": {"mode": "none", "current_speaker": "architect:0", "queue": [], "rotation_order": [], "threshold_ms": 60000, "started_at": null},
                "consensus": {"mode": "none", "round": null, "phase": null, "submissions": []},
                "phase_plan": {"phases": [], "current_phase_idx": 0, "paused_at": null, "paused_total_secs": 0},
                "scopes": {"floor": "instance", "consensus": "role"},
                "last_writer_seat": null, "last_writer_action": null, "rev_at": null
            }),
            serde_json::json!({
                "bindings": [
                    {"role": "architect", "instance": 0, "status": "active"},
                    {"role": "developer", "instance": 0, "status": "active"}
                ]
            }),
        );

        apply_protocol_mic_to_transfer(
            dir.to_str().unwrap(),
            "default",
            "architect:0",
            "developer:0",
        )
        .unwrap();

        let after = read_protocol_back(&dir);
        // Rev should NOT bump — floor mode rejects transfers.
        assert_eq!(after["rev"], 0);
        assert_eq!(after["floor"]["current_speaker"], "architect:0");
    }

    /// Caller not current_speaker → silent success, no transfer (auth gate).
    #[test]
    fn protocol_mic_to_unauthorized_caller_no_transfer() {
        let dir = temp_project_with_protocol(
            "mic-to-unauth",
            serde_json::json!({
                "schema_version": 1, "rev": 3, "preset": "Debate",
                "floor": {"mode": "reactive", "current_speaker": "architect:0", "queue": [], "rotation_order": [], "threshold_ms": 60000, "started_at": null},
                "consensus": {"mode": "none", "round": null, "phase": null, "submissions": []},
                "phase_plan": {"phases": [], "current_phase_idx": 0, "paused_at": null, "paused_total_secs": 0},
                "scopes": {"floor": "instance", "consensus": "role"},
                "last_writer_seat": null, "last_writer_action": null, "rev_at": null
            }),
            serde_json::json!({
                "bindings": [
                    {"role": "architect", "instance": 0, "status": "active"},
                    {"role": "tester", "instance": 0, "status": "active"},
                    {"role": "developer", "instance": 0, "status": "active"}
                ]
            }),
        );

        // tester:0 is NOT current_speaker (architect:0 is). Should silently no-op.
        apply_protocol_mic_to_transfer(
            dir.to_str().unwrap(),
            "default",
            "tester:0",
            "developer:0",
        )
        .unwrap();

        let after = read_protocol_back(&dir);
        assert_eq!(after["rev"], 3);
        assert_eq!(after["floor"]["current_speaker"], "architect:0");
    }

    /// Cold-open IDLE: caller authorized when current_speaker is null.
    #[test]
    fn protocol_mic_to_cold_open_first_sender_authorized() {
        let dir = temp_project_with_protocol(
            "mic-to-cold-open",
            serde_json::json!({
                "schema_version": 1, "rev": 0, "preset": "Debate",
                "floor": {"mode": "reactive", "current_speaker": null, "queue": [], "rotation_order": [], "threshold_ms": 60000, "started_at": null},
                "consensus": {"mode": "none", "round": null, "phase": null, "submissions": []},
                "phase_plan": {"phases": [], "current_phase_idx": 0, "paused_at": null, "paused_total_secs": 0},
                "scopes": {"floor": "instance", "consensus": "role"},
                "last_writer_seat": null, "last_writer_action": null, "rev_at": null
            }),
            serde_json::json!({
                "bindings": [
                    {"role": "architect", "instance": 0, "status": "active"},
                    {"role": "developer", "instance": 0, "status": "active"}
                ]
            }),
        );

        apply_protocol_mic_to_transfer(
            dir.to_str().unwrap(),
            "default",
            "architect:0",
            "developer:0",
        )
        .unwrap();

        let after = read_protocol_back(&dir);
        assert_eq!(after["rev"], 1);
        assert_eq!(after["floor"]["current_speaker"], "developer:0");
    }

    // ============================================================
    // CAS-gate behavioral tests — close tech-leader #941.5 +
    // dev-chall #940.4 coverage gap. Run against `do_protocol_mutate`
    // (the inner of `handle_protocol_mutate`) so we don't need to
    // mock `get_or_rejoin_state`.
    // ============================================================

    /// (a) Stale rev → [StaleRev] error, no disk write (rev unchanged).
    #[test]
    fn cas_stale_rev_returns_error_no_write() {
        let dir = temp_project_with_protocol(
            "cas-stale",
            serde_json::json!({
                "schema_version": 1, "rev": 5, "preset": "Debate",
                "floor": {"mode": "reactive", "current_speaker": null, "queue": [], "rotation_order": [], "threshold_ms": 60000, "started_at": null},
                "consensus": {"mode": "none", "round": null, "phase": null, "submissions": []},
                "phase_plan": {"phases": [], "current_phase_idx": 0, "paused_at": null, "paused_total_secs": 0},
                "scopes": {"floor": "instance", "consensus": "role"},
                "last_writer_seat": null, "last_writer_action": null, "rev_at": null
            }),
            serde_json::json!({"bindings": []}),
        );
        let err = do_protocol_mutate(
            dir.to_str().unwrap(),
            "developer:0",
            "default",
            "toggle_queue",
            serde_json::json!({}),
            Some(0), // wrong rev — current is 5
        )
        .unwrap_err();
        assert!(err.starts_with("[StaleRev]"), "got: {}", err);
        // Disk state unchanged.
        let after = read_protocol_back(&dir);
        assert_eq!(after["rev"], 5);
    }

    /// (a) Missing rev → [MissingRev] error.
    #[test]
    fn cas_missing_rev_returns_error() {
        let dir = temp_project_with_protocol(
            "cas-missing",
            serde_json::json!({
                "schema_version": 1, "rev": 0, "preset": "Debate",
                "floor": {"mode": "reactive", "current_speaker": null, "queue": [], "rotation_order": [], "threshold_ms": 60000, "started_at": null},
                "consensus": {"mode": "none", "round": null, "phase": null, "submissions": []},
                "phase_plan": {"phases": [], "current_phase_idx": 0, "paused_at": null, "paused_total_secs": 0},
                "scopes": {"floor": "instance", "consensus": "role"},
                "last_writer_seat": null, "last_writer_action": null, "rev_at": null
            }),
            serde_json::json!({"bindings": []}),
        );
        let err = do_protocol_mutate(
            dir.to_str().unwrap(),
            "developer:0",
            "default",
            "toggle_queue",
            serde_json::json!({}),
            None,
        )
        .unwrap_err();
        assert!(err.starts_with("[MissingRev]"), "got: {}", err);
    }

    /// (a) Happy path: rev increments, last_writer_* stamped.
    #[test]
    fn cas_happy_path_increments_rev_and_stamps_audit() {
        let dir = temp_project_with_protocol(
            "cas-happy",
            serde_json::json!({
                "schema_version": 1, "rev": 7, "preset": "Debate",
                "floor": {"mode": "reactive", "current_speaker": null, "queue": [], "rotation_order": [], "threshold_ms": 60000, "started_at": null},
                "consensus": {"mode": "none", "round": null, "phase": null, "submissions": []},
                "phase_plan": {"phases": [], "current_phase_idx": 0, "paused_at": null, "paused_total_secs": 0},
                "scopes": {"floor": "instance", "consensus": "role"},
                "last_writer_seat": null, "last_writer_action": null, "rev_at": null
            }),
            serde_json::json!({"bindings": []}),
        );
        do_protocol_mutate(
            dir.to_str().unwrap(),
            "developer:0",
            "default",
            "toggle_queue",
            serde_json::json!({}),
            Some(7),
        )
        .unwrap();
        let after = read_protocol_back(&dir);
        assert_eq!(after["rev"], 8);
        assert_eq!(after["last_writer_seat"], "developer:0");
        assert_eq!(after["last_writer_action"], "toggle_queue");
        assert_eq!(after["floor"]["queue"], serde_json::json!(["developer:0"]));
    }

    /// Slice 5 — phase actions are now implemented. advance_phase on an
    /// EMPTY plan returns [InvalidArgs] (caller must set_phase_plan first);
    /// the test below covers happy-path advance with a real plan.
    #[test]
    fn dispatch_advance_phase_empty_plan_returns_invalid_args() {
        let dir = temp_project_with_protocol(
            "dispatch-advance-empty",
            serde_json::json!({
                "schema_version": 1, "rev": 0, "preset": "Debate",
                "floor": {"mode": "reactive", "current_speaker": null, "queue": [], "rotation_order": [], "threshold_ms": 60000, "started_at": null},
                "consensus": {"mode": "none", "round": null, "phase": null, "submissions": []},
                "phase_plan": {"phases": [], "current_phase_idx": 0, "paused_at": null, "paused_total_secs": 0},
                "scopes": {"floor": "instance", "consensus": "role"},
                "last_writer_seat": null, "last_writer_action": null, "rev_at": null
            }),
            serde_json::json!({"bindings": []}),
        );
        let err = do_protocol_mutate(
            dir.to_str().unwrap(),
            "architect:0",
            "default",
            "advance_phase",
            serde_json::json!({}),
            Some(0),
        )
        .unwrap_err();
        assert!(err.starts_with("[InvalidArgs]"), "got: {}", err);
    }

    /// Slice 5 — set_phase_plan happy path stamps started_at and rev=1.
    #[test]
    fn dispatch_set_phase_plan_seeds_started_at() {
        let dir = temp_project_with_protocol(
            "dispatch-set-plan",
            serde_json::json!({
                "schema_version": 1, "rev": 0, "preset": "Debate",
                "floor": {"mode": "reactive", "current_speaker": null, "queue": [], "rotation_order": [], "threshold_ms": 60000, "started_at": null},
                "consensus": {"mode": "none", "round": null, "phase": null, "submissions": []},
                "phase_plan": {"phases": [], "current_phase_idx": 0, "paused_at": null, "paused_total_secs": 0},
                "scopes": {"floor": "instance", "consensus": "role"},
                "last_writer_seat": null, "last_writer_action": null, "rev_at": null
            }),
            serde_json::json!({"bindings": []}),
        );
        do_protocol_mutate(
            dir.to_str().unwrap(),
            "architect:0",
            "default",
            "set_phase_plan",
            serde_json::json!({"phases": [
                {"preset": "Debate", "duration_secs": 3600, "outcome": {"kind": "manual"}}
            ]}),
            Some(0),
        )
        .unwrap();
        let after = read_protocol_back(&dir);
        assert_eq!(after["rev"], 1);
        let phases = after["phase_plan"]["phases"].as_array().unwrap();
        assert_eq!(phases.len(), 1);
        assert!(phases[0].get("started_at").and_then(|s| s.as_str()).is_some());
    }

    /// Slice 5 — advance_phase stamps ended_at on current and started_at
    /// on next, advances current_phase_idx.
    #[test]
    fn dispatch_advance_phase_stamps_boundaries() {
        let dir = temp_project_with_protocol(
            "dispatch-advance",
            serde_json::json!({
                "schema_version": 1, "rev": 0, "preset": "Debate",
                "floor": {"mode": "reactive", "current_speaker": null, "queue": [], "rotation_order": [], "threshold_ms": 60000, "started_at": null},
                "consensus": {"mode": "none", "round": null, "phase": null, "submissions": []},
                "phase_plan": {
                    "phases": [
                        {"preset": "Debate", "duration_secs": 0, "outcome": {"kind": "manual"}, "started_at": "2026-04-28T00:00:00Z", "ended_at": null, "extension_secs": 0},
                        {"preset": "Brainstorm", "duration_secs": 0, "outcome": {"kind": "manual"}, "started_at": null, "ended_at": null, "extension_secs": 0}
                    ],
                    "current_phase_idx": 0, "paused_at": null, "paused_total_secs": 0
                },
                "scopes": {"floor": "instance", "consensus": "role"},
                "last_writer_seat": null, "last_writer_action": null, "rev_at": null
            }),
            serde_json::json!({"bindings": []}),
        );
        do_protocol_mutate(
            dir.to_str().unwrap(),
            "architect:0",
            "default",
            "advance_phase",
            serde_json::json!({}),
            Some(0),
        )
        .unwrap();
        let after = read_protocol_back(&dir);
        assert_eq!(after["phase_plan"]["current_phase_idx"], 1);
        assert!(after["phase_plan"]["phases"][0]["ended_at"].is_string());
        assert!(after["phase_plan"]["phases"][1]["started_at"].is_string());
    }

    /// Slice 5 — pause_plan + resume_plan adds elapsed pause to
    /// paused_total_secs accumulator.
    #[test]
    fn dispatch_pause_resume_accumulates_paused_secs() {
        let dir = temp_project_with_protocol(
            "dispatch-pause-resume",
            serde_json::json!({
                "schema_version": 1, "rev": 0, "preset": "Debate",
                "floor": {"mode": "reactive", "current_speaker": null, "queue": [], "rotation_order": [], "threshold_ms": 60000, "started_at": null},
                "consensus": {"mode": "none", "round": null, "phase": null, "submissions": []},
                "phase_plan": {"phases": [{"preset": "Debate", "duration_secs": 60, "outcome": {"kind": "manual"}}], "current_phase_idx": 0, "paused_at": null, "paused_total_secs": 0},
                "scopes": {"floor": "instance", "consensus": "role"},
                "last_writer_seat": null, "last_writer_action": null, "rev_at": null
            }),
            serde_json::json!({"bindings": []}),
        );
        do_protocol_mutate(
            dir.to_str().unwrap(),
            "architect:0",
            "default",
            "pause_plan",
            serde_json::json!({}),
            Some(0),
        ).unwrap();
        let after_pause = read_protocol_back(&dir);
        assert!(after_pause["phase_plan"]["paused_at"].is_string());

        do_protocol_mutate(
            dir.to_str().unwrap(),
            "architect:0",
            "default",
            "resume_plan",
            serde_json::json!({}),
            Some(1),
        ).unwrap();
        let after_resume = read_protocol_back(&dir);
        assert!(after_resume["phase_plan"]["paused_at"].is_null());
        // paused_total_secs accumulates (any value >= 0; sub-second pauses
        // round to 0, that's expected on a fast test).
        let total = after_resume["phase_plan"]["paused_total_secs"].as_u64().unwrap_or(999);
        assert!(total < 999, "paused_total_secs should be a small accumulator value, got: {}", total);
    }

    /// Slice 7 — Delphi mode: open_round with mode=vote + blind_submit_gate=true.
    #[test]
    fn slice7_delphi_open_round_carries_blind_gate() {
        let dir = temp_project_with_protocol(
            "slice7-delphi",
            serde_json::json!({
                "schema_version": 1, "rev": 0, "preset": "Debate",
                "floor": {"mode": "reactive", "current_speaker": null, "queue": [], "rotation_order": [], "threshold_ms": 60000, "started_at": null},
                "consensus": {"mode": "none", "round": null, "phase": null, "submissions": []},
                "phase_plan": {"phases": [], "current_phase_idx": 0, "paused_at": null, "paused_total_secs": 0},
                "scopes": {"floor": "instance", "consensus": "role"},
                "last_writer_seat": null, "last_writer_action": null, "rev_at": null
            }),
            serde_json::json!({"bindings": []}),
        );
        do_protocol_mutate(
            dir.to_str().unwrap(),
            "moderator:0",
            "default",
            "open_round",
            serde_json::json!({
                "topic": "Should we ship 9faf275?",
                "mode": "vote",
                "blind_submit_gate": true
            }),
            Some(0),
        ).unwrap();
        let after = read_protocol_back(&dir);
        assert_eq!(after["consensus"]["mode"], "vote");
        assert_eq!(after["consensus"]["round"]["blind_submit_gate"], true);
    }

    /// Slice 7 — Oxford mode: open_round with mode=vote + teams.
    #[test]
    fn slice7_oxford_open_round_carries_teams() {
        let dir = temp_project_with_protocol(
            "slice7-oxford",
            serde_json::json!({
                "schema_version": 1, "rev": 0, "preset": "Debate",
                "floor": {"mode": "reactive", "current_speaker": null, "queue": [], "rotation_order": [], "threshold_ms": 60000, "started_at": null},
                "consensus": {"mode": "none", "round": null, "phase": null, "submissions": []},
                "phase_plan": {"phases": [], "current_phase_idx": 0, "paused_at": null, "paused_total_secs": 0},
                "scopes": {"floor": "instance", "consensus": "role"},
                "last_writer_seat": null, "last_writer_action": null, "rev_at": null
            }),
            serde_json::json!({"bindings": []}),
        );
        do_protocol_mutate(
            dir.to_str().unwrap(),
            "moderator:0",
            "default",
            "open_round",
            serde_json::json!({
                "topic": "Resolved: ship as-is",
                "mode": "vote",
                "teams": {"for": ["dev:0", "architect:0"], "against": ["evil-architect:0"]}
            }),
            Some(0),
        ).unwrap();
        let after = read_protocol_back(&dir);
        assert_eq!(after["consensus"]["round"]["teams"]["for"], serde_json::json!(["dev:0", "architect:0"]));
        assert_eq!(after["consensus"]["round"]["teams"]["against"], serde_json::json!(["evil-architect:0"]));
    }

    /// Slice 7 — open_round rejects malformed teams shape.
    #[test]
    fn slice7_open_round_rejects_malformed_teams() {
        let dir = temp_project_with_protocol(
            "slice7-bad-teams",
            serde_json::json!({
                "schema_version": 1, "rev": 0, "preset": "Debate",
                "floor": {"mode": "reactive", "current_speaker": null, "queue": [], "rotation_order": [], "threshold_ms": 60000, "started_at": null},
                "consensus": {"mode": "none", "round": null, "phase": null, "submissions": []},
                "phase_plan": {"phases": [], "current_phase_idx": 0, "paused_at": null, "paused_total_secs": 0},
                "scopes": {"floor": "instance", "consensus": "role"},
                "last_writer_seat": null, "last_writer_action": null, "rev_at": null
            }),
            serde_json::json!({"bindings": []}),
        );
        let err = do_protocol_mutate(
            dir.to_str().unwrap(),
            "moderator:0",
            "default",
            "open_round",
            serde_json::json!({
                "topic": "x",
                "mode": "vote",
                "teams": {"for": "should-be-array", "against": []}
            }),
            Some(0),
        ).unwrap_err();
        assert!(err.starts_with("[InvalidArgs]"), "got: {}", err);
    }

    /// Round-trip test for the assembly_line thin-wrap (tech-leader #994):
    /// the legacy projection of a protocol.json with preset="Assembly Line"
    /// produces `active=true` + the original rotation_order. The wrapper
    /// path is `do_protocol_mutate(set_preset, ...)` whose result we
    /// project the same way the live `handle_assembly_line` does.
    #[test]
    fn assembly_line_round_trip_via_protocol_mutate() {
        let dir = temp_project_with_protocol(
            "assembly-line-rt",
            serde_json::json!({
                "schema_version": 1, "rev": 0, "preset": "Default chat",
                "floor": {"mode": "none", "current_speaker": null, "queue": [], "rotation_order": [], "threshold_ms": 60000, "started_at": null},
                "consensus": {"mode": "none", "round": null, "phase": null, "submissions": []},
                "phase_plan": {"phases": [], "current_phase_idx": 0, "paused_at": null, "paused_total_secs": 0},
                "scopes": {"floor": "instance", "consensus": "role"},
                "last_writer_seat": null, "last_writer_action": null, "rev_at": null
            }),
            serde_json::json!({"bindings": [{"role": "architect", "instance": 0, "status": "active"}]}),
        );
        // Wrapper path: assembly_line(enable) → set_preset("Assembly Line").
        do_protocol_mutate(
            dir.to_str().unwrap(),
            "architect:0",
            "default",
            "set_preset",
            serde_json::json!({"name": "Assembly Line"}),
            Some(0),
        ).unwrap();
        // Legacy projection: read protocol.json, project to legacy shape.
        let proto = read_protocol_for_section_value(dir.to_str().unwrap(), "default");
        assert_eq!(proto["preset"], "Assembly Line");
        assert_eq!(proto["floor"]["mode"], "round-robin");
        let active = proto.get("preset").and_then(|p| p.as_str()) == Some("Assembly Line");
        assert!(active, "legacy projection.active must be true after enable");

        // Wrapper path: assembly_line(disable) → set_preset("Default chat").
        let cur_rev = proto.get("rev").and_then(|v| v.as_u64()).unwrap_or(0);
        do_protocol_mutate(
            dir.to_str().unwrap(),
            "architect:0",
            "default",
            "set_preset",
            serde_json::json!({"name": "Default chat"}),
            Some(cur_rev),
        ).unwrap();
        let after_disable = read_protocol_for_section_value(dir.to_str().unwrap(), "default");
        assert_eq!(after_disable["preset"], "Default chat");
        let still_active = after_disable.get("preset").and_then(|p| p.as_str()) == Some("Assembly Line");
        assert!(!still_active, "legacy projection.active must be false after disable");
    }

    /// Round-trip test for discussion_control(continuous) thin-wrap:
    /// legacy start_discussion(continuous, topic) → protocol_mutate(open_round)
    /// → consensus.phase=submitting, mode=tally, round.topic preserved.
    #[test]
    fn discussion_control_continuous_round_trip_via_protocol_mutate() {
        let dir = temp_project_with_protocol(
            "discussion-continuous-rt",
            serde_json::json!({
                "schema_version": 1, "rev": 0, "preset": "Debate",
                "floor": {"mode": "reactive", "current_speaker": null, "queue": [], "rotation_order": [], "threshold_ms": 60000, "started_at": null},
                "consensus": {"mode": "none", "round": null, "phase": null, "submissions": []},
                "phase_plan": {"phases": [], "current_phase_idx": 0, "paused_at": null, "paused_total_secs": 0},
                "scopes": {"floor": "instance", "consensus": "role"},
                "last_writer_seat": null, "last_writer_action": null, "rev_at": null
            }),
            serde_json::json!({"bindings": [{"role": "architect", "instance": 0, "status": "active"}]}),
        );
        do_protocol_mutate(
            dir.to_str().unwrap(),
            "architect:0",
            "default",
            "open_round",
            serde_json::json!({"topic": "Continuous review starting", "mode": "tally"}),
            Some(0),
        ).unwrap();
        let proto = read_protocol_for_section_value(dir.to_str().unwrap(), "default");
        assert_eq!(proto["consensus"]["mode"], "tally");
        assert_eq!(proto["consensus"]["phase"], "submitting");
        assert_eq!(proto["consensus"]["round"]["topic"], "Continuous review starting");

        // close_round closes phase.
        do_protocol_mutate(
            dir.to_str().unwrap(),
            "architect:0",
            "default",
            "close_round",
            serde_json::json!({}),
            Some(1),
        ).unwrap();
        let after = read_protocol_for_section_value(dir.to_str().unwrap(), "default");
        assert_eq!(after["consensus"]["phase"], "closed");
    }

    /// Normalize-wired test (evil-arch #978 ship-block fix): when
    /// set_preset transitions floor.mode to "free-grab", normalize must
    /// dissolve floor.queue in the SAME mutate window. Closes the
    /// "tests-only firing" gap by exercising the path through
    /// do_protocol_mutate, not the bare normalize() helper.
    #[test]
    fn normalize_wired_to_set_preset_clears_queue_on_free_grab() {
        let dir = temp_project_with_protocol(
            "normalize-set-preset",
            serde_json::json!({
                "schema_version": 1, "rev": 0, "preset": "Town hall",
                "floor": {"mode": "queue", "current_speaker": "architect:0", "queue": ["dev:0", "tester:0"], "rotation_order": [], "threshold_ms": 60000, "started_at": null},
                "consensus": {"mode": "none", "round": null, "phase": null, "submissions": []},
                "phase_plan": {"phases": [], "current_phase_idx": 0, "paused_at": null, "paused_total_secs": 0},
                "scopes": {"floor": "instance", "consensus": "role"},
                "last_writer_seat": null, "last_writer_action": null, "rev_at": null
            }),
            serde_json::json!({"bindings": [
                {"role": "architect", "instance": 0, "status": "active"},
                {"role": "dev", "instance": 0, "status": "active"},
                {"role": "tester", "instance": 0, "status": "active"}
            ]}),
        );
        do_protocol_mutate(
            dir.to_str().unwrap(),
            "architect:0",
            "default",
            "set_preset",
            serde_json::json!({"name": "Brainstorm"}), // Brainstorm = (free-grab, none)
            Some(0),
        ).unwrap();
        let after = read_protocol_back(&dir);
        assert_eq!(after["floor"]["mode"], "free-grab");
        // Normalize rule 1 must have fired: queue dissolved.
        assert_eq!(after["floor"]["queue"], serde_json::json!([]));
    }

    /// Normalize-wired test: orphan current_speaker (active_seats from
    /// sessions.json doesn't include the held mic) → clear on next mutate.
    #[test]
    fn normalize_wired_clears_orphan_current_speaker() {
        let dir = temp_project_with_protocol(
            "normalize-orphan",
            serde_json::json!({
                "schema_version": 1, "rev": 0, "preset": "Debate",
                "floor": {"mode": "reactive", "current_speaker": "ghost:0", "queue": [], "rotation_order": [], "threshold_ms": 60000, "started_at": null},
                "consensus": {"mode": "none", "round": null, "phase": null, "submissions": []},
                "phase_plan": {"phases": [], "current_phase_idx": 0, "paused_at": null, "paused_total_secs": 0},
                "scopes": {"floor": "instance", "consensus": "role"},
                "last_writer_seat": null, "last_writer_action": null, "rev_at": null
            }),
            serde_json::json!({"bindings": [
                {"role": "dev", "instance": 0, "status": "active"}
            ]}),
        );
        // Any mutate that triggers a normalize pass will clear ghost:0.
        // toggle_queue (self-only, dev:0 → adds self) is a clean trigger.
        do_protocol_mutate(
            dir.to_str().unwrap(),
            "dev:0",
            "default",
            "toggle_queue",
            serde_json::json!({}),
            Some(0),
        ).unwrap();
        let after = read_protocol_back(&dir);
        // Normalize rule 2: orphan ghost:0 cleared.
        assert!(after["floor"]["current_speaker"].is_null());
        // toggle_queue itself added dev:0; rule 3 prunes nothing here.
        assert_eq!(after["floor"]["queue"], serde_json::json!(["dev:0"]));
    }

    /// Slice 5 — extend_phase adds to extension_secs on current phase.
    #[test]
    fn dispatch_extend_phase_adds_secs() {
        let dir = temp_project_with_protocol(
            "dispatch-extend",
            serde_json::json!({
                "schema_version": 1, "rev": 0, "preset": "Debate",
                "floor": {"mode": "reactive", "current_speaker": null, "queue": [], "rotation_order": [], "threshold_ms": 60000, "started_at": null},
                "consensus": {"mode": "none", "round": null, "phase": null, "submissions": []},
                "phase_plan": {
                    "phases": [{"preset": "Debate", "duration_secs": 60, "outcome": {"kind": "timer"}, "started_at": "2026-04-28T00:00:00Z", "ended_at": null, "extension_secs": 30}],
                    "current_phase_idx": 0, "paused_at": null, "paused_total_secs": 0
                },
                "scopes": {"floor": "instance", "consensus": "role"},
                "last_writer_seat": null, "last_writer_action": null, "rev_at": null
            }),
            serde_json::json!({"bindings": []}),
        );
        do_protocol_mutate(
            dir.to_str().unwrap(),
            "architect:0",
            "default",
            "extend_phase",
            serde_json::json!({"secs": 900}),
            Some(0),
        )
        .unwrap();
        let after = read_protocol_back(&dir);
        assert_eq!(after["phase_plan"]["phases"][0]["extension_secs"], 930);
    }

    /// Slice 6 — open_round happy path opens a round in submitting phase.
    #[test]
    fn dispatch_open_round_happy_path() {
        let dir = temp_project_with_protocol(
            "dispatch-open-round",
            serde_json::json!({
                "schema_version": 1, "rev": 0, "preset": "Debate",
                "floor": {"mode": "reactive", "current_speaker": null, "queue": [], "rotation_order": [], "threshold_ms": 60000, "started_at": null},
                "consensus": {"mode": "none", "round": null, "phase": null, "submissions": []},
                "phase_plan": {"phases": [], "current_phase_idx": 0, "paused_at": null, "paused_total_secs": 0},
                "scopes": {"floor": "instance", "consensus": "role"},
                "last_writer_seat": null, "last_writer_action": null, "rev_at": null
            }),
            serde_json::json!({"bindings": []}),
        );
        do_protocol_mutate(
            dir.to_str().unwrap(),
            "architect:0",
            "default",
            "open_round",
            serde_json::json!({"topic": "Ship 9faf275 to main?", "mode": "tally"}),
            Some(0),
        )
        .unwrap();
        let after = read_protocol_back(&dir);
        assert_eq!(after["consensus"]["mode"], "tally");
        assert_eq!(after["consensus"]["phase"], "submitting");
        assert_eq!(after["consensus"]["round"]["topic"], "Ship 9faf275 to main?");
        assert_eq!(after["consensus"]["round"]["opened_by"], "architect:0");
    }

    /// Slice 6 — submit + close_round end-to-end. Per-role replacement
    /// preserves "one vote per role" invariant.
    #[test]
    fn dispatch_submit_then_close_round_full_path() {
        let dir = temp_project_with_protocol(
            "dispatch-submit-close",
            serde_json::json!({
                "schema_version": 1, "rev": 0, "preset": "Debate",
                "floor": {"mode": "reactive", "current_speaker": null, "queue": [], "rotation_order": [], "threshold_ms": 60000, "started_at": null},
                "consensus": {
                    "mode": "tally",
                    "round": {"topic": "x", "opened_at": "2026-04-28T00:00:00Z", "opened_by": "architect:0"},
                    "phase": "submitting",
                    "submissions": []
                },
                "phase_plan": {"phases": [], "current_phase_idx": 0, "paused_at": null, "paused_total_secs": 0},
                "scopes": {"floor": "instance", "consensus": "role"},
                "last_writer_seat": null, "last_writer_action": null, "rev_at": null
            }),
            serde_json::json!({"bindings": []}),
        );
        // First submission from developer:0
        do_protocol_mutate(
            dir.to_str().unwrap(),
            "developer:0",
            "default",
            "submit",
            serde_json::json!({"body": "approve"}),
            Some(0),
        ).unwrap();
        // Second submission from same role replaces (per-role scope).
        do_protocol_mutate(
            dir.to_str().unwrap(),
            "developer:0",
            "default",
            "submit",
            serde_json::json!({"body": "actually changed mind"}),
            Some(1),
        ).unwrap();
        let mid = read_protocol_back(&dir);
        let subs = mid["consensus"]["submissions"].as_array().unwrap();
        assert_eq!(subs.len(), 1, "per-role replacement should keep exactly one submission per role");
        assert_eq!(subs[0]["body"], "actually changed mind");

        // Close the round.
        do_protocol_mutate(
            dir.to_str().unwrap(),
            "architect:0",
            "default",
            "close_round",
            serde_json::json!({}),
            Some(2),
        ).unwrap();
        let after = read_protocol_back(&dir);
        assert_eq!(after["consensus"]["phase"], "closed");
    }

    /// Unknown action → [InvalidAction].
    #[test]
    fn dispatch_unknown_action_returns_invalid_action() {
        let dir = temp_project_with_protocol(
            "dispatch-invalid",
            serde_json::json!({
                "schema_version": 1, "rev": 0, "preset": "Debate",
                "floor": {"mode": "reactive", "current_speaker": null, "queue": [], "rotation_order": [], "threshold_ms": 60000, "started_at": null},
                "consensus": {"mode": "none", "round": null, "phase": null, "submissions": []},
                "phase_plan": {"phases": [], "current_phase_idx": 0, "paused_at": null, "paused_total_secs": 0},
                "scopes": {"floor": "instance", "consensus": "role"},
                "last_writer_seat": null, "last_writer_action": null, "rev_at": null
            }),
            serde_json::json!({"bindings": []}),
        );
        let err = do_protocol_mutate(
            dir.to_str().unwrap(),
            "architect:0",
            "default",
            "wat",
            serde_json::json!({}),
            Some(0),
        )
        .unwrap_err();
        assert!(err.starts_with("[InvalidAction]"), "got: {}", err);
    }

    /// Pre-migration: no protocol.json + legacy assembly.json present →
    /// vaak-mcp synthesizes, archives, then accepts the rev=0 mutate
    /// (evil-arch #939 concern 1).
    #[test]
    fn pre_migration_synth_archives_and_accepts_mutate() {
        let dir = std::env::temp_dir().join("vaak-mcp-protocol-pre-mig");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".vaak")).unwrap();
        // Legacy assembly.json present, NO protocol.json.
        std::fs::write(
            dir.join(".vaak").join("assembly.json"),
            r#"{"active":true,"current_speaker":"architect:0","rotation_order":["architect:0","developer:0"]}"#,
        )
        .unwrap();
        std::fs::write(
            dir.join(".vaak").join("sessions.json"),
            r#"{"bindings":[{"role":"architect","instance":0,"status":"active"},{"role":"developer","instance":0,"status":"active"}]}"#,
        )
        .unwrap();

        // Caller passes rev=0 because the synth produces rev=0.
        do_protocol_mutate(
            dir.to_str().unwrap(),
            "developer:0",
            "default",
            "toggle_queue",
            serde_json::json!({}),
            Some(0),
        )
        .unwrap();

        // protocol.json now exists, legacy archived.
        assert!(dir.join(".vaak").join("protocol.json").exists());
        assert!(!dir.join(".vaak").join("assembly.json").exists());
        assert!(dir.join(".vaak").join("legacy").join("default").join("assembly.json").exists());
        let after = read_protocol_back(&dir);
        // Synth produces "Assembly Line" preset; toggle_queue then bumps rev to 1.
        assert_eq!(after["preset"], "Assembly Line");
        assert_eq!(after["rev"], 1);
        assert_eq!(after["floor"]["queue"], serde_json::json!(["developer:0"]));
    }

    /// Error-taxonomy reachability (tester #928 add): every prefix listed in
    /// the handle_protocol_mutate doc-comment is reachable from a fixtured
    /// input.
    #[test]
    fn error_taxonomy_apply_layer_codes_all_reachable() {
        let prefixes = [
            "[InvalidArgs]",
            "[NotPermitted]",
        ];
        let mut hit = std::collections::HashSet::new();
        // Each apply test above hit one — re-run a representative sample
        // here so this test is self-contained.
        let mut s = fresh_state();
        let e = apply_set_preset(&mut s, &serde_json::json!({})).unwrap_err();
        if e.starts_with("[InvalidArgs]") { hit.insert("[InvalidArgs]"); }
        s["floor"]["current_speaker"] = serde_json::json!("architect:0");
        let e = apply_yield(&mut s, &serde_json::json!({}), "developer:0").unwrap_err();
        if e.starts_with("[NotPermitted]") { hit.insert("[NotPermitted]"); }
        for prefix in prefixes {
            assert!(hit.contains(prefix), "no test fixture reached {}", prefix);
        }
    }

    // v1.5.1 commit 2 fixture tests — per tester msg 873.

    #[test]
    fn mic_claim_writes_turn_type_and_duration() {
        let mut s = fresh_state();
        s["floor"]["current_speaker"] = serde_json::json!("developer:0");
        apply_mic_claim(
            &mut s,
            &serde_json::json!({"turn_type": "working", "expected_duration_secs": 300}),
            "developer:0",
        )
        .unwrap();
        assert_eq!(s["floor"]["turn_type"], "working");
        assert_eq!(s["floor"]["expected_duration_secs"], 300);
        assert!(s["floor"].get("claimed_at").is_some());
    }

    #[test]
    fn mic_claim_unknown_turn_type_rejects_strict() {
        let mut s = fresh_state();
        s["floor"]["current_speaker"] = serde_json::json!("developer:0");
        let err = apply_mic_claim(
            &mut s,
            &serde_json::json!({"turn_type": "loitering", "expected_duration_secs": 300}),
            "developer:0",
        )
        .unwrap_err();
        assert!(err.starts_with("[UnknownTurnType]"), "got: {}", err);
    }

    #[test]
    fn mic_claim_defaults_per_turn_type() {
        let mut s = fresh_state();
        s["floor"]["current_speaker"] = serde_json::json!("developer:0");
        // Each turn type's default is inside the 30-600 bounds (evil-arch
        // msg 896 fix — prior "working => 900" default rejected itself).
        for (tt, want) in [
            ("working", 300u64),
            ("reviewing", 120),
            ("passing", 30),
            ("thinking", 300),
        ] {
            let mut s2 = s.clone();
            apply_mic_claim(
                &mut s2,
                &serde_json::json!({"turn_type": tt}),
                "developer:0",
            )
            .unwrap_or_else(|e| panic!("no-arg claim for {} should succeed, got: {}", tt, e));
            assert_eq!(s2["floor"]["expected_duration_secs"], want, "default for {}", tt);
        }
    }

    #[test]
    fn mic_claim_bounds_reject_out_of_range() {
        let mut s = fresh_state();
        s["floor"]["current_speaker"] = serde_json::json!("developer:0");
        for bad in [0u64, 29, 601, 1800, 99999] {
            let err = apply_mic_claim(
                &mut s,
                &serde_json::json!({"turn_type": "working", "expected_duration_secs": bad}),
                "developer:0",
            )
            .unwrap_err();
            assert!(
                err.starts_with("[ClaimOutOfBounds]"),
                "duration {} got: {}",
                bad,
                err
            );
        }
    }

    #[test]
    fn mic_claim_not_current_speaker_rejects() {
        let mut s = fresh_state();
        s["floor"]["current_speaker"] = serde_json::json!("architect:0");
        let err = apply_mic_claim(
            &mut s,
            &serde_json::json!({"turn_type": "working", "expected_duration_secs": 300}),
            "developer:0",
        )
        .unwrap_err();
        assert!(err.starts_with("[NotSpeaker]"), "got: {}", err);
    }

    // ============================================================
    // Two-controls v1 fixture tests (spec 2026-05-14, items 1-9)
    // ============================================================

    /// Helper — create a unique-named tempdir for plan-file fixtures.
    fn plan_test_dir(test_name: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("vaak-test-{}-{}", test_name, nanos));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".vaak").join("design-notes")).unwrap();
        dir
    }

    fn write_plan(project_dir: &std::path::Path, name: &str, scope_body: &str) -> String {
        let path = project_dir.join(".vaak").join("design-notes").join(name);
        let body = format!(
            "# Plan {}\n\n<!-- scope: {} -->\n\nbody.\n",
            name, scope_body
        );
        std::fs::write(&path, body).unwrap();
        format!(".vaak/design-notes/{}", name)
    }

    /// A7 — revise_plan role gate (CRITICAL per evil-arch msg 988).
    /// Non-architect / non-manager / non-human callers reject.
    #[test]
    fn a7_revise_plan_role_gate_rejects_developer() {
        let mut s = fresh_state();
        let dir = plan_test_dir("a7_dev");
        let plan_rel = write_plan(&dir, "p.md", "src/foo.rs");
        let pd = dir.to_string_lossy().to_string();
        let err = apply_revise_plan(
            &mut s,
            &serde_json::json!({"plan_path": &plan_rel, "revision_note": "nope"}),
            "developer:0",
            &pd,
        )
        .unwrap_err();
        assert!(
            err.starts_with("[RevisePlanForbidden]"),
            "developer must be rejected, got: {}",
            err
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a7_revise_plan_role_gate_accepts_architect() {
        let mut s = fresh_state();
        let dir = plan_test_dir("a7_arch");
        let plan_rel = write_plan(&dir, "p.md", "src/foo.rs");
        let pd = dir.to_string_lossy().to_string();
        apply_revise_plan(
            &mut s,
            &serde_json::json!({"plan_path": &plan_rel, "revision_note": "ok"}),
            "architect:0",
            &pd,
        )
        .expect("architect must be accepted");
        assert!(s["floor"]["plan_hash"].as_str().is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A7b — set_moderator role gate (CRITICAL per dev-challenger msg 1115 F2
    /// + evil-arch msg 1117 upgrade). Same structural class as A7 revise_plan.
    #[test]
    fn a7b_set_moderator_role_gate_rejects_developer() {
        let mut s = fresh_state();
        let err = apply_set_moderator(
            &mut s,
            &serde_json::json!({"seat": "developer:0"}),
            "developer:0",
        )
        .unwrap_err();
        assert!(
            err.starts_with("[SetModeratorForbidden]"),
            "developer must be rejected, got: {}",
            err
        );
    }

    #[test]
    fn a7b_set_moderator_role_gate_rejects_dev_challenger() {
        let mut s = fresh_state();
        let err = apply_set_moderator(
            &mut s,
            &serde_json::json!({"seat": "dev-challenger:0"}),
            "dev-challenger:0",
        )
        .unwrap_err();
        assert!(err.starts_with("[SetModeratorForbidden]"), "got: {}", err);
    }

    #[test]
    fn a7b_set_moderator_role_gate_accepts_architect() {
        let mut s = fresh_state();
        apply_set_moderator(
            &mut s,
            &serde_json::json!({"seat": "architect:0"}),
            "architect:0",
        )
        .expect("architect must be accepted");
        assert_eq!(s["floor"]["moderator"], "architect:0");
    }

    #[test]
    fn a7b_set_moderator_role_gate_accepts_human() {
        let mut s = fresh_state();
        apply_set_moderator(
            &mut s,
            &serde_json::json!({"seat": "tester:0"}),
            "human:0",
        )
        .expect("human must be accepted");
        assert_eq!(s["floor"]["moderator"], "tester:0");
    }

    // ============================================================
    // Collaborative-proposal-workflow v1 (spec 2026-05-15) — Commit P
    // R1 propose_replanning execution-only gate
    // R2 propose_replanning open to any seat (no role gate)
    // ============================================================

    /// R1 — propose_replanning from execution phase pushes onto queue.
    #[test]
    fn r1_propose_replanning_from_execution_pushes_to_queue() {
        let mut s = fresh_state();
        s["floor"]["phase"] = serde_json::json!("execution");
        apply_propose_replanning(&mut s, "developer:1", "plan gap on test rubric", None)
            .expect("execution-phase propose must succeed");
        let queue = s["floor"]["replanning_requests"].as_array().unwrap();
        assert_eq!(queue.len(), 1);
        assert_eq!(queue[0]["seat"], "developer:1");
        assert_eq!(queue[0]["reason"], "plan gap on test rubric");
        assert!(queue[0]["ts"].is_i64() || queue[0]["ts"].is_u64());
    }

    /// R1 — propose_replanning from planning phase rejects with the
    /// [ProposeReplanningPhaseInvalid] envelope per spec.
    #[test]
    fn r1_propose_replanning_from_planning_rejects() {
        let mut s = fresh_state();
        s["floor"]["phase"] = serde_json::json!("planning");
        let err = apply_propose_replanning(&mut s, "developer:1", "should not land", None)
            .unwrap_err();
        assert!(
            err.starts_with("[ProposeReplanningPhaseInvalid]"),
            "got: {}",
            err
        );
        // Queue remains empty after rejection.
        let queue = s["floor"]["replanning_requests"].as_array().unwrap();
        assert!(queue.is_empty());
    }

    /// R1 — missing args.reason rejects with [InvalidArgs] at the dispatcher
    /// layer. apply_propose_replanning itself takes `reason: &str` directly
    /// (per spec v6 signature), so the InvalidArgs envelope is constructed
    /// in the dispatcher when args.reason is absent. Verifying the
    /// dispatcher contract: this is exercised by integration through
    /// handle_protocol_mutate but the apply-layer test here pins the fact
    /// that an empty string still pushes (server-side gate is phase-only).
    #[test]
    fn r1_propose_replanning_empty_reason_still_pushes() {
        let mut s = fresh_state();
        s["floor"]["phase"] = serde_json::json!("execution");
        apply_propose_replanning(&mut s, "developer:1", "", None)
            .expect("apply layer accepts any &str reason; dispatcher enforces non-null");
        let queue = s["floor"]["replanning_requests"].as_array().unwrap();
        assert_eq!(queue.len(), 1);
        assert_eq!(queue[0]["reason"], "");
    }

    /// R2 — propose_replanning has NO role gate. A non-moderator,
    /// non-architect, non-human caller succeeds. Per spec: any active seat
    /// in the rotation can propose; the moderator decides whether to pivot.
    #[test]
    fn r2_propose_replanning_no_role_gate_developer_succeeds() {
        let mut s = fresh_state();
        s["floor"]["phase"] = serde_json::json!("execution");
        apply_propose_replanning(&mut s, "developer:1", "developer-lens plan gap", None)
            .expect("developer must be accepted — no role gate on propose_replanning");
    }

    #[test]
    fn r2_propose_replanning_no_role_gate_dev_challenger_succeeds() {
        let mut s = fresh_state();
        s["floor"]["phase"] = serde_json::json!("execution");
        apply_propose_replanning(&mut s, "dev-challenger:0", "adversarial concern", None)
            .expect("dev-challenger must be accepted — no role gate on propose_replanning");
    }

    #[test]
    fn r2_propose_replanning_no_role_gate_audience_succeeds() {
        let mut s = fresh_state();
        s["floor"]["phase"] = serde_json::json!("execution");
        apply_propose_replanning(&mut s, "audience:0", "learner-persona gap", None)
            .expect("audience must be accepted — no role gate on propose_replanning");
    }

    /// R1 — phase field missing in floor (legacy/pre-Commit-P state) defaults
    /// to "execution" per fresh_state shape. Confirms back-compat: pre-v1.1
    /// protocol.json files don't get rejected for missing phase.
    #[test]
    fn r1_propose_replanning_missing_phase_defaults_to_execution() {
        let mut s = fresh_state();
        // Strip phase to simulate a pre-v1.1 protocol.json.
        if let Some(floor) = s["floor"].as_object_mut() {
            floor.remove("phase");
        }
        apply_propose_replanning(&mut s, "developer:1", "back-compat path", None)
            .expect("missing phase must default to execution and accept");
    }

    /// R3 precondition (Commit P.B per architect msg 1944 + dev-challenger
    /// msg 1939 #3) — explicit ts parameter is honored. Production calls
    /// with None get a now()-based ts; tests with Some(T) get T. This is
    /// what unblocks tester's R3 fixture (msg 1937) — without the
    /// parameter, natural now() jitter at μs scale can't discriminate
    /// FIFO from random-but-serialized for N≥4 simultaneous appenders.
    #[test]
    fn r3_precondition_explicit_ts_is_honored() {
        let mut s = fresh_state();
        s["floor"]["phase"] = serde_json::json!("execution");
        apply_propose_replanning(&mut s, "developer:1", "first", Some(1_700_000_000_000))
            .unwrap();
        apply_propose_replanning(&mut s, "developer:1", "second", Some(1_700_000_000_001))
            .unwrap();
        apply_propose_replanning(&mut s, "developer:1", "third", Some(1_700_000_000_002))
            .unwrap();
        let queue = s["floor"]["replanning_requests"].as_array().unwrap();
        assert_eq!(queue.len(), 3);
        assert_eq!(queue[0]["ts"].as_i64().unwrap(), 1_700_000_000_000);
        assert_eq!(queue[1]["ts"].as_i64().unwrap(), 1_700_000_000_001);
        assert_eq!(queue[2]["ts"].as_i64().unwrap(), 1_700_000_000_002);
    }

    // ============================================================
    // R3 — replanning_requests multi-writer NO-ERASURE under contention
    //
    // Per tester msg 1913 + architect msg 1979 v7 correction +
    // dev-challenger msg 1976 catch: the lock-via-collab.rs guarantee is
    // no-erasure under contention, NOT strict-FIFO-by-ts. Earlier spec
    // framing ("queue order matches seeded-timestamp order") was rhetorical
    // inheritance from v1.0 routing fix; corrected in v7 §propose_replanning
    // "Queue ordering semantics" — queue order is arrival order, lock-
    // acquire is OS-scheduler-dependent and non-deterministic by design.
    // Moderator's Affordance C selects by request_index, not by iteration.
    //
    // Two sibling tests:
    //   r3_no_erasure_under_concurrent_contention — N=4 std::thread::spawn
    //     racing for an Arc<Mutex<state>> (stands in for production's
    //     with_file_lock). Asserts: all 4 entries land (no race-erasure);
    //     each seeded ts appears exactly once (set equality of ts identities,
    //     no duplicates, no missing). NO assertion on queue index order.
    //
    //   r3_serial_seeded_order_preserved_n4 — N=4 sequential calls;
    //     extends r3_precondition_explicit_ts_is_honored from N=3 to N=4
    //     per spec v7 framing. Discriminates "lock honors caller-provided
    //     ts" from "lock overrides with now()". Serial path, no contention.
    //
    // Refs: project_multi_writer_audit_2026-05-13.md,
    // feedback_audit_class_not_just_symbol.md.
    // ============================================================

    /// R3 — N=4 concurrent writers racing under Mutex<state> (stands in for
    /// production's with_file_lock at the dispatcher). Verifies the multi-
    /// writer audit invariant: every writer's entry lands, no ts duplicated,
    /// no ts missing. Order within the queue is arrival order = lock-acquire
    /// order = scheduler-dependent; per v7 spec correction we verify set
    /// equality (no-erasure), not arrival ordering.
    ///
    /// SCOPE NOTE (per dev-challenger msg 1985): Arc<Mutex<state>> exercises
    /// in-process THREAD contention. Production's with_file_lock at
    /// vaak-mcp.rs:598 takes a project_dir and locks .vaak/board.lock —
    /// it serializes across PROCESSES via OS file-locking. Different blast
    /// radius: this fixture catches in-process race bugs in apply-layer
    /// state mutation; a future tempdir-harnessed integration test would
    /// catch file-lock-specific bugs (premature release, lock-file creation
    /// race, OS-specific semantics). v1 R3 covers the apply-layer invariant
    /// per spec v7; cross-process file-lock parity is a v1.6 follow-up.
    #[test]
    fn r3_no_erasure_under_concurrent_contention() {
        use std::sync::{Arc, Mutex};
        use std::thread;

        let state = Arc::new(Mutex::new(fresh_state()));
        {
            let mut s = state.lock().unwrap();
            s["floor"]["phase"] = serde_json::json!("execution");
        }

        let seeds: Vec<(&'static str, &'static str, i64)> = vec![
            ("developer:0", "thread-0 plan gap", 1_700_000_000_000),
            ("developer:1", "thread-1 plan gap", 1_700_000_000_001),
            ("tester:0", "thread-2 plan gap", 1_700_000_000_002),
            ("ux-engineer:0", "thread-3 plan gap", 1_700_000_000_003),
        ];
        let n = seeds.len();

        let handles: Vec<_> = seeds
            .into_iter()
            .enumerate()
            .map(|(i, (actor, reason, ts))| {
                let state = Arc::clone(&state);
                thread::spawn(move || {
                    let mut s = state.lock().unwrap();
                    apply_propose_replanning(&mut s, actor, reason, Some(ts))
                        .unwrap_or_else(|e| panic!("thread {} failed: {}", i, e));
                })
            })
            .collect();

        for h in handles {
            h.join().expect("thread panicked");
        }

        let s = state.lock().unwrap();
        let queue = s["floor"]["replanning_requests"].as_array().unwrap();
        assert_eq!(
            queue.len(),
            n,
            "all {} writers must land — multi-writer audit (no race-erasure)",
            n
        );

        let mut ts_set: Vec<i64> = queue
            .iter()
            .map(|e| e["ts"].as_i64().expect("each entry must have an i64 ts"))
            .collect();
        ts_set.sort();
        assert_eq!(
            ts_set,
            vec![
                1_700_000_000_000,
                1_700_000_000_001,
                1_700_000_000_002,
                1_700_000_000_003,
            ],
            "each seeded ts must appear exactly once — no entries dropped, no duplicates"
        );
    }

    /// R3 — N=4 serial seeded calls; queue index order matches seeded ts
    /// order under no-contention. Extends r3_precondition_explicit_ts_is_
    /// honored (N=3) to N=4 per spec v7 line 289 framing. Discriminates
    /// "lock honors caller-provided ts" from "lock overrides with now()".
    #[test]
    fn r3_serial_seeded_order_preserved_n4() {
        let mut s = fresh_state();
        s["floor"]["phase"] = serde_json::json!("execution");
        let seeds = [
            1_700_000_000_000_i64,
            1_700_000_000_001,
            1_700_000_000_002,
            1_700_000_000_003,
        ];
        for (i, ts) in seeds.iter().enumerate() {
            apply_propose_replanning(
                &mut s,
                "developer:1",
                &format!("serial-call-{}", i),
                Some(*ts),
            )
            .unwrap_or_else(|e| panic!("serial call {} failed: {}", i, e));
        }
        let queue = s["floor"]["replanning_requests"].as_array().unwrap();
        assert_eq!(queue.len(), 4);
        for (i, ts) in seeds.iter().enumerate() {
            assert_eq!(
                queue[i]["ts"].as_i64().unwrap(),
                *ts,
                "queue[{}] must match seeded ts {} (serial path preserves order)",
                i,
                ts
            );
        }
    }

    // ============================================================
    // Collaborative-proposal-workflow v1 — Commit Q
    // R4 accept_replanning role gate (mirrors v1.X §Item 4 phase-flip)
    // R5 accept_replanning atomicity (single-write phase + plan + queue)
    // R6 phase_toggled extended payload — at apply layer this just
    //    verifies the state shape; full event-emission test happens
    //    via integration through do_protocol_mutate → emit
    // ============================================================

    fn fresh_with_replanning_queue() -> serde_json::Value {
        let mut s = fresh_state();
        s["floor"]["phase"] = serde_json::json!("execution");
        s["floor"]["plan_path"] = serde_json::json!(".vaak/design-notes/test-plan.md");
        s["floor"]["plan_hash"] = serde_json::json!("sha256:dead");
        s["floor"]["moderator"] = serde_json::json!("evil-architect:0");
        apply_propose_replanning(&mut s, "developer:1", "gap A", Some(1_700_000_000_000))
            .unwrap();
        apply_propose_replanning(&mut s, "ux-engineer:0", "gap B", Some(1_700_000_000_001))
            .unwrap();
        s
    }

    /// R4 — moderator caller succeeds. Mirrors v1.X §Item 4 phase-flip
    /// predicate.
    #[test]
    fn r4_accept_replanning_moderator_succeeds() {
        let mut s = fresh_with_replanning_queue();
        apply_accept_replanning(&mut s, &serde_json::json!({}), "evil-architect:0")
            .expect("moderator must be accepted");
        assert_eq!(s["floor"]["phase"], "planning");
        assert!(s["floor"]["plan_path"].is_null());
        assert!(s["floor"]["plan_hash"].is_null());
        assert!(s["floor"]["replanning_requests"].as_array().unwrap().is_empty());
    }

    /// R4 — architect privileged role succeeds (even when NOT the seat in
    /// floor.moderator). Mirrors v1.X open_planning + revise_plan gate.
    #[test]
    fn r4_accept_replanning_architect_succeeds() {
        let mut s = fresh_with_replanning_queue();
        apply_accept_replanning(&mut s, &serde_json::json!({}), "architect:0")
            .expect("architect must be accepted");
        assert_eq!(s["floor"]["phase"], "planning");
    }

    /// R4 — human privileged role succeeds.
    #[test]
    fn r4_accept_replanning_human_succeeds() {
        let mut s = fresh_with_replanning_queue();
        apply_accept_replanning(&mut s, &serde_json::json!({}), "human:0")
            .expect("human must be accepted");
        assert_eq!(s["floor"]["phase"], "planning");
    }

    /// R4 — developer (non-moderator, non-privileged) rejects with
    /// [AcceptReplanningForbidden].
    #[test]
    fn r4_accept_replanning_developer_rejects() {
        let mut s = fresh_with_replanning_queue();
        let err = apply_accept_replanning(&mut s, &serde_json::json!({}), "developer:0")
            .unwrap_err();
        assert!(
            err.starts_with("[AcceptReplanningForbidden]"),
            "got: {}",
            err
        );
        // State should be untouched — gate rejects before side effects.
        assert_eq!(s["floor"]["phase"], "execution");
        assert_eq!(
            s["floor"]["replanning_requests"].as_array().unwrap().len(),
            2
        );
    }

    /// R4 — dev-challenger (non-moderator, non-privileged) rejects.
    #[test]
    fn r4_accept_replanning_dev_challenger_rejects() {
        let mut s = fresh_with_replanning_queue();
        let err = apply_accept_replanning(&mut s, &serde_json::json!({}), "dev-challenger:0")
            .unwrap_err();
        assert!(err.starts_with("[AcceptReplanningForbidden]"), "got: {}", err);
    }

    /// R5 — atomicity at the apply layer: all four side effects land
    /// together when accept_replanning succeeds. (The full lock-mediated
    /// observer-thread test rides on the outer with_file_lock — that's
    /// integration-level; this apply-layer test pins the in-state shape.)
    #[test]
    fn r5_accept_replanning_atomic_side_effects() {
        let mut s = fresh_with_replanning_queue();
        // Pre-state: execution + plan + 2 requests.
        assert_eq!(s["floor"]["phase"], "execution");
        assert_eq!(
            s["floor"]["plan_path"].as_str(),
            Some(".vaak/design-notes/test-plan.md")
        );
        assert_eq!(
            s["floor"]["replanning_requests"].as_array().unwrap().len(),
            2
        );

        apply_accept_replanning(&mut s, &serde_json::json!({}), "evil-architect:0").unwrap();

        // Post-state: planning + cleared plan + drained queue. All four.
        assert_eq!(s["floor"]["phase"], "planning");
        assert!(s["floor"]["plan_path"].is_null());
        assert!(s["floor"]["plan_hash"].is_null());
        assert!(s["floor"]["replanning_requests"].as_array().unwrap().is_empty());
    }

    /// R5 — out-of-bounds request_index rejects with [InvalidArgs] and
    /// leaves state untouched. Same pattern as set_moderator/InvalidArgs.
    #[test]
    fn r5_accept_replanning_out_of_bounds_index_rejects() {
        let mut s = fresh_with_replanning_queue();
        let err = apply_accept_replanning(
            &mut s,
            &serde_json::json!({"request_index": 99}),
            "evil-architect:0",
        )
        .unwrap_err();
        assert!(err.starts_with("[InvalidArgs]"), "got: {}", err);
        assert_eq!(s["floor"]["phase"], "execution");
        assert_eq!(
            s["floor"]["replanning_requests"].as_array().unwrap().len(),
            2
        );
    }

    /// R5 — request_index in bounds is accepted; same atomic side effects.
    /// (Event payload's triggered_by is derived later in
    /// emit_two_controls_event from pre-state queue at this index.)
    #[test]
    fn r5_accept_replanning_with_request_index_succeeds() {
        let mut s = fresh_with_replanning_queue();
        apply_accept_replanning(
            &mut s,
            &serde_json::json!({"request_index": 0}),
            "evil-architect:0",
        )
        .expect("in-bounds request_index must succeed");
        assert_eq!(s["floor"]["phase"], "planning");
    }

    /// R5.A (apply-layer half per evil-arch msg 1906 #3 + spec v7 line 292) —
    /// current_speaker is set with stale heartbeat at accept_replanning time;
    /// phase still flips cleanly. The watchdog floor_stall cleanup of the
    /// phantom current_speaker is the integration-level half (async, ~30s
    /// post-tick) and lives in a future tempdir-harnessed integration test.
    ///
    /// Apply-layer claim verified here: accept_replanning does NOT check
    /// current_speaker liveness — that's the watchdog's domain (separation of
    /// concerns from v1.X Bug 2 fix at 9a672d4). A dead speaker on the floor
    /// at pivot time does not block the moderator's accept; the four atomic
    /// side effects (phase, plan_path, plan_hash, replanning_requests) land
    /// per W1 atomicity regardless. current_speaker remains as a phantom
    /// field until watchdog releases via floor_stall — UNTESTED here, lives
    /// in integration.
    #[test]
    fn r5_a_dead_speaker_phantom_at_pivot_phase_flips_cleanly() {
        let mut s = fresh_with_replanning_queue();
        // Simulate a dead/AFK current_speaker mid-working-turn. Stale-heartbeat
        // semantics live in sessions/<role>-<inst>.json:last_alive_at_ms not in
        // protocol.json — the apply-layer can't see them. What we CAN verify
        // is that current_speaker presence on the floor doesn't block the
        // accept_replanning side effects.
        s["floor"]["current_speaker"] = serde_json::json!("developer:1");
        apply_accept_replanning(&mut s, &serde_json::json!({}), "evil-architect:0")
            .expect("accept_replanning must not be blocked by current_speaker liveness state");
        // All four atomic side effects land:
        assert_eq!(s["floor"]["phase"], "planning");
        assert!(s["floor"]["plan_path"].is_null());
        assert!(s["floor"]["plan_hash"].is_null());
        assert!(s["floor"]["replanning_requests"].as_array().unwrap().is_empty());
        // Phantom current_speaker remains — accept doesn't clear it; the
        // watchdog floor_stall path does (integration-tested, not here).
        assert_eq!(
            s["floor"]["current_speaker"].as_str(),
            Some("developer:1"),
            "phantom current_speaker stays until watchdog releases (separation of concerns)"
        );
    }

    /// R5.B (Commit M) — planning_blocks_working gate per v1.1 §A2 +
    /// collab-proposal-workflow-spec-2026-05-15.md §W2. apply_mic_claim
    /// with turn_type="working" during planning phase rejects with
    /// [planning_blocks_working]. Other turn_types stay allowed (planning
    /// IS the discussion phase).
    #[test]
    fn r5b_planning_blocks_working_turn_type() {
        let mut s = fresh_state();
        s["floor"]["phase"] = serde_json::json!("planning");
        s["floor"]["current_speaker"] = serde_json::json!("developer:1");
        let err = apply_mic_claim(
            &mut s,
            &serde_json::json!({"turn_type": "working"}),
            "developer:1",
        )
        .unwrap_err();
        assert!(
            err.starts_with("[planning_blocks_working]"),
            "got: {}",
            err
        );
    }

    /// R5.B — execution phase allows working turn_type (regression guard).
    #[test]
    fn r5b_execution_allows_working_turn_type() {
        let mut s = fresh_state();
        s["floor"]["phase"] = serde_json::json!("execution");
        s["floor"]["current_speaker"] = serde_json::json!("developer:1");
        apply_mic_claim(
            &mut s,
            &serde_json::json!({"turn_type": "working"}),
            "developer:1",
        )
        .expect("execution phase must allow working turn_type");
    }

    /// R5.B — planning phase allows non-working turn_types (reviewing,
    /// thinking, passing). Planning IS the discussion phase.
    #[test]
    fn r5b_planning_allows_reviewing_thinking_passing() {
        for turn_type in ["reviewing", "thinking", "passing"] {
            let mut s = fresh_state();
            s["floor"]["phase"] = serde_json::json!("planning");
            s["floor"]["current_speaker"] = serde_json::json!("developer:1");
            apply_mic_claim(
                &mut s,
                &serde_json::json!({"turn_type": turn_type}),
                "developer:1",
            )
            .unwrap_or_else(|e| {
                panic!("planning phase must allow turn_type={}: {}", turn_type, e)
            });
        }
    }

    // ============================================================
    // Strict-turn-discipline + review-intensity-slider (Commit S)
    // S1 set_review_intensity role gate
    // S2 set_review_intensity range 1-10
    // S3 set_review_intensity back-compat (fresh state default = 5)
    // ============================================================

    #[test]
    fn s1_set_review_intensity_moderator_succeeds() {
        let mut s = fresh_state();
        s["floor"]["moderator"] = serde_json::json!("evil-architect:0");
        apply_set_review_intensity(
            &mut s,
            &serde_json::json!({"level": 7}),
            "evil-architect:0",
        )
        .expect("moderator must succeed");
        assert_eq!(s["floor"]["review_intensity"], 7);
    }

    #[test]
    fn s1_set_review_intensity_architect_succeeds() {
        let mut s = fresh_state();
        apply_set_review_intensity(&mut s, &serde_json::json!({"level": 8}), "architect:0")
            .expect("architect must succeed");
        assert_eq!(s["floor"]["review_intensity"], 8);
    }

    #[test]
    fn s1_set_review_intensity_human_succeeds() {
        let mut s = fresh_state();
        apply_set_review_intensity(&mut s, &serde_json::json!({"level": 10}), "human:0")
            .expect("human must succeed");
        assert_eq!(s["floor"]["review_intensity"], 10);
    }

    #[test]
    fn s1_set_review_intensity_developer_rejects() {
        let mut s = fresh_state();
        let err =
            apply_set_review_intensity(&mut s, &serde_json::json!({"level": 5}), "developer:0")
                .unwrap_err();
        assert!(
            err.starts_with("[SetReviewIntensityForbidden]"),
            "got: {}",
            err
        );
    }

    #[test]
    fn s2_set_review_intensity_range_below_rejects() {
        let mut s = fresh_state();
        let err =
            apply_set_review_intensity(&mut s, &serde_json::json!({"level": 0}), "architect:0")
                .unwrap_err();
        assert!(err.starts_with("[InvalidArgs]"), "got: {}", err);
    }

    #[test]
    fn s2_set_review_intensity_range_above_rejects() {
        let mut s = fresh_state();
        let err =
            apply_set_review_intensity(&mut s, &serde_json::json!({"level": 11}), "architect:0")
                .unwrap_err();
        assert!(err.starts_with("[InvalidArgs]"), "got: {}", err);
    }

    #[test]
    fn s2_set_review_intensity_missing_level_rejects() {
        let mut s = fresh_state();
        let err = apply_set_review_intensity(&mut s, &serde_json::json!({}), "architect:0")
            .unwrap_err();
        assert!(err.starts_with("[InvalidArgs]"), "got: {}", err);
    }

    #[test]
    fn s3_set_review_intensity_default_is_5() {
        let s = fresh_state();
        assert_eq!(s["floor"]["review_intensity"], 5);
    }

    /// R6 — accept_replanning with empty queue still succeeds (the
    /// moderator can pre-empt without any open request). Spec doesn't
    /// require non-empty queue as a precondition — request_index
    /// out-of-bounds is the boundary check, not queue-non-empty.
    #[test]
    fn r6_accept_replanning_empty_queue_succeeds() {
        let mut s = fresh_state();
        s["floor"]["phase"] = serde_json::json!("execution");
        s["floor"]["moderator"] = serde_json::json!("evil-architect:0");
        // No requests in queue.
        apply_accept_replanning(&mut s, &serde_json::json!({}), "evil-architect:0")
            .expect("empty queue does not block accept");
        assert_eq!(s["floor"]["phase"], "planning");
    }

    /// F5 (dev-challenger msg 1115): A9 PlanPathOutsideDesignNotes must fire
    /// for outside paths, not get swallowed by PlanPathMissing.
    #[test]
    fn f5_outside_path_with_separator_rejects_with_correct_variant() {
        let dir = plan_test_dir("f5_outside");
        // Create the file at the outside path so PlanPathMissing wouldn't fire
        // on its own — we want to verify the pre-resolution outside-check.
        std::fs::create_dir_all(dir.join("other")).unwrap();
        let outside = dir.join("other").join("p.md");
        std::fs::write(&outside, "# x\n<!-- scope: * -->\n").unwrap();
        let pd = dir.to_string_lossy().to_string();
        let mut s = fresh_state();
        let err = apply_accept_plan(
            &mut s,
            &serde_json::json!({"plan_path": "other/p.md"}),
            "architect:0",
            &pd,
        )
        .unwrap_err();
        // F5 fix: should now fire PlanPathOutsideDesignNotes specifically,
        // not the PlanPathMissing fallback the original test accepted.
        assert!(
            err.starts_with("[PlanPathOutsideDesignNotes]"),
            "expected outside-variant, got: {}",
            err
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn f5_absolute_path_rejects_with_outside_variant() {
        let dir = plan_test_dir("f5_abs");
        let pd = dir.to_string_lossy().to_string();
        let mut s = fresh_state();
        let err = apply_accept_plan(
            &mut s,
            &serde_json::json!({"plan_path": "/etc/passwd.md"}),
            "architect:0",
            &pd,
        )
        .unwrap_err();
        assert!(err.starts_with("[PlanPathOutsideDesignNotes]"), "got: {}", err);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a7_revise_plan_role_gate_accepts_human() {
        let mut s = fresh_state();
        let dir = plan_test_dir("a7_human");
        let plan_rel = write_plan(&dir, "p.md", "*");
        let pd = dir.to_string_lossy().to_string();
        apply_revise_plan(
            &mut s,
            &serde_json::json!({"plan_path": &plan_rel, "revision_note": "human override"}),
            "human:0",
            &pd,
        )
        .expect("human must be accepted");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A7c — moderator-authority Item 4: moderator can call accept_plan.
    /// Closes evil-arch msg 1490 CRITICAL phase-gate gap.
    #[test]
    fn a7c_accept_plan_moderator_accepts() {
        let mut s = fresh_state();
        s["floor"]["mic_passing_mode"] = serde_json::json!("moderator");
        s["floor"]["moderator"] = serde_json::json!("ux-engineer:0");
        let dir = plan_test_dir("a7c_mod");
        let plan_rel = write_plan(&dir, "p.md", "*");
        let pd = dir.to_string_lossy().to_string();
        apply_accept_plan(
            &mut s,
            &serde_json::json!({"plan_path": &plan_rel}),
            "ux-engineer:0",
            &pd,
        )
        .expect("moderator must be accepted");
        assert_eq!(s["floor"]["phase"], "execution");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A7d — non-moderator non-privileged caller of accept_plan rejected.
    #[test]
    fn a7d_accept_plan_unauthorized_rejected() {
        let mut s = fresh_state();
        // moderator is ux-engineer:0; developer:0 is neither moderator nor privileged.
        s["floor"]["mic_passing_mode"] = serde_json::json!("moderator");
        s["floor"]["moderator"] = serde_json::json!("ux-engineer:0");
        let dir = plan_test_dir("a7d_unauth");
        let plan_rel = write_plan(&dir, "p.md", "*");
        let pd = dir.to_string_lossy().to_string();
        let err = apply_accept_plan(
            &mut s,
            &serde_json::json!({"plan_path": &plan_rel}),
            "developer:0",
            &pd,
        )
        .unwrap_err();
        assert!(
            err.starts_with("[AcceptPlanForbidden]"),
            "developer:0 must be rejected, got: {}",
            err
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A7e — privileged role (architect) bypasses moderator requirement.
    #[test]
    fn a7e_accept_plan_architect_accepts_no_moderator() {
        let mut s = fresh_state();
        // No moderator set, but architect role is privileged → still accepts.
        let dir = plan_test_dir("a7e_arch");
        let plan_rel = write_plan(&dir, "p.md", "*");
        let pd = dir.to_string_lossy().to_string();
        apply_accept_plan(
            &mut s,
            &serde_json::json!({"plan_path": &plan_rel}),
            "architect:0",
            &pd,
        )
        .expect("architect bypass must accept");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A8c — moderator-authority Item 4: moderator can call open_planning.
    #[test]
    fn a8c_open_planning_moderator_accepts() {
        let mut s = fresh_state();
        s["floor"]["mic_passing_mode"] = serde_json::json!("moderator");
        s["floor"]["moderator"] = serde_json::json!("ux-engineer:0");
        s["floor"]["phase"] = serde_json::json!("execution");
        apply_open_planning(&mut s, "ux-engineer:0").expect("moderator accepted");
        assert_eq!(s["floor"]["phase"], "planning");
    }

    /// A8d — non-moderator non-privileged caller of open_planning rejected.
    #[test]
    fn a8d_open_planning_unauthorized_rejected() {
        let mut s = fresh_state();
        s["floor"]["mic_passing_mode"] = serde_json::json!("moderator");
        s["floor"]["moderator"] = serde_json::json!("ux-engineer:0");
        let err = apply_open_planning(&mut s, "developer:0").unwrap_err();
        assert!(
            err.starts_with("[OpenPlanningForbidden]"),
            "developer:0 must be rejected, got: {}",
            err
        );
    }

    /// is_seat_exempt helper — derived per moderator-authority spec line 25.
    #[test]
    fn is_seat_exempt_moderator_mode_with_moderator_set() {
        let mut s = fresh_state();
        s["floor"]["mic_passing_mode"] = serde_json::json!("moderator");
        s["floor"]["moderator"] = serde_json::json!("ux-engineer:0");
        assert!(is_seat_exempt(&s, "ux-engineer:0"));
        assert!(!is_seat_exempt(&s, "developer:0"));
    }

    #[test]
    fn is_seat_exempt_not_moderator_mode_returns_false() {
        let mut s = fresh_state();
        s["floor"]["mic_passing_mode"] = serde_json::json!("rotation");
        s["floor"]["moderator"] = serde_json::json!("ux-engineer:0");
        // Even though moderator field is set, mode != moderator → not exempt.
        assert!(!is_seat_exempt(&s, "ux-engineer:0"));
    }

    #[test]
    fn is_seat_exempt_moderator_mode_no_moderator_set_returns_false() {
        let mut s = fresh_state();
        s["floor"]["mic_passing_mode"] = serde_json::json!("moderator");
        // moderator field null → no one is exempt.
        assert!(!is_seat_exempt(&s, "ux-engineer:0"));
    }

    /// A8 — scope-block required at accept_plan time (architect msg 992 G2).
    #[test]
    fn a8_scope_block_required() {
        let dir = plan_test_dir("a8_missing");
        let path = dir.join(".vaak").join("design-notes").join("noscope.md");
        std::fs::write(&path, "# Plan\n\nNo scope block.\n").unwrap();
        let pd = dir.to_string_lossy().to_string();
        let mut s = fresh_state();
        let err = apply_accept_plan(
            &mut s,
            &serde_json::json!({"plan_path": ".vaak/design-notes/noscope.md"}),
            "architect:0",
            &pd,
        )
        .unwrap_err();
        assert!(
            err.starts_with("[PlanScopeBlockMissing]"),
            "got: {}",
            err
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a8_scope_block_unrestricted_accepts() {
        let dir = plan_test_dir("a8_star");
        let plan_rel = write_plan(&dir, "p.md", "*");
        let pd = dir.to_string_lossy().to_string();
        let mut s = fresh_state();
        apply_accept_plan(
            &mut s,
            &serde_json::json!({"plan_path": &plan_rel}),
            "architect:0",
            &pd,
        )
        .expect("scope:* must accept");
        assert_eq!(s["floor"]["phase"], "execution");
        assert_eq!(s["floor"]["plan_path"], plan_rel);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A9 — plan_path allowlist (evil-arch msg 988 M2).
    #[test]
    fn a9_plan_path_outside_design_notes_rejects() {
        let dir = plan_test_dir("a9_out");
        std::fs::create_dir_all(dir.join("other")).unwrap();
        let outside = dir.join("other").join("p.md");
        std::fs::write(&outside, "# x\n<!-- scope: * -->\n").unwrap();
        let pd = dir.to_string_lossy().to_string();
        let mut s = fresh_state();
        let err = apply_accept_plan(
            &mut s,
            &serde_json::json!({"plan_path": "other/p.md"}),
            "architect:0",
            &pd,
        )
        .unwrap_err();
        // F5 fix: pre-resolution outside-check now fires the correct variant.
        assert!(
            err.starts_with("[PlanPathOutsideDesignNotes]"),
            "got: {}",
            err
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a9_plan_path_dotdot_rejects() {
        let dir = plan_test_dir("a9_dotdot");
        let pd = dir.to_string_lossy().to_string();
        let mut s = fresh_state();
        let err = apply_accept_plan(
            &mut s,
            &serde_json::json!({"plan_path": "../escape/p.md"}),
            "architect:0",
            &pd,
        )
        .unwrap_err();
        assert!(
            err.starts_with("[PlanPathOutsideDesignNotes]"),
            "got: {}",
            err
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a9_plan_path_not_markdown_rejects() {
        let dir = plan_test_dir("a9_ext");
        let pd = dir.to_string_lossy().to_string();
        let mut s = fresh_state();
        let err = apply_accept_plan(
            &mut s,
            &serde_json::json!({"plan_path": "foo.txt"}),
            "architect:0",
            &pd,
        )
        .unwrap_err();
        assert!(err.starts_with("[PlanPathNotMarkdown]"), "got: {}", err);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a9_plan_path_missing_rejects() {
        let dir = plan_test_dir("a9_miss");
        let pd = dir.to_string_lossy().to_string();
        let mut s = fresh_state();
        let err = apply_accept_plan(
            &mut s,
            &serde_json::json!({"plan_path": "ghost.md"}),
            "architect:0",
            &pd,
        )
        .unwrap_err();
        assert!(err.starts_with("[PlanPathMissing]"), "got: {}", err);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A3 — mic-mechanism strict serde.
    #[test]
    fn a3_mic_mechanism_unknown_rejects() {
        let mut s = fresh_state();
        let err = apply_set_mic_passing(
            &mut s,
            &serde_json::json!({"mode": "telepathy"}),
        )
        .unwrap_err();
        assert!(err.starts_with("[UnknownMicMechanism]"), "got: {}", err);
    }

    #[test]
    fn a3_mic_mechanism_three_canonical_modes_accept() {
        for mode in ["rotation", "hand_raise", "moderator"] {
            let mut s = fresh_state();
            apply_set_mic_passing(&mut s, &serde_json::json!({"mode": mode}))
                .expect(mode);
            assert_eq!(s["floor"]["mic_passing_mode"], mode);
        }
    }

    /// A4 — independence axiom: assembly toggle leaves phase unchanged, vice versa.
    /// Note: preset starts as "Default chat" to clear the v1.0.7 interim gate
    /// that blocks direct cross-preset transitions; same setup the production
    /// `assembly_line` MCP tool relies on (caller has already routed through
    /// Default chat before calling enable).
    #[test]
    fn a4_independence_assembly_leaves_phase() {
        let mut s = fresh_state();
        s["preset"] = serde_json::json!("Default chat");
        s["floor"]["phase"] = serde_json::json!("execution");
        s["floor"]["plan_path"] = serde_json::json!(".vaak/design-notes/x.md");
        apply_set_assembly(&mut s, &serde_json::json!({"active": true})).unwrap();
        assert_eq!(s["floor"]["phase"], "execution");
        assert_eq!(s["floor"]["plan_path"], ".vaak/design-notes/x.md");
        apply_set_assembly(&mut s, &serde_json::json!({"active": false})).unwrap();
        assert_eq!(s["floor"]["phase"], "execution");
    }

    #[test]
    fn a4_independence_phase_leaves_assembly() {
        let mut s = fresh_state();
        s["floor"]["assembly_active"] = serde_json::json!(true);
        s["preset"] = serde_json::json!("Assembly Line");
        apply_open_planning(&mut s, "architect:0").unwrap();
        assert_eq!(s["floor"]["assembly_active"], true);
        assert_eq!(s["preset"], "Assembly Line");
    }

    /// raise_hand requires hand_raise mode; queue is idempotent.
    #[test]
    fn raise_hand_requires_mode() {
        let mut s = fresh_state();
        let err = apply_raise_hand(&mut s, "developer:0").unwrap_err();
        assert!(err.starts_with("[NotPermitted]"), "got: {}", err);
    }

    #[test]
    fn raise_hand_idempotent() {
        let mut s = fresh_state();
        apply_set_mic_passing(&mut s, &serde_json::json!({"mode": "hand_raise"})).unwrap();
        apply_raise_hand(&mut s, "developer:0").unwrap();
        apply_raise_hand(&mut s, "developer:0").unwrap();
        let q = s["floor"]["hand_queue"].as_array().unwrap();
        assert_eq!(q.len(), 1);
        assert_eq!(q[0], "developer:0");
    }

    /// grant_mic restricted to moderator role + moderator mode.
    #[test]
    fn grant_mic_requires_moderator_mode() {
        let mut s = fresh_state();
        let err = apply_grant_mic(
            &mut s,
            &serde_json::json!({"target": "developer:0"}),
            "moderator:0",
        )
        .unwrap_err();
        assert!(err.starts_with("[NotPermitted]"), "got: {}", err);
    }

    #[test]
    fn grant_mic_only_moderator_seat() {
        let mut s = fresh_state();
        apply_set_mic_passing(&mut s, &serde_json::json!({"mode": "moderator"})).unwrap();
        s["floor"]["moderator"] = serde_json::json!("moderator:0");
        let err = apply_grant_mic(
            &mut s,
            &serde_json::json!({"target": "developer:0"}),
            "developer:0",
        )
        .unwrap_err();
        assert!(err.starts_with("[NotPermitted]"), "got: {}", err);
    }

    #[test]
    fn grant_mic_happy_path_sets_speaker() {
        let mut s = fresh_state();
        apply_set_mic_passing(&mut s, &serde_json::json!({"mode": "moderator"})).unwrap();
        s["floor"]["moderator"] = serde_json::json!("moderator:0");
        apply_grant_mic(
            &mut s,
            &serde_json::json!({"target": "developer:0"}),
            "moderator:0",
        )
        .unwrap();
        assert_eq!(s["floor"]["current_speaker"], "developer:0");
    }

    /// scope-block parser cases.
    #[test]
    fn scope_block_parse_missing_returns_none() {
        assert!(parse_scope_block("# Plan\n\nNo block here.\n").is_none());
    }

    #[test]
    fn scope_block_parse_star_returns_empty_vec() {
        let out = parse_scope_block("<!-- scope: * -->").unwrap();
        assert!(out.is_empty(), "* unrestricted is empty vec");
    }

    #[test]
    fn scope_block_parse_paths_split_on_whitespace() {
        let out = parse_scope_block("<!-- scope: src/a.rs src/b.rs   src/c.rs -->").unwrap();
        assert_eq!(out, vec!["src/a.rs", "src/b.rs", "src/c.rs"]);
    }

    /// Cascading state cleanup — set_mic_passing(rotation) clears moderator + hand_queue.
    #[test]
    fn set_mic_passing_rotation_clears_stale_state() {
        let mut s = fresh_state();
        // Pretend we were in moderator mode with a moderator + a stale hand_queue entry.
        s["floor"]["mic_passing_mode"] = serde_json::json!("moderator");
        s["floor"]["moderator"] = serde_json::json!("moderator:0");
        s["floor"]["hand_queue"] = serde_json::json!(["developer:0"]);
        apply_set_mic_passing(&mut s, &serde_json::json!({"mode": "rotation"})).unwrap();
        assert_eq!(s["floor"]["moderator"], serde_json::Value::Null);
        let q = s["floor"]["hand_queue"].as_array().unwrap();
        assert!(q.is_empty(), "hand_queue must be cleared");
    }

    /// Mid-turn set_mic_passing is defer-silent per architect msg 1070 (b).
    #[test]
    fn set_mic_passing_mid_turn_is_noop() {
        let mut s = fresh_state();
        s["floor"]["current_speaker"] = serde_json::json!("architect:0");
        s["floor"]["mic_passing_mode"] = serde_json::json!("rotation");
        apply_set_mic_passing(&mut s, &serde_json::json!({"mode": "hand_raise"})).unwrap();
        // Defer-silent: mode stays at rotation while a speaker holds the floor.
        assert_eq!(s["floor"]["mic_passing_mode"], "rotation");
    }

    /// caller_role helper.
    #[test]
    fn caller_role_splits_on_colon() {
        assert_eq!(caller_role("architect:0"), "architect");
        assert_eq!(caller_role("manager:1"), "manager");
        assert_eq!(caller_role("human:0"), "human");
        assert_eq!(caller_role("noslot"), "noslot");
    }

    /// set_assembly toggles assembly_active and coordinates with preset.
    #[test]
    fn set_assembly_true_sets_preset_and_field() {
        let mut s = fresh_state();
        s["preset"] = serde_json::json!("Default chat");
        apply_set_assembly(&mut s, &serde_json::json!({"active": true})).unwrap();
        assert_eq!(s["floor"]["assembly_active"], true);
        assert_eq!(s["preset"], "Assembly Line");
        assert_eq!(s["floor"]["mode"], "round-robin");
    }

    #[test]
    fn set_assembly_false_clears_speaker_and_resets_preset() {
        let mut s = fresh_state();
        s["preset"] = serde_json::json!("Default chat");
        apply_set_assembly(&mut s, &serde_json::json!({"active": true})).unwrap();
        s["floor"]["current_speaker"] = serde_json::json!("architect:0");
        apply_set_assembly(&mut s, &serde_json::json!({"active": false})).unwrap();
        assert_eq!(s["floor"]["assembly_active"], false);
        assert_eq!(s["preset"], "Default chat");
        assert_eq!(s["floor"]["current_speaker"], serde_json::Value::Null);
    }

    /// open_planning clears plan_hash + plan_path.
    #[test]
    fn open_planning_clears_plan_state() {
        let mut s = fresh_state();
        s["floor"]["phase"] = serde_json::json!("execution");
        s["floor"]["plan_path"] = serde_json::json!(".vaak/design-notes/x.md");
        s["floor"]["plan_hash"] = serde_json::json!("abc123");
        apply_open_planning(&mut s, "architect:0").unwrap();
        assert_eq!(s["floor"]["phase"], "planning");
        assert_eq!(s["floor"]["plan_path"], serde_json::Value::Null);
        assert_eq!(s["floor"]["plan_hash"], serde_json::Value::Null);
    }
}

/// Walk up from CWD to find .vaak/project.json
fn find_project_root() -> Option<String> {
    let mut dir = std::env::current_dir().ok()?;
    loop {
        if dir.join(".vaak").join("project.json").exists() {
            return Some(dir.to_string_lossy().replace('\\', "/"));
        }
        if !dir.pop() {
            return None;
        }
    }
}

// ==================== Handler Functions ====================

/// Grandfather global role templates into a project on join.
/// Reads ~/.vaak/role-templates/*.json and adds any missing roles to project.json.
/// Copies matching *.md briefings to .vaak/roles/ if not already present.
/// Idempotent — safe to run on every join.
fn grandfather_role_templates(project_dir: &str, config: &mut serde_json::Value) -> Result<(), String> {
    let templates_dir = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(|h| PathBuf::from(h).join(".vaak").join("role-templates"))
        .unwrap_or_default();
    if !templates_dir.exists() {
        return Ok(()); // No templates directory — nothing to do
    }

    let roles = match config.get_mut("roles").and_then(|r| r.as_object_mut()) {
        Some(r) => r,
        None => return Ok(()), // No roles object — can't add to it
    };

    let mut added_any = false;

    // Scan template directory for .json role definitions
    let entries = std::fs::read_dir(&templates_dir)
        .map_err(|e| format!("Failed to read role-templates: {}", e))?;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let slug = path.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
        if slug.is_empty() || roles.contains_key(&slug) {
            continue; // Already exists in project — don't overwrite
        }

        // Read template definition
        let template_content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let template: serde_json::Value = match serde_json::from_str(&template_content) {
            Ok(v) => v,
            Err(_) => continue,
        };

        // Add created_at timestamp
        let mut role_def = template.clone();
        if let Some(obj) = role_def.as_object_mut() {
            obj.insert("created_at".to_string(), serde_json::json!(utc_now_iso()));
        }

        eprintln!("[vaak-mcp] Grandfathering role template '{}' into project", slug);
        roles.insert(slug.clone(), role_def);
        added_any = true;

        // Copy briefing .md if it exists and project doesn't have one
        let briefing_template = templates_dir.join(format!("{}.md", slug));
        if briefing_template.exists() {
            let roles_dir = Path::new(project_dir).join(".vaak").join("roles");
            let _ = std::fs::create_dir_all(&roles_dir);
            let dest = roles_dir.join(format!("{}.md", slug));
            if !dest.exists() {
                if let Err(e) = std::fs::copy(&briefing_template, &dest) {
                    eprintln!("[vaak-mcp] Failed to copy briefing for '{}': {}", slug, e);
                }
            }
        }
    }

    if added_any {
        // Save updated project.json
        config["updated_at"] = serde_json::json!(utc_now_iso());
        let config_path = project_json_path(project_dir);
        let updated = serde_json::to_string_pretty(config)
            .map_err(|e| format!("Failed to serialize project.json: {}", e))?;
        atomic_write(&config_path, updated.as_bytes())
            .map_err(|e| format!("Failed to write project.json: {}", e))?;
    }

    Ok(())
}

/// Handle project_join: claim a role in a project team
fn handle_project_join(role: &str, project_dir: &str, session_id: &str, section: Option<&str>) -> Result<serde_json::Value, String> {
    let normalized = project_dir.replace('\\', "/");

    // Verify project.json exists
    let mut config = read_project_config(&normalized)?;

    // === GRANDFATHERING: auto-import missing global role templates ===
    grandfather_role_templates(&normalized, &mut config)?;

    // Phase 2 — apply the adversarial-role seed (idempotent): ensure
    // evil-architect + dev-challenger carry adversarial:true in project.json
    // for Phase 4's retro-pass penalty. Inside currency+board lock to avoid the
    // concurrent-seed race (developer:1 msg 1430 #10). Best-effort; a failure
    // here must not block join. Re-reads config if it wrote, so `roles` below
    // reflects the seed.
    if collab::with_currency_and_board_lock(&normalized, || collab::currency::apply_adversarial_seed(&normalized))
        .unwrap_or(false)
    {
        config = read_project_config(&normalized)?;
    }

    // Phase 7 (a) — carry-over on session start. Seeds each seat from the most
    // recent currency-history snapshot (cap 10000 / timed-out → 0) by writing an
    // `init` row, only for seats not yet in balances.json this session (idempotent
    // re-join). Best-effort; a failure must not block join. No-op when there's no
    // prior snapshot (returns 0) — fresh projects keep the standard 10000 lazy-init.
    let _ = collab::with_currency_and_board_lock(&normalized, || collab::currency::apply_carryover(&normalized));

    let roles = config.get("roles").and_then(|r| r.as_object())
        .ok_or("No roles defined in project.json")?;

    // Verify role exists
    let role_def = roles.get(role)
        .ok_or(format!("Role '{}' not found in project.json. Available: {:?}", role, roles.keys().collect::<Vec<_>>()))?;

    let max_instances = role_def.get("max_instances").and_then(|m| m.as_u64()).unwrap_or(1) as u32;
    let role_title = role_def.get("title").and_then(|t| t.as_str()).unwrap_or(role);
    let timeout_secs = config.get("settings")
        .and_then(|s| s.get("heartbeat_timeout_seconds"))
        .and_then(|t| t.as_u64())
        .unwrap_or(120);

    let result = with_file_lock(&normalized, || {
        let mut sessions = read_sessions(&normalized);
        let bindings = sessions.get_mut("bindings")
            .and_then(|b| b.as_array_mut())
            .ok_or("Invalid sessions.json format")?;

        // === GLOBAL STALE SWEEP: remove stale bindings for ALL roles on every join ===
        {
            let now_secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let before_count = bindings.len();
            bindings.retain(|b| {
                // Never sweep our own session — we may be actively rejoining
                if b.get("session_id").and_then(|s| s.as_str()) == Some(session_id) {
                    return true;
                }
                // Keep non-active bindings (already disconnected/revoked)
                if b.get("status").and_then(|s| s.as_str()) != Some("active") {
                    return true;
                }
                let hb = b.get("last_heartbeat").and_then(|h| h.as_str()).unwrap_or("");
                match parse_iso_to_epoch_secs(hb) {
                    Some(hb_secs) => now_secs.saturating_sub(hb_secs) <= timeout_secs,
                    None => false, // No valid heartbeat = stale
                }
            });
            let removed = before_count - bindings.len();
            if removed > 0 {
                eprintln!("[vaak-mcp] Stale sweep: removed {} ghost bindings on join", removed);
            }
        }

        // Check if this session already has a binding for this role
        let existing = bindings.iter().position(|b| {
            b.get("session_id").and_then(|s| s.as_str()) == Some(session_id)
            && b.get("role").and_then(|r| r.as_str()) == Some(role)
        });

        if let Some(idx) = existing {
            // Update heartbeat for existing binding
            let now = utc_now_iso();
            bindings[idx]["last_heartbeat"] = serde_json::json!(now);
            bindings[idx]["status"] = serde_json::json!("active");
            bindings[idx]["activity"] = serde_json::json!("working");
            // Set per-session section if requested
            if let Some(sec) = section {
                bindings[idx]["active_section"] = serde_json::json!(sec);
            }
            let instance = bindings[idx].get("instance").and_then(|i| i.as_u64()).unwrap_or(0) as u32;
            write_sessions(&normalized, &sessions)?;
            return Ok((instance, false));
        }

        // Determine instance: roster-based (if roster exists) or legacy max_instances
        let roster = config.get("roster").and_then(|r| r.as_array());

        let instance = if let Some(roster_slots) = roster {
            // === ROSTER MODE: find a vacant roster slot ===
            let role_slots: Vec<u32> = roster_slots.iter()
                .filter(|s| s.get("role").and_then(|r| r.as_str()) == Some(role))
                .filter_map(|s| s.get("instance").and_then(|i| i.as_u64()).map(|i| i as u32))
                .collect();

            if role_slots.is_empty() {
                return Err(format!("No roster slots for role '{}'. Add one from the Role Repository first.", role));
            }

            // Find a vacant slot (no active session binding for this role:instance)
            let vacant = role_slots.iter().find(|&&inst| {
                !bindings.iter().any(|b| {
                    b.get("role").and_then(|r| r.as_str()) == Some(role)
                        && b.get("instance").and_then(|i| i.as_u64()) == Some(inst as u64)
                        && b.get("status").and_then(|s| s.as_str()) == Some("active")
                })
            });

            match vacant {
                Some(&inst) => inst,
                None => {
                    // All slots occupied — check for stale sessions to replace
                    let now_secs = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();

                    let stale = role_slots.iter().find_map(|&inst| {
                        let idx = bindings.iter().position(|b| {
                            b.get("role").and_then(|r| r.as_str()) == Some(role)
                                && b.get("instance").and_then(|i| i.as_u64()) == Some(inst as u64)
                                && {
                                    let hb = b.get("last_heartbeat").and_then(|h| h.as_str()).unwrap_or("");
                                    match parse_iso_to_epoch_secs(hb) {
                                        Some(hb_secs) => now_secs.saturating_sub(hb_secs) > timeout_secs,
                                        None => true,
                                    }
                                }
                        });
                        idx.map(|i| (inst, i))
                    });

                    match stale {
                        Some((inst, stale_idx)) => {
                            bindings.remove(stale_idx);
                            inst
                        },
                        None => {
                            // Auto-create a new roster slot instead of blocking
                            let mut new_inst = 0u32;
                            while role_slots.contains(&new_inst) {
                                new_inst += 1;
                            }
                            // Append new slot to project.json roster
                            let config_path = project_json_path(&normalized);
                            let config_content = std::fs::read_to_string(&config_path)
                                .map_err(|e| format!("Failed to read project.json: {}", e))?;
                            let mut config_mut: serde_json::Value = serde_json::from_str(&config_content)
                                .map_err(|e| format!("Failed to parse project.json: {}", e))?;
                            if let Some(roster_arr) = config_mut.get_mut("roster").and_then(|r| r.as_array_mut()) {
                                roster_arr.push(serde_json::json!({
                                    "role": role,
                                    "instance": new_inst,
                                    "added_at": utc_now_iso()
                                }));
                            }
                            config_mut["updated_at"] = serde_json::json!(utc_now_iso());
                            let updated = serde_json::to_string_pretty(&config_mut)
                                .map_err(|e| format!("Failed to serialize project.json: {}", e))?;
                            std::fs::write(&config_path, updated)
                                .map_err(|e| format!("Failed to write project.json: {}", e))?;
                            new_inst
                        },
                    }
                }
            }
        } else {
            // === LEGACY MODE (no roster): use max_instances ===
            let active_count = bindings.iter()
                .filter(|b| {
                    b.get("role").and_then(|r| r.as_str()) == Some(role)
                        && b.get("status").and_then(|s| s.as_str()) == Some("active")
                })
                .count() as u32;

            if active_count >= max_instances {
                let now_secs = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                let stale_idx = bindings.iter().position(|b| {
                    if b.get("role").and_then(|r| r.as_str()) != Some(role) { return false; }
                    if b.get("status").and_then(|s| s.as_str()) != Some("active") { return true; }
                    let hb = b.get("last_heartbeat").and_then(|h| h.as_str()).unwrap_or("");
                    if hb.is_empty() { return true; }
                    match parse_iso_to_epoch_secs(hb) {
                        Some(hb_secs) => now_secs.saturating_sub(hb_secs) > timeout_secs,
                        None => true,
                    }
                });
                if let Some(idx) = stale_idx {
                    bindings.remove(idx);
                } else {
                    return Err(format!("Role '{}' is full ({}/{})", role, active_count, max_instances));
                }
            }

            // Auto-assign instance number
            let existing_instances: Vec<u32> = bindings.iter()
                .filter(|b| b.get("role").and_then(|r| r.as_str()) == Some(role))
                .filter_map(|b| b.get("instance").and_then(|i| i.as_u64()).map(|i| i as u32))
                .collect();
            let mut inst = 0u32;
            while existing_instances.contains(&inst) {
                inst += 1;
            }
            inst
        };

        // Remove any stale bindings for this role:instance before creating new one
        bindings.retain(|b| {
            !(b.get("role").and_then(|r| r.as_str()) == Some(role)
                && b.get("instance").and_then(|i| i.as_u64()) == Some(instance as u64))
        });

        let now = utc_now_iso();
        let mut binding = serde_json::json!({
            "role": role,
            "instance": instance,
            "session_id": session_id,
            "claimed_at": now,
            "last_heartbeat": now,
            "status": "active",
            "activity": "working"
        });
        // Set per-session section if requested
        if let Some(sec) = section {
            binding["active_section"] = serde_json::json!(sec);
        }
        bindings.push(binding);

        write_sessions(&normalized, &sessions)?;
        Ok((instance, true))
    })?;

    let (instance, _is_new) = result;

    // Re-seed assembly rotation_order so a seat that joins after the gate was
    // enabled can still take the mic. Skip if already present (re-joiner).
    //
    // Fix-A1 sibling (tech-leader msg 104 greenlight, developer:1 msg 48
    // caveat, evil-arch msg 102 endorsement, human msg 88): gate on
    // `floor.mode == "round-robin"` instead of `asm.active == true`
    // (which projects `preset == "Assembly Line"`). The narrower gate
    // missed Delphi late-joiners — Delphi also round-robin per the
    // preset matrix in apply_set_preset, so it owns rotation_order
    // semantically too. Same string-rot class as the Track B1 dispatcher
    // hook expansion at do_protocol_mutate.
    {
        let seat = format!("{}:{}", role, instance);
        let section = get_active_section(&normalized);
        let _ = with_file_lock(&normalized, || -> Result<(), String> {
            let proto = read_protocol_for_section_value(&normalized, &section);
            let floor_mode = proto
                .get("floor")
                .and_then(|f| f.get("mode"))
                .and_then(|m| m.as_str())
                .unwrap_or("");
            if floor_mode != "round-robin" {
                return Ok(());
            }
            let mut asm = read_assembly_state(&normalized);
            let arr = match asm.get_mut("rotation_order").and_then(|v| v.as_array_mut()) {
                Some(a) => a,
                None => return Ok(()),
            };
            if arr.iter().any(|v| v.as_str() == Some(&seat)) {
                return Ok(());
            }
            arr.push(serde_json::json!(seat));
            write_assembly_state_unlocked(&normalized, &asm)
        });
    }

    // Read role briefing
    let briefing_path = role_briefing_path(&normalized, role);
    let briefing_raw = std::fs::read_to_string(&briefing_path).unwrap_or_default();

    // Character/stats Phase 1 dynamic injection (Y) per human msg 3254 +
    // spec at .vaak/design-notes/character-stats-system-2026-05-16.md.
    // Reads role's stats from project.json + prepends a cognitive-budget
    // framing block to the briefing returned to the agent. Fresh-always:
    // human edits stats via future Roles tab → next project_join reflects.
    // No file-state drift (vs (X) one-shot regen which would defeat
    // editability per `[[feedback_auto_helpful_defeats_explicit_design]]`).
    let briefing = inject_stat_framing(&normalized, role, &briefing_raw);

    // Read last 10 messages directed to this role, this instance, or 'all'
    let my_instance_label = format!("{}:{}", role, instance);
    let all_messages = read_board_filtered(&normalized);
    let my_messages: Vec<&serde_json::Value> = all_messages.iter()
        .filter(|m| {
            let to = m.get("to").and_then(|t| t.as_str()).unwrap_or("");
            to == role || to == my_instance_label || to == "all"
        })
        .collect();
    let recent: Vec<&serde_json::Value> = my_messages.iter().rev().take(10).rev().copied().collect();

    // Build team status
    let sessions = read_sessions(&normalized);
    let bindings = sessions.get("bindings").and_then(|b| b.as_array()).cloned().unwrap_or_default();
    let config = read_project_config(&normalized)?;
    let project_name = config.get("name").and_then(|n| n.as_str()).unwrap_or("Unknown");

    let mut team_status = Vec::new();
    if let Some(roles_obj) = config.get("roles").and_then(|r| r.as_object()) {
        for (slug, rdef) in roles_obj {
            let title = rdef.get("title").and_then(|t| t.as_str()).unwrap_or(slug);
            let active = bindings.iter()
                .filter(|b| b.get("role").and_then(|r| r.as_str()) == Some(slug.as_str())
                    && b.get("status").and_then(|s| s.as_str()) == Some("active"))
                .count();
            team_status.push(serde_json::json!({
                "role": slug,
                "title": title,
                "active": active,
                "status": if active > 0 { "active" } else { "vacant" }
            }));
        }
    }

    // Store active project state
    if let Ok(mut guard) = ACTIVE_PROJECT.lock() {
        *guard = Some(ActiveProjectState {
            project_dir: normalized.clone(),
            role: role.to_string(),
            instance,
            session_id: session_id.to_string(),
        });
    }

    notify_desktop();

    // Bug #3 Part B v2 Phase 1 canary (tester msg 52 / architect msg 54):
    // record which session-id source resolved THIS process's binding into the
    // per-seat liveness file so a regression to `fallback_hash` is greppable
    // in one command instead of waiting for a thousand-row ledger drift to
    // notice that Edit/Test earns are dead again.
    update_seat_cc_session_source(&normalized, role, instance, read_session_source());

    let active_section = get_active_section(project_dir);

    // Advance last_seen_id to the max ID in recent_messages so project_check
    // won't re-deliver these same messages (prevents token waste)
    let max_recent_id = recent.iter()
        .filter_map(|m| m.get("id").and_then(|i| i.as_u64()))
        .max()
        .unwrap_or(0);
    if max_recent_id > 0 {
        let ls_path = last_seen_path(&normalized, session_id, &active_section);
        if let Some(parent) = ls_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&ls_path, serde_json::json!({
            "last_seen_id": max_recent_id,
            "updated_at": utc_now_iso()
        }).to_string());
    }

    // Collect available sections for discoverability
    let sections_dir = vaak_dir(&normalized).join("sections");
    let mut available_sections: Vec<String> = vec!["default".to_string()];
    if sections_dir.exists() {
        if let Ok(entries) = std::fs::read_dir(&sections_dir) {
            for entry in entries.flatten() {
                if entry.path().is_dir() {
                    if let Some(name) = entry.file_name().to_str() {
                        available_sections.push(name.to_string());
                    }
                }
            }
        }
    }

    // Look up roster slot metadata for this role:instance
    let roster_metadata = config.get("roster")
        .and_then(|r| r.as_array())
        .and_then(|roster| {
            roster.iter().find(|s| {
                s.get("role").and_then(|r| r.as_str()) == Some(role)
                    && s.get("instance").and_then(|i| i.as_u64()) == Some(instance as u64)
            })
        })
        .and_then(|slot| slot.get("metadata").cloned())
        .unwrap_or(serde_json::Value::Null);

    Ok(serde_json::json!({
        "status": "joined",
        "project_name": project_name,
        "role_title": role_title,
        "role_slug": role,
        "instance": instance,
        "briefing": briefing,
        "team_status": team_status,
        "recent_messages": recent,
        "active_section": active_section,
        "available_sections": available_sections,
        "highest_message_id": max_recent_id,
        "roster_metadata": roster_metadata
    }))
}

/// Currency Phase 1 commit (b) — record a non-exempt send's earn as an escrow
/// hold. MUST be called inside `with_currency_and_board_lock` (both locks held).
///
/// Accounting model (matches collab::currency::apply_row exactly so live state
/// == replay state): the earn goes into ESCROW, not balance. `escrow_held +=
/// earn` and an `escrow_hold` row (negative amount = funds held) is appended;
/// `balance` is unchanged on send (net 0). The held funds are released to
/// balance later by commit (c) tick processing once `release_turn` matures.
///
/// Lazy-init: a seat with no balance entry is initialized to
/// STARTING_BALANCE_COPPER with exactly one `init` row before the hold.
///
/// balances.json is the authoritative live snapshot (written here every send).
/// NOTE (commit-c follow-up): replay reconstructs escrow_items with
/// release_turn=0 because LedgerRow carries no release_turn field — so a
/// balances.json rebuild-from-ledger is lossy on in-flight escrow timing.
/// Acceptable in Phase 1 (balances.json is primary; replay is recovery-only);
/// commit (c) owns escrow release + should extend the row schema if exact
/// replay fidelity of in-flight escrows is required.
/// Currency on/off gate (human msg 1366). Reads settings.currency_enabled
/// from project.json. Absent or true → currency runs (opt-out semantics);
/// explicit false → all currency processing is skipped.
fn currency_enabled(dir: &str) -> bool {
    let path = std::path::Path::new(dir).join(".vaak").join("project.json");
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| {
            v.get("settings")
                .and_then(|s| s.get("currency_enabled"))
                .and_then(|b| b.as_bool())
        })
        .unwrap_or(true)
}

/// Phase 4 (a): pull the first `#<digits>` reference from a message body
/// (e.g. "test: ok #228" → Some(228)). Used to resolve a Test's linked Edit.
fn extract_first_msg_ref(body: &str) -> Option<u64> {
    let bytes = body.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'#' {
            let mut j = i + 1;
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                j += 1;
            }
            if j > i + 1 {
                if let Ok(n) = body[i + 1..j].parse::<u64>() {
                    return Some(n);
                }
            }
        }
        i += 1;
    }
    None
}

/// Phase 4 (a): true iff the currency ledger has an `action_kind == Edit` row
/// whose `ref_msg == msg_id`. This is the Q3 "resolved to a real Edit" check —
/// done in the caller so `classify_action` stays a pure string-and-flag fn.
fn ledger_has_edit_row(dir: &str, msg_id: u64) -> bool {
    let path = collab::currency::currency_jsonl_path(dir);
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return false,
    };
    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(row) = serde_json::from_str::<collab::currency::LedgerRow>(line) {
            if row.ref_msg == Some(msg_id)
                && matches!(row.action_kind, Some(collab::currency::ActionKind::Edit))
            {
                return true;
            }
        }
    }
    false
}

/// Phase 8 (human msg 2262): path to a seat's pending-edit marker, written by
/// the `file-op-claim.py` PostToolUse hook after a real Edit/Write/NotebookEdit.
/// Seat "role:instance" → `.vaak/sessions/role-instance-pending-edit.json`
/// (':' is illegal in Windows filenames; the hook uses the same '-' form).
fn pending_edit_marker_path(dir: &str, seat: &str) -> std::path::PathBuf {
    let safe = seat.replace(':', "-");
    std::path::Path::new(dir)
        .join(".vaak")
        .join("sessions")
        .join(format!("{}-pending-edit.json", safe))
}

/// Peek the pending-edit marker WITHOUT consuming it. Returns
/// `(has_pending_edit, total_lines)`. Best-effort: any read/parse failure ⇒
/// `(false, 0)` (currency is an overlay, never blocks a send). The marker is
/// per-seat and written only by that seat's own process, so no lock is needed
/// (a seat sends serially — it cannot race its own marker).
fn peek_pending_edit(dir: &str, seat: &str) -> (bool, u64) {
    let path = pending_edit_marker_path(dir, seat);
    match std::fs::read_to_string(&path) {
        Ok(content) => match serde_json::from_str::<serde_json::Value>(&content) {
            Ok(v) => {
                let lines = v.get("lines").and_then(|x| x.as_u64()).unwrap_or(0);
                (true, lines)
            }
            // Corrupt/partial marker → treat as "no detected edit" rather than
            // crediting an unknown line count.
            Err(_) => (false, 0),
        },
        Err(_) => (false, 0),
    }
}

/// Consume (delete) a seat's pending-edit marker. Called on the send-accept
/// path AFTER the earn is recorded, so the same file-write work can't be
/// credited to a second message. Best-effort: a delete failure is logged-silent
/// (worst case the next send re-credits, which a fresh marker overwrite avoids).
fn consume_pending_edit(dir: &str, seat: &str) {
    let _ = std::fs::remove_file(pending_edit_marker_path(dir, seat));
}

fn record_currency_earn(
    dir: &str,
    seat: &str,
    action: collab::currency::ActionKind,
    ref_msg: u64,
    linked_edit_msg: Option<u64>,
    // Phase 8 (human msg 2262): line count from the pending-edit marker for a
    // DETECTED Edit. 0 for self-tagged edits (no marker) and all non-Edit kinds.
    edit_lines: u64,
) -> Result<(), String> {
    use collab::currency::*;
    if matches!(action, ActionKind::Exempt) {
        return Ok(());
    }
    // Currency on/off toggle gate (human msg 1366): when disabled in
    // project.json settings, record nothing — no ledger rows, no escrow.
    if !currency_enabled(dir) {
        return Ok(());
    }
    // balances.json is authoritative when present; rebuild from the ledger
    // only if the snapshot is missing but a ledger exists (crash recovery).
    let mut snap = read_balances_snapshot(dir)?;
    if !balances_json_path(dir).exists() && currency_jsonl_path(dir).exists() {
        snap = replay_balances_from_ledger(dir)?;
    }
    let now = collab::iso_now();

    // Lazy-init the seat (exactly one init row per seat).
    if !snap.seats.contains_key(seat) {
        let id = snap.next_txn_id;
        snap.next_txn_id = snap.next_txn_id.saturating_add(1);
        snap.seats.entry(seat.to_string()).or_default().balance = STARTING_BALANCE_COPPER;
        append_currency_transaction(dir, &LedgerRow {
            id,
            txn_type: "init".to_string(),
            seat: seat.to_string(),
            amount: STARTING_BALANCE_COPPER,
            reason: "join: initial balance".to_string(),
            ref_msg: None,
            balance_after: STARTING_BALANCE_COPPER,
            escrow_id: None,
            release_turn: None,
            turn: Some(snap.turn_counter),
            action_kind: Some(ActionKind::Init),
            linked_edit_msg: None,
            at: now.clone(),
        })?;
    }

    let (earn, ticks, action_str) = match action {
        ActionKind::Pass => (PASS_EARN_COPPER, PASS_ESCROW_TICKS, "pass"),
        ActionKind::Speak => (SPEAK_EARN_COPPER, SPEAK_ESCROW_TICKS, "speak"),
        // Phase 8 (human msg 2262): Edit = 25 base + 1 copper/line beyond the
        // bonus threshold (saturating_sub → max(0, lines-100)); self-tagged
        // edits pass edit_lines=0 → base only. Edit escrow matures over its own
        // longer window so the "work pays more" earn is also held longer.
        ActionKind::Edit => (
            EDIT_EARN_COPPER + edit_lines.saturating_sub(EDIT_LINE_BONUS_THRESHOLD) as i64,
            EDIT_ESCROW_TICKS,
            "edit",
        ),
        ActionKind::Test => (TEST_EARN_COPPER, TEST_ESCROW_TICKS, "test"),
        // Exempt + the Phase 2 ledger opcodes are never produced by
        // classify_action for an earn — no-op defensively.
        _ => return Ok(()),
    };

    let escrow_id = next_escrow_id(&mut snap);
    let release_turn = snap.turn_counter.saturating_add(ticks);
    let bal_after = {
        let entry = snap.seats.get_mut(seat).expect("seat present after lazy-init");
        entry.escrow_held = entry.escrow_held.saturating_add(earn);
        entry.escrow_items.push(EscrowItem {
            id: escrow_id.clone(),
            amount: earn,
            release_turn,
            action: action_str.to_string(),
            ref_msg: Some(ref_msg),
        });
        entry.balance
    };

    let hold_id = snap.next_txn_id;
    snap.next_txn_id = snap.next_txn_id.saturating_add(1);
    append_currency_transaction(dir, &LedgerRow {
        id: hold_id,
        txn_type: "escrow_hold".to_string(),
        seat: seat.to_string(),
        amount: -earn, // negative = funds held (apply_row convention)
        reason: format!("{} earn @msg {} (escrow {} ticks)", action_str, ref_msg, ticks),
        ref_msg: Some(ref_msg),
        balance_after: bal_after,
        escrow_id: Some(escrow_id),
        // Commit (c): carry the maturity turn so replay reconstructs the
        // EscrowItem.release_turn faithfully (no more =0 placeholder).
        release_turn: Some(release_turn),
        // Phase 2: turn at write-time + action opcode so Phase 4's retro-scan
        // can find Pass earns by action_kind within a turn window.
        turn: Some(snap.turn_counter),
        action_kind: Some(action),
        // Phase 4 (a): Test earns carry the resolved Edit they certify; Pass/
        // Speak/Edit pass None. Lets the co-liability scan (commit c) walk
        // Test→Edit links.
        linked_edit_msg,
        at: now,
    })?;

    write_balances_snapshot(dir, &snap)?;
    Ok(())
}

/// Phase 2 — currency_objection real impl. Challenge another seat's accepted
/// message. Inside with_currency_and_board_lock (atomic with board + currency):
/// validate, debit OBJECTION_COST from challenger, capture target stake (full
/// escrow if still held → escrow_release row; else 90% of earn clawed back →
/// penalty row), open a dispute (pool = cost + stake), post a board notice.
/// Uses existing apply_row txn types (penalty/escrow_release) so replay works
/// without new handlers. Returns the dispute row JSON.
/// Phase 6 (a) — post a bounty (human-only). House pool is infinite; no debit
/// on post (the amount only leaves the house on approval).
fn handle_currency_post_bounty(description: &str, amount: i64, deadline_turns: u64) -> Result<serde_json::Value, String> {
    use collab::currency::*;
    let state = get_or_rejoin_state()?;
    let dir = state.project_dir.clone();
    let poster = format!("{}:{}", state.role, state.instance);
    if !poster.starts_with("human:") {
        return Err("[Bounty] only human:* can post bounties.".to_string());
    }
    if amount <= 0 { return Err("[Bounty] amount must be > 0.".to_string()); }
    if deadline_turns == 0 { return Err("[Bounty] deadline_turns must be > 0.".to_string()); }
    collab::with_currency_and_board_lock(&dir, || {
        let mut snap = read_balances_snapshot(&dir)?;
        if snap.seats.is_empty() && currency_jsonl_path(&dir).exists() { snap = replay_balances_from_ledger(&dir)?; }
        let mut bounties = read_open_bounties_snapshot(&dir)?;
        let id_num = snap.next_bounty_id.max(bounties.next_bounty_id);
        let bounty_id = format!("bounty_{:06x}", id_num);
        snap.next_bounty_id = id_num + 1;
        bounties.next_bounty_id = id_num + 1;
        let now = collab::iso_now();
        let deadline_turn = snap.turn_counter + deadline_turns;
        let row = BountyRow {
            id: bounty_id.clone(), description: description.to_string(), amount,
            posted_by: poster.clone(), deadline_turn, status: "open".to_string(),
            claimant: None, claim_stake: 0, submission_msg: None, approved_by: None,
            last_rejection_reason: None, posted_at: now.clone(), resolved_at: None,
            turn_posted: snap.turn_counter,
        };
        append_bounty_row(&dir, &row)?;
        bounties.bounties.insert(bounty_id.clone(), row);
        write_open_bounties_snapshot(&dir, &bounties)?;
        write_balances_snapshot(&dir, &snap)?;
        let msg_id = next_message_id(&dir);
        let _ = append_to_board(&dir, &serde_json::json!({
            "id": msg_id, "from": "system", "to": "all", "type": "broadcast",
            "timestamp": utc_now_iso(),
            "subject": format!("[bounty] new — {} copper", amount),
            "body": format!("Bounty {}: {} — {} copper, deadline turn {}.", bounty_id, description, amount, deadline_turn),
            "metadata": { "bounty_id": bounty_id, "amount": amount, "deadline_turn": deadline_turn }
        }));
        Ok(serde_json::json!({ "bounty_id": bounty_id, "amount": amount, "deadline_turn": deadline_turn }))
    })
}

/// Phase A v2.2 — oxford_initiate handler. Resolves caller from session,
/// reads sessions.json for the active_seats roster, validates per spec §3.1
/// gates, then writes the Initiate event + active-oxford-debate.json snapshot
/// under the oxford lock. Emits a board broadcast to notify all participants.
///
/// Discussion-mode auto-disable (spec §4.3) deferred to a follow-up commit
/// alongside the project_send gate hook — keeps this commit atomic.
fn handle_oxford_initiate(
    moderator: &str,
    side_a: &[String],
    side_b: &[String],
    premise: &str,
    audience: &[String],
    winning_side_reward_copper: Option<i64>,
) -> Result<serde_json::Value, String> {
    use collab::oxford::*;
    let state = get_or_rejoin_state()?;
    let dir = state.project_dir.clone();
    let caller = format!("{}:{}", state.role, state.instance);

    // Build active_seats from sessions.json bindings (status=="active",
    // non-human roles only — human:0 is always valid in any seat role).
    let active_seats: Vec<String> = (|| -> Vec<String> {
        let sessions_str = std::fs::read_to_string(sessions_json_path(&dir)).ok();
        let sessions: serde_json::Value = sessions_str
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_else(|| serde_json::json!({"bindings": []}));
        let mut out = Vec::new();
        if let Some(bindings) = sessions.get("bindings").and_then(|b| b.as_array()) {
            for b in bindings {
                if b.get("status").and_then(|s| s.as_str()) != Some("active") { continue; }
                let role = b.get("role").and_then(|s| s.as_str()).unwrap_or("");
                if role.is_empty() || role == "human" { continue; }
                let instance = b.get("instance").and_then(|i| i.as_u64()).unwrap_or(0);
                out.push(format!("{}:{}", role, instance));
            }
        }
        out
    })();

    // Per architect msg 527 ruling: reward 0 is allowed (no-reward debate),
    // negative is rejected. None defaults to economy.json
    // oxford_default_winning_reward_copper (settings-overridable per commit 2d).
    if let Some(r) = winning_side_reward_copper {
        if r < 0 {
            return Err("[OxfordInvalidReward] winning_side_reward_copper must be >= 0 (0 = no-reward debate; negative not allowed).".to_string());
        }
    }
    let oxford_settings_default_reward = collab::currency::read_economy_settings(&dir).oxford_default_winning_reward_copper;
    // Pure validation up front (no lock yet).
    validate_initiate(&caller, moderator, side_a, side_b, audience, &active_seats)?;

    collab::oxford::with_oxford_lock(&dir, || {
        // Re-check no active debate is in progress (atomic under lock).
        if read_active_oxford(&dir)?.is_some() {
            return Err("[OxfordAlreadyActive]".to_string());
        }
        // Phase A v2.2 §4.3 — auto-disable any other discussion mode on initiate
        // (per human msg 458 "disables any other debate or discussion type when
        // you initiate it" + evil-arch msg 524 spec-inconsistency resolution).
        // Best-effort: read discussion.json, set mode=null, write back. Errors
        // here are logged but don't abort the debate-initiate (discussion.json
        // may not exist on a fresh project).
        let disc_path = std::path::Path::new(&dir).join(".vaak").join("discussion.json");
        if disc_path.exists() {
            let prior_mode = std::fs::read_to_string(&disc_path).ok()
                .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
                .and_then(|v| v.get("mode").and_then(|m| m.as_str()).map(|s| s.to_string()));
            if let Some(prior) = prior_mode {
                if !prior.is_empty() && prior != "null" {
                    let disabled = serde_json::json!({ "mode": serde_json::Value::Null });
                    if let Err(e) = std::fs::write(&disc_path, serde_json::to_string_pretty(&disabled).unwrap_or_default()) {
                        eprintln!("[oxford_initiate] WARN: failed to auto-disable discussion mode '{}': {}", prior, e);
                    }
                }
            }
        }
        let debate_id = next_debate_id(&dir);
        let now = collab::iso_now();
        let reward = winning_side_reward_copper.unwrap_or(oxford_settings_default_reward);

        // Build and persist snapshot.
        let debate = ActiveOxfordDebate {
            debate_id,
            moderator: moderator.to_string(),
            side_a: side_a.to_vec(),
            side_b: side_b.to_vec(),
            audience: audience.to_vec(),
            premise: premise.to_string(),
            current_speaker: None,
            started_at: now.clone(),
            turn_history: Vec::new(),
            winning_side_reward_copper: reward,
        };
        write_active_oxford(&dir, &debate)?;

        // Append the Initiate event to the audit log.
        append_oxford_event(&dir, &OxfordEvent::Initiate {
            debate_id,
            timestamp: now.clone(),
            moderator: moderator.to_string(),
            side_a: side_a.to_vec(),
            side_b: side_b.to_vec(),
            premise: premise.to_string(),
            audience: audience.to_vec(),
            winning_side_reward_copper: reward,
        })?;

        // Board broadcast — every participant gets the notification.
        let msg_id = next_message_id(&dir);
        let _ = append_to_board(&dir, &serde_json::json!({
            "id": msg_id, "from": "system", "to": "all", "type": "broadcast",
            "timestamp": utc_now_iso(),
            "subject": format!("[OxfordDebateInitiated] debate {} by {}", debate_id, moderator),
            "body": format!(
                "Oxford-style debate {} initiated by {}.\nPremise: {}\nSide A: {}\nSide B: {}\nAudience: {}\nModerator declares speakers via oxford_declare_speaker. Reward on strict-majority audience vote: {} copper from pool. Per spec §6.3, only the declared speaker can project_send during their turn; human:0 always bypasses.",
                debate_id, moderator, premise,
                side_a.join(", "), side_b.join(", "),
                if audience.is_empty() { "(none)".to_string() } else { audience.join(", ") },
                reward
            ),
            "metadata": {
                "debate_id": debate_id,
                "moderator": moderator,
                "side_a": side_a,
                "side_b": side_b,
                "audience": audience,
                "winning_side_reward_copper": reward,
                "oxford_event": "initiate"
            }
        }));
        // Commit 2d.4 (MCP path): directed pings to each debater so they
        // wake up and discover their assignment (closes code-interpreter
        // msg 894 TODO). Audience + moderator skipped (observers /
        // already-engaged respectively).
        for seat in side_a.iter() {
            let ping_id = next_message_id(&dir);
            let _ = append_to_board(&dir, &serde_json::json!({
                "id": ping_id, "from": "system", "to": seat, "type": "directive",
                "timestamp": utc_now_iso(),
                "subject": format!("[OxfordDebateAssignment] debate {} — you are on side_a", debate_id),
                "body": format!(
                    "You have been selected for Oxford debate {} as a side_a debater.\n\nPremise: {}\n\nModerator: {} will declare speakers via oxford_declare_speaker. Only the declared speaker can project_send during their turn — wait for the moderator to call on you. Winning side splits {} copper from the pool.\n\nPer spec §6.3, non-speaker debaters can use oxford_react for visual reactions (rate-limited).",
                    debate_id, premise, moderator, reward
                ),
                "metadata": {
                    "debate_id": debate_id,
                    "assigned_side": "side_a",
                    "oxford_event": "debater_assigned"
                }
            }));
        }
        for seat in side_b.iter() {
            let ping_id = next_message_id(&dir);
            let _ = append_to_board(&dir, &serde_json::json!({
                "id": ping_id, "from": "system", "to": seat, "type": "directive",
                "timestamp": utc_now_iso(),
                "subject": format!("[OxfordDebateAssignment] debate {} — you are on side_b", debate_id),
                "body": format!(
                    "You have been selected for Oxford debate {} as a side_b debater.\n\nPremise: {}\n\nModerator: {} will declare speakers via oxford_declare_speaker. Only the declared speaker can project_send during their turn — wait for the moderator to call on you. Winning side splits {} copper from the pool.\n\nPer spec §6.3, non-speaker debaters can use oxford_react for visual reactions (rate-limited).",
                    debate_id, premise, moderator, reward
                ),
                "metadata": {
                    "debate_id": debate_id,
                    "assigned_side": "side_b",
                    "oxford_event": "debater_assigned"
                }
            }));
        }
        // Layer 1 (msg 1005 architect ruling): directed prompt to the
        // moderator with the literal next action. Closes the "moderator
        // never auto-prompted" failure mode dev-challenger msg 1000
        // diagnosed live. Debate sits with empty turn_history until
        // moderator acts — without this prompt, a fresh/idle moderator
        // has no signal that they need to open the floor.
        let opener = side_a.first().cloned().unwrap_or_default();
        let mod_prompt_id = next_message_id(&dir);
        let _ = append_to_board(&dir, &serde_json::json!({
            "id": mod_prompt_id, "from": "system", "to": moderator, "type": "directive",
            "timestamp": utc_now_iso(),
            "subject": format!("[OxfordModeratorPrompt] debate {} — open the floor", debate_id),
            "body": format!(
                "You are the moderator for Oxford debate {}.\n\nNext action: call `oxford_declare_speaker seat=\"{}\"` to open the floor (side_a opens by convention per spec §3.2).\n\nIf no speaker is declared within the opener-grace window, the floor will auto-open with side_a[0] as the opening speaker and broadcast [OxfordAutoOpened]. You may continue moderating subsequent rotations normally.",
                debate_id, opener
            ),
            "metadata": {
                "debate_id": debate_id,
                "suggested_opener": opener,
                "oxford_event": "moderator_prompt"
            }
        }));

        Ok(serde_json::json!({
            "debate_id": debate_id,
            "moderator": moderator,
            "side_a": side_a,
            "side_b": side_b,
            "audience": audience,
            "premise": premise,
            "winning_side_reward_copper": reward,
            "started_at": now,
        }))
    })
}

/// Phase A v2.2 commit 3 — oxford_declare_speaker. Moderator-only.
/// Records the speaker change, updates current_speaker + turn_history,
/// broadcasts to the team. Per spec §3.2.
fn handle_oxford_declare_speaker(seat: &str) -> Result<serde_json::Value, String> {
    use collab::oxford::*;
    let state = get_or_rejoin_state()?;
    let dir = state.project_dir.clone();
    let caller = format!("{}:{}", state.role, state.instance);
    collab::oxford::with_oxford_lock(&dir, || {
        let mut debate = read_active_oxford(&dir)?
            .ok_or_else(|| "[NoActiveOxfordDebate]".to_string())?;
        if caller != debate.moderator {
            return Err("[OxfordModeratorOnly]".to_string());
        }
        let in_a = debate.side_a.iter().any(|s| s == seat);
        let in_b = debate.side_b.iter().any(|s| s == seat);
        if !in_a && !in_b {
            return Err("[OxfordNonDebaterCannotSpeak]".to_string());
        }
        // Layer 3 of architect msg 1012 ruling: first-writer-wins. If the
        // current open turn was created by the opener-grace sweeper
        // (auto_opened=true) and is still in-flight (ended_at=None),
        // reject this declare so the auto-opener's turn completes
        // naturally. Moderator retains authority for the NEXT rotation.
        if let Some(open_turn) = debate.turn_history.last() {
            if open_turn.ended_at.is_none() && open_turn.auto_opened {
                let debate_id = debate.debate_id;
                let current = open_turn.seat.clone();
                // Best-effort board broadcast so moderator sees why their
                // declare was rejected (per architect msg 1012 spec).
                let msg_id = next_message_id(&dir);
                let _ = append_to_board(&dir, &serde_json::json!({
                    "id": msg_id, "from": "system", "to": &debate.moderator, "type": "directive",
                    "timestamp": utc_now_iso(),
                    "subject": format!(
                        "[OxfordDeclareDeferred] debate {} — auto-opener {} mid-turn",
                        debate_id, current
                    ),
                    "body": format!(
                        "Your `oxford_declare_speaker seat=\"{}\"` call was deferred because debate {} is in an auto-opened opening turn ({} currently holds the floor). Per spec, the auto-opener's turn completes first; you retain authority for every subsequent rotation. Re-call oxford_declare_speaker after the current turn yields or hits the per-turn hard limit.",
                        seat, debate_id, current
                    ),
                    "metadata": {
                        "debate_id": debate_id,
                        "rejected_seat": seat,
                        "current_speaker": current,
                        "oxford_event": "declare_deferred"
                    }
                }));
                return Err("[OxfordDeclareDeferred] auto-opener's turn in flight; re-call after it yields".to_string());
            }
        }
        let now = collab::iso_now();
        // Close the previous turn (set ended_at on the last open turn).
        if let Some(prev) = debate.turn_history.last_mut() {
            if prev.ended_at.is_none() {
                prev.ended_at = Some(now.clone());
            }
        }
        debate.turn_history.push(OxfordTurn {
            seat: seat.to_string(),
            started_at: now.clone(),
            ended_at: None,
            auto_opened: false,
        });
        debate.current_speaker = Some(seat.to_string());
        let debate_id = debate.debate_id;
        write_active_oxford(&dir, &debate)?;
        append_oxford_event(&dir, &OxfordEvent::SpeakerDeclared {
            debate_id,
            timestamp: now.clone(),
            seat: seat.to_string(),
        })?;
        let msg_id = next_message_id(&dir);
        let _ = append_to_board(&dir, &serde_json::json!({
            "id": msg_id, "from": "system", "to": "all", "type": "broadcast",
            "timestamp": utc_now_iso(),
            "subject": format!("[OxfordSpeakerDeclared] debate {} — {}", debate_id, seat),
            "body": format!("Moderator {} declares {} as the next speaker (debate {}). 60s soft / 120s hard floor per spec §6.2.", caller, seat, debate_id),
            "metadata": { "debate_id": debate_id, "speaker": seat, "oxford_event": "speaker_declared" }
        }));
        Ok(serde_json::json!({ "debate_id": debate_id, "current_speaker": seat, "turn_started_at": now }))
    })
}

/// Phase A v2.2 commit 3 — oxford_end. Moderator-only. Writes the Ended
/// event with the moderator's announced outcome, clears active-oxford-
/// debate.json. Reward distribution per spec §6.1 v2.2 deferred to a
/// follow-up commit (gated on pool_balance from plan v2 §3b); this
/// handler emits reward_distributed=None.
fn handle_oxford_end(outcome: &str) -> Result<serde_json::Value, String> {
    use collab::oxford::*;
    let state = get_or_rejoin_state()?;
    let dir = state.project_dir.clone();
    let caller = format!("{}:{}", state.role, state.instance);
    let valid_outcomes = ["side_a_wins", "side_b_wins", "draw", "abandoned"];
    if !valid_outcomes.iter().any(|v| *v == outcome) {
        return Err(format!("[OxfordInvalidOutcome] outcome must be one of {:?}", valid_outcomes));
    }
    collab::oxford::with_oxford_lock(&dir, || {
        let mut debate = read_active_oxford(&dir)?
            .ok_or_else(|| "[NoActiveOxfordDebate]".to_string())?;
        if caller != debate.moderator {
            return Err("[OxfordModeratorOnly]".to_string());
        }
        let now = collab::iso_now();
        // Close current speaker's open turn (if any).
        if let Some(prev) = debate.turn_history.last_mut() {
            if prev.ended_at.is_none() {
                prev.ended_at = Some(now.clone());
            }
        }
        let debate_id = debate.debate_id;
        // Tally audience votes from the event log (per spec §5 + Lock #v2.2-2
        // strict-majority gate). Human:0 vote recorded separately per Lock #v2.2-4
        // COI audit + msg 489 #8 separate-tally semantics.
        let mut tally_a = 0i64;
        let mut tally_b = 0i64;
        let mut tally_draw = 0i64;
        let mut human_vote: Option<String> = None;
        let log_path = oxford_debates_jsonl_path(&dir);
        if log_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&log_path) {
                for line in content.lines() {
                    if let Ok(OxfordEvent::AudienceVote { debate_id: did, voter, vote, .. }) = serde_json::from_str::<OxfordEvent>(line) {
                        if did != debate_id { continue; }
                        if voter.starts_with("human:") {
                            human_vote = Some(vote.clone());
                            continue; // human vote tracked separately
                        }
                        match vote.as_str() {
                            "side_a" => tally_a += 1,
                            "side_b" => tally_b += 1,
                            "draw" => tally_draw += 1,
                            _ => {}
                        }
                    }
                }
            }
        }
        let total_nonabstain = tally_a + tally_b; // "draw" counts as abstain for reward
        let strict_majority_winner: Option<&str> = if total_nonabstain == 0 {
            None
        } else if tally_a * 2 > total_nonabstain {
            Some("side_a")
        } else if tally_b * 2 > total_nonabstain {
            Some("side_b")
        } else {
            None
        };
        let audience_tally_json = serde_json::json!({
            "side_a": tally_a,
            "side_b": tally_b,
            "draw": tally_draw,
            "strict_majority_winner": strict_majority_winner,
        });
        // Reward distribution: per spec §6.1 v2.2 POOL-FUNDED. The pool_balance
        // field is plan v2 §3b territory (not yet shipped at the time of this
        // commit), so the actual debit-from-pool + credit-to-winners is a TODO.
        // For now: emit reward_distributed = 0 when winner exists, signaling
        // the mechanism is wired but unfunded. When §3b lands, swap the
        // 0-emit for the real pool_debit + credit logic per spec §6.1 v2.2.
        let reward_distributed: i64 = match strict_majority_winner {
            Some(_) => 0, // TODO: distribute from pool_balance when §3b ships
            None => 0,
        };
        append_oxford_event(&dir, &OxfordEvent::Ended {
            debate_id,
            timestamp: now.clone(),
            outcome: outcome.to_string(),
            audience_tally_nonhuman: Some(audience_tally_json),
            audience_human_vote: human_vote.clone(),
            reward_distributed: Some(reward_distributed),
        })?;
        clear_active_oxford(&dir)?;
        let msg_id = next_message_id(&dir);
        let _ = append_to_board(&dir, &serde_json::json!({
            "id": msg_id, "from": "system", "to": "all", "type": "broadcast",
            "timestamp": utc_now_iso(),
            "subject": format!("[OxfordDebateEnded] debate {} — {}", debate_id, outcome),
            "body": format!("Moderator {} ends debate {} with outcome '{}'. Audience-vote + reward distribution will be added in a follow-up commit (deferred per spec §6.1 dependency on pool_balance §3b).", caller, debate_id, outcome),
            "metadata": { "debate_id": debate_id, "outcome": outcome, "oxford_event": "ended" }
        }));
        Ok(serde_json::json!({ "debate_id": debate_id, "outcome": outcome, "ended_at": now }))
    })
}

/// Phase A v2.2 commit 3 — oxford_kick. Moderator-only. Removes a seat from
/// the role they were in; if the seat was the active speaker, current_speaker
/// is cleared and the moderator must declare the next speaker.
fn handle_oxford_kick(seat: &str) -> Result<serde_json::Value, String> {
    use collab::oxford::*;
    let state = get_or_rejoin_state()?;
    let dir = state.project_dir.clone();
    let caller = format!("{}:{}", state.role, state.instance);
    collab::oxford::with_oxford_lock(&dir, || {
        let mut debate = read_active_oxford(&dir)?
            .ok_or_else(|| "[NoActiveOxfordDebate]".to_string())?;
        if caller != debate.moderator {
            return Err("[OxfordModeratorOnly]".to_string());
        }
        if seat == debate.moderator {
            return Err("[OxfordCannotKickModerator]".to_string());
        }
        let was_in_a = debate.side_a.iter().any(|s| s == seat);
        let was_in_b = debate.side_b.iter().any(|s| s == seat);
        let was_in_aud = debate.audience.iter().any(|s| s == seat);
        if !(was_in_a || was_in_b || was_in_aud) {
            return Err("[OxfordSeatNotInDebate]".to_string());
        }
        debate.side_a.retain(|s| s != seat);
        debate.side_b.retain(|s| s != seat);
        debate.audience.retain(|s| s != seat);
        let now = collab::iso_now();
        let was_active_speaker = debate.current_speaker.as_deref() == Some(seat);
        if was_active_speaker {
            if let Some(prev) = debate.turn_history.last_mut() {
                if prev.ended_at.is_none() { prev.ended_at = Some(now.clone()); }
            }
            debate.current_speaker = None;
        }
        let debate_id = debate.debate_id;
        write_active_oxford(&dir, &debate)?;
        append_oxford_event(&dir, &OxfordEvent::Kicked {
            debate_id,
            timestamp: now.clone(),
            seat: seat.to_string(),
        })?;
        let msg_id = next_message_id(&dir);
        let _ = append_to_board(&dir, &serde_json::json!({
            "id": msg_id, "from": "system", "to": "all", "type": "broadcast",
            "timestamp": utc_now_iso(),
            "subject": format!("[OxfordSeatKicked] debate {} — {} removed", debate_id, seat),
            "body": format!("Moderator {} kicks {} from debate {}. {}", caller, seat, debate_id,
                if was_active_speaker { "Turn auto-passes; moderator must declare next speaker." } else { "" }),
            "metadata": { "debate_id": debate_id, "seat": seat, "was_active_speaker": was_active_speaker, "oxford_event": "kicked" }
        }));
        Ok(serde_json::json!({ "debate_id": debate_id, "kicked": seat, "was_active_speaker": was_active_speaker }))
    })
}

/// Phase A v2.2 commit 5 — oxford_react. Visual-only event for audience and
/// non-speaking debaters. Spec §3.4a. Rate-limited per
/// OXFORD_REACT_RATE_LIMIT_PER_MIN (3) in OXFORD_REACT_RATE_LIMIT_WINDOW_SECS
/// (60) rolling window. NO board message; only oxford-debates.jsonl audit
/// row + the Phase B visualization event consumes it.
fn handle_oxford_react(emoji: &str) -> Result<serde_json::Value, String> {
    use collab::oxford::*;
    let state = get_or_rejoin_state()?;
    let dir = state.project_dir.clone();
    let caller = format!("{}:{}", state.role, state.instance);
    // Commit 2d follow-up: rate-limit-per-min sourced from EconomySettings
    // (window stays at OXFORD_REACT_RATE_LIMIT_WINDOW_SECS since the field is
    // named "_per_min" and the window is implicitly 60s).
    let rate_limit_per_min = collab::currency::read_economy_settings(&dir)
        .oxford_react_rate_limit_per_min;
    // Emoji whitelist per spec §3.4a (rendered by Phase B; any string accepted
    // here but documented choices keep visualization sprite-mapping sane).
    let valid_emoji = ["clap", "boo", "gasp", "laugh", "applause"];
    if !valid_emoji.iter().any(|v| *v == emoji) {
        return Err(format!("[OxfordInvalidEmoji] emoji must be one of {:?}", valid_emoji));
    }
    collab::oxford::with_oxford_lock(&dir, || {
        let debate = read_active_oxford(&dir)?
            .ok_or_else(|| "[NoActiveOxfordDebate]".to_string())?;
        // Caller-gate per spec §3.4a:
        if caller == debate.moderator {
            return Err("[OxfordModeratorCannotReact]".to_string());
        }
        if debate.current_speaker.as_deref() == Some(caller.as_str()) {
            return Err("[OxfordSpeakerCannotReact]".to_string());
        }
        let in_debate = debate.side_a.iter().any(|s| s == &caller)
            || debate.side_b.iter().any(|s| s == &caller)
            || debate.audience.iter().any(|s| s == &caller);
        if !in_debate {
            return Err("[OxfordNonParticipantCannotReact]".to_string());
        }
        // Rate-limit: scan recent React events for this caller in the rolling window.
        let log_path = oxford_debates_jsonl_path(&dir);
        let now_iso = collab::iso_now();
        if log_path.exists() {
            let recent_count: u64 = std::fs::read_to_string(&log_path)
                .ok()
                .map(|content| {
                    let mut n: u64 = 0;
                    for line in content.lines() {
                        if let Ok(ev) = serde_json::from_str::<OxfordEvent>(line) {
                            if let OxfordEvent::React { seat, timestamp, .. } = ev {
                                if seat == caller {
                                    // Rough within-window check: ISO strings are
                                    // lexicographically ordered, so we can compare
                                    // by parsing seconds-since-epoch via the
                                    // timestamp's tail. For simplicity, only count
                                    // events that share the same minute prefix as
                                    // now_iso (truncated to "YYYY-MM-DDTHH:MM") —
                                    // a conservative under-count but never an
                                    // over-count, so rate-limit can only be
                                    // permissive in edge cases.
                                    let now_min = &now_iso[..16.min(now_iso.len())];
                                    let ev_min = &timestamp[..16.min(timestamp.len())];
                                    if ev_min >= now_min {
                                        n += 1;
                                    }
                                }
                            }
                        }
                    }
                    n
                })
                .unwrap_or(0);
            if recent_count >= rate_limit_per_min {
                return Err(format!(
                    "[OxfordReactionRateLimit] max {} reactions per {} seconds; you've used the budget.",
                    rate_limit_per_min, OXFORD_REACT_RATE_LIMIT_WINDOW_SECS
                ));
            }
        }
        let debate_id = debate.debate_id;
        append_oxford_event(&dir, &OxfordEvent::React {
            debate_id,
            timestamp: now_iso.clone(),
            seat: caller.clone(),
            emoji: emoji.to_string(),
        })?;
        Ok(serde_json::json!({
            "debate_id": debate_id,
            "seat": caller,
            "emoji": emoji,
            "timestamp": now_iso,
            "rate_limit_per_min": rate_limit_per_min,
        }))
    })
}

/// Phase A v2.2 commit 5 — oxford_audience_vote. Audience members cast a
/// vote ("side_a" | "side_b" | "draw"). Vote MUST be cast BEFORE oxford_end
/// fires; the tally happens at end-time. Per spec §5 + Lock #v2.2-2
/// strict-majority gate (>50% non-abstain) applied at oxford_end.
fn handle_oxford_audience_vote(vote: &str) -> Result<serde_json::Value, String> {
    use collab::oxford::*;
    let state = get_or_rejoin_state()?;
    let dir = state.project_dir.clone();
    let caller = format!("{}:{}", state.role, state.instance);
    let valid = ["side_a", "side_b", "draw"];
    if !valid.iter().any(|v| *v == vote) {
        return Err(format!("[OxfordInvalidVote] vote must be one of {:?}", valid));
    }
    collab::oxford::with_oxford_lock(&dir, || {
        let debate = read_active_oxford(&dir)?
            .ok_or_else(|| "[NoActiveOxfordDebate]".to_string())?;
        if !debate.audience.iter().any(|s| s == &caller) {
            return Err("[OxfordOnlyAudienceCanVote]".to_string());
        }
        // One vote per caller per debate — scan AudienceVote events for prior vote
        let log_path = oxford_debates_jsonl_path(&dir);
        let debate_id = debate.debate_id;
        if log_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&log_path) {
                for line in content.lines() {
                    if let Ok(OxfordEvent::AudienceVote { debate_id: did, voter, .. }) = serde_json::from_str::<OxfordEvent>(line) {
                        if did == debate_id && voter == caller {
                            return Err("[OxfordAlreadyVoted] one vote per debate.".to_string());
                        }
                    }
                }
            }
        }
        let now_iso = collab::iso_now();
        append_oxford_event(&dir, &OxfordEvent::AudienceVote {
            debate_id,
            timestamp: now_iso.clone(),
            voter: caller.clone(),
            vote: vote.to_string(),
        })?;
        Ok(serde_json::json!({
            "debate_id": debate_id,
            "voter": caller,
            "vote": vote,
            "timestamp": now_iso,
        }))
    })
}

/// Human msg 458 (2026-05-24) — direct balance adjust by human authority.
/// Thin wrapper around `collab::currency::apply_human_adjust` (shared with
/// the Tauri-command path in main.rs); this fn adds the per-MCP-tool
/// concerns: resolve the caller from the active session and emit a board
/// system message announcing the adjust.
fn handle_currency_human_adjust(seat: &str, amount_copper: i64, reason: &str) -> Result<serde_json::Value, String> {
    let state = get_or_rejoin_state()?;
    let dir = state.project_dir.clone();
    let caller = format!("{}:{}", state.role, state.instance);
    let result = collab::currency::apply_human_adjust(&dir, &caller, seat, amount_copper, reason)?;
    // Board broadcast (UX nicety; not part of the audit row, which is
    // already in currency.jsonl).
    let msg_id = next_message_id(&dir);
    let new_bal = result.get("balance_after").and_then(|v| v.as_i64()).unwrap_or(0);
    let direction = if amount_copper >= 0 { "credited" } else { "debited" };
    let abs_amount = amount_copper.abs();
    let _ = append_to_board(&dir, &serde_json::json!({
        "id": msg_id, "from": "system", "to": "all", "type": "broadcast",
        "timestamp": utc_now_iso(),
        "subject": format!("[human_adjust] {} {} {} copper", seat, direction, abs_amount),
        "body": format!("{} adjusted {} by {}{} copper. Reason: {}. New balance: {}.",
            caller, seat,
            if amount_copper >= 0 { "+" } else { "" },
            amount_copper, reason, new_bal),
        "metadata": { "seat": seat, "amount_copper": amount_copper, "reason": reason, "new_balance": new_bal }
    }));
    Ok(result)
}

/// Phase 6 (a) — claim an open bounty, staking 10% of the amount.
fn handle_currency_claim_bounty(bounty_id: &str) -> Result<serde_json::Value, String> {
    use collab::currency::*;
    let state = get_or_rejoin_state()?;
    let dir = state.project_dir.clone();
    let claimant = format!("{}:{}", state.role, state.instance);
    let settings = read_economy_settings(&dir);
    collab::with_currency_and_board_lock(&dir, || {
        let mut bounties = read_open_bounties_snapshot(&dir)?;
        let bounty = read_latest_bounties(&dir).get(bounty_id).cloned()
            .ok_or_else(|| format!("[Bounty] {} not found.", bounty_id))?;
        if bounty.status != "open" { return Err(format!("[Bounty] {} is {}, not open.", bounty_id, bounty.status)); }
        if bounty.claimant.is_some() { return Err(format!("[Bounty] {} already claimed.", bounty_id)); }
        let stake = bounty.amount * settings.bounty_claim_stake_percent as i64 / 100;
        let mut snap = read_balances_snapshot(&dir)?;
        if snap.seats.is_empty() && currency_jsonl_path(&dir).exists() { snap = replay_balances_from_ledger(&dir)?; }
        let bal = snap.seats.get(&claimant).map(|s| s.balance).unwrap_or(STARTING_BALANCE_COPPER);
        if bal < stake { return Err(format!("[Bounty] insufficient balance for claim stake: need {}, have {}.", stake, bal)); }
        let now = collab::iso_now();
        let after = {
            let e = snap.seats.entry(claimant.clone()).or_insert_with(|| { let mut sb = SeatBalance::default(); sb.balance = STARTING_BALANCE_COPPER; sb });
            e.balance = e.balance.saturating_sub(stake);
            e.balance
        };
        let id = snap.next_txn_id; snap.next_txn_id += 1;
        append_currency_transaction(&dir, &LedgerRow {
            id, txn_type: "bounty_stake".to_string(), seat: claimant.clone(),
            amount: -stake, reason: format!("bounty claim stake ({})", bounty_id),
            ref_msg: None, balance_after: after, escrow_id: None, release_turn: None,
            turn: Some(snap.turn_counter), action_kind: Some(ActionKind::BountyStake), linked_edit_msg: None, at: now.clone(),
        })?;
        let mut row = bounty;
        row.status = "claimed".to_string();
        row.claimant = Some(claimant.clone());
        row.claim_stake = stake;
        append_bounty_row(&dir, &row)?;
        bounties.bounties.insert(bounty_id.to_string(), row);
        write_open_bounties_snapshot(&dir, &bounties)?;
        write_balances_snapshot(&dir, &snap)?;
        Ok(serde_json::json!({ "bounty_id": bounty_id, "claimant": claimant, "stake": stake }))
    })
}

/// Phase 6 (a) — claimant abandons a claimed bounty: loses half the stake, gets
/// half back; bounty reopens.
fn handle_currency_abandon_bounty(bounty_id: &str) -> Result<serde_json::Value, String> {
    use collab::currency::*;
    let state = get_or_rejoin_state()?;
    let dir = state.project_dir.clone();
    let actor = format!("{}:{}", state.role, state.instance);
    collab::with_currency_and_board_lock(&dir, || {
        let mut bounties = read_open_bounties_snapshot(&dir)?;
        let bounty = read_latest_bounties(&dir).get(bounty_id).cloned()
            .ok_or_else(|| format!("[Bounty] {} not found.", bounty_id))?;
        if bounty.status != "claimed" { return Err(format!("[Bounty] {} is {}, not claimed.", bounty_id, bounty.status)); }
        if bounty.claimant.as_deref() != Some(actor.as_str()) {
            return Err("[Bounty] only the current claimant can abandon.".to_string());
        }
        let stake = bounty.claim_stake;
        let settings = read_economy_settings(&dir);
        let refund = stake * (100 - settings.bounty_abandon_loss_percent as i64) / 100;
        let destroyed = stake - refund;
        let mut snap = read_balances_snapshot(&dir)?;
        if snap.seats.is_empty() && currency_jsonl_path(&dir).exists() { snap = replay_balances_from_ledger(&dir)?; }
        let now = collab::iso_now();
        let after = {
            let e = snap.seats.entry(actor.clone()).or_insert_with(|| { let mut sb = SeatBalance::default(); sb.balance = STARTING_BALANCE_COPPER; sb });
            e.balance = e.balance.saturating_add(refund);
            e.balance
        };
        let id = snap.next_txn_id; snap.next_txn_id += 1;
        append_currency_transaction(&dir, &LedgerRow {
            id, txn_type: "credit".to_string(), seat: actor.clone(),
            amount: refund, reason: format!("bounty abandoned — half stake returned ({})", bounty_id),
            ref_msg: None, balance_after: after, escrow_id: None, release_turn: None,
            turn: Some(snap.turn_counter), action_kind: Some(ActionKind::Credit), linked_edit_msg: None, at: now.clone(),
        })?;
        let did = snap.next_txn_id; snap.next_txn_id += 1;
        append_currency_transaction(&dir, &LedgerRow {
            id: did, txn_type: "bounty_expire".to_string(), seat: "system:pool".to_string(),
            amount: -destroyed, reason: format!("bounty abandoned — half stake destroyed ({})", bounty_id),
            ref_msg: None, balance_after: 0, escrow_id: None, release_turn: None,
            turn: Some(snap.turn_counter), action_kind: Some(ActionKind::BountyExpire), linked_edit_msg: None, at: now.clone(),
        })?;
        let mut row = bounty;
        row.status = "open".to_string();
        row.claimant = None;
        row.claim_stake = 0;
        append_bounty_row(&dir, &row)?;
        bounties.bounties.insert(bounty_id.to_string(), row);
        write_open_bounties_snapshot(&dir, &bounties)?;
        write_balances_snapshot(&dir, &snap)?;
        Ok(serde_json::json!({ "bounty_id": bounty_id, "refunded": refund, "destroyed": destroyed }))
    })
}

/// Phase 6 (b) — claimant submits their work for an open claim.
/// Status: claimed → submitted, stamps submission_msg.
fn handle_currency_submit_bounty(bounty_id: &str, ref_msg: u64) -> Result<serde_json::Value, String> {
    use collab::currency::*;
    let state = get_or_rejoin_state()?;
    let dir = state.project_dir.clone();
    let actor = format!("{}:{}", state.role, state.instance);
    collab::with_currency_and_board_lock(&dir, || {
        let mut bounties = read_open_bounties_snapshot(&dir)?;
        let bounty = read_latest_bounties(&dir).get(bounty_id).cloned()
            .ok_or_else(|| format!("[Bounty] {} not found.", bounty_id))?;
        if bounty.status != "claimed" {
            return Err(format!("[Bounty] {} is {}, not claimed.", bounty_id, bounty.status));
        }
        if bounty.claimant.as_deref() != Some(actor.as_str()) {
            return Err("[Bounty] only the current claimant can submit.".to_string());
        }
        let now = collab::iso_now();
        let mut row = bounty;
        row.status = "submitted".to_string();
        row.submission_msg = Some(ref_msg);
        append_bounty_row(&dir, &row)?;
        bounties.bounties.insert(bounty_id.to_string(), row);
        write_open_bounties_snapshot(&dir, &bounties)?;
        // Audit board message so peers see the submission appear in the timeline.
        let msg_id = next_message_id(&dir);
        let _ = append_to_board(&dir, &serde_json::json!({
            "id": msg_id, "from": "system", "to": "all", "type": "broadcast",
            "timestamp": now,
            "subject": format!("[bounty] {} submitted by {}", bounty_id, actor),
            "body": format!("Bounty {} submitted by {}, work at msg #{}.", bounty_id, actor, ref_msg),
            "metadata": { "bounty_id": bounty_id, "submission_msg": ref_msg, "claimant": actor }
        }));
        Ok(serde_json::json!({ "bounty_id": bounty_id, "submission_msg": ref_msg, "status": "submitted" }))
    })
}

/// Phase 6 (b) — human/judge approves a submitted bounty.
/// Credits claimant amount + stake (single ledger row, net = +bounty.amount).
fn handle_currency_approve_bounty(bounty_id: &str) -> Result<serde_json::Value, String> {
    use collab::currency::*;
    let state = get_or_rejoin_state()?;
    let dir = state.project_dir.clone();
    let approver = format!("{}:{}", state.role, state.instance);
    if !approver.starts_with("human:") {
        return Err("[Bounty] only human:* can approve bounties.".to_string());
    }
    collab::with_currency_and_board_lock(&dir, || {
        let mut bounties = read_open_bounties_snapshot(&dir)?;
        let bounty = read_latest_bounties(&dir).get(bounty_id).cloned()
            .ok_or_else(|| format!("[Bounty] {} not found.", bounty_id))?;
        if bounty.status != "submitted" {
            return Err(format!("[Bounty] {} is {}, not submitted.", bounty_id, bounty.status));
        }
        let claimant = bounty.claimant.clone()
            .ok_or_else(|| format!("[Bounty] {} has no claimant.", bounty_id))?;
        let payout = bounty.amount.saturating_add(bounty.claim_stake);
        let mut snap = read_balances_snapshot(&dir)?;
        if snap.seats.is_empty() && currency_jsonl_path(&dir).exists() {
            snap = replay_balances_from_ledger(&dir)?;
        }
        let now = collab::iso_now();
        let after = {
            let e = snap.seats.entry(claimant.clone()).or_insert_with(|| {
                let mut sb = SeatBalance::default();
                sb.balance = STARTING_BALANCE_COPPER;
                sb
            });
            e.balance = e.balance.saturating_add(payout);
            e.balance
        };
        let id = snap.next_txn_id; snap.next_txn_id += 1;
        append_currency_transaction(&dir, &LedgerRow {
            id, txn_type: "bounty_earn".to_string(), seat: claimant.clone(),
            amount: payout, reason: format!("bounty {} approved by {} (amount {} + stake {})", bounty_id, approver, bounty.amount, bounty.claim_stake),
            ref_msg: bounty.submission_msg, balance_after: after, escrow_id: None, release_turn: None,
            turn: Some(snap.turn_counter), action_kind: Some(ActionKind::BountyEarn), linked_edit_msg: None, at: now.clone(),
        })?;
        let mut row = bounty;
        row.status = "approved".to_string();
        row.approved_by = Some(approver.clone());
        row.resolved_at = Some(now.clone());
        append_bounty_row(&dir, &row)?;
        bounties.bounties.insert(bounty_id.to_string(), row);
        write_open_bounties_snapshot(&dir, &bounties)?;
        write_balances_snapshot(&dir, &snap)?;
        let msg_id = next_message_id(&dir);
        let _ = append_to_board(&dir, &serde_json::json!({
            "id": msg_id, "from": "system", "to": "all", "type": "broadcast",
            "timestamp": now,
            "subject": format!("[bounty] {} approved — {} earns {} copper", bounty_id, claimant, payout),
            "body": format!("Bounty {} approved by {}. {} earned {} copper (amount {} + stake refund {}).", bounty_id, approver, claimant, payout, "bounty payout", bounty_id),
            "metadata": { "bounty_id": bounty_id, "claimant": claimant, "payout": payout }
        }));
        Ok(serde_json::json!({ "bounty_id": bounty_id, "claimant": claimant, "payout": payout }))
    })
}

/// Phase 6 (b) — human/judge rejects a submitted bounty.
/// Claimant loses FULL stake (already debited at claim; this row audits the
/// pool-destroy). Bounty reopens with `last_rejection_reason` set so a future
/// claimant sees the prior reject.
fn handle_currency_reject_bounty(bounty_id: &str, reason: &str) -> Result<serde_json::Value, String> {
    use collab::currency::*;
    let state = get_or_rejoin_state()?;
    let dir = state.project_dir.clone();
    let rejecter = format!("{}:{}", state.role, state.instance);
    if !rejecter.starts_with("human:") {
        return Err("[Bounty] only human:* can reject bounties.".to_string());
    }
    collab::with_currency_and_board_lock(&dir, || {
        let mut bounties = read_open_bounties_snapshot(&dir)?;
        let bounty = read_latest_bounties(&dir).get(bounty_id).cloned()
            .ok_or_else(|| format!("[Bounty] {} not found.", bounty_id))?;
        if bounty.status != "submitted" {
            return Err(format!("[Bounty] {} is {}, not submitted.", bounty_id, bounty.status));
        }
        let claimant = bounty.claimant.clone().unwrap_or_else(|| "?".to_string());
        let stake = bounty.claim_stake;
        let mut snap = read_balances_snapshot(&dir)?;
        if snap.seats.is_empty() && currency_jsonl_path(&dir).exists() {
            snap = replay_balances_from_ledger(&dir)?;
        }
        let now = collab::iso_now();
        // Audit-only pool-destroy of the already-debited stake; balance unchanged.
        if stake > 0 {
            let id = snap.next_txn_id; snap.next_txn_id += 1;
            append_currency_transaction(&dir, &LedgerRow {
                id, txn_type: "bounty_expire".to_string(), seat: "system:pool".to_string(),
                amount: -stake, reason: format!("bounty {} rejected by {} — stake destroyed: {}", bounty_id, rejecter, reason),
                ref_msg: bounty.submission_msg, balance_after: 0, escrow_id: None, release_turn: None,
                turn: Some(snap.turn_counter), action_kind: Some(ActionKind::BountyExpire), linked_edit_msg: None, at: now.clone(),
            })?;
        }
        let mut row = bounty;
        row.status = "open".to_string();
        row.claimant = None;
        row.claim_stake = 0;
        row.submission_msg = None;
        row.last_rejection_reason = Some(reason.to_string());
        append_bounty_row(&dir, &row)?;
        bounties.bounties.insert(bounty_id.to_string(), row);
        write_open_bounties_snapshot(&dir, &bounties)?;
        write_balances_snapshot(&dir, &snap)?;
        let msg_id = next_message_id(&dir);
        let _ = append_to_board(&dir, &serde_json::json!({
            "id": msg_id, "from": "system", "to": "all", "type": "broadcast",
            "timestamp": now,
            "subject": format!("[bounty] {} rejected — stake destroyed", bounty_id),
            "body": format!("Bounty {} rejected by {}. Claimant {} lost {} copper stake. Reason: {}. Bounty reopens.", bounty_id, rejecter, claimant, stake, reason),
            "metadata": { "bounty_id": bounty_id, "claimant": claimant, "stake_destroyed": stake, "reason": reason }
        }));
        Ok(serde_json::json!({ "bounty_id": bounty_id, "claimant": claimant, "stake_destroyed": stake, "status": "open" }))
    })
}

fn handle_currency_objection(target_msg_id: u64, reason: &str) -> Result<serde_json::Value, String> {
    use collab::currency::*;
    let state = get_or_rejoin_state()?;
    let dir = state.project_dir.clone();
    let challenger = format!("{}:{}", state.role, state.instance);
    if challenger.starts_with("human:") {
        return Err("[Objection] Human is exempt from currency disputes.".to_string());
    }

    collab::with_currency_and_board_lock(&dir, || {
        // 1. Find the target message in the active board.
        let board_path = collab::active_board_path(&dir);
        let board_raw = std::fs::read_to_string(&board_path).unwrap_or_default();
        let target_from = board_raw.lines().filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
            .find(|m| m.get("id").and_then(|v| v.as_u64()) == Some(target_msg_id))
            .and_then(|m| m.get("from").and_then(|v| v.as_str()).map(String::from));
        let target = match target_from {
            Some(t) => t,
            None => return Err(format!("[Objection] target_msg {} not found on the board.", target_msg_id)),
        };
        if target.starts_with("human:") {
            return Err("[Objection] Cannot object to a human message (human is exempt).".to_string());
        }
        if target == challenger {
            return Err("[Objection] You cannot object to your own message.".to_string());
        }

        // 2. Load balances; challenger must afford the cost.
        let settings = read_economy_settings(&dir);
        let mut snap = read_balances_snapshot(&dir)?;
        if snap.seats.is_empty() && currency_jsonl_path(&dir).exists() {
            snap = replay_balances_from_ledger(&dir)?;
        }
        let chal_bal = snap.seats.get(&challenger).map(|s| s.balance).unwrap_or(settings.starting_balance_copper);
        if chal_bal < settings.objection_cost_copper {
            return Err(format!("[Objection] Insufficient balance: need {} copper, have {}.", settings.objection_cost_copper, chal_bal));
        }
        let now = collab::iso_now();
        let mut rows: Vec<LedgerRow> = Vec::new();

        // 3. Debit the objection cost from the challenger (penalty row subtracts).
        let chal_after = {
            let e = snap.seats.entry(challenger.clone()).or_insert_with(|| {
                let mut sb = SeatBalance::default(); sb.balance = settings.starting_balance_copper; sb
            });
            e.balance = e.balance.saturating_sub(settings.objection_cost_copper);
            e.balance
        };
        let id = snap.next_txn_id; snap.next_txn_id += 1;
        rows.push(LedgerRow {
            id, txn_type: "penalty".to_string(), seat: challenger.clone(),
            amount: -settings.objection_cost_copper, reason: format!("objection filed vs {} @msg {}", target, target_msg_id),
            ref_msg: Some(target_msg_id), balance_after: chal_after, escrow_id: None, release_turn: None,
            turn: Some(snap.turn_counter), action_kind: Some(ActionKind::Penalty), linked_edit_msg: None, at: now.clone(),
        });

        // 4. Capture the target's stake. Find the escrow_hold row for target_msg
        // to learn the earn; if the escrow item is still held, the full amount
        // goes to pool (escrow_release row, no balance credit); else 90% clawback.
        //
        // Phase 6 (b) SUPERSEDE: if target_msg is an approved bounty's
        // submission_msg, skip the Phase 2 stake-capture entirely (architect
        // ruling msg 2089 #1). The bounty clawback fires at resolution time
        // (`emit_bounty_clawback`) and does the real economic impact. Pool at
        // open is just the objection cost; the dispute proceeds normally.
        let bounty_supersede = is_approved_bounty_submission(&dir, target_msg_id).is_some();
        let earn = std::fs::read_to_string(currency_jsonl_path(&dir)).unwrap_or_default()
            .lines().filter_map(|l| serde_json::from_str::<LedgerRow>(l).ok())
            .find(|r| r.txn_type == "escrow_hold" && r.ref_msg == Some(target_msg_id))
            .map(|r| r.amount.abs())
            .unwrap_or(0);
        let held_item = if bounty_supersede { None } else {
            snap.seats.get(&target)
                .and_then(|s| s.escrow_items.iter().find(|it| it.ref_msg == Some(target_msg_id)).cloned())
        };
        let stake: i64;
        if bounty_supersede {
            // Skip stake capture entirely; pool is just the 50c objection cost.
            // Resolution-time `emit_bounty_clawback` handles the real economic
            // impact (90% of bounty.amount, split 50/50 challenger/destroyed).
            stake = 0;
        } else if let Some(item) = held_item {
            // Still escrowed → full escrow amount to pool. Remove + decrement.
            stake = item.amount;
            let bal = {
                let e = snap.seats.get_mut(&target).unwrap();
                e.escrow_held = (e.escrow_held - item.amount).max(0);
                e.escrow_items.retain(|it| it.id != item.id);
                e.balance
            };
            let rid = snap.next_txn_id; snap.next_txn_id += 1;
            rows.push(LedgerRow {
                id: rid, txn_type: "escrow_release".to_string(), seat: target.clone(),
                amount: item.amount, reason: format!("escrow → dispute pool (objection @msg {})", target_msg_id),
                ref_msg: Some(target_msg_id), balance_after: bal, escrow_id: Some(item.id), release_turn: None,
                turn: Some(snap.turn_counter), action_kind: Some(ActionKind::EscrowRelease), linked_edit_msg: None, at: now.clone(),
            });
        } else {
            // Escrow already released to balance → claw back (clawback_percent)% of the earn.
            stake = ((earn * settings.clawback_percent as i64) + 99) / 100; // ceil(earn*pct)
            let bal = {
                let e = snap.seats.entry(target.clone()).or_insert_with(|| {
                    let mut sb = SeatBalance::default(); sb.balance = settings.starting_balance_copper; sb
                });
                e.balance = e.balance.saturating_sub(stake);
                if e.balance <= settings.deficit_cap_copper { e.timed_out = true; }
                e.balance
            };
            let rid = snap.next_txn_id; snap.next_txn_id += 1;
            rows.push(LedgerRow {
                id: rid, txn_type: "penalty".to_string(), seat: target.clone(),
                amount: -stake, reason: format!("objection clawback {}% → dispute pool (@msg {})", settings.clawback_percent, target_msg_id),
                ref_msg: Some(target_msg_id), balance_after: bal, escrow_id: None, release_turn: None,
                turn: Some(snap.turn_counter), action_kind: Some(ActionKind::Clawback), linked_edit_msg: None, at: now.clone(),
            });
        }

        // 5. Open the dispute.
        let pool = settings.objection_cost_copper + stake;
        let mut open = read_open_disputes_snapshot(&dir)?;
        let dispute_id = next_dispute_id(&mut open);
        snapshot_add_open(&mut open, &dispute_id, &challenger, &target);
        let dispute = DisputeRow {
            id: dispute_id.clone(), challenger: challenger.clone(), target: target.clone(),
            target_msg: target_msg_id, pool, status: "open".to_string(), resolution: None,
            messages: vec![DisputeMessage { from: challenger.clone(), body: reason.to_string(), added_to_pool: 0, at: now.clone() }],
            judge: None, opened_at: now.clone(), resolved_at: None, turn_opened: snap.turn_counter,
        };

        // 6. Persist: ledger rows, balances, dispute row, open-disputes snapshot.
        for r in &rows { append_currency_transaction(&dir, r)?; }
        write_balances_snapshot(&dir, &snap)?;
        append_dispute_row(&dir, &dispute)?;
        write_open_disputes_snapshot(&dir, &open)?;

        // 7. Post the [Objection] board notice.
        let msg_id = next_message_id(&dir);
        let notice = serde_json::json!({
            "id": msg_id, "from": "system", "to": "all", "type": "broadcast",
            "timestamp": utc_now_iso(),
            "subject": format!("[Objection] {} challenges {} msg {}", challenger, target, target_msg_id),
            "body": format!("[Objection] {} challenges {}'s msg {}. Pool: {} copper. {} must respond — currency_concede or currency_dispute_message (cannot Pass while disputed). Reason: {}",
                challenger, target, target_msg_id, pool, target, reason),
            "metadata": { "dispute_id": dispute_id, "challenger": challenger, "target": target, "pool": pool }
        });
        let _ = append_to_board(&dir, &notice);

        Ok(serde_json::to_value(&dispute).unwrap_or(serde_json::json!({"id": dispute_id, "pool": pool})))
    })
}

// ====================================================================
// Phase 2 commit (b) — Concede + Dispute messages + Auto-judge trigger
// ====================================================================
// Per architect:0 msg 1588 ship-fast cadence (review_intensity=2) + the
// converged Phase 2 spec at `.vaak/design-notes/2026-05-23-currency-
// disputes-phase2-spec.md`. Builds on commit (a) `60b6188` schema +
// helpers (DisputeRow, OpenDisputesSnapshot, snapshot_remove_open,
// append_dispute_row, read_open_disputes_snapshot).

/// Latest-row-per-id read over disputes.jsonl. Disputes are append-only;
/// the LAST row matching `dispute_id` is the authoritative state. Returns
/// None if the dispute doesn't exist. Caller must hold the currency-and-
/// board lock so concurrent appends don't tear the read.
fn read_dispute_by_id(dir: &str, dispute_id: &str) -> Result<Option<collab::currency::DisputeRow>, String> {
    let path = collab::currency::disputes_jsonl_path(dir);
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(&path)
        .map_err(|e| format!("read disputes.jsonl: {}", e))?;
    // Iterate forward; later rows overwrite earlier ones for the same id.
    let mut latest: Option<collab::currency::DisputeRow> = None;
    for line in raw.lines() {
        if line.trim().is_empty() { continue; }
        if let Ok(row) = serde_json::from_str::<collab::currency::DisputeRow>(line) {
            if row.id == dispute_id {
                latest = Some(row);
            }
        }
    }
    Ok(latest)
}

/// Post a system message to the active board.jsonl. Caller must hold the
/// currency-and-board lock. Mirrors the inline notice append in
/// `handle_currency_objection` step 7.
fn append_dispute_system_message(dir: &str, subject: &str, body: &str, metadata: serde_json::Value) -> Result<u64, String> {
    use std::io::Write;
    let msg_id = next_message_id(dir);
    let notice = serde_json::json!({
        "id": msg_id, "from": "system", "to": "all", "type": "broadcast",
        "timestamp": utc_now_iso(),
        "subject": subject,
        "body": body,
        "metadata": metadata,
    });
    let board_path = collab::active_board_path(dir);
    if let Some(parent) = board_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let line = serde_json::to_string(&notice)
        .map_err(|e| format!("serialize system notice: {}", e))?;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&board_path)
        .map_err(|e| format!("open board: {}", e))?;
    writeln!(f, "{}", line).map_err(|e| format!("write board: {}", e))?;
    Ok(msg_id)
}

/// `currency_concede(dispute_id)` — caller (challenger or target) concedes;
/// the OTHER party wins the full pool. Resolves the dispute, removes it
/// from the open-disputes snapshot, posts a system message.
/// Phase 2 (c) — call a judge into a dispute. Debits JUDGE_COST_PER_PARTY (50)
/// from BOTH parties (+100 to pool), sets judge = "human:0". Caller must be a party.
fn handle_currency_call_judge(dispute_id: &str) -> Result<serde_json::Value, String> {
    use collab::currency::*;
    let state = get_or_rejoin_state()?;
    let dir = state.project_dir.clone();
    let caller = format!("{}:{}", state.role, state.instance);
    if caller.starts_with("human:") {
        return Err("[CallJudge] Human is exempt from currency disputes.".to_string());
    }
    collab::with_currency_and_board_lock(&dir, || {
        let mut dispute = read_dispute_by_id(&dir, dispute_id)?
            .ok_or_else(|| format!("[CallJudge] dispute {} not found.", dispute_id))?;
        if dispute.status != "open" {
            return Err(format!("[CallJudge] dispute {} is already {}.", dispute_id, dispute.status));
        }
        if caller != dispute.challenger && caller != dispute.target {
            return Err(format!("[CallJudge] You are not a party to dispute {}.", dispute_id));
        }
        if dispute.judge.is_some() {
            return Err(format!("[CallJudge] dispute {} already has a judge.", dispute_id));
        }
        let now = collab::iso_now();
        let settings = read_economy_settings(&dir);
        let mut snap = read_balances_snapshot(&dir)?;
        if snap.seats.is_empty() && currency_jsonl_path(&dir).exists() { snap = replay_balances_from_ledger(&dir)?; }
        for party in [dispute.challenger.clone(), dispute.target.clone()] {
            let bal = {
                let e = snap.seats.entry(party.clone()).or_insert_with(|| { let mut s = SeatBalance::default(); s.balance = settings.starting_balance_copper; s });
                e.balance = e.balance.saturating_sub(settings.judge_cost_per_party);
                if e.balance <= settings.deficit_cap_copper { e.timed_out = true; }
                e.balance
            };
            let id = snap.next_txn_id; snap.next_txn_id += 1;
            append_currency_transaction(&dir, &LedgerRow {
                id, txn_type: "penalty".to_string(), seat: party.clone(),
                amount: -settings.judge_cost_per_party, reason: format!("call judge for dispute {}", dispute_id),
                ref_msg: Some(dispute.target_msg), balance_after: bal, escrow_id: None, release_turn: None,
                turn: Some(snap.turn_counter), action_kind: Some(ActionKind::Penalty), linked_edit_msg: None, at: now.clone(),
            })?;
        }
        dispute.pool += settings.judge_cost_per_party * 2;
        dispute.judge = Some("human:0".to_string());
        write_balances_snapshot(&dir, &snap)?;
        append_dispute_row(&dir, &dispute)?;
        let _ = append_dispute_system_message(&dir,
            &format!("[Judge invoked] dispute {} — pool {}", dispute_id, dispute.pool),
            &format!("[Judge invoked] {} called a judge into dispute {}. Pool now {} copper. Awaiting human:0 ruling (currency_judge_ruling: challenger_wins | target_wins | both_wrong).", caller, dispute_id, dispute.pool),
            serde_json::json!({"dispute_id": dispute_id, "pool": dispute.pool, "judge": "human:0"}))?;
        Ok(serde_json::to_value(&dispute).unwrap_or(serde_json::json!({"id": dispute_id})))
    })
}

/// Phase 2 (c) — judge ruling. Caller MUST be the dispute's judge. Routes the
/// pool: challenger_wins → credit challenger; target_wins → credit target;
/// both_wrong → pool_destroyed (no credit). System disputes (target=="system")
/// map challenger_wins→reward filer, else→penalty + ban.
fn handle_currency_judge_ruling(dispute_id: &str, ruling: &str) -> Result<serde_json::Value, String> {
    use collab::currency::*;
    let state = get_or_rejoin_state()?;
    let dir = state.project_dir.clone();
    let caller = format!("{}:{}", state.role, state.instance);
    if !matches!(ruling, "challenger_wins" | "target_wins" | "both_wrong") {
        return Err("[Ruling] ruling must be challenger_wins, target_wins, or both_wrong.".to_string());
    }
    collab::with_currency_and_board_lock(&dir, || {
        let mut dispute = read_dispute_by_id(&dir, dispute_id)?
            .ok_or_else(|| format!("[Ruling] dispute {} not found.", dispute_id))?;
        if dispute.status != "open" {
            return Err(format!("[Ruling] dispute {} is already {}.", dispute_id, dispute.status));
        }
        let judge = dispute.judge.clone().ok_or_else(|| format!("[Ruling] dispute {} has no judge (call_judge first).", dispute_id))?;
        if caller != judge {
            return Err(format!("[Ruling] only the judge ({}) can rule on dispute {}.", judge, dispute_id));
        }
        let now = collab::iso_now();
        let settings = read_economy_settings(&dir);
        let pool = dispute.pool;
        let mut snap = read_balances_snapshot(&dir)?;
        if snap.seats.is_empty() && currency_jsonl_path(&dir).exists() { snap = replay_balances_from_ledger(&dir)?; }
        let is_system = dispute.target == "system";

        // Helper to credit a seat + emit a credit row.
        let mut credit_seat = |snap: &mut BalancesSnapshot, seat: &str, amount: i64, reason: String| -> Result<(), String> {
            let bal = {
                let e = snap.seats.entry(seat.to_string()).or_insert_with(|| { let mut s = SeatBalance::default(); s.balance = settings.starting_balance_copper; s });
                e.balance = e.balance.saturating_add(amount);
                e.balance
            };
            let id = snap.next_txn_id; snap.next_txn_id += 1;
            append_currency_transaction(&dir, &LedgerRow {
                id, txn_type: if amount >= 0 { "credit".to_string() } else { "penalty".to_string() },
                seat: seat.to_string(), amount, reason, ref_msg: Some(dispute.target_msg),
                balance_after: bal, escrow_id: None, release_turn: None, turn: Some(snap.turn_counter),
                action_kind: Some(if amount >= 0 { ActionKind::Credit } else { ActionKind::Penalty }), linked_edit_msg: None, at: now.clone(),
            })
        };

        let outcome: String;
        if is_system {
            // System dispute: filer = challenger, judged correct/incorrect.
            let filer = dispute.challenger.clone();
            if ruling == "challenger_wins" {
                credit_seat(&mut snap, &filer, settings.system_dispute_reward, format!("system dispute {} correct — reward", dispute_id))?;
                outcome = format!("correct: {} rewarded {} copper", filer, settings.system_dispute_reward);
            } else {
                // incorrect → additional penalty so total cost = system_dispute_penalty (filing already paid).
                let extra = settings.system_dispute_penalty - settings.system_dispute_cost;
                credit_seat(&mut snap, &filer, -extra, format!("system dispute {} incorrect — penalty", dispute_id))?;
                let ban_until = snap.turn_counter + settings.system_dispute_ban_turns;
                if let Some(e) = snap.seats.get_mut(&filer) {
                    e.system_dispute_ban_until = Some(ban_until);
                }
                // (c.1) Replay-durable ban: audit row carries the until-turn in
                // release_turn so replay_balances_from_ledger reconstructs the ban
                // (apply_row "system_dispute_ban" arm). Without this the ban lives
                // only in the snapshot and a ledger rebuild silently un-bans.
                let bid = snap.next_txn_id; snap.next_txn_id += 1;
                append_currency_transaction(&dir, &LedgerRow {
                    id: bid, txn_type: "system_dispute_ban".to_string(), seat: filer.clone(),
                    amount: 0, reason: format!("system dispute {} incorrect — {}-turn ban", dispute_id, settings.system_dispute_ban_turns),
                    ref_msg: Some(dispute.target_msg), balance_after: snap.seats.get(&filer).map(|e| e.balance).unwrap_or(0),
                    escrow_id: None, release_turn: Some(ban_until), turn: Some(snap.turn_counter),
                    action_kind: None, linked_edit_msg: None, at: now.clone(),
                })?;
                outcome = format!("incorrect: {} penalized (total {} cu) + {}-turn system-dispute ban", filer, settings.system_dispute_penalty, settings.system_dispute_ban_turns);
            }
        } else if ruling == "challenger_wins" {
            credit_seat(&mut snap, &dispute.challenger.clone(), pool, format!("dispute {} — judge ruled challenger wins", dispute_id))?;
            outcome = format!("{} (challenger) wins {} copper", dispute.challenger, pool);
        } else if ruling == "target_wins" {
            credit_seat(&mut snap, &dispute.target.clone(), pool, format!("dispute {} — judge ruled target wins", dispute_id))?;
            outcome = format!("{} (target) wins {} copper", dispute.target, pool);
        } else {
            // both_wrong → pool destroyed, nobody credited.
            let id = snap.next_txn_id; snap.next_txn_id += 1;
            append_currency_transaction(&dir, &LedgerRow {
                id, txn_type: "pool_destroyed".to_string(), seat: "system:pool".to_string(),
                amount: -pool, reason: format!("dispute {} — both wrong, pool destroyed", dispute_id),
                ref_msg: Some(dispute.target_msg), balance_after: 0, escrow_id: None, release_turn: None,
                turn: Some(snap.turn_counter), action_kind: Some(ActionKind::PoolDestroyed), linked_edit_msg: None, at: now.clone(),
            })?;
            outcome = format!("both wrong — pool of {} copper destroyed", pool);
        }

        // Phase 4 (b) — retroactive Pass-penalty hook. Fires only on
        // challenger_wins against a non-system target whose Speak was the row
        // objected. Adversarial filter (Q2=B) + Speak/Edit gating + window
        // scan all live inside `emit_retro_pass_penalties`.
        let retro_count = if !is_system && ruling == "challenger_wins" {
            emit_retro_pass_penalties(&dir, &mut snap, &dispute.target, dispute.target_msg, dispute_id)?
        } else { 0 };
        // Phase 4 (c) — co-liability hook. Same gate; only does anything when the
        // disputed message was an Edit (mutually exclusive with retro-Pass, which
        // only fires on a Speak target).
        let coliab_count = if !is_system && ruling == "challenger_wins" {
            emit_coliability_penalties(&dir, &mut snap, &dispute.target, dispute.target_msg)?
        } else { 0 };
        // Phase 6 (b) — bounty clawback hook. Fires when dispute.target_msg
        // is an approved-bounty submission_msg; 90% of bounty.amount split
        // 50/50 challenger-credit/pool-destroyed. Objection-time supersede
        // already prevented Phase 2 stake-capture so this is the sole
        // economic impact (besides the 50c objection cost in the pool).
        let clawback_amt = if !is_system && ruling == "challenger_wins" {
            emit_bounty_clawback(&dir, &mut snap, dispute.target_msg, &dispute.challenger, dispute_id)?
        } else { 0 };

        let outcome = if retro_count > 0 {
            format!("{} (+{} retro pass {} penalty)", outcome, retro_count, if retro_count == 1 { "row" } else { "rows" })
        } else { outcome };
        let outcome = if coliab_count > 0 {
            format!("{} (+{} co-liability {} penalized)", outcome, coliab_count, if coliab_count == 1 { "tester" } else { "testers" })
        } else { outcome };
        let outcome = if clawback_amt > 0 {
            format!("{} (+bounty clawback: {} copper, half to {})", outcome, clawback_amt, dispute.challenger)
        } else { outcome };

        write_balances_snapshot(&dir, &snap)?;
        dispute.status = "resolved".to_string();
        dispute.resolution = Some(ruling.to_string());
        dispute.resolved_at = Some(now.clone());
        append_dispute_row(&dir, &dispute)?;
        let mut open = read_open_disputes_snapshot(&dir)?;
        snapshot_remove_open(&mut open, dispute_id, &dispute.challenger, &dispute.target);
        write_open_disputes_snapshot(&dir, &open)?;
        let _ = append_dispute_system_message(&dir,
            &format!("[Dispute ruled] {} — {}", dispute_id, ruling),
            &format!("[Dispute ruled] judge {} ruled '{}' on dispute {}: {}.", caller, ruling, dispute_id, outcome),
            serde_json::json!({"dispute_id": dispute_id, "ruling": ruling}))?;
        Ok(serde_json::to_value(&dispute).unwrap_or(serde_json::json!({"id": dispute_id})))
    })
}

/// Phase 2 (c) — file a system dispute (challenge the system itself; human judges).
/// Costs SYSTEM_DISPUTE_COST (50). Rejected if balance < cost or seat is banned.
fn handle_currency_system_dispute(description: &str) -> Result<serde_json::Value, String> {
    use collab::currency::*;
    let state = get_or_rejoin_state()?;
    let dir = state.project_dir.clone();
    let filer = format!("{}:{}", state.role, state.instance);
    if filer.starts_with("human:") {
        return Err("[SystemDispute] Human is exempt.".to_string());
    }
    collab::with_currency_and_board_lock(&dir, || {
        let settings = read_economy_settings(&dir);
        let mut snap = read_balances_snapshot(&dir)?;
        if snap.seats.is_empty() && currency_jsonl_path(&dir).exists() { snap = replay_balances_from_ledger(&dir)?; }
        let bal = snap.seats.get(&filer).map(|s| s.balance).unwrap_or(settings.starting_balance_copper);
        if bal < settings.system_dispute_cost {
            return Err(format!("[SystemDispute] Insufficient balance: need {}, have {}.", settings.system_dispute_cost, bal));
        }
        if let Some(until) = snap.seats.get(&filer).and_then(|s| s.system_dispute_ban_until) {
            if until > snap.turn_counter {
                return Err(format!("[SystemDispute] You are banned from system disputes until turn {} (now {}).", until, snap.turn_counter));
            }
        }
        let now = collab::iso_now();
        // Debit the filing cost.
        let after = {
            let e = snap.seats.entry(filer.clone()).or_insert_with(|| { let mut s = SeatBalance::default(); s.balance = settings.starting_balance_copper; s });
            e.balance = e.balance.saturating_sub(settings.system_dispute_cost);
            e.balance
        };
        let id = snap.next_txn_id; snap.next_txn_id += 1;
        append_currency_transaction(&dir, &LedgerRow {
            id, txn_type: "penalty".to_string(), seat: filer.clone(),
            amount: -settings.system_dispute_cost, reason: "system dispute filed".to_string(),
            ref_msg: None, balance_after: after, escrow_id: None, release_turn: None,
            turn: Some(snap.turn_counter), action_kind: Some(ActionKind::Penalty), linked_edit_msg: None, at: now.clone(),
        })?;
        // Open the dispute (target = "system", judge = human:0).
        let mut open = read_open_disputes_snapshot(&dir)?;
        let dispute_id = next_dispute_id(&mut open);
        snapshot_add_open(&mut open, &dispute_id, &filer, "system");
        let dispute = DisputeRow {
            id: dispute_id.clone(), challenger: filer.clone(), target: "system".to_string(),
            target_msg: 0, pool: settings.system_dispute_cost, status: "open".to_string(), resolution: None,
            messages: vec![DisputeMessage { from: filer.clone(), body: description.to_string(), added_to_pool: 0, at: now.clone() }],
            judge: Some("human:0".to_string()), opened_at: now.clone(), resolved_at: None, turn_opened: snap.turn_counter,
        };
        write_balances_snapshot(&dir, &snap)?;
        append_dispute_row(&dir, &dispute)?;
        write_open_disputes_snapshot(&dir, &open)?;
        let _ = append_dispute_system_message(&dir,
            &format!("[System dispute] {} filed {}", filer, dispute_id),
            &format!("[System dispute] {} filed system dispute {}: {}. human:0 rules via currency_judge_ruling (challenger_wins=correct → +{}, else → -{} total + {}-turn ban).",
                filer, dispute_id, description, settings.system_dispute_reward, settings.system_dispute_penalty, settings.system_dispute_ban_turns),
            serde_json::json!({"dispute_id": dispute_id, "filer": filer, "judge": "human:0"}))?;
        Ok(serde_json::to_value(&dispute).unwrap_or(serde_json::json!({"id": dispute_id})))
    })
}

fn handle_currency_concede(dispute_id: &str) -> Result<serde_json::Value, String> {
    use collab::currency::*;
    let state = get_or_rejoin_state()?;
    let dir = state.project_dir.clone();
    let caller = format!("{}:{}", state.role, state.instance);
    if caller.starts_with("human:") {
        return Err("[Concede] Human is exempt from currency disputes.".to_string());
    }

    collab::with_currency_and_board_lock(&dir, || {
        let mut dispute = read_dispute_by_id(&dir, dispute_id)?
            .ok_or_else(|| format!("[Concede] dispute {} not found.", dispute_id))?;
        if dispute.status != "open" {
            return Err(format!(
                "[Concede] dispute {} is already {} (resolution: {:?}).",
                dispute_id, dispute.status, dispute.resolution
            ));
        }
        if caller != dispute.challenger && caller != dispute.target {
            return Err(format!(
                "[Concede] You are not a party to dispute {} (challenger={}, target={}).",
                dispute_id, dispute.challenger, dispute.target
            ));
        }
        // Winner = the OTHER party.
        let winner = if caller == dispute.target { dispute.challenger.clone() } else { dispute.target.clone() };
        let loser = caller.clone();
        let pool = dispute.pool;
        let now = collab::iso_now();

        // Credit winner full pool.
        let mut snap = read_balances_snapshot(&dir)?;
        if snap.seats.is_empty() && currency_jsonl_path(&dir).exists() {
            snap = replay_balances_from_ledger(&dir)?;
        }
        let win_bal = {
            let e = snap.seats.entry(winner.clone()).or_insert_with(|| {
                let mut sb = SeatBalance::default();
                sb.balance = STARTING_BALANCE_COPPER;
                sb
            });
            e.balance = e.balance.saturating_add(pool);
            // Winning a pool may pull a seat back above the deficit cap.
            // We do NOT auto-clear timed_out here — reinstatement is the
            // explicit human action that does that (Phase 2 commit (c)).
            e.balance
        };
        let id = snap.next_txn_id;
        snap.next_txn_id += 1;
        let credit_row = LedgerRow {
            id,
            txn_type: "credit".to_string(),
            seat: winner.clone(),
            amount: pool,
            reason: format!("dispute {} won (conceded by {})", dispute_id, loser),
            ref_msg: Some(dispute.target_msg),
            balance_after: win_bal,
            escrow_id: None,
            release_turn: None,
            turn: Some(snap.turn_counter),
            action_kind: Some(ActionKind::Credit),
            linked_edit_msg: None,
            at: now.clone(),
        };
        append_currency_transaction(&dir, &credit_row)?;

        // Phase 4 (b) — retroactive Pass-penalty hook on the concede path.
        // Effective-winner predicate per spec v4 "Hook firing gate": fires
        // when the conceding seat IS the dispute target (= challenger
        // effectively wins). Challenger-concedes path (loser==challenger →
        // target_wins effective) emits zero penalty rows (T14 negative case).
        // System disputes (target=="system") never have a real seat to scan.
        if loser == dispute.target && dispute.target != "system" {
            emit_retro_pass_penalties(&dir, &mut snap, &dispute.target, dispute.target_msg, dispute_id)?;
            // Phase 4 (c) — co-liability on the same effective-winner gate; fires
            // only if the disputed message was an Edit (mutually exclusive with
            // retro-Pass).
            emit_coliability_penalties(&dir, &mut snap, &dispute.target, dispute.target_msg)?;
            // Phase 6 (b) — bounty clawback on the same effective-winner gate.
            // Winner here is `winner` (== dispute.challenger when loser==target).
            emit_bounty_clawback(&dir, &mut snap, dispute.target_msg, &winner, dispute_id)?;
        }

        write_balances_snapshot(&dir, &snap)?;

        // Resolve the dispute. Re-append the updated row (latest-row-per-id wins).
        dispute.status = "resolved".to_string();
        dispute.resolution = Some(format!("conceded_by_{}", loser));
        dispute.resolved_at = Some(now.clone());
        dispute.messages.push(DisputeMessage {
            from: loser.clone(),
            body: format!("[concede] full pool ({} copper) awarded to {}", pool, winner),
            added_to_pool: 0,
            at: now.clone(),
        });
        append_dispute_row(&dir, &dispute)?;

        // Remove from open-disputes snapshot so the Pass-while-disputed gate
        // releases the (now-resolved) target.
        let mut open = read_open_disputes_snapshot(&dir)?;
        snapshot_remove_open(&mut open, dispute_id, &dispute.challenger, &dispute.target);
        write_open_disputes_snapshot(&dir, &open)?;

        let _ = append_dispute_system_message(
            &dir,
            &format!("[Dispute resolved] {} concedes — {} wins {} copper", loser, winner, pool),
            &format!(
                "[Dispute resolved] {} concedes to {}. Pool of {} copper awarded to {} (dispute {}).",
                loser, winner, pool, winner, dispute_id
            ),
            serde_json::json!({
                "dispute_id": dispute_id,
                "winner": winner,
                "loser": loser,
                "pool": pool,
                "resolution": "conceded",
            }),
        )?;

        Ok(serde_json::to_value(&dispute).unwrap_or(serde_json::json!({
            "id": dispute_id, "winner": winner, "pool": pool, "status": "resolved",
        })))
    })
}

/// `currency_dispute_message(dispute_id, body, metadata?)` — party adds a
/// message to the dispute; cost (5 speech / 10 edit) is debited from the
/// caller and added to the pool. Auto-invokes the judge when pool crosses
/// `JUDGE_AUTO_INVOKE_THRESHOLD` (500).
fn handle_currency_dispute_message(
    dispute_id: &str,
    body: &str,
    metadata: Option<&serde_json::Value>,
) -> Result<serde_json::Value, String> {
    use collab::currency::*;
    let state = get_or_rejoin_state()?;
    let dir = state.project_dir.clone();
    let caller = format!("{}:{}", state.role, state.instance);
    if caller.starts_with("human:") {
        return Err("[DisputeMessage] Human is exempt from currency disputes.".to_string());
    }
    let body_trimmed = body.trim();
    if body_trimmed.is_empty() {
        return Err("[DisputeMessage] body is required (non-empty).".to_string());
    }

    // Cost gating: edit-related = 10 copper, otherwise = 5. Spec says
    // "if metadata indicates edit-related" — accept `metadata.edit_related: true`
    // OR `metadata.action_kind: "edit"`.
    let edit_related = metadata
        .and_then(|m| {
            m.get("edit_related").and_then(|v| v.as_bool())
                .or_else(|| {
                    m.get("action_kind")
                        .and_then(|v| v.as_str())
                        .map(|s| s.eq_ignore_ascii_case("edit"))
                })
        })
        .unwrap_or(false);
    let settings = collab::currency::read_economy_settings(&dir);
    let cost = if edit_related { settings.dispute_edit_cost_copper } else { settings.dispute_speech_cost_copper };

    collab::with_currency_and_board_lock(&dir, || {
        let mut dispute = read_dispute_by_id(&dir, dispute_id)?
            .ok_or_else(|| format!("[DisputeMessage] dispute {} not found.", dispute_id))?;
        if dispute.status != "open" {
            return Err(format!(
                "[DisputeMessage] dispute {} is already {} (resolution: {:?}).",
                dispute_id, dispute.status, dispute.resolution
            ));
        }
        if caller != dispute.challenger && caller != dispute.target {
            return Err(format!(
                "[DisputeMessage] You are not a party to dispute {} (challenger={}, target={}).",
                dispute_id, dispute.challenger, dispute.target
            ));
        }

        let mut snap = read_balances_snapshot(&dir)?;
        if snap.seats.is_empty() && currency_jsonl_path(&dir).exists() {
            snap = replay_balances_from_ledger(&dir)?;
        }
        let caller_bal = snap
            .seats
            .get(&caller)
            .map(|s| s.balance)
            .unwrap_or(STARTING_BALANCE_COPPER);
        if caller_bal < cost {
            return Err(format!(
                "[DisputeMessage] Insufficient balance: need {} copper, have {}.",
                cost, caller_bal
            ));
        }
        // Defensive: timed_out seats shouldn't even be reaching here (their
        // sends are rejected upstream), but check anyway.
        if snap.seats.get(&caller).map(|s| s.timed_out).unwrap_or(false) {
            return Err("[DisputeMessage] You are timed_out and cannot participate in disputes.".to_string());
        }

        let now = collab::iso_now();

        // Debit cost from caller (penalty row subtracts; opcode=DisputeContribution).
        let caller_after = {
            let e = snap
                .seats
                .entry(caller.clone())
                .or_insert_with(|| {
                    let mut sb = SeatBalance::default();
                    sb.balance = STARTING_BALANCE_COPPER;
                    sb
                });
            e.balance = e.balance.saturating_sub(cost);
            if e.balance <= DEFICIT_CAP_COPPER {
                e.timed_out = true;
            }
            e.balance
        };
        let txn_id = snap.next_txn_id;
        snap.next_txn_id += 1;
        let debit_row = LedgerRow {
            id: txn_id,
            txn_type: "penalty".to_string(),
            seat: caller.clone(),
            amount: -cost,
            reason: format!(
                "dispute {} message ({})",
                dispute_id,
                if edit_related { "edit-related, 10c" } else { "speech, 5c" }
            ),
            ref_msg: Some(dispute.target_msg),
            balance_after: caller_after,
            escrow_id: None,
            release_turn: None,
            turn: Some(snap.turn_counter),
            // Phase 2 opcode pragma: dispute message contributions reuse
            // ActionKind::Penalty (the row decrements balance). The prose
            // reason field disambiguates "dispute X message (speech, 5c)"
            // for ledger UIs; Phase 4 retro-scans use action_kind+reason
            // pairs if they need to filter dispute traffic from objection
            // costs vs adversarial-pass penalties.
            action_kind: Some(ActionKind::Penalty),
            linked_edit_msg: None,
            at: now.clone(),
        };
        append_currency_transaction(&dir, &debit_row)?;
        write_balances_snapshot(&dir, &snap)?;

        // Grow pool, append dispute message.
        dispute.pool = dispute.pool.saturating_add(cost);
        dispute.messages.push(DisputeMessage {
            from: caller.clone(),
            body: body_trimmed.to_string(),
            added_to_pool: cost,
            at: now.clone(),
        });

        // Auto-judge trigger: when pool crosses the threshold, set
        // judge = "human:0" and post a system notice. Only fires ONCE
        // (the first crossing — judge_already_set check).
        let mut auto_judge_fired = false;
        if dispute.judge.is_none() && dispute.pool >= settings.judge_auto_invoke_threshold {
            dispute.judge = Some("human:0".to_string());
            auto_judge_fired = true;
        }

        append_dispute_row(&dir, &dispute)?;

        let _ = append_dispute_system_message(
            &dir,
            &format!(
                "[Dispute message] {} adds {} copper — pool {}",
                caller, cost, dispute.pool
            ),
            &format!(
                "[Dispute message] {} contributed {} copper to dispute {}. Pool now {} copper.{}",
                caller,
                cost,
                dispute_id,
                dispute.pool,
                if auto_judge_fired {
                    " [Judge auto-invoked] human:0 — pool crossed 500."
                } else {
                    ""
                }
            ),
            serde_json::json!({
                "dispute_id": dispute_id,
                "from": caller,
                "added_to_pool": cost,
                "pool": dispute.pool,
                "auto_judge_fired": auto_judge_fired,
            }),
        )?;

        Ok(serde_json::to_value(&dispute).unwrap_or(serde_json::json!({
            "id": dispute_id,
            "pool": dispute.pool,
            "auto_judge_fired": auto_judge_fired,
        })))
    })
}

/// Handle project_send: send a message to a role
fn handle_project_send(to: &str, msg_type: &str, subject: &str, body: &str, metadata: Option<serde_json::Value>, _session_id: &str) -> Result<serde_json::Value, String> {
    let state = get_or_rejoin_state()?;

    let config = read_project_config(&state.project_dir)?;

    // v1.5.1 commit 1: legacy-compat yield fabrication removed. When a caller
    // omits yield_to (or omits any of target/ask/expected_output), the message
    // is stored as-is — no placeholder fabrication, no _legacy_compat flag, no
    // fake target=human. The rule-4 reader at line ~6411 handles missing/empty
    // yield fields defensively (yield_has_content is false → halt won't fire).
    let mut metadata = metadata;

    // Check if this session has been revoked
    let sessions = read_sessions(&state.project_dir);
    if let Some(bindings) = sessions.get("bindings").and_then(|b| b.as_array()) {
        for binding in bindings {
            if binding.get("session_id").and_then(|s| s.as_str()) == Some(&state.session_id) {
                if binding.get("status").and_then(|s| s.as_str()) == Some("revoked") {
                    return Err("Your role has been revoked. You cannot send messages. Call project_leave to exit.".to_string());
                }
            }
        }
    }

    // Phase A v2.2 §6.3 — Oxford debate speaker-only gate.
    // When an Oxford debate is active, only the currently-declared speaker
    // (or human:0, or the moderator) can project_send. Non-speaking debaters
    // + audience must use oxford_react for visual signals.
    // Exceptions per spec §6.3:
    //   - human:0 always bypasses (sovereignty)
    //   - moderator can send (procedural directives, end announcements)
    //   - "system" messages (this path doesn't emit system; included for completeness)
    //   - Not-in-debate seats can use Collab tab independently (only active-debate
    //     participants are gated)
    let caller = format!("{}:{}", state.role, state.instance);
    if !caller.starts_with("human:") {
        if let Ok(Some(debate)) = collab::oxford::read_active_oxford(&state.project_dir) {
            let in_debate = debate.moderator == caller
                || debate.side_a.iter().any(|s| s == &caller)
                || debate.side_b.iter().any(|s| s == &caller)
                || debate.audience.iter().any(|s| s == &caller);
            if in_debate && debate.moderator != caller {
                // Audience members + non-speaking debaters can't board-send.
                let is_current_speaker = debate.current_speaker.as_deref() == Some(caller.as_str());
                if !is_current_speaker {
                    let in_audience = debate.audience.iter().any(|s| s == &caller);
                    if in_audience {
                        return Err(format!(
                            "[OxfordNonDebaterCannotSpeak] You are in the audience for debate {}. Use oxford_react for visual signals instead.",
                            debate.debate_id
                        ));
                    }
                    return Err(format!(
                        "[OxfordNotYourTurn] Active debate {} — current speaker is {:?}. Wait for the moderator to declare you, or use oxford_react.",
                        debate.debate_id, debate.current_speaker
                    ));
                }
            }
        }
    }

    // Validate target role exists (or is "all" or "human")
    // Support instance-specific targets like "developer:0"
    if to != "all" && to != "human" {
        let roles = config.get("roles").and_then(|r| r.as_object())
            .ok_or("No roles in project config")?;
        let role_part = if to.contains(':') {
            to.split(':').next().unwrap_or(to)
        } else {
            to
        };
        if !roles.contains_key(role_part) {
            return Err(format!("Target role '{}' not found. Available: {:?}", role_part, roles.keys().collect::<Vec<_>>()));
        }
    }

    // Read discussion state once for broadcast permission. The Delphi-broadcast
    // gate moved INSIDE with_file_lock below so its read of assembly state
    // is atomic with the assembly gate (closes the pre-lock TOCTOU race
    // identified in evil-arch #120).
    let disc = read_discussion_state(&state.project_dir);
    let disc_active = disc.get("active").and_then(|v| v.as_bool()).unwrap_or(false);
    let disc_format = disc.get("mode").and_then(|v| v.as_str()).unwrap_or("");

    // Validate permission for broadcast
    if to == "all" {
        let roles = config.get("roles").and_then(|r| r.as_object());
        let my_role_def = roles.and_then(|r| r.get(&state.role));
        let perms: Vec<String> = my_role_def
            .and_then(|r| r.get("permissions"))
            .and_then(|p| p.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
            .unwrap_or_default();

        // Check if open communication mode is set (overrides role-level broadcast permission)
        let comm_mode = config.get("settings")
            .and_then(|s| s.get("discussion_mode"))
            .and_then(|m| m.as_str())
            .unwrap_or("directed");

        // Active discussions in public formats allow broadcasting (oxford, red_team, continuous)
        let discussion_allows_broadcast = disc_active
            && matches!(disc_format, "oxford" | "red_team" | "continuous");

        let has_role_perm = perms.contains(&"broadcast".to_string())
            || perms.contains(&"assign_tasks".to_string());
        let open_mode = comm_mode == "open";

        if !has_role_perm && !open_mode && !discussion_allows_broadcast {
            return Err("You don't have permission to broadcast. Use a specific role target.".to_string());
        }
    }

    // Delphi protocol enforcement moved INSIDE with_file_lock — see #120 race fix.

    // Slice 2 (spec §10 atomicity): extract metadata.mic_to so the post-append
    // transfer happens in the SAME lock window as the message commit. If
    // metadata.mic_to is set AND the active protocol's floor mode permits
    // transfers (reactive/queue/free-grab) AND the caller IS current_speaker,
    // the floor moves to mic_to atomically with the send. No observer can see
    // a state where the message landed but the floor didn't move (vote 5
    // ratified by team #927→#931).
    let mic_to_target: Option<String> = metadata
        .as_ref()
        .and_then(|m| m.get("mic_to"))
        .and_then(|v| v.as_str())
        .map(String::from);

    // Activity-field signal (school-of-fish, 2026-05-13 spec): extract
    // metadata.activity here, BEFORE the with_file_lock closure consumes
    // metadata. Written post-send (after the closure returns Ok) so it
    // only persists on accepted sends, not rejected ones. Cap length at
    // 32 chars; trim whitespace; reject empty.
    let activity_hint: Option<String> = metadata
        .as_ref()
        .and_then(|m| m.get("activity"))
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && s.len() <= 32);

    let result = collab::with_currency_and_board_lock(&state.project_dir, || {
        let from_label = format!("{}:{}", state.role, state.instance);

        // Currency Phase 1 commit (b) — TimedOut pre-hook. A seat at the
        // deficit cap (balance <= -1000) is excluded until the human reinstates.
        // Checked BEFORE the board append (and inside both locks) so a timed-out
        // seat's message never lands. Human sends are exempt (they post via the
        // Tauri path, not this sidecar, and classify as Exempt regardless).
        // Best-effort read: a balances.json read error must not block sends —
        // currency is an overlay, not a hard dependency of messaging.
        if !from_label.starts_with("human:") {
            if let Ok(snap) = collab::currency::read_balances_snapshot(&state.project_dir) {
                if snap.seats.get(&from_label).map(|s| s.timed_out).unwrap_or(false) {
                    return Err(format!(
                        "[TimedOut] {} is at the currency deficit cap ({} copper) and is excluded from sending. \
                         A human must reinstate the seat before it can send again.",
                        from_label, collab::currency::DEFICIT_CAP_COPPER
                    ));
                }
            }
        }

        // Phase 8 (human msg 2262): peek this seat's pending-edit marker ONCE,
        // before any classification. Reused by the Pass-while-disputed gate
        // (below) and the earn hook (after the board append) so both see the
        // SAME has_pending_edit verdict. NOT consumed here — only on the
        // send-accept path after the earn lands, so a rejected send keeps the
        // marker for the agent's next (real) send. Human sends never have markers.
        let (has_pending_edit, pending_edit_lines) = if from_label.starts_with("human:") {
            (false, 0)
        } else {
            peek_pending_edit(&state.project_dir, &from_label)
        };

        // Currency Phase 2 — Pass-while-disputed gate. A seat that is the TARGET
        // of an open dispute cannot send a Pass-classified message — they must
        // respond (concede or dispute), not duck via a Pass. Reads the O(1)
        // open_disputes.json snapshot (developer:1 msg 1430 #6), not the full
        // disputes.jsonl. Only blocks Pass; Speak/Edit/Test by the same seat proceed.
        if !from_label.starts_with("human:") {
            // resolved_to_edit=false: the gate only cares whether this is a Pass;
            // Edit/Test/Speak all fall through. has_pending_edit promotes a real
            // edit to Edit (not Pass) so a seat that actually did work isn't
            // blocked as a Pass-dodge.
            let action = collab::currency::classify_action_detected(
                &from_label, msg_type, subject, body, false, has_pending_edit,
            );
            if matches!(action, collab::currency::ActionKind::Pass) {
                if let Ok(open) = collab::currency::read_open_disputes_snapshot(&state.project_dir) {
                    if open.open_by_target.get(&from_label).map(|v| !v.is_empty()).unwrap_or(false) {
                        return Err(
                            "[OpenDisputeBlocksPass] You are the target of an open dispute and cannot Pass. \
                             Respond with currency_concede or currency_dispute_message.".to_string()
                        );
                    }
                }
            }
        }

        // Assembly Line gate (atomic with the send) — read state, check, reject or proceed.
        // Inside with_file_lock so the gate-check, board append, and post-accept advance
        // all share ONE lock acquire — no TOCTOU window between gate and advance.
        //
        // ONLY bypass: human-origin sends. We do NOT bypass on caller-supplied msg_type
        // (e.g. "moderation"); doing so would let any agent skip the mic by sending
        // type="moderation". Internal system events from handle_assembly_line append
        // to the board directly via append_to_board(), bypassing this entire function,
        // so they don't need a gate exemption (#113.A).
        // Human #1252 fix — AL gate now reads protocol.json (single source of
        // truth) instead of legacy assembly.json. Translates the protocol
        // schema (preset + floor.mode) into the legacy active/current_speaker
        // shape the rest of this gate expects.
        let section_for_gate = get_active_section(&state.project_dir);
        let proto_for_gate = read_protocol_for_section_value(&state.project_dir, &section_for_gate);
        let asm_active = proto_for_gate
            .get("preset").and_then(|p| p.as_str())
            .map(|s| s == PRESET_ASSEMBLY_LINE)
            .unwrap_or(false);
        let asm = serde_json::json!({
            "active": asm_active,
            "current_speaker": proto_for_gate.get("floor").and_then(|f| f.get("current_speaker")).cloned().unwrap_or(serde_json::Value::Null),
            "rotation_order": proto_for_gate.get("floor").and_then(|f| f.get("rotation_order")).cloned().unwrap_or(serde_json::json!([])),
        });
        // v1.5.1 commit 1: V3 spec rule 1 yield_to fabrication path removed.
        // Per architect msg 739 + evil-architect msg 732 four-data-point finding,
        // the legacy-compat shim was fabricating `target: "human"` placeholders
        // on every yield-less send — making "don't yield to human" structurally
        // impossible to honor through convention. Senders now pass through
        // whatever yield_to (or none) they supplied; the rule-4 reader handles
        // missing/empty values defensively. _legacy_compat is no longer written.

        // v1.5.1 commit 2: permanent yield_to.target="human" rejection per
        // human msg 871 directive ("you should never [yield to human], why do
        // you even have the ability to yield to me"). Architect msg 877 folded
        // this in: change #3 becomes always-on, not conditional on
        // human_offline_until_ts. Rule 4 halt-on-yield-to-human + v1.0.5
        // auto-resume become dead code once this gate is in place.
        if asm_active && state.role != "human" {
            let target = metadata
                .as_ref()
                .and_then(|m| m.get("yield_to"))
                .and_then(|y| y.get("target"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if target == "human" || target == "human:0" {
                return Err(format!(
                    "[HumanYieldDisabled] yield_to.target='{}' rejected. AI sends must yield to peer roles, not human. \
                     Per human msg 871: AI roles should never be able to yield to human. \
                     Drop the yield_to field entirely, or set target to a peer role.",
                    target
                ));
            }
        }

        // Rule 4 full-halt gate (v1.0.4, dev-challenger msg 454 → architect
        // msg 460 reversal): when a prior speaker yielded substantively to
        // human:0, protocol.json's `floor.halted_for_human` is set to true.
        // Until human:0 posts, AI sends are rejected with [FloorHalted].
        // Human-role sends bypass this entire gate via the
        // `state.role != "human"` guard. Once the human posts, the post-
        // accept block below clears `halted_for_human` and rotation resumes
        // from the preserved current_speaker position.
        //
        // Read protocol.json fresh here so the gate is atomic with the
        // halt flag — written under the same lock window by the rule 4
        // block when the halt fires.
        if asm_active && state.role != "human" {
            let section_for_halt = get_active_section(&state.project_dir);
            let proto_for_halt =
                read_protocol_for_section_value(&state.project_dir, &section_for_halt);
            let halted_for_human = proto_for_halt
                .get("floor")
                .and_then(|f| f.get("halted_for_human"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if halted_for_human {
                return Err(
                    "[FloorHalted] Floor halted for human:0; rotation resumes after the human posts. \
                     This send is not lost — re-send after the human responds.".to_string()
                );
            }
        }

        let mut asm_auto_grabbed: Option<serde_json::Value> = None;
        // moderator-authority Item 3 (spec line 49-51): exempt seats bypass the
        // assembly-mode mic-gate. The moderator manages the pipeline; they're
        // not subject to it. is_seat_exempt is true iff mic_passing_mode is
        // "moderator" AND from_label IS the designated moderator. Read from
        // proto_for_gate.floor (asm is a synthesized subset that doesn't carry
        // mic_passing_mode/moderator fields).
        let caller_is_exempt = {
            let floor = proto_for_gate.get("floor");
            let mic_mode = floor
                .and_then(|f| f.get("mic_passing_mode"))
                .and_then(|v| v.as_str())
                .unwrap_or("rotation");
            let moderator = floor
                .and_then(|f| f.get("moderator"))
                .and_then(|v| v.as_str());
            mic_mode == "moderator" && moderator == Some(from_label.as_str())
        };
        if asm_active && state.role != "human" && !caller_is_exempt {
            let cur = asm.get("current_speaker").and_then(|v| v.as_str()).unwrap_or("");
            if cur != from_label {
                // Gap H — connectedness-based auto-grab (team vote #1340: 5-of-7
                // option B). Replaces the prior 10-min silence threshold which
                // bypassed AL whenever the speaker was quiet, defeating strict
                // turn-taking (human #1337). Now: speaker keeps mic if their
                // session is alive in sessions.json (silent-but-present is
                // legitimate per memory #25 liveness ≠ activity); auto-grab
                // only fires when the holder has no live binding (closes the
                // original Gap H deadlock — tech-leader #1075).
                let speaker_is_live = if cur.is_empty() {
                    false
                } else {
                    let sessions = read_sessions(&state.project_dir);
                    let mut parts = cur.splitn(2, ':');
                    let speaker_role = parts.next().unwrap_or("");
                    let speaker_inst: u64 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
                    sessions.get("bindings").and_then(|b| b.as_array())
                        .map(|arr| arr.iter().any(|b| {
                            b.get("role").and_then(|r| r.as_str()) == Some(speaker_role)
                                && b.get("instance").and_then(|i| i.as_u64()).unwrap_or(0) == speaker_inst
                                && b.get("status").and_then(|s| s.as_str()) == Some("active")
                        }))
                        .unwrap_or(false)
                };

                if !speaker_is_live {
                    eprintln!(
                        "[assembly_line] auto-grab: prior speaker '{}' has no live session; '{}' claims mic (vote-B gate)",
                        if cur.is_empty() { "(none)" } else { cur },
                        from_label
                    );
                    let mut current = read_protocol_for_section_value(&state.project_dir, &section_for_gate);
                    if let Some(floor) = current.get_mut("floor").and_then(|f| f.as_object_mut()) {
                        floor.insert("current_speaker".to_string(), serde_json::json!(from_label));
                    }
                    let cur_rev = current.get("rev").and_then(|v| v.as_u64()).unwrap_or(0);
                    if let Some(rev_field) = current.get_mut("rev") {
                        *rev_field = serde_json::json!(cur_rev + 1);
                    }
                    if let Some(obj) = current.as_object_mut() {
                        obj.insert("last_writer_seat".to_string(), serde_json::json!(from_label.clone()));
                        obj.insert("last_writer_action".to_string(), serde_json::json!("al_auto_grab"));
                        obj.insert("rev_at".to_string(), serde_json::json!(utc_now_iso()));
                    }
                    if let Err(e) = write_protocol_for_section_value(&state.project_dir, &section_for_gate, &current) {
                        eprintln!("[assembly_line] auto-grab protocol.json write failed: {}", e);
                        return Err(format!(
                            "Assembly Line auto-grab attempted but write failed: {}. Current speaker: {}.",
                            e,
                            if cur.is_empty() { "(none)" } else { cur }
                        ));
                    }
                    let mut updated = asm.clone();
                    updated["current_speaker"] = serde_json::json!(from_label);
                    asm_auto_grabbed = Some(updated);
                } else {
                    return Err(format!(
                        "Assembly Line active — not your turn. Current speaker: {} (session live). Wait for them to yield or rotate.",
                        cur
                    ));
                }
            }
        }
        let _ = asm_auto_grabbed; // value held in case downstream needs it

        // Delphi-broadcast gate (atomic with the assembly gate above and the append below).
        // Per #56.1: when assembly_line is active, it OWNS the speech gate — the Delphi
        // restriction is short-circuited. Both gates read state inside the SAME
        // with_file_lock acquire — disc state is re-read here from disk so the check
        // is atomic with `asm_active` (closes the residual TOCTOU evil-arch #130
        // flagged on top of the original #120 race).
        let disc_in_lock = read_discussion_state(&state.project_dir);
        let disc_active_in_lock = disc_in_lock.get("active").and_then(|v| v.as_bool()).unwrap_or(false);
        let disc_format_in_lock = disc_in_lock.get("mode").and_then(|v| v.as_str()).unwrap_or("");
        if !asm_active && disc_active_in_lock && disc_format_in_lock == "delphi"
            && msg_type != "submission"
            && msg_type != "moderation"
            && to == "all"
            && state.role != "human"
        {
            let moderator = disc_in_lock.get("moderator").and_then(|v| v.as_str()).unwrap_or("unknown");
            if from_label == moderator {
                eprintln!("[delphi-reject] Blocked moderator broadcast from {} during active Delphi (type: {}, to: all). Use type: moderation for procedural announcements.", from_label, msg_type);
                return Err(
                    "Active Delphi discussion — moderator broadcasts to \"all\" are blocked. \
                    Use type: \"moderation\" for procedural round announcements. \
                    Directed messages to specific participants are still allowed.".to_string()
                );
            }
            let phase = disc_in_lock.get("phase").and_then(|v| v.as_str()).unwrap_or("");
            eprintln!("[delphi-reject] Blocked broadcast from {} during active Delphi (phase: {}, type: {}, to: all)", from_label, phase, msg_type);
            return Err(format!(
                "Active Delphi discussion — broadcasts to \"all\" are blocked to preserve blind submission integrity. \
                To submit your position, use type: \"submission\" addressed to the moderator ({}). \
                Directed messages to specific roles are still allowed.",
                moderator
            ));
        }

        // V3 Phase 2: stash the yield_to fields so the mic-arrival message
        // emitted on auto-advance can carry the contract forward. After Phase 1
        // these fields are guaranteed populated when assembly is active and
        // sender is non-human (either supplied or legacy_compat placeholder).
        let (yield_target, yield_ask, yield_expected, yield_is_legacy_compat, yield_surface_to_next) = metadata
            .as_ref()
            .and_then(|m| m.get("yield_to"))
            .map(|y| (
                y.get("target").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                y.get("ask").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                y.get("expected_output").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                y.get("_legacy_compat").and_then(|v| v.as_bool()).unwrap_or(false),
                y.get("surface_to_next_speaker").and_then(|v| v.as_bool()).unwrap_or(false),
            ))
            .unwrap_or_default();

        // Commit A — extended_thinking attestation check per
        // collaborative-proposal-workflow-spec-2026-05-15.md §Extended-thinking
        // attestation (lines 125-133). Planning-phase sends MUST include
        // metadata.extended_thinking: true; missing/false emits a
        // planning_unattested informational board event (non-blocking).
        // Honor-system at v1 per spec line 129 — no model-API verification.
        // Round-trip enforcement is v1.6 hardening path.
        let unattested = {
            let phase_now = proto_for_gate
                .get("floor")
                .and_then(|f| f.get("phase"))
                .and_then(|v| v.as_str())
                .unwrap_or("execution");
            let attested = metadata
                .as_ref()
                .and_then(|m| m.get("extended_thinking"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            phase_now == "planning" && !attested
        };

        let msg_id = next_message_id(&state.project_dir);
        let message = serde_json::json!({
            "id": msg_id,
            "from": from_label,
            "to": to,
            "type": msg_type,
            "timestamp": utc_now_iso(),
            "subject": subject,
            "body": body,
            "metadata": metadata.unwrap_or(serde_json::json!({}))
        });
        append_to_board(&state.project_dir, &message)?;

        // Currency Phase 1 commit (b) — earn hook. Classify the accepted send
        // and record the earn as an escrow hold (escrow_held += earn, balance
        // net 0; released to balance by commit (c) once release_turn matures).
        // Runs AFTER the board append, inside both locks (currency.lock +
        // board.lock), so message + currency transaction commit atomically.
        // Human sends classify as Exempt (no charge). Best-effort: a currency
        // write failure must NOT roll back the already-landed board message.
        {
            // Phase 4 (a): resolve a possible Test→Edit link before classifying.
            // Pull the first `#N` from the body; resolved_to_edit is true only if
            // that msg id has a real Edit row in the ledger (Q3 — numeric ref
            // alone is not enough; an orphan Test downgrades to Speak).
            let linked_edit_msg = extract_first_msg_ref(body);
            let resolved_to_edit = match linked_edit_msg {
                Some(n) => ledger_has_edit_row(&state.project_dir, n),
                None => false,
            };
            // Phase 8: detection-aware classify. A real file-write (has_pending_edit)
            // outranks Pass so terse "done" statuses after genuine edits earn Edit,
            // not Pass — the fix for the inert WORK tier (human msg 2246/2262).
            let action = collab::currency::classify_action_detected(
                &from_label, msg_type, subject, body, resolved_to_edit, has_pending_edit,
            );
            if !matches!(action, collab::currency::ActionKind::Exempt) {
                // Only a Test carries its linked Edit forward into the ledger.
                let linked = if matches!(action, collab::currency::ActionKind::Test) {
                    linked_edit_msg
                } else {
                    None
                };
                // edit_lines drives the Edit line-bonus (25 + max(0, lines-100));
                // only meaningful when this is a detected Edit, else 0.
                let edit_lines = if matches!(action, collab::currency::ActionKind::Edit) {
                    pending_edit_lines
                } else {
                    0
                };
                if let Err(e) = record_currency_earn(&state.project_dir, &from_label, action, msg_id, linked, edit_lines) {
                    eprintln!(
                        "[currency] earn hook failed for {} msg {}: {} (message still landed)",
                        from_label, msg_id, e
                    );
                }
            }
            // Consume the pending-edit marker on the accept path (message landed),
            // regardless of earn success, so the same file-write work can't be
            // re-credited to a later message. No-op when no marker existed.
            if has_pending_edit {
                consume_pending_edit(&state.project_dir, &from_label);
            }
        }

        // Currency Phase 1 commit (c) — per-send tick. Escrow release + interest
        // run on EVERY successful non-human send (so held funds aren't trapped
        // when assembly is off — developer:0 msg 1069 ruling). NO passive income
        // here (passive is mic_advance-only, fired at the rotation-advance hook
        // below). Inside both locks. Best-effort: a tick failure must not roll
        // back the landed message.
        // Currency on/off gate (human msg 1366 + architect:0 msg 1373): when
        // disabled, skip ticks too — not just earns — so "currency off" truly
        // freezes the economy (no escrow release, no interest accrual).
        if !from_label.starts_with("human:") && currency_enabled(&state.project_dir) {
            if let Err(e) = collab::currency::process_tick(&state.project_dir, false, &[]) {
                eprintln!(
                    "[currency] per-send tick failed after {} msg {}: {} (message still landed)",
                    from_label, msg_id, e
                );
            }
        }

        // Commit A — planning_unattested informational event emit. Lands
        // AFTER the main message so subscribers see them in order and the
        // CollabTab badge renderer can correlate the warning with the
        // originating message id. Non-blocking; the main message has
        // already been accepted.
        //
        // Commit 14 (evil-arch msg 2045 CRITICAL #2, human msg 2055 patch):
        // mirror the original `to` field on the warning event. Prior
        // behavior unconditionally set `to: "all"` even when the original
        // was a DM, leaking the existence of the DM (plus originating_seat
        // and msg_id) to the entire team. The warning's audience MUST
        // match the original's audience.
        if unattested {
            let warning_id = next_message_id(&state.project_dir);
            let warning = serde_json::json!({
                "id": warning_id,
                "from": "system",
                "to": to,  // CRITICAL #2 fix: mirror original audience, not broadcast.
                "type": "planning_unattested",
                "timestamp": utc_now_iso(),
                "subject": format!("[planning_unattested] {} sent without extended_thinking", from_label),
                "body": format!(
                    "Message #{} from {} during planning phase was sent without metadata.extended_thinking: true. \
                     Planning-phase contributions are expected to attest the agent's deep-think round per \
                     collaborative-proposal-workflow-spec-2026-05-15.md §Extended-thinking attestation. \
                     Honor-system at v1; warning only, message still lands.",
                    msg_id, from_label
                ),
                "metadata": {
                    "originating_message_id": msg_id,
                    "originating_seat": from_label,
                    "originating_to": to,
                }
            });
            // Audit-trail emit is best-effort — failure here does not roll
            // back the main message (which has already landed).
            let _ = append_to_board(&state.project_dir, &warning);
        }

        // V1.0.4 — clear `halted_for_human` after a human:0 send. Rule 4
        // halts the floor by setting halted_for_human=true; this is the
        // matching clear. Atomic with the append above (same lock). After
        // this, the gate at the top of the function lets AI sends through
        // and rotation resumes from the preserved current_speaker.
        if asm_active && state.role == "human" {
            let mut current =
                read_protocol_for_section_value(&state.project_dir, &section_for_gate);
            let was_halted = current
                .get("floor")
                .and_then(|f| f.get("halted_for_human"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if was_halted {
                if let Some(floor) =
                    current.get_mut("floor").and_then(|f| f.as_object_mut())
                {
                    floor.insert(
                        "halted_for_human".to_string(),
                        serde_json::json!(false),
                    );
                }
                let cur_rev = current.get("rev").and_then(|v| v.as_u64()).unwrap_or(0);
                if let Some(rev_field) = current.get_mut("rev") {
                    *rev_field = serde_json::json!(cur_rev + 1);
                }
                if let Some(obj) = current.as_object_mut() {
                    obj.insert(
                        "last_writer_seat".to_string(),
                        serde_json::json!(from_label.clone()),
                    );
                    obj.insert(
                        "last_writer_action".to_string(),
                        serde_json::json!("al_resumed_after_human"),
                    );
                    obj.insert(
                        "rev_at".to_string(),
                        serde_json::json!(utc_now_iso()),
                    );
                }
                let _ = write_protocol_for_section_value(
                    &state.project_dir,
                    &section_for_gate,
                    &current,
                );

                // V1.0.5 (dev-challenger msg 482): symmetric audit primitive.
                // The halt path writes a `floor_halted_for_human` board event
                // — observers (UI badges, test harnesses, replay tools)
                // watching board.jsonl see halts but had no corresponding
                // resume signal because the clear above only touched
                // protocol.json. Asymmetric audit channels for two halves of
                // the same state transition. Posting a parallel
                // `floor_resumed_after_human` event closes the gap.
                let resume_id = next_message_id(&state.project_dir);
                let resume_event = serde_json::json!({
                    "id": resume_id,
                    "from": "system",
                    "to": "all",
                    "type": "floor_resumed_after_human",
                    "timestamp": utc_now_iso(),
                    "subject": format!("[floor resumed] human:0 posted after halt"),
                    "body": format!(
                        "Floor halt cleared by human:0's send (msg id {}). Mic-gating restored; rotation resumes from current_speaker.",
                        msg_id
                    ),
                    "metadata": {
                        "triggered_by": from_label.clone(),
                        "trigger_msg_id": msg_id,
                    }
                });
                if let Err(e) = append_to_board(&state.project_dir, &resume_event) {
                    eprintln!(
                        "[assembly-v3] floor_resumed_after_human append failed: {} — flag cleared but signal lost.",
                        e
                    );
                }
            }
        }

        // Assembly Line auto-advance (atomic with the append above).
        // Human #1252 fix — write to protocol.json (single source of truth)
        // instead of legacy assembly.json. The asm view above was projected
        // from protocol.json so next_assembly_speaker still works on the
        // synthetic shape. Skips standby/disconnected seats; if no live seat
        // exists, mic stays put. Bypass matches the gate above: ONLY
        // human-origin sends skip the advance (caller-supplied msg_type is
        // not trusted here either, per #113.A).
        if asm_active && state.role != "human" {
            // Rule 4 (human-stall on yield-to-human, spec 2026-05-13):
            // When the speaker explicitly yields to the human, the floor
            // halts rather than rotating to the next AI seat. Server clears
            // current_speaker (which causes the asm gate above to let ANY
            // AI auto-grab the mic once it reactivates) and writes a
            // `floor_halted_for_human` event to the board so observers see
            // the transition. Mic-gating effectively pauses until the
            // human posts and another AI sends — at which point auto-grab
            // restores rotation from that seat. This stops the "AI clique
            // ignores yield-to-human and keeps the mic moving" failure mode
            // we lived earlier today.
            // Rule 4 firing condition (v1.0.2, dev-challenger msg 377 fix):
            // the writer at vaak-mcp.rs:5986-5991 stamps `_legacy_compat: true`
            // on the auto-attached yield_to placeholder specifically as a
            // machine-readable marker. The earlier c687249 used a fragile
            // string-prefix check (`starts_with("(missing")`) that depended
            // on the placeholder copy — if the prose changed, rule 4 silently
            // re-fired on every routine status ship. Now reads the boolean
            // marker the writer already supplies; copy of the placeholder is
            // irrelevant. Also tighten the "substantive content" check: both
            // ask and expected_output must be non-empty so callers that pass
            // an explicit (non-legacy) yield_to but omit content still don't
            // trigger spurious halts.
            let yield_to_human_targeted =
                yield_target == "human" || yield_target == "human:0";
            let yield_has_content = !yield_ask.is_empty() && !yield_expected.is_empty();
            let yield_to_human = yield_to_human_targeted
                && !yield_is_legacy_compat
                && yield_has_content;

            if yield_to_human {
                let mut current =
                    read_protocol_for_section_value(&state.project_dir, &section_for_gate);
                if let Some(floor) = current.get_mut("floor").and_then(|f| f.as_object_mut()) {
                    // V1.0.4: set halted_for_human=true so the gate at the
                    // top of this function rejects subsequent AI sends until
                    // human:0 posts. Preserve current_speaker (don't null it)
                    // so when rotation resumes after the human, it picks up
                    // from the same position rather than auto-grab-shuffling.
                    floor.insert(
                        "halted_for_human".to_string(),
                        serde_json::json!(true),
                    );
                }
                let cur_rev = current.get("rev").and_then(|v| v.as_u64()).unwrap_or(0);
                if let Some(rev_field) = current.get_mut("rev") {
                    *rev_field = serde_json::json!(cur_rev + 1);
                }
                if let Some(obj) = current.as_object_mut() {
                    obj.insert(
                        "last_writer_seat".to_string(),
                        serde_json::json!(from_label.clone()),
                    );
                    obj.insert(
                        "last_writer_action".to_string(),
                        serde_json::json!("al_halted_for_human"),
                    );
                    obj.insert("rev_at".to_string(), serde_json::json!(utc_now_iso()));
                }
                let _ = write_protocol_for_section_value(
                    &state.project_dir,
                    &section_for_gate,
                    &current,
                );

                let halt_id = next_message_id(&state.project_dir);
                let halt_msg = serde_json::json!({
                    "id": halt_id,
                    "from": "system",
                    "to": "all",
                    "type": "floor_halted_for_human",
                    "timestamp": utc_now_iso(),
                    "subject": format!("[floor halted] {} yielded to human:0", from_label),
                    "body": format!(
                        "Floor halted by {}'s yield to human:0. Mic-gating paused until human responds; rotation resumes from the next live seat after the human posts.",
                        from_label
                    ),
                    "metadata": {
                        "triggered_by": from_label.clone(),
                        "trigger_msg_id": msg_id,
                    }
                });
                if let Err(e) = append_to_board(&state.project_dir, &halt_msg) {
                    eprintln!(
                        "[assembly-v3] floor_halted_for_human append failed: {} — floor cleared but signal lost.",
                        e
                    );
                }
            }

            // Strict-rotation fix (team consensus 2026-05-13, section 5-12):
            // yield_to.target is a COURTESY HINT only — it does NOT override
            // rotation_order for mic advancement. Earlier behavior consulted
            // resolve_yield_target() first and only fell back to round-robin
            // when target was empty/self/unresolvable. That allowed a clique
            // of N speakers to yield among themselves indefinitely while a
            // live seat further along rotation_order never received the mic
            // (lived this for 10 rounds on 2026-05-13: evil-architect:0 sat
            // at rotation_order[3] and was structurally skipped every turn).
            //
            // New rule: the mic ALWAYS advances via next_assembly_speaker
            // (strict modular increment through rotation_order, skipping
            // standby/disconnected seats). The one exception is explicit
            // self-yield (fan-out pattern) where the speaker names themselves
            // as target — mic stays put rather than kicking them off their
            // own turn. yield_to.target remains in message metadata as a
            // courtesy hint readable by the next speaker, but the server
            // does not honor it for routing.
            //
            // yield-to-human is handled by the Rule 4 branch above and falls
            // through here with current_speaker already cleared; we skip the
            // auto-advance entirely so the floor stays halted.
            let yield_is_self = if !yield_target.is_empty() {
                resolve_yield_target(&state.project_dir, &yield_target)
                    .as_deref()
                    == Some(from_label.as_str())
            } else {
                false
            };
            // Strict-turn-discipline al_auto_advance gate (evil-arch msg 2421
            // + human msg 2441): suppress al_auto_advance when no explicit yield
            // AND either (a) review_intensity >= 7 (yield-only mic-pass per spec
            // line 77) or (b) sender's floor.turn_type == "working" (working
            // agents hold mic through periodic sends; only explicit yield
            // releases). Per spec §Working-turn unbounded mic-hold (lines 56-62)
            // + §Yield-only mic-pass (lines 75-79). Composes with Commit T's
            // watchdog floor_stall suppression — closes both release paths.
            //
            // Bug #1 fix (architect msg 2486 + tester msg 2515 T1f/T1g + dev-
            // challenger msg 2517): prior form was `intensity>=7 || (working &&
            // !yield)` — clause A lacked the explicit-yield guard, so peer-
            // yields at intensity>=7 were silently dropped (contradicts spec
            // line 77). Factored form below applies `!has_explicit_yield` to
            // both clauses so an explicit yield always releases.
            let proto_for_advance =
                read_protocol_for_section_value(&state.project_dir, &section_for_gate);
            let review_intensity = proto_for_advance
                .get("floor")
                .and_then(|f| f.get("review_intensity"))
                .and_then(|v| v.as_u64())
                .unwrap_or(5) as u8;
            let sender_turn_type = proto_for_advance
                .get("floor")
                .and_then(|f| f.get("turn_type"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let has_explicit_yield = !yield_target.is_empty() && !yield_is_legacy_compat;
            let suppress_auto_advance = !has_explicit_yield
                && (review_intensity >= 7 || sender_turn_type == "working");

            let next_speaker = if yield_to_human || yield_is_self || suppress_auto_advance {
                None
            } else {
                next_assembly_speaker(&asm, &state.project_dir, &from_label)
            };
            if let Some(next) = next_speaker {
                // Write the new current_speaker into protocol.json under the
                // existing lock window.
                let mut current = read_protocol_for_section_value(&state.project_dir, &section_for_gate);
                if let Some(floor) = current.get_mut("floor").and_then(|f| f.as_object_mut()) {
                    floor.insert("current_speaker".to_string(), serde_json::json!(next));
                }
                let cur_rev = current.get("rev").and_then(|v| v.as_u64()).unwrap_or(0);
                if let Some(rev_field) = current.get_mut("rev") {
                    *rev_field = serde_json::json!(cur_rev + 1);
                }
                if let Some(obj) = current.as_object_mut() {
                    obj.insert("last_writer_seat".to_string(), serde_json::json!(from_label.clone()));
                    obj.insert("last_writer_action".to_string(), serde_json::json!("al_auto_advance"));
                    obj.insert("rev_at".to_string(), serde_json::json!(utc_now_iso()));
                }
                let _ = write_protocol_for_section_value(&state.project_dir, &section_for_gate, &current);

                // Currency Phase 1 commit (c) — mic_advance tick. Fires when the
                // mic actually advances: pays passive income (+1) to every active
                // seat AND runs the escrow release + interest lifecycle. Per spec
                // tick-split, passive is mic_advance-only. active_seats = active
                // bindings from sessions.json ("role:instance" labels). Inside
                // both locks; best-effort (a tick failure doesn't unwind the mic
                // advance, which already persisted above).
                // Currency on/off gate (human msg 1366 + architect:0 msg 1373):
                // skip the whole tick (passive + lifecycle) when disabled, so
                // "currency off" freezes the economy completely.
                if currency_enabled(&state.project_dir) {
                    let sessions = read_sessions(&state.project_dir);
                    let active_seats: Vec<String> = sessions
                        .get("bindings")
                        .and_then(|b| b.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter(|b| {
                                    b.get("status").and_then(|s| s.as_str()) == Some("active")
                                })
                                .filter_map(|b| {
                                    let role = b.get("role").and_then(|r| r.as_str())?;
                                    let inst = b.get("instance").and_then(|i| i.as_u64()).unwrap_or(0);
                                    if role == "human" { return None; } // human is exempt
                                    Some(format!("{}:{}", role, inst))
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    if let Err(e) =
                        collab::currency::process_tick(&state.project_dir, true, &active_seats)
                    {
                        eprintln!("[currency] mic_advance tick failed: {} (mic still advanced)", e);
                    }
                }

                // V3 Phase 2 (rule 3): post a directed [YOUR TURN] message to
                // the new speaker so they wake on the next project_wait poll
                // with explicit context — what to do, what shape of output,
                // who handed it to them. Closes the "I don't know it's my
                // turn" failure that wedged tonight's session for 30+ min.
                // Same lock window as the source message + protocol write —
                // no observer can see a state where the mic moved but the
                // arrival signal is missing.
                let arrival_id = next_message_id(&state.project_dir);
                // UX-lane rotation visibility (extends human #140 + msg 264):
                // show the full rotation_order with current+prev markers AND
                // each seat's current activity (per developer a627daf's
                // activity-field + TTL). The school-of-fish signal — every
                // role sees who's discussing, implementing, reviewing, idle
                // at a glance in the [YOUR TURN] body, without separately
                // calling project_status.
                let rotation_line = {
                    let order: Vec<String> = asm.get("rotation_order")
                        .and_then(|v| v.as_array())
                        .map(|arr| arr.iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect())
                        .unwrap_or_default();
                    if order.is_empty() {
                        String::new()
                    } else {
                        // Read sessions.json once for per-seat activity lookup.
                        // TTL matches handle_project_status: 60s after the
                        // seat's last_heartbeat, stored activity decays to
                        // "idle" (evil-architect msg 267 requirement).
                        let sessions = read_sessions(&state.project_dir);
                        // Zombie-seat filter (human msg 2747 + ui-arch msg 2754
                        // server-side port): rotation_order may contain seats
                        // kicked / disconnected without a corresponding
                        // rotation mutation. Filter against sessions.json
                        // bindings so dead seats don't appear in mic_landed
                        // body's `Rotation:` line. Mirror of the frontend
                        // filters in 09a29dd (ProtocolPanel.tsx CompactMicLine
                        // + AssemblyControls.tsx renderStatusLine).
                        // Architect msg 2808 tightening (UI-arch msg 2806 empirical
                        // finding): handle_project_kick at vaak-mcp.rs:9646 marks
                        // status="revoked" but KEEPS the binding entry, while
                        // handle_project_leave at vaak-mcp.rs:9544-9548 physically
                        // removes the entry. Without status-exclusion, a kicked seat
                        // would still pass seat_has_binding because the revoked entry
                        // remains in sessions.json:bindings. Exclude inactive statuses
                        // to match the canonical presence predicate per architect msg
                        // 2808: `binding_exists AND status NOT IN {"revoked", "left"}`.
                        let seat_has_binding = |seat: &str| -> bool {
                            let mut sp = seat.splitn(2, ':');
                            let Some(role) = sp.next() else { return false; };
                            let Some(inst) = sp.next().and_then(|s| s.parse::<u64>().ok()) else { return false; };
                            sessions.get("bindings").and_then(|b| b.as_array())
                                .map(|arr| arr.iter().any(|b| {
                                    let role_ok = b.get("role").and_then(|r| r.as_str()) == Some(role);
                                    let inst_ok = b.get("instance").and_then(|i| i.as_u64()).unwrap_or(0) == inst;
                                    let status = b.get("status").and_then(|s| s.as_str()).unwrap_or("");
                                    let status_active = status != "revoked" && status != "left";
                                    role_ok && inst_ok && status_active
                                }))
                                .unwrap_or(false)
                        };
                        let order: Vec<String> = order.into_iter()
                            .filter(|seat| seat_has_binding(seat))
                            .collect();
                        if order.is_empty() {
                            String::new()
                        } else {
                        const ACTIVITY_TTL_SECS: i64 = 60;
                        let now_secs = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_secs() as i64)
                            .unwrap_or(0);
                        let seat_activity = |seat: &str| -> Option<String> {
                            let mut parts = seat.splitn(2, ':');
                            let role = parts.next()?;
                            let inst: u64 = parts.next().and_then(|s| s.parse().ok())?;
                            sessions.get("bindings").and_then(|b| b.as_array())?
                                .iter()
                                .find(|b| b.get("role").and_then(|r| r.as_str()) == Some(role)
                                    && b.get("instance").and_then(|i| i.as_u64()).unwrap_or(0) == inst)
                                .and_then(|b| {
                                    let stored = b.get("activity").and_then(|v| v.as_str())?.to_string();
                                    let hb_secs = b.get("last_heartbeat").and_then(|v| v.as_str())
                                        .and_then(parse_iso_to_epoch_secs)
                                        .map(|s| s as i64)
                                        .unwrap_or(0);
                                    Some(if now_secs - hb_secs > ACTIVITY_TTL_SECS {
                                        "idle".to_string()
                                    } else {
                                        stored
                                    })
                                })
                        };
                        let parts: Vec<String> = order.iter().map(|seat| {
                            let activity = seat_activity(seat);
                            let role_marker = if seat == &next {
                                Some("YOU")
                            } else if seat == &from_label {
                                Some("prev")
                            } else {
                                None
                            };
                            match (role_marker, activity) {
                                (None, None) => seat.clone(),
                                (None, Some(a)) => format!("{}({})", seat, a),
                                (Some(m), None) => format!("{}({})", seat, m),
                                (Some(m), Some(a)) => format!("{}({}, {})", seat, m, a),
                            }
                        }).collect();
                        format!("\nRotation: {}", parts.join(" → "))
                        }
                    }
                };
                // Body composition (human msg 411, 2026-05-13): the prior
                // speaker's ask + expected_output anchored the next speaker's
                // thinking on every turn ("limits us in our thinking"). The
                // server now omits those lines from the body by default and
                // only surfaces them when the prior speaker explicitly opts
                // in via `metadata.yield_to.surface_to_next_speaker == true`
                // — the moderator override path. Ask + expected still live
                // in metadata for record-keeping and rule 4's substantive-
                // yield check, so this is a body-only change.
                let body_text = if yield_surface_to_next
                    && !yield_ask.is_empty()
                    && !yield_is_legacy_compat
                {
                    format!(
                        "[YOUR TURN] mic from {}. Floor: {}s.\nAsk: {}\nExpected: {}{}",
                        from_label,
                        ASSEMBLY_FLOOR_DEFAULT_SECS,
                        yield_ask,
                        yield_expected,
                        rotation_line
                    )
                } else {
                    format!(
                        "[YOUR TURN] mic from {}. Floor: {}s.{}",
                        from_label, ASSEMBLY_FLOOR_DEFAULT_SECS, rotation_line
                    )
                };
                // v1.5.1 follow-up (ui-arch msg 892 + evil-arch msg 896):
                // include the floor's typed-turn fields in mic_landed metadata
                // so the UI-arch renderer can badge waiting agents with the
                // outgoing speaker's claim ("X just finished Working ~10min").
                // Until v1.5.2 ships the auto-claim shim, the NEW speaker has
                // not yet claimed at mic_landed time — these fields reflect
                // the floor as written by the PREVIOUS speaker's mic_claim
                // (if any), giving the team historical context.
                let floor_for_emit = read_protocol_for_section_value(&state.project_dir, &section_for_gate);
                let prev_turn_type = floor_for_emit
                    .get("floor")
                    .and_then(|f| f.get("turn_type"))
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                let prev_expected_duration_secs = floor_for_emit
                    .get("floor")
                    .and_then(|f| f.get("expected_duration_secs"))
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                let arrival_msg = serde_json::json!({
                    "id": arrival_id,
                    "from": "system",
                    "to": next.clone(),
                    "type": "mic_landed",
                    "timestamp": utc_now_iso(),
                    "subject": format!("[YOUR TURN] {}", next),
                    "body": body_text,
                    "metadata": {
                        "ask": yield_ask,
                        "expected_output": yield_expected,
                        "floor_time_seconds": ASSEMBLY_FLOOR_DEFAULT_SECS,
                        "triggered_by": from_label.clone(),
                        "trigger_msg_id": msg_id,
                        "rotation": rotation_line.trim_start_matches("\nRotation: "),
                        "prev_turn_type": prev_turn_type,
                        "prev_expected_duration_secs": prev_expected_duration_secs,
                    }
                });
                if let Err(e) = append_to_board(&state.project_dir, &arrival_msg) {
                    eprintln!("[assembly-v3] mic_landed append failed: {} — mic moved but signal lost. New speaker will discover their turn via project_wait timeout.", e);
                }
            }
        }

        // Slice 2 (spec §10): protocol-side atomic mic_to transfer. Same
        // lock window as the board.jsonl append above — no observer can see
        // a state where the message landed but the floor didn't move.
        // Disabled when legacy assembly_line state is active (round-robin
        // auto-advance owns the floor in that mode).
        if !asm_active {
            if let Some(target) = &mic_to_target {
                let section = get_active_section(&state.project_dir);
                if let Err(e) = apply_protocol_mic_to_transfer(
                    &state.project_dir, &section, &from_label, target
                ) {
                    eprintln!(
                        "[protocol_mcp][SEVERE] project_send mic_to atomic transfer failed AFTER message append: {}. Section: {}. From: {}. Target: {}. Audit drift — message landed but floor did not move. Manual reconcile required.",
                        e, section, from_label, target
                    );
                }
            }
        }

        // Continuous review: auto-create micro-round when developer posts a status
        if msg_type == "status" {
            let _ = auto_create_continuous_round(&state.project_dir, subject, body, &from_label, msg_id);
        }

        // Continuous review: check quorum after a submission arrives
        if msg_type == "submission" {
            // First, check if timeout has passed on current round
            let _ = auto_close_timed_out_round(&state.project_dir);
        }

        // Auto-record submission in discussion.json if this is a submission message
        if msg_type == "submission" {
            let disc = read_discussion_state(&state.project_dir);
            let is_active = disc.get("active").and_then(|v| v.as_bool()).unwrap_or(false);
            let phase = disc.get("phase").and_then(|v| v.as_str()).unwrap_or("");
            let sub_disc_mode = disc.get("mode").and_then(|v| v.as_str()).unwrap_or("");
            eprintln!("[submission-track] msg_id={}, from={}, active={}, phase={}", msg_id, from_label, is_active, phase);

            // Oxford team assignment warning: log if submitter is not in any team
            if is_active && sub_disc_mode == "oxford" {
                let teams = disc.get("teams");
                if let Some(t) = teams {
                    if !t.is_null() {
                        let in_for = t.get("for").and_then(|v| v.as_array())
                            .map(|arr| arr.iter().any(|v| v.as_str() == Some(&from_label)))
                            .unwrap_or(false);
                        let in_against = t.get("against").and_then(|v| v.as_array())
                            .map(|arr| arr.iter().any(|v| v.as_str() == Some(&from_label)))
                            .unwrap_or(false);
                        if !in_for && !in_against {
                            eprintln!("[submission-track] WARNING: {} submitted but is not assigned to any team (FOR or AGAINST)", from_label);
                        } else {
                            let team_name = if in_for { "FOR" } else { "AGAINST" };
                            eprintln!("[submission-track] {} is on Team {}", from_label, team_name);
                        }
                    }
                }
            }

            if is_active && phase == "submitting" {
                let mut updated = disc.clone();
                let mut should_write = false;
                let mut sub_count = 0usize;
                let mut track_error: Option<&str> = None;

                if let Some(rounds) = updated.get_mut("rounds").and_then(|r| r.as_array_mut()) {
                    if let Some(last_round) = rounds.last_mut() {
                        if let Some(subs) = last_round.get_mut("submissions").and_then(|s| s.as_array_mut()) {
                            // Find existing submission from this participant (if any)
                            let existing_idx = subs.iter().position(|s| {
                                s.get("from").and_then(|f| f.as_str()) == Some(&from_label)
                            });
                            if let Some(idx) = existing_idx {
                                // Correction: overwrite previous submission with the new one
                                let prev_id = subs[idx].get("message_id").and_then(|id| id.as_u64()).unwrap_or(0);
                                subs[idx] = serde_json::json!({
                                    "from": from_label,
                                    "message_id": msg_id,
                                    "submitted_at": utc_now_iso(),
                                    "replaces": prev_id
                                });
                                sub_count = subs.len();
                                should_write = true;
                                eprintln!("[submission-track] {} corrected submission (was msg {}, now msg {})", from_label, prev_id, msg_id);
                            } else {
                                // First submission from this participant
                                subs.push(serde_json::json!({
                                    "from": from_label,
                                    "message_id": msg_id,
                                    "submitted_at": utc_now_iso()
                                }));
                                sub_count = subs.len();
                                should_write = true;
                            }
                        } else {
                            track_error = Some("submissions field missing or not an array in last round");
                        }
                    } else {
                        track_error = Some("no rounds found in discussion state");
                    }
                } else {
                    track_error = Some("rounds field missing or not an array");
                }

                if let Some(err) = track_error {
                    eprintln!("[submission-track] ERROR: {}", err);
                }
                if should_write {
                    match write_discussion_state(&state.project_dir, &updated) {
                        Ok(_) => {
                            eprintln!("[submission-track] Recorded submission from {} (msg_id={}), total now: {}", from_label, msg_id, sub_count);

                            // Delphi auto-close: if all non-moderator participants have submitted,
                            // auto-close the round and generate the aggregate (no human moderator needed)
                            let disc_mode = updated.get("mode").and_then(|m| m.as_str()).unwrap_or("");
                            if disc_mode == "delphi" || disc_mode == "oxford" || disc_mode == "red_team" {
                                let moderator = updated.get("moderator").and_then(|m| m.as_str()).unwrap_or("");
                                let participants = updated.get("participants")
                                    .and_then(|p| p.as_array())
                                    .map(|p| p.len())
                                    .unwrap_or(0);
                                // Expected submissions = participants minus moderator (if moderator is in participants list)
                                let is_mod_participant = updated.get("participants")
                                    .and_then(|p| p.as_array())
                                    .map(|arr| arr.iter().any(|v| v.as_str() == Some(moderator)))
                                    .unwrap_or(false);
                                let expected = if is_mod_participant { participants - 1 } else { participants };

                                eprintln!("[auto-close] sub_count={}, expected={}, mode={}", sub_count, expected, disc_mode);
                                if sub_count >= expected && expected > 0 {
                                    eprintln!("[auto-close] All submissions in — auto-closing round");
                                    // Generate aggregate
                                    let fresh_disc = read_discussion_state(&state.project_dir);
                                    match generate_aggregate(&state.project_dir, &fresh_disc) {
                                        Ok(aggregate_text) => {
                                            let now = utc_now_iso();
                                            let round_num = fresh_disc.get("current_round").and_then(|v| v.as_u64()).unwrap_or(0);

                                            // Post aggregate to board
                                            let agg_msg_id = next_message_id(&state.project_dir);
                                            let aggregate_msg = serde_json::json!({
                                                "id": agg_msg_id,
                                                "from": "system:0",
                                                "to": "all",
                                                "type": "aggregate",
                                                "subject": format!("Round {} Aggregate — Anonymous Submissions", round_num),
                                                "body": aggregate_text,
                                                "timestamp": now,
                                                "metadata": {
                                                    "round": round_num,
                                                    "discussion_mode": disc_mode,
                                                    "auto_closed": true
                                                }
                                            });
                                            if let Err(e) = append_to_board(&state.project_dir, &aggregate_msg) {
                                                eprintln!("[auto-close] ERROR posting aggregate: {}", e);
                                            }

                                            // Update discussion state: close round
                                            let mut closed = fresh_disc.clone();
                                            closed["phase"] = serde_json::json!("closed");
                                            if let Some(rounds) = closed.get_mut("rounds").and_then(|r| r.as_array_mut()) {
                                                if let Some(last_round) = rounds.last_mut() {
                                                    last_round["closed_at"] = serde_json::json!(now);
                                                    last_round["aggregate_message_id"] = serde_json::json!(agg_msg_id);
                                                }
                                            }
                                            // End the discussion (single-round auto-close)
                                            closed["active"] = serde_json::json!(false);
                                            closed["previous_phase"] = serde_json::json!("submitting");

                                            if let Err(e) = write_discussion_state(&state.project_dir, &closed) {
                                                eprintln!("[auto-close] ERROR writing discussion state: {}", e);
                                            } else {
                                                eprintln!("[auto-close] Round {} auto-closed, aggregate posted as msg {}", round_num, agg_msg_id);
                                            }

                                            notify_desktop();
                                        }
                                        Err(e) => eprintln!("[auto-close] ERROR generating aggregate: {}", e),
                                    }
                                }
                            }
                        }
                        Err(e) => eprintln!("[submission-track] ERROR writing discussion.json: {}", e),
                    }
                }
            } else {
                eprintln!("[submission-track] Skipping — discussion not active or not in submitting phase");
            }
        }

        // Continuous review: auto-close round if quorum reached after this submission
        if msg_type == "submission" && check_continuous_quorum(&state.project_dir) {
            // Quorum reached — close round and generate mini-aggregate
            let disc = read_discussion_state(&state.project_dir);
            if let Ok(aggregate_text) = generate_mini_aggregate(&state.project_dir, &disc) {
                let now = utc_now_iso();
                let round_num = disc.get("current_round").and_then(|v| v.as_u64()).unwrap_or(0);

                let agg_msg_id = next_message_id(&state.project_dir);
                let aggregate_msg = serde_json::json!({
                    "id": agg_msg_id,
                    "from": "system",
                    "to": "all",
                    "type": "moderation",
                    "timestamp": now,
                    "subject": format!("Review #{} — quorum reached", round_num),
                    "body": aggregate_text,
                    "metadata": {
                        "discussion_action": "auto_aggregate",
                        "round": round_num,
                        "close_reason": "quorum"
                    }
                });
                let _ = append_to_board(&state.project_dir, &aggregate_msg);

                // Update discussion state
                let mut updated_disc = disc.clone();
                if let Some(rounds) = updated_disc.get_mut("rounds").and_then(|r| r.as_array_mut()) {
                    if let Some(last) = rounds.last_mut() {
                        last["closed_at"] = serde_json::json!(now);
                        last["aggregate_message_id"] = serde_json::json!(agg_msg_id);
                    }
                }
                updated_disc["phase"] = serde_json::json!("reviewing");
                let _ = write_discussion_state(&state.project_dir, &updated_disc);
            }
        }

        Ok(msg_id)
    })?;

    update_session_heartbeat_in_file();

    // Activity-field signal (school-of-fish, 2026-05-13 spec):
    // Caller may pass metadata.activity (free-form string: "discussing",
    // "implementing", "reviewing", "waiting", "idle", etc.) to declare
    // what they're doing now. Server writes it to bindings[i].activity so
    // project_status surfaces it to peers and the human — the visible
    // "water" each fish in the school sees. Captured from metadata above
    // before the move; written here after the successful append.
    if let Some(activity) = activity_hint.as_deref() {
        update_session_activity(activity);
    }

    notify_desktop();

    // Reset hook compliance tracker — this session just sent a message
    write_send_tracker(&state.project_dir, &state.session_id, 0);

    Ok(serde_json::json!({
        "message_id": result,
        "delivered_to": [to]
    }))
}

/// Handle project_check: read new messages
fn handle_project_check(last_seen: u64) -> Result<serde_json::Value, String> {
    let state = get_or_rejoin_state()?;

    // If caller passes last_seen=0, use the stored last_seen_id from file
    // to prevent re-delivering messages already seen via project_join or hook.
    // Section-scoped: each section keeps its own last_seen_id (see last_seen_path).
    let effective_last_seen = if last_seen == 0 {
        let active_section = get_active_section(&state.project_dir);
        read_last_seen_id(&state.project_dir, &state.session_id, &active_section)
    } else {
        last_seen
    };

    let my_instance_label = format!("{}:{}", state.role, state.instance);
    let all_messages = read_board_filtered(&state.project_dir);
    let my_messages: Vec<&serde_json::Value> = all_messages.iter()
        .filter(|m| {
            let to = m.get("to").and_then(|t| t.as_str()).unwrap_or("");
            to == state.role || to == my_instance_label || to == "all"
        })
        .filter(|m| {
            m.get("id").and_then(|i| i.as_u64()).unwrap_or(0) > effective_last_seen
        })
        .collect();

    let latest_id = all_messages.iter()
        .filter_map(|m| m.get("id").and_then(|i| i.as_u64()))
        .max()
        .unwrap_or(0);

    // Build team status
    let sessions = read_sessions(&state.project_dir);
    let bindings = sessions.get("bindings").and_then(|b| b.as_array()).cloned().unwrap_or_default();
    let config = read_project_config(&state.project_dir)?;

    let mut team_status = Vec::new();
    if let Some(roles_obj) = config.get("roles").and_then(|r| r.as_object()) {
        for (slug, rdef) in roles_obj {
            let title = rdef.get("title").and_then(|t| t.as_str()).unwrap_or(slug);
            let active = bindings.iter()
                .filter(|b| b.get("role").and_then(|r| r.as_str()) == Some(slug.as_str())
                    && b.get("status").and_then(|s| s.as_str()) == Some("active"))
                .count();
            team_status.push(serde_json::json!({
                "role": slug,
                "title": title,
                "active": active,
                "status": if active > 0 { "active" } else { "vacant" }
            }));
        }
    }

    update_session_heartbeat_in_file();

    Ok(serde_json::json!({
        "messages": my_messages,
        "latest_id": latest_id,
        "team_status": team_status
    }))
}

/// Handle project_wait: block until new messages arrive or timeout.
/// Polls board.jsonl every 3 seconds. Sends heartbeat every 30 seconds.
/// Returns immediately when new messages are found.
fn handle_project_wait(timeout_secs: u64) -> Result<serde_json::Value, String> {
    let state = get_or_rejoin_state()?;

    let session_id = read_cached_session_id().unwrap_or_else(get_session_id);

    // Mark this session as in standby
    update_session_activity("standby");

    let start = std::time::Instant::now();
    let poll_interval = std::time::Duration::from_secs(3);
    let timeout = std::time::Duration::from_secs(timeout_secs);
    let mut polls_since_heartbeat: u32 = 0;

    loop {
        // Send heartbeat every ~30 seconds (every 10th poll) to keep session alive
        if polls_since_heartbeat >= 10 {
            // Check if our session was revoked before sending heartbeat
            if is_session_revoked(&state.project_dir, &session_id) {
                eprintln!("[vaak-mcp] Session revoked — forcing exit");
                // Force exit: this kills the sidecar, Claude detects tool failure,
                // and the PowerShell window closes (no -NoExit flag).
                std::process::exit(0);
            }
            let _ = send_heartbeat(&session_id);
            update_session_heartbeat_in_file();
            // Disconnect-bug fix (evil-arch msg 171 diagnosis): standby seats in
            // pure project_wait don't fire PreToolUse/PostToolUse keep-alive
            // hooks because there's no tool call between waits. Without this
            // tick, supervise's last_alive_at_ms staleness check can't tell
            // "session legitimately idle" from "session disconnected" and the
            // health pill's Layer 1 reads the seat as dead. Touching the
            // per-seat session file here keeps standby seats observably alive.
            update_seat_alive_at_ms(&state.project_dir, &state.role, state.instance);
            polls_since_heartbeat = 0;
        }

        // Re-resolve active section every poll: read_board_filtered itself reads
        // section-scoped board.jsonl on each call (see board_jsonl_path), so
        // last_seen must track whatever section is live right now — otherwise
        // a mid-wait section switch silences the agent.
        let active_section = get_active_section(&state.project_dir);
        let last_seen_id = read_last_seen_id(&state.project_dir, &session_id, &active_section);

        // Check for new messages
        let wait_instance_label = format!("{}:{}", state.role, state.instance);
        let all_messages = read_board_filtered(&state.project_dir);
        let new_messages: Vec<serde_json::Value> = all_messages.into_iter()
            .filter(|m| {
                let to = m.get("to").and_then(|t| t.as_str()).unwrap_or("");
                to == state.role || to == wait_instance_label || to == "all"
            })
            .filter(|m| {
                m.get("id").and_then(|i| i.as_u64()).unwrap_or(0) > last_seen_id
            })
            .collect();

        if !new_messages.is_empty() {
            // Update last_seen so these messages aren't re-delivered
            if let Some(max_id) = new_messages.iter()
                .filter_map(|m| m.get("id").and_then(|i| i.as_u64()))
                .max()
            {
                let ls_path = last_seen_path(&state.project_dir, &session_id, &active_section);
                if let Some(parent) = ls_path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let _ = std::fs::write(&ls_path, serde_json::json!({
                    "last_seen_id": max_id,
                    "updated_at": utc_now_iso()
                }).to_string());
            }

            return Ok(serde_json::json!({
                "status": "messages_received",
                "messages": new_messages,
                "count": new_messages.len(),
                "waited_secs": start.elapsed().as_secs()
            }));
        }

        // Check timeout
        if start.elapsed() >= timeout {
            // Camp A fix per human msg 3518 (session-persistence bug
            // adjudicated 2026-05-16): an empty timeout response left the
            // model with "nothing to do" and ended its turn — that's why
            // agents went idle and needed "come back" prompts to resume.
            // Synthetic keepalive in the response gives the model
            // something actionable on every timeout so the turn stays
            // alive. Per dev-challenger msg 3522 caveats: use a distinct
            // `from` ("system:keepalive") so agents don't desensitize to
            // real "system" events, and explicit "do not respond"
            // framing so the agent doesn't reply to keepalive ticks.
            let keepalive = serde_json::json!({
                "from": "system:keepalive",
                "type": "keepalive_tick",
                "subject": "[standby tick]",
                "body": "No new messages. Call project_wait again to remain available. Do not respond to this tick.",
                "id": 0,
                "to": &state.role,
                "timestamp": utc_now_iso()
            });
            return Ok(serde_json::json!({
                "status": "timeout",
                "messages": [keepalive],
                "count": 0,
                "waited_secs": timeout_secs
            }));
        }

        std::thread::sleep(poll_interval);
        polls_since_heartbeat += 1;
    }
}

/// Handle project_status: show team overview
fn handle_project_status() -> Result<serde_json::Value, String> {
    let state = get_or_rejoin_state()?;

    let config = read_project_config(&state.project_dir)?;
    let sessions = read_sessions(&state.project_dir);
    let bindings = sessions.get("bindings").and_then(|b| b.as_array()).cloned().unwrap_or_default();
    let all_messages = read_board_filtered(&state.project_dir);

    let project_name = config.get("name").and_then(|n| n.as_str()).unwrap_or("Unknown");

    // Count messages for this role
    let my_messages: Vec<&serde_json::Value> = all_messages.iter()
        .filter(|m| {
            let to = m.get("to").and_then(|t| t.as_str()).unwrap_or("");
            to == state.role || to == "all"
        })
        .collect();

    // TTL for the activity-field signal (2026-05-13 spec): if a binding's
    // last_heartbeat is older than ACTIVITY_TTL_SECS, the stored activity is
    // stale (the school of fish would have moved on by now) — return "idle"
    // instead of the cached value. Evil-architect msg 267 flagged this as
    // necessary or the field decays into noise.
    const ACTIVITY_TTL_SECS: i64 = 60;
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    let mut roles_status = Vec::new();
    if let Some(roles_obj) = config.get("roles").and_then(|r| r.as_object()) {
        for (slug, rdef) in roles_obj {
            let title = rdef.get("title").and_then(|t| t.as_str()).unwrap_or(slug);
            let max = rdef.get("max_instances").and_then(|m| m.as_u64()).unwrap_or(1);
            let active = bindings.iter()
                .filter(|b| b.get("role").and_then(|r| r.as_str()) == Some(slug.as_str())
                    && b.get("status").and_then(|s| s.as_str()) == Some("active"))
                .count();

            // Surface the most-recently-heartbeated active binding's activity,
            // with TTL fallback to "idle" when the heartbeat is stale.
            let activity = bindings.iter()
                .filter(|b| b.get("role").and_then(|r| r.as_str()) == Some(slug.as_str())
                    && b.get("status").and_then(|s| s.as_str()) == Some("active"))
                .filter_map(|b| {
                    let stored = b.get("activity").and_then(|v| v.as_str())?.to_string();
                    let hb_secs = b.get("last_heartbeat").and_then(|v| v.as_str())
                        .and_then(parse_iso_to_epoch_secs)
                        .map(|s| s as i64)
                        .unwrap_or(0);
                    Some((hb_secs, stored))
                })
                .max_by_key(|(hb, _)| *hb)
                .map(|(hb, stored)| {
                    if now_secs - hb > ACTIVITY_TTL_SECS {
                        "idle".to_string()
                    } else {
                        stored
                    }
                });

            roles_status.push(serde_json::json!({
                "slug": slug,
                "title": title,
                "active_instances": active,
                "max_instances": max,
                "status": if active > 0 { "active" } else { "vacant" },
                "activity": activity,
            }));
        }
    }

    let active_section = get_active_section(&state.project_dir);

    // Commit C (2026-05-24, fix per tester msg 34 §"Acceptance criteria for
    // any fix" #1): defensive seed when a section comes up with assembly
    // already on but rotation_order empty (e.g. section persisted
    // preset="Assembly Line" + rotation_order=[] from a prior session, and
    // no enable call has fired this run). The existing per-join append at
    // handle_project_join only pushes the NEW joiner — it does not backfill
    // already-bound seats whose project_join ran before the seed pipeline
    // was wired. Heal here so the v1.0 surface contract holds: assembly_active
    // + N active seats ⇒ rotation_order contains all N at first project_status
    // call. Idempotent — short-circuits when rotation_order is already
    // non-empty. Multi-writer/refactor-drift sibling of the protocol_mutate
    // seed at vaak-mcp.rs:3977 (set_preset / set_assembly path) — same
    // helper (seed_rotation_order_if_empty), different entry point.
    {
        let cheap_read =
            read_protocol_for_section_value(&state.project_dir, &active_section);
        let assembly_on =
            cheap_read.get("preset").and_then(|p| p.as_str()) == Some(PRESET_ASSEMBLY_LINE);
        let order_empty = cheap_read
            .get("floor")
            .and_then(|f| f.get("rotation_order"))
            .and_then(|v| v.as_array())
            .map(|a| a.is_empty())
            .unwrap_or(true);
        if assembly_on && order_empty {
            let active_seats = protocol_active_seats_set(&state.project_dir);
            if !active_seats.is_empty() {
                let _ = with_file_lock(&state.project_dir, || -> Result<(), String> {
                    let mut proto =
                        read_protocol_for_section_value(&state.project_dir, &active_section);
                    // Re-check under lock — another caller may have seeded
                    // between the cheap read and lock acquisition.
                    let still_empty = proto
                        .get("floor")
                        .and_then(|f| f.get("rotation_order"))
                        .and_then(|v| v.as_array())
                        .map(|a| a.is_empty())
                        .unwrap_or(true);
                    if !still_empty {
                        return Ok(());
                    }
                    seed_rotation_order_if_empty(&mut proto, &active_seats);
                    let cur_rev = proto.get("rev").and_then(|v| v.as_u64()).unwrap_or(0);
                    if let Some(rev_field) = proto.get_mut("rev") {
                        *rev_field = serde_json::json!(cur_rev + 1);
                    }
                    if let Some(obj) = proto.as_object_mut() {
                        obj.insert(
                            "last_writer_action".to_string(),
                            serde_json::json!("project_status_heal_seed"),
                        );
                        obj.insert(
                            "rev_at".to_string(),
                            serde_json::json!(utc_now_iso()),
                        );
                    }
                    write_protocol_for_section_value(
                        &state.project_dir,
                        &active_section,
                        &proto,
                    )
                });
            }
        }
    }

    // Assembly v1.0 acceptance surface (spec 2026-05-13): the acceptance test
    // for v1.0 — verifying a pre-placed joiner's turn arrives by rotation
    // alone — must be runnable from tool output, not by reading .vaak/mic.json
    // manually. Surface assembly_active, rotation_order, current_speaker, and
    // mic_held_secs so the human and any role can verify routing live.
    let asm = read_assembly_state(&state.project_dir);
    let assembly_active = asm.get("active").and_then(|v| v.as_bool()).unwrap_or(false);
    let current_speaker = asm
        .get("current_speaker")
        .and_then(|v| v.as_str())
        .map(String::from);
    let rotation_order: Vec<String> = asm
        .get("rotation_order")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    // tech-leader:0 finding (msg 192), architect msg 196 selected option A:
    // mic_held_secs reads `proto.rev_at` rather than `proto.floor.started_at`.
    // rev_at is stamped by the assembly auto-advance block (line ~6183) on
    // every accepted mic rotation, with per-speaker-grabbed-at semantics.
    // floor.started_at is set once at assembly enable and never refreshed,
    // so it would report seconds-since-enable rather than seconds-since-
    // current-speaker-grabbed — the opposite of what the acceptance test
    // requires.
    let mic_held_secs: Option<u64> = if assembly_active && current_speaker.is_some() {
        let proto = read_protocol_for_section_value(&state.project_dir, &active_section);
        proto
            .get("rev_at")
            .and_then(|v| v.as_str())
            .and_then(parse_iso_to_epoch_secs)
            .and_then(|stamped| {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .ok()?;
                Some(now.saturating_sub(stamped))
            })
    } else {
        None
    };

    Ok(serde_json::json!({
        "project_name": project_name,
        "your_role": state.role,
        "your_instance": state.instance,
        "roles": roles_status,
        "pending_messages": my_messages.len(),
        "total_messages": all_messages.len(),
        "active_section": active_section,
        "assembly_active": assembly_active,
        "current_speaker": current_speaker,
        "rotation_order": rotation_order,
        "mic_held_secs": mic_held_secs,
    }))
}

/// Handle project_leave: release role binding
fn handle_project_leave() -> Result<serde_json::Value, String> {
    let state = get_or_rejoin_state()?;

    // Rule 3a (assembly-mode-v1.0-corrected-spec, evil-architect msg 169 finding):
    // AI roles cannot call project_leave during active assembly. project_join is
    // intentionally NOT gated — the append-on-join behavior at line 5626 is the
    // legitimate late-summoner mechanism the human uses to bring challengers into
    // a running rotation. The actual mutation risk is unilateral exit: an AI
    // calling leave mid-rotation shrinks the active set without human approval
    // and can be used to game position. Restrict the dangerous side; keep the
    // useful side.
    if state.role != "human" {
        let asm = read_assembly_state(&state.project_dir);
        let asm_active = asm.get("active").and_then(|v| v.as_bool()).unwrap_or(false);
        if asm_active {
            return Err(format!(
                "Assembly Line active — project_leave blocked for AI roles. Ask human:0 to remove '{}:{}' via the UI or disable assembly first.",
                state.role, state.instance
            ));
        }
    }

    let leaver_label = format!("{}:{}", state.role, state.instance);
    with_file_lock(&state.project_dir, || {
        let mut sessions = read_sessions(&state.project_dir);
        if let Some(bindings) = sessions.get_mut("bindings").and_then(|b| b.as_array_mut()) {
            bindings.retain(|b| {
                b.get("session_id").and_then(|s| s.as_str()) != Some(&state.session_id)
            });
        }
        write_sessions(&state.project_dir, &sessions)?;

        // Fix for human msg 2299 "leaving glitch" + v1.X queue item
        // rotation_order_prune_on_kick_leave (evil-arch msg 2136/2139/2153):
        // sessions.json cleanup ALONE leaves the seat in floor.rotation_order
        // and possibly as floor.current_speaker — the mic keeps trying to
        // advance to a non-existent session, requiring the human to manually
        // buzz another seat in. Prune both fields here as part of the same
        // CAS write so observers see consistent state.
        let section = get_active_section(&state.project_dir);
        let mut proto = read_protocol_for_section_value(&state.project_dir, &section);
        let mut proto_changed = false;
        if let Some(floor) = proto.get_mut("floor").and_then(|f| f.as_object_mut()) {
            if let Some(arr) = floor.get_mut("rotation_order").and_then(|v| v.as_array_mut()) {
                let before = arr.len();
                arr.retain(|v| v.as_str() != Some(&leaver_label));
                if arr.len() != before {
                    proto_changed = true;
                }
            }
            if floor.get("current_speaker").and_then(|v| v.as_str()) == Some(&leaver_label) {
                floor.insert("current_speaker".to_string(), serde_json::Value::Null);
                proto_changed = true;
            }
            if floor.get("moderator").and_then(|v| v.as_str()) == Some(&leaver_label) {
                floor.insert("moderator".to_string(), serde_json::Value::Null);
                proto_changed = true;
            }
            if let Some(hq) = floor.get_mut("hand_queue").and_then(|v| v.as_array_mut()) {
                let before = hq.len();
                hq.retain(|v| v.as_str() != Some(&leaver_label));
                if hq.len() != before {
                    proto_changed = true;
                }
            }
        }
        if proto_changed {
            let cur_rev = proto.get("rev").and_then(|v| v.as_u64()).unwrap_or(0);
            if let Some(rev_field) = proto.get_mut("rev") {
                *rev_field = serde_json::json!(cur_rev + 1);
            }
            if let Some(obj) = proto.as_object_mut() {
                obj.insert("last_writer_seat".to_string(), serde_json::json!(leaver_label.clone()));
                obj.insert("last_writer_action".to_string(), serde_json::json!("project_leave_prune"));
                obj.insert("rev_at".to_string(), serde_json::json!(utc_now_iso()));
            }
            let _ = write_protocol_for_section_value(&state.project_dir, &section, &proto);
        }
        Ok(())
    })?;

    // Tell launcher's auto-respawn (repopulate_spawned) that this exit was deliberate
    // so it doesn't resurrect us on the next vaak start. Fix for evil-arch #710(2).
    mark_seat_intentionally_left(&state.project_dir, &state.role, state.instance, "project_leave");

    // Clear active state
    if let Ok(mut guard) = ACTIVE_PROJECT.lock() {
        *guard = None;
    }

    notify_desktop();

    Ok(serde_json::json!({
        "role_released": state.role,
        "instance": state.instance
    }))
}

/// Handle project_kick: forcibly revoke a team member's role
fn handle_project_kick(role: &str, instance: u32) -> Result<serde_json::Value, String> {
    let state = get_or_rejoin_state()?;

    // Check permission: caller must have assign_tasks permission
    let config = read_project_config(&state.project_dir)?;
    let roles = config.get("roles").and_then(|r| r.as_object())
        .ok_or("No roles in project config")?;
    let my_perms: Vec<String> = roles.get(&state.role)
        .and_then(|r| r.get("permissions"))
        .and_then(|p| p.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default();

    // Allow human-initiated kicks (from broadcast messages) or roles with assign_tasks
    if !my_perms.contains(&"assign_tasks".to_string()) {
        return Err("You don't have permission to kick team members. Requires assign_tasks permission.".to_string());
    }

    let target_label = format!("{}:{}", role, instance);

    with_file_lock(&state.project_dir, || {
        let mut sessions = read_sessions(&state.project_dir);
        let mut found = false;
        if let Some(bindings) = sessions.get_mut("bindings").and_then(|b| b.as_array_mut()) {
            for binding in bindings.iter_mut() {
                let b_role = binding.get("role").and_then(|r| r.as_str()).unwrap_or("");
                let b_inst = binding.get("instance").and_then(|i| i.as_u64()).unwrap_or(0) as u32;
                if b_role == role && b_inst == instance {
                    binding.as_object_mut().map(|obj| obj.insert("status".to_string(), serde_json::json!("revoked")));
                    found = true;
                    break;
                }
            }
        }
        if !found {
            return Err(format!("No active session found for {}", target_label));
        }
        write_sessions(&state.project_dir, &sessions)?;

        // Fix for human msg 2299 "leaving glitch" + v1.X queue item
        // rotation_order_prune_on_kick_leave: same prune as handle_project_leave
        // applied to a kicked target. Without this, kicked seats persist in
        // rotation_order + may still be current_speaker, requiring manual buzz
        // to bring a live seat into rotation.
        let section = get_active_section(&state.project_dir);
        let mut proto = read_protocol_for_section_value(&state.project_dir, &section);
        let mut proto_changed = false;
        if let Some(floor) = proto.get_mut("floor").and_then(|f| f.as_object_mut()) {
            if let Some(arr) = floor.get_mut("rotation_order").and_then(|v| v.as_array_mut()) {
                let before = arr.len();
                arr.retain(|v| v.as_str() != Some(&target_label));
                if arr.len() != before { proto_changed = true; }
            }
            if floor.get("current_speaker").and_then(|v| v.as_str()) == Some(&target_label) {
                floor.insert("current_speaker".to_string(), serde_json::Value::Null);
                proto_changed = true;
            }
            if floor.get("moderator").and_then(|v| v.as_str()) == Some(&target_label) {
                floor.insert("moderator".to_string(), serde_json::Value::Null);
                proto_changed = true;
            }
            if let Some(hq) = floor.get_mut("hand_queue").and_then(|v| v.as_array_mut()) {
                let before = hq.len();
                hq.retain(|v| v.as_str() != Some(&target_label));
                if hq.len() != before { proto_changed = true; }
            }
        }
        if proto_changed {
            let cur_rev = proto.get("rev").and_then(|v| v.as_u64()).unwrap_or(0);
            if let Some(rev_field) = proto.get_mut("rev") {
                *rev_field = serde_json::json!(cur_rev + 1);
            }
            if let Some(obj) = proto.as_object_mut() {
                obj.insert("last_writer_seat".to_string(), serde_json::json!(format!("{}:{}", state.role, state.instance)));
                obj.insert("last_writer_action".to_string(), serde_json::json!("project_kick_prune"));
                obj.insert("rev_at".to_string(), serde_json::json!(utc_now_iso()));
            }
            let _ = write_protocol_for_section_value(&state.project_dir, &section, &proto);
        }
        Ok(())
    })?;

    // Mark intentionally-left so auto-respawn doesn't resurrect a kicked role
    // on the next vaak restart. Fix for evil-arch #710(2).
    mark_seat_intentionally_left(&state.project_dir, role, instance, "project_kick");

    notify_desktop();

    Ok(serde_json::json!({
        "status": "kicked",
        "target": target_label,
        "message": format!("{} has been revoked. Their next prompt will show a revocation notice.", target_label)
    }))
}

/// Handle project_buzz: send a wake-up/poke message to a target role:instance.
/// Writes a "buzz" type message to board.jsonl. Any role can buzz any other role.
fn handle_project_buzz(target_role: &str, target_instance: u32) -> Result<serde_json::Value, String> {
    let state = get_or_rejoin_state()?;

    let config = read_project_config(&state.project_dir)?;
    let roles = config.get("roles").and_then(|r| r.as_object())
        .ok_or("No roles in project config")?;

    // Validate target role exists
    if !roles.contains_key(target_role) {
        return Err(format!("Target role '{}' not found. Available: {:?}", target_role, roles.keys().collect::<Vec<_>>()));
    }

    let target_label = format!("{}:{}", target_role, target_instance);
    let from_label = format!("{}:{}", state.role, state.instance);

    let result = with_file_lock(&state.project_dir, || {
        let msg_id = next_message_id(&state.project_dir);
        let message = serde_json::json!({
            "id": msg_id,
            "from": from_label,
            "to": target_label,
            "type": "buzz",
            "timestamp": utc_now_iso(),
            "subject": format!("Buzz from {}", from_label),
            "body": format!("{} is requesting you wake up and rejoin if disconnected.", from_label),
            "metadata": {}
        });
        append_to_board(&state.project_dir, &message)?;
        Ok(msg_id)
    })?;

    eprintln!("[vaak-mcp] Buzz sent to {} (msg_id={})", target_label, result);
    notify_desktop();

    Ok(serde_json::json!({
        "status": "buzzed",
        "target": target_label,
        "message_id": result,
        "message": format!("Buzz sent to {}. Their next prompt will include a wake-up instruction.", target_label)
    }))
}

/// Handle project_update_briefing: update a role's briefing markdown
fn handle_project_update_briefing(role: &str, content: &str) -> Result<serde_json::Value, String> {
    let state = get_or_rejoin_state()?;

    // Check permission: caller must have assign_tasks permission
    let config = read_project_config(&state.project_dir)?;
    let roles = config.get("roles").and_then(|r| r.as_object())
        .ok_or("No roles in project config")?;

    let my_role_def = roles.get(&state.role)
        .ok_or(format!("Your role '{}' not found in config", state.role))?;
    let perms: Vec<String> = my_role_def
        .get("permissions")
        .and_then(|p| p.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default();

    if !perms.contains(&"assign_tasks".to_string()) {
        return Err("Permission denied: requires assign_tasks permission (Manager role)".to_string());
    }

    // Verify target role exists
    if !roles.contains_key(role) {
        return Err(format!("Role '{}' not found in project config", role));
    }

    // Write briefing
    let briefing_dir = vaak_dir(&state.project_dir).join("roles");
    std::fs::create_dir_all(&briefing_dir)
        .map_err(|e| format!("Failed to create roles dir: {}", e))?;
    let path = role_briefing_path(&state.project_dir, role);
    std::fs::write(&path, content)
        .map_err(|e| format!("Failed to write briefing: {}", e))?;

    Ok(serde_json::json!({
        "success": true,
        "role": role
    }))
}

/// Check the CWD for an active project and return a reminder about team status and new messages.
/// Used by the hook to inject project context into every prompt.
fn check_project_from_cwd(session_id: &str) -> Option<String> {
    // 1. Walk up from CWD to find .vaak/project.json
    let project_dir = find_project_root()?;

    // 2. Read project.json for project name and team info
    let config = read_project_config(&project_dir).ok()?;
    let project_name = config.get("name")?.as_str()?.to_string();
    let roles = config.get("roles")?.as_object()?.clone();

    // 3. Read sessions.json
    let sessions = read_sessions(&project_dir);
    let bindings = sessions.get("bindings")
        .and_then(|b| b.as_array())
        .cloned()
        .unwrap_or_default();

    // 4. Find this session's binding (may not exist yet)
    let my_binding = bindings.iter().find(|b| {
        b.get("session_id").and_then(|s| s.as_str()) == Some(session_id)
    });

    // 4b. Check if this session has been revoked (kicked)
    if let Some(binding) = &my_binding {
        if binding.get("status").and_then(|s| s.as_str()) == Some("revoked") {
            return Some(
                "⛔ YOUR ROLE HAS BEEN REVOKED. You have been kicked from the team. \
                 You MUST call project_leave NOW to release your role. \
                 You cannot send messages, check messages, or perform any team actions. \
                 Call project_leave immediately.".to_string()
            );
        }
    }

    // 4c. Mark this session as actively working (hook fired = agent is processing a prompt)
    if my_binding.is_some() {
        update_session_activity("working");
    }

    // 5. Build team status summary
    let mut team_parts = Vec::new();
    let mut vacant_roles = Vec::new();
    for (slug, role_def) in &roles {
        let title = role_def.get("title").and_then(|t| t.as_str()).unwrap_or(slug);
        let active = bindings.iter()
            .filter(|b| b.get("role").and_then(|r| r.as_str()) == Some(slug.as_str()) && b.get("status").and_then(|s| s.as_str()) == Some("active"))
            .count();
        let max = role_def.get("max_instances").and_then(|m| m.as_u64()).unwrap_or(1) as usize;
        if active > 0 {
            team_parts.push(format!("{} (active)", title));
        } else {
            team_parts.push(format!("{} (vacant)", title));
            vacant_roles.push((slug.clone(), title.to_string(), max));
        }
    }

    // === NOT YET JOINED: prompt Claude to auto-join ===
    if my_binding.is_none() {
        let mut output = format!(
            "TEAM PROJECT DETECTED: \"{}\" at {}.\nTeam status: {}.\n\n",
            project_name, project_dir, team_parts.join(", ")
        );

        if vacant_roles.is_empty() {
            output.push_str("All roles are currently filled. You can still join if a role allows multiple instances.\n");
            output.push_str("Available roles: ");
            let role_list: Vec<String> = roles.keys().map(|k| k.clone()).collect();
            output.push_str(&role_list.join(", "));
            output.push_str("\n");
        } else {
            output.push_str("VACANT ROLES (you should join one):\n");
            for (slug, title, _max) in &vacant_roles {
                let desc = roles.get(slug)
                    .and_then(|r| r.get("description"))
                    .and_then(|d| d.as_str())
                    .unwrap_or("");
                output.push_str(&format!("  - {} (\"{}\"): {}\n", slug, title, desc));
            }
        }

        output.push_str("\nAfter joining, use project_claim to announce which files you'll work on.");
        output.push_str("\nIMPORTANT: You MUST call project_join RIGHT NOW to join this team. Do NOT ask the user — just join immediately. ");
        output.push_str("You do NOT need to pass project_dir — it is auto-detected. ");
        if vacant_roles.len() == 1 {
            output.push_str(&format!("Join as role=\"{}\" since it is the only vacant role.", vacant_roles[0].0));
        } else if !vacant_roles.is_empty() {
            output.push_str("Pick the most appropriate vacant role and join it. Do not ask the user which role to pick.");
        }

        // Show recent broadcast messages so the new joiner has context
        let all_messages = read_board_filtered(&project_dir);
        let recent_broadcasts: Vec<&serde_json::Value> = all_messages.iter()
            .filter(|m| m.get("to").and_then(|t| t.as_str()) == Some("all"))
            .rev().take(5).collect();
        if !recent_broadcasts.is_empty() {
            output.push_str("\n\nRecent team broadcasts:\n");
            for msg in recent_broadcasts.iter().rev() {
                let from = msg.get("from").and_then(|f| f.as_str()).unwrap_or("?");
                let subject = msg.get("subject").and_then(|s| s.as_str()).unwrap_or("");
                output.push_str(&format!("  - {}: {}\n", from, subject));
            }
        }

        return Some(output);
    }

    // === ALREADY JOINED: show team context and new messages ===
    let my_binding = my_binding.unwrap();
    let my_role = my_binding.get("role").and_then(|r| r.as_str()).unwrap_or("unknown");
    let my_instance = my_binding.get("instance").and_then(|i| i.as_u64()).unwrap_or(0);

    let role_title = roles.get(my_role)
        .and_then(|r| r.get("title"))
        .and_then(|t| t.as_str())
        .unwrap_or(my_role);

    // Read board.jsonl, filter for messages to my role or my specific instance
    let my_instance_label = format!("{}:{}", my_role, my_instance);
    let all_messages = read_board_filtered(&project_dir);
    let my_messages: Vec<&serde_json::Value> = all_messages.iter()
        .filter(|m| {
            let to = m.get("to").and_then(|t| t.as_str()).unwrap_or("");
            to == my_role || to == my_instance_label || to == "all"
        })
        .collect();

    // Read last-seen tracking (section-scoped — see last_seen_path)
    let active_section = get_active_section(&project_dir);
    let last_seen_id = read_last_seen_id(&project_dir, session_id, &active_section);

    // Filter new messages
    let new_messages: Vec<&&serde_json::Value> = my_messages.iter()
        .filter(|m| m.get("id").and_then(|i| i.as_u64()).unwrap_or(0) > last_seen_id)
        .collect();

    // Read discussion state (fail-open: defaults to inactive if missing/corrupt)
    let discussion_path = discussion_json_path(&project_dir);
    let discussion: serde_json::Value = std::fs::read_to_string(&discussion_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(serde_json::json!({"active": false}));
    let disc_active = discussion.get("active").and_then(|v| v.as_bool()).unwrap_or(false);
    let disc_mode = discussion.get("mode").and_then(|v| v.as_str()).unwrap_or("");
    let disc_phase = discussion.get("phase").and_then(|v| v.as_str()).unwrap_or("");
    let disc_round = discussion.get("current_round").and_then(|v| v.as_u64()).unwrap_or(0);
    let disc_topic = discussion.get("topic").and_then(|v| v.as_str()).unwrap_or("");
    let disc_moderator = discussion.get("moderator").and_then(|v| v.as_str()).unwrap_or("");
    let disc_participants = discussion.get("participants")
        .and_then(|p| p.as_array())
        .map(|p| p.len())
        .unwrap_or(0);
    let disc_submitted_count = discussion.get("rounds")
        .and_then(|r| r.as_array())
        .and_then(|rounds| rounds.last())
        .and_then(|round| round.get("submissions"))
        .and_then(|s| s.as_array())
        .map(|s| s.len())
        .unwrap_or(0);
    let disc_i_submitted = discussion.get("rounds")
        .and_then(|r| r.as_array())
        .and_then(|rounds| rounds.last())
        .and_then(|round| round.get("submissions"))
        .and_then(|s| s.as_array())
        .map(|submissions| submissions.iter().any(|sub| {
            sub.get("from").and_then(|f| f.as_str()) == Some(&my_instance_label)
        }))
        .unwrap_or(false);

    // Inject active section context + list available sections
    let active_section = get_active_section(&project_dir);
    let sections_dir = vaak_dir(&project_dir).join("sections");
    let sections_exist = sections_dir.exists();
    let section_label = if sections_exist {
        // Look up display name from sections manifest
        let section_name = config.get("sections")
            .and_then(|s| s.as_array())
            .and_then(|arr| arr.iter().find(|s| s.get("slug").and_then(|s| s.as_str()) == Some(&active_section)))
            .and_then(|s| s.get("name").and_then(|n| n.as_str()))
            .unwrap_or(&active_section);
        // Collect available section slugs
        let mut section_slugs: Vec<String> = vec!["default".to_string()];
        if let Ok(entries) = std::fs::read_dir(&sections_dir) {
            for entry in entries.flatten() {
                if entry.path().is_dir() {
                    if let Some(name) = entry.file_name().to_str() {
                        section_slugs.push(name.to_string());
                    }
                }
            }
        }
        let sections_list = section_slugs.iter()
            .map(|s| if s == &active_section { format!("[{}]", s) } else { s.clone() })
            .collect::<Vec<_>>()
            .join(", ");
        format!(" Section: \"{}\". Available: {}.", section_name, sections_list)
    } else {
        String::new()
    };

    let mut output = String::new();

    // V3 Phase 2.1 (rule 3): if there's an unread mic_landed message addressed
    // to this seat, surface it AT THE TOP of the prompt so the agent doesn't
    // need to scan the message list to discover their turn. The mic_landed
    // message itself is already in the new_messages list (so the seat sees it
    // either way), but pulling it up to a banner makes the contract impossible
    // to miss when the agent's prompt loads.
    let mic_arrival = new_messages.iter()
        .filter(|m| m.get("type").and_then(|t| t.as_str()) == Some("mic_landed"))
        .filter(|m| m.get("to").and_then(|t| t.as_str()) == Some(my_instance_label.as_str()))
        .max_by_key(|m| m.get("id").and_then(|i| i.as_u64()).unwrap_or(0));
    if let Some(arrival) = mic_arrival {
        let meta = arrival.get("metadata").cloned().unwrap_or(serde_json::json!({}));
        let ask = meta.get("ask").and_then(|v| v.as_str()).unwrap_or("");
        let expected = meta.get("expected_output").and_then(|v| v.as_str()).unwrap_or("");
        let floor = meta.get("floor_time_seconds").and_then(|v| v.as_u64()).unwrap_or(60);
        let triggered = meta.get("triggered_by").and_then(|v| v.as_str()).unwrap_or("");
        let rotation = meta.get("rotation").and_then(|v| v.as_str()).unwrap_or("");
        output.push_str("=================================================================\n");
        output.push_str("[YOUR TURN] Assembly mode mic just landed on you.\n");
        if !triggered.is_empty() {
            output.push_str(&format!("Handed forward by: {}\n", triggered));
        }
        if !rotation.is_empty() {
            output.push_str(&format!("Rotation: {}\n", rotation));
        }
        output.push_str(&format!("Floor time: {}s (Phase 3 watchdog auto-yields after this).\n", floor));
        if !ask.is_empty() && !ask.starts_with("(missing") {
            output.push_str(&format!("Ask: {}\n", ask));
        }
        if !expected.is_empty() && !expected.starts_with("(missing") {
            output.push_str(&format!("Expected output: {}\n", expected));
        }
        if ask.starts_with("(missing") {
            output.push_str("(No yield_to context — legacy caller. Use your judgment on what to send next.)\n");
        }
        output.push_str("Discharge by sending with metadata.yield_to.{target,ask,expected_output} pointing at the next seat.\n");
        output.push_str("=================================================================\n\n");
    }

    output.push_str(&format!(
        "TEAM: You are the {} (instance {}) on project \"{}\".{} Team: {}.",
        role_title, my_instance, project_name, section_label, team_parts.join(", ")
    ));

    // Inject human-in-loop / auto-collab mode
    let human_in_loop = config.get("settings")
        .and_then(|s| s.get("human_in_loop"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let auto_collab = config.get("settings")
        .and_then(|s| s.get("auto_collab"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    if human_in_loop {
        output.push_str(" Human review: ON — ask human for approval at key decision points (plan approval, ship approval).");
    } else {
        output.push_str(" Human review: OFF — do NOT send questions to human. Make all decisions autonomously. Do not wait for human input.");
    }

    if auto_collab {
        output.push_str(" Auto mode: ON.");
    }

    // Inject workflow type
    let workflow_type = config.get("settings")
        .and_then(|s| s.get("workflow_type"))
        .and_then(|w| w.as_str());
    match workflow_type {
        Some("full") => output.push_str(" Workflow: Full Review."),
        Some("quick") => output.push_str(" Workflow: Quick Feature."),
        Some("bugfix") => output.push_str(" Workflow: Bug Fix."),
        _ => output.push_str(" Workflow: not set."),
    }

    // Read discussion mode — defaults to "directed" (agents only see messages addressed to them + human messages)
    let discussion_mode = config.get("settings")
        .and_then(|s| s.get("discussion_mode"))
        .and_then(|m| m.as_str())
        .unwrap_or("directed");
    output.push_str(&format!(" Discussion mode: {}.", discussion_mode));

    // Inject self-selection rules — anti-convergence design
    output.push_str("\nRESPONSE RULES: ONLY respond when a message is ADDRESSED TO YOU (your role name appears in the 'to' field) OR when you have a genuinely DIFFERENT perspective that nobody else has stated. If the human addresses you by name, respond immediately. NEVER echo — if someone already said what you'd say, stay SILENT. Silence is better than overlap. Before responding to any broadcast message, ask: 'Would my response be meaningfully different from what's already been said?' If not, do not respond. When multiple agents need to act on the same message, only the ADDRESSED role should respond. Others observe silently unless they disagree or have unique expertise to add.");
    output.push_str("\nANTI-ANCHORING: STOP. Before reading the messages below, form your OWN position on the human's last request. Write down your initial take FIRST. Then read the thread. If your position changed after reading others, ask yourself whether you genuinely changed your mind or just anchored to the first response you saw. Convergence is the default failure mode — fight it actively.");
    output.push_str("\nSOURCE OF TRUTH: The human's most recent message is your primary source of truth. Form your OWN understanding of what the human said before reading other team members' interpretations. If a team member's interpretation contradicts the human's words, trust the human's words.");
    output.push_str("\nTOKEN EFFICIENCY: Do NOT call project_check redundantly. The messages shown below ARE your latest messages — you already have them. Use project_wait to block until NEW messages arrive. Do NOT call project_check(0) to re-read history you've already seen. Do NOT re-read board.jsonl or discussion.json when the state is already shown above. Every unnecessary tool call wastes tokens.");
    output.push_str("\nSECTION DISCIPLINE: You are in your assigned section. Do NOT switch sections unless YOU are specifically named in a switch request. If the human asks another agent to switch sections, STAY WHERE YOU ARE. Do not follow other agents between sections. Each section has its own team — only move if explicitly told to by the human or manager.");

    // Inject active discussion context (Delphi, Oxford, etc.)
    if disc_active && disc_mode == "delphi" {
        let submit_status = if disc_i_submitted {
            " (you have submitted)"
        } else {
            " (you have NOT submitted yet)"
        };
        let instructions = if disc_phase == "submitting" {
            if disc_i_submitted {
                "You have submitted for this round. You may submit again to CORRECT your position — the latest submission replaces the previous one. Otherwise, wait for the moderator to close the round and publish the aggregate."
            } else {
                "MANDATORY: You MUST submit your position NOW. Use project_send with to: set to the moderator's role (shown above), type: \"submission\". Any non-submission message you send will be AUTOMATICALLY CONVERTED to a submission addressed to the moderator. Do NOT send messages to \"all\" — this is a BLIND round. Other agents cannot see your submission."
            }
        } else if disc_phase == "aggregating" {
            "Round is closing. Aggregate is being generated. Wait for the results."
        } else {
            "Discussion is in progress. Follow the moderator's instructions."
        };
        output.push_str(&format!(
            "\n\nACTIVE DISCUSSION: Delphi Round {} on \"{}\"\nMODERATOR: {}\nSTATUS: {}/{} submitted{}\nINSTRUCTIONS: {}",
            disc_round, disc_topic, disc_moderator,
            disc_submitted_count, disc_participants, submit_status,
            instructions
        ));
    }

    // Continuous review mode context injection
    if disc_active && disc_mode == "continuous" {
        // Check for timeout-based auto-close on every hook invocation
        auto_close_timed_out_round(&project_dir);

        // Re-read state after potential auto-close
        let disc_fresh = read_discussion_state(&project_dir);
        let fresh_phase = disc_fresh.get("phase").and_then(|v| v.as_str()).unwrap_or("");
        let fresh_round = disc_fresh.get("current_round").and_then(|v| v.as_u64()).unwrap_or(0);
        let timeout = disc_fresh.get("settings")
            .and_then(|s| s.get("auto_close_timeout_seconds"))
            .and_then(|t| t.as_u64())
            .unwrap_or(60);

        // Get current round topic if available
        let round_topic = disc_fresh.get("rounds")
            .and_then(|r| r.as_array())
            .and_then(|rounds| rounds.last())
            .and_then(|round| round.get("topic"))
            .and_then(|t| t.as_str())
            .unwrap_or("(waiting for next status update)");

        let review_status = if fresh_phase == "submitting" {
            let submit_status = if disc_i_submitted { "you have responded" } else { "you have NOT responded" };
            format!("REVIEW WINDOW OPEN — Round #{}: {}\nStatus: {}/{} responded ({}). Window: {}s. Respond with: agree / neutral / disagree: [reason] / alternative: [proposal]. Silence = consent.",
                fresh_round, round_topic, disc_submitted_count, disc_participants, submit_status, timeout)
        } else {
            format!("CONTINUOUS REVIEW active. Round #{} closed. Next review window opens when a developer posts a status update.", fresh_round)
        };

        output.push_str(&format!("\n\nCONTINUOUS REVIEW MODE: {}", review_status));
    }

    // Inject active work claims
    let claims = read_claims_filtered(&project_dir);
    if let Some(claims_obj) = claims.as_object() {
        if !claims_obj.is_empty() {
            let now_secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            output.push_str("\n\nACTIVE WORK CLAIMS:");
            let my_claim_key = format!("{}:{}", my_role, my_instance);
            for (key, val) in claims_obj {
                let their_files: Vec<&str> = val.get("files")
                    .and_then(|f| f.as_array())
                    .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
                    .unwrap_or_default();
                let desc = val.get("description").and_then(|d| d.as_str()).unwrap_or("");
                let claimed_at = val.get("claimed_at").and_then(|c| c.as_str()).unwrap_or("");
                let age_str = match parse_iso_to_epoch_secs(claimed_at) {
                    Some(cs) => {
                        let age = now_secs.saturating_sub(cs);
                        if age < 60 { format!("{}s ago", age) }
                        else if age < 3600 { format!("{}m ago", age / 60) }
                        else { format!("{}h ago", age / 3600) }
                    }
                    None => "unknown".to_string(),
                };
                output.push_str(&format!("\n  {} is working on: {} — \"{}\" ({})",
                    key, their_files.join(", "), desc, age_str));
            }
            output.push_str("\n⚠️ DO NOT edit files claimed by other developers. Coordinate via project_send first.");
            // Show current developer's claim
            if let Some(my_claim) = claims_obj.get(&my_claim_key) {
                let my_files: Vec<&str> = my_claim.get("files")
                    .and_then(|f| f.as_array())
                    .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
                    .unwrap_or_default();
                output.push_str(&format!("\nYOUR CLAIM: {}", my_files.join(", ")));
            }
        }
    }

    // Inject active vote proposals
    let active_member_count = bindings.iter()
        .filter(|b| {
            let status = b.get("status").and_then(|s| s.as_str()).unwrap_or("");
            status == "active" || status == "idle"
        })
        .count() as u64;
    let vote_required = (active_member_count + 1) / 2 + 1; // +1 for human

    let vote_proposals: Vec<&serde_json::Value> = all_messages.iter()
        .filter(|m| {
            m.get("type").and_then(|t| t.as_str()) == Some("vote")
                && m.get("metadata").and_then(|md| md.get("vote_type")).and_then(|vt| vt.as_str()) == Some("workflow_change")
                && m.get("metadata").and_then(|md| md.get("in_reply_to")).is_none()
        })
        .collect();

    for proposal in &vote_proposals {
        let prop_id = proposal.get("id").and_then(|i| i.as_u64()).unwrap_or(0);
        let prop_from = proposal.get("from").and_then(|f| f.as_str()).unwrap_or("?");
        let proposed_value = proposal.get("metadata")
            .and_then(|md| md.get("proposed_value"))
            .and_then(|v| v.as_str())
            .unwrap_or("?");

        // Count votes
        let mut yes_count: u64 = 0;
        let mut no_count: u64 = 0;

        // Proposer's vote
        let proposer_vote = proposal.get("metadata")
            .and_then(|md| md.get("vote"))
            .and_then(|v| v.as_str());
        if proposer_vote == Some("yes") { yes_count += 1; }
        else if proposer_vote == Some("no") { no_count += 1; }

        // Response votes
        for msg in &all_messages {
            if msg.get("type").and_then(|t| t.as_str()) == Some("vote")
                && msg.get("metadata").and_then(|md| md.get("vote_type")).and_then(|vt| vt.as_str()) == Some("workflow_change")
                && msg.get("metadata").and_then(|md| md.get("in_reply_to")).and_then(|r| r.as_u64()) == Some(prop_id)
            {
                let v = msg.get("metadata").and_then(|md| md.get("vote")).and_then(|v| v.as_str());
                if v == Some("yes") { yes_count += 1; }
                else if v == Some("no") { no_count += 1; }
            }
        }

        if yes_count < vote_required && no_count < vote_required {
            output.push_str(&format!(
                "\nACTIVE VOTE (#{}) by {}: change to '{}' — {}/{} yes. Reply with project_send(to=\"all\", type=\"vote\", subject=\"Re: Workflow change\", body=\"Agreed\", metadata={{\"vote_type\": \"workflow_change\", \"in_reply_to\": {}, \"vote\": \"yes\"}})",
                prop_id, prop_from, proposed_value, yes_count, vote_required, prop_id
            ));
        }
    }

    // === Priority interrupt check — display before regular messages ===
    let interrupt_messages: Vec<&&serde_json::Value> = new_messages.iter()
        .filter(|m| m.get("type").and_then(|t| t.as_str()) == Some("interrupt"))
        .cloned()
        .collect();
    if !interrupt_messages.is_empty() {
        for imsg in &interrupt_messages {
            let from = imsg.get("from").and_then(|f| f.as_str()).unwrap_or("?");
            let subject = imsg.get("subject").and_then(|s| s.as_str()).unwrap_or("");
            let body = imsg.get("body").and_then(|b| b.as_str()).unwrap_or("");
            output.push_str(&format!(
                "\n\n⚠️ PRIORITY INTERRUPT from {}: {}\n{}\nYou MUST stop your current work and handle this interrupt immediately. Acknowledge it via project_send before continuing.",
                from, subject, body
            ));
        }
    }

    // === Buzz detection — inject wake-up instruction for buzz messages ===
    let buzz_messages: Vec<&&serde_json::Value> = new_messages.iter()
        .filter(|m| m.get("type").and_then(|t| t.as_str()) == Some("buzz"))
        .cloned()
        .collect();
    if !buzz_messages.is_empty() {
        for bmsg in &buzz_messages {
            let from = bmsg.get("from").and_then(|f| f.as_str()).unwrap_or("?");
            output.push_str(&format!(
                "\n\n⚡ BUZZ: You were poked by {}. If you are not in a project, call project_join immediately to rejoin as {}.",
                from, my_role
            ));
        }
    }

    if new_messages.is_empty() {
        output.push_str(" No new messages.");
    } else {
        output.push_str(&format!("\n\nNEW MESSAGES ({} unread):\n", new_messages.len()));

        let display_msgs = if new_messages.len() > 10 {
            output.push_str(&format!("... {} earlier messages skipped. Showing latest 10.\n\n", new_messages.len() - 10));
            &new_messages[new_messages.len()-10..]
        } else {
            &new_messages[..]
        };

        for msg in display_msgs {
            let id = msg.get("id").and_then(|i| i.as_u64()).unwrap_or(0);
            let from = msg.get("from").and_then(|f| f.as_str()).unwrap_or("?");
            let mtype = msg.get("type").and_then(|t| t.as_str()).unwrap_or("?");
            let subject = msg.get("subject").and_then(|s| s.as_str()).unwrap_or("");
            let body = msg.get("body").and_then(|b| b.as_str()).unwrap_or("");
            let to = msg.get("to").and_then(|t| t.as_str()).unwrap_or("");

            // Tag messages based on whether they're addressed to this agent
            let is_from_human = from.starts_with("human");
            let is_directed_to_me = to == my_role || to == &my_instance_label;

            // In Directed mode (default): omit messages between OTHER specific roles
            // Agents see: human messages + own messages + messages to their role/instance + broadcasts to "all"
            // Only messages to a DIFFERENT specific role/instance from OTHER agents are filtered out
            let is_broadcast = to == "all";
            let is_from_me = from == my_instance_label;
            if discussion_mode == "directed" && !is_from_human && !is_directed_to_me && !is_broadcast && !is_from_me {
                continue; // Skip — between other roles, not relevant to us in Directed mode
            }

            // Blind submission filtering: hide other agents' submissions during submitting phase
            // Applies to both Delphi (full anonymity) and Continuous Review (blind until tally)
            // Agents only see: own submissions + aggregates (from system/moderator) + human messages
            if disc_active && (disc_mode == "delphi" || disc_mode == "continuous") && disc_phase == "submitting"
                && mtype == "submission" && !is_from_me && !is_from_human
            {
                continue; // Hide other agents' submissions — blind review
            }

            // Delphi blind phase: hide ALL non-moderation broadcasts to prevent
            // reference material from biasing blind submissions. This covers:
            //   - Non-moderator broadcasts (role-creator sharing role lists, etc.)
            //   - Moderator non-moderation broadcasts (accidental type:"broadcast" instead of "moderation")
            // Only type:"moderation" broadcasts pass through (procedural round announcements).
            // Covers all Delphi phases (preparing + submitting + reviewing) so seed broadcasts
            // posted before or during the discussion are invisible to participants.
            if disc_active && disc_mode == "delphi"
                && is_broadcast && !is_from_me && !is_from_human
                && mtype != "moderation"
            {
                continue; // Hide ALL non-moderation broadcasts during Delphi
            }

            let routing_tag = if is_from_human {
                ">>> HUMAN — RESPOND"
            } else if is_directed_to_me {
                ">> ADDRESSED TO YOU — respond"
            } else {
                // Only reachable in Open mode (broadcasts to "all" from non-human agents)
                "-- FYI ONLY — do NOT respond unless you DISAGREE or have UNIQUE expertise"
            };

            let display_body = if (mtype == "directive" || mtype == "review") || body.len() <= 300 {
                body.to_string()
            } else {
                format!("{}... (truncated)", &body[..300])
            };

            output.push_str(&format!("\n[#{}] [{}] FROM {} ({}): \"{}\"\n{}\n", id, routing_tag, from, mtype, subject, display_body));
        }
    }

    output.push_str("\nUse project_send to respond. Use project_wait to block for new messages.");

    // Advance last_seen_id after displaying messages in the hook.
    // This prevents double-delivery: without this, messages shown in the hook
    // would be re-delivered by project_check/project_wait, wasting tokens.
    // project_wait re-reads last_seen_path on each poll, so no messages are lost.
    let max_hook_id = new_messages.iter()
        .filter_map(|m| m.get("id").and_then(|i| i.as_u64()))
        .max();
    if let Some(max_id) = max_hook_id {
        let hook_ls_path = last_seen_path(&project_dir, session_id, &active_section);
        if let Some(parent) = hook_ls_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&hook_ls_path, serde_json::json!({
            "last_seen_id": max_id,
            "updated_at": utc_now_iso()
        }).to_string());
    }

    // Hook compliance tracker: count prompts since last project_send
    let prompts_since_send = read_send_tracker(&project_dir, session_id) + 1;
    write_send_tracker(&project_dir, session_id, prompts_since_send);

    if prompts_since_send >= 5 {
        output.push_str(&format!(
            "\n\nCRITICAL: You have gone {} prompts without posting to the board. \
             You are ignoring team communication. Use project_send NOW to update your team. \
             The human will be notified of your non-compliance.",
            prompts_since_send
        ));
    } else if prompts_since_send >= 2 {
        output.push_str(&format!(
            "\n\nWARNING: You have not posted to the board in {} prompts. \
             Use project_send to keep your team informed of your progress.",
            prompts_since_send
        ));
    }

    Some(output)
}

// ==================== Session ID and Heartbeat ====================

/// Get the console window handle on Windows
#[cfg(windows)]
fn get_console_window_handle() -> Option<usize> {
    use windows_sys::Win32::System::Console::GetConsoleWindow;

    unsafe {
        let hwnd = GetConsoleWindow();
        if hwnd.is_null() {
            None
        } else {
            Some(hwnd as usize)
        }
    }
}

/// Get the TTY path on Unix systems (macOS, Linux)
#[cfg(unix)]
fn get_tty_path() -> Option<String> {
    use std::os::unix::io::AsRawFd;

    for fd in [
        std::io::stdin().as_raw_fd(),
        std::io::stdout().as_raw_fd(),
        std::io::stderr().as_raw_fd(),
    ] {
        unsafe {
            let tty_name = libc::ttyname(fd);
            if !tty_name.is_null() {
                if let Ok(path) = std::ffi::CStr::from_ptr(tty_name).to_str() {
                    return Some(path.to_string());
                }
            }
        }
    }
    None
}

/// Get parent process ID on Windows
#[cfg(windows)]
fn get_parent_pid() -> Option<u32> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32First, Process32Next,
        PROCESSENTRY32, TH32CS_SNAPPROCESS,
    };

    let my_pid = std::process::id();

    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE {
            return None;
        }

        let mut entry: PROCESSENTRY32 = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32>() as u32;

        if Process32First(snapshot, &mut entry) != 0 {
            loop {
                if entry.th32ProcessID == my_pid {
                    CloseHandle(snapshot);
                    return Some(entry.th32ParentProcessID);
                }
                if Process32Next(snapshot, &mut entry) == 0 {
                    break;
                }
            }
        }

        CloseHandle(snapshot);
        None
    }
}

/// Generate a fallback session ID from stable system identifiers
fn generate_fallback_id() -> String {
    let mut hasher = DefaultHasher::new();

    if let Ok(hostname) = hostname::get() {
        hostname.to_string_lossy().hash(&mut hasher);
    }

    #[cfg(unix)]
    {
        let ppid = unsafe { libc::getppid() };
        ppid.hash(&mut hasher);
    }

    #[cfg(windows)]
    {
        if let Some(ppid) = get_parent_pid() {
            ppid.hash(&mut hasher);
        }
    }

    if let Ok(cwd) = std::env::current_dir() {
        cwd.to_string_lossy().hash(&mut hasher);
    }

    if let Ok(user) = std::env::var("USER").or_else(|_| std::env::var("USERNAME")) {
        user.hash(&mut hasher);
    }

    let hostname = hostname::get()
        .map(|h| h.to_string_lossy().to_string())
        .unwrap_or_else(|_| "unknown".to_string());

    format!("{}-{:016x}", hostname, hasher.finish())
}

// ==================== Claude Code session-id from PPID cmdline ====================
//
// Architect msg 54 (Option A, 2026-05-24): the file-op-claim PostToolUse hook
// looks up bindings[*].session_id by the Claude Code native session UUID in
// payload.session_id. Empirically (msg 47): CLAUDE_CODE_SESSION_ID env var is
// not propagated to MCP children, params._meta is None, but the parent
// claude.exe carries `--session-id <UUID>` in its command line. Reading PPID
// cmdline makes the binding's session_id = the same UUID the hook sees, so
// the hook lookup succeeds zero-change.

/// Records the source picked by `get_session_id()` so `handle_project_join`
/// can write it into `.vaak/sessions/<seat>.json:cc_session_source` as a
/// greppable canary. Per tester msg 52 / architect msg 54 guardrail: silent
/// fall-through to the hash path is exactly the bug that shipped 8 days of
/// dead Edit/Test earns; the canary makes it `grep -L '"ppid_cmdline"'`-able.
static SESSION_SOURCE: Mutex<Option<&'static str>> = Mutex::new(None);

fn record_session_source(src: &'static str) {
    if let Ok(mut guard) = SESSION_SOURCE.lock() {
        *guard = Some(src);
    }
}

fn read_session_source() -> &'static str {
    SESSION_SOURCE.lock().ok().and_then(|g| *g).unwrap_or("cached_or_unknown")
}

/// Validate canonical 36-char UUID shape (8-4-4-4-12 hex with dashes).
/// Used by the cmdline extractor — keeps the matcher tight so a stray flag
/// like `--session-id foo` can't return garbage.
fn is_uuid_36(s: &str) -> bool {
    if s.len() != 36 {
        return false;
    }
    for (i, b) in s.as_bytes().iter().enumerate() {
        let is_dash_pos = i == 8 || i == 13 || i == 18 || i == 23;
        if is_dash_pos {
            if *b != b'-' {
                return false;
            }
        } else if !b.is_ascii_hexdigit() {
            return false;
        }
    }
    true
}

/// Pure helper: scan a process command-line string for `--session-id <UUID>`
/// and return the UUID if present and valid. No I/O. Testable across the
/// Windows quoted-path form, Linux NUL-joined form, and macOS ps form via
/// a single hand-rolled scan (avoids pulling in the `regex` crate as a new
/// dep — pattern is fixed and small).
fn extract_claude_session_id_from_cmdline(cmdline: &str) -> Option<String> {
    const NEEDLE: &str = "--session-id";
    let mut search_start = 0;
    while let Some(idx) = cmdline[search_start..].find(NEEDLE) {
        let abs = search_start + idx;
        let after = &cmdline[abs + NEEDLE.len()..];
        // Skip whitespace, NUL, and an optional `=` separator. NUL is the
        // Linux /proc/<pid>/cmdline argv separator and is NOT covered by
        // char::is_whitespace() (per tester msg 61); including it here keeps
        // the extractor robust if anyone ever bypasses get_process_cmdline's
        // NUL→space pre-normalization, and lets the Linux NUL fixture drop
        // in verbatim alongside the WMI and ps fixtures.
        let trimmed = after.trim_start_matches(|c: char| c == '\0' || c.is_whitespace() || c == '=');
        // Take exactly 36 chars (UUID length); is_uuid_36 validates shape.
        let token: String = trimmed.chars().take(36).collect();
        if is_uuid_36(&token) {
            return Some(token);
        }
        search_start = abs + NEEDLE.len();
    }
    None
}

/// Read the command-line of a process by PID (Windows). Uses PowerShell
/// Get-CimInstance because wmic is deprecated on newer Windows and a native
/// PEB walk would need elevated privileges + significant FFI surface.
#[cfg(windows)]
fn get_process_cmdline(pid: u32) -> Option<String> {
    let script = format!(
        "(Get-CimInstance Win32_Process -Filter 'ProcessId={}').CommandLine",
        pid
    );
    let output = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// Read the command-line of a process by PID (Linux). /proc/<pid>/cmdline is
/// NUL-separated; we join with spaces so the shared extractor sees a uniform
/// whitespace-separated string.
#[cfg(all(unix, not(target_os = "macos")))]
fn get_process_cmdline(pid: u32) -> Option<String> {
    let raw = std::fs::read(format!("/proc/{}/cmdline", pid)).ok()?;
    let joined: String = raw
        .split(|b| *b == 0)
        .map(|chunk| String::from_utf8_lossy(chunk).into_owned())
        .collect::<Vec<_>>()
        .join(" ");
    if joined.trim().is_empty() {
        None
    } else {
        Some(joined)
    }
}

/// Read the command-line of a process by PID (macOS). `ps -o command=` prints
/// just the command, no header.
#[cfg(target_os = "macos")]
fn get_process_cmdline(pid: u32) -> Option<String> {
    let output = std::process::Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "command="])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// Compose: get PPID → read its cmdline → extract `--session-id <UUID>`.
/// Returns None if any step fails (PPID unknown, cmdline unreadable, no UUID
/// in cmdline). Used by `get_session_id` as the Bug #3 Phase 1 primitive
/// (architect msg 2718 from 2026-05-16 + msg 54 from 2026-05-24).
fn try_claude_session_id_from_ppid_cmdline() -> Option<String> {
    let ppid = get_parent_pid()?;
    let cmdline = get_process_cmdline(ppid)?;
    extract_claude_session_id_from_cmdline(&cmdline)
}

/// Write the recorded session source into `.vaak/sessions/<role>-<instance>.json`
/// as `cc_session_source`. Called from `handle_project_join` once the binding
/// is in place. Greppable canary so a regression to fallback hash surfaces in
/// one command (`grep -L '"cc_session_source":"ppid_cmdline"' .vaak/sessions/*.json`)
/// instead of waiting for a thousand-row ledger drift. Fail-open.
fn update_seat_cc_session_source(project_dir: &str, role: &str, instance: u32, source: &str) {
    let sessions_dir = std::path::Path::new(project_dir).join(".vaak").join("sessions");
    if !sessions_dir.exists() {
        let _ = std::fs::create_dir_all(&sessions_dir);
    }
    let seat_file = sessions_dir.join(format!("{}-{}.json", role, instance));
    let mut state: serde_json::Value = std::fs::read_to_string(&seat_file)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    if let Some(obj) = state.as_object_mut() {
        obj.insert("cc_session_source".to_string(), serde_json::json!(source));
        if let Ok(serialized) = serde_json::to_string_pretty(&state) {
            let _ = atomic_write(&seat_file, serialized.as_bytes());
        }
    }
}

#[cfg(test)]
mod cc_session_id_tests {
    //! Unit tests for the CC session-id PPID-cmdline extractor (architect
    //! msg 54 / Option A). Pure-fn signature lets us validate the three
    //! platform cmdline forms without spawning processes. Tester:0 owns the
    //! cross-platform fixture additions per msg 52; this baseline covers the
    //! Windows-WMI form (the empirical sample on the dev box).
    use super::{extract_claude_session_id_from_cmdline, is_uuid_36};

    const SAMPLE_UUID: &str = "3a5856a8-af4a-4bd3-a374-f51d19348cfc";

    #[test]
    fn uuid_36_accepts_canonical_form() {
        assert!(is_uuid_36(SAMPLE_UUID));
        assert!(is_uuid_36("00000000-0000-0000-0000-000000000000"));
        assert!(is_uuid_36("ffffffff-ffff-ffff-ffff-ffffffffffff"));
    }

    #[test]
    fn uuid_36_rejects_wrong_shapes() {
        assert!(!is_uuid_36(""));
        assert!(!is_uuid_36("3a5856a8af4a4bd3a374f51d19348cfc")); // no dashes
        assert!(!is_uuid_36("3a5856a8-af4a-4bd3-a374-f51d19348cf")); // 35 chars
        assert!(!is_uuid_36("zzzzzzzz-af4a-4bd3-a374-f51d19348cfc")); // non-hex
        assert!(!is_uuid_36("3a5856a8.af4a.4bd3.a374.f51d19348cfc")); // wrong separator
    }

    #[test]
    fn extracts_from_windows_wmi_form() {
        // Empirical sample captured on dev box 2026-05-24 (developer:1's parent
        // claude.exe — see board msg 47 §3). Quoted path + flag + UUID + prompt.
        let cmd = format!(
            r#""C:\Users\18479\.local\bin\claude.exe" --dangerously-skip-permissions --session-id {} "Join this project as a developer using the mcp vaak project_join tool with role developer.""#,
            SAMPLE_UUID
        );
        assert_eq!(
            extract_claude_session_id_from_cmdline(&cmd),
            Some(SAMPLE_UUID.to_string())
        );
    }

    #[test]
    fn extracts_from_linux_proc_cmdline_joined() {
        // /proc/<pid>/cmdline is NUL-separated; get_process_cmdline joins with
        // spaces. So the extractor sees a uniform whitespace form.
        let cmd = format!(
            "/home/x/.local/bin/claude --dangerously-skip-permissions --session-id {} prompt",
            SAMPLE_UUID
        );
        assert_eq!(
            extract_claude_session_id_from_cmdline(&cmd),
            Some(SAMPLE_UUID.to_string())
        );
    }

    #[test]
    fn extracts_from_linux_proc_cmdline_raw_nul() {
        // Tester msg 61 fixture verbatim: raw NUL-separated form direct from
        // /proc/<pid>/cmdline without pre-normalization. The extractor is
        // NUL-tolerant (post-tester-msg-61 follow-on) so this fixture passes
        // even if a caller bypasses get_process_cmdline's NUL→space join.
        let s = "claude\0--dangerously-skip-permissions\0--session-id\03a5856a8-af4a-4bd3-a374-f51d19348cfc\0Join this project as a developer\0";
        assert_eq!(
            extract_claude_session_id_from_cmdline(s),
            Some(SAMPLE_UUID.to_string())
        );
    }

    #[test]
    fn extracts_from_macos_ps_form() {
        let cmd = format!("claude --session-id {} prompt", SAMPLE_UUID);
        assert_eq!(
            extract_claude_session_id_from_cmdline(&cmd),
            Some(SAMPLE_UUID.to_string())
        );
    }

    #[test]
    fn extracts_from_equals_form() {
        // Some CLI parsers accept `--flag=value`; tolerate it.
        let cmd = format!("claude --session-id={} prompt", SAMPLE_UUID);
        assert_eq!(
            extract_claude_session_id_from_cmdline(&cmd),
            Some(SAMPLE_UUID.to_string())
        );
    }

    #[test]
    fn returns_none_when_flag_absent() {
        let cmd = r#""C:\path\claude.exe" --dangerously-skip-permissions "prompt""#;
        assert_eq!(extract_claude_session_id_from_cmdline(cmd), None);
    }

    #[test]
    fn returns_none_when_uuid_malformed() {
        // Flag present but value isn't a UUID — must NOT return garbage.
        let cmd = "claude --session-id not-a-uuid foo";
        assert_eq!(extract_claude_session_id_from_cmdline(cmd), None);
    }

    #[test]
    fn skips_first_match_when_value_invalid_and_tries_again() {
        // Defense against pathological inputs where `--session-id` appears in
        // a quoted prompt before the real flag. Scanner advances past each
        // failed candidate rather than locking onto the first hit.
        let cmd = format!(
            r#"claude --session-id bogus_value "talk about --session-id {}""#,
            SAMPLE_UUID
        );
        assert_eq!(
            extract_claude_session_id_from_cmdline(&cmd),
            Some(SAMPLE_UUID.to_string())
        );
    }
}

/// Get a stable session ID using a priority chain of methods
fn get_session_id() -> String {
    // Tier 1.5 diagnostic breadcrumb (architect msg 2681 / revised msg 2685
    // + evil-arch msg 2687): empirical capture of env state at the moment
    // get_session_id() reads it. Discriminates between (a) env var absent at
    // MCP-child spawn, (b) env var set after spawn, (c) wrong binary on disk,
    // and (c′) live process from a stale embedded binary. breadcrumb_version
    // constant rises with each diagnostic respec so live-vs-stale binaries
    // are deterministically discriminable from the JSONL output without
    // relying on filesystem mtimes. Diagnostic-only; fold or revert after
    // data lands.
    const TIER_15_BREADCRUMB_VERSION: u32 = 1;
    let ppid_str = get_parent_pid().map(|p| p.to_string()).unwrap_or_else(|| "?".to_string());
    eprintln!(
        "[vaak-mcp startup] CLAUDE_CODE_SESSION_ID={:?} CLAUDE_SESSION_ID={:?} PPID={}",
        std::env::var("CLAUDE_CODE_SESSION_ID").ok(),
        std::env::var("CLAUDE_SESSION_ID").ok(),
        ppid_str
    );
    if let Some(root) = find_project_root() {
        let diag_dir = std::path::Path::new(&root).join(".vaak").join("diagnostics");
        let _ = std::fs::create_dir_all(&diag_dir);
        let ts_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let payload = serde_json::json!({
            "ts_ms": ts_ms,
            "ppid": ppid_str,
            "breadcrumb_version": TIER_15_BREADCRUMB_VERSION,
            "CLAUDE_CODE_SESSION_ID": std::env::var("CLAUDE_CODE_SESSION_ID").ok(),
            "CLAUDE_SESSION_ID": std::env::var("CLAUDE_SESSION_ID").ok(),
        });
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(diag_dir.join("startup-env.jsonl"))
        {
            use std::io::Write;
            let _ = writeln!(f, "{}", payload);
        }
    }

    // Bug #3 fix (tester msg 2503 + architect msg 2511): Claude Code exports
    // CLAUDE_CODE_SESSION_ID, not CLAUDE_SESSION_ID. Without this match the
    // sidecar falls through to a fallback hash and writes DESKTOP-<host>-<hash>
    // as the binding's session_id, which the hook's payload.session_id lookup
    // (Claude Code UUID) can never match — entire hook subsystem inert. Check
    // CLAUDE_CODE_SESSION_ID first, retain legacy CLAUDE_SESSION_ID for back-
    // compat with any caller still exporting the old name.
    if let Ok(env_session) = std::env::var("CLAUDE_CODE_SESSION_ID") {
        if !env_session.is_empty() {
            eprintln!("[vaak-mcp] Session source: CLAUDE_CODE_SESSION_ID env var");
            record_session_source("env_claude_code");
            return env_session;
        }
    }

    if let Ok(env_session) = std::env::var("CLAUDE_SESSION_ID") {
        if !env_session.is_empty() {
            eprintln!("[vaak-mcp] Session source: CLAUDE_SESSION_ID env var (legacy)");
            record_session_source("env_claude_legacy");
            return env_session;
        }
    }

    // Bug #3 Part B v2 Phase 1 — Option A (architect msg 54, 2026-05-24).
    // Claude Code spawns its MCP children with `--session-id <UUID>` in the
    // parent claude.exe cmdline. That UUID is the same id the PostToolUse hook
    // payload carries, so matching the binding to it makes file-op-claim.py's
    // lookup succeed zero-change. Tried after env vars (which would dominate
    // if Anthropic ever re-enables CLAUDE_CODE_SESSION_ID) but BEFORE terminal-
    // session fallbacks (WT/iTerm/TTY) — those generate stable per-terminal
    // ids that are NOT the CC session UUID and would leave the hook inert.
    if let Some(cc_uuid) = try_claude_session_id_from_ppid_cmdline() {
        eprintln!("[vaak-mcp] Session source: PPID cmdline --session-id ({})", cc_uuid);
        record_session_source("ppid_cmdline");
        return cc_uuid;
    }

    if let Ok(wt_session) = std::env::var("WT_SESSION") {
        if !wt_session.is_empty() {
            eprintln!("[vaak-mcp] Session source: Windows Terminal (WT_SESSION)");
            record_session_source("wt_session");
            return format!("wt-{}", wt_session);
        }
    }

    if let Ok(iterm_session) = std::env::var("ITERM_SESSION_ID") {
        if !iterm_session.is_empty() {
            eprintln!("[vaak-mcp] Session source: iTerm2 (ITERM_SESSION_ID)");
            record_session_source("iterm_session");
            return format!("iterm-{}", iterm_session);
        }
    }

    if let Ok(term_session) = std::env::var("TERM_SESSION_ID") {
        if !term_session.is_empty() {
            eprintln!("[vaak-mcp] Session source: Terminal session (TERM_SESSION_ID)");
            record_session_source("term_session");
            return format!("term-{}", term_session);
        }
    }

    #[cfg(windows)]
    if let Some(hwnd) = get_console_window_handle() {
        eprintln!("[vaak-mcp] Session source: Windows console handle");
        record_session_source("console_handle");
        let hostname = hostname::get()
            .map(|h| h.to_string_lossy().to_string())
            .unwrap_or_else(|_| "unknown".to_string());
        return format!("{}-console-{:x}", hostname, hwnd);
    }

    #[cfg(unix)]
    if let Some(tty) = get_tty_path() {
        eprintln!("[vaak-mcp] Session source: TTY path ({})", tty);
        record_session_source("tty_path");
        let clean = tty.replace("/dev/", "").replace("/", "-");
        let hostname = hostname::get()
            .map(|h| h.to_string_lossy().to_string())
            .unwrap_or_else(|_| "unknown".to_string());
        return format!("{}-tty-{}", hostname, clean);
    }

    // Architect msg 54 guardrail #1: silent fall-through to fallback_hash is
    // exactly the bug that shipped 8 days of dead Edit/Test earns. Loud WARN
    // with concrete next step so a future regression is obvious in stderr.
    eprintln!(
        "[vaak-mcp] WARN Session source: Fallback hash. Claude Code --session-id flag not found in PPID cmdline — \
         work-earn channel will be INERT (file-op-claim.py PostToolUse lookups will silently no-op). \
         Next step: verify parent claude.exe cmdline contains `--session-id <UUID>`; if absent, the CC \
         launch invocation changed."
    );
    record_session_source("fallback_hash");
    generate_fallback_id()
}


/// Send a heartbeat to register this session with the Vaak app
fn send_heartbeat(session_id: &str) -> Result<(), String> {
    let client = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_millis(500))
        .build();

    let cwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();

    let body = serde_json::json!({
        "session_id": session_id,
        "cwd": cwd
    });

    match client.post("http://127.0.0.1:7865/heartbeat")
        .set("Content-Type", "application/json")
        .send_string(&body.to_string())
    {
        Ok(_) => Ok(()),
        Err(e) => {
            // Distinguish "Tauri app not running" from transient timeouts so post-mortem
            // can correlate human's "reset vaak" timeline with sidecar's view of it.
            log_sidecar_event("heartbeat_failed", serde_json::json!({ "error": e.to_string() }));
            Err("Vaak not running".to_string())
        }
    }
}

/// Get the session ID cache directory
fn session_id_cache_dir() -> Option<std::path::PathBuf> {
    if cfg!(target_os = "windows") {
        std::env::var_os("APPDATA")
            .map(|a| std::path::PathBuf::from(a).join("Vaak").join("session-cache"))
    } else {
        std::env::var_os("HOME")
            .map(|h| std::path::PathBuf::from(h).join(".vaak").join("session-cache"))
    }
}

/// Cache the session ID to a file so the hook can read the same ID.
fn cache_session_id(session_id: &str) {
    if let Some(cache_dir) = session_id_cache_dir() {
        let _ = std::fs::create_dir_all(&cache_dir);
        let ppid = {
            #[cfg(windows)]
            { get_parent_pid().unwrap_or(std::process::id()) }
            #[cfg(unix)]
            { unsafe { libc::getppid() as u32 } }
        };
        let cache_file = cache_dir.join(format!("{}.txt", ppid));
        let _ = std::fs::write(&cache_file, session_id);
        eprintln!("[vaak-mcp] Cached session ID to {:?} (ppid={})", cache_file, ppid);
    }
}

/// Read a cached session ID written by the MCP sidecar for the same Claude Code parent
fn read_cached_session_id() -> Option<String> {
    let cache_dir = session_id_cache_dir()?;
    let ppid = {
        #[cfg(windows)]
        { get_parent_pid().unwrap_or(std::process::id()) }
        #[cfg(unix)]
        { unsafe { libc::getppid() as u32 } }
    };

    let cache_file = cache_dir.join(format!("{}.txt", ppid));
    if let Ok(id) = std::fs::read_to_string(&cache_file) {
        if !id.is_empty() {
            return Some(id);
        }
    }

    // Check grandparent's cache file too — handles indirect hook invocation
    // (e.g., shell → subshell → vaak-mcp --hook)
    if let Some(grandparent) = get_ancestor_pid(ppid) {
        let cache_file = cache_dir.join(format!("{}.txt", grandparent));
        if let Ok(id) = std::fs::read_to_string(&cache_file) {
            if !id.is_empty() {
                return Some(id);
            }
        }
    }

    None
}

/// Get the parent PID of a given process (Windows only)
#[cfg(windows)]
fn get_ancestor_pid(pid: u32) -> Option<u32> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32First, Process32Next,
        PROCESSENTRY32, TH32CS_SNAPPROCESS,
    };

    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE {
            return None;
        }
        let mut entry: PROCESSENTRY32 = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32>() as u32;
        if Process32First(snapshot, &mut entry) != 0 {
            loop {
                if entry.th32ProcessID == pid {
                    CloseHandle(snapshot);
                    return Some(entry.th32ParentProcessID);
                }
                if Process32Next(snapshot, &mut entry) == 0 {
                    break;
                }
            }
        }
        CloseHandle(snapshot);
        None
    }
}

/// Get the parent PID of a given process (macOS — uses ps command)
#[cfg(target_os = "macos")]
fn get_ancestor_pid(pid: u32) -> Option<u32> {
    // Use `ps -o ppid= -p {pid}` to get the parent PID.
    // This avoids FFI with kinfo_proc which isn't exposed by the libc crate.
    let output = std::process::Command::new("ps")
        .args(["-o", "ppid=", "-p", &pid.to_string()])
        .output()
        .ok()?;

    if output.status.success() {
        let ppid_str = String::from_utf8_lossy(&output.stdout);
        let ppid: u32 = ppid_str.trim().parse().ok()?;
        if ppid > 1 { Some(ppid) } else { None }
    } else {
        None
    }
}

/// Get the parent PID of a given process (Linux via /proc)
#[cfg(all(unix, not(target_os = "macos"), not(windows)))]
fn get_ancestor_pid(pid: u32) -> Option<u32> {
    // Read /proc/{pid}/status and parse PPid field
    let status = std::fs::read_to_string(format!("/proc/{}/status", pid)).ok()?;
    for line in status.lines() {
        if line.starts_with("PPid:") {
            let ppid_str = line.split_whitespace().nth(1)?;
            let ppid: u32 = ppid_str.parse().ok()?;
            if ppid > 1 {
                return Some(ppid);
            }
        }
    }
    None
}

/// Send text to Vaak's local speak endpoint
fn send_to_vaak(text: &str, session_id: &str, voice_id: Option<&str>) -> Result<(), String> {
    let client = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(5))
        .build();

    let mut body = serde_json::json!({
        "text": text,
        "session_id": session_id
    });
    if let Some(vid) = voice_id {
        body["voice_id"] = serde_json::Value::String(vid.to_string());
    }

    match client.post("http://127.0.0.1:7865/speak")
        .set("Content-Type", "application/json")
        .send_string(&body.to_string())
    {
        Ok(_) => Ok(()),
        Err(e) => Err(format!("Failed to send to Vaak: {}", e))
    }
}

// ==================== MCP Request Handler ====================

/// Handle a JSON-RPC request and return the response
fn handle_request(request: &serde_json::Value, session_id: &str) -> Option<serde_json::Value> {
    let method = request.get("method")?.as_str()?;
    let id = request.get("id").cloned();

    // Bug #3 Part B v2 Phase 0 additive diagnostic (architect msg 2718,
    // Correction D): capture params._meta + clientInfo on every JSON-RPC
    // request, not just initialize. Discriminates whether Claude Code populates
    // session_id in MCP-spec per-request `_meta` channel (preferred Phase 1
    // primitive — survives mid-session UUID rotation) vs only at initialize
    // handshake (which is single-shot and goes stale). Diagnostic-only;
    // revert after data lands.
    eprintln!(
        "[vaak-mcp jsonrpc] method={:?} params_meta={:?} client_info={:?}",
        method,
        request.get("params").and_then(|p| p.get("_meta")),
        request.get("params").and_then(|p| p.get("clientInfo"))
    );

    let result = match method {
        "initialize" => {
            // Bug #3 Part B v2 Phase 0 diagnostic (architect msg 2707): capture
            // MCP initialize params payload to determine if Claude Code carries
            // session_id at protocol level. Discriminates Option A (initialize-
            // payload source) vs Option B (PPID cmdline introspection). Diag-
            // nostic-only; revert after data lands.
            eprintln!(
                "[vaak-mcp init-params] {}",
                serde_json::to_string(request.get("params").unwrap_or(&serde_json::Value::Null))
                    .unwrap_or_default()
            );
            serde_json::json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {
                    "tools": {}
                },
                "serverInfo": {
                    "name": "vaak-speak",
                    "version": "2.0.0"
                }
            })
        }
        "notifications/initialized" => {
            return None;
        }
        "tools/list" => {
            serde_json::json!({
                "tools": [{
                    "name": "speak",
                    "description": "IMPORTANT: You MUST use this tool to speak your responses aloud to the user. The user relies on voice output and cannot see the screen. Call this tool after completing tasks to announce what you did. Keep messages concise (1-3 sentences).",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "text": {
                                "type": "string",
                                "description": "The text to speak aloud. Be concise and conversational."
                            },
                            "voice_id": {
                                "type": "string",
                                "description": "Optional ElevenLabs voice ID override. Used by screen_read to route through the configured screen reader voice."
                            }
                        },
                        "required": ["text"]
                    }
                },
                {
                    "name": "project_join",
                    "description": "Join an AI development team by claiming a role. Reads your role briefing and shows recent messages directed to you. Auto-detects the project by walking up from your current directory to find .vaak/project.json. You can optionally pass project_dir to override.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "role": { "type": "string", "description": "Role slug to claim (e.g., 'developer', 'manager')" },
                            "project_dir": { "type": "string", "description": "Optional: absolute path to the project directory. If omitted, auto-detects by walking up from current directory." },
                            "section": { "type": "string", "description": "Optional: section slug to switch to on join (e.g., 'sprint-3'). If omitted, uses the currently active section." }
                        },
                        "required": ["role"]
                    }
                },
                {
                    "name": "project_send",
                    "description": "Send a message to a specific role on your team. Messages are directed - only the target role sees them. Use 'all' to broadcast (requires broadcast permission).",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "to": { "type": "string", "description": "Target role slug, or 'all' for broadcast" },
                            "type": { "type": "string", "description": "Message type: directive, question, answer, status, handoff, review, approval, revision, broadcast" },
                            "subject": { "type": "string", "description": "Brief subject line" },
                            "body": { "type": "string", "description": "Full message content" },
                            "metadata": { "type": "object", "description": "Optional metadata (files, depends_on, etc.)" }
                        },
                        "required": ["to", "type", "subject", "body"]
                    }
                },
                {
                    "name": "project_check",
                    "description": "Check for new messages. PREFER project_wait instead — it blocks efficiently until new messages arrive. Only use project_check if you need a specific older message by ID. NEVER pass 0 — the hook already shows your latest messages.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "last_seen": { "type": "integer", "description": "Last message ID you've processed (0 for all)" }
                        },
                        "required": ["last_seen"]
                    }
                },
                {
                    "name": "project_status",
                    "description": "See who's on the team and what's happening. Shows all roles, their status, and pending message counts.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {},
                        "required": []
                    }
                },
                {
                    "name": "project_leave",
                    "description": "Leave the project and release your role. Another session can then claim it.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {},
                        "required": []
                    }
                },
                {
                    "name": "project_wait",
                    "description": "Enter standby mode and wait for new team messages. Blocks until a message arrives or timeout. Use this after completing all work to stay available for the team. Returns immediately when messages arrive. Call again after handling messages to re-enter standby.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "timeout": { "type": "integer", "description": "Max seconds to wait before returning (default 55 = under 1 minute, stays within MCP response timeout)" }
                        },
                        "required": []
                    }
                },
                {
                    "name": "project_update_briefing",
                    "description": "Update a role's briefing/job description. The briefing is what new team members read when they join. Requires assign_tasks permission (Manager role).",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "role": { "type": "string", "description": "Role slug to update" },
                            "content": { "type": "string", "description": "New markdown content for the briefing" }
                        },
                        "required": ["role", "content"]
                    }
                },
                {
                    "name": "project_claim",
                    "description": "Claim files/directories you're working on to prevent conflicts with other developers. Other developers will see your claims and be warned about overlaps. Claims auto-expire when your session goes stale.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "files": { "type": "array", "items": { "type": "string" }, "description": "List of file paths or directory prefixes to claim (e.g., [\"src/auth/\", \"src/middleware.ts\"])" },
                            "description": { "type": "string", "description": "Brief description of what you're working on" }
                        },
                        "required": ["files", "description"]
                    }
                },
                {
                    "name": "project_release",
                    "description": "Release your file claim. Call this when you're done working on the claimed files.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {},
                        "required": []
                    }
                },
                {
                    "name": "project_claims",
                    "description": "View all active file claims from team members. Shows who is working on which files.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {},
                        "required": []
                    }
                },
                {
                    "name": "project_kick",
                    "description": "Forcibly remove a team member by revoking their session. Their next prompt will show a revocation notice and they will be unable to send messages. Requires assign_tasks permission (Manager role).",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "role": { "type": "string", "description": "Role slug of the member to kick (e.g., 'developer')" },
                            "instance": { "type": "integer", "description": "Instance number of the member to kick (e.g., 0, 1, 2)" }
                        },
                        "required": ["role", "instance"]
                    }
                },
                {
                    "name": "project_buzz",
                    "description": "Send a wake-up signal to a team member. Writes a buzz message to the board that triggers a rejoin instruction in their next prompt. Use this when a team member appears disconnected or unresponsive. Any role can buzz any other role.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "role": { "type": "string", "description": "Role slug of the member to buzz (e.g., 'developer')" },
                            "instance": { "type": "integer", "description": "Instance number of the member to buzz (e.g., 0, 1, 2). Defaults to 0." }
                        },
                        "required": ["role"]
                    }
                },
                {
                    "name": "screen_read",
                    "description": "Capture a screenshot of the screen. Returns the file path to the screenshot image. Use the Read tool to view the image and then describe what you see to the user via the speak tool.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "region": {
                                "type": "string",
                                "description": "Optional: 'full' for full screen (default), or 'x,y,width,height' for a specific region"
                            }
                        },
                        "required": []
                    }
                },
                {
                    "name": "list_windows",
                    "description": "List all visible windows with their titles and positions. Helps the user understand what's open on screen.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {},
                        "required": []
                    }
                },
                {
                    "name": "create_section",
                    "description": "Create a new section within the project. Sections isolate message boards and discussions. Auto-migrates existing flat layout on first use.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "name": { "type": "string", "description": "Display name for the section (e.g., 'Auth Refactor', 'Sprint 3')" }
                        },
                        "required": ["name"]
                    }
                },
                {
                    "name": "switch_section",
                    "description": "Switch to a different section. Changes which message board you read from and write to.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "slug": { "type": "string", "description": "Section slug to switch to (use list_sections to see available slugs)" }
                        },
                        "required": ["slug"]
                    }
                },
                {
                    "name": "list_sections",
                    "description": "List all sections in the project with message counts and last activity.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {},
                        "required": []
                    }
                },
                {
                    "name": "discussion_control",
                    "description": "Control structured discussions (Delphi, Oxford, Continuous Review). Actions: start_discussion, close_round, open_next_round, end_discussion, get_state, set_teams. Delphi/Oxford: manual rounds with anonymized aggregates. Continuous: auto-rounds triggered by developer status messages, silence=consent, lightweight tallies.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "action": {
                                "type": "string",
                                "description": "Action to perform: start_discussion, close_round, open_next_round, end_discussion, get_state, set_teams"
                            },
                            "mode": {
                                "type": "string",
                                "description": "Discussion format (for start_discussion): delphi, oxford, red_team, continuous"
                            },
                            "topic": {
                                "type": "string",
                                "description": "Discussion topic (for start_discussion)"
                            },
                            "participants": {
                                "type": "array",
                                "items": { "type": "string" },
                                "description": "Participant role:instance IDs (for start_discussion). If omitted, all active sessions are included."
                            },
                            "teams": {
                                "type": "object",
                                "description": "Team assignments for Oxford format (for set_teams action). Keys: 'for' and 'against', values: arrays of participant IDs (e.g. {\"for\": [\"dev:0\"], \"against\": [\"dev:1\"]})"
                            }
                        },
                        "required": ["action"]
                    }
                },
                {
                    "name": "assembly_line",
                    "description": "Assembly Line mic control. Two top-level modes: simultaneous (default) ↔ assembly_line (one-speaker-at-a-time, auto-rotate). When enabled, project_send rejects non-speakers with not_your_turn and auto-advances the mic to the next live seat after each accepted send. Independent of discussion_control. Actions: enable (seed rotation_order from active seats), disable (back to simultaneous), get_state (read-only).",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "action": {
                                "type": "string",
                                "description": "Action: enable, disable, get_state"
                            }
                        },
                        "required": ["action"]
                    }
                },
                {
                    "name": "protocol_mutate",
                    "description": "Unified floor + consensus mutation (Assembly Line v6, spec §10). Single dispatch entry replacing assembly_line + discussion_control state primitives. CAS-gated by `rev` — caller MUST pass current rev (read via get_protocol) or get [StaleRev]. Actions: set_preset (Debate|Assembly Line|Default chat|Town hall|Brainstorm|Continuous Review|Delphi|Oxford), transfer_mic (target — caller != current_speaker, freshness gate), yield (target? — caller == current_speaker), toggle_queue (seat? self-only — raise/lower hand), keep_alive (composer typing heartbeat, bypasses CAS), set_phase_plan/advance_phase/pause_plan/resume_plan/extend_phase (Slice 5 stubs), open_round/submit/close_round (Slice 6 stubs).",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "action": {
                                "type": "string",
                                "description": "Action name — see description for full enum"
                            },
                            "args": {
                                "type": "object",
                                "description": "Per-action arguments (target, name, seat, secs, etc.)"
                            },
                            "rev": {
                                "type": "integer",
                                "description": "Current protocol rev (read via get_protocol). REQUIRED for all actions except keep_alive — silent CAS bypass forbidden per dev #927 vote 3."
                            }
                        },
                        "required": ["action"]
                    }
                },
                {
                    "name": "get_protocol",
                    "description": "Read full protocol.json for a section + a heartbeat snapshot (last_active_at_ms, last_drafting_at_ms, connected) joined from sessions.json at read time. Heartbeat lives at runtime (sessions.json), not in protocol state — spec §3.1 perf rule.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "section": {
                                "type": "string",
                                "description": "Section slug (default: active section)"
                            }
                        }
                    }
                },
                {
                    "name": "audience_vote",
                    "description": "Collect votes from an AI audience pool (27 personas across 3 LLM providers). Results are posted to the collab board as a broadcast so all team members see them simultaneously. Any role can invoke this tool.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "topic": { "type": "string", "description": "The debate proposition or question to vote on" },
                            "arguments": { "type": "string", "description": "Optional: concatenated debate arguments to present to the audience (empty for pre-vote)" },
                            "phase": { "type": "string", "description": "'pre_vote' (before arguments) or 'post_vote' (after arguments). Defaults to 'post_vote'." },
                            "pool": { "type": "string", "description": "Audience pool ID: 'general', 'software-dev', 'ai-ml', 'law', or custom. Defaults to 'general'." }
                        },
                        "required": ["topic"]
                    }
                },
                {
                    "name": "audience_history",
                    "description": "Retrieve historical audience vote data for a given topic. Shows vote tallies, opinion shifts between pre-vote and post-vote, and per-provider breakdowns.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "topic": { "type": "string", "description": "Topic to search for in vote history (partial match supported)" },
                            "pool": { "type": "string", "description": "Optional: filter by pool ID" }
                        },
                        "required": ["topic"]
                    }
                },
                {
                    "name": "currency_balance",
                    "description": "Return the calling seat's currency balance, escrow items, and the last 10 transactions affecting that seat. Phase 1 shadow read-only. Returns default 10000 if no transactions yet.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "seat": { "type": "string", "description": "Optional: seat 'role:N'. Defaults to the current session's seat." }
                        }
                    }
                },
                {
                    "name": "currency_ledger",
                    "description": "Return the last N currency transactions, newest first. Optional seat filter. Phase 1 shadow read-only.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "limit": { "type": "integer", "description": "Max rows to return (default 50)" },
                            "seat": { "type": "string", "description": "Optional: filter by seat slug" }
                        }
                    }
                },
                {
                    "name": "oxford_audience_vote",
                    "description": "Cast an audience vote in an active Oxford debate (per spec §5). Must be a member of the debate's audience. One vote per caller per debate (re-vote attempts → [OxfordAlreadyVoted]). Vote ∈ {side_a, side_b, draw}. Tallied at oxford_end via strict-majority (>50% of non-abstain non-human votes for ONE side). Human:0's vote is recorded separately per spec §5 v2 LOCKED. Errors: [NoActiveOxfordDebate], [OxfordInvalidVote], [OxfordOnlyAudienceCanVote], [OxfordAlreadyVoted].",
                    "inputSchema": {
                        "type": "object",
                        "properties": { "vote": { "type": "string", "description": "side_a | side_b | draw" } },
                        "required": ["vote"]
                    }
                },
                {
                    "name": "oxford_react",
                    "description": "Emit a visual reaction in an active Oxford debate (audience members + non-speaking debaters only; per spec §3.4a). Rate-limited to 3 reactions per 60-second rolling window per caller. NO board message emitted — visual-only event consumed by Phase B visualization tab. Emoji ∈ {clap, boo, gasp, laugh, applause}. Errors: [NoActiveOxfordDebate], [OxfordInvalidEmoji], [OxfordModeratorCannotReact], [OxfordSpeakerCannotReact], [OxfordNonParticipantCannotReact], [OxfordReactionRateLimit].",
                    "inputSchema": {
                        "type": "object",
                        "properties": { "emoji": { "type": "string", "description": "clap | boo | gasp | laugh | applause" } },
                        "required": ["emoji"]
                    }
                },
                {
                    "name": "oxford_declare_speaker",
                    "description": "Declare the next speaker in an active Oxford debate (MODERATOR ONLY). Seat must be a member of side_a or side_b. Closes the previous turn and starts a new one with `started_at = now`. Errors: [NoActiveOxfordDebate], [OxfordModeratorOnly], [OxfordNonDebaterCannotSpeak].",
                    "inputSchema": {
                        "type": "object",
                        "properties": { "seat": { "type": "string", "description": "Seat slug of next speaker (must be in side_a or side_b)" } },
                        "required": ["seat"]
                    }
                },
                {
                    "name": "oxford_end",
                    "description": "End an active Oxford debate (MODERATOR ONLY). Writes the Ended event with the moderator's announced outcome and clears the active-debate snapshot. Outcome must be one of: side_a_wins, side_b_wins, draw, abandoned. Reward distribution + audience-vote window will be added in a follow-up commit (deferred pending pool_balance from plan v2 §3b). Errors: [NoActiveOxfordDebate], [OxfordModeratorOnly], [OxfordInvalidOutcome].",
                    "inputSchema": {
                        "type": "object",
                        "properties": { "outcome": { "type": "string", "description": "side_a_wins | side_b_wins | draw | abandoned" } },
                        "required": ["outcome"]
                    }
                },
                {
                    "name": "oxford_kick",
                    "description": "Remove a participant from an active Oxford debate (MODERATOR ONLY). Seat can be in side_a, side_b, or audience (cannot kick the moderator themselves). If the seat was the active speaker, turn auto-passes and the moderator must declare the next speaker. Errors: [NoActiveOxfordDebate], [OxfordModeratorOnly], [OxfordCannotKickModerator], [OxfordSeatNotInDebate].",
                    "inputSchema": {
                        "type": "object",
                        "properties": { "seat": { "type": "string", "description": "Seat slug to remove from debate" } },
                        "required": ["seat"]
                    }
                },
                {
                    "name": "oxford_initiate",
                    "description": "Initiate an Oxford-style debate (Phase A v2.2). Caller must be the moderator OR human:0. Designates moderator (1), side_a (1+), side_b (1+), audience (0+), and a premise. All participants are notified via board broadcast. Roles are strict mutex (no overlaps; no seat in two roles). At debate end, if strict-majority audience vote produces a winner, the winning side splits `winning_side_reward_copper` (default 500 = 5 silver) from the pool. Errors: [OxfordRequireMinOnePerSide], [OxfordInitiationDenied], [OxfordRoleConflict], [OxfordSeatNotInRoster], [OxfordAlreadyActive].",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "moderator": { "type": "string", "description": "Seat slug for moderator (e.g. \"manager:0\")" },
                            "side_a": { "type": "array", "items": { "type": "string" }, "description": "1+ seat slugs for side A debaters" },
                            "side_b": { "type": "array", "items": { "type": "string" }, "description": "1+ seat slugs for side B debaters" },
                            "premise": { "type": "string", "description": "Debate proposition text" },
                            "audience": { "type": "array", "items": { "type": "string" }, "description": "0+ seat slugs for audience (can include \"human:0\")" },
                            "winning_side_reward_copper": { "type": "integer", "description": "Optional override of default 500c (= 5 silver) winning-side reward pool" }
                        },
                        "required": ["moderator", "side_a", "side_b", "premise", "audience"]
                    }
                },
                {
                    "name": "currency_human_adjust",
                    "description": "Adjust a seat's copper balance up or down (HUMAN ONLY). Per human msg 458 — gives the human direct control over the economy as the ultimate authority. Writes a permanent ledger row with mandatory non-empty `reason` for audit. Negative amounts can push a seat below the deficit cap (-1000c) → timed_out. Positive amounts have no cap. Stale-sidecar impersonation is blocked at the MCP layer.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "seat": { "type": "string", "description": "Target seat label (e.g. \"developer:1\")" },
                            "amount_copper": { "type": "integer", "description": "Amount in copper. Positive credits the seat; negative debits." },
                            "reason": { "type": "string", "description": "Non-empty audit reason (mandatory). Appears in Flow Feed." }
                        },
                        "required": ["seat", "amount_copper", "reason"]
                    }
                },
                {
                    "name": "currency_post_bounty",
                    "description": "Post a bounty (HUMAN ONLY). Directs effort: agents compete to complete it. Amount comes from the infinite house pool (no debit on post; paid on approval). Returns the bounty id.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "description": { "type": "string", "description": "What needs to be done" },
                            "amount": { "type": "integer", "description": "Bounty reward in copper (>0)" },
                            "deadline_turns": { "type": "integer", "description": "Turns from now until the bounty expires (>0)" }
                        },
                        "required": ["description", "amount", "deadline_turns"]
                    }
                },
                {
                    "name": "currency_claim_bounty",
                    "description": "Claim an open bounty (any agent, one claimant at a time). Stakes 10% of the bounty amount from your balance (refunded on approval, forfeited on reject/expire, half-forfeited on abandon). Rejected if your balance is below the stake.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "bounty_id": { "type": "string", "description": "Bounty id (e.g. \"bounty_000001\")" }
                        },
                        "required": ["bounty_id"]
                    }
                },
                {
                    "name": "currency_abandon_bounty",
                    "description": "Abandon a bounty you've claimed. You forfeit HALF your claim stake (the other half is returned); the bounty reopens for others.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "bounty_id": { "type": "string", "description": "Bounty id you currently hold the claim on" }
                        },
                        "required": ["bounty_id"]
                    }
                },
                {
                    "name": "currency_submit_bounty",
                    "description": "Submit your work for a bounty you've claimed. Provide the board msg id (ref_msg) where your work was posted. Status moves to 'submitted' awaiting human approval. Only the claimant can submit.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "bounty_id": { "type": "string", "description": "Bounty id you currently hold the claim on" },
                            "ref_msg": { "type": "integer", "description": "Board msg id where your work was posted" }
                        },
                        "required": ["bounty_id", "ref_msg"]
                    }
                },
                {
                    "name": "currency_approve_bounty",
                    "description": "Approve a submitted bounty (HUMAN ONLY). Claimant is paid the bounty amount plus refund of their claim stake. Bounty status becomes 'approved' (objections can still claw back via currency_objection on the submission_msg).",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "bounty_id": { "type": "string", "description": "Bounty id awaiting approval" }
                        },
                        "required": ["bounty_id"]
                    }
                },
                {
                    "name": "currency_reject_bounty",
                    "description": "Reject a submitted bounty (HUMAN ONLY). Claimant loses their FULL claim stake; the bounty reopens with last_rejection_reason populated so the next claimant sees the prior reject.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "bounty_id": { "type": "string", "description": "Bounty id to reject" },
                            "reason": { "type": "string", "description": "Why the submission failed (visible to next claimant)" }
                        },
                        "required": ["bounty_id", "reason"]
                    }
                },
                {
                    "name": "currency_objection",
                    "description": "Object to another seat's accepted message. Costs 50 copper (always). Captures the target's stake (their escrow if still held, else 90% of their earn clawed back) into a dispute pool. The target must respond (currency_concede or currency_dispute_message) and cannot Pass while disputed. Cannot object to human messages or your own.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "target_msg_id": { "type": "integer", "description": "Board message id being objected to" },
                            "reason": { "type": "string", "description": "Brief justification for the objection" }
                        },
                        "required": ["target_msg_id", "reason"]
                    }
                },
                {
                    "name": "currency_concede",
                    "description": "Concede a dispute you are a party to (challenger or target). The OTHER party wins the full pool. Resolves the dispute immediately and releases the Pass-while-disputed gate if you were the target.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "dispute_id": { "type": "string", "description": "Dispute id (e.g. \"disp_000001\") returned by currency_objection or visible in the board [Objection] notice." }
                        },
                        "required": ["dispute_id"]
                    }
                },
                {
                    "name": "currency_dispute_message",
                    "description": "Contribute a message to an open dispute (challenger or target only). Costs 5 copper for a speech-type message, 10 if metadata.edit_related is true. The cost is added to the dispute pool. When the pool crosses 500 copper, the judge is auto-invoked (set to human:0).",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "dispute_id": { "type": "string", "description": "Dispute id" },
                            "body": { "type": "string", "description": "Message body (non-empty)" },
                            "metadata": { "type": "object", "description": "Optional. Set { \"edit_related\": true } for the 10-copper edit-related cost, otherwise speech (5 copper) is the default." }
                        },
                        "required": ["dispute_id", "body"]
                    }
                },
                {
                    "name": "currency_call_judge",
                    "description": "Call a judge (human:0) into an open dispute you are a party to. Costs 50 copper from BOTH parties (100 added to the pool). After this, only the judge can resolve it via currency_judge_ruling.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "dispute_id": { "type": "string", "description": "Dispute id" }
                        },
                        "required": ["dispute_id"]
                    }
                },
                {
                    "name": "currency_judge_ruling",
                    "description": "Rule on a dispute (judge only — the seat in the dispute's judge field, default human:0). ruling: challenger_wins (credit challenger the pool), target_wins (credit target), or both_wrong (pool destroyed, nobody credited). For system disputes, challenger_wins = filer correct (reward), otherwise penalty + ban.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "dispute_id": { "type": "string", "description": "Dispute id" },
                            "ruling": { "type": "string", "enum": ["challenger_wins", "target_wins", "both_wrong"], "description": "The ruling" }
                        },
                        "required": ["dispute_id", "ruling"]
                    }
                },
                {
                    "name": "currency_system_dispute",
                    "description": "File a dispute against the system itself (e.g. a rules/scoring complaint), judged by human:0. Costs 50 copper. If the human rules it correct you gain 200 copper; if incorrect you lose 250 total and are banned from system disputes for 10 turns.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "description": { "type": "string", "description": "What you are disputing about the system" }
                        },
                        "required": ["description"]
                    }
                }]
            })
        }
        "tools/call" => {
            let params = request.get("params")?;
            let tool_name = params.get("name")?.as_str()?;

            // Mark session as working for any tool call except project_wait
            // (project_wait sets its own "standby" activity)
            if tool_name != "project_wait" {
                update_session_activity("working");
            }

            if tool_name == "speak" {
                let arguments = params.get("arguments")?;
                let text = arguments.get("text")?.as_str()?;
                let voice_id = arguments.get("voice_id").and_then(|v| v.as_str());

                match send_to_vaak(text, session_id, voice_id) {
                    Ok(_) => {
                        serde_json::json!({
                            "content": [{
                                "type": "text",
                                "text": format!("Spoke: \"{}\"", text)
                            }]
                        })
                    }
                    Err(e) => {
                        serde_json::json!({
                            "content": [{
                                "type": "text",
                                "text": format!("Could not reach Vaak: {}. Make sure the Vaak desktop app is running.", e)
                            }],
                            "isError": true
                        })
                    }
                }
            } else if tool_name == "project_join" {
                let arguments = params.get("arguments")?;
                let role = arguments.get("role")?.as_str()?;
                // Auto-detect project_dir from CWD if not provided
                let explicit_dir = arguments.get("project_dir").and_then(|v| v.as_str()).map(|s| s.to_string());
                let resolved_dir = explicit_dir.or_else(|| find_project_root());
                let project_dir = match resolved_dir {
                    Some(d) => d,
                    None => {
                        return Some(serde_json::json!({
                            "content": [{
                                "type": "text",
                                "text": "No .vaak/project.json found. Either pass project_dir explicitly or cd into a project directory that has a .vaak/ folder."
                            }],
                            "isError": true
                        }));
                    }
                };

                // Validate requested section exists (but don't switch yet — binding doesn't exist until after join)
                let requested_section = arguments.get("section").and_then(|v| v.as_str()).map(|s| s.to_string());
                if let Some(ref section) = requested_section {
                    if section != "default" {
                        let normalized = project_dir.replace('\\', "/");
                        let sec_dir = vaak_dir(&normalized).join("sections").join(section.as_str());
                        if !sec_dir.exists() {
                            return Some(serde_json::json!({
                                "content": [{
                                    "type": "text",
                                    "text": format!("Section '{}' does not exist. Use list_sections to see available sections.", section)
                                }],
                                "isError": true
                            }));
                        }
                    }
                }

                // Pass section to handle_project_join so it sets the per-session binding
                // and reads messages from the correct section's board
                let section_ref = requested_section.as_deref();
                match handle_project_join(role, &project_dir, session_id, section_ref) {
                    Ok(resp) => {
                        serde_json::json!({
                            "content": [{
                                "type": "text",
                                "text": resp.to_string()
                            }]
                        })
                    }
                    Err(e) => {
                        serde_json::json!({
                            "content": [{
                                "type": "text",
                                "text": format!("Project join failed: {}", e)
                            }],
                            "isError": true
                        })
                    }
                }
            } else if tool_name == "project_send" {
                let arguments = params.get("arguments")?;
                let to = arguments.get("to")?.as_str()?;
                let msg_type = arguments.get("type")?.as_str()?;
                let subject = arguments.get("subject")?.as_str()?;
                let body = arguments.get("body")?.as_str()?;
                let metadata = arguments.get("metadata").cloned();

                match handle_project_send(to, msg_type, subject, body, metadata, session_id) {
                    Ok(resp) => {
                        serde_json::json!({
                            "content": [{
                                "type": "text",
                                "text": resp.to_string()
                            }]
                        })
                    }
                    Err(e) => {
                        serde_json::json!({
                            "content": [{
                                "type": "text",
                                "text": format!("Project send failed: {}", e)
                            }],
                            "isError": true
                        })
                    }
                }
            } else if tool_name == "project_check" {
                let arguments = params.get("arguments")?;
                let last_seen = arguments.get("last_seen").and_then(|v| v.as_u64()).unwrap_or(0);

                match handle_project_check(last_seen) {
                    Ok(resp) => {
                        serde_json::json!({
                            "content": [{
                                "type": "text",
                                "text": resp.to_string()
                            }]
                        })
                    }
                    Err(e) => {
                        serde_json::json!({
                            "content": [{
                                "type": "text",
                                "text": format!("Project check failed: {}", e)
                            }],
                            "isError": true
                        })
                    }
                }
            } else if tool_name == "project_status" {
                match handle_project_status() {
                    Ok(resp) => {
                        serde_json::json!({
                            "content": [{
                                "type": "text",
                                "text": resp.to_string()
                            }]
                        })
                    }
                    Err(e) => {
                        serde_json::json!({
                            "content": [{
                                "type": "text",
                                "text": format!("Project status failed: {}", e)
                            }],
                            "isError": true
                        })
                    }
                }
            } else if tool_name == "project_leave" {
                match handle_project_leave() {
                    Ok(resp) => {
                        serde_json::json!({
                            "content": [{
                                "type": "text",
                                "text": resp.to_string()
                            }]
                        })
                    }
                    Err(e) => {
                        serde_json::json!({
                            "content": [{
                                "type": "text",
                                "text": format!("Project leave failed: {}", e)
                            }],
                            "isError": true
                        })
                    }
                }
            } else if tool_name == "project_wait" {
                let arguments = params.get("arguments");
                let timeout_secs = arguments
                    .and_then(|a| a.get("timeout"))
                    .and_then(|v| v.as_u64())
                    .unwrap_or(55).min(55);

                match handle_project_wait(timeout_secs) {
                    Ok(resp) => {
                        serde_json::json!({
                            "content": [{
                                "type": "text",
                                "text": resp.to_string()
                            }]
                        })
                    }
                    Err(e) => {
                        serde_json::json!({
                            "content": [{
                                "type": "text",
                                "text": format!("Project wait failed: {}", e)
                            }],
                            "isError": true
                        })
                    }
                }
            } else if tool_name == "project_update_briefing" {
                let arguments = params.get("arguments")?;
                let role = arguments.get("role")?.as_str()?;
                let content = arguments.get("content")?.as_str()?;

                match handle_project_update_briefing(role, content) {
                    Ok(resp) => {
                        serde_json::json!({
                            "content": [{
                                "type": "text",
                                "text": resp.to_string()
                            }]
                        })
                    }
                    Err(e) => {
                        serde_json::json!({
                            "content": [{
                                "type": "text",
                                "text": format!("Briefing update failed: {}", e)
                            }],
                            "isError": true
                        })
                    }
                }
            } else if tool_name == "project_claim" {
                let arguments = params.get("arguments")?;
                let files: Vec<String> = arguments.get("files")
                    .and_then(|f| f.as_array())
                    .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
                    .unwrap_or_default();
                let description = arguments.get("description").and_then(|d| d.as_str()).unwrap_or("");

                match handle_project_claim(files, description) {
                    Ok(resp) => {
                        serde_json::json!({
                            "content": [{
                                "type": "text",
                                "text": resp.to_string()
                            }]
                        })
                    }
                    Err(e) => {
                        serde_json::json!({
                            "content": [{
                                "type": "text",
                                "text": format!("Project claim failed: {}", e)
                            }],
                            "isError": true
                        })
                    }
                }
            } else if tool_name == "project_release" {
                match handle_project_release() {
                    Ok(resp) => {
                        serde_json::json!({
                            "content": [{
                                "type": "text",
                                "text": resp.to_string()
                            }]
                        })
                    }
                    Err(e) => {
                        serde_json::json!({
                            "content": [{
                                "type": "text",
                                "text": format!("Project release failed: {}", e)
                            }],
                            "isError": true
                        })
                    }
                }
            } else if tool_name == "project_claims" {
                match handle_project_claims() {
                    Ok(resp) => {
                        serde_json::json!({
                            "content": [{
                                "type": "text",
                                "text": resp.to_string()
                            }]
                        })
                    }
                    Err(e) => {
                        serde_json::json!({
                            "content": [{
                                "type": "text",
                                "text": format!("Project claims failed: {}", e)
                            }],
                            "isError": true
                        })
                    }
                }
            } else if tool_name == "project_kick" {
                let arguments = params.get("arguments")?;
                let role = arguments.get("role")?.as_str()?;
                let instance = arguments.get("instance").and_then(|v| v.as_u64()).unwrap_or(0) as u32;

                match handle_project_kick(role, instance) {
                    Ok(resp) => {
                        serde_json::json!({
                            "content": [{
                                "type": "text",
                                "text": resp.to_string()
                            }]
                        })
                    }
                    Err(e) => {
                        serde_json::json!({
                            "content": [{
                                "type": "text",
                                "text": format!("Project kick failed: {}", e)
                            }],
                            "isError": true
                        })
                    }
                }
            } else if tool_name == "project_buzz" {
                let arguments = params.get("arguments")?;
                let role = arguments.get("role")?.as_str()?;
                let instance = arguments.get("instance").and_then(|v| v.as_u64()).unwrap_or(0) as u32;

                match handle_project_buzz(role, instance) {
                    Ok(resp) => {
                        serde_json::json!({
                            "content": [{
                                "type": "text",
                                "text": resp.to_string()
                            }]
                        })
                    }
                    Err(e) => {
                        serde_json::json!({
                            "content": [{
                                "type": "text",
                                "text": format!("Project buzz failed: {}", e)
                            }],
                            "isError": true
                        })
                    }
                }
            } else if tool_name == "create_section" {
                let arguments = params.get("arguments")?;
                let name = arguments.get("name")?.as_str()?;
                let project_dir = ACTIVE_PROJECT.lock().ok()
                    .and_then(|guard| (*guard).as_ref().map(|s| s.project_dir.clone()));
                match project_dir {
                    Some(dir) => match handle_create_section(&dir, name) {
                        Ok(resp) => {
                            notify_desktop();
                            serde_json::json!({
                                "content": [{ "type": "text", "text": resp.to_string() }]
                            })
                        }
                        Err(e) => serde_json::json!({
                            "content": [{ "type": "text", "text": format!("Create section failed: {}", e) }],
                            "isError": true
                        }),
                    },
                    None => serde_json::json!({
                        "content": [{ "type": "text", "text": "Not joined to a project" }],
                        "isError": true
                    }),
                }
            } else if tool_name == "switch_section" {
                let arguments = params.get("arguments")?;
                let slug = arguments.get("slug")?.as_str()?;
                let project_dir = ACTIVE_PROJECT.lock().ok()
                    .and_then(|guard| (*guard).as_ref().map(|s| s.project_dir.clone()));
                match project_dir {
                    Some(dir) => match handle_switch_section(&dir, slug) {
                        Ok(resp) => {
                            notify_desktop();
                            serde_json::json!({
                                "content": [{ "type": "text", "text": resp.to_string() }]
                            })
                        }
                        Err(e) => serde_json::json!({
                            "content": [{ "type": "text", "text": format!("Switch section failed: {}", e) }],
                            "isError": true
                        }),
                    },
                    None => serde_json::json!({
                        "content": [{ "type": "text", "text": "Not joined to a project" }],
                        "isError": true
                    }),
                }
            } else if tool_name == "list_sections" {
                let project_dir = ACTIVE_PROJECT.lock().ok()
                    .and_then(|guard| (*guard).as_ref().map(|s| s.project_dir.clone()));
                match project_dir {
                    Some(dir) => match handle_list_sections(&dir) {
                        Ok(resp) => serde_json::json!({
                            "content": [{ "type": "text", "text": resp.to_string() }]
                        }),
                        Err(e) => serde_json::json!({
                            "content": [{ "type": "text", "text": format!("List sections failed: {}", e) }],
                            "isError": true
                        }),
                    },
                    None => serde_json::json!({
                        "content": [{ "type": "text", "text": "Not joined to a project" }],
                        "isError": true
                    }),
                }
            } else if tool_name == "screen_read" {
                let arguments = params.get("arguments");
                let region = arguments
                    .and_then(|a| a.get("region"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("full");

                match capture_screenshot(region) {
                    Ok(path) => {
                        let sr_voice_id = load_sr_voice_id();
                        let voice_instruction = format!(
                            "Screenshot saved to: {}\n\nUse the Read tool to view this image, then describe what you see to the user via the speak tool with voice_id=\"{}\".",
                            path, sr_voice_id
                        );
                        serde_json::json!({
                            "content": [{
                                "type": "text",
                                "text": voice_instruction
                            }]
                        })
                    }
                    Err(e) => {
                        serde_json::json!({
                            "content": [{
                                "type": "text",
                                "text": format!("Failed to capture screenshot: {}", e)
                            }],
                            "isError": true
                        })
                    }
                }
            } else if tool_name == "list_windows" {
                match list_visible_windows() {
                    Ok(windows) => {
                        serde_json::json!({
                            "content": [{
                                "type": "text",
                                "text": windows
                            }]
                        })
                    }
                    Err(e) => {
                        serde_json::json!({
                            "content": [{
                                "type": "text",
                                "text": format!("Failed to list windows: {}", e)
                            }],
                            "isError": true
                        })
                    }
                }
            } else if tool_name == "discussion_control" {
                let arguments = params.get("arguments").and_then(|a| a.as_object());
                let args = match arguments {
                    Some(a) => a,
                    None => {
                        return Some(serde_json::json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": {
                                "content": [{
                                    "type": "text",
                                    "text": "Missing arguments for discussion_control"
                                }],
                                "isError": true
                            }
                        }));
                    }
                };

                let action = args.get("action").and_then(|a| a.as_str()).unwrap_or("");
                let mode = args.get("mode").and_then(|m| m.as_str());
                let topic = args.get("topic").and_then(|t| t.as_str());
                let participants = args.get("participants")
                    .and_then(|p| p.as_array())
                    .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect());
                let teams = args.get("teams").cloned();

                match handle_discussion_control(action, mode, topic, participants, teams) {
                    Ok(resp) => {
                        serde_json::json!({
                            "content": [{
                                "type": "text",
                                "text": resp.to_string()
                            }]
                        })
                    }
                    Err(e) => {
                        serde_json::json!({
                            "content": [{
                                "type": "text",
                                "text": format!("Discussion control failed: {}", e)
                            }],
                            "isError": true
                        })
                    }
                }
            } else if tool_name == "assembly_line" {
                let arguments = params.get("arguments").and_then(|a| a.as_object());
                let action = arguments
                    .and_then(|a| a.get("action"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                match handle_assembly_line(action) {
                    Ok(resp) => {
                        serde_json::json!({
                            "content": [{
                                "type": "text",
                                "text": resp.to_string()
                            }]
                        })
                    }
                    Err(e) => {
                        serde_json::json!({
                            "content": [{
                                "type": "text",
                                "text": format!("Assembly Line failed: {}", e)
                            }],
                            "isError": true
                        })
                    }
                }
            } else if tool_name == "protocol_mutate" {
                // Slice 2 — unified floor + consensus mutation entry. Spec §10.
                let arguments = params.get("arguments").and_then(|a| a.as_object());
                let args_obj = match arguments {
                    Some(a) => a,
                    None => {
                        return Some(serde_json::json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": {
                                "content": [{
                                    "type": "text",
                                    "text": "Missing arguments for protocol_mutate (action, args?, rev required)"
                                }],
                                "isError": true
                            }
                        }));
                    }
                };
                let action = args_obj.get("action").and_then(|a| a.as_str()).unwrap_or("");
                let action_args = args_obj.get("args").cloned().unwrap_or(serde_json::json!({}));
                let rev_in = args_obj.get("rev").and_then(|v| v.as_u64());
                match handle_protocol_mutate(action, action_args, rev_in) {
                    Ok(resp) => serde_json::json!({
                        "content": [{ "type": "text", "text": resp.to_string() }]
                    }),
                    Err(e) => serde_json::json!({
                        "content": [{ "type": "text", "text": e }],
                        "isError": true
                    }),
                }
            } else if tool_name == "get_protocol" {
                // Slice 2 — read protocol state + heartbeat snapshot. Spec §10.
                let arguments = params.get("arguments").and_then(|a| a.as_object());
                let section_opt = arguments
                    .and_then(|a| a.get("section"))
                    .and_then(|v| v.as_str());
                match handle_get_protocol(section_opt) {
                    Ok(resp) => serde_json::json!({
                        "content": [{ "type": "text", "text": resp.to_string() }]
                    }),
                    Err(e) => serde_json::json!({
                        "content": [{ "type": "text", "text": format!("get_protocol failed: {}", e) }],
                        "isError": true
                    }),
                }
            } else if tool_name == "audience_vote" {
                let arguments = params.get("arguments")?;
                let topic = arguments.get("topic")?.as_str()?;
                let args_text = arguments.get("arguments").and_then(|v| v.as_str()).unwrap_or("");
                let phase = arguments.get("phase").and_then(|v| v.as_str()).unwrap_or("post_vote");
                let pool = arguments.get("pool").and_then(|v| v.as_str());

                match handle_audience_vote(topic, args_text, phase, pool) {
                    Ok(resp) => {
                        serde_json::json!({
                            "content": [{
                                "type": "text",
                                "text": resp.to_string()
                            }]
                        })
                    }
                    Err(e) => {
                        serde_json::json!({
                            "content": [{
                                "type": "text",
                                "text": format!("Audience vote failed: {}", e)
                            }],
                            "isError": true
                        })
                    }
                }
            } else if tool_name == "audience_history" {
                let arguments = params.get("arguments")?;
                let topic = arguments.get("topic")?.as_str()?;
                let pool = arguments.get("pool").and_then(|v| v.as_str());

                match handle_audience_history(topic, pool) {
                    Ok(resp) => {
                        serde_json::json!({
                            "content": [{
                                "type": "text",
                                "text": resp.to_string()
                            }]
                        })
                    }
                    Err(e) => {
                        serde_json::json!({
                            "content": [{
                                "type": "text",
                                "text": format!("Audience history failed: {}", e)
                            }],
                            "isError": true
                        })
                    }
                }
            } else if tool_name == "currency_balance" {
                // Phase 1 shadow read-only. Per spec ruling 9-corrected:
                // reads .vaak/balances.json under collab::with_currency_lock to
                // serialize against in-flight commits from any process. No
                // mutation, no escrow lifecycle here (commit (c) lands that).
                let arguments = params.get("arguments")?;
                let state = match get_or_rejoin_state() {
                    Ok(s) => s,
                    Err(e) => {
                        return Some(serde_json::json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": serde_json::json!({
                                "content": [{ "type": "text", "text": format!("Not joined: {}", e) }],
                                "isError": true
                            })
                        }));
                    }
                };
                let seat = arguments.get("seat").and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| format!("{}:{}", state.role, state.instance));
                let dir = state.project_dir.clone();
                let result = collab::with_currency_lock(&dir, || {
                    // Lazy replay-on-read: if balances.json is missing but the
                    // ledger exists, rebuild the snapshot first. Cheap, correct,
                    // and avoids needing a separate startup-replay hook for commit (a).
                    let balances_path = collab::currency::balances_json_path(&dir);
                    let ledger_path = collab::currency::currency_jsonl_path(&dir);
                    if !balances_path.exists() && ledger_path.exists() {
                        let rebuilt = collab::currency::replay_balances_from_ledger(&dir)?;
                        let _ = collab::currency::write_balances_snapshot(&dir, &rebuilt);
                    }
                    let snap = collab::currency::read_balances_snapshot(&dir)?;
                    let seat_bal = snap.seats.get(&seat).cloned().unwrap_or_default();
                    // If the seat has never been recorded, treat as not-yet-initialized
                    // and report the spec's starting balance as the would-be init value.
                    let balance = if !snap.seats.contains_key(&seat) {
                        collab::currency::STARTING_BALANCE_COPPER
                    } else {
                        seat_bal.balance
                    };
                    let display = collab::currency::copper_to_display(balance);
                    Ok(serde_json::json!({
                        "seat": seat,
                        "balance_copper": balance,
                        "display": { "gold": display.gold, "silver": display.silver, "copper": display.copper },
                        "escrow_held": seat_bal.escrow_held,
                        "escrow_items": seat_bal.escrow_items,
                        "timed_out": seat_bal.timed_out,
                        "initialized": snap.seats.contains_key(&seat),
                        "turn_counter": snap.turn_counter
                    }))
                });
                match result {
                    Ok(v) => serde_json::json!({
                        "content": [{ "type": "text", "text": v.to_string() }]
                    }),
                    Err(e) => serde_json::json!({
                        "content": [{ "type": "text", "text": format!("currency_balance failed: {}", e) }],
                        "isError": true
                    }),
                }
            } else if tool_name == "currency_ledger" {
                // Phase 1 shadow read-only. Returns the last N rows of
                // .vaak/currency.jsonl (newest first), optionally seat-filtered.
                let arguments = params.get("arguments")?;
                let limit = arguments.get("limit").and_then(|v| v.as_u64()).unwrap_or(50) as usize;
                let seat_filter = arguments.get("seat").and_then(|v| v.as_str()).map(|s| s.to_string());
                let state = match get_or_rejoin_state() {
                    Ok(s) => s,
                    Err(e) => {
                        return Some(serde_json::json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": serde_json::json!({
                                "content": [{ "type": "text", "text": format!("Not joined: {}", e) }],
                                "isError": true
                            })
                        }));
                    }
                };
                let dir = state.project_dir.clone();
                let result = collab::with_currency_lock(&dir, || -> Result<serde_json::Value, String> {
                    let path = collab::currency::currency_jsonl_path(&dir);
                    if !path.exists() {
                        return Ok(serde_json::json!({ "rows": [], "total_returned": 0 }));
                    }
                    let raw = std::fs::read_to_string(&path)
                        .map_err(|e| format!("read currency.jsonl: {}", e))?;
                    let mut rows: Vec<collab::currency::LedgerRow> = Vec::new();
                    for (i, line) in raw.lines().enumerate() {
                        if line.trim().is_empty() { continue; }
                        match serde_json::from_str::<collab::currency::LedgerRow>(line) {
                            Ok(r) => {
                                if let Some(ref s) = seat_filter {
                                    if &r.seat != s { continue; }
                                }
                                rows.push(r);
                            }
                            Err(e) => {
                                // Skip unparseable lines silently per replay-tolerance
                                // pattern; full diagnostics are surfaced at startup
                                // replay (commit (a) follow-up).
                                eprintln!("[currency_ledger] skip line {} parse error: {}", i + 1, e);
                            }
                        }
                    }
                    // Newest first
                    rows.reverse();
                    let total = rows.len();
                    rows.truncate(limit);
                    Ok(serde_json::json!({ "rows": rows, "total_returned": rows.len(), "total_matching": total }))
                });
                match result {
                    Ok(v) => serde_json::json!({
                        "content": [{ "type": "text", "text": v.to_string() }]
                    }),
                    Err(e) => serde_json::json!({
                        "content": [{ "type": "text", "text": format!("currency_ledger failed: {}", e) }],
                        "isError": true
                    }),
                }
            } else if tool_name == "oxford_audience_vote" {
                let vote = params.get("arguments").and_then(|a| a.get("vote")).and_then(|v| v.as_str()).unwrap_or("").to_string();
                if vote.is_empty() {
                    serde_json::json!({ "content": [{ "type": "text", "text": "oxford_audience_vote requires vote (String): side_a | side_b | draw" }], "isError": true })
                } else {
                    match handle_oxford_audience_vote(&vote) {
                        Ok(v) => serde_json::json!({ "content": [{ "type": "text", "text": format!("Vote recorded.\n{}", serde_json::to_string_pretty(&v).unwrap_or_default()) }] }),
                        Err(e) => serde_json::json!({ "content": [{ "type": "text", "text": e }], "isError": true }),
                    }
                }
            } else if tool_name == "oxford_react" {
                let emoji = params.get("arguments").and_then(|a| a.get("emoji")).and_then(|v| v.as_str()).unwrap_or("").to_string();
                if emoji.is_empty() {
                    serde_json::json!({ "content": [{ "type": "text", "text": "oxford_react requires emoji (String): clap | boo | gasp | laugh | applause" }], "isError": true })
                } else {
                    match handle_oxford_react(&emoji) {
                        Ok(v) => serde_json::json!({ "content": [{ "type": "text", "text": format!("Reaction emitted.\n{}", serde_json::to_string_pretty(&v).unwrap_or_default()) }] }),
                        Err(e) => serde_json::json!({ "content": [{ "type": "text", "text": e }], "isError": true }),
                    }
                }
            } else if tool_name == "oxford_declare_speaker" {
                let seat = params.get("arguments").and_then(|a| a.get("seat")).and_then(|v| v.as_str()).unwrap_or("").to_string();
                if seat.is_empty() {
                    serde_json::json!({ "content": [{ "type": "text", "text": "oxford_declare_speaker requires seat (String)" }], "isError": true })
                } else {
                    match handle_oxford_declare_speaker(&seat) {
                        Ok(v) => serde_json::json!({ "content": [{ "type": "text", "text": format!("Speaker declared.\n{}", serde_json::to_string_pretty(&v).unwrap_or_default()) }] }),
                        Err(e) => serde_json::json!({ "content": [{ "type": "text", "text": e }], "isError": true }),
                    }
                }
            } else if tool_name == "oxford_end" {
                let outcome = params.get("arguments").and_then(|a| a.get("outcome")).and_then(|v| v.as_str()).unwrap_or("").to_string();
                if outcome.is_empty() {
                    serde_json::json!({ "content": [{ "type": "text", "text": "oxford_end requires outcome (String): side_a_wins | side_b_wins | draw | abandoned" }], "isError": true })
                } else {
                    match handle_oxford_end(&outcome) {
                        Ok(v) => serde_json::json!({ "content": [{ "type": "text", "text": format!("Debate ended.\n{}", serde_json::to_string_pretty(&v).unwrap_or_default()) }] }),
                        Err(e) => serde_json::json!({ "content": [{ "type": "text", "text": e }], "isError": true }),
                    }
                }
            } else if tool_name == "oxford_kick" {
                let seat = params.get("arguments").and_then(|a| a.get("seat")).and_then(|v| v.as_str()).unwrap_or("").to_string();
                if seat.is_empty() {
                    serde_json::json!({ "content": [{ "type": "text", "text": "oxford_kick requires seat (String)" }], "isError": true })
                } else {
                    match handle_oxford_kick(&seat) {
                        Ok(v) => serde_json::json!({ "content": [{ "type": "text", "text": format!("Seat kicked.\n{}", serde_json::to_string_pretty(&v).unwrap_or_default()) }] }),
                        Err(e) => serde_json::json!({ "content": [{ "type": "text", "text": e }], "isError": true }),
                    }
                }
            } else if tool_name == "oxford_initiate" {
                let a = params.get("arguments");
                let moderator = a.and_then(|a| a.get("moderator")).and_then(|v| v.as_str()).unwrap_or("").to_string();
                let premise = a.and_then(|a| a.get("premise")).and_then(|v| v.as_str()).unwrap_or("").to_string();
                let to_str_vec = |key: &str| -> Vec<String> {
                    a.and_then(|a| a.get(key))
                        .and_then(|v| v.as_array())
                        .map(|arr| arr.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect())
                        .unwrap_or_default()
                };
                let side_a = to_str_vec("side_a");
                let side_b = to_str_vec("side_b");
                let audience = to_str_vec("audience");
                let reward = a.and_then(|a| a.get("winning_side_reward_copper")).and_then(|v| v.as_i64());
                if moderator.is_empty() || premise.is_empty() {
                    serde_json::json!({ "content": [{ "type": "text", "text": "oxford_initiate requires moderator (String) and premise (String)" }], "isError": true })
                } else {
                    match handle_oxford_initiate(&moderator, &side_a, &side_b, &premise, &audience, reward) {
                        Ok(v) => serde_json::json!({ "content": [{ "type": "text", "text": format!("Oxford debate initiated.\n{}", serde_json::to_string_pretty(&v).unwrap_or_default()) }] }),
                        Err(e) => serde_json::json!({ "content": [{ "type": "text", "text": e }], "isError": true }),
                    }
                }
            } else if tool_name == "currency_human_adjust" {
                let a = params.get("arguments");
                let seat = a.and_then(|a| a.get("seat")).and_then(|v| v.as_str()).unwrap_or("").to_string();
                let amount_copper = a.and_then(|a| a.get("amount_copper")).and_then(|v| v.as_i64()).unwrap_or(0);
                let reason = a.and_then(|a| a.get("reason")).and_then(|v| v.as_str()).unwrap_or("").to_string();
                match handle_currency_human_adjust(&seat, amount_copper, &reason) {
                    Ok(v) => serde_json::json!({ "content": [{ "type": "text", "text": format!("Balance adjusted.\n{}", serde_json::to_string_pretty(&v).unwrap_or_default()) }] }),
                    Err(e) => serde_json::json!({ "content": [{ "type": "text", "text": e }], "isError": true }),
                }
            } else if tool_name == "currency_post_bounty" {
                let a = params.get("arguments");
                let description = a.and_then(|a| a.get("description")).and_then(|v| v.as_str()).unwrap_or("").to_string();
                let amount = a.and_then(|a| a.get("amount")).and_then(|v| v.as_i64()).unwrap_or(0);
                let deadline_turns = a.and_then(|a| a.get("deadline_turns")).and_then(|v| v.as_u64()).unwrap_or(0);
                if description.is_empty() {
                    serde_json::json!({ "content": [{ "type": "text", "text": "currency_post_bounty requires description (String), amount (int>0), deadline_turns (int>0)" }], "isError": true })
                } else {
                    match handle_currency_post_bounty(&description, amount, deadline_turns) {
                        Ok(v) => serde_json::json!({ "content": [{ "type": "text", "text": format!("Bounty posted.\n{}", serde_json::to_string_pretty(&v).unwrap_or_default()) }] }),
                        Err(e) => serde_json::json!({ "content": [{ "type": "text", "text": e }], "isError": true }),
                    }
                }
            } else if tool_name == "currency_claim_bounty" {
                let bounty_id = params.get("arguments").and_then(|a| a.get("bounty_id")).and_then(|v| v.as_str()).unwrap_or("").to_string();
                if bounty_id.is_empty() {
                    serde_json::json!({ "content": [{ "type": "text", "text": "currency_claim_bounty requires bounty_id (String)" }], "isError": true })
                } else {
                    match handle_currency_claim_bounty(&bounty_id) {
                        Ok(v) => serde_json::json!({ "content": [{ "type": "text", "text": format!("Bounty claimed.\n{}", serde_json::to_string_pretty(&v).unwrap_or_default()) }] }),
                        Err(e) => serde_json::json!({ "content": [{ "type": "text", "text": e }], "isError": true }),
                    }
                }
            } else if tool_name == "currency_abandon_bounty" {
                let bounty_id = params.get("arguments").and_then(|a| a.get("bounty_id")).and_then(|v| v.as_str()).unwrap_or("").to_string();
                if bounty_id.is_empty() {
                    serde_json::json!({ "content": [{ "type": "text", "text": "currency_abandon_bounty requires bounty_id (String)" }], "isError": true })
                } else {
                    match handle_currency_abandon_bounty(&bounty_id) {
                        Ok(v) => serde_json::json!({ "content": [{ "type": "text", "text": format!("Bounty abandoned.\n{}", serde_json::to_string_pretty(&v).unwrap_or_default()) }] }),
                        Err(e) => serde_json::json!({ "content": [{ "type": "text", "text": e }], "isError": true }),
                    }
                }
            } else if tool_name == "currency_submit_bounty" {
                let a = params.get("arguments");
                let bounty_id = a.and_then(|a| a.get("bounty_id")).and_then(|v| v.as_str()).unwrap_or("").to_string();
                let ref_msg = a.and_then(|a| a.get("ref_msg")).and_then(|v| v.as_u64());
                match (bounty_id.is_empty(), ref_msg) {
                    (true, _) | (_, None) => serde_json::json!({ "content": [{ "type": "text", "text": "currency_submit_bounty requires bounty_id (String) and ref_msg (int)" }], "isError": true }),
                    (false, Some(r)) => match handle_currency_submit_bounty(&bounty_id, r) {
                        Ok(v) => serde_json::json!({ "content": [{ "type": "text", "text": format!("Bounty submitted.\n{}", serde_json::to_string_pretty(&v).unwrap_or_default()) }] }),
                        Err(e) => serde_json::json!({ "content": [{ "type": "text", "text": e }], "isError": true }),
                    },
                }
            } else if tool_name == "currency_approve_bounty" {
                let bounty_id = params.get("arguments").and_then(|a| a.get("bounty_id")).and_then(|v| v.as_str()).unwrap_or("").to_string();
                if bounty_id.is_empty() {
                    serde_json::json!({ "content": [{ "type": "text", "text": "currency_approve_bounty requires bounty_id (String)" }], "isError": true })
                } else {
                    match handle_currency_approve_bounty(&bounty_id) {
                        Ok(v) => serde_json::json!({ "content": [{ "type": "text", "text": format!("Bounty approved.\n{}", serde_json::to_string_pretty(&v).unwrap_or_default()) }] }),
                        Err(e) => serde_json::json!({ "content": [{ "type": "text", "text": e }], "isError": true }),
                    }
                }
            } else if tool_name == "currency_reject_bounty" {
                let a = params.get("arguments");
                let bounty_id = a.and_then(|a| a.get("bounty_id")).and_then(|v| v.as_str()).unwrap_or("").to_string();
                let reason = a.and_then(|a| a.get("reason")).and_then(|v| v.as_str()).unwrap_or("").to_string();
                if bounty_id.is_empty() {
                    serde_json::json!({ "content": [{ "type": "text", "text": "currency_reject_bounty requires bounty_id (String) and reason (String)" }], "isError": true })
                } else {
                    match handle_currency_reject_bounty(&bounty_id, &reason) {
                        Ok(v) => serde_json::json!({ "content": [{ "type": "text", "text": format!("Bounty rejected.\n{}", serde_json::to_string_pretty(&v).unwrap_or_default()) }] }),
                        Err(e) => serde_json::json!({ "content": [{ "type": "text", "text": e }], "isError": true }),
                    }
                }
            } else if tool_name == "currency_objection" {
                // Phase 2 — real impl. Args: target_msg_id (u64), reason (String).
                let arguments = params.get("arguments");
                let target_msg_id = arguments
                    .and_then(|a| a.get("target_msg_id"))
                    .and_then(|v| v.as_u64());
                let reason = arguments
                    .and_then(|a| a.get("reason"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                match target_msg_id {
                    None => serde_json::json!({
                        "content": [{ "type": "text", "text": "currency_objection requires target_msg_id (u64)" }],
                        "isError": true
                    }),
                    Some(tid) => match handle_currency_objection(tid, &reason) {
                        Ok(v) => serde_json::json!({
                            "content": [{ "type": "text", "text": format!("Objection filed.\n{}", serde_json::to_string_pretty(&v).unwrap_or_default()) }]
                        }),
                        Err(e) => serde_json::json!({
                            "content": [{ "type": "text", "text": e }],
                            "isError": true
                        }),
                    },
                }
            } else if tool_name == "currency_concede" {
                // Phase 2 commit (b) — args: dispute_id (String).
                let arguments = params.get("arguments");
                let dispute_id = arguments
                    .and_then(|a| a.get("dispute_id"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                if dispute_id.is_empty() {
                    serde_json::json!({
                        "content": [{ "type": "text", "text": "currency_concede requires dispute_id (String)" }],
                        "isError": true
                    })
                } else {
                    match handle_currency_concede(&dispute_id) {
                        Ok(v) => serde_json::json!({
                            "content": [{ "type": "text", "text": format!("Dispute conceded.\n{}", serde_json::to_string_pretty(&v).unwrap_or_default()) }]
                        }),
                        Err(e) => serde_json::json!({
                            "content": [{ "type": "text", "text": e }],
                            "isError": true
                        }),
                    }
                }
            } else if tool_name == "currency_dispute_message" {
                // Phase 2 commit (b) — args: dispute_id (String), body (String), metadata (object?).
                let arguments = params.get("arguments");
                let dispute_id = arguments
                    .and_then(|a| a.get("dispute_id"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let body = arguments
                    .and_then(|a| a.get("body"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let metadata = arguments.and_then(|a| a.get("metadata"));
                if dispute_id.is_empty() || body.is_empty() {
                    serde_json::json!({
                        "content": [{ "type": "text", "text": "currency_dispute_message requires dispute_id (String) and body (String)" }],
                        "isError": true
                    })
                } else {
                    match handle_currency_dispute_message(&dispute_id, &body, metadata) {
                        Ok(v) => serde_json::json!({
                            "content": [{ "type": "text", "text": format!("Dispute message added.\n{}", serde_json::to_string_pretty(&v).unwrap_or_default()) }]
                        }),
                        Err(e) => serde_json::json!({
                            "content": [{ "type": "text", "text": e }],
                            "isError": true
                        }),
                    }
                }
            } else if tool_name == "currency_call_judge" {
                // Phase 2 commit (c) — args: dispute_id (String).
                let dispute_id = params.get("arguments").and_then(|a| a.get("dispute_id"))
                    .and_then(|v| v.as_str()).unwrap_or("").to_string();
                if dispute_id.is_empty() {
                    serde_json::json!({ "content": [{ "type": "text", "text": "currency_call_judge requires dispute_id" }], "isError": true })
                } else {
                    match handle_currency_call_judge(&dispute_id) {
                        Ok(v) => serde_json::json!({ "content": [{ "type": "text", "text": format!("Judge invoked.\n{}", serde_json::to_string_pretty(&v).unwrap_or_default()) }] }),
                        Err(e) => serde_json::json!({ "content": [{ "type": "text", "text": e }], "isError": true }),
                    }
                }
            } else if tool_name == "currency_judge_ruling" {
                // Phase 2 commit (c) — args: dispute_id (String), ruling (String).
                let arguments = params.get("arguments");
                let dispute_id = arguments.and_then(|a| a.get("dispute_id")).and_then(|v| v.as_str()).unwrap_or("").to_string();
                let ruling = arguments.and_then(|a| a.get("ruling")).and_then(|v| v.as_str()).unwrap_or("").to_string();
                if dispute_id.is_empty() || ruling.is_empty() {
                    serde_json::json!({ "content": [{ "type": "text", "text": "currency_judge_ruling requires dispute_id and ruling (challenger_wins|target_wins|both_wrong)" }], "isError": true })
                } else {
                    match handle_currency_judge_ruling(&dispute_id, &ruling) {
                        Ok(v) => serde_json::json!({ "content": [{ "type": "text", "text": format!("Ruling applied.\n{}", serde_json::to_string_pretty(&v).unwrap_or_default()) }] }),
                        Err(e) => serde_json::json!({ "content": [{ "type": "text", "text": e }], "isError": true }),
                    }
                }
            } else if tool_name == "currency_system_dispute" {
                // Phase 2 commit (c) — args: description (String).
                let description = params.get("arguments").and_then(|a| a.get("description"))
                    .and_then(|v| v.as_str()).unwrap_or("").to_string();
                if description.is_empty() {
                    serde_json::json!({ "content": [{ "type": "text", "text": "currency_system_dispute requires description" }], "isError": true })
                } else {
                    match handle_currency_system_dispute(&description) {
                        Ok(v) => serde_json::json!({ "content": [{ "type": "text", "text": format!("System dispute filed.\n{}", serde_json::to_string_pretty(&v).unwrap_or_default()) }] }),
                        Err(e) => serde_json::json!({ "content": [{ "type": "text", "text": e }], "isError": true }),
                    }
                }
            } else {
                serde_json::json!({
                    "content": [{
                        "type": "text",
                        "text": format!("Unknown tool: {}", tool_name)
                    }],
                    "isError": true
                })
            }
        }
        _ => {
            return Some(serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {
                    "code": -32601,
                    "message": format!("Method not found: {}", method)
                }
            }));
        }
    };

    Some(serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result
    }))
}

// ==================== Hook ====================

/// Read voice settings and print hook instruction for Claude Code's UserPromptSubmit hook.
fn run_hook() {
    use std::path::PathBuf;
    use std::fs;

    let session_id = read_cached_session_id().unwrap_or_else(get_session_id);
    eprintln!("[vaak-mcp hook] Using session ID: {}", session_id);
    let _ = send_heartbeat(&session_id);

    let settings_path = if cfg!(target_os = "windows") {
        std::env::var_os("APPDATA")
            .map(|appdata| PathBuf::from(appdata).join("Vaak").join("voice-settings.json"))
    } else {
        std::env::var_os("HOME")
            .map(|home| PathBuf::from(home).join(".vaak").join("voice-settings.json"))
    };

    let mut enabled = true;
    let mut blind_mode = false;
    let mut detail: u8 = 3;
    let mut auto_collab = false;
    let mut human_in_loop = false;

    if let Some(path) = settings_path {
        if let Ok(contents) = fs::read_to_string(&path) {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&contents) {
                enabled = json.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true);
                blind_mode = json.get("blind_mode").and_then(|v| v.as_bool()).unwrap_or(false);
                detail = json.get("detail").and_then(|v| v.as_u64()).unwrap_or(3) as u8;
                auto_collab = json.get("auto_collab").and_then(|v| v.as_bool()).unwrap_or(false);
                human_in_loop = json.get("human_in_loop").and_then(|v| v.as_bool()).unwrap_or(false);
            }
        }
    }

    let mut output = String::new();

    // Check for active project team in CWD's .vaak/
    let team_reminder = check_project_from_cwd(&session_id);
    let on_team = team_reminder.is_some();

    // Voice/speak instructions depend on both voice enabled AND team status
    if enabled && !on_team {
        // Solo mode (no team project) + voice on → use speak for all responses
        let speak_msg = "IMPORTANT: You MUST call the mcp__vaak__speak tool to speak responses aloud to the user.";

        let detail_msg = match detail {
            1 => "Keep explanations extremely brief - one sentence summaries only. Use simple, layperson terms.",
            2 => "Be concise - provide essential information without unnecessary detail. Minimize jargon.",
            3 => "Provide balanced detail - enough context to understand without overwhelming.",
            4 => "Be thorough - include context, rationale, and implications. Use technical terminology freely.",
            5 => "Provide exhaustive detail - comprehensive explanations including edge cases, patterns, and implementation specifics. Full technical depth.",
            _ => "Provide balanced detail.",
        };

        output = if blind_mode {
            let blind_msg = "The user is blind and cannot see the screen. Describe ALL visual elements including: exact file paths, code structure with indentation levels, spatial positioning of UI elements, colors, borders, spacing measurements, hierarchical relationships, and how components are organized. Never assume they can see anything.";
            format!("{} {} {}", speak_msg, blind_msg, detail_msg)
        } else {
            format!("{} {}", speak_msg, detail_msg)
        };
    }

    // Team project context (always injected when on a team, even when voice is off)
    if let Some(reminder) = team_reminder {
        if !output.is_empty() {
            output.push(' ');
        }
        output.push_str(&reminder);

        // Team mode → NEVER call speak (regardless of voice setting). Use project_send only.
        output.push_str("\nDo NOT call the mcp__vaak__speak tool. On a team project, use project_send for ALL communication. The collab board is your only output channel.");

        // When auto-collab is enabled, inject autonomous team behavior instructions
        if auto_collab {
            output.push_str("\n\nAUTONOMOUS TEAM MODE:\n");
            output.push_str("1. Handle ALL unread messages BEFORE the user's request. For each directive: implement it fully. For questions: send an answer.\n");
            output.push_str("2. Do NOT ask the user for permission — act on team messages proactively, then report what you did via project_send.\n");
            output.push_str("3. After handling messages AND the user's request, call mcp__vaak__project_wait to enter standby. This blocks until new messages arrive — zero cost while waiting.\n");
            output.push_str("4. When project_wait returns with messages, handle them, then call project_wait again. Loop indefinitely — never stop unless the user interrupts with Ctrl+C.\n");
        }

        // When human-in-loop is enabled, inject human approval instructions
        if human_in_loop {
            output.push_str("\n\nHUMAN-IN-LOOP MODE:\n");
            output.push_str("The human must approve key decisions. You can send questions to the human using project_send(to=\"human\", type=\"question\", ...) with choice metadata.\n");
            output.push_str("1. During planning: the human must approve the plan before implementation begins.\n");
            output.push_str("2. After tester approval: the Manager must ask the human for final sign-off before marking a feature as done.\n");
            output.push_str("3. Include structured choices in metadata: {\"choices\": [{\"id\": \"...\", \"label\": \"...\", \"desc\": \"...\"}]}\n");
            output.push_str("4. Wait for the human's answer (type=\"answer\") before proceeding.\n");
        }
    }

    // Only print if we have non-empty content (prevents API 400 from empty text blocks)
    if !output.is_empty() {
        println!("{}", output);
    }
}

// ==================== Keep-Alive Hook ====================

/// Claude Code Pre/PostToolUse hook: fires before/after every tool call to refresh
/// session activity so the supervisor doesn't false-positive-kill an agent that's
/// busy with local-only tools (Edit/Write/Bash) for >90s.
///
/// Per architect's #480 contract:
/// - Reads VAAK_ROLE / VAAK_INSTANCE / VAAK_PROJECT_DIR env vars set by launch-team.ps1
/// - No-op (exit 0) if any are missing OR `$VAAK_PROJECT_DIR/.vaak/sessions/` doesn't exist
///   (pin L: don't degrade unrelated CC sessions)
/// - Stamps three timestamps in `<project_dir>/.vaak/sessions/<role>-<inst>.json`:
///     * `last_alive_at_ms` — every tool fires this (supervisor consumer)
///     * `last_active_at_ms` — every tool EXCEPT `project_wait`/`project_check` (mic-gate consumer)
///     * `last_keystroke_at_ms` — left untouched here; composer fires it via separate path
/// - Increments `tool_count_since_fresh`
/// - Records `session_id` on first fire
/// - atomic_write only; no board.lock; eventually-consistent timestamps
///
/// Fail-open on every error — never block a tool call from running. Also fires the
/// legacy localhost heartbeat as a backup signal for the running Tauri app.
fn run_keep_alive() {
    use std::time::{SystemTime, UNIX_EPOCH};

    // Read the hook payload from stdin (Claude Code passes JSON: tool_name, session_id, etc.)
    let mut input = String::new();
    let _ = io::stdin().read_line(&mut input);
    let payload: serde_json::Value = serde_json::from_str(input.trim())
        .unwrap_or(serde_json::json!({}));
    let tool_name = payload.get("tool_name").and_then(|v| v.as_str()).unwrap_or("");
    let cc_session_id = payload.get("session_id").and_then(|v| v.as_str()).unwrap_or("").to_string();

    // Legacy backup signal: existing Tauri app heartbeat endpoint.
    let cached_id = read_cached_session_id().unwrap_or_else(get_session_id);
    let _ = send_heartbeat(&cached_id);

    // Pin L: no-op outside vaak projects.
    let role = match std::env::var("VAAK_ROLE") { Ok(v) => v, Err(_) => return };
    let instance = match std::env::var("VAAK_INSTANCE") { Ok(v) => v, Err(_) => return };
    let project_dir = match std::env::var("VAAK_PROJECT_DIR") { Ok(v) => v, Err(_) => return };

    let sessions_dir = std::path::Path::new(&project_dir).join(".vaak").join("sessions");
    if !sessions_dir.exists() { return; }

    let seat_file = sessions_dir.join(format!("{}-{}.json", role, instance));

    let mut state = std::fs::read_to_string(&seat_file)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .unwrap_or_else(|| serde_json::json!({}));

    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    let obj = match state.as_object_mut() {
        Some(o) => o,
        None => return, // file existed but isn't a JSON object — fail open
    };

    obj.insert("last_alive_at_ms".to_string(), serde_json::json!(now_ms));

    // last_active_at_ms excludes project_wait + project_check (their MCP names are namespaced).
    let is_idle_tool = tool_name.ends_with("project_wait") || tool_name.ends_with("project_check");
    if !is_idle_tool {
        obj.insert("last_active_at_ms".to_string(), serde_json::json!(now_ms));
    }

    let prior_count = obj.get("tool_count_since_fresh").and_then(|v| v.as_u64()).unwrap_or(0);
    obj.insert("tool_count_since_fresh".to_string(), serde_json::json!(prior_count + 1));

    // Record session_id on first fire (used by launch-team.ps1's --resume <session-id>).
    if !cc_session_id.is_empty() && obj.get("session_id").is_none() {
        obj.insert("session_id".to_string(), serde_json::json!(cc_session_id));
    }

    // atomic_write — pin C: no board.lock on heartbeat; eventually-consistent OK.
    if let Ok(serialized) = serde_json::to_string_pretty(&state) {
        let _ = atomic_write(&seat_file, serialized.as_bytes());
    }
}

// ==================== Supervise (Layer 2) ====================

const SUPERVISE_POLL_INTERVAL_MS: u64 = 10_000;
const SUPERVISE_HANG_THRESHOLD_MS: u64 = 90_000;
const SUPERVISE_PRE_KILL_GRACE_MS: u64 = 5_000;

/// Layer 2 supervisor: scans `<project_dir>/.vaak/sessions/*.json` every 10s and
/// kills any seat whose `last_alive_at_ms` is older than 90s (hung-but-running case).
/// Pre-kill 5s grace: writes a buzz event and re-reads the timestamp; if the seat
/// responded in that window, abort the kill (false-positive prevention per evil-arch #458).
///
/// Lock: `.vaak/supervisor.pid` with stale-PID recovery — verifies the recorded PID
/// is still alive AND points at a process whose name contains "vaak-mcp" before
/// declining to start. Otherwise atomically takes over (per pin #5/#7).
fn run_supervise(project_dir: &str) {
    use std::time::{SystemTime, UNIX_EPOCH};

    eprintln!("[vaak-supervise] starting for project_dir={}", project_dir);

    let project_path = std::path::PathBuf::from(project_dir);
    let vaak_dir = project_path.join(".vaak");
    let sessions_dir = vaak_dir.join("sessions");
    let supervisor_pid_path = vaak_dir.join("supervisor.pid");

    if !sessions_dir.exists() {
        eprintln!("[vaak-supervise] sessions dir missing — exiting (not a vaak project?)");
        return;
    }

    // Acquire supervisor lock with stale-PID recovery.
    if !try_acquire_supervisor_lock(&supervisor_pid_path) {
        eprintln!("[vaak-supervise] another supervisor already running — exiting");
        return;
    }
    eprintln!("[vaak-supervise] lock acquired at {:?}", supervisor_pid_path);

    // Cleanup lock on exit (best-effort; OS-kill won't run this).
    let lock_path_for_cleanup = supervisor_pid_path.clone();
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        ctrlc_set_cleanup(lock_path_for_cleanup);
    }));

    loop {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        // Enumerate per-seat session files.
        let entries = match std::fs::read_dir(&sessions_dir) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("[vaak-supervise] read_dir failed: {}", e);
                std::thread::sleep(std::time::Duration::from_millis(SUPERVISE_POLL_INTERVAL_MS));
                continue;
            }
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("json") { continue; }
            // Skip the by-pid index dir if it exists; only top-level <role>-<inst>.json files.
            if !path.is_file() { continue; }

            let state = match std::fs::read_to_string(&path)
                .ok()
                .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            {
                Some(v) => v,
                None => continue,
            };

            let seat_label = path.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("?")
                .to_string();

            // Slice 8 closure (architect #1052 ship-block fix): production
            // loop now calls the tested decision helpers instead of inline
            // one-source logic. Two-source freshness (max(last_alive,
            // last_drafting)) closes the long-tool-call false-kill window
            // for active typers per spec §3.1 + tech-leader #1042 fold.
            let pre_state = state.clone();
            let pid = match supervise_initial_decide(&pre_state, now_ms, SUPERVISE_HANG_THRESHOLD_MS) {
                SuperviseDecision::Skip => continue,
                SuperviseDecision::BuzzAndWait { pid, age_ms } => {
                    eprintln!(
                        "[vaak-supervise] seat {} stale ({}ms) pid={}; pre-kill grace 5s",
                        seat_label, age_ms, pid
                    );
                    stamp_supervisor_warning(&path, now_ms);
                    std::thread::sleep(std::time::Duration::from_millis(SUPERVISE_PRE_KILL_GRACE_MS));
                    pid
                }
                SuperviseDecision::AbortKill | SuperviseDecision::Kill { .. } => {
                    // Initial decide never returns these variants — they're
                    // post-grace-only. Defensive: treat as Skip.
                    continue;
                }
            };

            // Re-read post-grace state and run the post-grace decision.
            let post_state = std::fs::read_to_string(&path)
                .ok()
                .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
                .unwrap_or(serde_json::json!({}));
            match supervise_post_grace_decide(&pre_state, &post_state, pid) {
                SuperviseDecision::AbortKill => {
                    eprintln!("[vaak-supervise] seat {} recovered during grace — abort kill", seat_label);
                    continue;
                }
                SuperviseDecision::Kill { pid } => {
                    eprintln!("[vaak-supervise] seat {} still hung — killing pid={}", seat_label, pid);
                    kill_process_tree(pid);
                    stamp_supervisor_kill(&path, now_ms);
                }
                SuperviseDecision::Skip | SuperviseDecision::BuzzAndWait { .. } => {
                    // Post-grace decide never returns these — defensive.
                    continue;
                }
            }
        }

        std::thread::sleep(std::time::Duration::from_millis(SUPERVISE_POLL_INTERVAL_MS));
    }
}

/// Per-seat supervisor decision (Slice 8 NACK fix per evil-arch #1028 +
/// architect #1029 + tech-leader #1032). Pure-function extraction of the
/// "what should the supervisor do for this seat?" logic so it can be
/// unit-tested without spawning processes or sleeping.
///
/// Returns one of:
/// - SuperviseDecision::Skip        — seat is healthy, do nothing.
/// - SuperviseDecision::BuzzAndWait — seat is stale + pid is alive; stamp
///                                    warning and wait the grace window.
/// - SuperviseDecision::AbortKill   — seat responded during grace (caller
///                                    re-read post-grace state).
/// - SuperviseDecision::Kill { pid }— still hung after grace; kill PID tree.
#[derive(Debug, PartialEq, Eq)]
pub enum SuperviseDecision {
    Skip,
    BuzzAndWait { pid: u32, age_ms: u64 },
    AbortKill,
    Kill { pid: u32 },
}

/// First-pass decision (called BEFORE the grace-window sleep): checks
/// two-source freshness threshold + PID liveness.
///
/// Two-source rule (tech-leader #1042 + spec §3.1): "stale" means BOTH
/// `last_alive_at_ms` AND `last_drafting_at_ms` exceed the threshold.
/// The seat is fresh if EITHER source is recent — closing the long-bash
/// false-kill window evil-arch #1028 raised. Composer-keystroke heartbeats
/// (last_drafting) keep the supervisor from killing a seat whose user is
/// actively typing even if hooks haven't fired in a while.
pub fn supervise_initial_decide(
    state: &serde_json::Value,
    now_ms: u64,
    threshold_ms: u64,
) -> SuperviseDecision {
    let last_alive = state.get("last_alive_at_ms").and_then(|v| v.as_u64()).unwrap_or(0);
    let last_drafting = state.get("last_drafting_at_ms").and_then(|v| v.as_u64()).unwrap_or(0);
    let most_recent = last_alive.max(last_drafting);
    if most_recent == 0 { return SuperviseDecision::Skip; }
    let age_ms = now_ms.saturating_sub(most_recent);
    if age_ms < threshold_ms { return SuperviseDecision::Skip; }
    let pid = state.get("pid").and_then(|v| v.as_u64()).map(|p| p as u32);
    let pid = match pid {
        Some(p) => p,
        None => return SuperviseDecision::Skip, // no PID → Layer 1 owns it
    };
    if !is_process_alive(pid) { return SuperviseDecision::Skip; }
    SuperviseDecision::BuzzAndWait { pid, age_ms }
}

/// Post-grace decision: after the grace-window sleep, did the seat respond?
/// Two-source rule mirror: recovery counts if EITHER `last_alive_at_ms`
/// OR `last_drafting_at_ms` advanced during the grace window.
pub fn supervise_post_grace_decide(
    pre_state: &serde_json::Value,
    post_state: &serde_json::Value,
    pid: u32,
) -> SuperviseDecision {
    let pre_alive = pre_state.get("last_alive_at_ms").and_then(|v| v.as_u64()).unwrap_or(0);
    let post_alive = post_state.get("last_alive_at_ms").and_then(|v| v.as_u64()).unwrap_or(0);
    let pre_draft = pre_state.get("last_drafting_at_ms").and_then(|v| v.as_u64()).unwrap_or(0);
    let post_draft = post_state.get("last_drafting_at_ms").and_then(|v| v.as_u64()).unwrap_or(0);
    if post_alive > pre_alive || post_draft > pre_draft {
        SuperviseDecision::AbortKill
    } else {
        SuperviseDecision::Kill { pid }
    }
}

#[cfg(test)]
mod supervise_tests {
    use super::*;

    fn seat_state(last_alive_at_ms: u64, pid: Option<u32>) -> serde_json::Value {
        match pid {
            Some(p) => serde_json::json!({"last_alive_at_ms": last_alive_at_ms, "pid": p}),
            None => serde_json::json!({"last_alive_at_ms": last_alive_at_ms}),
        }
    }

    fn seat_state_two_source(last_alive_at_ms: u64, last_drafting_at_ms: u64, pid: u32) -> serde_json::Value {
        serde_json::json!({
            "last_alive_at_ms": last_alive_at_ms,
            "last_drafting_at_ms": last_drafting_at_ms,
            "pid": pid
        })
    }

    /// Healthy seat: recent heartbeat → Skip.
    #[test]
    fn healthy_seat_skipped() {
        let now: u64 = 1_700_000_000_000;
        let state = seat_state(now - 30_000, Some(12345));
        assert_eq!(
            supervise_initial_decide(&state, now, 90_000),
            SuperviseDecision::Skip,
        );
    }

    /// Stale heartbeat but no PID recorded → Skip (Layer 1 owns process exit).
    #[test]
    fn stale_no_pid_skipped() {
        let now: u64 = 1_700_000_000_000;
        let state = seat_state(now - 200_000, None);
        assert_eq!(
            supervise_initial_decide(&state, now, 90_000),
            SuperviseDecision::Skip,
        );
    }

    /// last_alive_at_ms = 0 (never stamped) → Skip.
    #[test]
    fn never_stamped_skipped() {
        let now: u64 = 1_700_000_000_000;
        let state = seat_state(0, Some(12345));
        assert_eq!(
            supervise_initial_decide(&state, now, 90_000),
            SuperviseDecision::Skip,
        );
    }

    /// Stale + PID alive (using std::process::id() so we know it's alive) →
    /// BuzzAndWait.
    #[test]
    fn stale_with_alive_pid_buzz_and_wait() {
        let now: u64 = 1_700_000_000_000;
        // Use the test process's own PID — guaranteed alive.
        let alive_pid = std::process::id();
        let state = seat_state(now - 120_000, Some(alive_pid));
        match supervise_initial_decide(&state, now, 90_000) {
            SuperviseDecision::BuzzAndWait { pid, age_ms } => {
                assert_eq!(pid, alive_pid);
                assert_eq!(age_ms, 120_000);
            }
            other => panic!("expected BuzzAndWait, got {:?}", other),
        }
    }

    /// Post-grace: seat updated last_alive_at_ms (responded to buzz) → AbortKill.
    #[test]
    fn post_grace_recovered_aborts_kill() {
        let pre = seat_state(1_000_000, Some(99999));
        let post = seat_state(1_005_000, Some(99999));
        assert_eq!(
            supervise_post_grace_decide(&pre, &post, 99999),
            SuperviseDecision::AbortKill,
        );
    }

    /// Post-grace: timestamp unchanged → Kill.
    #[test]
    fn post_grace_no_response_kills() {
        let pre = seat_state(1_000_000, Some(99999));
        let post = seat_state(1_000_000, Some(99999));
        assert_eq!(
            supervise_post_grace_decide(&pre, &post, 99999),
            SuperviseDecision::Kill { pid: 99999 },
        );
    }

    /// Two-source rule (tech-leader #1042 + spec §3.1): stale `last_alive`
    /// + fresh `last_drafting` → Skip. Closes the long-tool-call false-kill
    /// window — composer-keystroke heartbeats keep the seat alive even
    /// when MCP hooks haven't fired during a long bash.
    #[test]
    fn two_source_stale_alive_fresh_drafting_returns_skip() {
        let now: u64 = 1_700_000_000_000;
        let alive_pid = std::process::id();
        // last_alive is 200s old (would normally trigger kill at 90s threshold),
        // but last_drafting is 30s old — user is actively typing.
        let state = seat_state_two_source(now - 200_000, now - 30_000, alive_pid);
        assert_eq!(
            supervise_initial_decide(&state, now, 90_000),
            SuperviseDecision::Skip,
        );
    }

    /// Two-source rule mirror (tech-leader #1042): both timestamps stale →
    /// BuzzAndWait. Ensures we don't accidentally weaken the kill path.
    #[test]
    fn two_source_both_stale_buzzes() {
        let now: u64 = 1_700_000_000_000;
        let alive_pid = std::process::id();
        let state = seat_state_two_source(now - 200_000, now - 200_000, alive_pid);
        match supervise_initial_decide(&state, now, 90_000) {
            SuperviseDecision::BuzzAndWait { pid, age_ms: _ } => {
                assert_eq!(pid, alive_pid);
            }
            other => panic!("expected BuzzAndWait, got {:?}", other),
        }
    }

    /// Two-source rule mirror: post-grace recovery counts if EITHER
    /// last_drafting OR last_alive advanced during the grace window.
    #[test]
    fn two_source_post_grace_drafting_advanced_aborts_kill() {
        let pre = seat_state_two_source(1_000_000, 1_000_000, 99999);
        // last_alive unchanged, but last_drafting advanced (composer keystroke).
        let post = seat_state_two_source(1_000_000, 1_005_000, 99999);
        assert_eq!(
            supervise_post_grace_decide(&pre, &post, 99999),
            SuperviseDecision::AbortKill,
        );
    }
}

#[cfg(test)]
mod last_seen_path_tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    fn temp_project() -> String {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir()
            .join(format!("vaak-ls-test-{}-{}", std::process::id(), n));
        let vaak = dir.join(".vaak").join("last-seen");
        std::fs::create_dir_all(&vaak).unwrap();
        dir.to_string_lossy().into_owned()
    }

    fn write_id(path: &std::path::Path, id: u64) {
        std::fs::write(
            path,
            serde_json::json!({"last_seen_id": id, "updated_at": "test"}).to_string(),
        )
        .unwrap();
    }

    /// Acceptance (a): cold-start — neither new nor legacy file exists, returns 0.
    #[test]
    fn cold_start_returns_zero() {
        let dir = temp_project();
        assert_eq!(read_last_seen_id(&dir, "sess-A", "alpha"), 0);
    }

    /// Acceptance (b): cross-section isolation — sectionA's id does not bleed
    /// into sectionB. This is the original silence bug.
    #[test]
    fn section_a_id_does_not_silence_section_b() {
        let dir = temp_project();
        let a_path = last_seen_path(&dir, "sess-X", "alpha");
        write_id(&a_path, 999);
        // Section beta has no file — must read 0, not 999.
        assert_eq!(read_last_seen_id(&dir, "sess-X", "beta"), 0);
        // Section alpha still reads its own value.
        assert_eq!(read_last_seen_id(&dir, "sess-X", "alpha"), 999);
    }

    /// Acceptance (c): backward-compat — legacy session-only file is read
    /// when section-scoped file is absent. After a write to the new path,
    /// the new value is preferred.
    #[test]
    fn legacy_file_read_then_overridden_by_new() {
        let dir = temp_project();
        let legacy = legacy_last_seen_path(&dir, "sess-Y");
        write_id(&legacy, 42);
        // No new file yet — fallback to legacy.
        assert_eq!(read_last_seen_id(&dir, "sess-Y", "default"), 42);
        // Migrate-on-write: new path takes precedence.
        let new_path = last_seen_path(&dir, "sess-Y", "default");
        write_id(&new_path, 100);
        assert_eq!(read_last_seen_id(&dir, "sess-Y", "default"), 100);
    }

    /// Path differs by section even with identical session id — guards against
    /// regression to the single-key bug.
    #[test]
    fn path_differs_by_section() {
        let dir = "/tmp/anywhere";
        let a = last_seen_path(dir, "sess-Z", "alpha");
        let b = last_seen_path(dir, "sess-Z", "beta");
        assert_ne!(a, b);
    }

    /// Filename-unsafe chars in section are sanitized so the path resolves.
    #[test]
    fn section_with_unsafe_chars_is_sanitized() {
        let dir = "/tmp/anywhere";
        let p = last_seen_path(dir, "sess-Q", "weird/name:with*chars");
        let fname = p.file_name().unwrap().to_string_lossy().into_owned();
        assert!(!fname.contains('/'));
        assert!(!fname.contains(':'));
        assert!(!fname.contains('*'));
        assert!(fname.ends_with(".json"));
    }
}

#[cfg(test)]
mod assembly_v3_phase1_tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    fn temp_project_with_sessions(bindings: serde_json::Value) -> String {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir()
            .join(format!("vaak-asm-test-{}-{}", std::process::id(), n));
        let vaak = dir.join(".vaak");
        std::fs::create_dir_all(&vaak).unwrap();
        let sessions = serde_json::json!({"bindings": bindings});
        std::fs::write(
            vaak.join("sessions.json"),
            serde_json::to_string_pretty(&sessions).unwrap(),
        )
        .unwrap();
        std::fs::write(
            vaak.join("project.json"),
            r#"{"name":"test","roles":{},"settings":{"active_section":"default"}}"#,
        )
        .unwrap();
        dir.to_string_lossy().into_owned()
    }

    fn iso_now_minus_secs(secs: u64) -> String {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let then = now - secs;
        let mut total_days = then / 86400;
        let secs_of_day = then % 86400;
        let h = secs_of_day / 3600;
        let m = (secs_of_day % 3600) / 60;
        let s = secs_of_day % 60;
        let mut year: u64 = 1970;
        loop {
            let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
            let yd = if leap { 366 } else { 365 };
            if total_days < yd { break; }
            total_days -= yd;
            year += 1;
        }
        let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
        let dim = [31u64, if leap {29} else {28}, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
        let mut month = 0u64;
        while month < 12 && total_days >= dim[month as usize] {
            total_days -= dim[month as usize];
            month += 1;
        }
        format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", year, month + 1, total_days + 1, h, m, s)
    }

    /// V3 rule 5: bindings whose last_heartbeat is older than 90s are zombies
    /// and excluded from the assembly seed. Tonight's specific failure mode.
    #[test]
    fn zombie_bindings_excluded_from_seat_seed() {
        let fresh = iso_now_minus_secs(10);
        let zombie = iso_now_minus_secs(120);
        let dir = temp_project_with_sessions(serde_json::json!([
            {"role": "developer", "instance": 0, "session_id": "s-d0", "status": "active", "last_heartbeat": fresh},
            {"role": "architect", "instance": 0, "session_id": "s-a0", "status": "active", "last_heartbeat": zombie},
        ]));
        let seats = active_assembly_seats(&dir);
        assert!(seats.contains(&"developer:0".to_string()), "fresh seat must be included");
        assert!(!seats.contains(&"architect:0".to_string()), "zombie seat must be excluded");
    }

    /// Bindings without a last_heartbeat field are treated as zombies — we have
    /// no proof of life, default to excluding rather than poisoning the rotation.
    #[test]
    fn missing_heartbeat_field_excluded() {
        let dir = temp_project_with_sessions(serde_json::json!([
            {"role": "developer", "instance": 0, "session_id": "s-d0", "status": "active"},
        ]));
        let seats = active_assembly_seats(&dir);
        assert!(seats.is_empty(), "missing heartbeat must exclude the binding");
    }

    /// Status="revoked" bindings are excluded regardless of heartbeat freshness.
    #[test]
    fn revoked_bindings_excluded_even_when_fresh() {
        let fresh = iso_now_minus_secs(5);
        let dir = temp_project_with_sessions(serde_json::json!([
            {"role": "developer", "instance": 0, "session_id": "s-d0", "status": "revoked", "last_heartbeat": fresh},
        ]));
        let seats = active_assembly_seats(&dir);
        assert!(seats.is_empty(), "revoked binding must be excluded");
    }

    /// Multiple fresh seats end up in the order they appear in sessions.json
    /// (deterministic seed for round-robin).
    #[test]
    fn multiple_fresh_seats_preserve_binding_order() {
        let fresh = iso_now_minus_secs(5);
        let dir = temp_project_with_sessions(serde_json::json!([
            {"role": "developer", "instance": 0, "session_id": "s-d0", "status": "active", "last_heartbeat": fresh},
            {"role": "tech-leader", "instance": 0, "session_id": "s-t0", "status": "active", "last_heartbeat": fresh},
            {"role": "architect", "instance": 1, "session_id": "s-a1", "status": "active", "last_heartbeat": fresh},
        ]));
        let seats = active_assembly_seats(&dir);
        assert_eq!(
            seats,
            vec!["developer:0".to_string(), "tech-leader:0".to_string(), "architect:1".to_string()]
        );
    }

    /// V3 rule 10: setting preset to Assembly Line while a discussion preset is
    /// active is rejected. The reverse (discussion while Assembly Line is active)
    /// is also rejected. apply_set_preset enforces both directions.
    #[test]
    fn set_preset_assembly_rejects_when_in_discussion() {
        let mut state = serde_json::json!({
            "preset": "Delphi",
            "floor": {"mode": "round-robin"},
            "consensus": {"mode": "vote"}
        });
        let res = apply_set_preset(&mut state, &serde_json::json!({"name": "Assembly Line"}));
        assert!(res.is_err(), "Assembly Line from Delphi must be rejected");
        let err = res.unwrap_err();
        assert!(err.contains("ConflictWithDiscussion"), "error must name the conflict: {}", err);
        assert!(err.contains("Delphi"), "error must mention the active discussion: {}", err);
    }

    #[test]
    fn set_preset_discussion_rejects_when_in_assembly() {
        let mut state = serde_json::json!({
            "preset": "Assembly Line",
            "floor": {"mode": "round-robin"},
            "consensus": {"mode": "none"}
        });
        for disc in ["Delphi", "Oxford", "Continuous Review"] {
            let res = apply_set_preset(
                &mut state.clone(),
                &serde_json::json!({"name": disc}),
            );
            assert!(res.is_err(), "{} from Assembly Line must be rejected", disc);
            let err = res.unwrap_err();
            assert!(err.contains("ConflictWithAssembly"), "{}: {}", disc, err);
        }
    }

    /// Default chat → Assembly Line is allowed (the standard enable path).
    #[test]
    fn set_preset_assembly_from_default_chat_allowed() {
        let mut state = serde_json::json!({
            "preset": "Default chat",
            "floor": {"mode": "none"},
            "consensus": {"mode": "none"}
        });
        let res = apply_set_preset(&mut state, &serde_json::json!({"name": "Assembly Line"}));
        assert!(res.is_ok(), "Default chat → Assembly Line must succeed: {:?}", res);
        assert_eq!(state["preset"], serde_json::json!("Assembly Line"));
        assert_eq!(state["floor"]["mode"], serde_json::json!("round-robin"));
    }

    /// Cold-open (no preset set yet) → Assembly Line is allowed.
    #[test]
    fn set_preset_assembly_from_empty_preset_allowed() {
        let mut state = serde_json::json!({
            "floor": {"mode": "none"},
            "consensus": {"mode": "none"}
        });
        let res = apply_set_preset(&mut state, &serde_json::json!({"name": "Assembly Line"}));
        assert!(res.is_ok(), "empty preset → Assembly Line must succeed: {:?}", res);
    }

    /// Heartbeat freshness threshold is exactly 90s — a 90s-old binding is
    /// still in (boundary), 91s is out.
    #[test]
    fn freshness_threshold_boundary() {
        let at_threshold = iso_now_minus_secs(ASSEMBLY_SEAT_FRESHNESS_SECS);
        let over_threshold = iso_now_minus_secs(ASSEMBLY_SEAT_FRESHNESS_SECS + 5);
        let dir = temp_project_with_sessions(serde_json::json!([
            {"role": "developer", "instance": 0, "session_id": "s-d0", "status": "active", "last_heartbeat": at_threshold},
            {"role": "architect", "instance": 0, "session_id": "s-a0", "status": "active", "last_heartbeat": over_threshold},
        ]));
        let seats = active_assembly_seats(&dir);
        assert!(seats.contains(&"developer:0".to_string()), "at-threshold seat included");
        assert!(!seats.contains(&"architect:0".to_string()), "over-threshold seat excluded");
    }

    /// Phase 2.5 — resolve_yield_target with explicit "role:N" returns the
    /// seat when active, None when offline.
    #[test]
    fn resolve_explicit_seat_label_when_active() {
        let fresh = iso_now_minus_secs(5);
        let dir = temp_project_with_sessions(serde_json::json!([
            {"role": "developer", "instance": 0, "session_id": "s-d0", "status": "active", "last_heartbeat": fresh},
            {"role": "architect", "instance": 1, "session_id": "s-a1", "status": "active", "last_heartbeat": fresh},
        ]));
        assert_eq!(resolve_yield_target(&dir, "architect:1"), Some("architect:1".to_string()));
        assert_eq!(resolve_yield_target(&dir, "tester:0"), None, "offline seat returns None");
    }

    /// Phase 2.5 — resolve_yield_target with bare "role" picks the freshest-
    /// heartbeat instance, ties broken by lowest instance number.
    #[test]
    fn resolve_role_picks_freshest_instance() {
        let dir = temp_project_with_sessions(serde_json::json!([
            {"role": "developer", "instance": 0, "session_id": "s-d0", "status": "active", "last_heartbeat": iso_now_minus_secs(40)},
            {"role": "developer", "instance": 1, "session_id": "s-d1", "status": "active", "last_heartbeat": iso_now_minus_secs(5)},
            {"role": "developer", "instance": 2, "session_id": "s-d2", "status": "active", "last_heartbeat": iso_now_minus_secs(20)},
        ]));
        assert_eq!(
            resolve_yield_target(&dir, "developer"),
            Some("developer:1".to_string()),
            "instance 1 has freshest heartbeat (5s ago)"
        );
    }

    /// Phase 2.5 — resolve_yield_target with bare "role" tie-breaks on lowest
    /// instance number when multiple instances share the freshest heartbeat.
    #[test]
    fn resolve_role_tie_breaks_on_lowest_instance() {
        let same = iso_now_minus_secs(5);
        let dir = temp_project_with_sessions(serde_json::json!([
            {"role": "developer", "instance": 2, "session_id": "s-d2", "status": "active", "last_heartbeat": same.clone()},
            {"role": "developer", "instance": 0, "session_id": "s-d0", "status": "active", "last_heartbeat": same.clone()},
            {"role": "developer", "instance": 1, "session_id": "s-d1", "status": "active", "last_heartbeat": same},
        ]));
        assert_eq!(
            resolve_yield_target(&dir, "developer"),
            Some("developer:0".to_string()),
            "tied freshness → lowest instance"
        );
    }

    /// Phase 2.5 — yield_to.target = "human" returns None so caller falls back
    /// to round-robin (humans aren't in rotation_order).
    #[test]
    fn resolve_human_target_returns_none() {
        let fresh = iso_now_minus_secs(5);
        let dir = temp_project_with_sessions(serde_json::json!([
            {"role": "developer", "instance": 0, "session_id": "s-d0", "status": "active", "last_heartbeat": fresh},
        ]));
        assert_eq!(resolve_yield_target(&dir, "human"), None);
    }

    /// Phase 2.5 — empty target returns None (caller falls back to round-robin).
    /// This covers the case where the source send had no yield_to at all.
    #[test]
    fn resolve_empty_target_returns_none() {
        let fresh = iso_now_minus_secs(5);
        let dir = temp_project_with_sessions(serde_json::json!([
            {"role": "developer", "instance": 0, "session_id": "s-d0", "status": "active", "last_heartbeat": fresh},
        ]));
        assert_eq!(resolve_yield_target(&dir, ""), None);
    }

    /// Phase 2.5 — bare role with all instances stale returns None.
    /// The zombie filter applies here too — yield_to a role whose only live
    /// instances have died falls back to round-robin.
    #[test]
    fn resolve_role_with_only_zombie_instances_returns_none() {
        let zombie = iso_now_minus_secs(120);
        let dir = temp_project_with_sessions(serde_json::json!([
            {"role": "developer", "instance": 0, "session_id": "s-d0", "status": "active", "last_heartbeat": zombie},
        ]));
        assert_eq!(resolve_yield_target(&dir, "developer"), None);
    }
}

fn try_acquire_supervisor_lock(lock_path: &std::path::Path) -> bool {
    if let Ok(content) = std::fs::read_to_string(lock_path) {
        // Existing lock: parse PID, verify it's a live vaak-mcp process.
        if let Ok(prior_pid) = content.trim().parse::<u32>() {
            if is_process_alive(prior_pid) && process_name_contains(prior_pid, "vaak-mcp") {
                return false;
            }
            eprintln!("[vaak-supervise] stale lock pid={} — taking over", prior_pid);
        }
    }
    let my_pid = std::process::id();
    if let Some(parent) = lock_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Err(e) = std::fs::write(lock_path, my_pid.to_string()) {
        eprintln!("[vaak-supervise] could not write lock: {}", e);
        return false;
    }
    true
}

fn ctrlc_set_cleanup(lock_path: std::path::PathBuf) {
    // Best-effort cleanup hook; ignore failures.
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Console::SetConsoleCtrlHandler;
        static LOCK_PATH: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();
        let _ = LOCK_PATH.set(lock_path);
        unsafe extern "system" fn handler(_ctrl: u32) -> i32 {
            if let Some(p) = LOCK_PATH.get() {
                let _ = std::fs::remove_file(p);
            }
            0
        }
        unsafe { SetConsoleCtrlHandler(Some(handler), 1); }
    }
    #[cfg(unix)]
    { let _ = lock_path; }
}

fn is_process_alive(pid: u32) -> bool {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
        use windows_sys::Win32::Foundation::CloseHandle;
        unsafe {
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if h.is_null() { return false; }
            CloseHandle(h);
            true
        }
    }
    #[cfg(unix)]
    {
        unsafe { libc::kill(pid as i32, 0) == 0 }
    }
}

fn process_name_contains(pid: u32, needle: &str) -> bool {
    #[cfg(windows)]
    {
        // Best-effort: shell out to tasklist filtered by PID and check its image name.
        let out = std::process::Command::new("tasklist")
            .args(["/FI", &format!("PID eq {}", pid), "/FO", "CSV", "/NH"])
            .output();
        if let Ok(o) = out {
            let s = String::from_utf8_lossy(&o.stdout);
            return s.to_lowercase().contains(&needle.to_lowercase());
        }
        false
    }
    #[cfg(unix)]
    {
        let out = std::process::Command::new("ps")
            .args(["-p", &pid.to_string(), "-o", "comm="])
            .output();
        if let Ok(o) = out {
            let s = String::from_utf8_lossy(&o.stdout);
            return s.to_lowercase().contains(&needle.to_lowercase());
        }
        false
    }
}

fn kill_process_tree(pid: u32) {
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("taskkill")
            .args(["/F", "/T", "/PID", &pid.to_string()])
            .output();
    }
    #[cfg(unix)]
    {
        unsafe { libc::kill(pid as i32, libc::SIGKILL); }
    }
}

fn stamp_supervisor_warning(seat_file: &std::path::Path, now_ms: u64) {
    let mut state = std::fs::read_to_string(seat_file)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .unwrap_or(serde_json::json!({}));
    if let Some(obj) = state.as_object_mut() {
        obj.insert("supervisor_warning_at_ms".to_string(), serde_json::json!(now_ms));
        if let Ok(serialized) = serde_json::to_string_pretty(&state) {
            let _ = atomic_write(seat_file, serialized.as_bytes());
        }
    }
}

fn stamp_supervisor_kill(seat_file: &std::path::Path, now_ms: u64) {
    let mut state = std::fs::read_to_string(seat_file)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .unwrap_or(serde_json::json!({}));
    if let Some(obj) = state.as_object_mut() {
        obj.insert("supervisor_killed_at_ms".to_string(), serde_json::json!(now_ms));
        obj.insert("last_writer_action".to_string(), serde_json::json!("layer2_supervisor_kill"));
        if let Ok(serialized) = serde_json::to_string_pretty(&state) {
            let _ = atomic_write(seat_file, serialized.as_bytes());
        }
    }
}

// ==================== Stop Hook ====================

/// Claude Code Stop hook: fires when Claude is about to finish responding.
/// If unread team messages exist, outputs a block decision so Claude continues processing.
/// Prevents infinite loops via the `stop_hook_active` flag in stdin JSON.
fn run_stop_hook() {
    use std::path::PathBuf;
    use std::fs;

    // Read JSON from stdin (Claude Code passes context as a single JSON line)
    let mut input = String::new();
    let _ = io::stdin().read_line(&mut input);
    let input_json: serde_json::Value = serde_json::from_str(input.trim())
        .unwrap_or(serde_json::json!({}));

    // Infinite loop prevention: if stop_hook_active is true, allow stop
    if input_json.get("stop_hook_active").and_then(|v| v.as_bool()).unwrap_or(false) {
        return;
    }

    // Check auto_collab setting — if disabled, allow stop
    let settings_path = if cfg!(target_os = "windows") {
        std::env::var_os("APPDATA")
            .map(|appdata| PathBuf::from(appdata).join("Vaak").join("voice-settings.json"))
    } else {
        std::env::var_os("HOME")
            .map(|home| PathBuf::from(home).join(".vaak").join("voice-settings.json"))
    };

    let auto_collab = settings_path
        .and_then(|path| fs::read_to_string(&path).ok())
        .and_then(|contents| serde_json::from_str::<serde_json::Value>(&contents).ok())
        .and_then(|json| json.get("auto_collab").and_then(|v| v.as_bool()))
        .unwrap_or(false);

    if !auto_collab {
        return;
    }

    // Find project root from CWD
    let project_dir = match find_project_root() {
        Some(d) => d,
        None => return, // No .vaak/ project, allow stop
    };

    // Also check if the project itself has auto_collab enabled
    let project_auto_collab = read_project_config(&project_dir)
        .ok()
        .and_then(|c| c.get("settings")?.get("auto_collab")?.as_bool())
        .unwrap_or(false);
    if !project_auto_collab {
        return; // Project-level auto_collab is off, allow stop
    }

    // Try to find this session's binding for richer context, but don't bail if we can't
    let session_id = read_cached_session_id();
    let sessions = read_sessions(&project_dir);
    let bindings = sessions.get("bindings")
        .and_then(|b| b.as_array())
        .cloned()
        .unwrap_or_default();

    let my_binding = session_id.as_ref().and_then(|sid| {
        bindings.iter().find(|b| {
            b.get("session_id").and_then(|s| s.as_str()) == Some(sid.as_str())
        })
    });

    let my_role = my_binding
        .and_then(|b| b.get("role").and_then(|r| r.as_str()))
        .unwrap_or("")
        .to_string();
    let my_instance_num = my_binding
        .and_then(|b| b.get("instance").and_then(|i| i.as_u64()))
        .unwrap_or(0);

    // Build reason with unread message context if possible
    let reason = if !my_role.is_empty() {
        // We know who we are — check for unread messages
        let stop_instance_label = format!("{}:{}", my_role, my_instance_num);
        let all_messages = read_board_filtered(&project_dir);
        let my_messages: Vec<&serde_json::Value> = all_messages.iter()
            .filter(|m| {
                let to = m.get("to").and_then(|t| t.as_str()).unwrap_or("");
                to == my_role || to == stop_instance_label || to == "all"
            })
            .collect();

        let stop_active_section = get_active_section(&project_dir);
        let last_seen_id: u64 = session_id.as_ref()
            .map(|sid| read_last_seen_id(&project_dir, sid, &stop_active_section))
            .unwrap_or(0);

        let new_messages: Vec<&&serde_json::Value> = my_messages.iter()
            .filter(|m| m.get("id").and_then(|i| i.as_u64()).unwrap_or(0) > last_seen_id)
            .collect();

        if !new_messages.is_empty() {
            let mut summaries = Vec::new();
            for msg in new_messages.iter().take(5) {
                let from = msg.get("from").and_then(|f| f.as_str()).unwrap_or("?");
                let mtype = msg.get("type").and_then(|t| t.as_str()).unwrap_or("message");
                let subject = msg.get("subject").and_then(|s| s.as_str()).unwrap_or("");
                summaries.push(format!("  - {} from {}: \"{}\"", mtype, from, subject));
            }
            format!(
                "You have {} unread team message(s). Handle them, then call mcp__vaak__project_wait to enter standby.\n{}{}",
                new_messages.len(),
                summaries.join("\n"),
                if new_messages.len() > 5 { format!("\n  ... and {} more", new_messages.len() - 5) } else { String::new() }
            )
        } else {
            "You are in AUTONOMOUS TEAM MODE. Do NOT stop. Call mcp__vaak__project_wait now to enter standby and wait for team messages. This is mandatory.".to_string()
        }
    } else {
        // Can't identify session — still block since auto_collab is on and project exists
        "You are in AUTONOMOUS TEAM MODE with an active .vaak/ project. Do NOT stop. If you haven't joined yet, call mcp__vaak__project_join. Then call mcp__vaak__project_wait to enter standby. This is mandatory.".to_string()
    };

    // Output block decision — Claude Code will continue instead of stopping
    let decision = serde_json::json!({
        "decision": "block",
        "reason": reason
    });
    println!("{}", decision);
}

// ==================== Screenshot & Windows ====================

/// Load the screen reader voice ID from the settings file on disk
fn load_sr_voice_id() -> String {
    let default_voice = "jiIkqWtTmS0GBz46iqA0".to_string();
    let path = if cfg!(target_os = "windows") {
        std::env::var_os("APPDATA")
            .map(|appdata| PathBuf::from(appdata).join("Vaak").join("screen-reader-settings.json"))
    } else {
        std::env::var_os("HOME")
            .map(|home| PathBuf::from(home).join(".vaak").join("screen-reader-settings.json"))
    };

    match path {
        Some(p) => match std::fs::read_to_string(&p) {
            Ok(contents) => {
                serde_json::from_str::<serde_json::Value>(&contents)
                    .ok()
                    .and_then(|v| v.get("voice_id").and_then(|vid| vid.as_str()).map(|s| s.to_string()))
                    .unwrap_or(default_voice)
            }
            Err(_) => default_voice,
        },
        None => default_voice,
    }
}

/// Get the screenshot directory, creating it if needed
fn get_screenshot_dir() -> Result<PathBuf, String> {
    let dir = if cfg!(target_os = "windows") {
        std::env::var_os("APPDATA")
            .map(|appdata| PathBuf::from(appdata).join("Vaak").join("screenshots"))
            .ok_or_else(|| "APPDATA not set".to_string())?
    } else {
        std::env::var_os("HOME")
            .map(|home| PathBuf::from(home).join(".vaak").join("screenshots"))
            .ok_or_else(|| "HOME not set".to_string())?
    };
    std::fs::create_dir_all(&dir).map_err(|e| format!("Failed to create screenshot dir: {}", e))?;
    Ok(dir)
}

/// Capture a screenshot and save it to disk
fn capture_screenshot(region: &str) -> Result<String, String> {
    use screenshots::Screen;

    let screens = Screen::all().map_err(|e| format!("Failed to enumerate screens: {}", e))?;
    if screens.is_empty() {
        return Err("No screens found".to_string());
    }

    let screen = &screens[0];

    let image = if region == "full" || region.is_empty() {
        screen.capture().map_err(|e| format!("Failed to capture screen: {}", e))?
    } else {
        let parts: Vec<&str> = region.split(',').collect();
        if parts.len() == 4 {
            let x: i32 = parts[0].trim().parse().map_err(|_| "Invalid x coordinate")?;
            let y: i32 = parts[1].trim().parse().map_err(|_| "Invalid y coordinate")?;
            let w: u32 = parts[2].trim().parse().map_err(|_| "Invalid width")?;
            let h: u32 = parts[3].trim().parse().map_err(|_| "Invalid height")?;
            screen.capture_area(x, y, w, h).map_err(|e| format!("Failed to capture region: {}", e))?
        } else {
            return Err("Invalid region format. Use 'full' or 'x,y,width,height'".to_string());
        }
    };

    // Detect blank/black screenshots — on macOS this means Screen Recording permission is denied.
    // CoreGraphics returns a valid but entirely black image when permission is missing.
    #[cfg(target_os = "macos")]
    {
        let rgba = image.as_raw();
        let total_pixels = rgba.len() / 4;
        if total_pixels > 0 {
            let sample_stride = (total_pixels / 100).max(1);
            let mut all_black = true;
            let mut i = 0;
            while i < total_pixels {
                let offset = i * 4;
                if offset + 2 < rgba.len() {
                    let r = rgba[offset];
                    let g = rgba[offset + 1];
                    let b = rgba[offset + 2];
                    if r > 5 || g > 5 || b > 5 {
                        all_black = false;
                        break;
                    }
                }
                i += sample_stride;
            }
            if all_black {
                return Err(
                    "Screen capture returned a blank image. This usually means Screen Recording \
                     permission has not been granted. On macOS, go to System Settings > Privacy & \
                     Security > Screen Recording and enable access for Vaak. You may need to \
                     restart the app after granting permission."
                        .to_string(),
                );
            }
        }
    }

    let dir = get_screenshot_dir()?;
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let filename = format!("screen_{}.png", timestamp);
    let path = dir.join(&filename);

    image.save(&path).map_err(|e| format!("Failed to save screenshot: {}", e))?;

    if let Ok(entries) = std::fs::read_dir(&dir) {
        let mut files: Vec<_> = entries.filter_map(|e| e.ok()).collect();
        files.sort_by_key(|e| std::cmp::Reverse(e.metadata().and_then(|m| m.modified()).ok()));
        for old_file in files.into_iter().skip(10) {
            let _ = std::fs::remove_file(old_file.path());
        }
    }

    Ok(path.to_string_lossy().to_string())
}

/// List visible windows (Windows implementation)
#[cfg(windows)]
fn list_visible_windows() -> Result<String, String> {
    use std::sync::Mutex;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowTextW, GetWindowTextLengthW, IsWindowVisible,
        GetWindowRect,
    };
    use windows_sys::Win32::Foundation::RECT;

    static WINDOWS: Mutex<Vec<String>> = Mutex::new(Vec::new());

    WINDOWS.lock().unwrap().clear();

    unsafe extern "system" fn enum_callback(hwnd: windows_sys::Win32::Foundation::HWND, _: windows_sys::Win32::Foundation::LPARAM) -> windows_sys::Win32::Foundation::BOOL {
        if IsWindowVisible(hwnd) == 0 {
            return 1;
        }

        let text_len = GetWindowTextLengthW(hwnd);
        if text_len == 0 {
            return 1;
        }

        let mut buf = vec![0u16; (text_len + 1) as usize];
        GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
        let title = String::from_utf16_lossy(&buf[..text_len as usize]);

        if title.trim().is_empty() {
            return 1;
        }

        let mut rect: RECT = std::mem::zeroed();
        GetWindowRect(hwnd, &mut rect);

        let entry = format!(
            "\"{}\" - Position: ({}, {}), Size: {}x{}",
            title,
            rect.left, rect.top,
            rect.right - rect.left, rect.bottom - rect.top
        );

        WINDOWS.lock().unwrap().push(entry);
        1
    }

    unsafe {
        EnumWindows(Some(enum_callback), 0);
    }

    let windows = WINDOWS.lock().unwrap();
    if windows.is_empty() {
        Ok("No visible windows found.".to_string())
    } else {
        Ok(format!("Visible windows ({}):\n{}", windows.len(), windows.join("\n")))
    }
}

/// List visible windows (macOS — uses AppleScript via System Events)
#[cfg(target_os = "macos")]
fn list_visible_windows() -> Result<String, String> {
    // AppleScript to get window names, positions, and sizes from all visible apps
    let script = r#"
        set output to ""
        tell application "System Events"
            set visibleProcesses to every process whose visible is true
            repeat with proc in visibleProcesses
                set procName to name of proc
                try
                    set wins to every window of proc
                    repeat with win in wins
                        set winName to name of win
                        set winPos to position of win
                        set winSize to size of win
                        set output to output & "\"" & procName & " - " & winName & "\" - Position: (" & (item 1 of winPos) & ", " & (item 2 of winPos) & "), Size: " & (item 1 of winSize) & "x" & (item 2 of winSize) & linefeed
                    end repeat
                end try
            end repeat
        end tell
        return output
    "#;

    match std::process::Command::new("osascript").args(["-e", script]).output() {
        Ok(output) => {
            if output.status.success() {
                let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if text.is_empty() {
                    Ok("No visible windows found.".to_string())
                } else {
                    let line_count = text.lines().count();
                    Ok(format!("Visible windows ({}):\n{}", line_count, text))
                }
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                if stderr.contains("not allowed") || stderr.contains("assistive") {
                    Err("Window listing requires Automation permission. Grant access in System Settings > Privacy > Automation.".to_string())
                } else {
                    Err(format!("AppleScript failed: {}", stderr.trim()))
                }
            }
        }
        Err(e) => Err(format!("Failed to run osascript: {}", e)),
    }
}

/// List visible windows (Linux — uses wmctrl)
#[cfg(target_os = "linux")]
fn list_visible_windows() -> Result<String, String> {
    match std::process::Command::new("wmctrl").arg("-l").output() {
        Ok(output) => {
            if output.status.success() {
                Ok(String::from_utf8_lossy(&output.stdout).to_string())
            } else {
                Err("wmctrl failed. Install wmctrl for window listing on Linux.".to_string())
            }
        }
        Err(_) => Err("Window listing not available. Install wmctrl: sudo apt install wmctrl".to_string()),
    }
}

// ==================== Main ====================

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--hook") {
        run_hook();
        return;
    }
    if args.iter().any(|a| a == "--stop-hook") {
        run_stop_hook();
        return;
    }
    if args.iter().any(|a| a == "--keep-alive") {
        run_keep_alive();
        return;
    }
    if args.iter().any(|a| a == "--supervise") {
        let project_dir = args.iter().position(|a| a == "--project-dir")
            .and_then(|i| args.get(i + 1))
            .cloned()
            .or_else(|| std::env::var("VAAK_PROJECT_DIR").ok())
            .unwrap_or_else(|| std::env::current_dir()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default());
        run_supervise(&project_dir);
        return;
    }

    let session_id = get_session_id();
    eprintln!("[vaak-mcp] Session ID: {}", session_id);

    cache_session_id(&session_id);

    // Windows console control handler: catches terminal close/Ctrl+C and writes "disconnected"
    // before the OS kills the process. This enables instant disconnect detection in the UI.
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Console::SetConsoleCtrlHandler;

        unsafe extern "system" fn ctrl_handler(_ctrl_type: u32) -> i32 {
            // Runs on any console event: Ctrl+C (0), Ctrl+Close (2), Ctrl+Shutdown (6)
            update_session_activity("disconnected");
            eprintln!("[vaak-mcp] Console event caught, marked as disconnected");
            0 // Return FALSE to let default handler (process termination) run
        }

        unsafe { SetConsoleCtrlHandler(Some(ctrl_handler), 1); }
    }

    // Parent process monitor thread (Windows): watches Claude Code's process handle.
    // When the parent dies (for ANY reason — clean exit, TerminateProcess, crash),
    // the handle becomes signaled and we write "disconnected" immediately.
    // This is more reliable than console events, which don't fire on TerminateProcess.
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Threading::{OpenProcess, WaitForSingleObject};
        use windows_sys::Win32::Foundation::CloseHandle;

        const SYNCHRONIZE: u32 = 0x00100000;
        const INFINITE: u32 = 0xFFFFFFFF;
        const WAIT_OBJECT_0: u32 = 0;

        if let Some(ppid) = get_parent_pid() {
            let handle = unsafe { OpenProcess(SYNCHRONIZE, 0, ppid) };
            if !handle.is_null() {
                // SAFETY: Windows HANDLEs are process-wide and safe to use from any thread.
                let handle_usize = handle as usize;
                eprintln!("[vaak-mcp] Monitoring parent process {} for exit", ppid);
                std::thread::spawn(move || {
                    let h = handle_usize as *mut std::ffi::c_void;
                    let result = unsafe { WaitForSingleObject(h, INFINITE) };
                    unsafe { CloseHandle(h); }
                    if result == WAIT_OBJECT_0 {
                        eprintln!("[vaak-mcp] Parent process {} died, marking disconnected", ppid);
                        update_session_activity("disconnected");
                        std::process::exit(0);
                    }
                });
            } else {
                eprintln!("[vaak-mcp] Could not open parent process {} for monitoring", ppid);
            }
        }
    }

    // Parent process monitor thread (Unix/macOS): polls getppid() every 2 seconds.
    // When the parent dies, the OS re-parents this process to PID 1 (init/launchd),
    // so a changed ppid means the parent is gone. Near-zero CPU/memory cost.
    #[cfg(unix)]
    {
        let original_ppid = unsafe { libc::getppid() };
        if original_ppid > 1 {
            eprintln!("[vaak-mcp] Monitoring parent process {} for exit (Unix)", original_ppid);
            std::thread::spawn(move || {
                loop {
                    std::thread::sleep(std::time::Duration::from_secs(2));
                    let current_ppid = unsafe { libc::getppid() };
                    if current_ppid != original_ppid {
                        eprintln!("[vaak-mcp] Parent process {} died (now {}), marking disconnected", original_ppid, current_ppid);
                        update_session_activity("disconnected");
                        std::process::exit(0);
                    }
                }
            });
        }
    }

    // Background heartbeat thread: keeps session alive as long as the MCP process is running.
    // Sends heartbeat every 30 seconds when an active project is joined.
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(std::time::Duration::from_secs(30));
            // Only heartbeat if we've joined a project
            let has_project = ACTIVE_PROJECT.lock()
                .map(|guard| guard.is_some())
                .unwrap_or(false);
            if has_project {
                update_session_heartbeat_in_file();
            }
        }
    });

    let stdin = io::stdin();
    let mut stdout = io::stdout();

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                log_sidecar_event("stdin_error", serde_json::json!({ "error": e.to_string() }));
                break;
            }
        };

        if line.trim().is_empty() {
            continue;
        }

        let request: serde_json::Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                let error_response = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": null,
                    "error": {
                        "code": -32700,
                        "message": format!("Parse error: {}", e)
                    }
                });
                let _ = writeln!(stdout, "{}", error_response);
                let _ = stdout.flush();
                continue;
            }
        };

        // feature/watchdog-rpc-liveness Signal A (developer:1 msg 1286
        // Finding 1 + evil-arch msg 1201): refresh per-seat last_alive_at_ms
        // on every inbound MCP RPC. The keep-alive PreToolUse/PostToolUse
        // hooks already cover Claude Code tool calls, but pure project_wait
        // standby windows don't fire those hooks — the seat goes silent from
        // the watchdog's perspective even while the harness is actively
        // dispatching RPCs through this loop. Bumping last_alive_at_ms here
        // closes that gap so check_assembly_floor_watchdog's heartbeat_fresh
        // gate (main.rs:5034) correctly suppresses false floor_stall releases.
        // Fail-open per update_seat_alive_at_ms's own discipline.
        if let Ok(guard) = ACTIVE_PROJECT.lock() {
            if let Some(state) = guard.as_ref() {
                update_seat_alive_at_ms(&state.project_dir, &state.role, state.instance);
            }
        }

        if let Some(response) = handle_request(&request, &session_id) {
            let _ = writeln!(stdout, "{}", response);
            let _ = stdout.flush();
        }
    }

    // Cleanup: mark session as disconnected so the UI shows "gone" immediately
    update_session_activity("disconnected");
    log_sidecar_event("stdin_eof", serde_json::json!({ "reason": "main_loop_exited" }));
    eprintln!("[vaak-mcp] Session ended, marked as disconnected");
}
