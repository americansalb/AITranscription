# Vaak Collaboration System: Vision & Implementation Plan

---

## Part 0: Codebase Context

This section provides everything a fresh Claude Code session needs to understand the existing Vaak application before working on the collaboration system. Read this first.

### What is Vaak?

Vaak is a desktop application that bridges Claude Code (Anthropic's CLI tool) with voice output and screen reading capabilities. It's built with Tauri (Rust backend + web frontend) and connects to a Python FastAPI backend for transcription services. The collaboration system is a feature within Vaak that enables multiple Claude Code sessions to work together as a team.

### Project Directory Structure

```
C:\Users\18479\Desktop\LOCAL APP TESTING\AITranscription\
├── desktop/                          # Tauri desktop application
│   ├── src/                          # React/TypeScript frontend
│   │   ├── App.tsx                   # Main window component (recording, transcription UI)
│   │   ├── TranscriptApp.tsx         # Transcript window component (has Collab tab)
│   │   ├── components/
│   │   │   ├── CollabTab.tsx         # Collaboration UI component (TO BE REWRITTEN)
│   │   │   ├── Settings.tsx          # Settings panel
│   │   │   ├── QueueSlidePanel.tsx   # Voice queue UI
│   │   │   └── ...
│   │   ├── lib/
│   │   │   ├── collabTypes.ts        # TypeScript types for collab (TO BE REWRITTEN)
│   │   │   ├── speak.ts              # Voice output listener
│   │   │   ├── api.ts                # Backend API client
│   │   │   └── ...
│   │   └── styles/
│   │       ├── collab.css            # Collab tab styles (TO BE REWRITTEN)
│   │       └── ...
│   ├── src-tauri/
│   │   ├── src/
│   │   │   ├── main.rs              # Tauri app entry point, HTTP server, window management
│   │   │   ├── collab.rs            # Collab file parser + session registry (TO BE REWRITTEN)
│   │   │   ├── audio.rs             # Audio recording
│   │   │   ├── database.rs          # Local queue database
│   │   │   ├── focus_tracker.rs     # Window focus tracking
│   │   │   ├── keyboard_hook.rs     # Global keyboard hooks
│   │   │   └── uia_capture.rs       # UI Automation tree capture
│   │   ├── binaries/
│   │   │   └── vaak-mcp-x86_64-pc-windows-msvc.exe  # Compiled MCP sidecar
│   │   └── tauri.conf.json          # Window config, app metadata
│   ├── package.json                  # npm scripts: dev, build, tauri
│   ├── vite.config.ts               # Vite bundler config
│   └── tsconfig.json
├── backend/                          # Python FastAPI backend (separate process)
│   ├── app/
│   │   ├── main.py                  # FastAPI app entry point
│   │   ├── api/                     # API route handlers
│   │   ├── core/                    # Config, database setup
│   │   ├── models/                  # SQLAlchemy models
│   │   └── services/               # Business logic
│   └── pyproject.toml
├── mcp-speak/                        # (Legacy) Python MCP server package
├── COLLAB_REDESIGN.md               # THIS DOCUMENT
└── CLAUDE.md                        # Project-level Claude Code instructions
```

### Tauri Application Architecture

Vaak's Tauri app has **4 webview windows**, each rendering a different route of the same React app:

| Window Label | Title | Route | Purpose | Default Visible |
|---|---|---|---|---|
| `main` | Vaak | `index.html` (/) | Recording UI, transcription, settings | Yes |
| `transcript` | Claude Sessions - Transcript | `index.html#/transcript` | Voice queue, session list, **Collab tab** | No |
| `recording-indicator` | (none) | `index.html#/overlay` | Floating recording indicator | No |
| `screen-reader` | Vaak - Screen Reader | `index.html#/screen-reader` | Screen reader conversation UI | No |

**The Collab tab lives in the `transcript` window.** This is important because Tauri events must be emitted to the correct window label. The current code emits collab events to `"transcript"`:

```rust
// In main.rs — events are targeted to the transcript window
if let Some(window) = app_handle.get_webview_window("transcript") {
    let _ = window.emit("collab-update", &json);
}
```

### How Tauri Commands Work

The React frontend calls Rust functions via Tauri's `invoke` API:

```typescript
// Frontend (React/TypeScript)
import { invoke } from "@tauri-apps/api/core";
const result = await invoke<ReturnType>("command_name", { arg1: "value" });
```

```rust
// Backend (Rust) — registered in main.rs tauri::Builder
#[tauri::command]
fn command_name(arg1: String) -> Result<serde_json::Value, String> {
    // ... implementation
    Ok(serde_json::json!({"key": "value"}))
}

// Commands must be registered in the builder (main.rs ~line 2174):
.invoke_handler(tauri::generate_handler![
    // ... other commands ...
    watch_collab_dir   // <-- collab command registered here
]);
```

### How Tauri Events Work

Rust backend can push data to the frontend via events:

```rust
// Rust emits an event to a specific window
if let Some(window) = app_handle.get_webview_window("transcript") {
    let _ = window.emit("collab-update", &json_payload);
}
```

```typescript
// React listens for events
import { listen } from "@tauri-apps/api/event";
const unlisten = await listen<PayloadType>("collab-update", (event) => {
    console.log(event.payload);
});
// Call unlisten() to stop listening
```

### The MCP Sidecar: vaak-mcp

Claude Code communicates with Vaak through an **MCP (Model Context Protocol) sidecar** — a separate binary that Claude Code spawns as a subprocess.

**Source file:** `desktop/src-tauri/src/bin/vaak-mcp.rs` (1430 lines)

**How it works:**

1. Claude Code reads `~/.claude.json` to discover MCP servers
2. It spawns `vaak-mcp.exe` as a subprocess
3. Communication happens over **stdio using JSON-RPC 2.0** (one JSON object per line)
4. Claude Code sends tool calls, the sidecar processes them and returns results

**MCP configuration** (auto-created by Vaak on first run, written by `setup_claude_code_integration()` in main.rs):

File: `~/.claude.json`
```json
{
  "mcpServers": {
    "vaak": {
      "type": "stdio",
      "command": "C:\\Users\\18479\\Desktop\\LOCAL APP TESTING\\AITranscription\\desktop\\src-tauri\\binaries\\vaak-mcp-x86_64-pc-windows-msvc.exe",
      "args": []
    }
  }
}
```

**Tool registration pattern** in vaak-mcp.rs:

```rust
// Tools are defined in the "tools/list" handler (~line 871):
"tools/list" => {
    serde_json::json!({
        "tools": [{
            "name": "tool_name",
            "description": "What it does",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "arg1": { "type": "string", "description": "..." }
                },
                "required": ["arg1"]
            }
        }]
    })
}

// Tool calls are dispatched in the "tools/call" handler (~line 962):
"tools/call" => {
    let tool_name = params.get("name")?.as_str()?;
    let arguments = params.get("arguments")?;

    if tool_name == "tool_name" {
        let arg1 = arguments.get("arg1")?.as_str()?;
        // ... handle tool call, return result
        serde_json::json!({
            "content": [{ "type": "text", "text": "result" }]
        })
    }
}
```

**Current MCP tools exposed by vaak-mcp:**
- `speak` — Send text-to-speech to Vaak
- `collab_join` — Join a collaboration (TO BE REPLACED)
- `collab_send` — Send a collab message (TO BE REPLACED)
- `collab_check` — Poll for collab messages (TO BE REPLACED)
- `screen_read` — Capture a screenshot
- `list_windows` — List visible windows

### The UserPromptSubmit Hook

The hook is the mechanism for pushing information into Claude Code's context. It runs every time the user sends a message in any Claude Code session.

**How it works:**

1. Vaak creates a wrapper script at `~/.claude/hooks/vaak-hook.cmd` (Windows) or `~/.claude/hooks/vaak-hook.sh` (Unix)
2. The wrapper calls: `vaak-mcp.exe --hook`
3. The `--hook` flag causes the sidecar to:
   - Read voice settings from `%APPDATA%\Vaak\voice-settings.json`
   - Read the cached session ID from `%APPDATA%\Vaak\session-cache/{ppid}.txt`
   - Send a heartbeat to `http://127.0.0.1:7865/heartbeat`
   - Check for active collaboration in `.vaak/collab.md` (current working directory)
   - Print instructions to stdout (which Claude Code injects into its context)
4. The hook is registered in `~/.claude/settings.json`:

```json
{
  "hooks": {
    "UserPromptSubmit": [{
      "hooks": [{
        "type": "command",
        "command": "C:\\Users\\18479\\.claude\\hooks\\vaak-hook.cmd"
      }]
    }]
  }
}
```

**Key constraint:** The hook output goes to **stdout** and is appended to the system prompt. It must be plain text. This is the ONLY way to push information into a Claude Code session without the user manually typing it.

### Session ID System

Each Claude Code session gets a unique, stable session ID. The MCP sidecar determines this ID using a priority chain:

1. `CLAUDE_SESSION_ID` env var (explicit override)
2. `WT_SESSION` env var (Windows Terminal GUID) → `"wt-{guid}"`
3. `ITERM_SESSION_ID` env var (iTerm2) → `"iterm-{uuid}"`
4. `TERM_SESSION_ID` env var (Terminal.app) → `"term-{id}"`
5. Console window handle (Windows) → `"{hostname}-console-{hwnd}"`
6. TTY path (Unix) → `"{hostname}-tty-{path}"`
7. Fallback: hash of hostname + parent PID + CWD + username

The session ID is **cached** to `%APPDATA%\Vaak\session-cache\{ppid}.txt` by the sidecar so the hook subprocess (which has a different PID) can read the same ID.

### The HTTP Server (port 7865)

Vaak's Tauri app runs a lightweight HTTP server on `127.0.0.1:7865` (started in main.rs ~line 687). It handles:

| Endpoint | Method | Purpose |
|---|---|---|
| `/speak` | POST | Receive text-to-speech requests from MCP sidecar |
| `/heartbeat` | POST | Receive session heartbeats, track active sessions |
| `/collab/notify` | POST | Receive notification that collab file changed (TO BE UPDATED) |
| `/sessions/names` | POST | Set friendly names for sessions |

When the sidecar sends a message or modifies collaboration state, it POSTs to `/collab/notify` to wake up the desktop UI immediately (instead of waiting for the 1-second file watcher poll).

### The File Watcher (Polling Thread)

A background thread in main.rs (`start_collab_watcher()`, ~line 1551) polls the collaboration file every 1 second:

1. Check if a directory is being watched (set via `watch_collab_dir` Tauri command)
2. Compare the file's modification time to the last known time
3. If changed: read and parse the file, emit `"collab-update"` event to the frontend
4. The frontend's CollabTab component receives the event and updates the UI

### The File Locking Mechanism

Concurrent writes to collaboration files are protected by exclusive file locks:

```rust
// In vaak-mcp.rs — with_file_lock() function (~line 227)
// Creates/opens .vaak/collab.lock (or board.lock for new system)
// Windows: LockFileEx with LOCKFILE_EXCLUSIVE_LOCK
// Unix: libc::flock with LOCK_EX
// Executes the closure, then releases the lock
fn with_file_lock<F, R>(project_dir: &str, f: F) -> Result<R, String>
where F: FnOnce() -> Result<R, String>
{
    // 1. Create .vaak directory if needed
    // 2. Open/create lock file
    // 3. Acquire exclusive lock (blocks if another process holds it)
    // 4. Execute f()
    // 5. Release lock
    // 6. Return result
}
```

**This mechanism should be preserved in the redesign.** It correctly handles concurrent access from multiple MCP sidecar processes.

### Current Collab Implementation (Being Replaced)

The current collaboration system uses a single `collab.md` file with an embedded JSON header. Here's what exists and what's changing:

**Files being replaced:**
- `vaak-mcp.rs` lines 20-518: `ACTIVE_COLLAB` static, `CollabHeader`/`CollabMsg` structs, `parse_collab_header()`, `parse_collab_messages()`, `rebuild_collab_file()`, `handle_collab_join()`, `handle_collab_send()`, `handle_collab_check()`, `check_collab_from_file()` — All replaced with new project-based implementations
- `collab.rs` lines 91-178: `ParsedCollab` struct, `parse_collab_file()` — Replaced with new structs and parsers
- `CollabTab.tsx`: Entire component — Rewritten for project dashboard
- `collabTypes.ts`: All interfaces — Rewritten for new data structures
- `collab.css`: All styles — Rewritten for new UI

**Files being modified:**
- `vaak-mcp.rs` tool registration (~line 871): Replace collab_join/send/check with project_join/send/check/status/leave/update_briefing
- `vaak-mcp.rs` tool dispatch (~line 962): Add handlers for new tools
- `vaak-mcp.rs` hook output (~line 1146): Update `run_hook()` and `check_collab_from_file()` for new project format
- `main.rs` HTTP server (~line 743): Update `/collab/notify` endpoint
- `main.rs` file watcher (~line 1551): Watch board.jsonl and sessions.json instead of collab.md
- `main.rs` Tauri commands (~line 1527): Replace `watch_collab_dir` with project-aware commands
- `main.rs` command registration (~line 2174): Register new commands
- `TranscriptApp.tsx` (~line 941): CollabTab import and rendering (minimal change)

**Code being kept as-is:**
- `collab.rs` lines 1-89: `SessionRegistry`, `SessionInfo` — Still used for heartbeat tracking
- `vaak-mcp.rs` lines 520-756: Session ID determination, heartbeat, caching — Still used
- `vaak-mcp.rs` lines 227-279: `with_file_lock()` — Reused for new file locking
- `vaak-mcp.rs` lines 283-292: `notify_desktop()` — Reused for push notifications

### How to Build and Test

```bash
# Start the Tauri desktop app in dev mode (frontend + Rust backend)
cd desktop
npm run tauri dev

# Start the Python backend (separate terminal)
cd backend
python -m uvicorn app.main:app --host 127.0.0.1 --port 19836 --reload

# Build the MCP sidecar separately (if needed)
cd desktop/src-tauri
cargo build --bin vaak-mcp --release
```

The MCP sidecar is compiled as part of `npm run tauri dev` but can also be built independently. After building, the binary is at `desktop/src-tauri/target/debug/vaak-mcp.exe` (dev) or `desktop/src-tauri/target/release/vaak-mcp.exe` (release).

### Error Handling Patterns

Throughout the Rust code, errors are handled by returning `Result<T, String>` where the error is a human-readable message. The MCP tool responses use:

```rust
// Success
serde_json::json!({
    "content": [{ "type": "text", "text": result.to_string() }]
})

// Error
serde_json::json!({
    "content": [{ "type": "text", "text": format!("Error: {}", e) }],
    "isError": true
})
```

---

## Part 1: Vision

### The Problem

When a single Claude Code session works on a complex project for an extended period, it develops blind spots. It commits to approaches without questioning assumptions. It hallucinates something — a non-existent API, an incorrect function signature — and then builds on top of that hallucination because it's already in context. There's no second opinion, no fresh pair of eyes, no one to say "wait, that's wrong."

Human development teams solve this naturally: code reviews, pair programming, architecture reviews, QA testing. Each person brings an independent perspective, catches different mistakes, and challenges different assumptions. The team's output is better than any individual's because errors get caught at multiple stages.

### The Insight

A fresh Claude Code terminal session is genuinely a different mind. It has no prior context, no inherited assumptions, no blind spots from the previous conversation. When you open a new terminal and ask Claude to review code that another Claude session wrote, it evaluates it independently — and it catches things the original session missed. This isn't theoretical; it's observable in practice.

This is fundamentally different from Claude Code's built-in sub-agent system (the Task tool). Sub-agents are dispatched by a parent session — they inherit the parent's framing, work within the parent's assumptions, and don't maintain independent long-running context. Two separate terminal sessions are genuinely independent minds: different context windows, different reasoning chains, different blind spots. That independence is the source of their value.

This means **multiple independent Claude Code sessions, each with a specialized role, can function as a development team**. Not simulated collaboration, but genuine cognitive diversity — each session forms its own mental model of the code, reasons independently, and produces work that gets verified by other independent sessions.

### The Vision

**Vaak's collaboration system enables users to assemble and manage AI development teams.**

A user defines a project and creates roles: Architect, Senior Developer, Junior Developer, Tester, Security Auditor, Technical Writer — whatever the project needs. Each role gets a job description that defines its responsibilities and focus areas. Claude Code sessions "join" the project by claiming a role, reading their job description, and getting to work.

The roles are fully customizable. You could have five developers all working on different parts of the project, or one consultant, one senior developer, and one junior developer. You might have a Security Auditor who only reviews code, a Technical Writer who maintains documentation, or a DevOps specialist who handles deployment. The team composition is defined per project and can change over time — roles can be added, removed, or reassigned as the project evolves.

A Manager role coordinates the team. The Manager reads all messages, creates task assignments, reviews outputs, requests revisions, and reports progress to the user. The user only needs to talk to the Manager — they don't manually coordinate dozens of terminals. The Manager IS a Claude Code session, but one with special permissions and a coordination-focused briefing.

Key principles:

1. **Independent reasoning**: Each role is a separate Claude Code session with its own context window. They don't share conversation history. This is a feature, not a limitation — it's what provides cognitive diversity and error correction.

2. **Persistent projects**: The project outlives any individual session. When a session ends (terminal closed, context exhausted), the role becomes vacant. A new session can claim that role, read the briefing, and pick up where the last one left off. The project has continuity even as team members come and go.

3. **Dynamic team composition**: Roles are created, modified, and removed as the project evolves. You might start with an Architect and a Developer. Later, add a Tester. Split the Developer role into Frontend and Backend specialists. Add a Security Auditor for the final review. The team adapts to the project's needs.

4. **Hierarchical management**: A Manager role coordinates the team. The Manager reads all messages, creates task assignments, reviews outputs, requests revisions, and reports to the user. The user only needs to talk to the Manager — they don't manually coordinate 20 terminals.

5. **Structured communication**: Messages are directed (from one role to another), typed (directive, question, handoff, status update), and filtered (each role only sees messages relevant to them). This prevents context pollution while maintaining coordination.

6. **Error correction through independence**: When the Architect designs an API, the Developer discovers immediately if it's hallucinated when they try to implement it. When the Developer writes code, the Tester catches bugs the Developer couldn't see because the Tester has fresh eyes. Multiple independent perspectives are the best defense against hallucination.

7. **Scale**: A project can have 2 roles or 30. The system works the same way regardless of team size because the Manager handles coordination and each role only sees its relevant messages.

### Why This Matters

This is not "two chatbots talking to each other." This is a framework for running an AI development team that produces higher-quality output than any single session could, because it replicates the error-correction mechanisms of human teams: independent review, specialized expertise, fresh perspectives, and structured handoffs.

The user's experience is: define your project, describe the roles you need, and manage the team through a single Manager interface. The implementation details — file-based messaging, hook injection, session binding — are invisible. What the user sees is a team of specialists working on their project.

---

## Part 2: Architecture

### File Structure

All collaboration state lives in the project directory under `.vaak/`:

```
project-root/
  .vaak/
    project.json          # Project definition, role registry, settings
    sessions.json          # Active session bindings (role -> session ID, heartbeat)
    board.jsonl            # Message board (append-only, one JSON object per line)
    board.lock             # File lock for atomic writes to board
    roles/
      manager.md           # Manager role briefing / job description
      architect.md         # Architect role briefing
      developer.md         # Developer role briefing
      tester.md            # Tester role briefing
      ...                  # Any custom roles
    last-seen/
      {session_id}.json    # Per-session tracking of last read message ID
```

### project.json

The single source of truth about the project and its team.

```json
{
  "project_id": "a1b2c3d4",
  "name": "My Application",
  "description": "A web application with React frontend and Python backend",
  "created_at": "2026-02-04T15:30:00Z",
  "updated_at": "2026-02-04T16:45:00Z",
  "roles": {
    "manager": {
      "title": "Project Manager",
      "description": "Coordinates all team members. Receives requirements from the user, breaks them into tasks, assigns work, reviews outputs, and ensures the project vision is maintained.",
      "max_instances": 1,
      "permissions": ["assign_tasks", "review", "approve", "broadcast"],
      "created_at": "2026-02-04T15:30:00Z"
    },
    "architect": {
      "title": "Software Architect",
      "description": "Designs system architecture, defines API contracts, makes technology decisions, reviews code for architectural consistency.",
      "max_instances": 1,
      "permissions": ["review"],
      "created_at": "2026-02-04T15:30:00Z"
    },
    "dev-backend": {
      "title": "Backend Developer",
      "description": "Implements backend API endpoints, database models, and business logic. Works with Python/FastAPI.",
      "max_instances": 3,
      "permissions": [],
      "created_at": "2026-02-04T15:35:00Z"
    },
    "dev-frontend": {
      "title": "Frontend Developer",
      "description": "Implements React UI components, state management, and user interactions.",
      "max_instances": 2,
      "permissions": [],
      "created_at": "2026-02-04T15:35:00Z"
    },
    "tester": {
      "title": "QA Tester",
      "description": "Writes and runs tests. Reviews code for bugs, edge cases, and error handling gaps. Approaches code with skepticism.",
      "max_instances": 2,
      "permissions": [],
      "created_at": "2026-02-04T15:40:00Z"
    }
  },
  "settings": {
    "heartbeat_timeout_seconds": 120,
    "message_retention_days": 30
  }
}
```

Notes:
- `max_instances` allows multiple sessions in the same role (e.g., 3 backend developers)
- `permissions` controls what actions a role can perform:
  - `assign_tasks` — Can send `directive` type messages
  - `review` — Can send `review`, `approval`, `revision` type messages
  - `approve` — Can approve handoffs
  - `broadcast` — Can send messages to "all" roles
- Roles without special permissions can send: `question`, `answer`, `status`, `handoff`
- Roles are identified by a slug key (e.g., "dev-backend"), displayed by their title

### sessions.json

Tracks which Claude Code sessions currently hold which roles.

```json
{
  "bindings": [
    {
      "role": "manager",
      "instance": 0,
      "session_id": "DESKTOP-8MD44CF-console-407f6",
      "claimed_at": "2026-02-04T15:31:00Z",
      "last_heartbeat": "2026-02-04T16:45:12Z",
      "status": "active"
    },
    {
      "role": "dev-backend",
      "instance": 0,
      "session_id": "wt-abc123-def456",
      "claimed_at": "2026-02-04T15:36:00Z",
      "last_heartbeat": "2026-02-04T16:44:58Z",
      "status": "active"
    },
    {
      "role": "dev-backend",
      "instance": 1,
      "session_id": "wt-789xyz-000111",
      "claimed_at": "2026-02-04T16:00:00Z",
      "last_heartbeat": "2026-02-04T16:20:00Z",
      "status": "stale"
    }
  ]
}
```

Notes:
- `instance` allows multiple sessions in the same role (dev-backend #0, dev-backend #1)
- `status` is derived from heartbeat: "active" (< timeout), "stale" (> timeout)
- Stale sessions can be reclaimed by new sessions joining that role
- This file is read/written under file lock (uses board.lock)

**Conflict resolution for role claiming:**
- When a session calls `project_join`, it acquires the file lock, reads sessions.json
- If the role has available capacity (active instances < max_instances), the session is added
- If the role is full, the system checks for stale bindings and replaces one
- If the role is full with all active sessions, the join fails with an error

### board.jsonl (Message Board)

Append-only message log. One JSON object per line (JSONL format) for efficient appending without rewriting the entire file.

```jsonl
{"id":1,"from":"manager","to":"architect","type":"directive","timestamp":"2026-02-04T15:32:00Z","subject":"Design authentication system","body":"We need JWT-based auth with refresh tokens. Design the API contracts and database schema. Consider OAuth2 for future social login support.","metadata":{}}
{"id":2,"from":"architect","to":"manager","type":"status","timestamp":"2026-02-04T15:45:00Z","subject":"Auth design complete","body":"I've created the auth design. Key decisions: JWT with 15-min access tokens, 7-day refresh tokens stored in httponly cookies. Schema uses a users table with email/password_hash and a refresh_tokens table. Design doc written to docs/auth-design.md.","metadata":{"files":["docs/auth-design.md"]}}
{"id":3,"from":"manager","to":"dev-backend","type":"directive","timestamp":"2026-02-04T15:46:00Z","subject":"Implement auth endpoints","body":"Implement the auth system per the Architect's design in docs/auth-design.md. Create: POST /auth/register, POST /auth/login, POST /auth/refresh, POST /auth/logout. Follow existing patterns in app/api/.","metadata":{"depends_on":2,"files":["docs/auth-design.md"]}}
{"id":4,"from":"dev-backend","to":"manager","type":"handoff","timestamp":"2026-02-04T16:30:00Z","subject":"Auth endpoints implemented","body":"All four endpoints implemented and manually tested. Files changed: app/api/auth.py (new), app/models/user.py (modified), app/models/refresh_token.py (new), alembic/versions/003_auth.py (new).","metadata":{"files_changed":["app/api/auth.py","app/models/user.py","app/models/refresh_token.py","alembic/versions/003_auth.py"]}}
{"id":5,"from":"manager","to":"tester","type":"directive","timestamp":"2026-02-04T16:31:00Z","subject":"Test auth endpoints","body":"The Backend Developer has implemented auth endpoints. Review the code in app/api/auth.py and write comprehensive tests. Check for: input validation, error handling, token expiry, race conditions on refresh.","metadata":{"depends_on":4,"files":["app/api/auth.py"]}}
```

Message types:
- `directive` — Assignment of work (requires `assign_tasks` permission)
- `question` — Asking for clarification or input
- `answer` — Response to a question
- `status` — Progress update
- `handoff` — Completed work being passed to next stage
- `review` — Code review feedback (requires `review` permission)
- `approval` — Work approved (requires `approve` permission)
- `revision` — Work needs changes (requires `review` permission)
- `broadcast` — Message to all roles (requires `broadcast` permission)

Routing:
- `to` can be a single role slug ("dev-backend") or "all" for broadcasts
- When `to` is a role with multiple instances, all instances see it (first to claim it works on it)
- The hook filters messages by the current session's role

**Appending to board.jsonl:**
1. Acquire exclusive lock on `board.lock`
2. Read last line to determine the next message ID (or count lines)
3. Append new JSON line with `\n` separator
4. Release lock
5. Call `notify_desktop()` to wake up the UI

### last-seen Tracking (.vaak/last-seen/{session_id}.json)

Each session tracks which messages it has already seen:

```json
{
  "last_seen_id": 3,
  "updated_at": "2026-02-04T16:00:00Z"
}
```

The hook reads this file to determine which messages are new. After injecting messages into Claude's context, it updates this file. This is per-session (not per-role) because multiple instances of the same role need independent tracking.

**File path:** `.vaak/last-seen/{session_id}.json` where `{session_id}` is the cached session ID (sanitized for filesystem safety — replace special characters with underscores).

### Role Briefings (.vaak/roles/*.md)

Each role gets a markdown file that serves as its job description and onboarding document. This is what a new Claude session reads when it claims a role.

Example: `.vaak/roles/dev-backend.md`

```markdown
# Backend Developer

## Responsibilities
- Implement API endpoints using FastAPI
- Write database models and migrations with SQLAlchemy/Alembic
- Follow existing code patterns in the codebase
- Write clean, testable code with proper error handling

## Tech Stack
- Python 3.11+, FastAPI, SQLAlchemy 2.0, Alembic
- PostgreSQL database
- Pydantic v2 for validation

## Conventions
- All endpoints go in app/api/
- Models go in app/models/
- Business logic goes in app/services/
- Follow existing import patterns and code style

## Current Focus
Implementing the authentication system per docs/auth-design.md.

## Recent Context
- The Architect designed the auth system (message #2)
- You were assigned to implement it (message #3)
```

The Manager (or user through the UI) can update these briefings as the project evolves.

---

## Part 3: MCP Tool Interface

The MCP sidecar (vaak-mcp) exposes these tools to Claude Code sessions. Each tool is registered in the `"tools/list"` handler and dispatched in the `"tools/call"` handler of vaak-mcp.rs.

**In-memory state** (static variables in vaak-mcp.rs):

```rust
// Replace the current ACTIVE_COLLAB with:
static ACTIVE_PROJECT: Mutex<Option<ActiveProjectState>> = Mutex::new(None);

struct ActiveProjectState {
    project_dir: String,    // Normalized path (forward slashes)
    role: String,           // Role slug (e.g., "dev-backend")
    instance: u32,          // Instance number (0, 1, 2...)
    session_id: String,     // This session's ID
}
```

### project_join

Claim a role in a project. Reads the role briefing and returns it along with recent messages.

```
Tool name: "project_join"
Description: "Join an AI development team by claiming a role. Reads your role briefing and shows recent messages directed to you. The project_dir must contain a .vaak/project.json file (create one through the Vaak desktop app)."

Arguments:
  role: string        — Role slug to claim (e.g., "dev-backend")
  project_dir: string — Absolute path to the project directory

Returns (as JSON string in content[0].text):
  {
    "project_name": "My Application",
    "role_title": "Backend Developer",
    "role_slug": "dev-backend",
    "instance": 0,
    "briefing": "# Backend Developer\n\n## Responsibilities\n...",
    "team_status": [
      {"role": "manager", "title": "Project Manager", "active": 1, "max": 1},
      {"role": "dev-backend", "title": "Backend Developer", "active": 2, "max": 3}
    ],
    "recent_messages": [
      {"id": 3, "from": "manager", "type": "directive", "subject": "Implement auth", "body": "..."}
    ],
    "status": "joined"
  }

Error cases:
  - "No .vaak/project.json found in {path}" — project not initialized
  - "Role 'xyz' not found in project" — invalid role slug
  - "Role 'dev-backend' is full (3/3 active instances)" — all slots taken
```

**Implementation steps:**
1. Normalize project_dir (replace `\` with `/`)
2. Read `.vaak/project.json`, verify role exists
3. Acquire file lock on `board.lock`
4. Read `sessions.json`, check role capacity
5. Add binding (or replace stale one), write `sessions.json`
6. Release lock
7. Read role briefing from `.vaak/roles/{role}.md`
8. Read last N messages from `board.jsonl` directed to this role
9. Store state in `ACTIVE_PROJECT` static
10. Call `notify_desktop()`
11. Return result

### project_send

Send a directed message to one or more roles.

```
Tool name: "project_send"
Description: "Send a message to a specific role on your team. Messages are directed — only the target role sees them. Use 'all' to broadcast (requires broadcast permission)."

Arguments:
  to: string          — Target role slug, or "all" for broadcast
  type: string        — Message type: directive, question, answer, status, handoff, review, approval, revision, broadcast
  subject: string     — Brief subject line (displayed in message headers)
  body: string        — Full message content
  metadata: object?   — Optional: {"files": [...], "files_changed": [...], "depends_on": N}

Returns:
  {"message_id": 6, "delivered_to": ["dev-backend"]}

Error cases:
  - "Not in a project. Call project_join first." — ACTIVE_PROJECT is None
  - "Permission denied: 'directive' requires 'assign_tasks' permission" — role lacks permission
  - "Unknown target role: 'xyz'" — invalid role slug in 'to' field
```

**Implementation steps:**
1. Read ACTIVE_PROJECT state
2. Validate permission for the message type
3. Acquire file lock
4. Count lines in board.jsonl to determine next ID
5. Build JSON message object
6. Append line to board.jsonl
7. Release lock
8. Call `notify_desktop()`
9. Return result

### project_check

Check for new messages directed to your role.

```
Tool name: "project_check"
Description: "Check for new messages from your team. Pass the last message number you've seen (0 to get all). The hook automatically shows new messages, but use this for explicit polling or to see older history."

Arguments:
  last_seen: integer  — Last message ID you've processed (0 for all)

Returns:
  {
    "messages": [
      {"id": 4, "from": "dev-backend", "type": "handoff", "timestamp": "...", "subject": "...", "body": "...", "metadata": {...}}
    ],
    "latest_id": 5,
    "team_status": [...]
  }
```

### project_status

Get a snapshot of the project.

```
Tool name: "project_status"
Description: "See who's on the team and what's happening. Shows all roles, their status, and pending message counts."

Arguments: (none)

Returns:
  {
    "project_name": "My Application",
    "your_role": "dev-backend",
    "your_instance": 0,
    "roles": [
      {"slug": "manager", "title": "Project Manager", "active_instances": 1, "max_instances": 1, "status": "active"},
      {"slug": "dev-backend", "title": "Backend Developer", "active_instances": 2, "max_instances": 3, "status": "active"},
      {"slug": "tester", "title": "QA Tester", "active_instances": 0, "max_instances": 2, "status": "vacant"}
    ],
    "pending_messages": 2,
    "total_messages": 5
  }
```

### project_update_briefing

Update a role's briefing file. Used by the Manager to update task assignments or context.

```
Tool name: "project_update_briefing"
Description: "Update a role's briefing/job description. The briefing is what new team members read when they join. Typically used by the Manager to update assignments."

Arguments:
  role: string        — Role slug to update
  content: string     — New markdown content for the briefing

Returns:
  {"success": true, "role": "dev-backend"}

Error cases:
  - "Permission denied: requires 'assign_tasks' permission" — only Manager can update briefings
```

### project_leave

Release your role so another session can claim it.

```
Tool name: "project_leave"
Description: "Leave the project and release your role. Another session can then claim it."

Arguments: (none)

Returns:
  {"role_released": "dev-backend", "instance": 0}
```

**Implementation:** Acquire file lock, remove binding from sessions.json, clear ACTIVE_PROJECT state, release lock, notify_desktop.

---

## Part 4: Hook Integration

The UserPromptSubmit hook is the primary push mechanism. It fires on every user prompt in every Claude Code session.

### How the Hook Finds Project State

The hook runs as `vaak-mcp.exe --hook` in a separate short-lived process. It needs to:

1. **Find the session ID:** Read from cache at `%APPDATA%\Vaak\session-cache\{ppid}.txt` (cached by the long-running MCP sidecar). Falls back to computing it fresh if cache miss.

2. **Find the project:** Walk up from the current working directory looking for `.vaak/project.json`. Check CWD first, then parent, then grandparent, etc. (Similar to how git finds `.git`). This handles cases where Claude Code's CWD is a subdirectory of the project.

3. **Find the role:** Read `sessions.json`, look for a binding matching this session ID. If found, this session is in a project and has a role.

4. **Find new messages:** Read `board.jsonl`, filter for messages where `to` matches the session's role. Read `.vaak/last-seen/{session_id}.json` to get the last-seen message ID. Messages with `id > last_seen` are new.

5. **Update last-seen:** Write the current highest message ID to the last-seen file so the same messages aren't injected again on the next prompt.

### Hook Output Format

The hook prints to stdout. This text is injected into Claude's context as part of the system prompt.

**When not in a project (no .vaak/project.json found):**
```
IMPORTANT: You MUST call the mcp__vaak__speak tool to speak every response aloud. [... existing voice instructions ...]
```
(Same as current behavior — voice instructions only)

**When in a project with no new messages:**
```
IMPORTANT: You MUST call the mcp__vaak__speak tool to speak every response aloud. [... voice instructions ...]
TEAM: You are the Backend Developer (instance 0) on project "My Application". Team: Manager (active), Architect (active), Backend Dev x2 (you + 1), Tester (vacant). No new messages. Use project_send to communicate, project_check to see history.
```

**When in a project with new messages:**
```
IMPORTANT: You MUST call the mcp__vaak__speak tool to speak every response aloud. [... voice instructions ...]
TEAM: You are the Backend Developer (instance 0) on project "My Application". Team: Manager (active), Architect (active), Backend Dev x2 (you + 1), Tester (vacant).

NEW MESSAGES (2 unread):

[#3] FROM Manager (directive): "Implement auth endpoints"
Implement the auth system per the Architect's design in docs/auth-design.md. Create: POST /auth/register, POST /auth/login, POST /auth/refresh, POST /auth/logout. Follow existing patterns in app/api/.

[#6] FROM Tester (review): "Auth endpoint issues found"
Found 3 issues in app/api/auth.py: (1) No rate limiting on /auth/login. (2) Refresh token not invalidated on /auth/logout. (3) Missing email format validation in /auth/register.

Use project_send to respond. Use project_check for full history.
```

This way, Claude naturally sees the messages as part of its conversation context and responds accordingly — no need to explicitly call project_check in most cases.

### Message Size Management

If there are many new messages (e.g., 50 unread), injecting all of them would consume too much context. The hook should:
- Show the 10 most recent messages in full
- Summarize older ones: "... and 40 earlier messages. Use project_check(0) to see all."
- Always show directives and reviews in full (they require action)
- Truncate very long message bodies to 500 characters with "... (truncated, use project_check to see full)"

---

## Part 5: Desktop UI (Collab Tab Redesign)

The Collab tab lives in `TranscriptApp.tsx` within the `transcript` window. It's rendered inside a div that's always mounted (so event listeners stay active):

```typescript
// In TranscriptApp.tsx (~line 941)
<div style={{ display: activeTab === "collab" ? "contents" : "none" }}>
  <CollabTab />
</div>
```

### Views

**1. Project Setup (shown when no project is loaded)**
- Centered layout with Vaak logo/icon
- "Watch Project" section:
  - Text input for project directory path
  - "Browse" button (opens native folder picker via `@tauri-apps/plugin-dialog`)
  - "Watch" button — starts monitoring .vaak/ directory
- "Create Project" button — opens project creation wizard (Phase 2)
- Error display for invalid paths

**2. Project Overview (default view when project is loaded)**
- **Header bar:** Project name, description, "Stop" button
- **Role cards** in a responsive grid (CSS grid, auto-fill, min 200px):
  - Card background: subtle dark (`rgba(255,255,255,0.03)`)
  - Role title (white, 14px, bold)
  - Status indicator: green dot = active, yellow dot = stale, gray dot = vacant
  - Instance count: "2/3 active" or "vacant"
  - Brief description (gray, 12px, truncated to 2 lines)
  - Click to expand role detail
- **"Add Role" button** at the end of the grid (dashed border, plus icon)

**3. Message Timeline (scrollable, below role cards)**
- Chronological message list (newest at bottom, auto-scroll)
- Each message card:
  - Left border color matches sender role (blue for architect, green for developer, etc.)
  - Header: #{id} | sender role badge | → recipient role | type badge | timestamp
  - Subject line (bold, 14px)
  - Body text (regular, 13px, pre-wrap)
  - Metadata pills (files, dependencies)
- Filter bar at top:
  - Role filter dropdown
  - Type filter dropdown
  - Text search input

**4. Role Detail (expandable/modal from clicking a role card)**
- Full role briefing (rendered markdown)
- "Edit Briefing" button → opens text editor
- Session info: session ID, joined timestamp, last heartbeat
- Message history filtered to this role

### Tauri Commands Needed

```rust
// New commands to register in main.rs:

#[tauri::command]
fn watch_project_dir(dir: String) -> Result<serde_json::Value, String>
// Reads .vaak/project.json, sessions.json, board.jsonl
// Stores dir in WATCHED_PROJECT_DIR static
// Returns full project state for initial UI render

#[tauri::command]
fn stop_watching_project() -> Result<(), String>
// Clears WATCHED_PROJECT_DIR static (fixes current bug where stop doesn't clear Rust state)

#[tauri::command]
fn create_project(dir: String, name: String, description: String, roles: Vec<RoleConfig>) -> Result<serde_json::Value, String>
// Creates .vaak/ directory structure, project.json, role briefing files
// Returns created project state (Phase 2)

#[tauri::command]
fn update_role(dir: String, role_slug: String, updates: RoleUpdate) -> Result<(), String>
// Updates role in project.json and/or role briefing file (Phase 2)

#[tauri::command]
fn add_role(dir: String, role: RoleConfig) -> Result<(), String>
// Adds a new role to project.json, creates briefing file (Phase 2)

#[tauri::command]
fn remove_role(dir: String, role_slug: String) -> Result<(), String>
// Removes role from project.json, optionally deletes briefing (Phase 2)
```

### Tauri Events

```
"project-update" — Emitted by file watcher when any .vaak/ file changes
  Payload: Full project state (project.json + sessions.json + board.jsonl parsed)

"project-file-changed" — Emitted by /collab/notify HTTP handler
  Payload: {} (triggers re-read from React side)
```

### File Watcher Updates

The `start_collab_watcher()` function in main.rs needs to be updated:

- Watch the `.vaak/` directory (not a single file)
- Check modification times of: `project.json`, `sessions.json`, `board.jsonl`
- If any changed: re-parse all three and emit `"project-update"` event
- For board.jsonl specifically: only read new lines (track file size/line count, seek to last position)

---

## Part 6: Implementation Phases

### Phase 1: Foundation

**Goal**: Get the core protocol working with 2 roles (Manager + Developer). Manual project setup (create .vaak/ files by hand or via MCP tool). Basic UI showing project state and messages.

**Changes by file:**

**desktop/src-tauri/src/bin/vaak-mcp.rs:**
1. Replace `ACTIVE_COLLAB` static with `ACTIVE_PROJECT` static (new struct)
2. Remove: `CollabHeader`, `CollabMsg`, `CollabParticipant` structs
3. Remove: `parse_collab_header()`, `parse_collab_messages()`, `rebuild_collab_file()`, `generate_collab_id()`, `now_iso()` (replace with proper time handling), `time_short()`
4. Remove: `handle_collab_join()`, `handle_collab_send()`, `handle_collab_check()`, `check_collab_from_file()`
5. Add: `ActiveProjectState` struct, project file reading/writing functions
6. Add: `handle_project_join()`, `handle_project_send()`, `handle_project_check()`, `handle_project_status()`, `handle_project_leave()`, `handle_project_update_briefing()`
7. Add: `check_project_from_cwd()` — replaces `check_collab_from_file()`, walks up directories to find `.vaak/project.json`
8. Add: JSONL append function with file lock
9. Add: `find_project_root()` — walks up from CWD looking for `.vaak/project.json`
10. Update: `"tools/list"` handler — replace collab tools with project tools
11. Update: `"tools/call"` handler — dispatch to new handlers
12. Update: `run_hook()` — call `check_project_from_cwd()` instead of `check_collab_from_file()`, inject role context and full messages
13. Keep: `with_file_lock()`, `notify_desktop()`, session ID system, `send_heartbeat()`, `send_to_vaak()`, `cache_session_id()`, `read_cached_session_id()`

**desktop/src-tauri/src/collab.rs:**
1. Keep: `SessionRegistry`, `SessionInfo` (lines 1-89) — still used for heartbeat tracking
2. Remove: `CollabParticipant`, `CollabMessage`, `ParsedCollab`, `parse_collab_file()` (lines 91-178)
3. Add: `ProjectState` struct (mirrors project.json), `SessionBinding` struct (mirrors sessions.json entries), `BoardMessage` struct (mirrors board.jsonl entries)
4. Add: `ParsedProject` struct — combines all state for the frontend: project info, sessions, messages
5. Add: `parse_project_dir()` — reads all .vaak/ files and returns `ParsedProject`

**desktop/src-tauri/src/main.rs:**
1. Update: HTTP handler for `/collab/notify` → emit `"project-file-changed"` event (rename for clarity)
2. Update: `start_collab_watcher()` → `start_project_watcher()` — watch .vaak/ directory, parse all files on change
3. Update: `watch_collab_dir()` → `watch_project_dir()` — new Tauri command
4. Add: `stop_watching_project()` Tauri command
5. Update: Command registration in `tauri::generate_handler![]`
6. Update: Static variables — rename from `COLLAB_WATCHED_DIR` to `PROJECT_WATCHED_DIR`, track mtime for multiple files

**desktop/src/components/CollabTab.tsx:**
1. Full rewrite — project overview with role cards and message timeline
2. Event listeners for `"project-update"` and `"project-file-changed"`
3. Basic role card display (title, status, instances)
4. Message timeline with role-colored borders and type badges

**desktop/src/lib/collabTypes.ts:**
1. Full rewrite — new interfaces: `ProjectConfig`, `RoleConfig`, `SessionBinding`, `BoardMessage`, `ParsedProject`

**desktop/src/styles/collab.css:**
1. Full rewrite — new styles for project dashboard, role cards, message timeline

**desktop/src/TranscriptApp.tsx:**
1. Minimal change — update import if component file is renamed

### Phase 2: Role Management UI

**Goal**: Enable creating and managing custom roles through the desktop UI.

1. Add "Create Project" wizard component — form for project name, description, template selection
2. Add "Add Role" form — title, slug auto-generation, description, max instances, permissions
3. Add "Edit Role" form — modify existing role properties and briefing
4. Add "Remove Role" confirmation dialog
5. Add Tauri commands: `create_project`, `add_role`, `update_role`, `remove_role`
6. Add role briefing editor (markdown textarea with preview)

### Phase 3: Manager Capabilities

**Goal**: Give the Manager role special tools for team coordination.

1. Add `project_assign` MCP tool — create a structured task assignment
2. Add `project_review` MCP tool — approve or request revisions on a handoff
3. Manager-specific hook output: team overview, pending reviews, blocked tasks
4. `project_update_briefing` — already defined in Phase 1 tools, implement the permission check

### Phase 4: Scale & Polish

**Goal**: Support larger teams and improve the experience.

1. Message threading (replies reference parent message ID via `metadata.in_reply_to`)
2. Task tracking (assignments have status: pending, in_progress, completed, revision_needed)
3. Activity log in the UI (who joined, who left, role changes)
4. Message search and filtering in the UI
5. Notification sounds / visual alerts when new messages arrive
6. Export project history for documentation
7. Role colors assignable per role (not just architect=blue, developer=green)

---

## Part 7: Key Design Decisions

### Why JSONL instead of a single JSON file or Markdown?

- **Append-only**: New messages are appended as single lines. No need to read-parse-modify-write the entire file. This is critical for concurrency — two sessions can append near-simultaneously with minimal lock contention.
- **Efficient reads**: The hook only needs to read lines after the last-seen offset. For a board with 1000 messages, checking for new ones only reads the tail.
- **Human-readable**: Each line is a complete JSON object. You can `tail -5 board.jsonl` to see the last 5 messages.
- **Corruption-resilient**: If a write is interrupted mid-line, only that one line is corrupted. The rest of the file remains valid. A JSON file with 1000 messages would be entirely corrupted by a partial write.

### Why file-based instead of a database or server?

- **Zero infrastructure**: No server to run, no database to set up. The project directory IS the collaboration state.
- **Debuggable**: You can read the files directly. Open board.jsonl in a text editor and see every message.
- **Portable**: Copy the .vaak directory and you have the full project history.
- **Git-friendly**: The collaboration state can be committed to the repo if desired.
- **Works offline**: No network dependency. Everything is local filesystem operations.

### Why directed messages instead of a shared chat?

- **Context management**: With 10+ roles, a shared chat would flood each session with irrelevant messages. Directed messages ensure each role only sees what's relevant to them.
- **Preserves independence**: A Developer doesn't need to see the Manager's conversation with the Tester. Keeping contexts separate preserves the "fresh perspective" benefit.
- **Scales**: Adding more roles doesn't increase noise for existing roles.

### Why the Manager pattern?

- **User experience**: The user talks to one session, not 20. The Manager is the single point of contact.
- **Coordination**: Someone needs to break down requirements, assign tasks, and verify outputs. An AI Manager can do this.
- **Quality control**: The Manager reviews handoffs before passing work to the next stage, catching issues early.

### Why role briefings as separate files?

- **Onboarding**: When a new session claims a role, it needs to know what to do. The briefing is its job description.
- **Continuity**: Sessions are ephemeral, but the role briefing persists. Knowledge survives session changes.
- **Customizable**: The Manager (or user) can update briefings as the project evolves, changing a role's focus without restarting the session.
- **Readable**: Markdown files that anyone (human or AI) can read and edit directly.

### Why walk up directories to find .vaak/?

- Claude Code sessions may have their CWD set to a subdirectory (e.g., `project/backend/` or `project/src/`). Walking up ensures the project is found regardless of which subdirectory the session is in.
- This mirrors how Git finds `.git/` — it's a proven pattern that developers expect.
- The walk stops at the filesystem root to avoid infinite loops.

### Why per-session last-seen tracking instead of per-role?

- Multiple instances of the same role need independent tracking. If dev-backend #0 reads message #5, dev-backend #1 shouldn't also mark it as seen.
- Session IDs are stable within a terminal session, so the tracking persists correctly across multiple prompts.
- Using session ID (not role) also handles the case where a session leaves one role and joins another — its last-seen state doesn't carry over.
