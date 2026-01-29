# Claude Code Voice Integration Setup - MCP Server

This guide helps you set up Scribe's voice integration with Claude Code using the MCP server on any platform.

---

## Quick Setup

### **Step 1: Install the MCP Server**

```bash
cd mcp-speak
pip install -e .
```

This installs the `scribe-speak` MCP server that handles voice output.

---

### **Step 2: Configure Claude Code**

#### **Option A: Automatic Setup (Recommended)**

Run this script from the project root:

```bash
# Make it executable (Mac/Linux)
chmod +x claude-hooks/setup-claude-integration.sh

# Run the setup
./claude-hooks/setup-claude-integration.sh
```

This will:
- Detect your platform (Windows/Mac/Linux)
- Find the `scribe-speak` command location
- Create/update `~/.claude/settings.json` automatically
- Update `~/CLAUDE.md` with correct instructions

#### **Option B: Manual Setup**

**For macOS/Linux:**

Create or edit `~/.claude/settings.json`:
```json
{
  "mcpServers": {
    "scribe": {
      "command": "scribe-speak"
    }
  }
}
```

**For Windows:**

Find where `scribe-speak.exe` is installed:
```bash
where scribe-speak
```

Then create or edit `%USERPROFILE%\.claude\settings.json`:
```json
{
  "mcpServers": {
    "scribe": {
      "command": "C:\\Users\\YOUR_USERNAME\\AppData\\Local\\Programs\\Python\\Python313\\Scripts\\scribe-speak.exe"
    }
  }
}
```
(Replace the path with your actual installation path)

---

### **Step 3: Verify Installation**

**Check the MCP server is installed:**
```bash
# Mac/Linux
which scribe-speak

# Windows
where scribe-speak
```

You should see a path to the executable.

**Test the voice integration:**

1. Start Scribe desktop app
2. Open Claude Code in a terminal
3. Run: `claude "test /speak 'Hello from Claude Code'"`
4. You should hear the voice output through your speakers

---

## How It Works

### **Session Management**

Each terminal window gets a unique session ID based on its process ID:

```
Session ID = hostname-{parent_process_id}
```

**Example:**
- Terminal 1 (shell PID 1234): `MacBook-1234`
- Terminal 2 (shell PID 5678): `MacBook-5678`

All Claude calls from the **same terminal** share the **same session**.

### **Voice Output**

When Claude Code calls `/speak`:
1. MCP server receives the text
2. Automatically adds stable session ID based on terminal PID
3. Sends to Scribe's `/speak` endpoint (http://127.0.0.1:7865/speak)
4. Scribe plays the audio and logs it in the transcript viewer

---

## Troubleshooting

### **"Command not found: scribe-speak"**

The MCP server isn't in your PATH.

**Fix:**
```bash
# Find where pip installed it
pip show mcp-speak | grep Location

# Add that location to PATH, or use full path in settings.json
```

### **Multiple sessions for same terminal**

Claude might be using `curl` instead of the MCP tool.

**Check:** Look at Scribe console logs. If session IDs have format `session-{timestamp}-{random}`, Claude is using curl.

**Fix:**
1. Make sure `~/CLAUDE.md` instructs Claude to use `/speak` tool
2. Make sure it explicitly says **NOT to use curl**
3. Restart Claude Code windows

### **No voice output**

**Check:**
1. Is Scribe desktop app running?
2. Is the speak server running on port 7865?
   ```bash
   curl http://127.0.0.1:7865/speak -X POST -H "Content-Type: application/json" -d '{"text":"test"}'
   ```
3. Does Claude Code show the MCP server is initialized?
   - Look for: `Initializing MCP server: scribe`

---

## Platform Differences

| Platform | Command in settings.json | Location |
|----------|-------------------------|----------|
| macOS | `scribe-speak` | `/usr/local/bin/scribe-speak` (or similar) |
| Linux | `scribe-speak` | `/usr/local/bin/scribe-speak` (or similar) |
| Windows | Full path to `.exe` | `C:\Users\...\Python313\Scripts\scribe-speak.exe` |

**Why?**
- On Unix systems (Mac/Linux), installed scripts are typically in PATH
- On Windows, Python Scripts folder may not be in PATH, so full path is needed

---

## See Also

- [SESSION_MANAGEMENT_FIX.md](SESSION_MANAGEMENT_FIX.md) - Detailed explanation of how session management works
- [claude-hooks/SETUP.md](claude-hooks/SETUP.md) - PostToolUse hooks for automatic code explanations
- [mcp-speak/README.md](mcp-speak/README.md) - MCP server documentation
