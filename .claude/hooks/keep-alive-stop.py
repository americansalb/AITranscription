#!/usr/bin/env python3
"""Stop hook — keep Vaak team seats alive in the standby loop.

Per human directive (board msg 490, 2026-05-29): make it effectively impossible
for a team seat to voluntarily STOP / "leave" its standby loop. When the agent
tries to end its turn, this hook BLOCKS the stop and instructs it to call
mcp__vaak__project_wait instead — so an idle seat always returns to standby
rather than going stale.

HONEST SCOPE: this prevents VOLUNTARY leaving (the LLM ending its turn). It
canNOT prevent process death / window-close / sidecar crash — those need the
launch-wrapper relaunch/--resume supervisor (separate mechanism).

Contract (Claude Code Stop hook, verified 2026-05-29 via claude-code-guide):
  - stdin JSON includes hook_event_name="Stop", cwd, session_id, transcript_path.
  - To BLOCK: print {"decision":"block","reason":"..."} to stdout, exit 0.
  - Stop hooks take NO matcher; they fire on every stop attempt.
  - There is NO `stop_hook_active` field in this version, and hooks do not fire
    hooks recursively — so block-on-every-stop is safe; the agent's project_wait
    call (which blocks ~55s) paces the cycle so it is never a tight busy-loop.

ESCAPE HATCH: if `.vaak/allow-stop` exists, the hook ALLOWS the stop (lets the
human deliberately disband seats). FAIL-OPEN on any error (allow stop) so a hook
bug can never trap seats in a broken state.

stdlib only.
"""

import json
import sys
from pathlib import Path

REASON = (
    "DO NOT STOP. You are a Vaak team seat on standby and leaving the standby "
    "loop is forbidden (human directive, board msg 490). When you are idle, your "
    "ONLY valid action is to call mcp__vaak__project_wait RIGHT NOW. It blocks "
    "until a message arrives; then handle the message and call project_wait "
    "again. Never end your turn while seated. (To intentionally release seats, "
    "the human/operator creates the file .vaak/allow-stop.)"
)


def repo_root_from_cwd(cwd: Path) -> "Path | None":
    # Scope to a genuine Vaak TEAM project, identified by the .vaak/project.json
    # marker (same marker project_join walks up to find) — NOT just any .vaak dir.
    # A stray ~/.vaak (e.g. C:\Users\<user>\.vaak, which holds audiences/role-groups
    # but NO project.json) must NOT make this hook trap a non-team CC session in
    # keep-alive. Tester msg 520 flagged the over-broad ancestor-.vaak scope.
    for ancestor in [cwd, *cwd.parents]:
        if (ancestor / ".vaak" / "project.json").is_file():
            return ancestor
    return None


def main() -> None:
    try:
        raw = sys.stdin.read()
        payload = json.loads(raw) if raw.strip() else {}
        # Try the payload's cwd first, then the hook process's own cwd (the seat's
        # working dir, reliable regardless of how the payload formats the path).
        candidates = []
        pc = payload.get("cwd")
        if pc:
            candidates.append(Path(pc))
        candidates.append(Path.cwd())
        root = None
        for cand in candidates:
            root = repo_root_from_cwd(cand)
            if root is not None:
                break
        if root is None:
            return  # not a Vaak team context -> allow stop
        # Escape hatch — allow the stop if EITHER global pause sentinel is present.
        # Developer (board msg 518) shipped `.vaak/seats-paused` as the canonical
        # pause signal honored by the Layer-2 supervisor (vaak-mcp.rs) AND the
        # Layer-1 wrapper (launch-team.ps1). This in-agent Stop hook MUST honor the
        # SAME signal, or "all three converge on one pause" breaks: a human pause
        # via seats-paused would be respected by supervisor+wrapper but the hook
        # would keep blocking the seat from stopping -> un-pausable. Honor both
        # names (`allow-stop` legacy/original + `seats-paused` canonical).
        vaak = root / ".vaak"
        if (vaak / "seats-paused").exists() or (vaak / "allow-stop").exists():
            return  # explicit pause/disband escape hatch -> allow stop
        # Block the stop and steer the seat back into standby.
        print(json.dumps({"decision": "block", "reason": REASON}))
    except Exception:
        # Fail OPEN: a hook bug must never trap seats in a broken state.
        return


if __name__ == "__main__":
    main()
