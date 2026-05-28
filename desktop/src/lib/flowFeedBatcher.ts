/**
 * Flow Feed batching — collapses per-turn rotation events (passive, interest,
 * decay) into one-line summaries so the currency activity feed shows ONE row
 * per turn instead of N rows per N seats per turn. Extracted from
 * CollabTab.tsx flowFeedRows useMemo so it can be unit-tested in isolation.
 *
 * Three batch types:
 *   - passive: every seat earns 1 copper per turn — "N seats earned 1 copper passive (turn T)"
 *   - interest: every seat earns 1c per 10c held per turn (amounts vary) —
 *     "N seats earned X copper interest (turn T)"
 *   - decay: every seat loses % of balance to decay per turn (no dedicated
 *     row type — sniffed via reason/action_kind containing "decay") —
 *     "N seats lost X copper to decay (turn T)"
 *
 * Three-way flush ordering: entering any batch type flushes the OTHER two
 * batches before accumulating into its own; any non-batch row flushes all
 * three batches before emitting.
 */

// Mirrors CollabTab.tsx:162 — kept inline rather than extracting to a shared
// module in this commit; future commit can unify.
export type CurrencyTier = "earn" | "hold" | "loss" | "dispute" | "passive" | "destroyed" | "bounty" | "session-start";

export interface CurrencyFeedRow {
  id?: string;
  type?: string;
  seat?: string;
  amount?: number;
  reason?: string;
  ref_msg?: number | string;
  balance_after?: number;
  escrow_id?: string;
  release_turn?: number;
  action_kind?: string;
  turn?: number;
  linked_edit_msg?: number;
  at?: string;
}

export interface FlowFeedRow {
  key: string;
  text: string;
  tier: CurrencyTier;
  seat?: string;
  at?: string;
}

export interface FormatFn {
  (row: CurrencyFeedRow): { text: string; tier: CurrencyTier };
}

const isDecayRow = (row: CurrencyFeedRow): boolean => {
  if (row.type !== "debit") return false;
  const reason = (row.reason || "").toLowerCase();
  const kind = (row.action_kind || "").toLowerCase();
  return reason.includes("decay") || kind.includes("decay");
};

/**
 * Build the user-facing flow-feed row list from raw currency feed rows.
 * @param currencyFeed Raw rows in chronological order.
 * @param formatLine Per-row formatter for non-batched rows.
 * @param sliceTail Number of trailing rows to return (default 50).
 */
export function buildFlowFeedRows(
  currencyFeed: CurrencyFeedRow[],
  formatLine: FormatFn,
  sliceTail = 50,
): FlowFeedRow[] {
  const out: FlowFeedRow[] = [];
  let passiveBatch: { turn: number | undefined; count: number; at?: string } | null = null;
  let interestBatch: { turn: number | undefined; count: number; total: number; at?: string } | null = null;
  let decayBatch: { turn: number | undefined; count: number; total: number; at?: string } | null = null;

  const flushPassive = () => {
    if (!passiveBatch) return;
    const turnLabel = passiveBatch.turn != null ? ` (turn ${passiveBatch.turn})` : "";
    out.push({
      key: `passive-turn-${passiveBatch.turn ?? `idx${out.length}`}`,
      text: `${passiveBatch.count} seat${passiveBatch.count === 1 ? "" : "s"} earned 1 copper passive${turnLabel}`,
      tier: "passive",
      at: passiveBatch.at,
    });
    passiveBatch = null;
  };
  const flushInterest = () => {
    if (!interestBatch) return;
    const turnLabel = interestBatch.turn != null ? ` (turn ${interestBatch.turn})` : "";
    out.push({
      key: `interest-turn-${interestBatch.turn ?? `idx${out.length}`}`,
      text: `${interestBatch.count} seat${interestBatch.count === 1 ? "" : "s"} earned ${interestBatch.total.toLocaleString()} copper interest${turnLabel}`,
      tier: "earn",
      at: interestBatch.at,
    });
    interestBatch = null;
  };
  const flushDecay = () => {
    if (!decayBatch) return;
    const turnLabel = decayBatch.turn != null ? ` (turn ${decayBatch.turn})` : "";
    out.push({
      key: `decay-turn-${decayBatch.turn ?? `idx${out.length}`}`,
      text: `${decayBatch.count} seat${decayBatch.count === 1 ? "" : "s"} lost ${decayBatch.total.toLocaleString()} copper to decay${turnLabel}`,
      tier: "loss",
      at: decayBatch.at,
    });
    decayBatch = null;
  };

  for (const row of currencyFeed) {
    if (row.type === "passive") {
      flushInterest();
      flushDecay();
      if (passiveBatch && passiveBatch.turn === row.turn) {
        passiveBatch.count += 1;
        passiveBatch.at = row.at;
      } else {
        flushPassive();
        passiveBatch = { turn: row.turn, count: 1, at: row.at };
      }
      continue;
    }
    if (row.type === "interest") {
      flushPassive();
      flushDecay();
      const amt = typeof row.amount === "number" ? Math.abs(row.amount) : 0;
      if (interestBatch && interestBatch.turn === row.turn) {
        interestBatch.count += 1;
        interestBatch.total += amt;
        interestBatch.at = row.at;
      } else {
        flushInterest();
        interestBatch = { turn: row.turn, count: 1, total: amt, at: row.at };
      }
      continue;
    }
    if (isDecayRow(row)) {
      flushPassive();
      flushInterest();
      const amt = typeof row.amount === "number" ? Math.abs(row.amount) : 0;
      if (decayBatch && decayBatch.turn === row.turn) {
        decayBatch.count += 1;
        decayBatch.total += amt;
        decayBatch.at = row.at;
      } else {
        flushDecay();
        decayBatch = { turn: row.turn, count: 1, total: amt, at: row.at };
      }
      continue;
    }
    flushPassive();
    flushInterest();
    flushDecay();
    const formatted = formatLine(row);
    out.push({ key: row.id || `${row.at ?? ""}-${out.length}`, text: formatted.text, tier: formatted.tier, seat: row.seat, at: row.at });
  }
  flushPassive();
  flushInterest();
  flushDecay();
  return out.slice(-sliceTail);
}

export const _internal = { isDecayRow };
