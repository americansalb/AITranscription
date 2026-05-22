// DiscussionSettingsPopover — Phase 1b of fresh-layout v2 (architect msg 468 + 484).
//
// Portal-mounted modal popover that hosts AssemblyControls formerly housed
// in the inline Discussion Mode CollapsibleSection. Opened by the ⚙ gear
// button in the top bar; closes on Escape / click-outside / explicit close.
//
// Why a popover vs the inline collapsible:
// - The Discussion Mode band expanded was ~150px of vertical chrome on
//   every CollabTab render with AL active. Per human msg 237/457 + fresh-
//   layout spec organizing principle (message timeline ≥60% of viewport),
//   the controls should be reachable but not always-rendered.
// - Spec at .vaak/design-notes/collabtab-fresh-layout-2026-05-22.md §
//   "What moves where": Discussion Mode band → ⚙ popover.
// - Always-rendered ~30px discussion-mode strip in CollabTab top bar
//   carries the discoverability surface (preset name + Configure ⚙)
//   per evil-arch msg 390 + architect msg 391 — preserves the msg-5450
//   lesson that hiding controls behind a gear-only surface kills
//   discoverability.
//
// Phase 1b polish (this revision, per dev-chall msg 504 FU1+FU2 + evil-arch msg 509 E2):
// - FU1 — full focus-trap: Tab from last focusable wraps to first; Shift+Tab
//   from first wraps to last. Replaces prior auto-focus-only "light trap"
//   that let keyboard users escape the modal via Tab to browser chrome.
// - FU2 — focus restore to opener on close: captures document.activeElement
//   when popover opens and restores focus when it closes. Keyboard users
//   land back on the Configure ⚙ button instead of body.
// - E2 (separate; in CollabTab.tsx, not here) — ProtocolPanel (force-release,
//   yield, current speaker quick-actions) split OUT of the popover so it
//   stays inline-visible. This popover now hosts AssemblyControls only.

import { useEffect, useRef, type ReactNode } from "react";
import { createPortal } from "react-dom";

export interface DiscussionSettingsPopoverProps {
  open: boolean;
  onClose: () => void;
  children: ReactNode;
  title?: string;
  /** Optional widget rendered in the popover's bottom action row. */
  footer?: ReactNode;
}

const FOCUSABLE_SELECTOR =
  "button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex='-1'])";

export function DiscussionSettingsPopover({
  open,
  onClose,
  children,
  title = "Discussion Mode Settings",
  footer,
}: DiscussionSettingsPopoverProps) {
  const containerRef = useRef<HTMLDivElement | null>(null);
  // FU2 — capture opener so we can restore focus on close. document.activeElement
  // at the moment open flips false→true is the Configure ⚙ button (or whatever
  // element invoked us). Stored in ref so it survives renders without retriggering
  // the effect chain.
  const openerRef = useRef<HTMLElement | null>(null);

  // Escape closes (FU1 unchanged) + FU1 boundary wrap on Tab/Shift+Tab.
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        onClose();
        return;
      }
      if (e.key !== "Tab") return;
      // FU1 — Tab boundary wrap. Without this, Tab from last focusable cycles
      // out to browser chrome (or, with shift, to page content behind the
      // backdrop). Modal contract requires staying inside the dialog.
      const root = containerRef.current;
      if (!root) return;
      const focusables = Array.from(
        root.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR)
      ).filter((el) => !el.hasAttribute("aria-hidden"));
      if (focusables.length === 0) {
        e.preventDefault();
        return;
      }
      const first = focusables[0];
      const last = focusables[focusables.length - 1];
      const active = document.activeElement as HTMLElement | null;
      if (e.shiftKey && active === first) {
        e.preventDefault();
        last.focus();
      } else if (!e.shiftKey && active === last) {
        e.preventDefault();
        first.focus();
      }
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [open, onClose]);

  // FU2 — capture opener on open, restore on close. Effect runs on every
  // open flip; the unmount/close handler restores focus. Guard against
  // restoring to elements that may have been removed (e.g. opener
  // unmounted while popover was open).
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

  // Focus first focusable on open (FU1 entry).
  useEffect(() => {
    if (!open) return;
    const root = containerRef.current;
    if (!root) return;
    const focusable = root.querySelector<HTMLElement>(FOCUSABLE_SELECTOR);
    focusable?.focus();
  }, [open]);

  // Click-outside closes. Uses container ref so synthetic events inside
  // the popover don't trigger close.
  const onBackdropClick = (e: React.MouseEvent) => {
    if (e.target === e.currentTarget) onClose();
  };

  if (!open) return null;

  return createPortal(
    <div
      className="discussion-settings-popover-backdrop"
      onClick={onBackdropClick}
      role="presentation"
    >
      <div
        ref={containerRef}
        className="discussion-settings-popover"
        role="dialog"
        aria-modal="true"
        aria-label={title}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="discussion-settings-popover-header">
          <span className="discussion-settings-popover-title">{title}</span>
          <button
            type="button"
            className="discussion-settings-popover-close"
            onClick={onClose}
            aria-label="Close settings"
            title="Close (Esc)"
          >
            ×
          </button>
        </div>
        <div className="discussion-settings-popover-body">{children}</div>
        {footer && (
          <div className="discussion-settings-popover-footer">{footer}</div>
        )}
      </div>
    </div>,
    document.body
  );
}
