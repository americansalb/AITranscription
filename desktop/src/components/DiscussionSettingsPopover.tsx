// DiscussionSettingsPopover — Phase 1b of fresh-layout v2 (architect msg 468 + 484).
//
// Portal-mounted modal popover that hosts AssemblyControls + ProtocolPanel
// formerly housed in the inline Discussion Mode CollapsibleSection. Opened
// by the ⚙ gear button in the top bar; closes on Escape / click-outside /
// explicit close.
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

export function DiscussionSettingsPopover({
  open,
  onClose,
  children,
  title = "Discussion Mode Settings",
  footer,
}: DiscussionSettingsPopoverProps) {
  const containerRef = useRef<HTMLDivElement | null>(null);

  // Escape closes; focus-trap is light-weight (focuses first focusable on
  // open, returns focus to opener via DOM management at consumer site).
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        onClose();
      }
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [open, onClose]);

  // Focus first focusable on open (light focus-trap).
  useEffect(() => {
    if (!open) return;
    const root = containerRef.current;
    if (!root) return;
    const focusable = root.querySelector<HTMLElement>(
      "button, [href], input, select, textarea, [tabindex]:not([tabindex='-1'])"
    );
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
