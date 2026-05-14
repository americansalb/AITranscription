"""
Audit responds_to compliance across the board.

Proposal 2 (msg 717) verification: mandatory responds_to with a starting_new_thread
escape hatch. This script walks .vaak/board.jsonl and emits messages that would
have failed the proposed schema check (neither responds_to nor starting_new_thread
in metadata).

Run: python .vaak/scripts/audit_responds_to_compliance.py [section_slug]

Output:
  - Per-message lines for non-compliant messages (id, from, type, subject)
  - Per-role compliance percentages
  - Section totals
"""
import json
import sys
from collections import defaultdict
from pathlib import Path

EXEMPT_TYPES = {"mic_landed", "mic_released", "floor_halted_for_human"}
EXEMPT_FROM = {"system", "human", "human:0"}


def is_exempt(msg: dict) -> bool:
    if msg.get("type") in EXEMPT_TYPES:
        return True
    if msg.get("from") in EXEMPT_FROM:
        return True
    return False


def is_compliant(msg: dict) -> bool:
    md = msg.get("metadata") or {}
    if md.get("starting_new_thread") is True:
        return True
    rt = md.get("responds_to")
    if isinstance(rt, dict) and rt.get("speaker"):
        return True
    return False


def load_board(path: Path):
    for line_num, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        line = line.strip()
        if not line:
            continue
        try:
            yield json.loads(line)
        except json.JSONDecodeError as e:
            print(f"WARN line {line_num}: {e}", file=sys.stderr)


def main():
    repo = Path(__file__).resolve().parents[2]
    section = sys.argv[1] if len(sys.argv) > 1 else "5-12"

    # Board layout: .vaak/sections/<section>/board.jsonl, falling back to .vaak/board.jsonl
    section_board = repo / ".vaak" / "sections" / section / "board.jsonl"
    flat_board = repo / ".vaak" / "board.jsonl"
    board = section_board if section_board.exists() else flat_board
    if not board.exists():
        print(f"ERROR: no board found at {section_board} or {flat_board}", file=sys.stderr)
        sys.exit(1)

    per_role_total = defaultdict(int)
    per_role_compliant = defaultdict(int)
    non_compliant = []
    exempt_count = 0
    total = 0

    for msg in load_board(board):
        total += 1
        if is_exempt(msg):
            exempt_count += 1
            continue
        sender = msg.get("from", "unknown")
        per_role_total[sender] += 1
        if is_compliant(msg):
            per_role_compliant[sender] += 1
        else:
            non_compliant.append(msg)

    print(f"Board: {board}")
    print(f"Total messages: {total}")
    print(f"Exempt (system/human/floor-events): {exempt_count}")
    print(f"In-scope (would be schema-checked): {total - exempt_count}")
    print()
    print("Per-role compliance:")
    for role in sorted(per_role_total):
        t = per_role_total[role]
        c = per_role_compliant[role]
        pct = (c / t * 100) if t else 0
        print(f"  {role:30s} {c:4d}/{t:4d}  {pct:5.1f}%")
    print()
    print(f"Non-compliant message count: {len(non_compliant)}")
    print(f"(showing first 20)")
    for msg in non_compliant[:20]:
        subj = (msg.get("subject") or "")[:60]
        print(f"  id={msg.get('id'):>4}  {msg.get('from'):24s}  {msg.get('type'):12s}  {subj}")


if __name__ == "__main__":
    main()
