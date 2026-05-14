"""
Audit pass-rate across the board.

Proposal 3 (msg 717) verification: passing-by-default culture. Without
instrumentation, the briefing edit ships but adoption is invisible. This script
walks .vaak/board.jsonl and emits per-role pass-rate metrics.

Run: python .vaak/scripts/audit_pass_rate.py [section_slug]

Output:
  - Per-role pass-rate (passes / total substantive turns)
  - Pass classification breakdown by signal
"""
import json
import re
import sys
from collections import defaultdict
from pathlib import Path

EXEMPT_TYPES = {"mic_landed", "mic_released", "floor_halted_for_human"}
EXEMPT_FROM = {"system", "human", "human:0"}

PASS_PATTERNS = [
    re.compile(r"\bpass(ing)?\b", re.I),
    re.compile(r"\bstanding by\b", re.I),
    re.compile(r"\byielding\b", re.I),
    re.compile(r"\back(nowledged)?\b", re.I),
    re.compile(r"\bnoted\b", re.I),
    re.compile(r"^(re|ack):", re.I),
]


def classify(msg: dict) -> tuple[str, str | None]:
    """Return (category, signal) where category is one of:
    pass | ack | substantive
    """
    body = (msg.get("body") or "").strip()
    md = msg.get("metadata") or {}
    activity = md.get("activity") or ""

    if not body or len(body) < 30:
        return "pass", "empty_or_tiny"

    if activity == "standby" and len(body) < 200:
        return "pass", "standby_short"

    first_line = body.splitlines()[0][:200]
    for pat in PASS_PATTERNS:
        if pat.search(first_line):
            if len(body) < 250:
                return "ack", f"pattern:{pat.pattern}"

    return "substantive", None


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

    section_board = repo / ".vaak" / "sections" / section / "board.jsonl"
    flat_board = repo / ".vaak" / "board.jsonl"
    board = section_board if section_board.exists() else flat_board
    if not board.exists():
        print(f"ERROR: no board found at {section_board} or {flat_board}", file=sys.stderr)
        sys.exit(1)

    per_role = defaultdict(lambda: {"pass": 0, "ack": 0, "substantive": 0})
    signal_counts = defaultdict(int)

    for msg in load_board(board):
        if msg.get("type") in EXEMPT_TYPES:
            continue
        if msg.get("from") in EXEMPT_FROM:
            continue
        cat, signal = classify(msg)
        per_role[msg.get("from", "unknown")][cat] += 1
        if signal:
            signal_counts[signal] += 1

    print(f"Board: {board}")
    print()
    print(f"{'Role':30s} {'Pass':>6s} {'Ack':>6s} {'Subst':>6s} {'Total':>6s} {'Pass%':>6s}")
    print("-" * 70)
    grand_pass = grand_ack = grand_subst = 0
    for role in sorted(per_role):
        d = per_role[role]
        total = d["pass"] + d["ack"] + d["substantive"]
        pct = (d["pass"] / total * 100) if total else 0
        print(f"{role:30s} {d['pass']:>6d} {d['ack']:>6d} {d['substantive']:>6d} {total:>6d} {pct:>5.1f}%")
        grand_pass += d["pass"]
        grand_ack += d["ack"]
        grand_subst += d["substantive"]
    grand_total = grand_pass + grand_ack + grand_subst
    grand_pct = (grand_pass / grand_total * 100) if grand_total else 0
    print("-" * 70)
    print(f"{'TOTAL':30s} {grand_pass:>6d} {grand_ack:>6d} {grand_subst:>6d} {grand_total:>6d} {grand_pct:>5.1f}%")
    print()
    print("Signal counts (which heuristic fired for pass/ack):")
    for sig in sorted(signal_counts, key=signal_counts.get, reverse=True):
        print(f"  {signal_counts[sig]:5d}  {sig}")


if __name__ == "__main__":
    main()
