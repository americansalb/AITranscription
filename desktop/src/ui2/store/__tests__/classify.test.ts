import { describe, expect, it } from "vitest";
import { classify, discussionKey, role } from "../classify";
import type { BoardMessage } from "../types";

let nextId = 1;
function msg(partial: Partial<BoardMessage>): BoardMessage {
  return {
    id: nextId++,
    from: "developer:0",
    to: "all",
    type: "status",
    timestamp: "2026-06-09T20:00:00Z",
    subject: "",
    body: "",
    metadata: {},
    ...partial,
  } as BoardMessage;
}

const NO_DISCUSSIONS = new Set<string>();

describe("role normalization (MED-1)", () => {
  it("strips instance suffix", () => {
    expect(role("human:0")).toBe("human");
    expect(role("code-interpreter:12")).toBe("code-interpreter");
    expect(role("all")).toBe("all");
  });
  it("is total over garbage", () => {
    expect(role(undefined)).toBe("");
    expect(role(42)).toBe("");
    expect(role("  HUMAN:0 ")).toBe("human");
  });
});

describe("R1 — human posts", () => {
  it("matches human with and without instance", () => {
    expect(classify(msg({ from: "human:0" }), NO_DISCUSSIONS).rule).toBe("R1");
    expect(classify(msg({ from: "human" }), NO_DISCUSSIONS).rule).toBe("R1");
  });
});

describe("R2 — decision cards, author-gated (HIGH fix)", () => {
  it("relay card with choices is R2", () => {
    const m = msg({ from: "code-interpreter:0", to: "human:0", metadata: { choices: [{ id: "a", label: "Approve" }] } });
    expect(classify(m, NO_DISCUSSIONS).rule).toBe("R2");
  });
  it("a non-relay agent attaching choices does NOT become a card — door closed", () => {
    const m = msg({ from: "developer:0", to: "human:0", metadata: { choices: [{ id: "a", label: "x" }] } });
    const t = classify(m, NO_DISCUSSIONS);
    expect(t.rule).toBe("R6"); // falls through to the DM rule, flagged
  });
  it("non-relay choices to all falls to R7", () => {
    const m = msg({ from: "developer:0", to: "all", metadata: { choices: [] , } });
    expect(classify(m, NO_DISCUSSIONS).rule).toBe("R7");
  });
});

describe("R3 — relay posts", () => {
  it("relay to all and to human:0 are expanded", () => {
    expect(classify(msg({ from: "code-interpreter:0", to: "all" }), NO_DISCUSSIONS).rule).toBe("R3");
    expect(classify(msg({ from: "code-interpreter:0", to: "human:0" }), NO_DISCUSSIONS).rule).toBe("R3");
  });
  it("relay directed to a peer is NOT R3 (edge case 5)", () => {
    expect(classify(msg({ from: "code-interpreter:0", to: "developer" }), NO_DISCUSSIONS).rule).toBe("R7");
  });
});

describe("R4 — verdicts by field predicate only (MED-2)", () => {
  it("oxford ended (board msg 207 shape)", () => {
    const m = msg({ from: "system", metadata: { debate_id: 16, outcome: "side_b_wins", oxford_event: "ended" } });
    const t = classify(m, NO_DISCUSSIONS);
    expect(t.rule).toBe("R4");
    expect(t.rule === "R4" && t.discussionKey).toBe("oxford-16");
  });
  it("discussion end (board msg 220 shape)", () => {
    const m = msg({ from: "moderator:0", metadata: { discussion_action: "end", final_round: 1 } });
    expect(classify(m, NO_DISCUSSIONS).rule).toBe("R4");
  });
});

describe("R5/R7 — lifecycle resolution (LOW-1)", () => {
  const live = new Set(["oxford-16"]);
  it("keyed event with live discussion folds to R5", () => {
    const m = msg({ from: "system", metadata: { debate_id: 16, oxford_event: "phase_transition" } });
    expect(classify(m, live).rule).toBe("R5");
  });
  it("orphaned key falls to R7, not a phantom row", () => {
    const m = msg({ from: "system", metadata: { debate_id: 99, oxford_event: "phase_transition" } });
    expect(classify(m, live).rule).toBe("R7");
    const bare = msg({ from: "system", type: "moderation", metadata: { round: 3 } });
    expect(classify(bare, NO_DISCUSSIONS).rule).toBe("R7");
  });
});

describe("R6 — DMs to human from non-relay", () => {
  it("normalizes to-address (human vs human:0)", () => {
    expect(classify(msg({ from: "tech-leader:0", to: "human" }), NO_DISCUSSIONS).rule).toBe("R6");
    expect(classify(msg({ from: "tech-leader:0", to: "human:0" }), NO_DISCUSSIONS).rule).toBe("R6");
  });
  it("system to human is not flagged", () => {
    expect(classify(msg({ from: "system", to: "human:0" }), NO_DISCUSSIONS).rule).toBe("R7");
  });
});

describe("totality (edge case 4)", () => {
  it("never throws on malformed input", () => {
    const broken = { id: 1, from: null, to: undefined, type: 7, timestamp: "x", subject: null, body: null, metadata: "garbage" } as unknown as BoardMessage;
    expect(() => classify(broken, NO_DISCUSSIONS)).not.toThrow();
    expect(classify(broken, NO_DISCUSSIONS).rule).toBe("R7");
  });
});

describe("discussionKey tie-break", () => {
  it("debate_id wins over discussion_action", () => {
    const m = msg({ metadata: { debate_id: 16, discussion_action: "start" } });
    expect(discussionKey(m)).toBe("oxford-16");
  });
});
