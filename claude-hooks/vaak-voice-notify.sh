#!/bin/bash
# Claude Code PostToolUse hook that sends code changes to Vaak for voice explanation.
#
# When Claude Code writes or edits a file, this hook sends the event to
# Vaak's backend, which generates a spoken explanation using Claude Haiku
# and Groq TTS.
#
# Installation:
#   1. Copy this file to ~/.claude/hooks/
#   2. Make executable: chmod +x ~/.claude/hooks/vaak-voice-notify.sh
#   3. Add hook configuration to ~/.claude/settings.json (see SETUP.md)
#
# Requirements:
#   - curl
#   - jq
#   - Vaak backend running on localhost:8000

# Configuration
VAAK_API_URL="${VAAK_API_URL:-http://localhost:8000}"
VAAK_ENDPOINT="/api/v1/claude-event"
TIMEOUT_SECONDS=5

# Read JSON payload from stdin
INPUT=$(cat)

# Extract fields using jq
TOOL_NAME=$(echo "$INPUT" | jq -r '.tool_name // empty')
HOOK_EVENT=$(echo "$INPUT" | jq -r '.hook_event_name // empty')

# Only process Write/Edit operations from PostToolUse events
if [[ "$HOOK_EVENT" == "PostToolUse" && ("$TOOL_NAME" == "Write" || "$TOOL_NAME" == "Edit") ]]; then
    # Send to Vaak backend asynchronously (don't block Claude Code)
    curl -s -X POST "${VAAK_API_URL}${VAAK_ENDPOINT}" \
        -H "Content-Type: application/json" \
        -d "$INPUT" \
        --max-time "$TIMEOUT_SECONDS" \
        > /dev/null 2>&1 &
fi

# CRITICAL: Always exit 0 so we don't block Claude Code
# Voice explanation is an enhancement, not a requirement
exit 0
