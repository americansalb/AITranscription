// Pure feed derivation: classified messages → FeedRow[].
// R5 events fold into ONE living row per discussion (per-discussion identity:
// non-Oxford keys derive from the start-message id, ends retire keys); R7
// folds into time-burst rows (gap > 10 min closes a burst).
// Mute (IA table §2): while muted, R3–R7 accrue with ZERO screen movement —
// the single "caught up" row is emitted only by the unmute derivation.
// Audit invariant: every message lands in exactly one row or the engine-only
// set — reconcile() proves it.
import { classify, discussionKey, isDiscussionStart } from "./classify";
import type { BoardMessage, FeedRow, Treatment } from "./types";

export const BURST_GAP_MS = 10 * 60 * 1000;

export interface CatchupRange {
  from: number;
  to: number;
}

function ts(msg: BoardMessage): number {
  const t = Date.parse(msg.timestamp);
  return Number.isFinite(t) ? t : 0;
}

function discussionLabel(events: BoardMessage[], key: string): string {
  const start = events.find(isDiscussionStart);
  if (start?.subject) return start.subject;
  return key.startsWith("oxford-") ? `Oxford ${key.slice(7)}` : "Discussion";
}

export interface DerivedFeed {
  rows: FeedRow[];
  engineOnly: BoardMessage[]; // R6 + mute-accrued — never surfaced, always audited
  protocolViolations: number;
  classified: Map<number, Treatment>;
}

/**
 * deriveFeed — the only place feed structure is computed.
 * `mutedAtId`: room muted at message id N → R3–R7 with id > N accrue silently.
 * `catchup`: set by the unmute action — that id range renders as ONE
 * "caught up" row instead of re-deriving as ordinary rows.
 */
export function deriveFeed(
  messages: BoardMessage[],
  mutedAtId: number | null,
  catchup: CatchupRange | null = null,
): DerivedFeed {
  const live = new Set<string>();
  let continuousKey: string | null = null;

  const rows: FeedRow[] = [];
  const engineOnly: BoardMessage[] = [];
  const classified = new Map<number, Treatment>();
  const discussionRows = new Map<string, Extract<FeedRow, { kind: "discussion" }>>();
  let burst: Extract<FeedRow, { kind: "burst" }> | null = null;
  let catchupRow: Extract<FeedRow, { kind: "burst" }> | null = null;
  let violations = 0;

  const closeBurst = () => {
    burst = null;
  };

  const pushToBurst = (msg: BoardMessage) => {
    if (burst && ts(msg) - Date.parse(burst.lastTimestamp) > BURST_GAP_MS) closeBurst();
    if (!burst) {
      burst = {
        kind: "burst",
        key: `b${msg.id}`,
        count: 0,
        protocolViolations: 0,
        firstTimestamp: msg.timestamp,
        lastTimestamp: msg.timestamp,
        events: [],
      };
      rows.push(burst);
    }
    burst.events.push(msg);
    burst.count++;
    burst.lastTimestamp = msg.timestamp;
  };

  for (const msg of messages) {
    // open discussion identities on start events, sequentially (a start may
    // not retroactively claim earlier events — edge case 3)
    if (isDiscussionStart(msg)) {
      const raw = discussionKey(msg);
      if (raw === "continuous") {
        continuousKey = `disc-${msg.id}`;
        live.add("continuous");
      } else if (raw) {
        live.add(raw);
      }
    }

    const t = classify(msg, live);
    classified.set(msg.id, t);

    // R6 never surfaces, in any mode
    if (t.rule === "R6") {
      violations++;
      engineOnly.push(msg);
      continue;
    }

    // mute overlay: zero screen movement — no rows, no count ticks
    if (mutedAtId !== null && msg.id > mutedAtId && t.rule !== "R1" && t.rule !== "R2") {
      engineOnly.push(msg);
      continue;
    }

    // unmute catch-up: the accrued range folds into ONE row, not ordinary rows
    if (catchup && msg.id >= catchup.from && msg.id <= catchup.to && t.rule !== "R1" && t.rule !== "R2") {
      if (!catchupRow) {
        catchupRow = {
          kind: "burst",
          key: `muted-catchup-${catchup.from}`,
          count: 0,
          protocolViolations: 0,
          firstTimestamp: msg.timestamp,
          lastTimestamp: msg.timestamp,
          events: [],
        };
        closeBurst();
        rows.push(catchupRow);
      }
      catchupRow.events.push(msg);
      catchupRow.count++;
      catchupRow.lastTimestamp = msg.timestamp;
      continue;
    }

    switch (t.rule) {
      case "R1":
        closeBurst();
        rows.push({ kind: "message", key: `m${msg.id}`, msg, voice: "human" });
        break;
      case "R2":
        closeBurst();
        rows.push({ kind: "card", key: `c${msg.id}`, msg });
        break;
      case "R3":
        closeBurst();
        rows.push({ kind: "message", key: `m${msg.id}`, msg, voice: "relay" });
        break;
      case "R4":
      case "R5": {
        const key = t.discussionKey === "continuous" ? continuousKey : t.discussionKey;
        if (!key) {
          // orphan lifecycle/end with no open discussion — catch-all (edge case 3)
          pushToBurst(msg);
          break;
        }
        let row = discussionRows.get(key);
        if (!row) {
          row = {
            kind: "discussion",
            key: `d-${key}`,
            discussionKey: key,
            label: "",
            eventCount: 0,
            verdict: null,
            lastTimestamp: msg.timestamp,
            events: [],
          };
          discussionRows.set(key, row);
          closeBurst();
          rows.push(row); // one living row, positioned at first event
        }
        row.events.push(msg);
        row.eventCount++;
        row.lastTimestamp = msg.timestamp;
        if (t.rule === "R4") {
          row.verdict = msg;
          // retire the identity: the next start opens a NEW row
          if (t.discussionKey === "continuous") {
            continuousKey = null;
            live.delete("continuous");
          } else {
            live.delete(t.discussionKey);
          }
        }
        break;
      }
      case "R7":
        pushToBurst(msg);
        break;
    }
  }

  for (const row of discussionRows.values()) {
    row.label = discussionLabel(row.events, row.discussionKey);
  }
  return { rows, engineOnly, protocolViolations: violations, classified };
}

/** Audit invariant (§0): every input message is accounted for exactly once. */
export function reconcile(messages: BoardMessage[], feed: DerivedFeed): boolean {
  const seen = new Set<number>();
  const add = (m: BoardMessage) => {
    if (seen.has(m.id)) return false;
    seen.add(m.id);
    return true;
  };
  for (const row of feed.rows) {
    if (row.kind === "message" || row.kind === "card") {
      if (!add(row.msg)) return false;
    } else if (row.kind === "discussion" || row.kind === "burst") {
      for (const m of row.events) if (!add(m)) return false;
    }
  }
  for (const m of feed.engineOnly) if (!add(m)) return false;
  return messages.every((m) => seen.has(m.id));
}
