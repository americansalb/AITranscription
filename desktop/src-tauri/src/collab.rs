use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// A participant in a collaboration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Participant {
    pub role: String,
    pub session_id: String,
    pub joined_at: String,
    pub last_heartbeat: u64,
}

/// A single message in the collaboration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollabMessage {
    pub number: u32,
    pub role: String,
    pub timestamp: String,
    pub text: String,
}

/// Sidecar state (collab-state.json)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollabState {
    pub collab_id: String,
    pub project_dir: String,
    pub created_at: String,
    pub participants: Vec<Participant>,
    pub message_count: u32,
    pub messages: Vec<CollabMessage>,
    pub last_activity: String,
}

/// In-memory collaboration store, keyed by project_dir
pub struct CollabStore {
    collabs: HashMap<String, CollabState>,
}

fn now_iso() -> String {
    let dur = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs();
    // Simple ISO-ish timestamp
    let hours = (secs % 86400) / 3600;
    let mins = (secs % 3600) / 60;
    let s = secs % 60;
    format!(
        "{}T{:02}:{:02}:{:02}Z",
        chrono_date_from_epoch(secs),
        hours,
        mins,
        s
    )
}

fn chrono_date_from_epoch(epoch_secs: u64) -> String {
    // Simple date calculation
    let days = epoch_secs / 86400;
    let mut y = 1970u64;
    let mut remaining = days;
    loop {
        let days_in_year = if is_leap(y) { 366 } else { 365 };
        if remaining < days_in_year {
            break;
        }
        remaining -= days_in_year;
        y += 1;
    }
    let leap = is_leap(y);
    let month_days: [u64; 12] = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut m = 0;
    for md in &month_days {
        if remaining < *md {
            break;
        }
        remaining -= *md;
        m += 1;
    }
    format!("{}-{:02}-{:02}", y, m + 1, remaining + 1)
}

fn is_leap(y: u64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn time_short() -> String {
    let dur = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs();
    let hours = (secs % 86400) / 3600;
    let mins = (secs % 3600) / 60;
    let s = secs % 60;
    format!("{:02}:{:02}:{:02}", hours, mins, s)
}

fn generate_collab_id() -> String {
    let now = now_millis();
    let suffix: String = (0..6)
        .map(|i| {
            let idx = ((now >> (i * 5)) % 36) as u8;
            if idx < 10 {
                (b'0' + idx) as char
            } else {
                (b'a' + (idx - 10)) as char
            }
        })
        .collect();
    suffix
}

fn vaak_dir(project_dir: &str) -> PathBuf {
    Path::new(project_dir).join(".vaak")
}

fn collab_md_path(project_dir: &str) -> PathBuf {
    vaak_dir(project_dir).join("collab.md")
}

fn collab_state_path(project_dir: &str) -> PathBuf {
    vaak_dir(project_dir).join("collab-state.json")
}

impl CollabStore {
    pub fn new() -> Self {
        Self {
            collabs: HashMap::new(),
        }
    }

    /// Join or create a collaboration
    pub fn join(
        &mut self,
        session_id: &str,
        role: &str,
        project_dir: &str,
    ) -> Result<serde_json::Value, String> {
        let normalized = project_dir.replace('\\', "/");

        // Load from disk if not in memory
        if !self.collabs.contains_key(&normalized) {
            if let Some(state) = self.load_state(&normalized) {
                self.collabs.insert(normalized.clone(), state);
            }
        }

        if let Some(state) = self.collabs.get(&normalized) {
            // Check if role is already taken by a different session
            for p in &state.participants {
                if p.role == role && p.session_id != session_id {
                    return Err(format!("Role '{}' is already taken by session {}", role, p.session_id));
                }
                if p.session_id == session_id {
                    // Already joined - just return current state
                    let partner = state.participants.iter().find(|pp| pp.session_id != session_id);
                    return Ok(serde_json::json!({
                        "collab_id": state.collab_id,
                        "status": if partner.is_some() { "paired" } else { "waiting" },
                        "partner": partner.map(|p| serde_json::json!({
                            "role": p.role,
                            "session_id": p.session_id
                        })),
                        "message_count": state.message_count
                    }));
                }
            }
            if state.participants.len() >= 2 {
                return Err("Collaboration full (2 participants max)".to_string());
            }
        }

        // Create or join
        let state = self.collabs.entry(normalized.clone()).or_insert_with(|| {
            let now = now_iso();
            CollabState {
                collab_id: generate_collab_id(),
                project_dir: normalized.clone(),
                created_at: now.clone(),
                participants: Vec::new(),
                message_count: 0,
                messages: Vec::new(),
                last_activity: now,
            }
        });

        let participant = Participant {
            role: role.to_string(),
            session_id: session_id.to_string(),
            joined_at: now_iso(),
            last_heartbeat: now_millis(),
        };
        state.participants.push(participant);
        let _ = state;

        // Write files
        self.write_files(&normalized)?;

        let state = self.collabs.get(&normalized).unwrap();
        let partner = state
            .participants
            .iter()
            .find(|p| p.session_id != session_id);
        Ok(serde_json::json!({
            "collab_id": state.collab_id,
            "status": if partner.is_some() { "paired" } else { "waiting" },
            "partner": partner.map(|p| serde_json::json!({
                "role": p.role,
                "session_id": p.session_id
            })),
            "message_count": state.message_count
        }))
    }

    /// Send a message
    pub fn send(
        &mut self,
        session_id: &str,
        message: &str,
    ) -> Result<serde_json::Value, String> {
        // Find which collab this session belongs to
        let project_dir = self
            .collabs
            .iter()
            .find(|(_, s)| s.participants.iter().any(|p| p.session_id == session_id))
            .map(|(k, _)| k.clone())
            .ok_or("Session not in any collaboration. Call collab_join first.")?;

        let state = self.collabs.get_mut(&project_dir).unwrap();

        // Find the role for this session
        let role = state
            .participants
            .iter()
            .find(|p| p.session_id == session_id)
            .map(|p| p.role.clone())
            .ok_or("Session not found in collaboration")?;

        state.message_count += 1;
        let msg = CollabMessage {
            number: state.message_count,
            role: role.clone(),
            timestamp: time_short(),
            text: message.to_string(),
        };
        state.messages.push(msg.clone());
        state.last_activity = now_iso();

        // Update heartbeat
        for p in &mut state.participants {
            if p.session_id == session_id {
                p.last_heartbeat = now_millis();
            }
        }

        let msg_count = state.message_count;
        let role_copy = role.clone();
        let _ = state;

        self.write_files(&project_dir)?;

        Ok(serde_json::json!({
            "message_number": msg_count,
            "role": role_copy
        }))
    }

    /// Check for new messages
    pub fn check(
        &mut self,
        session_id: &str,
        last_seen: u32,
    ) -> Result<serde_json::Value, String> {
        // Find which collab this session belongs to
        let project_dir = self
            .collabs
            .iter()
            .find(|(_, s)| s.participants.iter().any(|p| p.session_id == session_id))
            .map(|(k, _)| k.clone())
            .ok_or("Session not in any collaboration. Call collab_join first.")?;

        let state = self.collabs.get_mut(&project_dir).unwrap();

        // Update heartbeat for this session
        for p in &mut state.participants {
            if p.session_id == session_id {
                p.last_heartbeat = now_millis();
            }
        }

        let new_messages: Vec<&CollabMessage> = state
            .messages
            .iter()
            .filter(|m| m.number > last_seen)
            .collect();

        // Check partner activity (active if heartbeat within 30s)
        let now = now_millis();
        let partner_active = state
            .participants
            .iter()
            .any(|p| p.session_id != session_id && (now - p.last_heartbeat) < 30_000);

        Ok(serde_json::json!({
            "messages": new_messages,
            "latest_message_number": state.message_count,
            "partner_active": partner_active
        }))
    }

    /// Leave a collaboration
    pub fn leave(&mut self, session_id: &str) -> Result<serde_json::Value, String> {
        let project_dir = self
            .collabs
            .iter()
            .find(|(_, s)| s.participants.iter().any(|p| p.session_id == session_id))
            .map(|(k, _)| k.clone())
            .ok_or("Session not in any collaboration")?;

        {
            let state = self.collabs.get_mut(&project_dir).unwrap();
            state.participants.retain(|p| p.session_id != session_id);
            state.last_activity = now_iso();
        }

        self.write_files(&project_dir)?;

        // If no participants left, remove from memory
        let empty = self.collabs.get(&project_dir).map(|s| s.participants.is_empty()).unwrap_or(false);
        if empty {
            self.collabs.remove(&project_dir);
        }

        Ok(serde_json::json!({"status": "left"}))
    }

    /// Get current state for frontend
    pub fn get_state(&self) -> serde_json::Value {
        let collabs: Vec<serde_json::Value> = self
            .collabs
            .values()
            .map(|s| {
                serde_json::json!({
                    "collab_id": s.collab_id,
                    "project_dir": s.project_dir,
                    "participants": s.participants,
                    "message_count": s.message_count,
                    "messages": s.messages,
                    "last_activity": s.last_activity
                })
            })
            .collect();
        serde_json::json!({ "collaborations": collabs })
    }

    fn load_state(&self, project_dir: &str) -> Option<CollabState> {
        let path = collab_state_path(project_dir);
        let content = fs::read_to_string(&path).ok()?;
        serde_json::from_str(&content).ok()
    }

    fn write_files(&self, project_dir: &str) -> Result<(), String> {
        let state = self.collabs.get(project_dir).ok_or("Collab not found")?;

        // Ensure .vaak directory exists
        let dir = vaak_dir(project_dir);
        fs::create_dir_all(&dir).map_err(|e| format!("Failed to create .vaak dir: {}", e))?;

        // Write collab-state.json
        let json = serde_json::to_string_pretty(state)
            .map_err(|e| format!("Failed to serialize state: {}", e))?;
        fs::write(collab_state_path(project_dir), &json)
            .map_err(|e| format!("Failed to write state file: {}", e))?;

        // Write collab.md
        let md = self.render_markdown(state);
        fs::write(collab_md_path(project_dir), &md)
            .map_err(|e| format!("Failed to write collab.md: {}", e))?;

        Ok(())
    }

    fn render_markdown(&self, state: &CollabState) -> String {
        let mut md = format!(
            "<!-- vaak-collab v1 | collab_id: {} | created: {} -->\n\n",
            state.collab_id, state.created_at
        );

        // Extract project name from path
        let project_name = state
            .project_dir
            .rsplit('/')
            .next()
            .unwrap_or(&state.project_dir);
        md.push_str(&format!("# Collaboration: {}\n\n", project_name));

        md.push_str("## Participants\n");
        for p in &state.participants {
            md.push_str(&format!(
                "- **{}** (session: {}) - joined {}\n",
                p.role,
                &p.session_id[..p.session_id.len().min(12)],
                p.joined_at
            ));
        }
        md.push_str("\n---\n\n");

        for msg in &state.messages {
            md.push_str(&format!(
                "### [{:03}] {} ({})\n{}\n\n---\n\n",
                msg.number, msg.role, msg.timestamp, msg.text
            ));
        }

        md
    }
}
