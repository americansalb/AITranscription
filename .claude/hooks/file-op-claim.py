#!/usr/bin/env python3
"""Commit C — Auto-claim PostToolUse hook per strict-turn-discipline spec
§Auto-claim from file operations.

Reads Claude Code's PostToolUse tool input + result JSON from stdin,
extracts file_path on successful Read/Edit/Write/NotebookEdit, and
appends a claim entry to .vaak/claims.json for the calling seat.

Action-reaction binding (human msg 2209): claim STATE is derived from
observed file ops, not from manual project_claim calls. Stale claims
expire 300s after last update.

Seat resolution: walks up from cwd to find .vaak/sessions.json, looks
up the calling Claude Code session_id (CLAUDE_SESSION_ID env), maps to
seat slug "role:instance" via the bindings list.

stdlib only — works on every dev machine that has the Vaak backend
(Python ≥3.x). Cross-OS uniform via shebang on Linux/macOS + .cmd
shim on Windows (.claude/hooks/file-op-claim.cmd).

Exit 0 always — hook failure shouldn't block the tool call.
"""

import json
import os
import sys
import time
from pathlib import Path


CLAIM_TTL_SECS = 300


def repo_root_from_cwd() -> Path | None:
    """Walk up from cwd looking for .vaak directory."""
    cwd = Path.cwd()
    for ancestor in [cwd, *cwd.parents]:
        if (ancestor / ".vaak").is_dir():
            return ancestor
    return None


def seat_for_session(vaak_dir: Path, session_id: str) -> str | None:
    """Look up role:instance for a given Claude Code session_id."""
    try:
        sessions = json.loads((vaak_dir / "sessions.json").read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return None
    for b in sessions.get("bindings", []) or []:
        if b.get("session_id") == session_id and b.get("status") == "active":
            role = b.get("role")
            inst = b.get("instance")
            if role is not None and inst is not None:
                return f"{role}:{inst}"
    return None


def append_claim(vaak_dir: Path, seat: str, file_path: str) -> None:
    """Write a claim entry to .vaak/claims.json. Prunes entries older than
    CLAIM_TTL_SECS on every write so stale claims don't accumulate."""
    claims_path = vaak_dir / "claims.json"
    now = int(time.time())
    try:
        data = json.loads(claims_path.read_text(encoding="utf-8"))
        if not isinstance(data, dict):
            data = {}
    except (OSError, json.JSONDecodeError):
        data = {}

    entries: list = data.get("entries", [])
    # Prune stale + same-seat-same-file (we'll re-add fresh below).
    entries = [
        e for e in entries
        if isinstance(e, dict)
        and e.get("ts", 0) > now - CLAIM_TTL_SECS
        and not (e.get("seat") == seat and e.get("file_path") == file_path)
    ]
    entries.append({"seat": seat, "file_path": file_path, "ts": now})
    data["entries"] = entries
    data["updated_at"] = now

    try:
        claims_path.write_text(json.dumps(data, indent=2), encoding="utf-8")
    except OSError:
        pass


def main() -> None:
    # Hook gets tool input + result via stdin as JSON. Per Claude Code's
    # PostToolUse spec, the payload has tool_name + tool_input + tool_response.
    try:
        raw = sys.stdin.read()
        if not raw.strip():
            sys.exit(0)
        payload = json.loads(raw)
    except (OSError, json.JSONDecodeError):
        sys.exit(0)

    # Only auto-claim on successful file-op tools.
    tool = payload.get("tool_name", "")
    if tool not in ("Read", "Edit", "Write", "NotebookEdit"):
        sys.exit(0)

    # Check tool success (envelope varies; be defensive).
    response = payload.get("tool_response") or payload.get("tool_result") or {}
    is_error = (
        response.get("is_error") is True
        or response.get("isError") is True
        or response.get("status") == "error"
    )
    if is_error:
        sys.exit(0)

    file_path = (
        payload.get("tool_input", {}).get("file_path")
        or payload.get("tool_input", {}).get("path")
    )
    if not file_path:
        sys.exit(0)

    vaak_dir_root = repo_root_from_cwd()
    if vaak_dir_root is None:
        sys.exit(0)
    vaak_dir = vaak_dir_root / ".vaak"

    session_id = os.environ.get("CLAUDE_SESSION_ID", "")
    if not session_id:
        sys.exit(0)

    seat = seat_for_session(vaak_dir, session_id)
    if seat is None:
        sys.exit(0)

    append_claim(vaak_dir, seat, file_path)
    sys.exit(0)


if __name__ == "__main__":
    main()
