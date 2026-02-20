use parking_lot::Mutex;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use tauri::State;

/// A Claude agent spawned from the Team Launcher UI
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
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
        let temp_dir = std::env::temp_dir();
        let script_name = format!("vaak-launch-{}-{}.ps1", role, std::process::id());
        let script_path = temp_dir.join(&script_name);
        let ps_script = format!(
            "Set-Location \"{}\"\n& \"{}\" --dangerously-skip-permissions \"{}\"",
            project_dir, claude_path, join_prompt
        );
        std::fs::write(&script_path, &ps_script)
            .map_err(|e| format!("Failed to write launch script: {}", e))?;

        // Use WMI (Invoke-CimMethod) to create the agent process.
        // WMI creates the process via the WMI service (wmiprvse.exe), so it is
        // NOT in Tauri's Job Object and survives app restarts. This avoids the
        // CREATE_BREAKAWAY_FROM_JOB "Access denied" issue entirely.
        let script_path_str = script_path.to_str().unwrap_or("").replace("'", "''");
        let ps_cmd = format!(
            "$r = Invoke-CimMethod -ClassName Win32_Process -MethodName Create -Arguments @{{CommandLine='powershell.exe -ExecutionPolicy Bypass -NoExit -File \"{}\"'}}; Write-Output $r.ProcessId",
            script_path_str
        );
        let ps_args = ["-NoProfile", "-WindowStyle", "Hidden", "-Command", &ps_cmd];

        let output = Command::new("powershell")
            .args(&ps_args)
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|e| format!("Failed to spawn via WMI: {}", e))?;

        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let stderr_str = String::from_utf8_lossy(&output.stderr).trim().to_string();

        if !output.status.success() || stdout.is_empty() {
            return Err(format!("WMI spawn failed for {}: stdout='{}' stderr='{}'", role, stdout, stderr_str));
        }

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
        Command::new("x-terminal-emulator")
            .args([
                "-e",
                &format!("bash -c \"{}\"", bash_cmd),
            ])
            .spawn()
            .or_else(|_| {
                Command::new("gnome-terminal")
                    .args([
                        "--",
                        "bash",
                        "-c",
                        &bash_cmd,
                    ])
                    .spawn()
            })
            .or_else(|_| {
                Command::new("xterm")
                    .args([
                        "-e",
                        &bash_cmd,
                    ])
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
    spawned.push(SpawnedAgent {
        pid,
        role: role.to_string(),
        instance,
        spawned_at: now.clone(),
    });

    // Persist PIDs to disk so they survive app restarts
    let all: Vec<SpawnedAgent> = spawned.clone();
    drop(spawned);
    if let Some(dir) = launcher.project_dir.lock().as_ref() {
        save_spawned_to_disk(dir, &all);
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
fn save_spawned_to_disk(project_dir: &str, agents: &[SpawnedAgent]) {
    let path = std::path::Path::new(project_dir)
        .join(".vaak")
        .join("spawned.json");
    if let Ok(json) = serde_json::to_string_pretty(agents) {
        let _ = std::fs::write(path, json);
    }
}

/// Load spawned agents from .vaak/spawned.json (fallback for after app restart).
fn load_spawned_from_disk(project_dir: &str) -> Vec<SpawnedAgent> {
    let path = std::path::Path::new(project_dir)
        .join(".vaak")
        .join("spawned.json");
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Re-populate the in-memory spawned list from disk on app startup.
/// Keeps entries whose PIDs are still alive, and auto-respawns dead agents
/// so team members survive app restarts.
#[tauri::command]
pub fn repopulate_spawned(project_dir: String, state: State<'_, LauncherState>) -> Result<u32, String> {
    *state.project_dir.lock() = Some(project_dir.clone());
    let disk_agents = load_spawned_from_disk(&project_dir);
    let mut spawned = state.spawned.lock();
    let mut alive_count = 0u32;
    let mut dead_agents: Vec<SpawnedAgent> = Vec::new();

    for agent in disk_agents {
        if is_pid_alive(agent.pid) {
            if !spawned.iter().any(|a| a.role == agent.role && a.instance == agent.instance) {
                eprintln!("[launcher] Reconnected to alive agent: {}:{} (PID {})", agent.role, agent.instance, agent.pid);
                spawned.push(agent);
                alive_count += 1;
            }
        } else {
            eprintln!("[launcher] Agent {}:{} (PID {}) is dead — removing from tracker", agent.role, agent.instance, agent.pid);
            dead_agents.push(agent);
        }
    }

    // Update disk with only alive agents (remove dead ones)
    let all: Vec<SpawnedAgent> = spawned.clone();
    drop(spawned);
    save_spawned_to_disk(&project_dir, &all);

    if !dead_agents.is_empty() {
        eprintln!("[launcher] Cleaned up {} dead agent(s) from spawned.json (not respawning)", dead_agents.len());
    }

    Ok(alive_count)
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
