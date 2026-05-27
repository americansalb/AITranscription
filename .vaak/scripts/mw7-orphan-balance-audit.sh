#!/usr/bin/env bash
# VAAK_FP:SHA-13.3:mw7-orphan-balance-audit.sh
#
# MW7 orphan-balance audit per multi-writer audit contract.md v6 +
# tester:0 msg 1784 empirical finding (4 orphan balances 20200cu = 64% M0).
#
# Reports seats that exist in .vaak/balances.json but have NO active
# binding in .vaak/sessions.json. These are "orphan" balances — currency
# held by seats no agent is actively running. Likely candidates for
# reclamation under the economy-redesign Tier 1 D5.1 inactivity policy.
#
# READ-ONLY. Does not modify any state. Prints to stdout. Exit 0 always.
#
# Usage:
#   bash .vaak/scripts/mw7-orphan-balance-audit.sh
#
# Output format:
#   - List of orphan seats with balance + escrow
#   - Total orphan balance
#   - Total M0 (all balances)
#   - Orphan percentage of M0

set -u

VAAK_DIR="$(cd "$(dirname "$0")/.." && pwd)"
BAL="$VAAK_DIR/balances.json"
SES="$VAAK_DIR/sessions.json"

if [ ! -f "$BAL" ]; then
    echo "ERR: $BAL not found"
    exit 0
fi
if [ ! -f "$SES" ]; then
    echo "ERR: $SES not found"
    exit 0
fi

python - "$BAL" "$SES" <<'PY'
import json, sys

bal_path, ses_path = sys.argv[1], sys.argv[2]

with open(bal_path) as f: bal = json.load(f)
with open(ses_path) as f: ses = json.load(f)

seats = bal.get("seats", {})
active = {
    f"{b['role']}:{b['instance']}"
    for b in ses.get("bindings", [])
    if b.get("status") == "active"
}

orphans = []
total_m0 = 0
for seat, data in seats.items():
    bal_cu = data.get("balance", 0)
    esc_cu = data.get("escrow_held", 0)
    total_m0 += bal_cu + esc_cu
    if seat not in active:
        orphans.append((seat, bal_cu, esc_cu))

print(f"=== MW7 Orphan-Balance Audit ===")
print(f"Total seats in balances.json: {len(seats)}")
print(f"Active bindings in sessions.json: {len(active)}")
print(f"Orphans (in balances, NOT active): {len(orphans)}")
print()

if not orphans:
    print("OK — no orphan balances.")
else:
    print("Orphan seats:")
    orphan_total = 0
    for seat, bal_cu, esc_cu in sorted(orphans, key=lambda x: -(x[1] + x[2])):
        line_total = bal_cu + esc_cu
        orphan_total += line_total
        print(f"  {seat:<28} balance={bal_cu:>6}cu  escrow={esc_cu:>4}cu  total={line_total:>6}cu")
    print()
    print(f"Orphan total:  {orphan_total} cu")
    print(f"M0 total:      {total_m0} cu")
    pct = (orphan_total * 100.0 / total_m0) if total_m0 else 0
    print(f"Orphan share:  {pct:.1f}% of M0")
    print()
    print("Recommendation: economy-redesign Tier 1 D5.1 inactivity")
    print("reclamation would route these to system:treasury (70%) +")
    print("active floor agents (30% Robin-Hood split per architect msg 1588).")
PY
