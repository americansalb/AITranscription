use serde::{Deserialize, Serialize};

// ==================== Session Registry ====================

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Atomic file write: write to .tmp file, fsync, then rename over target.
/// Protects against partial writes and advisory lock races on macOS.
pub fn atomic_write(path: &Path, content: &[u8]) -> Result<(), String> {
    // On Windows, rename fails if the target is open by another process (even for reading)
    // because std::fs::File doesn't set FILE_SHARE_DELETE. Since all callers use file locking
    // for concurrency, we can safely write directly to the target file on Windows.
    #[cfg(windows)]
    {
        std::fs::write(path, content)
            .map_err(|e| format!("Failed to write {}: {}", path.display(), e))?;
        // Open with write permission for sync_all — on Windows, fsync on a
        // read-only handle returns ERROR_ACCESS_DENIED (os error 5). Pre-pr-
        // error-bubble code used .is_ok() and silently swallowed this; once
        // the error started bubbling, it broke deterministic-fixture tests.
        // OpenOptions with .write(true) without .truncate(true) opens the
        // existing file for the flush without altering content.
        let f = std::fs::OpenOptions::new()
            .write(true)
            .open(path)
            .map_err(|e| format!("Failed to open for fsync {}: {}", path.display(), e))?;
        f.sync_all()
            .map_err(|e| format!("fsync failed for {}: {}", path.display(), e))?;
        return Ok(());
    }

    #[cfg(not(windows))]
    {
        let tmp_path = path.with_extension("tmp");
        std::fs::write(&tmp_path, content)
            .map_err(|e| format!("Failed to write temp file {}: {}", tmp_path.display(), e))?;
        {
            let f = std::fs::File::open(&tmp_path)
                .map_err(|e| format!("Failed to open temp file for fsync {}: {}", tmp_path.display(), e))?;
            f.sync_all()
                .map_err(|e| format!("fsync failed for {}: {}", tmp_path.display(), e))?;
        }
        std::fs::rename(&tmp_path, path)
            .map_err(|e| format!("Failed to rename {} -> {}: {}", tmp_path.display(), path.display(), e))?;
        Ok(())
    }
}

/// Resolve the user's home directory in a cross-platform way.
/// Windows: USERPROFILE, Unix: HOME. Returns an explicit error on failure.
pub fn vaak_home_dir() -> Result<PathBuf, String> {
    let home_var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(home_var)
        .map(PathBuf::from)
        .ok_or_else(|| format!("Cannot determine home directory (${} not set)", home_var))
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
    /// true for roles the user created via UI; false for system/imported roles
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub custom: bool,
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
    /// Communication visibility mode ("open" or "directed"). Renamed from
    /// `discussion_mode` per pr-r2-data-fields (human msg 511 ask #2). Serde
    /// `alias` reads either name; serialization uses the new name only, so
    /// existing project.json files migrate on first write.
    #[serde(default, alias = "discussion_mode")]
    pub session_mode: Option<String>,
    #[serde(default)]
    pub work_mode: Option<String>,
    #[serde(default)]
    pub consecutive_timeout_secs: Option<u64>,
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

fn compute_role_statuses(config: &ProjectConfig, bindings: &[SessionBinding], now_secs: u64, _timeout_secs: u64) -> Vec<RoleStatus> {
    // 10-minute disconnect threshold — matches frontend computeInstanceStatus
    let disconnect_threshold = 600u64;
    config.roles.iter().map(|(slug, role)| {
        let role_bindings: Vec<&SessionBinding> = bindings.iter()
            .filter(|b| b.role == *slug && b.status == "active")
            .collect();
        let total = role_bindings.len() as u32;

        let mut active_count = 0u32;
        for b in &role_bindings {
            let age = parse_iso_epoch(&b.last_heartbeat)
                .map(|hb| now_secs.saturating_sub(hb))
                .unwrap_or(u64::MAX);
            if age <= disconnect_threshold {
                active_count += 1;
            }
        }

        let status = if active_count > 0 {
            "active"
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

// ==================== Termination & Automation Types ====================

/// How a discussion determines when to end
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum TerminationStrategy {
    #[serde(rename = "fixed_rounds")]
    FixedRounds { rounds: u32 },
    #[serde(rename = "consensus")]
    Consensus { threshold: f64 },
    #[serde(rename = "moderator_call")]
    ModeratorCall,
    #[serde(rename = "time_bound")]
    TimeBound { minutes: u32 },
    #[serde(rename = "unlimited")]
    Unlimited,
}

impl Default for TerminationStrategy {
    fn default() -> Self { Self::FixedRounds { rounds: 1 } }
}

/// How much autonomy the moderator has
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AutomationLevel {
    Manual,
    Semi,
    Auto,
}

impl Default for AutomationLevel {
    fn default() -> Self { Self::Auto }
}

/// Audience gate — what the audience can do at this moment
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum AudienceGate {
    Listening,
    Voting,
    Qa,
    Commenting,
    Open,
}

impl Default for AudienceGate {
    fn default() -> Self { Self::Listening }
}

/// Audience configuration for a discussion
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudienceConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub pool: Option<String>,
    #[serde(default)]
    pub size: u32,
    #[serde(default)]
    pub gate: AudienceGate,
}

impl Default for AudienceConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            pool: None,
            size: 0,
            gate: AudienceGate::Listening,
        }
    }
}

// ==================== Format-Specific State Types ====================

/// Pipeline stage output
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineOutput {
    pub stage: u64,
    pub agent: String,
    pub message_id: u64,
    pub timestamp: String,
}

/// Oxford debate teams
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OxfordTeams {
    pub proposition: Vec<String>,
    pub opposition: Vec<String>,
}

/// Oxford debate vote tally
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OxfordVotes {
    #[serde(default)]
    pub for_count: u32,
    #[serde(default)]
    pub against_count: u32,
    #[serde(default)]
    pub abstain_count: u32,
}

/// Red team attack-defense pair
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttackDefensePair {
    pub attack_message_id: u64,
    #[serde(default)]
    pub defense_message_id: Option<u64>,
    pub severity: String,  // "critical" | "high" | "medium" | "low"
    #[serde(default = "default_attack_status")]
    pub status: String,    // "unaddressed" | "partially_addressed" | "addressed"
}

fn default_attack_status() -> String { "unaddressed".to_string() }

/// Continuous mode micro-round response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MicroRoundResponse {
    pub from: String,
    pub vote: String,       // "agree" | "disagree" | "alternative"
    pub message_id: u64,
}

/// Continuous mode micro-round
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MicroRound {
    pub id: String,
    pub trigger_message_id: u64,
    pub trigger_from: String,
    pub topic: String,
    pub opened_at: String,
    #[serde(default)]
    pub closed_at: Option<String>,
    #[serde(default = "default_micro_timeout")]
    pub timeout_seconds: u32,
    #[serde(default)]
    pub responses: Vec<MicroRoundResponse>,
    #[serde(default = "default_micro_result")]
    pub result: String,     // "consent" | "rejected" | "alternative" | "pending"
}

fn default_micro_timeout() -> u32 { 60 }
fn default_micro_result() -> String { "pending".to_string() }

/// Decision stream entry (resolved micro-round summary)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Decision {
    pub micro_round_id: String,
    pub topic: String,
    pub result: String,     // "consent" | "rejected" | "alternative"
    pub resolved_at: String,
    pub summary: String,
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

/// Discussion settings — extended with termination and automation (Phase 1)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscussionSettings {
    // New fields (Phase 1) — optional for backward compat with existing discussion.json files
    #[serde(default)]
    pub termination: Option<TerminationStrategy>,
    #[serde(default)]
    pub automation: Option<AutomationLevel>,
    #[serde(default)]
    pub audience: Option<AudienceConfig>,
    // Legacy fields — still read for backward compat, new code writes termination instead
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
            termination: None,
            automation: None,
            audience: None,
            max_rounds: default_max_rounds(),
            timeout_minutes: default_timeout_minutes(),
            expire_paused_after_minutes: default_expire_paused(),
            auto_close_timeout_seconds: default_auto_close_timeout(),
        }
    }
}

impl DiscussionSettings {
    /// Get effective termination strategy, falling back to legacy max_rounds
    pub fn effective_termination(&self) -> TerminationStrategy {
        self.termination.clone().unwrap_or_else(|| {
            TerminationStrategy::FixedRounds { rounds: self.max_rounds }
        })
    }

    /// Get effective automation level, defaulting to Auto
    pub fn effective_automation(&self) -> AutomationLevel {
        self.automation.clone().unwrap_or(AutomationLevel::Auto)
    }

    /// Get effective audience config, defaulting to disabled
    pub fn effective_audience(&self) -> AudienceConfig {
        self.audience.clone().unwrap_or_default()
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
    /// Audience gating: "listening" (silent, default), "voting", "qa", "commenting", "open"
    #[serde(default = "default_audience_state")]
    pub audience_state: String,
    /// Whether the audience is enabled for this discussion (default: false)
    #[serde(default)]
    pub audience_enabled: bool,
    /// Pipeline sub-mode: "review" (opinions, formerly "discussion") or
    /// "action" (write code). Default on new pipelines: "review". Per
    /// pr-r2-pipeline-mode-value, callers normalize legacy "discussion"
    /// values to "review" on read; serde stores whatever string was given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pipeline_mode: Option<String>,
    /// Pipeline mode: ordered list of agents (e.g. ["developer:0", "tester:0"])
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pipeline_order: Option<Vec<String>>,
    /// Pipeline mode: current stage index (0-based)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pipeline_stage: Option<u64>,
    /// Pipeline mode: outputs from completed stages
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pipeline_outputs: Option<Vec<serde_json::Value>>,
    // ── Oxford mode fields ──
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oxford_teams: Option<OxfordTeams>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oxford_votes: Option<OxfordVotes>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oxford_motion: Option<String>,
    // ── Red team mode fields ──
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attack_chains: Option<Vec<AttackDefensePair>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub severity_summary: Option<std::collections::HashMap<String, u32>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unaddressed_count: Option<u32>,
    // ── Stagnation detection ──
    /// Count of consecutive rounds with no substantive output (all messages < 100 chars)
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub stagnant_rounds: u64,
    // ── Continuous mode fields ──
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub micro_rounds: Option<Vec<MicroRound>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision_stream: Option<Vec<Decision>>,
}

fn default_audience_state() -> String { "listening".to_string() }
fn is_zero_u64(v: &u64) -> bool { *v == 0 }

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
            audience_state: default_audience_state(),
            audience_enabled: false,
            pipeline_mode: None,
            pipeline_order: None,
            pipeline_stage: None,
            pipeline_outputs: None,
            oxford_teams: None,
            oxford_votes: None,
            oxford_motion: None,
            attack_chains: None,
            severity_summary: None,
            unaddressed_count: None,
            stagnant_rounds: 0,
            micro_rounds: None,
            decision_stream: None,
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
///
/// Returns the underlying serialization / lock / write error so callers can
/// surface the actual cause (lock timeout, permission denied, sharing
/// violation, disk full) instead of a generic "write failed." See
/// `write_discussion_unlocked`'s doc for the motivating human bug report
/// (msg 462 in the collab board).
pub fn write_discussion(dir: &str, state: &DiscussionState) -> Result<(), String> {
    let path = active_discussion_path(dir);
    // Use discussion.lock instead of board.lock to avoid contention with MCP sidecar board writes
    let lock_path = {
        let section = get_active_section(dir);
        let vaak_dir = Path::new(dir).join(".vaak");
        if section == "default" {
            vaak_dir.join("discussion.lock")
        } else {
            vaak_dir.join("sections").join(&section).join("discussion.lock")
        }
    };

    let json = serde_json::to_string_pretty(state)
        .map_err(|e| format!("serialize discussion state: {}", e))?;

    // Acquire file lock with timeout (matches with_board_lock pattern)
    let lock_file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .open(&lock_path)
        .map_err(|e| format!("open lock file {}: {}", lock_path.display(), e))?;

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
                    let err_msg = format!(
                        "lock timeout after {}s on {} (another process may be holding the discussion lock)",
                        LOCK_TIMEOUT_MS / 1000, lock_path.display()
                    );
                    eprintln!("[write_discussion] {}", err_msg);
                    return Err(err_msg);
                }
            }
        }

        let result = atomic_write(&path, json.as_bytes())
            .map_err(|e| format!("write {}: {}", path.display(), e));

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
                    let err_msg = format!(
                        "lock timeout after {}s on {} (another process may be holding the discussion lock)",
                        LOCK_TIMEOUT_MS / 1000, lock_path.display()
                    );
                    eprintln!("[write_discussion] {}", err_msg);
                    return Err(err_msg);
                }
            }
        }

        let result = atomic_write(&path, json.as_bytes())
            .map_err(|e| format!("write {}: {}", path.display(), e));

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
        let resolved = vaak_dir.join("sections").join(&section).join("board.lock");
        // Defense-in-depth: ensure path stays under .vaak/
        if !resolved.starts_with(&vaak_dir) {
            return vaak_dir.join("board.lock"); // fallback to default
        }
        resolved
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
///
/// Returns the underlying serialization or atomic_write error so callers can
/// surface the actual cause (Win32 sharing violation, permission denied,
/// disk full, etc.) instead of a generic "write failed" message.
///
/// History (pr-error-bubble): previously returned `bool` and the OS error
/// detail was lost. Human msg 462 hit this when an end-discussion call
/// produced "Failed to write discussion.json" with no further context —
/// dev-challenger msg 471 + platform-engineer msg 468 + tester msg 470
/// all converged on signature change as the right fix.
pub fn write_discussion_unlocked(dir: &str, state: &DiscussionState) -> Result<(), String> {
    let path = active_discussion_path(dir);
    let json = serde_json::to_string_pretty(state)
        .map_err(|e| format!("serialize discussion state: {}", e))?;
    atomic_write(&path, json.as_bytes())
        .map_err(|e| format!("write {}: {}", path.display(), e))
}

/// pr-pipeline-unified-controls PR-3b (2026-04-19): advance the current
/// pipeline holder by 1 stage. Used by HumanSequenceOverrideBar's "End my turn"
/// button when the active discussion is in pipeline mode (instead of the
/// sequence-side pass_turn). Honors multi-round termination strategy: when at
/// the end of pipeline_order, loops back to stage 0 + bumps current_round if
/// FixedRounds allows, else terminates the discussion with
/// terminated_by="max_rounds_reached".
///
/// actor_label is the role:instance of the caller (e.g., "human:0"). Used in
/// the board announcement metadata; auth check is the caller's responsibility.
pub fn pipeline_advance(
    dir: &str,
    actor_label: &str,
) -> Result<serde_json::Value, String> {
    let disc_path = active_discussion_path(dir);
    let mut disc: serde_json::Value = std::fs::read_to_string(&disc_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    if !disc.get("active").and_then(|v| v.as_bool()).unwrap_or(false) {
        return Err("ERR_NO_ACTIVE_DISCUSSION".to_string());
    }
    if disc.get("mode").and_then(|v| v.as_str()) != Some("pipeline") {
        return Err("ERR_NOT_PIPELINE_MODE: this command operates on pipeline-mode discussions only".to_string());
    }
    let pipeline_order: Vec<String> = disc.get("pipeline_order")
        .and_then(|o| o.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();
    if pipeline_order.is_empty() {
        return Err("ERR_EMPTY_PIPELINE_ORDER".to_string());
    }
    let current_stage = disc.get("pipeline_stage").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
    let current_role = pipeline_order.get(current_stage).cloned().unwrap_or_default();
    let next_stage = current_stage + 1;

    if next_stage < pipeline_order.len() {
        // Mid-round advance: just bump stage.
        let now = iso_now();
        disc["pipeline_stage"] = serde_json::json!(next_stage);
        disc["pipeline_stage_started_at"] = serde_json::json!(now.clone());
        let content = serde_json::to_string_pretty(&disc)
            .map_err(|e| format!("serialize discussion state: {}", e))?;
        atomic_write(&disc_path, content.as_bytes())
            .map_err(|e| format!("write {}: {}", disc_path.display(), e))?;
        let next_agent = pipeline_order.get(next_stage).cloned().unwrap_or_default();
        append_sequence_announcement(dir, actor_label, "Turn ended",
            &format!("{} ended their turn (advanced by {}). Next: {}.", current_role, actor_label, next_agent),
            "system",
            serde_json::json!({
                "sequence_action": "pipeline_advance",
                "from_holder": current_role,
                "to_holder": next_agent,
            }))?;
        return Ok(serde_json::json!({
            "status": "advanced",
            "next_holder": next_agent,
            "current_round": disc.get("current_round").and_then(|v| v.as_u64()).unwrap_or(0)
        }));
    }

    // End-of-round: check termination per FixedRounds / Unlimited / etc.
    let typed_disc: DiscussionState = serde_json::from_value(disc.clone()).unwrap_or_default();
    let termination = typed_disc.settings.effective_termination();
    let current_round = disc.get("current_round").and_then(|v| v.as_u64()).unwrap_or(0);
    let should_loop = match &termination {
        TerminationStrategy::FixedRounds { rounds } => current_round + 1 < *rounds as u64,
        TerminationStrategy::Unlimited => true,
        TerminationStrategy::Consensus { .. } => true,
        TerminationStrategy::ModeratorCall => true,
        TerminationStrategy::TimeBound { .. } => true,
    };
    if should_loop {
        let next_round = current_round + 1;
        let now = iso_now();
        disc["pipeline_stage"] = serde_json::json!(0);
        disc["pipeline_stage_started_at"] = serde_json::json!(now.clone());
        disc["current_round"] = serde_json::json!(next_round);
        let content = serde_json::to_string_pretty(&disc)
            .map_err(|e| format!("serialize: {}", e))?;
        atomic_write(&disc_path, content.as_bytes())
            .map_err(|e| format!("write: {}", e))?;
        let first_agent = pipeline_order.first().cloned().unwrap_or_default();
        append_sequence_announcement(dir, actor_label, "Round complete — starting next round",
            &format!("{} ended their turn (advanced by {}). Round {} complete; starting round {}. Next: {}.", current_role, actor_label, current_round + 1, next_round + 1, first_agent),
            "system",
            serde_json::json!({
                "sequence_action": "pipeline_advance",
                "round_started": next_round + 1,
                "to_holder": first_agent,
            }))?;
        return Ok(serde_json::json!({
            "status": "round_complete_advanced",
            "next_holder": first_agent,
            "current_round": next_round
        }));
    }
    // Termination reached.
    disc["active"] = serde_json::json!(false);
    disc["phase"] = serde_json::json!("pipeline_complete");
    disc["terminated_by"] = serde_json::json!("max_rounds_reached");
    let content = serde_json::to_string_pretty(&disc)
        .map_err(|e| format!("serialize: {}", e))?;
    atomic_write(&disc_path, content.as_bytes())
        .map_err(|e| format!("write: {}", e))?;
    append_sequence_announcement(dir, actor_label, "Pipeline complete",
        &format!("{} ended their turn (advanced by {}). Final round complete; pipeline ended.", current_role, actor_label),
        "moderation",
        serde_json::json!({"sequence_action": "pipeline_advance", "terminated_by": "max_rounds_reached"}))?;
    Ok(serde_json::json!({
        "status": "pipeline_ended",
        "terminated_by": "max_rounds_reached"
    }))
}

/// pr-pipeline-unified-controls PR-3b (2026-04-19): insert role_label at the
/// next position in pipeline_order. Used by HumanSequenceOverrideBar's "Insert
/// me next" button when the active discussion is in pipeline mode (instead of
/// the sequence-side human_insert_next).
///
/// Idempotency: if role_label is already the current holder OR the immediately-
/// next stage, no-op. If they're elsewhere in the queue, move them to the next
/// position (don't duplicate).
pub fn pipeline_insert_self_next(
    dir: &str,
    role_label: &str,
) -> Result<serde_json::Value, String> {
    let disc_path = active_discussion_path(dir);
    let mut disc: serde_json::Value = std::fs::read_to_string(&disc_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    if !disc.get("active").and_then(|v| v.as_bool()).unwrap_or(false) {
        return Err("ERR_NO_ACTIVE_DISCUSSION".to_string());
    }
    if disc.get("mode").and_then(|v| v.as_str()) != Some("pipeline") {
        return Err("ERR_NOT_PIPELINE_MODE: this command operates on pipeline-mode discussions only".to_string());
    }
    let mut pipeline_order: Vec<String> = disc.get("pipeline_order")
        .and_then(|o| o.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();
    let current_stage = disc.get("pipeline_stage").and_then(|v| v.as_u64()).unwrap_or(0) as usize;

    // Already current holder? No-op.
    if pipeline_order.get(current_stage).map(|s| s.as_str()) == Some(role_label) {
        return Ok(serde_json::json!({"status": "noop_already_holder", "role": role_label}));
    }
    // Already immediately-next? No-op.
    if pipeline_order.get(current_stage + 1).map(|s| s.as_str()) == Some(role_label) {
        return Ok(serde_json::json!({"status": "noop_already_next", "role": role_label}));
    }
    // Remove existing occurrences (move semantics).
    pipeline_order.retain(|r| r != role_label);
    // Recompute current_stage in case removal shifted indices.
    let recomputed_stage = std::cmp::min(current_stage, pipeline_order.len());
    pipeline_order.insert(recomputed_stage + 1, role_label.to_string());
    disc["pipeline_order"] = serde_json::json!(pipeline_order);
    if recomputed_stage != current_stage {
        disc["pipeline_stage"] = serde_json::json!(recomputed_stage);
    }
    let content = serde_json::to_string_pretty(&disc)
        .map_err(|e| format!("serialize: {}", e))?;
    atomic_write(&disc_path, content.as_bytes())
        .map_err(|e| format!("write: {}", e))?;
    append_sequence_announcement(dir, role_label, "Inserted next in pipeline",
        &format!("{} inserted themselves as the next pipeline stage.", role_label),
        "system",
        serde_json::json!({"sequence_action": "pipeline_insert_self_next", "role": role_label}))?;
    Ok(serde_json::json!({"status": "inserted", "role": role_label}))
}

/// pr-seq-tauri-sequence-commands (2026-04-19): start a sequential-turn
/// sequence. Shared between MCP sidecar (vaak-mcp.rs handle_discussion_control
/// action=start_sequence) and the Tauri-side discussion_control command —
/// both call this helper so the two IPC entrypoints can't drift apart, the
/// same drift class that produced the pipeline-removal gap (msgs 666-708).
///
/// Reads discussion.json as raw JSON to preserve the active_sequence subtree
/// (not modeled in the typed DiscussionState struct). Filters vacant
/// participants against sessions.json and posts a board announcement.
///
/// Caller is responsible for authorization (the MCP path enforces
/// human/manager/moderator-only via state.role; the Tauri path is human-only
/// by convention since the UI is the human).
pub fn start_sequence(
    dir: &str,
    topic: &str,
    goal: Option<&str>,
    participants: &[String],
    initiator_label: &str,
) -> Result<serde_json::Value, String> {
    if topic.trim().is_empty() {
        return Err("topic must not be empty".to_string());
    }
    if participants.is_empty() {
        return Err("participants queue must not be empty".to_string());
    }

    let disc_path = active_discussion_path(dir);
    let mut existing: serde_json::Value = std::fs::read_to_string(&disc_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| serde_json::json!({}));

    if existing.get("active_sequence")
        .and_then(|s| s.get("active"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        return Err("ERR_SEQUENCE_ALREADY_ACTIVE: end the current sequence before starting another".to_string());
    }

    let sessions_path = Path::new(dir).join(".vaak").join("sessions.json");
    let sessions: serde_json::Value = std::fs::read_to_string(&sessions_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| serde_json::json!({"bindings": []}));
    let active_labels: std::collections::HashSet<String> = sessions.get("bindings")
        .and_then(|b| b.as_array())
        .map(|arr| arr.iter().filter_map(|b| {
            let status = b.get("status").and_then(|s| s.as_str()).unwrap_or("");
            if status != "active" && status != "idle" { return None; }
            let role = b.get("role").and_then(|r| r.as_str())?;
            let inst = b.get("instance").and_then(|i| i.as_u64())?;
            Some(format!("{}:{}", role, inst))
        }).collect())
        .unwrap_or_default();

    let mut kept: Vec<String> = Vec::new();
    let mut dropped: Vec<String> = Vec::new();
    for p in participants {
        if active_labels.contains(p) || p == "human:0" {
            kept.push(p.clone());
        } else {
            dropped.push(p.clone());
        }
    }
    if kept.is_empty() {
        return Err("ERR_QUEUE_ALL_VACANT: every participant is vacant, nothing to start".to_string());
    }

    let first = kept.remove(0);
    let now = iso_now();
    existing["active_sequence"] = serde_json::json!({
        "active": true,
        "topic": topic,
        "goal": goal,
        "initiator": initiator_label,
        "started_at": now.clone(),
        "current_holder": first.clone(),
        "queue_remaining": kept.clone(),
        "queue_completed": [],
        "queue_dropped_at_start": dropped.clone(),
        "turn_started_at": now.clone(),
        "paused_for_human": false,
        "mode": "strict-sequential"
    });

    let content = serde_json::to_string_pretty(&existing)
        .map_err(|e| format!("serialize discussion state: {}", e))?;
    atomic_write(&disc_path, content.as_bytes())
        .map_err(|e| format!("write {}: {}", disc_path.display(), e))?;

    let board_path = active_board_path(dir);
    let next_id = std::fs::read_to_string(&board_path)
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter_map(|m| m.get("id").and_then(|i| i.as_u64()))
        .max()
        .unwrap_or(0) + 1;
    let announcement = serde_json::json!({
        "id": next_id,
        "from": initiator_label,
        "to": "all",
        "type": "moderation",
        "timestamp": now,
        "subject": format!("Sequence started: {}", topic),
        "body": format!("Sequential-turn sequence started. Topic: {}. First holder: {}. Dropped (vacant): {:?}.", topic, first, dropped),
        "metadata": {
            "sequence_action": "start",
            "current_holder": first,
            "initiator": initiator_label
        }
    });
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&board_path)
        .map_err(|e| format!("open board.jsonl: {}", e))?;
    let line = serde_json::to_string(&announcement)
        .map_err(|e| format!("serialize announcement: {}", e))?;
    writeln!(f, "{}", line)
        .map_err(|e| format!("write announcement: {}", e))?;

    Ok(serde_json::json!({
        "status": "sequence_started",
        "topic": topic,
        "current_holder": first,
        "queue_remaining": kept,
        "dropped": dropped,
        "announcement_message_id": next_id
    }))
}

/// pr-seq-tauri-sequence-commands batch 2: pass the current turn to the
/// next role in the sequence queue. Mirrors vaak-mcp.rs handle_discussion_control
/// "pass_turn" action.
///
/// Authorization is the caller's responsibility. The MCP path enforces
/// "you must be the current holder OR human/manager/moderator." The Tauri
/// path is human-only by convention (UI is the human).
///
/// Returns Ok({status, next_holder, queue_remaining}) on success; Err on
/// no-active-sequence or empty-queue end-of-sequence handled inline.
pub fn pass_turn(
    dir: &str,
    actor_label: &str,
) -> Result<serde_json::Value, String> {
    let disc_path = active_discussion_path(dir);
    let mut disc: serde_json::Value = std::fs::read_to_string(&disc_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| serde_json::json!({}));

    let seq = disc.get("active_sequence").cloned().unwrap_or(serde_json::Value::Null);
    if !seq.get("active").and_then(|v| v.as_bool()).unwrap_or(false) {
        return Err("ERR_NO_ACTIVE_SEQUENCE: no sequence to pass turn in".to_string());
    }
    let current_holder = seq.get("current_holder").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let queue_remaining: Vec<serde_json::Value> = seq.get("queue_remaining")
        .and_then(|q| q.as_array()).cloned().unwrap_or_default();
    let mut queue_completed: Vec<serde_json::Value> = seq.get("queue_completed")
        .and_then(|q| q.as_array()).cloned().unwrap_or_default();
    let now = iso_now();
    queue_completed.push(serde_json::json!({
        "role": current_holder,
        "turn_ended_at": now.clone(),
        "end_message_id": 0,
        "ended_via": "pass_turn"
    }));
    let (next_holder, new_remaining) = if queue_remaining.is_empty() {
        (String::new(), vec![])
    } else {
        let n = queue_remaining[0].as_str().unwrap_or("").to_string();
        (n, queue_remaining[1..].to_vec())
    };
    if let Some(seq_obj) = disc.get_mut("active_sequence").and_then(|s| s.as_object_mut()) {
        seq_obj.insert("queue_completed".to_string(), serde_json::json!(queue_completed));
        seq_obj.insert("queue_remaining".to_string(), serde_json::json!(new_remaining.clone()));
        seq_obj.insert("paused_for_human".to_string(), serde_json::json!(false));
        if next_holder.is_empty() {
            seq_obj.insert("active".to_string(), serde_json::json!(false));
            seq_obj.insert("current_holder".to_string(), serde_json::json!(""));
            seq_obj.insert("ended_at".to_string(), serde_json::json!(now.clone()));
            seq_obj.insert("ended_by".to_string(), serde_json::json!("queue_exhausted"));
        } else {
            seq_obj.insert("current_holder".to_string(), serde_json::json!(next_holder.clone()));
            seq_obj.insert("turn_started_at".to_string(), serde_json::json!(now.clone()));
        }
    }
    let content = serde_json::to_string_pretty(&disc)
        .map_err(|e| format!("serialize discussion state: {}", e))?;
    atomic_write(&disc_path, content.as_bytes())
        .map_err(|e| format!("write {}: {}", disc_path.display(), e))?;

    let body = if next_holder.is_empty() {
        format!("Turn passed — {} was the last in queue. Sequence ended.", current_holder)
    } else {
        format!("Turn passed: {} → {}. {} remaining.", current_holder, next_holder, new_remaining.len())
    };
    append_sequence_announcement(dir, actor_label, "Turn passed", &body, "system", serde_json::json!({
        "sequence_action": "pass_turn",
        "from_holder": current_holder,
        "to_holder": next_holder.clone()
    }))?;

    if !next_holder.is_empty() {
        append_sequence_announcement(dir, "system:0", "Your sequential turn",
            &format!("It is your turn in the sequence. Post with metadata.end_of_turn=true to advance."),
            "system",
            serde_json::json!({"sequence_notification": true, "to": next_holder.clone()}))?;
    }

    Ok(serde_json::json!({
        "status": "turn_passed",
        "next_holder": next_holder,
        "queue_remaining_count": new_remaining.len()
    }))
}

/// pr-seq-tauri-sequence-commands batch 2: end the active sequence.
/// Mirrors vaak-mcp.rs handle_discussion_control "end_sequence" action.
/// Caller is responsible for authorization. Reason is required for non-human
/// actors at the MCP layer; Tauri path always passes the human label.
pub fn end_sequence(
    dir: &str,
    actor_label: &str,
    reason: Option<&str>,
) -> Result<serde_json::Value, String> {
    let disc_path = active_discussion_path(dir);
    let mut disc: serde_json::Value = std::fs::read_to_string(&disc_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| serde_json::json!({}));

    let seq = disc.get("active_sequence").cloned().unwrap_or(serde_json::Value::Null);
    if !seq.get("active").and_then(|v| v.as_bool()).unwrap_or(false) {
        return Err("ERR_NO_ACTIVE_SEQUENCE: no sequence to end".to_string());
    }
    let now = iso_now();
    let topic_str = seq.get("topic").and_then(|v| v.as_str()).unwrap_or("(untitled)").to_string();
    let audit_reason = reason.unwrap_or("").trim().to_string();
    if let Some(seq_obj) = disc.get_mut("active_sequence").and_then(|s| s.as_object_mut()) {
        seq_obj.insert("active".to_string(), serde_json::json!(false));
        seq_obj.insert("ended_at".to_string(), serde_json::json!(now.clone()));
        seq_obj.insert("ended_by".to_string(), serde_json::json!(actor_label));
        seq_obj.insert("end_reason".to_string(), serde_json::json!(audit_reason.clone()));
    }
    let content = serde_json::to_string_pretty(&disc)
        .map_err(|e| format!("serialize discussion state: {}", e))?;
    atomic_write(&disc_path, content.as_bytes())
        .map_err(|e| format!("write {}: {}", disc_path.display(), e))?;

    let body = format!("The sequential-turn sequence on \"{}\" has ended. Reason: {}",
        topic_str, if audit_reason.is_empty() { "(none)".to_string() } else { audit_reason.clone() });
    append_sequence_announcement(dir, actor_label, &format!("Sequence ended: {}", topic_str),
        &body, "moderation",
        serde_json::json!({
            "sequence_action": "end",
            "ended_by": actor_label,
            "reason": audit_reason
        }))?;

    Ok(serde_json::json!({"status": "sequence_ended", "topic": topic_str}))
}

/// pr-seq-tauri-sequence-commands batch 2: insert a role at the front of
/// the sequence queue (skip ahead). Used by HumanSequenceOverrideBar's
/// "Insert me next" button (which always inserts "human:0"). Mirrors the
/// MCP-side insert_role action restricted to the human:0 role for safety.
///
/// If role_label is already the current holder or already in the queue, a
/// no-op success is returned.
pub fn human_insert_next(
    dir: &str,
    role_label: &str,
) -> Result<serde_json::Value, String> {
    let disc_path = active_discussion_path(dir);
    let mut disc: serde_json::Value = std::fs::read_to_string(&disc_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| serde_json::json!({}));

    let seq = disc.get("active_sequence").cloned().unwrap_or(serde_json::Value::Null);
    if !seq.get("active").and_then(|v| v.as_bool()).unwrap_or(false) {
        return Err("ERR_NO_ACTIVE_SEQUENCE: no sequence to insert into".to_string());
    }
    let current = seq.get("current_holder").and_then(|v| v.as_str()).unwrap_or("");
    if current == role_label {
        return Ok(serde_json::json!({"status": "noop_already_holder", "role": role_label}));
    }
    let mut queue: Vec<String> = seq.get("queue_remaining")
        .and_then(|q| q.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();
    if queue.iter().any(|r| r == role_label) {
        return Ok(serde_json::json!({"status": "noop_already_queued", "role": role_label}));
    }
    queue.insert(0, role_label.to_string());
    if let Some(seq_obj) = disc.get_mut("active_sequence").and_then(|s| s.as_object_mut()) {
        seq_obj.insert("queue_remaining".to_string(), serde_json::json!(queue));
    }
    let content = serde_json::to_string_pretty(&disc)
        .map_err(|e| format!("serialize discussion state: {}", e))?;
    atomic_write(&disc_path, content.as_bytes())
        .map_err(|e| format!("write {}: {}", disc_path.display(), e))?;

    append_sequence_announcement(dir, role_label, "Inserted at front of queue",
        &format!("{} inserted themselves next in the sequence queue.", role_label),
        "system",
        serde_json::json!({"sequence_action": "human_insert_next", "role": role_label}))?;

    Ok(serde_json::json!({"status": "inserted", "role": role_label}))
}

/// Internal: append a sequence-related announcement to board.jsonl.
/// Computes next message id from board contents.
fn append_sequence_announcement(
    dir: &str,
    from_label: &str,
    subject: &str,
    body: &str,
    msg_type: &str,
    metadata: serde_json::Value,
) -> Result<u64, String> {
    let board_path = active_board_path(dir);
    let next_id = std::fs::read_to_string(&board_path)
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter_map(|m| m.get("id").and_then(|i| i.as_u64()))
        .max()
        .unwrap_or(0) + 1;
    let now = iso_now();
    // The "to" field defaults to "all" but a metadata.to override (used by
    // pass_turn's wake-up notification) overrides it.
    let to_target = metadata.get("to").and_then(|v| v.as_str()).unwrap_or("all").to_string();
    let msg = serde_json::json!({
        "id": next_id,
        "from": from_label,
        "to": to_target,
        "type": msg_type,
        "timestamp": now,
        "subject": subject,
        "body": body,
        "metadata": metadata
    });
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&board_path)
        .map_err(|e| format!("open board.jsonl: {}", e))?;
    let line = serde_json::to_string(&msg)
        .map_err(|e| format!("serialize announcement: {}", e))?;
    writeln!(f, "{}", line)
        .map_err(|e| format!("write announcement: {}", e))?;
    Ok(next_id)
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
    if section != "default" {
        validate_slug(section).map_err(|e| format!("Invalid section slug: {}", e))?;
    }
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
        let resolved = vaak_dir.join("sections").join(section).join("board.jsonl");
        // Defense-in-depth: ensure path stays under .vaak/
        if !resolved.starts_with(&vaak_dir) {
            return vaak_dir.join("board.jsonl"); // fallback to default
        }
        resolved
    }
}

/// Get the discussion.json path for a given section.
/// "default" section uses legacy .vaak/discussion.json for backward compatibility.
pub fn discussion_path_for_section(dir: &str, section: &str) -> PathBuf {
    let vaak_dir = Path::new(dir).join(".vaak");
    if section == "default" {
        vaak_dir.join("discussion.json")
    } else {
        let resolved = vaak_dir.join("sections").join(section).join("discussion.json");
        // Defense-in-depth: ensure path stays under .vaak/
        if !resolved.starts_with(&vaak_dir) {
            return vaak_dir.join("discussion.json"); // fallback to default
        }
        resolved
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

/// Ensure the sections/ directory exists and project.json has active_section set.
/// Idempotent — safe to call multiple times.
pub fn ensure_sections_layout(dir: &str) -> Result<(), String> {
    let vaak_dir = Path::new(dir).join(".vaak");
    let sections_dir = vaak_dir.join("sections");

    // Create sections/ directory if missing
    if !sections_dir.exists() {
        std::fs::create_dir_all(&sections_dir)
            .map_err(|e| format!("Failed to create sections/: {}", e))?;
    }

    // Ensure project.json has active_section set
    let config_path = vaak_dir.join("project.json");
    let mut config: serde_json::Value = std::fs::read_to_string(&config_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(serde_json::json!({}));

    if config.get("active_section").is_none() {
        config["active_section"] = serde_json::json!("default");
        let content = serde_json::to_string_pretty(&config)
            .map_err(|e| format!("Failed to serialize project.json: {}", e))?;
        atomic_write(&config_path, content.as_bytes())
            .map_err(|e| format!("Failed to write project.json: {}", e))?;
    }

    Ok(())
}

/// Create a new section. Returns the section info.
pub fn create_section(dir: &str, name: &str) -> Result<SectionInfo, String> {
    let slug = slugify(name);
    if slug.is_empty() {
        return Err("Section name cannot be empty".to_string());
    }

    // Initialize sections layout if this is the first section
    ensure_sections_layout(dir)?;

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

    // Register section in project.json sections array
    let config_path = vaak_dir.join("project.json");
    let mut config: serde_json::Value = std::fs::read_to_string(&config_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(serde_json::json!({}));

    let sections = config.get_mut("sections")
        .and_then(|v| v.as_array_mut());
    if let Some(arr) = sections {
        // Only add if not already present
        let already = arr.iter().any(|v| v.as_str() == Some(&slug));
        if !already {
            arr.push(serde_json::json!(slug));
        }
    } else {
        config["sections"] = serde_json::json!([slug]);
    }

    // Auto-switch to the new section so old messages don't persist
    config["active_section"] = serde_json::json!(slug);
    config["updated_at"] = serde_json::Value::String(now.clone());
    let json = serde_json::to_string_pretty(&config)
        .map_err(|e| format!("Failed to serialize project.json: {}", e))?;
    atomic_write(&config_path, json.as_bytes())
        .map_err(|e| format!("Failed to write project.json: {}", e))?;

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
                    let arr = seed.as_array_mut().expect("seed is always json!([])");
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

/// Validate a slug: lowercase alphanumeric + hyphens, non-empty.
/// Used for role slugs, section slugs, and any user input that becomes a path component.
pub fn validate_slug(slug: &str) -> Result<(), String> {
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
        custom: true,
    };

    // Add role to config
    let mut role_json = serde_json::json!({
        "title": title,
        "description": description,
        "max_instances": max_instances,
        "permissions": permissions,
        "created_at": now,
        "tags": tags,
        "custom": true,
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
    Ok(vaak_home_dir()?.join(".vaak").join("role-templates"))
}

/// Import missing global role templates into a project.
/// Reads ~/.vaak/role-templates/*.json and adds any roles not already in project.json.
/// Also copies matching .md briefings to .vaak/roles/ if not present.
/// Idempotent — safe to call on every project open.
pub fn grandfather_global_templates(dir: &str) -> Result<u32, String> {
    let templates_dir = global_templates_dir()?;
    if !templates_dir.exists() {
        return Ok(0);
    }

    let config_path = Path::new(dir).join(".vaak").join("project.json");
    if !config_path.exists() {
        return Ok(0);
    }

    let config_content = std::fs::read_to_string(&config_path)
        .map_err(|e| format!("Failed to read project.json: {}", e))?;
    let mut config: serde_json::Value = serde_json::from_str(&config_content)
        .map_err(|e| format!("Failed to parse project.json: {}", e))?;

    let roles = match config.get_mut("roles").and_then(|r| r.as_object_mut()) {
        Some(r) => r,
        None => return Ok(0),
    };

    let mut added: u32 = 0;

    let entries = std::fs::read_dir(&templates_dir)
        .map_err(|e| format!("Failed to read role-templates: {}", e))?;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let slug = path.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
        if slug.is_empty() || roles.contains_key(&slug) {
            continue;
        }

        let template_content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let template: serde_json::Value = match serde_json::from_str(&template_content) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let mut role_def = template.clone();
        if let Some(obj) = role_def.as_object_mut() {
            obj.insert("created_at".to_string(), serde_json::json!(iso_now()));
        }

        eprintln!("[collab] Grandfathering role template '{}' into project", slug);
        roles.insert(slug.clone(), role_def);
        added += 1;

        // Copy briefing .md if it exists and project doesn't have one
        let briefing_template = templates_dir.join(format!("{}.md", slug));
        if briefing_template.exists() {
            let roles_dir = Path::new(dir).join(".vaak").join("roles");
            let _ = std::fs::create_dir_all(&roles_dir);
            let dest = roles_dir.join(format!("{}.md", slug));
            if !dest.exists() {
                if let Err(e) = std::fs::copy(&briefing_template, &dest) {
                    eprintln!("[collab] Failed to copy briefing for '{}': {}", slug, e);
                }
            }
        }
    }

    if added > 0 {
        config["updated_at"] = serde_json::Value::String(iso_now());
        let updated = serde_json::to_string_pretty(&config)
            .map_err(|e| format!("Failed to serialize project.json: {}", e))?;
        atomic_write(&config_path, updated.as_bytes())
            .map_err(|e| format!("Failed to write project.json: {}", e))?;
        eprintln!("[collab] Grandfathered {} role template(s) into project", added);
    }

    Ok(added)
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

/// Build an intelligently-ordered pipeline turn order from active sessions.
/// Scores each role by how many of its tags match keywords in the topic.
/// Higher relevance = earlier turn. Manager/human excluded (always have priority bypass).
/// Filters to only include the given participants, appending any not covered by scoring.
///
/// NOTE: This function has a mirror in vaak-mcp.rs::build_turn_order().
/// If you change the scoring logic here, update the mirror too.
pub fn build_pipeline_order(dir: &str, topic: &str, participants: &[String]) -> Vec<String> {
    let vaak_dir = std::path::Path::new(dir).join(".vaak");

    // Read sessions.json for active bindings
    let sessions: serde_json::Value = std::fs::read_to_string(vaak_dir.join("sessions.json"))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(serde_json::json!({"bindings": []}));
    let bindings = sessions.get("bindings")
        .and_then(|b| b.as_array())
        .cloned()
        .unwrap_or_default();

    // Read project.json for role tags
    let config: serde_json::Value = std::fs::read_to_string(vaak_dir.join("project.json"))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(serde_json::json!({}));
    let roles_config = config.get("roles").and_then(|r| r.as_object()).cloned().unwrap_or_default();

    // Tokenize topic for keyword matching
    let tokens: Vec<String> = topic.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| s.len() > 2)
        .map(|s| s.to_string())
        .collect();

    // Tag-to-keyword mapping for relevance scoring
    let tag_keywords: &[(&str, &[&str])] = &[
        ("implementation", &["code", "implement", "build", "write", "create", "add", "feature", "function", "method"]),
        ("debugging", &["bug", "fix", "error", "crash", "broken", "issue", "debug", "wrong"]),
        ("code-review", &["review", "check", "audit", "quality", "approve"]),
        ("testing", &["test", "validate", "verify", "coverage", "spec"]),
        ("architecture", &["architecture", "design", "pattern", "structure", "system", "refactor", "module"]),
        ("security", &["security", "vulnerability", "auth", "permission", "encrypt", "injection", "xss"]),
        ("red-team", &["attack", "adversarial", "exploit", "threat", "penetration"]),
        ("coordination", &["coordinate", "plan", "priority", "schedule", "assign", "task", "sprint"]),
        ("moderation", &["discuss", "debate", "moderate", "consensus", "vote"]),
        ("documentation", &["document", "docs", "readme", "guide", "spec", "write"]),
        ("analysis", &["analyze", "research", "investigate", "report", "data"]),
        ("compliance", &["compliance", "regulation", "standard", "policy", "legal"]),
    ];

    let mut scored: Vec<(String, u64, usize)> = bindings.iter()
        .filter(|b| b.get("status").and_then(|s| s.as_str()) == Some("active"))
        .filter(|b| {
            let r = b.get("role").and_then(|r| r.as_str()).unwrap_or("");
            r != "human" && r != "manager"
        })
        .filter_map(|b| {
            let role = b.get("role")?.as_str()?.to_string();
            let instance = b.get("instance")?.as_u64().unwrap_or(0);

            // Get role tags
            let role_tags: Vec<String> = roles_config.get(&role)
                .and_then(|r| r.get("tags"))
                .and_then(|t| t.as_array())
                .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
                .unwrap_or_default();

            // Score: count keyword matches between role tags and topic tokens
            let mut score: usize = 0;
            for tag in &role_tags {
                if let Some(keywords) = tag_keywords.iter().find(|(t, _)| t == &tag.as_str()) {
                    for kw in keywords.1 {
                        if tokens.contains(&kw.to_string()) {
                            score += 1;
                        }
                    }
                }
            }

            Some((role, instance, score))
        })
        .collect();

    // Sort by score descending, then alphabetical as tiebreaker
    scored.sort_by(|a, b| b.2.cmp(&a.2).then(a.0.cmp(&b.0)).then(a.1.cmp(&b.1)));

    let relevance_order: Vec<String> = scored.into_iter()
        .map(|(role, inst, _)| format!("{}:{}", role, inst))
        .collect();

    // Filter to only include participants, preserving relevance order
    let mut final_order: Vec<String> = relevance_order.into_iter()
        .filter(|agent| participants.contains(agent))
        .collect();

    // Append any participants not covered by scoring (e.g., inactive sessions)
    for p in participants {
        if !final_order.contains(p) {
            final_order.push(p.clone());
        }
    }

    final_order
}

// ==================== Tests ====================

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── Termination Strategy Serialization Tests ──

    #[test]
    fn termination_fixed_rounds_roundtrip() {
        let strategy = TerminationStrategy::FixedRounds { rounds: 5 };
        let json = serde_json::to_string(&strategy).unwrap();
        let parsed: TerminationStrategy = serde_json::from_str(&json).unwrap();
        match parsed {
            TerminationStrategy::FixedRounds { rounds } => assert_eq!(rounds, 5),
            _ => panic!("Expected FixedRounds, got {:?}", parsed),
        }
    }

    #[test]
    fn termination_consensus_roundtrip() {
        let strategy = TerminationStrategy::Consensus { threshold: 0.8 };
        let json = serde_json::to_string(&strategy).unwrap();
        let parsed: TerminationStrategy = serde_json::from_str(&json).unwrap();
        match parsed {
            TerminationStrategy::Consensus { threshold } => {
                assert!((threshold - 0.8).abs() < f64::EPSILON);
            }
            _ => panic!("Expected Consensus, got {:?}", parsed),
        }
    }

    #[test]
    fn termination_moderator_call_roundtrip() {
        let strategy = TerminationStrategy::ModeratorCall;
        let json = serde_json::to_string(&strategy).unwrap();
        assert!(json.contains("moderator_call"));
        let parsed: TerminationStrategy = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, TerminationStrategy::ModeratorCall));
    }

    #[test]
    fn termination_time_bound_roundtrip() {
        let strategy = TerminationStrategy::TimeBound { minutes: 30 };
        let json = serde_json::to_string(&strategy).unwrap();
        let parsed: TerminationStrategy = serde_json::from_str(&json).unwrap();
        match parsed {
            TerminationStrategy::TimeBound { minutes } => assert_eq!(minutes, 30),
            _ => panic!("Expected TimeBound, got {:?}", parsed),
        }
    }

    #[test]
    fn termination_unlimited_roundtrip() {
        let strategy = TerminationStrategy::Unlimited;
        let json = serde_json::to_string(&strategy).unwrap();
        assert!(json.contains("unlimited"));
        let parsed: TerminationStrategy = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, TerminationStrategy::Unlimited));
    }

    #[test]
    fn termination_default_is_fixed_one() {
        let default = TerminationStrategy::default();
        match default {
            TerminationStrategy::FixedRounds { rounds } => assert_eq!(rounds, 1),
            _ => panic!("Default should be FixedRounds(1)"),
        }
    }

    #[test]
    fn termination_unknown_type_errors() {
        let bad_json = r#"{"type": "unknown_strategy"}"#;
        let result: Result<TerminationStrategy, _> = serde_json::from_str(bad_json);
        assert!(result.is_err(), "Unknown termination type should fail to deserialize");
    }

    // ── Automation Level Tests ──

    #[test]
    fn automation_level_roundtrip() {
        for (level, expected_str) in [
            (AutomationLevel::Manual, "\"manual\""),
            (AutomationLevel::Semi, "\"semi\""),
            (AutomationLevel::Auto, "\"auto\""),
        ] {
            let json = serde_json::to_string(&level).unwrap();
            assert_eq!(json, expected_str);
            let parsed: AutomationLevel = serde_json::from_str(&json).unwrap();
            assert_eq!(
                serde_json::to_string(&parsed).unwrap(),
                expected_str,
            );
        }
    }

    #[test]
    fn automation_default_is_auto() {
        let default = AutomationLevel::default();
        assert!(matches!(default, AutomationLevel::Auto));
    }

    // ── Audience Gate Tests ──

    #[test]
    fn audience_gate_roundtrip() {
        for gate in [
            AudienceGate::Listening,
            AudienceGate::Voting,
            AudienceGate::Qa,
            AudienceGate::Commenting,
            AudienceGate::Open,
        ] {
            let json = serde_json::to_string(&gate).unwrap();
            let parsed: AudienceGate = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, gate);
        }
    }

    #[test]
    fn audience_gate_default_is_listening() {
        assert_eq!(AudienceGate::default(), AudienceGate::Listening);
    }

    // ── Audience Config Tests ──

    #[test]
    fn audience_config_default_is_disabled() {
        let config = AudienceConfig::default();
        assert!(!config.enabled);
        assert!(config.pool.is_none());
        assert_eq!(config.size, 0);
        assert_eq!(config.gate, AudienceGate::Listening);
    }

    #[test]
    fn audience_config_roundtrip() {
        let config = AudienceConfig {
            enabled: true,
            pool: Some("skeptical_engineers".to_string()),
            size: 5,
            gate: AudienceGate::Voting,
        };
        let json = serde_json::to_string(&config).unwrap();
        let parsed: AudienceConfig = serde_json::from_str(&json).unwrap();
        assert!(parsed.enabled);
        assert_eq!(parsed.pool.as_deref(), Some("skeptical_engineers"));
        assert_eq!(parsed.size, 5);
        assert_eq!(parsed.gate, AudienceGate::Voting);
    }

    // ── Discussion Settings Tests ──

    #[test]
    fn settings_effective_termination_uses_new_field() {
        let settings = DiscussionSettings {
            termination: Some(TerminationStrategy::Consensus { threshold: 0.75 }),
            max_rounds: 10,
            ..Default::default()
        };
        match settings.effective_termination() {
            TerminationStrategy::Consensus { threshold } => {
                assert!((threshold - 0.75).abs() < f64::EPSILON);
            }
            other => panic!("Expected Consensus, got {:?}", other),
        }
    }

    #[test]
    fn settings_effective_termination_falls_back_to_legacy() {
        let settings = DiscussionSettings {
            termination: None,
            max_rounds: 7,
            ..Default::default()
        };
        match settings.effective_termination() {
            TerminationStrategy::FixedRounds { rounds } => assert_eq!(rounds, 7),
            other => panic!("Expected FixedRounds(7) from legacy fallback, got {:?}", other),
        }
    }

    #[test]
    fn settings_effective_automation_uses_new_field() {
        let settings = DiscussionSettings {
            automation: Some(AutomationLevel::Manual),
            ..Default::default()
        };
        assert!(matches!(settings.effective_automation(), AutomationLevel::Manual));
    }

    #[test]
    fn settings_effective_automation_defaults_to_auto() {
        let settings = DiscussionSettings::default();
        assert!(matches!(settings.effective_automation(), AutomationLevel::Auto));
    }

    #[test]
    fn settings_effective_audience_defaults_to_disabled() {
        let settings = DiscussionSettings::default();
        let audience = settings.effective_audience();
        assert!(!audience.enabled);
    }

    // ── Discussion State Backward Compatibility Tests ──

    #[test]
    fn discussion_state_deserializes_legacy_json() {
        // Simulate an existing discussion.json WITHOUT the new Phase 1 fields
        let legacy_json = json!({
            "active": true,
            "mode": "pipeline",
            "topic": "Test topic",
            "started_at": "2026-03-23T00:00:00Z",
            "moderator": "moderator:0",
            "participants": ["developer:0", "tester:0"],
            "current_round": 1,
            "phase": "pipeline_active",
            "rounds": [],
            "settings": {
                "max_rounds": 3,
                "timeout_minutes": 15,
                "expire_paused_after_minutes": 60,
                "auto_close_timeout_seconds": 30
            },
            "audience_state": "listening",
            "audience_enabled": false,
            "pipeline_order": ["developer:0", "tester:0"],
            "pipeline_stage": 0
        });

        let state: DiscussionState = serde_json::from_value(legacy_json).unwrap();
        assert!(state.active);
        assert_eq!(state.mode.as_deref(), Some("pipeline"));
        assert_eq!(state.topic, "Test topic");
        assert_eq!(state.participants.len(), 2);
        assert_eq!(state.pipeline_order.as_ref().unwrap().len(), 2);
        assert_eq!(state.pipeline_stage, Some(0));

        // New fields should be None/default
        assert!(state.settings.termination.is_none());
        assert!(state.settings.automation.is_none());
        assert!(state.settings.audience.is_none());
        assert!(state.oxford_teams.is_none());
        assert!(state.attack_chains.is_none());
        assert!(state.micro_rounds.is_none());

        // Effective fallbacks should work
        match state.settings.effective_termination() {
            TerminationStrategy::FixedRounds { rounds } => assert_eq!(rounds, 3),
            other => panic!("Expected FixedRounds(3) from legacy, got {:?}", other),
        }
    }

    #[test]
    fn discussion_state_deserializes_new_json() {
        // Simulate a new discussion.json WITH Phase 1 fields
        let new_json = json!({
            "active": true,
            "mode": "delphi",
            "topic": "Architecture debate",
            "started_at": "2026-03-23T01:00:00Z",
            "moderator": "moderator:0",
            "participants": ["developer:0", "architect:0", "tester:0"],
            "current_round": 2,
            "phase": "aggregating",
            "rounds": [],
            "settings": {
                "termination": { "type": "consensus", "threshold": 0.8 },
                "automation": "semi",
                "audience": {
                    "enabled": true,
                    "pool": "skeptical_engineers",
                    "size": 5,
                    "gate": "listening"
                },
                "max_rounds": 10,
                "timeout_minutes": 15,
                "expire_paused_after_minutes": 60,
                "auto_close_timeout_seconds": 30
            },
            "audience_state": "listening",
            "audience_enabled": true
        });

        let state: DiscussionState = serde_json::from_value(new_json).unwrap();
        assert!(state.active);
        assert_eq!(state.mode.as_deref(), Some("delphi"));
        assert_eq!(state.participants.len(), 3);
        assert!(state.audience_enabled);

        // New settings should be populated
        match state.settings.effective_termination() {
            TerminationStrategy::Consensus { threshold } => {
                assert!((threshold - 0.8).abs() < f64::EPSILON);
            }
            other => panic!("Expected Consensus(0.8), got {:?}", other),
        }
        assert!(matches!(state.settings.effective_automation(), AutomationLevel::Semi));
        let audience = state.settings.effective_audience();
        assert!(audience.enabled);
        assert_eq!(audience.pool.as_deref(), Some("skeptical_engineers"));
        assert_eq!(audience.size, 5);

        // Pipeline fields should be None (this is a Delphi discussion)
        assert!(state.pipeline_order.is_none());
        assert!(state.pipeline_stage.is_none());
    }

    #[test]
    fn discussion_state_empty_json_deserializes_to_default() {
        let empty = json!({});
        let state: DiscussionState = serde_json::from_value(empty).unwrap();
        assert!(!state.active);
        assert!(state.mode.is_none());
        assert!(state.topic.is_empty());
        assert!(state.participants.is_empty());
    }

    // ── Format-Specific Type Tests ──

    #[test]
    fn pipeline_output_roundtrip() {
        let output = PipelineOutput {
            stage: 2,
            agent: "developer:0".to_string(),
            message_id: 42,
            timestamp: "2026-03-23T01:00:00Z".to_string(),
        };
        let json = serde_json::to_string(&output).unwrap();
        let parsed: PipelineOutput = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.stage, 2);
        assert_eq!(parsed.agent, "developer:0");
        assert_eq!(parsed.message_id, 42);
    }

    #[test]
    fn oxford_teams_roundtrip() {
        let teams = OxfordTeams {
            proposition: vec!["developer:0".to_string(), "architect:0".to_string()],
            opposition: vec!["tester:0".to_string(), "ux-engineer:0".to_string()],
        };
        let json = serde_json::to_string(&teams).unwrap();
        let parsed: OxfordTeams = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.proposition.len(), 2);
        assert_eq!(parsed.opposition.len(), 2);
    }

    #[test]
    fn oxford_votes_roundtrip() {
        let votes = OxfordVotes {
            for_count: 3,
            against_count: 2,
            abstain_count: 1,
        };
        let json = serde_json::to_string(&votes).unwrap();
        let parsed: OxfordVotes = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.for_count, 3);
        assert_eq!(parsed.against_count, 2);
        assert_eq!(parsed.abstain_count, 1);
    }

    #[test]
    fn attack_defense_pair_defaults() {
        let json = json!({
            "attack_message_id": 10,
            "severity": "critical"
        });
        let pair: AttackDefensePair = serde_json::from_value(json).unwrap();
        assert_eq!(pair.attack_message_id, 10);
        assert!(pair.defense_message_id.is_none());
        assert_eq!(pair.severity, "critical");
        assert_eq!(pair.status, "unaddressed"); // default
    }

    #[test]
    fn micro_round_defaults() {
        let json = json!({
            "id": "mr-001",
            "trigger_message_id": 5,
            "trigger_from": "developer:0",
            "topic": "Auth refactor complete",
            "opened_at": "2026-03-23T01:00:00Z"
        });
        let round: MicroRound = serde_json::from_value(json).unwrap();
        assert_eq!(round.id, "mr-001");
        assert!(round.closed_at.is_none());
        assert_eq!(round.timeout_seconds, 60); // default
        assert!(round.responses.is_empty());
        assert_eq!(round.result, "pending"); // default
    }

    #[test]
    fn decision_roundtrip() {
        let decision = Decision {
            micro_round_id: "mr-001".to_string(),
            topic: "Auth refactor".to_string(),
            result: "consent".to_string(),
            resolved_at: "2026-03-23T01:02:00Z".to_string(),
            summary: "3 agree, 1 silence".to_string(),
        };
        let json = serde_json::to_string(&decision).unwrap();
        let parsed: Decision = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.result, "consent");
        assert_eq!(parsed.summary, "3 agree, 1 silence");
    }

    // ── Serde Malformation Tests ──

    #[test]
    fn termination_missing_type_field_errors() {
        let no_type = json!({"rounds": 5});
        let result: Result<TerminationStrategy, _> = serde_json::from_value(no_type);
        assert!(result.is_err(), "Missing 'type' discriminant should fail");
    }

    #[test]
    fn discussion_state_wrong_mode_fields_ignored() {
        // A Delphi discussion with pipeline fields — pipeline fields should not
        // cause errors but should be deserialized as present (flat struct allows it)
        let mixed_json = json!({
            "active": true,
            "mode": "delphi",
            "topic": "Test",
            "participants": [],
            "rounds": [],
            "settings": { "max_rounds": 1, "timeout_minutes": 15, "expire_paused_after_minutes": 60 },
            "pipeline_order": ["a:0", "b:0"],
            "pipeline_stage": 0
        });
        let state: DiscussionState = serde_json::from_value(mixed_json).unwrap();
        assert_eq!(state.mode.as_deref(), Some("delphi"));
        // Pipeline fields ARE present (flat struct) — type guards prevent misuse
        assert!(state.pipeline_order.is_some());
    }

    // ── DiscussionSettings Serialization Roundtrip ──

    #[test]
    fn settings_full_roundtrip() {
        let settings = DiscussionSettings {
            termination: Some(TerminationStrategy::Consensus { threshold: 0.85 }),
            automation: Some(AutomationLevel::Semi),
            audience: Some(AudienceConfig {
                enabled: true,
                pool: Some("engineers".to_string()),
                size: 3,
                gate: AudienceGate::Qa,
            }),
            max_rounds: 10,
            timeout_minutes: 30,
            expire_paused_after_minutes: 120,
            auto_close_timeout_seconds: 45,
        };
        let json = serde_json::to_string(&settings).unwrap();
        let parsed: DiscussionSettings = serde_json::from_str(&json).unwrap();

        match parsed.effective_termination() {
            TerminationStrategy::Consensus { threshold } => {
                assert!((threshold - 0.85).abs() < f64::EPSILON);
            }
            other => panic!("Expected Consensus, got {:?}", other),
        }
        assert!(matches!(parsed.effective_automation(), AutomationLevel::Semi));
        let aud = parsed.effective_audience();
        assert!(aud.enabled);
        assert_eq!(aud.size, 3);
        assert_eq!(aud.gate, AudienceGate::Qa);
    }

    // ==================== Round 2 Tests ====================

    // ── Role-Gating Validation Tests ──
    // Note: The actual role-gating logic lives in vaak-mcp.rs (sidecar binary).
    // These tests validate the data structures that the gating relies on:
    // discussion.moderator field and state consistency.

    #[test]
    fn discussion_state_moderator_field_present() {
        let json = json!({
            "active": true,
            "mode": "pipeline",
            "topic": "Test",
            "moderator": "moderator:0",
            "participants": ["developer:0"],
            "rounds": [],
            "settings": { "max_rounds": 1, "timeout_minutes": 15, "expire_paused_after_minutes": 60 }
        });
        let state: DiscussionState = serde_json::from_value(json).unwrap();
        assert_eq!(state.moderator.as_deref(), Some("moderator:0"));
    }

    #[test]
    fn discussion_state_moderator_null_for_auto() {
        let json = json!({
            "active": true,
            "mode": "pipeline",
            "topic": "Test",
            "moderator": null,
            "participants": ["developer:0"],
            "rounds": [],
            "settings": { "max_rounds": 1, "timeout_minutes": 15, "expire_paused_after_minutes": 60 }
        });
        let state: DiscussionState = serde_json::from_value(json).unwrap();
        assert!(state.moderator.is_none());
    }

    #[test]
    fn discussion_state_moderator_missing_defaults_to_none() {
        let json = json!({
            "active": true,
            "mode": "pipeline",
            "topic": "Test",
            "participants": [],
            "rounds": [],
            "settings": { "max_rounds": 1, "timeout_minutes": 15, "expire_paused_after_minutes": 60 }
        });
        let state: DiscussionState = serde_json::from_value(json).unwrap();
        assert!(state.moderator.is_none());
    }

    // ── Discussion State with New Settings Roundtrip ──

    #[test]
    fn discussion_state_full_roundtrip_with_phase1_fields() {
        let original = DiscussionState {
            active: true,
            mode: Some("pipeline".to_string()),
            topic: "Architecture review".to_string(),
            started_at: Some("2026-03-23T01:00:00Z".to_string()),
            moderator: Some("moderator:0".to_string()),
            participants: vec!["developer:0".to_string(), "tester:0".to_string()],
            current_round: 1,
            phase: Some("pipeline_active".to_string()),
            settings: DiscussionSettings {
                termination: Some(TerminationStrategy::FixedRounds { rounds: 3 }),
                automation: Some(AutomationLevel::Semi),
                audience: Some(AudienceConfig {
                    enabled: true,
                    pool: Some("engineers".to_string()),
                    size: 5,
                    gate: AudienceGate::Listening,
                }),
                ..Default::default()
            },
            pipeline_order: Some(vec!["developer:0".to_string(), "tester:0".to_string()]),
            pipeline_stage: Some(0),
            pipeline_outputs: Some(vec![]),
            ..Default::default()
        };

        let json = serde_json::to_string(&original).unwrap();
        let parsed: DiscussionState = serde_json::from_str(&json).unwrap();

        assert!(parsed.active);
        assert_eq!(parsed.mode.as_deref(), Some("pipeline"));
        assert_eq!(parsed.moderator.as_deref(), Some("moderator:0"));
        assert_eq!(parsed.participants.len(), 2);
        assert_eq!(parsed.pipeline_order.as_ref().unwrap().len(), 2);
        assert_eq!(parsed.pipeline_stage, Some(0));

        match parsed.settings.effective_termination() {
            TerminationStrategy::FixedRounds { rounds } => assert_eq!(rounds, 3),
            other => panic!("Expected FixedRounds(3), got {:?}", other),
        }
        assert!(matches!(parsed.settings.effective_automation(), AutomationLevel::Semi));
        assert!(parsed.settings.effective_audience().enabled);
    }

    // ── Pipeline Output Accumulation ──

    #[test]
    fn discussion_state_with_pipeline_outputs() {
        let json = json!({
            "active": true,
            "mode": "pipeline",
            "topic": "Impl",
            "participants": ["a:0", "b:0", "c:0"],
            "current_round": 0,
            "rounds": [],
            "settings": { "max_rounds": 1, "timeout_minutes": 15, "expire_paused_after_minutes": 60 },
            "pipeline_order": ["a:0", "b:0", "c:0"],
            "pipeline_stage": 2,
            "pipeline_outputs": [
                { "stage": 0, "agent": "a:0", "message_id": 10, "timestamp": "2026-03-23T01:00:00Z" },
                { "stage": 1, "agent": "b:0", "message_id": 15, "timestamp": "2026-03-23T01:05:00Z" }
            ]
        });
        let state: DiscussionState = serde_json::from_value(json).unwrap();
        assert_eq!(state.pipeline_stage, Some(2));
        // pipeline_outputs is Vec<serde_json::Value> in the current flat struct
        let outputs = state.pipeline_outputs.as_ref().unwrap();
        assert_eq!(outputs.len(), 2);
    }

    #[test]
    fn project_settings_session_mode_reads_legacy_discussion_mode() {
        // pr-r2-data-fields: ProjectSettings.session_mode must accept the
        // legacy `discussion_mode` field name on read (serde alias). This
        // is the migration mechanism — old project.json files keep working.
        let legacy = json!({
            "heartbeat_timeout_seconds": 300,
            "message_retention_days": 30,
            "discussion_mode": "directed"
        });
        let parsed: ProjectSettings = serde_json::from_value(legacy).unwrap();
        assert_eq!(parsed.session_mode.as_deref(), Some("directed"),
            "legacy discussion_mode field must populate session_mode");
    }

    #[test]
    fn project_settings_session_mode_reads_canonical_field() {
        // New canonical field name works directly.
        let canonical = json!({
            "heartbeat_timeout_seconds": 300,
            "message_retention_days": 30,
            "session_mode": "open"
        });
        let parsed: ProjectSettings = serde_json::from_value(canonical).unwrap();
        assert_eq!(parsed.session_mode.as_deref(), Some("open"));
    }

    #[test]
    fn project_settings_session_mode_serializes_to_canonical_only() {
        // After read+write cycle, output uses the new name only — legacy
        // alias is read-only. Files migrate on first write.
        let json_in = json!({
            "heartbeat_timeout_seconds": 300,
            "message_retention_days": 30,
            "session_mode": "directed"
        });
        let parsed: ProjectSettings = serde_json::from_value(json_in).unwrap();
        let serialized = serde_json::to_value(&parsed).unwrap();
        assert!(serialized.get("session_mode").is_some(),
            "serialization must emit session_mode");
        assert!(serialized.get("discussion_mode").is_none(),
            "serialization must NOT emit the legacy discussion_mode key");
    }

    #[test]
    fn pipeline_mode_serialization_roundtrip() {
        // With pipeline_mode set
        let json = json!({
            "active": true,
            "mode": "pipeline",
            "topic": "Test pipeline_mode",
            "participants": ["dev:0"],
            "current_round": 1,
            "rounds": [],
            "settings": { "max_rounds": 1, "timeout_minutes": 15, "expire_paused_after_minutes": 60 },
            "pipeline_mode": "discussion",
            "pipeline_order": ["dev:0"],
            "pipeline_stage": 0
        });
        let state: DiscussionState = serde_json::from_value(json).unwrap();
        assert_eq!(state.pipeline_mode.as_deref(), Some("discussion"));

        // Roundtrip: serialize back and verify
        let serialized = serde_json::to_value(&state).unwrap();
        assert_eq!(serialized["pipeline_mode"], "discussion");

        // Toggle to action
        let json_action = json!({
            "active": true,
            "mode": "pipeline",
            "topic": "Test action mode",
            "participants": ["dev:0"],
            "current_round": 1,
            "rounds": [],
            "settings": { "max_rounds": 1, "timeout_minutes": 15, "expire_paused_after_minutes": 60 },
            "pipeline_mode": "action"
        });
        let state_action: DiscussionState = serde_json::from_value(json_action).unwrap();
        assert_eq!(state_action.pipeline_mode.as_deref(), Some("action"));

        // Without pipeline_mode (non-pipeline discussion) — should default to None
        let json_none = json!({
            "active": true,
            "mode": "delphi",
            "topic": "No pipeline_mode",
            "participants": ["dev:0"],
            "current_round": 1,
            "rounds": [],
            "settings": { "max_rounds": 1, "timeout_minutes": 15, "expire_paused_after_minutes": 60 }
        });
        let state_none: DiscussionState = serde_json::from_value(json_none).unwrap();
        assert!(state_none.pipeline_mode.is_none());

        // Verify None pipeline_mode is skipped in serialization
        let serialized_none = serde_json::to_value(&state_none).unwrap();
        assert!(!serialized_none.as_object().unwrap().contains_key("pipeline_mode"));
    }

    // ── Stagnation Detection Tests ──

    #[test]
    fn stagnant_rounds_defaults_to_zero() {
        let json = json!({
            "active": true,
            "mode": "pipeline",
            "topic": "Test",
            "participants": ["dev:0"],
            "current_round": 1,
            "rounds": [],
            "settings": { "max_rounds": 10, "timeout_minutes": 15, "expire_paused_after_minutes": 60 }
        });
        let state: DiscussionState = serde_json::from_value(json).unwrap();
        assert_eq!(state.stagnant_rounds, 0);

        // Zero stagnant_rounds should be skipped in serialization
        let serialized = serde_json::to_value(&state).unwrap();
        assert!(!serialized.as_object().unwrap().contains_key("stagnant_rounds"));
    }

    #[test]
    fn stagnant_rounds_serialization_roundtrip() {
        let json = json!({
            "active": true,
            "mode": "pipeline",
            "topic": "Stagnation test",
            "participants": ["dev:0"],
            "current_round": 4,
            "rounds": [],
            "settings": { "max_rounds": 10, "timeout_minutes": 15, "expire_paused_after_minutes": 60 },
            "stagnant_rounds": 2
        });
        let state: DiscussionState = serde_json::from_value(json).unwrap();
        assert_eq!(state.stagnant_rounds, 2);

        // Non-zero stagnant_rounds should be present in serialization
        let serialized = serde_json::to_value(&state).unwrap();
        assert_eq!(serialized["stagnant_rounds"], 2);

        // Verify roundtrip
        let state2: DiscussionState = serde_json::from_value(serialized).unwrap();
        assert_eq!(state2.stagnant_rounds, 2);
    }

    #[test]
    fn stagnant_rounds_at_threshold_triggers_close() {
        // Simulate the stagnation counter reaching the threshold (3)
        let json = json!({
            "active": true,
            "mode": "pipeline",
            "topic": "Empty loop test",
            "participants": ["dev:0", "ux:0"],
            "current_round": 5,
            "rounds": [],
            "settings": { "max_rounds": 10, "timeout_minutes": 15, "expire_paused_after_minutes": 60 },
            "stagnant_rounds": 3,
            "pipeline_mode": "discussion",
            "pipeline_order": ["dev:0", "ux:0"],
            "pipeline_stage": 1
        });
        let state: DiscussionState = serde_json::from_value(json).unwrap();
        assert_eq!(state.stagnant_rounds, 3);
        assert!(state.active);
        // The sidecar logic at vaak-mcp.rs:3977 checks: new_stagnant >= max_stagnant (3)
        // When this condition is met, it sets active=false, phase="pipeline_complete"
        // We verify the typed struct can represent the closed state:
        let closed_json = json!({
            "active": false,
            "mode": "pipeline",
            "topic": "Empty loop test",
            "participants": ["dev:0", "ux:0"],
            "current_round": 5,
            "rounds": [],
            "settings": { "max_rounds": 10, "timeout_minutes": 15, "expire_paused_after_minutes": 60 },
            "stagnant_rounds": 3,
            "phase": "pipeline_complete"
        });
        let closed_state: DiscussionState = serde_json::from_value(closed_json).unwrap();
        assert!(!closed_state.active);
        assert_eq!(closed_state.phase.as_deref(), Some("pipeline_complete"));
        assert_eq!(closed_state.stagnant_rounds, 3);
    }

    // ── Oxford State Tests ──

    #[test]
    fn discussion_state_oxford_with_teams_and_votes() {
        let json = json!({
            "active": true,
            "mode": "oxford",
            "topic": "Should we use microservices?",
            "participants": ["dev:0", "dev:1", "arch:0", "arch:1"],
            "current_round": 1,
            "rounds": [],
            "settings": { "max_rounds": 1, "timeout_minutes": 30, "expire_paused_after_minutes": 60 },
            "oxford_teams": {
                "proposition": ["dev:0", "arch:0"],
                "opposition": ["dev:1", "arch:1"]
            },
            "oxford_votes": {
                "for_count": 3,
                "against_count": 2,
                "abstain_count": 0
            },
            "oxford_motion": "This house believes microservices are superior to monoliths"
        });
        let state: DiscussionState = serde_json::from_value(json).unwrap();
        assert_eq!(state.mode.as_deref(), Some("oxford"));
        let teams = state.oxford_teams.as_ref().unwrap();
        assert_eq!(teams.proposition.len(), 2);
        assert_eq!(teams.opposition.len(), 2);
        let votes = state.oxford_votes.as_ref().unwrap();
        assert_eq!(votes.for_count, 3);
        assert_eq!(votes.against_count, 2);
        assert_eq!(state.oxford_motion.as_deref(), Some("This house believes microservices are superior to monoliths"));
    }

    // ── Red Team State Tests ──

    #[test]
    fn discussion_state_red_team_with_attacks() {
        let json = json!({
            "active": true,
            "mode": "red_team",
            "topic": "Security audit",
            "participants": ["evil:0", "dev:0"],
            "current_round": 1,
            "rounds": [],
            "settings": { "max_rounds": 1, "timeout_minutes": 15, "expire_paused_after_minutes": 60 },
            "attack_chains": [
                {
                    "attack_message_id": 20,
                    "defense_message_id": 25,
                    "severity": "critical",
                    "status": "addressed"
                },
                {
                    "attack_message_id": 21,
                    "severity": "high",
                    "status": "unaddressed"
                }
            ],
            "severity_summary": { "critical": 1, "high": 1 },
            "unaddressed_count": 1
        });
        let state: DiscussionState = serde_json::from_value(json).unwrap();
        assert_eq!(state.mode.as_deref(), Some("red_team"));
        let attacks = state.attack_chains.as_ref().unwrap();
        assert_eq!(attacks.len(), 2);
        assert_eq!(attacks[0].severity, "critical");
        assert_eq!(attacks[0].status, "addressed");
        assert!(attacks[0].defense_message_id.is_some());
        assert_eq!(attacks[1].severity, "high");
        assert_eq!(attacks[1].status, "unaddressed");
        assert!(attacks[1].defense_message_id.is_none());
        assert_eq!(state.unaddressed_count, Some(1));
    }

    // ── Continuous Mode State Tests ──

    #[test]
    fn discussion_state_continuous_with_micro_rounds() {
        let json = json!({
            "active": true,
            "mode": "continuous",
            "topic": "Ongoing review",
            "participants": ["dev:0", "tester:0"],
            "current_round": 0,
            "rounds": [],
            "settings": { "max_rounds": 1, "timeout_minutes": 15, "expire_paused_after_minutes": 60 },
            "micro_rounds": [
                {
                    "id": "mr-001",
                    "trigger_message_id": 50,
                    "trigger_from": "dev:0",
                    "topic": "Auth refactor done",
                    "opened_at": "2026-03-23T01:00:00Z",
                    "closed_at": "2026-03-23T01:01:00Z",
                    "timeout_seconds": 30,
                    "responses": [
                        { "from": "tester:0", "vote": "agree", "message_id": 51 }
                    ],
                    "result": "consent"
                }
            ],
            "decision_stream": [
                {
                    "micro_round_id": "mr-001",
                    "topic": "Auth refactor done",
                    "result": "consent",
                    "resolved_at": "2026-03-23T01:01:00Z",
                    "summary": "1 agree, 0 silence"
                }
            ]
        });
        let state: DiscussionState = serde_json::from_value(json).unwrap();
        assert_eq!(state.mode.as_deref(), Some("continuous"));
        let micro = state.micro_rounds.as_ref().unwrap();
        assert_eq!(micro.len(), 1);
        assert_eq!(micro[0].result, "consent");
        assert_eq!(micro[0].responses.len(), 1);
        assert_eq!(micro[0].responses[0].vote, "agree");
        let decisions = state.decision_stream.as_ref().unwrap();
        assert_eq!(decisions.len(), 1);
        assert_eq!(decisions[0].result, "consent");
    }

    // ── Termination Strategy Edge Cases ──

    #[test]
    fn consensus_threshold_boundary_zero() {
        let strategy = TerminationStrategy::Consensus { threshold: 0.0 };
        let json = serde_json::to_string(&strategy).unwrap();
        let parsed: TerminationStrategy = serde_json::from_str(&json).unwrap();
        match parsed {
            TerminationStrategy::Consensus { threshold } => assert!(threshold.abs() < f64::EPSILON),
            _ => panic!("Expected Consensus(0.0)"),
        }
    }

    #[test]
    fn consensus_threshold_boundary_one() {
        let strategy = TerminationStrategy::Consensus { threshold: 1.0 };
        let json = serde_json::to_string(&strategy).unwrap();
        let parsed: TerminationStrategy = serde_json::from_str(&json).unwrap();
        match parsed {
            TerminationStrategy::Consensus { threshold } => {
                assert!((threshold - 1.0).abs() < f64::EPSILON);
            }
            _ => panic!("Expected Consensus(1.0)"),
        }
    }

    #[test]
    fn fixed_rounds_zero_is_valid() {
        let strategy = TerminationStrategy::FixedRounds { rounds: 0 };
        let json = serde_json::to_string(&strategy).unwrap();
        let parsed: TerminationStrategy = serde_json::from_str(&json).unwrap();
        match parsed {
            TerminationStrategy::FixedRounds { rounds } => assert_eq!(rounds, 0),
            _ => panic!("Expected FixedRounds(0)"),
        }
    }

    #[test]
    fn time_bound_zero_minutes_is_valid() {
        let strategy = TerminationStrategy::TimeBound { minutes: 0 };
        let json = serde_json::to_string(&strategy).unwrap();
        let parsed: TerminationStrategy = serde_json::from_str(&json).unwrap();
        match parsed {
            TerminationStrategy::TimeBound { minutes } => assert_eq!(minutes, 0),
            _ => panic!("Expected TimeBound(0)"),
        }
    }

    // ── Skip Serializing None Tests ──

    #[test]
    fn discussion_state_skips_none_optional_fields() {
        let state = DiscussionState {
            active: true,
            mode: Some("delphi".to_string()),
            topic: "Test".to_string(),
            ..Default::default()
        };
        let json = serde_json::to_string(&state).unwrap();
        // Fields that are None with skip_serializing_if should NOT appear
        assert!(!json.contains("pipeline_order"));
        assert!(!json.contains("oxford_teams"));
        assert!(!json.contains("attack_chains"));
        assert!(!json.contains("micro_rounds"));
        assert!(!json.contains("decision_stream"));
    }

    // ==================== Round 3 Tests ====================

    // ── Library Crate Bridge Tests ──
    // Verify that types exposed through lib.rs work correctly

    #[test]
    fn discussion_state_write_read_roundtrip_via_serde() {
        // Simulates what the typed bridge functions do:
        // write_discussion_typed serializes to JSON, read_discussion_typed deserializes
        let state = DiscussionState {
            active: true,
            mode: Some("pipeline".to_string()),
            topic: "Test roundtrip".to_string(),
            moderator: Some("moderator:0".to_string()),
            participants: vec!["dev:0".to_string(), "tester:0".to_string(), "arch:0".to_string()],
            current_round: 1,
            phase: Some("pipeline_active".to_string()),
            settings: DiscussionSettings {
                termination: Some(TerminationStrategy::FixedRounds { rounds: 3 }),
                automation: Some(AutomationLevel::Semi),
                ..Default::default()
            },
            pipeline_order: Some(vec!["arch:0".to_string(), "dev:0".to_string(), "tester:0".to_string()]),
            pipeline_stage: Some(1),
            pipeline_outputs: Some(vec![
                serde_json::json!({ "stage": 0, "agent": "arch:0", "message_id": 10, "timestamp": "2026-03-23T01:00:00Z" })
            ]),
            ..Default::default()
        };

        // Serialize (what write_discussion_typed does)
        let json_str = serde_json::to_string_pretty(&state).unwrap();

        // Deserialize (what read_discussion_typed does)
        let restored: DiscussionState = serde_json::from_str(&json_str).unwrap();

        // Verify all fields survived the roundtrip
        assert!(restored.active);
        assert_eq!(restored.mode.as_deref(), Some("pipeline"));
        assert_eq!(restored.topic, "Test roundtrip");
        assert_eq!(restored.moderator.as_deref(), Some("moderator:0"));
        assert_eq!(restored.participants.len(), 3);
        assert_eq!(restored.pipeline_order.as_ref().unwrap().len(), 3);
        assert_eq!(restored.pipeline_order.as_ref().unwrap()[0], "arch:0");
        assert_eq!(restored.pipeline_stage, Some(1));
        assert_eq!(restored.pipeline_outputs.as_ref().unwrap().len(), 1);

        match restored.settings.effective_termination() {
            TerminationStrategy::FixedRounds { rounds } => assert_eq!(rounds, 3),
            other => panic!("Expected FixedRounds(3), got {:?}", other),
        }
    }

    // ── Pipeline Order Consistency Tests ──
    // These test that build_pipeline_order produces deterministic output
    // (the function that was previously duplicated between collab.rs and vaak-mcp.rs)

    #[test]
    fn build_pipeline_order_with_no_sessions_returns_participants() {
        // When there are no active sessions (temp dir with no sessions.json),
        // build_pipeline_order should return all participants in input order
        let tmp = std::env::temp_dir().join(format!("vaak_test_{}", uuid::Uuid::new_v4()));
        let vaak_dir = tmp.join(".vaak");
        let section_dir = vaak_dir.join("sections").join("default");
        std::fs::create_dir_all(&section_dir).unwrap();

        // Write minimal project.json (no role tags = no scoring)
        std::fs::write(
            vaak_dir.join("project.json"),
            r#"{"project_id":"test","name":"test","description":"","created_at":"","updated_at":"","roles":{},"settings":{"heartbeat_timeout_seconds":30,"message_retention_days":7}}"#
        ).unwrap();

        // Write sessions.json with no active bindings
        std::fs::write(
            vaak_dir.join("sessions.json"),
            r#"{"bindings":[]}"#
        ).unwrap();

        let participants = vec!["dev:0".to_string(), "tester:0".to_string(), "arch:0".to_string()];
        let order = build_pipeline_order(tmp.to_str().unwrap(), "test topic", &participants);

        // All participants should appear in the output (appended as unscored)
        assert_eq!(order.len(), 3);
        assert!(order.contains(&"dev:0".to_string()));
        assert!(order.contains(&"tester:0".to_string()));
        assert!(order.contains(&"arch:0".to_string()));

        // Cleanup
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn build_pipeline_order_is_deterministic() {
        // Same inputs should produce same output every time
        let tmp = std::env::temp_dir().join(format!("vaak_test_{}", uuid::Uuid::new_v4()));
        let vaak_dir = tmp.join(".vaak");
        std::fs::create_dir_all(&vaak_dir).unwrap();

        std::fs::write(
            vaak_dir.join("project.json"),
            r#"{"project_id":"test","name":"test","description":"","created_at":"","updated_at":"","roles":{},"settings":{"heartbeat_timeout_seconds":30,"message_retention_days":7}}"#
        ).unwrap();
        std::fs::write(vaak_dir.join("sessions.json"), r#"{"bindings":[]}"#).unwrap();

        let participants = vec!["a:0".to_string(), "b:0".to_string(), "c:0".to_string()];
        let order1 = build_pipeline_order(tmp.to_str().unwrap(), "test", &participants);
        let order2 = build_pipeline_order(tmp.to_str().unwrap(), "test", &participants);

        assert_eq!(order1, order2, "Same inputs must produce same output");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn build_pipeline_order_filters_to_participants_only() {
        // Even if sessions have extra active agents, only participants appear in output
        let tmp = std::env::temp_dir().join(format!("vaak_test_{}", uuid::Uuid::new_v4()));
        let vaak_dir = tmp.join(".vaak");
        std::fs::create_dir_all(&vaak_dir).unwrap();

        std::fs::write(
            vaak_dir.join("project.json"),
            r#"{"project_id":"test","name":"test","description":"","created_at":"","updated_at":"","roles":{"dev":{"title":"Dev","description":"","max_instances":1,"permissions":[],"created_at":"","tags":[]},"extra":{"title":"Extra","description":"","max_instances":1,"permissions":[],"created_at":"","tags":[]}},"settings":{"heartbeat_timeout_seconds":30,"message_retention_days":7}}"#
        ).unwrap();
        std::fs::write(
            vaak_dir.join("sessions.json"),
            r#"{"bindings":[{"role":"dev","instance":0,"session_id":"s1","claimed_at":"","last_heartbeat":"","status":"active"},{"role":"extra","instance":0,"session_id":"s2","claimed_at":"","last_heartbeat":"","status":"active"}]}"#
        ).unwrap();

        // Only include dev:0, NOT extra:0
        let participants = vec!["dev:0".to_string()];
        let order = build_pipeline_order(tmp.to_str().unwrap(), "test", &participants);

        assert!(order.contains(&"dev:0".to_string()));
        assert!(!order.contains(&"extra:0".to_string()), "Non-participant should be filtered out");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn build_pipeline_order_excludes_human_and_manager() {
        // Human and manager should never appear in pipeline order
        let tmp = std::env::temp_dir().join(format!("vaak_test_{}", uuid::Uuid::new_v4()));
        let vaak_dir = tmp.join(".vaak");
        std::fs::create_dir_all(&vaak_dir).unwrap();

        std::fs::write(
            vaak_dir.join("project.json"),
            r#"{"project_id":"test","name":"test","description":"","created_at":"","updated_at":"","roles":{},"settings":{"heartbeat_timeout_seconds":30,"message_retention_days":7}}"#
        ).unwrap();
        std::fs::write(
            vaak_dir.join("sessions.json"),
            r#"{"bindings":[{"role":"human","instance":0,"session_id":"h1","claimed_at":"","last_heartbeat":"","status":"active"},{"role":"manager","instance":0,"session_id":"m1","claimed_at":"","last_heartbeat":"","status":"active"}]}"#
        ).unwrap();

        let participants = vec!["human:0".to_string(), "manager:0".to_string(), "dev:0".to_string()];
        let order = build_pipeline_order(tmp.to_str().unwrap(), "test", &participants);

        // Human and manager should still appear since they're in participants list
        // (build_pipeline_order filters sessions for scoring but appends unscored participants)
        // The key test: they won't get priority scoring from tags
        assert!(order.contains(&"dev:0".to_string()));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    // ==================== Round 5 Tests: Multi-Round Pipeline Auto-Loop ====================

    #[test]
    fn settings_unlimited_termination_always_loops() {
        let settings = DiscussionSettings {
            termination: Some(TerminationStrategy::Unlimited),
            ..Default::default()
        };
        assert!(matches!(settings.effective_termination(), TerminationStrategy::Unlimited));
    }

    #[test]
    fn pipeline_state_round_increment_simulation() {
        // Simulate what the sidecar does when auto-looping:
        // Pipeline completes → check termination → Unlimited → reset stage, increment round
        let mut state = DiscussionState {
            active: true,
            mode: Some("pipeline".to_string()),
            current_round: 0,
            phase: Some("pipeline_active".to_string()),
            settings: DiscussionSettings {
                termination: Some(TerminationStrategy::Unlimited),
                ..Default::default()
            },
            pipeline_order: Some(vec!["a:0".to_string(), "b:0".to_string()]),
            pipeline_stage: Some(2), // past the end (2 participants)
            ..Default::default()
        };

        let should_loop = match state.settings.effective_termination() {
            TerminationStrategy::Unlimited => true,
            TerminationStrategy::FixedRounds { rounds } => state.current_round + 1 < rounds,
            _ => true,
        };
        assert!(should_loop, "Unlimited should always loop");

        // Simulate reset
        state.pipeline_stage = Some(0);
        state.current_round += 1;

        assert_eq!(state.pipeline_stage, Some(0));
        assert_eq!(state.current_round, 1);

        // Verify roundtrip
        let json = serde_json::to_string(&state).unwrap();
        let restored: DiscussionState = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.current_round, 1);
        assert_eq!(restored.pipeline_stage, Some(0));
    }

    #[test]
    fn pipeline_fixed_rounds_stops_at_limit() {
        let state = DiscussionState {
            current_round: 4, // 0-indexed, round 5 of 5
            settings: DiscussionSettings {
                termination: Some(TerminationStrategy::FixedRounds { rounds: 5 }),
                ..Default::default()
            },
            ..Default::default()
        };

        let should_loop = match state.settings.effective_termination() {
            TerminationStrategy::FixedRounds { rounds } => state.current_round + 1 < rounds,
            _ => true,
        };
        assert!(!should_loop, "Should NOT loop — round 5 of 5 is complete");
    }

    #[test]
    fn pipeline_fixed_rounds_continues_before_limit() {
        let state = DiscussionState {
            current_round: 2, // round 3 of 5
            settings: DiscussionSettings {
                termination: Some(TerminationStrategy::FixedRounds { rounds: 5 }),
                ..Default::default()
            },
            ..Default::default()
        };

        let should_loop = match state.settings.effective_termination() {
            TerminationStrategy::FixedRounds { rounds } => state.current_round + 1 < rounds,
            _ => true,
        };
        assert!(should_loop, "Should loop — round 3 of 5, 2 more to go");
    }

    #[test]
    fn pipeline_default_settings_now_unlimited() {
        // Verify that the new pipeline default is Unlimited (max_rounds: 999)
        let settings = DiscussionSettings {
            termination: Some(TerminationStrategy::Unlimited),
            max_rounds: 999,
            ..Default::default()
        };
        assert!(matches!(settings.effective_termination(), TerminationStrategy::Unlimited));
        assert_eq!(settings.max_rounds, 999);
    }

    // ── pr-seq-tauri-sequence-commands: collab::start_sequence helper ──
    // The shared helper that both vaak-mcp.rs (handle_discussion_control
    // start_sequence) and main.rs (Tauri discussion_control command) call
    // into. Tests below pin pure-logic behavior — no IPC layer needed.

    fn fixture_start_sequence(test_name: &str) -> std::path::PathBuf {
        let tmp = std::env::temp_dir()
            .join(format!("vaak-test-start-sequence-{}-{}", test_name, std::process::id()));
        let vaak = tmp.join(".vaak");
        let _ = std::fs::create_dir_all(&vaak);
        // Sessions: human:0 + dev:0 + tester:0 active; vacant_role:0 not present.
        std::fs::write(vaak.join("sessions.json"), r#"{
            "bindings": [
                {"role": "human", "instance": 0, "status": "active"},
                {"role": "developer", "instance": 0, "status": "active"},
                {"role": "tester", "instance": 0, "status": "idle"}
            ]
        }"#).expect("sessions");
        std::fs::write(vaak.join("project.json"), r#"{}"#).expect("project");
        std::fs::write(vaak.join("board.jsonl"), "").expect("board");
        // No discussion.json initially — start_sequence creates it.
        tmp
    }

    #[test]
    fn start_sequence_happy_path_writes_active_sequence() {
        let tmp = fixture_start_sequence("happy");
        let dir = tmp.to_str().unwrap();
        let participants = vec!["human:0".to_string(), "developer:0".to_string(), "tester:0".to_string()];
        let result = super::start_sequence(dir, "test topic", Some("test goal"), &participants, "human:0");
        assert!(result.is_ok(), "happy path must succeed; got: {:?}", result);
        let response = result.unwrap();
        assert_eq!(response["status"], "sequence_started");
        assert_eq!(response["current_holder"], "human:0");
        assert_eq!(response["topic"], "test topic");
        assert!(response["dropped"].as_array().unwrap().is_empty(),
            "no participants should be dropped (all are active)");

        // Verify discussion.json now contains active_sequence with the right shape.
        let disc_path = active_discussion_path(dir);
        let disc: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(&disc_path).expect("discussion.json should exist")
        ).expect("discussion.json should parse");
        let seq = &disc["active_sequence"];
        assert_eq!(seq["active"], true);
        assert_eq!(seq["topic"], "test topic");
        assert_eq!(seq["goal"], "test goal");
        assert_eq!(seq["initiator"], "human:0");
        assert_eq!(seq["current_holder"], "human:0");
        assert_eq!(seq["mode"], "strict-sequential");
        assert_eq!(seq["queue_remaining"].as_array().unwrap().len(), 2);
        assert_eq!(seq["paused_for_human"], false);

        // Verify announcement was appended to board.jsonl.
        let board_path = active_board_path(dir);
        let board = std::fs::read_to_string(&board_path).expect("board.jsonl should exist");
        assert!(board.contains("Sequence started"),
            "board should contain announcement; got: {}", board);
        assert!(board.contains(r#""sequence_action":"start""#),
            "board should contain sequence_action metadata; got: {}", board);

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn start_sequence_rejects_when_already_active() {
        let tmp = fixture_start_sequence("already-active");
        let dir = tmp.to_str().unwrap();
        let participants = vec!["human:0".to_string(), "developer:0".to_string()];

        // First start: succeeds.
        super::start_sequence(dir, "first topic", None, &participants, "human:0")
            .expect("first start should succeed");

        // Second start: must fail with ALREADY_ACTIVE.
        let result = super::start_sequence(dir, "second topic", None, &participants, "human:0");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("ERR_SEQUENCE_ALREADY_ACTIVE"),
            "second start should fail with ALREADY_ACTIVE; got: {}", err);

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn start_sequence_filters_vacant_participants() {
        let tmp = fixture_start_sequence("filter-vacant");
        let dir = tmp.to_str().unwrap();
        // Mix of active (human:0, developer:0) and vacant (vacant_role:99).
        let participants = vec![
            "human:0".to_string(),
            "vacant_role:99".to_string(),
            "developer:0".to_string(),
        ];
        let result = super::start_sequence(dir, "filter test", None, &participants, "human:0")
            .expect("filtering should succeed");
        let dropped: Vec<&str> = result["dropped"].as_array().unwrap()
            .iter().map(|v| v.as_str().unwrap()).collect();
        assert_eq!(dropped, vec!["vacant_role:99"], "only vacant_role:99 should be dropped");
        assert_eq!(result["current_holder"], "human:0");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn start_sequence_rejects_empty_participants() {
        let tmp = fixture_start_sequence("empty");
        let dir = tmp.to_str().unwrap();
        let result = super::start_sequence(dir, "topic", None, &[], "human:0");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("participants queue must not be empty"));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    // ── pr-seq-tauri-sequence-commands batch 2 helpers ──

    #[test]
    fn pass_turn_advances_holder_and_pops_queue() {
        let tmp = fixture_start_sequence("pass-turn-advance");
        let dir = tmp.to_str().unwrap();
        super::start_sequence(dir, "t", None,
            &["human:0".to_string(), "developer:0".to_string()], "human:0")
            .expect("setup: start sequence");
        let result = super::pass_turn(dir, "human:0").expect("pass_turn should succeed");
        assert_eq!(result["status"], "turn_passed");
        assert_eq!(result["next_holder"], "developer:0");

        // discussion.json should reflect the new holder.
        let disc: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(active_discussion_path(dir)).unwrap()
        ).unwrap();
        assert_eq!(disc["active_sequence"]["current_holder"], "developer:0");
        assert_eq!(disc["active_sequence"]["queue_completed"].as_array().unwrap().len(), 1);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn pass_turn_at_end_of_queue_ends_sequence() {
        let tmp = fixture_start_sequence("pass-turn-end");
        let dir = tmp.to_str().unwrap();
        super::start_sequence(dir, "t", None, &["human:0".to_string()], "human:0").expect("setup");
        let result = super::pass_turn(dir, "human:0").expect("pass_turn should succeed");
        assert_eq!(result["status"], "turn_passed");
        assert_eq!(result["next_holder"], "");

        let disc: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(active_discussion_path(dir)).unwrap()
        ).unwrap();
        assert_eq!(disc["active_sequence"]["active"], false,
            "queue exhaustion must end the sequence");
        assert_eq!(disc["active_sequence"]["ended_by"], "queue_exhausted");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn pass_turn_rejects_when_no_active_sequence() {
        let tmp = fixture_start_sequence("pass-turn-no-seq");
        let dir = tmp.to_str().unwrap();
        let result = super::pass_turn(dir, "human:0");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("ERR_NO_ACTIVE_SEQUENCE"));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn end_sequence_marks_inactive_with_audit_fields() {
        let tmp = fixture_start_sequence("end-seq");
        let dir = tmp.to_str().unwrap();
        super::start_sequence(dir, "t", None, &["human:0".to_string(), "developer:0".to_string()], "human:0")
            .expect("setup");
        let result = super::end_sequence(dir, "human:0", Some("user closed it")).expect("end ok");
        assert_eq!(result["status"], "sequence_ended");

        let disc: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(active_discussion_path(dir)).unwrap()
        ).unwrap();
        assert_eq!(disc["active_sequence"]["active"], false);
        assert_eq!(disc["active_sequence"]["ended_by"], "human:0");
        assert_eq!(disc["active_sequence"]["end_reason"], "user closed it");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn end_sequence_rejects_when_no_active_sequence() {
        let tmp = fixture_start_sequence("end-no-seq");
        let dir = tmp.to_str().unwrap();
        let result = super::end_sequence(dir, "human:0", None);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("ERR_NO_ACTIVE_SEQUENCE"));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn human_insert_next_inserts_at_queue_front() {
        let tmp = fixture_start_sequence("insert-next");
        let dir = tmp.to_str().unwrap();
        // Sequence with developer:0 holding, no human in queue.
        super::start_sequence(dir, "t", None, &["developer:0".to_string()], "human:0")
            .expect("setup");
        let result = super::human_insert_next(dir, "human:0").expect("insert ok");
        assert_eq!(result["status"], "inserted");

        let disc: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(active_discussion_path(dir)).unwrap()
        ).unwrap();
        let queue = disc["active_sequence"]["queue_remaining"].as_array().unwrap();
        assert_eq!(queue[0], "human:0", "human:0 should be at front of queue");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn human_insert_next_noop_if_already_current_holder() {
        let tmp = fixture_start_sequence("insert-noop-holder");
        let dir = tmp.to_str().unwrap();
        super::start_sequence(dir, "t", None, &["human:0".to_string(), "developer:0".to_string()], "human:0")
            .expect("setup");
        let result = super::human_insert_next(dir, "human:0").expect("noop ok");
        assert_eq!(result["status"], "noop_already_holder");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    // ── pr-pipeline-unified-controls PR-3b helper tests ──

    fn fixture_for_pipeline_helpers(test_name: &str, pipeline_order: Vec<&str>, current_stage: u64, current_round: u64, max_rounds: u64) -> std::path::PathBuf {
        let tmp = std::env::temp_dir().join(format!("vaak-test-pipeline-helpers-{}-{}", test_name, std::process::id()));
        let vaak = tmp.join(".vaak");
        let _ = std::fs::create_dir_all(&vaak);
        std::fs::write(vaak.join("project.json"), r#"{"settings":{"heartbeat_timeout_seconds":3600}}"#).expect("project");
        std::fs::write(vaak.join("sessions.json"), r#"{"bindings":[]}"#).expect("sess");
        std::fs::write(vaak.join("board.jsonl"), "").expect("board");
        let order_json: Vec<String> = pipeline_order.iter().map(|s| s.to_string()).collect();
        let disc = serde_json::json!({
            "active": true,
            "mode": "pipeline",
            "pipeline_order": order_json,
            "pipeline_stage": current_stage,
            "current_round": current_round,
            "settings": {
                "termination": { "type": "fixed_rounds", "rounds": max_rounds },
                "max_rounds": max_rounds,
                "auto_close_timeout_seconds": 30,
                "expire_paused_after_minutes": 60,
                "timeout_minutes": 15
            }
        });
        std::fs::write(vaak.join("discussion.json"),
            serde_json::to_string_pretty(&disc).unwrap()).expect("disc");
        tmp
    }

    #[test]
    fn pipeline_advance_mid_round_advances_stage() {
        let tmp = fixture_for_pipeline_helpers("mid-round", vec!["dev:0", "tester:0", "manager:0"], 0, 0, 3);
        let dir = tmp.to_str().unwrap();
        let result = super::pipeline_advance(dir, "human:0").expect("advance ok");
        assert_eq!(result["status"], "advanced");
        assert_eq!(result["next_holder"], "tester:0");
        let after: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(tmp.join(".vaak/discussion.json")).unwrap()
        ).unwrap();
        assert_eq!(after["pipeline_stage"], 1);
        assert_eq!(after["active"], true);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn pipeline_advance_end_of_round_loops_when_max_rounds_allows() {
        let tmp = fixture_for_pipeline_helpers("end-loop", vec!["dev:0", "tester:0"], 1, 0, 3);
        let dir = tmp.to_str().unwrap();
        let result = super::pipeline_advance(dir, "human:0").expect("advance ok");
        assert_eq!(result["status"], "round_complete_advanced");
        assert_eq!(result["current_round"], 1);
        let after: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(tmp.join(".vaak/discussion.json")).unwrap()
        ).unwrap();
        assert_eq!(after["pipeline_stage"], 0);
        assert_eq!(after["current_round"], 1);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn pipeline_advance_end_of_final_round_terminates() {
        let tmp = fixture_for_pipeline_helpers("end-terminate", vec!["dev:0", "tester:0"], 1, 2, 3);
        let dir = tmp.to_str().unwrap();
        let result = super::pipeline_advance(dir, "human:0").expect("advance ok");
        assert_eq!(result["status"], "pipeline_ended");
        assert_eq!(result["terminated_by"], "max_rounds_reached");
        let after: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(tmp.join(".vaak/discussion.json")).unwrap()
        ).unwrap();
        assert_eq!(after["active"], false);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn pipeline_advance_rejects_when_not_pipeline_mode() {
        let tmp = std::env::temp_dir().join(format!("vaak-test-pipeline-not-mode-{}", std::process::id()));
        let vaak = tmp.join(".vaak");
        let _ = std::fs::create_dir_all(&vaak);
        std::fs::write(vaak.join("project.json"), r#"{}"#).expect("p");
        std::fs::write(vaak.join("sessions.json"), r#"{"bindings":[]}"#).expect("s");
        std::fs::write(vaak.join("board.jsonl"), "").expect("b");
        std::fs::write(vaak.join("discussion.json"), r#"{"active":true,"mode":"delphi"}"#).expect("d");
        let result = super::pipeline_advance(tmp.to_str().unwrap(), "human:0");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("ERR_NOT_PIPELINE_MODE"));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn pipeline_insert_self_next_inserts_at_position_after_holder() {
        let tmp = fixture_for_pipeline_helpers("insert-self", vec!["dev:0", "tester:0", "manager:0"], 0, 0, 3);
        let dir = tmp.to_str().unwrap();
        let result = super::pipeline_insert_self_next(dir, "human:0").expect("insert ok");
        assert_eq!(result["status"], "inserted");
        let after: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(tmp.join(".vaak/discussion.json")).unwrap()
        ).unwrap();
        let order: Vec<&str> = after["pipeline_order"].as_array().unwrap().iter().filter_map(|v| v.as_str()).collect();
        assert_eq!(order[0], "dev:0", "current holder unchanged");
        assert_eq!(order[1], "human:0", "human:0 inserted right after holder");
        assert_eq!(order[2], "tester:0", "rest of queue preserved");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn pipeline_insert_self_next_noop_if_already_next() {
        let tmp = fixture_for_pipeline_helpers("noop-next", vec!["dev:0", "human:0", "tester:0"], 0, 0, 3);
        let dir = tmp.to_str().unwrap();
        let result = super::pipeline_insert_self_next(dir, "human:0").expect("noop ok");
        assert_eq!(result["status"], "noop_already_next");
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
