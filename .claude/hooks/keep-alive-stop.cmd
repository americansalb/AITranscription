@echo off
:: Stop hook shim — keep Vaak team seats in the standby loop (human msg 490).
:: Forwards the Stop-event JSON (stdin) to the Python implementation, which
:: blocks the stop (decision:block) unless .vaak/allow-stop exists. Exit 0 always
:: — the JSON on stdout is the decision, not the exit code.
python "%~dp0keep-alive-stop.py"
exit /b 0
