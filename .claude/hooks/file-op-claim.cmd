@echo off
:: Commit C — Windows .cmd shim for the PostToolUse auto-claim hook.
:: Reads tool input/result JSON from stdin + forwards to the Python
:: implementation. Same exit-0-always policy.
python "%~dp0file-op-claim.py"
exit /b 0
