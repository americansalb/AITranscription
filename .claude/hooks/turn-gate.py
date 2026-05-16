#!/usr/bin/env python3
"""Commit G — Read-gate PreToolUse hook per strict-turn-discipline spec
§Read-embargo (lines 40-54).

Gates Read/Edit/Write/Grep/Glob/WebFetch/WebSearch tool calls based on
`floor.review_intensity` discipline level + caller's seat:

  Level 1-5: PASS (no gate; current behavior preserved)
  Level 6-7: AUDIT — emit read_off_turn board event, tool succeeds
  Level 8:   SOFT BLOCK — deny UNLESS tool input has --peek-acknowledged
  Level 9-10: HARD BLOCK — unconditional deny

Exempt seats (all levels):
  - seat == "human"
  - seat == floor.moderator
  - seat == floor.current_speaker

Reads `.vaak/sections/<active-section>/protocol.json` (or `.vaak/protocol.json`
for default section). Derives seat via `payload.session_id` (Claude Code
hook contract) against sessions.json bindings.

Output schema (Claude Code PreToolUse contract): emit JSON to stdout to
deny via {"decision": "block", "reason": "..."}. Exit 0 always — the
JSON output is the decision, not the exit code.

stdlib only.
"""

import json
import sys
from pathlib import Path


def repo_root_from_cwd() -> Path | None:
    cwd = Path.cwd()
    for ancestor in [cwd, *cwd.parents]:
        if (ancestor / ".vaak").is_dir():
            return ancestor
    return None


def active_section(vaak_dir: Path) -> str:
    try:
        proj = json.loads((vaak_dir / "project.json").read_text(encoding="utf-8"))
        return proj.get("active_section") or "default"
    except (OSError, json.JSONDecodeError):
        return "default"


def protocol_path(vaak_dir: Path, section: str) -> Path:
    if section == "default":
        return vaak_dir / "protocol.json"
    return vaak_dir / "sections" / section / "protocol.json"


def read_protocol(vaak_dir: Path) -> dict | None:
    section = active_section(vaak_dir)
    try:
        return json.loads(protocol_path(vaak_dir, section).read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return None


def seat_for_session(vaak_dir: Path, session_id: str) -> str | None:
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


def emit_read_off_turn_audit(
    vaak_dir: Path, seat: str, tool: str, file_path: str | None, level: int, override: bool
) -> None:
    """Append a read_off_turn event to the active section's board.jsonl
    for audit. Best-effort; failure does not affect the gate decision."""
    section = active_section(vaak_dir)
    if section == "default":
        board_path = vaak_dir / "board.jsonl"
    else:
        board_path = vaak_dir / "sections" / section / "board.jsonl"
    if not board_path.parent.exists():
        return
    from datetime import datetime, timezone
    msg = {
        "from": "system",
        "to": "all",
        "type": "read_off_turn",
        "timestamp": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "subject": f"[read_off_turn] {seat} {tool}",
        "body": f"Off-turn tool call ({tool}) by {seat} at review_intensity={level}{' (override)' if override else ''}",
        "metadata": {
            "seat": seat,
            "tool": tool,
            "file_path": file_path or "",
            "review_intensity": level,
            "override": override,
        },
    }
    try:
        with board_path.open("a", encoding="utf-8") as f:
            f.write(json.dumps(msg) + "\n")
    except OSError:
        pass


def deny(reason: str) -> None:
    """Emit Claude Code's deny decision + exit 0 so the JSON output is
    consumed, not treated as error."""
    print(json.dumps({"decision": "block", "reason": reason}))
    sys.exit(0)


def main() -> None:
    try:
        raw = sys.stdin.read()
        if not raw.strip():
            sys.exit(0)
        payload = json.loads(raw)
    except (OSError, json.JSONDecodeError):
        sys.exit(0)

    tool = payload.get("tool_name", "")
    if tool not in ("Read", "Edit", "Write", "Grep", "Glob", "WebFetch", "WebSearch", "NotebookEdit"):
        sys.exit(0)

    vaak_dir_root = repo_root_from_cwd()
    if vaak_dir_root is None:
        sys.exit(0)
    vaak_dir = vaak_dir_root / ".vaak"

    proto = read_protocol(vaak_dir)
    if proto is None:
        sys.exit(0)
    floor = proto.get("floor") or {}
    level = int(floor.get("review_intensity") or 5)

    # Level 1-5: pass through, no gate.
    if level < 6:
        sys.exit(0)

    # Determine caller seat. Claude Code's hook contract passes session_id in
    # the stdin payload (verified: env grep shows CLAUDE_CODE_SESSION_ID is set
    # but no CLAUDE_SESSION_ID; payload.session_id is the canonical source per
    # https://docs.claude.com/en/docs/claude-code/hooks). Tester msg 2503 +
    # architect msg 2511 verified the prior env var path was inert.
    session_id = payload.get("session_id", "")
    if not session_id:
        sys.exit(0)
    seat = seat_for_session(vaak_dir, session_id)
    if seat is None:
        sys.exit(0)

    # Exempt: human / moderator / current_speaker.
    current_speaker = floor.get("current_speaker")
    moderator = floor.get("moderator")
    if seat == "human" or seat == "human:0":
        sys.exit(0)
    if moderator and seat == moderator:
        sys.exit(0)
    if current_speaker and seat == current_speaker:
        sys.exit(0)

    file_path = (
        payload.get("tool_input", {}).get("file_path")
        or payload.get("tool_input", {}).get("path")
    )

    # Level 6-7: audit-only — emit event + pass.
    if level <= 7:
        emit_read_off_turn_audit(vaak_dir, seat, tool, file_path, level, override=False)
        sys.exit(0)

    # Level 8: soft block with --peek-acknowledged override.
    if level == 8:
        # Check for override marker in tool input.
        tool_input = payload.get("tool_input", {})
        # Override can come via _peek_acknowledged metadata flag OR
        # a `--peek-acknowledged` suffix in a path/query arg (best-effort).
        override = bool(tool_input.get("_peek_acknowledged"))
        if not override:
            for v in tool_input.values():
                if isinstance(v, str) and "--peek-acknowledged" in v:
                    override = True
                    break
        emit_read_off_turn_audit(vaak_dir, seat, tool, file_path, level, override=override)
        if override:
            sys.exit(0)
        deny(
            f"[read_off_turn] {seat} cannot run {tool} while current_speaker={current_speaker} "
            f"at review_intensity={level}. Add _peek_acknowledged: true to tool_input or wait for your turn."
        )

    # Level 9-10: hard block, no override.
    emit_read_off_turn_audit(vaak_dir, seat, tool, file_path, level, override=False)
    deny(
        f"[read_off_turn] {seat} cannot run {tool} while current_speaker={current_speaker} "
        f"at review_intensity={level}. Strict turn discipline — wait for your turn."
    )


if __name__ == "__main__":
    main()
