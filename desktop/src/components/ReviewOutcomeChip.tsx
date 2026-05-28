/**
 * ReviewOutcomeChip — compact one-line summary of a closed review window.
 * Per architect spec at `.vaak/design-notes/2026-05-28-continuous-review-
 * redesign-spec.md` §"System tracks per review window" examples:
 *   - "✓ reviewed by @dev-challenger:0 (APPROVE) @tester:0 (APPROVE)"
 *   - "✗ BLOCKED by @dev-challenger:0: missing error handling"
 *
 * Used in place of the full ReviewWindow component on message cards
 * after the review has closed (accepted / blocked outcome reached).
 * The full ReviewWindow is for OPEN windows where action buttons matter;
 * this chip is the after-the-fact record.
 */

import type { ReviewWindowState } from "./ReviewWindow";
import {
  formatBlockReason,
  formatAcceptedSummary,
} from "../lib/reviewOutcomeFormat";

export function ReviewOutcomeChip(props: {
  state: ReviewWindowState;
}) {
  const { state } = props;

  if (!state.outcome) return null;

  const isBlocked = state.outcome === "blocked";
  const summary = isBlocked
    ? formatBlockReason(state)
    : formatAcceptedSummary(state);

  return (
    <div
      className={`review-outcome-chip review-outcome-chip--${state.outcome}`}
      title={`Review of ${state.commit_sha.slice(0, 8)} — ${state.outcome}`}
    >
      <span className="review-outcome-chip-mark" aria-hidden="true">
        {isBlocked ? "✗" : "✓"}
      </span>
      <span className="review-outcome-chip-text">{summary}</span>
    </div>
  );
}
