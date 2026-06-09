// Pure feed derivation: classified messages → FeedRow[].
// R5 events fold into ONE living row per discussion; R7 folds into
// time-burst rows (gap > 10 min closes a burst). Audit invariant: every
// message lands in exactly one row or the engine-only set — reconcile() proves it.
import { classify, discussionKey, isDiscussionStart } from "./classify";
import type { BoardMessage, FeedRow, Treatment } from "./types";

export const BURST_GAP_MS = 10 * 60 * 1000;

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
  engineOnly: BoardMessage[]; // R6 — never surfaced, always audited
  protocolViolations: number;
  classified: Map<number, Treatment>;
}

/**
 * deriveFeed — the only place feed structure is computed.
 * `mutedAtId`: when the room is muted at message id N, messages with id > N
 * matching R3–R7 accrue to a catch-up set instead of rendering (§2 overlay).
 */
export function deriveFeed(messages: BoardMessage[], mutedAtId: number | null): DerivedFeed {
  const live = new Set<string>();
  for (const m of messages) {
    if (isDiscussionStart(m)) {
      const k = discussionKey(m);
      if (k) live.add(k);
    }
  }

  const rows: FeedRow[] = [];
  const engineOnly: BoardMessage[] = [];
  const classified = new Map<number, Treatment>();
  const discussionRows = new Map<string, Extract<FeedRow, { kind: "discussion" }>>();
  let burst: Extract<FeedRow, { kind: "burst" }> | null = null;
  let violations = 0;
  let mutedCount = 0;

  const closeBurst = () => {
    burst = null;
  };

  for (const msg of messages) {
    const t = classify(msg, live);
    classified.set(msg.id, t);

    const isMuted = mutedAtId !== null && msg.id > mutedAtId && t.rule !== "R1" && t.rule !== "R2";
    if (isMuted) {
      mutedCount++;
      if (t.rule === "R6") violations++;
      engineOnly.push(msg);
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
        const key = t.discussionKey;
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
        if (t.rule === "R4") row.verdict = msg;
        break;
      }
      case "R6":
        violations++;
        engineOnly.push(msg);
        if (burst) burst.protocolViolations++;
        break;
      case "R7": {
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
        break;
      }
    }
  }

  for (const row of discussionRows.values()) {
    row.label = discussionLabel(row.events, row.discussionKey);
  }
  if (mutedAtId !== null && mutedCount > 0) {
    rows.push({
      kind: "burst",
      key: "muted-catchup",
      count: mutedCount,
      protocolViolations: 0,
      firstTimestamp: "",
      lastTimestamp: "",
      events: [], // contents stay in the Engine Room until unmute re-derives
    });
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
