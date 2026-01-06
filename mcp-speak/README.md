# Scribe Speak - Claude Code Voice Integration

Makes Claude Code speak through Scribe. When Claude Code uses the `speak` tool, you hear it through your speakers.

## Setup (2 steps)

### 1. Install the MCP server

```bash
cd mcp-speak
pip install -e .
```

### 2. Add to Claude Code

Edit `~/.claude/settings.json`:

```json
{
  "mcpServers": {
    "scribe": {
      "command": "scribe-speak"
    }
  }
}
```

That's it. Now tell Claude Code: "Use the speak tool to announce what you're doing."

## How it works

```
Claude Code → speak("I added a login function") → Scribe → You hear it
```

- Claude Code calls the `speak` tool
- MCP server sends text to Scribe (localhost:7865)
- Scribe speaks it using your system's text-to-speech
- **No API keys needed** - uses free system TTS

## Requirements

- Scribe desktop app must be running
- Python 3.10+
