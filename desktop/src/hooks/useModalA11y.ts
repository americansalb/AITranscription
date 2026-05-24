import { useEffect, useRef, type RefObject } from "react";

/**
 * Shared a11y behaviors for dialog modals — applied to AdjustBalanceModal,
 * EconomySettingsModal, and OxfordSetupModal so they all meet the same
 * keyboard-navigation contract per ui-architect msg 641/698/729:
 *
 *   1. **Escape closes** (unless the caller's `closeAllowed()` returns false
 *      — modals use this to prevent close during a busy/save round-trip).
 *   2. **Tab is trapped within the dialog.** Tab from the last focusable wraps
 *      to the first; Shift+Tab from the first wraps to the last. Without this,
 *      keyboard users can Tab out to browser chrome / content behind the
 *      backdrop — violates the modal contract.
 *   3. **First focusable receives focus on open**, unless the modal already
 *      managed its own focus (e.g., AdjustBalanceModal focuses the amount
 *      input on mount). The hook only focuses if no descendant of `containerRef`
 *      currently owns focus.
 *   4. **Focus is restored to the opener on close** — captured at open time
 *      via document.activeElement so keyboard users land back on the trigger
 *      button instead of body.
 *
 * Pattern extracted from DiscussionSettingsPopover (Phase 1b polish FU1+FU2,
 * dev-chall msg 504 + evil-arch msg 509). Same selector + restore-guard logic.
 */
const FOCUSABLE_SELECTOR =
  "button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex='-1'])";

export function useModalA11y(opts: {
  open: boolean;
  onClose: () => void;
  containerRef: RefObject<HTMLElement>;
  /** Block Escape close (e.g., return `busy` so a save round-trip can't be aborted). Default: always allowed. */
  closeAllowed?: () => boolean;
}): void {
  const { open, onClose, containerRef, closeAllowed } = opts;
  const openerRef = useRef<HTMLElement | null>(null);

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        if (closeAllowed && !closeAllowed()) return;
        e.stopPropagation();
        onClose();
        return;
      }
      if (e.key !== "Tab") return;
      const root = containerRef.current;
      if (!root) return;
      const focusables = Array.from(
        root.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR),
      ).filter((el) => !el.hasAttribute("aria-hidden") && el.offsetParent !== null);
      if (focusables.length === 0) {
        e.preventDefault();
        return;
      }
      const first = focusables[0];
      const last = focusables[focusables.length - 1];
      const active = document.activeElement as HTMLElement | null;
      if (e.shiftKey && (active === first || !root.contains(active))) {
        e.preventDefault();
        last.focus();
      } else if (!e.shiftKey && (active === last || !root.contains(active))) {
        e.preventDefault();
        first.focus();
      }
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [open, onClose, closeAllowed, containerRef]);

  useEffect(() => {
    if (!open) {
      const opener = openerRef.current;
      if (opener && document.body.contains(opener)) {
        opener.focus();
      }
      openerRef.current = null;
      return;
    }
    openerRef.current = document.activeElement as HTMLElement | null;
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const root = containerRef.current;
    if (!root) return;
    if (root.contains(document.activeElement)) return;
    const focusable = root.querySelector<HTMLElement>(FOCUSABLE_SELECTOR);
    focusable?.focus();
  }, [open, containerRef]);
}
