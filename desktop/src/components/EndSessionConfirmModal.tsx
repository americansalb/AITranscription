import { useEffect, useRef, useState } from "react";
import "./EndSessionConfirmModal.css";

/**
 * PR H3 v2 — End Session confirmation modal.
 *
 * Why: destructive moderator actions still need a guardrail against
 * accidental clicks, but the prior typed-confirm + mandatory-reason gate
 * was too much friction for the common case of a moderator ending their
 * own session (human msg 462). The modal now just asks for explicit
 * confirmation via Cancel/End buttons; a reason textarea is provided
 * but optional — if left blank the parent falls back to
 * DEFAULT_END_REASON so the backend's ≥3-char audit rule still passes.
 *
 * Keeps: focus trap, Escape-to-cancel, Enter-to-confirm, AAA contrast,
 * prefers-reduced-motion honor. Drops: mandatory reason, typed "END"
 * string, backdrop-click dismissal.
 */

interface EndSessionConfirmModalProps {
  /** Whether the modal is mounted/visible */
  open: boolean;
  /** Optional context: current discussion topic, surfaced in the dialog body */
  topic?: string;
  /** Called with the user-typed reason (may be empty). Parent substitutes
   *  a default when blank so the backend audit contract is met. */
  onConfirm: (reason: string) => void | Promise<void>;
  /** Called when moderator cancels (Escape or Cancel button).
   *  Backdrop click does NOT cancel — destructive-confirm dialogs should only
   *  dismiss on explicit intent. */
  onCancel: () => void;
}

export function EndSessionConfirmModal({
  open,
  topic,
  onConfirm,
  onCancel,
}: EndSessionConfirmModalProps) {
  const [reason, setReason] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const confirmBtnRef = useRef<HTMLButtonElement | null>(null);
  const dialogRef = useRef<HTMLDivElement | null>(null);
  const previouslyFocused = useRef<HTMLElement | null>(null);

  // Reset state each time the modal opens (not on every render)
  useEffect(() => {
    if (open) {
      setReason("");
      setSubmitting(false);
      previouslyFocused.current = document.activeElement as HTMLElement | null;
      // Focus lands on the confirm button by default — one-tap close for the
      // common case where the moderator just wants to end. They can Tab to
      // the reason field if they want to add audit context.
      queueMicrotask(() => confirmBtnRef.current?.focus());
    } else if (previouslyFocused.current) {
      previouslyFocused.current.focus();
      previouslyFocused.current = null;
    }
  }, [open]);

  if (!open) return null;

  const handleConfirm = async () => {
    if (submitting) return;
    setSubmitting(true);
    try {
      await onConfirm(reason.trim());
    } finally {
      setSubmitting(false);
    }
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Escape") {
      e.preventDefault();
      if (!submitting) onCancel();
      return;
    }
    if (e.key === "Enter" && !e.shiftKey) {
      // Enter inside the reason textarea should insert newlines, not submit.
      const target = e.target as HTMLElement;
      if (target.tagName === "TEXTAREA") return;
      if (!submitting) {
        e.preventDefault();
        void handleConfirm();
      }
    }
    // Focus trap: cycle Tab within the dialog
    if (e.key === "Tab" && dialogRef.current) {
      const focusable = dialogRef.current.querySelectorAll<HTMLElement>(
        'button:not([disabled]), textarea, input, [tabindex]:not([tabindex="-1"])'
      );
      if (focusable.length === 0) return;
      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      const active = document.activeElement as HTMLElement;
      if (e.shiftKey && active === first) {
        e.preventDefault();
        last.focus();
      } else if (!e.shiftKey && active === last) {
        e.preventDefault();
        first.focus();
      }
    }
  };

  return (
    <div
      className="end-session-overlay"
      onKeyDown={handleKeyDown}
    >
      <div
        ref={dialogRef}
        className="end-session-modal"
        role="dialog"
        aria-modal="true"
        aria-labelledby="end-session-title"
        aria-describedby="end-session-desc"
      >
        <div className="end-session-header">
          <h3 id="end-session-title">End Session?</h3>
        </div>

        <div className="end-session-body">
          <p id="end-session-desc" className="end-session-warning">
            This will close the {topic ? <strong>"{topic}"</strong> : "current"} discussion for everyone.
            Agents bound to the discussion will be stopped. This cannot be undone.
          </p>

          <label className="end-session-label" htmlFor="end-session-reason">
            Reason <span className="end-session-optional">(optional)</span>
          </label>
          <textarea
            id="end-session-reason"
            className="end-session-reason"
            value={reason}
            onChange={(e) => setReason(e.target.value)}
            placeholder="Add audit context — leave blank to use a default"
            rows={2}
            maxLength={500}
          />
        </div>

        <div className="end-session-actions">
          <button
            type="button"
            className="end-session-btn end-session-btn-cancel"
            onClick={onCancel}
            disabled={submitting}
          >
            Cancel
          </button>
          <button
            ref={confirmBtnRef}
            type="button"
            className="end-session-btn end-session-btn-confirm"
            onClick={handleConfirm}
            disabled={submitting}
          >
            {submitting ? "Ending..." : "End Session"}
          </button>
        </div>
      </div>
    </div>
  );
}
