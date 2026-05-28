/**
 * Review-outcome text formatting. Extracted from ReviewOutcomeChip.tsx
 * so the formatting rules are testable + reusable.
 *
 * Per architect spec at `.vaak/design-notes/2026-05-28-continuous-review-
 * redesign-spec.md` §"System tracks per review window" examples:
 *   - "✓ reviewed by @dev-challenger:0 (APPROVE) @tester:0 (APPROVE)"
 *   - "✗ BLOCKED by @dev-challenger:0: missing error handling"
 */

import type { ReviewWindowState } from "../components/ReviewWindow";

/**
 * Block reason text for a blocked window. Returns "" if no named BLOCK
 * is found (caller should treat empty as "no block info available").
 */
export function formatBlockReason(state: ReviewWindowState): string {
  const blocker = state.responses.find(
    (r) => r.response_type === "BLOCK" && r.was_named,
  );
  if (!blocker) return "";
  const seat = blocker.seat;
  const text = blocker.text?.trim();
  return text ? `BLOCKED by @${seat}: ${text}` : `BLOCKED by @${seat}`;
}

/**
 * Accepted-outcome summary listing each named reviewer + their response
 * type, with silence-from-named annotated as `(silence=APPROVE)` to
 * surface the spec rule.
 */
export function formatAcceptedSummary(state: ReviewWindowState): string {
  const namedResponses = state.named_reviewers.map((seat) => {
    const r = state.responses.find((x) => x.seat === seat);
    if (!r) return `@${seat} (silence=APPROVE)`;
    if (r.response_type === "APPROVE") return `@${seat} (APPROVE)`;
    if (r.response_type === "COMMENT") return `@${seat} (COMMENT)`;
    return `@${seat} (${r.response_type})`;
  });
  return namedResponses.length > 0
    ? `reviewed by ${namedResponses.join(" ")}`
    : "reviewed — no named reviewers";
}
