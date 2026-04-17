import { useEffect, useRef, useState } from "react";
import { getRoleColor } from "../utils/roleColors";
import "./ModeratorPicker.css";

/**
 * PR H3 — moderator-designation control.
 *
 * A small popover triggered from the "Change" affordance next to the
 * current moderator badge on DiscussionPanel. Lists participants of
 * the active session as moderator candidates. Clicking one fires
 * `onSelect(role, instance)` which the parent wires to
 * invoke("set_session_moderator", { dir, role, instance }).
 *
 * Keyboard: arrow keys cycle candidates, Enter selects, Escape closes.
 * Focus returns to the triggering button on close.
 *
 * Errors from the invoke are surfaced via the existing
 * showModeratorError toast channel — no error UI inside this component.
 */

export interface ModeratorPickerCandidate {
  /** role:instance label, e.g. "developer:0" */
  id: string;
  /** role slug for color lookup, e.g. "developer" */
  role: string;
  /** instance number for the invoke call */
  instance: number;
}

interface ModeratorPickerProps {
  /** Is the popover open? Parent owns this. */
  open: boolean;
  /** Close request (Escape, backdrop click, after successful select) */
  onClose: () => void;
  /** All candidate role:instance participants available to designate */
  candidates: ModeratorPickerCandidate[];
  /** Currently-designated moderator id ("role:instance"), highlighted in the list */
  currentModeratorId: string | null;
  /** Called when the user picks a new moderator. Parent handles the Tauri invoke. */
  onSelect: (role: string, instance: number) => void | Promise<void>;
}

export function ModeratorPicker({
  open,
  onClose,
  candidates,
  currentModeratorId,
  onSelect,
}: ModeratorPickerProps) {
  const [focusedIndex, setFocusedIndex] = useState(0);
  const [submitting, setSubmitting] = useState(false);
  const menuRef = useRef<HTMLDivElement | null>(null);
  const buttonRefs = useRef<(HTMLButtonElement | null)[]>([]);

  useEffect(() => {
    if (!open) {
      setSubmitting(false);
      return;
    }
    // On open, focus the current moderator row if present, else the first row.
    const currentIdx = candidates.findIndex(c => c.id === currentModeratorId);
    const initial = currentIdx >= 0 ? currentIdx : 0;
    setFocusedIndex(initial);
    queueMicrotask(() => buttonRefs.current[initial]?.focus());
  }, [open, candidates, currentModeratorId]);

  if (!open) return null;

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Escape") {
      e.preventDefault();
      if (!submitting) onClose();
      return;
    }
    if (e.key === "ArrowDown") {
      e.preventDefault();
      const next = (focusedIndex + 1) % candidates.length;
      setFocusedIndex(next);
      buttonRefs.current[next]?.focus();
      return;
    }
    if (e.key === "ArrowUp") {
      e.preventDefault();
      const next = (focusedIndex - 1 + candidates.length) % candidates.length;
      setFocusedIndex(next);
      buttonRefs.current[next]?.focus();
      return;
    }
  };

  const handlePick = async (candidate: ModeratorPickerCandidate) => {
    if (submitting) return;
    if (candidate.id === currentModeratorId) {
      // Picking the same person is a no-op; just close.
      onClose();
      return;
    }
    setSubmitting(true);
    try {
      await onSelect(candidate.role, candidate.instance);
      onClose();
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <>
      {/* Transparent backdrop captures outside clicks without darkening the
          page — this is a small popover, not a destructive modal. */}
      <div
        className="moderator-picker-backdrop"
        onClick={() => !submitting && onClose()}
      />
      <div
        ref={menuRef}
        className="moderator-picker-menu"
        role="menu"
        aria-label="Change moderator"
        onKeyDown={handleKeyDown}
      >
        <div className="moderator-picker-header">Designate moderator</div>
        {candidates.length === 0 ? (
          <div className="moderator-picker-empty">No eligible candidates</div>
        ) : (
          candidates.map((c, i) => {
            const isCurrent = c.id === currentModeratorId;
            return (
              <button
                key={c.id}
                ref={(el) => { buttonRefs.current[i] = el; }}
                type="button"
                role="menuitemradio"
                aria-checked={isCurrent}
                className={`moderator-picker-item${isCurrent ? " moderator-picker-item-current" : ""}`}
                onClick={() => handlePick(c)}
                disabled={submitting}
              >
                <span
                  className="moderator-picker-swatch"
                  style={{ background: getRoleColor(c.role) }}
                  aria-hidden="true"
                />
                <span className="moderator-picker-label">{c.id}</span>
                {isCurrent && (
                  <span className="moderator-picker-current-tag" aria-hidden="true">current</span>
                )}
              </button>
            );
          })
        )}
      </div>
    </>
  );
}
