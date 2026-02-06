use serde::{Deserialize, Serialize};

// ==================== Session Registry ====================

use std::collections::HashMap;
use std::path::Path;
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
pub struct ProjectConfig {
    pub project_id: String,
    pub name: String,
    pub description: String,
    pub created_at: String,
    pub updated_at: String,
    pub roles: HashMap<String, RoleConfig>,
    pub settings: ProjectSettings,
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionBinding {
    pub role: String,
    pub instance: u32,
    pub session_id: String,
    pub claimed_at: String,
    pub last_heartbeat: String,
    pub status: String,
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

    // 3. Read board.jsonl (may not exist yet) — one JSON per line
    let mut messages: Vec<BoardMessage> = std::fs::read_to_string(vaak_dir.join("board.jsonl"))
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
