# Claude Code Voice Integration Setup

This guide explains how to configure Claude Code to send code change events to Scribe for voice explanations.

## Prerequisites

- Scribe backend running on `localhost:8000`
- Scribe desktop app open with voice feature enabled
- `jq` and `curl` installed on your system

## Installation

### 1. Create the hooks directory

```bash
mkdir -p ~/.claude/hooks
```

### 2. Copy the hook script

```bash
cp claude-hooks/scribe-voice-notify.sh ~/.claude/hooks/
chmod +x ~/.claude/hooks/scribe-voice-notify.sh
```

### 3. Configure Claude Code settings

Edit `~/.claude/settings.json` and add (or merge) the hooks configuration:

```json
{
  "hooks": {
    "PostToolUse": [
      {
        "matcher": "Write|Edit",
        "hooks": [
          {
            "type": "command",
            "command": "~/.claude/hooks/scribe-voice-notify.sh",
            "timeout": 10
          }
        ]
      }
    ]
  }
}
```

If you have existing settings, merge the hooks section carefully.

### 4. Verify installation

Test the hook script directly:

```bash
echo '{"hook_event_name":"PostToolUse","tool_name":"Write","tool_input":{"file_path":"test.ts","content":"console.log(\"hello\")"}}' | ~/.claude/hooks/scribe-voice-notify.sh
```

This should silently send the event to Scribe (if the backend is running).

## How It Works

```
You: "Add a login function"
         │
         ▼
Claude Code writes code
         │
         ▼
PostToolUse hook fires ───────────────────────────┐
         │                                        │
         │ (Claude continues immediately)         │
         ▼                                        ▼
    More code work                    Scribe receives event
                                              │
                                              ▼
                                    Haiku generates explanation
                                              │
                                              ▼
                                    Groq TTS creates audio
                                              │
                                              ▼
                                    You hear: "I added a login
                                    function that validates..."
```

## Troubleshooting

### Hook not firing

1. Check Claude Code settings are correct:
   ```bash
   cat ~/.claude/settings.json | jq '.hooks'
   ```

2. Verify script is executable:
   ```bash
   ls -la ~/.claude/hooks/scribe-voice-notify.sh
   ```

### No audio playing

1. Verify Scribe backend is running:
   ```bash
   curl http://localhost:8000/api/v1/health
   ```

2. Check SSE stream is working:
   ```bash
   curl -N http://localhost:8000/api/v1/voice-stream
   ```
   You should see `data: {"type": "connected", ...}` immediately.

3. Verify Scribe desktop app has voice enabled in settings.

### Testing the full flow

Send a test event manually:

```bash
curl -X POST http://localhost:8000/api/v1/claude-event \
  -H "Content-Type: application/json" \
  -d '{
    "hook_event_name": "PostToolUse",
    "tool_name": "Write",
    "tool_input": {
      "file_path": "src/auth.ts",
      "content": "export function login(email: string, password: string) {\n  return email && password.length >= 8;\n}"
    }
  }'
```

You should hear Scribe explain the code change.

## Configuration Options

### Environment Variables

- `SCRIBE_API_URL`: Override the backend URL (default: `http://localhost:8000`)

### Customizing the hook

You can modify `~/.claude/hooks/scribe-voice-notify.sh` to:

- Filter specific file types
- Skip certain directories
- Add logging for debugging

## Disabling Voice Integration

To temporarily disable without removing the hook:

1. In Scribe settings, toggle off "Voice explanations"

Or to completely remove:

1. Delete the hook configuration from `~/.claude/settings.json`
2. Remove the script: `rm ~/.claude/hooks/scribe-voice-notify.sh`
