/**
 * Tests for reviewWindowAggregator — walks a message stream and builds
 * per-commit ReviewWindowState. Pins the SHA-CR.5 aggregation semantics
 * against regressions.
 */
import { describe, it, expect } from "vitest";

import {
  buildReviewWindowByCommit,
  type BoardMessageLike,
} from "../lib/reviewWindowAggregator";

const T0 = "2026-05-28T20:00:00Z";
const T0_MS = Date.parse(T0);

describe("buildReviewWindowByCommit — opens window on ship", () => {
  it("returns empty map when no ship messages exist", () => {
    const map = buildReviewWindowByCommit([], T0_MS);
    expect(map.size).toBe(0);
  });

  it("opens a window for each ship message", () => {
    const msgs: BoardMessageLike[] = [
      {
        type: "ship",
        from: "developer:0",
        timestamp: T0,
        metadata: { commit_sha: "abc123", reviewers: ["tester:0", "dev-challenger:0"], review_timer_secs: 300 },
      },
    ];
    const map = buildReviewWindowByCommit(msgs, T0_MS);
    expect(map.size).toBe(1);
    const w = map.get("abc123")!;
    expect(w.commit_sha).toBe("abc123");
    expect(w.builder).toBe("developer:0");
    expect(w.named_reviewers).toEqual(["tester:0", "dev-challenger:0"]);
    expect(w.timer_duration_secs).toBe(300);
    expect(w.responses).toEqual([]);
    expect(w.outcome).toBeUndefined();
    expect(w.closed_at).toBeUndefined();
  });

  it("ignores ship messages without a commit_sha", () => {
    const msgs: BoardMessageLike[] = [
      { type: "ship", from: "dev:0", timestamp: T0, metadata: { reviewers: ["a:0"] } },
    ];
    expect(buildReviewWindowByCommit(msgs, T0_MS).size).toBe(0);
  });

  it("defaults timer to 300s when metadata.review_timer_secs missing", () => {
    const msgs: BoardMessageLike[] = [
      { type: "ship", from: "dev:0", timestamp: T0, metadata: { commit_sha: "x", reviewers: [] } },
    ];
    expect(buildReviewWindowByCommit(msgs, T0_MS).get("x")!.timer_duration_secs).toBe(300);
  });

  it("filters non-string entries from metadata.reviewers", () => {
    const msgs: BoardMessageLike[] = [
      {
        type: "ship",
        from: "dev:0",
        timestamp: T0,
        metadata: { commit_sha: "x", reviewers: ["tester:0", 42, null, "dev-challenger:0"] as unknown as string[] },
      },
    ];
    expect(buildReviewWindowByCommit(msgs, T0_MS).get("x")!.named_reviewers).toEqual([
      "tester:0",
      "dev-challenger:0",
    ]);
  });
});

describe("buildReviewWindowByCommit — accumulates review responses", () => {
  it("attaches APPROVE/BLOCK/COMMENT responses by commit_sha", () => {
    const msgs: BoardMessageLike[] = [
      { type: "ship", from: "dev:0", timestamp: T0, metadata: { commit_sha: "x", reviewers: ["tester:0", "dc:0"], review_timer_secs: 300 } },
      {
        type: "review_response",
        from: "tester:0",
        body: "looks good",
        timestamp: "2026-05-28T20:01:00Z",
        metadata: { commit_sha: "x", response_type: "APPROVE", was_named: true },
      },
      {
        type: "review_response",
        from: "dc:0",
        body: "missing error handling",
        timestamp: "2026-05-28T20:02:00Z",
        metadata: { commit_sha: "x", response_type: "BLOCK", was_named: true },
      },
      {
        type: "review_response",
        from: "ui-arch:0",
        body: "fyi",
        timestamp: "2026-05-28T20:03:00Z",
        metadata: { commit_sha: "x", response_type: "COMMENT" },
      },
    ];
    const w = buildReviewWindowByCommit(msgs, T0_MS).get("x")!;
    expect(w.responses).toHaveLength(3);
    expect(w.responses[0]).toEqual({
      seat: "tester:0",
      response_type: "APPROVE",
      text: "looks good",
      at: "2026-05-28T20:01:00Z",
      was_named: true,
    });
    expect(w.responses[1].response_type).toBe("BLOCK");
    expect(w.responses[2]).toEqual({
      seat: "ui-arch:0",
      response_type: "COMMENT",
      text: "fyi",
      at: "2026-05-28T20:03:00Z",
      was_named: false,
    });
  });

  it("auto-flags was_named when seat is in the named_reviewers list", () => {
    const msgs: BoardMessageLike[] = [
      { type: "ship", from: "dev:0", timestamp: T0, metadata: { commit_sha: "x", reviewers: ["tester:0"], review_timer_secs: 300 } },
      {
        type: "review_response",
        from: "tester:0",
        timestamp: "2026-05-28T20:01:00Z",
        metadata: { commit_sha: "x", response_type: "APPROVE" }, // was_named missing
      },
    ];
    expect(buildReviewWindowByCommit(msgs, T0_MS).get("x")!.responses[0].was_named).toBe(true);
  });

  it("drops orphan responses without a preceding ship message", () => {
    const msgs: BoardMessageLike[] = [
      {
        type: "review_response",
        from: "tester:0",
        timestamp: T0,
        metadata: { commit_sha: "abc", response_type: "APPROVE", was_named: true },
      },
    ];
    expect(buildReviewWindowByCommit(msgs, T0_MS).size).toBe(0);
  });

  it("drops responses with unknown response_type", () => {
    const msgs: BoardMessageLike[] = [
      { type: "ship", from: "dev:0", timestamp: T0, metadata: { commit_sha: "x", reviewers: ["tester:0"] } },
      {
        type: "review_response",
        from: "tester:0",
        timestamp: "2026-05-28T20:01:00Z",
        metadata: { commit_sha: "x", response_type: "NOPE", was_named: true },
      },
    ];
    expect(buildReviewWindowByCommit(msgs, T0_MS).get("x")!.responses).toHaveLength(0);
  });

  it("omits body text when message body is empty string", () => {
    const msgs: BoardMessageLike[] = [
      { type: "ship", from: "dev:0", timestamp: T0, metadata: { commit_sha: "x", reviewers: ["t:0"] } },
      {
        type: "review_response",
        from: "t:0",
        body: "   ",
        timestamp: "2026-05-28T20:01:00Z",
        metadata: { commit_sha: "x", response_type: "APPROVE" },
      },
    ];
    expect(buildReviewWindowByCommit(msgs, T0_MS).get("x")!.responses[0].text).toBeUndefined();
  });
});

describe("buildReviewWindowByCommit — outcome computation", () => {
  const open = (sha: string, reviewers: string[], timer = 300): BoardMessageLike => ({
    type: "ship",
    from: "dev:0",
    timestamp: T0,
    metadata: { commit_sha: sha, reviewers, review_timer_secs: timer },
  });
  const respond = (
    sha: string,
    seat: string,
    type: "APPROVE" | "BLOCK" | "COMMENT",
    at: string,
    was_named = true,
  ): BoardMessageLike => ({
    type: "review_response",
    from: seat,
    body: "",
    timestamp: at,
    metadata: { commit_sha: sha, response_type: type, was_named },
  });

  it("blocked outcome on any named BLOCK", () => {
    const map = buildReviewWindowByCommit(
      [
        open("x", ["t:0", "dc:0"]),
        respond("x", "t:0", "APPROVE", "2026-05-28T20:01:00Z"),
        respond("x", "dc:0", "BLOCK", "2026-05-28T20:02:00Z"),
      ],
      T0_MS,
    );
    const w = map.get("x")!;
    expect(w.outcome).toBe("blocked");
    expect(w.closed_at).toBe("2026-05-28T20:02:00Z");
  });

  it("accepted outcome when all named reviewers respond non-blocking", () => {
    const map = buildReviewWindowByCommit(
      [
        open("x", ["t:0", "dc:0"]),
        respond("x", "t:0", "APPROVE", "2026-05-28T20:01:00Z"),
        respond("x", "dc:0", "COMMENT", "2026-05-28T20:02:00Z"),
      ],
      T0_MS,
    );
    const w = map.get("x")!;
    expect(w.outcome).toBe("accepted");
    expect(w.closed_at).toBe("2026-05-28T20:02:00Z");
  });

  it("accepted outcome when timer expires with no responses", () => {
    const map = buildReviewWindowByCommit(
      [open("x", ["t:0"], 60)],
      Date.parse(T0) + 61_000, // 1s after timer
    );
    const w = map.get("x")!;
    expect(w.outcome).toBe("accepted");
    expect(w.closed_at).toBe(new Date(Date.parse(T0) + 60_000).toISOString());
  });

  it("remains open when timer has not expired and not all named responded", () => {
    const map = buildReviewWindowByCommit(
      [
        open("x", ["t:0", "dc:0"]),
        respond("x", "t:0", "APPROVE", "2026-05-28T20:01:00Z"),
      ],
      T0_MS + 10_000, // 10s in; timer 300s
    );
    const w = map.get("x")!;
    expect(w.outcome).toBeUndefined();
    expect(w.closed_at).toBeUndefined();
  });

  it("BLOCK from uninvited reviewer does NOT block the window (advisory only)", () => {
    const map = buildReviewWindowByCommit(
      [
        open("x", ["t:0"]),
        respond("x", "uninvited:0", "BLOCK", "2026-05-28T20:01:00Z", false),
        respond("x", "t:0", "APPROVE", "2026-05-28T20:02:00Z"),
      ],
      T0_MS,
    );
    const w = map.get("x")!;
    expect(w.outcome).toBe("accepted");
  });

  it("no-named-reviewers window does NOT auto-close on all-responded heuristic", () => {
    // Edge: 0 named reviewers + no timer expiry → window stays open
    // (spec says commit ships unreviewed when nobody else is online; the
    // close happens at timer-expiry, not at responded-count).
    const map = buildReviewWindowByCommit(
      [open("x", [], 60)],
      T0_MS + 10_000, // 10s in; timer 60s
    );
    const w = map.get("x")!;
    expect(w.outcome).toBeUndefined();
  });
});
