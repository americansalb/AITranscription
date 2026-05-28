/**
 * Tests for flowFeedBatcher — per-turn batching of passive / interest / decay
 * rows in the currency activity feed.
 *
 * Covers:
 *   - Passive: per-turn batching with count
 *   - Interest: per-turn batching with sum (amounts vary)
 *   - Decay: per-turn batching with sum (sniffed via reason/action_kind)
 *   - Three-way flush ordering: entering one batch type flushes the other two
 *   - Non-batch rows flush all three batches before emitting
 *   - sliceTail: trims to the last N rows
 *   - Edge cases: empty feed, single row, decay-without-explicit-type
 */
import { describe, it, expect } from "vitest";

import { buildFlowFeedRows, type CurrencyFeedRow, type FormatFn } from "../lib/flowFeedBatcher";

const passthroughFormat: FormatFn = (row) => ({
  text: `${row.seat ?? "someone"} debit ${row.amount ?? 0}`,
  tier: "loss",
});

describe("buildFlowFeedRows — passive batching", () => {
  it("collapses N passive rows from the same turn into one summary line", () => {
    const feed: CurrencyFeedRow[] = [
      { type: "passive", turn: 5, seat: "architect:0", at: "t1" },
      { type: "passive", turn: 5, seat: "developer:0", at: "t2" },
      { type: "passive", turn: 5, seat: "tester:0", at: "t3" },
    ];
    const rows = buildFlowFeedRows(feed, passthroughFormat);
    expect(rows).toHaveLength(1);
    expect(rows[0].text).toBe("3 seats earned 1 copper passive (turn 5)");
    expect(rows[0].tier).toBe("passive");
    expect(rows[0].at).toBe("t3");
  });

  it("uses singular 'seat' for count of 1", () => {
    const feed: CurrencyFeedRow[] = [
      { type: "passive", turn: 5, seat: "architect:0", at: "t1" },
    ];
    const rows = buildFlowFeedRows(feed, passthroughFormat);
    expect(rows[0].text).toBe("1 seat earned 1 copper passive (turn 5)");
  });

  it("starts a new batch when turn changes", () => {
    const feed: CurrencyFeedRow[] = [
      { type: "passive", turn: 5, seat: "a:0", at: "t1" },
      { type: "passive", turn: 5, seat: "b:0", at: "t2" },
      { type: "passive", turn: 6, seat: "a:0", at: "t3" },
    ];
    const rows = buildFlowFeedRows(feed, passthroughFormat);
    expect(rows).toHaveLength(2);
    expect(rows[0].text).toBe("2 seats earned 1 copper passive (turn 5)");
    expect(rows[1].text).toBe("1 seat earned 1 copper passive (turn 6)");
  });

  it("handles passive rows without a turn field", () => {
    const feed: CurrencyFeedRow[] = [
      { type: "passive", seat: "a:0", at: "t1" },
      { type: "passive", seat: "b:0", at: "t2" },
    ];
    const rows = buildFlowFeedRows(feed, passthroughFormat);
    expect(rows).toHaveLength(1);
    expect(rows[0].text).toBe("2 seats earned 1 copper passive");
  });
});

describe("buildFlowFeedRows — interest batching", () => {
  it("sums interest amounts across same-turn rows", () => {
    const feed: CurrencyFeedRow[] = [
      { type: "interest", turn: 5, amount: 12, seat: "a:0", at: "t1" },
      { type: "interest", turn: 5, amount: 8, seat: "b:0", at: "t2" },
      { type: "interest", turn: 5, amount: 5, seat: "c:0", at: "t3" },
    ];
    const rows = buildFlowFeedRows(feed, passthroughFormat);
    expect(rows).toHaveLength(1);
    expect(rows[0].text).toBe("3 seats earned 25 copper interest (turn 5)");
    expect(rows[0].tier).toBe("earn");
  });

  it("uses absolute value of negative amounts", () => {
    const feed: CurrencyFeedRow[] = [
      { type: "interest", turn: 5, amount: -10, seat: "a:0" },
    ];
    const rows = buildFlowFeedRows(feed, passthroughFormat);
    expect(rows[0].text).toBe("1 seat earned 10 copper interest (turn 5)");
  });
});

describe("buildFlowFeedRows — decay batching", () => {
  it("collapses decay rows (debit + reason includes 'decay') into one summary", () => {
    const feed: CurrencyFeedRow[] = [
      { type: "debit", reason: "decay 1.0%", turn: 5, amount: 100, seat: "a:0", at: "t1" },
      { type: "debit", reason: "decay 1.0%", turn: 5, amount: 200, seat: "b:0", at: "t2" },
    ];
    const rows = buildFlowFeedRows(feed, passthroughFormat);
    expect(rows).toHaveLength(1);
    expect(rows[0].text).toBe("2 seats lost 300 copper to decay (turn 5)");
    expect(rows[0].tier).toBe("loss");
  });

  it("sniffs decay via action_kind when reason is silent", () => {
    const feed: CurrencyFeedRow[] = [
      { type: "debit", action_kind: "DECAY", turn: 5, amount: 50, seat: "a:0" },
      { type: "debit", action_kind: "decay", turn: 5, amount: 75, seat: "b:0" },
    ];
    const rows = buildFlowFeedRows(feed, passthroughFormat);
    expect(rows).toHaveLength(1);
    expect(rows[0].text).toBe("2 seats lost 125 copper to decay (turn 5)");
  });

  it("does NOT batch debit rows whose reason/action_kind lacks 'decay'", () => {
    const feed: CurrencyFeedRow[] = [
      { type: "debit", reason: "speak penalty", turn: 5, amount: 50, seat: "a:0" },
      { type: "debit", reason: "speak penalty", turn: 5, amount: 50, seat: "b:0" },
    ];
    const rows = buildFlowFeedRows(feed, passthroughFormat);
    // Two non-decay debit rows pass through formatLine individually.
    expect(rows).toHaveLength(2);
  });
});

describe("buildFlowFeedRows — three-way flush ordering", () => {
  it("flushes passive when interest starts in the same stream", () => {
    const feed: CurrencyFeedRow[] = [
      { type: "passive", turn: 5, seat: "a:0", at: "t1" },
      { type: "passive", turn: 5, seat: "b:0", at: "t2" },
      { type: "interest", turn: 5, amount: 10, seat: "a:0", at: "t3" },
    ];
    const rows = buildFlowFeedRows(feed, passthroughFormat);
    expect(rows).toHaveLength(2);
    expect(rows[0].text).toContain("passive");
    expect(rows[1].text).toContain("interest");
  });

  it("flushes both passive and interest when decay starts", () => {
    const feed: CurrencyFeedRow[] = [
      { type: "passive", turn: 5, seat: "a:0" },
      { type: "interest", turn: 5, amount: 5, seat: "a:0" },
      { type: "debit", reason: "decay", turn: 5, amount: 50, seat: "a:0" },
    ];
    const rows = buildFlowFeedRows(feed, passthroughFormat);
    expect(rows).toHaveLength(3);
    expect(rows[0].text).toContain("passive");
    expect(rows[1].text).toContain("interest");
    expect(rows[2].text).toContain("decay");
  });

  it("flushes all three batches when a non-batch row appears", () => {
    const feed: CurrencyFeedRow[] = [
      { type: "passive", turn: 5, seat: "a:0" },
      { type: "interest", turn: 5, amount: 5, seat: "a:0" },
      { type: "debit", reason: "decay", turn: 5, amount: 50, seat: "a:0" },
      { type: "credit", reason: "speak", amount: 10, seat: "a:0", id: "credit-1" },
    ];
    const rows = buildFlowFeedRows(feed, passthroughFormat);
    expect(rows).toHaveLength(4);
    expect(rows[0].text).toContain("passive");
    expect(rows[1].text).toContain("interest");
    expect(rows[2].text).toContain("decay");
    expect(rows[3].text).toBe("a:0 debit 10");
  });
});

describe("buildFlowFeedRows — tail slicing + edge cases", () => {
  it("returns last N rows when feed exceeds sliceTail", () => {
    const feed: CurrencyFeedRow[] = Array.from({ length: 100 }, (_, i) => ({
      type: "credit",
      reason: "speak",
      amount: 1,
      seat: `seat:${i}`,
      id: `id-${i}`,
    }));
    const rows = buildFlowFeedRows(feed, passthroughFormat, 10);
    expect(rows).toHaveLength(10);
    expect(rows[0].text).toContain("seat:90");
    expect(rows[9].text).toContain("seat:99");
  });

  it("returns empty array for empty feed", () => {
    expect(buildFlowFeedRows([], passthroughFormat)).toEqual([]);
  });

  it("flushes pending batches at end of feed", () => {
    const feed: CurrencyFeedRow[] = [
      { type: "passive", turn: 5, seat: "a:0", at: "t1" },
    ];
    const rows = buildFlowFeedRows(feed, passthroughFormat);
    expect(rows).toHaveLength(1);
    expect(rows[0].text).toBe("1 seat earned 1 copper passive (turn 5)");
  });
});
