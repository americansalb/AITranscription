/**
 * Tests for currencyDisplay — formatCurrencyLine + buildMsgEconomy.
 *
 * Both are pure functions extracted from CollabTab.tsx. Tests pin the
 * 13-row transaction→display mapping (formatCurrencyLine) and the
 * per-message economic footer composition (buildMsgEconomy) against
 * regressions.
 */
import { describe, it, expect } from "vitest";

import {
  formatCurrencyLine,
  buildMsgEconomy,
} from "../lib/currencyDisplay";

describe("formatCurrencyLine — by row type", () => {
  it("init: session-start carry-over preserves prose verbatim", () => {
    const row = { type: "init", reason: "Session started. Carry-over:\narchitect:0 +500" };
    expect(formatCurrencyLine(row)).toEqual({
      text: "Session started. Carry-over:\narchitect:0 +500",
      tier: "session-start",
    });
  });

  it("init: non-carry-over uses default join text", () => {
    expect(formatCurrencyLine({ type: "init", seat: "architect:0" })).toEqual({
      text: "architect:0 joined with 10,000 copper",
      tier: "passive",
    });
  });

  it("escrow_hold: includes release_turn when present", () => {
    expect(formatCurrencyLine({ type: "escrow_hold", seat: "a:0", amount: 10, release_turn: 5 }))
      .toEqual({ text: "a:0 — 10 copper held in escrow (turn 5)", tier: "hold" });
  });

  it("escrow_release: format", () => {
    expect(formatCurrencyLine({ type: "escrow_release", seat: "a:0", amount: 10 }))
      .toEqual({ text: "a:0 — 10 copper escrow released", tier: "earn" });
  });

  it("passive: format", () => {
    expect(formatCurrencyLine({ type: "passive", seat: "a:0" }))
      .toEqual({ text: "a:0 earned 1 copper passive", tier: "passive" });
  });

  it("interest: format with amount", () => {
    expect(formatCurrencyLine({ type: "interest", seat: "a:0", amount: 5 }))
      .toEqual({ text: "a:0 earned 5 copper interest", tier: "earn" });
  });

  it("clawback: format", () => {
    expect(formatCurrencyLine({ type: "clawback", seat: "a:0", amount: 50 }))
      .toEqual({ text: "a:0 — 50 copper seized for dispute", tier: "dispute" });
  });

  it("penalty: format", () => {
    expect(formatCurrencyLine({ type: "penalty", seat: "a:0", amount: 25 }))
      .toEqual({ text: "a:0 penalized 25 copper (adversarial pass)", tier: "loss" });
  });

  it("reinstate: format", () => {
    expect(formatCurrencyLine({ type: "reinstate", seat: "a:0" }))
      .toEqual({ text: "a:0 reinstated — balance reset to 0", tier: "earn" });
  });

  it("bounty_stake/earn/clawback/expire: formats", () => {
    expect(formatCurrencyLine({ type: "bounty_stake", seat: "a:0", amount: 100 }))
      .toEqual({ text: "a:0 staked 100 copper to claim a bounty", tier: "bounty" });
    expect(formatCurrencyLine({ type: "bounty_earn", seat: "a:0", amount: 1000 }))
      .toEqual({ text: "a:0 earned 1,000 copper completing a bounty", tier: "bounty" });
    expect(formatCurrencyLine({ type: "bounty_clawback", seat: "a:0", amount: 50 }))
      .toEqual({ text: "a:0 — 50 copper clawed back on bounty objection", tier: "dispute" });
    expect(formatCurrencyLine({ type: "bounty_expire", amount: 100 }))
      .toEqual({ text: "100 copper destroyed on expired/abandoned bounty", tier: "destroyed" });
  });
});

describe("formatCurrencyLine — credit/debit reason-sniffing", () => {
  it("credit + reason 'won dispute' → dispute label", () => {
    expect(formatCurrencyLine({ type: "credit", seat: "a:0", amount: 50, reason: "won dispute" }))
      .toEqual({ text: "a:0 won dispute — 50 copper awarded", tier: "dispute" });
  });

  it("credit + reason 'edit' includes ref_msg if present", () => {
    expect(formatCurrencyLine({ type: "credit", seat: "a:0", amount: 10, reason: "edit", ref_msg: 42 }))
      .toEqual({ text: "a:0 earned 10 copper editing (msg #42)", tier: "earn" });
  });

  it("credit + reason 'pass' → fixed 1-copper text", () => {
    expect(formatCurrencyLine({ type: "credit", seat: "a:0", amount: 1, reason: "pass" }))
      .toEqual({ text: "a:0 earned 1 copper passing", tier: "earn" });
  });

  it("credit + reason 'speak' includes ref_msg", () => {
    expect(formatCurrencyLine({ type: "credit", seat: "a:0", amount: 3, reason: "speak", ref_msg: "42" }))
      .toEqual({ text: "a:0 earned 3 copper speaking (msg #42)", tier: "earn" });
  });

  it("credit fallback: bare amount + tier earn", () => {
    expect(formatCurrencyLine({ type: "credit", seat: "a:0", amount: 7 }))
      .toEqual({ text: "a:0 earned 7 copper", tier: "earn" });
  });

  it("debit + reason 'pool_destroyed' → catnip text", () => {
    expect(formatCurrencyLine({ type: "debit", amount: 100, reason: "pool_destroyed" }))
      .toEqual({ text: "Pool destroyed — 100 copper to catnip", tier: "destroyed" });
  });

  it("debit + reason 'objection' → fixed 50-copper text", () => {
    expect(formatCurrencyLine({ type: "debit", seat: "a:0", reason: "objection" }))
      .toEqual({ text: "a:0 filed objection — 50 copper", tier: "dispute" });
  });

  it("debit fallback: includes reason in parens", () => {
    expect(formatCurrencyLine({ type: "debit", seat: "a:0", amount: 5, reason: "decay 1.0%" }))
      .toEqual({ text: "a:0 — 5 copper debited (decay 1.0%)", tier: "loss" });
  });

  it("unknown type: passive-tier fallback", () => {
    expect(formatCurrencyLine({ type: "novel_type", seat: "a:0", amount: 10 }))
      .toEqual({ text: "a:0 — 10 copper", tier: "passive" });
  });

  it("missing amount: shows '?'", () => {
    expect(formatCurrencyLine({ type: "credit", seat: "a:0", reason: "speak" }))
      .toEqual({ text: "a:0 earned ? copper speaking", tier: "earn" });
  });

  it("missing seat: 'someone' fallback", () => {
    expect(formatCurrencyLine({ type: "passive" }))
      .toEqual({ text: "someone earned 1 copper passive", tier: "passive" });
  });
});

describe("buildMsgEconomy — earn + release composition", () => {
  it("returns null when no rows and no dispute", () => {
    expect(buildMsgEconomy(undefined, undefined)).toBeNull();
    expect(buildMsgEconomy([], undefined)).toBeNull();
  });

  it("settled credit + release → 'released' chip with action label", () => {
    const rows = [
      { type: "escrow_hold", amount: 3, action_kind: "speak", reason: "speak msg" },
      { type: "escrow_release", amount: 3 },
      { type: "credit", amount: 3 },
    ];
    const chips = buildMsgEconomy(rows, undefined);
    expect(chips).toEqual([{ text: "+3⊕ speak · ✓ released", tier: "earn" }]);
  });

  it("hold without release → 'in escrow' with release_turn", () => {
    const rows = [
      { type: "escrow_hold", amount: 3, release_turn: 7, action_kind: "speak" },
    ];
    const chips = buildMsgEconomy(rows, undefined);
    expect(chips).toEqual([{ text: "+3⊕ speak · in escrow → turn 7", tier: "hold" }]);
  });

  it("action label is sniffed from reason when action_kind missing", () => {
    const rows = [
      { type: "escrow_hold", amount: 3, release_turn: 7, reason: "edited message" },
    ];
    const chips = buildMsgEconomy(rows, undefined);
    expect(chips).toEqual([{ text: "+3⊕ edit · in escrow → turn 7", tier: "hold" }]);
  });

  it("falls back to 'earn' when nothing sniffable", () => {
    const rows = [
      { type: "escrow_hold", amount: 3, release_turn: 7 },
    ];
    const chips = buildMsgEconomy(rows, undefined);
    expect(chips).toEqual([{ text: "+3⊕ earn · in escrow → turn 7", tier: "hold" }]);
  });
});

describe("buildMsgEconomy — penalty + clawback + dispute chips", () => {
  it("penalty adds loss chip", () => {
    const rows = [
      { type: "credit", amount: 10, action_kind: "speak" },
      { type: "penalty", amount: 25 },
    ];
    const chips = buildMsgEconomy(rows, undefined);
    expect(chips).toEqual([
      { text: "+10⊕ speak · ✓ released", tier: "earn" },
      { text: "−25⊕ penalty", tier: "loss" },
    ]);
  });

  it("clawback adds dispute chip", () => {
    const rows = [
      { type: "credit", amount: 10, action_kind: "speak" },
      { type: "clawback", amount: 50 },
    ];
    const chips = buildMsgEconomy(rows, undefined);
    expect(chips).toEqual([
      { text: "+10⊕ speak · ✓ released", tier: "earn" },
      { text: "−50⊕ clawed back", tier: "dispute" },
    ]);
  });

  it("open dispute adds objected-by chip with pool", () => {
    const chips = buildMsgEconomy(undefined, {
      id: "d-1", status: "open", challenger: "dev-challenger:0", pool: 50,
    });
    expect(chips).toEqual([
      { text: "⊗ objected by dev-challenger:0 · pool 50⊕", tier: "dispute" },
    ]);
  });

  it("resolved dispute renders the resolution", () => {
    const chips = buildMsgEconomy(undefined, {
      id: "d-1", status: "resolved", resolution: "objection sustained",
    });
    expect(chips).toEqual([
      { text: "⊗ objection sustained", tier: "dispute" },
    ]);
  });

  it("resolved dispute without resolution string falls back to 'resolved'", () => {
    const chips = buildMsgEconomy(undefined, { id: "d-1", status: "rejected" });
    expect(chips).toEqual([
      { text: "⊗ resolved", tier: "dispute" },
    ]);
  });
});
