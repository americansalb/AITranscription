@echo off
:: Keep-alive MARKER hook shim (Layer-2 busy-aware FRESH-relaunch gate inputs).
:: Forwards the hook-event JSON (stdin) to the vaak-mcp sidecar's --keep-alive
:: mode (run_keep_alive), which stamps the seat file (.vaak/sessions/<seat>.json):
::   UserPromptSubmit      -> turn_active_started_at_ms  (turn-active marker SET)
::   PreToolUse            -> in_flight_{tool,started_at} (live-op marker SET)
::   PostToolUse (success) -> clear in_flight + last_successful_work_at_ms (clause-2)
::   PostToolUseFailure    -> clear in_flight (no work-success)
:: project_wait success (server-side) clears turn_active + stamps clause-1.
::
:: Binary path mirrors ~/.claude.json's vaak MCP server command, resolved relative
:: to this hook dir so it survives a repo move. Exit 0 ALWAYS — the stamp is a
:: best-effort heartbeat; a hook error must never block the tool or end the turn.
"%~dp0..\..\desktop\src-tauri\binaries\vaak-mcp-x86_64-pc-windows-msvc.exe" --keep-alive
exit /b 0
