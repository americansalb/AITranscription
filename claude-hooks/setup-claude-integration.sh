#!/bin/bash
# Automatic setup script for Claude Code voice integration
# Works on macOS, Linux, and Windows (with Git Bash/WSL)

set -e

echo "=== Vaak Claude Code Integration Setup ==="
echo ""

# Detect platform
OS=$(uname -s)
case "$OS" in
    Darwin*)
        PLATFORM="mac"
        SETTINGS_DIR="$HOME/.claude"
        CLAUDE_MD="$HOME/CLAUDE.md"
        ;;
    Linux*)
        PLATFORM="linux"
        SETTINGS_DIR="$HOME/.claude"
        CLAUDE_MD="$HOME/CLAUDE.md"
        ;;
    MINGW*|MSYS*|CYGWIN*)
        PLATFORM="windows"
        SETTINGS_DIR="$USERPROFILE/.claude"
        CLAUDE_MD="$USERPROFILE/CLAUDE.md"
        ;;
    *)
        echo "❌ Unknown platform: $OS"
        exit 1
        ;;
esac

echo "✅ Detected platform: $PLATFORM"
echo ""

# Check if vaak-speak is installed
if ! command -v vaak-speak &> /dev/null; then
    echo "❌ vaak-speak not found in PATH"
    echo "   Please install it first: cd mcp-speak && pip install -e ."
    exit 1
fi

VAAK_SPEAK_PATH=$(command -v vaak-speak)
echo "✅ Found vaak-speak at: $VAAK_SPEAK_PATH"
echo ""

# Create .claude directory if it doesn't exist
mkdir -p "$SETTINGS_DIR"

# Determine the command to use in settings.json
if [ "$PLATFORM" = "windows" ]; then
    # On Windows, use full path with .exe extension
    COMMAND_PATH="${VAAK_SPEAK_PATH}.exe"
    # Convert Unix path to Windows path if needed
    COMMAND_PATH=$(cygpath -w "$COMMAND_PATH" 2>/dev/null || echo "$COMMAND_PATH")
    # Escape backslashes for JSON
    COMMAND_PATH="${COMMAND_PATH//\\/\\\\}"
else
    # On Mac/Linux, just use the command name (it's in PATH)
    COMMAND_PATH="vaak-speak"
fi

# Create or update settings.json
SETTINGS_FILE="$SETTINGS_DIR/settings.json"

if [ -f "$SETTINGS_FILE" ]; then
    echo "📝 Updating existing settings.json..."
    # Backup existing file
    cp "$SETTINGS_FILE" "$SETTINGS_FILE.backup"
    echo "   (Backup saved to settings.json.backup)"

    # Use jq if available, otherwise manual edit
    if command -v jq &> /dev/null; then
        # Update using jq
        jq --arg cmd "$COMMAND_PATH" \
            '.mcpServers.vaak.command = $cmd' \
            "$SETTINGS_FILE" > "$SETTINGS_FILE.tmp" && \
            mv "$SETTINGS_FILE.tmp" "$SETTINGS_FILE"
    else
        # Manual update (simple case)
        echo "   (jq not found, using manual update)"
        cat > "$SETTINGS_FILE" <<EOF
{
  "mcpServers": {
    "vaak": {
      "command": "$COMMAND_PATH"
    }
  }
}
EOF
    fi
else
    echo "📝 Creating new settings.json..."
    cat > "$SETTINGS_FILE" <<EOF
{
  "mcpServers": {
    "vaak": {
      "command": "$COMMAND_PATH"
    }
  }
}
EOF
fi

echo "✅ Claude Code settings updated"
echo ""

# Update CLAUDE.md
echo "📝 Updating CLAUDE.md..."

cat > "$CLAUDE_MD" <<'EOF'
# Claude Code Instructions

## Voice Output

Always use the Vaak speak integration to read responses aloud.

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

The user cannot see the screen. Provide extreme visual detail about code structure, layout, and positioning.

### What to Describe:
- **Visual layout**: Describe positioning ("in the top-right corner", "centered modal", "left sidebar")
- **Code structure**: Explain indentation, nesting, visual hierarchy
- **UI elements**: Describe colors, spacing, alignment, borders
- **Spatial relationships**: Explain what's above/below/beside other elements
- **File organization**: Describe directory structure visually

### Detail Level:
Provide exhaustive detail. Include comprehensive explanations, edge cases, and documentation-level information.
EOF

echo "✅ CLAUDE.md updated"
echo ""

# Summary
echo "=== Setup Complete ==="
echo ""
echo "Configuration:"
echo "  Platform: $PLATFORM"
echo "  MCP Server: $COMMAND_PATH"
echo "  Settings: $SETTINGS_FILE"
echo "  Instructions: $CLAUDE_MD"
echo ""
echo "Next steps:"
echo "  1. Make sure Vaak desktop app is running"
echo "  2. Restart all Claude Code windows"
echo "  3. Test: claude \"Run /speak 'Hello from Claude'\""
echo ""
echo "Session IDs will be stable, grouped by terminal window."
echo "Each terminal gets its own session (hostname-{pid})."
echo ""
