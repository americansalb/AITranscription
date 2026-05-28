/**
 * Tests for reviewOutcomeFormat — formatBlockReason + formatAcceptedSummary.
 *
 * Pins the human-facing text format for closed review windows so the spec
 * example output ("✓ reviewed by @dev-challenger:0 (APPROVE) @tester:0
 * (APPROVE)" / "✗ BLOCKED by @dev-challenger:0: missing error handling")
 * is regression-safe.
 */
import { describe, it, expect } from "vitest";

import { formatBlockReason, formatAcceptedSummary } from "../lib/reviewOutcomeFormat";
import type { ReviewWindowState } from "../components/ReviewWindow";

const baseState: ReviewWindowState = {
  commit_sha: "abc12345",
  builder: "developer:0",
  named_reviewers: ["dev-challenger:0", "tester:0"],
  responses: [],
  timer_duration_secs: 300,
  opened_at: "2026-05-28T20:00:00Z",
};

describe("formatBlockReason", () => {
  it("returns empty string when no named BLOCK response exists", () => {
    expect(formatBlockReason({ ...baseState, responses: [] })).toBe("");
    expect(
      formatBlockReason({
        ...baseState,
        responses: [
          {
            seat: "dev-challenger:0",
            response_type: "APPROVE",
            at: "2026-05-28T20:01:00Z",
            was_named: true,
          },
        ],
      }),
    ).toBe("");
  });

  it("returns 'BLOCKED by @seat: reason' when a named BLOCK has text", () => {
    expect(
      formatBlockReason({
        ...baseState,
        responses: [
          {
            seat: "dev-challenger:0",
            response_type: "BLOCK",
            text: "missing error handling",
            at: "2026-05-28T20:01:00Z",
            was_named: true,
          },
        ],
      }),
    ).toBe("BLOCKED by @dev-challenger:0: missing error handling");
  });

  it("falls back to 'BLOCKED by @seat' when a named BLOCK has no text", () => {
    expect(
      formatBlockReason({
        ...baseState,
        responses: [
          {
            seat: "dev-challenger:0",
            response_type: "BLOCK",
            at: "2026-05-28T20:01:00Z",
            was_named: true,
          },
        ],
      }),
    ).toBe("BLOCKED by @dev-challenger:0");
  });

  it("trims block reason whitespace", () => {
    expect(
      formatBlockReason({
        ...baseState,
        responses: [
          {
            seat: "dev-challenger:0",
            response_type: "BLOCK",
            text: "  needs error handling  ",
            at: "2026-05-28T20:01:00Z",
            was_named: true,
          },
        ],
      }),
    ).toBe("BLOCKED by @dev-challenger:0: needs error handling");
  });

  it("falls back to 'BLOCKED by @seat' when block reason is whitespace-only", () => {
    expect(
      formatBlockReason({
        ...baseState,
        responses: [
          {
            seat: "dev-challenger:0",
            response_type: "BLOCK",
            text: "   ",
            at: "2026-05-28T20:01:00Z",
            was_named: true,
          },
        ],
      }),
    ).toBe("BLOCKED by @dev-challenger:0");
  });

  it("ignores BLOCK from uninvited reviewer (was_named=false)", () => {
    expect(
      formatBlockReason({
        ...baseState,
        responses: [
          {
            seat: "ux-eng:0",
            response_type: "BLOCK",
            text: "advisory only",
            at: "2026-05-28T20:01:00Z",
            was_named: false,
          },
        ],
      }),
    ).toBe("");
  });

  it("returns the FIRST named BLOCK when multiple exist", () => {
    expect(
      formatBlockReason({
        ...baseState,
        responses: [
          {
            seat: "dev-challenger:0",
            response_type: "BLOCK",
            text: "first reason",
            at: "2026-05-28T20:01:00Z",
            was_named: true,
          },
          {
            seat: "tester:0",
            response_type: "BLOCK",
            text: "second reason",
            at: "2026-05-28T20:02:00Z",
            was_named: true,
          },
        ],
      }),
    ).toBe("BLOCKED by @dev-challenger:0: first reason");
  });
});

describe("formatAcceptedSummary", () => {
  it("annotates each named reviewer with their response type", () => {
    expect(
      formatAcceptedSummary({
        ...baseState,
        responses: [
          {
            seat: "dev-challenger:0",
            response_type: "APPROVE",
            at: "2026-05-28T20:01:00Z",
            was_named: true,
          },
          {
            seat: "tester:0",
            response_type: "APPROVE",
            at: "2026-05-28T20:02:00Z",
            was_named: true,
          },
        ],
      }),
    ).toBe("reviewed by @dev-challenger:0 (APPROVE) @tester:0 (APPROVE)");
  });

  it("annotates silent named reviewers as silence=APPROVE per spec rule", () => {
    expect(
      formatAcceptedSummary({
        ...baseState,
        responses: [
          {
            seat: "dev-challenger:0",
            response_type: "APPROVE",
            at: "2026-05-28T20:01:00Z",
            was_named: true,
          },
          // tester:0 silent — no response
        ],
      }),
    ).toBe("reviewed by @dev-challenger:0 (APPROVE) @tester:0 (silence=APPROVE)");
  });

  it("surfaces COMMENT responses as non-blocking annotation", () => {
    expect(
      formatAcceptedSummary({
        ...baseState,
        responses: [
          {
            seat: "dev-challenger:0",
            response_type: "COMMENT",
            text: "fyi",
            at: "2026-05-28T20:01:00Z",
            was_named: true,
          },
          {
            seat: "tester:0",
            response_type: "APPROVE",
            at: "2026-05-28T20:02:00Z",
            was_named: true,
          },
        ],
      }),
    ).toBe("reviewed by @dev-challenger:0 (COMMENT) @tester:0 (APPROVE)");
  });

  it("returns the no-reviewers fallback when named_reviewers is empty", () => {
    expect(
      formatAcceptedSummary({ ...baseState, named_reviewers: [], responses: [] }),
    ).toBe("reviewed — no named reviewers");
  });

  it("does NOT include uninvited COMMENTs in the summary", () => {
    expect(
      formatAcceptedSummary({
        ...baseState,
        responses: [
          {
            seat: "dev-challenger:0",
            response_type: "APPROVE",
            at: "2026-05-28T20:01:00Z",
            was_named: true,
          },
          {
            seat: "tester:0",
            response_type: "APPROVE",
            at: "2026-05-28T20:02:00Z",
            was_named: true,
          },
          {
            seat: "ux-eng:0",
            response_type: "COMMENT",
            text: "fyi",
            at: "2026-05-28T20:03:00Z",
            was_named: false,
          },
        ],
      }),
    ).toBe("reviewed by @dev-challenger:0 (APPROVE) @tester:0 (APPROVE)");
  });

  it("preserves named_reviewers ordering in the output", () => {
    const out = formatAcceptedSummary({
      ...baseState,
      named_reviewers: ["zeta:0", "alpha:0"],
      responses: [],
    });
    expect(out).toBe("reviewed by @zeta:0 (silence=APPROVE) @alpha:0 (silence=APPROVE)");
  });
});
