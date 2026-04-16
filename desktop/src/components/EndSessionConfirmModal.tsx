import { useEffect, useRef, useState } from "react";
import "./EndSessionConfirmModal.css";

/**
 * PR H3 v2 — End Session typed-confirm modal.
 *
 * Why: destructive moderator actions must require deliberate intent. Spec
 * `.vaak/specs/pr-h3-moderator-toolbar.md` § "End session" mandates a reason
 * field (≥3 chars) plus a literal "type END" confirmation input, with focus
 * trap + Escape-to-cancel + Enter-to-confirm per WCAG 2.1 AA. This component
 * is the guardrail between the red End button and `invoke("end_discussion")`.
 *
 * Reason is surfaced to onConfirm as a trimmed string; the parent is
 * responsible for broadcasting it to the board before the end call (the
 * existing Tauri `end_discussion` command does not yet accept a reason arg —
 * see developer msg 374 deferred item). When the command signature gains
 * `reason`, the parent can pass it through; this component's contract is
 * unchanged.
 */

const REASON_MIN_LENGTH = 3;
const CONFIRM_WORD = "END";

interface EndSessionConfirmModalProps {
  /** Whether the modal is mounted/visible */
  open: boolean;
  /** Optional context: current discussion topic, surfaced in the dialog title */
  topic?: string;
  /** Called with the trimmed reason when the moderator confirms */
  onConfirm: (reason: string) => void | Promise<void>;
  /** Called when moderator cancels (Escape, backdrop click, Cancel button) */
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
  const reasonRef = useRef<HTMLTextAreaElement | null>(null);
  const dialogRef = useRef<HTMLDivElement | null>(null);
  const previouslyFocused = useRef<HTMLElement | null>(null);

  // Reset state each time the modal opens (not on every render)
  useEffect(() => {
    if (open) {
      setReason("");
      setTypedConfirm("");
      setSubmitting(false);
      previouslyFocused.current = document.activeElement as HTMLElement | null;
      // Defer focus until after paint so screen readers announce the dialog first
      queueMicrotask(() => reasonRef.current?.focus());
    } else if (previouslyFocused.current) {
      // Restore focus to the trigger element when the modal closes
      previouslyFocused.current.focus();
      previouslyFocused.current = null;
    }
  }, [open]);

  if (!open) return null;

  const reasonValid = reason.trim().length >= REASON_MIN_LENGTH;
  const confirmValid = typedConfirm === CONFIRM_WORD;
  const canSubmit = reasonValid && confirmValid && !submitting;

  const handleConfirm = async () => {
    if (!canSubmit) return;
    setSubmitting(true);
    try {
      await onConfirm(reason.trim());
    } finally {
      // Parent controls `open`; leaving submitting=true briefly prevents
      // double-click resubmission if the parent is slow to close us.
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
      onClick={() => !submitting && onCancel()}
      onKeyDown={handleKeyDown}
    >
      <div
        ref={dialogRef}
        className="end-session-modal"
        role="dialog"
        aria-modal="true"
        aria-labelledby="end-session-title"
        aria-describedby="end-session-desc"
        onClick={(e) => e.stopPropagation()}
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
            Reason <span className="end-session-required" aria-hidden="true">*</span>
            <span className="end-session-sr-only"> (required, minimum 3 characters)</span>
          </label>
          <textarea
            id="end-session-reason"
            ref={reasonRef}
            className="end-session-reason"
            value={reason}
            onChange={(e) => setReason(e.target.value)}
            placeholder="Why are you ending this session?"
            rows={3}
            maxLength={500}
            aria-invalid={reason.length > 0 && !reasonValid}
            aria-describedby={reason.length > 0 && !reasonValid ? "end-session-reason-err" : undefined}
          />
          {reason.length > 0 && !reasonValid && (
            <div id="end-session-reason-err" className="end-session-field-err" role="alert">
              Reason must be at least {REASON_MIN_LENGTH} characters.
            </div>
          )}

          <label className="end-session-label" htmlFor="end-session-typed">
            Type <code>END</code> to confirm
          </label>
          <input
            id="end-session-typed"
            className="end-session-typed"
            type="text"
            value={typedConfirm}
            onChange={(e) => setTypedConfirm(e.target.value)}
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
