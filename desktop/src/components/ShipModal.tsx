import { useEffect, useRef, useState } from "react";
import { useModalA11y } from "../hooks/useModalA11y";

/**
 * ShipModal — appears when the builder is ready to ship a commit and needs
 * to name reviewers. Per architect spec at
 * `.vaak/design-notes/2026-05-28-continuous-review-redesign-spec.md` §"UI
 * surface additions" item 2 + human msg 2549 directive.
 *
 * Form: commit SHA + ≥2 reviewer checkboxes (if 2+ non-builder seats
 * available) + optional ship-message body + Ship button. On submit, calls
 * the new `review_ship` Tauri command (pending — landing in developer-lane
 * after Phase 1 hot-reload completes). For now, the submit handler posts
 * the structured ship broadcast directly via `project_send` with
 * `metadata.reviewers: [...]` per the architect spec §"State storage
 * architectural question" Option B (extend board.jsonl).
 *
 * Stub status: UI shape correct; backend `review_ship` MCP tool not yet
 * implemented. Once the tool ships, this modal's submit handler swaps to
 * invoke `review_ship_cmd` instead of the structured-broadcast workaround.
 */

export type ShipSubmit = {
  commit_sha: string;
  named_reviewers: string[];
  body: string;
  timer_secs: number;
};

const TIMER_PRESETS: Array<{ value: number; label: string }> = [
  { value: 30, label: "30 seconds — tight feedback loop" },
  { value: 60, label: "1 minute — default per msg 2549" },
  { value: 120, label: "2 minutes — relaxed" },
  { value: 300, label: "5 minutes — async-style" },
];

export function ShipModal(props: {
  open: boolean;
  projectDir: string;
  builderSeat: string; // e.g. "developer:0"
  activeSeats: string[]; // labels like "manager:0", "architect:0"; INCLUDES builderSeat
  defaultTimerSecs?: number;
  prefilledCommitSha?: string;
  onClose: () => void;
  onShipped?: (config: ShipSubmit) => void;
}) {
  const {
    open,
    projectDir,
    builderSeat,
    activeSeats,
    defaultTimerSecs,
    prefilledCommitSha,
    onClose,
    onShipped,
  } = props;

  const [commitSha, setCommitSha] = useState<string>(prefilledCommitSha ?? "");
  const [reviewers, setReviewers] = useState<Set<string>>(new Set());
  const [body, setBody] = useState<string>("");
  const [timerSecs, setTimerSecs] = useState<number>(defaultTimerSecs ?? 60);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const dialogRef = useRef<HTMLDivElement>(null);
  const commitRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (open) {
      setCommitSha(prefilledCommitSha ?? "");
      setReviewers(new Set());
      setBody("");
      setTimerSecs(defaultTimerSecs ?? 60);
      setError(null);
      setBusy(false);
      const t = setTimeout(() => commitRef.current?.focus(), 0);
      return () => clearTimeout(t);
    }
  }, [open, defaultTimerSecs, prefilledCommitSha]);

  useModalA11y({
    open,
    onClose,
    containerRef: dialogRef,
    closeAllowed: () => !busy,
  });

  if (!open) return null;

  // Eligible reviewers = active seats minus the builder + minus the human.
  // Human is excluded per spec — review is a peer-among-agents activity;
  // human acts via the economic backstop (currency_objection) instead.
  const eligibleReviewers = activeSeats.filter(
    (s) => s !== builderSeat && !s.startsWith("human:"),
  );

  const minReviewersRequired = Math.min(2, eligibleReviewers.length);
  const reviewerCount = reviewers.size;
  const minSatisfied = reviewerCount >= minReviewersRequired;

  const toggleReviewer = (seat: string) => {
    const next = new Set(reviewers);
    if (next.has(seat)) next.delete(seat);
    else next.add(seat);
    setReviewers(next);
  };

  const valid = commitSha.trim().length > 0 && minSatisfied;

  const submit = async () => {
    if (!valid || busy) return;
    setBusy(true);
    setError(null);
    try {
      const { invoke } = await import("@tauri-apps/api/core");

      // Stub: until the `review_ship_cmd` Tauri command lands (developer
      // lane after Phase 1 hot-reload), we post a structured ship broadcast
      // directly via project_send. The new server-side review-window
      // sweeper (SHA-CR.sweeper commit 49cafbf) will pick up the named
      // reviewers from metadata once the backend handler exists.
      await invoke("project_send_cmd", {
        dir: projectDir,
        to: "all",
        type: "ship",
        subject: `SHIPPED ${commitSha.trim()}`,
        body: body.trim() || `Reviewers: ${Array.from(reviewers).join(", ")}.`,
        metadata: {
          extended_thinking: true,
          commit_sha: commitSha.trim(),
          reviewers: Array.from(reviewers),
          review_timer_secs: timerSecs,
        },
      });

      if (onShipped) {
        onShipped({
          commit_sha: commitSha.trim(),
          named_reviewers: Array.from(reviewers),
          body: body.trim(),
          timer_secs: timerSecs,
        });
      }
      onClose();
    } catch (e: any) {
      setError(String(e?.message ?? e));
      setBusy(false);
    }
  };

  return (
    <div className="shm-backdrop" onClick={() => { if (!busy) onClose(); }}>
      <div
        ref={dialogRef}
        className="shm-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="shm-title"
        onClick={(e) => e.stopPropagation()}
      >
        <h3 id="shm-title" className="shm-title">Ship commit + open review window</h3>
        <p className="shm-subtitle">
          Per spec: name at least {minReviewersRequired} reviewer{minReviewersRequired === 1 ? "" : "s"} (the eligible non-builder seats). Named reviewers respond APPROVE / BLOCK / COMMENT within the timer. Silence = APPROVE. <em>currency_objection</em> remains available as the uninvited backstop.
        </p>

        {error && <div className="shm-error">{error}</div>}

        <label className="shm-field">
          <span className="shm-field-label">Commit SHA</span>
          <input
            ref={commitRef}
            className="shm-input"
            type="text"
            value={commitSha}
            onChange={(e) => setCommitSha(e.target.value)}
            placeholder="e.g. 7090f5a"
            spellCheck={false}
          />
        </label>

        <div className="shm-field">
          <span className="shm-field-label">
            Reviewers <span className="shm-count">({reviewerCount}/{eligibleReviewers.length} selected; min {minReviewersRequired})</span>
          </span>
          {eligibleReviewers.length === 0 ? (
            <div className="shm-hint">No other active seats available. Commit will ship unreviewed (allowed per spec when nobody else is online).</div>
          ) : (
            <div className="shm-seat-grid">
              {eligibleReviewers.map((seat) => {
                const checked = reviewers.has(seat);
                return (
                  <label key={seat} className={`shm-seat-pill${checked ? " shm-seat-pill-active" : ""}`}>
                    <input
                      type="checkbox"
                      checked={checked}
                      onChange={() => toggleReviewer(seat)}
                    />
                    {seat}
                  </label>
                );
              })}
            </div>
          )}
        </div>

        <label className="shm-field">
          <span className="shm-field-label">Review window timer</span>
          <select
            className="shm-select"
            value={timerSecs}
            onChange={(e) => setTimerSecs(Number(e.target.value))}
          >
            {TIMER_PRESETS.map((p) => (
              <option key={p.value} value={p.value}>{p.label}</option>
            ))}
          </select>
        </label>

        <label className="shm-field">
          <span className="shm-field-label">Ship body <span className="shm-hint">(optional context for reviewers)</span></span>
          <textarea
            className="shm-textarea"
            value={body}
            onChange={(e) => setBody(e.target.value)}
            placeholder="What changed; what to focus the review on."
            rows={3}
          />
        </label>

        <div className="shm-summary" aria-live="polite">
          <strong>Setup:</strong>{" "}
          commit={commitSha.trim() || <em>(missing)</em>}, reviewers={reviewerCount > 0 ? Array.from(reviewers).join(", ") : <em>none</em>}, timer={timerSecs}s
        </div>

        <div className="shm-actions">
          <button type="button" className="shm-btn shm-btn-cancel" onClick={onClose} disabled={busy}>Cancel</button>
          <button
            type="button"
            className="shm-btn shm-btn-submit"
            onClick={submit}
            disabled={!valid || busy}
            title={!valid
              ? !commitSha.trim()
                ? "Commit SHA required"
                : `Need at least ${minReviewersRequired} reviewer${minReviewersRequired === 1 ? "" : "s"}`
              : `Ship ${commitSha.trim()} and open review window`}
          >
            {busy ? "Shipping…" : "Ship + open review"}
          </button>
        </div>
      </div>
    </div>
  );
}
