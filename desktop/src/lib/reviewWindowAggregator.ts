/**
 * Review-window state aggregation — walks a message stream and builds
 * per-commit ReviewWindowState. Extracted from CollabTab.tsx (SHA-CR.5
 * commit 0d73d5e) so the logic has a single source of truth + can be
 * unit-tested in isolation.
 *
 * Inputs: messages with `type: "ship"` or `type: "review_response"`
 * plus the structured metadata fields agreed in
 * `.vaak/design-notes/2026-05-28-continuous-review-redesign-spec.md`.
 *
 * Output: Map<commit_sha → ReviewWindowState> with outcome + closed_at
 * computed for any window where any BLOCK landed, all named reviewers
 * responded, or the timer expired.
 */

import type { ReviewWindowState, ReviewResponseType } from "../components/ReviewWindow";

/** Minimum shape this module needs from a board message. */
export interface BoardMessageLike {
  type?: string;
  from?: string;
  body?: string;
  timestamp: string; // ISO 8601
  metadata?: Record<string, unknown> | null;
}

/**
 * Build the per-commit review-window state from a chronological message
 * list. `nowMs` is used for timer-expiry computation; callers should pass
 * `Date.now()` in production and a fixed value in tests.
 */
export function buildReviewWindowByCommit(
  messages: BoardMessageLike[],
  nowMs: number = Date.now(),
): Map<string, ReviewWindowState> {
  const map = new Map<string, ReviewWindowState>();

  for (const m of messages) {
    const md = (m.metadata ?? {}) as Record<string, unknown>;
    const commitSha = typeof md.commit_sha === "string" ? md.commit_sha : null;
    if (!commitSha) continue;

    if (m.type === "ship") {
      const reviewers = Array.isArray(md.reviewers)
        ? (md.reviewers as unknown[]).filter((s): s is string => typeof s === "string")
        : [];
      const timerSecs = typeof md.review_timer_secs === "number" ? md.review_timer_secs : 300;
      map.set(commitSha, {
        commit_sha: commitSha,
        builder: m.from ?? "unknown",
        named_reviewers: reviewers,
        responses: [],
        timer_duration_secs: timerSecs,
        opened_at: m.timestamp,
      });
      continue;
    }

    if (m.type === "review_response") {
      const w = map.get(commitSha);
      if (!w) continue; // orphan response — drop silently
      const responseType: ReviewResponseType | null =
        md.response_type === "APPROVE" || md.response_type === "BLOCK" || md.response_type === "COMMENT"
          ? (md.response_type as ReviewResponseType)
          : null;
      if (!responseType) continue;
      w.responses.push({
        seat: m.from ?? "unknown",
        response_type: responseType,
        text: typeof m.body === "string" && m.body.trim().length > 0 ? m.body : undefined,
        at: m.timestamp,
        was_named: md.was_named === true || w.named_reviewers.includes(m.from ?? ""),
      });
    }
  }

  // Outcome computation pass. Mirrors the inline logic at SHA-CR.5 so
  // closed_at + outcome surface immediately without waiting for a
  // server-side close broadcast.
  for (const w of map.values()) {
    const namedBlock = w.responses.find((r) => r.response_type === "BLOCK" && r.was_named);
    if (namedBlock) {
      w.outcome = "blocked";
      w.closed_at = namedBlock.at;
      continue;
    }
    const namedResponded = new Set(
      w.responses.filter((r) => r.was_named).map((r) => r.seat),
    );
    const allNamedResponded =
      w.named_reviewers.length > 0 &&
      w.named_reviewers.every((s) => namedResponded.has(s));
    const openedAtMs = Date.parse(w.opened_at);
    const expired =
      !isNaN(openedAtMs) && (nowMs - openedAtMs) / 1000 >= w.timer_duration_secs;
    if (allNamedResponded || expired) {
      w.outcome = "accepted";
      w.closed_at = expired
        ? new Date(openedAtMs + w.timer_duration_secs * 1000).toISOString()
        : w.responses[w.responses.length - 1]?.at;
    }
  }

  return map;
}
