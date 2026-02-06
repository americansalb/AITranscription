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

fn vaak_dir(project_dir: &str) -> PathBuf {
    Path::new(project_dir).join(".vaak")
}

fn project_json_path(project_dir: &str) -> PathBuf {
    vaak_dir(project_dir).join("project.json")
}

fn sessions_json_path(project_dir: &str) -> PathBuf {
    vaak_dir(project_dir).join("sessions.json")
}

fn board_jsonl_path(project_dir: &str) -> PathBuf {
    vaak_dir(project_dir).join("board.jsonl")
}

fn role_briefing_path(project_dir: &str, role: &str) -> PathBuf {
    vaak_dir(project_dir).join("roles").join(format!("{}.md", role))
}

fn last_seen_path(project_dir: &str, session_id: &str) -> PathBuf {
    let safe_id = session_id.replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "_");
    vaak_dir(project_dir).join("last-seen").join(format!("{}.json", safe_id))
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
/// On Windows uses LockFileEx, on Unix uses flock.
fn with_file_lock<F, R>(project_dir: &str, f: F) -> Result<R, String>
where
    F: FnOnce() -> Result<R, String>,
{
    let dir = vaak_dir(project_dir);
    std::fs::create_dir_all(&dir).map_err(|e| format!("Failed to create .vaak dir: {}", e))?;

    let lock_path = dir.join("board.lock");
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
            // Re-create the binding if it was removed (e.g. by cleanup)
            if !found {
                bindings.push(serde_json::json!({
                    "role": state.role,
                    "instance": state.instance,
                    "session_id": state.session_id,
                    "claimed_at": &now,
                    "last_heartbeat": &now,
                    "status": "active"
                }));
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
    let state = {
        let guard = ACTIVE_PROJECT.lock().map_err(|_| "Lock poisoned")?;
        guard.as_ref().ok_or("Not in a project. Call project_join first.")?.clone()
    };

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
    let state = {
        let guard = ACTIVE_PROJECT.lock().map_err(|_| "Lock poisoned")?;
        guard.as_ref().ok_or("Not in a project. Call project_join first.")?.clone()
    };

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
    let state = {
        let guard = ACTIVE_PROJECT.lock().map_err(|_| "Lock poisoned")?;
        guard.as_ref().ok_or("Not in a project. Call project_join first.")?.clone()
    };

    let claims = read_claims_filtered(&state.project_dir);

    Ok(serde_json::json!({
        "claims": claims
    }))
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

/// Handle project_join: claim a role in a project team
fn handle_project_join(role: &str, project_dir: &str, session_id: &str) -> Result<serde_json::Value, String> {
    let normalized = project_dir.replace('\\', "/");

    // Verify project.json exists
    let config = read_project_config(&normalized)?;
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
            let instance = bindings[idx].get("instance").and_then(|i| i.as_u64()).unwrap_or(0) as u32;
            write_sessions(&normalized, &sessions)?;
            return Ok((instance, false));
        }

        // Count active instances of this role
        let active_count = bindings.iter()
            .filter(|b| {
                b.get("role").and_then(|r| r.as_str()) == Some(role)
                && b.get("status").and_then(|s| s.as_str()) == Some("active")
            })
            .count() as u32;

        if active_count >= max_instances {
            // Check for stale bindings to replace
            let now_secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let stale_idx = bindings.iter().position(|b| {
                if b.get("role").and_then(|r| r.as_str()) != Some(role) { return false; }
                if b.get("status").and_then(|s| s.as_str()) != Some("active") { return true; }
                // Check heartbeat age against timeout
                let hb = b.get("last_heartbeat").and_then(|h| h.as_str()).unwrap_or("");
                if hb.is_empty() { return true; }
                match parse_iso_to_epoch_secs(hb) {
                    Some(hb_secs) => now_secs.saturating_sub(hb_secs) > timeout_secs,
                    None => true, // Can't parse timestamp, consider stale
                }
            });

            if let Some(idx) = stale_idx {
                bindings.remove(idx);
            } else {
                return Err(format!("Role '{}' is full ({}/{})", role, active_count, max_instances));
            }
        }

        // Determine instance number
        let existing_instances: Vec<u32> = bindings.iter()
            .filter(|b| b.get("role").and_then(|r| r.as_str()) == Some(role))
            .filter_map(|b| b.get("instance").and_then(|i| i.as_u64()).map(|i| i as u32))
            .collect();
        let mut instance = 0u32;
        while existing_instances.contains(&instance) {
            instance += 1;
        }

        let now = utc_now_iso();
        bindings.push(serde_json::json!({
            "role": role,
            "instance": instance,
            "session_id": session_id,
            "claimed_at": now,
            "last_heartbeat": now,
            "status": "active"
        }));

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

    Ok(serde_json::json!({
        "status": "joined",
        "project_name": project_name,
        "role_title": role_title,
        "role_slug": role,
        "instance": instance,
        "briefing": briefing,
        "team_status": team_status,
        "recent_messages": recent
    }))
}

/// Handle project_send: send a message to a role
fn handle_project_send(to: &str, msg_type: &str, subject: &str, body: &str, metadata: Option<serde_json::Value>, _session_id: &str) -> Result<serde_json::Value, String> {
    let state = {
        let guard = ACTIVE_PROJECT.lock().map_err(|_| "Lock poisoned")?;
        guard.as_ref().ok_or("Not in a project. Call project_join first.")?.clone()
    };

    let config = read_project_config(&state.project_dir)?;

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

    // Validate permission for broadcast
    if to == "all" {
        let roles = config.get("roles").and_then(|r| r.as_object());
        let my_role_def = roles.and_then(|r| r.get(&state.role));
        let perms: Vec<String> = my_role_def
            .and_then(|r| r.get("permissions"))
            .and_then(|p| p.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
            .unwrap_or_default();
        if !perms.contains(&"broadcast".to_string()) && !perms.contains(&"assign_tasks".to_string()) {
            return Err("You don't have permission to broadcast. Use a specific role target.".to_string());
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
        Ok(msg_id)
    })?;

    update_session_heartbeat_in_file();
    notify_desktop();

    Ok(serde_json::json!({
        "message_id": result,
        "delivered_to": [to]
    }))
}

/// Handle project_check: read new messages
fn handle_project_check(last_seen: u64) -> Result<serde_json::Value, String> {
    let state = {
        let guard = ACTIVE_PROJECT.lock().map_err(|_| "Lock poisoned")?;
        guard.as_ref().ok_or("Not in a project. Call project_join first.")?.clone()
    };

    let my_instance_label = format!("{}:{}", state.role, state.instance);
    let all_messages = read_board_filtered(&state.project_dir);
    let my_messages: Vec<&serde_json::Value> = all_messages.iter()
        .filter(|m| {
            let to = m.get("to").and_then(|t| t.as_str()).unwrap_or("");
            to == state.role || to == my_instance_label || to == "all"
        })
        .filter(|m| {
            m.get("id").and_then(|i| i.as_u64()).unwrap_or(0) > last_seen
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
    let state = {
        let guard = ACTIVE_PROJECT.lock().map_err(|_| "Lock poisoned")?;
        guard.as_ref().ok_or("Not in a project. Call project_join first.")?.clone()
    };

    let session_id = read_cached_session_id().unwrap_or_else(get_session_id);
    let ls_path = last_seen_path(&state.project_dir, &session_id);

    let start = std::time::Instant::now();
    let poll_interval = std::time::Duration::from_secs(3);
    let timeout = std::time::Duration::from_secs(timeout_secs);
    let mut polls_since_heartbeat: u32 = 0;

    loop {
        // Send heartbeat every ~30 seconds (every 10th poll) to keep session alive
        if polls_since_heartbeat >= 10 {
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
    let state = {
        let guard = ACTIVE_PROJECT.lock().map_err(|_| "Lock poisoned")?;
        guard.as_ref().ok_or("Not in a project. Call project_join first.")?.clone()
    };

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

    Ok(serde_json::json!({
        "project_name": project_name,
        "your_role": state.role,
        "your_instance": state.instance,
        "roles": roles_status,
        "pending_messages": my_messages.len(),
        "total_messages": all_messages.len()
    }))
}

/// Handle project_leave: release role binding
fn handle_project_leave() -> Result<serde_json::Value, String> {
    let state = {
        let guard = ACTIVE_PROJECT.lock().map_err(|_| "Lock poisoned")?;
        guard.as_ref().ok_or("Not in a project. Call project_join first.")?.clone()
    };

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

/// Handle project_update_briefing: update a role's briefing markdown
fn handle_project_update_briefing(role: &str, content: &str) -> Result<serde_json::Value, String> {
    let state = {
        let guard = ACTIVE_PROJECT.lock().map_err(|_| "Lock poisoned")?;
        guard.as_ref().ok_or("Not in a project. Call project_join first.")?.clone()
    };

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

    let mut output = format!(
        "TEAM: You are the {} (instance {}) on project \"{}\". Team: {}.",
        role_title, my_instance, project_name, team_parts.join(", ")
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

    if new_messages.is_empty() {
        output.push_str(" No new messages.");
    } else {
        output.push_str(&format!("\n\nNEW MESSAGES ({} unread):\n", new_messages.len()));

        let display_msgs = if new_messages.len() > 10 {
            output.push_str(&format!("... and {} earlier messages. Use project_check(0) to see all.\n\n", new_messages.len() - 10));
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

            let display_body = if (mtype == "directive" || mtype == "review") || body.len() <= 500 {
                body.to_string()
            } else {
                format!("{}... (truncated, use project_check to see full)", &body[..500])
            };

            output.push_str(&format!("\n[#{}] FROM {} ({}): \"{}\"\n{}\n", id, from, mtype, subject, display_body));
        }
    }

    output.push_str("\nUse project_send to respond. Use project_check for full history.");

    // Update last-seen
    if let Some(max_id) = my_messages.iter()
        .filter_map(|m| m.get("id").and_then(|i| i.as_u64()))
        .max()
    {
        let ls_dir = vaak_dir(&project_dir).join("last-seen");
        let _ = std::fs::create_dir_all(&ls_dir);
        let _ = std::fs::write(&ls_path, serde_json::json!({
            "last_seen_id": max_id,
            "updated_at": utc_now_iso()
        }).to_string());
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

    #[cfg(windows)]
    {
        if let Some(grandparent) = get_ancestor_pid(ppid) {
            let cache_file = cache_dir.join(format!("{}.txt", grandparent));
            if let Ok(id) = std::fs::read_to_string(&cache_file) {
                if !id.is_empty() {
                    return Some(id);
                }
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
                            "project_dir": { "type": "string", "description": "Optional: absolute path to the project directory. If omitted, auto-detects by walking up from current directory." }
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
                    "description": "Check for new messages from your team. Pass the last message ID you've seen (0 to get all). The hook automatically shows new messages, but use this for explicit polling or to see older history.",
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
                            "timeout": { "type": "integer", "description": "Max seconds to wait before returning (default 300 = 5 minutes)" }
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
                }]
            })
        }
        "tools/call" => {
            let params = request.get("params")?;
            let tool_name = params.get("name")?.as_str()?;

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

                match handle_project_join(role, &project_dir, session_id) {
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
                    .unwrap_or(300);

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

    // Voice instructions only when enabled
    if enabled {
        let speak_msg = "IMPORTANT: You MUST call the mcp__vaak__speak tool to speak responses aloud to the user. When on a team project, use project_send for ALL team communication FIRST, then call speak with a SHORT summary for your local terminal user. The board is for the team. Speak is for the human at your terminal. Never use speak as a substitute for project_send.";

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

    // Check for active project team in CWD's .vaak/ (always, even when voice is off)
    if let Some(team_reminder) = check_project_from_cwd(&session_id) {
        if !output.is_empty() {
            output.push(' ');
        }
        output.push_str(&team_reminder);

        // When auto-collab is enabled, inject autonomous team behavior instructions
        if auto_collab {
            output.push_str("\n\nAUTONOMOUS TEAM MODE:\n");
            output.push_str("1. Handle ALL unread messages BEFORE the user's request. For each directive: implement it fully. For questions: send an answer.\n");
            output.push_str("2. Do NOT ask the user for permission — act on team messages proactively, then speak what you did.\n");
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
        files.sort_by_key(|e| std::cmp::Reverse(e.path()));
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

/// List visible windows (Unix stub)
#[cfg(not(windows))]
fn list_visible_windows() -> Result<String, String> {
    match std::process::Command::new("wmctrl").arg("-l").output() {
        Ok(output) => {
            if output.status.success() {
                Ok(String::from_utf8_lossy(&output.stdout).to_string())
            } else {
                Err("wmctrl failed. Install wmctrl for window listing on Linux.".to_string())
            }
        }
        Err(_) => Err("Window listing not available on this platform. Install wmctrl on Linux.".to_string()),
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
}
