// The IA decision table compiled to one pure, total function.
// Spec: .vaak/design-notes/2026-06-09-ui2-phase0-ia-decision-table.md
// Priority-ordered, first match wins. classify() never throws.
import type { BoardMessage, Treatment } from "./types";

/** Normalization (table preamble): role("human:0") === "human". */
export function role(x: unknown): string {
  if (typeof x !== "string") return "";
  const i = x.indexOf(":");
  return (i === -1 ? x : x.slice(0, i)).trim().toLowerCase();
}

const RELAY = "code-interpreter";
const CARD_AUTHORS = new Set([RELAY, "human", "system"]);

function meta(msg: BoardMessage): Record<string, unknown> {
  return msg.metadata && typeof msg.metadata === "object"
    ? (msg.metadata as Record<string, unknown>)
    : {};
}

function isCardShaped(msg: BoardMessage): boolean {
  return Array.isArray(meta(msg).choices) || msg.type === "decision";
}

/** Raw discussion key for R4/R5 grouping. Tie-break: debate_id first.
 * Non-Oxford discussions return the sentinel "continuous" — deriveFeed maps it
 * to a per-discussion identity keyed by the start-message id, so sequential
 * Delphi/Continuous discussions never merge into one row (adversary pass
 * msg 282 MED-3 / msg 284 MED-2). */
export function discussionKey(msg: BoardMessage): string | null {
  const m = meta(msg);
  if (m.debate_id !== undefined && m.debate_id !== null) return `oxford-${m.debate_id}`;
  if (m.oxford_event !== undefined) return "oxford-unkeyed";
  if (m.discussion_action !== undefined || m.round !== undefined) return "continuous";
  return null;
}

function isVerdictEvent(msg: BoardMessage): boolean {
  const m = meta(msg);
  // Field predicates only (msgs 207, 220 shapes) — no prose matching.
  return m.oxford_event === "ended" || m.discussion_action === "end";
}

/**
 * classify — total function over any board message.
 * `liveDiscussions` = keys the store has a start record for (edge case 3:
 * an R5 key that resolves to no live discussion falls through to R7).
 */
export function classify(msg: BoardMessage, liveDiscussions: ReadonlySet<string>): Treatment {
  const from = role(msg.from);
  const to = role(msg.to);

  // R1 — human's own posts
  if (from === "human") return { rule: "R1", kind: "human" };

  // R2 — decision cards, author-gated BEFORE the field match (decree §8.2:
  // relay is the only agent writer of cards). Non-authorized card-shaped
  // messages fall through to R6/R7 and are flagged there.
  if (isCardShaped(msg) && CARD_AUTHORS.has(from)) return { rule: "R2", kind: "card" };

  // R4 — discussion verdicts (before R3: a system "end" event is a verdict
  // even though system is also a permitted card author; before R5 by spec).
  if (isVerdictEvent(msg)) {
    const key = discussionKey(msg) ?? "discussion-active";
    return { rule: "R4", kind: "verdict", discussionKey: key };
  }

  // R3 — relay posts to the room or the human
  if (from === RELAY && (to === "all" || to === "human")) return { rule: "R3", kind: "relay" };

  // R5 — discussion lifecycle, only if the key resolves to a live discussion
  const key = discussionKey(msg);
  if (key !== null && liveDiscussions.has(key)) {
    return { rule: "R5", kind: "lifecycle", discussionKey: key };
  }

  // R6 — DM-style message to human from a non-relay seat (complaint #1)
  if (to === "human" && from !== RELAY && from !== "system") {
    return { rule: "R6", kind: "dm-violation" };
  }

  // R7 — everything else, including unresolvable R5 keys (catch-all, total)
  return { rule: "R7", kind: "engine" };
}

/** Discussion start records — what makes a key "live" for R5. */
export function isDiscussionStart(msg: BoardMessage): boolean {
  const m = meta(msg);
  return (
    m.discussion_action === "start" ||
    m.oxford_event === "initiated" ||
    (role(msg.from) === "system" && msg.type === "moderation" && m.discussion_action === "start")
  );
}
