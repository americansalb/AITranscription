use serde::{Deserialize, Serialize};

// ==================== Session Registry ====================

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Atomic file write: write to .tmp file, fsync, then rename over target.
/// Protects against partial writes and advisory lock races on macOS.
pub fn atomic_write(path: &Path, content: &[u8]) -> Result<(), String> {
    let tmp_path = path.with_extension("tmp");
    // Write content to temp file
    std::fs::write(&tmp_path, content)
        .map_err(|e| format!("Failed to write temp file {}: {}", tmp_path.display(), e))?;
    // fsync the temp file to ensure data is on disk
    if let Ok(f) = std::fs::File::open(&tmp_path) {
        let _ = f.sync_all();
    }
    // Atomic rename (on Unix, rename is atomic; on Windows, it's close enough)
    std::fs::rename(&tmp_path, path)
        .map_err(|e| format!("Failed to rename {} -> {}: {}", tmp_path.display(), path.display(), e))?;
    Ok(())
}

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

fn default_true() -> bool { true }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompanionConfig {
    pub role: String,
    #[serde(default)]
    pub optional: bool,
    #[serde(default = "default_true")]
    pub default_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleConfig {
    pub title: String,
    pub description: String,
    pub max_instances: u32,
    pub permissions: Vec<String>,
    pub created_at: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub companions: Vec<CompanionConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RosterSlot {
    pub role: String,
    pub instance: u32,
    pub added_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RosterSlotWithStatus {
    pub role: String,
    pub instance: u32,
    pub added_at: String,
    pub status: String, // "vacant", "standby", "working"
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
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
    #[serde(default, alias = "joined_at")]
    pub claimed_at: String,
    pub last_heartbeat: String,
    pub status: String,
    #[serde(default)]
    pub activity: Option<String>,
    #[serde(default)]
    pub last_working_at: Option<String>,
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
    let iso_clean = iso.trim_end_matches('Z');
    // Handle +HH:MM or -HH:MM timezone offset by stripping it
    let iso_clean = if let Some(plus_pos) = iso_clean.rfind('+') {
        if plus_pos > 10 { &iso_clean[..plus_pos] } else { iso_clean }
    } else if let Some(minus_pos) = iso_clean.rfind('-') {
        if minus_pos > 10 { &iso_clean[..minus_pos] } else { iso_clean }
    } else {
        iso_clean
    };
    let (date_part, time_part) = iso_clean.split_once('T')?;
    let dp: Vec<&str> = date_part.split('-').collect();
    let tp: Vec<&str> = time_part.split(':').collect();
    if dp.len() != 3 || tp.len() < 3 { return None; }
    let (year, month, day): (u64, u64, u64) = (dp[0].parse().ok()?, dp[1].parse().ok()?, dp[2].parse().ok()?);
    // Handle fractional seconds like "45.123"
    let sec: u64 = tp[2].split('.').next()?.parse().ok()?;
    let (hour, min): (u64, u64) = (tp[0].parse().ok()?, tp[1].parse().ok()?);
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
            let _ = atomic_write(&claims_path, s.as_bytes());
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

    // Acquire file lock with timeout (matches with_board_lock pattern)
    let lock_file = match std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .open(&lock_path)
    {
        Ok(f) => f,
        Err(_) => return false,
    };

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
                    eprintln!("[write_discussion] Lock timeout after {}s on {}", LOCK_TIMEOUT_MS / 1000, lock_path.display());
                    return false;
                }
            }
        }

        let result = atomic_write(&path, json.as_bytes()).is_ok();

        unsafe { UnlockFileEx(handle as _, 0, u32::MAX, u32::MAX, &mut overlapped); }
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
                    eprintln!("[write_discussion] Lock timeout after {}s on {}", LOCK_TIMEOUT_MS / 1000, lock_path.display());
                    return false;
                }
            }
        }

        let result = atomic_write(&path, json.as_bytes()).is_ok();

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

    // Stale lock protection: try non-blocking first, then retry with timeout.
    // OS-level file locks (LockFileEx/flock) auto-release on process death,
    // but a hung process can hold the lock indefinitely. The timeout prevents
    // infinite blocking in that case.
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
                        "board.lock held for >{}s — possible stale lock from hung process. Lock file: {}",
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
                        "board.lock held for >{}s — possible stale lock from hung process. Lock file: {}",
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

/// Write discussion state WITHOUT acquiring a lock.
/// Use only inside a with_board_lock closure to avoid the dual-writer race.
pub fn write_discussion_unlocked(dir: &str, state: &DiscussionState) -> bool {
    let path = active_discussion_path(dir);
    let json = match serde_json::to_string_pretty(state) {
        Ok(s) => s,
        Err(_) => return false,
    };
    atomic_write(&path, json.as_bytes()).is_ok()
}

/// Compact board.jsonl by removing messages older than `max_age_days`.
/// Keeps the last `min_keep` messages regardless of age to preserve context.
/// Returns (kept, removed) counts. Uses board lock for safety.
pub fn compact_board(dir: &str, max_age_days: u64, min_keep: usize) -> Result<(usize, usize), String> {
    with_board_lock(dir, || {
        let board_path = active_board_path(dir);
        let content = std::fs::read_to_string(&board_path).unwrap_or_default();
        let lines: Vec<&str> = content.lines().filter(|l| !l.trim().is_empty()).collect();
        let total = lines.len();

        if total <= min_keep {
            return Ok((total, 0));
        }

        let now_epoch = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let cutoff = now_epoch.saturating_sub(max_age_days * 86400);

        // Parse each line and check timestamp
        let mut keep: Vec<&str> = Vec::with_capacity(total);
        let mut removed = 0usize;

        for line in &lines {
            let should_keep = if let Ok(msg) = serde_json::from_str::<serde_json::Value>(line) {
                let ts_str = msg.get("timestamp").and_then(|t| t.as_str()).unwrap_or("");
                match parse_iso_epoch(ts_str) {
                    Some(epoch) => epoch >= cutoff,
                    None => true, // Keep unparseable messages
                }
            } else {
                true // Keep unparseable lines
            };

            if should_keep {
                keep.push(line);
            } else {
                removed += 1;
            }
        }

        // Always keep at least min_keep messages (take from the end = most recent)
        if keep.len() < min_keep && total >= min_keep {
            keep = lines[total - min_keep..].to_vec();
            removed = total - min_keep;
        }

        if removed == 0 {
            return Ok((total, 0));
        }

        // Write compacted board via atomic rename
        let tmp_path = board_path.with_extension("jsonl.tmp");
        let mut output = String::with_capacity(keep.iter().map(|l| l.len() + 1).sum());
        for line in &keep {
            output.push_str(line);
            output.push('\n');
        }
        std::fs::write(&tmp_path, &output)
            .map_err(|e| format!("Failed to write temp board: {}", e))?;
        std::fs::rename(&tmp_path, &board_path)
            .map_err(|e| format!("Failed to rename temp board: {}", e))?;

        Ok((keep.len(), removed))
    })
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
            Ok(s) => atomic_write(&sessions_path, s.as_bytes()).is_ok(),
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
            Ok(s) => atomic_write(&sessions_path, s.as_bytes()).is_ok(),
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
    atomic_write(&config_path, json.as_bytes())
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
/// Optional metadata (e.g., `{"pool_id": "software-dev"}` for audience roles).
pub fn roster_add_slot(dir: &str, role: &str, metadata: Option<serde_json::Value>) -> Result<RosterSlot, String> {
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
        metadata: metadata.clone(),
    };

    let mut slot_json = serde_json::json!({
        "role": role,
        "instance": instance,
        "added_at": now
    });
    if let Some(ref meta) = metadata {
        slot_json["metadata"] = meta.clone();
    }
    roster.push(slot_json);

    // Update timestamp
    config["updated_at"] = serde_json::json!(iso_now());

    let updated = serde_json::to_string_pretty(&config)
        .map_err(|e| format!("Failed to serialize project.json: {}", e))?;
    atomic_write(&config_path, updated.as_bytes())
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
    atomic_write(&config_path, updated.as_bytes())
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
                    let _ = atomic_write(&sessions_path, updated.as_bytes());
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

        let metadata = slot.get("metadata").cloned();
        result.push(RosterSlotWithStatus {
            role: role.to_string(),
            instance,
            added_at,
            status,
            session_id,
            metadata,
        });
    }

    Ok(result)
}

// ==================== Role CRUD ====================

const BUILT_IN_ROLES: &[&str] = &["developer", "manager", "architect", "tester", "moderator"];

/// Validate a role slug: lowercase alphanumeric + hyphens, non-empty.
fn validate_slug(slug: &str) -> Result<(), String> {
    if slug.is_empty() {
        return Err("Role slug cannot be empty".to_string());
    }
    if !slug.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') {
        return Err("Role slug must be lowercase alphanumeric with hyphens only".to_string());
    }
    if slug.starts_with('-') || slug.ends_with('-') {
        return Err("Role slug cannot start or end with a hyphen".to_string());
    }
    Ok(())
}

/// Create a new role in project.json and write its briefing file.
pub fn create_role(
    dir: &str,
    slug: &str,
    title: &str,
    description: &str,
    permissions: Vec<String>,
    max_instances: u32,
    briefing: &str,
    tags: Vec<String>,
    companions: Vec<CompanionConfig>,
) -> Result<RoleConfig, String> {
    validate_slug(slug)?;

    let vaak_dir = Path::new(dir).join(".vaak");
    let config_path = vaak_dir.join("project.json");
    let lock_path = vaak_dir.join("board.lock");

    // Acquire file lock for project.json modification
    let lock_file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .open(&lock_path)
        .map_err(|e| format!("Failed to open lock file: {}", e))?;

    #[cfg(windows)]
    let result = {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{LockFileEx, UnlockFileEx, LOCKFILE_EXCLUSIVE_LOCK};
        use windows_sys::Win32::System::IO::OVERLAPPED;

        let handle = lock_file.as_raw_handle();
        let mut overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };
        let locked = unsafe {
            LockFileEx(handle as _, LOCKFILE_EXCLUSIVE_LOCK, 0, u32::MAX, u32::MAX, &mut overlapped)
        };
        if locked == 0 {
            return Err("Failed to acquire lock".to_string());
        }

        let result = create_role_inner(&config_path, &vaak_dir, slug, title, description, &permissions, max_instances, briefing, &tags, &companions);

        unsafe { UnlockFileEx(handle as _, 0, u32::MAX, u32::MAX, &mut overlapped); }
        result
    };

    #[cfg(unix)]
    let result = {
        use std::os::unix::io::AsRawFd;
        let fd = lock_file.as_raw_fd();
        if unsafe { libc::flock(fd, libc::LOCK_EX) } != 0 {
            return Err("Failed to acquire lock".to_string());
        }

        let result = create_role_inner(&config_path, &vaak_dir, slug, title, description, &permissions, max_instances, briefing, &tags, &companions);

        unsafe { libc::flock(fd, libc::LOCK_UN); }
        result
    };

    // Auto-save as global template (non-blocking: log error but don't fail role creation)
    if result.is_ok() {
        if let Err(e) = save_role_as_global_template(dir, slug) {
            eprintln!("[collab] Auto-save global template for '{}' failed: {}", slug, e);
        }
    }

    result
}

fn create_role_inner(
    config_path: &Path,
    vaak_dir: &Path,
    slug: &str,
    title: &str,
    description: &str,
    permissions: &[String],
    max_instances: u32,
    briefing: &str,
    tags: &[String],
    companions: &[CompanionConfig],
) -> Result<RoleConfig, String> {
    let content = std::fs::read_to_string(config_path)
        .map_err(|e| format!("Failed to read project.json: {}", e))?;
    let mut config: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse project.json: {}", e))?;

    // Check slug uniqueness
    if let Some(roles) = config.get("roles").and_then(|r| r.as_object()) {
        if roles.contains_key(slug) {
            return Err(format!("Role '{}' already exists", slug));
        }
    }

    let now = iso_now();
    let role_config = RoleConfig {
        title: title.to_string(),
        description: description.to_string(),
        max_instances,
        permissions: permissions.to_vec(),
        created_at: now.clone(),
        tags: tags.to_vec(),
        companions: companions.to_vec(),
    };

    // Add role to config
    let mut role_json = serde_json::json!({
        "title": title,
        "description": description,
        "max_instances": max_instances,
        "permissions": permissions,
        "created_at": now,
        "tags": tags,
    });
    if !companions.is_empty() {
        role_json["companions"] = serde_json::to_value(companions)
            .map_err(|e| format!("Failed to serialize companions: {}", e))?;
    }

    config.get_mut("roles")
        .and_then(|r| r.as_object_mut())
        .ok_or("No roles object in project.json")?
        .insert(slug.to_string(), role_json);

    config["updated_at"] = serde_json::Value::String(iso_now());

    let updated = serde_json::to_string_pretty(&config)
        .map_err(|e| format!("Failed to serialize project.json: {}", e))?;
    atomic_write(config_path, updated.as_bytes())
        .map_err(|e| format!("Failed to write project.json: {}", e))?;

    // Create briefing file
    let roles_dir = vaak_dir.join("roles");
    std::fs::create_dir_all(&roles_dir)
        .map_err(|e| format!("Failed to create roles directory: {}", e))?;
    let briefing_path = roles_dir.join(format!("{}.md", slug));
    std::fs::write(&briefing_path, briefing)
        .map_err(|e| format!("Failed to write briefing file: {}", e))?;

    Ok(role_config)
}

/// Update an existing role's metadata and/or briefing.
pub fn update_role(
    dir: &str,
    slug: &str,
    title: Option<&str>,
    description: Option<&str>,
    permissions: Option<Vec<String>>,
    max_instances: Option<u32>,
    briefing: Option<&str>,
    tags: Option<Vec<String>>,
    companions: Option<Vec<CompanionConfig>>,
) -> Result<RoleConfig, String> {
    let vaak_dir = Path::new(dir).join(".vaak");
    let config_path = vaak_dir.join("project.json");
    let lock_path = vaak_dir.join("board.lock");

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
            return Err("Failed to acquire lock".to_string());
        }

        let result = update_role_inner(&config_path, &vaak_dir, slug, title, description, permissions.as_deref(), max_instances, briefing, tags.as_deref(), companions.as_deref());

        unsafe { UnlockFileEx(handle as _, 0, u32::MAX, u32::MAX, &mut overlapped); }
        result
    }

    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        let fd = lock_file.as_raw_fd();
        if unsafe { libc::flock(fd, libc::LOCK_EX) } != 0 {
            return Err("Failed to acquire lock".to_string());
        }

        let result = update_role_inner(&config_path, &vaak_dir, slug, title, description, permissions.as_deref(), max_instances, briefing, tags.as_deref(), companions.as_deref());

        unsafe { libc::flock(fd, libc::LOCK_UN); }
        result
    }
}

fn update_role_inner(
    config_path: &Path,
    vaak_dir: &Path,
    slug: &str,
    title: Option<&str>,
    description: Option<&str>,
    permissions: Option<&[String]>,
    max_instances: Option<u32>,
    briefing: Option<&str>,
    tags: Option<&[String]>,
    companions: Option<&[CompanionConfig]>,
) -> Result<RoleConfig, String> {
    let content = std::fs::read_to_string(config_path)
        .map_err(|e| format!("Failed to read project.json: {}", e))?;
    let mut config: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse project.json: {}", e))?;

    {
        let role = config.get_mut("roles")
            .and_then(|r| r.as_object_mut())
            .and_then(|roles| roles.get_mut(slug))
            .ok_or(format!("Role '{}' not found", slug))?;

        if let Some(t) = title {
            role["title"] = serde_json::Value::String(t.to_string());
        }
        if let Some(d) = description {
            role["description"] = serde_json::Value::String(d.to_string());
        }
        if let Some(p) = permissions {
            role["permissions"] = serde_json::json!(p);
        }
        if let Some(m) = max_instances {
            role["max_instances"] = serde_json::json!(m);
        }
        if let Some(t) = tags {
            role["tags"] = serde_json::json!(t);
        }
        if let Some(c) = companions {
            if c.is_empty() {
                role.as_object_mut().map(|o| o.remove("companions"));
            } else {
                role["companions"] = serde_json::to_value(c)
                    .map_err(|e| format!("Failed to serialize companions: {}", e))?;
            }
        }
    }

    config["updated_at"] = serde_json::Value::String(iso_now());

    // Re-read the updated role for the return value
    let updated_role: RoleConfig = config.get("roles")
        .and_then(|r| r.get(slug))
        .and_then(|r| serde_json::from_value(r.clone()).ok())
        .ok_or(format!("Failed to read back updated role '{}'", slug))?;

    let updated = serde_json::to_string_pretty(&config)
        .map_err(|e| format!("Failed to serialize project.json: {}", e))?;
    atomic_write(config_path, updated.as_bytes())
        .map_err(|e| format!("Failed to write project.json: {}", e))?;

    // Update briefing file if provided
    if let Some(b) = briefing {
        let briefing_path = vaak_dir.join("roles").join(format!("{}.md", slug));
        std::fs::write(&briefing_path, b)
            .map_err(|e| format!("Failed to write briefing file: {}", e))?;
    }

    Ok(updated_role)
}

/// Delete a role from project.json, remove its briefing file and roster entries.
/// Refuses to delete built-in roles or roles with active sessions.
pub fn delete_role(dir: &str, slug: &str) -> Result<(), String> {
    // Check built-in
    if BUILT_IN_ROLES.contains(&slug) {
        return Err(format!("Cannot delete built-in role '{}'", slug));
    }

    let vaak_dir = Path::new(dir).join(".vaak");
    let config_path = vaak_dir.join("project.json");
    let sessions_path = vaak_dir.join("sessions.json");
    let lock_path = vaak_dir.join("board.lock");

    // Check for active sessions before acquiring lock
    let timeout_secs = {
        let content = std::fs::read_to_string(&config_path)
            .map_err(|e| format!("Failed to read project.json: {}", e))?;
        let config: serde_json::Value = serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse project.json: {}", e))?;
        config.get("settings")
            .and_then(|s| s.get("heartbeat_timeout_seconds"))
            .and_then(|t| t.as_u64())
            .unwrap_or(120)
    };

    if let Ok(sessions_content) = std::fs::read_to_string(&sessions_path) {
        if let Ok(sessions) = serde_json::from_str::<serde_json::Value>(&sessions_content) {
            if let Some(bindings) = sessions.get("bindings").and_then(|b| b.as_array()) {
                let now_secs = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
                let has_active = bindings.iter().any(|b| {
                    let role_match = b.get("role").and_then(|r| r.as_str()) == Some(slug);
                    let is_active = b.get("status").and_then(|s| s.as_str()) == Some("active");
                    let hb = b.get("last_heartbeat").and_then(|h| h.as_str()).unwrap_or("");
                    let is_fresh = parse_iso_epoch(hb)
                        .map(|hb_secs| now_secs.saturating_sub(hb_secs) <= timeout_secs)
                        .unwrap_or(false);
                    role_match && is_active && is_fresh
                });
                if has_active {
                    return Err(format!("Cannot delete role '{}': has active sessions. Remove agents first.", slug));
                }
            }
        }
    }

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
            return Err("Failed to acquire lock".to_string());
        }

        let result = delete_role_inner(&config_path, &vaak_dir, slug);

        unsafe { UnlockFileEx(handle as _, 0, u32::MAX, u32::MAX, &mut overlapped); }
        result
    }

    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        let fd = lock_file.as_raw_fd();
        if unsafe { libc::flock(fd, libc::LOCK_EX) } != 0 {
            return Err("Failed to acquire lock".to_string());
        }

        let result = delete_role_inner(&config_path, &vaak_dir, slug);

        unsafe { libc::flock(fd, libc::LOCK_UN); }
        result
    }
}

fn delete_role_inner(
    config_path: &Path,
    vaak_dir: &Path,
    slug: &str,
) -> Result<(), String> {
    let content = std::fs::read_to_string(config_path)
        .map_err(|e| format!("Failed to read project.json: {}", e))?;
    let mut config: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse project.json: {}", e))?;

    // Remove from roles catalog
    let removed = config.get_mut("roles")
        .and_then(|r| r.as_object_mut())
        .map(|roles| roles.remove(slug).is_some())
        .unwrap_or(false);

    if !removed {
        return Err(format!("Role '{}' not found in project.json", slug));
    }

    // Remove roster entries for this role
    if let Some(roster) = config.get_mut("roster").and_then(|r| r.as_array_mut()) {
        roster.retain(|s| {
            s.get("role").and_then(|r| r.as_str()) != Some(slug)
        });
    }

    config["updated_at"] = serde_json::Value::String(iso_now());

    let updated = serde_json::to_string_pretty(&config)
        .map_err(|e| format!("Failed to serialize project.json: {}", e))?;
    atomic_write(config_path, updated.as_bytes())
        .map_err(|e| format!("Failed to write project.json: {}", e))?;

    // Remove briefing file (best-effort)
    let briefing_path = vaak_dir.join("roles").join(format!("{}.md", slug));
    let _ = std::fs::remove_file(&briefing_path);

    // Remove session bindings for this role (best-effort)
    let sessions_path = vaak_dir.join("sessions.json");
    if let Ok(sessions_content) = std::fs::read_to_string(&sessions_path) {
        if let Ok(mut sessions) = serde_json::from_str::<serde_json::Value>(&sessions_content) {
            if let Some(bindings) = sessions.get_mut("bindings").and_then(|b| b.as_array_mut()) {
                bindings.retain(|b| {
                    b.get("role").and_then(|r| r.as_str()) != Some(slug)
                });
                if let Ok(updated) = serde_json::to_string_pretty(&sessions) {
                    let _ = atomic_write(&sessions_path, updated.as_bytes());
                }
            }
        }
    }

    Ok(())
}

// ==================== Global Role Templates ====================

/// Get the global role-templates directory (~/.vaak/role-templates/)
fn global_templates_dir() -> Result<PathBuf, String> {
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .ok_or("Cannot determine home directory")?;
    Ok(PathBuf::from(home).join(".vaak").join("role-templates"))
}

/// Save a role from a project as a global template.
/// Copies the role definition to ~/.vaak/role-templates/{slug}.json
/// and the briefing to ~/.vaak/role-templates/{slug}.md
pub fn save_role_as_global_template(dir: &str, slug: &str) -> Result<(), String> {
    validate_slug(slug)?;

    const BUILT_IN_ROLES: &[&str] = &["developer", "manager", "architect", "tester", "moderator"];
    if BUILT_IN_ROLES.contains(&slug) {
        return Err(format!("Cannot overwrite built-in role '{}' as a global template", slug));
    }

    let vaak_dir = Path::new(dir).join(".vaak");
    let config_path = vaak_dir.join("project.json");

    // Read role from project.json
    let content = std::fs::read_to_string(&config_path)
        .map_err(|e| format!("Failed to read project.json: {}", e))?;
    let config: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse project.json: {}", e))?;

    let role_def = config
        .get("roles")
        .and_then(|r| r.get(slug))
        .ok_or(format!("Role '{}' not found in project", slug))?
        .clone();

    // Create templates directory
    let templates_dir = global_templates_dir()?;
    std::fs::create_dir_all(&templates_dir)
        .map_err(|e| format!("Failed to create templates directory: {}", e))?;

    // Write role definition (strip created_at — it gets re-added on import)
    let mut template = role_def.clone();
    if let Some(obj) = template.as_object_mut() {
        obj.remove("created_at");
    }
    let template_path = templates_dir.join(format!("{}.json", slug));
    let json = serde_json::to_string_pretty(&template)
        .map_err(|e| format!("Failed to serialize role template: {}", e))?;
    std::fs::write(&template_path, json)
        .map_err(|e| format!("Failed to write template file: {}", e))?;

    // Copy briefing if it exists
    let briefing_src = vaak_dir.join("roles").join(format!("{}.md", slug));
    if briefing_src.exists() {
        let briefing_dest = templates_dir.join(format!("{}.md", slug));
        std::fs::copy(&briefing_src, &briefing_dest)
            .map_err(|e| format!("Failed to copy briefing template: {}", e))?;
    }

    Ok(())
}

/// List all global role templates.
/// Returns a JSON object: { slug: { title, description, tags, permissions, max_instances } }
pub fn list_global_role_templates() -> Result<serde_json::Value, String> {
    let templates_dir = global_templates_dir()?;
    if !templates_dir.exists() {
        return Ok(serde_json::json!({}));
    }

    let mut result = serde_json::Map::new();
    let entries = std::fs::read_dir(&templates_dir)
        .map_err(|e| format!("Failed to read templates directory: {}", e))?;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let slug = path.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
        if slug.is_empty() {
            continue;
        }
        if let Ok(content) = std::fs::read_to_string(&path) {
            if let Ok(template) = serde_json::from_str::<serde_json::Value>(&content) {
                result.insert(slug, template);
            }
        }
    }

    Ok(serde_json::Value::Object(result))
}

// ==================== Role Groups ====================

/// A role entry within a role group
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleGroupEntry {
    pub slug: String,
    #[serde(default = "default_one")]
    pub instances: u32,
    #[serde(default)]
    pub tags: Vec<String>,
}

fn default_one() -> u32 { 1 }

/// A role group (team preset)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleGroup {
    pub name: String,
    pub slug: String,
    #[serde(default)]
    pub icon: String,
    #[serde(default)]
    pub description: String,
    pub roles: Vec<RoleGroupEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub order: Option<u32>,
}

/// Save (upsert) a role group into project.json > role_groups[].
/// Matches by slug: updates if exists, appends if new.
pub fn save_role_group(dir: &str, group: RoleGroup) -> Result<RoleGroup, String> {
    validate_slug(&group.slug)?;

    let vaak_dir = Path::new(dir).join(".vaak");
    let config_path = vaak_dir.join("project.json");
    let lock_path = vaak_dir.join("board.lock");

    let lock_file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .open(&lock_path)
        .map_err(|e| format!("Failed to open lock file: {}", e))?;

    #[cfg(windows)]
    let result = {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{LockFileEx, UnlockFileEx, LOCKFILE_EXCLUSIVE_LOCK};
        use windows_sys::Win32::System::IO::OVERLAPPED;

        let handle = lock_file.as_raw_handle();
        let mut overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };
        let locked = unsafe {
            LockFileEx(handle as _, LOCKFILE_EXCLUSIVE_LOCK, 0, u32::MAX, u32::MAX, &mut overlapped)
        };
        if locked == 0 {
            return Err("Failed to acquire lock".to_string());
        }

        let result = save_role_group_inner(&config_path, &group);

        unsafe { UnlockFileEx(handle as _, 0, u32::MAX, u32::MAX, &mut overlapped); }
        result
    };

    #[cfg(unix)]
    let result = {
        use std::os::unix::io::AsRawFd;
        let fd = lock_file.as_raw_fd();
        if unsafe { libc::flock(fd, libc::LOCK_EX) } != 0 {
            return Err("Failed to acquire lock".to_string());
        }

        let result = save_role_group_inner(&config_path, &group);

        unsafe { libc::flock(fd, libc::LOCK_UN); }
        result
    };

    let _ = lock_file;
    result
}

fn save_role_group_inner(config_path: &Path, group: &RoleGroup) -> Result<RoleGroup, String> {
    let content = std::fs::read_to_string(config_path)
        .map_err(|e| format!("Failed to read project.json: {}", e))?;
    let mut config: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse project.json: {}", e))?;

    // Ensure role_groups array exists
    if config.get("role_groups").is_none() {
        config["role_groups"] = serde_json::json!([]);
    }

    let group_json = serde_json::to_value(group)
        .map_err(|e| format!("Failed to serialize role group: {}", e))?;

    let groups = config.get_mut("role_groups")
        .and_then(|g| g.as_array_mut())
        .ok_or("role_groups is not an array")?;

    // Upsert: find by slug, replace if exists, append if new
    let existing_idx = groups.iter().position(|g| {
        g.get("slug").and_then(|s| s.as_str()) == Some(&group.slug)
    });

    if let Some(idx) = existing_idx {
        groups[idx] = group_json;
    } else {
        groups.push(group_json);
    }

    config["updated_at"] = serde_json::Value::String(iso_now());

    let updated = serde_json::to_string_pretty(&config)
        .map_err(|e| format!("Failed to serialize project.json: {}", e))?;
    atomic_write(config_path, updated.as_bytes())
        .map_err(|e| format!("Failed to write project.json: {}", e))?;

    Ok(group.clone())
}

/// Delete a role group from project.json by slug.
pub fn delete_role_group(dir: &str, slug: &str) -> Result<(), String> {
    validate_slug(slug)?;

    let vaak_dir = Path::new(dir).join(".vaak");
    let config_path = vaak_dir.join("project.json");
    let lock_path = vaak_dir.join("board.lock");

    let lock_file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .open(&lock_path)
        .map_err(|e| format!("Failed to open lock file: {}", e))?;

    #[cfg(windows)]
    let result = {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{LockFileEx, UnlockFileEx, LOCKFILE_EXCLUSIVE_LOCK};
        use windows_sys::Win32::System::IO::OVERLAPPED;

        let handle = lock_file.as_raw_handle();
        let mut overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };
        let locked = unsafe {
            LockFileEx(handle as _, LOCKFILE_EXCLUSIVE_LOCK, 0, u32::MAX, u32::MAX, &mut overlapped)
        };
        if locked == 0 {
            return Err("Failed to acquire lock".to_string());
        }

        let result = delete_role_group_inner(&config_path, slug);

        unsafe { UnlockFileEx(handle as _, 0, u32::MAX, u32::MAX, &mut overlapped); }
        result
    };

    #[cfg(unix)]
    let result = {
        use std::os::unix::io::AsRawFd;
        let fd = lock_file.as_raw_fd();
        if unsafe { libc::flock(fd, libc::LOCK_EX) } != 0 {
            return Err("Failed to acquire lock".to_string());
        }

        let result = delete_role_group_inner(&config_path, slug);

        unsafe { libc::flock(fd, libc::LOCK_UN); }
        result
    };

    let _ = lock_file;
    result
}

fn delete_role_group_inner(config_path: &Path, slug: &str) -> Result<(), String> {
    let content = std::fs::read_to_string(config_path)
        .map_err(|e| format!("Failed to read project.json: {}", e))?;
    let mut config: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse project.json: {}", e))?;

    if let Some(groups) = config.get_mut("role_groups").and_then(|g| g.as_array_mut()) {
        let before = groups.len();
        groups.retain(|g| g.get("slug").and_then(|s| s.as_str()) != Some(slug));
        if groups.len() == before {
            return Err(format!("Role group '{}' not found", slug));
        }
    } else {
        return Err("No role_groups in project.json".to_string());
    }

    config["updated_at"] = serde_json::Value::String(iso_now());

    let updated = serde_json::to_string_pretty(&config)
        .map_err(|e| format!("Failed to serialize project.json: {}", e))?;
    atomic_write(config_path, updated.as_bytes())
        .map_err(|e| format!("Failed to write project.json: {}", e))?;

    Ok(())
}

/// Remove a global role template.
pub fn remove_global_role_template(slug: &str) -> Result<(), String> {
    validate_slug(slug)?;

    let templates_dir = global_templates_dir()?;
    let json_path = templates_dir.join(format!("{}.json", slug));
    let md_path = templates_dir.join(format!("{}.md", slug));

    if !json_path.exists() && !md_path.exists() {
        return Err(format!("No global template found for '{}'", slug));
    }

    if json_path.exists() {
        std::fs::remove_file(&json_path)
            .map_err(|e| format!("Failed to remove template: {}", e))?;
    }
    if md_path.exists() {
        let _ = std::fs::remove_file(&md_path);
    }

    Ok(())
}
