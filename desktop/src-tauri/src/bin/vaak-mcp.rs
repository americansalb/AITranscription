//! Vaak MCP Server - Bridges Claude Code to Vaak for voice output
//!
//! This is a minimal MCP (Model Context Protocol) server that provides a `speak` tool
//! for Claude Code to send text-to-speech requests to the Vaak desktop app.
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

// Import typed DiscussionState from the library crate — enables typed reads/writes
// instead of raw serde_json::Value manipulation. Migration is incremental:
// new code uses typed functions, old code still works via JSON.
use vaak_desktop::collab::{self, DiscussionState};
use vaak_desktop::build_info;

/// Atomic file write. On Windows, writes directly (rename over open files fails).
/// On Unix, uses tmp+rename for atomicity. All callers use file locking for concurrency.
fn atomic_write(path: &Path, content: &[u8]) -> Result<(), String> {
    #[cfg(windows)]
    {
        std::fs::write(path, content)
            .map_err(|e| format!("Failed to write {}: {}", path.display(), e))?;
        if let Ok(f) = std::fs::File::open(path) {
            let _ = f.sync_all();
        }
        return Ok(());
    }

    #[cfg(not(windows))]
    {
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

/// One-shot guard for the background heartbeat ticker. The ticker is spawned
/// the first time `ensure_heartbeat_ticker_started` is called (typically from
/// `handle_project_join`) and lives until the sidecar process exits. There is
/// no stop signal — process death is the stop signal, which is correct for a
/// per-process daemon thread.
///
/// Why a global atomic instead of per-session state: ACTIVE_PROJECT may be
/// re-bound when a session rejoins (`get_or_rejoin_state`), but the OS
/// process is the same. Spawning a fresh thread on every rejoin would leak
/// threads. One thread per sidecar process, reading whatever ACTIVE_PROJECT
/// currently holds.
static HEARTBEAT_TICKER_STARTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Spawn the background heartbeat ticker if not already running.
///
/// Per architect msg 539 + platform-engineer msg 534 (pr-sidecar-heartbeat-thread):
/// before this PR, heartbeats only fired on `project_wait` poll iterations,
/// `project_send`, and `project_claim`. An agent doing several minutes of
/// `Bash`/`Read`/`Grep` tool-calls between message-sends would age past the
/// pipeline-stage watchdog (DEFAULT_PIPELINE_STAGE_TIMEOUT_SECS) and get
/// auto-skipped while alive.
///
/// This thread is the agent's continuous liveness signal — every 30 seconds
/// it bumps `last_heartbeat` regardless of what the agent is doing. The
/// watchdog now reflects "process alive" rather than "process recently sent
/// a message." Decoupling is the architectural fix.
///
/// Cadence: 30s — paired with the 360s watchdog default (post-stopgap),
/// gives 12 ticks of margin before staleness; small enough that sessions.json
/// write contention stays negligible.
fn ensure_heartbeat_ticker_started() {
    use std::sync::atomic::Ordering;
    if HEARTBEAT_TICKER_STARTED.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_err() {
        return; // Already running
    }
    std::thread::spawn(|| {
        let interval = std::time::Duration::from_secs(30);
        loop {
            std::thread::sleep(interval);
            // No-op if no active project bound yet (process started but not joined).
            let bound = ACTIVE_PROJECT.lock().ok().and_then(|g| g.as_ref().cloned());
            if let Some(state) = bound {
                update_session_heartbeat_in_file();
                check_pipeline_ack_timeout_and_skip(&state.project_dir);
            }
        }
    });
}

/// Pipeline turn-ack watchdog (pr-pipeline-turn-ack-p2). If the current pipeline
/// stage's assigned role has not broadcast within the configured timeout of
/// being notified, force-advance past them. Prevents the "1hr silent agent
/// stalls everyone" failure mode the human flagged in msg 1111.
///
/// Timeout source: project.json > settings > pipeline_ack_timeout_secs (default 30).
/// Tester msg 1136 flagged that 30s is aggressive for stages that take minutes
/// to compose (e.g. an 8000-word architect output). Per-project tunable.
const PIPELINE_ACK_TIMEOUT_DEFAULT: i64 = 30;
fn pipeline_ack_timeout_secs(project_dir: &str) -> i64 {
    std::fs::read_to_string(project_json_path(project_dir)).ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.get("settings")?.get("pipeline_ack_timeout_secs")?.as_i64())
        .filter(|&t| t > 0 && t <= 3600)
        .unwrap_or(PIPELINE_ACK_TIMEOUT_DEFAULT)
}
fn check_pipeline_ack_timeout_and_skip(project_dir: &str) {
    let disc = read_discussion_state_raw(project_dir);
    if disc.get("active").and_then(|v| v.as_bool()) != Some(true) { return; }
    if disc.get("mode").and_then(|v| v.as_str()) != Some("pipeline") { return; }
    if disc.get("paused_at").and_then(|v| v.as_str()).is_some() { return; }
    let started_at_str = match disc.get("pipeline_stage_started_at").and_then(|v| v.as_str()) {
        Some(s) => s, None => return,
    };
    let started_at = match chrono_iso_to_unix(started_at_str) { Some(t) => t, None => return };
    let elapsed = utc_now_unix() - started_at;
    let timeout = pipeline_ack_timeout_secs(project_dir);
    if elapsed < timeout { return; }
    let pipeline_order = match disc.get("pipeline_order").and_then(|v| v.as_array()) {
        Some(a) => a.clone(), None => return,
    };
    let current_stage = disc.get("pipeline_stage").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
    let current_role = match pipeline_order.get(current_stage).and_then(|v| v.as_str()) {
        Some(r) => r.to_string(), None => return,
    };
    if board_has_message_from_since(project_dir, &current_role, started_at_str) { return; }
    // Force-advance past the silent role.
    let next_stage = current_stage + 1;
    let mut updated = disc.clone();
    if next_stage >= pipeline_order.len() {
        // End of round — skipped role was the last. Loop or complete handled by the
        // next broadcast path; for the watchdog, just bump stage and post a system msg.
        updated["pipeline_stage"] = serde_json::json!(next_stage);
        updated["pipeline_stage_started_at"] = serde_json::json!(utc_now_iso());
    } else {
        updated["pipeline_stage"] = serde_json::json!(next_stage);
        updated["pipeline_stage_started_at"] = serde_json::json!(utc_now_iso());
    }
    let _ = write_discussion_state(project_dir, &updated);
    post_turn_system_message(project_dir, &format!("{} did not respond within {}s — pipeline auto-advanced (skipped)", current_role, timeout));
    if next_stage < pipeline_order.len() {
        let next_agent = pipeline_order.get(next_stage).and_then(|v| v.as_str()).unwrap_or("?");
        let wake_msg = serde_json::json!({
            "id": next_message_id(project_dir),
            "from": "system:0",
            "to": next_agent,
            "type": "system",
            "subject": "Your pipeline stage",
            "body": format!("It is now your turn in the pipeline (stage {}/{}). Previous role was auto-skipped after {}s no response.", next_stage + 1, pipeline_order.len(), timeout),
            "timestamp": utc_now_iso(),
            "metadata": {"pipeline_notification": true, "auto_skip_predecessor": current_role}
        });
        let _ = append_to_board(project_dir, &wake_msg);
    }
}

fn read_discussion_state_raw(project_dir: &str) -> serde_json::Value {
    let path = vaak_dir(project_dir).join("sections").join(read_active_section(project_dir)).join("discussion.json");
    let path = if path.exists() { path } else { vaak_dir(project_dir).join("discussion.json") };
    std::fs::read_to_string(&path).ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| serde_json::json!({}))
}

fn read_active_section(project_dir: &str) -> String {
    let path = project_json_path(project_dir);
    std::fs::read_to_string(&path).ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.get("active_section").and_then(|s| s.as_str()).map(String::from))
        .unwrap_or_else(|| "default".to_string())
}

fn board_has_message_from_since(project_dir: &str, role: &str, since_iso: &str) -> bool {
    let since_unix = chrono_iso_to_unix(since_iso).unwrap_or(0);
    let path = vaak_dir(project_dir).join("sections").join(read_active_section(project_dir)).join("board.jsonl");
    let path = if path.exists() { path } else { vaak_dir(project_dir).join("board.jsonl") };
    let content = match std::fs::read_to_string(&path) { Ok(s) => s, Err(_) => return false };
    for line in content.lines() {
        if let Ok(msg) = serde_json::from_str::<serde_json::Value>(line) {
            let from = msg.get("from").and_then(|v| v.as_str()).unwrap_or("");
            if from != role { continue; }
            let ts = msg.get("timestamp").and_then(|v| v.as_str()).unwrap_or("");
            if chrono_iso_to_unix(ts).unwrap_or(0) > since_unix { return true; }
        }
    }
    false
}

fn chrono_iso_to_unix(iso: &str) -> Option<i64> {
    // Parse "2026-04-17T19:54:31Z" format minimally without chrono dep.
    let s = iso.trim_end_matches('Z');
    let parts: Vec<&str> = s.split(|c| c == 'T' || c == '-' || c == ':').collect();
    if parts.len() < 6 { return None; }
    let y: i64 = parts[0].parse().ok()?;
    let mo: i64 = parts[1].parse().ok()?;
    let d: i64 = parts[2].parse().ok()?;
    let h: i64 = parts[3].parse().ok()?;
    let mi: i64 = parts[4].parse().ok()?;
    let se: i64 = parts[5].parse().ok()?;
    // Days since epoch using Howard Hinnant's algorithm.
    let y = if mo <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y / 400 } else { (y - 399) / 400 };
    let yoe = y - era * 400;
    let doy = (153 * (if mo > 2 { mo - 3 } else { mo + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    Some(days * 86400 + h * 3600 + mi * 60 + se)
}

fn utc_now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

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

fn turn_state_path(project_dir: &str) -> PathBuf {
    vaak_dir(project_dir).join("turn_state.json")
}

/// Read turn_state.json for consecutive mode turn tracking
fn read_turn_state(project_dir: &str) -> serde_json::Value {
    let path = turn_state_path(project_dir);
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(serde_json::json!({"completed": true}))
}

/// Write turn_state.json atomically
fn write_turn_state(project_dir: &str, state: &serde_json::Value) -> Result<(), String> {
    let path = turn_state_path(project_dir);
    let content = serde_json::to_string_pretty(state)
        .map_err(|e| format!("Failed to serialize turn state: {}", e))?;
    atomic_write(&path, content.as_bytes())
}

/// Build a relevance-scored pipeline order. Delegates to the library crate's
/// collab::build_pipeline_order — single source of truth, no more duplication.
fn build_pipeline_order(project_dir: &str, topic: &str, participants: &[String]) -> Vec<String> {
    collab::build_pipeline_order(project_dir, topic, participants)
}

/// Shuffle a pipeline order using Fisher-Yates with cryptographic entropy.
fn shuffle_pipeline_order(order: &mut Vec<String>) {
    if order.len() <= 1 { return; }
    let mut rng_state = uuid::Uuid::new_v4().as_u128();
    for i in (1..order.len()).rev() {
        rng_state ^= rng_state << 13;
        rng_state ^= rng_state >> 7;
        rng_state ^= rng_state << 17;
        let j = (rng_state as usize) % (i + 1);
        order.swap(i, j);
    }
}

/// Get all active non-human, non-manager agents as "role:instance" strings.
/// Used by consecutive mode where there's no explicit participant list.
fn get_all_active_agents(project_dir: &str) -> Vec<String> {
    let sessions = read_sessions(project_dir);
    let bindings = sessions.get("bindings")
        .and_then(|b| b.as_array())
        .cloned()
        .unwrap_or_default();
    bindings.iter()
        .filter(|b| b.get("status").and_then(|s| s.as_str()) == Some("active"))
        .filter(|b| {
            let r = b.get("role").and_then(|r| r.as_str()).unwrap_or("");
            r != "human" && r != "manager"
        })
        .filter_map(|b| {
            let role = b.get("role")?.as_str()?.to_string();
            let instance = b.get("instance")?.as_u64().unwrap_or(0);
            Some(format!("{}:{}", role, instance))
        })
        .collect()
}

/// Initialize or reset turn state for a new broadcast message in consecutive mode.
/// Uses relevance scoring to determine turn order based on message content.
fn reset_turn_state(project_dir: &str, trigger_msg_id: u64, trigger_text: &str) -> Result<(), String> {
    // For consecutive mode, include ALL active agents (no participant filter)
    let all_active = get_all_active_agents(project_dir);
    let relevance_order = build_pipeline_order(project_dir, trigger_text, &all_active);
    let first = relevance_order.first().cloned().unwrap_or_default();
    let state = serde_json::json!({
        "trigger_msg_id": trigger_msg_id,
        "relevance_order": relevance_order,
        "current_index": 0,
        "responded": [],
        "passed": [],
        "completed": false,
        "started_at": utc_now_iso()
    });
    let result = write_turn_state(project_dir, &state);
    if result.is_ok() && !first.is_empty() {
        post_turn_system_message(project_dir, &format!("Turn started: {}", first));
        // Send directed notification to wake the first agent from project_wait
        // Use full role:instance so only the specific instance is notified
        let wake_msg = serde_json::json!({
            "id": next_message_id(project_dir),
            "from": "system:0",
            "to": first,
            "type": "system",
            "subject": "Your turn",
            "body": format!("It is now your turn to respond. Broadcast your response to all."),
            "timestamp": utc_now_iso(),
            "metadata": {"turn_notification": true}
        });
        let _ = append_to_board(project_dir, &wake_msg);
    }
    result
}

/// Advance the turn to the next agent. Called after an agent responds or passes.
fn advance_turn(project_dir: &str, agent_label: &str, passed: bool) -> Result<(), String> {
    with_file_lock(project_dir, || {
        let mut state = read_turn_state(project_dir);
        if state.get("completed").and_then(|c| c.as_bool()).unwrap_or(true) {
            return Ok(());
        }

        // Record this agent's action
        let list_key = if passed { "passed" } else { "responded" };
        if let Some(arr) = state.get_mut(list_key).and_then(|a| a.as_array_mut()) {
            if !arr.iter().any(|v| v.as_str() == Some(agent_label)) {
                arr.push(serde_json::json!(agent_label));
            }
        }

        // Track consecutive timeouts per agent for auto-pruning
        let timeout_counts = state.get("timeout_counts").cloned().unwrap_or(serde_json::json!({}));
        let mut tc = timeout_counts.as_object().cloned().unwrap_or_default();
        if passed {
            let count = tc.get(agent_label).and_then(|v| v.as_u64()).unwrap_or(0);
            tc.insert(agent_label.to_string(), serde_json::json!(count + 1));
        } else {
            tc.remove(agent_label);
        }
        state["timeout_counts"] = serde_json::json!(tc);

        // Advance current_index past agents that have already responded or passed
        let relevance_order = state.get("relevance_order")
            .and_then(|t| t.as_array())
            .cloned()
            .unwrap_or_default();
        let responded: Vec<String> = state.get("responded")
            .and_then(|r| r.as_array())
            .map(|a| a.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
            .unwrap_or_default();
        let passed_list: Vec<String> = state.get("passed")
            .and_then(|p| p.as_array())
            .map(|a| a.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
            .unwrap_or_default();

        let mut idx = state.get("current_index").and_then(|i| i.as_u64()).unwrap_or(0) as usize;
        let mut completed = false;
        // Move to next unhandled agent
        loop {
            idx += 1;
            if idx >= relevance_order.len() {
                state["completed"] = serde_json::json!(true);
                completed = true;
                break;
            }
            let next = relevance_order[idx].as_str().unwrap_or("");
            // Skip agents with 2+ consecutive timeouts (auto-pruned)
            let next_timeouts = tc.get(next).and_then(|v| v.as_u64()).unwrap_or(0);
            if next_timeouts >= 2 {
                // Auto-mark as passed
                if let Some(arr) = state.get_mut("passed").and_then(|a| a.as_array_mut()) {
                    if !arr.iter().any(|v| v.as_str() == Some(next)) {
                        arr.push(serde_json::json!(next));
                    }
                }
                continue;
            }
            if !responded.contains(&next.to_string()) && !passed_list.contains(&next.to_string()) {
                break;
            }
        }
        state["current_index"] = serde_json::json!(idx);
        // Update started_at for timeout tracking of the new turn holder
        state["turn_started_at"] = serde_json::json!(utc_now_iso());

        let result = write_turn_state(project_dir, &state);

        // Post system message about turn change and notify next agent
        if result.is_ok() {
            if completed {
                post_turn_system_message(project_dir, "Round complete");
            } else if let Some(next_agent) = relevance_order.get(idx).and_then(|v| v.as_str()) {
                let action = if passed { "passed" } else { "responded" };
                post_turn_system_message(project_dir, &format!("{} {} — turn: {}", agent_label, action, next_agent));
                // Send directed notification to wake the next agent from project_wait
                // Use full role:instance so only the specific instance is notified
                let wake_msg = serde_json::json!({
                    "id": next_message_id(project_dir),
                    "from": "system:0",
                    "to": next_agent,
                    "type": "system",
                    "subject": "Your turn",
                    "body": format!("It is now your turn to respond. Broadcast your response to all."),
                    "timestamp": utc_now_iso(),
                    "metadata": {"turn_notification": true}
                });
                let _ = append_to_board(project_dir, &wake_msg);
            }
        }
        result
    })
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

fn last_seen_path(project_dir: &str, session_id: &str) -> PathBuf {
    let safe_id = session_id.replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "_");
    vaak_dir(project_dir).join("last-seen").join(format!("{}.json", safe_id))
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

    // Auto-switch to the new section so old messages don't persist
    config["active_section"] = serde_json::json!(slug);

    let content = serde_json::to_string_pretty(&config)
        .map_err(|e| format!("Failed to serialize project.json: {}", e))?;
    atomic_write(&config_path, content.as_bytes())
        .map_err(|e| format!("Failed to write project.json: {}", e))?;

    eprintln!("[sections] Created and switched to section '{}' (slug: {})", name, slug);

    Ok(serde_json::json!({
        "status": "created",
        "slug": slug,
        "name": name,
        "switched": true
    }))
}

/// Switch active section for the current session.
fn handle_switch_section(project_dir: &str, slug: &str) -> Result<serde_json::Value, String> {
    // Validate slug to prevent path traversal (e.g., "../../.." escaping .vaak/)
    if slug != "default" {
        if slug.is_empty() {
            return Err("Section slug cannot be empty".to_string());
        }
        if !slug.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') {
            return Err("Section slug must be lowercase alphanumeric with hyphens only".to_string());
        }
        if slug.starts_with('-') || slug.ends_with('-') {
            return Err("Section slug cannot start or end with a hyphen".to_string());
        }
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
    let max_id: u64 = std::fs::read_to_string(&path)
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter_map(|m| m.get("id").and_then(|i| i.as_u64()))
        .max()
        .unwrap_or(0);
    max_id + 1
}

/// Write a system message to board.jsonl for turn changes
fn post_turn_system_message(project_dir: &str, body: &str) {
    let msg = serde_json::json!({
        "id": next_message_id(project_dir),
        "from": "system:0",
        "to": "all",
        "type": "system",
        "subject": "",
        "body": body,
        "timestamp": utc_now_iso(),
        "metadata": {}
    });
    let _ = append_to_board(project_dir, &msg);
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

    // Moderator-exit auto-pause (dev-challenger msg 172 attack 3, tech-leader msg 292).
    // Why: if the moderator's session terminates mid-discussion (process dies, terminal
    // closes, parent-process monitor triggers), the discussion is left without a
    // moderator but still "active." The next auto-advance / consensus check would
    // still fire, pretending the moderator is there. Pausing on disconnect freezes
    // state cleanly; the PR A consensus detector defers while `paused_at.is_some()`.
    // Runs OUTSIDE the lock above to avoid re-entry — pause_if_current_session_is_moderator
    // re-acquires the lock itself.
    if activity == "disconnected" {
        pause_if_current_session_is_moderator(&state.project_dir, &state.session_id, &state.role, state.instance);
    }
}

/// Pause the active discussion if the given session is its moderator.
///
/// Why: see update_session_activity comment. Ensures moderator-exit during an active
/// session leaves the discussion in a recoverable paused state rather than an
/// ambiguously-moderated active state.
///
/// Semantics: reads discussion state, compares moderator field to the exiting
/// session's `role:instance` label, sets `paused_at = now` + `paused_reason =
/// "moderator_exit"` if they match. No-op otherwise. Does not auto-resume; the
/// next moderator claim must call `resume` explicitly.
fn pause_if_current_session_is_moderator(project_dir: &str, _session_id: &str, role: &str, instance: u32) {
    let label = format!("{}:{}", role, instance);
    let _ = with_file_lock(project_dir, || {
        let disc = read_discussion_state(project_dir);
        let is_active = disc.get("active").and_then(|v| v.as_bool()).unwrap_or(false);
        if !is_active {
            return Ok(());
        }
        let moderator = disc.get("moderator").and_then(|m| m.as_str()).unwrap_or("");
        if moderator != label {
            return Ok(());
        }
        let already_paused = disc.get("paused_at").and_then(|v| v.as_str()).is_some();
        if already_paused {
            return Ok(());
        }
        let now = utc_now_iso();
        let mut updated = disc.clone();
        updated["paused_at"] = serde_json::json!(now);
        updated["paused_reason"] = serde_json::json!("moderator_exit");
        write_discussion_state(project_dir, &updated)?;

        // Emit system message so the team and UI see the pause.
        let msg_id = next_message_id(project_dir);
        let announcement = serde_json::json!({
            "id": msg_id,
            "from": "system:0",
            "to": "all",
            "type": "system",
            "timestamp": now,
            "subject": "Discussion auto-paused: moderator exited",
            "body": format!("The moderator ({}) disconnected. Discussion is paused until a moderator resumes or rejoins.", label),
            "metadata": {
                "paused_reason": "moderator_exit",
                "exited_moderator": label
            }
        });
        append_to_board(project_dir, &announcement)?;
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
fn read_claims_filtered(project_dir: &str) -> serde_json::Value {
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

    let mut clean = serde_json::Map::new();
    let mut any_removed = false;

    for (key, val) in claims_obj {
        let session_id = val.get("session_id").and_then(|s| s.as_str()).unwrap_or("");
        let binding = bindings.iter().find(|b| {
            b.get("session_id").and_then(|s| s.as_str()) == Some(session_id)
        });
        let is_stale = match binding {
            None => true,
            Some(b) => {
                let hb = b.get("last_heartbeat").and_then(|h| h.as_str()).unwrap_or("");
                let age = parse_iso_to_epoch_secs(hb)
                    .map(|hb_secs| now_secs.saturating_sub(hb_secs))
                    .unwrap_or(u64::MAX);
                age > gone_threshold
            }
        };
        if is_stale {
            any_removed = true;
        } else {
            clean.insert(key.clone(), val.clone());
        }
    }

    let result = serde_json::Value::Object(clean);
    if any_removed {
        let _ = write_claims(project_dir, &result);
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

/// Read discussion state as a TYPED struct.
/// Use this for new code paths. Returns Default (inactive) if file is missing or unparseable.
fn read_discussion_typed(project_dir: &str) -> DiscussionState {
    std::fs::read_to_string(discussion_json_path(project_dir))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Write discussion state from a TYPED struct.
/// Use this for new code paths. Acquires file lock.
fn write_discussion_typed(project_dir: &str, state: &DiscussionState) -> Result<(), String> {
    let content = serde_json::to_string_pretty(state)
        .map_err(|e| format!("Failed to serialize discussion state: {}", e))?;
    let pd = project_dir.to_string();
    with_discussion_lock(project_dir, move || {
        let path = discussion_json_path(&pd);
        atomic_write(&path, content.as_bytes())
    })
}

/// LEGACY: Read discussion state as raw JSON Value.
/// Prefer read_discussion_typed for new code paths.
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
            "audience_state": "listening",
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

/// Default seconds before a pipeline stage's holder is considered stale and
/// auto-skipped by the watchdog. Configurable via project.json
/// `settings.pipeline_stage_timeout_secs`.
///
/// History:
///   - 180s (initial): chosen to fire earlier than heartbeat_timeout_seconds
///     (300s default) so the pipeline keeps flowing before an agent is fully
///     declared dead.
///   - 360s (pr-watchdog-timeout-stopgap, per tech-leader msg 540 +
///     platform-engineer msg 534 Flag-1): bumped to 2× heartbeat_timeout
///     because heartbeats were event-driven (only fired on project_wait/
///     send/claim — see `ensure_heartbeat_ticker_started` for the proper
///     fix). An agent doing 4-5 minutes of Bash/Read/Grep tool-calls
///     between message-sends would age past 180s and get skipped while
///     alive. Stopgap until all sidecar binaries are rebuilt with the
///     pr-sidecar-heartbeat-thread fix and existing agents respawn.
///
/// After all agents run the heartbeat-thread version, this can drop back
/// to 180s in a future cleanup.
const DEFAULT_PIPELINE_STAGE_TIMEOUT_SECS: u64 = 360;

/// Read the pipeline-stage timeout from project.json with fallback to default.
fn pipeline_stage_timeout_secs(project_dir: &str) -> u64 {
    let cfg_path = vaak_dir(project_dir).join("project.json");
    std::fs::read_to_string(&cfg_path).ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|j| j.get("settings")?.get("pipeline_stage_timeout_secs")?.as_u64())
        .unwrap_or(DEFAULT_PIPELINE_STAGE_TIMEOUT_SECS)
}

/// Pipeline auto-skip watchdog: if the current stage holder has not heartbeated
/// in `timeout_secs`, advance the stage and post a system message naming what
/// was skipped and why.
///
/// Why this exists (human msg 511 ask #4): when an agent's terminal closes
/// mid-pipeline, the stage holder never broadcasts, and the existing
/// broadcast-driven auto-advance never fires. Result: pipeline stalls
/// indefinitely until human intervention. tech-leader:1 holding stage 7 of
/// the prior round-1 pipeline for >2 hours triggered this work. Watchdog
/// runs in every active agent's `project_wait` loop — first agent to detect
/// the staleness wins, others see updated state on next tick.
///
/// Returns Ok(true) if a skip was performed, Ok(false) otherwise.
fn auto_skip_stale_pipeline_stage(project_dir: &str, timeout_secs: u64) -> Result<bool, String> {
    let disc = read_discussion_state(project_dir);
    let active = disc.get("active").and_then(|v| v.as_bool()).unwrap_or(false);
    let mode = disc.get("mode").and_then(|v| v.as_str()).unwrap_or("");
    if !active || mode != "pipeline" {
        return Ok(false);
    }
    if disc.get("paused_at").and_then(|v| v.as_str()).is_some() {
        return Ok(false);
    }

    let pipeline_order: Vec<String> = disc.get("pipeline_order")
        .and_then(|o| o.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();
    let current_stage = disc.get("pipeline_stage").and_then(|s| s.as_u64()).unwrap_or(0) as usize;
    if current_stage >= pipeline_order.len() {
        return Ok(false);
    }

    let stage_holder = match pipeline_order.get(current_stage) {
        Some(s) if !s.is_empty() => s.clone(),
        _ => return Ok(false),
    };

    // Find the stage holder's last_heartbeat
    let sessions = read_sessions(project_dir);
    let bindings = sessions.get("bindings").and_then(|b| b.as_array()).cloned().unwrap_or_default();
    let stage_session = bindings.iter().find(|b| {
        let role = b.get("role").and_then(|r| r.as_str()).unwrap_or("");
        let inst = b.get("instance").and_then(|i| i.as_u64()).unwrap_or(0);
        format!("{}:{}", role, inst) == stage_holder
    });

    let last_hb = stage_session
        .and_then(|s| s.get("last_heartbeat").and_then(|h| h.as_str()))
        .and_then(parse_iso_to_epoch_secs);
    let now = parse_iso_to_epoch_secs(&utc_now_iso()).unwrap_or(0);

    let is_stale = match last_hb {
        Some(hb) => now.saturating_sub(hb) > timeout_secs,
        None => true, // No session record at all = stale
    };
    if !is_stale {
        return Ok(false);
    }

    // Acquire the discussion lock and re-verify (TOCTOU): another watchdog in
    // a sibling agent's project_wait may have already advanced. Skip-update
    // happens inside the lock so writes are serialized.
    let project_dir_owned = project_dir.to_string();
    let stage_holder_inner = stage_holder.clone();
    let advanced_stage: Option<usize> = with_discussion_lock(project_dir, move || -> Result<Option<usize>, String> {
        let disc2 = read_discussion_state(&project_dir_owned);
        let cur2 = disc2.get("pipeline_stage").and_then(|s| s.as_u64()).unwrap_or(0) as usize;
        if cur2 != current_stage {
            return Ok(None); // Already advanced by another watchdog or by the agent itself
        }

        let mut outputs = disc2.get("pipeline_outputs").and_then(|o| o.as_array()).cloned().unwrap_or_default();
        outputs.push(serde_json::json!({
            "stage": current_stage,
            "agent": stage_holder_inner,
            "skipped": true,
            "skip_reason": format!("stale: no heartbeat for >{}s", timeout_secs),
            "timestamp": utc_now_iso(),
        }));

        let mut updated = disc2.clone();
        updated["pipeline_outputs"] = serde_json::json!(outputs);
        updated["pipeline_stage"] = serde_json::json!(current_stage + 1);
        write_discussion_state_unlocked(&project_dir_owned, &updated)?;
        Ok(Some(current_stage + 1))
    })?;

    let next_stage = match advanced_stage {
        Some(s) => s,
        None => return Ok(false),
    };

    // Post system messages outside the discussion lock — board has its own lock chain.
    let skip_msg = serde_json::json!({
        "id": next_message_id(project_dir),
        "from": "system:0",
        "to": "all",
        "type": "system",
        "subject": format!("Stage {} auto-skipped: {} stale", current_stage + 1, stage_holder),
        "body": format!(
            "Pipeline stage {} ({}) auto-skipped — no heartbeat in >{}s. Pipeline advancing to stage {}/{}.",
            current_stage + 1, stage_holder, timeout_secs, next_stage + 1, pipeline_order.len()
        ),
        "timestamp": utc_now_iso(),
        "metadata": {
            "pipeline_auto_skip": true,
            "skipped_agent": stage_holder,
            "skipped_stage": current_stage,
            "timeout_secs": timeout_secs
        }
    });
    let _ = append_to_board(project_dir, &skip_msg);

    if next_stage < pipeline_order.len() {
        let next_agent = pipeline_order[next_stage].clone();
        let wake_msg = serde_json::json!({
            "id": next_message_id(project_dir),
            "from": "system:0",
            "to": next_agent,
            "type": "system",
            "subject": "Your pipeline stage",
            "body": format!(
                "It is now your turn in the pipeline (stage {}/{}). The previous stage was auto-skipped due to a stale agent. Review previous outputs and broadcast your response.",
                next_stage + 1, pipeline_order.len()
            ),
            "timestamp": utc_now_iso(),
            "metadata": {"pipeline_notification": true, "preceded_by_skip": true}
        });
        let _ = append_to_board(project_dir, &wake_msg);
    } else {
        // Auto-skip pushed past the final stage — emit completion notice so
        // the round can close on the next tick. Don't toggle active here;
        // round-end logic is owned by the broadcast handler.
        let done_msg = serde_json::json!({
            "id": next_message_id(project_dir),
            "from": "system:0",
            "to": "all",
            "type": "system",
            "subject": "Pipeline round ended via auto-skip",
            "body": format!("Final stage ({}) was auto-skipped — round considered complete. Next round / consensus check pending.", stage_holder),
            "timestamp": utc_now_iso(),
            "metadata": {"pipeline_auto_skip": true, "round_ended_via_skip": true}
        });
        let _ = append_to_board(project_dir, &done_msg);
    }

    Ok(true)
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

/// Returns true if every participant in the pipeline has an `accept` vote
/// in the most-recently-completed round's messages.
///
/// Why: human msg 145 identified "pipeline got stuck and wouldn't end on its own"
/// as the top pain. The existing stagnation detector at vaak-mcp.rs:4108+ uses
/// body-length (<100 chars) as the signal, which silently misses verbose
/// stall-close messages like the ones we produced in msgs 89/92/95/98/101/104/107.
/// A metadata-based consensus detector catches unanimity directly.
///
/// Semantics locked in developer msg 155:
/// - Snapshot: latest vote per participant in the round wins over earlier votes
/// - Round-boundary: single round only, no cross-round accumulation
/// - Counts every participant in pipeline_order (no synthesizer exclusion per
///   tech-leader msg 200 scope cut — option (a) from dev-challenger msg 151)
/// - Reads metadata keys `vote` and `synthesis_vote_reaffirmed.choice`; either satisfies
///   (both were in use during the stall-close rounds 4-5 of this very conversation)
/// - Empty participant list returns false (no consensus from nothing)
fn round_reached_consensus(
    participants: &[&str],
    round_message_ids: &[u64],
    board: &[serde_json::Value],
) -> bool {
    if participants.is_empty() {
        return false;
    }
    participants.iter().all(|participant| {
        // Latest-vote-wins: reverse scan the round's messages, find the most recent
        // vote-bearing message from this participant.
        let vote = round_message_ids.iter().rev().find_map(|msg_id| {
            let msg = board.iter().find(|m| m.get("id").and_then(|i| i.as_u64()) == Some(*msg_id))?;
            let from = msg.get("from").and_then(|f| f.as_str())?;
            if from != *participant { return None; }
            let meta = msg.get("metadata")?;
            meta.get("vote").and_then(|v| v.as_str())
                .or_else(|| meta.get("synthesis_vote_reaffirmed")
                    .and_then(|v| v.get("choice"))
                    .and_then(|c| c.as_str()))
                .map(|s| s.to_string())
        });
        vote.as_deref() == Some("accept")
    })
}

/// Validate that a moderator-issued action carries a sufficient audit reason.
///
/// Why: dev-challenger msg 172 flagged that unaudited privileged actions (end_discussion,
/// pipeline_next) let a moderator silently terminate or skip ahead without a trail.
/// Mandating a non-empty reason ≥3 chars after trim forces accountability at the API layer,
/// not at policy. Empty strings AND whitespace-only strings are both rejected per tester
/// msg 181 (gap test #3: `"    "` and `"n/a"` should not pass).
///
/// Returns Ok for non-high-risk actions (they bypass the check) and for valid reasons.
/// Returns Err(message) if a high-risk action is called with a reason too short after trim.
fn validate_moderator_reason(action: &str, reason: Option<&str>) -> Result<(), String> {
    const HIGH_RISK_ACTIONS: &[&str] = &["end_discussion", "pipeline_next"];
    if !HIGH_RISK_ACTIONS.contains(&action) {
        return Ok(());
    }
    let trimmed = reason.unwrap_or("").trim();
    if trimmed.len() < 3 {
        return Err(format!(
            "Action '{}' requires a non-empty reason (min 3 chars after trim). Moderator justification is recorded in the audit trail.",
            action
        ));
    }
    Ok(())
}

/// True if the sessions snapshot contains an active (or idle) `manager:N` binding.
///
/// Kept for completeness; NOT used by the moderator gate (which checks
/// `has_active_session_for_label` with the `discussion.moderator` field instead).
/// Architect msg 310 + vision § 11.4b separate moderator from manager: manager
/// has independent capabilities; the moderator-fallback invariant cares about
/// the moderator seat, not the manager seat.
fn has_active_manager_in_sessions(sessions: &serde_json::Value) -> bool {
    sessions.get("bindings")
        .and_then(|b| b.as_array())
        .map(|bindings| {
            bindings.iter().any(|b| {
                let role = b.get("role").and_then(|r| r.as_str()).unwrap_or("");
                let status = b.get("status").and_then(|s| s.as_str()).unwrap_or("");
                role == "manager" && (status == "active" || status == "idle")
            })
        })
        .unwrap_or(false)
}

/// True if the sessions snapshot contains an active (or idle) binding for the
/// given `role:instance` label (e.g. "moderator:0").
///
/// Why: architect msg 310 corrected my earlier item-1 code in PR 4 that checked
/// `has_active_manager_in_sessions` for the moderator gate. Vision § 11.4 says
/// the human-yields-to-claimed-moderator invariant cares about the *moderator* seat
/// (whoever is stored in `discussion.moderator`), not the manager role. Manager has
/// its own invariant (§ 11.4b) and does not gate moderator actions.
///
/// Semantics: parses "role:instance" label, returns true iff any binding matches
/// that exact role+instance with status in {active, idle}. Malformed labels or
/// bindings without an instance field return false.
fn has_active_session_for_label(sessions: &serde_json::Value, label: &str) -> bool {
    let (target_role, target_instance) = match label.split_once(':') {
        Some((r, i)) => (r, i),
        None => return false,
    };
    sessions.get("bindings")
        .and_then(|b| b.as_array())
        .map(|bindings| {
            bindings.iter().any(|b| {
                let role = b.get("role").and_then(|r| r.as_str()).unwrap_or("");
                let instance_str = b.get("instance")
                    .and_then(|i| i.as_u64())
                    .map(|n| n.to_string())
                    .unwrap_or_default();
                let status = b.get("status").and_then(|s| s.as_str()).unwrap_or("");
                role == target_role
                    && instance_str == target_instance
                    && (status == "active" || status == "idle")
            })
        })
        .unwrap_or(false)
}

/// Source of truth for moderator-action error codes.
///
/// Why an enum and not string-prefixed strings anymore: dev-challenger msg 358 +
/// architect msg 352 mandated a Rust↔TS drift-guard test (pr-t6). That test can't
/// pattern-match on free-form strings — it needs reflectable variant names.
///
/// `Display` preserves the exact `[error_code: CODE] k='v' k='v': reason` wire
/// format the UX side already parses (see `parseModeratorError` in
/// `desktop/src/lib/collabTypes.ts`). Do not change the rendering without also
/// updating the TS mirror and the two existing behavior tests below.
///
/// Adding a variant? Update `variant_tag()`, `Display`, and the TS union in the
/// same PR — `test_ts_types_match_rust_enum_variants` will fail otherwise.
#[derive(Debug)]
enum ModeratorError {
    /// A moderator capability is not valid in the active session format
    /// (e.g. `reorder_pipeline` invoked during a Delphi round).
    CapabilityNotSupportedForFormat {
        capability: String,
        format: String,
        reason: String,
    },
    /// Human caller tried to bypass a moderator-only action while an active
    /// moderator session exists. Per vision § 11.4, human must route through
    /// the claimed moderator.
    HumanBypassYieldsToModerator {
        moderator: String,
        action: String,
        caller: String,
    },
}

impl ModeratorError {
    /// Returns the wire tag string (the `X` in `[error_code: X]`).
    /// Used by the TS drift-guard test to reflect the variant set.
    #[cfg(test)]
    fn variant_tag(&self) -> &'static str {
        match self {
            ModeratorError::CapabilityNotSupportedForFormat { .. } => "CAPABILITY_NOT_SUPPORTED_FOR_FORMAT",
            ModeratorError::HumanBypassYieldsToModerator { .. } => "HUMAN_BYPASS_YIELDS_TO_MODERATOR",
        }
    }

    /// Single source of truth for "one instance per variant" used by all
    /// drift-guard tests. Anchors `all_variant_tags()` AND
    /// `moderator_error_variant_tag_matches_wire_prefix` to the same set.
    ///
    /// **Drift-guard sync points** when adding a variant `FooBar` (per
    /// tech-leader msg 423 + architect msg 429 vision § 11.13):
    ///   1. add `FooBar { ... }` to the enum (Rust)
    ///   2. add `FooBar { .. } => "FOO_BAR"` to `variant_tag()` — compiler forces
    ///   3. add `FooBar { .. } => write!(...)` to `Display` — compiler forces
    ///   4. add `FooBar { .. } => "FOO_BAR"` to the `_exhaustive_check` closure
    ///      in `all_variant_tags()` — compiler forces (closure match exhaustive)
    ///   5. add `ModeratorError::FooBar { ... }` to this samples function —
    ///      compiler-silent but reviewer-obvious (4 prior sync points all
    ///      mention FooBar by name)
    ///   6. add `"FOO_BAR"` to `ModeratorErrorCode` union in
    ///      `desktop/src/lib/collabTypes.ts` (TS)
    ///   7. extend `parseModeratorError` for the new shape (TS) if applicable
    ///
    /// Strum (option 1 from dev-challenger msg 407) would collapse 5 → 4 by
    /// deriving the samples set automatically. Rejected per architect msg 429 +
    /// platform-engineer msg 413: proc-macro cold-build cost on Windows NTFS is
    /// disproportionate for a 2-variant enum. Revisit if the variant set grows.
    #[cfg(test)]
    fn samples() -> Vec<ModeratorError> {
        vec![
            ModeratorError::CapabilityNotSupportedForFormat {
                capability: String::new(),
                format: String::new(),
                reason: String::new(),
            },
            ModeratorError::HumanBypassYieldsToModerator {
                moderator: String::new(),
                action: String::new(),
                caller: String::new(),
            },
            // ── Negative-test harness ──
            // Uncomment the line below (and add a matching `FooBar` variant +
            // arms in `variant_tag` / `Display` / `all_variant_tags`'s
            // exhaustive-check closure) to verify drift-guard fails as expected.
            // `cargo test test_ts_types_match_rust_enum_variants` should report
            // the TS union missing `FOO_BAR`. Re-comment after manual verification.
            // ModeratorError::FooBar { sample: String::new() },
        ]
    }

    /// All tag strings currently defined. Two layers of drift-guard:
    ///   1. `_exhaustive_check` closure forces the compiler to refuse a new
    ///      variant unless a match arm with its literal tag is added here.
    ///   2. Returned tags derive from `variant_tag()` applied to `samples()`,
    ///      so the array values cannot diverge from the wire format.
    ///
    /// What this still doesn't catch: forgetting to add an instance to
    /// `samples()`. The only no-deps fix is reviewer discipline (the
    /// `samples()` doc above lists all 7 sync points). Strum closes it but
    /// is rejected for this enum size.
    #[cfg(test)]
    fn all_variant_tags() -> Vec<&'static str> {
        let _exhaustive_check: fn(&ModeratorError) -> &'static str = |e| match e {
            ModeratorError::CapabilityNotSupportedForFormat { .. } => "CAPABILITY_NOT_SUPPORTED_FOR_FORMAT",
            ModeratorError::HumanBypassYieldsToModerator { .. } => "HUMAN_BYPASS_YIELDS_TO_MODERATOR",
        };
        Self::samples().iter().map(ModeratorError::variant_tag).collect()
    }
}

impl std::fmt::Display for ModeratorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ModeratorError::CapabilityNotSupportedForFormat { capability, format, reason } => {
                write!(
                    f,
                    "[error_code: CAPABILITY_NOT_SUPPORTED_FOR_FORMAT] capability='{}' format='{}': {}",
                    capability, format, reason
                )
            }
            ModeratorError::HumanBypassYieldsToModerator { moderator, action, caller } => {
                write!(
                    f,
                    "[error_code: HUMAN_BYPASS_YIELDS_TO_MODERATOR] moderator='{}': Action '{}' requires the moderator role. You are {}. Route through the active moderator or wait for their stage.",
                    moderator, action, caller
                )
            }
        }
    }
}

/// Format a capability-not-supported-for-format error with a structured error code
/// so UX can pattern-match for tooltip rendering.
///
/// Why: dev-challenger msg 172 attack 2 flagged silent-failure UI bugs — moderator
/// capabilities that appear available but are format-scoped (e.g. `reorder_pipeline`
/// shown in Delphi where it does nothing). UX msg 166 confirmed the need for a
/// structured tag so tooltips render `"Only available in pipeline mode"` rather
/// than falling back to a generic string.
///
/// Delegates to `ModeratorError::CapabilityNotSupportedForFormat`; the enum's
/// `Display` impl owns the exact wire format. Kept as a free function so
/// existing call sites don't need to import the enum path.
fn format_capability_error(capability: &str, format: &str, reason: &str) -> String {
    ModeratorError::CapabilityNotSupportedForFormat {
        capability: capability.to_string(),
        format: format.to_string(),
        reason: reason.to_string(),
    }
    .to_string()
}

/// Validate a decision record for the `record_decision` action.
///
/// Why: tech-leader msg 279 routed the `decisions` primitive into PR 4. A decision
/// without a claim or a valid status is noise — it defeats the point of making
/// pipeline output actionable. This predicate enforces the contract at the API
/// layer so downstream tooling (UI, exports, postmortems) can rely on shape.
///
/// Shape: `{claim: string ≥5 chars after trim, status: accepted|deferred|rejected,
///          owner?: string, next_action?: string}`.
///
/// Returns Ok((claim, status, owner, next_action)) as owned strings on success,
/// or Err(message) describing what's wrong.
fn validate_decision_record(
    decision: Option<&serde_json::Value>,
) -> Result<(String, String, Option<String>, Option<String>), String> {
    let decision = decision.ok_or_else(||
        "record_decision requires a `decision` object. Shape: {claim, status, owner?, next_action?}.".to_string())?;
    let obj = decision.as_object().ok_or_else(||
        "`decision` must be a JSON object.".to_string())?;

    let claim = obj.get("claim").and_then(|c| c.as_str()).unwrap_or("").trim().to_string();
    if claim.len() < 5 {
        return Err("decision.claim must be a non-empty string with >=5 chars after trim.".to_string());
    }

    let status = obj.get("status").and_then(|s| s.as_str()).unwrap_or("").trim().to_string();
    const VALID_STATUSES: &[&str] = &["accepted", "deferred", "rejected"];
    if !VALID_STATUSES.contains(&status.as_str()) {
        return Err(format!(
            "decision.status must be one of: accepted, deferred, rejected. Got: '{}'.",
            status
        ));
    }

    let owner = obj.get("owner").and_then(|o| o.as_str()).map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    let next_action = obj.get("next_action").and_then(|n| n.as_str()).map(|s| s.trim().to_string()).filter(|s| !s.is_empty());

    Ok((claim, status, owner, next_action))
}

fn handle_discussion_control(action: &str, mode: Option<&str>, topic: Option<&str>, participants: Option<Vec<String>>, teams: Option<serde_json::Value>, reason: Option<&str>, decision: Option<serde_json::Value>) -> Result<serde_json::Value, String> {
    let state = get_or_rejoin_state()?;

    let my_label = format!("{}:{}", state.role, state.instance);

    // ── Moderator role-gating ──
    // These actions require the moderator role (or human override).
    // Guards are checked BEFORE dispatching to action handlers.
    let moderator_only_actions = [
        "close_round", "open_next_round", "pause", "resume",
        "pipeline_next", "end_discussion", "gate_audience",
        "inject_summary", "skip_participant", "reorder_pipeline",
        "toggle_pipeline_mode", "update_settings", "record_decision",
    ];
    if moderator_only_actions.contains(&action) {
        // Human-yields-to-claimed-MODERATOR refinement.
        //
        // Provenance chain: architect msg 310 (moderator != manager), tech-leader
        // msg 316 (retraction) + msg 326 (final scope) + msg 337 (drop pause filter),
        // dev-challenger msg 313, platform-engineer msg 320, architect's cf96dce
        // vision § 11.4a.
        //
        // Why: vision § 11.4 "Moderator Fallback Invariant" — human:0's implicit
        // bypass on moderator gates holds ONLY when no moderator is actually present.
        // If `discussion.moderator` names an active session, that session is
        // authoritative and human must route through them.
        //
        // Moderator ≠ Manager (architect msg 310): this predicate checks the MODERATOR
        // seat (stored in discussion.moderator). Manager has separate capabilities
        // per § 11.4b and does NOT gate moderator actions.
        //
        // Paused moderator RETAINS authority (vision § 11.4a, tech-leader msg 337):
        // pause is a deliberate session-state action, not a release of claim. Only
        // explicit departure (project_leave) clears the moderator seat. No pause
        // filter on this predicate.
        //
        // TOCTOU: this check reads state at one instant; action executes at another.
        // Cost of the race is a misleading error on one retry; cost of locking is
        // serializing every moderator gate check. Accepting the race per msg 323 #3.
        let discussion = read_discussion_state(&state.project_dir);
        let moderator = discussion.get("moderator")
            .and_then(|m| m.as_str())
            .unwrap_or("");
        let moderator_is_active = !moderator.is_empty()
            && has_active_session_for_label(&read_sessions(&state.project_dir), moderator);

        let human_bypass_ok = state.role == "human" && !moderator_is_active;

        if !human_bypass_ok {
            // Allow if caller is the assigned moderator, OR if no moderator is set (auto-mode)
            if !moderator.is_empty() && my_label != moderator {
                return Err(ModeratorError::HumanBypassYieldsToModerator {
                    moderator: moderator.to_string(),
                    action: action.to_string(),
                    caller: my_label.clone(),
                }
                .to_string());
            }
        }
    }

    // ── High-risk action audit gate ──
    // Human always bypasses.
    if state.role != "human" {
        validate_moderator_reason(action, reason)?;
    }

    match action {
        "start_discussion" => {
            let mode = mode.ok_or("mode is required for start_discussion")?;
            let topic = topic.ok_or("topic is required for start_discussion")?;

            // Validate mode — only discussion formats, not communication modes
            if !["delphi", "oxford", "red_team", "continuous", "pipeline"].contains(&mode) {
                return Err(format!("Invalid discussion format '{}'. Must be: delphi, oxford, red_team, continuous, pipeline. (Communication modes 'open'/'directed' are set separately via set_discussion_mode.)", mode));
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

            // Determine the actual moderator: prefer the "moderator" role if it
            // exists in the roster (even if not yet active — discussion-bound roles
            // auto-start when the discussion begins). Fall back to the caller.
            let discussion_moderator = {
                // First check if "moderator" role exists in the project roster
                let roster_has_moderator = read_project_config(&state.project_dir)
                    .ok()
                    .and_then(|cfg| cfg.get("roles").cloned())
                    .and_then(|roles| roles.get("moderator").cloned())
                    .is_some();

                if roster_has_moderator {
                    // Check for an active session first
                    let sessions = read_sessions(&state.project_dir);
                    let active_moderator = sessions.get("bindings")
                        .and_then(|b| b.as_array())
                        .and_then(|bindings| {
                            bindings.iter().find(|b| {
                                let role = b.get("role").and_then(|r| r.as_str()).unwrap_or("");
                                let status = b.get("status").and_then(|s| s.as_str()).unwrap_or("");
                                role == "moderator" && (status == "active" || status == "idle")
                            })
                        })
                        .and_then(|b| {
                            let instance = b.get("instance").and_then(|i| i.as_u64())?;
                            Some(format!("moderator:{}", instance))
                        });
                    // If no active session, use moderator:0 (will auto-start as discussion-bound)
                    active_moderator.unwrap_or_else(|| "moderator:0".to_string())
                } else {
                    my_label.clone()
                }
            };

            let now = utc_now_iso();

            // Continuous mode starts in "reviewing" phase with no rounds —
            // rounds are auto-created when developers post status messages.
            // "reviewing" = ready for next auto-trigger (consistent with post-close phase).
            // Other modes (delphi/oxford/red_team) start with round 1 open.
            let (initial_round, initial_phase, initial_rounds) = if mode == "pipeline" {
                // Pipeline starts at stage 0 (first participant), phase "pipeline_active"
                (0u64, "pipeline_active", serde_json::json!([]))
            } else if mode == "continuous" {
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

            // For pipeline mode, compute the stage order using build_pipeline_order
            // (same function as collab.rs — single source of truth for ordering)
            let pipeline_order: Option<Vec<String>> = if mode == "pipeline" {
                Some(build_pipeline_order(&state.project_dir, topic, &participant_list))
            } else {
                None
            };

            // Why: architect msg 169 locked the "Session = container, Format = behavior"
            // split. Every discussion instance needs a stable identity so downstream tooling
            // (UI, exports, postmortems, audit trails) can correlate messages to a specific
            // run across restarts. Using uuid v4 here; v7 deferred until Cargo.toml's uuid
            // dependency is bumped to >=1.7 per platform-engineer msg 175. Field is additive
            // JSON — DiscussionState struct reads default None for unknown fields today.
            let session_id = uuid::Uuid::new_v4().to_string();
            let mut new_state = serde_json::json!({
                "session_id": session_id,
                "active": true,
                "mode": mode,
                "topic": topic,
                "started_at": now,
                "moderator": discussion_moderator,
                "participants": participant_list,
                "teams": null,
                "current_round": initial_round,
                "phase": initial_phase,
                "paused_at": null,
                "expire_at": null,
                "previous_phase": null,
                "rounds": initial_rounds,
                "audience_state": "listening",
                "audience_enabled": false,
                "settings": {
                    "max_rounds": if mode == "continuous" { 999 } else if mode == "pipeline" { 5 } else { 10 },
                    "timeout_minutes": 15,
                    "expire_paused_after_minutes": 60,
                    "auto_close_timeout_seconds": if mode == "continuous" { 60 } else if mode == "pipeline" { 30 } else { 0 },
                    "termination": serde_json::json!(null),
                    "automation": "auto"
                }
            });

            // Add pipeline-specific fields. Per pr-r2-pipeline-mode-value
            // (UX msg 521 + tech-leader msg 554 audit): the sub-mode that
            // alternates with "action" is now called "review" everywhere.
            // UI already labels it that way; this aligns the underlying
            // value so the data layer matches the visible name.
            if let Some(ref order) = pipeline_order {
                new_state["pipeline_order"] = serde_json::json!(order);
                new_state["pipeline_stage"] = serde_json::json!(0);
                new_state["pipeline_stage_started_at"] = serde_json::json!(utc_now_iso());
                new_state["pipeline_outputs"] = serde_json::json!([]);
                new_state["pipeline_mode"] = serde_json::json!("review");
            }

            with_file_lock(&state.project_dir, || {
                write_discussion_state(&state.project_dir, &new_state)?;

                // NOTE: We do NOT update project.json discussion_mode here.
                // Communication mode (directed/open) and discussion format (delphi/oxford)
                // are orthogonal concepts. project.json stores the communication mode.
                // discussion.json stores the discussion format. They operate independently.

                // Post announcement to board
                let msg_id = next_message_id(&state.project_dir);
                let announcement_body = if mode == "pipeline" {
                    let order = new_state.get("pipeline_order").and_then(|o| o.as_array())
                        .map(|a| a.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>().join(" → "))
                        .unwrap_or_default();
                    let first = new_state.get("pipeline_order").and_then(|o| o.as_array())
                        .and_then(|a| a.first()).and_then(|v| v.as_str()).unwrap_or("?");
                    format!("Pipeline started.\n\n**Topic:** {}\n**Pipeline order:** {}\n**Current stage:** {} (1/{})\n\nEach agent processes in order, seeing all previous outputs. Respond with a broadcast when your stage is complete.",
                        topic, order, first, participant_list.len())
                } else if mode == "continuous" {
                    format!("Continuous Review mode activated.\n\n**Topic:** {}\n**Moderator:** {}\n**Participants:** {}\n\nReview windows open automatically when developers post status updates. Respond with: agree / neutral / disagree: [reason] / alternative: [proposal]. Silence within the timeout = consent.",
                        topic, discussion_moderator, participant_list.join(", "))
                } else if mode == "delphi" {
                    format!("A Delphi discussion is being prepared.\n\n**Topic:** {}\n**Moderator:** {}\n**Participants:** {}\n**Phase:** Preparing (broadcasts locked)\n\nAll broadcasts to \"all\" are now blocked to protect blind submission integrity. The moderator will coordinate privately via directed messages, then open Round 1 when ready. Do NOT share reference material publicly.",
                        topic, discussion_moderator, participant_list.join(", "))
                } else {
                    format!("A {} discussion has been started.\n\n**Topic:** {}\n**Moderator:** {}\n**Participants:** {}\n**Round:** 1\n\nSubmit your position using type: submission, addressed to the moderator.",
                        mode, topic, discussion_moderator, participant_list.join(", "))
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
                "moderator": discussion_moderator
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

        "pipeline_next" => {
            // Moderator selects the next pipeline stage agent (typed struct access)
            let typed_disc = read_discussion_typed(&state.project_dir);
            if !typed_disc.active {
                return Err("No active discussion".to_string());
            }
            if typed_disc.mode.as_deref() != Some("pipeline") {
                let fmt = typed_disc.mode.as_deref().unwrap_or("(none)");
                return Err(format_capability_error("pipeline_next", fmt,
                    "pipeline_next is only valid for Pipeline discussions"));
            }
            let target = topic.ok_or("Specify the next agent as the topic parameter (e.g., 'developer:1')")?;
            let pipeline_order_vec = typed_disc.pipeline_order.clone().unwrap_or_default();
            let pipeline_order: Vec<serde_json::Value> = pipeline_order_vec.iter().map(|s| serde_json::json!(s)).collect();
            let current_stage = typed_disc.pipeline_stage.unwrap_or(0) as usize;
            // Read raw JSON for mutation (typed write not yet used for complex updates)
            let disc = read_discussion_state(&state.project_dir);

            // Audit trail for the jump — required non-empty reason enforced above for pipeline_next.
            // Why: dev-challenger msg 172 tier table flagged jump-to-stage as skipping participants;
            // the reason must be traceable on the wake message so the skipped roles and UI can
            // reconstruct why the moderator intervened.
            let audit_reason = reason.unwrap_or("").trim().to_string();
            let audit_timestamp = utc_now_iso();

            with_file_lock(&state.project_dir, || {
                let mut updated = disc.clone();
                // Update pipeline order: insert the target as the next stage
                let mut new_order: Vec<serde_json::Value> = pipeline_order[..current_stage].to_vec();
                // Keep completed stages, add new target as next
                new_order.push(serde_json::json!(target));
                // Add remaining unprocessed agents (excluding the target if already present)
                for item in &pipeline_order[current_stage..] {
                    if item.as_str() != Some(target) {
                        new_order.push(item.clone());
                    }
                }
                updated["pipeline_order"] = serde_json::json!(new_order);
                updated["pipeline_stage"] = serde_json::json!(current_stage);
                write_discussion_state(&state.project_dir, &updated)?;

                // Post system message and wake the target agent
                // Use full role:instance so only the specific instance is notified
                post_turn_system_message(&state.project_dir, &format!("Moderator selected next stage: {} — reason: {}", target, audit_reason));
                let wake_msg = serde_json::json!({
                    "id": next_message_id(&state.project_dir),
                    "from": "system:0",
                    "to": target,
                    "type": "system",
                    "subject": "Your pipeline stage",
                    "body": format!("The moderator has selected you as the next pipeline stage. Review previous outputs and broadcast your response.\n\nModerator reason: {}", audit_reason),
                    "timestamp": audit_timestamp,
                    "metadata": {
                        "pipeline_notification": true,
                        "moderator_action": {
                            "action": "pipeline_next",
                            "reason": audit_reason,
                            "timestamp": audit_timestamp,
                            "actor": my_label,
                            "affected_role": target
                        }
                    }
                });
                append_to_board(&state.project_dir, &wake_msg)?;
                Ok(())
            })?;

            notify_desktop();
            Ok(serde_json::json!({
                "status": "next_stage_set",
                "next_agent": target,
                "stage": current_stage + 1
            }))
        }

        "toggle_pipeline_mode" => {
            // Toggle pipeline between "review" (formerly "discussion" — opinions)
            // and "action" (write code). Per pr-r2-pipeline-mode-value: legacy
            // "discussion" value is normalized to "review" on read so old state
            // files toggle correctly; new writes always emit "review".
            let typed_disc = read_discussion_typed(&state.project_dir);
            if !typed_disc.active {
                return Err("No active discussion".to_string());
            }
            if typed_disc.mode.as_deref() != Some("pipeline") {
                let fmt = typed_disc.mode.as_deref().unwrap_or("(none)");
                return Err(format_capability_error("toggle_pipeline_mode", fmt,
                    "toggle_pipeline_mode is only valid for Pipeline discussions"));
            }
            let raw_mode = typed_disc.pipeline_mode.as_deref().unwrap_or("review");
            // Normalize legacy "discussion" → "review" so toggle treats them as
            // the same logical state.
            let current_mode = if raw_mode == "discussion" { "review" } else { raw_mode };
            let new_mode = if current_mode == "review" { "action" } else { "review" };
            let disc = read_discussion_state(&state.project_dir);

            with_file_lock(&state.project_dir, || {
                let mut updated = disc.clone();
                updated["pipeline_mode"] = serde_json::json!(new_mode);
                write_discussion_state(&state.project_dir, &updated)?;
                post_turn_system_message(&state.project_dir,
                    &format!("Pipeline mode switched to: {} (was: {})", new_mode, current_mode));
                Ok(())
            })?;

            notify_desktop();
            Ok(serde_json::json!({
                "status": "pipeline_mode_toggled",
                "previous_mode": current_mode,
                "new_mode": new_mode
            }))
        }

        "end_discussion" => {
            // Use typed struct for reliable field access
            let typed_disc = read_discussion_typed(&state.project_dir);
            if !typed_disc.active {
                return Err("No active discussion to end".to_string());
            }

            let now = utc_now_iso();
            let round_num = typed_disc.current_round as u64;
            let topic = typed_disc.topic.as_str();
            // Read raw JSON for mutation (write path still uses JSON)
            let discussion = read_discussion_state(&state.project_dir);

            with_file_lock(&state.project_dir, || {
                let mut updated = discussion.clone();
                updated["active"] = serde_json::json!(false);
                updated["phase"] = serde_json::json!("complete");
                write_discussion_state(&state.project_dir, &updated)?;

                // Post end announcement with moderator-action audit metadata.
                // Why: dev-challenger msg 172 + tech-leader msg 178 require every privileged
                // moderator action to carry {action, reason, timestamp} so downstream tooling
                // (UI, exports, postmortems) can surface who ended what and why. The 'reason'
                // field is validated non-empty above for high-risk actions — safe to unwrap.
                let audit_reason = reason.unwrap_or("").trim().to_string();
                let msg_id = next_message_id(&state.project_dir);
                let announcement = serde_json::json!({
                    "id": msg_id,
                    "from": my_label,
                    "to": "all",
                    "type": "moderation",
                    "timestamp": now,
                    "subject": format!("Discussion ended: {}", topic),
                    "body": format!("The discussion on \"{}\" has concluded after {} round(s). Reason: {}", topic, round_num, audit_reason),
                    "metadata": {
                        "discussion_action": "end",
                        "final_round": round_num,
                        "moderator_action": {
                            "action": "end_discussion",
                            "reason": audit_reason,
                            "timestamp": now,
                            "actor": my_label
                        }
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

        "pause" => {
            let typed_disc = read_discussion_typed(&state.project_dir);
            if !typed_disc.active {
                return Err("No active discussion to pause".to_string());
            }
            if typed_disc.paused_at.is_some() {
                return Err("Discussion is already paused".to_string());
            }
            let now = utc_now_iso();
            let discussion = read_discussion_state(&state.project_dir);
            with_file_lock(&state.project_dir, || {
                let mut updated = discussion.clone();
                updated["paused_at"] = serde_json::json!(now);
                write_discussion_state(&state.project_dir, &updated)?;

                let msg_id = next_message_id(&state.project_dir);
                let announcement = serde_json::json!({
                    "id": msg_id,
                    "from": my_label,
                    "to": "all",
                    "type": "system",
                    "timestamp": now,
                    "subject": "Discussion paused",
                    "body": "The discussion has been paused. It will resume when the moderator or human resumes it.",
                    "metadata": {"discussion_action": "pause"}
                });
                append_to_board(&state.project_dir, &announcement)?;
                Ok(())
            })?;
            notify_desktop();
            Ok(serde_json::json!({"status": "paused", "paused_at": now}))
        }

        "resume" => {
            let typed_disc = read_discussion_typed(&state.project_dir);
            if !typed_disc.active {
                return Err("No active discussion to resume".to_string());
            }
            if typed_disc.paused_at.is_none() {
                return Err("Discussion is not paused".to_string());
            }
            let now = utc_now_iso();
            let discussion = read_discussion_state(&state.project_dir);
            with_file_lock(&state.project_dir, || {
                let mut updated = discussion.clone();
                updated["paused_at"] = serde_json::json!(null);
                write_discussion_state(&state.project_dir, &updated)?;

                let msg_id = next_message_id(&state.project_dir);
                let announcement = serde_json::json!({
                    "id": msg_id,
                    "from": my_label,
                    "to": "all",
                    "type": "system",
                    "timestamp": now,
                    "subject": "Discussion resumed",
                    "body": "The discussion has been resumed.",
                    "metadata": {"discussion_action": "resume"}
                });
                append_to_board(&state.project_dir, &announcement)?;
                Ok(())
            })?;
            notify_desktop();
            Ok(serde_json::json!({"status": "resumed"}))
        }

        "update_settings" => {
            let typed_disc = read_discussion_typed(&state.project_dir);
            if !typed_disc.active {
                return Err("No active discussion to update settings for".to_string());
            }
            let discussion = read_discussion_state(&state.project_dir);
            with_file_lock(&state.project_dir, || {
                let mut updated = discussion.clone();
                // Accept max_rounds from the topic parameter (repurposed for settings value)
                if let Some(ref value) = topic {
                    if let Ok(max_r) = value.parse::<u64>() {
                        updated["settings"]["max_rounds"] = serde_json::json!(max_r);
                    }
                }
                write_discussion_state(&state.project_dir, &updated)?;
                Ok(())
            })?;
            notify_desktop();
            Ok(serde_json::json!({"status": "settings_updated"}))
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

        "record_decision" => {
            // Moderator records a decision reached during the discussion.
            //
            // Why: tech-leader msg 279 routed the `decisions` primitive into PR 4 per the
            // msg 56 ledger. Pipeline sessions produce transcripts; decisions make the
            // output actionable. Appending to a structured `decisions` array on the
            // discussion state lets downstream UI render a checklist and downstream
            // postmortems measure how many decisions actually shipped.
            //
            // Shape enforced by validate_decision_record:
            //   claim (>=5 chars), status in {accepted, deferred, rejected}, owner?, next_action?.
            //
            // Each decision also posts a concise board message so the team sees it in-band.
            let (claim, status, owner, next_action) = validate_decision_record(decision.as_ref())?;
            let now = utc_now_iso();
            let typed_disc = read_discussion_typed(&state.project_dir);
            if !typed_disc.active {
                return Err("No active discussion to record decisions against.".to_string());
            }
            let raw_disc = read_discussion_state(&state.project_dir);

            with_file_lock(&state.project_dir, || {
                let mut updated = raw_disc.clone();
                let mut decisions = updated.get("decisions").and_then(|d| d.as_array()).cloned().unwrap_or_default();
                let decision_entry = serde_json::json!({
                    "claim": claim,
                    "status": status,
                    "owner": owner,
                    "next_action": next_action,
                    "recorded_at": now,
                    "recorded_by": my_label,
                });
                decisions.push(decision_entry.clone());
                updated["decisions"] = serde_json::json!(decisions);
                write_discussion_state(&state.project_dir, &updated)?;

                // Post concise board message so the team sees the decision in-band.
                let msg_id = next_message_id(&state.project_dir);
                let status_glyph = match status.as_str() {
                    "accepted" => "✓",
                    "deferred" => "⚠",
                    "rejected" => "✗",
                    _ => "•",
                };
                let body = match (&owner, &next_action) {
                    (Some(o), Some(n)) => format!("{} {} — {} — owner: {} — next: {}", status_glyph, status, claim, o, n),
                    (Some(o), None) => format!("{} {} — {} — owner: {}", status_glyph, status, claim, o),
                    (None, Some(n)) => format!("{} {} — {} — next: {}", status_glyph, status, claim, n),
                    (None, None) => format!("{} {} — {}", status_glyph, status, claim),
                };
                let announcement = serde_json::json!({
                    "id": msg_id,
                    "from": my_label,
                    "to": "all",
                    "type": "moderation",
                    "timestamp": now,
                    "subject": format!("Decision {}: {}", status, if claim.chars().count() > 60 { format!("{}…", claim.chars().take(60).collect::<String>()) } else { claim.clone() }),
                    "body": body,
                    "metadata": {
                        "discussion_action": "record_decision",
                        "decision": decision_entry,
                        "moderator_action": {
                            "action": "record_decision",
                            "timestamp": now,
                            "actor": my_label,
                        }
                    }
                });
                append_to_board(&state.project_dir, &announcement)?;
                Ok(())
            })?;

            notify_desktop();
            Ok(serde_json::json!({
                "status": "decision_recorded",
                "decision_status": status,
                "claim": claim,
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

        "create_review_round" => {
            // Moderator-initiated review round in continuous mode
            let round_topic = topic.unwrap_or("Review round");

            let disc = read_discussion_state(&state.project_dir);
            if !disc.get("active").and_then(|v| v.as_bool()).unwrap_or(false) {
                return Err("No active discussion.".to_string());
            }
            let disc_mode = disc.get("mode").and_then(|v| v.as_str()).unwrap_or("");
            if disc_mode != "continuous" {
                return Err(format!("create_review_round only works in continuous mode (current: {})", disc_mode));
            }

            // Only the moderator can create review rounds
            let moderator = disc.get("moderator").and_then(|v| v.as_str()).unwrap_or("");
            if my_label != moderator {
                return Err(format!("Only the moderator ({}) can create review rounds", moderator));
            }

            // Check no round is already open
            let current_phase = disc.get("phase").and_then(|v| v.as_str()).unwrap_or("");
            if current_phase == "submitting" {
                return Err("A round is already open. Close it first.".to_string());
            }

            let now = utc_now_iso();
            let mut updated = disc.clone();
            let current_round = updated.get("current_round").and_then(|v| v.as_u64()).unwrap_or(0);
            let next_round = current_round + 1;

            updated["current_round"] = serde_json::json!(next_round);
            updated["phase"] = serde_json::json!("submitting");

            if let Some(rounds) = updated.get_mut("rounds").and_then(|r| r.as_array_mut()) {
                rounds.push(serde_json::json!({
                    "number": next_round,
                    "opened_at": now,
                    "closed_at": null,
                    "submissions": [],
                    "aggregate_message_id": null,
                    "auto_triggered": false,
                    "topic": round_topic,
                    "trigger_from": my_label
                }));
            }

            let timeout_secs = updated.get("settings")
                .and_then(|s| s.get("auto_close_timeout_seconds"))
                .and_then(|v| v.as_u64())
                .unwrap_or(60);

            with_file_lock(&state.project_dir, || {
                write_discussion_state(&state.project_dir, &updated)?;

                let msg_id = next_message_id(&state.project_dir);
                let announcement = serde_json::json!({
                    "id": msg_id,
                    "from": "system",
                    "to": "all",
                    "type": "moderation",
                    "timestamp": now,
                    "subject": format!("Review #{}: {}", next_round, round_topic),
                    "body": format!("**REVIEW WINDOW OPEN** ({}s)\n{} opened review round #{}: {}\n\nRespond with: agree / disagree: [reason] / alternative: [proposal]\nSilence within {}s = consent.",
                        timeout_secs, my_label, next_round, round_topic, timeout_secs),
                    "metadata": {
                        "discussion_action": "review_round",
                        "round": next_round,
                        "timeout_seconds": timeout_secs
                    }
                });
                append_to_board(&state.project_dir, &announcement)?;
                Ok(())
            })?;

            notify_desktop();
            Ok(serde_json::json!({
                "status": "review_round_created",
                "round": next_round,
                "topic": round_topic,
                "timeout_seconds": timeout_secs
            }))
        }

        "audience_control" => {
            // Set audience state: listening, voting, qa, commenting, open
            let new_state = mode.ok_or("mode is required for audience_control (listening, voting, qa, commenting, open)")?;
            if !["listening", "voting", "qa", "commenting", "open"].contains(&new_state) {
                return Err(format!("Invalid audience state '{}'. Must be: listening, voting, qa, commenting, open", new_state));
            }

            let disc = read_discussion_state(&state.project_dir);
            if !disc.get("active").and_then(|v| v.as_bool()).unwrap_or(false) {
                return Err("No active discussion. Audience control requires an active discussion.".to_string());
            }

            // Only the moderator can control audience
            let moderator = disc.get("moderator").and_then(|v| v.as_str()).unwrap_or("");
            if my_label != moderator {
                return Err(format!("Only the moderator ({}) can control the audience", moderator));
            }

            let mut updated = disc.clone();
            updated["audience_state"] = serde_json::json!(new_state);
            // Enable/disable audience based on state
            updated["audience_enabled"] = serde_json::json!(new_state != "listening");

            with_file_lock(&state.project_dir, || {
                write_discussion_state(&state.project_dir, &updated)?;

                // Announce the state change
                let msg_id = next_message_id(&state.project_dir);
                let announcement = serde_json::json!({
                    "id": msg_id,
                    "from": "system",
                    "to": "all",
                    "type": "moderation",
                    "timestamp": utc_now_iso(),
                    "subject": format!("Audience state: {}", new_state),
                    "body": match new_state {
                        "listening" => "Audience is now in listen-only mode. No audience responses will be posted.",
                        "voting" => "Audience voting is now OPEN. Audience members may submit votes.",
                        "qa" => "Audience Q&A is now OPEN. Audience members may ask questions.",
                        "commenting" => "Audience commenting is now OPEN. Audience members may share reactions.",
                        "open" => "Audience is now fully OPEN. All audience responses will be posted.",
                        _ => "Audience state changed."
                    },
                    "metadata": {
                        "discussion_action": "audience_control",
                        "audience_state": new_state
                    }
                });
                append_to_board(&state.project_dir, &announcement)?;
                Ok(())
            })?;

            notify_desktop();
            Ok(serde_json::json!({
                "status": "audience_state_changed",
                "audience_state": new_state
            }))
        }

        _ => Err(format!("Unknown discussion action: '{}'. Valid: start_discussion, close_round, open_next_round, end_discussion, get_state, set_teams, create_review_round, audience_control", action))
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

    // Also import global role groups (~/.vaak/role-groups.json) if project has none
    let role_groups_path = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(|h| PathBuf::from(h).join(".vaak").join("role-groups.json"))
        .unwrap_or_default();

    if role_groups_path.exists() {
        let project_groups = config.get("role_groups").and_then(|g| g.as_array());
        let has_groups = project_groups.map(|a| !a.is_empty()).unwrap_or(false);

        if !has_groups {
            if let Ok(content) = std::fs::read_to_string(&role_groups_path) {
                if let Ok(groups) = serde_json::from_str::<serde_json::Value>(&content) {
                    if groups.is_array() {
                        eprintln!("[vaak-mcp] Importing global role groups into project");
                        config["role_groups"] = groups;
                        added_any = true;
                    }
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

    // Read role briefing
    let briefing_path = role_briefing_path(&normalized, role);
    let briefing = std::fs::read_to_string(&briefing_path).unwrap_or_default();

    // Read last 10 messages directed to this role, this instance, or 'all'
    // Roles with "see_all" permission (e.g., manager) see ALL messages
    let my_instance_label = format!("{}:{}", role, instance);
    let has_see_all = role_has_see_all(&normalized, role);
    let all_messages = read_board_filtered(&normalized);
    let my_messages: Vec<&serde_json::Value> = all_messages.iter()
        .filter(|m| {
            if has_see_all { return true; }
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

    // Spawn the background heartbeat ticker (no-op if already running for this
    // sidecar process — see ensure_heartbeat_ticker_started). This is the fix
    // for the "agent disappears after 40 min of tool-calls" pattern: heartbeat
    // now ticks independently of project_wait/send/claim cadence.
    ensure_heartbeat_ticker_started();

    notify_desktop();

    let active_section = get_active_section(project_dir);

    // Advance last_seen_id to the max ID in recent_messages so project_check
    // won't re-deliver these same messages (prevents token waste)
    let max_recent_id = recent.iter()
        .filter_map(|m| m.get("id").and_then(|i| i.as_u64()))
        .max()
        .unwrap_or(0);
    if max_recent_id > 0 {
        let ls_path = last_seen_path(&normalized, session_id);
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

/// Handle project_send: send a message to a role
fn handle_project_send(to: &str, msg_type: &str, subject: &str, body: &str, metadata: Option<serde_json::Value>, _session_id: &str) -> Result<serde_json::Value, String> {
    let state = get_or_rejoin_state()?;

    let config = read_project_config(&state.project_dir)?;

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

    // Read discussion state once for broadcast permission and Delphi enforcement
    let disc = read_discussion_state(&state.project_dir);
    let disc_active = disc.get("active").and_then(|v| v.as_bool()).unwrap_or(false);
    let disc_format = disc.get("mode").and_then(|v| v.as_str()).unwrap_or("");

    // ── Sequential-turn gate (pr-seq-1, per manager msg 377 + 384) ──
    //
    // When `discussion.json` has an `active_sequence.current_holder`, only that
    // role (or human/manager) can post to the board. Everyone else gets
    // ERR_NOT_YOUR_TURN. This is the strict-sequential enforcement point per
    // `feedback_pipeline_mode_strict` — gate at project_send layer, no soft
    // exceptions. Escalation to human (`to == "human"`) is always allowed
    // since that's the override path the human can always pull.
    let my_label_seq = format!("{}:{}", state.role, state.instance);
    if let Some(seq) = disc.get("active_sequence") {
        let seq_active = seq.get("active").and_then(|v| v.as_bool()).unwrap_or(false);
        let paused = seq.get("paused_for_human").and_then(|v| v.as_bool()).unwrap_or(false);
        if seq_active && !paused {
            let current_holder = seq.get("current_holder").and_then(|v| v.as_str()).unwrap_or("");
            let is_current = my_label_seq == current_holder;
            let is_human = state.role == "human";
            let is_manager_override = state.role == "manager" && metadata
                .as_ref()
                .and_then(|m| m.get("moderator_override"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let is_moderator_override = state.role == "moderator" && metadata
                .as_ref()
                .and_then(|m| m.get("moderator_override"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let to_human = to == "human";
            if !is_current && !is_human && !is_manager_override && !is_moderator_override && !to_human {
                return Err(format!(
                    "ERR_NOT_YOUR_TURN: sequential-turn gate active, current holder is {}. \
                    Sender {} is not the turn-holder. Escalate to human via to=\"human\", \
                    or wait for your turn. Moderator/manager bypass requires \
                    metadata.moderator_override=true.",
                    current_holder, my_label_seq
                ));
            }
        }
    }

    // Broadcast permission: all roles can broadcast by default.
    // Noise control is handled by work mode (consecutive/simultaneous) and
    // communication mode (directed/open), not by permission flags.

    // Pipeline mode: strict gating per human msg 1122 ("discussion mode is meaningless if not strict").
    // Old behavior: only `to == "all"` was blocked, leaving `to: <role>` directed messages as a loophole.
    // New behavior (pr-pipeline-gate-strict): block ALL sends from non-current-stage roles unless
    // they're one of these explicit exceptions:
    //   - type=="question" directed to current-stage role
    //   - type=="answer" in_reply_to a question from the current-stage role
    //   - type=="ack" (turn-acknowledgement, must come from any role to enable pr-pipeline-turn-ack flow)
    //   - to=="human" (always allowed — human is always reachable)
    //   - sender role is "human" or "manager" (override authority)
    if disc_active && disc_format == "pipeline" && state.role != "human" && state.role != "manager" {
        let typed_disc = read_discussion_typed(&state.project_dir);
        let pipeline_order = typed_disc.pipeline_order.as_deref().unwrap_or(&[]);
        let current_stage = typed_disc.pipeline_stage.unwrap_or(0) as usize;
        let pipeline_phase = typed_disc.phase.as_deref().unwrap_or("");
        let from_label = format!("{}:{}", state.role, state.instance);
        let current_agent = pipeline_order.get(current_stage).map(|s| s.as_str()).unwrap_or("");
        let is_complete = pipeline_phase == "pipeline_complete";
        let is_current = from_label == current_agent;
        if !is_complete && !is_current && to != "human" {
            // Check the three allowed exception types
            let to_role_part = to.split(':').next().unwrap_or(to);
            let current_role_part = current_agent.split(':').next().unwrap_or(current_agent);
            let is_question_to_current = msg_type == "question" && to_role_part == current_role_part;
            // Per dev-challenger msg 1139 attack #2: ack must come from the current-stage holder,
            // otherwise any idle role could send "ack" to bypass the gate.
            let is_ack = msg_type == "ack" && from_label == current_agent;
            let is_answer_to_current_question = msg_type == "answer" && {
                if let Some(reply_id) = metadata.as_ref().and_then(|m| m.get("in_reply_to")).and_then(|v| v.as_u64()) {
                    let referenced = read_board_filtered(&state.project_dir).into_iter()
                        .find(|m| m.get("id").and_then(|i| i.as_u64()) == Some(reply_id));
                    referenced.map(|m| {
                        let ref_type = m.get("type").and_then(|v| v.as_str()).unwrap_or("");
                        let ref_from = m.get("from").and_then(|v| v.as_str()).unwrap_or("");
                        ref_type == "question" && ref_from == current_agent
                    }).unwrap_or(false)
                } else { false }
            };
            if !is_question_to_current && !is_ack && !is_answer_to_current_question {
                return Err(format!(
                    "Pipeline mode strict gating: not your stage. Current stage: {} ({}/{}). \
                    Allowed during pipeline: type=question to current-stage role, type=answer to current-stage role's question (with in_reply_to), type=ack, or to=human. \
                    Sender: {}, attempted: type={} to={}.",
                    current_agent, current_stage + 1, pipeline_order.len(), from_label, msg_type, to));
            }
        }
    }

    // Validate message type against role permissions.
    // Maps message types to required permissions. Human is always exempt.
    if state.role != "human" {
        let roles = config.get("roles").and_then(|r| r.as_object());
        let my_role_def = roles.and_then(|r| r.get(&state.role));
        let perms: Vec<String> = my_role_def
            .and_then(|r| r.get("permissions"))
            .and_then(|p| p.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
            .unwrap_or_default();

        let required_perm = match msg_type {
            "directive" | "approval" | "revision" => Some("assign_tasks"),
            "review" => Some("review"),
            "moderation" => Some("moderation"),
            "handoff" => Some("handoff"),
            // status, question, answer, submission, vote — allowed for all roles
            _ => None,
        };

        if let Some(perm) = required_perm {
            if !perms.contains(&perm.to_string()) {
                return Err(format!(
                    "Permission denied: message type '{}' requires '{}' permission. Your permissions: {:?}",
                    msg_type, perm, perms
                ));
            }
        }
    }

    // Block acknowledgment-only messages (e.g., "Got it", "Will do", "Okay").
    // These waste message bandwidth and add no value.
    if state.role != "human" {
        let trimmed = body.trim().to_lowercase();
        let ack_patterns = [
            "got it", "will do", "okay", "ok", "understood", "acknowledged",
            "thanks", "thank you", "noted", "roger", "copy that", "on it",
            "sure", "yes", "yep", "yeah", "ack", "k",
        ];
        if trimmed.len() < 20 && ack_patterns.iter().any(|p| trimmed == *p || trimmed == format!("{}.", p) || trimmed == format!("{}!", p)) {
            return Err("Acknowledgment-only messages are not allowed. Either do the work, ask a question, or provide substantive information.".to_string());
        }
    }

    // Block non-manager roles from messaging the human directly.
    // Only the manager can send messages to the human. Exception: type "answer"
    // (replying to a human question) is always allowed from any role.
    if to == "human" && state.role != "human" && state.role != "manager" && msg_type != "answer" {
        return Err("Only the Manager can message the human directly. Send your message to the Manager for relay, or to the relevant peer role.".to_string());
    }

    // Consecutive mode turn enforcement — block out-of-turn broadcasts.
    // Directed messages to specific roles are always allowed for coordination.
    // Manager always has priority. Human is always exempt.
    if to == "all" && state.role != "human" {
        let work_mode = config.get("settings")
            .and_then(|s| s.get("work_mode"))
            .and_then(|w| w.as_str())
            .unwrap_or("simultaneous");

        if work_mode == "consecutive" {
            let turn_state = read_turn_state(&state.project_dir);
            let turn_completed = turn_state.get("completed").and_then(|c| c.as_bool()).unwrap_or(true);

            if !turn_completed && state.role != "manager" {
                let relevance_order = turn_state.get("relevance_order")
                    .and_then(|t| t.as_array())
                    .cloned()
                    .unwrap_or_default();
                let current_idx = turn_state.get("current_index")
                    .and_then(|i| i.as_u64())
                    .unwrap_or(0) as usize;

                // Auto-advance timeout: if current turn holder has been waiting too long, skip them
                let timeout_secs = config.get("settings")
                    .and_then(|s| s.get("consecutive_timeout_secs"))
                    .and_then(|t| t.as_u64())
                    .unwrap_or(120);
                let turn_started = turn_state.get("turn_started_at")
                    .or_else(|| turn_state.get("started_at"))
                    .and_then(|t| t.as_str())
                    .unwrap_or("");

                // Parse ISO timestamp and check timeout
                if !turn_started.is_empty() {
                    // Simple seconds-since comparison using system time
                    // (turn_started_at is updated on each advance)
                    let now = utc_now_iso();
                    if let (Some(start_secs), Some(now_secs)) = (
                        parse_iso_to_epoch_secs(turn_started),
                        parse_iso_to_epoch_secs(&now)
                    ) {
                        if now_secs > start_secs + timeout_secs {
                            // Timeout: auto-skip current turn holder
                            let current_label = relevance_order.get(current_idx)
                                .and_then(|v| v.as_str())
                                .unwrap_or("unknown");
                            eprintln!("[consecutive] Timeout: skipping {} after {}s", current_label, timeout_secs);
                            post_turn_system_message(&state.project_dir, &format!("{} skipped (timeout {}s)", current_label, timeout_secs));
                            let _ = advance_turn(&state.project_dir, current_label, true);
                            // Re-read state after advancement
                            let updated_state = read_turn_state(&state.project_dir);
                            let new_completed = updated_state.get("completed").and_then(|c| c.as_bool()).unwrap_or(true);
                            if new_completed {
                                // All turns done after timeout skip — allow this send
                            } else {
                                let new_idx = updated_state.get("current_index").and_then(|i| i.as_u64()).unwrap_or(0) as usize;
                                let new_order = updated_state.get("relevance_order").and_then(|t| t.as_array()).cloned().unwrap_or_default();
                                let new_current = new_order.get(new_idx).and_then(|v| v.as_str()).unwrap_or("");
                                let my_label = format!("{}:{}", state.role, state.instance);
                                if my_label != new_current {
                                    return Err(format!(
                                        "Consecutive mode: not your turn. Current: {}. You may send directed messages to specific roles, or wait for your turn.",
                                        new_current
                                    ));
                                }
                            }
                        } else {
                            // No timeout — enforce turn order
                            let current_label = relevance_order.get(current_idx)
                                .and_then(|v| v.as_str())
                                .unwrap_or("");
                            let my_label = format!("{}:{}", state.role, state.instance);
                            if my_label != current_label {
                                return Err(format!(
                                    "Consecutive mode: not your turn. Current: {}. You may send directed messages to specific roles, or wait for your turn.",
                                    current_label
                                ));
                            }
                        }
                    }
                }
            }
        }
    }

    // Audience gating: enforce audience_state during active discussions.
    // The audience role can only speak when the moderator has opened their state.
    // Submissions (to moderator) are always allowed regardless of audience state.
    if state.role == "audience" && disc_active {
        let audience_state = disc.get("audience_state")
            .and_then(|v| v.as_str())
            .unwrap_or("listening");

        // Submissions to the moderator are always allowed (e.g., vote results)
        if msg_type != "submission" {
            let allowed = match audience_state {
                "listening" => false, // silent — cannot speak
                "voting" => msg_type == "vote" || msg_type == "status", // can only submit votes
                "qa" => msg_type == "question", // can only ask questions
                "commenting" => msg_type == "status" || msg_type == "question", // can comment/question
                "open" => true, // unrestricted
                _ => false,
            };

            if !allowed {
                let hint = match audience_state {
                    "listening" => "The audience is in LISTENING mode — you cannot speak until the moderator opens the floor.",
                    "voting" => "The audience is in VOTING mode — only vote submissions are allowed.",
                    "qa" => "The audience is in Q&A mode — only questions are allowed.",
                    "commenting" => "The audience is in COMMENTING mode — only status updates and questions are allowed.",
                    _ => "The audience state does not allow this message type.",
                };
                return Err(format!(
                    "Audience gating: message type '{}' blocked. {}",
                    msg_type, hint
                ));
            }
        }
    }

    // Delphi protocol enforcement: block ALL non-procedural broadcasts during active Delphi.
    // Applies to the ENTIRE Delphi lifecycle (all phases, not just submitting).
    // Oxford/red_team/continuous allow public broadcasts — only Delphi is restricted.
    // Directed messages (to specific roles) are always allowed — agents need to coordinate.
    //
    // Allowed through:
    //   - type:"submission" (participant blind submissions to moderator)
    //   - type:"moderation" (system/moderator procedural announcements)
    //   - Messages from human (always exempt)
    //   - Directed messages (to != "all") — not affected by this check
    //
    // Blocked:
    //   - type:"broadcast" from ANYONE including the moderator (prevents leaking reference material)
    //   - type:"status", "answer", "directive", etc. to "all" from any non-human
    {
        let moderator = disc.get("moderator").and_then(|v| v.as_str()).unwrap_or("unknown");
        let from = format!("{}:{}", state.role, state.instance);

        if disc_active && disc_format == "delphi"
            && msg_type != "submission"
            && msg_type != "moderation"
            && to == "all"
            && state.role != "human"
        {
            if from == moderator {
                eprintln!("[delphi-reject] Blocked moderator broadcast from {} during active Delphi (type: {}, to: all). Use type: moderation for procedural announcements.", from, msg_type);
                return Err(
                    "Active Delphi discussion — moderator broadcasts to \"all\" are blocked. \
                    Use type: \"moderation\" for procedural round announcements. \
                    Directed messages to specific participants are still allowed.".to_string()
                );
            }
            let phase = disc.get("phase").and_then(|v| v.as_str()).unwrap_or("");
            eprintln!("[delphi-reject] Blocked broadcast from {} during active Delphi (phase: {}, type: {}, to: all)", from, phase, msg_type);
            return Err(format!(
                "Active Delphi discussion — broadcasts to \"all\" are blocked to preserve blind submission integrity. \
                To submit your position, use type: \"submission\" addressed to the moderator ({}). \
                Directed messages to specific roles are still allowed.",
                moderator
            ));
        }
    }

    let result = with_file_lock(&state.project_dir, || {
        let msg_id = next_message_id(&state.project_dir);
        let from_label = format!("{}:{}", state.role, state.instance);
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

        // Continuous review: notify moderator when developer posts a status
        // (moderator decides whether to open a review round)
        if msg_type == "status" {
            let disc = read_discussion_state(&state.project_dir);
            let is_active = disc.get("active").and_then(|v| v.as_bool()).unwrap_or(false);
            let mode = disc.get("mode").and_then(|v| v.as_str()).unwrap_or("");
            let moderator = disc.get("moderator").and_then(|v| v.as_str()).unwrap_or("").to_string();
            if is_active && mode == "continuous" && from_label != moderator && !moderator.is_empty() {
                // Dedup: skip if there's already a pending auto_review_prompt
                // (no review round opened since the last notification)
                let phase = disc.get("phase").and_then(|v| v.as_str()).unwrap_or("");
                let has_open_round = phase == "submitting";
                let board_content = std::fs::read_to_string(
                    board_jsonl_path(&state.project_dir)
                ).unwrap_or_default();
                let has_pending_prompt = board_content.lines().rev().take(50).any(|line| {
                    line.contains("auto_review_prompt") && line.contains(&moderator)
                });
                if has_pending_prompt && !has_open_round {
                    // Already notified, moderator hasn't acted yet — skip
                } else {

                let notify_id = next_message_id(&state.project_dir);
                let notification = serde_json::json!({
                    "id": notify_id,
                    "from": "system",
                    "to": moderator,
                    "type": "status",
                    "timestamp": utc_now_iso(),
                    "subject": format!("Status update from {} — open a review round?", from_label),
                    "body": format!("{} posted a status: \"{}\". Use discussion_control with action create_review_round to open a review round, or ignore to skip.", from_label, subject),
                    "metadata": {
                        "auto_review_prompt": true,
                        "trigger_from": from_label.clone(),
                        "trigger_message_id": msg_id
                    }
                });
                let _ = append_to_board(&state.project_dir, &notification);

                } // else (not deduped)
            }
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

                                            // Update discussion state: close round, transition to "reviewing"
                                            // (moderator decides whether to open next round or end discussion)
                                            let mut closed = fresh_disc.clone();
                                            closed["phase"] = serde_json::json!("reviewing");
                                            closed["previous_phase"] = serde_json::json!("submitting");
                                            if let Some(rounds) = closed.get_mut("rounds").and_then(|r| r.as_array_mut()) {
                                                if let Some(last_round) = rounds.last_mut() {
                                                    last_round["closed_at"] = serde_json::json!(now);
                                                    last_round["aggregate_message_id"] = serde_json::json!(agg_msg_id);
                                                }
                                            }

                                            if let Err(e) = write_discussion_state(&state.project_dir, &closed) {
                                                eprintln!("[auto-close] ERROR writing discussion state: {}", e);
                                            } else {
                                                eprintln!("[auto-close] Round {} auto-closed, aggregate posted as msg {}", round_num, agg_msg_id);
                                            }

                                            // Notify the moderator to decide next steps
                                            let mod_notify_id = next_message_id(&state.project_dir);
                                            let mod_notification = serde_json::json!({
                                                "id": mod_notify_id,
                                                "from": "system",
                                                "to": moderator,
                                                "type": "status",
                                                "timestamp": now,
                                                "subject": format!("Round {} complete — all submissions in", round_num),
                                                "body": format!("All {} submissions received and aggregate posted. Use discussion_control with action open_next_round to start another round, or end_discussion to conclude.", expected),
                                                "metadata": {
                                                    "round_complete": true,
                                                    "round": round_num
                                                }
                                            });
                                            let _ = append_to_board(&state.project_dir, &mod_notification);

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
    notify_desktop();

    // Reset hook compliance tracker — this session just sent a message
    write_send_tracker(&state.project_dir, &state.session_id, 0);

    // Consecutive mode turn state management
    let work_mode = config.get("settings")
        .and_then(|s| s.get("work_mode"))
        .and_then(|w| w.as_str())
        .unwrap_or("simultaneous");

    if work_mode == "consecutive" {
        let from_label = format!("{}:{}", state.role, state.instance);

        // Only human broadcasts trigger new turn rounds — manager broadcasts don't
        if to == "all" && state.role == "human" {
            let trigger_text = format!("{} {}", subject, body);
            let _ = reset_turn_state(&state.project_dir, result, &trigger_text);
        }

        // If an agent responds to a broadcast (not a directed message), advance turn
        // Check if body starts with "PASS:" to detect a pass
        if to == "all" && state.role != "human" {
            let is_pass = body.trim().to_uppercase().starts_with("PASS:");
            let _ = advance_turn(&state.project_dir, &from_label, is_pass);
        }
    }

    // ── Sequential-turn advance (pr-seq-1) ──
    //
    // When the current turn-holder posts a message tagged `metadata.end_of_turn == true`,
    // move them to `queue_completed` and promote the next entry in `queue_remaining`.
    // If the queue is empty, mark the sequence inactive — no auto-restart, per
    // manager msg 377 (explicit end only). A system message records the advance
    // so the board has an audit trail. See `feedback_pipeline_checkin_first`:
    // only messages tagged `end_of_turn: true` close the turn; all other
    // messages are treated as in-progress (multi-message turns supported).
    {
        let disc = read_discussion_state(&state.project_dir);
        let is_end_of_turn = metadata
            .as_ref()
            .and_then(|m| m.get("end_of_turn"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if is_end_of_turn {
            if let Some(seq) = disc.get("active_sequence") {
                let seq_active = seq.get("active").and_then(|v| v.as_bool()).unwrap_or(false);
                let current_holder = seq.get("current_holder").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let from_label = format!("{}:{}", state.role, state.instance);
                if seq_active && current_holder == from_label {
                    let mut updated = disc.clone();
                    let queue_remaining: Vec<serde_json::Value> = updated
                        .get("active_sequence")
                        .and_then(|s| s.get("queue_remaining"))
                        .and_then(|q| q.as_array())
                        .cloned()
                        .unwrap_or_default();
                    let mut queue_completed: Vec<serde_json::Value> = updated
                        .get("active_sequence")
                        .and_then(|s| s.get("queue_completed"))
                        .and_then(|q| q.as_array())
                        .cloned()
                        .unwrap_or_default();
                    queue_completed.push(serde_json::json!(current_holder));
                    let (next_holder, new_remaining) = if queue_remaining.is_empty() {
                        (String::new(), vec![])
                    } else {
                        let next = queue_remaining[0].as_str().unwrap_or("").to_string();
                        let rest = queue_remaining[1..].to_vec();
                        (next, rest)
                    };
                    let now_iso = utc_now_iso();
                    if let Some(seq_obj) = updated.get_mut("active_sequence").and_then(|s| s.as_object_mut()) {
                        seq_obj.insert("queue_remaining".to_string(), serde_json::json!(new_remaining));
                        seq_obj.insert("queue_completed".to_string(), serde_json::json!(queue_completed));
                        if next_holder.is_empty() {
                            seq_obj.insert("active".to_string(), serde_json::json!(false));
                            seq_obj.insert("current_holder".to_string(), serde_json::json!(""));
                            seq_obj.insert("ended_at".to_string(), serde_json::json!(now_iso.clone()));
                            seq_obj.insert("ended_by".to_string(), serde_json::json!("queue_exhausted"));
                        } else {
                            seq_obj.insert("current_holder".to_string(), serde_json::json!(next_holder.clone()));
                            seq_obj.insert("turn_started_at".to_string(), serde_json::json!(now_iso.clone()));
                        }
                    }
                    let _ = write_discussion_state(&state.project_dir, &updated);
                    if next_holder.is_empty() {
                        post_turn_system_message(
                            &state.project_dir,
                            &format!("Sequence ended — {} completed the final turn. Queue exhausted.", from_label),
                        );
                    } else {
                        post_turn_system_message(
                            &state.project_dir,
                            &format!("Turn advanced — {} → {} (end_of_turn). {} remaining in queue.",
                                from_label, next_holder, new_remaining.len()),
                        );
                        let wake_msg = serde_json::json!({
                            "id": next_message_id(&state.project_dir),
                            "from": "system:0",
                            "to": next_holder,
                            "type": "system",
                            "subject": "Your sequential turn",
                            "body": format!("It is now your turn in the sequential sequence. Previous holder: {}. When you finish, post with metadata.end_of_turn=true to advance.", from_label),
                            "timestamp": now_iso,
                            "metadata": {"sequence_notification": true, "previous_holder": from_label}
                        });
                        let _ = append_to_board(&state.project_dir, &wake_msg);
                    }
                }
            }
        }
    }

    // Pipeline discussion: advance stage when current stage agent broadcasts
    {
        let disc = read_discussion_state(&state.project_dir);
        let disc_active = disc.get("active").and_then(|v| v.as_bool()).unwrap_or(false);
        let disc_mode = disc.get("mode").and_then(|v| v.as_str()).unwrap_or("");
        if disc_active && disc_mode == "pipeline" && to == "all" {
            let pipeline_order = disc.get("pipeline_order").and_then(|o| o.as_array()).cloned().unwrap_or_default();
            let current_stage = disc.get("pipeline_stage").and_then(|s| s.as_u64()).unwrap_or(0) as usize;
            let from_label = format!("{}:{}", state.role, state.instance);
            let current_agent = pipeline_order.get(current_stage).and_then(|v| v.as_str()).unwrap_or("");

            if from_label == current_agent {
                let mut updated_disc = disc.clone();
                // Record this stage's output
                let mut outputs = updated_disc.get("pipeline_outputs").and_then(|o| o.as_array()).cloned().unwrap_or_default();
                outputs.push(serde_json::json!({
                    "stage": current_stage,
                    "agent": from_label,
                    "message_id": result,
                    "timestamp": utc_now_iso()
                }));
                updated_disc["pipeline_outputs"] = serde_json::json!(outputs);

                let next_stage = current_stage + 1;
                if next_stage >= pipeline_order.len() {
                    // All stages complete for this round — check termination strategy
                    let typed_disc = read_discussion_typed(&state.project_dir);
                    let termination = typed_disc.settings.effective_termination();
                    let current_round = updated_disc.get("current_round").and_then(|v| v.as_u64()).unwrap_or(0);

                    let should_loop = match &termination {
                        vaak_desktop::collab::TerminationStrategy::FixedRounds { rounds } => current_round + 1 < *rounds as u64,
                        vaak_desktop::collab::TerminationStrategy::Unlimited => true,
                        vaak_desktop::collab::TerminationStrategy::Consensus { .. } => true, // moderator decides
                        vaak_desktop::collab::TerminationStrategy::ModeratorCall => true, // moderator decides
                        vaak_desktop::collab::TerminationStrategy::TimeBound { .. } => true, // time decides
                    };

                    // Stagnation detection: auto-close if N consecutive rounds have no substantive output
                    let stagnant_rounds = updated_disc.get("stagnant_rounds").and_then(|v| v.as_u64()).unwrap_or(0);
                    let disc_mode = updated_disc.get("mode").and_then(|v| v.as_str()).unwrap_or("");
                    let max_stagnant = if disc_mode == "pipeline" { 1u64 } else { 3u64 };
                    let round_is_stagnant = {
                        // Check if all pipeline_outputs for this round are short (< 100 chars body)
                        let round_start_stage = 0usize;
                        let round_outputs = outputs.iter().filter(|o| {
                            o.get("stage").and_then(|s| s.as_u64()).map(|s| s as usize) >= Some(round_start_stage)
                        });
                        // Read the actual message bodies from the board
                        let board = read_board(&state.project_dir);
                        let mut all_short = true;
                        for output in round_outputs {
                            let msg_id = output.get("message_id").and_then(|i| i.as_u64()).unwrap_or(0);
                            if let Some(msg) = board.iter().find(|m| m.get("id").and_then(|i| i.as_u64()) == Some(msg_id)) {
                                let msg_body = msg.get("body").and_then(|b| b.as_str()).unwrap_or("");
                                if msg_body.len() >= 100 {
                                    all_short = false;
                                    break;
                                }
                            }
                        }
                        all_short && current_round > 0 // Don't count round 0 as stagnant
                    };
                    let new_stagnant = if round_is_stagnant { stagnant_rounds + 1 } else { 0 };
                    updated_disc["stagnant_rounds"] = serde_json::json!(new_stagnant);

                    // Consensus detection: auto-close if every pipeline participant voted `accept`
                    // in the just-completed round.
                    // Why: the stagnation check above uses body-length <100 chars and misses
                    // verbose stall-close content (see msgs 89/92/95/98/101/104/107 from this
                    // very conversation). Metadata-based consensus catches unanimity directly.
                    // Semantics per developer msg 155 + locked with dev-challenger msg 151:
                    //   snapshot (latest vote per role), round-boundary (single round window),
                    //   count every participant, read both `vote` and `synthesis_vote_reaffirmed.choice`.
                    let round_is_consensus = {
                        let round_len = pipeline_order.len();
                        let round_start_idx = outputs.len().saturating_sub(round_len);
                        let round_msg_ids: Vec<u64> = outputs[round_start_idx..].iter()
                            .filter_map(|o| o.get("message_id").and_then(|i| i.as_u64()))
                            .collect();
                        let participants: Vec<&str> = pipeline_order.iter()
                            .filter_map(|v| v.as_str())
                            .collect();
                        let board = read_board(&state.project_dir);
                        round_reached_consensus(&participants, &round_msg_ids, &board)
                    };

                    // Check if discussion is paused — don't auto-advance
                    let is_paused = updated_disc.get("paused_at").and_then(|v| v.as_str()).is_some();
                    if is_paused {
                        let _ = write_discussion_state(&state.project_dir, &updated_disc);
                        post_turn_system_message(&state.project_dir,
                            &format!("Round {} complete — discussion is paused. Waiting for resume.", current_round + 1));
                    } else if should_loop && round_is_consensus {
                        // Consensus reached — auto-close with distinct reason.
                        // Why: dev-challenger msg 172 attack 3 — consensus-close and stagnation-close
                        // are semantically different failure/success modes; the board message should
                        // let the UI and human distinguish them.
                        updated_disc["active"] = serde_json::json!(false);
                        updated_disc["phase"] = serde_json::json!("pipeline_complete");
                        updated_disc["terminated_by"] = serde_json::json!("consensus");
                        let _ = write_discussion_state(&state.project_dir, &updated_disc);
                        post_turn_system_message(&state.project_dir,
                            "Pipeline auto-closed: consensus reached. Every participant voted `accept` in the final round. See decisions record for accepted items.");
                    } else if should_loop && new_stagnant >= max_stagnant {
                        // Stagnation limit reached — auto-close
                        updated_disc["active"] = serde_json::json!(false);
                        updated_disc["phase"] = serde_json::json!("pipeline_complete");
                        updated_disc["terminated_by"] = serde_json::json!("stagnation");
                        let _ = write_discussion_state(&state.project_dir, &updated_disc);
                        post_turn_system_message(&state.project_dir,
                            &format!("Discussion auto-closed: {} consecutive rounds with no substantive output. Start a new discussion when there's work to discuss.", max_stagnant));
                    } else if should_loop {
                        // Auto-advance to next round
                        let next_round = current_round + 1;
                        updated_disc["pipeline_stage"] = serde_json::json!(0);
                        updated_disc["pipeline_stage_started_at"] = serde_json::json!(utc_now_iso());
                        updated_disc["current_round"] = serde_json::json!(next_round);
                        let _ = write_discussion_state(&state.project_dir, &updated_disc);

                        let first_agent = pipeline_order.first().and_then(|v| v.as_str()).unwrap_or("?");
                        post_turn_system_message(&state.project_dir, &format!("Round {} complete — starting round {}. First up: {}", current_round + 1, next_round + 1, first_agent));

                        // Wake first agent for the new round
                        let wake_msg = serde_json::json!({
                            "id": next_message_id(&state.project_dir),
                            "from": "system:0",
                            "to": first_agent,
                            "type": "system",
                            "subject": "Your pipeline stage",
                            "body": format!("Round {} has started. It is now your turn in the pipeline (stage 1/{}). Review previous round outputs and broadcast your response.", next_round + 1, pipeline_order.len()),
                            "timestamp": utc_now_iso(),
                            "metadata": {"pipeline_notification": true}
                        });
                        let _ = append_to_board(&state.project_dir, &wake_msg);
                    } else {
                        // Pipeline truly complete — all rounds done
                        updated_disc["phase"] = serde_json::json!("pipeline_complete");
                        updated_disc["pipeline_stage"] = serde_json::json!(next_stage);
                        let _ = write_discussion_state(&state.project_dir, &updated_disc);
                        post_turn_system_message(&state.project_dir, "Pipeline complete — all stages finished");
                    }
                } else {
                    // Check if paused before advancing to next stage
                    let is_paused = updated_disc.get("paused_at").and_then(|v| v.as_str()).is_some();
                    if is_paused {
                        updated_disc["pipeline_stage"] = serde_json::json!(next_stage);
                        let _ = write_discussion_state(&state.project_dir, &updated_disc);
                        post_turn_system_message(&state.project_dir, &format!("Stage {} complete ({}) — discussion is paused. Next up: stage {}/{} when resumed.", current_stage + 1, from_label, next_stage + 1, pipeline_order.len()));
                    } else {
                        // Advance to next stage
                        updated_disc["pipeline_stage"] = serde_json::json!(next_stage);
                        updated_disc["pipeline_stage_started_at"] = serde_json::json!(utc_now_iso());
                        let _ = write_discussion_state(&state.project_dir, &updated_disc);
                        let next_agent = pipeline_order.get(next_stage).and_then(|v| v.as_str()).unwrap_or("?");
                        post_turn_system_message(&state.project_dir, &format!("Stage {} complete ({}) — next: {} ({}/{})", current_stage + 1, from_label, next_agent, next_stage + 1, pipeline_order.len()));
                        // Wake next agent (use full role:instance so only the specific instance is notified)
                        let wake_msg = serde_json::json!({
                            "id": next_message_id(&state.project_dir),
                            "from": "system:0",
                            "to": next_agent,
                            "type": "system",
                            "subject": "Your pipeline stage",
                            "body": format!("It is now your turn in the pipeline (stage {}/{}). Review previous outputs and broadcast your response.", next_stage + 1, pipeline_order.len()),
                            "timestamp": utc_now_iso(),
                            "metadata": {"pipeline_notification": true}
                        });
                        let _ = append_to_board(&state.project_dir, &wake_msg);
                    }
                }
            }
        }
    }

    Ok(serde_json::json!({
        "message_id": result,
        "delivered_to": [to]
    }))
}

/// Handle project_check: read new messages
fn handle_project_check(last_seen: u64) -> Result<serde_json::Value, String> {
    let state = get_or_rejoin_state()?;

    // If caller passes last_seen=0, use the stored last_seen_id from file
    // to prevent re-delivering messages already seen via project_join or hook
    let effective_last_seen = if last_seen == 0 {
        let ls_path = last_seen_path(&state.project_dir, &state.session_id);
        std::fs::read_to_string(&ls_path)
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .and_then(|j| j.get("last_seen_id")?.as_u64())
            .unwrap_or(0)
    } else {
        last_seen
    };

    let my_instance_label = format!("{}:{}", state.role, state.instance);
    let has_see_all = role_has_see_all(&state.project_dir, &state.role);
    let all_messages = read_board_filtered(&state.project_dir);
    let my_messages: Vec<&serde_json::Value> = all_messages.iter()
        .filter(|m| {
            if has_see_all { return true; }
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
    let ls_path = last_seen_path(&state.project_dir, &session_id);

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
            polls_since_heartbeat = 0;
        }

        // Pipeline mode: check for stale stage holder and auto-skip
        // (per human msg 511 ask #4 — pipeline shouldn't stall when an agent
        // closes its terminal mid-stage).
        {
            let stage_timeout = pipeline_stage_timeout_secs(&state.project_dir);
            if let Ok(true) = auto_skip_stale_pipeline_stage(&state.project_dir, stage_timeout) {
                eprintln!("[project_wait watchdog] pipeline stage auto-skipped (stale holder)");
            }
        }

        // Consecutive mode: check for stale turns and auto-advance (timeout watchdog)
        {
            let config: serde_json::Value = std::fs::read_to_string(
                std::path::Path::new(&state.project_dir).join(".vaak").join("project.json")
            ).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(serde_json::json!({}));
            let wm = config.get("settings").and_then(|s| s.get("work_mode")).and_then(|w| w.as_str()).unwrap_or("simultaneous");
            if wm == "consecutive" {
                let ts = read_turn_state(&state.project_dir);
                let completed = ts.get("completed").and_then(|c| c.as_bool()).unwrap_or(true);
                if !completed {
                    let timeout = config.get("settings").and_then(|s| s.get("consecutive_timeout_secs")).and_then(|t| t.as_u64()).unwrap_or(120);
                    let turn_started = ts.get("turn_started_at").or_else(|| ts.get("started_at")).and_then(|t| t.as_str()).unwrap_or("");
                    if !turn_started.is_empty() {
                        let now_str = utc_now_iso();
                        if let (Some(start), Some(now)) = (parse_iso_to_epoch_secs(turn_started), parse_iso_to_epoch_secs(&now_str)) {
                            if now > start + timeout {
                                let order = ts.get("relevance_order").and_then(|t| t.as_array()).cloned().unwrap_or_default();
                                let idx = ts.get("current_index").and_then(|i| i.as_u64()).unwrap_or(0) as usize;
                                if let Some(stale) = order.get(idx).and_then(|v| v.as_str()) {
                                    eprintln!("[project_wait watchdog] Timeout: skipping {} after {}s", stale, timeout);
                                    post_turn_system_message(&state.project_dir, &format!("{} skipped (timeout {}s)", stale, timeout));
                                    let _ = advance_turn(&state.project_dir, stale, true);
                                }
                            }
                        }
                    }
                }
            }
        }

        // Read current last_seen
        let last_seen_id: u64 = std::fs::read_to_string(&ls_path)
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .and_then(|j| j.get("last_seen_id")?.as_u64())
            .unwrap_or(0);

        // Check for new messages
        let wait_instance_label = format!("{}:{}", state.role, state.instance);
        let has_see_all = role_has_see_all(&state.project_dir, &state.role);
        let all_messages = read_board_filtered(&state.project_dir);
        let new_messages: Vec<serde_json::Value> = all_messages.into_iter()
            .filter(|m| {
                if has_see_all { return true; }
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
                let ls_dir = vaak_dir(&state.project_dir).join("last-seen");
                let _ = std::fs::create_dir_all(&ls_dir);
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
            return Ok(serde_json::json!({
                "status": "timeout",
                "messages": [],
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

    let mut roles_status = Vec::new();
    if let Some(roles_obj) = config.get("roles").and_then(|r| r.as_object()) {
        for (slug, rdef) in roles_obj {
            let title = rdef.get("title").and_then(|t| t.as_str()).unwrap_or(slug);
            let max = rdef.get("max_instances").and_then(|m| m.as_u64()).unwrap_or(1);
            let active = bindings.iter()
                .filter(|b| b.get("role").and_then(|r| r.as_str()) == Some(slug.as_str())
                    && b.get("status").and_then(|s| s.as_str()) == Some("active"))
                .count();
            roles_status.push(serde_json::json!({
                "slug": slug,
                "title": title,
                "active_instances": active,
                "max_instances": max,
                "status": if active > 0 { "active" } else { "vacant" }
            }));
        }
    }

    let active_section = get_active_section(&state.project_dir);

    Ok(serde_json::json!({
        "project_name": project_name,
        "your_role": state.role,
        "your_instance": state.instance,
        "roles": roles_status,
        "pending_messages": my_messages.len(),
        "total_messages": all_messages.len(),
        "active_section": active_section
    }))
}

/// Handle project_leave: release role binding
fn handle_project_leave() -> Result<serde_json::Value, String> {
    let state = get_or_rejoin_state()?;

    with_file_lock(&state.project_dir, || {
        let mut sessions = read_sessions(&state.project_dir);
        if let Some(bindings) = sessions.get_mut("bindings").and_then(|b| b.as_array_mut()) {
            bindings.retain(|b| {
                b.get("session_id").and_then(|s| s.as_str()) != Some(&state.session_id)
            });
        }
        write_sessions(&state.project_dir, &sessions)?;
        Ok(())
    })?;

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

/// Check if a role has the "see_all" permission in project config.
/// Roles with this permission can see all messages regardless of the `to` field.
fn role_has_see_all(project_dir: &str, role: &str) -> bool {
    read_project_config(project_dir)
        .ok()
        .and_then(|config| {
            config.get("roles")?
                .get(role)?
                .get("permissions")?
                .as_array()
                .map(|arr| arr.iter().any(|v| v.as_str() == Some("see_all")))
        })
        .unwrap_or(false)
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
        Ok(())
    })?;

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

    // Re-inject abbreviated role briefing to prevent context drift.
    // Full briefing is shown at join; here we inject key boundaries/anti-patterns.
    let briefing_path = role_briefing_path(&project_dir, my_role);
    let role_reminder = if let Ok(briefing_text) = std::fs::read_to_string(&briefing_path) {
        // Extract lines containing key boundary markers
        let mut reminder_lines = Vec::new();
        let mut in_relevant_section = false;
        for line in briefing_text.lines() {
            if line.starts_with("## Role Boundaries") || line.starts_with("## Anti-patterns")
                || line.starts_with("## Core Responsibilities") || line.contains("YOU DO NOT")
                || line.contains("YOU OWN") || line.contains("NEVER") {
                in_relevant_section = true;
            } else if line.starts_with("## ") && in_relevant_section {
                in_relevant_section = false;
            }
            if in_relevant_section {
                reminder_lines.push(line);
            }
        }
        if !reminder_lines.is_empty() {
            format!("\nROLE REMINDER (from your briefing):\n{}", reminder_lines.join("\n"))
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    // Read board.jsonl, filter for messages to my role or my specific instance
    // Roles with "see_all" permission see all messages
    let my_instance_label = format!("{}:{}", my_role, my_instance);
    let has_see_all = role_has_see_all(&project_dir, my_role);
    let all_messages = read_board_filtered(&project_dir);
    let my_messages: Vec<&serde_json::Value> = all_messages.iter()
        .filter(|m| {
            if has_see_all { return true; }
            let to = m.get("to").and_then(|t| t.as_str()).unwrap_or("");
            to == my_role || to == my_instance_label || to == "all"
        })
        .collect();

    // Read last-seen tracking
    let ls_path = last_seen_path(&project_dir, session_id);
    let last_seen_id: u64 = std::fs::read_to_string(&ls_path)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|j| j.get("last_seen_id")?.as_u64())
        .unwrap_or(0);

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

    let mut output = format!(
        "TEAM: You are the {} (instance {}) on project \"{}\".{} Team: {}.",
        role_title, my_instance, project_name, section_label, team_parts.join(", ")
    );

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

    // Read communication-visibility mode — defaults to "directed" (agents only
    // see messages addressed to them + human messages). Per pr-r2-data-fields:
    // canonical key is `session_mode`; legacy `discussion_mode` is read as a
    // fallback so projects whose project.json hasn't been migrated still work.
    let discussion_mode = config.get("settings")
        .and_then(|s| s.get("session_mode").or_else(|| s.get("discussion_mode")))
        .and_then(|m| m.as_str())
        .unwrap_or("directed");
    output.push_str(&format!(" Session mode: {}.", discussion_mode));

    // Inject self-selection rules — anti-convergence design
    output.push_str("\nRESPONSE RULES: ONLY respond when a message is ADDRESSED TO YOU (your role name appears in the 'to' field) OR when you have a genuinely DIFFERENT perspective that nobody else has stated. If the human addresses you by name, respond immediately. NEVER echo — if someone already said what you'd say, stay SILENT. Silence is better than overlap. Before responding to any broadcast message, ask: 'Would my response be meaningfully different from what's already been said?' If not, do not respond. When multiple agents need to act on the same message, only the ADDRESSED role should respond. Others observe silently unless they disagree or have unique expertise to add.");
    output.push_str("\nANTI-ANCHORING: STOP. Before reading the messages below, form your OWN position on the human's last request. Write down your initial take FIRST. Then read the thread. If your position changed after reading others, ask yourself whether you genuinely changed your mind or just anchored to the first response you saw. Convergence is the default failure mode — fight it actively.");
    output.push_str("\nSOURCE OF TRUTH: The human's most recent message is your primary source of truth. Form your OWN understanding of what the human said before reading other team members' interpretations. If a team member's interpretation contradicts the human's words, trust the human's words.");
    output.push_str("\nTOKEN EFFICIENCY: Do NOT call project_check redundantly. The messages shown below ARE your latest messages — you already have them. Use project_wait to block until NEW messages arrive. Do NOT call project_check(0) to re-read history you've already seen. Do NOT re-read board.jsonl or discussion.json when the state is already shown above. Every unnecessary tool call wastes tokens.");
    output.push_str("\nSECTION DISCIPLINE: You are in your assigned section. Do NOT switch sections unless YOU are specifically named in a switch request. If the human asks another agent to switch sections, STAY WHERE YOU ARE. Do not follow other agents between sections. Each section has its own team — only move if explicitly told to by the human or manager.");
    output.push_str("\nCOMMUNICATION RULES (enforced by system):\n- Talk to the relevant peer role, not the human. Only the Manager relays to the human.\n- Default to silence — if you have no assigned work, say nothing.\n- Stay in your role's scope. If something is outside your domain, send it to the right role.");

    // Consecutive mode — brief queue position indicator
    let work_mode = config.get("settings")
        .and_then(|s| s.get("work_mode"))
        .and_then(|w| w.as_str())
        .unwrap_or("simultaneous");

    if work_mode == "consecutive" {
        let turn_state = read_turn_state(&project_dir);
        let turn_completed = turn_state.get("completed").and_then(|c| c.as_bool()).unwrap_or(true);

        if !turn_completed {
            let relevance_order = turn_state.get("relevance_order")
                .and_then(|t| t.as_array())
                .cloned()
                .unwrap_or_default();
            let current_idx = turn_state.get("current_index")
                .and_then(|i| i.as_u64())
                .unwrap_or(0) as usize;
            let current_turn = relevance_order.get(current_idx)
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let my_label = format!("{}:{}", my_role, my_instance);
            let my_position = relevance_order.iter()
                .position(|v| v.as_str() == Some(&my_label));

            if my_role == "manager" {
                output.push_str("\nCONSECUTIVE MODE: You have PRIORITY — you may respond at any time.");
            } else if my_label == current_turn {
                output.push_str("\nCONSECUTIVE MODE: It is YOUR TURN. Respond now, or send PASS: to skip.");
            } else if let Some(pos) = my_position {
                output.push_str(&format!(
                    "\nCONSECUTIVE MODE: You are #{} in queue. Current turn: {}. project_send will block until your turn.",
                    pos + 1, current_turn
                ));
            }
        }

        output.push_str("\nWork mode: consecutive.");
    } else {
        output.push_str("\nWork mode: simultaneous.");
    }

    // Inject abbreviated role briefing reminder (boundaries, anti-patterns)
    if !role_reminder.is_empty() {
        output.push_str(&role_reminder);
    }

    // Universal role boundary enforcement — always injected regardless of briefing content
    output.push_str(&format!(
        "\n\nROLE BOUNDARY ENFORCEMENT: You are {}:{}. Stay STRICTLY within your role's scope. \
        If a task falls outside your responsibilities, hand it off to the appropriate role using \
        project_send. Do NOT perform work that belongs to another role — delegate instead. \
        Do NOT make decisions reserved for other roles (e.g., architecture decisions belong to \
        architect, task assignments belong to manager). When uncertain, ASK before acting.",
        my_role, my_instance
    ));

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

    // Pipeline discussion context injection
    if disc_active && disc_mode == "pipeline" {
        let disc_fresh = read_discussion_state(&project_dir);
        let pipeline_order = disc_fresh.get("pipeline_order").and_then(|o| o.as_array()).cloned().unwrap_or_default();
        let current_stage = disc_fresh.get("pipeline_stage").and_then(|s| s.as_u64()).unwrap_or(0) as usize;
        let pipeline_outputs = disc_fresh.get("pipeline_outputs").and_then(|o| o.as_array()).cloned().unwrap_or_default();
        let pipeline_phase = disc_fresh.get("phase").and_then(|v| v.as_str()).unwrap_or("");
        // pr-r2-pipeline-mode-value: normalize legacy "discussion" to "review"
        // so prompt strings always render the canonical name regardless of
        // when the discussion.json was written.
        let pipeline_mode_raw = disc_fresh.get("pipeline_mode").and_then(|v| v.as_str()).unwrap_or("review");
        let pipeline_mode = if pipeline_mode_raw == "discussion" { "review" } else { pipeline_mode_raw };
        let my_label = format!("{}:{}", my_role, my_instance);
        let current_agent = pipeline_order.get(current_stage).and_then(|v| v.as_str()).unwrap_or("");
        let order_display = pipeline_order.iter().enumerate()
            .map(|(i, v)| {
                let name = v.as_str().unwrap_or("?");
                if i == current_stage && pipeline_phase != "pipeline_complete" {
                    format!("**[{}. {}]**", i + 1, name)
                } else if i < current_stage || pipeline_phase == "pipeline_complete" {
                    format!("~~{}. {}~~", i + 1, name)
                } else {
                    format!("{}. {}", i + 1, name)
                }
            })
            .collect::<Vec<_>>()
            .join(" → ");

        if pipeline_phase == "pipeline_complete" {
            output.push_str(&format!("\n\nPIPELINE COMPLETE: All {} stages finished.\nTopic: {}\nOrder: {}",
                pipeline_order.len(), disc_topic, order_display));
        } else if my_label == current_agent {
            output.push_str(&format!("\n\nPIPELINE — YOUR STAGE ({}/{}) [{}]: You are the current stage. Topic: \"{}\"\nOrder: {}",
                current_stage + 1, pipeline_order.len(), pipeline_mode.to_uppercase(), disc_topic, order_display));
            // Show previous stage outputs
            if !pipeline_outputs.is_empty() {
                output.push_str("\n\nPREVIOUS STAGE OUTPUTS (build on these):");
                for po in &pipeline_outputs {
                    let agent = po.get("agent").and_then(|a| a.as_str()).unwrap_or("?");
                    let msg_id = po.get("message_id").and_then(|i| i.as_u64()).unwrap_or(0);
                    output.push_str(&format!("\n  - Stage {}: {} (msg #{})", po.get("stage").and_then(|s| s.as_u64()).map(|s| s + 1).unwrap_or(0), agent, msg_id));
                }
            }
            output.push_str("\n\nBroadcast your response when ready. Your output becomes input for the next stage.");
        } else if my_role == "moderator" || my_role == "manager" {
            output.push_str(&format!("\n\nPIPELINE ORCHESTRATOR VIEW [{}] — Current stage: {} ({}/{}). Topic: \"{}\"\nOrder: {}",
                pipeline_mode.to_uppercase(), current_agent, current_stage + 1, pipeline_order.len(), disc_topic, order_display));
            if !pipeline_outputs.is_empty() {
                output.push_str("\n\nSTAGE OUTPUTS SO FAR:");
                for po in &pipeline_outputs {
                    let agent = po.get("agent").and_then(|a| a.as_str()).unwrap_or("?");
                    let msg_id = po.get("message_id").and_then(|i| i.as_u64()).unwrap_or(0);
                    output.push_str(&format!("\n  - Stage {}: {} (msg #{})", po.get("stage").and_then(|s| s.as_u64()).map(|s| s + 1).unwrap_or(0), agent, msg_id));
                }
            }
            output.push_str("\n\nTo override the next stage: call discussion_control with action: 'pipeline_next', topic: 'role:instance'");
        } else {
            output.push_str(&format!("\n\nPIPELINE ACTIVE [{}] — Not your stage. Current: {} ({}/{}). Topic: \"{}\"\nOrder: {}\nWait for your turn. Call project_wait to enter standby.",
                pipeline_mode.to_uppercase(), current_agent, current_stage + 1, pipeline_order.len(), disc_topic, order_display));
        }
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

    // Consecutive mode read-side gating: suppress messages for out-of-turn agents
    let consecutive_suppressed = {
        let wm = config.get("settings")
            .and_then(|s| s.get("work_mode"))
            .and_then(|w| w.as_str())
            .unwrap_or("simultaneous");
        if wm == "consecutive" && my_role != "manager" && my_role != "human" {
            let ts = read_turn_state(&project_dir);
            let completed = ts.get("completed").and_then(|c| c.as_bool()).unwrap_or(true);
            if !completed {
                let order = ts.get("relevance_order").and_then(|t| t.as_array()).cloned().unwrap_or_default();
                let idx = ts.get("current_index").and_then(|i| i.as_u64()).unwrap_or(0) as usize;
                let current = order.get(idx).and_then(|v| v.as_str()).unwrap_or("");
                let me = format!("{}:{}", my_role, my_instance);
                me != current
            } else {
                false
            }
        } else {
            false
        }
    };

    if consecutive_suppressed {
        output.push_str("\n\nCONSECUTIVE MODE: It is NOT your turn. You may READ the messages below to prepare, but do NOT broadcast a response yet. Call project_wait to enter standby until your turn arrives. Broadcasts will be blocked until then.");
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
            let is_manager = my_role == "manager";
            if discussion_mode == "directed" && !is_from_human && !is_directed_to_me && !is_broadcast && !is_from_me && !is_manager {
                continue; // Skip — between other roles, not relevant to us in Directed mode (managers see all)
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
    // Don't advance last_seen_id when messages are suppressed (consecutive mode, not your turn)
    // so the agent sees them when their turn arrives
    let max_hook_id = new_messages.iter()
        .filter_map(|m| m.get("id").and_then(|i| i.as_u64()))
        .max();
    if let Some(max_id) = max_hook_id {
        let hook_ls_path = last_seen_path(&project_dir, session_id);
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

/// Get a stable session ID using a priority chain of methods
fn get_session_id() -> String {
    if let Ok(env_session) = std::env::var("CLAUDE_SESSION_ID") {
        if !env_session.is_empty() {
            eprintln!("[vaak-mcp] Session source: CLAUDE_SESSION_ID env var");
            return env_session;
        }
    }

    if let Ok(wt_session) = std::env::var("WT_SESSION") {
        if !wt_session.is_empty() {
            eprintln!("[vaak-mcp] Session source: Windows Terminal (WT_SESSION)");
            return format!("wt-{}", wt_session);
        }
    }

    if let Ok(iterm_session) = std::env::var("ITERM_SESSION_ID") {
        if !iterm_session.is_empty() {
            eprintln!("[vaak-mcp] Session source: iTerm2 (ITERM_SESSION_ID)");
            return format!("iterm-{}", iterm_session);
        }
    }

    if let Ok(term_session) = std::env::var("TERM_SESSION_ID") {
        if !term_session.is_empty() {
            eprintln!("[vaak-mcp] Session source: Terminal session (TERM_SESSION_ID)");
            return format!("term-{}", term_session);
        }
    }

    #[cfg(windows)]
    if let Some(hwnd) = get_console_window_handle() {
        eprintln!("[vaak-mcp] Session source: Windows console handle");
        let hostname = hostname::get()
            .map(|h| h.to_string_lossy().to_string())
            .unwrap_or_else(|_| "unknown".to_string());
        return format!("{}-console-{:x}", hostname, hwnd);
    }

    #[cfg(unix)]
    if let Some(tty) = get_tty_path() {
        eprintln!("[vaak-mcp] Session source: TTY path ({})", tty);
        let clean = tty.replace("/dev/", "").replace("/", "-");
        let hostname = hostname::get()
            .map(|h| h.to_string_lossy().to_string())
            .unwrap_or_else(|_| "unknown".to_string());
        return format!("{}-tty-{}", hostname, clean);
    }

    eprintln!("[vaak-mcp] Session source: Fallback hash");
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
        Err(_) => Err("Vaak not running".to_string())
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

    let result = match method {
        "initialize" => {
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
                    "description": "Send a message to a specific role on your team. Messages are directed - only the target role sees them. Use 'all' to broadcast to everyone.",
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
                    "description": "Control structured discussions (Delphi, Oxford, Continuous Review). Actions: start_discussion, close_round, open_next_round, end_discussion, get_state, set_teams, create_review_round, audience_control. Delphi/Oxford: manual rounds with anonymized aggregates. Continuous: moderator-controlled rounds (moderator gets notified on status updates, uses create_review_round to open rounds). audience_control: set audience state (listening/voting/qa/commenting/open) and toggle audience_enabled — only moderator can use this. High-risk actions (end_discussion, pipeline_next) require a non-empty 'reason' for audit trail.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "action": {
                                "type": "string",
                                "description": "Action to perform: start_discussion, close_round, open_next_round, end_discussion, get_state, set_teams, create_review_round, audience_control"
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
                            },
                            "reason": {
                                "type": "string",
                                "description": "Moderator justification for the action. Required for high-risk actions (end_discussion, pipeline_next). Non-empty after trim, minimum 3 chars. Recorded in audit trail."
                            },
                            "decision": {
                                "type": "object",
                                "description": "Decision record for action 'record_decision'. Shape: {claim: string, status: 'accepted'|'deferred'|'rejected', owner?: string, next_action?: string}. All moderator-only; appends to the discussion's decisions array."
                            }
                        },
                        "required": ["action"]
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
                let reason = args.get("reason").and_then(|r| r.as_str());
                let decision = args.get("decision").cloned();

                match handle_discussion_control(action, mode, topic, participants, teams, reason, decision) {
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
        let has_see_all = role_has_see_all(&project_dir, &my_role);
        let all_messages = read_board_filtered(&project_dir);
        let my_messages: Vec<&serde_json::Value> = all_messages.iter()
            .filter(|m| {
                if has_see_all { return true; }
                let to = m.get("to").and_then(|t| t.as_str()).unwrap_or("");
                to == my_role || to == stop_instance_label || to == "all"
            })
            .collect();

        let last_seen_id: u64 = session_id.as_ref()
            .and_then(|sid| {
                let ls_path = last_seen_path(&project_dir, sid);
                fs::read_to_string(&ls_path).ok()
                    .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
                    .and_then(|j| j.get("last_seen_id")?.as_u64())
            })
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
    if args.iter().any(|a| a == "--build-info") {
        println!("{}", build_info::as_json());
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
            Err(_) => break,
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

        if let Some(response) = handle_request(&request, &session_id) {
            let _ = writeln!(stdout, "{}", response);
            let _ = stdout.flush();
        }
    }

    // Cleanup: mark session as disconnected so the UI shows "gone" immediately
    update_session_activity("disconnected");
    eprintln!("[vaak-mcp] Session ended, marked as disconnected");
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── ensure_heartbeat_ticker_started (pr-sidecar-heartbeat-thread) ──

    #[test]
    fn ensure_heartbeat_ticker_started_is_idempotent() {
        // Calling twice must spawn at most one thread. The AtomicBool guard
        // ensures get-and-set semantics; second call observes "already true"
        // and returns without spawning. We can't directly count threads, but
        // we can verify the AtomicBool transitions correctly.
        use std::sync::atomic::Ordering;

        // Reset for this test (other tests may have set it; this is not a
        // production code path so the reset is acceptable test isolation).
        HEARTBEAT_TICKER_STARTED.store(false, Ordering::SeqCst);

        ensure_heartbeat_ticker_started();
        assert!(
            HEARTBEAT_TICKER_STARTED.load(Ordering::SeqCst),
            "first call must flip the guard to true"
        );

        // Second call: should be a no-op. Guard stays true.
        ensure_heartbeat_ticker_started();
        assert!(
            HEARTBEAT_TICKER_STARTED.load(Ordering::SeqCst),
            "second call must not flip the guard back"
        );

        // The thread is now running forever in this test process; harmless
        // because it no-ops when ACTIVE_PROJECT is None.
    }

    #[test]
    fn non_high_risk_action_accepts_no_reason() {
        assert!(validate_moderator_reason("pause", None).is_ok());
        assert!(validate_moderator_reason("resume", None).is_ok());
        assert!(validate_moderator_reason("get_state", None).is_ok());
        assert!(validate_moderator_reason("close_round", Some("")).is_ok());
    }

    #[test]
    fn high_risk_end_discussion_rejects_none() {
        let err = validate_moderator_reason("end_discussion", None).unwrap_err();
        assert!(err.contains("end_discussion"));
        assert!(err.contains("reason"));
    }

    #[test]
    fn high_risk_end_discussion_rejects_empty() {
        assert!(validate_moderator_reason("end_discussion", Some("")).is_err());
    }

    #[test]
    fn high_risk_end_discussion_rejects_whitespace_only() {
        assert!(validate_moderator_reason("end_discussion", Some("   ")).is_err());
        assert!(validate_moderator_reason("end_discussion", Some("\t\n")).is_err());
    }

    #[test]
    fn high_risk_end_discussion_rejects_below_min_length() {
        assert!(validate_moderator_reason("end_discussion", Some("ab")).is_err());
        assert!(validate_moderator_reason("end_discussion", Some(" a ")).is_err());
    }

    #[test]
    fn high_risk_end_discussion_accepts_valid_reason() {
        assert!(validate_moderator_reason("end_discussion", Some("done")).is_ok());
        assert!(validate_moderator_reason("end_discussion", Some("   consensus reached   ")).is_ok());
    }

    #[test]
    fn high_risk_pipeline_next_enforces_reason() {
        assert!(validate_moderator_reason("pipeline_next", None).is_err());
        assert!(validate_moderator_reason("pipeline_next", Some("")).is_err());
        assert!(validate_moderator_reason("pipeline_next", Some("skip tester"))
            .is_ok());
    }

    // --- PR A consensus-detection tests ---

    fn mk_vote_msg(id: u64, from: &str, vote: &str) -> serde_json::Value {
        serde_json::json!({
            "id": id,
            "from": from,
            "metadata": { "vote": vote, "on": 56 }
        })
    }

    fn mk_reaffirmed_msg(id: u64, from: &str, choice: &str) -> serde_json::Value {
        serde_json::json!({
            "id": id,
            "from": from,
            "metadata": { "synthesis_vote_reaffirmed": { "choice": choice, "on": 56 } }
        })
    }

    fn mk_plain_msg(id: u64, from: &str) -> serde_json::Value {
        serde_json::json!({
            "id": id,
            "from": from,
            "metadata": {}
        })
    }

    #[test]
    fn consensus_all_accept_returns_true() {
        let participants = vec!["developer:0", "tester:0", "architect:0"];
        let board = vec![
            mk_vote_msg(1, "developer:0", "accept"),
            mk_vote_msg(2, "tester:0", "accept"),
            mk_vote_msg(3, "architect:0", "accept"),
        ];
        let round_ids = vec![1u64, 2, 3];
        assert!(round_reached_consensus(&participants, &round_ids, &board));
    }

    #[test]
    fn consensus_missing_one_vote_returns_false() {
        let participants = vec!["developer:0", "tester:0", "architect:0"];
        let board = vec![
            mk_vote_msg(1, "developer:0", "accept"),
            mk_vote_msg(2, "tester:0", "accept"),
            // architect:0 never voted
        ];
        let round_ids = vec![1u64, 2];
        assert!(!round_reached_consensus(&participants, &round_ids, &board));
    }

    #[test]
    fn consensus_reject_vote_blocks_consensus() {
        let participants = vec!["developer:0", "tester:0"];
        let board = vec![
            mk_vote_msg(1, "developer:0", "accept"),
            mk_vote_msg(2, "tester:0", "reject"),
        ];
        let round_ids = vec![1u64, 2];
        assert!(!round_reached_consensus(&participants, &round_ids, &board));
    }

    #[test]
    fn consensus_defer_vote_blocks_consensus() {
        let participants = vec!["developer:0", "tester:0"];
        let board = vec![
            mk_vote_msg(1, "developer:0", "accept"),
            mk_vote_msg(2, "tester:0", "defer"),
        ];
        let round_ids = vec![1u64, 2];
        assert!(!round_reached_consensus(&participants, &round_ids, &board));
    }

    #[test]
    fn consensus_empty_participants_returns_false() {
        let participants: Vec<&str> = vec![];
        let board: Vec<serde_json::Value> = vec![];
        let round_ids: Vec<u64> = vec![];
        assert!(!round_reached_consensus(&participants, &round_ids, &board));
    }

    #[test]
    fn consensus_snapshot_semantics_latest_vote_wins() {
        // developer votes accept, then reject — reject wins (latest vote)
        let participants = vec!["developer:0"];
        let board = vec![
            mk_vote_msg(1, "developer:0", "accept"),
            mk_vote_msg(2, "developer:0", "reject"),
        ];
        let round_ids = vec![1u64, 2];
        assert!(!round_reached_consensus(&participants, &round_ids, &board));
    }

    #[test]
    fn consensus_snapshot_semantics_reject_then_accept_passes() {
        // developer changes mind: reject, then accept — accept wins (latest)
        let participants = vec!["developer:0"];
        let board = vec![
            mk_vote_msg(1, "developer:0", "reject"),
            mk_vote_msg(2, "developer:0", "accept"),
        ];
        let round_ids = vec![1u64, 2];
        assert!(round_reached_consensus(&participants, &round_ids, &board));
    }

    #[test]
    fn consensus_reads_synthesis_vote_reaffirmed_key() {
        // Mixed: one uses `vote`, one uses `synthesis_vote_reaffirmed.choice`
        let participants = vec!["developer:0", "tester:0"];
        let board = vec![
            mk_vote_msg(1, "developer:0", "accept"),
            mk_reaffirmed_msg(2, "tester:0", "accept"),
        ];
        let round_ids = vec![1u64, 2];
        assert!(round_reached_consensus(&participants, &round_ids, &board));
    }

    #[test]
    fn consensus_ignores_messages_outside_current_round() {
        // Prior round's reject should NOT block current round's unanimous accept
        let participants = vec!["developer:0"];
        let board = vec![
            mk_vote_msg(1, "developer:0", "reject"),  // prior round
            mk_vote_msg(2, "developer:0", "accept"),  // current round
        ];
        let round_ids = vec![2u64]; // only current round
        assert!(round_reached_consensus(&participants, &round_ids, &board));
    }

    #[test]
    fn consensus_plain_message_without_vote_metadata_is_not_accept() {
        let participants = vec!["developer:0"];
        let board = vec![mk_plain_msg(1, "developer:0")];
        let round_ids = vec![1u64];
        assert!(!round_reached_consensus(&participants, &round_ids, &board));
    }

    // --- PR 4a decision-record tests ---

    #[test]
    fn decision_accepts_minimal_valid_record() {
        let d = serde_json::json!({ "claim": "use Session name", "status": "accepted" });
        let out = validate_decision_record(Some(&d)).unwrap();
        assert_eq!(out.0, "use Session name");
        assert_eq!(out.1, "accepted");
        assert!(out.2.is_none());
        assert!(out.3.is_none());
    }

    #[test]
    fn decision_accepts_full_record() {
        let d = serde_json::json!({
            "claim": "ship PR 4a",
            "status": "accepted",
            "owner": "developer:0",
            "next_action": "commit + merge"
        });
        let out = validate_decision_record(Some(&d)).unwrap();
        assert_eq!(out.2.as_deref(), Some("developer:0"));
        assert_eq!(out.3.as_deref(), Some("commit + merge"));
    }

    #[test]
    fn decision_rejects_none() {
        let err = validate_decision_record(None).unwrap_err();
        assert!(err.contains("record_decision requires"));
    }

    #[test]
    fn decision_rejects_non_object() {
        let d = serde_json::json!("not an object");
        assert!(validate_decision_record(Some(&d)).is_err());
    }

    #[test]
    fn decision_rejects_short_claim() {
        let d = serde_json::json!({ "claim": "hi", "status": "accepted" });
        assert!(validate_decision_record(Some(&d)).is_err());
    }

    #[test]
    fn decision_rejects_whitespace_claim() {
        let d = serde_json::json!({ "claim": "     ", "status": "accepted" });
        assert!(validate_decision_record(Some(&d)).is_err());
    }

    #[test]
    fn decision_rejects_unknown_status() {
        let d = serde_json::json!({ "claim": "valid claim", "status": "pending" });
        let err = validate_decision_record(Some(&d)).unwrap_err();
        assert!(err.contains("accepted, deferred, rejected"));
    }

    #[test]
    fn decision_accepts_all_three_statuses() {
        for status in &["accepted", "deferred", "rejected"] {
            let d = serde_json::json!({ "claim": "valid claim", "status": status });
            assert!(validate_decision_record(Some(&d)).is_ok(), "status '{}' should be valid", status);
        }
    }

    #[test]
    fn decision_normalizes_empty_owner_to_none() {
        let d = serde_json::json!({ "claim": "valid claim", "status": "accepted", "owner": "   " });
        let out = validate_decision_record(Some(&d)).unwrap();
        assert!(out.2.is_none());
    }

    // ─────────────────────────────────────────────────────────────────────
    // pr-t: placeholder tests for 5 open items from board msgs 255, 279, 282.
    //
    // Why placeholders: the features these exercise are not implemented yet.
    // Writing the tests now (#[ignore]'d) locks the acceptance criteria into
    // the repo so they can't ship silently without meeting them. Each test
    // unignores when the matching code lands.
    // ─────────────────────────────────────────────────────────────────────

    /// Open item 1 (dev-challenger msg 255 attack 2, tech-leader msg 279):
    /// Pipeline-only moderator capabilities (e.g. reorder_pipeline, jump_to_stage)
    /// must fail-fast with a distinct error variant when the active session
    /// format does not support them. String-match on the message is brittle —
    /// UX needs an enum to pattern-match for tooltip rendering.
    ///
    /// Unignore when `ModeratorError::CapabilityNotSupportedForFormat` (or
    /// equivalent distinct variant) exists in the error type and the capability
    /// check in handle_discussion_control honors format.
    #[test]
    #[ignore = "feature not implemented — unignore when CapabilityNotSupportedForFormat variant exists"]
    fn test_format_gated_capability_returns_distinct_error_variant() {
        // Shape of the assertion once implemented:
        //   let err = try_moderator_action("reorder_pipeline", format = "delphi");
        //   assert!(matches!(err, ModeratorError::CapabilityNotSupportedForFormat { .. }));
        unimplemented!("waiting on PR 4 format-gating");
    }

    /// Open item 2 (dev-challenger msg 255 attack 3, tester msg 159):
    /// When the role holding the moderator seat terminates its session mid-pipeline,
    /// the pipeline must auto-pause (set session.paused_at) rather than silently
    /// stall. PR A's consensus detector must return early while paused —
    /// paused != stagnant.
    #[test]
    #[ignore = "feature not implemented — unignore when session.paused_at is wired"]
    fn test_moderator_exit_auto_pauses_session_and_consensus_defers_while_paused() {
        // Assertion shape:
        //   1. simulate moderator session_terminated event
        //   2. assert discussion.get("paused_at").is_some()
        //   3. call round_reached_consensus(...) with all-accept votes
        //   4. expect false (paused state suppresses consensus-triggered terminate)
        unimplemented!("waiting on moderator-exit pause hook");
    }

    // Open item 3 — FINAL per tech-leader msg 326 (msg 303 retracted by msg 318).
    // Architect msg 310 identified the moderator/manager conflation in msg 303;
    // vision § 11.4 / § 11.4b hold:
    //   - human bypass on moderator_only_actions yields when MODERATOR is
    //     claimed (not when manager is claimed)
    //   - manager has separate capabilities, orthogonal to moderator gates
    // Dev-challenger msg 313 split this into 3 distinct tests with a pause
    // filter so the invariant is unambiguous. Platform msg 320 requires the
    // check live inside with_file_lock to close the TOCTOU race.

    /// 3a (positive): moderator is claimed → human bypass on moderator gates
    /// disabled. Error should be `ModeratorError::HumanBypassYieldsToModerator
    /// { moderator: "<role:instance>" }`.
    #[test]
    #[ignore = "feature not implemented — unignore when item #3 ships in PR 4"]
    fn test_human_yields_when_moderator_claimed() {
        // Assertion shape:
        //   1. discussion.moderator = "moderator:0" (active session)
        //   2. call from role="human" to a moderator_only_action
        //   3. expect Err with HumanBypassYieldsToModerator { moderator: "moderator:0" }
        //   4. predicate must execute inside with_file_lock (narrative comment required)
        unimplemented!("waiting on PR 4 item #3");
    }

    /// 3b (negative): only manager is claimed, moderator vacant. Human bypass
    /// must STILL apply — manager presence does not trigger the yield. Guards
    /// against the moderator/manager conflation tech-leader retracted twice.
    #[test]
    #[ignore = "feature not implemented — unignore when item #3 ships in PR 4"]
    fn test_human_retains_bypass_when_only_manager_claimed() {
        // Assertion shape:
        //   1. discussion.moderator unset; manager:0 has active session
        //   2. call from role="human" to a moderator_only_action
        //   3. expect Ok — bypass still holds (manager presence irrelevant)
        unimplemented!("waiting on PR 4 item #3");
    }

    /// 3c (pause retains authority per architect § 11.4a): moderator is claimed
    /// but session is paused. Paused moderator STILL holds authority — human
    /// bypass stays yielded. Tech-leader msg 338 locked this disposition; the
    /// dev-challenger msg 313 pause-filter direction was retracted when vision
    /// § 11.4a merged as cf96dce. Moderator is authoritative across pause.
    #[test]
    #[ignore = "feature not implemented — unignore when item #3 ships in PR 4"]
    fn test_human_yields_when_moderator_paused() {
        // Assertion shape:
        //   1. discussion.moderator = "moderator:0", session.is_paused == true
        //   2. call from role="human" to a moderator_only_action
        //   3. expect Err HumanBypassYieldsToModerator — paused moderator
        //      retains authority, human bypass does NOT re-enable
        unimplemented!("waiting on PR 4 item #3");
    }

    /// Open item 4 (dev-challenger msg 172 attack 1, developer msg 223 defer):
    /// HIGH_RISK_ACTIONS (vaak-mcp.rs:2173) currently covers only end_discussion
    /// and pipeline_next. Dev-challenger's tier table mandates reorder_pipeline,
    /// skip_participant, and jump_to_stage also require reasons. Developer
    /// deferred until handlers for those actions exist — adding the gate now is
    /// dead code. This test is the ship-gate for when those handlers land.
    #[test]
    #[ignore = "deferred per developer msg 223 — unignore when reorder/skip/jump handlers exist"]
    fn test_high_risk_actions_list_covers_reorder_skip_jump() {
        // Assertion shape:
        //   for action in ["reorder_pipeline", "skip_participant", "jump_to_stage"]:
        //     assert!(validate_moderator_reason(action, None).is_err());
        //     assert!(validate_moderator_reason(action, Some("")).is_err());
        //     assert!(validate_moderator_reason(action, Some("legitimate reason")).is_ok());
        unimplemented!("deferred — handlers not in code yet");
    }

    /// Open item 5 (tech-leader msg 279, new after their silent-stage failure):
    /// When a pipeline stage remains silent past a configurable deadline, the
    /// moderator should auto-advance rather than block the entire pipeline on
    /// an unresponsive role. This is the exact failure mode tech-leader hit at
    /// their own msg 279 stage-6 timeout (~8 min silent). The feature is
    /// complementary to PR A's consensus-based auto-termination; this one fires
    /// on silence, not on agreement.
    #[test]
    fn test_pipeline_auto_advances_after_stage_timeout() {
        // Real implementation lands with pr-pipeline-stale-watchdog (this PR).
        // Shape: write a pipeline-mode discussion.json + a sessions.json with
        // the current stage holder's last_heartbeat well beyond timeout. Call
        // auto_skip_stale_pipeline_stage(timeout=1). Assert:
        //   - returned Ok(true)
        //   - discussion.json's pipeline_stage advanced by 1
        //   - board.jsonl gained a `pipeline_auto_skip` system message naming
        //     the skipped agent
        let tmp = std::env::temp_dir().join(format!("vaak-test-stale-pipeline-{}", std::process::id()));
        let vaak = tmp.join(".vaak");
        let _ = std::fs::create_dir_all(&vaak);

        // Minimal project.json — auto_skip reads pipeline_stage_timeout_secs
        // here but our explicit timeout arg overrides; still need it parseable.
        std::fs::write(vaak.join("project.json"),
            r#"{"settings":{"heartbeat_timeout_seconds":300,"pipeline_stage_timeout_secs":1}}"#)
            .expect("write project.json");

        // Pipeline discussion with ghost:0 holding stage 0 of 2
        std::fs::write(vaak.join("discussion.json"), r#"{
            "active": true,
            "mode": "pipeline",
            "pipeline_order": ["ghost:0", "willing:0"],
            "pipeline_stage": 0,
            "pipeline_outputs": []
        }"#).expect("write discussion.json");

        // Stale heartbeat: way before now
        std::fs::write(vaak.join("sessions.json"), r#"{
            "bindings": [
                {"role":"ghost","instance":0,"status":"active","last_heartbeat":"2020-01-01T00:00:00Z"},
                {"role":"willing","instance":0,"status":"active","last_heartbeat":"2099-01-01T00:00:00Z"}
            ]
        }"#).expect("write sessions.json");

        // Empty board so next_message_id starts at 1
        std::fs::write(vaak.join("board.jsonl"), "").expect("write board.jsonl");

        let result = auto_skip_stale_pipeline_stage(tmp.to_str().unwrap(), 1);
        assert!(result.is_ok(), "auto_skip should succeed, got {:?}", result);
        assert_eq!(result.unwrap(), true, "stale stage holder should trigger skip");

        // Verify pipeline_stage advanced
        let disc_after: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(vaak.join("discussion.json")).expect("read discussion.json")
        ).expect("parse discussion.json");
        assert_eq!(
            disc_after.get("pipeline_stage").and_then(|s| s.as_u64()),
            Some(1),
            "pipeline_stage should have advanced from 0 to 1"
        );

        // Verify board got a pipeline_auto_skip message naming ghost:0
        let board = std::fs::read_to_string(vaak.join("board.jsonl")).expect("read board.jsonl");
        let lines: Vec<&str> = board.lines().filter(|l| !l.trim().is_empty()).collect();
        let skip_msg = lines.iter()
            .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
            .find(|m| m.get("metadata")
                .and_then(|md| md.get("pipeline_auto_skip"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false));
        assert!(skip_msg.is_some(), "board should contain a pipeline_auto_skip system message");
        let skip_msg = skip_msg.unwrap();
        assert_eq!(
            skip_msg.get("metadata").and_then(|m| m.get("skipped_agent")).and_then(|s| s.as_str()),
            Some("ghost:0"),
            "skip message should name the stale agent"
        );

        // Cleanup
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_pipeline_auto_skip_no_op_when_holder_has_fresh_heartbeat() {
        // Inverse case: if the stage holder is heartbeating, the watchdog
        // must NOT skip. Otherwise it would steal stages from agents that
        // are actively working.
        let tmp = std::env::temp_dir().join(format!("vaak-test-fresh-pipeline-{}", std::process::id()));
        let vaak = tmp.join(".vaak");
        let _ = std::fs::create_dir_all(&vaak);

        std::fs::write(vaak.join("project.json"),
            r#"{"settings":{"heartbeat_timeout_seconds":300}}"#)
            .expect("write project.json");
        std::fs::write(vaak.join("discussion.json"), r#"{
            "active": true,
            "mode": "pipeline",
            "pipeline_order": ["awake:0", "willing:0"],
            "pipeline_stage": 0,
            "pipeline_outputs": []
        }"#).expect("write discussion.json");
        // Future heartbeat — definitely not stale
        std::fs::write(vaak.join("sessions.json"), r#"{
            "bindings": [
                {"role":"awake","instance":0,"status":"active","last_heartbeat":"2099-01-01T00:00:00Z"}
            ]
        }"#).expect("write sessions.json");
        std::fs::write(vaak.join("board.jsonl"), "").expect("write board.jsonl");

        let result = auto_skip_stale_pipeline_stage(tmp.to_str().unwrap(), 1);
        assert!(result.is_ok(), "auto_skip should succeed");
        assert_eq!(result.unwrap(), false, "fresh heartbeat must NOT trigger skip");

        let disc_after: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(vaak.join("discussion.json")).expect("read")
        ).expect("parse");
        assert_eq!(
            disc_after.get("pipeline_stage").and_then(|s| s.as_u64()),
            Some(0),
            "pipeline_stage should be unchanged"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_pipeline_auto_skip_no_op_when_paused() {
        // Pause is a deliberate hold — the watchdog must not auto-advance
        // through it. Otherwise pause loses its semantic value as a
        // moderator-controlled freeze.
        let tmp = std::env::temp_dir().join(format!("vaak-test-paused-pipeline-{}", std::process::id()));
        let vaak = tmp.join(".vaak");
        let _ = std::fs::create_dir_all(&vaak);

        std::fs::write(vaak.join("project.json"),
            r#"{"settings":{"heartbeat_timeout_seconds":300}}"#)
            .expect("write project.json");
        std::fs::write(vaak.join("discussion.json"), r#"{
            "active": true,
            "mode": "pipeline",
            "paused_at": "2026-01-01T00:00:00Z",
            "pipeline_order": ["ghost:0", "willing:0"],
            "pipeline_stage": 0,
            "pipeline_outputs": []
        }"#).expect("write discussion.json");
        // Stale, but paused — must not skip
        std::fs::write(vaak.join("sessions.json"), r#"{
            "bindings": [
                {"role":"ghost","instance":0,"status":"active","last_heartbeat":"2020-01-01T00:00:00Z"}
            ]
        }"#).expect("write sessions.json");
        std::fs::write(vaak.join("board.jsonl"), "").expect("write board.jsonl");

        let result = auto_skip_stale_pipeline_stage(tmp.to_str().unwrap(), 1);
        assert!(result.is_ok(), "auto_skip should succeed");
        assert_eq!(result.unwrap(), false, "paused pipeline must NOT auto-skip");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    // ─────────────────────────────────────────────────────────────────────
    // pr-t2: additional placeholder tests per tech-leader msg 293 — splits
    // merged placeholders and adds complementary coverage for PR 4's scope.
    // ─────────────────────────────────────────────────────────────────────

    /// Split of test 2 (tech-leader msg 293): isolate the consensus-defers-while-paused
    /// behavior from the auto-pause-on-moderator-exit behavior. Two mechanisms,
    /// two tests. PR 4's moderator-exit handler sets `paused_at`; PR A's
    /// consensus check must honor it. These can fail independently.
    #[test]
    #[ignore = "feature not implemented — unignore when round_reached_consensus honors paused_at"]
    fn test_consensus_check_defers_while_paused() {
        // Assertion shape:
        //   1. set discussion.paused_at = Some(iso_timestamp)
        //   2. call round_reached_consensus with all-accept votes
        //   3. expect false — paused suppresses consensus-triggered terminate
        //      regardless of vote state
        unimplemented!("waiting on paused_at honoring in round_reached_consensus");
    }

    // NOTE: `test_human_regains_moderator_on_manager_exit` removed per
    // tech-leader msg 303 follow-through. The "regains" test was paired with
    // the dropped "yields to claimed manager" test — if human always bypasses
    // (philosophy per msg 303), there is no bypass state to lose and regain.

    /// Complement of test 5 (tech-leader msg 293): when the silent-turn
    /// auto-advance fires, it must emit the same moderator_action audit
    /// metadata {action, reason, timestamp, actor, affected_role} that
    /// human-invoked actions produce. Otherwise auto-advances become
    /// invisible in the audit trail, which breaks the postmortem-evidence
    /// principle from architect msg 190.
    #[test]
    #[ignore = "feature not implemented — unignore when auto-advance emits audit metadata"]
    fn test_silent_turn_emits_moderator_action_audit() {
        // Assertion shape:
        //   1. set stage_deadline_secs = 5, advance to stage N, wait 6s
        //   2. assert the emitted stage_auto_advanced board message carries:
        //        metadata.moderator_action = {
        //          action: "auto_advance",
        //          reason: "stage timeout",
        //          timestamp: iso_str,
        //          actor: "system:auto",
        //          affected_role: "<skipped-role-instance>"
        //        }
        unimplemented!("waiting on audit metadata on auto-advance");
    }

    /// pr-t6 addition (architect msg 352, dev-challenger msg 358, tech-leader msg 368):
    /// Drift guard for the Rust ↔ TypeScript enum mirror. When developer ships
    /// `pr-4-frontend-types`, the `ModeratorError` variants in
    /// `desktop/src/lib/collabTypes.ts` must match the Rust source 1:1. Without
    /// this test, a Rust variant add/rename silently diverges the two sides and
    /// UX's pattern-matching loses coverage.
    ///
    /// Unignore when:
    ///   (a) `ModeratorError` has been promoted to a real Rust enum (currently
    ///       string-prefixed error codes per developer msg 343 format-gating)
    ///   (b) `pr-4-frontend-types` has landed with a mirrored TS discriminated union
    #[test]
    fn test_ts_types_match_rust_enum_variants() {
        // Locate desktop/src/lib/collabTypes.ts from CARGO_MANIFEST_DIR so the
        // test runs from any cwd. CARGO_MANIFEST_DIR points at desktop/src-tauri/
        // so we go up one level.
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
            .expect("CARGO_MANIFEST_DIR not set during cargo test");
        let ts_path = std::path::PathBuf::from(&manifest_dir)
            .join("..")
            .join("src")
            .join("lib")
            .join("collabTypes.ts");
        let ts_src = std::fs::read_to_string(&ts_path)
            .unwrap_or_else(|e| panic!("failed to read {}: {}", ts_path.display(), e));

        // Extract the ModeratorErrorCode union body. The shape we mirror:
        //   export type ModeratorErrorCode =
        //     | "CAPABILITY_NOT_SUPPORTED_FOR_FORMAT"
        //     | "HUMAN_BYPASS_YIELDS_TO_MODERATOR";
        // We grab everything between `export type ModeratorErrorCode =` and the
        // terminating `;`, then pull each quoted literal.
        let start_marker = "export type ModeratorErrorCode =";
        let start = ts_src.find(start_marker).unwrap_or_else(|| {
            panic!(
                "ModeratorErrorCode union not found in {} — did the TS mirror move?",
                ts_path.display()
            )
        });
        let rest = &ts_src[start + start_marker.len()..];
        let end = rest.find(';').expect("ModeratorErrorCode union has no terminating semicolon");
        let union_body = &rest[..end];

        let mut ts_tags: Vec<String> = union_body
            .split('"')
            .enumerate()
            .filter_map(|(i, s)| if i % 2 == 1 { Some(s.to_string()) } else { None })
            .collect();
        ts_tags.sort();

        let mut rust_tags: Vec<String> = ModeratorError::all_variant_tags()
            .iter()
            .map(|s| s.to_string())
            .collect();
        rust_tags.sort();

        assert_eq!(
            rust_tags, ts_tags,
            "ModeratorError Rust↔TS drift detected.\n  Rust: {:?}\n  TS:   {:?}\n\
             Update desktop/src/lib/collabTypes.ts (ModeratorErrorCode union) and \
             `ModeratorError::all_variant_tags()` together.",
            rust_tags, ts_tags
        );
    }

    // ─────────────────────────────────────────────────────────────────────
    // PR 4 helper tests (pure predicates, no I/O).
    // These tests cover helpers the PR 4 code relies on; the moderator-gate
    // and auto-pause integration points still need the placeholder tests
    // above to unignore.
    // ─────────────────────────────────────────────────────────────────────

    #[test]
    fn has_active_manager_returns_false_on_empty_sessions() {
        let sessions = serde_json::json!({ "bindings": [] });
        assert!(!has_active_manager_in_sessions(&sessions));
    }

    #[test]
    fn has_active_manager_returns_false_when_no_manager_role() {
        let sessions = serde_json::json!({
            "bindings": [
                { "role": "developer", "instance": 0, "status": "active" },
                { "role": "tester", "instance": 0, "status": "active" }
            ]
        });
        assert!(!has_active_manager_in_sessions(&sessions));
    }

    #[test]
    fn has_active_manager_returns_true_for_active_manager() {
        let sessions = serde_json::json!({
            "bindings": [
                { "role": "developer", "instance": 0, "status": "active" },
                { "role": "manager", "instance": 0, "status": "active" }
            ]
        });
        assert!(has_active_manager_in_sessions(&sessions));
    }

    #[test]
    fn has_active_manager_returns_true_for_idle_manager() {
        let sessions = serde_json::json!({
            "bindings": [
                { "role": "manager", "instance": 0, "status": "idle" }
            ]
        });
        assert!(has_active_manager_in_sessions(&sessions));
    }

    #[test]
    fn has_active_manager_returns_false_for_disconnected_manager() {
        let sessions = serde_json::json!({
            "bindings": [
                { "role": "manager", "instance": 0, "status": "disconnected" }
            ]
        });
        assert!(!has_active_manager_in_sessions(&sessions));
    }

    #[test]
    fn moderator_error_display_preserves_wire_format_for_capability() {
        // Lock byte-for-byte rendering — UX's parseModeratorError regex depends on
        // the exact `key='value'` shape. Any drift here breaks the toast.
        let err = ModeratorError::CapabilityNotSupportedForFormat {
            capability: "reorder_pipeline".to_string(),
            format: "delphi".to_string(),
            reason: "only valid for Pipeline".to_string(),
        };
        assert_eq!(
            err.to_string(),
            "[error_code: CAPABILITY_NOT_SUPPORTED_FOR_FORMAT] capability='reorder_pipeline' format='delphi': only valid for Pipeline"
        );
    }

    #[test]
    fn moderator_error_display_preserves_wire_format_for_bypass() {
        let err = ModeratorError::HumanBypassYieldsToModerator {
            moderator: "moderator:0".to_string(),
            action: "pause".to_string(),
            caller: "human:0".to_string(),
        };
        assert_eq!(
            err.to_string(),
            "[error_code: HUMAN_BYPASS_YIELDS_TO_MODERATOR] moderator='moderator:0': Action 'pause' requires the moderator role. You are human:0. Route through the active moderator or wait for their stage."
        );
    }

    #[test]
    fn moderator_error_variant_tag_matches_wire_prefix() {
        // Per tech-leader msg 423/431: derive cases from the same `samples()`
        // function used by `all_variant_tags()` so drift-guards share a single
        // variant-iteration source. A forgotten sample addition surfaces here
        // AND in the TS-mirror drift test simultaneously.
        for case in ModeratorError::samples() {
            let rendered = case.to_string();
            let expected_prefix = format!("[error_code: {}]", case.variant_tag());
            assert!(
                rendered.starts_with(&expected_prefix),
                "variant_tag()/Display drift: variant_tag()={:?} but rendered={:?}",
                case.variant_tag(), rendered
            );
        }
    }

    #[test]
    fn format_capability_error_emits_structured_tag() {
        let err = format_capability_error("pipeline_next", "delphi", "only valid for Pipeline");
        assert!(err.starts_with("[error_code: CAPABILITY_NOT_SUPPORTED_FOR_FORMAT]"));
        assert!(err.contains("capability='pipeline_next'"));
        assert!(err.contains("format='delphi'"));
        assert!(err.contains("only valid for Pipeline"));
    }

    #[test]
    fn format_capability_error_is_parseable_by_error_code() {
        // UX relies on the [error_code: ...] tag to pattern-match for tooltip
        // rendering. This test locks the tag format so UX integration can't
        // break silently on string changes.
        let err = format_capability_error("reorder_pipeline", "continuous", "reason text");
        assert!(err.contains("[error_code: CAPABILITY_NOT_SUPPORTED_FOR_FORMAT]"));
    }

    // --- PR 4 fix tests: has_active_session_for_label (architect msg 310 correction) ---

    #[test]
    fn label_match_returns_true_for_exact_role_instance_match() {
        let sessions = serde_json::json!({
            "bindings": [
                { "role": "moderator", "instance": 0, "status": "active" }
            ]
        });
        assert!(has_active_session_for_label(&sessions, "moderator:0"));
    }

    #[test]
    fn label_match_returns_false_for_wrong_instance() {
        let sessions = serde_json::json!({
            "bindings": [
                { "role": "moderator", "instance": 0, "status": "active" }
            ]
        });
        assert!(!has_active_session_for_label(&sessions, "moderator:1"));
    }

    #[test]
    fn label_match_returns_false_for_wrong_role() {
        let sessions = serde_json::json!({
            "bindings": [
                { "role": "manager", "instance": 0, "status": "active" }
            ]
        });
        // Critical: manager presence does NOT satisfy a moderator label check
        // (architect msg 310 correction).
        assert!(!has_active_session_for_label(&sessions, "moderator:0"));
    }

    #[test]
    fn label_match_returns_false_for_disconnected_session() {
        let sessions = serde_json::json!({
            "bindings": [
                { "role": "moderator", "instance": 0, "status": "disconnected" }
            ]
        });
        assert!(!has_active_session_for_label(&sessions, "moderator:0"));
    }

    #[test]
    fn label_match_accepts_idle_status() {
        let sessions = serde_json::json!({
            "bindings": [
                { "role": "moderator", "instance": 0, "status": "idle" }
            ]
        });
        assert!(has_active_session_for_label(&sessions, "moderator:0"));
    }

    #[test]
    fn label_match_returns_false_for_malformed_label() {
        let sessions = serde_json::json!({
            "bindings": [
                { "role": "moderator", "instance": 0, "status": "active" }
            ]
        });
        assert!(!has_active_session_for_label(&sessions, "moderator")); // no colon
        assert!(!has_active_session_for_label(&sessions, "")); // empty
    }
}
