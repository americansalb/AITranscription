#!/usr/bin/env python3
"""Commit C — Auto-claim PostToolUse hook per strict-turn-discipline spec
§Auto-claim from file operations.

Reads Claude Code's PostToolUse tool input + result JSON from stdin,
extracts file_path on successful Read/Edit/Write/NotebookEdit, and
upserts a claim entry into .vaak/claims.json for the calling seat.

Action-reaction binding (human msg 2209): claim STATE is derived from
observed file ops, not from manual project_claim calls.

claims.json shape (matches collab.rs::read_claims_filtered):
{
  "role:instance": {
    "session_id": "...",
    "files": ["path1", "path2"],
    "description": "auto-claim",
    "claimed_at": "ISO-8601"
  },
  ...
}

stdlib only. Cross-OS uniform via shebang on Linux/macOS + .cmd shim
on Windows. Exit 0 always — hook failure shouldn't block tool calls.
"""

import json
import sys
from datetime import datetime, timezone
from pathlib import Path


MAX_FILES_PER_CLAIM = 20


def repo_root_from_cwd() -> Path | None:
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


def upsert_claim(vaak_dir: Path, seat: str, session_id: str, file_path: str) -> None:
    """Upsert claim entry for seat. Adds file_path to the files list and
    bumps claimed_at. Caps files at MAX_FILES_PER_CLAIM (LRU)."""
    claims_path = vaak_dir / "claims.json"
    now_iso = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
    try:
        data = json.loads(claims_path.read_text(encoding="utf-8"))
        if not isinstance(data, dict):
            data = {}
    except (OSError, json.JSONDecodeError):
        data = {}

    entry = data.get(seat) or {}
    files: list = entry.get("files") if isinstance(entry.get("files"), list) else []
    # Move-to-front: remove existing instance + append fresh at end.
    files = [f for f in files if f != file_path]
    files.append(file_path)
    if len(files) > MAX_FILES_PER_CLAIM:
        files = files[-MAX_FILES_PER_CLAIM:]

    data[seat] = {
        "session_id": session_id,
        "files": files,
        "description": "auto-claim (PostToolUse)",
        "claimed_at": now_iso,
    }

    try:
        claims_path.write_text(json.dumps(data, indent=2), encoding="utf-8")
    except OSError:
        pass


def main() -> None:
    try:
        raw = sys.stdin.read()
        if not raw.strip():
            sys.exit(0)
        payload = json.loads(raw)
    except (OSError, json.JSONDecodeError):
        sys.exit(0)

    tool = payload.get("tool_name", "")
    if tool not in ("Read", "Edit", "Write", "NotebookEdit"):
        sys.exit(0)

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

    # Claude Code's hook contract passes session_id in the stdin payload
    # (verified: CLAUDE_CODE_SESSION_ID is the actual env var name; payload.
    # session_id is the canonical source). Tester msg 2503 + architect msg 2511
    # verified the prior env var path was inert.
    session_id = payload.get("session_id", "")
    if not session_id:
        sys.exit(0)

    seat = seat_for_session(vaak_dir, session_id)
    if seat is None:
        sys.exit(0)

    upsert_claim(vaak_dir, seat, session_id, file_path)
    sys.exit(0)


if __name__ == "__main__":
    main()
