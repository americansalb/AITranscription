import { describe, expect, it } from "vitest";
import { deriveFeed, reconcile } from "../digest";
import { deriveDock } from "../dock";
import { seatLiveness } from "../liveness";
import type { BoardMessage } from "../types";

function msg(id: number, partial: Partial<BoardMessage>): BoardMessage {
  return {
    id,
    from: "developer:0",
    to: "all",
    type: "status",
    timestamp: `2026-06-09T20:${String(id).padStart(2, "0")}:00Z`,
    subject: "",
    body: "",
    metadata: {},
    ...partial,
  } as BoardMessage;
}

describe("deriveFeed — folding", () => {
  it("a full discussion folds to ONE living row with verdict (§4.5 one-card test)", () => {
    const messages = [
      msg(1, { from: "system", type: "moderation", subject: "Oxford #16", metadata: { debate_id: 16, oxford_event: "initiated" } }),
      msg(2, { from: "system", metadata: { debate_id: 16, oxford_event: "phase_transition" } }),
      msg(3, { from: "system", metadata: { debate_id: 16, oxford_event: "speaker_yielded" } }),
      msg(4, { from: "system", subject: "ended", metadata: { debate_id: 16, oxford_event: "ended", outcome: "side_b_wins" } }),
    ];
    const feed = deriveFeed(messages, null);
    const discussionRows = feed.rows.filter((r) => r.kind === "discussion");
    expect(discussionRows).toHaveLength(1);
    const row = discussionRows[0];
    expect(row.kind === "discussion" && row.eventCount).toBe(4);
    expect(row.kind === "discussion" && row.verdict?.id).toBe(4);
    expect(reconcile(messages, feed)).toBe(true);
  });

  it("R7 traffic bursts on 10-min gaps", () => {
    const messages = [
      msg(1, { timestamp: "2026-06-09T20:00:00Z" }),
      msg(2, { timestamp: "2026-06-09T20:05:00Z" }),
      msg(3, { timestamp: "2026-06-09T20:30:00Z" }), // > 10 min gap → new burst
    ];
    const feed = deriveFeed(messages, null);
    const bursts = feed.rows.filter((r) => r.kind === "burst");
    expect(bursts).toHaveLength(2);
    expect(bursts[0].kind === "burst" && bursts[0].count).toBe(2);
    expect(reconcile(messages, feed)).toBe(true);
  });

  it("R6 DMs never surface but are counted and audited", () => {
    const messages = [msg(1, { from: "tech-leader:0", to: "human:0", body: "psst" })];
    const feed = deriveFeed(messages, null);
    expect(feed.rows.filter((r) => r.kind !== "burst")).toHaveLength(0);
    expect(feed.protocolViolations).toBe(1);
    expect(feed.engineOnly).toHaveLength(1);
    expect(reconcile(messages, feed)).toBe(true);
  });

  it("human and relay render as expanded message rows", () => {
    const messages = [
      msg(1, { from: "human:0", body: "are we done" }),
      msg(2, { from: "code-interpreter:0", to: "all", body: "status" }),
    ];
    const feed = deriveFeed(messages, null);
    expect(feed.rows.map((r) => r.kind)).toEqual(["message", "message"]);
  });
});

describe("deriveFeed — mute overlay (§2)", () => {
  const messages = [
    msg(1, { from: "code-interpreter:0", to: "all", body: "before mute" }),
    msg(2, { from: "code-interpreter:0", to: "all", body: "after mute — must not render" }),
    msg(3, { from: "human:0", body: "human still renders" }),
    msg(4, {
      from: "code-interpreter:0",
      to: "human:0",
      subject: "card",
      metadata: { choices: [{ id: "a", label: "go" }] },
    }),
  ];

  it("while muted: ZERO movement — only R1/R2 render, no catch-up row, no ticking", () => {
    const feed = deriveFeed(messages, 1); // muted at id 1
    const kinds = feed.rows.map((r) => r.kind);
    expect(kinds).toContain("card"); // R2 passes the overlay
    expect(kinds.filter((k) => k === "message")).toHaveLength(2); // msg1 (pre-mute) + human
    expect(feed.rows.some((r) => r.kind === "burst")).toBe(false); // nothing else moves
    expect(feed.engineOnly.map((m) => m.id)).toEqual([2]); // accrued, audited
    expect(reconcile(messages, feed)).toBe(true);
  });

  it("on unmute: accrued range folds into ONE catch-up row, not ordinary rows", () => {
    const feed = deriveFeed(messages, null, { from: 2, to: 4 });
    const catchup = feed.rows.find((r) => r.kind === "burst" && r.key.startsWith("muted-catchup"));
    expect(catchup && catchup.kind === "burst" && catchup.count).toBe(1); // msg 2 only
    expect(catchup && catchup.kind === "burst" && catchup.events.map((m) => m.id)).toEqual([2]);
    // R1 and R2 inside the range rendered normally (they were visible during mute)
    expect(feed.rows.map((r) => r.kind)).toContain("card");
    expect(reconcile(messages, feed)).toBe(true);
  });
});

describe("deriveFeed — per-discussion identity (msg 282 MED-3 / 284 MED-2)", () => {
  it("two sequential continuous discussions = two rows, each keeping its own verdict", () => {
    const messages = [
      msg(1, { from: "system", type: "moderation", subject: "review A", metadata: { discussion_action: "start" } }),
      msg(2, { from: "system", type: "moderation", metadata: { discussion_action: "auto_round", round: 1 } }),
      msg(3, { from: "moderator:0", subject: "A ended", metadata: { discussion_action: "end", final_round: 1 } }),
      msg(4, { from: "system", type: "moderation", subject: "review B", metadata: { discussion_action: "start" } }),
      msg(5, { from: "moderator:0", subject: "B ended", metadata: { discussion_action: "end", final_round: 1 } }),
    ];
    const feed = deriveFeed(messages, null);
    const rows = feed.rows.filter((r) => r.kind === "discussion");
    expect(rows).toHaveLength(2);
    expect(rows[0].kind === "discussion" && rows[0].verdict?.subject).toBe("A ended");
    expect(rows[1].kind === "discussion" && rows[1].verdict?.subject).toBe("B ended");
    expect(rows[0].kind === "discussion" && rows[0].label).toBe("review A");
    expect(reconcile(messages, feed)).toBe(true);
  });

  it("an R4 landing while muted still retires the key; the unmute derivation marks the verdict", () => {
    const messages = [
      msg(1, { from: "system", type: "moderation", subject: "review A", metadata: { discussion_action: "start" } }),
      msg(2, { from: "system", type: "moderation", metadata: { discussion_action: "auto_round", round: 1 } }),
      msg(3, { from: "moderator:0", subject: "A ended", metadata: { discussion_action: "end", final_round: 1 } }),
      msg(4, { from: "system", type: "moderation", subject: "review B", metadata: { discussion_action: "start" } }),
      msg(5, { from: "system", type: "moderation", metadata: { discussion_action: "auto_round", round: 1 } }),
    ];
    // muted at id 2: the end (3) and B's events accrue silently
    const muted = deriveFeed(messages, 2);
    const mutedRows = muted.rows.filter((r) => r.kind === "discussion");
    expect(mutedRows).toHaveLength(1); // B opens no visible row while muted
    expect(mutedRows[0].kind === "discussion" && mutedRows[0].verdict).toBeNull(); // zero movement
    expect(reconcile(messages, muted)).toBe(true);
    // unmute: catch-up derivation marks A's verdict; B's events stay in catch-up
    const after = deriveFeed(messages, null, { from: 3, to: 5 });
    const rows = after.rows.filter((r) => r.kind === "discussion");
    expect(rows[0].kind === "discussion" && rows[0].verdict?.subject).toBe("A ended");
    expect(reconcile(messages, after)).toBe(true);
  });

  it("an orphan end (no open discussion) lands in the catch-all burst, no crash", () => {
    const messages = [msg(1, { from: "moderator:0", metadata: { discussion_action: "end" } })];
    const feed = deriveFeed(messages, null);
    expect(feed.rows.filter((r) => r.kind === "discussion")).toHaveLength(0);
    expect(feed.rows.filter((r) => r.kind === "burst")).toHaveLength(1);
    expect(reconcile(messages, feed)).toBe(true);
  });
});

describe("deriveDock — card lifecycle (§4.2)", () => {
  const card = (id: number) =>
    msg(id, {
      from: "code-interpreter:0",
      to: "human:0",
      subject: `Decision #${id}`,
      metadata: { choices: [{ id: "a", label: "yes" }] },
    });

  it("one active, queued cards name their blocker", () => {
    const messages = [card(1), card(2)];
    const feed = deriveFeed(messages, null);
    const dock = deriveDock(messages, feed.classified);
    expect(dock[0].status).toBe("active");
    expect(dock[1].status).toBe("queued");
    expect(dock[1].blockedBy).toBe(1);
  });

  it("a human reply with in_reply_to resolves the card (msg 263 shape)", () => {
    const messages = [
      card(1),
      msg(2, { from: "human:0", body: "Finish everything", metadata: { in_reply_to: 1 } }),
      card(3),
    ];
    const feed = deriveFeed(messages, null);
    const dock = deriveDock(messages, feed.classified);
    expect(dock[0].status).toBe("resolved");
    expect(dock[0].resolvedChoice).toBe("Finish everything");
    expect(dock[1].status).toBe("active"); // the queue advances
  });

  it("a string-typed in_reply_to still resolves (msg 282 LOW-2)", () => {
    const messages = [
      card(1),
      msg(2, {
        from: "human:0",
        body: "ok",
        metadata: { in_reply_to: "1" as unknown as number },
      }),
    ];
    const feed = deriveFeed(messages, null);
    const dock = deriveDock(messages, feed.classified);
    expect(dock[0].status).toBe("resolved");
  });
});

describe("seatLiveness — warm zombie never renders healthy", () => {
  const now = Date.parse("2026-06-09T20:00:00Z");
  const fresh = "2026-06-09T19:59:00Z";
  const stale = "2026-06-09T17:00:00Z";
  it("heartbeat fresh + work stale = warm-zombie, not working", () => {
    expect(seatLiveness(fresh, stale, now)).toBe("warm-zombie");
    expect(seatLiveness(fresh, null, now)).toBe("warm-zombie");
  });
  it("both fresh = working; heartbeat stale = dead regardless of work field", () => {
    expect(seatLiveness(fresh, fresh, now)).toBe("working");
    expect(seatLiveness(stale, fresh, now)).toBe("dead");
    expect(seatLiveness(null, null, now)).toBe("dead");
  });
});
