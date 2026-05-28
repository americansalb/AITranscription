/**
 * ReviewWindow — appears on a commit message card while a review window
 * is open. Per architect spec at
 * `.vaak/design-notes/2026-05-28-continuous-review-redesign-spec.md` §"UI
 * surface additions" item 3 + human msg 2549 directive.
 *
 * Shows timer countdown, named reviewers + their response status
 * (pending / approved / blocked / commented), uninvited comment count.
 *
 * Named reviewers see APPROVE / BLOCK / COMMENT buttons.
 * Uninvited see only COMMENT (advisory; non-blocking).
 *
 * Stub status: UI shape correct; backend `review_respond_cmd` MCP tool
 * not yet implemented. Submit handlers post structured response broadcasts
 * via project_send_cmd as the canonical input the new tool will consume
 * when developer-lane lands it post-Phase-1.
 */
import { useEffect, useState } from "react";

export type ReviewResponseType = "APPROVE" | "BLOCK" | "COMMENT";

export interface ReviewResponse {
  seat: string;
  response_type: ReviewResponseType;
  text?: string;
  at: string; // ISO timestamp
  was_named: boolean;
}

export interface ReviewWindowState {
  commit_sha: string;
  builder: string;
  named_reviewers: string[];
  responses: ReviewResponse[];
  timer_duration_secs: number;
  opened_at: string; // ISO timestamp
  closed_at?: string;
  outcome?: "accepted" | "blocked";
}

function formatRemaining(remainingSecs: number): string {
  if (remainingSecs <= 0) return "expired";
  if (remainingSecs >= 60) {
    const m = Math.floor(remainingSecs / 60);
    const s = remainingSecs % 60;
    return s === 0 ? `${m}m` : `${m}m${s}s`;
  }
  return `${remainingSecs}s`;
}

export function ReviewWindow(props: {
  state: ReviewWindowState;
  /** Current viewer's seat label (e.g., "tester:0"). */
  viewerSeat: string;
  projectDir: string;
  /**
   * Called after a response submit succeeds. Caller is responsible for
   * re-fetching the latest window state so the UI reflects the new
   * response.
   */
  onResponded?: (response: ReviewResponse) => void;
}) {
  const { state, viewerSeat, projectDir, onResponded } = props;

  const [now, setNow] = useState<number>(() => Date.now());
  const [commentText, setCommentText] = useState<string>("");
  const [blockText, setBlockText] = useState<string>("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (state.closed_at) return; // no countdown when closed
    const id = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(id);
  }, [state.closed_at]);

  const openedAtMs = Date.parse(state.opened_at);
  const elapsedSecs = Math.max(0, Math.floor((now - openedAtMs) / 1000));
  const remainingSecs = state.closed_at
    ? 0
    : Math.max(0, state.timer_duration_secs - elapsedSecs);

  const viewerIsNamed = state.named_reviewers.includes(viewerSeat);
  const viewerIsBuilder = viewerSeat === state.builder;
  const viewerAlreadyResponded = state.responses.some((r) => r.seat === viewerSeat);

  // Per spec: APPROVE / BLOCK only available to named reviewers; uninvited
  // can COMMENT only.
  const canApprove = viewerIsNamed && !viewerAlreadyResponded && !state.closed_at;
  const canBlock = viewerIsNamed && !viewerAlreadyResponded && !state.closed_at;
  const canComment = !viewerIsBuilder && !state.closed_at;

  const namedReviewerStatuses = state.named_reviewers.map((seat) => {
    const response = state.responses.find((r) => r.seat === seat);
    return { seat, response };
  });

  const uninvitedComments = state.responses.filter(
    (r) => !r.was_named && r.response_type === "COMMENT",
  );

  const submitResponse = async (
    response_type: ReviewResponseType,
    text?: string,
  ) => {
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      // Stub: until `review_respond_cmd` Tauri command lands, post a
      // structured response broadcast directly via project_send. The
      // SHA-CR.sweeper-driven server-side close logic will pick up the
      // response from board events once the backend handler exists.
      await invoke("project_send_cmd", {
        dir: projectDir,
        to: "all",
        type: "review_response",
        subject: `${response_type} review of ${state.commit_sha}`,
        body: text ?? "",
        metadata: {
          extended_thinking: true,
          commit_sha: state.commit_sha,
          response_type,
          was_named: viewerIsNamed,
        },
      });
      const synthetic: ReviewResponse = {
        seat: viewerSeat,
        response_type,
        text,
        at: new Date().toISOString(),
        was_named: viewerIsNamed,
      };
      if (onResponded) onResponded(synthetic);
      setCommentText("");
      setBlockText("");
    } catch (e: any) {
      setError(String(e?.message ?? e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className={`review-window${state.closed_at ? " review-window-closed" : ""}`}>
      <div className="review-window-header">
        <span className="review-window-title">
          Review · {state.commit_sha.slice(0, 8)} · by {state.builder}
        </span>
        <span className="review-window-timer" aria-live="polite">
          {state.closed_at
            ? state.outcome === "accepted"
              ? "✓ accepted"
              : state.outcome === "blocked"
                ? "✗ blocked"
                : "closed"
            : `${formatRemaining(remainingSecs)} remaining`}
        </span>
      </div>

      {error && <div className="review-window-error">{error}</div>}

      <div className="review-window-named-reviewers">
        {namedReviewerStatuses.length === 0 ? (
          <span className="review-window-empty-named">No named reviewers (commit ships unreviewed).</span>
        ) : (
          namedReviewerStatuses.map(({ seat, response }) => (
            <span
              key={seat}
              className={`review-window-named-pill review-window-named-pill--${response?.response_type?.toLowerCase() ?? "pending"}`}
              title={response?.text ? `${seat}: ${response.text}` : seat}
            >
              {response?.response_type === "APPROVE" && "✓ "}
              {response?.response_type === "BLOCK" && "✗ "}
              {response?.response_type === "COMMENT" && "💬 "}
              {!response && "○ "}
              {seat}
            </span>
          ))
        )}
      </div>

      {uninvitedComments.length > 0 && (
        <div className="review-window-uninvited">
          + {uninvitedComments.length} uninvited comment{uninvitedComments.length === 1 ? "" : "s"}
        </div>
      )}

      {!state.closed_at && (canApprove || canBlock || canComment) && (
        <div className="review-window-actions">
          {canApprove && (
            <button
              type="button"
              className="review-window-btn review-window-btn-approve"
              onClick={() => submitResponse("APPROVE")}
              disabled={busy}
              title={`Approve ${state.commit_sha.slice(0, 8)}`}
            >
              ✓ Approve
            </button>
          )}
          {canBlock && (
            <details className="review-window-block-details">
              <summary className="review-window-btn review-window-btn-block">✗ Block</summary>
              <div className="review-window-block-form">
                <textarea
                  className="review-window-block-input"
                  placeholder="Reason for blocking — required."
                  value={blockText}
                  onChange={(e) => setBlockText(e.target.value)}
                  rows={2}
                />
                <button
                  type="button"
                  className="review-window-btn review-window-btn-block-submit"
                  onClick={() => submitResponse("BLOCK", blockText.trim())}
                  disabled={busy || blockText.trim().length === 0}
                >
                  Submit block
                </button>
              </div>
            </details>
          )}
          {canComment && (
            <details className="review-window-comment-details">
              <summary className="review-window-btn review-window-btn-comment">💬 Comment</summary>
              <div className="review-window-comment-form">
                <textarea
                  className="review-window-comment-input"
                  placeholder={viewerIsNamed ? "Comment (advisory, doesn't change outcome)" : "Uninvited comment (advisory only)"}
                  value={commentText}
                  onChange={(e) => setCommentText(e.target.value)}
                  rows={2}
                />
                <button
                  type="button"
                  className="review-window-btn review-window-btn-comment-submit"
                  onClick={() => submitResponse("COMMENT", commentText.trim())}
                  disabled={busy || commentText.trim().length === 0}
                >
                  Submit comment
                </button>
              </div>
            </details>
          )}
        </div>
      )}
    </div>
  );
}
