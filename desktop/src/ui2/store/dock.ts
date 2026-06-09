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

/** A human reply that resolves a card: in_reply_to + (choice_id | panel-other text). */
function resolutionOf(card: BoardMessage, messages: BoardMessage[]): BoardMessage | null {
  for (const m of messages) {
    if (role(m.from) !== "human") continue;
    const mm = meta(m);
    if (mm.in_reply_to === card.id) return m;
  }
  return null;
}

export function deriveDock(
  messages: BoardMessage[],
  classified: ReadonlyMap<number, Treatment>,
): DecisionCardState[] {
  const cards = messages.filter((m) => classified.get(m.id)?.rule === "R2");
  const states: DecisionCardState[] = [];
  let activeAssigned: number | null = null;

  for (const card of cards) {
    const resolution = resolutionOf(card, messages);
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
