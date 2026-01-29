//! Scribe MCP Server - Bridges Claude Code to Scribe for voice output
//!
//! This is a minimal MCP (Model Context Protocol) server that provides a `speak` tool
//! for Claude Code to send text-to-speech requests to the Scribe desktop app.
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

    // Try stdin, stdout, stderr in order
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

/// Get parent process ID on Windows (simple single lookup, no tree walking)
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

    // Include hostname
    if let Ok(hostname) = hostname::get() {
        hostname.to_string_lossy().hash(&mut hasher);
    }

    // Include parent PID
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

    // Include current working directory
    if let Ok(cwd) = std::env::current_dir() {
        cwd.to_string_lossy().hash(&mut hasher);
    }

    // Include username for extra uniqueness
    if let Ok(user) = std::env::var("USER").or_else(|_| std::env::var("USERNAME")) {
        user.hash(&mut hasher);
    }

    let hostname = hostname::get()
        .map(|h| h.to_string_lossy().to_string())
        .unwrap_or_else(|_| "unknown".to_string());

    format!("{}-{:016x}", hostname, hasher.finish())
}

/// Get a stable session ID using a priority chain of methods
///
/// Priority:
/// 1. CLAUDE_SESSION_ID - explicit override
/// 2. WT_SESSION - Windows Terminal (GUID, very unique)
/// 3. ITERM_SESSION_ID - iTerm2 on macOS (UUID)
/// 4. TERM_SESSION_ID - Terminal.app and other terminals
/// 5. Console window handle (Windows) or TTY path (Unix)
/// 6. Fallback hash
fn get_session_id() -> String {
    // Priority 1: Explicit override via CLAUDE_SESSION_ID
    if let Ok(env_session) = std::env::var("CLAUDE_SESSION_ID") {
        if !env_session.is_empty() {
            eprintln!("[scribe-mcp] Session source: CLAUDE_SESSION_ID env var");
            return env_session;
        }
    }

    // Priority 2: Windows Terminal GUID (WT_SESSION)
    if let Ok(wt_session) = std::env::var("WT_SESSION") {
        if !wt_session.is_empty() {
            eprintln!("[scribe-mcp] Session source: Windows Terminal (WT_SESSION)");
            return format!("wt-{}", wt_session);
        }
    }

    // Priority 3: iTerm2 session ID (macOS)
    if let Ok(iterm_session) = std::env::var("ITERM_SESSION_ID") {
        if !iterm_session.is_empty() {
            eprintln!("[scribe-mcp] Session source: iTerm2 (ITERM_SESSION_ID)");
            return format!("iterm-{}", iterm_session);
        }
    }

    // Priority 4: Generic terminal session ID (Terminal.app, etc)
    if let Ok(term_session) = std::env::var("TERM_SESSION_ID") {
        if !term_session.is_empty() {
            eprintln!("[scribe-mcp] Session source: Terminal session (TERM_SESSION_ID)");
            return format!("term-{}", term_session);
        }
    }

    // Priority 5a: Windows console window handle
    #[cfg(windows)]
    if let Some(hwnd) = get_console_window_handle() {
        eprintln!("[scribe-mcp] Session source: Windows console handle");
        let hostname = hostname::get()
            .map(|h| h.to_string_lossy().to_string())
            .unwrap_or_else(|_| "unknown".to_string());
        return format!("{}-console-{:x}", hostname, hwnd);
    }

    // Priority 5b: Unix TTY path
    #[cfg(unix)]
    if let Some(tty) = get_tty_path() {
        eprintln!("[scribe-mcp] Session source: TTY path ({})", tty);
        // Sanitize: /dev/pts/3 -> pts-3, /dev/ttys001 -> ttys001
        let clean = tty.replace("/dev/", "").replace("/", "-");
        let hostname = hostname::get()
            .map(|h| h.to_string_lossy().to_string())
            .unwrap_or_else(|_| "unknown".to_string());
        return format!("{}-tty-{}", hostname, clean);
    }

    // Priority 6: Fallback hash (deterministic based on system state)
    eprintln!("[scribe-mcp] Session source: Fallback hash");
    generate_fallback_id()
}

/// Send text to Scribe's local speak endpoint
fn send_to_scribe(text: &str, session_id: &str) -> Result<(), String> {
    let client = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(5))
        .build();

    let body = serde_json::json!({
        "text": text,
        "session_id": session_id
    });

    match client.post("http://127.0.0.1:7865/speak")
        .set("Content-Type", "application/json")
        .send_string(&body.to_string())
    {
        Ok(_) => Ok(()),
        Err(e) => Err(format!("Failed to send to Scribe: {}", e))
    }
}

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
                    "name": "scribe-speak",
                    "version": "1.0.0"
                }
            })
        }
        "notifications/initialized" => {
            // No response needed for notifications
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
                            }
                        },
                        "required": ["text"]
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

                match send_to_scribe(text, session_id) {
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
                                "text": format!("Could not reach Scribe: {}. Make sure the Scribe desktop app is running.", e)
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
            // Unknown method - return error
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

fn main() {
    let session_id = get_session_id();
    eprintln!("[scribe-mcp] Session ID: {}", session_id);

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

        // Parse JSON-RPC request
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

        // Handle the request
        if let Some(response) = handle_request(&request, &session_id) {
            let _ = writeln!(stdout, "{}", response);
            let _ = stdout.flush();
        }
    }
}
