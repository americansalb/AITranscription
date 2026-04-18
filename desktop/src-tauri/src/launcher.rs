use parking_lot::Mutex;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::State;

/// A Claude agent spawned from the Team Launcher UI
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct SpawnedAgent {
    pub pid: u32,
    pub role: String,
    pub instance: i32,
    /// Format contract: fixed-width ISO-8601 UTC (`YYYY-MM-DDTHH:MM:SSZ`). All
    /// writers in this module use `format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", ...)`.
    /// `load_spawned_from_disk` sorts lexicographically and relies on that
    /// equaling chronological order. Introducing timezone offsets or
    /// fractional seconds silently breaks dedupe. (dev-challenger:1 msg 241
    /// Finding 2.)
    pub spawned_at: String,
}

/// Tracks all spawned Claude agent processes
pub struct LauncherState {
    pub spawned: Arc<Mutex<Vec<SpawnedAgent>>>,
    pub project_dir: Mutex<Option<String>>,
    /// Guard against concurrent `relaunch_spawned` invocations (tester:1 msg 168).
    /// Rapid double-click would otherwise snapshot in-memory before the first
    /// batch had pushed all its new PIDs, producing double-spawns of whichever
    /// dead entries hadn't been processed yet. Set true on entry, cleared
    /// when the bg stagger thread finishes. Bg-thread LauncherState clones
    /// constructed inside other spawn paths initialize this to false — they
    /// don't coordinate with the real Tauri state, so their value doesn't
    /// matter.
    pub relaunch_in_progress: Arc<AtomicBool>,
}

impl Default for LauncherState {
    fn default() -> Self {
        Self {
            spawned: Arc::new(Mutex::new(Vec::new())),
            project_dir: Mutex::new(None),
            relaunch_in_progress: Arc::new(AtomicBool::new(false)),
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
    #[cfg(target_os = "macos")]
    {
        // macOS GUI apps (launched from Finder/Dock/Spotlight) get a minimal PATH
        // (/usr/bin:/bin:/usr/sbin:/sbin) and do NOT inherit the user's shell PATH.
        // Running `which claude` directly would return false even if claude is installed
        // via npm/nvm/fnm/homebrew. Use a login shell to source the user's profile first.
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
        let output = Command::new(&shell)
            .args(["-l", "-c", "which claude"])
            .output()
            .map_err(|e| format!("Failed to check for claude via login shell: {}", e))?;
        Ok(output.status.success())
    }
    #[cfg(target_os = "linux")]
    {
        let output = Command::new("which")
            .arg("claude")
            .output()
            .map_err(|e| format!("Failed to run 'which claude': {}", e))?;
        Ok(output.status.success())
    }
}

/// Check if an ANTHROPIC_API_KEY (or CLAUDE_API_KEY) environment variable is set.
/// Returns a struct with `has_key` bool and `key_source` string (which env var was found).
#[tauri::command]
pub fn check_anthropic_key() -> Result<serde_json::Value, String> {
    // Check the two common env var names Claude Code looks for
    if let Ok(val) = std::env::var("ANTHROPIC_API_KEY") {
        if !val.trim().is_empty() {
            return Ok(serde_json::json!({
                "has_key": true,
                "key_source": "ANTHROPIC_API_KEY"
            }));
        }
    }
    if let Ok(val) = std::env::var("CLAUDE_API_KEY") {
        if !val.trim().is_empty() {
            return Ok(serde_json::json!({
                "has_key": true,
                "key_source": "CLAUDE_API_KEY"
            }));
        }
    }
    Ok(serde_json::json!({
        "has_key": false,
        "key_source": null
    }))
}

/// Build the join prompt for a given role
fn build_join_prompt(role: &str) -> String {
    format!(
        "Join this project as a {} using the mcp vaak project_join tool with role {}. Then call project_wait in a loop to stay available for messages.",
        role, role
    )
}

/// Read companion roles from project.json for a given role.
/// Returns a list of (role_slug, optional, default_enabled) tuples.
fn get_companions(project_dir: &str, role: &str) -> Vec<(String, bool)> {
    let config_path = Path::new(project_dir).join(".vaak").join("project.json");
    let content = match std::fs::read_to_string(&config_path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    let config: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let companions = config
        .get("roles")
        .and_then(|r| r.get(role))
        .and_then(|r| r.get("companions"))
        .and_then(|c| c.as_array());
    match companions {
        Some(arr) => arr.iter().filter_map(|c| {
            // Support both string format "role-slug" and object format { role, optional, default_enabled }
            if let Some(s) = c.as_str() {
                Some((s.to_string(), true))
            } else if let Some(obj) = c.as_object() {
                let slug = obj.get("role").and_then(|r| r.as_str())?.to_string();
                let default_enabled = obj.get("default_enabled").and_then(|d| d.as_bool()).unwrap_or(true);
                Some((slug, default_enabled))
            } else {
                None
            }
        }).collect(),
        None => Vec::new(),
    }
}

/// Tauri command: get companion roles for a given role slug.
#[tauri::command]
pub fn get_role_companions(
    project_dir: String,
    role: String,
) -> Result<Vec<serde_json::Value>, String> {
    let companions = get_companions(&project_dir, &role);
    Ok(companions.iter().map(|(slug, default_enabled)| {
        serde_json::json!({
            "role": slug,
            "default_enabled": default_enabled,
        })
    }).collect())
}

/// Internal: spawn a single Claude agent (does not require Tauri State).
/// If `roster_instance` is Some, use that instance number (for roster-based launch).
/// If None, auto-assign based on count of previously spawned agents of this role.
fn do_spawn_member(project_dir: &str, role: &str, roster_instance: Option<i32>, launcher: &LauncherState) -> Result<(), String> {
    // Remember project dir for later use (kill_team_member session cleanup)
    *launcher.project_dir.lock() = Some(project_dir.to_string());

    let join_prompt = build_join_prompt(role);

    #[cfg(target_os = "windows")]
    let real_pid: u32 = {
        // Resolve the full path to claude.exe BEFORE writing the script.
        // WMI (Invoke-CimMethod) creates processes via wmiprvse.exe which does NOT
        // inherit the user's PATH, so bare "claude" won't be found.
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;

        let where_output = Command::new("where")
            .arg("claude")
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|e| format!("Failed to run 'where claude': {}", e))?;

        let claude_path = if where_output.status.success() {
            let raw = String::from_utf8_lossy(&where_output.stdout);
            // `where` may return multiple lines; take the first .exe path
            raw.lines()
                .find(|l| l.trim().ends_with(".exe"))
                .unwrap_or("claude")
                .trim()
                .to_string()
        } else {
            // Fallback: hope it's on PATH (matches old behavior)
            "claude".to_string()
        };

        // Write temp .ps1 script (same as working launch-team.ps1 approach)
        // The script uses .current_dir() on the WMI process instead of interpolating
        // the project_dir into PowerShell, avoiding injection via $(), backticks, etc.
        // Only the claude path and prompt are interpolated (with single-quote escaping).
        let temp_dir = std::env::temp_dir();
        let script_name = format!("vaak-launch-{}-{}.ps1", role, std::process::id());
        let script_path = temp_dir.join(&script_name);
        let safe_claude = claude_path.replace('\'', "''");
        let safe_prompt = join_prompt.replace('\'', "''");
        let ps_script = format!(
            "& '{}' --dangerously-skip-permissions '{}'",
            safe_claude, safe_prompt
        );
        std::fs::write(&script_path, &ps_script)
            .map_err(|e| format!("Failed to write launch script: {}", e))?;

        // Use WMI (Invoke-CimMethod) to create the agent process.
        // WMI creates the process via the WMI service (wmiprvse.exe), so it is
        // NOT in Tauri's Job Object and survives app restarts. This avoids the
        // CREATE_BREAKAWAY_FROM_JOB "Access denied" issue entirely.
        // Pass CurrentDirectory via WMI instead of interpolating into the PS script,
        // matching the .current_dir() pattern used in open_terminal_in_dir.
        let script_path_str = script_path.to_str().unwrap_or("").replace("'", "''");
        let safe_dir_ps = project_dir.replace('\'', "''");
        let ps_cmd = format!(
            "$r = Invoke-CimMethod -ClassName Win32_Process -MethodName Create -Arguments @{{CommandLine='powershell.exe -ExecutionPolicy Bypass -NoExit -File \"{}\"';CurrentDirectory='{}'}}; Write-Output $r.ProcessId",
            script_path_str, safe_dir_ps
        );
        let ps_args = ["-NoProfile", "-WindowStyle", "Hidden", "-Command", &ps_cmd];

        let output = Command::new("powershell")
            .args(&ps_args)
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|e| {
                let _ = std::fs::remove_file(&script_path);
                format!("Failed to spawn via WMI: {}", e)
            })?;

        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let stderr_str = String::from_utf8_lossy(&output.stderr).trim().to_string();

        if !output.status.success() || stdout.is_empty() {
            let _ = std::fs::remove_file(&script_path);
            return Err(format!("WMI spawn failed for {}: stdout='{}' stderr='{}'", role, stdout, stderr_str));
        }

        // Clean up temp script after a delay — the spawned PowerShell needs time to read it.
        // WMI returns the PID immediately, but the new process hasn't loaded the file yet.
        let cleanup_path = script_path.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(10));
            let _ = std::fs::remove_file(&cleanup_path);
        });

        eprintln!("[launcher] Spawned {} via WMI (independent of Job Object)", role);

        stdout.parse::<u32>()
            .map_err(|e| format!("Failed to parse WMI PID '{}': {}", stdout, e))?
    };

    #[cfg(target_os = "macos")]
    let real_pid: u32 = {
        // Write a temp .sh launcher script that:
        // 1. Records its own PID to a known file
        // 2. Uses `exec` to replace itself with claude (so PID file points to claude)
        let temp_dir = std::env::temp_dir();
        let inst = roster_instance.unwrap_or(0);
        let pid_file = temp_dir.join(format!("vaak-agent-{}-{}-{}.pid", role, inst, std::process::id()));
        let script_name = format!("vaak-launch-{}-{}-{}.sh", role, inst, std::process::id());
        let script_path = temp_dir.join(&script_name);

        // Escape single quotes in prompt and project_dir for safe shell interpolation.
        // Uses the standard shell idiom: ' → '\'' (end quote, escaped quote, reopen quote).
        // Both values are placed inside single quotes in the script to prevent
        // metacharacter expansion ($, `, \, etc.) — fixing command injection risk.
        let safe_prompt = join_prompt.replace('\'', "'\\''");
        let safe_dir = project_dir.replace('\'', "'\\''");
        let safe_pid = pid_file.to_string_lossy().replace('\'', "'\\''");
        let sh_script = format!(
            "#!/bin/sh\n\
             # Verify claude is on PATH before proceeding\n\
             if ! command -v claude >/dev/null 2>&1; then\n\
               echo 'ERROR: claude not found on PATH' >&2\n\
               exit 1\n\
             fi\n\
             echo $$ > '{pid_file}'\n\
             cd '{project_dir}'\n\
             exec claude --dangerously-skip-permissions '{prompt}'\n",
            pid_file = safe_pid,
            project_dir = safe_dir,
            prompt = safe_prompt,
        );
        std::fs::write(&script_path, &sh_script)
            .map_err(|e| format!("Failed to write launch script: {}", e))?;

        // Make executable (owner-only: rwx for owner, no access for others)
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o700));

        // Launch in Terminal.app via `open -a Terminal`
        // Use .output() to capture exit code and detect permission failures
        let open_result = Command::new("open")
            .args(["-a", "Terminal", script_path.to_str().unwrap_or("")])
            .output()
            .map_err(|e| format!("Failed to run 'open -a Terminal': {}", e))?;

        if !open_result.status.success() {
            let _ = std::fs::remove_file(&script_path);
            let stderr = String::from_utf8_lossy(&open_result.stderr);
            // Best-effort detection of macOS permission denial from Launch Services.
            // `open -a Terminal` uses Launch Services (not osascript), so error strings
            // differ from AppleScript's -1743. These heuristics cover common patterns;
            // the generic fallback error at the end catches anything we miss.
            if stderr.contains("not allowed") || stderr.contains("not permitted")
                || stderr.contains("-1743") || stderr.contains("permission")
            {
                return Err(
                    "macOS blocked Vaak from opening Terminal.app. \
                     Go to System Settings > Privacy & Security > Automation \
                     and enable 'Terminal' under 'Vaak'. Then try again.".to_string()
                );
            }
            return Err(format!("Failed to open Terminal.app for {}: {}", role, stderr));
        }

        eprintln!("[launcher] Spawned {} via Terminal.app, waiting for PID file...", role);

        // Poll for the PID file (250ms intervals, 20s timeout — generous for cold Terminal.app launch)
        let mut pid: Option<u32> = None;
        for _ in 0..80 {
            std::thread::sleep(std::time::Duration::from_millis(250));
            if let Ok(content) = std::fs::read_to_string(&pid_file) {
                if let Ok(p) = content.trim().parse::<u32>() {
                    pid = Some(p);
                    // Clean up PID file and launch script (contains prompt text)
                    let _ = std::fs::remove_file(&pid_file);
                    let _ = std::fs::remove_file(&script_path);
                    break;
                }
            }
        }

        match pid {
            Some(p) => {
                eprintln!("[launcher] Got claude PID {} for {}", p, role);
                p
            }
            None => {
                // Clean up stale PID file and launch script
                let _ = std::fs::remove_file(&pid_file);
                let _ = std::fs::remove_file(&script_path);
                return Err(format!(
                    "Timed out waiting for agent PID for {}. \
                     If Terminal.app did not open, check System Settings > \
                     Privacy & Security > Automation permissions for Vaak. \
                     If claude is not installed, install it with: npm install -g @anthropic-ai/claude-code", role
                ));
            }
        }
    };

    #[cfg(target_os = "linux")]
    let child = {
        // Escape single quotes to prevent shell injection in project_dir and prompt
        let safe_dir = project_dir.replace('\'', "'\\''");
        let safe_prompt = join_prompt.replace('\'', "'\\''");
        let bash_cmd = format!(
            "cd '{}' && claude --dangerously-skip-permissions '{}'",
            safe_dir, safe_prompt
        );
        // Pass bash_cmd as a direct -c argument to bash, NOT through a shell string.
        // Using separate args avoids double-quote re-interpretation of $, `, \.
        Command::new("x-terminal-emulator")
            .args(["-e", "bash", "-c", &bash_cmd])
            .spawn()
            .or_else(|_| {
                Command::new("gnome-terminal")
                    .args(["--", "bash", "-c", &bash_cmd])
                    .spawn()
            })
            .or_else(|_| {
                Command::new("xterm")
                    .args(["-e", "bash", "-c", &bash_cmd])
                    .spawn()
            })
            .map_err(|e| format!("Failed to spawn terminal: {}", e))?
    };

    // On Windows and macOS, real_pid is the actual claude PID.
    // On Linux, child.id() is the terminal emulator PID (best we have).
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    let pid = real_pid;
    #[cfg(target_os = "linux")]
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
    let instance = roster_instance.unwrap_or_else(|| {
        spawned.iter().filter(|a| a.role == role).count() as i32
    });
    // Dedupe in-memory on the same (role, instance) key the disk merge below
    // uses. Keeps in-memory and disk consistent if a stale entry slipped in
    // from a concurrent kill/relaunch edge or a future caller that forgot
    // cleanup. (dev-challenger:1 msg 241 Finding 3.)
    spawned.retain(|a| !(a.role == role && a.instance == instance));
    spawned.push(SpawnedAgent {
        pid,
        role: role.to_string(),
        instance,
        spawned_at: now.clone(),
    });

    // Persist PIDs to disk so they survive app restarts.
    //
    // pr-manifest-durability (2026-04-18): read the existing disk manifest,
    // replace any prior entry for this role+instance, and write back. Preserves
    // dead entries from prior sessions that only exist on disk (PR2 made
    // `repopulate_spawned` reconnect-only, so dead entries are never loaded
    // into in-memory `spawned`). Without this merge, any launch would clobber
    // the last-team manifest and break PR3's Relaunch button. Mirrors the
    // read-merge-write pattern already in `kill_team_member` (see line 546+).
    let new_entry = SpawnedAgent {
        pid,
        role: role.to_string(),
        instance,
        spawned_at: now.clone(),
    };
    drop(spawned);
    if let Some(dir) = launcher.project_dir.lock().as_ref() {
        let mut disk_agents = load_spawned_from_disk(dir);
        disk_agents.retain(|a| !(a.role == new_entry.role && a.instance == new_entry.instance));
        disk_agents.push(new_entry);
        save_spawned_to_disk(dir, &disk_agents);
    }

    Ok(())
}

/// Spawn a single Claude agent in a new terminal window.
/// `instance` is optional — if provided, uses the roster instance number for accurate PID tracking.
/// If omitted (frontend doesn't send it), auto-assigns based on count.
/// Also auto-launches any companion roles (e.g. audience with moderator).
/// `skip_companions` can be set to true from the frontend if the user unchecked the companion toggle.
#[tauri::command]
pub fn launch_team_member(
    project_dir: String,
    role: String,
    instance: Option<i32>,
    skip_companions: Option<bool>,
    state: State<'_, LauncherState>,
) -> Result<(), String> {
    do_spawn_member(&project_dir, &role, instance, &state)?;

    // Auto-launch companion roles (unless opted out)
    if !skip_companions.unwrap_or(false) {
        let companions = get_companions(&project_dir, &role);
        for (companion_slug, default_enabled) in companions {
            if default_enabled {
                eprintln!("[launcher] Auto-launching companion '{}' for role '{}'", companion_slug, role);
                // Stagger companion launch by 2 seconds in a background thread
                let dir = project_dir.clone();
                let slug = companion_slug.clone();
                let spawned_clone = Arc::clone(&state.spawned);
                let dir_clone = project_dir.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_secs(2));
                    let bg_state = LauncherState {
                        spawned: spawned_clone,
                        project_dir: Mutex::new(Some(dir_clone)),
                        relaunch_in_progress: Arc::new(AtomicBool::new(false)),
                    };
                    if let Err(e) = do_spawn_member(&dir, &slug, None, &bg_state) {
                        eprintln!("[launcher] Failed to launch companion '{}': {}", slug, e);
                    }
                });
            }
        }
    }

    Ok(())
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
    do_spawn_member(&project_dir, &first_role, None, &state)?;

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
                relaunch_in_progress: Arc::new(AtomicBool::new(false)),
            };
            for role in &remaining_roles {
                std::thread::sleep(std::time::Duration::from_secs(2));
                if let Err(e) = do_spawn_member(&dir, role, None, &bg_state) {
                    eprintln!("[launcher] Failed to launch {}: {}", role, e);
                }
            }
        });
    }

    Ok(roles.len() as u32)
}

/// Kill a spawned team member by role and instance, and revoke their session.
/// Works for both launcher-spawned agents (PID tracked) and manually-launched ones.
/// Falls back to disk-persisted PIDs if the in-memory list doesn't have the agent.
#[tauri::command]
pub fn kill_team_member(
    role: String,
    instance: i32,
    state: State<'_, LauncherState>,
) -> Result<(), String> {
    let mut spawned = state.spawned.lock();
    let mut found_pid = false;

    // 1. Try in-memory spawned list first
    if let Some(pos) = spawned.iter().position(|a| a.role == role && a.instance == instance) {
        let agent = spawned.remove(pos);
        let _ = kill_process(agent.pid);
        found_pid = true;
    }

    let project_dir = state.project_dir.lock().clone();
    let all: Vec<SpawnedAgent> = spawned.clone();
    drop(spawned);

    // 2. If not found in memory, try disk-persisted PIDs (survives app restart)
    if !found_pid {
        if let Some(ref dir) = project_dir {
            let disk_agents = load_spawned_from_disk(dir);
            if let Some(agent) = disk_agents.iter().find(|a| a.role == role && a.instance == instance) {
                let _ = kill_process(agent.pid);
                found_pid = true;
            }
        }
    }

    // Update disk file
    if let Some(ref dir) = project_dir {
        let disk_agents = load_spawned_from_disk(dir);
        let filtered: Vec<SpawnedAgent> = disk_agents.into_iter()
            .filter(|a| !(a.role == role && a.instance == instance))
            .collect();
        save_spawned_to_disk(dir, &filtered);
    }

    // Always revoke the session, whether we had a PID or not.
    // The agent will detect revocation on its next heartbeat and exit.
    if let Some(ref dir) = project_dir {
        revoke_session(dir, &role, instance)?;

        // Also kill companion roles (e.g., killing moderator also kills audience)
        let companions = get_companions(dir, &role);
        for (companion_slug, _) in companions {
            // Kill instance 0 of each companion (companions are single-instance)
            eprintln!("[launcher] Auto-killing companion '{}' (parent role '{}' killed)", companion_slug, role);
            // Kill process if tracked
            let mut spawned = state.spawned.lock();
            if let Some(pos) = spawned.iter().position(|a| a.role == companion_slug) {
                let agent = spawned.remove(pos);
                let _ = kill_process(agent.pid);
            }
            drop(spawned);
            // Update disk
            let disk_agents = load_spawned_from_disk(dir);
            let filtered: Vec<SpawnedAgent> = disk_agents.into_iter()
                .filter(|a| a.role != companion_slug)
                .collect();
            save_spawned_to_disk(dir, &filtered);
            // Revoke session
            let _ = revoke_session(dir, &companion_slug, 0);
        }
    }
    Ok(())
}

/// Kill all spawned team members and revoke all non-human sessions.
/// Uses both in-memory and disk-persisted PIDs for completeness.
#[tauri::command]
pub fn kill_all_team_members(state: State<'_, LauncherState>) -> Result<(), String> {
    let mut spawned = state.spawned.lock();
    // Best-effort kill any tracked PIDs from memory
    for agent in spawned.drain(..) {
        let _ = kill_process(agent.pid);
    }
    drop(spawned);

    if let Some(dir) = state.project_dir.lock().clone() {
        // Also kill any disk-persisted PIDs (from before restart)
        let disk_agents = load_spawned_from_disk(&dir);
        for agent in &disk_agents {
            let _ = kill_process(agent.pid);
        }
        // Clear the disk file
        save_spawned_to_disk(&dir, &[]);

        // Revoke ALL non-human sessions from sessions.json
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

/// Find the main window handle (HWND) for a given process ID.
/// Uses EnumWindows to iterate all top-level windows, checking each one's PID.
/// Returns the first visible window belonging to the target PID.
#[cfg(target_os = "windows")]
fn find_window_by_pid(target_pid: u32) -> Option<windows_sys::Win32::Foundation::HWND> {
    use windows_sys::Win32::Foundation::*;
    use windows_sys::Win32::UI::WindowsAndMessaging::*;

    struct SearchState {
        target_pid: u32,
        found_hwnd: HWND,
    }

    unsafe extern "system" fn enum_callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let state = &mut *(lparam as *mut SearchState);
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid == state.target_pid && IsWindowVisible(hwnd) != 0 {
            state.found_hwnd = hwnd;
            return 0; // Stop enumeration
        }
        1 // Continue enumeration
    }

    let mut state = SearchState {
        target_pid,
        found_hwnd: std::ptr::null_mut(),
    };
    unsafe {
        EnumWindows(Some(enum_callback), &mut state as *mut SearchState as LPARAM);
    }
    if !state.found_hwnd.is_null() {
        Some(state.found_hwnd)
    } else {
        None
    }
}

/// Bring a spawned agent's terminal window to the foreground.
/// Looks up the PID from the spawned agents list, finds the window, and focuses it.
#[tauri::command]
pub fn focus_agent_window(
    role: String,
    instance: i32,
    state: State<'_, LauncherState>,
) -> Result<(), String> {
    // Look up PID from in-memory spawned list
    let spawned = state.spawned.lock();
    let agent = spawned.iter().find(|a| a.role == role && a.instance == instance);

    let pid = match agent {
        Some(a) => a.pid,
        None => {
            // Try disk-persisted PIDs as fallback
            drop(spawned);
            let project_dir = state.project_dir.lock().clone();
            if let Some(dir) = project_dir {
                let disk_agents = load_spawned_from_disk(&dir);
                disk_agents.iter()
                    .find(|a| a.role == role && a.instance == instance)
                    .map(|a| a.pid)
                    .ok_or(format!("No spawned agent found for {}:{}", role, instance))?
            } else {
                return Err(format!("No spawned agent found for {}:{}", role, instance));
            }
        }
    };

    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::SetForegroundWindow;
        let hwnd = find_window_by_pid(pid)
            .ok_or(format!("No visible window found for {}:{} (PID {})", role, instance, pid))?;
        unsafe {
            SetForegroundWindow(hwnd);
        }
        Ok(())
    }

    #[cfg(target_os = "macos")]
    {
        // Get the TTY device for this PID so we can find its Terminal.app window
        let ps_output = std::process::Command::new("ps")
            .args(["-o", "tty=", "-p", &pid.to_string()])
            .output()
            .map_err(|e| format!("Failed to run ps: {}", e))?;

        let tty = String::from_utf8_lossy(&ps_output.stdout).trim().to_string();
        if tty.is_empty() || tty == "??" {
            return Err(format!(
                "Cannot find terminal for {}:{} (PID {}) — process may not have a TTY",
                role, instance, pid
            ));
        }

        // Activate Terminal.app and bring the window/tab with this TTY to front
        let script = format!(
            r#"tell application "Terminal"
    activate
    set targetTTY to "/dev/{}"
    repeat with w in windows
        repeat with t in tabs of w
            if tty of t is targetTTY then
                set selected tab of w to t
                set index of w to 1
                return
            end if
        end repeat
    end repeat
end tell"#,
            tty
        );

        let result = std::process::Command::new("osascript")
            .args(["-e", &script])
            .output()
            .map_err(|e| format!("Failed to run osascript: {}", e))?;

        if !result.status.success() {
            let stderr = String::from_utf8_lossy(&result.stderr);
            if stderr.contains("not allowed") || stderr.contains("-1743") {
                return Err(
                    "Automation permission required. Go to System Settings > \
                     Privacy & Security > Automation and enable Terminal for Vaak."
                        .to_string(),
                );
            }
            return Err(format!(
                "Failed to focus Terminal window for {}:{}: {}",
                role, instance, stderr.trim()
            ));
        }

        Ok(())
    }

    #[cfg(target_os = "linux")]
    {
        // Use xdotool to find and activate the window by PID
        let xdotool = std::process::Command::new("xdotool")
            .args(["search", "--pid", &pid.to_string()])
            .output();

        match xdotool {
            Ok(output) if output.status.success() => {
                let window_id = String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .next()
                    .unwrap_or("")
                    .to_string();
                if !window_id.is_empty() {
                    let _ = std::process::Command::new("xdotool")
                        .args(["windowactivate", &window_id])
                        .output();
                    Ok(())
                } else {
                    Err(format!(
                        "No window found for {}:{} (PID {})",
                        role, instance, pid
                    ))
                }
            }
            _ => Err(
                "Focus requires xdotool on Linux. Install with: sudo apt install xdotool"
                    .to_string(),
            ),
        }
    }
}

/// Send keystrokes to a spawned agent's terminal to wake it up.
/// Uses SetForegroundWindow + SendInput to type text into the agent's console.
/// This is the "real buzz" — it reaches agents even when their MCP connection is dead.
#[tauri::command]
pub fn buzz_agent_terminal(
    role: String,
    instance: i32,
    state: State<'_, LauncherState>,
) -> Result<String, String> {
    // Look up PID — same pattern as focus_agent_window
    let spawned = state.spawned.lock();
    let agent = spawned.iter().find(|a| a.role == role && a.instance == instance);

    let pid = match agent {
        Some(a) => a.pid,
        None => {
            drop(spawned);
            let project_dir = state.project_dir.lock().clone();
            if let Some(dir) = project_dir {
                let disk_agents = load_spawned_from_disk(&dir);
                disk_agents.iter()
                    .find(|a| a.role == role && a.instance == instance)
                    .map(|a| a.pid)
                    .ok_or(format!("No spawned agent found for {}:{}", role, instance))?
            } else {
                return Err(format!("No spawned agent found for {}:{}", role, instance));
            }
        }
    };

    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{SetForegroundWindow, GetForegroundWindow};
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;

        // Find and focus the agent's terminal window
        let hwnd = find_window_by_pid(pid)
            .ok_or(format!("No visible window found for {}:{} (PID {})", role, instance, pid))?;

        // Save the current foreground window so we can restore it after buzzing
        let prev_hwnd = unsafe { GetForegroundWindow() };

        unsafe {
            SetForegroundWindow(hwnd);
        }

        // Brief pause for window activation
        std::thread::sleep(std::time::Duration::from_millis(100));

        // Build SendInput events for "back" + Enter
        let text = "back";
        let mut inputs: Vec<INPUT> = Vec::new();

        for c in text.encode_utf16() {
            // Key down
            inputs.push(INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: 0,
                        wScan: c,
                        dwFlags: KEYEVENTF_UNICODE,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            });
            // Key up
            inputs.push(INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: 0,
                        wScan: c,
                        dwFlags: KEYEVENTF_UNICODE | KEYEVENTF_KEYUP,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            });
        }

        // Enter key (VK_RETURN = 0x0D)
        inputs.push(INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: 0x0D,
                    wScan: 0,
                    dwFlags: 0,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        });
        inputs.push(INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: 0x0D,
                    wScan: 0,
                    dwFlags: KEYEVENTF_KEYUP,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        });

        let sent = unsafe {
            SendInput(
                inputs.len() as u32,
                inputs.as_ptr(),
                std::mem::size_of::<INPUT>() as i32,
            )
        };

        if sent == 0 {
            // Restore focus even on failure
            if !prev_hwnd.is_null() {
                unsafe { SetForegroundWindow(prev_hwnd); }
            }
            return Err(format!("SendInput failed for {}:{} (PID {})", role, instance, pid));
        }

        // Brief pause to let keystrokes land, then restore the user's window
        std::thread::sleep(std::time::Duration::from_millis(150));
        if !prev_hwnd.is_null() {
            unsafe { SetForegroundWindow(prev_hwnd); }
        }

        Ok(format!("Buzzed {}:{} — sent {} keystrokes to PID {}", role, instance, sent, pid))
    }

    #[cfg(target_os = "macos")]
    {
        // Get the TTY device for this PID
        let ps_output = std::process::Command::new("ps")
            .args(["-o", "tty=", "-p", &pid.to_string()])
            .output()
            .map_err(|e| format!("Failed to run ps: {}", e))?;

        let tty = String::from_utf8_lossy(&ps_output.stdout).trim().to_string();
        if tty.is_empty() || tty == "??" {
            return Err(format!(
                "Cannot find terminal for {}:{} (PID {})",
                role, instance, pid
            ));
        }

        // Save the frontmost app so we can restore focus after buzzing
        let save_front = std::process::Command::new("osascript")
            .args(["-e", r#"tell application "System Events" to get name of first process whose frontmost is true"#])
            .output()
            .ok()
            .and_then(|o| if o.status.success() {
                Some(String::from_utf8_lossy(&o.stdout).trim().to_string())
            } else {
                None
            });

        // Focus the Terminal window/tab, then type "back" + Enter via System Events keystroke
        let script = format!(
            r#"tell application "Terminal"
    activate
    set targetTTY to "/dev/{}"
    repeat with w in windows
        repeat with t in tabs of w
            if tty of t is targetTTY then
                set selected tab of w to t
                set index of w to 1
                delay 0.1
                tell application "System Events"
                    keystroke "back"
                    keystroke return
                end tell
                return
            end if
        end repeat
    end repeat
end tell"#,
            tty
        );

        let result = std::process::Command::new("osascript")
            .args(["-e", &script])
            .output()
            .map_err(|e| format!("Failed to run osascript: {}", e))?;

        if !result.status.success() {
            let stderr = String::from_utf8_lossy(&result.stderr);
            if stderr.contains("not allowed") || stderr.contains("-1743") {
                // System Events keystroke requires Accessibility permission;
                // Terminal window iteration requires Automation permission.
                return Err(
                    "Buzz requires two macOS permissions: \
                     (1) System Settings > Privacy & Security > Automation — enable Terminal for Vaak. \
                     (2) System Settings > Privacy & Security > Accessibility — enable Vaak."
                        .to_string(),
                );
            }
            return Err(format!(
                "Failed to buzz {}:{}: {}",
                role, instance, stderr.trim()
            ));
        }

        // Restore the user's previously-focused app (avoid focus stealing)
        if let Some(front_app) = save_front {
            if front_app != "Terminal" {
                std::thread::sleep(std::time::Duration::from_millis(150));
                let _ = std::process::Command::new("osascript")
                    .args(["-e", &format!(r#"tell application "{}" to activate"#, front_app)])
                    .output();
            }
        }

        Ok(format!(
            "Buzzed {}:{} via Terminal.app keystroke on /dev/{}",
            role, instance, tty
        ))
    }

    #[cfg(target_os = "linux")]
    {
        // Use xdotool to focus the window and send keystrokes
        let xdotool = std::process::Command::new("xdotool")
            .args(["search", "--pid", &pid.to_string()])
            .output();

        match xdotool {
            Ok(output) if output.status.success() => {
                let window_id = String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .next()
                    .unwrap_or("")
                    .to_string();
                if !window_id.is_empty() {
                    let _ = std::process::Command::new("xdotool")
                        .args(["windowactivate", "--sync", &window_id])
                        .output();
                    let _ = std::process::Command::new("xdotool")
                        .args(["type", "--window", &window_id, "back"])
                        .output();
                    let _ = std::process::Command::new("xdotool")
                        .args(["key", "--window", &window_id, "Return"])
                        .output();
                    Ok(format!(
                        "Buzzed {}:{} via xdotool on window {}",
                        role, instance, window_id
                    ))
                } else {
                    Err(format!(
                        "No window found for {}:{} (PID {})",
                        role, instance, pid
                    ))
                }
            }
            _ => Err(
                "Buzz requires xdotool on Linux. Install with: sudo apt install xdotool"
                    .to_string(),
            ),
        }
    }
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
        // Kill child processes first (process tree), then parent.
        // pkill -P kills all children of the given PID.
        // This matches Windows behavior of `taskkill /T` (tree kill).
        let _ = Command::new("pkill")
            .args(["-TERM", "-P", &pid.to_string()])
            .stderr(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .output();

        // Brief pause for children to terminate
        std::thread::sleep(std::time::Duration::from_millis(100));

        // Now kill the parent process
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

/// Persist spawned agents to .vaak/spawned.json so PIDs survive app restarts.
pub(crate) fn save_spawned_to_disk(project_dir: &str, agents: &[SpawnedAgent]) {
    let path = std::path::Path::new(project_dir)
        .join(".vaak")
        .join("spawned.json");
    if let Ok(json) = serde_json::to_string_pretty(agents) {
        let _ = std::fs::write(path, json);
    }
}

/// Load spawned agents from .vaak/spawned.json (fallback for after app restart).
///
/// pr-manifest-durability (2026-04-18): dedupe by `(role, instance)`, keeping
/// the entry with the newest `spawned_at`. The manifest accumulates over many
/// watchdog-era respawns (historical bug, fixed by PR1+PR2) and would
/// otherwise cause `relaunch_spawned` to spawn N copies of the same role on
/// a single click. Dedupe runs on every read so callers can assume the
/// returned list has at most one entry per `(role, instance)` key.
pub(crate) fn load_spawned_from_disk(project_dir: &str) -> Vec<SpawnedAgent> {
    let path = std::path::Path::new(project_dir)
        .join(".vaak")
        .join("spawned.json");
    let raw: Vec<SpawnedAgent> = std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    let mut sorted = raw;
    sorted.sort_by(|a, b| b.spawned_at.cmp(&a.spawned_at));
    let mut seen: std::collections::HashSet<(String, i32)> = std::collections::HashSet::new();
    sorted.retain(|a| seen.insert((a.role.clone(), a.instance)));
    sorted
}

/// Opt-in gate for the dead-agent watchdog. Default: disabled.
/// Set `settings.watchdog_respawn_dead_agents = true` in project.json to
/// re-enable the auto-respawn loop. See `check_and_respawn_dead_agents`.
///
/// Pure helper — reads project.json and returns the effective bool. Exposed
/// as `pub(crate)` so tests can drive it with fixture project directories
/// without instantiating the full Tauri state.
pub(crate) fn watchdog_respawn_enabled(project_dir: &str) -> bool {
    let path = std::path::Path::new(project_dir)
        .join(".vaak")
        .join("project.json");
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.get("settings")?.get("watchdog_respawn_dead_agents")?.as_bool())
        .unwrap_or(false)
}

/// Disk manifest entry enriched with a server-side `alive` probe.
///
/// Returned by `peek_spawned_manifest` so the frontend can render the
/// PreviousTeamBanner (ux-engineer:0) without calling `is_pid_alive` from
/// JS (it can't). Shape matches ux-engineer:0 msg 155 / msg 179 spec.
#[derive(serde::Serialize, Clone, Debug)]
pub struct SpawnedManifestEntry {
    pub role: String,
    pub instance: i32,
    pub pid: u32,
    pub spawned_at: String,
    pub alive: bool,
}

/// Return the deduped `spawned.json` manifest with a fresh PID-alive probe
/// per entry. Pure read — no mutation. Frontend uses the `alive` flag to
/// decide whether a given role needs the Relaunch affordance.
///
/// Holds `spawned.lock()` across the disk read to serialize against
/// `do_spawn_member`'s in-memory push and the kill paths (tech-leader:0
/// msg 246 item 3). Partial coverage only: `do_spawn_member` releases
/// the lock before its own disk I/O, so concurrent disk writes are still
/// possible. Full coverage via a disk-level file lock is tracked as tech
/// debt (evil-architect:0 msg 243 Option 3).
#[tauri::command]
pub fn peek_spawned_manifest(
    project_dir: String,
    state: State<'_, LauncherState>,
) -> Vec<SpawnedManifestEntry> {
    let _guard = state.spawned.lock();
    load_spawned_from_disk(&project_dir)
        .into_iter()
        .map(|a| SpawnedManifestEntry {
            alive: is_pid_alive(a.pid),
            role: a.role,
            instance: a.instance,
            pid: a.pid,
            spawned_at: a.spawned_at,
        })
        .collect()
}

/// Clear the `spawned.json` manifest on human request. Wired to the Dismiss
/// action on ux-engineer:0's PreviousTeamBanner (msg 179). Writes an empty
/// Vec; the file is kept rather than deleted so path-based watchers don't
/// fire a gone-then-recreated churn. Holds `spawned.lock()` for the same
/// reason as `peek_spawned_manifest` above.
#[tauri::command]
pub fn discard_spawned_manifest(
    project_dir: String,
    state: State<'_, LauncherState>,
) -> Result<(), String> {
    let _guard = state.spawned.lock();
    save_spawned_to_disk(&project_dir, &[]);
    Ok(())
}

/// RAII guard that clears the `relaunch_in_progress` atomic on drop — covers
/// both normal exit and panic unwind. Per tech-leader:1 msg 215, evil-architect
/// msg 217, dev-challenger:1 msg 221: explicit `store(false)` in the bg thread
/// was unreachable if `do_spawn_member` / `thread::sleep` / any transitive call
/// panicked, leaving the gate stuck true forever and silently killing the
/// Relaunch feature until app restart. The guard's Drop fires on unwind, so
/// the gate always clears.
struct RelaunchGate(Arc<AtomicBool>);
impl Drop for RelaunchGate {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// Explicit human-triggered relaunch of every dead entry in `spawned.json`.
///
/// pr-relaunch-spawned (2026-04-18). Paired with PR2's reconnect-only
/// `repopulate_spawned`: alive entries are skipped (they're already in-memory
/// via reconnect); dead entries get `do_spawn_member` with a 2s stagger,
/// matching the proven cadence from `launch_team`. Runs on a background
/// thread so the command returns promptly — UI shows the spinner for the
/// queue length, not the wall-clock of all spawns.
///
/// Returns the number of dead entries queued for relaunch. Zero means the
/// manifest is empty or everyone is already alive.
#[tauri::command]
pub fn relaunch_spawned(project_dir: String, state: State<'_, LauncherState>) -> Result<u32, String> {
    // Double-click race gate (tester:1 msg 168). A second invocation that
    // arrives before the first stagger finishes would snapshot in-memory
    // before the first batch's new PIDs land — and the disk PIDs for the
    // not-yet-processed entries are still dead — so the second call would
    // queue the same roles again, producing 2x spawns. compare_exchange
    // returns Err if already true; we return Ok(0) so the UI treats this
    // as "nothing to do" rather than surfacing an error.
    if state
        .relaunch_in_progress
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        eprintln!("[launcher] relaunch_spawned: already in progress, ignoring");
        return Ok(0);
    }

    *state.project_dir.lock() = Some(project_dir.clone());
    let disk_agents = load_spawned_from_disk(&project_dir);
    let spawned_now = state.spawned.lock();

    let to_relaunch: Vec<SpawnedAgent> = disk_agents
        .into_iter()
        .filter(|a| !spawned_now.iter().any(|m| m.role == a.role && m.instance == a.instance))
        .filter(|a| !is_pid_alive(a.pid))
        .collect();
    drop(spawned_now);

    if to_relaunch.is_empty() {
        // No bg thread will start, so clear the gate directly rather than
        // constructing a one-shot guard.
        state.relaunch_in_progress.store(false, Ordering::Release);
        return Ok(0);
    }

    let queued = to_relaunch.len() as u32;
    let dir_clone = project_dir.clone();
    let spawned_clone = Arc::clone(&state.spawned);
    let gate_clone = Arc::clone(&state.relaunch_in_progress);
    std::thread::spawn(move || {
        // Guard covers normal exit AND panic unwind. The final store is no
        // longer explicit — dropping `_guard` at end-of-scope runs it.
        let _guard = RelaunchGate(gate_clone);
        let bg_state = LauncherState {
            spawned: spawned_clone,
            project_dir: Mutex::new(Some(dir_clone.clone())),
            relaunch_in_progress: Arc::new(AtomicBool::new(false)),
        };
        for (i, agent) in to_relaunch.iter().enumerate() {
            if i > 0 {
                std::thread::sleep(std::time::Duration::from_secs(2));
            }
            eprintln!("[launcher] Relaunching {}:{} (human-triggered)", agent.role, agent.instance);
            if let Err(e) = do_spawn_member(&dir_clone, &agent.role, Some(agent.instance as i32), &bg_state) {
                eprintln!("[launcher] Failed to relaunch {}:{}: {}", agent.role, agent.instance, e);
            }
        }
    });

    Ok(queued)
}

/// Re-populate the in-memory spawned list from disk on app startup.
///
/// Reconnect-only (pr-repopulate-reconnect-only, 2026-04-18). Alive PIDs are
/// attached to in-memory state so kill/status keep working across app
/// restarts. Dead entries are left in `spawned.json` as-is so a future
/// "Relaunch last team" button can offer them to the human. Nothing spawns
/// here — the old pr-respawn-dead-agents auto-restart was producing new
/// PowerShell windows on every app start without human consent.
///
/// Disk is not rewritten in this function. Alive and dead entries in the
/// existing file remain untouched; launch/kill paths update the file on
/// their own actions. Removing the in-function write also closes the
/// TOCTOU window evil-architect:0 flagged in msg 67 — dead entries are
/// never transiently dropped.
///
/// Returns the count of alive agents reconnected from disk.
#[tauri::command]
pub fn repopulate_spawned(project_dir: String, state: State<'_, LauncherState>) -> Result<u32, String> {
    *state.project_dir.lock() = Some(project_dir.clone());
    let disk_agents = load_spawned_from_disk(&project_dir);
    let mut spawned = state.spawned.lock();
    let mut alive_count = 0u32;

    for agent in disk_agents {
        if is_pid_alive(agent.pid) {
            if !spawned.iter().any(|a| a.role == agent.role && a.instance == agent.instance) {
                eprintln!("[launcher] Reconnected to alive agent: {}:{} (PID {})", agent.role, agent.instance, agent.pid);
                spawned.push(agent);
                alive_count += 1;
            }
        } else {
            eprintln!("[launcher] Agent {}:{} (PID {}) is dead — leaving in manifest for Relaunch", agent.role, agent.instance, agent.pid);
        }
    }

    Ok(alive_count)
}

/// Periodic watchdog: detect spawned agents whose PID is dead OR whose
/// vaak heartbeat has gone stale beyond a threshold, and auto-respawn them.
///
/// Why both checks (per human msg 512/515): the prior `is_pid_alive`-only
/// approach misses the common case where Claude exits but PowerShell (with
/// `-NoExit`) stays open — PID is alive, but no heartbeats are reaching
/// sessions.json, so the agent is effectively dead from vaak's POV. The
/// human reported this exact pattern: "they finish and they like sign off
/// and I have to open the powershell and buzz them back in."
///
/// `stale_threshold_secs` defaults to 90s if not provided. Below the
/// project's `heartbeat_timeout_seconds` (typically 300s) so respawn
/// happens before the agent is fully declared gone, giving a fresh
/// process time to claim the role before the slot is reassigned.
///
/// Returns the count of agents respawned this call. Frontend should call
/// on a setInterval (recommended ~60s).
///
/// pr-watchdog-opt-in (2026-04-18): the watchdog now early-returns unless
/// `settings.watchdog_respawn_dead_agents == true` in project.json. The human
/// reported the app was spawning new PowerShells every ~1-2 minutes because
/// this watchdog kept respawning last-session's dead entries. New rule:
/// roles only launch when the human clicks. Keep the detection code intact
/// so a future "Relaunch dead" button can reuse it on demand.
#[tauri::command]
pub fn check_and_respawn_dead_agents(
    project_dir: String,
    stale_threshold_secs: Option<u64>,
    state: State<'_, LauncherState>,
) -> Result<u32, String> {
    if !watchdog_respawn_enabled(&project_dir) {
        return Ok(0);
    }

    let threshold = stale_threshold_secs.unwrap_or(90);
    *state.project_dir.lock() = Some(project_dir.clone());

    let spawned_now: Vec<SpawnedAgent> = state.spawned.lock().clone();
    if spawned_now.is_empty() {
        return Ok(0);
    }

    // Read sessions.json for heartbeat lookup
    let sessions_path = std::path::Path::new(&project_dir)
        .join(".vaak")
        .join("sessions.json");
    let sessions_val: serde_json::Value = std::fs::read_to_string(&sessions_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(serde_json::json!({"bindings": []}));

    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let bindings = sessions_val.get("bindings")
        .and_then(|b| b.as_array())
        .cloned()
        .unwrap_or_default();

    let mut to_respawn: Vec<SpawnedAgent> = Vec::new();
    for agent in &spawned_now {
        let pid_dead = !is_pid_alive(agent.pid);

        // Find this agent's session record by role + instance
        let session = bindings.iter().find(|b| {
            let role = b.get("role").and_then(|r| r.as_str()).unwrap_or("");
            let inst = b.get("instance").and_then(|i| i.as_u64()).unwrap_or(0);
            role == agent.role && inst as i32 == agent.instance
        });

        let heartbeat_stale = match session.and_then(|s| s.get("last_heartbeat").and_then(|h| h.as_str())) {
            Some(hb_iso) => {
                // Reuse the same epoch parser the MCP sidecar uses
                match parse_iso_to_secs(hb_iso) {
                    Some(hb_secs) => now_secs.saturating_sub(hb_secs) > threshold,
                    None => true, // unparseable timestamp = treat as stale
                }
            }
            None => true, // no session record = stale
        };

        if pid_dead || heartbeat_stale {
            eprintln!(
                "[launcher] Agent {}:{} needs respawn (pid_dead={}, heartbeat_stale={})",
                agent.role, agent.instance, pid_dead, heartbeat_stale
            );
            to_respawn.push(agent.clone());
        }
    }

    if to_respawn.is_empty() {
        return Ok(0);
    }

    let respawn_count = to_respawn.len() as u32;

    // Remove dead/stale entries from in-memory + disk before respawn so the
    // about-to-spawn entries don't double-count
    {
        let mut spawned_lock = state.spawned.lock();
        spawned_lock.retain(|a| !to_respawn.iter().any(|r| r.role == a.role && r.instance == a.instance));
        let snapshot: Vec<SpawnedAgent> = spawned_lock.clone();
        drop(spawned_lock);
        save_spawned_to_disk(&project_dir, &snapshot);
    }

    // Background respawn with stagger
    let dir_clone = project_dir.clone();
    let spawned_clone = Arc::clone(&state.spawned);
    std::thread::spawn(move || {
        let bg_state = LauncherState {
            spawned: spawned_clone,
            project_dir: Mutex::new(Some(dir_clone.clone())),
            relaunch_in_progress: Arc::new(AtomicBool::new(false)),
        };
        for (i, agent) in to_respawn.iter().enumerate() {
            if i > 0 {
                std::thread::sleep(std::time::Duration::from_secs(2));
            }
            eprintln!("[launcher] Respawning {}:{} (watchdog)", agent.role, agent.instance);
            if let Err(e) = do_spawn_member(&dir_clone, &agent.role, Some(agent.instance as i32), &bg_state) {
                eprintln!("[launcher] Watchdog respawn failed for {}:{}: {}", agent.role, agent.instance, e);
            }
        }
    });

    Ok(respawn_count)
}

/// Parse an ISO-8601 string ("2026-04-17T00:44:17Z") to seconds since
/// epoch. Minimal — handles the format vaak's heartbeats use, doesn't
/// pull chrono just for this.
fn parse_iso_to_secs(iso: &str) -> Option<u64> {
    // Format: YYYY-MM-DDTHH:MM:SSZ — fixed offsets
    if iso.len() < 19 { return None; }
    let year: i64 = iso[0..4].parse().ok()?;
    let month: i64 = iso[5..7].parse().ok()?;
    let day: i64 = iso[8..10].parse().ok()?;
    let hour: i64 = iso[11..13].parse().ok()?;
    let minute: i64 = iso[14..16].parse().ok()?;
    let second: i64 = iso[17..19].parse().ok()?;

    // Days from epoch to year start (Howard Hinnant's algorithm, simplified)
    let y = if month <= 2 { year - 1 } else { year };
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = (y - era * 400) as i64;
    let doy = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days_since_epoch = era * 146097 + doe - 719468;

    let total = days_since_epoch * 86400 + hour * 3600 + minute * 60 + second;
    if total < 0 { None } else { Some(total as u64) }
}

/// Check if a PID is still alive (cross-platform)
fn is_pid_alive(pid: u32) -> bool {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        Command::new("tasklist")
            .args(["/FI", &format!("PID eq {}", pid), "/NH"])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map(|o| {
                let out = String::from_utf8_lossy(&o.stdout);
                out.contains(&pid.to_string())
            })
            .unwrap_or(false)
    }
    #[cfg(not(target_os = "windows"))]
    {
        // Use signal 0 to check if process exists without killing it.
        // Works on both macOS and Linux (unlike /proc which is Linux-only).
        Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stderr(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
}

/// Check macOS TCC permissions needed for the app to function.
/// Returns a JSON-serializable struct with boolean fields for each permission.
/// On non-macOS platforms, all permissions return true (not applicable).
#[derive(serde::Serialize)]
pub struct MacPermissions {
    pub automation: bool,
    pub accessibility: bool,
    pub screen_recording: bool,
    pub platform: String,
}

#[tauri::command]
pub fn check_macos_permissions() -> MacPermissions {
    #[cfg(target_os = "macos")]
    {
        // Test Automation permission: can we talk to Terminal.app?
        let automation = Command::new("osascript")
            .args(["-e", r#"tell application "Terminal" to get name"#])
            .output()
            .map(|o| {
                if o.status.success() {
                    true
                } else {
                    let stderr = String::from_utf8_lossy(&o.stderr);
                    // -1743 = "not allowed assistive access" / Automation denied
                    !(stderr.contains("not allowed") || stderr.contains("-1743"))
                }
            })
            .unwrap_or(false);

        // Test Accessibility permission: can we use System Events?
        let accessibility = Command::new("osascript")
            .args(["-e", r#"tell application "System Events" to get name of first process whose frontmost is true"#])
            .output()
            .map(|o| {
                if o.status.success() {
                    true
                } else {
                    let stderr = String::from_utf8_lossy(&o.stderr);
                    !(stderr.contains("not allowed") || stderr.contains("-1743"))
                }
            })
            .unwrap_or(false);

        // Test Screen Recording permission via CoreGraphics preflight
        let screen_recording = {
            #[link(name = "CoreGraphics", kind = "framework")]
            extern "C" {
                fn CGPreflightScreenCaptureAccess() -> u8;
            }
            unsafe { CGPreflightScreenCaptureAccess() != 0 }
        };

        MacPermissions {
            automation,
            accessibility,
            screen_recording,
            platform: "macos".to_string(),
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        MacPermissions {
            automation: true,
            accessibility: true,
            screen_recording: true,
            platform: if cfg!(target_os = "windows") { "windows" } else { "linux" }.to_string(),
        }
    }
}

/// Open a macOS System Settings pane URL (e.g., x-apple.systempreferences:...).
/// On non-macOS platforms this is a no-op.
#[tauri::command]
pub fn open_macos_settings(pane_url: String) -> Result<(), String> {
    // Only allow macOS system preferences URLs — reject arbitrary URLs/paths
    if !pane_url.starts_with("x-apple.systempreferences:") {
        return Err("Invalid settings pane URL".to_string());
    }
    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .arg(&pane_url)
            .output()
            .map_err(|e| format!("Failed to open System Settings: {}", e))?;
    }
    Ok(())
}

/// Open a URL in the system's default browser.
/// Only allows https:// URLs to prevent arbitrary command execution.
#[tauri::command]
pub fn open_url_in_browser(url: String) -> Result<(), String> {
    if !url.starts_with("https://") {
        return Err("Only https:// URLs are allowed".to_string());
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        Command::new("cmd")
            .args(["/c", "start", "", &url])
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|e| format!("Failed to open URL: {}", e))?;
    }
    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .arg(&url)
            .spawn()
            .map_err(|e| format!("Failed to open URL: {}", e))?;
    }
    #[cfg(target_os = "linux")]
    {
        Command::new("xdg-open")
            .arg(&url)
            .spawn()
            .map_err(|e| format!("Failed to open URL: {}", e))?;
    }
    Ok(())
}

/// Check if npm is available on the system PATH.
/// On macOS, uses a login shell to pick up nvm/fnm/homebrew paths.
#[tauri::command]
pub fn check_npm_installed() -> Result<bool, String> {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        let output = Command::new("where")
            .arg("npm")
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|e| format!("Failed to run 'where npm': {}", e))?;
        Ok(output.status.success())
    }
    #[cfg(target_os = "macos")]
    {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
        let output = Command::new(&shell)
            .args(["-l", "-c", "which npm"])
            .output()
            .map_err(|e| format!("Failed to check for npm: {}", e))?;
        Ok(output.status.success())
    }
    #[cfg(target_os = "linux")]
    {
        let output = Command::new("which")
            .arg("npm")
            .output()
            .map_err(|e| format!("Failed to run 'which npm': {}", e))?;
        Ok(output.status.success())
    }
}

/// Install Claude Code CLI via npm. Returns Ok(output) on success.
/// Runs `npm install -g @anthropic-ai/claude-code` with a 120-second timeout.
/// Uses login shell on macOS to pick up nvm/fnm/homebrew PATH.
#[tauri::command]
pub fn install_claude_cli() -> Result<String, String> {
    let timeout = std::time::Duration::from_secs(120);

    #[cfg(target_os = "windows")]
    let mut child = {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        Command::new("npm")
            .args(["install", "-g", "@anthropic-ai/claude-code"])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|e| format!("Failed to start npm install: {}", e))?
    };

    #[cfg(target_os = "macos")]
    let mut child = {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
        Command::new(&shell)
            .args(["-l", "-c", "npm install -g @anthropic-ai/claude-code"])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| format!("Failed to start npm install: {}", e))?
    };

    #[cfg(target_os = "linux")]
    let mut child = {
        Command::new("npm")
            .args(["install", "-g", "@anthropic-ai/claude-code"])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| format!("Failed to start npm install: {}", e))?
    };

    // Poll with timeout instead of blocking indefinitely
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let stdout = child.stdout.take()
                    .map(|mut s| { let mut buf = String::new(); std::io::Read::read_to_string(&mut s, &mut buf).ok(); buf })
                    .unwrap_or_default();
                let stderr = child.stderr.take()
                    .map(|mut s| { let mut buf = String::new(); std::io::Read::read_to_string(&mut s, &mut buf).ok(); buf })
                    .unwrap_or_default();

                if status.success() {
                    return Ok(stdout);
                } else {
                    let msg = if stderr.contains("EACCES") || stderr.contains("permission denied") {
                        "Permission denied. On macOS/Linux, try: sudo npm install -g @anthropic-ai/claude-code".to_string()
                    } else {
                        format!("npm install failed: {}", stderr.trim())
                    };
                    return Err(msg);
                }
            }
            Ok(None) => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    return Err("Installation timed out after 120 seconds. Check your network connection and try again.".to_string());
                }
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
            Err(e) => return Err(format!("Failed to check install status: {}", e)),
        }
    }
}

/// Check if Homebrew is installed (macOS only). Returns false on other platforms.
#[tauri::command]
pub fn check_homebrew_installed() -> Result<bool, String> {
    #[cfg(target_os = "macos")]
    {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
        let output = Command::new(&shell)
            .args(["-l", "-c", "which brew"])
            .output()
            .map_err(|e| format!("Failed to check for brew: {}", e))?;
        Ok(output.status.success())
    }
    #[cfg(not(target_os = "macos"))]
    {
        Ok(false)
    }
}

/// Install Node.js via the best available method for the platform.
/// macOS: tries Homebrew first (`brew install node`), falls back to opening nodejs.org.
/// Windows/Linux: opens nodejs.org download page.
/// Returns Ok("installed") on success, Ok("browser") if it fell back to opening the browser.
#[tauri::command]
pub fn install_nodejs() -> Result<String, String> {
    #[cfg(target_os = "macos")]
    {
        // Try Homebrew first
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
        let brew_check = Command::new(&shell)
            .args(["-l", "-c", "which brew"])
            .output()
            .ok();

        if brew_check.as_ref().map(|o| o.status.success()).unwrap_or(false) {
            // Homebrew is available — install Node.js
            let timeout = std::time::Duration::from_secs(180);
            let mut child = Command::new(&shell)
                .args(["-l", "-c", "brew install node"])
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .map_err(|e| format!("Failed to start brew install: {}", e))?;

            let start = std::time::Instant::now();
            loop {
                match child.try_wait() {
                    Ok(Some(status)) => {
                        if status.success() {
                            return Ok("installed".to_string());
                        } else {
                            let stderr = child.stderr.take()
                                .map(|mut s| { let mut buf = String::new(); std::io::Read::read_to_string(&mut s, &mut buf).ok(); buf })
                                .unwrap_or_default();
                            return Err(format!("brew install node failed: {}", stderr.trim()));
                        }
                    }
                    Ok(None) => {
                        if start.elapsed() > timeout {
                            let _ = child.kill();
                            return Err("Installation timed out after 180 seconds".to_string());
                        }
                        std::thread::sleep(std::time::Duration::from_millis(500));
                    }
                    Err(e) => return Err(format!("Failed to check install status: {}", e)),
                }
            }
        }

        // No Homebrew — fall back to opening browser
        Command::new("open")
            .arg("https://nodejs.org")
            .spawn()
            .map_err(|e| format!("Failed to open browser: {}", e))?;
        return Ok("browser".to_string());
    }

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        Command::new("cmd")
            .args(["/c", "start", "", "https://nodejs.org"])
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|e| format!("Failed to open browser: {}", e))?;
        Ok("browser".to_string())
    }

    #[cfg(target_os = "linux")]
    {
        Command::new("xdg-open")
            .arg("https://nodejs.org")
            .spawn()
            .map_err(|e| format!("Failed to open browser: {}", e))?;
        Ok("browser".to_string())
    }
}

/// Open a terminal window in the given directory.
/// macOS: opens Terminal.app; Windows: opens PowerShell; Linux: opens default terminal.
#[tauri::command]
pub fn open_terminal_in_dir(dir: String) -> Result<(), String> {
    let path = Path::new(&dir);
    if !path.exists() {
        return Err(format!("Directory does not exist: {}", dir));
    }

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        // Use cmd /c start with .current_dir() to avoid interpolating the path
        // into a PowerShell string, which would allow injection via $(), backticks, etc.
        Command::new("cmd")
            .args(["/c", "start", "powershell", "-NoExit"])
            .current_dir(&dir)
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|e| format!("Failed to open PowerShell: {}", e))?;
    }

    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .args(["-a", "Terminal", &dir])
            .spawn()
            .map_err(|e| format!("Failed to open Terminal.app: {}", e))?;
    }

    #[cfg(target_os = "linux")]
    {
        // Use .current_dir() so every terminal inherits the right working directory,
        // regardless of whether it supports --workdir flags.
        let launched = Command::new("x-terminal-emulator")
            .current_dir(&dir)
            .spawn()
            .is_ok()
            || Command::new("gnome-terminal")
                .args(["--working-directory", &dir])
                .spawn()
                .is_ok()
            || Command::new("xterm")
                .current_dir(&dir)
                .spawn()
                .is_ok();
        if !launched {
            return Err("No terminal emulator found".to_string());
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── parse_iso_to_secs (pr-respawn-dead-agents) ─────────────────────
    // Locks the minimal ISO-8601 parser the watchdog uses. Drift here would
    // mean the heartbeat-staleness check silently uses bogus epoch values
    // and either over-triggers (respawning live agents) or under-triggers
    // (leaving dead agents alive).

    #[test]
    fn parse_iso_to_secs_handles_known_recent_timestamp() {
        // 2026-04-17T00:44:17Z is from real sessions.json output during the
        // session that triggered this PR. Parsed value should be a positive
        // u64; we don't assert exact since UNIX_EPOCH math depends on locale
        // settings on Windows, but cross-check that round-trip via the
        // current clock keeps parser+now within 5-year sanity bounds.
        let result = parse_iso_to_secs("2026-04-17T00:44:17Z");
        assert!(result.is_some(), "should parse the heartbeat shape");
        let secs = result.unwrap();
        // Reasonable sanity: between 2020 (1577836800) and 2050 (2524608000)
        assert!(secs > 1_577_836_800 && secs < 2_524_608_000,
                "epoch should land in [2020, 2050], got {}", secs);
    }

    #[test]
    fn parse_iso_to_secs_rejects_short_string() {
        assert!(parse_iso_to_secs("").is_none());
        assert!(parse_iso_to_secs("2026-04-17").is_none());
        assert!(parse_iso_to_secs("not-a-date").is_none());
    }

    #[test]
    fn parse_iso_to_secs_returns_increasing_for_later_times() {
        // Ordering test — if A is later than B, parse(A) > parse(B).
        // Catches off-by-one or month/day swap bugs.
        let earlier = parse_iso_to_secs("2026-04-17T00:00:00Z").unwrap();
        let later = parse_iso_to_secs("2026-04-17T00:44:17Z").unwrap();
        let next_day = parse_iso_to_secs("2026-04-18T00:00:00Z").unwrap();
        assert!(later > earlier, "later time should yield larger epoch");
        assert!(next_day > later, "next day should yield larger epoch");
        assert_eq!(later - earlier, 44 * 60 + 17, "44m17s difference");
    }

    // ── build_join_prompt ──────────────────────────────────────────────

    #[test]
    fn test_build_join_prompt_contains_role() {
        let prompt = build_join_prompt("developer");
        assert!(prompt.contains("developer"), "prompt should contain role name");
        assert!(prompt.contains("project_join"), "prompt should mention project_join");
        assert!(prompt.contains("project_wait"), "prompt should mention project_wait");
    }

    #[test]
    fn test_build_join_prompt_different_roles() {
        let architect = build_join_prompt("architect");
        let tester = build_join_prompt("tester");
        assert!(architect.contains("architect"));
        assert!(tester.contains("tester"));
        assert_ne!(architect, tester, "different roles should produce different prompts");
    }

    #[test]
    fn test_build_join_prompt_special_chars_in_role() {
        let prompt = build_join_prompt("evil-architect");
        assert!(prompt.contains("evil-architect"));
    }

    // ── LauncherState ──────────────────────────────────────────────────

    #[test]
    fn test_launcher_state_default() {
        let state = LauncherState::default();
        assert!(state.spawned.lock().is_empty(), "spawned list should start empty");
        assert!(state.project_dir.lock().is_none(), "project_dir should start as None");
    }

    #[test]
    fn test_launcher_state_push_agent() {
        let state = LauncherState::default();
        let mut spawned = state.spawned.lock();
        spawned.push(SpawnedAgent {
            pid: 12345,
            role: "developer".to_string(),
            instance: 0,
            spawned_at: "2026-03-04T00:00:00Z".to_string(),
        });
        assert_eq!(spawned.len(), 1);
        assert_eq!(spawned[0].pid, 12345);
        assert_eq!(spawned[0].role, "developer");
        assert_eq!(spawned[0].instance, 0);
    }

    #[test]
    fn test_launcher_state_multiple_agents() {
        let state = LauncherState::default();
        let mut spawned = state.spawned.lock();
        for i in 0..3 {
            spawned.push(SpawnedAgent {
                pid: 100 + i,
                role: "developer".to_string(),
                instance: i as i32,
                spawned_at: "2026-03-04T00:00:00Z".to_string(),
            });
        }
        assert_eq!(spawned.len(), 3);
        assert_eq!(spawned.iter().filter(|a| a.role == "developer").count(), 3);
    }

    // ── get_companions ─────────────────────────────────────────────────

    #[test]
    fn test_get_companions_missing_project_json() {
        let result = get_companions("/tmp/nonexistent-vaak-test-dir-12345", "moderator");
        assert!(result.is_empty(), "missing project.json should return empty");
    }

    #[test]
    fn test_get_companions_no_companions_key() {
        let tmp = std::env::temp_dir().join("vaak-test-companions-no-key");
        let vaak_dir = tmp.join(".vaak");
        let _ = std::fs::create_dir_all(&vaak_dir);
        let config = serde_json::json!({
            "roles": {
                "developer": {
                    "title": "Developer"
                }
            }
        });
        std::fs::write(vaak_dir.join("project.json"), config.to_string()).unwrap();

        let result = get_companions(tmp.to_str().unwrap(), "developer");
        assert!(result.is_empty(), "role without companions should return empty");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_get_companions_string_format() {
        let tmp = std::env::temp_dir().join("vaak-test-companions-str");
        let vaak_dir = tmp.join(".vaak");
        let _ = std::fs::create_dir_all(&vaak_dir);
        let config = serde_json::json!({
            "roles": {
                "moderator": {
                    "title": "Moderator",
                    "companions": ["audience"]
                }
            }
        });
        std::fs::write(vaak_dir.join("project.json"), config.to_string()).unwrap();

        let result = get_companions(tmp.to_str().unwrap(), "moderator");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, "audience");
        assert_eq!(result[0].1, true, "string format should default to enabled");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_get_companions_object_format() {
        let tmp = std::env::temp_dir().join("vaak-test-companions-obj");
        let vaak_dir = tmp.join(".vaak");
        let _ = std::fs::create_dir_all(&vaak_dir);
        let config = serde_json::json!({
            "roles": {
                "moderator": {
                    "title": "Moderator",
                    "companions": [
                        {"role": "audience", "optional": true, "default_enabled": false},
                        {"role": "stats-auditor", "optional": false, "default_enabled": true}
                    ]
                }
            }
        });
        std::fs::write(vaak_dir.join("project.json"), config.to_string()).unwrap();

        let result = get_companions(tmp.to_str().unwrap(), "moderator");
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].0, "audience");
        assert_eq!(result[0].1, false, "audience should be disabled");
        assert_eq!(result[1].0, "stats-auditor");
        assert_eq!(result[1].1, true, "stats-auditor should be enabled");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_get_companions_nonexistent_role() {
        let tmp = std::env::temp_dir().join("vaak-test-companions-norol");
        let vaak_dir = tmp.join(".vaak");
        let _ = std::fs::create_dir_all(&vaak_dir);
        let config = serde_json::json!({
            "roles": {
                "developer": { "title": "Developer" }
            }
        });
        std::fs::write(vaak_dir.join("project.json"), config.to_string()).unwrap();

        let result = get_companions(tmp.to_str().unwrap(), "nonexistent-role");
        assert!(result.is_empty());

        let _ = std::fs::remove_dir_all(&tmp);
    }

    // ── save/load spawned to disk ──────────────────────────────────────

    #[test]
    fn test_save_and_load_spawned_roundtrip() {
        let tmp = std::env::temp_dir().join("vaak-test-spawned-rt");
        let vaak_dir = tmp.join(".vaak");
        let _ = std::fs::create_dir_all(&vaak_dir);

        let agents = vec![
            SpawnedAgent {
                pid: 111,
                role: "developer".to_string(),
                instance: 0,
                spawned_at: "2026-03-04T10:00:00Z".to_string(),
            },
            SpawnedAgent {
                pid: 222,
                role: "tester".to_string(),
                instance: 0,
                spawned_at: "2026-03-04T10:01:00Z".to_string(),
            },
        ];

        save_spawned_to_disk(tmp.to_str().unwrap(), &agents);
        let loaded = load_spawned_from_disk(tmp.to_str().unwrap());

        // pr-manifest-durability (e06a32e) added dedupe-on-load which sorts by
        // spawned_at DESC, so positional indexing is no longer stable. Assert
        // presence by (role, instance) lookup instead.
        assert_eq!(loaded.len(), 2);
        let dev = loaded.iter().find(|a| a.role == "developer" && a.instance == 0)
            .expect("developer:0 entry missing after round-trip");
        assert_eq!(dev.pid, 111);
        let tester = loaded.iter().find(|a| a.role == "tester" && a.instance == 0)
            .expect("tester:0 entry missing after round-trip");
        assert_eq!(tester.pid, 222);

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_load_spawned_missing_file() {
        let result = load_spawned_from_disk("/tmp/nonexistent-vaak-test-99999");
        assert!(result.is_empty(), "missing file should return empty vec");
    }

    #[test]
    fn test_save_spawned_empty_list() {
        let tmp = std::env::temp_dir().join("vaak-test-spawned-empty");
        let vaak_dir = tmp.join(".vaak");
        let _ = std::fs::create_dir_all(&vaak_dir);

        save_spawned_to_disk(tmp.to_str().unwrap(), &[]);
        let loaded = load_spawned_from_disk(tmp.to_str().unwrap());
        assert!(loaded.is_empty());

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_save_spawned_overwrites_previous() {
        let tmp = std::env::temp_dir().join("vaak-test-spawned-overwrite");
        let vaak_dir = tmp.join(".vaak");
        let _ = std::fs::create_dir_all(&vaak_dir);

        let agents1 = vec![SpawnedAgent {
            pid: 111,
            role: "developer".to_string(),
            instance: 0,
            spawned_at: "2026-03-04T10:00:00Z".to_string(),
        }];
        save_spawned_to_disk(tmp.to_str().unwrap(), &agents1);

        let agents2 = vec![SpawnedAgent {
            pid: 999,
            role: "architect".to_string(),
            instance: 0,
            spawned_at: "2026-03-04T11:00:00Z".to_string(),
        }];
        save_spawned_to_disk(tmp.to_str().unwrap(), &agents2);

        let loaded = load_spawned_from_disk(tmp.to_str().unwrap());
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].pid, 999);
        assert_eq!(loaded[0].role, "architect");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    // ── revoke_session ─────────────────────────────────────────────────

    #[test]
    fn test_revoke_session_removes_target() {
        let tmp = std::env::temp_dir().join("vaak-test-revoke-1");
        let vaak_dir = tmp.join(".vaak");
        let _ = std::fs::create_dir_all(&vaak_dir);

        let sessions = serde_json::json!({
            "bindings": [
                {"session_id": "s1", "role": "developer", "instance": 0},
                {"session_id": "s2", "role": "tester", "instance": 0},
                {"session_id": "s3", "role": "developer", "instance": 1}
            ]
        });
        std::fs::write(vaak_dir.join("sessions.json"), sessions.to_string()).unwrap();

        revoke_session(tmp.to_str().unwrap(), "developer", 0).unwrap();

        let content = std::fs::read_to_string(vaak_dir.join("sessions.json")).unwrap();
        let result: serde_json::Value = serde_json::from_str(&content).unwrap();
        let bindings = result["bindings"].as_array().unwrap();
        assert_eq!(bindings.len(), 2, "should have removed developer:0");
        assert_eq!(bindings[0]["role"].as_str().unwrap(), "tester");
        assert_eq!(bindings[1]["role"].as_str().unwrap(), "developer");
        assert_eq!(bindings[1]["instance"].as_i64().unwrap(), 1);

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_revoke_session_no_match() {
        let tmp = std::env::temp_dir().join("vaak-test-revoke-nomatch");
        let vaak_dir = tmp.join(".vaak");
        let _ = std::fs::create_dir_all(&vaak_dir);

        let sessions = serde_json::json!({
            "bindings": [
                {"session_id": "s1", "role": "developer", "instance": 0}
            ]
        });
        std::fs::write(vaak_dir.join("sessions.json"), sessions.to_string()).unwrap();

        revoke_session(tmp.to_str().unwrap(), "architect", 0).unwrap();

        let content = std::fs::read_to_string(vaak_dir.join("sessions.json")).unwrap();
        let result: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(result["bindings"].as_array().unwrap().len(), 1);

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_revoke_session_missing_file() {
        let result = revoke_session("/tmp/nonexistent-vaak-test-99999", "developer", 0);
        assert!(result.is_err(), "missing sessions.json should return error");
    }

    // ── revoke_all_sessions ────────────────────────────────────────────

    #[test]
    fn test_revoke_all_sessions_keeps_human() {
        let tmp = std::env::temp_dir().join("vaak-test-revoke-all");
        let vaak_dir = tmp.join(".vaak");
        let _ = std::fs::create_dir_all(&vaak_dir);

        let sessions = serde_json::json!({
            "bindings": [
                {"session_id": "s1", "role": "human", "instance": 0},
                {"session_id": "s2", "role": "developer", "instance": 0},
                {"session_id": "s3", "role": "architect", "instance": 0},
                {"session_id": "s4", "role": "tester", "instance": 0}
            ]
        });
        std::fs::write(vaak_dir.join("sessions.json"), sessions.to_string()).unwrap();

        revoke_all_sessions(tmp.to_str().unwrap()).unwrap();

        let content = std::fs::read_to_string(vaak_dir.join("sessions.json")).unwrap();
        let result: serde_json::Value = serde_json::from_str(&content).unwrap();
        let bindings = result["bindings"].as_array().unwrap();
        assert_eq!(bindings.len(), 1, "only human should remain");
        assert_eq!(bindings[0]["role"].as_str().unwrap(), "human");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_revoke_all_sessions_empty_bindings() {
        let tmp = std::env::temp_dir().join("vaak-test-revoke-all-empty");
        let vaak_dir = tmp.join(".vaak");
        let _ = std::fs::create_dir_all(&vaak_dir);

        let sessions = serde_json::json!({"bindings": []});
        std::fs::write(vaak_dir.join("sessions.json"), sessions.to_string()).unwrap();

        revoke_all_sessions(tmp.to_str().unwrap()).unwrap();

        let content = std::fs::read_to_string(vaak_dir.join("sessions.json")).unwrap();
        let result: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(result["bindings"].as_array().unwrap().len(), 0);

        let _ = std::fs::remove_dir_all(&tmp);
    }

    // ── kill_tracked_agent ─────────────────────────────────────────────

    #[test]
    fn test_kill_tracked_agent_not_found() {
        let state = LauncherState::default();
        let result = kill_tracked_agent("nonexistent", 0, &state);
        assert!(!result, "should return false when no matching agent");
    }

    // ── SpawnedAgent serialization ─────────────────────────────────────

    #[test]
    fn test_spawned_agent_json_roundtrip() {
        let agent = SpawnedAgent {
            pid: 42,
            role: "evil-architect".to_string(),
            instance: 1,
            spawned_at: "2026-03-04T12:00:00Z".to_string(),
        };
        let json = serde_json::to_string(&agent).unwrap();
        let deserialized: SpawnedAgent = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.pid, 42);
        assert_eq!(deserialized.role, "evil-architect");
        assert_eq!(deserialized.instance, 1);
        assert_eq!(deserialized.spawned_at, "2026-03-04T12:00:00Z");
    }

    #[test]
    fn test_spawned_agent_vec_serialization() {
        let agents = vec![
            SpawnedAgent { pid: 1, role: "a".into(), instance: 0, spawned_at: "t1".into() },
            SpawnedAgent { pid: 2, role: "b".into(), instance: 1, spawned_at: "t2".into() },
        ];
        let json = serde_json::to_string_pretty(&agents).unwrap();
        let loaded: Vec<SpawnedAgent> = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.len(), 2);
    }

    // ── watchdog_respawn_enabled (PR1.5 — pr-watchdog-opt-in, commit 7f73972) ──
    //
    // The watchdog gate is the difference between "app quietly does nothing on a
    // 60s tick" and "app auto-spawns 10 PowerShell windows every 60s until the
    // user closes it." Every fallback path here must collapse to `false` —
    // default-off is the contract the human's msg 71 directive rests on.

    fn write_project_json(vaak_dir: &std::path::Path, body: &str) {
        std::fs::create_dir_all(vaak_dir).unwrap();
        std::fs::write(vaak_dir.join("project.json"), body).unwrap();
    }

    #[test]
    fn watchdog_respawn_enabled_returns_false_when_file_missing() {
        let tmp = tempfile::tempdir().unwrap();
        // .vaak/ doesn't exist — simulate fresh install.
        assert!(!watchdog_respawn_enabled(tmp.path().to_str().unwrap()),
                "missing project.json must collapse to false (default-off contract)");
    }

    #[test]
    fn watchdog_respawn_enabled_returns_false_on_malformed_json() {
        let tmp = tempfile::tempdir().unwrap();
        write_project_json(&tmp.path().join(".vaak"), "{ not valid json");
        assert!(!watchdog_respawn_enabled(tmp.path().to_str().unwrap()),
                "garbage project.json must not panic and must fall back to false");
    }

    #[test]
    fn watchdog_respawn_enabled_returns_false_when_settings_missing() {
        let tmp = tempfile::tempdir().unwrap();
        write_project_json(&tmp.path().join(".vaak"), r#"{"project_name": "x"}"#);
        assert!(!watchdog_respawn_enabled(tmp.path().to_str().unwrap()),
                "valid project.json with no settings block → false");
    }

    #[test]
    fn watchdog_respawn_enabled_returns_false_when_key_missing() {
        let tmp = tempfile::tempdir().unwrap();
        write_project_json(
            &tmp.path().join(".vaak"),
            r#"{"settings": {"some_other_flag": true}}"#,
        );
        assert!(!watchdog_respawn_enabled(tmp.path().to_str().unwrap()),
                "settings block present but key absent → false");
    }

    #[test]
    fn watchdog_respawn_enabled_returns_false_for_non_bool_values() {
        // Paranoid coverage: truthy-but-not-bool values must NOT flip the gate on.
        // serde_json's `.as_bool()` returns None for strings and numbers; paired
        // with `.unwrap_or(false)` in the helper, every case lands on false.
        let tmp = tempfile::tempdir().unwrap();
        let vaak = tmp.path().join(".vaak");

        for body in [
            r#"{"settings": {"watchdog_respawn_dead_agents": "true"}}"#,
            r#"{"settings": {"watchdog_respawn_dead_agents": 1}}"#,
            r#"{"settings": {"watchdog_respawn_dead_agents": "yes"}}"#,
            r#"{"settings": {"watchdog_respawn_dead_agents": null}}"#,
        ] {
            write_project_json(&vaak, body);
            assert!(
                !watchdog_respawn_enabled(tmp.path().to_str().unwrap()),
                "non-bool value {} must not enable the watchdog",
                body
            );
        }
    }

    #[test]
    fn watchdog_respawn_enabled_returns_false_when_explicitly_false() {
        let tmp = tempfile::tempdir().unwrap();
        write_project_json(
            &tmp.path().join(".vaak"),
            r#"{"settings": {"watchdog_respawn_dead_agents": false}}"#,
        );
        assert!(!watchdog_respawn_enabled(tmp.path().to_str().unwrap()));
    }

    #[test]
    fn watchdog_respawn_enabled_returns_true_only_on_explicit_true() {
        let tmp = tempfile::tempdir().unwrap();
        write_project_json(
            &tmp.path().join(".vaak"),
            r#"{"settings": {"watchdog_respawn_dead_agents": true}}"#,
        );
        assert!(watchdog_respawn_enabled(tmp.path().to_str().unwrap()),
                "explicit true must enable — legacy users who want the old behavior");
    }

    // ── spawned.json round-trip via load/save helpers ──────────────────────
    //
    // Baseline guarantees that the dev-challenger:1 msg 161 repro test below
    // can trust as invariants.

    #[test]
    fn load_spawned_from_disk_returns_empty_when_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let agents = load_spawned_from_disk(tmp.path().to_str().unwrap());
        assert!(agents.is_empty(), "missing file → empty Vec, never panics");
    }

    #[test]
    fn load_spawned_from_disk_handles_malformed_json() {
        let tmp = tempfile::tempdir().unwrap();
        let vaak = tmp.path().join(".vaak");
        std::fs::create_dir_all(&vaak).unwrap();
        std::fs::write(vaak.join("spawned.json"), "not json").unwrap();
        let agents = load_spawned_from_disk(tmp.path().to_str().unwrap());
        assert!(agents.is_empty(),
                "corrupt spawned.json must not panic Tauri startup");
    }

    #[test]
    fn save_and_load_spawned_round_trips_multiple_entries() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".vaak")).unwrap();
        let original = vec![
            SpawnedAgent { pid: 100, role: "developer".into(), instance: 0, spawned_at: "2026-04-18T20:00:00Z".into() },
            SpawnedAgent { pid: 200, role: "tester".into(),    instance: 1, spawned_at: "2026-04-18T20:01:00Z".into() },
        ];
        save_spawned_to_disk(tmp.path().to_str().unwrap(), &original);
        let loaded = load_spawned_from_disk(tmp.path().to_str().unwrap());
        // Dedupe-on-load sorts by spawned_at DESC — assert by lookup, not position.
        assert_eq!(loaded.len(), 2);
        let dev = loaded.iter().find(|a| a.role == "developer").unwrap();
        assert_eq!(dev.pid, 100);
        let tester = loaded.iter().find(|a| a.role == "tester").unwrap();
        assert_eq!(tester.instance, 1);
    }

    // ── dev-challenger:1 msg 161 / tech-leader:0 msg 191 — manifest-wipe regression guards ──
    //
    // Post-PR2 (cd97ee5, repopulate reconnect-only), `repopulate_spawned` leaves
    // dead entries in `spawned.json` for PR3's "Relaunch last team" button.
    // pr-manifest-durability (e06a32e) then fixed `do_spawn_member` to load
    // existing disk entries, dedupe on (role, instance), and write back —
    // preserving dead entries across user launches. The tests below lock those
    // invariants so a future refactor of either function doesn't silently
    // regress to the pre-fix behavior.
    //
    // NOTE: these tests do not invoke the real `do_spawn_member` (which shells
    // out to PowerShell). They model the DISK contract — what every caller of
    // `save_spawned_to_disk` must assume — which is the surface that had the
    // defect.

    #[test]
    fn save_spawned_to_disk_unconditionally_overwrites_contents() {
        // Documents the pitfall pr-manifest-durability guards against: if any
        // caller writes an alive-only Vec without first reading-and-merging
        // the existing disk state, dead entries are lost. This test locks that
        // behavior into the test suite so anyone reintroducing the old pattern
        // (spawned.clone() → save) knows exactly what they're throwing away.
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_str().unwrap();
        std::fs::create_dir_all(tmp.path().join(".vaak")).unwrap();

        // Prior session left 3 dead entries on disk. Post-PR2, repopulate_spawned
        // loads them into… nothing: it pushes only alive PIDs into in-memory,
        // and these PIDs are dead, so in-memory stays empty.
        let prior_session_dead = vec![
            SpawnedAgent { pid: 1001, role: "developer".into(), instance: 0, spawned_at: "2026-04-17T10:00:00Z".into() },
            SpawnedAgent { pid: 1002, role: "tester".into(),    instance: 0, spawned_at: "2026-04-17T10:01:00Z".into() },
            SpawnedAgent { pid: 1003, role: "manager".into(),   instance: 0, spawned_at: "2026-04-17T10:02:00Z".into() },
        ];
        save_spawned_to_disk(dir, &prior_session_dead);

        // Simulate post-restart in-memory state: empty (no alive reconnects).
        let mut in_memory: Vec<SpawnedAgent> = Vec::new();

        // User clicks "Launch Team Member → moderator:0". do_spawn_member
        // pushes the new alive entry into in-memory, then writes the clone.
        in_memory.push(SpawnedAgent {
            pid: 9999,
            role: "moderator".into(),
            instance: 0,
            spawned_at: "2026-04-18T20:30:00Z".into(),
        });
        save_spawned_to_disk(dir, &in_memory);

        let on_disk = load_spawned_from_disk(dir);

        // Calling `save_spawned_to_disk` with an alive-only Vec is an
        // unconditional overwrite — the 3 prior-session dead entries are
        // gone. This is the raw disk contract; the caller (`do_spawn_member`)
        // is responsible for reading-and-merging before calling. See the
        // read-merge-write block in do_spawn_member for the safe pattern.
        assert_eq!(
            on_disk.len(),
            1,
            "save_spawned_to_disk overwrites — callers MUST merge disk state first. \
             Regression guard per dev-challenger:1 msg 161 / tech-leader:0 msg 191."
        );
        assert_eq!(on_disk[0].role, "moderator");
    }

    // The actual read-merge-write contract that `do_spawn_member` now follows
    // (pr-manifest-durability e06a32e). Models the exact pattern at
    // launcher.rs:441-456: load disk → retain-not-same-key → push new → save.
    #[test]
    fn do_spawn_member_read_merge_write_preserves_dead_entries_from_prior_session() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_str().unwrap();
        std::fs::create_dir_all(tmp.path().join(".vaak")).unwrap();

        let prior_dead = vec![
            SpawnedAgent { pid: 1001, role: "developer".into(), instance: 0, spawned_at: "2026-04-17T10:00:00Z".into() },
            SpawnedAgent { pid: 1002, role: "tester".into(),    instance: 0, spawned_at: "2026-04-17T10:01:00Z".into() },
        ];
        save_spawned_to_disk(dir, &prior_dead);

        // When B-narrow ships, do_spawn_member will load-disk, merge the new
        // entry (deduping on role+instance), and write. Simulate the target
        // behavior here as a specification:
        let mut on_disk = load_spawned_from_disk(dir);
        let new_entry = SpawnedAgent {
            pid: 9999,
            role: "moderator".into(),
            instance: 0,
            spawned_at: "2026-04-18T20:30:00Z".into(),
        };
        on_disk.retain(|a| !(a.role == new_entry.role && a.instance == new_entry.instance));
        on_disk.push(new_entry);
        save_spawned_to_disk(dir, &on_disk);

        let final_state = load_spawned_from_disk(dir);
        assert_eq!(final_state.len(), 3, "B-narrow must preserve prior dead entries");
        let roles: std::collections::HashSet<&str> = final_state.iter().map(|a| a.role.as_str()).collect();
        assert!(roles.contains("developer"));
        assert!(roles.contains("tester"));
        assert!(roles.contains("moderator"));
    }

    // Dedupe-on-load contract (pr-manifest-durability e06a32e).
    // Historical contamination: the watchdog-era code wrote a fresh entry on
    // every respawn, so a real user's spawned.json accumulated dozens of
    // duplicate (role, instance) rows. Without dedupe, `relaunch_spawned`
    // would spawn N copies per click. Sort-desc + HashSet-keyed retain
    // collapses to newest-wins.
    #[test]
    fn load_spawned_from_disk_dedupes_by_role_instance_newest_wins() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_str().unwrap();
        std::fs::create_dir_all(tmp.path().join(".vaak")).unwrap();

        // A polluted manifest from days of watchdog activity: same (role, instance)
        // appearing multiple times with ascending spawned_at.
        let polluted = vec![
            SpawnedAgent { pid: 1, role: "developer".into(), instance: 0, spawned_at: "2026-04-15T00:00:00Z".into() },
            SpawnedAgent { pid: 2, role: "developer".into(), instance: 0, spawned_at: "2026-04-16T00:00:00Z".into() },
            SpawnedAgent { pid: 3, role: "developer".into(), instance: 0, spawned_at: "2026-04-17T00:00:00Z".into() },
            SpawnedAgent { pid: 4, role: "tester".into(),    instance: 0, spawned_at: "2026-04-17T00:00:00Z".into() },
        ];
        save_spawned_to_disk(dir, &polluted);

        let loaded = load_spawned_from_disk(dir);
        assert_eq!(loaded.len(), 2, "dedupe: 3 developer:0 entries collapse to 1");
        let dev = loaded.iter().find(|a| a.role == "developer" && a.instance == 0).unwrap();
        assert_eq!(dev.pid, 3, "newest-wins: pid=3 (2026-04-17) beats pid=1,2");
    }

    // ── RelaunchGate RAII drop semantics (manager msg 219 panic-safety slate) ──
    //
    // Pre-bb1616f the explicit `store(false)` in the bg stagger thread was
    // unreachable under panic — gate stuck true forever, Relaunch silently dead
    // until app restart. tech-leader:1 msg 215 + evil-architect:0 msg 217 +
    // dev-challenger:1 msg 221 converged on a Drop-based RAII guard. These tests
    // lock that contract: normal exit clears, panic unwind clears, early-return
    // clears, and drop is write-false (not toggle).

    #[test]
    fn relaunch_gate_clears_atomic_on_normal_drop() {
        let flag = Arc::new(AtomicBool::new(true));
        {
            let _gate = RelaunchGate(Arc::clone(&flag));
        }
        assert!(!flag.load(Ordering::Acquire),
                "gate must clear atomic via Drop on normal scope exit");
    }

    #[test]
    fn relaunch_gate_clears_atomic_on_panic_unwind() {
        let flag = Arc::new(AtomicBool::new(true));
        let flag_for_panic = Arc::clone(&flag);

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _gate = RelaunchGate(Arc::clone(&flag_for_panic));
            panic!("simulated do_spawn_member panic mid-stagger");
        }));

        assert!(result.is_err(), "inner closure must have panicked");
        assert!(!flag.load(Ordering::Acquire),
                "gate must clear atomic during panic unwind — this is the whole point of the Drop impl");
    }

    #[test]
    fn relaunch_gate_clears_atomic_on_early_return() {
        fn helper(flag: Arc<AtomicBool>, early: bool) -> Result<u32, String> {
            let _gate = RelaunchGate(Arc::clone(&flag));
            if early {
                return Ok(0);
            }
            Ok(42)
        }
        let flag = Arc::new(AtomicBool::new(true));
        let _ = helper(Arc::clone(&flag), true);
        assert!(!flag.load(Ordering::Acquire),
                "gate must clear on early-return path");

        flag.store(true, Ordering::Release);
        let _ = helper(Arc::clone(&flag), false);
        assert!(!flag.load(Ordering::Acquire),
                "gate must clear on fall-through path too");
    }

    #[test]
    fn relaunch_gate_does_not_disturb_fresh_false_atomic() {
        let flag = Arc::new(AtomicBool::new(false));
        {
            let _gate = RelaunchGate(Arc::clone(&flag));
        }
        assert!(!flag.load(Ordering::Acquire),
                "dropping a gate must leave atomic false, not toggle it to true");
    }
}
