"""
Audit turn duration across the board.

Proposal 1 (msg 717) verification: typed turn declaration (turn_type +
expected_duration_secs). This script walks .vaak/board.jsonl and computes
actual mic_held_secs by pairing mic_landed and mic_released events per speaker.

When proposal 1 lands and writers start emitting `expected_duration_secs` in
mic_landed metadata, this script will also compare declared vs actual and
surface dodge-vector signatures (declared 900s, held 30s, three turns running).

Run: python .vaak/scripts/audit_turn_duration.py [section_slug]

Output:
  - Per-speaker turn-duration distribution (count, min, mean, max)
  - When proposal-1 fields are present: declared vs actual ratio
  - Dodge-vector signatures: under-utilizers (>3 turns at <20% of declared)
"""
import json
import sys
from collections import defaultdict
from datetime import datetime
from pathlib import Path
from statistics import mean, median

EXEMPT_FROM = {"system", "human", "human:0"}


def parse_ts(s: str) -> datetime:
    return datetime.fromisoformat(s.replace("Z", "+00:00"))


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

    landed_by_speaker: dict[str, dict] = {}
    durations: dict[str, list[float]] = defaultdict(list)
    declared_actual: dict[str, list[tuple[float, float]]] = defaultdict(list)

    for msg in load_board(board):
        mtype = msg.get("type")
        if mtype == "mic_landed":
            target = msg.get("to") or msg.get("metadata", {}).get("speaker")
            if not target or target in EXEMPT_FROM:
                continue
            landed_by_speaker[target] = {
                "ts": parse_ts(msg["timestamp"]),
                "expected": (msg.get("metadata") or {}).get("expected_duration_secs"),
            }
        elif mtype == "mic_released":
            sender = (msg.get("metadata") or {}).get("from_speaker") or msg.get("from")
            landed = landed_by_speaker.pop(sender, None)
            if not landed:
                continue
            released_ts = parse_ts(msg["timestamp"])
            secs = (released_ts - landed["ts"]).total_seconds()
            durations[sender].append(secs)
            if landed["expected"] is not None:
                declared_actual[sender].append((landed["expected"], secs))

    print(f"Board: {board}")
    print()
    print(f"{'Speaker':30s} {'N':>5s} {'min':>7s} {'mean':>7s} {'med':>7s} {'max':>7s}")
    print("-" * 70)
    for speaker in sorted(durations):
        d = durations[speaker]
        if not d:
            continue
        print(f"{speaker:30s} {len(d):>5d} {min(d):>6.0f}s {mean(d):>6.0f}s {median(d):>6.0f}s {max(d):>6.0f}s")

    if any(declared_actual.values()):
        print()
        print("Declared vs actual (proposal 1 fields present):")
        for speaker, pairs in declared_actual.items():
            ratios = [actual / declared for declared, actual in pairs if declared > 0]
            print(f"  {speaker:30s} avg_ratio={mean(ratios):.2f} n={len(ratios)}")
            under = [r for r in ratios if r < 0.2]
            if len(under) >= 3:
                print(f"    DODGE-VECTOR SIGNATURE: {len(under)} turns at <20% of declared")
    else:
        print()
        print("(Proposal 1 fields not yet present — declared/actual ratio unavailable.)")


if __name__ == "__main__":
    main()
