use parking_lot::Mutex;
use std::process::Command;
use std::sync::Arc;
use tauri::State;

/// A Claude agent spawned from the Team Launcher UI
#[derive(serde::Serialize, Clone, Debug)]
pub struct SpawnedAgent {
    pub pid: u32,
    pub role: String,
    pub instance: i32,
    pub spawned_at: String,
}

/// Tracks all spawned Claude agent processes
pub struct LauncherState {
    pub spawned: Arc<Mutex<Vec<SpawnedAgent>>>,
    pub project_dir: Mutex<Option<String>>,
}

impl Default for LauncherState {
    fn default() -> Self {
        Self {
            spawned: Arc::new(Mutex::new(Vec::new())),
            project_dir: Mutex::new(None),
        }
    }
}

/// Check if the `claude` CLI is installed and available on PATH
#[tauri::command]
pub fn check_claude_installed() -> Result<bool, String> {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        let output = Command::new("where")
            .arg("claude")
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|e| format!("Failed to run 'where claude': {}", e))?;
        Ok(output.status.success())
    }
    #[cfg(not(target_os = "windows"))]
    {
        let output = Command::new("which")
            .arg("claude")
            .output()
            .map_err(|e| format!("Failed to run 'which claude': {}", e))?;
        Ok(output.status.success())
    }
}

/// Build the join prompt for a given role
fn build_join_prompt(role: &str) -> String {
    format!(
        "Join this project as a {} using the mcp vaak project_join tool with role {}. Then call project_wait in a loop to stay available for messages.",
        role, role
    )
}

/// Internal: spawn a single Claude agent (does not require Tauri State)
fn do_spawn_member(project_dir: &str, role: &str, launcher: &LauncherState) -> Result<(), String> {
    // Remember project dir for later use (kill_team_member session cleanup)
    *launcher.project_dir.lock() = Some(project_dir.to_string());

    let join_prompt = build_join_prompt(role);

    #[cfg(target_os = "windows")]
    let real_pid: u32 = {
        // Write temp .ps1 script (same as working launch-team.ps1 approach)
        let temp_dir = std::env::temp_dir();
        let script_name = format!("vaak-launch-{}-{}.ps1", role, std::process::id());
        let script_path = temp_dir.join(&script_name);
        let ps_script = format!(
            "Set-Location \"{}\"\nclaude --dangerously-skip-permissions \"{}\"",
            project_dir, join_prompt
        );
        std::fs::write(&script_path, &ps_script)
            .map_err(|e| format!("Failed to write launch script: {}", e))?;

        // Use Start-Process -PassThru to get the real visible terminal PID.
        // .output() blocks until the hidden shell exits (fast — Start-Process is non-blocking),
        // then we parse the PID from stdout. No race conditions, no temp files.
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;

        let output = Command::new("powershell")
            .args([
                "-NoProfile",
                "-WindowStyle", "Hidden",
                "-Command",
                &format!(
                    "$p = Start-Process powershell -PassThru -WorkingDirectory '{}' -ArgumentList '-ExecutionPolicy','Bypass','-File','{}'; Write-Output $p.Id",
                    project_dir.replace("'", "''"),
                    script_path.to_str().unwrap_or("").replace("'", "''")
                ),
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|e| format!("Failed to spawn PowerShell: {}", e))?;

        let pid_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
        pid_str.parse::<u32>()
            .map_err(|e| format!("Failed to parse spawned PID '{}': {}", pid_str, e))?
    };

    #[cfg(target_os = "macos")]
    let child = {
        let script = format!(
            "tell application \"Terminal\" to do script \"cd '{}' && claude --dangerously-skip-permissions '{}'\"",
            project_dir, join_prompt
        );
        Command::new("osascript")
            .args(["-e", &script])
            .spawn()
            .map_err(|e| format!("Failed to spawn Terminal: {}", e))?
    };

    #[cfg(target_os = "linux")]
    let child = {
        Command::new("x-terminal-emulator")
            .args([
                "-e",
                &format!(
                    "bash -c \"cd '{}' && claude --dangerously-skip-permissions '{}'\"",
                    project_dir, join_prompt
                ),
            ])
            .spawn()
            .or_else(|_| {
                Command::new("gnome-terminal")
                    .args([
                        "--",
                        "bash",
                        "-c",
                        &format!(
                            "cd '{}' && claude --dangerously-skip-permissions '{}'",
                            project_dir, join_prompt
                        ),
                    ])
                    .spawn()
            })
            .or_else(|_| {
                Command::new("xterm")
                    .args([
                        "-e",
                        &format!(
                            "cd '{}' && claude --dangerously-skip-permissions '{}'",
                            project_dir, join_prompt
                        ),
                    ])
                    .spawn()
            })
            .map_err(|e| format!("Failed to spawn terminal: {}", e))?
    };

    // On Windows, real_pid is the actual visible terminal PID (from -PassThru).
    // On other platforms, child.id() is the best we have.
    #[cfg(target_os = "windows")]
    let pid = real_pid;
    #[cfg(not(target_os = "windows"))]
    let pid = child.id();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| {
            let secs = d.as_secs();
            let s = secs % 60;
            let m = (secs / 60) % 60;
            let h = (secs / 3600) % 24;
            let days_since_epoch = secs / 86400;
            let mut y = 1970i64;
            let mut remaining = days_since_epoch as i64;
            loop {
                let days_in_year = if y % 4 == 0 && (y % 100 != 0 || y % 400 == 0) { 366 } else { 365 };
                if remaining < days_in_year { break; }
                remaining -= days_in_year;
                y += 1;
            }
            let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
            let month_days = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
            let mut month = 0usize;
            for (i, &md) in month_days.iter().enumerate() {
                if remaining < md as i64 { month = i; break; }
                remaining -= md as i64;
            }
            format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", y, month + 1, remaining + 1, h, m, s)
        })
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string());

    let mut spawned = launcher.spawned.lock();
    let instance = spawned.iter().filter(|a| a.role == role).count() as i32;
    spawned.push(SpawnedAgent {
        pid,
        role: role.to_string(),
        instance,
        spawned_at: now,
    });

    Ok(())
}

/// Spawn a single Claude agent in a new terminal window
#[tauri::command]
pub fn launch_team_member(
    project_dir: String,
    role: String,
    state: State<'_, LauncherState>,
) -> Result<(), String> {
    do_spawn_member(&project_dir, &role, &state)
}

/// Spawn multiple Claude agents with a 2-second stagger (non-blocking)
#[tauri::command]
pub fn launch_team(
    project_dir: String,
    roles: Vec<String>,
    state: State<'_, LauncherState>,
) -> Result<u32, String> {
    if roles.is_empty() {
        return Ok(0);
    }

    // Launch the first one immediately for instant feedback
    let first_role = roles[0].clone();
    do_spawn_member(&project_dir, &first_role, &state)?;

    // Spawn remaining roles in a background thread with stagger
    if roles.len() > 1 {
        let remaining_roles: Vec<String> = roles[1..].to_vec();
        let dir = project_dir.clone();
        let spawned_clone = Arc::clone(&state.spawned);
        let dir_clone = project_dir.clone();
        std::thread::spawn(move || {
            let bg_state = LauncherState {
                spawned: spawned_clone,
                project_dir: Mutex::new(Some(dir_clone)),
            };
            for role in &remaining_roles {
                std::thread::sleep(std::time::Duration::from_secs(2));
                if let Err(e) = do_spawn_member(&dir, role, &bg_state) {
                    eprintln!("[launcher] Failed to launch {}: {}", role, e);
                }
            }
        });
    }

    Ok(roles.len() as u32)
}

/// Kill a spawned team member by role and instance, and revoke their session.
/// Works for both launcher-spawned agents (PID tracked) and manually-launched ones.
#[tauri::command]
pub fn kill_team_member(
    role: String,
    instance: i32,
    state: State<'_, LauncherState>,
) -> Result<(), String> {
    let mut spawned = state.spawned.lock();
    if let Some(pos) = spawned.iter().position(|a| a.role == role && a.instance == instance) {
        let agent = spawned.remove(pos);
        let _ = kill_process(agent.pid); // best-effort kill
    }
    drop(spawned); // Release lock before file I/O

    // Always revoke the session, whether we had a PID or not.
    // The agent will detect revocation on its next heartbeat and exit.
    if let Some(dir) = state.project_dir.lock().clone() {
        revoke_session(&dir, &role, instance)?;
    }
    Ok(())
}

/// Kill all spawned team members and revoke all non-human sessions
#[tauri::command]
pub fn kill_all_team_members(state: State<'_, LauncherState>) -> Result<(), String> {
    let mut spawned = state.spawned.lock();
    // Best-effort kill any tracked PIDs
    for agent in spawned.drain(..) {
        let _ = kill_process(agent.pid);
    }
    drop(spawned);

    // Revoke ALL non-human sessions from sessions.json
    if let Some(dir) = state.project_dir.lock().clone() {
        revoke_all_sessions(&dir)?;
    }
    Ok(())
}

/// Get list of all spawned agents
#[tauri::command]
pub fn get_spawned_agents(state: State<'_, LauncherState>) -> Result<Vec<SpawnedAgent>, String> {
    let spawned = state.spawned.lock();
    Ok(spawned.clone())
}

/// Remove a session binding from .vaak/sessions.json by role:instance
fn revoke_session(project_dir: &str, role: &str, instance: i32) -> Result<(), String> {
    let sessions_path = std::path::Path::new(project_dir)
        .join(".vaak")
        .join("sessions.json");

    let content = std::fs::read_to_string(&sessions_path)
        .map_err(|e| format!("Failed to read sessions.json: {}", e))?;

    let mut sessions: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse sessions.json: {}", e))?;

    if let Some(bindings) = sessions.get_mut("bindings").and_then(|b| b.as_array_mut()) {
        let before_len = bindings.len();
        bindings.retain(|b| {
            let b_role = b.get("role").and_then(|r| r.as_str()).unwrap_or("");
            let b_instance = b.get("instance").and_then(|i| i.as_i64()).unwrap_or(-1) as i32;
            !(b_role == role && b_instance == instance)
        });
        if bindings.len() < before_len {
            let updated = serde_json::to_string_pretty(&sessions)
                .map_err(|e| format!("Failed to serialize sessions.json: {}", e))?;
            std::fs::write(&sessions_path, updated)
                .map_err(|e| format!("Failed to write sessions.json: {}", e))?;
        }
    }

    Ok(())
}

/// Remove ALL non-human session bindings from .vaak/sessions.json
fn revoke_all_sessions(project_dir: &str) -> Result<(), String> {
    let sessions_path = std::path::Path::new(project_dir)
        .join(".vaak")
        .join("sessions.json");

    let content = std::fs::read_to_string(&sessions_path)
        .map_err(|e| format!("Failed to read sessions.json: {}", e))?;

    let mut sessions: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse sessions.json: {}", e))?;

    if let Some(bindings) = sessions.get_mut("bindings").and_then(|b| b.as_array_mut()) {
        // Keep only human sessions
        bindings.retain(|b| {
            let b_role = b.get("role").and_then(|r| r.as_str()).unwrap_or("");
            b_role == "human"
        });
        let updated = serde_json::to_string_pretty(&sessions)
            .map_err(|e| format!("Failed to serialize sessions.json: {}", e))?;
        std::fs::write(&sessions_path, updated)
            .map_err(|e| format!("Failed to write sessions.json: {}", e))?;
    }

    Ok(())
}

/// Kill a process by PID (platform-specific)
fn kill_process(pid: u32) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        // Use taskkill /F /T to kill process tree (PowerShell + claude child)
        let output = Command::new("taskkill")
            .args(["/F", "/T", "/PID", &pid.to_string()])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|e| format!("Failed to run taskkill: {}", e))?;
        if output.status.success() {
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(format!("taskkill failed for PID {}: {}", pid, stderr.trim()))
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        use std::process::Command;
        // Kill process group
        let output = Command::new("kill")
            .args(["-TERM", &pid.to_string()])
            .output()
            .map_err(|e| format!("Failed to run kill: {}", e))?;
        if output.status.success() {
            Ok(())
        } else {
            Err(format!("kill failed for PID {}", pid))
        }
    }
}

/// Kill a tracked agent by role:instance. Returns true if a matching PID was found and killed.
/// This does NOT revoke the session — caller is responsible for session cleanup.
pub fn kill_tracked_agent(role: &str, instance: i32, state: &LauncherState) -> bool {
    let mut spawned = state.spawned.lock();
    if let Some(pos) = spawned.iter().position(|a| a.role == role && a.instance == instance) {
        let agent = spawned.remove(pos);
        let _ = kill_process(agent.pid);
        true
    } else {
        false
    }
}

/// Cleanup function — kill all spawned agents on app exit.
/// Call this from the Tauri shutdown hook.
pub fn cleanup_all_spawned(state: &LauncherState) {
    let mut spawned = state.spawned.lock();
    for agent in spawned.drain(..) {
        let _ = kill_process(agent.pid);
    }
}
