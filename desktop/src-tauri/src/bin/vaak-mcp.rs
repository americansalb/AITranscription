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
    std::fs::write(&config_path, content)
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
    std::fs::write(&config_path, content)
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
    std::fs::write(&sessions_path, sessions_content)
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

/// Parse an ISO 8601 timestamp (YYYY-MM-DDTHH:MM:SSZ) to seconds since epoch.
/// Returns None if the format can't be parsed.
fn parse_iso_to_epoch_secs(iso: &str) -> Option<u64> {
    // Expected format: "2026-02-05T04:11:10Z"
    let iso = iso.trim_end_matches('Z');
    let (date_part, time_part) = iso.split_once('T')?;
    let date_parts: Vec<&str> = date_part.split('-').collect();
    let time_parts: Vec<&str> = time_part.split(':').collect();
    if date_parts.len() != 3 || time_parts.len() != 3 { return None; }

    let year: u64 = date_parts[0].parse().ok()?;
    let month: u64 = date_parts[1].parse().ok()?;
    let day: u64 = date_parts[2].parse().ok()?;
    let hour: u64 = time_parts[0].parse().ok()?;
    let min: u64 = time_parts[1].parse().ok()?;
    let sec: u64 = time_parts[2].parse().ok()?;

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

    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{LockFileEx, UnlockFileEx, LOCKFILE_EXCLUSIVE_LOCK};
        use windows_sys::Win32::System::IO::OVERLAPPED;

        let handle = lock_file.as_raw_handle();
        let mut overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };

        let locked = unsafe {
            LockFileEx(handle as _, LOCKFILE_EXCLUSIVE_LOCK, 0, u32::MAX, u32::MAX, &mut overlapped)
        };
        if locked == 0 {
            return Err("Failed to acquire file lock".to_string());
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
        let ret = unsafe { libc::flock(fd, libc::LOCK_EX) };
        if ret != 0 {
            return Err("Failed to acquire file lock".to_string());
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
    std::fs::write(&path, content)
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
    std::fs::write(&path, content)
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

fn write_discussion_state(project_dir: &str, state: &serde_json::Value) -> Result<(), String> {
    let path = discussion_json_path(project_dir);
    let content = serde_json::to_string_pretty(state)
        .map_err(|e| format!("Failed to serialize discussion state: {}", e))?;
    std::fs::write(&path, content)
        .map_err(|e| format!("Failed to write discussion.json: {}", e))?;
    Ok(())
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
        // Standard: randomize order using Fisher-Yates shuffle
        let mut bodies: Vec<&str> = entries.iter().map(|(_, b)| b.as_str()).collect();
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let mut rng_state = seed;
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
    let mut disagree_reasons: Vec<String> = Vec::new();
    let mut alternatives: Vec<String> = Vec::new();

    for msg in &all_messages {
        let id = msg.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
        if !msg_ids.contains(&id) { continue; }

        let body = msg.get("body").and_then(|b| b.as_str()).unwrap_or("").trim().to_lowercase();

        if body.starts_with("agree") || body == "lgtm" || body == "approved" || body == "+1" {
            agree_count += 1;
        } else if body.starts_with("disagree") || body.starts_with("object") || body.starts_with("-1") {
            let reason = msg.get("body").and_then(|b| b.as_str()).unwrap_or("(no reason)").to_string();
            disagree_reasons.push(reason);
        } else if body.starts_with("alternative") || body.starts_with("suggest") || body.starts_with("instead") {
            let proposal = msg.get("body").and_then(|b| b.as_str()).unwrap_or("(no proposal)").to_string();
            alternatives.push(proposal);
        } else {
            // Treat unclassified as a comment — count as "reviewed"
            disagree_reasons.push(msg.get("body").and_then(|b| b.as_str()).unwrap_or("(comment)").to_string());
        }
    }

    let total = submissions.len();
    let consensus = if disagree_reasons.is_empty() && alternatives.is_empty() {
        "APPROVED"
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
    let disc = read_discussion_state(project_dir);
    let is_active = disc.get("active").and_then(|v| v.as_bool()).unwrap_or(false);
    let mode = disc.get("mode").and_then(|v| v.as_str()).unwrap_or("");

    if !is_active || mode != "continuous" {
        return None;
    }

    // Don't create rounds for the moderator's own moderation messages
    let moderator = disc.get("moderator").and_then(|v| v.as_str()).unwrap_or("");
    if author == moderator {
        return None;
    }

    // Close any open round that's timed out before creating a new one
    let _ = auto_close_timed_out_round(project_dir);

    let now = utc_now_iso();
    let mut updated = disc.clone();

    // Check if there's already an open round — don't create a new one
    let current_phase = updated.get("phase").and_then(|v| v.as_str()).unwrap_or("");
    if current_phase == "submitting" {
        // There's already an open round collecting responses — don't create another
        return None;
    }

    let current_round = updated.get("current_round").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
    let next_round = current_round + 1;

    // Build round topic from the status message
    let topic = if !status_msg_subject.is_empty() {
        status_msg_subject.to_string()
    } else if status_msg_body.len() > 200 {
        format!("{}...", &status_msg_body[..200])
    } else {
        status_msg_body.to_string()
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
            "trigger_from": author,
            "trigger_subject": topic,
            "trigger_message_id": msg_id
        }));
    }

    let _ = write_discussion_state(project_dir, &updated);

    // Post review window notification
    let timeout = updated.get("settings")
        .and_then(|s| s.get("auto_close_timeout_seconds"))
        .and_then(|t| t.as_u64())
        .unwrap_or(60);

    let board_msg_id = next_message_id(project_dir);
    let notification = serde_json::json!({
        "id": board_msg_id,
        "from": "system",
        "to": "all",
        "type": "moderation",
        "timestamp": now,
        "subject": format!("Review #{}: {}", next_round, if topic.len() > 80 { &topic[..80] } else { &topic }),
        "body": format!("**REVIEW WINDOW OPEN** ({}s)\n{} reported: {}\n\nRespond with: agree / disagree: [reason] / alternative: [proposal]\nSilence within {}s = consent.", timeout, author, topic, timeout),
        "metadata": {
            "discussion_action": "auto_round",
            "round": next_round,
            "author": author,
            "timeout_seconds": timeout
        }
    });
    let _ = append_to_board(project_dir, &notification);

    Some(next_round)
}

/// Check if the current round in a continuous discussion has timed out.
/// If so, auto-close it and generate a mini-aggregate.
fn auto_close_timed_out_round(project_dir: &str) -> bool {
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
    let _ = write_discussion_state(project_dir, &updated);

    true
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

    // Call the backend API at http://127.0.0.1:19836/api/v1/audience/vote
    eprintln!("[audience_vote] Calling backend: topic='{}', phase={}, pool={:?}",
        &topic[..topic.len().min(80)], backend_phase, pool);

    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(120)) // 27 parallel LLM calls can take time
        .build();

    let resp = agent.post("http://127.0.0.1:19836/api/v1/audience/vote")
        .set("Content-Type", "application/json")
        .send_string(&request_body.to_string())
        .map_err(|e| format!("Backend API call failed: {}. Is the backend running on port 19836?", e))?;

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
    let state = get_or_rejoin_state()?;

    let my_label = format!("{}:{}", state.role, state.instance);

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
                    format!("Continuous Review mode activated.\n\n**Topic:** {}\n**Moderator:** {}\n**Participants:** {}\n\nReview windows open automatically when developers post status updates. Respond with: agree / disagree: [reason] / alternative: [proposal]. Silence within the timeout = consent.",
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
            let discussion = read_discussion_state(&state.project_dir);
            Ok(discussion)
        }

        _ => Err(format!("Unknown discussion action: '{}'. Valid: start_discussion, close_round, open_next_round, end_discussion, get_state, set_teams", action))
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
        std::fs::write(&config_path, updated)
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

    let active_section = get_active_section(project_dir);

    // Advance last_seen_id to the max ID in recent_messages so project_check
    // won't re-deliver these same messages (prevents token waste)
    let max_recent_id = recent.iter()
        .filter_map(|m| m.get("id").and_then(|i| i.as_u64()))
        .max()
        .unwrap_or(0);
    if max_recent_id > 0 {
        let ls_path = last_seen_path(&normalized, session_id);
        let _ = std::fs::create_dir_all(ls_path.parent().unwrap());
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

        // Read current last_seen
        let last_seen_id: u64 = std::fs::read_to_string(&ls_path)
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .and_then(|j| j.get("last_seen_id")?.as_u64())
            .unwrap_or(0);

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

    // Read board.jsonl, filter for messages to my role or my specific instance
    let my_instance_label = format!("{}:{}", my_role, my_instance);
    let all_messages = read_board_filtered(&project_dir);
    let my_messages: Vec<&serde_json::Value> = all_messages.iter()
        .filter(|m| {
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
            format!("REVIEW WINDOW OPEN — Round #{}: {}\nStatus: {}/{} responded ({}). Window: {}s. Respond with: agree / disagree: [reason] / alternative: [proposal]. Silence = consent.",
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
        let hook_ls_path = last_seen_path(&project_dir, session_id);
        let _ = std::fs::create_dir_all(hook_ls_path.parent().unwrap());
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

/// Get the parent PID of a given process (macOS via sysctl)
#[cfg(target_os = "macos")]
fn get_ancestor_pid(pid: u32) -> Option<u32> {
    // Use sysctl kern.proc.pid.{pid} to get kinfo_proc with parent PID
    let mut mib: [libc::c_int; 4] = [
        libc::CTL_KERN,
        libc::KERN_PROC,
        libc::KERN_PROC_PID,
        pid as libc::c_int,
    ];

    let mut info: libc::kinfo_proc = unsafe { std::mem::zeroed() };
    let mut size = std::mem::size_of::<libc::kinfo_proc>();

    let ret = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            4,
            &mut info as *mut _ as *mut libc::c_void,
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };

    if ret == 0 && size > 0 {
        let ppid = info.kp_eproc.e_ppid;
        if ppid > 1 {
            Some(ppid as u32)
        } else {
            None
        }
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
        let all_messages = read_board_filtered(&project_dir);
        let my_messages: Vec<&serde_json::Value> = all_messages.iter()
            .filter(|m| {
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
