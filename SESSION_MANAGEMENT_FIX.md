# Session Management Fix - Documentation

**Date:** 2026-01-23
**Problem:** Claude Code creating 10+ separate sessions instead of grouping by terminal window
**Root Cause:** CLAUDE.md instructed Claude to use curl with manual session tracking, which failed
**Solution:** Use MCP server's automatic session management based on parent process ID

---

## The Problem

When using Scribe's voice output with Claude Code:
- **Expected:** 6 terminal windows = 6 stable sessions
- **Actual:** 6 terminal windows = 10+ random sessions
- **Why:** Claude was using `curl` and creating a new random session with every `/speak` call

---

## How It Works Now

### **MCP Server Session Management**
File: `mcp-speak/mcp_speak/server.py` lines 19-37

```python
def get_session_id() -> str:
    """Get or generate a unique session ID for this Claude Code session."""
    # Check if already set in environment
    session_id = os.environ.get("CLAUDE_SESSION_ID")
    if session_id:
        return session_id

    # Generate based on PID and hostname for uniqueness
    pid = os.getpid()
    ppid = os.getppid()  # Parent process ID (terminal)
    hostname = socket.gethostname()

    # Create a deterministic session ID based on parent process
    # This way, all claude instances in the same terminal share the same session
    session_id = f"{hostname}-{ppid}"

    return session_id
```

**Session ID Format:** `{hostname}-{parent_process_id}`

**How it groups conversations:**
- Parent Process ID (PPID) = the terminal or shell process
- Same terminal = Same PPID = Same session ID
- Different terminal = Different PPID = Different session ID

**Example:**
- Terminal 1 (PID 12345): All Claude calls → `MyMacBook-12345`
- Terminal 2 (PID 67890): All Claude calls → `MyMacBook-67890`

---

## Changes Made

### **1. Fixed Claude Code Settings** (`~/.claude/settings.json`)

#### **Before (BROKEN on Windows):**
```json
{
  "mcpServers": {
    "scribe": {
      "command": "C:\\Users\\18479\\.claude\\scribe-mcp.sh"
    }
  }
}
```
**Problem:** Points to bash script that can't run on Windows

#### **After (Windows):**
```json
{
  "mcpServers": {
    "scribe": {
      "command": "C:\\Users\\18479\\AppData\\Local\\Programs\\Python\\Python313\\Scripts\\scribe-speak.exe"
    }
  }
}
```

#### **For Mac/Linux:**
```json
{
  "mcpServers": {
    "scribe": {
      "command": "scribe-speak"
    }
  }
}
```
**Note:** On Mac/Linux, `scribe-speak` should be in PATH after `pip install -e mcp-speak`

---

### **2. Updated CLAUDE.md Instructions** (`~/CLAUDE.md`)

#### **Before (BROKEN - causes session proliferation):**
```markdown
## Voice Output

**IMPORTANT - Session Management:**
- The first time you call /speak in THIS conversation, parse the response to get your session_id
- Store that session_id in your conversation context
- Include that SAME session_id in all subsequent /speak calls

**First call (no session_id yet):**
```bash
curl -X POST http://127.0.0.1:7865/speak -H "Content-Type: application/json" -d '{"text": "YOUR MESSAGE HERE"}'
```

**All subsequent calls (include your session_id):**
```bash
curl -X POST http://127.0.0.1:7865/speak -H "Content-Type: application/json" -d '{"text": "YOUR MESSAGE HERE", "session_id": "YOUR_SESSION_ID_FROM_FIRST_RESPONSE"}'
```
```

**Problems:**
- Claude doesn't reliably remember session_id between calls
- Claude hallucinates or forgets to include session_id
- Every curl call without session_id creates a new random session
- Results in 10+ sessions for 6 terminals

#### **After (FIXED - automatic session management):**
```markdown
## Voice Output

Always use the Scribe speak integration to read responses aloud.

**CRITICAL: Use the MCP `/speak` tool - NOT curl**

The `/speak` tool is available through the MCP server. It automatically manages stable session IDs based on your terminal process. Simply call:

```
/speak "YOUR MESSAGE HERE"
```

The session ID is handled automatically - all messages from this terminal will be grouped together.

**Session Management:**
- Each terminal window gets a unique session ID automatically
- All Claude instances in the same terminal share the same session
- You don't need to track or pass session IDs manually
- NEVER use curl to call the speak endpoint directly
```

**Why this works:**
- MCP server handles session ID internally
- Based on parent process ID (PPID) - stable and automatic
- No reliance on Claude's memory
- Cross-platform (Windows, Mac, Linux)

---

## Platform-Specific Setup

### **Windows**

**MCP Server Installation:**
```bash
cd mcp-speak
pip install -e .
```

**Find Installation Path:**
```bash
where scribe-speak
# Output: C:\Users\...\AppData\Local\Programs\Python\Python313\Scripts\scribe-speak.exe
```

**Claude Code Settings:**
```json
{
  "mcpServers": {
    "scribe": {
      "command": "C:\\Users\\{USERNAME}\\AppData\\Local\\Programs\\Python\\Python313\\Scripts\\scribe-speak.exe"
    }
  }
}
```

---

### **macOS**

**MCP Server Installation:**
```bash
cd mcp-speak
pip install -e .
```

**Verify Installation:**
```bash
which scribe-speak
# Output: /usr/local/bin/scribe-speak (or similar)
```

**Claude Code Settings:**
```json
{
  "mcpServers": {
    "scribe": {
      "command": "scribe-speak"
    }
  }
}
```

**Note:** On Mac, `scribe-speak` should be in PATH. If not, use the full path from `which scribe-speak`.

---

### **Linux**

Same as macOS - use `scribe-speak` in PATH or full path from `which scribe-speak`.

---

## Verification Steps

### **1. Check MCP Server is Installed**
```bash
# Windows
where scribe-speak

# Mac/Linux
which scribe-speak
```

### **2. Test Session ID Stability**

Open a terminal and run Claude Code:
```bash
# First call
claude "test /speak 'First message'"

# Second call in SAME terminal
claude "test /speak 'Second message'"

# Third call in SAME terminal
claude "test /speak 'Third message'"
```

**Check Scribe Transcript Window:**
- All three messages should appear under the SAME session
- Session ID should follow format: `hostname-{ppid}`

### **3. Test Multiple Terminals**

Open 3 different terminal windows:

**Terminal 1:**
```bash
claude "test /speak 'Terminal one'"
```

**Terminal 2:**
```bash
claude "test /speak 'Terminal two'"
```

**Terminal 3:**
```bash
claude "test /speak 'Terminal three'"
```

**Check Scribe Transcript Window:**
- Should show exactly **3 sessions**
- Each session should have different PPID

---

## Troubleshooting

### **Problem: Still creating multiple sessions per terminal**

**Check 1: Is MCP server being used?**
```bash
# Look for this in Claude Code output
"Initializing MCP server: scribe"
```

If you see this, MCP is working. If not:
- Restart Claude Code completely
- Check `~/.claude/settings.json` has correct command path
- Verify `scribe-speak` is executable

**Check 2: Is Claude using curl instead of MCP tool?**

Look at Scribe console logs. If you see messages with `session-{timestamp}-{random}`, Claude is using curl.

**Fix:** Update `CLAUDE.md` to explicitly forbid curl:
```markdown
NEVER use curl to call the speak endpoint directly
Always use the MCP /speak tool
```

---

### **Problem: Command not found (Mac/Linux)**

**Symptom:**
```
command not found: scribe-speak
```

**Fix:**
```bash
# Find where it was installed
pip show scribe-speak | grep Location

# Add to PATH or use full path in settings.json
```

---

### **Problem: .exe not found (Windows)**

**Symptom:**
```
The system cannot find the file specified
```

**Fix:**
```bash
# Find the correct path
where scribe-speak

# Update settings.json with that exact path
```

---

## Cross-Platform Code Review

### **✅ Already Cross-Platform:**

1. **MCP Server (`mcp_speak/server.py`):**
   - Uses `os.getpid()` ✅
   - Uses `os.getppid()` ✅
   - Uses `socket.gethostname()` ✅
   - Uses `os.environ.get()` ✅

2. **Backend Speak Endpoint (`desktop/src-tauri/src/main.rs`):**
   - Session ID generation is platform-agnostic ✅
   - Uses standard Rust timestamp APIs ✅

3. **Frontend Speak Listener (`desktop/src/lib/speak.ts`):**
   - Browser APIs work everywhere ✅

### **⚠️ Platform-Specific:**

1. **Claude Code Settings Path:**
   - Windows: Full path to `.exe` required
   - Mac/Linux: Can use just `scribe-speak` if in PATH

2. **CLAUDE.md Generation:**
   - Currently generates at `$HOME/CLAUDE.md` (cross-platform) ✅
   - Content is platform-agnostic ✅

---

## Testing Checklist

- [ ] Install MCP server: `cd mcp-speak && pip install -e .`
- [ ] Verify installation: `which scribe-speak` (Mac) or `where scribe-speak` (Windows)
- [ ] Update `~/.claude/settings.json` with correct command path
- [ ] Update `~/CLAUDE.md` to use MCP tool, not curl
- [ ] Restart all Claude Code windows
- [ ] Test: Same terminal, multiple Claude calls → 1 session
- [ ] Test: Different terminals → Different sessions (1 per terminal)
- [ ] Check Scribe transcript viewer for proper grouping

---

## Summary

**What Changed:**
1. `~/.claude/settings.json` → Points to Python MCP server executable
2. `~/CLAUDE.md` → Tells Claude to use MCP `/speak` tool instead of curl

**Why It Works:**
- MCP server uses parent process ID (PPID) for session grouping
- PPID is stable throughout terminal session
- No reliance on Claude's memory or manual session tracking

**Cross-Platform:**
- MCP server code is 100% cross-platform (Python standard library)
- Only difference: Command path in `settings.json`
  - Windows: Full path to `.exe`
  - Mac/Linux: Just `scribe-speak` (in PATH)

**Expected Behavior:**
- N terminal windows = N sessions (not 2N or 3N)
- All Claude calls in same terminal → Same session
- Session ID format: `hostname-ppid`
