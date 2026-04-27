#!/usr/bin/env bash
# Claude Code PreToolUse hook. Reads .vaak/<section>/assembly.json and blocks
# Edit/Write/NotebookEdit/Bash when build_mode == false. Install by adding to
# ~/.claude/settings.json:
#
#   "hooks": {
#     "PreToolUse": [
#       { "matcher": ".*", "hooks": [{ "type": "command", "command": "bash /abs/path/to/build-mode-gate.sh" }] }
#     ]
#   }
#
# The MCP layer cannot gate Edit/Write/Bash directly because those are CC
# native tools, not MCP tools. The hook runs inside CC before each tool call
# and exits non-zero (with stderr) to block. Reads assembly.json on every
# invocation — clients can't cache the build_mode value.

set -u

event=$(cat)
tool_name=$(printf '%s' "$event" \
    | tr '\n' ' ' \
    | sed -n 's/.*"tool_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p')

case "$tool_name" in
    Edit|Write|NotebookEdit|Bash) ;;
    *) exit 0 ;;
esac

dir="$PWD"
while [ -n "$dir" ] && [ "$dir" != "/" ]; do
    if [ -f "$dir/.vaak/project.json" ]; then break; fi
    dir=$(dirname "$dir")
done
if [ ! -f "$dir/.vaak/project.json" ]; then exit 0; fi

section=$(sed -n 's/.*"active_section"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$dir/.vaak/project.json" | head -n1)
section=${section:-default}

if [ "$section" = "default" ]; then
    asm="$dir/.vaak/assembly.json"
else
    asm="$dir/.vaak/sections/$section/assembly.json"
fi

if [ ! -f "$asm" ]; then exit 0; fi

build_mode=$(sed -n 's/.*"build_mode"[[:space:]]*:[[:space:]]*\(true\|false\).*/\1/p' "$asm" | head -n1)

if [ "$build_mode" = "false" ]; then
    cat <<'EOF' >&2
Build Mode is OFF for this assembly.
Code-mutating tools (Edit, Write, NotebookEdit, Bash) are blocked.
Toggle Build Mode on from the Assembly Settings to allow code changes.
EOF
    exit 2
fi

exit 0
