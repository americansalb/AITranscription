/**
 * Currency display formatting — pure functions that turn raw currency.jsonl
 * rows into user-facing text + tier. Extracted from CollabTab.tsx so they
 * can be unit-tested in isolation and reused (e.g. by the flow feed and
 * the per-message economic footer).
 *
 * Two functions:
 *   - formatCurrencyLine: row → { text, tier } for a single ledger row
 *   - buildMsgEconomy: rows + optional dispute → economic-footer chip list
 */

import type { CurrencyFeedRow, CurrencyTier } from "./flowFeedBatcher";

export type { CurrencyFeedRow, CurrencyTier };

export interface DisputeRow {
  id?: string;
  challenger?: string;
  pool?: number;
  status?: string;
  resolution?: string;
}

export interface EconChip {
  text: string;
  tier: CurrencyTier;
}

// The 13-row transaction→display mapping (human msg 1872, verbatim).
// reason is prose so sub-types are matched by keyword (includes), not equality.
export function formatCurrencyLine(row: CurrencyFeedRow): { text: string; tier: CurrencyTier } {
  const seat = row.seat || "someone";
  const amt = typeof row.amount === "number" ? Math.abs(row.amount).toLocaleString() : "?";
  const reason = (row.reason || "").toLowerCase();
  const kind = (row.action_kind || "").toLowerCase();
  const ref = row.ref_msg != null ? `#${row.ref_msg}` : "";
  const has = (...keys: string[]) => keys.some((k) => reason.includes(k) || kind.includes(k));

  switch (row.type) {
    case "init":
      // Phase 7 (c) — Session-start carry-over banner. Backend writes a
      // single Init row with multi-line `reason` summarizing per-seat
      // carry-over. Renderer keeps the prose verbatim (CSS handles wrap).
      if (row.reason && row.reason.startsWith("Session started. Carry-over:"))
        return { text: row.reason, tier: "session-start" as CurrencyTier };
      return { text: `${seat} joined with 10,000 copper`, tier: "passive" };
    case "escrow_hold":
      return { text: `${seat} — ${amt} copper held in escrow${row.release_turn != null ? ` (turn ${row.release_turn})` : ""}`, tier: "hold" };
    case "escrow_release":
      return { text: `${seat} — ${amt} copper escrow released`, tier: "earn" };
    case "passive":
      return { text: `${seat} earned 1 copper passive`, tier: "passive" };
    case "interest":
      return { text: `${seat} earned ${amt} copper interest`, tier: "earn" };
    case "clawback":
      return { text: `${seat} — ${amt} copper seized for dispute`, tier: "dispute" };
    case "penalty":
      return { text: `${seat} penalized ${amt} copper (adversarial pass)`, tier: "loss" };
    case "reinstate":
      return { text: `${seat} reinstated — balance reset to 0`, tier: "earn" };
    case "bounty_stake":
      return { text: `${seat} staked ${amt} copper to claim a bounty`, tier: "bounty" };
    case "bounty_earn":
      return { text: `${seat} earned ${amt} copper completing a bounty`, tier: "bounty" };
    case "bounty_clawback":
      return { text: `${seat} — ${amt} copper clawed back on bounty objection`, tier: "dispute" };
    case "bounty_expire":
      return { text: `${amt} copper destroyed on expired/abandoned bounty`, tier: "destroyed" };
    case "credit":
      if (has("dispute_won", "dispute won", "won dispute"))
        return { text: `${seat} won dispute — ${amt} copper awarded`, tier: "dispute" };
      if (has("edit"))
        return { text: `${seat} earned ${amt} copper editing${ref ? ` (msg ${ref})` : ""}`, tier: "earn" };
      if (has("pass"))
        return { text: `${seat} earned 1 copper passing`, tier: "earn" };
      if (has("speak"))
        return { text: `${seat} earned ${amt} copper speaking${ref ? ` (msg ${ref})` : ""}`, tier: "earn" };
      return { text: `${seat} earned ${amt} copper${ref ? ` (msg ${ref})` : ""}`, tier: "earn" };
    case "debit":
      if (has("pool_destroyed", "pool destroyed"))
        return { text: `Pool destroyed — ${amt} copper to catnip`, tier: "destroyed" };
      if (has("objection"))
        return { text: `${seat} filed objection — 50 copper`, tier: "dispute" };
      return { text: `${seat} — ${amt} copper debited${row.reason ? ` (${row.reason})` : ""}`, tier: "loss" };
    default:
      return { text: `${seat} — ${amt} copper${row.reason ? ` (${row.reason})` : ""}`, tier: "passive" };
  }
}

// Change #1 (human msg 2262): per-message economic footer. The economic story
// belongs ON the message that caused it, not only in the sidebar feed.
export function buildMsgEconomy(
  rows: CurrencyFeedRow[] | undefined,
  dispute: DisputeRow | undefined,
): EconChip[] | null {
  const chips: EconChip[] = [];
  if (rows && rows.length) {
    const hold = rows.find((r) => r.type === "escrow_hold");
    const release = rows.find((r) => r.type === "escrow_release");
    const credit = rows.find((r) => r.type === "credit");
    const penalty = rows.find((r) => r.type === "penalty");
    const clawback = rows.find((r) => r.type === "clawback");

    const reason = `${hold?.reason ?? ""} ${credit?.reason ?? ""}`.toLowerCase();
    const actLabel =
      (hold?.action_kind || credit?.action_kind || "").toLowerCase() ||
      (reason.includes("edit") ? "edit"
        : reason.includes("test") ? "test"
        : reason.includes("pass") ? "pass"
        : reason.includes("speak") ? "speak"
        : "earn");

    const earnAmt =
      typeof credit?.amount === "number" ? Math.abs(credit.amount)
        : typeof hold?.amount === "number" ? Math.abs(hold.amount)
        : null;

    if (earnAmt != null) {
      if (release || credit) {
        chips.push({ text: `+${earnAmt}⊕ ${actLabel} · ✓ released`, tier: "earn" });
      } else if (hold) {
        const turnTxt = hold.release_turn != null ? ` · in escrow → turn ${hold.release_turn}` : " · in escrow";
        chips.push({ text: `+${earnAmt}⊕ ${actLabel}${turnTxt}`, tier: "hold" });
      } else {
        chips.push({ text: `+${earnAmt}⊕ ${actLabel}`, tier: "earn" });
      }
    }
    if (penalty && typeof penalty.amount === "number") {
      chips.push({ text: `−${Math.abs(penalty.amount)}⊕ penalty`, tier: "loss" });
    }
    if (clawback && typeof clawback.amount === "number") {
      chips.push({ text: `−${Math.abs(clawback.amount)}⊕ clawed back`, tier: "dispute" });
    }
  }
  if (dispute) {
    if (dispute.status === "open") {
      chips.push({
        text: `⊗ objected by ${dispute.challenger ?? "?"} · pool ${dispute.pool ?? 0}⊕`,
        tier: "dispute",
      });
    } else {
      chips.push({ text: `⊗ ${dispute.resolution || "resolved"}`, tier: "dispute" });
    }
  }
  return chips.length ? chips : null;
}
