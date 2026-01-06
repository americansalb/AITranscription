# MCP Speak

An MCP server that gives Claude Code the ability to speak aloud. When Claude Code uses the `speak` tool, you'll hear it through your speakers.

## Why?

If you're paying for Claude Max/Pro ($20-200/month), you already have unlimited Claude usage. This lets Claude Code announce what it's doing **without any extra API costs** - the explanation comes from Claude Code itself, not a separate API call.

## Quick Start

### 1. Install the MCP server

```bash
cd mcp-speak
pip install -e .
```

### 2. Add to Claude Code settings

Edit `~/.claude/settings.json`:

```json
{
  "mcpServers": {
    "speak": {
      "command": "mcp-speak",
      "env": {
        "GROQ_API_KEY": "your-groq-api-key-here"
      }
    }
  }
}
```

**Note:** Groq API key is optional but recommended for better voice quality. Without it, the server falls back to your system's built-in TTS (macOS `say`, Windows SAPI, Linux `espeak`).

Get a free Groq API key at: https://console.groq.com/keys

### 3. Tell Claude Code to use it

When starting a Claude Code session, tell it:

> "When you write or edit files, use the speak tool to briefly announce what you did."

Or add it to your custom instructions in `~/.claude/CLAUDE.md`:

```markdown
## Voice Announcements

After writing or editing files, use the `speak` tool to briefly announce what you did.
Keep announcements to 1-2 sentences. Example: "I added a login function that validates email and password."
```

## How It Works

```
You: "Add authentication"
         │
         ▼
Claude Code writes auth.ts
         │
         ▼
Claude Code calls speak("I added a login function that validates credentials")
         │
         ▼
You hear the announcement through your speakers
```

The key insight: **Claude Code generates the explanation as part of its normal output** (which you're already paying for), then calls the speak tool to vocalize it.

## Voice Options

When Groq TTS is configured, you can specify different voices:

- `Aria-PlayAI` (default) - Female, natural
- `Atlas-PlayAI` - Male, natural
- `Indigo-PlayAI` - Neutral

Example: `speak("Hello world", voice="Atlas-PlayAI")`

## Troubleshooting

### No sound

1. Check your system volume
2. Verify the MCP server is running: `mcp-speak` should start without errors
3. Test system TTS directly:
   - macOS: `say "hello"`
   - Linux: `espeak "hello"`
   - Windows: Should work automatically

### Groq TTS not working

1. Verify your API key is set correctly in settings.json
2. Check Groq console for API status: https://console.groq.com

### Claude Code not using speak tool

Make sure you've told Claude Code to use it. Add to your prompt or CLAUDE.md file.

## Uninstall

1. Remove from `~/.claude/settings.json`
2. `pip uninstall mcp-speak`
