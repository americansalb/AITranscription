use serde::{Deserialize, Serialize};

// ==================== Session Registry ====================

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Info about an active Claude Code session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub session_id: String,
    pub last_heartbeat: u64,
    pub hostname: String,
    pub cwd: String,
    #[serde(default)]
    pub name: String,
}

/// Tracks active Claude Code sessions via heartbeats
pub struct SessionRegistry {
    sessions: HashMap<String, SessionInfo>,
    names: HashMap<String, String>,
}

impl SessionRegistry {
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
            names: HashMap::new(),
        }
    }

    pub fn update_heartbeat(&mut self, session_id: &str, cwd: Option<&str>) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let hostname = hostname::get()
            .map(|h| h.to_string_lossy().to_string())
            .unwrap_or_else(|_| "unknown".to_string());

        let stored_name = self.names.get(session_id).cloned().unwrap_or_default();

        self.sessions.entry(session_id.to_string())
            .and_modify(|s| {
                s.last_heartbeat = now;
                if let Some(c) = cwd {
                    if !c.is_empty() {
                        s.cwd = c.to_string();
                    }
                }
                if s.name.is_empty() && !stored_name.is_empty() {
                    s.name = stored_name.clone();
                }
            })
            .or_insert(SessionInfo {
                session_id: session_id.to_string(),
                last_heartbeat: now,
                hostname,
                cwd: cwd.unwrap_or("").to_string(),
                name: stored_name,
            });
    }

    pub fn set_session_names(&mut self, names: &[(String, String)]) {
        for (session_id, name) in names {
            self.names.insert(session_id.clone(), name.clone());
            if let Some(info) = self.sessions.get_mut(session_id) {
                info.name = name.clone();
            }
        }
    }

    pub fn get_all_names(&self) -> &HashMap<String, String> {
        &self.names
    }

    pub fn get_active(&self) -> Vec<&SessionInfo> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        self.sessions.values()
            .filter(|s| (now - s.last_heartbeat) < 120_000)
            .collect()
    }
}

// ==================== Project Types (for desktop app UI) ====================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleConfig {
    pub title: String,
    pub description: String,
    pub max_instances: u32,
    pub permissions: Vec<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RosterSlot {
    pub role: String,
    pub instance: u32,
    pub added_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RosterSlotWithStatus {
    pub role: String,
    pub instance: u32,
    pub added_at: String,
    pub status: String, // "vacant", "standby", "working"
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectConfig {
    pub project_id: String,
    pub name: String,
    pub description: String,
    pub created_at: String,
    pub updated_at: String,
    pub roles: HashMap<String, RoleConfig>,
    pub settings: ProjectSettings,
    #[serde(default)]
    pub roster: Option<Vec<RosterSlot>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectSettings {
    pub heartbeat_timeout_seconds: u64,
    pub message_retention_days: u64,
    #[serde(default)]
    pub workflow_type: Option<String>,
    #[serde(default)]
    pub auto_collab: Option<bool>,
    #[serde(default)]
    pub human_in_loop: Option<bool>,
    #[serde(default)]
    pub workflow_colors: Option<std::collections::HashMap<String, String>>,
    #[serde(default)]
    pub discussion_mode: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionBinding {
    pub role: String,
    pub instance: u32,
    pub session_id: String,
    pub claimed_at: String,
    pub last_heartbeat: String,
    pub status: String,
    #[serde(default)]
    pub activity: Option<String>,
    #[serde(default)]
    pub active_section: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionsFile {
    pub bindings: Vec<SessionBinding>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoardMessage {
    pub id: u64,
    pub from: String,
    pub to: String,
    #[serde(rename = "type")]
    pub msg_type: String,
    pub timestamp: String,
    pub subject: String,
    pub body: String,
    #[serde(default)]
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleStatus {
    pub slug: String,
    pub title: String,
    pub active_instances: u32,
    pub max_instances: u32,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileClaim {
    pub role_instance: String,
    pub files: Vec<String>,
    pub description: String,
    pub claimed_at: String,
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedProject {
    pub config: ProjectConfig,
    pub sessions: Vec<SessionBinding>,
    pub messages: Vec<BoardMessage>,
    pub role_statuses: Vec<RoleStatus>,
    pub claims: Vec<FileClaim>,
}

/// Public wrapper for parse_iso_epoch (used by main.rs for claims staleness check)
pub fn parse_iso_epoch_pub(iso: &str) -> Option<u64> {
    parse_iso_epoch(iso)
}

/// Parse ISO 8601 timestamp to epoch seconds (for heartbeat age comparison)
fn parse_iso_epoch(iso: &str) -> Option<u64> {
    let iso = iso.trim_end_matches('Z');
    let (date_part, time_part) = iso.split_once('T')?;
    let dp: Vec<&str> = date_part.split('-').collect();
    let tp: Vec<&str> = time_part.split(':').collect();
    if dp.len() != 3 || tp.len() != 3 { return None; }
    let (year, month, day): (u64, u64, u64) = (dp[0].parse().ok()?, dp[1].parse().ok()?, dp[2].parse().ok()?);
    let (hour, min, sec): (u64, u64, u64) = (tp[0].parse().ok()?, tp[1].parse().ok()?, tp[2].parse().ok()?);
    let mut total_days: u64 = 0;
    for y in 1970..year {
        let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
        total_days += if leap { 366 } else { 365 };
    }
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let md: [u64; 12] = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    for m in 0..(month.saturating_sub(1) as usize) {
        total_days += md.get(m).copied().unwrap_or(30);
    }
    total_days += day.saturating_sub(1);
    Some(total_days * 86400 + hour * 3600 + min * 60 + sec)
}

/// Parse a .vaak/ project directory into structured data for the UI.
/// Automatically cleans stale sessions (heartbeat older than timeout).
pub fn parse_project_dir(dir: &str) -> Option<ParsedProject> {
    let vaak_dir = Path::new(dir).join(".vaak");

    // 1. Read project.json
    let config: ProjectConfig = serde_json::from_str(
        &std::fs::read_to_string(vaak_dir.join("project.json")).ok()?
    ).ok()?;

    // 2. Read sessions.json (may not exist yet)
    let mut sessions_file: SessionsFile = std::fs::read_to_string(vaak_dir.join("sessions.json"))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(SessionsFile { bindings: vec![] });

    // 2b. Compute staleness for display only — NEVER remove sessions here.
    // Removal only happens in handle_project_join when a new agent needs the slot.
    let timeout_secs = config.settings.heartbeat_timeout_seconds;
    let now_secs = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();

    // 3. Read board.jsonl from active section (may not exist yet) — one JSON per line
    let board_path = active_board_path(dir);
    let mut messages: Vec<BoardMessage> = std::fs::read_to_string(&board_path)
        .unwrap_or_default()
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();

    // 3b. Apply message retention filtering (0 = keep all)
    let retention_days = config.settings.message_retention_days;
    if retention_days > 0 {
        let max_age_secs = retention_days * 86400;
        messages.retain(|msg| {
            match parse_iso_epoch(&msg.timestamp) {
                Some(msg_secs) => now_secs.saturating_sub(msg_secs) <= max_age_secs,
                None => true, // keep messages with unparseable timestamps
            }
        });
    }

    // 4. Compute role statuses from config + sessions (with heartbeat-based staleness)
    let role_statuses = compute_role_statuses(&config, &sessions_file.bindings, now_secs, timeout_secs);

    // 5. Read claims.json and filter stale entries
    let gone_threshold = (timeout_secs as f64 * 2.5) as u64;
    let claims = read_claims_filtered(&vaak_dir, &sessions_file.bindings, now_secs, gone_threshold);

    Some(ParsedProject {
        config,
        sessions: sessions_file.bindings,
        messages,
        role_statuses,
        claims,
    })
}

/// Read claims.json and filter out stale entries whose session is gone.
fn read_claims_filtered(vaak_dir: &Path, bindings: &[SessionBinding], now_secs: u64, gone_threshold: u64) -> Vec<FileClaim> {
    let claims_path = vaak_dir.join("claims.json");
    let content = match std::fs::read_to_string(&claims_path) {
        Ok(c) => c,
        Err(_) => return vec![],
    };
    let claims_map: std::collections::HashMap<String, serde_json::Value> = match serde_json::from_str(&content) {
        Ok(m) => m,
        Err(_) => return vec![],
    };

    let mut result = Vec::new();
    let mut any_removed = false;
    let mut clean_map = serde_json::Map::new();

    for (key, val) in &claims_map {
        let session_id = val.get("session_id").and_then(|s| s.as_str()).unwrap_or("");
        // Check if this session is still active (not gone)
        let binding = bindings.iter().find(|b| b.session_id == session_id);
        let is_stale = match binding {
            None => true, // No binding at all
            Some(b) => {
                let age = parse_iso_epoch(&b.last_heartbeat)
                    .map(|hb| now_secs.saturating_sub(hb))
                    .unwrap_or(u64::MAX);
                age > gone_threshold
            }
        };

        if is_stale {
            any_removed = true;
            continue;
        }

        // Parse into FileClaim
        let files: Vec<String> = val.get("files")
            .and_then(|f| f.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
            .unwrap_or_default();
        let description = val.get("description").and_then(|d| d.as_str()).unwrap_or("").to_string();
        let claimed_at = val.get("claimed_at").and_then(|c| c.as_str()).unwrap_or("").to_string();

        result.push(FileClaim {
            role_instance: key.clone(),
            files,
            description,
            claimed_at,
            session_id: session_id.to_string(),
        });
        clean_map.insert(key.clone(), val.clone());
    }

    // Write back cleaned version if any were removed
    if any_removed {
        if let Ok(s) = serde_json::to_string_pretty(&clean_map) {
            let _ = std::fs::write(&claims_path, s);
        }
    }

    result
}

fn compute_role_statuses(config: &ProjectConfig, bindings: &[SessionBinding], now_secs: u64, timeout_secs: u64) -> Vec<RoleStatus> {
    let auto_collab = config.settings.auto_collab.unwrap_or(false);
    let gone_threshold = (timeout_secs as f64 * 2.5) as u64;
    config.roles.iter().map(|(slug, role)| {
        let role_bindings: Vec<&SessionBinding> = bindings.iter()
            .filter(|b| b.role == *slug && b.status == "active")
            .collect();
        let total = role_bindings.len() as u32;

        let mut fresh_count = 0u32;
        let mut idle_count = 0u32;
        let mut gone_count = 0u32;
        for b in &role_bindings {
            let age = parse_iso_epoch(&b.last_heartbeat)
                .map(|hb| now_secs.saturating_sub(hb))
                .unwrap_or(u64::MAX);
            if age <= timeout_secs {
                fresh_count += 1;
            } else if age <= gone_threshold {
                idle_count += 1;
            } else if auto_collab {
                // When auto_collab is on, never mark sessions as "gone" —
                // treat them as idle so they persist in the UI
                idle_count += 1;
            } else {
                gone_count += 1;
            }
        }

        let status = if fresh_count > 0 {
            "active"
        } else if idle_count > 0 {
            "idle"
        } else if gone_count > 0 {
            "gone"
        } else {
            "vacant"
        };

        RoleStatus {
            slug: slug.clone(),
            title: role.title.clone(),
            active_instances: total,
            max_instances: role.max_instances,
            status: status.to_string(),
        }
    }).collect()
}

// ==================== Discussion State ====================

/// A single submission within a Delphi round
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscussionSubmission {
    pub from: String,
    pub message_id: u64,
    pub submitted_at: String,
}

/// A single round within a discussion
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscussionRound {
    pub number: u32,
    pub opened_at: String,
    #[serde(default)]
    pub closed_at: Option<String>,
    #[serde(default)]
    pub submissions: Vec<DiscussionSubmission>,
    #[serde(default)]
    pub aggregate_message_id: Option<u64>,
    /// For continuous review: the status message that triggered this round
    #[serde(default)]
    pub trigger_message_id: Option<u64>,
    /// For continuous review: who posted the triggering status (also written as "author" by MCP)
    #[serde(default)]
    pub trigger_from: Option<String>,
    /// For continuous review: the author who triggered this round (used by MCP for quorum checks)
    #[serde(default)]
    pub author: Option<String>,
    /// For continuous review: subject of the triggering status
    #[serde(default)]
    pub trigger_subject: Option<String>,
    /// For continuous review: whether this round was auto-triggered by a status message
    #[serde(default)]
    pub auto_triggered: Option<bool>,
    /// Per-round topic (continuous mode: the status message that triggered it)
    #[serde(default)]
    pub topic: Option<String>,
}

/// Discussion settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscussionSettings {
    #[serde(default = "default_max_rounds")]
    pub max_rounds: u32,
    #[serde(default = "default_timeout_minutes")]
    pub timeout_minutes: u32,
    #[serde(default = "default_expire_paused")]
    pub expire_paused_after_minutes: u32,
    /// Auto-close timeout for continuous review rounds (seconds). 0 = no auto-close.
    #[serde(default = "default_auto_close_timeout")]
    pub auto_close_timeout_seconds: u32,
}

fn default_max_rounds() -> u32 { 10 }
fn default_timeout_minutes() -> u32 { 15 }
fn default_expire_paused() -> u32 { 60 }
fn default_auto_close_timeout() -> u32 { 60 }

impl Default for DiscussionSettings {
    fn default() -> Self {
        Self {
            max_rounds: default_max_rounds(),
            timeout_minutes: default_timeout_minutes(),
            expire_paused_after_minutes: default_expire_paused(),
            auto_close_timeout_seconds: default_auto_close_timeout(),
        }
    }
}

/// Active discussion state — stored in .vaak/discussion.json
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscussionState {
    #[serde(default)]
    pub active: bool,
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub topic: String,
    #[serde(default)]
    pub started_at: Option<String>,
    #[serde(default)]
    pub moderator: Option<String>,
    #[serde(default)]
    pub participants: Vec<String>,
    #[serde(default)]
    pub current_round: u32,
    #[serde(default)]
    pub phase: Option<String>,
    #[serde(default)]
    pub paused_at: Option<String>,
    #[serde(default)]
    pub expire_at: Option<String>,
    #[serde(default)]
    pub previous_phase: Option<String>,
    #[serde(default)]
    pub rounds: Vec<DiscussionRound>,
    #[serde(default)]
    pub settings: DiscussionSettings,
}

impl Default for DiscussionState {
    fn default() -> Self {
        Self {
            active: false,
            mode: None,
            topic: String::new(),
            started_at: None,
            moderator: None,
            participants: Vec::new(),
            current_round: 0,
            phase: None,
            paused_at: None,
            expire_at: None,
            previous_phase: None,
            rounds: Vec::new(),
            settings: DiscussionSettings::default(),
        }
    }
}

/// Read discussion state from the active section's discussion.json.
/// Returns default (inactive) state if file doesn't exist or is unparseable.
pub fn read_discussion(dir: &str) -> DiscussionState {
    let path = active_discussion_path(dir);
    match std::fs::read_to_string(&path) {
        Ok(content) => {
            serde_json::from_str(&content).unwrap_or_default()
        }
        Err(_) => DiscussionState::default(),
    }
}

/// Write discussion state to the active section's discussion.json with file locking.
/// Returns true on success, false on failure.
pub fn write_discussion(dir: &str, state: &DiscussionState) -> bool {
    let path = active_discussion_path(dir);
    let lock_path = active_lock_path(dir);

    let json = match serde_json::to_string_pretty(state) {
        Ok(s) => s,
        Err(_) => return false,
    };

    // Acquire file lock
    let lock_file = match std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .open(&lock_path)
    {
        Ok(f) => f,
        Err(_) => return false,
    };

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
        if locked == 0 { return false; }

        let result = std::fs::write(&path, &json).is_ok();

        unsafe { UnlockFileEx(handle as _, 0, u32::MAX, u32::MAX, &mut overlapped); }
        result
    }

    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        let fd = lock_file.as_raw_fd();
        if unsafe { libc::flock(fd, libc::LOCK_EX) } != 0 { return false; }

        let result = std::fs::write(&path, &json).is_ok();

        unsafe { libc::flock(fd, libc::LOCK_UN); }
        result
    }
}

/// Get the lock file path for the active section.
/// "default" section uses legacy .vaak/board.lock, others use sections/{slug}/board.lock.
pub fn active_lock_path(dir: &str) -> std::path::PathBuf {
    let section = get_active_section(dir);
    let vaak_dir = Path::new(dir).join(".vaak");
    if section == "default" {
        vaak_dir.join("board.lock")
    } else {
        vaak_dir.join("sections").join(section).join("board.lock")
    }
}

/// Execute a closure while holding an exclusive lock on the active section's board.lock.
/// Use this to wrap read-modify-write operations on discussion.json
/// so that MCP sidecar writes (submissions) are not lost.
pub fn with_board_lock<F, R>(dir: &str, f: F) -> Result<R, String>
where
    F: FnOnce() -> Result<R, String>,
{
    let lock_path = active_lock_path(dir);

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
            return Err("Failed to acquire board.lock".to_string());
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
        if unsafe { libc::flock(fd, libc::LOCK_EX) } != 0 {
            return Err("Failed to acquire board.lock".to_string());
        }
        let result = f();
        unsafe { libc::flock(fd, libc::LOCK_UN); }
        result
    }
}

/// Write discussion state WITHOUT acquiring a lock.
/// Use only inside a with_board_lock closure to avoid the dual-writer race.
pub fn write_discussion_unlocked(dir: &str, state: &DiscussionState) -> bool {
    let path = active_discussion_path(dir);
    let json = match serde_json::to_string_pretty(state) {
        Ok(s) => s,
        Err(_) => return false,
    };
    std::fs::write(&path, json).is_ok()
}

/// Remove session bindings whose heartbeat age exceeds timeout * 2.5 (gone threshold).
/// Uses file locking to safely modify sessions.json.
/// Returns true if any bindings were removed.
pub fn cleanup_gone_sessions(dir: &str) -> bool {
    let vaak_dir = Path::new(dir).join(".vaak");
    let sessions_path = vaak_dir.join("sessions.json");
    let lock_path = vaak_dir.join("board.lock");

    // Read current sessions
    let content = match std::fs::read_to_string(&sessions_path) {
        Ok(c) => c,
        Err(_) => return false,
    };
    let mut sessions: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return false,
    };

    // Read project settings
    let config_path = vaak_dir.join("project.json");
    let config_val = std::fs::read_to_string(&config_path)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok());

    // If auto_collab is on, NEVER clean up sessions — they should persist indefinitely
    let auto_collab = config_val.as_ref()
        .and_then(|v| v.get("settings")?.get("auto_collab")?.as_bool())
        .unwrap_or(false);
    if auto_collab {
        return false;
    }

    let timeout_secs = config_val.as_ref()
        .and_then(|v| v.get("settings")?.get("heartbeat_timeout_seconds")?.as_u64())
        .unwrap_or(120);
    let gone_threshold = (timeout_secs as f64 * 2.5) as u64;

    let now_secs = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();

    let bindings = match sessions.get_mut("bindings").and_then(|b| b.as_array_mut()) {
        Some(b) => b,
        None => return false,
    };

    let before_len = bindings.len();
    bindings.retain(|b| {
        let hb = b.get("last_heartbeat").and_then(|h| h.as_str()).unwrap_or("");
        let age = parse_iso_epoch(hb)
            .map(|hb_secs| now_secs.saturating_sub(hb_secs))
            .unwrap_or(u64::MAX);
        age <= gone_threshold
    });

    if bindings.len() == before_len {
        return false; // Nothing to clean up
    }

    // Acquire file lock and write back
    let lock_file = match std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .open(&lock_path)
    {
        Ok(f) => f,
        Err(_) => return false,
    };

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
        if locked == 0 { return false; }

        let result = match serde_json::to_string_pretty(&sessions) {
            Ok(s) => std::fs::write(&sessions_path, s).is_ok(),
            Err(_) => false,
        };

        unsafe { UnlockFileEx(handle as _, 0, u32::MAX, u32::MAX, &mut overlapped); }
        result
    }

    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        let fd = lock_file.as_raw_fd();
        if unsafe { libc::flock(fd, libc::LOCK_EX) } != 0 { return false; }

        let result = match serde_json::to_string_pretty(&sessions) {
            Ok(s) => std::fs::write(&sessions_path, s).is_ok(),
            Err(_) => false,
        };

        unsafe { libc::flock(fd, libc::LOCK_UN); }
        result
    }
}

// ==================== Sections ====================

/// Info about a project section (sub-context with its own message board)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SectionInfo {
    pub slug: String,
    pub name: String,
    pub created_at: String,
    pub message_count: u64,
    pub last_activity: Option<String>,
    pub is_active: bool,
}

/// Generate ISO 8601 UTC timestamp
pub fn iso_now() -> String {
    let dur = SystemTime::now()
        .duration_since(UNIX_EPOCH)
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
    for i in 0..12 {
        if remaining < month_days[i] { break; }
        remaining -= month_days[i];
        m = i as u64 + 1;
    }
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", y, m + 1, remaining + 1, hours, mins, s)
}

/// Slugify a section name: lowercase, replace non-alphanumeric with hyphens, collapse
pub fn slugify(name: &str) -> String {
    let raw: String = name.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect();
    raw.split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<&str>>()
        .join("-")
}

/// Get the active section slug from project.json. Returns "default" if not set.
pub fn get_active_section(dir: &str) -> String {
    let config_path = Path::new(dir).join(".vaak").join("project.json");
    std::fs::read_to_string(&config_path)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.get("active_section")?.as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "default".to_string())
}

/// Set the active section in project.json
pub fn set_active_section(dir: &str, section: &str) -> Result<(), String> {
    let config_path = Path::new(dir).join(".vaak").join("project.json");
    let content = std::fs::read_to_string(&config_path)
        .map_err(|e| format!("Failed to read project.json: {}", e))?;
    let mut config: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse project.json: {}", e))?;

    config["active_section"] = serde_json::Value::String(section.to_string());
    config["updated_at"] = serde_json::Value::String(iso_now());

    let json = serde_json::to_string_pretty(&config)
        .map_err(|e| format!("Failed to serialize project.json: {}", e))?;
    std::fs::write(&config_path, json)
        .map_err(|e| format!("Failed to write project.json: {}", e))?;
    Ok(())
}

/// Get the board.jsonl path for a given section.
/// "default" section uses legacy .vaak/board.jsonl for backward compatibility.
pub fn board_path_for_section(dir: &str, section: &str) -> PathBuf {
    let vaak_dir = Path::new(dir).join(".vaak");
    if section == "default" {
        vaak_dir.join("board.jsonl")
    } else {
        vaak_dir.join("sections").join(section).join("board.jsonl")
    }
}

/// Get the discussion.json path for a given section.
/// "default" section uses legacy .vaak/discussion.json for backward compatibility.
pub fn discussion_path_for_section(dir: &str, section: &str) -> PathBuf {
    let vaak_dir = Path::new(dir).join(".vaak");
    if section == "default" {
        vaak_dir.join("discussion.json")
    } else {
        vaak_dir.join("sections").join(section).join("discussion.json")
    }
}

/// Get the board.jsonl path for the active section.
pub fn active_board_path(dir: &str) -> PathBuf {
    board_path_for_section(dir, &get_active_section(dir))
}

/// Get the discussion.json path for the active section.
pub fn active_discussion_path(dir: &str) -> PathBuf {
    discussion_path_for_section(dir, &get_active_section(dir))
}

/// Create a new section. Returns the section info.
pub fn create_section(dir: &str, name: &str) -> Result<SectionInfo, String> {
    let slug = slugify(name);
    if slug.is_empty() {
        return Err("Section name cannot be empty".to_string());
    }

    let vaak_dir = Path::new(dir).join(".vaak");
    let sec_dir = vaak_dir.join("sections").join(&slug);
    if sec_dir.exists() {
        return Err(format!("Section '{}' already exists", slug));
    }

    std::fs::create_dir_all(&sec_dir)
        .map_err(|e| format!("Failed to create section directory: {}", e))?;

    // Create empty board.jsonl
    std::fs::write(sec_dir.join("board.jsonl"), "")
        .map_err(|e| format!("Failed to create board.jsonl: {}", e))?;

    let now = iso_now();

    // Write section metadata
    let meta = serde_json::json!({
        "name": name,
        "slug": slug,
        "created_at": now,
    });
    std::fs::write(
        sec_dir.join("section.json"),
        serde_json::to_string_pretty(&meta).unwrap_or_default(),
    )
    .map_err(|e| format!("Failed to write section.json: {}", e))?;

    Ok(SectionInfo {
        slug,
        name: name.to_string(),
        created_at: now,
        message_count: 0,
        last_activity: None,
        is_active: false,
    })
}

/// Count messages and find last activity timestamp from a board.jsonl file.
fn count_board_messages(board_path: &Path) -> (u64, Option<String>) {
    match std::fs::read_to_string(board_path) {
        Ok(content) => {
            let lines: Vec<&str> = content.lines().filter(|l| !l.trim().is_empty()).collect();
            let count = lines.len() as u64;
            let last = lines.last()
                .and_then(|l| serde_json::from_str::<serde_json::Value>(l).ok())
                .and_then(|v| v.get("timestamp")?.as_str().map(|s| s.to_string()));
            (count, last)
        }
        Err(_) => (0, None),
    }
}

/// List all sections in the project.
pub fn list_sections(dir: &str) -> Vec<SectionInfo> {
    let vaak_dir = Path::new(dir).join(".vaak");
    let sections_dir = vaak_dir.join("sections");
    let active = get_active_section(dir);
    let mut sections = Vec::new();

    // Always include "default" section (legacy root files)
    let default_board = vaak_dir.join("board.jsonl");
    let (default_count, default_last) = count_board_messages(&default_board);
    let project_created = std::fs::read_to_string(vaak_dir.join("project.json"))
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.get("created_at")?.as_str().map(|s| s.to_string()))
        .unwrap_or_default();

    sections.push(SectionInfo {
        slug: "default".to_string(),
        name: "Default".to_string(),
        created_at: project_created,
        message_count: default_count,
        last_activity: default_last,
        is_active: active == "default",
    });

    // Scan sections/ directory for additional sections
    if let Ok(entries) = std::fs::read_dir(&sections_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() { continue; }
            let slug = match path.file_name().and_then(|n| n.to_str()) {
                Some(s) => s.to_string(),
                None => continue,
            };

            let meta_path = path.join("section.json");
            let (name, created_at) = if let Ok(content) = std::fs::read_to_string(&meta_path) {
                if let Ok(meta) = serde_json::from_str::<serde_json::Value>(&content) {
                    (
                        meta.get("name").and_then(|n| n.as_str()).unwrap_or(&slug).to_string(),
                        meta.get("created_at").and_then(|c| c.as_str()).unwrap_or("").to_string(),
                    )
                } else {
                    (slug.clone(), String::new())
                }
            } else {
                (slug.clone(), String::new())
            };

            let board_path = path.join("board.jsonl");
            let (message_count, last_activity) = count_board_messages(&board_path);

            sections.push(SectionInfo {
                slug: slug.clone(),
                name,
                created_at,
                message_count,
                last_activity,
                is_active: active == slug,
            });
        }
    }

    // Sort: active first, then by created_at
    sections.sort_by(|a, b| {
        b.is_active.cmp(&a.is_active)
            .then_with(|| a.created_at.cmp(&b.created_at))
    });
    sections
}

// ==================== Roster Management ====================

/// Add a roster slot for a role. Auto-assigns instance number. No max_instances limit.
pub fn roster_add_slot(dir: &str, role: &str) -> Result<RosterSlot, String> {
    let config_path = Path::new(dir).join(".vaak").join("project.json");
    let content = std::fs::read_to_string(&config_path)
        .map_err(|e| format!("Failed to read project.json: {}", e))?;
    let mut config: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse project.json: {}", e))?;

    // Verify role exists in catalog
    let _role_def = config.get("roles")
        .and_then(|r| r.get(role))
        .ok_or(format!("Role '{}' not found in project.json roles catalog", role))?;

    // Get or create roster array — auto-migrate from sessions on first use
    let needs_migration = match config.get("roster").and_then(|r| r.as_array()) {
        Some(arr) if !arr.is_empty() => false,
        _ => true, // None, null, or empty array all trigger migration
    };
    if needs_migration {
        // Migration: seed roster from existing active sessions
        let mut seed = serde_json::json!([]);
        let sessions_path = Path::new(dir).join(".vaak").join("sessions.json");
        if let Ok(sc) = std::fs::read_to_string(&sessions_path) {
            if let Ok(sv) = serde_json::from_str::<serde_json::Value>(&sc) {
                if let Some(bindings) = sv.get("bindings").and_then(|b| b.as_array()) {
                    let arr = seed.as_array_mut().unwrap();
                    for b in bindings {
                        let b_role = b.get("role").and_then(|r| r.as_str()).unwrap_or("");
                        let b_inst = b.get("instance").and_then(|i| i.as_u64()).unwrap_or(0);
                        let claimed = b.get("claimed_at").and_then(|c| c.as_str()).unwrap_or("");
                        // Only add if not already seeded (dedup)
                        let already = arr.iter().any(|s| {
                            s.get("role").and_then(|r| r.as_str()) == Some(b_role)
                                && s.get("instance").and_then(|i| i.as_u64()) == Some(b_inst)
                        });
                        if !already && !b_role.is_empty() {
                            arr.push(serde_json::json!({
                                "role": b_role,
                                "instance": b_inst,
                                "added_at": if claimed.is_empty() { iso_now() } else { claimed.to_string() }
                            }));
                        }
                    }
                }
            }
        }
        config["roster"] = seed;
    }
    let roster = config.get_mut("roster").and_then(|r| r.as_array_mut())
        .ok_or("Failed to access roster array")?;

    // Count existing slots for this role
    let existing: Vec<u32> = roster.iter()
        .filter(|s| s.get("role").and_then(|r| r.as_str()) == Some(role))
        .filter_map(|s| s.get("instance").and_then(|i| i.as_u64()).map(|i| i as u32))
        .collect();

    // No max_instances enforcement — users can add unlimited slots per role

    // Auto-assign instance number (find first gap)
    let mut instance = 0u32;
    while existing.contains(&instance) {
        instance += 1;
    }

    let now = iso_now();
    let slot = RosterSlot {
        role: role.to_string(),
        instance,
        added_at: now.clone(),
    };

    roster.push(serde_json::json!({
        "role": role,
        "instance": instance,
        "added_at": now
    }));

    // Update timestamp
    config["updated_at"] = serde_json::json!(iso_now());

    let updated = serde_json::to_string_pretty(&config)
        .map_err(|e| format!("Failed to serialize project.json: {}", e))?;
    std::fs::write(&config_path, updated)
        .map_err(|e| format!("Failed to write project.json: {}", e))?;

    Ok(slot)
}

/// Remove a roster slot and revoke any bound session.
pub fn roster_remove_slot(dir: &str, role: &str, instance: i32) -> Result<(), String> {
    let vaak_dir = Path::new(dir).join(".vaak");
    let config_path = vaak_dir.join("project.json");

    // 1. Remove slot from project.json roster
    let content = std::fs::read_to_string(&config_path)
        .map_err(|e| format!("Failed to read project.json: {}", e))?;
    let mut config: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse project.json: {}", e))?;

    if let Some(roster) = config.get_mut("roster").and_then(|r| r.as_array_mut()) {
        let before_len = roster.len();
        roster.retain(|s| {
            !(s.get("role").and_then(|r| r.as_str()) == Some(role)
                && s.get("instance").and_then(|i| i.as_i64()) == Some(instance as i64))
        });
        if roster.len() == before_len {
            return Err(format!("No roster slot found for {}:{}", role, instance));
        }
    } else {
        return Err("No roster array in project.json".to_string());
    }

    config["updated_at"] = serde_json::json!(iso_now());
    let updated = serde_json::to_string_pretty(&config)
        .map_err(|e| format!("Failed to serialize project.json: {}", e))?;
    std::fs::write(&config_path, updated)
        .map_err(|e| format!("Failed to write project.json: {}", e))?;

    // 2. Revoke any bound session in sessions.json
    let sessions_path = vaak_dir.join("sessions.json");
    if let Ok(sessions_content) = std::fs::read_to_string(&sessions_path) {
        if let Ok(mut sessions) = serde_json::from_str::<serde_json::Value>(&sessions_content) {
            if let Some(bindings) = sessions.get_mut("bindings").and_then(|b| b.as_array_mut()) {
                bindings.retain(|b| {
                    !(b.get("role").and_then(|r| r.as_str()) == Some(role)
                        && b.get("instance").and_then(|i| i.as_i64()) == Some(instance as i64))
                });
                if let Ok(updated) = serde_json::to_string_pretty(&sessions) {
                    let _ = std::fs::write(&sessions_path, updated);
                }
            }
        }
    }

    Ok(())
}

/// Get roster with computed status by cross-referencing sessions.json.
pub fn roster_get(dir: &str) -> Result<Vec<RosterSlotWithStatus>, String> {
    let vaak_dir = Path::new(dir).join(".vaak");

    // Read project.json
    let config_path = vaak_dir.join("project.json");
    let content = std::fs::read_to_string(&config_path)
        .map_err(|e| format!("Failed to read project.json: {}", e))?;
    let config: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse project.json: {}", e))?;

    let roster = match config.get("roster").and_then(|r| r.as_array()) {
        Some(r) => r,
        None => return Ok(vec![]), // No roster = empty
    };

    // Read sessions.json
    let sessions_path = vaak_dir.join("sessions.json");
    let bindings: Vec<serde_json::Value> = std::fs::read_to_string(&sessions_path)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.get("bindings")?.as_array().cloned())
        .unwrap_or_default();

    let timeout_secs = config.get("settings")
        .and_then(|s| s.get("heartbeat_timeout_seconds"))
        .and_then(|t| t.as_u64())
        .unwrap_or(120);
    let now_secs = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();

    let mut result = Vec::new();
    for slot in roster {
        let role = slot.get("role").and_then(|r| r.as_str()).unwrap_or("");
        let instance = slot.get("instance").and_then(|i| i.as_u64()).unwrap_or(0) as u32;
        let added_at = slot.get("added_at").and_then(|a| a.as_str()).unwrap_or("").to_string();

        // Find matching session binding
        let binding = bindings.iter().find(|b| {
            b.get("role").and_then(|r| r.as_str()) == Some(role)
                && b.get("instance").and_then(|i| i.as_u64()) == Some(instance as u64)
                && b.get("status").and_then(|s| s.as_str()) == Some("active")
        });

        let (status, session_id) = match binding {
            Some(b) => {
                // Check if heartbeat is fresh
                let hb = b.get("last_heartbeat").and_then(|h| h.as_str()).unwrap_or("");
                let is_stale = match parse_iso_epoch(hb) {
                    Some(hb_secs) => now_secs.saturating_sub(hb_secs) > timeout_secs,
                    None => true,
                };
                if is_stale {
                    ("vacant".to_string(), None)
                } else {
                    let activity = b.get("activity").and_then(|a| a.as_str()).unwrap_or("standby");
                    let sid = b.get("session_id").and_then(|s| s.as_str()).map(|s| s.to_string());
                    (activity.to_string(), sid)
                }
            },
            None => ("vacant".to_string(), None),
        };

        result.push(RosterSlotWithStatus {
            role: role.to_string(),
            instance,
            added_at,
            status,
            session_id,
        });
    }

    Ok(result)
}
