#!/bin/sh
PROJECT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROLES="${@:-manager architect developer developer tester tester}"

for role in $ROLES; do
    if command -v gnome-terminal >/dev/null 2>&1; then
        gnome-terminal -- bash -c "cd '$PROJECT_DIR' && claude --dangerously-skip-permissions 'Join this project as a $role using the mcp vaak project_join tool with role $role. Then call project_wait in a loop to stay available for messages.'; exec bash"
    elif command -v open >/dev/null 2>&1; then
        # macOS
        SCRIPT=$(mktemp /tmp/vaak-launch-$role-XXXXXX.sh)
        cat > "$SCRIPT" << INNER
#!/bin/sh
cd '$PROJECT_DIR'
claude --dangerously-skip-permissions 'Join this project as a $role using the mcp vaak project_join tool with role $role. Then call project_wait in a loop to stay available for messages.'
INNER
        chmod +x "$SCRIPT"
        open -a Terminal "$SCRIPT"
    else
        # Fallback: xterm
        xterm -e bash -c "cd '$PROJECT_DIR' && claude --dangerously-skip-permissions 'Join this project as a $role using the mcp vaak project_join tool with role $role. Then call project_wait in a loop to stay available for messages.'; exec bash" &
    fi
    sleep 2
done

echo "Launched team members: $ROLES"
