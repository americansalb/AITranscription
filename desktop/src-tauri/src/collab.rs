// ============================================================
// Resilience-stack timer registry (mirror — keep in sync with
// protocol.rs and vaak-mcp.rs)
// ============================================================
// Per evil-arch #923 + dev-chall #917.1, the AL vision intentionally
// keeps timers decentralized at their consumers — only when consumers
// can find each other does decentralization work.
//
//   floor.threshold_ms (per-section, default 60_000)
//                                       — protocol.rs::MIC_GRAB_THRESHOLD_MS
//                                         (mic freshness gate, spec §2)
//   SUPERVISOR_STALL_SECS = 90          — vaak-mcp.rs supervisor loop
//                                         (90s stall before pre-kill buzz)
//   PRE_KILL_GRACE_SECS = 5             — vaak-mcp.rs supervisor loop
//                                         (5s grace before taskkill)
//   KEEP_ALIVE_DEBOUNCE_MS ≈ 10_000     — composer (UI) keystroke heartbeat
//   MIC_AUTOROTATE_SECS = 600           — assembly_line auto-rotation
//                                         (10-min idle = grab, human #903)
//
// Spec: .vaak/al-architecture-diagram.md §2 (single threshold for the
// freshness gate only) + §12 (resilience layers).
// ============================================================

use serde::{Deserialize, Serialize};

// ==================== Session Registry ====================

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

// ==================== Staleness thresholds (single source of truth) ====================
// Per evil-architect:0 msg 5043 F-EA-CA-1 + architect:0 msg 5046 const-extraction
// directive: consolidate all liveness thresholds in one place so a future "tighten
// timing" PR is a single-file change instead of a hunt across collab.rs / main.rs /
// protocol.rs / vaak-mcp.rs. This is the active-claims-v1 first member; subsequent
// thresholds (mic-rotation, claims_auto_release, decision_stale) migrate here as
// each lane lands its own gate cycle.
pub mod staleness_thresholds {
    /// A seat is "alive_state=stale" when its `.vaak/sessions/<role>-<inst>.json:
    /// last_alive_at_ms` is older than this. Mirrors the value already used by
    /// `list_active_seats_cmd` in main.rs:3473 so the moderator picker and the
    /// active-claims panel speak the same language. 120s = 4× the 30s heartbeat
    /// cadence; conservative enough to not false-positive a legitimate long
    /// thinking pause but tight enough that a dead sidecar surfaces within
    /// ~2min on the UI.
    pub const ALIVE_STATE_STALE_MS: u64 = 120_000;
}

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
    /// true for roles the user created via UI; false for system/imported roles
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub custom: bool,
    /// Character/stats system per human msg 3254 + spec at
    /// .vaak/design-notes/character-stats-system-2026-05-16.md.
    /// Phase 1: stats stored at role level (all instances inherit). Each
    /// stat is 1-10. None = legacy role with no stats yet; UI/prompt
    /// generator defaults to 5 across the board until human edits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stats: Option<RoleStats>,
    /// Optional avatar URL (HTTPS only per Phase 1). Fallback to
    /// role-color initial circle when missing or load fails.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar_url: Option<String>,
}

/// Per-role character stats. Each axis 1-10. See human msg 3254 + spec for
/// definitions. Phase 1: data-only — prompt generator + UI consume.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleStats {
    /// Technical Depth — code/architecture/systems engagement
    pub td: u8,
    /// Adversarial Rigor — push-back + verification
    pub ar: u8,
    /// Communication Precision — clarity + conciseness
    pub cp: u8,
    /// Domain Ownership — depth in one area vs spread
    /// (uses `domain` field name to avoid Rust `do` keyword collision)
    #[serde(rename = "do")]
    pub domain: u8,
    /// Process Discipline — verify-before-asserting
    pub pd: u8,
    /// Judgment Under Ambiguity — clean calls under uncertainty
    pub ja: u8,
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
    // ROOT CAUSE of human msg 1506+1533+1536 toggle-shows-On bug per developer:1
    // disk-read: set_currency_enabled was correctly writing project.json, but
    // parse_project_dir → ProjectSettings struct lacked this field → serde silently
    // dropped it → the frontend always saw `settings.currency_enabled === undefined`
    // → `undefined !== false === true` → badge always rendered "On" no matter what
    // the disk said. Adding the field makes parse_project_dir round-trip it.
    #[serde(default)]
    pub currency_enabled: Option<bool>,
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
    /// Active-claims v1 (per architect msg 5044 + ui-arch:1 msg 5048 craft brief):
    /// derived per-claim from `.vaak/sessions/<role>-<inst>.json:last_alive_at_ms`
    /// at read time. "active" | "stale" | "unknown". Optional for back-compat
    /// with frontends that haven't been updated; old callers see the field
    /// as undefined and fall through to the prior "no indicator" behavior.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alive_state: Option<String>,
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

/// Read claims.json and filter out claims whose session is fully gone (no
/// binding at all OR binding is past `gone_threshold` on the legacy
/// heartbeat). For SURVIVING claims, derive an `alive_state` per the
/// keepalive-v1 `last_alive_at_ms` contract (SHA 533b458) so the UI can
/// visually mark "alive but stale" without dropping the card.
///
/// active-claims-v1 (architect msg 5044/5046/5049 + ui-arch msg 5048):
/// - `alive_state = "active"` when last_alive_at_ms within ALIVE_STATE_STALE_MS
/// - `alive_state = "stale"`  when older
/// - `alive_state = "unknown"` when seat session file is missing / unreadable
///   (just-joined seat before first keepalive write, or pre-instrumentation
///   project)
///
/// The legacy "session gone entirely" drop still applies — claims from a
/// session that left the project disappear from the panel; only claims from
/// seats still bound to the project survive with a possible "stale" flag.
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

    let now_ms = now_secs.saturating_mul(1000);
    let sessions_subdir = vaak_dir.join("sessions");

    let mut result = Vec::new();
    let mut any_removed = false;
    let mut clean_map = serde_json::Map::new();

    for (key, val) in &claims_map {
        let session_id = val.get("session_id").and_then(|s| s.as_str()).unwrap_or("");
        // Drop entirely-gone sessions (no binding OR legacy heartbeat past gone-threshold).
        let binding = bindings.iter().find(|b| b.session_id == session_id);
        let is_gone = match binding {
            None => true,
            Some(b) => {
                let age = parse_iso_epoch(&b.last_heartbeat)
                    .map(|hb| now_secs.saturating_sub(hb))
                    .unwrap_or(u64::MAX);
                age > gone_threshold
            }
        };

        if is_gone {
            any_removed = true;
            continue;
        }

        // Surviving claim — derive alive_state from per-seat keepalive file.
        // The role_instance key is "role:N" — split into the filename pattern
        // the keepalive supervisor uses: "role-N.json" in .vaak/sessions/.
        let alive_state: Option<String> = (|| {
            let (role, instance) = key.split_once(':')?;
            let seat_file = sessions_subdir.join(format!("{}-{}.json", role, instance));
            let raw = std::fs::read_to_string(&seat_file).ok()?;
            let parsed: serde_json::Value = serde_json::from_str(&raw).ok()?;
            let last_alive_at_ms = parsed.get("last_alive_at_ms").and_then(|m| m.as_u64()).unwrap_or(0);
            if last_alive_at_ms == 0 {
                return Some("unknown".to_string());
            }
            let stale_ms = now_ms.saturating_sub(last_alive_at_ms);
            if stale_ms > staleness_thresholds::ALIVE_STATE_STALE_MS {
                Some("stale".to_string())
            } else {
                Some("active".to_string())
            }
        })().or_else(|| Some("unknown".to_string()));

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
            alive_state,
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

/// Set the active section in project.json AND write the `.vaak/active-section`
/// marker file (two-controls v1, finding #10 / spec §95). The marker is the
/// single source of truth for the pre-commit hook's "which section binds this
/// commit" resolution. Both writes use atomic tempfile-rename; project.json
/// write happens first because it's the canonical store, marker file is the
/// hook-side mirror.
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

    write_active_section_marker(dir, section)?;
    Ok(())
}

/// Write the `.vaak/active-section` marker file (two-controls v1, finding #10).
/// Used by the pre-commit hook to resolve "which section binds this commit"
/// without reading project.json (which carries other state). Single-line file
/// containing the section slug. Atomic via tempfile-rename — Windows ≥7 stdlib
/// `std::fs::rename` calls MoveFileExW(REPLACE_EXISTING) by default
/// (architect msg 1051 + platform-engineer msg 1049).
pub fn write_active_section_marker(dir: &str, section: &str) -> Result<(), String> {
    let marker_path = Path::new(dir).join(".vaak").join("active-section");
    atomic_write(&marker_path, section.as_bytes())
        .map_err(|e| format!("Failed to write .vaak/active-section marker: {}", e))?;
    Ok(())
}

/// Read the `.vaak/active-section` marker file, returning "default" on absence
/// or read failure (matches pre-commit hook semantics).
pub fn read_active_section_marker(dir: &str) -> String {
    let marker_path = Path::new(dir).join(".vaak").join("active-section");
    std::fs::read_to_string(&marker_path)
        .map(|s| s.trim().to_string())
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "default".to_string())
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

// ==================== Assembly Line state ====================
// Mirrors vaak-mcp.rs's helpers; the two binaries write to the SAME assembly.json
// so the Tauri-side toggle and the MCP-side gate share state at the disk level.

pub fn assembly_path_for_section(dir: &str, section: &str) -> PathBuf {
    if section == "default" {
        Path::new(dir).join(".vaak").join("assembly.json")
    } else {
        Path::new(dir).join(".vaak").join("sections").join(section).join("assembly.json")
    }
}

pub fn active_assembly_path(dir: &str) -> PathBuf {
    assembly_path_for_section(dir, &get_active_section(dir))
}

pub fn read_assembly(dir: &str) -> serde_json::Value {
    std::fs::read_to_string(active_assembly_path(dir))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| serde_json::json!({
            "active": false,
            "current_speaker": null,
            "rotation_order": [],
            "started_at": null,
            "started_by": null
        }))
}

pub fn write_assembly_unlocked(dir: &str, state: &serde_json::Value) -> Result<(), String> {
    let path = active_assembly_path(dir);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let content = serde_json::to_string_pretty(state)
        .map_err(|e| format!("Failed to serialize assembly state: {}", e))?;
    atomic_write(&path, content.as_bytes())
        .map_err(|e| format!("Failed to write assembly.json: {}", e))
}

/// Shared mutation entry point for Assembly Line state. Both the Tauri command
/// (`set_assembly_state`) and the MCP `assembly_line` tool call this — single
/// source of truth for the enable/disable semantics. Returns the new state
/// after writing it to disk under the cross-process board.lock acquire.
pub fn set_assembly_v0(dir: &str, action: &str, actor: &str) -> Result<serde_json::Value, String> {
    with_board_lock(dir, || {
        let new_state = match action {
            "enable" => {
                // V3 spec rule 10: assembly mode and discussion modes are mutually
                // exclusive — Delphi blind submissions and continuous-review auto-
                // triggers both violate the single-speaker contract. Closing the
                // door at enable is cheaper than negotiating precedence at runtime.
                let disc_path = Path::new(dir).join(".vaak").join("discussion.json");
                let disc_active = std::fs::read_to_string(&disc_path)
                    .ok()
                    .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
                    .and_then(|v| v.get("active").and_then(|a| a.as_bool()))
                    .unwrap_or(false);
                if disc_active {
                    return Err("Cannot enable Assembly Line while a discussion is active. \
                        End the discussion first via discussion_control(end_discussion).".to_string());
                }
                let order = active_assembly_seats(dir);
                if order.is_empty() {
                    return Err("Cannot enable Assembly Line: no active seats with fresh heartbeats. \
                        (Bindings older than 90s are excluded as zombies — V3 rule 5. \
                        If you expect a seat to be eligible, have it call project_join again.)".to_string());
                }
                let first = order[0].clone();
                serde_json::json!({
                    "active": true,
                    "current_speaker": first,
                    "rotation_order": order,
                    "started_at": iso_now(),
                    "started_by": actor
                })
            }
            "disable" => {
                serde_json::json!({
                    "active": false,
                    "current_speaker": null,
                    "rotation_order": [],
                    "started_at": null,
                    "started_by": null
                })
            }
            other => return Err(format!("Unknown assembly action: '{}'. Valid: enable, disable", other)),
        };
        write_assembly_unlocked(dir, &new_state)?;
        Ok(new_state)
    })
}

/// Heartbeat freshness threshold for assembly seat eligibility, in seconds.
/// Spec V3 rule 5: bindings whose last_heartbeat is older than this are treated
/// as zombies and excluded from rotation_order at seed time. Mirrors the same
/// constant in vaak-mcp.rs (sidecar mid-rotation skip uses its own copy).
const ASSEMBLY_SEAT_FRESHNESS_SECS: u64 = 90;

/// List active+idle session seats as "role:instance" in the order they appear in sessions.json.
/// Filters bindings with stale heartbeats (>ASSEMBLY_SEAT_FRESHNESS_SECS) so a
/// dead binding doesn't end up holding the mic at seed — V3 spec rule 5.
pub fn active_assembly_seats(dir: &str) -> Vec<String> {
    let sessions_path = Path::new(dir).join(".vaak").join("sessions.json");
    let json: serde_json::Value = std::fs::read_to_string(&sessions_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(serde_json::Value::Null);
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    json.get("bindings")
        .and_then(|b| b.as_array())
        .map(|bindings| {
            bindings.iter()
                .filter(|b| {
                    let st = b.get("status").and_then(|s| s.as_str()).unwrap_or("");
                    st == "active" || st == "idle"
                })
                .filter(|b| {
                    let hb = b.get("last_heartbeat").and_then(|v| v.as_str()).unwrap_or("");
                    match parse_iso_epoch(hb) {
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

    // 3. Prune protocol.json (per human msg 2299 + a091870 fix extended to
    // roster_remove_slot): rotation_order / current_speaker / moderator /
    // hand_queue references to the removed seat get cleared so the mic
    // doesn't stall on a zombie. Same shape as handle_project_leave's
    // protocol prune. Applied to ALL section protocol.json files since
    // the seat may appear in multiple sections.
    let target_label = format!("{}:{}", role, instance);
    let sections_dir = vaak_dir.join("sections");
    let mut proto_paths: Vec<std::path::PathBuf> = vec![vaak_dir.join("protocol.json")];
    if let Ok(entries) = std::fs::read_dir(&sections_dir) {
        for entry in entries.flatten() {
            let p = entry.path().join("protocol.json");
            if p.exists() {
                proto_paths.push(p);
            }
        }
    }
    for proto_path in proto_paths {
        let Ok(content) = std::fs::read_to_string(&proto_path) else { continue };
        let Ok(mut proto) = serde_json::from_str::<serde_json::Value>(&content) else { continue };
        let mut changed = false;
        if let Some(floor) = proto.get_mut("floor").and_then(|f| f.as_object_mut()) {
            if let Some(arr) = floor.get_mut("rotation_order").and_then(|v| v.as_array_mut()) {
                let before = arr.len();
                arr.retain(|v| v.as_str() != Some(&target_label));
                if arr.len() != before { changed = true; }
            }
            if floor.get("current_speaker").and_then(|v| v.as_str()) == Some(&target_label) {
                floor.insert("current_speaker".to_string(), serde_json::Value::Null);
                changed = true;
            }
            if floor.get("moderator").and_then(|v| v.as_str()) == Some(&target_label) {
                floor.insert("moderator".to_string(), serde_json::Value::Null);
                changed = true;
            }
            if let Some(hq) = floor.get_mut("hand_queue").and_then(|v| v.as_array_mut()) {
                let before = hq.len();
                hq.retain(|v| v.as_str() != Some(&target_label));
                if hq.len() != before { changed = true; }
            }
        }
        if changed {
            let cur_rev = proto.get("rev").and_then(|v| v.as_u64()).unwrap_or(0);
            if let Some(rev_field) = proto.get_mut("rev") {
                *rev_field = serde_json::json!(cur_rev + 1);
            }
            if let Some(obj) = proto.as_object_mut() {
                obj.insert("last_writer_action".to_string(), serde_json::json!("roster_remove_slot_prune"));
                obj.insert("rev_at".to_string(), serde_json::json!(iso_now()));
            }
            if let Ok(updated) = serde_json::to_string_pretty(&proto) {
                let _ = atomic_write(&proto_path, updated.as_bytes());
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
    stats: Option<RoleStats>,
    avatar_url: Option<String>,
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

        let result = create_role_inner(&config_path, &vaak_dir, slug, title, description, &permissions, max_instances, briefing, &tags, &companions, stats.as_ref(), avatar_url.as_deref());

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

        let result = create_role_inner(&config_path, &vaak_dir, slug, title, description, &permissions, max_instances, briefing, &tags, &companions, stats.as_ref(), avatar_url.as_deref());

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
    stats: Option<&RoleStats>,
    avatar_url: Option<&str>,
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
        stats: stats.cloned(),
        avatar_url: avatar_url.map(|s| s.to_string()),
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
    if let Some(s) = stats {
        role_json["stats"] = serde_json::to_value(s)
            .map_err(|e| format!("Failed to serialize stats: {}", e))?;
    }
    if let Some(url) = avatar_url {
        if !url.is_empty() {
            role_json["avatar_url"] = serde_json::Value::String(url.to_string());
        }
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
    stats: Option<RoleStats>,
    avatar_url: Option<String>,
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

        let result = update_role_inner(&config_path, &vaak_dir, slug, title, description, permissions.as_deref(), max_instances, briefing, tags.as_deref(), companions.as_deref(), stats.as_ref(), avatar_url.as_deref());

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

        let result = update_role_inner(&config_path, &vaak_dir, slug, title, description, permissions.as_deref(), max_instances, briefing, tags.as_deref(), companions.as_deref(), stats.as_ref(), avatar_url.as_deref());

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
    stats: Option<&RoleStats>,
    avatar_url: Option<&str>,
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
        if let Some(s) = stats {
            role["stats"] = serde_json::to_value(s)
                .map_err(|e| format!("Failed to serialize stats: {}", e))?;
        }
        if let Some(url) = avatar_url {
            if url.is_empty() {
                role.as_object_mut().map(|o| o.remove("avatar_url"));
            } else {
                role["avatar_url"] = serde_json::Value::String(url.to_string());
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

// ==================== Decision Panel v1 ====================
//
// Per the 6 adversarial flags (board msgs 4784/4787/4789/4811), pending
// human-decisions get a dedicated UI surface so they stop getting buried in
// board-scroll noise. The wire format reuses the existing project_send +
// metadata.choices schema agents already produce — no MCP changes required.
//
// Pose:    project_send(to="human", type="question",
//                       metadata={ choices:[{id,label,desc,recommended?}],
//                                  allow_other?: bool,
//                                  question_hash?: string })
// Resolve: human picks an option in the panel → resolve_decision_cmd writes a
//          type:"answer" board message (existing pattern) AND appends a
//          resolution entry to decisions.jsonl for fast cross-session lookup.
// Other:   human types free-form → ALSO emits a type:"directive" board
//          message with metadata.in_reply_to:<decision_id> so the team picks
//          it up on rotation (flag #3).
// Cancel:  human dismisses → cancel-resolution entry; the question stays in
//          board history but disappears from the pending panel.
// Stale:   resolutions also auto-include a synthesized "stale_archive" entry
//          when the original pose is >24h old, server-side at read time
//          (flag #4 — no background job needed for v1).

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionResolution {
    pub decision_id: u64,
    /// "resolve" | "cancel"
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub option_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub other_text: Option<String>,
    /// For cancel: "author_cancel" | "stale_archive" | "board_resolved"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub at: String,
    pub by: String,
}

pub fn decisions_jsonl_path_for_section(dir: &str, section: &str) -> PathBuf {
    let vaak_dir = Path::new(dir).join(".vaak");
    if section == "default" {
        vaak_dir.join("decisions.jsonl")
    } else {
        vaak_dir.join("sections").join(section).join("decisions.jsonl")
    }
}

pub fn active_decisions_path(dir: &str) -> PathBuf {
    decisions_jsonl_path_for_section(dir, &get_active_section(dir))
}

/// Read the resolution log. Last-write-wins per decision_id (a cancel after
/// a resolve takes precedence — the human changed their mind).
pub fn read_decision_resolutions(dir: &str) -> HashMap<u64, DecisionResolution> {
    let path = active_decisions_path(dir);
    let mut map: HashMap<u64, DecisionResolution> = HashMap::new();
    let content = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(_) => return map,
    };
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() { continue; }
        if let Ok(r) = serde_json::from_str::<DecisionResolution>(line) {
            map.insert(r.decision_id, r);
        }
    }
    map
}

pub fn append_decision_resolution(dir: &str, r: &DecisionResolution) -> Result<(), String> {
    let path = active_decisions_path(dir);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let line = serde_json::to_string(r)
        .map_err(|e| format!("Failed to serialize resolution: {}", e))?;
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| format!("Failed to open decisions.jsonl: {}", e))?;
    writeln!(f, "{}", line)
        .map_err(|e| format!("Failed to write resolution: {}", e))?;
    Ok(())
}

// ============================================================
// Currency Ledger — Phase 1 (commit a)
// ============================================================
// Per architect:0 msg 1135 + plan `.vaak/design-notes/2026-05-23-currency-
// ledger-phase1-{spec,plan}.md`. Project-wide ledger (.vaak/currency.jsonl
// append-only) + snapshot (.vaak/balances.json via atomic_write).
//
// Lock semantics (ruling 9-corrected per dev-challenger:0 msg 1123 +
// developer:0 msg 1129):
//   Sole entry point for touching both ledger and board is
//   `with_currency_and_board_lock(dir, F)`. Outer = `.vaak/currency.lock`
//   (section-independent, project-wide). Inner = section-scoped board.lock
//   via `with_board_lock`. Closure-nest auto-LIFO release. Manual
//   composition of with_currency_lock + with_board_lock is a deadlock-by-
//   reverse-order risk; ALWAYS use the combined helper.
//
// The sidecar binary (bin/vaak-mcp.rs) defines its own
// `with_currency_and_board_lock` that nests vaak-mcp.rs::with_file_lock
// (section-scoped) inside this same outer currency lock. Cross-binary
// parity: same path `.vaak/currency.lock`, same ordering rule.
// ============================================================

pub mod currency {
    use super::atomic_write;
    use serde::{Deserialize, Serialize};
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};

    // ---- Constants (spec §"Constants") ----
    // Per human msg 647: 5 silver (500c) starting balance for new roles.
    // Was 10_000 (1 gold) — lowered to make first-time-seen seats earn their
    // way up instead of starting rich. Existing balances unaffected; only
    // first-time-seen seats lazy-init at this value (and Phase 7 carry-over
    // governs returning seats per snapshot). This will move into economy.json
    // per human msg 657 directive as soon as that infra ships.
    pub const STARTING_BALANCE_COPPER: i64 = 500;
    pub const DEFICIT_CAP_COPPER: i64 = -1_000;
    pub const PASS_EARN_COPPER: i64 = 1;
    pub const SPEAK_EARN_COPPER: i64 = 10;
    // Phase 4 (a): Edit + Test earns. Phase 8 (human msg 2262): an auto-DETECTED
    // edit (real file writes via the file-op-claim.py PostToolUse marker) is the
    // economy's "work pays more than talk" lever. Base 25 + 1 copper per line
    // beyond EDIT_LINE_BONUS_THRESHOLD (matches the human's "+75 edit (150 lines)"
    // example: 25 + max(0, 150-100) = 75). Edit + Test escrow mature over their
    // own longer windows per human msg 18 (2026-05-24): pass=10, speak=20,
    // edit=50, test=50 — heavier work, heavier hold.
    pub const EDIT_EARN_COPPER: i64 = 25;
    // Plan v2 P1-2b (architect ruling Option A msg 469, human msg 543
    // observation, dev:1 msg 547 ship). Raised 10 → 20 to flatten the
    // Test/Speak dominance asymmetry that produced ZERO Test rows in a
    // 16-hour live session. EV of testing an Edit was +10 (earn) − 15
    // (co-liability) = structurally negative. Now +20 − 15 = +5, closing
    // half the gap. Future v3 (Phase 9/10 candidate per architect msg
    // 545) may further scale Test earn proportionally to edit_lines for
    // large-Edit risk premium; v2 keeps the flat raise pending live data.
    pub const TEST_EARN_COPPER: i64 = 20;
    pub const EDIT_LINE_BONUS_THRESHOLD: u64 = 100; // +1 copper/line beyond this
    pub const PASS_ESCROW_TICKS: u64 = 10;
    pub const SPEAK_ESCROW_TICKS: u64 = 20;
    pub const EDIT_ESCROW_TICKS: u64 = 50;
    pub const TEST_ESCROW_TICKS: u64 = 50;
    pub const PASSIVE_PER_TICK_COPPER: i64 = 1;
    pub const INTEREST_MIN_HELD_COPPER: i64 = 10;
    // Commit E (2026-05-24): zeroed to kill the interest-stacking exploit
    // (evil-arch msg 172 + dev:1 msg 175 + architect ruling msg 180). Positive
    // interest on penalty escrow inverted the SPEAK incentive (-10 hold, +20
    // interest over 20 ticks = net +20 cu pure profit; stacked across concurrent
    // escrows). Phase-5 redesign may re-introduce a positive incentive on a
    // coherent vehicle (e.g., bounty stake), NOT on penalty escrow.
    pub const INTEREST_PER_10_COPPER_HELD: i64 = 0;
    pub const PASS_BODY_LEN_THRESHOLD: usize = 100;

    // ---- Phase 2 (Disputes) constants (spec §Constants) ----
    pub const OBJECTION_COST_COPPER: i64 = 50;
    pub const DISPUTE_SPEECH_COST_COPPER: i64 = 5;
    pub const DISPUTE_EDIT_COST_COPPER: i64 = 10;
    pub const JUDGE_COST_PER_PARTY: i64 = 50;          // 50 each → 100 to pool
    pub const JUDGE_AUTO_INVOKE_THRESHOLD: i64 = 500;
    pub const SYSTEM_DISPUTE_COST: i64 = 50;
    pub const SYSTEM_DISPUTE_REWARD: i64 = 200;        // correct ruling
    pub const SYSTEM_DISPUTE_PENALTY: i64 = 250;       // incorrect — total debit
    pub const SYSTEM_DISPUTE_BAN_TURNS: u64 = 10;
    pub const CLAWBACK_PERCENT: u64 = 90;              // when escrow already released

    // ---- Phase 4 (b) Retroactive Pass-penalty constants ----
    // Per spec v4 (`.vaak/design-notes/2026-05-23-currency-phase4-spec.md`).
    // Small per-row sting (-3 copper) × 12-turn scan window = max -36 copper
    // for a fully-passive seat, enough to disincentivize rubber-stamp Pass
    // spam without nuking a normally-active seat.
    pub const RETRO_PASS_PENALTY_COPPER: i64 = 3;
    pub const RETRO_PASS_SCAN_WINDOW_TURNS: u64 = 12;
    // Phase 4 (c): co-liability — a tester who certified an Edit that the team
    // later ruled bad shares the blame. 15 copper per tester (per-seat, not
    // per-test-row — Q2 dedupe). Steeper than the retro-Pass sting: a bad
    // certification is a stronger signal than a lazy pass.
    pub const COLIABILITY_TEST_PENALTY_COPPER: i64 = 15;
    // Phase 6: bounty economy.
    pub const BOUNTY_CLAIM_STAKE_PERCENT: u64 = 10;        // 10% of bounty held as claim stake
    pub const BOUNTY_ABANDON_LOSS_PERCENT: u64 = 50;       // abandon → lose half the stake
    pub const BOUNTY_REJECT_LOSS_PERCENT: u64 = 100;       // reject → lose full stake
    pub const BOUNTY_OBJECTION_CLAWBACK_PERCENT: u64 = 90; // successful objection on approved bounty

    // ---- Decay tax (human msg 458, 2026-05-24) ----
    // Per-turn wealth tax to create a structural sink against inflation. Human
    // picked evil-arch msg 428's spec: 1% labeled "copper" + 0.5% labeled "silver"
    // per turn = 1.5% of balance per turn destroyed. Emits TWO ledger rows per
    // seat per turn (one copper-labeled, one silver-labeled) for Flow Feed
    // readability. Excludes escrow (in-flight work protected). Floor at
    // DECAY_FLOOR_COPPER to prevent decay-driven timeouts.
    pub const DECAY_COPPER_PCT_PER_TURN_TENTHS: i64 = 10;  // 10 tenths = 1.0%
    pub const DECAY_SILVER_PCT_PER_TURN_TENTHS: i64 = 5;   //  5 tenths = 0.5%
    pub const DECAY_FLOOR_COPPER: i64 = 100;               // balance below this is not taxed

    // ===========================================================================
    // Economy settings — runtime-tunable constants per human msg 657.
    // ===========================================================================
    // The constants above are now DEFAULTS. Live values are read from
    // .vaak/economy.json on every read_economy_settings() call (no startup
    // cache, so UI-driven edits land next-turn without rebuild — per tester:0
    // msg 663 #2 cache-coherence requirement). Missing file → all defaults.
    // Missing field → that field's default. Parse error → all defaults
    // (best-effort posture; logs warn).
    //
    // Implementation pattern: every code site that previously read a constant
    // directly will instead call `read_economy_settings(dir).field`. Constant
    // names preserved as static initializers + as the Default impl.

    pub fn economy_json_path(dir: &str) -> PathBuf {
        Path::new(dir).join(".vaak").join("economy.json")
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct EconomySettings {
        // Base
        #[serde(default = "default_starting_balance_copper")]
        pub starting_balance_copper: i64,
        #[serde(default = "default_deficit_cap_copper")]
        pub deficit_cap_copper: i64,
        #[serde(default = "default_passive_per_tick_copper")]
        pub passive_per_tick_copper: i64,

        // Earn tier
        #[serde(default = "default_pass_earn_copper")]
        pub pass_earn_copper: i64,
        #[serde(default = "default_speak_earn_copper")]
        pub speak_earn_copper: i64,
        #[serde(default = "default_edit_earn_copper")]
        pub edit_earn_copper: i64,
        #[serde(default = "default_test_earn_copper")]
        pub test_earn_copper: i64,
        #[serde(default = "default_edit_line_bonus_threshold")]
        pub edit_line_bonus_threshold: u64,

        // Escrow ticks
        #[serde(default = "default_pass_escrow_ticks")]
        pub pass_escrow_ticks: u64,
        #[serde(default = "default_speak_escrow_ticks")]
        pub speak_escrow_ticks: u64,
        #[serde(default = "default_edit_escrow_ticks")]
        pub edit_escrow_ticks: u64,
        #[serde(default = "default_test_escrow_ticks")]
        pub test_escrow_ticks: u64,

        // Interest
        #[serde(default = "default_interest_min_held_copper")]
        pub interest_min_held_copper: i64,
        #[serde(default = "default_interest_per_10_copper_held")]
        pub interest_per_10_copper_held: i64,

        // Classifier
        #[serde(default = "default_pass_body_len_threshold")]
        pub pass_body_len_threshold: usize,

        // Disputes
        #[serde(default = "default_objection_cost_copper")]
        pub objection_cost_copper: i64,
        #[serde(default = "default_dispute_speech_cost_copper")]
        pub dispute_speech_cost_copper: i64,
        #[serde(default = "default_dispute_edit_cost_copper")]
        pub dispute_edit_cost_copper: i64,
        #[serde(default = "default_judge_cost_per_party")]
        pub judge_cost_per_party: i64,
        #[serde(default = "default_judge_auto_invoke_threshold")]
        pub judge_auto_invoke_threshold: i64,
        #[serde(default = "default_system_dispute_cost")]
        pub system_dispute_cost: i64,
        #[serde(default = "default_system_dispute_reward")]
        pub system_dispute_reward: i64,
        #[serde(default = "default_system_dispute_penalty")]
        pub system_dispute_penalty: i64,
        #[serde(default = "default_system_dispute_ban_turns")]
        pub system_dispute_ban_turns: u64,
        #[serde(default = "default_clawback_percent")]
        pub clawback_percent: u64,

        // Penalty hooks
        #[serde(default = "default_retro_pass_penalty_copper")]
        pub retro_pass_penalty_copper: i64,
        #[serde(default = "default_retro_pass_scan_window_turns")]
        pub retro_pass_scan_window_turns: u64,
        #[serde(default = "default_coliability_test_penalty_copper")]
        pub coliability_test_penalty_copper: i64,

        // Bounty
        #[serde(default = "default_bounty_claim_stake_percent")]
        pub bounty_claim_stake_percent: u64,
        #[serde(default = "default_bounty_abandon_loss_percent")]
        pub bounty_abandon_loss_percent: u64,
        #[serde(default = "default_bounty_reject_loss_percent")]
        pub bounty_reject_loss_percent: u64,
        #[serde(default = "default_bounty_objection_clawback_percent")]
        pub bounty_objection_clawback_percent: u64,

        // Decay tax
        #[serde(default = "default_decay_copper_pct_per_turn_tenths")]
        pub decay_copper_pct_per_turn_tenths: i64,
        #[serde(default = "default_decay_silver_pct_per_turn_tenths")]
        pub decay_silver_pct_per_turn_tenths: i64,
        #[serde(default = "default_decay_floor_copper")]
        pub decay_floor_copper: i64,

        // Oxford-debate (commit 2d — completes "every economic constant" per human msg 657)
        #[serde(default = "default_oxford_default_winning_reward_copper")]
        pub oxford_default_winning_reward_copper: i64,
        #[serde(default = "default_oxford_turn_soft_limit_secs")]
        pub oxford_turn_soft_limit_secs: u64,
        #[serde(default = "default_oxford_turn_hard_limit_secs")]
        pub oxford_turn_hard_limit_secs: u64,
        #[serde(default = "default_oxford_audience_vote_window_secs")]
        pub oxford_audience_vote_window_secs: u64,
        #[serde(default = "default_oxford_moderator_vacancy_timeout_secs")]
        pub oxford_moderator_vacancy_timeout_secs: u64,
        #[serde(default = "default_oxford_react_rate_limit_per_min")]
        pub oxford_react_rate_limit_per_min: u64,
    }

    // ---- per-field default fns (required by serde(default = "...")) ----
    fn default_starting_balance_copper() -> i64 { STARTING_BALANCE_COPPER }
    fn default_deficit_cap_copper() -> i64 { DEFICIT_CAP_COPPER }
    fn default_passive_per_tick_copper() -> i64 { PASSIVE_PER_TICK_COPPER }
    fn default_pass_earn_copper() -> i64 { PASS_EARN_COPPER }
    fn default_speak_earn_copper() -> i64 { SPEAK_EARN_COPPER }
    fn default_edit_earn_copper() -> i64 { EDIT_EARN_COPPER }
    fn default_test_earn_copper() -> i64 { TEST_EARN_COPPER }
    fn default_edit_line_bonus_threshold() -> u64 { EDIT_LINE_BONUS_THRESHOLD }
    fn default_pass_escrow_ticks() -> u64 { PASS_ESCROW_TICKS }
    fn default_speak_escrow_ticks() -> u64 { SPEAK_ESCROW_TICKS }
    fn default_edit_escrow_ticks() -> u64 { EDIT_ESCROW_TICKS }
    fn default_test_escrow_ticks() -> u64 { TEST_ESCROW_TICKS }
    fn default_interest_min_held_copper() -> i64 { INTEREST_MIN_HELD_COPPER }
    fn default_interest_per_10_copper_held() -> i64 { INTEREST_PER_10_COPPER_HELD }
    fn default_pass_body_len_threshold() -> usize { PASS_BODY_LEN_THRESHOLD }
    fn default_objection_cost_copper() -> i64 { OBJECTION_COST_COPPER }
    fn default_dispute_speech_cost_copper() -> i64 { DISPUTE_SPEECH_COST_COPPER }
    fn default_dispute_edit_cost_copper() -> i64 { DISPUTE_EDIT_COST_COPPER }
    fn default_judge_cost_per_party() -> i64 { JUDGE_COST_PER_PARTY }
    fn default_judge_auto_invoke_threshold() -> i64 { JUDGE_AUTO_INVOKE_THRESHOLD }
    fn default_system_dispute_cost() -> i64 { SYSTEM_DISPUTE_COST }
    fn default_system_dispute_reward() -> i64 { SYSTEM_DISPUTE_REWARD }
    fn default_system_dispute_penalty() -> i64 { SYSTEM_DISPUTE_PENALTY }
    fn default_system_dispute_ban_turns() -> u64 { SYSTEM_DISPUTE_BAN_TURNS }
    fn default_clawback_percent() -> u64 { CLAWBACK_PERCENT }
    fn default_retro_pass_penalty_copper() -> i64 { RETRO_PASS_PENALTY_COPPER }
    fn default_retro_pass_scan_window_turns() -> u64 { RETRO_PASS_SCAN_WINDOW_TURNS }
    fn default_coliability_test_penalty_copper() -> i64 { COLIABILITY_TEST_PENALTY_COPPER }
    fn default_bounty_claim_stake_percent() -> u64 { BOUNTY_CLAIM_STAKE_PERCENT }
    fn default_bounty_abandon_loss_percent() -> u64 { BOUNTY_ABANDON_LOSS_PERCENT }
    fn default_bounty_reject_loss_percent() -> u64 { BOUNTY_REJECT_LOSS_PERCENT }
    fn default_bounty_objection_clawback_percent() -> u64 { BOUNTY_OBJECTION_CLAWBACK_PERCENT }
    fn default_decay_copper_pct_per_turn_tenths() -> i64 { DECAY_COPPER_PCT_PER_TURN_TENTHS }
    fn default_decay_silver_pct_per_turn_tenths() -> i64 { DECAY_SILVER_PCT_PER_TURN_TENTHS }
    fn default_decay_floor_copper() -> i64 { DECAY_FLOOR_COPPER }
    fn default_oxford_default_winning_reward_copper() -> i64 { super::oxford::OXFORD_DEFAULT_WINNING_REWARD_COPPER }
    fn default_oxford_turn_soft_limit_secs() -> u64 { super::oxford::OXFORD_TURN_SOFT_LIMIT_SECS }
    fn default_oxford_turn_hard_limit_secs() -> u64 { super::oxford::OXFORD_TURN_HARD_LIMIT_SECS }
    fn default_oxford_audience_vote_window_secs() -> u64 { super::oxford::OXFORD_AUDIENCE_VOTE_WINDOW_SECS }
    fn default_oxford_moderator_vacancy_timeout_secs() -> u64 { super::oxford::OXFORD_MODERATOR_VACANCY_TIMEOUT_SECS }
    fn default_oxford_react_rate_limit_per_min() -> u64 { super::oxford::OXFORD_REACT_RATE_LIMIT_PER_MIN }

    impl Default for EconomySettings {
        fn default() -> Self {
            // Serde-roundtripping an empty object hits every default fn — keeps
            // the defaults-source single (no duplication between this impl and
            // the per-field default fns above).
            serde_json::from_str("{}").expect("EconomySettings defaults must serialize")
        }
    }

    /// Read economy settings from .vaak/economy.json. Returns defaults when
    /// the file is absent OR unparseable (best-effort — never blocks the
    /// economy on a malformed settings file). No caching: every call reads
    /// the file fresh, so UI-driven edits land next-tick.
    pub fn read_economy_settings(dir: &str) -> EconomySettings {
        let path = economy_json_path(dir);
        if !path.exists() {
            return EconomySettings::default();
        }
        let raw = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[currency.economy] WARN: read failed for {} — using defaults: {}", path.display(), e);
                return EconomySettings::default();
            }
        };
        match serde_json::from_str::<EconomySettings>(&raw) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[currency.economy] WARN: parse failed for {} — using defaults: {}", path.display(), e);
                EconomySettings::default()
            }
        }
    }

    /// Atomically write economy settings to .vaak/economy.json. Caller is
    /// responsible for any audit ledger row + sanity validation.
    pub fn write_economy_settings(dir: &str, settings: &EconomySettings) -> Result<(), String> {
        let path = economy_json_path(dir);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("economy.json mkdir: {}", e))?;
        }
        let json = serde_json::to_string_pretty(settings)
            .map_err(|e| format!("economy.json serialize: {}", e))?;
        atomic_write(&path, json.as_bytes())
    }

    /// Round-up integer division: `ceil(numer / denom)` over non-negative ints.
    /// Used by the decay tax to round fractional decay UP to the nearest whole
    /// (per human msg 420 spec: "5 copper rounded up to the nearest whole").
    #[inline]
    fn ceil_div(numer: i64, denom: i64) -> i64 {
        if numer <= 0 || denom <= 0 { return 0; }
        (numer + denom - 1) / denom
    }

    /// Human msg 458 — direct human authority over balances. Pure currency
    /// mutation extracted into collab so both the MCP-tool path (vaak-mcp.rs)
    /// and the Tauri-command path (main.rs) can call it without duplicating
    /// the lock + ledger + snapshot dance. Callers are responsible for any
    /// board broadcast (system message) AFTER this returns.
    ///
    /// Validates: non-empty seat, non-empty reason, non-zero amount, caller
    /// starts with "human:", seat is already known to the snapshot. Returns
    /// {balance_before, balance_after, timed_out, ...} JSON for the caller.
    pub fn apply_human_adjust(
        dir: &str,
        caller: &str,
        seat: &str,
        amount_copper: i64,
        reason: &str,
    ) -> Result<serde_json::Value, String> {
        if !caller.starts_with("human:") {
            return Err("[HumanAdjust] only human:* can adjust balances directly.".to_string());
        }
        if seat.trim().is_empty() {
            return Err("[HumanAdjust] seat label is required.".to_string());
        }
        if reason.trim().is_empty() {
            return Err("[HumanAdjustReasonRequired] non-empty reason is mandatory (audit requirement per architect msg 469).".to_string());
        }
        if amount_copper == 0 {
            return Err("[HumanAdjust] amount_copper must be non-zero (positive = credit, negative = debit).".to_string());
        }
        super::with_currency_and_board_lock(dir, || {
            let mut snap = read_balances_snapshot(dir)?;
            if snap.seats.is_empty() && currency_jsonl_path(dir).exists() {
                snap = replay_balances_from_ledger(dir)?;
            }
            if !snap.seats.contains_key(seat) {
                return Err(format!("[HumanAdjust] seat {} has no balance entry yet (must join first).", seat));
            }
            let pre_bal = snap.seats.get(seat).unwrap().balance;
            let new_bal = pre_bal.saturating_add(amount_copper);
            {
                let e = snap.seats.get_mut(seat).unwrap();
                e.balance = new_bal;
                if e.balance <= DEFICIT_CAP_COPPER {
                    e.timed_out = true;
                }
            }
            let id = snap.next_txn_id;
            snap.next_txn_id += 1;
            let txn = LedgerRow {
                id,
                txn_type: "human_adjust".to_string(),
                seat: seat.to_string(),
                amount: amount_copper,
                reason: format!("human_adjust by {}: {}", caller, reason),
                ref_msg: None,
                balance_after: new_bal,
                escrow_id: None,
                release_turn: None,
                turn: Some(snap.turn_counter),
                // Tester:0 msg 663 #3 spec-drift fix — distinct action_kind so
                // per-action_kind telemetry doesn't conflate human-issued debits
                // with retro-Pass + co-liability penalties.
                action_kind: Some(ActionKind::HumanAdjust),
                linked_edit_msg: None,
                at: super::iso_now(),
            };
            append_currency_transaction(dir, &txn)?;
            write_balances_snapshot(dir, &snap)?;
            Ok(serde_json::json!({
                "seat": seat,
                "amount_copper": amount_copper,
                "balance_before": pre_bal,
                "balance_after": new_bal,
                "timed_out": new_bal <= DEFICIT_CAP_COPPER,
                "reason": reason,
                "txn_id": id,
            }))
        })
    }

    // ---- Types ----

    /// Action classification for a project_send. Exempt = human (no charge).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    pub enum ActionKind {
        // classify_action returns these three (Phase 1)
        Pass, Speak, Exempt,
        // Phase 2 ledger opcodes (dev-challenger:0 msg 1428 — stable opcode for
        // programmatic scans, separate from prose `reason`). Edit/Test reserved
        // for Phase 4. Exempt kept so classify_action still compiles.
        Init, Edit, Test,
        Credit, EscrowHold, EscrowRelease,
        Passive, Interest, Penalty, Clawback,
        PoolDestroyed,
        // Phase 6: bounty economy opcodes.
        BountyStake,    // claimant stakes 10% to claim a bounty (debit)
        BountyEarn,     // claimant paid out on approval (credit = amount + stake)
        BountyClawback, // objection-on-approved-bounty claws back 90% of payout
        BountyExpire,   // expired/abandoned bounty stake destroyed
        // Human msg 458: per-turn wealth tax. Emitted in two flavors for Flow
        // Feed readability (copper-decay + silver-decay), both destroy copper
        // to the system sink (no per-seat redistribution).
        Decay,
        // Human msg 458 + tester:0 msg 663 #3 spec-drift fix: distinct
        // action_kind for human-issued adjusts so telemetry (architect plan
        // §5c per-action_kind dashboard) doesn't lump these with retro-Pass
        // + co-liability penalties. Sign of `amount` distinguishes credit
        // vs debit; the action_kind is the categorical tag.
        HumanAdjust,
        // Human msg 657: distinct action_kind for the audit row emitted when
        // economy.json settings are written via UI. Captures who-tuned-what-
        // when at the ledger layer (per evil-arch msg 661 requirement). Amount
        // is always 0; the reason field carries the field name + old → new.
        EconomyTune,
    }

    /// Display split. balances.json carries copper only; UI consumers call
    /// `copper_to_display`. Per ui-architect:0 msg 1071 single-helper rule.
    #[derive(Debug, Clone, Copy, Serialize, Deserialize)]
    pub struct CopperDisplay { pub gold: i64, pub silver: i64, pub copper: i64 }

    /// Currency transaction row appended to .vaak/currency.jsonl.
    /// Self-describing per ui-architect:0 msg 1071 — `reason` is human-prose,
    /// not an opcode. Future ledger UIs render rows without joining board.jsonl.
    /// Default derived (Phase 2) so construction sites can `..Default::default()`
    /// the optional Phase-2 fields they don't populate.
    #[derive(Debug, Clone, Default, Serialize, Deserialize)]
    pub struct LedgerRow {
        pub id: u64,
        #[serde(rename = "type")]
        pub txn_type: String, // "init" | "credit" | "escrow_hold" | "escrow_release" | "passive" | "interest" | "penalty"
        pub seat: String,
        pub amount: i64,
        pub reason: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub ref_msg: Option<u64>,
        pub balance_after: i64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub escrow_id: Option<String>,
        /// Commit (c): the maturity turn for an escrow_hold row, so replay can
        /// reconstruct EscrowItem.release_turn faithfully (previously lost — set
        /// to 0 on rebuild). Optional + serde default for backward-compat with
        /// commit-(a)/(b) rows written before this field existed.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub release_turn: Option<u64>,
        /// Phase 2: monotonic turn counter at write time. Phase 4 retro-scan
        /// filters by turn window; None on Phase 1 rows (skipped as "predates
        /// the field" per developer:1 msg 1430 #11).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub turn: Option<u64>,
        /// Phase 2: stable opcode for programmatic scans, separate from prose
        /// `reason` (dev-challenger:0 msg 1428). Phase 4 filters action_kind==Pass.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub action_kind: Option<ActionKind>,
        /// Phase 2: for Test rows (Phase 4), points at the edit being tested;
        /// co-liability scan finds linked Tests via this (developer:1 msg 1430 #5).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub linked_edit_msg: Option<u64>,
        pub at: String, // ISO8601
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct EscrowItem {
        pub id: String,           // "esc_{:06x}"
        pub amount: i64,
        pub release_turn: u64,
        pub action: String,       // "pass" | "speak" (Edit/Test in Phase 3)
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub ref_msg: Option<u64>,
    }

    #[derive(Debug, Clone, Default, Serialize, Deserialize)]
    pub struct SeatBalance {
        pub balance: i64,
        pub escrow_held: i64,
        #[serde(default)]
        pub escrow_items: Vec<EscrowItem>,
        #[serde(default)]
        pub timed_out: bool,
        /// Phase 2 (c): turn until which this seat is banned from filing system
        /// disputes (set on an incorrect system_dispute ruling; cleared on
        /// reinstate). None = not banned.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub system_dispute_ban_until: Option<u64>,
    }

    #[derive(Debug, Clone, Default, Serialize, Deserialize)]
    pub struct BalancesSnapshot {
        pub turn_counter: u64,
        #[serde(default)]
        pub next_txn_id: u64,
        #[serde(default)]
        pub next_escrow_id: u64,
        /// Phase 6: monotonic bounty id counter (serde-default for back-compat
        /// with pre-Phase-6 snapshots).
        #[serde(default)]
        pub next_bounty_id: u64,
        pub seats: HashMap<String, SeatBalance>,
    }

    // ---- Phase 2: Disputes ----

    /// A single message exchanged inside a dispute (spec §disputes.jsonl).
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct DisputeMessage {
        pub from: String,
        pub body: String,
        #[serde(default)]
        pub added_to_pool: i64,
        pub at: String,
    }

    /// A dispute row appended to .vaak/disputes.jsonl (append-only; the latest
    /// row with a given id is the current state). status: open|resolved|destroyed.
    /// resolution: null|challenger_wins|target_wins|both_wrong|conceded_by_<seat>.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct DisputeRow {
        pub id: String, // "disp_{:06x}"
        pub challenger: String,
        pub target: String,
        pub target_msg: u64,
        pub pool: i64,
        pub status: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub resolution: Option<String>,
        #[serde(default)]
        pub messages: Vec<DisputeMessage>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub judge: Option<String>,
        pub opened_at: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub resolved_at: Option<String>,
        #[serde(default)]
        pub turn_opened: u64,
    }

    /// Snapshot mirror for the O(1) Pass-while-disputed gate (developer:1 msg
    /// 1430 #6). The send-path reads this small file, not the whole disputes.jsonl.
    #[derive(Debug, Clone, Default, Serialize, Deserialize)]
    pub struct OpenDisputesSnapshot {
        #[serde(default)]
        pub open_by_target: HashMap<String, Vec<String>>,
        #[serde(default)]
        pub open_by_challenger: HashMap<String, Vec<String>>,
        #[serde(default)]
        pub next_dispute_id: u64,
    }

    // ---- Phase 6: Bounties ----

    /// A bounty row appended to .vaak/bounties.jsonl (append-only; latest row
    /// per `id` is current state). status: open|claimed|submitted|approved|
    /// rejected|expired|abandoned.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct BountyRow {
        pub id: String, // "bounty_{:06x}"
        pub description: String,
        pub amount: i64,
        pub posted_by: String,
        pub deadline_turn: u64,
        pub status: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub claimant: Option<String>,
        #[serde(default)]
        pub claim_stake: i64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub submission_msg: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub approved_by: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub last_rejection_reason: Option<String>,
        pub posted_at: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub resolved_at: Option<String>,
        #[serde(default)]
        pub turn_posted: u64,
    }

    /// Snapshot mirror for fast UI render + lifecycle lookups. Maps bounty id →
    /// latest row. next_bounty_id mirrors balances.json for id allocation.
    #[derive(Debug, Clone, Default, Serialize, Deserialize)]
    pub struct OpenBountiesSnapshot {
        #[serde(default)]
        pub next_bounty_id: u64,
        #[serde(default)]
        pub bounties: HashMap<String, BountyRow>,
    }

    // ---- Paths ----

    fn vaak_root(dir: &str) -> PathBuf {
        Path::new(dir).join(".vaak")
    }
    pub fn currency_jsonl_path(dir: &str) -> PathBuf {
        vaak_root(dir).join("currency.jsonl")
    }
    pub fn balances_json_path(dir: &str) -> PathBuf {
        vaak_root(dir).join("balances.json")
    }
    pub fn currency_lock_path(dir: &str) -> PathBuf {
        vaak_root(dir).join("currency.lock")
    }
    pub fn disputes_jsonl_path(dir: &str) -> PathBuf {
        vaak_root(dir).join("disputes.jsonl")
    }
    pub fn open_disputes_json_path(dir: &str) -> PathBuf {
        vaak_root(dir).join("open_disputes.json")
    }
    pub fn bounties_jsonl_path(dir: &str) -> PathBuf {
        vaak_root(dir).join("bounties.jsonl")
    }
    pub fn open_bounties_json_path(dir: &str) -> PathBuf {
        vaak_root(dir).join("open_bounties.json")
    }

    // ---- Display helper ----

    /// Single source of truth for copper → gold/silver/copper display split.
    /// Per ui-architect:0 msg 1071: do NOT re-implement this elsewhere.
    /// Per-field sign: negative copper splits into negative gold/silver/copper.
    pub fn copper_to_display(c: i64) -> CopperDisplay {
        let sign: i64 = if c < 0 { -1 } else { 1 };
        let abs = c.abs();
        let gold = abs / 10_000;
        let silver = (abs % 10_000) / 100;
        let copper = abs % 100;
        CopperDisplay { gold: sign * gold, silver: sign * silver, copper: sign * copper }
    }

    // ---- Snapshot I/O ----

    pub fn read_balances_snapshot(dir: &str) -> Result<BalancesSnapshot, String> {
        let path = balances_json_path(dir);
        if !path.exists() {
            return Ok(BalancesSnapshot::default());
        }
        let raw = std::fs::read_to_string(&path)
            .map_err(|e| format!("read balances.json: {}", e))?;
        serde_json::from_str(&raw).map_err(|e| format!("parse balances.json: {}", e))
    }

    pub fn write_balances_snapshot(dir: &str, snap: &BalancesSnapshot) -> Result<(), String> {
        let path = balances_json_path(dir);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let body = serde_json::to_vec_pretty(snap)
            .map_err(|e| format!("serialize balances.json: {}", e))?;
        atomic_write(&path, &body)
    }

    /// Append a transaction row to currency.jsonl. Must be called inside
    /// with_currency_and_board_lock (or a test that explicitly holds the
    /// currency lock). Caller is responsible for setting `id` and
    /// `balance_after` correctly relative to the in-memory snapshot.
    pub fn append_currency_transaction(dir: &str, row: &LedgerRow) -> Result<(), String> {
        let path = currency_jsonl_path(dir);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        use std::io::Write;
        let line = serde_json::to_string(row)
            .map_err(|e| format!("serialize ledger row: {}", e))?;
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| format!("open currency.jsonl: {}", e))?;
        writeln!(f, "{}", line).map_err(|e| format!("write currency.jsonl: {}", e))?;
        Ok(())
    }

    /// Generate the next escrow id as `esc_{:06x}` from the snapshot's
    /// monotonic counter. Caller updates `snap.next_escrow_id` after use.
    pub fn next_escrow_id(snap: &mut BalancesSnapshot) -> String {
        let id = snap.next_escrow_id;
        snap.next_escrow_id = snap.next_escrow_id.wrapping_add(1);
        format!("esc_{:06x}", id)
    }

    // ---- Phase 2: Dispute I/O ----
    // All callers must hold with_currency_and_board_lock (same lock as
    // currency + board per Phase 1 ruling 9-corrected).

    /// Append a dispute row to disputes.jsonl (append-only; latest row per id
    /// wins on read). Mirrors append_currency_transaction.
    pub fn append_dispute_row(dir: &str, row: &DisputeRow) -> Result<(), String> {
        let path = disputes_jsonl_path(dir);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        use std::io::Write;
        let line = serde_json::to_string(row)
            .map_err(|e| format!("serialize dispute row: {}", e))?;
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| format!("open disputes.jsonl: {}", e))?;
        writeln!(f, "{}", line).map_err(|e| format!("write disputes.jsonl: {}", e))?;
        Ok(())
    }

    /// Read the open-disputes snapshot (default-empty when the file is absent).
    pub fn read_open_disputes_snapshot(dir: &str) -> Result<OpenDisputesSnapshot, String> {
        let path = open_disputes_json_path(dir);
        if !path.exists() {
            return Ok(OpenDisputesSnapshot::default());
        }
        let raw = std::fs::read_to_string(&path)
            .map_err(|e| format!("read open_disputes.json: {}", e))?;
        serde_json::from_str(&raw).map_err(|e| format!("parse open_disputes.json: {}", e))
    }

    /// Atomic-write the open-disputes snapshot (mirrors write_balances_snapshot).
    pub fn write_open_disputes_snapshot(dir: &str, snap: &OpenDisputesSnapshot) -> Result<(), String> {
        let path = open_disputes_json_path(dir);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let body = serde_json::to_vec_pretty(snap)
            .map_err(|e| format!("serialize open_disputes.json: {}", e))?;
        atomic_write(&path, &body)
    }

    /// Generate the next dispute id `disp_{:06x}` from the snapshot counter.
    pub fn next_dispute_id(snap: &mut OpenDisputesSnapshot) -> String {
        let id = snap.next_dispute_id;
        snap.next_dispute_id = snap.next_dispute_id.wrapping_add(1);
        format!("disp_{:06x}", id)
    }

    // ---- Phase 6: Bounty helpers (mirror the dispute helpers above) ----

    pub fn append_bounty_row(dir: &str, row: &BountyRow) -> Result<(), String> {
        let path = bounties_jsonl_path(dir);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        use std::io::Write;
        let line = serde_json::to_string(row)
            .map_err(|e| format!("serialize bounty row: {}", e))?;
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| format!("open bounties.jsonl: {}", e))?;
        writeln!(f, "{}", line).map_err(|e| format!("write bounties.jsonl: {}", e))?;
        Ok(())
    }

    /// Read the open-bounties snapshot (default-empty when absent).
    pub fn read_open_bounties_snapshot(dir: &str) -> Result<OpenBountiesSnapshot, String> {
        let path = open_bounties_json_path(dir);
        if !path.exists() {
            return Ok(OpenBountiesSnapshot::default());
        }
        let raw = std::fs::read_to_string(&path)
            .map_err(|e| format!("read open_bounties.json: {}", e))?;
        serde_json::from_str(&raw).map_err(|e| format!("parse open_bounties.json: {}", e))
    }

    /// Atomic-write the open-bounties snapshot.
    pub fn write_open_bounties_snapshot(dir: &str, snap: &OpenBountiesSnapshot) -> Result<(), String> {
        let path = open_bounties_json_path(dir);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let body = serde_json::to_vec_pretty(snap)
            .map_err(|e| format!("serialize open_bounties.json: {}", e))?;
        atomic_write(&path, &body)
    }

    /// Latest row per bounty id, replayed from bounties.jsonl (append-only).
    pub fn read_latest_bounties(dir: &str) -> HashMap<String, BountyRow> {
        let mut latest: HashMap<String, BountyRow> = HashMap::new();
        if let Ok(content) = std::fs::read_to_string(bounties_jsonl_path(dir)) {
            for line in content.lines() {
                if line.trim().is_empty() { continue; }
                if let Ok(row) = serde_json::from_str::<BountyRow>(line) {
                    latest.insert(row.id.clone(), row);
                }
            }
        }
        latest
    }

    // ---- Phase 7: Persistence (session snapshot + carry-over) ----

    pub fn currency_history_dir(dir: &str) -> PathBuf {
        vaak_root(dir).join("currency-history")
    }

    /// Build the end-of-session snapshot JSON (spec §Item 1). Aggregates the
    /// current balances + the ledger/disputes/bounties into per-seat lifetime-ish
    /// metrics. `times_timed_out` is best-effort (1 if currently timed_out, else
    /// counts penalty rows that drove balance under the cap is not tracked — we
    /// report the current flag). Pure read; no locks taken here (caller holds).
    pub fn build_session_snapshot(dir: &str) -> serde_json::Value {
        let bal = read_balances_snapshot(dir).unwrap_or_default();
        let ledger = read_ledger_rows(dir).unwrap_or_default();
        let bounties = read_latest_bounties(dir);
        // latest dispute row per id
        let mut disputes: HashMap<String, DisputeRow> = HashMap::new();
        if let Ok(c) = std::fs::read_to_string(disputes_jsonl_path(dir)) {
            for l in c.lines() {
                if l.trim().is_empty() { continue; }
                if let Ok(d) = serde_json::from_str::<DisputeRow>(l) { disputes.insert(d.id.clone(), d); }
            }
        }

        let mut seats = serde_json::Map::new();
        let mut pool_destroyed: i64 = 0;
        for row in &ledger {
            if row.txn_type == "pool_destroyed" || row.txn_type == "bounty_expire" {
                pool_destroyed += row.amount.abs();
            }
        }

        // union of seats from balances + ledger
        let mut all_seats: std::collections::BTreeSet<String> = bal.seats.keys().cloned().collect();
        for r in &ledger { if r.seat != "system:pool" { all_seats.insert(r.seat.clone()); } }

        for seat in all_seats {
            let sb = bal.seats.get(&seat);
            let final_balance = sb.map(|s| s.balance).unwrap_or(STARTING_BALANCE_COPPER);
            let timed_out = sb.map(|s| s.timed_out).unwrap_or(false);
            let (mut earned, mut lost) = (0i64, 0i64);
            let (mut speaks, mut edits, mut tests, mut passes, mut adv_pass) = (0u64, 0u64, 0u64, 0u64, 0u64);
            let mut starting_balance = STARTING_BALANCE_COPPER;
            for r in ledger.iter().filter(|r| r.seat == seat) {
                match r.txn_type.as_str() {
                    "init" => starting_balance = r.amount,
                    "credit" | "escrow_release" | "interest" | "passive" | "bounty_earn" => { if r.amount > 0 { earned += r.amount; } }
                    "penalty" | "clawback" | "debit" | "bounty_stake" => {
                        lost += r.amount.abs();
                        if r.reason.to_lowercase().contains("adversarial pass") { adv_pass += 1; }
                    }
                    _ => {}
                }
                match r.action_kind {
                    Some(ActionKind::Speak) => speaks += 1,
                    Some(ActionKind::Edit) => edits += 1,
                    Some(ActionKind::Test) => tests += 1,
                    Some(ActionKind::Pass) => passes += 1,
                    _ => {}
                }
            }
            let mut obj_filed = 0u64; let mut obj_recv = 0u64; let mut dwon = 0u64; let mut dlost = 0u64;
            for d in disputes.values() {
                if d.challenger == seat { obj_filed += 1; }
                if d.target == seat { obj_recv += 1; }
                let res = d.resolution.as_deref().unwrap_or("");
                let challenger_won = res == "challenger_wins" || res.starts_with("conceded_by_") && res != format!("conceded_by_{}", d.challenger);
                if d.status == "resolved" {
                    if challenger_won {
                        if d.challenger == seat { dwon += 1; } else if d.target == seat { dlost += 1; }
                    } else if res == "target_wins" || res == format!("conceded_by_{}", d.challenger) {
                        if d.target == seat { dwon += 1; } else if d.challenger == seat { dlost += 1; }
                    }
                }
            }
            let bounties_completed = bounties.values()
                .filter(|b| b.status == "approved" && b.claimant.as_deref() == Some(seat.as_str()))
                .count() as u64;

            seats.insert(seat.clone(), serde_json::json!({
                "final_balance": final_balance,
                "starting_balance": starting_balance,
                "total_earned": earned,
                "total_lost": lost,
                "speaks": speaks, "edits": edits, "tests": tests, "passes": passes,
                "objections_filed": obj_filed, "objections_received": obj_recv,
                "disputes_won": dwon, "disputes_lost": dlost,
                "bounties_completed": bounties_completed,
                "times_timed_out": if timed_out { 1 } else { 0 },
                "adversarial_pass_penalties": adv_pass,
            }));
        }

        let now = super::iso_now();
        let date = now.get(0..10).unwrap_or("").to_string();
        serde_json::json!({
            "session_date": date,
            "session_end_ts": now,
            "duration_turns": bal.turn_counter,
            "seats": seats,
            "total_pool_destroyed": pool_destroyed,
        })
    }

    /// Write the session snapshot to .vaak/currency-history/<date>-NNN.json with a
    /// 3-digit zero-padded sequence (filename lex order = chronological). Caller
    /// holds the currency lock. Returns the written path.
    pub fn write_session_snapshot(dir: &str) -> Result<PathBuf, String> {
        let snapshot = build_session_snapshot(dir);
        let date = snapshot.get("session_date").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
        let hist = currency_history_dir(dir);
        let _ = std::fs::create_dir_all(&hist);
        // find next sequence for this date
        let mut max_seq = 0u32;
        if let Ok(entries) = std::fs::read_dir(&hist) {
            for e in entries.flatten() {
                if let Some(name) = e.file_name().to_str() {
                    if let Some(rest) = name.strip_prefix(&format!("{}-", date)) {
                        if let Some(num) = rest.strip_suffix(".json") {
                            if let Ok(n) = num.parse::<u32>() { if n > max_seq { max_seq = n; } }
                        }
                    }
                }
            }
        }
        let path = hist.join(format!("{}-{:03}.json", date, max_seq + 1));
        let body = serde_json::to_vec_pretty(&snapshot).map_err(|e| format!("serialize snapshot: {}", e))?;
        atomic_write(&path, &body)?;
        Ok(path)
    }

    /// Carry-over on session start (spec §Item 2). For each seat in the MOST
    /// RECENT snapshot, compute the carried starting_balance (cap 10000, timed-out
    /// → 0, deficit → 0) and seed balances.json + an `init` ledger row. Only seeds
    /// seats NOT already in balances.json (idempotent re-join). Also appends one
    /// multi-line "Session started" banner row for the Flow Feed (spec §Item 4).
    /// Returns the count of carried seats. Caller holds the currency lock.
    pub fn apply_carryover(dir: &str) -> Result<u64, String> {
        let hist = currency_history_dir(dir);
        // most recent snapshot file (zero-padded → lex max is newest)
        let mut files: Vec<PathBuf> = match std::fs::read_dir(&hist) {
            Ok(e) => e.flatten().map(|x| x.path())
                .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("json")).collect(),
            Err(_) => return Ok(0),
        };
        if files.is_empty() { return Ok(0); }
        files.sort();
        let latest = files.last().unwrap();
        let snap_json: serde_json::Value = match std::fs::read_to_string(latest)
            .ok().and_then(|s| serde_json::from_str(&s).ok()) {
            Some(v) => v,
            None => return Ok(0),
        };
        let prev_date = snap_json.get("session_date").and_then(|v| v.as_str()).unwrap_or("?").to_string();
        let seats = match snap_json.get("seats").and_then(|v| v.as_object()) {
            Some(s) => s,
            None => return Ok(0),
        };

        let mut bal = read_balances_snapshot(dir)?;
        if bal.seats.is_empty() && currency_jsonl_path(dir).exists() {
            bal = replay_balances_from_ledger(dir)?;
        }
        let now = super::iso_now();
        let mut carried = 0u64;
        let mut banner_lines: Vec<String> = vec!["Session started. Carry-over:".to_string()];
        for (seat, sdata) in seats {
            if bal.seats.contains_key(seat) { continue; } // already seeded this session
            let prev_final = sdata.get("final_balance").and_then(|v| v.as_i64()).unwrap_or(STARTING_BALANCE_COPPER);
            let (start, note) = if prev_final > STARTING_BALANCE_COPPER {
                (STARTING_BALANCE_COPPER, format!("capped from {}", prev_final))
            } else if prev_final <= DEFICIT_CAP_COPPER {
                (0, "timed out last session".to_string())
            } else if prev_final > 0 {
                (prev_final, "carried".to_string())
            } else {
                (0, "deficit not carried".to_string())
            };
            bal.seats.entry(seat.clone()).or_default().balance = start;
            let id = bal.next_txn_id; bal.next_txn_id = bal.next_txn_id.saturating_add(1);
            append_currency_transaction(dir, &LedgerRow {
                id, txn_type: "init".to_string(), seat: seat.clone(), amount: start,
                reason: format!("carried over from session {}", prev_date),
                ref_msg: None, balance_after: start, escrow_id: None, release_turn: None,
                turn: Some(bal.turn_counter), action_kind: Some(ActionKind::Init), linked_edit_msg: None, at: now.clone(),
            })?;
            banner_lines.push(format!("{}: {} copper ({})", seat, start, note));
            carried += 1;
        }
        if carried > 0 {
            write_balances_snapshot(dir, &bal)?;
            let id = bal.next_txn_id;
            append_currency_transaction(dir, &LedgerRow {
                id, txn_type: "init".to_string(), seat: "system:session".to_string(), amount: 0,
                reason: banner_lines.join("\n"),
                ref_msg: None, balance_after: 0, escrow_id: None, release_turn: None,
                turn: Some(bal.turn_counter), action_kind: Some(ActionKind::Init), linked_edit_msg: None, at: now.clone(),
            })?;
        }
        Ok(carried)
    }

    /// Add an open dispute to the snapshot's by-target + by-challenger indexes.
    pub fn snapshot_add_open(snap: &mut OpenDisputesSnapshot, dispute_id: &str, challenger: &str, target: &str) {
        snap.open_by_target.entry(target.to_string()).or_default().push(dispute_id.to_string());
        snap.open_by_challenger.entry(challenger.to_string()).or_default().push(dispute_id.to_string());
    }

    /// Remove a resolved/destroyed dispute from the snapshot indexes.
    pub fn snapshot_remove_open(snap: &mut OpenDisputesSnapshot, dispute_id: &str, challenger: &str, target: &str) {
        if let Some(v) = snap.open_by_target.get_mut(target) {
            v.retain(|d| d != dispute_id);
            if v.is_empty() { snap.open_by_target.remove(target); }
        }
        if let Some(v) = snap.open_by_challenger.get_mut(challenger) {
            v.retain(|d| d != dispute_id);
            if v.is_empty() { snap.open_by_challenger.remove(challenger); }
        }
    }

    /// Phase 2 / Phase 4 prep: the roles flagged adversarial:true. Hardcoded in
    /// Rust (not read from the runtime tree) for robustness — the source
    /// `migrations/seed-adversarial-tags.json` documents + version-controls the
    /// same list (survives a fresh clone, the gap ui-architect:0 msg 1219 flagged).
    pub const ADVERSARIAL_SEED_ROLES: &[&str] = &["evil-architect", "dev-challenger"];

    /// If any ADVERSARIAL_SEED_ROLES role exists in project.json without
    /// `adversarial: true`, write the tag in (atomic). Returns true if it wrote.
    /// Caller MUST hold with_currency_and_board_lock (developer:1 msg 1430 #10
    /// race: two concurrent seats both seeding project.json).
    pub fn apply_adversarial_seed(dir: &str) -> Result<bool, String> {
        let path = Path::new(dir).join(".vaak").join("project.json");
        let raw = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(_) => return Ok(false), // no project.json yet — nothing to seed
        };
        let mut val: serde_json::Value = serde_json::from_str(&raw)
            .map_err(|e| format!("parse project.json for adversarial seed: {}", e))?;
        let mut changed = false;
        if let Some(roles) = val.get_mut("roles").and_then(|r| r.as_object_mut()) {
            for role in ADVERSARIAL_SEED_ROLES {
                if let Some(rc) = roles.get_mut(*role).and_then(|r| r.as_object_mut()) {
                    let already = rc.get("adversarial").and_then(|b| b.as_bool()).unwrap_or(false);
                    if !already {
                        rc.insert("adversarial".to_string(), serde_json::Value::Bool(true));
                        changed = true;
                    }
                }
            }
        }
        if changed {
            let body = serde_json::to_vec_pretty(&val)
                .map_err(|e| format!("serialize project.json after adversarial seed: {}", e))?;
            atomic_write(&path, &body)?;
        }
        Ok(changed)
    }

    // ---- Replay (rebuild snapshot from currency.jsonl) ----

    /// Replay currency.jsonl line-by-line and rebuild the BalancesSnapshot.
    /// Per architect ruling + dev-challenger:0 msg 1080 nit #2:
    ///   - Last line that fails to parse → WARN-and-skip (partial-write tolerance)
    ///   - Any earlier line that fails to parse → HARD ERROR
    ///   - Duplicate `type:"init"` for same seat → HARD ERROR
    pub fn replay_balances_from_ledger(dir: &str) -> Result<BalancesSnapshot, String> {
        let path = currency_jsonl_path(dir);
        if !path.exists() {
            return Ok(BalancesSnapshot::default());
        }
        let raw = std::fs::read_to_string(&path)
            .map_err(|e| format!("read currency.jsonl: {}", e))?;
        let lines: Vec<&str> = raw.lines().collect();
        let mut snap = BalancesSnapshot::default();
        let mut max_txn_id: u64 = 0;
        let last_idx = lines.len().saturating_sub(1);
        for (i, line) in lines.iter().enumerate() {
            if line.trim().is_empty() { continue; }
            let row: LedgerRow = match serde_json::from_str(line) {
                Ok(r) => r,
                Err(e) => {
                    if i == last_idx {
                        eprintln!(
                            "[currency.replay] WARN: skipping unparseable last line {} (partial write tolerance): {}",
                            i + 1, e
                        );
                        continue;
                    } else {
                        return Err(format!(
                            "currency.jsonl parse error at line {}: {} (line: {})",
                            i + 1, e, line
                        ));
                    }
                }
            };
            apply_row(&mut snap, &row)?;
            if row.id > max_txn_id { max_txn_id = row.id; }
        }
        snap.next_txn_id = max_txn_id.saturating_add(1);
        Ok(snap)
    }

    /// Apply a single ledger row to an in-memory snapshot. Pure function;
    /// no I/O. Used by both replay and live-write paths.
    fn apply_row(snap: &mut BalancesSnapshot, row: &LedgerRow) -> Result<(), String> {
        let seat_entry = snap.seats.entry(row.seat.clone()).or_default();
        match row.txn_type.as_str() {
            "init" => {
                // Invariant per dev-challenger:0 msg 1080 nit #2: exactly ONE init per seat.
                // Detect via: if seat already has a non-zero balance OR any prior escrow,
                // a second init is a HARD ERROR.
                if seat_entry.balance != 0
                    || !seat_entry.escrow_items.is_empty()
                    || seat_entry.escrow_held != 0
                {
                    return Err(format!(
                        "currency.replay HARD ERROR: duplicate init for seat {} (row id {})",
                        row.seat, row.id
                    ));
                }
                seat_entry.balance = row.amount;
            }
            "credit" => {
                seat_entry.balance = seat_entry.balance.saturating_add(row.amount);
            }
            "passive" => {
                seat_entry.balance = seat_entry.balance.saturating_add(row.amount);
            }
            "interest" => {
                seat_entry.balance = seat_entry.balance.saturating_add(row.amount);
            }
            "penalty" => {
                seat_entry.balance = seat_entry.balance.saturating_add(row.amount);
                if seat_entry.balance <= DEFICIT_CAP_COPPER {
                    seat_entry.timed_out = true;
                }
            }
            "decay" => {
                // Human msg 458 per-turn wealth tax. Amount is negative.
                // Decay never trips timed_out (floor at DECAY_FLOOR_COPPER
                // is enforced at the write side); intentionally NO deficit
                // check here so replay matches live behavior.
                seat_entry.balance = seat_entry.balance.saturating_add(row.amount);
            }
            "human_adjust" => {
                // Human msg 458 — direct human authority over balances.
                // Positive credits, negative debits; can trip timed_out
                // (mirrors penalty arm) when a negative adjust crosses the
                // deficit cap. Audit row preserved verbatim in `reason`.
                seat_entry.balance = seat_entry.balance.saturating_add(row.amount);
                if seat_entry.balance <= DEFICIT_CAP_COPPER {
                    seat_entry.timed_out = true;
                }
            }
            "escrow_hold" => {
                // Amount is negative (funds held). Track in escrow_held + escrow_items.
                let held = row.amount.abs();
                seat_entry.escrow_held = seat_entry.escrow_held.saturating_add(held);
                if let Some(id) = &row.escrow_id {
                    seat_entry.escrow_items.push(EscrowItem {
                        id: id.clone(),
                        amount: held,
                        // Commit (c): faithfully reconstruct maturity from the row.
                        // Falls back to 0 for legacy commit-(a)/(b) rows that
                        // predate the release_turn field (they mature on next tick).
                        release_turn: row.release_turn.unwrap_or(0),
                        action: row.reason.clone(),
                        ref_msg: row.ref_msg,
                    });
                }
            }
            "escrow_release" => {
                // Amount is positive (funds returned to balance). Remove the matching item.
                let amt = row.amount;
                seat_entry.escrow_held = (seat_entry.escrow_held - amt).max(0);
                if let Some(id) = &row.escrow_id {
                    seat_entry.escrow_items.retain(|it| &it.id != id);
                }
                // Released amount is already in row.balance_after via the credit path.
            }
            // Phase 2 (c): reinstatement. balance set to the row's amount (0 per
            // directive — not 10000); punitive + escrow state fully cleared.
            "reinstate" => {
                seat_entry.balance = row.amount;
                seat_entry.timed_out = false;
                seat_entry.escrow_items.clear();
                seat_entry.escrow_held = 0;
                seat_entry.system_dispute_ban_until = None;
            }
            // Phase 2 (c): pool destroyed (both_wrong ruling). Audit-only — no
            // balance change anywhere (the row's seat is "system:pool").
            "pool_destroyed" => {}
            // Phase 2 (c.1): system-dispute ban, made replay-durable. The ban is
            // SET via this audit row (release_turn carries the until-turn) and
            // CLEARED via the reinstate row above — both reconstructable from the
            // ledger, so balances.json stays a rebuildable cache of currency.jsonl.
            "system_dispute_ban" => {
                seat_entry.system_dispute_ban_until = row.release_turn;
            }
            // Phase 6: bounty ledger opcodes. Stake/clawback are debits (amount
            // negative), earn is a credit (amount positive). The global deficit
            // check below trips timed_out as needed. bounty_expire is audit-only
            // (the stake was already debited at claim; this documents the burn —
            // seat is "system:pool", no real balance change).
            "bounty_stake" | "bounty_earn" | "bounty_clawback" => {
                seat_entry.balance = seat_entry.balance.saturating_add(row.amount);
            }
            "bounty_expire" => {}
            other => {
                return Err(format!(
                    "currency.replay HARD ERROR: unknown transaction type {:?} (row id {})",
                    other, row.id
                ));
            }
        }
        if seat_entry.balance <= DEFICIT_CAP_COPPER {
            seat_entry.timed_out = true;
        }
        Ok(())
    }

    /// Phase 4 (b) — does seat's role have `adversarial: true` in project.json?
    /// Locks the Q2=B filter (human msg 1924) onto retro-Pass penalty: only
    /// targets whose role was seeded adversarial (or hand-flagged in
    /// project.json) get the retro Pass penalty. Returns false on any I/O or
    /// parse error — fail-closed so we don't penalize non-adversarial seats
    /// when project.json is briefly unavailable.
    pub fn is_adversarial_role(seat: &str, dir: &str) -> bool {
        let role = match seat.split(':').next() {
            Some(r) if !r.is_empty() => r,
            _ => return false,
        };
        let path = Path::new(dir).join(".vaak").join("project.json");
        let raw = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(_) => return false,
        };
        let val: serde_json::Value = match serde_json::from_str(&raw) {
            Ok(v) => v,
            Err(_) => return false,
        };
        val.get("roles")
            .and_then(|r| r.get(role))
            .and_then(|rc| rc.get("adversarial"))
            .and_then(|b| b.as_bool())
            .unwrap_or(false)
    }

    /// Phase 4 (b) — parsed iterator over currency.jsonl rows. Used by the
    /// retro-Pass scan + (future) co-liability scan. Unparseable lines are
    /// logged and skipped (best-effort during the live hook; replay path uses
    /// the stricter `replay_balances_from_ledger`). Caller MUST hold the
    /// currency lock for read-consistency.
    pub fn read_ledger_rows(dir: &str) -> Result<Vec<LedgerRow>, String> {
        let path = currency_jsonl_path(dir);
        if !path.exists() {
            return Ok(Vec::new());
        }
        let raw = std::fs::read_to_string(&path)
            .map_err(|e| format!("read currency.jsonl: {}", e))?;
        let mut rows: Vec<LedgerRow> = Vec::new();
        for (i, line) in raw.lines().enumerate() {
            if line.trim().is_empty() { continue; }
            match serde_json::from_str::<LedgerRow>(line) {
                Ok(r) => rows.push(r),
                Err(e) => {
                    eprintln!(
                        "[currency.read_ledger_rows] WARN: skip line {}: {}",
                        i + 1, e
                    );
                }
            }
        }
        Ok(rows)
    }

    /// Phase 4 (b) — Retroactive Pass-penalty hook.
    /// Fires from `currency_judge_ruling` (challenger_wins) and
    /// `currency_concede` (target concedes → challenger effectively wins).
    /// Gates per spec v4 "Hook firing gate" + "Retroactive Pass-penalty algorithm":
    ///   1. `is_adversarial_role(target_seat, dir)` (Q2=B, human msg 1924 — LOCKED).
    ///   2. Target row is a `Speak` (Edit targets → co-liability path, commit (c)).
    ///   3. Target Speak row's `turn` is `Some(_)` (Phase 1 legacy rows skipped;
    ///      no backfill per anti-scope).
    /// Scans the seat's `Pass` rows in `[from_turn..target_turn)` window
    /// (inclusive-start, exclusive-end, per Q1 ruling) and emits one
    /// `Penalty` row per Pass row, debiting balance directly (not escrow).
    /// Returns count of penalty rows emitted (0 if any gate skipped).
    ///
    /// MUST be called inside `with_currency_and_board_lock`. Caller is
    /// responsible for writing the snapshot afterward; this fn mutates
    /// `snap` in-place and appends rows to currency.jsonl.
    pub fn emit_retro_pass_penalties(
        dir: &str,
        snap: &mut BalancesSnapshot,
        target_seat: &str,
        target_msg: u64,
        dispute_id: &str,
    ) -> Result<u64, String> {
        // Gate 1: Q2=B adversarial filter (human msg 1924, LOCKED — non-adversarial
        // targets escape the retro hook entirely; T20 is the regression guard).
        if !is_adversarial_role(target_seat, dir) {
            return Ok(0);
        }

        let ledger = read_ledger_rows(dir)?;

        // Gate 2: target row must be a Speak with `action_kind == Speak`. Edit
        // targets are co-liability (commit c) territory; Phase 1 legacy rows
        // without `action_kind` are skipped per "no backfill" anti-scope.
        let target_speak = ledger.iter().find(|r| {
            r.seat == target_seat
                && r.ref_msg == Some(target_msg)
                && r.action_kind == Some(ActionKind::Speak)
        });
        let target_speak = match target_speak {
            Some(r) => r,
            None => return Ok(0),
        };

        // Gate 3: target_turn must be Some (Phase 1 legacy compat).
        let target_turn = match target_speak.turn {
            Some(t) => t,
            None => return Ok(0),
        };

        // Window [from_turn..target_turn): inclusive start, exclusive end.
        // Per evil-arch msg 808 + dev:1 commit 2c: read from EconomySettings so
        // retro-Pass penalty magnitude + scan window are live-tunable via UI.
        let settings = read_economy_settings(dir);
        let from_turn = target_turn.saturating_sub(settings.retro_pass_scan_window_turns);
        let now = super::iso_now();
        let mut count: u64 = 0;

        for row in ledger.iter() {
            if row.seat != target_seat { continue; }
            if row.action_kind != Some(ActionKind::Pass) { continue; }
            let row_turn = match row.turn {
                Some(t) => t,
                None => continue, // None-skip (per developer:1 1899 legacy-compat)
            };
            if row_turn < from_turn || row_turn >= target_turn { continue; }

            let new_balance = {
                let e = snap.seats.entry(target_seat.to_string()).or_insert_with(|| {
                    let mut sb = SeatBalance::default();
                    sb.balance = settings.starting_balance_copper;
                    sb
                });
                e.balance = e.balance.saturating_sub(settings.retro_pass_penalty_copper);
                // Deficit-cap interaction (spec T19): penalty stack that crosses
                // deficit_cap_copper trips timed_out. Mirrors apply_row's penalty
                // arm so the live snapshot stays consistent with a fresh replay.
                if e.balance <= settings.deficit_cap_copper {
                    e.timed_out = true;
                }
                e.balance
            };

            let id = snap.next_txn_id;
            snap.next_txn_id += 1;
            append_currency_transaction(dir, &LedgerRow {
                id,
                txn_type: "penalty".to_string(),
                seat: target_seat.to_string(),
                amount: -settings.retro_pass_penalty_copper,
                reason: format!("adversarial pass (retro from {})", dispute_id),
                ref_msg: row.ref_msg.or(Some(target_msg)),
                balance_after: new_balance,
                escrow_id: None,
                release_turn: None,
                turn: Some(snap.turn_counter),
                action_kind: Some(ActionKind::Penalty),
                linked_edit_msg: None,
                at: now.clone(),
            })?;
            count += 1;
        }

        Ok(count)
    }

    /// Phase 4 (c): co-liability scan. When the challenger effectively won AND
    /// the disputed message was an Edit, the testers who CERTIFIED that edit
    /// (Test rows whose `linked_edit_msg == target_msg`) share the blame:
    /// COLIABILITY_TEST_PENALTY_COPPER each, deduped per seat (Q2 — N tests
    /// from one tester = one penalty). NO adversarial filter (architect 2021:
    /// Q2=B was scoped to retro-Pass only). Returns the number of seats hit.
    ///
    /// Mutual exclusion with retro-Pass is structural: this fn returns 0 unless
    /// the target row is an Edit, and emit_retro_pass_penalties returns 0 unless
    /// it's a Speak — so for any one target exactly one path fires (spec T10).
    pub fn emit_coliability_penalties(
        dir: &str,
        snap: &mut BalancesSnapshot,
        target_seat: &str,
        target_msg: u64,
    ) -> Result<u64, String> {
        let ledger = read_ledger_rows(dir)?;

        // Gate: the disputed message must be an Edit. (Speak targets are the
        // retro-Pass path; Phase 1 legacy rows without action_kind are skipped.)
        let is_edit_target = ledger.iter().any(|r| {
            r.seat == target_seat
                && r.ref_msg == Some(target_msg)
                && r.action_kind == Some(ActionKind::Edit)
        });
        if !is_edit_target {
            return Ok(0);
        }
        // Per evil-arch msg 808 + dev:1 commit 2c: read from EconomySettings so
        // co-liability penalty magnitude is live-tunable via UI.
        let settings = read_economy_settings(dir);

        let now = super::iso_now();
        let mut penalized: std::collections::HashSet<String> = std::collections::HashSet::new();

        for row in ledger.iter() {
            if row.action_kind != Some(ActionKind::Test) {
                continue;
            }
            if row.linked_edit_msg != Some(target_msg) {
                continue;
            }
            // Q2 per-seat dedupe: first Test row from a seat penalizes it once;
            // subsequent rows from the same seat are skipped.
            if !penalized.insert(row.seat.clone()) {
                continue;
            }

            let new_balance = {
                let e = snap.seats.entry(row.seat.clone()).or_insert_with(|| {
                    let mut sb = SeatBalance::default();
                    sb.balance = settings.starting_balance_copper;
                    sb
                });
                e.balance = e.balance.saturating_sub(settings.coliability_test_penalty_copper);
                // Deficit-cap interaction (spec T11): co-liability stacking past
                // deficit_cap_copper trips timed_out, mirroring apply_row's penalty
                // arm so the live snapshot matches a fresh replay.
                if e.balance <= settings.deficit_cap_copper {
                    e.timed_out = true;
                }
                e.balance
            };

            let id = snap.next_txn_id;
            snap.next_txn_id += 1;
            append_currency_transaction(dir, &LedgerRow {
                id,
                txn_type: "penalty".to_string(),
                seat: row.seat.clone(),
                amount: -settings.coliability_test_penalty_copper,
                reason: format!("co-liability — tested bad edit msg #{}", target_msg),
                ref_msg: Some(target_msg),
                balance_after: new_balance,
                escrow_id: None,
                release_turn: None,
                turn: Some(snap.turn_counter),
                action_kind: Some(ActionKind::Penalty),
                linked_edit_msg: Some(target_msg),
                at: now.clone(),
            })?;
        }

        Ok(penalized.len() as u64)
    }

    /// Phase 6 (b) — does `msg_id` correspond to the `submission_msg` of a
    /// currently-approved bounty? Used by:
    ///   (1) `handle_currency_objection` to short-circuit standard Phase 2
    ///       stake capture — when objecting to an approved bounty submission,
    ///       only the 50c objection cost lands in the pool, and the resolution
    ///       hook (`emit_bounty_clawback`) handles the real economic impact.
    ///   (2) `emit_bounty_clawback` itself at resolution time to decide
    ///       whether to fire.
    /// Reads `.vaak/open_bounties.json` (snapshot) for O(1) lookup.
    /// Architect ruling msg 2089 #1 ("bounty clawback SUPERSEDES standard
    /// Phase 2 escrow-clawback"), spec v3 §"Objection on approved bounty".
    pub fn is_approved_bounty_submission(dir: &str, msg_id: u64) -> Option<BountyRow> {
        let snap = match read_open_bounties_snapshot(dir) {
            Ok(s) => s,
            Err(_) => return None,
        };
        for (_id, row) in snap.bounties.iter() {
            if row.status == "approved" && row.submission_msg == Some(msg_id) {
                return Some(row.clone());
            }
        }
        None
    }

    /// Phase 6 (b) — Bounty-objection clawback hook.
    /// Fires from `currency_judge_ruling` and `currency_concede` when the
    /// challenger effectively wins AND `dispute.target_msg` matches an
    /// approved bounty's `submission_msg`. Per spec v3 §"Objection on approved
    /// bounty" + architect ruling 50/50 split:
    ///   - Total clawback = bounty.amount × BOUNTY_OBJECTION_CLAWBACK_PERCENT / 100 (90%)
    ///   - Debit claimant's balance by clawback
    ///   - Split: 50% credited to challenger, 50% pool-destroyed (audit)
    ///   - Bounty row appended with status="rejected", last_rejection_reason
    /// Returns clawback amount (0 if not a bounty submission). Caller holds
    /// the currency+board lock and writes the snapshot afterward.
    pub fn emit_bounty_clawback(
        dir: &str,
        snap: &mut BalancesSnapshot,
        target_msg: u64,
        challenger_seat: &str,
        dispute_id: &str,
    ) -> Result<i64, String> {
        let bounty = match is_approved_bounty_submission(dir, target_msg) {
            Some(b) => b,
            None => return Ok(0),
        };
        let claimant = match bounty.claimant.as_deref() {
            Some(c) => c.to_string(),
            None => return Ok(0), // approved bounty with no claimant is malformed; skip
        };

        let clawback = bounty.amount.saturating_mul(BOUNTY_OBJECTION_CLAWBACK_PERCENT as i64) / 100;
        let challenger_share = clawback / 2;
        let destroyed_share = clawback - challenger_share; // covers odd cents

        let now = super::iso_now();

        // 1. Debit the claimant by full clawback.
        let claimant_after = {
            let e = snap.seats.entry(claimant.clone()).or_insert_with(|| {
                let mut sb = SeatBalance::default();
                sb.balance = STARTING_BALANCE_COPPER;
                sb
            });
            e.balance = e.balance.saturating_sub(clawback);
            if e.balance <= DEFICIT_CAP_COPPER {
                e.timed_out = true;
            }
            e.balance
        };
        let id = snap.next_txn_id;
        snap.next_txn_id += 1;
        append_currency_transaction(dir, &LedgerRow {
            id,
            txn_type: "bounty_clawback".to_string(),
            seat: claimant.clone(),
            amount: -clawback,
            reason: format!(
                "bounty {} clawback ({}% on objection from {})",
                bounty.id, BOUNTY_OBJECTION_CLAWBACK_PERCENT, dispute_id
            ),
            ref_msg: Some(target_msg),
            balance_after: claimant_after,
            escrow_id: None,
            release_turn: None,
            turn: Some(snap.turn_counter),
            action_kind: Some(ActionKind::BountyClawback),
            linked_edit_msg: None,
            at: now.clone(),
        })?;

        // 2. Credit the challenger half the clawback.
        if challenger_share > 0 {
            let challenger_after = {
                let e = snap.seats.entry(challenger_seat.to_string()).or_insert_with(|| {
                    let mut sb = SeatBalance::default();
                    sb.balance = STARTING_BALANCE_COPPER;
                    sb
                });
                e.balance = e.balance.saturating_add(challenger_share);
                e.balance
            };
            let cid = snap.next_txn_id;
            snap.next_txn_id += 1;
            append_currency_transaction(dir, &LedgerRow {
                id: cid,
                txn_type: "credit".to_string(),
                seat: challenger_seat.to_string(),
                amount: challenger_share,
                reason: format!(
                    "bounty {} clawback share ({} of {} from {})",
                    bounty.id, challenger_share, clawback, dispute_id
                ),
                ref_msg: Some(target_msg),
                balance_after: challenger_after,
                escrow_id: None,
                release_turn: None,
                turn: Some(snap.turn_counter),
                action_kind: Some(ActionKind::Credit),
                linked_edit_msg: None,
                at: now.clone(),
            })?;
        }

        // 3. Audit the destroyed share (no balance change; seat="system:pool").
        if destroyed_share > 0 {
            let did = snap.next_txn_id;
            snap.next_txn_id += 1;
            append_currency_transaction(dir, &LedgerRow {
                id: did,
                txn_type: "pool_destroyed".to_string(),
                seat: "system:pool".to_string(),
                amount: -destroyed_share,
                reason: format!(
                    "bounty {} clawback destroyed share ({} of {} from {})",
                    bounty.id, destroyed_share, clawback, dispute_id
                ),
                ref_msg: Some(target_msg),
                balance_after: 0,
                escrow_id: None,
                release_turn: None,
                turn: Some(snap.turn_counter),
                action_kind: Some(ActionKind::PoolDestroyed),
                linked_edit_msg: None,
                at: now.clone(),
            })?;
        }

        // 4. Append a bounty row marking the bounty rejected via objection. The
        // bounty does NOT reopen (objection-rejected is terminal; the work was
        // already paid+clawed-back).
        let mut updated = bounty.clone();
        updated.status = "rejected".to_string();
        updated.last_rejection_reason = Some(format!("objection sustained ({})", dispute_id));
        updated.resolved_at = Some(now.clone());
        append_bounty_row(dir, &updated)?;
        let mut bounties = read_open_bounties_snapshot(dir).unwrap_or_default();
        bounties.bounties.insert(updated.id.clone(), updated);
        write_open_bounties_snapshot(dir, &bounties)?;

        Ok(clawback)
    }

    /// Phase 6 (b) — Bounty expiration sweep called from `process_tick` after
    /// passive income. For each open or claimed bounty whose `deadline_turn <=
    /// snap.turn_counter`:
    ///   - `claimed`: claimant loses FULL stake. Audit `bounty_expire` row
    ///     against system:pool (balance was already debited at claim; no
    ///     additional balance change). Status → "expired".
    ///   - `open`: no penalty; status → "expired".
    /// Returns count of bounties expired this tick. MUST be called inside the
    /// currency+board lock; mutates `snap` (only next_txn_id) and writes
    /// bounties.jsonl + open_bounties.json snapshot.
    pub fn expire_overdue_bounties(
        dir: &str,
        snap: &mut BalancesSnapshot,
    ) -> Result<u64, String> {
        let mut bounties = read_open_bounties_snapshot(dir)?;
        let turn = snap.turn_counter;
        // Snapshot the keys; we may mutate the map.
        let candidates: Vec<String> = bounties
            .bounties
            .iter()
            .filter(|(_, b)| {
                (b.status == "open" || b.status == "claimed") && b.deadline_turn <= turn
            })
            .map(|(id, _)| id.clone())
            .collect();
        if candidates.is_empty() {
            return Ok(0);
        }
        let now = super::iso_now();
        let mut count: u64 = 0;
        for id in candidates {
            let bounty = match bounties.bounties.get(&id).cloned() {
                Some(b) => b,
                None => continue,
            };
            if bounty.status == "claimed" {
                let stake = bounty.claim_stake;
                if stake > 0 {
                    let tid = snap.next_txn_id;
                    snap.next_txn_id += 1;
                    append_currency_transaction(
                        dir,
                        &LedgerRow {
                            id: tid,
                            txn_type: "bounty_expire".to_string(),
                            seat: "system:pool".to_string(),
                            amount: -stake,
                            reason: format!(
                                "bounty {} expired @turn {} — claimant {} stake destroyed",
                                bounty.id,
                                turn,
                                bounty.claimant.as_deref().unwrap_or("?")
                            ),
                            ref_msg: None,
                            balance_after: 0,
                            escrow_id: None,
                            release_turn: None,
                            turn: Some(turn),
                            action_kind: Some(ActionKind::BountyExpire),
                            linked_edit_msg: None,
                            at: now.clone(),
                        },
                    )?;
                }
            }
            let mut updated = bounty.clone();
            updated.status = "expired".to_string();
            updated.resolved_at = Some(now.clone());
            append_bounty_row(dir, &updated)?;
            bounties.bounties.insert(updated.id.clone(), updated);
            count += 1;
        }
        write_open_bounties_snapshot(dir, &bounties)?;
        Ok(count)
    }

    /// Classify a project_send into Pass / Speak / Exempt per spec §
    /// "Pass classification" (ui-architect:2 msg 1073 anchored fix folded
    /// in architect:0 msg 1075 ruling 1):
    ///   Exempt = sender starts with "human:"
    ///   Pass   = type=="status" AND ( body.trim().chars().count() < 100
    ///                                  OR body.trim().to_lowercase().starts_with("pass")
    ///                                  OR subject.eq_ignore_ascii_case("passing") )
    ///   Speak  = everything else
    /// Phase 4 (a): `resolved_to_edit` is computed by the CALLER (vaak-mcp
    /// project_send) — it reads the ledger and reports whether the send's
    /// linked_edit_msg points at a real `action_kind == Edit` row. Keeping that
    /// I/O in the caller leaves this fn pure + unit-testable (Q3 ruling).
    ///
    /// Precedence is the early-return ORDER: Exempt > Pass > Edit > Test > Speak
    /// (architect ruling v3, per dev-challenger:0 1897 + developer:0 1901). T18
    /// is the regression guard — flipping any two blocks fails it.
    pub fn classify_action(
        from: &str,
        msg_type: &str,
        subject: &str,
        body: &str,
        resolved_to_edit: bool,
    ) -> ActionKind {
        if from.starts_with("human:") {
            return ActionKind::Exempt;
        }
        if msg_type == "status" {
            // Commit D (2026-05-24, architect msg 180 RULING 3): narrow PASS
            // qualifier — joint AND between subject_in_whitelist AND
            // body_in_whitelist. The bare body_len<PASS_BODY_LEN_THRESHOLD
            // short-circuit is GONE — length is not signal, substance template
            // is. Both sides of the message must match a canonical pass
            // pattern; anything else falls through to SPEAK (or Edit/Test
            // arms if applicable).
            //
            // Subject whitelist: empty OR "pass" / "passing" / "pass." / "passing."
            //   (case-insensitive)
            // Body whitelist:    empty OR matches body_matches_pass_template:
            //   - "pass" / "passing" (with optional trailing period)
            //   - "Read msg <N> [...] passing[.]"
            //   - "Read msg <N> [...] no add (from|on) [...] -lane[.]"
            //
            // Anti-pattern killer: ("passing", "No add. Standing by for human
            // restart.") now classifies SPEAK — body fails whitelist.
            let subject_in_whitelist = is_subject_pass_whitelist(subject);
            let body_in_whitelist = is_body_pass_whitelist(body);
            if subject_in_whitelist && body_in_whitelist {
                return ActionKind::Pass;
            }
        }
        // Phase 4 (a): Edit before Test (precedence). Edit = explicit type,
        // "[edit]" subject, or loose "edit: …" body prefix.
        if is_edit_action(msg_type, subject, body) {
            return ActionKind::Edit;
        }
        // Test = the same shape AND a resolvable linked Edit. Orphan Tests
        // (no real Edit to certify) fall through to Speak (dev-challenger 1428).
        if is_test_action(msg_type, subject, body) && resolved_to_edit {
            return ActionKind::Test;
        }
        ActionKind::Speak
    }

    /// Phase 8 (human msg 2262): classify with PostToolUse edit-DETECTION.
    ///
    /// `has_pending_edit` is set by the caller when this seat has a pending-edit
    /// marker written by `file-op-claim.py` after a real Edit/Write/NotebookEdit
    /// tool call. A DETECTED edit is the ONLY thing that outranks the Pass arm:
    /// doing the work must pay even when the agent sends a terse "done"/"passing"
    /// status (the exact case that made the whole WORK tier inert — real commits
    /// scored as plain Speak/Pass). Self-tagged edits (`type:"edit"`, `[edit]`
    /// subject, `edit:` body) deliberately stay BELOW Pass inside
    /// `classify_action`, so a self-declared tag can NOT be used to dodge the
    /// Pass-while-disputed gate or inflate a genuine pass — detection is
    /// ungameable, self-declaration is not. This preserves every existing
    /// precedence test (T18 et al.) while making real edits beat Pass.
    ///
    /// Keeps `classify_action` a pure string-and-flag fn; the file-system peek
    /// that produces `has_pending_edit` lives in the sidecar caller.
    pub fn classify_action_detected(
        from: &str,
        msg_type: &str,
        subject: &str,
        body: &str,
        resolved_to_edit: bool,
        has_pending_edit: bool,
    ) -> ActionKind {
        // Humans are exempt regardless of any stray marker (they post via the
        // Tauri path, not this sidecar, and never run the seat hook).
        if !from.starts_with("human:") && has_pending_edit {
            return ActionKind::Edit;
        }
        classify_action(from, msg_type, subject, body, resolved_to_edit)
    }

    /// Commit D (2026-05-24, architect msg 180 RULING 3) — subject whitelist
    /// for the narrow PASS qualifier. Subject is whitelisted when it is empty
    /// OR matches `(passing|pass)[.]?` (case-insensitive, trimmed).
    fn is_subject_pass_whitelist(subject: &str) -> bool {
        let s = subject.trim();
        if s.is_empty() {
            return true;
        }
        let lower = s.to_lowercase();
        matches!(lower.as_str(), "pass" | "passing" | "pass." | "passing.")
    }

    /// Commit D (2026-05-24, architect msg 180 RULING 3) — body whitelist for
    /// the narrow PASS qualifier. Body is whitelisted when it is empty OR
    /// matches a canonical pass template:
    ///   - "pass" / "passing" (with optional trailing period)
    ///   - "Read msg <N> [...] passing[.]"
    ///   - "Read msg <N> [...] no add (from|on) [...] -lane[.]"
    /// Pure string scan (no regex dep). Case-insensitive. Period-stripped at
    /// the tail for forgiving template matching.
    fn is_body_pass_whitelist(body: &str) -> bool {
        let s = body.trim();
        if s.is_empty() {
            return true;
        }
        let lower = s.to_lowercase();
        let core = lower.trim_end_matches('.');
        // Bare "pass" / "passing" (any case, optional trailing period)
        if core == "pass" || core == "passing" {
            return true;
        }
        // "Read msg <digits>..." canonical patterns.
        if let Some(rest) = lower.strip_prefix("read msg ") {
            let digit_len = rest.chars().take_while(|c| c.is_ascii_digit()).count();
            if digit_len > 0 {
                let after_digits = &rest[digit_len..];
                // Tail must contain "passing" OR (("no add from" OR "no add on")
                // AND "-lane"). The exact regex per architect msg 180:
                //   ^read msg \d+.*passing\.?$
                //   ^read msg \d+.*no add (from|on).*-lane\.?$
                if after_digits.contains("passing") {
                    return true;
                }
                let has_no_add_lens =
                    after_digits.contains("no add from") || after_digits.contains("no add on");
                let has_lane = after_digits.contains("-lane");
                if has_no_add_lens && has_lane {
                    return true;
                }
            }
        }
        false
    }

    /// `msg_type=="edit"` OR subject "[edit]…" OR body "edit: …" (loose prefix).
    fn is_edit_action(msg_type: &str, subject: &str, body: &str) -> bool {
        msg_type == "edit"
            || subject.trim_start().to_lowercase().starts_with("[edit]")
            || body_has_action_prefix(body, "edit")
    }

    /// `msg_type=="test"` OR subject "[test]…" OR body "test: …". The
    /// linked-Edit requirement is enforced separately via `resolved_to_edit`.
    fn is_test_action(msg_type: &str, subject: &str, body: &str) -> bool {
        msg_type == "test"
            || subject.trim_start().to_lowercase().starts_with("[test]")
            || body_has_action_prefix(body, "test")
    }

    /// Matches `^\[?word\]?\s*[:—-]\s+` case-insensitively — the ergonomic
    /// "word: …" / "[word] — …" body prefix. Pure string scan (no regex dep).
    fn body_has_action_prefix(body: &str, word: &str) -> bool {
        let lower = body.trim_start().to_lowercase();
        let s = lower.strip_prefix('[').unwrap_or(&lower);
        let s = match s.strip_prefix(word) {
            Some(r) => r,
            None => return false,
        };
        let s = s.strip_prefix(']').unwrap_or(s);
        let s = s.trim_start(); // \s*
        let s = match s.strip_prefix(|c: char| c == ':' || c == '—' || c == '-') {
            Some(r) => r,
            None => return false,
        };
        // \s+ : at least one whitespace must follow the separator.
        s.starts_with(|c: char| c.is_whitespace())
    }

    // ── tester:0 acceptance units (autonomous run, human msg 2074) ──────────
    // Pure-function executed verification of the Phase 4 classifier (T1-4/17/18)
    // + Phase 6 stake/clawback constant-formula math (T4). No logic changes;
    // `matches!` used so ActionKind needs no extra derives.
    #[cfg(test)]
    mod tester_acceptance_units {
        use super::*;

        // ── Phase 4: classify_action precedence + Edit/Test detection ──
        #[test]
        fn t_human_is_exempt() {
            assert!(matches!(
                classify_action("human:0", "edit", "anything", "edit: x", true),
                ActionKind::Exempt
            ));
        }

        #[test]
        fn t_canonical_passing_subject_is_pass() {
            // Commit D (2026-05-24): replaces t_short_status_is_pass — the bare
            // body-length short-circuit is gone. PASS qualifier now requires
            // subject + body BOTH in whitelist. Canonical subject="passing"
            // with empty body classifies PASS.
            assert!(matches!(
                classify_action("dev:0", "status", "passing", "", false),
                ActionKind::Pass
            ));
        }

        #[test]
        fn t_commit_d_anti_pattern_passing_subject_substantive_body_is_speak() {
            // Architect msg 180 RULING 3 killer case: subject="passing"
            // (whitelisted) + body="No add. Standing by for human restart."
            // (NOT whitelisted) → joint AND fails → falls through to SPEAK.
            // Pre-Commit D this classified PASS under the body<100 short-circuit;
            // post-Commit D the substantive body is correctly priced.
            // Replaces t18_pass_body_beats_edit_subject (premise invalid under
            // joint AND — subject "[edit]" is not in the pass whitelist, so the
            // old "Pass shadows Edit" nuance no longer applies).
            assert!(matches!(
                classify_action(
                    "dev:0",
                    "status",
                    "passing",
                    "No add. Standing by for human restart.",
                    false
                ),
                ActionKind::Speak
            ));
        }

        #[test]
        fn t1_edit_explicit_type() {
            assert!(matches!(
                classify_action("dev:0", "edit", "fix race", "a sufficiently long body here", false),
                ActionKind::Edit
            ));
        }

        #[test]
        fn t2_edit_subject_prefix() {
            // Body MUST exceed PASS_BODY_LEN_THRESHOLD (100), else a status msg
            // classifies Pass (short) before the Edit-subject arm. (Nuance found
            // by the executed slate; pinned separately in the test below.)
            let long_body = "this is a deliberately long edit description body that exceeds one hundred characters so the short-status pass rule does not shadow the edit-subject detection";
            assert!(long_body.chars().count() >= PASS_BODY_LEN_THRESHOLD);
            assert!(matches!(
                classify_action("dev:0", "status", "[edit] fix", long_body, false),
                ActionKind::Edit
            ));
        }

        #[test]
        fn t17_edit_body_prefix() {
            let long_body = "edit: this is a deliberately long edit body that exceeds one hundred characters so it is not treated as a short status pass and reaches the edit-prefix detection arm";
            assert!(long_body.chars().count() >= PASS_BODY_LEN_THRESHOLD);
            assert!(matches!(
                classify_action("dev:0", "status", "ack", long_body, false),
                ActionKind::Edit
            ));
        }

        #[test]
        fn t_commit_d_short_body_edit_subject_classifies_edit() {
            // Post-Commit D (architect msg 180 RULING 3): a status msg with
            // subject="[edit] fix" + short body NOW classifies Edit, not Pass.
            // Reason: subject "[edit] fix" is NOT in the PASS subject whitelist
            // ({empty, pass, passing, pass., passing.}) → joint AND fails → falls
            // through to the Edit arm, which fires on `subject.starts_with("[edit]")`.
            // Pre-Commit D this classified Pass under the body<100 short-circuit;
            // the old nuance ("body must be long for Edit subject to win on
            // type=status") is gone — length is not signal under joint AND.
            // The reliable Edit path remains explicit type="edit" (t1), which
            // skips the Pass arm entirely regardless.
            assert!(matches!(
                classify_action("dev:0", "status", "[edit] fix", "short body", false),
                ActionKind::Edit
            ));
        }

        #[test]
        fn t3_test_with_resolved_link() {
            assert!(matches!(
                classify_action("dev:0", "test", "x", "certifies fix #5", true),
                ActionKind::Test
            ));
        }

        #[test]
        fn t4_orphan_test_downgrades_to_speak() {
            // type=test but resolved_to_edit=false → not a real certification → Speak.
            assert!(matches!(
                classify_action("dev:0", "test", "x", "certifies fix #5", false),
                ActionKind::Speak
            ));
        }

        #[test]
        fn t18_edit_beats_test_precedence() {
            // A message matching BOTH edit and test shapes → Edit wins (checked first).
            assert!(matches!(
                classify_action("dev:0", "edit", "[test]", "test: and edit both", true),
                ActionKind::Edit
            ));
        }

        #[test]
        fn t_speak_fallback() {
            assert!(matches!(
                classify_action("dev:0", "review", "topic", "a normal substantive review message body", false),
                ActionKind::Speak
            ));
        }

        // ── Phase 8: classify_action_detected — edit DETECTION beats Pass ──
        #[test]
        fn t_detected_edit_beats_short_status_pass() {
            // The bug we are fixing: agent edits files then sends a terse status.
            // Without detection this is Pass (+1); with detection it is Edit.
            assert!(matches!(
                classify_action_detected("dev:0", "status", "done", "done", false, true),
                ActionKind::Edit
            ));
        }

        #[test]
        fn t_detected_edit_beats_pass_body() {
            // body starts "pass" → classify_action alone returns Pass; detection
            // overrides because real file writes happened.
            assert!(matches!(
                classify_action_detected("dev:0", "status", "passing", "pass", false, true),
                ActionKind::Edit
            ));
        }

        #[test]
        fn t_no_detection_falls_through_to_classify_action() {
            // has_pending_edit=false → identical to classify_action.
            // Post-Commit D (2026-05-24): ("done", "done") is NOT a canonical
            // pass shape — subject "done" fails subject_whitelist, body "done"
            // fails body_whitelist (only "pass"/"passing"/"read msg..." patterns
            // qualify). So this falls through to SPEAK, not PASS. Pre-Commit D
            // this returned PASS via the body<100 short-circuit. The intent of
            // the test — proving has_pending_edit=false drops back to the
            // underlying classify_action result — is preserved by asserting on
            // SPEAK, the current correct classification for this shape.
            assert!(matches!(
                classify_action_detected("dev:0", "status", "done", "done", false, false),
                ActionKind::Speak
            ));
        }

        #[test]
        fn t_detected_edit_ignored_for_human() {
            // A stray marker must never reclassify a human send away from Exempt.
            assert!(matches!(
                classify_action_detected("human:0", "status", "done", "done", false, true),
                ActionKind::Exempt
            ));
        }

        // ── Phase 8: Edit earn = 25 base + 1 copper/line beyond threshold ──
        #[test]
        fn t_edit_line_bonus_math() {
            // Matches the human's "+75 edit (150 lines)" example.
            let earn = |lines: u64| EDIT_EARN_COPPER + lines.saturating_sub(EDIT_LINE_BONUS_THRESHOLD) as i64;
            assert_eq!(earn(150), 75); // 25 + (150-100)
            assert_eq!(earn(100), 25); // at threshold → base only
            assert_eq!(earn(40), 25);  // below threshold → base only (saturating)
            assert_eq!(earn(0), 25);   // self-tagged edit w/ no marker → base only
        }

        // ── body_has_action_prefix edge cases (dev-challenger msg 2053 #8) ──
        #[test]
        fn t_prefix_no_false_positive_on_editor() {
            // "editor: ..." must NOT match the "edit" action prefix.
            assert!(!body_has_action_prefix("editor: refactor", "edit"));
        }

        #[test]
        fn t_prefix_matches_dash_separator() {
            assert!(body_has_action_prefix("edit - fix it", "edit"));
        }

        #[test]
        fn t_prefix_requires_trailing_space() {
            // "edit:x" (no space after separator) must NOT match.
            assert!(!body_has_action_prefix("edit:x", "edit"));
        }

        // ── Phase 6: stake / abandon / clawback constant-formula math (T4) ──
        #[test]
        fn t4_bounty_stake_math() {
            assert_eq!(2000u64 * BOUNTY_CLAIM_STAKE_PERCENT / 100, 200);
            assert_eq!(999u64 * BOUNTY_CLAIM_STAKE_PERCENT / 100, 99);
            assert_eq!(1u64 * BOUNTY_CLAIM_STAKE_PERCENT / 100, 0); // <10c → 0 stake (documented)
        }

        #[test]
        fn t_bounty_abandon_half() {
            assert_eq!(200u64 * BOUNTY_ABANDON_LOSS_PERCENT / 100, 100);
        }

        #[test]
        fn t_bounty_objection_clawback_90pct() {
            assert_eq!(2000u64 * BOUNTY_OBJECTION_CLAWBACK_PERCENT / 100, 1800);
        }

        // ── Human msg 458 (2026-05-24): per-turn decay tax ──
        // 1% (copper-labeled) + 0.5% (silver-labeled) of TOTAL BALANCE per turn,
        // ceil-rounded, floor at DECAY_FLOOR_COPPER. See process_tick comment for
        // the total-balance-vs-denomination-bucket interpretation rationale.

        #[test]
        fn t_decay_ceil_div_helper() {
            assert_eq!(ceil_div(0, 1000), 0);
            assert_eq!(ceil_div(1, 1000), 1);   // any positive numer → at least 1
            assert_eq!(ceil_div(999, 1000), 1);
            assert_eq!(ceil_div(1000, 1000), 1);
            assert_eq!(ceil_div(1001, 1000), 2);
        }

        #[test]
        fn t_decay_constants() {
            assert_eq!(DECAY_COPPER_PCT_PER_TURN_TENTHS, 10); // 1.0%
            assert_eq!(DECAY_SILVER_PCT_PER_TURN_TENTHS, 5);  // 0.5%
            assert_eq!(DECAY_FLOOR_COPPER, 100);
        }

        // Helper that mirrors the decay math in process_tick so we can unit-test
        // the per-balance behavior without spinning up an on-disk snapshot.
        fn decay_loss_for_balance(bal: i64) -> (i64, i64) {
            if bal < DECAY_FLOOR_COPPER { return (0, 0); }
            let d = copper_to_display(bal);
            let copper_loss = ceil_div(d.copper * DECAY_COPPER_PCT_PER_TURN_TENTHS, 1000);
            let silver_loss_silvers = ceil_div(d.silver * DECAY_SILVER_PCT_PER_TURN_TENTHS, 1000);
            let silver_loss_cu = silver_loss_silvers * 100;
            let mut total = copper_loss + silver_loss_cu;
            let max_drain = (bal - DECAY_FLOOR_COPPER).max(0);
            if total > max_drain { total = max_drain; }
            let c_row = copper_loss.min(total);
            let s_row = total - c_row;
            (c_row, s_row)
        }

        #[test]
        fn t_decay_gold_only_hoarder_loses_nothing() {
            // 10000 = 1g 0s 0c → no decay (per human spec "keeps all of their gold").
            assert_eq!(decay_loss_for_balance(10_000), (0, 0));
            // 30000 = 3g 0s 0c → no decay (evil-arch msg 473 table row).
            assert_eq!(decay_loss_for_balance(30_000), (0, 0));
            // 1_500_000 = 150g 0s 0c → no decay.
            assert_eq!(decay_loss_for_balance(1_500_000), (0, 0));
        }

        #[test]
        fn t_decay_evil_arch_msg_473_table() {
            // Per evil-arch msg 473 corrected table:
            //  10,500 = 1g 5s 0c   → 0c copper + 100c silver = 100c
            //  12,345 = 1g 23s 45c → 1c copper + 100c silver = 101c
            //  9,999  = 0g 99s 99c → 1c copper + 100c silver = 101c
            assert_eq!(decay_loss_for_balance(10_500), (0, 100));
            assert_eq!(decay_loss_for_balance(12_345), (1, 100));
            assert_eq!(decay_loss_for_balance(9_999),  (1, 100));
        }

        #[test]
        fn t_decay_max_per_seat_per_turn_is_101() {
            // Per evil-arch's math: max bucket residual = 99s 99c regardless of
            // gold portion. ceil(99×0.005)=1s, ceil(99×0.01)=1c → 101c total.
            let max_residual_balance = 99 * 100 + 99; // 9999c
            assert_eq!(decay_loss_for_balance(max_residual_balance), (1, 100));
            // Adding any amount of gold doesn't change max decay.
            assert_eq!(decay_loss_for_balance(50_000 + 9999), (1, 100));
        }

        #[test]
        fn t_decay_floor_protects_small_balances() {
            // Below floor → no decay.
            assert_eq!(decay_loss_for_balance(DECAY_FLOOR_COPPER - 1), (0, 0));
            assert_eq!(decay_loss_for_balance(50), (0, 0));
            // Floor itself is NOT decayed (treated as the protection threshold).
            assert_eq!(decay_loss_for_balance(DECAY_FLOOR_COPPER), (0, 0));
        }

        #[test]
        fn t_decay_floor_clamp_caps_drain() {
            // Balance = floor + 1 (101c) has 1g=0, silver=1, copper=1.
            // Raw decay would be ceil(1×0.01)=1c copper + ceil(1×0.005)=1s=100c → 101c.
            // But max_drain = 1c. So decay clamps to 1c (copper-row only; silver-row=0).
            assert_eq!(decay_loss_for_balance(DECAY_FLOOR_COPPER + 1), (1, 0));
        }

        // ── Commit E (2026-05-24): interest-stacking exploit closed ──
        // Architect ruling msg 180 + evil-arch msg 172 + dev:1 msg 175.
        // With INTEREST_PER_10_COPPER_HELD = 0, even an escrow item at the
        // INTEREST_MIN_HELD_COPPER threshold (10cu) generates zero interest
        // per tick. Stacking N concurrent eligible escrows therefore also
        // generates zero. This re-aligns SPEAK net P&L with the spec intent
        // (penalty escrow, NOT a savings instrument): -10 hold, +10 refund
        // at maturity, 0 interest = net 0 cu (was net +20 cu pre-fix).
        #[test]
        fn t_commit_e_interest_per_10_copper_held_is_zero() {
            assert_eq!(INTEREST_PER_10_COPPER_HELD, 0);
        }

        #[test]
        fn t_commit_e_threshold_escrow_yields_zero_interest_per_tick() {
            // Mirrors the formula at collab.rs:4434-4438 against a single
            // threshold-amount escrow item.
            let amount: i64 = INTEREST_MIN_HELD_COPPER; // 10
            let interest_per_tick = (amount / 10) * INTEREST_PER_10_COPPER_HELD;
            assert_eq!(interest_per_tick, 0);
        }

        #[test]
        fn t_commit_e_stacked_speak_escrows_yield_zero_interest() {
            // Pre-fix, a seat holding 5 concurrent SPEAK escrows (each 10cu)
            // earned 5cu per tick × 20 ticks = +100cu pure profit.
            // Post-fix, the sum is 0 regardless of count.
            let amount: i64 = INTEREST_MIN_HELD_COPPER;
            let stacked: i64 = (0..5)
                .map(|_| (amount / 10) * INTEREST_PER_10_COPPER_HELD)
                .sum();
            assert_eq!(stacked, 0);
        }

        #[test]
        fn t_commit_e_speak_net_pnl_over_20_ticks_is_neutral() {
            // Net P&L of one SPEAK escrow over its full 20-tick window:
            //   = -SPEAK_EARN_COPPER (hold) + 0 (interest accrued) + SPEAK_EARN_COPPER (refund)
            //   = 0
            // Pre-fix this was +20 (the bug); post-fix it is neutral.
            let interest_over_window: i64 =
                (0..SPEAK_ESCROW_TICKS as i64).map(|_| (SPEAK_EARN_COPPER / 10) * INTEREST_PER_10_COPPER_HELD).sum();
            let net_pnl: i64 = -SPEAK_EARN_COPPER + interest_over_window + SPEAK_EARN_COPPER;
            assert_eq!(net_pnl, 0);
        }

        // ── Commit D (2026-05-24): classifier retune to joint AND ──
        // Architect ruling msg 180 RULING 3 + evil-arch msg 177 + tester msg
        // 147 fixture set + dev:1 msg 175 finding #3. PASS qualifier is now a
        // joint AND between subject_in_whitelist AND body_in_whitelist; the
        // body<PASS_BODY_LEN_THRESHOLD short-circuit is GONE. Length is not
        // signal — substance template is.

        #[test]
        fn t_commit_d_subject_whitelist_unit() {
            assert!(is_subject_pass_whitelist(""));
            assert!(is_subject_pass_whitelist("passing"));
            assert!(is_subject_pass_whitelist("Passing"));
            assert!(is_subject_pass_whitelist("PASSING"));
            assert!(is_subject_pass_whitelist("pass"));
            assert!(is_subject_pass_whitelist("pass."));
            assert!(is_subject_pass_whitelist("passing."));
            assert!(is_subject_pass_whitelist("  passing  ")); // trimmed
            // Substantive subjects must NOT be whitelisted.
            assert!(!is_subject_pass_whitelist("passing the mic"));
            assert!(!is_subject_pass_whitelist("status update"));
            assert!(!is_subject_pass_whitelist("[edit] fix"));
            assert!(!is_subject_pass_whitelist("s"));
            assert!(!is_subject_pass_whitelist("re: topic"));
        }

        #[test]
        fn t_commit_d_body_whitelist_unit() {
            assert!(is_body_pass_whitelist(""));
            assert!(is_body_pass_whitelist("pass"));
            assert!(is_body_pass_whitelist("Pass."));
            assert!(is_body_pass_whitelist("passing"));
            assert!(is_body_pass_whitelist("passing."));
            assert!(is_body_pass_whitelist("PASSING."));
            // Canonical "Read msg <N>...passing." pattern.
            assert!(is_body_pass_whitelist("Read msg 162 from architect:0. Passing."));
            // Canonical "Read msg <N>...no add from <lens>-lane." pattern.
            assert!(is_body_pass_whitelist(
                "Read msg 162 from architect:0. No add from developer-lane."
            ));
            assert!(is_body_pass_whitelist(
                "Read msg 100 from x. No add on test-lane."
            ));
            // Anti-pattern bodies must NOT be whitelisted.
            assert!(!is_body_pass_whitelist("No add. Standing by for human restart."));
            assert!(!is_body_pass_whitelist("Wrote project_X. Standing by."));
            assert!(!is_body_pass_whitelist("alive"));
            assert!(!is_body_pass_whitelist("agreed"));
            assert!(!is_body_pass_whitelist("ok"));
            // "Read msg" without canonical tail → SPEAK.
            assert!(!is_body_pass_whitelist(
                "Read msg 5 from x. Disagree with conclusion."
            ));
        }

        #[test]
        fn t_commit_d_canonical_pass_shapes() {
            // ("passing", "") → PASS
            assert!(matches!(
                classify_action("dev:0", "status", "passing", "", false),
                ActionKind::Pass
            ));
            // ("", "Pass.") → PASS
            assert!(matches!(
                classify_action("dev:0", "status", "", "Pass.", false),
                ActionKind::Pass
            ));
            // ("passing", "Read msg 162 from architect:0. No add from developer-lane.") → PASS
            assert!(matches!(
                classify_action(
                    "dev:0",
                    "status",
                    "passing",
                    "Read msg 162 from architect:0. No add from developer-lane.",
                    false
                ),
                ActionKind::Pass
            ));
            // ("", "") — degenerate both-empty case → PASS (whitelist coverage).
            assert!(matches!(
                classify_action("dev:0", "status", "", "", false),
                ActionKind::Pass
            ));
        }

        #[test]
        fn t_commit_d_anti_pattern_substantive_status_is_speak() {
            // tester msg 147 fixture: SPEAK-class cases that escape the new
            // whitelist (substantive bodies fail body_whitelist).
            assert!(matches!(
                classify_action(
                    "dev:0",
                    "status",
                    "passing",
                    "Wrote project_X. Standing by.",
                    false
                ),
                ActionKind::Speak
            ));
            assert!(matches!(
                classify_action("dev:0", "status", "status", "alive", false),
                ActionKind::Speak
            ));
            assert!(matches!(
                classify_action("dev:0", "status", "tester:0 active", "", false),
                ActionKind::Speak
            ));
            assert!(matches!(
                classify_action("dev:0", "status", "re: topic", "agreed", false),
                ActionKind::Speak
            ));
        }

        #[test]
        fn t_commit_d_msg_110_113_115_anti_pattern_is_speak() {
            // The killer case architect msg 180 used to invalidate evil-arch's
            // option (ii). subject="passing" (whitelisted) but
            // body="No add. Standing by for human restart." (NOT whitelisted)
            // → joint AND fails → SPEAK. Under the pre-Commit D rule, this
            // classified PASS via the body<100 short-circuit; Commit D closes
            // the loophole.
            assert!(matches!(
                classify_action(
                    "dev:0",
                    "status",
                    "passing",
                    "No add. Standing by for human restart.",
                    false
                ),
                ActionKind::Speak
            ));
        }

        #[test]
        fn t_commit_d_edit_arm_still_fires_when_pass_fails() {
            // Regression guard: when joint AND fails, Edit/Test arms still
            // fire correctly. status + "[edit] fix" subject → not in subject
            // whitelist → falls through to Edit arm.
            assert!(matches!(
                classify_action(
                    "dev:0",
                    "status",
                    "[edit] fix",
                    "real edit description with enough body",
                    false
                ),
                ActionKind::Edit
            ));
            // Speak fallback when nothing matches any arm.
            assert!(matches!(
                classify_action("dev:0", "review", "thoughts", "long review body", false),
                ActionKind::Speak
            ));
        }

        // ── Tester msg 790 (2026-05-25): T-conservation property tests ──
        //
        // Invariant: total system copper (Σ seat.balance + Σ seat.escrow_held
        // across all seats) equals starting state plus net per-row deltas
        // applied via `apply_row`. Catches double-credit, silent-loss, and
        // wrong-arm-routing bugs in the apply dispatcher that per-action unit
        // tests (above) miss because they only check single rows in isolation.
        //
        // Class of bug these tests target: live ledger empirical (~7.1%
        // session inflation observed at msg 287) was caused by a divergence
        // between expected-from-source and runtime-from-binary behavior.
        // The conservation invariant catches drift the moment it appears
        // because Σ is a structural property, not a per-call assertion.

        fn total_system_copper(snap: &BalancesSnapshot) -> i64 {
            snap.seats
                .values()
                .map(|s| s.balance + s.escrow_held)
                .sum()
        }

        fn ledger_row(id: u64, txn_type: &str, seat: &str, amount: i64) -> LedgerRow {
            LedgerRow {
                id,
                txn_type: txn_type.to_string(),
                seat: seat.to_string(),
                amount,
                reason: String::new(),
                balance_after: 0,
                ..Default::default()
            }
        }

        #[test]
        fn t_conservation_init_mints() {
            let mut snap = BalancesSnapshot::default();
            assert_eq!(total_system_copper(&snap), 0);
            let row = ledger_row(1, "init", "a:0", STARTING_BALANCE_COPPER);
            apply_row(&mut snap, &row).unwrap();
            assert_eq!(total_system_copper(&snap), STARTING_BALANCE_COPPER);
        }

        #[test]
        fn t_conservation_speak_escrow_lifecycle_full_cycle() {
            // SPEAK lifecycle apply_row effects:
            //   escrow_hold: balance unchanged, escrow_held += |amount| → system +N
            //   credit:      balance += amount                          → system +N
            //   escrow_release: escrow_held -= amount                   → system -N
            // Net per Speak: system gains exactly SPEAK_EARN_COPPER. The
            // earn-net should equal the constant the spec promises.
            let mut snap = BalancesSnapshot::default();
            snap.seats.entry("a:0".to_string()).or_default();
            let baseline = total_system_copper(&snap);
            let n = SPEAK_EARN_COPPER;

            // escrow_hold: amount stored negative, escrow_held += abs(amount)
            let hold = LedgerRow {
                id: 1,
                txn_type: "escrow_hold".to_string(),
                seat: "a:0".to_string(),
                amount: -n,
                escrow_id: Some("e1".to_string()),
                release_turn: Some(SPEAK_ESCROW_TICKS),
                ..Default::default()
            };
            apply_row(&mut snap, &hold).unwrap();
            assert_eq!(total_system_copper(&snap), baseline + n);

            // credit: balance += amount
            let credit = ledger_row(2, "credit", "a:0", n);
            apply_row(&mut snap, &credit).unwrap();
            assert_eq!(total_system_copper(&snap), baseline + 2 * n);

            // escrow_release: escrow_held -= amount
            let release = LedgerRow {
                id: 3,
                txn_type: "escrow_release".to_string(),
                seat: "a:0".to_string(),
                amount: n,
                escrow_id: Some("e1".to_string()),
                ..Default::default()
            };
            apply_row(&mut snap, &release).unwrap();
            assert_eq!(total_system_copper(&snap), baseline + n);
        }

        #[test]
        fn t_conservation_penalty_destroys() {
            let mut snap = BalancesSnapshot::default();
            snap.seats.entry("a:0".to_string()).or_default().balance = 1000;
            let baseline = total_system_copper(&snap);
            let row = ledger_row(1, "penalty", "a:0", -50);
            apply_row(&mut snap, &row).unwrap();
            assert_eq!(total_system_copper(&snap), baseline - 50);
        }

        #[test]
        fn t_conservation_pool_destroyed_is_audit_only() {
            // pool_destroyed rows are audit markers — they MUST NOT mutate
            // real-seat balances. Regression guard: if a future apply_row
            // arm accidentally adds the amount to system:pool's balance, the
            // total-system invariant would falsely shrink even though no
            // real seat lost copper.
            let mut snap = BalancesSnapshot::default();
            snap.seats.entry("a:0".to_string()).or_default().balance = 1000;
            let baseline = total_system_copper(&snap);

            let row = LedgerRow {
                id: 1,
                txn_type: "pool_destroyed".to_string(),
                seat: "system:pool".to_string(),
                amount: -100,
                ..Default::default()
            };
            apply_row(&mut snap, &row).unwrap();
            assert_eq!(snap.seats.get("a:0").unwrap().balance, 1000);
            // system:pool seat entry exists with balance 0; conservation
            // unchanged because no real flow happened.
            assert_eq!(total_system_copper(&snap), baseline);
        }

        #[test]
        fn t_conservation_human_adjust_credit_and_debit() {
            // Human msg 458 — human-issued credits and debits flow through
            // apply_row as plain balance mutations. Conservation tracks them
            // as external-pool flows in either direction.
            let mut snap = BalancesSnapshot::default();
            snap.seats.entry("a:0".to_string()).or_default().balance = 500;
            let baseline = total_system_copper(&snap);

            apply_row(&mut snap, &ledger_row(1, "human_adjust", "a:0", 1000)).unwrap();
            assert_eq!(total_system_copper(&snap), baseline + 1000);

            apply_row(&mut snap, &ledger_row(2, "human_adjust", "a:0", -300)).unwrap();
            assert_eq!(total_system_copper(&snap), baseline + 700);
        }

        #[test]
        fn t_conservation_decay_destroys() {
            let mut snap = BalancesSnapshot::default();
            snap.seats.entry("a:0".to_string()).or_default().balance = 10000;
            let baseline = total_system_copper(&snap);
            let row = ledger_row(1, "decay", "a:0", -100);
            apply_row(&mut snap, &row).unwrap();
            assert_eq!(total_system_copper(&snap), baseline - 100);
        }

        #[test]
        fn t_conservation_multi_seat_speak_session() {
            // 3 seats each init + 1 SPEAK lifecycle. Verifies the invariant
            // composes across seats and rows — analogous to a single tick's
            // worth of live activity. Catches per-seat state leakage between
            // apply_row calls.
            let mut snap = BalancesSnapshot::default();
            let seats = ["a:0", "b:0", "c:0"];
            let mut id: u64 = 1;
            for seat in &seats {
                apply_row(&mut snap, &ledger_row(id, "init", seat, STARTING_BALANCE_COPPER)).unwrap();
                id += 1;
            }
            let after_init = total_system_copper(&snap);
            assert_eq!(after_init, STARTING_BALANCE_COPPER * seats.len() as i64);

            let n = SPEAK_EARN_COPPER;
            for (i, seat) in seats.iter().enumerate() {
                let esc = format!("esc_{:03x}", i);
                apply_row(&mut snap, &LedgerRow {
                    id, txn_type: "escrow_hold".to_string(), seat: seat.to_string(),
                    amount: -n, escrow_id: Some(esc.clone()), ..Default::default()
                }).unwrap();
                id += 1;
                apply_row(&mut snap, &ledger_row(id, "credit", seat, n)).unwrap();
                id += 1;
                apply_row(&mut snap, &LedgerRow {
                    id, txn_type: "escrow_release".to_string(), seat: seat.to_string(),
                    amount: n, escrow_id: Some(esc), ..Default::default()
                }).unwrap();
                id += 1;
            }
            // Each seat gained exactly n. Total system: 3 inits + 3 speak earns.
            assert_eq!(total_system_copper(&snap), after_init + n * seats.len() as i64);
        }

        #[test]
        fn t_conservation_init_rejects_double() {
            // The init invariant is the foundation of the conservation
            // property: exactly ONE init per seat over the ledger's lifetime.
            // A duplicate would silently double-mint. This test enforces the
            // HARD ERROR per dev-challenger:0 msg 1080 nit #2.
            let mut snap = BalancesSnapshot::default();
            let row1 = ledger_row(1, "init", "a:0", STARTING_BALANCE_COPPER);
            apply_row(&mut snap, &row1).unwrap();
            let row2 = ledger_row(2, "init", "a:0", STARTING_BALANCE_COPPER);
            let result = apply_row(&mut snap, &row2);
            assert!(result.is_err(), "duplicate init must be HARD ERROR");
            // Even on error, snap state stays at the first init's value.
            assert_eq!(total_system_copper(&snap), STARTING_BALANCE_COPPER);
        }

        #[test]
        fn t_conservation_bounty_stake_then_earn_full_cycle() {
            // bounty_stake debits claimant (system loses N).
            // bounty_earn later credits claimant the full payout amount.
            // For a 2000c bounty with 10% stake: stake=200, earn=2000+200=2200
            // (returns stake + bounty amount per Phase 6 spec).
            // Net seat delta = +2000 (the bounty amount).
            let mut snap = BalancesSnapshot::default();
            snap.seats.entry("a:0".to_string()).or_default().balance = 1000;
            let baseline = total_system_copper(&snap);

            // bounty_stake: amount=-200 (debit)
            apply_row(&mut snap, &ledger_row(1, "bounty_stake", "a:0", -200)).unwrap();
            assert_eq!(total_system_copper(&snap), baseline - 200);

            // bounty_earn: amount=+2200 (returns stake + earn)
            apply_row(&mut snap, &ledger_row(2, "bounty_earn", "a:0", 2200)).unwrap();
            assert_eq!(total_system_copper(&snap), baseline + 2000);
        }

        #[test]
        fn t_conservation_bounty_clawback_destroys() {
            // bounty_clawback on objection-sustained removes copper from
            // claimant's balance. Conservation: system shrinks by clawback
            // amount (offset by challenger credit + pool destroy in real
            // flow, but apply_row at the per-row level just sees the debit).
            let mut snap = BalancesSnapshot::default();
            snap.seats.entry("a:0".to_string()).or_default().balance = 2200;
            let baseline = total_system_copper(&snap);

            apply_row(&mut snap, &ledger_row(1, "bounty_clawback", "a:0", -1800)).unwrap();
            assert_eq!(total_system_copper(&snap), baseline - 1800);
            assert_eq!(snap.seats.get("a:0").unwrap().balance, 400);
        }

        #[test]
        fn t_conservation_reinstate_resets_state() {
            // Phase 2 (c) — reinstate sets balance to row.amount (0 per
            // human directive), clears timed_out + escrow_items + ban.
            // Conservation: system delta = row.amount - prior_total_for_seat.
            let mut snap = BalancesSnapshot::default();
            let s = snap.seats.entry("a:0".to_string()).or_default();
            s.balance = -1500; // deficit
            s.timed_out = true;
            s.escrow_held = 500;
            s.escrow_items.push(EscrowItem {
                id: "e1".to_string(),
                amount: 500,
                release_turn: 10,
                action: "speak".to_string(),
                ref_msg: None,
            });
            let baseline = total_system_copper(&snap); // -1000

            apply_row(&mut snap, &ledger_row(1, "reinstate", "a:0", 0)).unwrap();
            let s = snap.seats.get("a:0").unwrap();
            assert_eq!(s.balance, 0);
            assert_eq!(s.escrow_held, 0);
            assert!(s.escrow_items.is_empty());
            assert!(!s.timed_out);
            // System delta: was -1000 (=-1500+500), now 0 → delta +1000.
            assert_eq!(total_system_copper(&snap), baseline + 1000);
        }

        #[test]
        fn t_conservation_unknown_txn_type_hard_errors() {
            // Regression guard on apply_row's wildcard arm. Any
            // unrecognized txn_type MUST return Err (replay-HARD-ERROR
            // semantics). Silent-accept on unknown opcodes would let
            // future schema additions go untested.
            let mut snap = BalancesSnapshot::default();
            let row = ledger_row(1, "totally_made_up_opcode", "a:0", 100);
            let result = apply_row(&mut snap, &row);
            assert!(result.is_err(), "unknown txn_type must be HARD ERROR");
            assert_eq!(total_system_copper(&snap), 0);
        }

        #[test]
        fn t_conservation_penalty_trips_deficit_cap() {
            // Penalty arm sets timed_out=true when balance drops to/below
            // DEFICIT_CAP_COPPER. Conservation: balance still drops by
            // exactly the penalty amount; the flag is orthogonal.
            let mut snap = BalancesSnapshot::default();
            snap.seats.entry("a:0".to_string()).or_default().balance = -900;
            let baseline = total_system_copper(&snap);
            apply_row(&mut snap, &ledger_row(1, "penalty", "a:0", -200)).unwrap();
            let s = snap.seats.get("a:0").unwrap();
            assert!(s.timed_out, "deficit cap should trip timed_out flag");
            assert_eq!(s.balance, -1100);
            assert_eq!(total_system_copper(&snap), baseline - 200);
        }

        // ── Tester msg 819 (2026-05-25): T-decay-property multi-tick ──
        //
        // Decay must monotonically drain balance toward DECAY_FLOOR_COPPER,
        // never below. Properties verified across simulated 200-tick runs:
        //   - Floor invariant: balance >= DECAY_FLOOR_COPPER at every tick
        //   - Monotonicity: balance(t+1) <= balance(t) for all t
        //   - Eventual convergence: high starting balance reaches floor in
        //     bounded turns (no infinite tail beyond computed steady-state)

        fn simulate_decay_tick(bal: i64) -> i64 {
            let (copper_loss, silver_loss) = decay_loss_for_balance(bal);
            bal - copper_loss - silver_loss
        }

        #[test]
        fn t_decay_floor_is_inviolable_across_many_ticks() {
            // Simulate 500 decay ticks on a starting balance of 10,000c
            // (1 gold). Floor must hold every tick.
            let mut bal = 10_000;
            for _t in 0..500 {
                bal = simulate_decay_tick(bal);
                assert!(
                    bal >= DECAY_FLOOR_COPPER,
                    "decay drained below floor: bal={}",
                    bal
                );
            }
        }

        #[test]
        fn t_decay_is_monotonically_nonincreasing() {
            // A pure tax mechanism must NEVER increase the seat's balance.
            // Property: bal(t+1) <= bal(t) for all t.
            let mut bal = 50_000;
            for _t in 0..300 {
                let prev = bal;
                bal = simulate_decay_tick(bal);
                assert!(bal <= prev, "decay INCREASED balance: {} -> {}", prev, bal);
            }
        }

        #[test]
        fn t_decay_preserves_gold_does_not_converge_to_floor() {
            // EMPIRICAL FINDING from this test cycle: decay only taxes
            // copper + silver buckets per spec (architect msg 424). A
            // balance composed entirely of gold (e.g. 100,000c = 10g 0s
            // 0c) sees ZERO decay per tick because `copper_to_display`
            // returns d.copper=0, d.silver=0 — no taxable portion.
            //
            // Property under test: gold IS a safe haven from decay
            // (intended, not a bug). Means decay alone CANNOT drain a
            // pure-gold balance to floor. This is the gold-hoarding
            // dynamic tester:0 msg 463 flagged — surfaced empirically
            // here as an acceptance-row not a bug.
            //
            // If a future change extends decay to include gold (e.g.
            // 0.1% per turn on gold tier), this test fails — that's a
            // semantic-change flag, not a regression.
            let mut bal = 100_000; // 10g 0s 0c
            for _t in 0..500 {
                bal = simulate_decay_tick(bal);
            }
            assert_eq!(bal, 100_000, "decay drained pure-gold balance (unexpected — spec says gold preserved)");
        }

        #[test]
        fn t_decay_drains_silver_and_copper_buckets_to_gold_boundary() {
            // Complementary to the gold-preservation property: a balance
            // with copper + silver components decays those components
            // away until only the gold tier (+ floor protection) remains.
            // 10,500c = 10g 5s 0c → eventually decays to 10,000c (10g).
            let mut bal = 10_500; // 10g 5s 0c
            let mut tick = 0u32;
            while bal > 10_000 && tick < 2_000 {
                bal = simulate_decay_tick(bal);
                tick += 1;
            }
            assert!(
                bal <= 10_000,
                "decay failed to drain silver/copper buckets in 2000 ticks; bal={}",
                bal
            );
            // Subsequent ticks must not drain further (gold-tier protected).
            let post_drain = bal;
            for _t in 0..100 {
                bal = simulate_decay_tick(bal);
            }
            assert_eq!(bal, post_drain, "decay drained past gold-tier boundary");
        }

        #[test]
        fn t_decay_zero_at_exact_floor() {
            // At the floor exactly, no further decay fires. Regression
            // guard for off-by-one in the `bal < DECAY_FLOOR_COPPER`
            // gate (would drain the floor to floor-1 silently).
            assert_eq!(decay_loss_for_balance(DECAY_FLOOR_COPPER), (0, 0));
            let bal = DECAY_FLOOR_COPPER;
            let after = simulate_decay_tick(bal);
            assert_eq!(after, bal, "decay drained at-floor balance");
        }

        // ── Tester msg 819 (2026-05-25): T-economy-defaults-pass-validators ──
        //
        // Regression guard: EconomySettings::default() values MUST satisfy
        // every semantic validator in main.rs::write_economy_settings_cmd
        // (lines 3845-3905 at the time of this test). If a future default
        // change drifts out of bounds, this test fails immediately at
        // cargo test, no UI-submit needed to surface the bug.
        //
        // Mirrors the 9 invariants from the consolidated list per
        // architect msg 701 + evil-arch msg 691 + tester msg 693.

        #[test]
        fn t_economy_defaults_pass_all_validators() {
            let s = EconomySettings::default();
            // 1. interest_per_10_copper_held >= 0
            assert!(s.interest_per_10_copper_held >= 0,
                "default interest_per_10_copper_held ({}) violates >=0",
                s.interest_per_10_copper_held);
            // 2. starting_balance_copper > 0
            assert!(s.starting_balance_copper > 0,
                "default starting_balance_copper ({}) violates >0",
                s.starting_balance_copper);
            // 3. all escrow_ticks_* > 0
            assert!(s.pass_escrow_ticks > 0, "pass_escrow_ticks must be >0");
            assert!(s.speak_escrow_ticks > 0, "speak_escrow_ticks must be >0");
            assert!(s.edit_escrow_ticks > 0, "edit_escrow_ticks must be >0");
            assert!(s.test_escrow_ticks > 0, "test_escrow_ticks must be >0");
            // 4. decay_floor_copper <= starting_balance_copper
            assert!(s.decay_floor_copper <= s.starting_balance_copper,
                "decay_floor_copper ({}) > starting_balance_copper ({}) — fresh seats would start below floor",
                s.decay_floor_copper, s.starting_balance_copper);
            // 5. objection_cost_copper <= starting_balance_copper / 5
            assert!(s.objection_cost_copper <= s.starting_balance_copper / 5,
                "objection_cost_copper ({}) > starting_balance_copper/5 ({}) — fresh seats can't afford an objection",
                s.objection_cost_copper, s.starting_balance_copper / 5);
            // 6. deficit_cap_copper <= 0
            assert!(s.deficit_cap_copper <= 0,
                "deficit_cap_copper ({}) must be <=0 (negative threshold for timeout)",
                s.deficit_cap_copper);
            // 7-10. percent fields in [0, 100]
            assert!(s.bounty_claim_stake_percent <= 100, "bounty_claim_stake_percent must be <=100");
            assert!(s.bounty_abandon_loss_percent <= 100, "bounty_abandon_loss_percent must be <=100");
            assert!(s.bounty_reject_loss_percent <= 100, "bounty_reject_loss_percent must be <=100");
            assert!(s.bounty_objection_clawback_percent <= 100, "bounty_objection_clawback_percent must be <=100");
            assert!(s.clawback_percent <= 100, "clawback_percent must be <=100");
        }

        #[test]
        fn t_economy_defaults_oxford_invariants() {
            // Tester msg 847 extension for commit 6f31cf8 (dev:1) which added
            // 6 Oxford fields + 3 new semantic validators in main.rs:
            //   - oxford_turn_hard_limit_secs >= oxford_turn_soft_limit_secs
            //   - oxford_default_winning_reward_copper >= 0
            //   - oxford_audience_vote_window_secs > 0
            //
            // Mirrors the consolidated validator list (architect msg 701 +
            // evil-arch msg 691 + tester msg 693 + evil-arch msg 845).
            let s = EconomySettings::default();
            assert!(
                s.oxford_turn_hard_limit_secs >= s.oxford_turn_soft_limit_secs,
                "oxford_turn_hard_limit_secs ({}) must be >= oxford_turn_soft_limit_secs ({}) — hard ceiling can't be below soft target",
                s.oxford_turn_hard_limit_secs, s.oxford_turn_soft_limit_secs
            );
            assert!(
                s.oxford_default_winning_reward_copper >= 0,
                "oxford_default_winning_reward_copper ({}) must be >= 0",
                s.oxford_default_winning_reward_copper
            );
            assert!(
                s.oxford_audience_vote_window_secs > 0,
                "oxford_audience_vote_window_secs ({}) must be > 0 — zero window = no vote possible",
                s.oxford_audience_vote_window_secs
            );
            // Additional sanity (not in main.rs validators but worth pinning):
            assert!(
                s.oxford_moderator_vacancy_timeout_secs > 0,
                "moderator vacancy timeout of 0 would auto-kill any debate the moment a moderator left"
            );
            assert!(
                s.oxford_react_rate_limit_per_min > 0,
                "react rate limit of 0 would disable all audience reactions"
            );
        }
    }

    /// Commit (c) — process ONE currency tick. MUST be called inside
    /// `with_currency_and_board_lock` (both locks held).
    ///
    /// Tick split per spec §"Tick semantics" (developer:0 msg 1069 ruling):
    ///   - turn_counter increments every tick (per-send AND per mic_advance).
    ///   - Escrow interest + escrow release run on EVERY tick (so funds aren't
    ///     trapped when assembly is off).
    ///   - Passive income runs ONLY on mic_advance ticks (`on_mic_advance`),
    ///     paid to every seat in `active_seats` ("reward for being present in
    ///     rotation"). active_seats is ignored when on_mic_advance is false.
    ///
    /// Ordering within a tick: interest (on escrow held at tick start) → release
    /// matured escrow (release_turn <= turn_counter) → passive income.
    /// balances.json is rewritten once at the end; all ledger rows appended.
    pub fn process_tick(
        dir: &str,
        on_mic_advance: bool,
        active_seats: &[String],
    ) -> Result<(), String> {
        // Human msg 657 + architect msg 671 #1: re-read settings on every tick
        // so UI-driven economy.json edits land next-tick without restart. No
        // startup caching — the file is small (< 1KB) and parse cost is < 1ms.
        let settings = read_economy_settings(dir);

        let mut snap = read_balances_snapshot(dir)?;
        if !balances_json_path(dir).exists() && currency_jsonl_path(dir).exists() {
            snap = replay_balances_from_ledger(dir)?;
        }
        let now = super::iso_now();
        snap.turn_counter = snap.turn_counter.saturating_add(1);
        let turn = snap.turn_counter;

        let mut next_id = snap.next_txn_id;
        let mut rows: Vec<LedgerRow> = Vec::new();

        let seats: Vec<String> = snap.seats.keys().cloned().collect();
        for seat in &seats {
            // --- Interest on currently-held escrow (items >= interest_min_held_copper) ---
            let interest: i64 = snap
                .seats
                .get(seat)
                .map(|e| {
                    e.escrow_items
                        .iter()
                        .filter(|it| it.amount >= settings.interest_min_held_copper)
                        .map(|it| (it.amount / 10) * settings.interest_per_10_copper_held)
                        .sum()
                })
                .unwrap_or(0);
            if interest > 0 {
                let bal = {
                    let e = snap.seats.get_mut(seat).unwrap();
                    e.balance = e.balance.saturating_add(interest);
                    e.balance
                };
                rows.push(LedgerRow {
                    id: next_id,
                    txn_type: "interest".to_string(),
                    seat: seat.clone(),
                    amount: interest,
                    reason: format!("escrow interest @turn {}", turn),
                    ref_msg: None,
                    balance_after: bal,
                    escrow_id: None,
                    release_turn: None,
                    turn: Some(turn),
                    action_kind: None,
                    linked_edit_msg: None,
                    at: now.clone(),
                });
                next_id += 1;
            }

            // --- Release matured escrow items (release_turn <= turn) ---
            let matured: Vec<EscrowItem> = {
                let e = snap.seats.get_mut(seat).unwrap();
                let (mature, keep): (Vec<EscrowItem>, Vec<EscrowItem>) =
                    e.escrow_items.drain(..).partition(|it| it.release_turn <= turn);
                e.escrow_items = keep;
                mature
            };
            for item in matured {
                let bal = {
                    let e = snap.seats.get_mut(seat).unwrap();
                    e.balance = e.balance.saturating_add(item.amount);
                    e.escrow_held = (e.escrow_held - item.amount).max(0);
                    e.balance
                };
                // credit row (funds settle into balance) ...
                rows.push(LedgerRow {
                    id: next_id,
                    txn_type: "credit".to_string(),
                    seat: seat.clone(),
                    amount: item.amount,
                    reason: format!("escrow release: {} settled @turn {}", item.action, turn),
                    ref_msg: item.ref_msg,
                    balance_after: bal,
                    escrow_id: Some(item.id.clone()),
                    release_turn: None,
                    turn: Some(turn),
                    action_kind: None,
                    linked_edit_msg: None,
                    at: now.clone(),
                });
                next_id += 1;
                // ... + escrow_release row (clears the held item)
                rows.push(LedgerRow {
                    id: next_id,
                    txn_type: "escrow_release".to_string(),
                    seat: seat.clone(),
                    amount: item.amount,
                    reason: format!("escrow matured: {} @turn {}", item.action, turn),
                    ref_msg: item.ref_msg,
                    balance_after: bal,
                    escrow_id: Some(item.id),
                    release_turn: None,
                    turn: Some(turn),
                    action_kind: None,
                    linked_edit_msg: None,
                    at: now.clone(),
                });
                next_id += 1;
            }
        }

        // --- Decay tax (human msg 458, evil-arch msg 473 corrected spec) ---
        // Every turn (NOT gated on on_mic_advance): each seat with positive
        // balance > DECAY_FLOOR_COPPER loses:
        //   copper_loss = ceil(copper_bucket × 1.0%)   bucket in 0..=99
        //   silver_loss = ceil(silver_bucket × 0.5%)   bucket in 0..=99, in SILVERS (×100 for cu)
        // Gold portion is UNTOUCHED — per human msg 420 "keeps all of their
        // gold" + architect msg 469 ratification. Max decay per seat per turn
        // is therefore 1c + (1silver=100c) = 101c regardless of balance size.
        // Gold-tier hoarders are immune to decay by design (safe-haven tier).
        //
        // Source of denomination split: existing copper_to_display() helper
        // (collab.rs:3059) — single source of truth per ui-arch msg 1071.
        //
        // Decay applies to BALANCE only (escrow_held is in-flight work,
        // protected per evil-arch msg 428 edge case). Floor at
        // DECAY_FLOOR_COPPER prevents decay from creating timeouts.
        //
        // Two ledger rows per non-zero decay (copper-labeled + silver-labeled)
        // for Flow Feed readability; zero-amount rows skipped.
        //
        // This block REPLACED an earlier total-balance interpretation that
        // evil-arch msg 473 correctly flagged as violating the gold-preserved
        // contract.
        for seat in &seats {
            let pre_bal = match snap.seats.get(seat) {
                Some(e) => e.balance,
                None => continue,
            };
            if pre_bal < settings.decay_floor_copper {
                continue; // no decay below the floor
            }
            let display = copper_to_display(pre_bal);
            // ceil(copper_bucket × pct/1000) where pct is in tenths-of-percent.
            let copper_loss_cu = ceil_div(display.copper * settings.decay_copper_pct_per_turn_tenths, 1000);
            let silver_loss_silvers = ceil_div(display.silver * settings.decay_silver_pct_per_turn_tenths, 1000);
            let silver_loss_cu = silver_loss_silvers * 100;

            let mut total_loss = copper_loss_cu + silver_loss_cu;
            if total_loss == 0 {
                continue; // gold-only hoarder, or no fractional residual
            }
            // Floor protection: never decay past decay_floor_copper.
            let max_drain = (pre_bal - settings.decay_floor_copper).max(0);
            if total_loss > max_drain {
                total_loss = max_drain;
            }
            if total_loss == 0 {
                continue;
            }
            // Apportion: copper-row first up to its slot, silver-row gets the
            // remainder. Keeps row labels honest when floor clamps the tail.
            let copper_row_amount = copper_loss_cu.min(total_loss);
            let silver_row_amount = total_loss - copper_row_amount;

            if copper_row_amount > 0 {
                let bal = {
                    let e = snap.seats.get_mut(seat).unwrap();
                    e.balance = e.balance.saturating_sub(copper_row_amount);
                    e.balance
                };
                rows.push(LedgerRow {
                    id: next_id,
                    txn_type: "decay".to_string(),
                    seat: seat.clone(),
                    amount: -copper_row_amount,
                    reason: format!("copper decay @turn {} (1.0% of {}c bucket)", turn, display.copper),
                    ref_msg: None,
                    balance_after: bal,
                    escrow_id: None,
                    release_turn: None,
                    turn: Some(turn),
                    action_kind: Some(ActionKind::Decay),
                    linked_edit_msg: None,
                    at: now.clone(),
                });
                next_id += 1;
            }
            if silver_row_amount > 0 {
                let bal = {
                    let e = snap.seats.get_mut(seat).unwrap();
                    e.balance = e.balance.saturating_sub(silver_row_amount);
                    e.balance
                };
                rows.push(LedgerRow {
                    id: next_id,
                    txn_type: "decay".to_string(),
                    seat: seat.clone(),
                    amount: -silver_row_amount,
                    reason: format!("silver decay @turn {} (0.5% of {}s bucket)", turn, display.silver),
                    ref_msg: None,
                    balance_after: bal,
                    escrow_id: None,
                    release_turn: None,
                    turn: Some(turn),
                    action_kind: Some(ActionKind::Decay),
                    linked_edit_msg: None,
                    at: now.clone(),
                });
                next_id += 1;
            }
        }

        // --- Passive income (mic_advance ticks only) ---
        if on_mic_advance {
            for seat in active_seats {
                // Lazy-init an active seat that has never sent (so it still
                // earns passive). Exactly one init row; apply_row guards dups.
                if !snap.seats.contains_key(seat) {
                    snap.seats.entry(seat.clone()).or_default().balance = settings.starting_balance_copper;
                    rows.push(LedgerRow {
                        id: next_id,
                        txn_type: "init".to_string(),
                        seat: seat.clone(),
                        amount: settings.starting_balance_copper,
                        reason: "join: initial balance (passive tick)".to_string(),
                        ref_msg: None,
                        balance_after: settings.starting_balance_copper,
                        escrow_id: None,
                        release_turn: None,
                        turn: Some(turn),
                        action_kind: Some(ActionKind::Init),
                        linked_edit_msg: None,
                        at: now.clone(),
                    });
                    next_id += 1;
                }
                let bal = {
                    let e = snap.seats.get_mut(seat).unwrap();
                    e.balance = e.balance.saturating_add(settings.passive_per_tick_copper);
                    e.balance
                };
                rows.push(LedgerRow {
                    id: next_id,
                    txn_type: "passive".to_string(),
                    seat: seat.clone(),
                    amount: settings.passive_per_tick_copper,
                    reason: format!("passive rotation tick @turn {}", turn),
                    ref_msg: None,
                    balance_after: bal,
                    escrow_id: None,
                    release_turn: None,
                    turn: Some(turn),
                    action_kind: None,
                    linked_edit_msg: None,
                    at: now.clone(),
                });
                next_id += 1;
            }
        }

        snap.next_txn_id = next_id;
        for row in &rows {
            append_currency_transaction(dir, row)?;
        }
        // Phase 6 (b) — sweep expired bounties (claimed → stake destroyed,
        // open → just marked expired). Runs after passive income so the same
        // snapshot write captures everything. Errors are logged but don't
        // abort the tick (mirroring the tick's overall best-effort posture).
        if let Err(e) = expire_overdue_bounties(dir, &mut snap) {
            eprintln!("[currency.process_tick] WARN: expire sweep error: {}", e);
        }
        write_balances_snapshot(dir, &snap)?;
        Ok(())
    }
}

// ============================================================
// Phase A — Oxford-Style Debate (spec v2.2, architect msg 510 green-light)
// Foundation skeleton: schema types + path helpers + lock primitive +
// initiate validator. MCP tool wiring, audience voting, react, gate logic
// land in follow-up commits.
// ============================================================
pub mod oxford {
    use super::atomic_write;
    use serde::{Deserialize, Serialize};
    use std::path::{Path, PathBuf};

    // ---- Constants (Phase A spec v2.2 §6.1 + §6.2) ----
    pub const OXFORD_DEFAULT_WINNING_REWARD_COPPER: i64 = 500; // 5 silver per dev:1 msg 498
    pub const OXFORD_TURN_SOFT_LIMIT_SECS: u64 = 60;            // soft limit per evil-arch msg 489 #2
    pub const OXFORD_TURN_HARD_LIMIT_SECS: u64 = 120;           // hard ceiling
    pub const OXFORD_AUDIENCE_VOTE_WINDOW_SECS: u64 = 30;       // §5
    pub const OXFORD_MODERATOR_VACANCY_TIMEOUT_SECS: u64 = 300; // §6.5 v2 new
    pub const OXFORD_REACT_RATE_LIMIT_PER_MIN: u64 = 3;         // §3.4a
    pub const OXFORD_REACT_RATE_LIMIT_WINDOW_SECS: u64 = 60;

    /// Append-only event log for an entire debate's lifecycle. Single source
    /// of truth for replay + audit. See spec §4.1 for the row shapes.
    pub fn oxford_debates_jsonl_path(dir: &str) -> PathBuf {
        Path::new(dir).join(".vaak").join("oxford-debates.jsonl")
    }

    /// Snapshot of the CURRENT active debate (one at a time per project per
    /// spec §6.4). Removed when debate ends. Single-writer per §6.5 v2 lock.
    pub fn active_oxford_path(dir: &str) -> PathBuf {
        Path::new(dir).join(".vaak").join("active-oxford-debate.json")
    }

    /// Project-wide oxford lock — gates all writes to active-oxford-debate.json
    /// and oxford-debates.jsonl. Per spec §6.5 v2: "active-oxford-debate.json
    /// writes use explicit lock file."
    pub fn oxford_lock_path(dir: &str) -> PathBuf {
        Path::new(dir).join(".vaak").join("oxford.lock")
    }

    // ---- Schema types ----

    /// A single turn entry in the debate history. `ended_at` is None for
    /// the currently-active turn.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct OxfordTurn {
        pub seat: String,
        pub started_at: String, // ISO8601
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub ended_at: Option<String>,
    }

    /// Snapshot of the currently-active Oxford debate. Atomically written
    /// after each state-changing MCP tool call. Removed (file deleted) when
    /// debate ends — readers must tolerate file-absent as "no active debate."
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct ActiveOxfordDebate {
        pub debate_id: u64,
        pub moderator: String,
        pub side_a: Vec<String>,
        pub side_b: Vec<String>,
        pub audience: Vec<String>,
        pub premise: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub current_speaker: Option<String>,
        pub started_at: String,
        #[serde(default)]
        pub turn_history: Vec<OxfordTurn>,
        /// v2.2 (per dev:1 msg 498 + human msg 497): winning-side reward
        /// configured at initiate, default OXFORD_DEFAULT_WINNING_REWARD_COPPER
        /// (500 = 5 silver). Distributed pool-funded at end on strict-majority
        /// audience vote.
        pub winning_side_reward_copper: i64,
    }

    /// Lifecycle event appended to oxford-debates.jsonl. Tagged union over
    /// the event kinds in spec §4.1.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(tag = "event", rename_all = "snake_case")]
    pub enum OxfordEvent {
        Initiate {
            debate_id: u64,
            timestamp: String,
            moderator: String,
            side_a: Vec<String>,
            side_b: Vec<String>,
            premise: String,
            audience: Vec<String>,
            winning_side_reward_copper: i64,
        },
        SpeakerDeclared { debate_id: u64, timestamp: String, seat: String },
        React { debate_id: u64, timestamp: String, seat: String, emoji: String },
        Kicked { debate_id: u64, timestamp: String, seat: String },
        AudienceVote { debate_id: u64, timestamp: String, voter: String, vote: String },
        Ended {
            debate_id: u64,
            timestamp: String,
            outcome: String, // "side_a_wins" | "side_b_wins" | "draw" | "no_winner" | "abandoned"
            audience_tally_nonhuman: Option<serde_json::Value>,
            audience_human_vote: Option<String>,
            reward_distributed: Option<i64>, // total cu distributed (None if no_winner)
        },
    }

    // ---- Lock primitive ----

    /// Project-wide oxford lock. Closure-style; reuses the existing currency
    /// lock as the underlying serialization primitive (oxford writes touch
    /// the same per-project file area, and reward-distribution at debate-end
    /// already needs currency lock). The dedicated oxford.lock path is
    /// reserved per spec §6.5 v2 for a future refactor that splits the locks
    /// if write contention becomes measurable. For now, single shared lock
    /// preserves the spec's "single-writer constraint" guarantee — no sidecar
    /// can write the oxford state outside this primitive.
    pub fn with_oxford_lock<F, R>(dir: &str, f: F) -> Result<R, String>
    where
        F: FnOnce() -> Result<R, String>,
    {
        super::with_currency_lock(dir, f)
    }

    // ---- I/O primitives ----

    pub fn read_active_oxford(dir: &str) -> Result<Option<ActiveOxfordDebate>, String> {
        let path = active_oxford_path(dir);
        if !path.exists() {
            return Ok(None);
        }
        let raw = std::fs::read_to_string(&path).map_err(|e| format!("oxford read: {}", e))?;
        if raw.trim().is_empty() || raw.trim() == "{}" {
            return Ok(None);
        }
        serde_json::from_str::<ActiveOxfordDebate>(&raw)
            .map(Some)
            .map_err(|e| format!("oxford parse: {}", e))
    }

    pub fn write_active_oxford(dir: &str, debate: &ActiveOxfordDebate) -> Result<(), String> {
        let path = active_oxford_path(dir);
        let json = serde_json::to_string_pretty(debate)
            .map_err(|e| format!("oxford serialize: {}", e))?;
        atomic_write(&path, json.as_bytes())
    }

    pub fn clear_active_oxford(dir: &str) -> Result<(), String> {
        let path = active_oxford_path(dir);
        if path.exists() {
            std::fs::remove_file(&path).map_err(|e| format!("oxford clear: {}", e))?;
        }
        Ok(())
    }

    pub fn append_oxford_event(dir: &str, event: &OxfordEvent) -> Result<(), String> {
        use std::io::Write;
        let path = oxford_debates_jsonl_path(dir);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("oxford mkdir: {}", e))?;
        }
        let line = serde_json::to_string(event).map_err(|e| format!("oxford serialize event: {}", e))?;
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| format!("oxford open append: {}", e))?;
        writeln!(f, "{}", line).map_err(|e| format!("oxford append write: {}", e))?;
        Ok(())
    }

    /// Compute the next debate_id by scanning oxford-debates.jsonl for the
    /// highest `Initiate` event's debate_id. Returns 1 if no debates yet.
    pub fn next_debate_id(dir: &str) -> u64 {
        let path = oxford_debates_jsonl_path(dir);
        if !path.exists() {
            return 1;
        }
        let raw = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(_) => return 1,
        };
        let mut max_id: u64 = 0;
        for line in raw.lines() {
            if let Ok(event) = serde_json::from_str::<OxfordEvent>(line) {
                if let OxfordEvent::Initiate { debate_id, .. } = event {
                    if debate_id > max_id {
                        max_id = debate_id;
                    }
                }
            }
        }
        max_id + 1
    }

    /// Pure validation helper (Phase A spec §3.1 gates). Returns Err with
    /// specific gate-error strings for the caller to surface. Caller is
    /// responsible for the actual write + lock acquisition.
    pub fn validate_initiate(
        caller: &str,
        moderator: &str,
        side_a: &[String],
        side_b: &[String],
        audience: &[String],
        active_seats: &[String],
    ) -> Result<(), String> {
        if side_a.is_empty() || side_b.is_empty() {
            return Err("[OxfordRequireMinOnePerSide]".to_string());
        }
        // Caller must be the moderator OR human:0
        if caller != moderator && !caller.starts_with("human:") {
            return Err("[OxfordInitiationDenied]".to_string());
        }
        // Moderator may not appear in any side or audience.
        if side_a.iter().any(|s| s == moderator) || side_b.iter().any(|s| s == moderator) || audience.iter().any(|s| s == moderator) {
            return Err("[OxfordRoleConflict]".to_string());
        }
        // No seat may appear in both side_a and side_b.
        for s in side_a {
            if side_b.iter().any(|b| b == s) {
                return Err("[OxfordRoleConflict]".to_string());
            }
            if audience.iter().any(|a| a == s) {
                return Err("[OxfordRoleConflict]".to_string());
            }
        }
        for s in side_b {
            if audience.iter().any(|a| a == s) {
                return Err("[OxfordRoleConflict]".to_string());
            }
        }
        // All non-human seats must be in the active roster.
        let in_roster = |seat: &str| seat.starts_with("human:") || active_seats.iter().any(|r| r == seat);
        if !in_roster(moderator) {
            return Err("[OxfordSeatNotInRoster]".to_string());
        }
        for s in side_a.iter().chain(side_b.iter()).chain(audience.iter()) {
            if !in_roster(s) {
                return Err("[OxfordSeatNotInRoster]".to_string());
            }
        }
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn s(v: &[&str]) -> Vec<String> {
            v.iter().map(|x| x.to_string()).collect()
        }

        #[test]
        fn t_validate_empty_side_rejected() {
            let active = s(&["architect:0", "developer:1", "evil-architect:0"]);
            let err = validate_initiate("manager:0", "manager:0", &[], &s(&["developer:1"]), &[], &active).unwrap_err();
            assert_eq!(err, "[OxfordRequireMinOnePerSide]");
        }

        #[test]
        fn t_validate_caller_not_moderator_or_human_rejected() {
            let active = s(&["architect:0", "developer:1"]);
            let err = validate_initiate(
                "developer:1",          // caller
                "manager:0",            // moderator
                &s(&["architect:0"]), &s(&["developer:1"]), &[], &active,
            ).unwrap_err();
            assert_eq!(err, "[OxfordInitiationDenied]");
        }

        #[test]
        fn t_validate_human_can_initiate_any() {
            let active = s(&["architect:0", "developer:1", "manager:0"]);
            let result = validate_initiate(
                "human:0",
                "manager:0",
                &s(&["architect:0"]), &s(&["developer:1"]), &[], &active,
            );
            assert!(result.is_ok());
        }

        #[test]
        fn t_validate_moderator_in_side_rejected() {
            let active = s(&["architect:0", "developer:1", "manager:0"]);
            let err = validate_initiate(
                "manager:0", "manager:0",
                &s(&["manager:0", "architect:0"]), &s(&["developer:1"]),
                &[], &active,
            ).unwrap_err();
            assert_eq!(err, "[OxfordRoleConflict]");
        }

        #[test]
        fn t_validate_same_seat_both_sides_rejected() {
            let active = s(&["architect:0", "developer:1", "manager:0"]);
            let err = validate_initiate(
                "manager:0", "manager:0",
                &s(&["architect:0", "developer:1"]),
                &s(&["developer:1"]), // dev:1 on both sides
                &[], &active,
            ).unwrap_err();
            assert_eq!(err, "[OxfordRoleConflict]");
        }

        #[test]
        fn t_validate_seat_not_in_roster_rejected() {
            let active = s(&["architect:0", "developer:1", "manager:0"]);
            let err = validate_initiate(
                "manager:0", "manager:0",
                &s(&["architect:0"]),
                &s(&["nobody:99"]),    // not in roster
                &[], &active,
            ).unwrap_err();
            assert_eq!(err, "[OxfordSeatNotInRoster]");
        }

        #[test]
        fn t_validate_human_audience_member_allowed() {
            let active = s(&["architect:0", "developer:1", "manager:0"]);
            let result = validate_initiate(
                "human:0", "manager:0",
                &s(&["architect:0"]), &s(&["developer:1"]),
                &s(&["human:0"]),     // human in audience
                &active,
            );
            assert!(result.is_ok());
        }

        #[test]
        fn t_next_debate_id_empty_dir_returns_one() {
            let tmp = std::env::temp_dir().join(format!("oxford_test_{}", std::process::id()));
            let _ = std::fs::create_dir_all(tmp.join(".vaak"));
            let dir_str = tmp.to_string_lossy().to_string();
            assert_eq!(next_debate_id(&dir_str), 1);
            let _ = std::fs::remove_dir_all(&tmp);
        }
    }
}

// ---- Currency lock primitives (commit a) ----
//
// Sole entry point: `with_currency_and_board_lock`. Acquires the project-wide
// `.vaak/currency.lock` as OUTER, then delegates to the section-scoped board
// lock as INNER. Closure-nest auto-LIFO release. Cross-binary parity with the
// sidecar's `with_currency_and_board_lock` in bin/vaak-mcp.rs — same outer
// path, same ordering, MUST NOT diverge.

/// Project-wide currency lock (section-independent). Closure-style; same
/// pattern as `with_board_lock` but on a fixed `.vaak/currency.lock` path.
pub fn with_currency_lock<F, R>(dir: &str, f: F) -> Result<R, String>
where
    F: FnOnce() -> Result<R, String>,
{
    let lock_path = currency::currency_lock_path(dir);
    if let Some(parent) = lock_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let lock_file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .open(&lock_path)
        .map_err(|e| format!("Failed to open currency lock file: {}", e))?;

    const LOCK_TIMEOUT_MS: u64 = 10_000;
    const LOCK_RETRY_MS: u64 = 50;

    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{LockFileEx, UnlockFileEx, LOCKFILE_EXCLUSIVE_LOCK, LOCKFILE_FAIL_IMMEDIATELY};
        use windows_sys::Win32::System::IO::OVERLAPPED;

        let handle = lock_file.as_raw_handle();
        let mut overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };
        let locked = unsafe {
            LockFileEx(handle as _, LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY, 0, u32::MAX, u32::MAX, &mut overlapped)
        };
        if locked == 0 {
            let start = std::time::Instant::now();
            loop {
                std::thread::sleep(std::time::Duration::from_millis(LOCK_RETRY_MS));
                let retry = unsafe {
                    LockFileEx(handle as _, LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY, 0, u32::MAX, u32::MAX, &mut overlapped)
                };
                if retry != 0 { break; }
                if start.elapsed().as_millis() as u64 > LOCK_TIMEOUT_MS {
                    return Err(format!(
                        "currency.lock held for >{}s — stale lock from hung process. Lock file: {}",
                        LOCK_TIMEOUT_MS / 1000, lock_path.display()
                    ));
                }
            }
        }
        let result = f();
        let mut ov2: OVERLAPPED = unsafe { std::mem::zeroed() };
        let _ = unsafe { UnlockFileEx(handle as _, 0, u32::MAX, u32::MAX, &mut ov2) };
        return result;
    }

    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        let fd = lock_file.as_raw_fd();
        let start = std::time::Instant::now();
        loop {
            let r = unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) };
            if r == 0 { break; }
            if start.elapsed().as_millis() as u64 > LOCK_TIMEOUT_MS {
                return Err(format!(
                    "currency.lock held for >{}s — stale lock. Lock file: {}",
                    LOCK_TIMEOUT_MS / 1000, lock_path.display()
                ));
            }
            std::thread::sleep(std::time::Duration::from_millis(LOCK_RETRY_MS));
        }
        let result = f();
        unsafe { libc::flock(fd, libc::LOCK_UN); }
        return result;
    }

    #[allow(unreachable_code)]
    {
        // Fallback for non-Windows/Unix targets (none in our deploy set).
        let _ = lock_file;
        f()
    }
}

/// Sole entry point for code that touches BOTH currency.jsonl/balances.json
/// AND board.jsonl/protocol.json. Acquires currency.lock as OUTER, then
/// delegates to `with_board_lock` (section-scoped) as INNER. Closure-nest
/// auto-LIFO release. MUST NOT compose `with_currency_lock` + `with_board_lock`
/// manually — that's a deadlock-by-reverse-order risk (dev-challenger:0
/// msg 1123 single-entry-point guardrail).
///
/// Cross-binary parity: `bin/vaak-mcp.rs::with_currency_and_board_lock`
/// MUST follow the same path: `.vaak/currency.lock` outer, section-scoped
/// board lock inner. See `bin/vaak-mcp.rs` for the sidecar mirror.
pub fn with_currency_and_board_lock<F, R>(dir: &str, f: F) -> Result<R, String>
where
    F: FnOnce() -> Result<R, String>,
{
    with_currency_lock(dir, || with_board_lock(dir, f))
}
