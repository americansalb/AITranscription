/**
 * ConfirmDialog — Reusable confirmation dialog for destructive actions.
 * Supports optional typed confirmation (e.g., type project name to delete).
 */

import { useCallback, useState } from "react";
import { useFocusTrap } from "../hooks/useFocusTrap";

interface ConfirmDialogProps {
  title: string;
  message: string;
  confirmLabel?: string;
  /** If set, user must type this exact text to confirm */
  requireTyped?: string;
  variant?: "danger" | "warning" | "default";
  onConfirm: () => void;
  onCancel: () => void;
}

export function ConfirmDialog({
  title,
  message,
  confirmLabel = "Confirm",
  requireTyped,
  variant = "default",
  onConfirm,
  onCancel,
}: ConfirmDialogProps) {
  const [typed, setTyped] = useState("");
  const closeHandler = useCallback(() => onCancel(), [onCancel]);
  const modalRef = useFocusTrap(true, closeHandler);

  const canConfirm = requireTyped ? typed === requireTyped : true;
  const btnClass = variant === "danger" ? "btn btn-danger" : "btn btn-primary";

  return (
    <div
      className="modal-backdrop"
      onClick={(e) => { if (e.target === e.currentTarget) onCancel(); }}
      role="dialog"
      aria-modal="true"
      aria-label={title}
    >
      <div className="modal" ref={modalRef} style={{ maxWidth: 420 }}>
        <div className="modal-header">
          <h2 className="modal-title">{title}</h2>
          <button className="btn btn-ghost" onClick={onCancel} aria-label="Close">
            {"\u2715"}
          </button>
        </div>

        <div style={{ marginBottom: "var(--space-3)", color: "var(--text-secondary)", fontSize: "var(--text-sm)" }}>
          {message}
        </div>

        {requireTyped && (
          <div className="field" style={{ marginBottom: "var(--space-3)" }}>
            <label className="field-label" htmlFor="confirm-typed">
              Type <strong>{requireTyped}</strong> to confirm
            </label>
            <input
              id="confirm-typed"
              className="input"
              value={typed}
              onChange={(e) => setTyped(e.target.value)}
              placeholder={requireTyped}
              autoFocus
              autoComplete="off"
            />
          </div>
        )}

        <div style={{ display: "flex", gap: "var(--space-2)", justifyContent: "flex-end" }}>
          <button className="btn btn-ghost" onClick={onCancel}>Cancel</button>
          <button className={btnClass} onClick={onConfirm} disabled={!canConfirm}>
            {confirmLabel}
          </button>
        </div>
      </div>
    </div>
  );
}
