// Decision Dock state machine (decree §4.2) — pure derivation.
// One active card; queued cards visible with the blocker NAMED (the
// msg-104/122 silent-block failure made visible); resolved cards show
// what was chosen and when.
import { role } from "./classify";
import type { BoardMessage, DecisionCardState, Treatment } from "./types";

function meta(msg: BoardMessage): Record<string, unknown> {
  return msg.metadata && typeof msg.metadata === "object"
    ? (msg.metadata as Record<string, unknown>)
    : {};
}

/** One pass: card id → the human reply that resolves it (in_reply_to). */
function buildResolutionIndex(messages: BoardMessage[]): Map<number, BoardMessage> {
  const index = new Map<number, BoardMessage>();
  for (const m of messages) {
    if (role(m.from) !== "human") continue;
    const target = meta(m).in_reply_to;
    if (typeof target === "number" && !index.has(target)) index.set(target, m);
  }
  return index;
}

export function deriveDock(
  messages: BoardMessage[],
  classified: ReadonlyMap<number, Treatment>,
): DecisionCardState[] {
  const cards = messages.filter((m) => classified.get(m.id)?.rule === "R2");
  const resolutions = buildResolutionIndex(messages);
  const states: DecisionCardState[] = [];
  let activeAssigned: number | null = null;

  for (const card of cards) {
    const resolution = resolutions.get(card.id) ?? null;
    if (resolution) {
      const mm = meta(resolution);
      const choice =
        typeof mm.choice_id === "string" && mm.choice_id.length > 0
          ? mm.choice_id
          : resolution.body || "(resolved)";
      states.push({
        msg: card,
        status: "resolved",
        blockedBy: null,
        resolvedChoice: choice,
        resolvedAt: resolution.timestamp,
      });
    } else if (activeAssigned === null) {
      activeAssigned = card.id;
      states.push({ msg: card, status: "active", blockedBy: null, resolvedChoice: null, resolvedAt: null });
    } else {
      states.push({
        msg: card,
        status: "queued",
        blockedBy: activeAssigned,
        resolvedChoice: null,
        resolvedAt: null,
      });
    }
  }
  return states;
}
