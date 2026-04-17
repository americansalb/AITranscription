import { useEffect, useRef, useState } from "react";
import "./EndSessionConfirmModal.css";

/**
 * PR H3 v2 — End Session confirmation modal.
 *
 * Why: destructive moderator actions need a misclick guard. Per architect
 * msg 480 (post-human-feedback in msg 462), the right friction is:
 * typed "END" confirmation (prevents single-touchpad-tap from wiping
 * session state), reason textarea strictly OPTIONAL (parent substitutes
 * DEFAULT_END_REASON on empty so the backend's ≥3-char audit rule still
 * passes — see CollabTab.tsx doEndDiscussion).
 *
 * Keeps: focus trap, Escape cancel, AAA contrast, prefers-reduced-motion
 * honor, no backdrop dismissal.
 */

const CONFIRM_WORD = "END";

interface EndSessionConfirmModalProps {
  /** Whether the modal is mounted/visible */
  open: boolean;
  /** Optional context: current discussion topic, surfaced in the dialog body */
  topic?: string;
  /** Called with the user-typed reason (may be empty). Parent MUST substitute
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
  const [typedConfirm, setTypedConfirm] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const confirmBtnRef = useRef<HTMLButtonElement | null>(null);
  const dialogRef = useRef<HTMLDivElement | null>(null);
  const previouslyFocused = useRef<HTMLElement | null>(null);

  // Reset state each time the modal opens (not on every render)
  useEffect(() => {
    if (open) {
      setReason("");
      setTypedConfirm("");
      setSubmitting(false);
      previouslyFocused.current = document.activeElement as HTMLElement | null;
      // Focus lands on the red End Session button by default (architect msg 480)
      // — gives immediate visual cue of the destructive action. User types
      // END in the confirm field (Shift+Tab twice back into it) or clicks
      // to target the field directly.
      queueMicrotask(() => confirmBtnRef.current?.focus());
    } else if (previouslyFocused.current) {
      previouslyFocused.current.focus();
      previouslyFocused.current = null;
    }
  }, [open]);

  if (!open) return null;

  const confirmValid = typedConfirm === CONFIRM_WORD;
  const canSubmit = confirmValid && !submitting;

  const handleConfirm = async () => {
    if (!canSubmit) return;
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
      if (canSubmit) {
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

          <label className="end-session-label" htmlFor="end-session-typed">
            Type <code>END</code> to confirm
          </label>
          <input
            id="end-session-typed"
            className="end-session-typed"
            type="text"
            value={typedConfirm}
            /* Normalize case + strip surrounding whitespace so the visual
               `text-transform: uppercase` matches the stored value — typing
               "end " would otherwise leave the button disabled despite the
               on-screen text reading "END". */
            onChange={(e) => setTypedConfirm(e.target.value.toUpperCase().trim())}
            autoComplete="off"
            spellCheck={false}
            aria-invalid={typedConfirm.length > 0 && !confirmValid}
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
            disabled={!canSubmit}
          >
            {submitting ? "Ending..." : "End Session"}
          </button>
        </div>
      </div>
    </div>
  );
}
