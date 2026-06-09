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
  it("while muted only R1/R2 render; the rest accrues to one catch-up row", () => {
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
    const feed = deriveFeed(messages, 1); // muted at id 1
    const kinds = feed.rows.map((r) => r.kind);
    expect(kinds).toContain("card"); // R2 passes the overlay
    expect(kinds.filter((k) => k === "message")).toHaveLength(2); // msg1 (pre-mute) + human
    const catchup = feed.rows.find((r) => r.kind === "burst" && r.key === "muted-catchup");
    expect(catchup && catchup.kind === "burst" && catchup.count).toBe(1);
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
