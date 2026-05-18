// CollapsibleSection — single source of truth for the collapsible-header
// pattern that emerged across roster-section + claims-section + DecisionPanel
// during layout-density-v1.2 (commits 1c5678d, c115441, 795db42).
//
// Path B (per architect msg 5249 + ui-architect msg 5257 F-UIA-CTR-3 contract)
// extracts the byte-identical chevron + header + aria-expanded + keyboard
// pattern these surfaces all copy-pasted. Foundation for the CollabTab
// restructure (msg 5238 spec), which adds 2 more collapsible bands
// (Discussion Mode card + Team Section) on top of the 3 existing ones.
//
// Contract per F-UIA-CTR-3:
//   - Chevron rotates 200ms ease-out on toggle (CSS transition)
//   - Border-bottom on header ONLY when expanded — collapsed cards visually
//     merge into a single strip; expanded cards declare their boundary
//   - Title renders as <h3> (top-strip project name is h1, within-card
//     labels are h4; h2 reserved/skipped — never stack identical weights
//     across multiple collapsible bands)
//   - Default state derives at call site from content presence (e.g.
//     `collapsed ?? (count === 0)`); wrapper is fully controlled
//
// Controlled API keeps the persistence/derivation logic at the call site
// where the data lives. The wrapper is purely presentational + behavioral
// (toggle, accessibility, animation contract).

import type { ReactNode, KeyboardEvent } from "react";

export interface CollapsibleSectionProps {
  /**
   * Stable identifier used for `aria-controls` + the body element `id`.
   * Each instance must be unique within the document.
   */
  id: string;

  /**
   * Heading content rendered inside the <h3>. Strings or rich nodes
   * (e.g. inline counts, badges) both supported.
   */
  title: ReactNode;

  /**
   * Optional content rendered to the right of the title (counts, status
   * indicators, tag chips). Stays inside the clickable header.
   */
  trailing?: ReactNode;

  /** Controlled collapsed state. Caller owns the source of truth. */
  collapsed: boolean;

  /** Click + keyboard (Enter/Space) handler. Caller toggles state. */
  onToggle: () => void;

  /**
   * Optional extra className for the outer wrapper. Stacks with
   * `collapsible-section` + `collapsible-section-collapsed`.
   */
  className?: string;

  /**
   * Body rendered inside `<div id="{id}-body">`. Not rendered when
   * collapsed (parity with the existing roster-section + claims-section
   * pattern; CSS-driven hide via max-height adds smooth animation but
   * keeps DOM mounted, deferred to a follow-on if needed).
   */
  children: ReactNode;

  /**
   * Optional override for the header `title=""` tooltip. Default is
   * "Expand {title}" / "Collapse {title}" when title is a string;
   * falls back to "Expand section" / "Collapse section" otherwise.
   */
  headerTooltip?: { expand: string; collapse: string };
}

export function CollapsibleSection({
  id,
  title,
  trailing,
  collapsed,
  onToggle,
  className,
  children,
  headerTooltip,
}: CollapsibleSectionProps) {
  const handleKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      onToggle();
    }
  };

  const tooltipExpand =
    headerTooltip?.expand ?? (typeof title === "string" ? `Expand ${title}` : "Expand section");
  const tooltipCollapse =
    headerTooltip?.collapse ?? (typeof title === "string" ? `Collapse ${title}` : "Collapse section");

  const wrapperClass =
    `collapsible-section${collapsed ? " collapsible-section-collapsed" : ""}` +
    (className ? ` ${className}` : "");

  const bodyId = `${id}-body`;

  return (
    <div className={wrapperClass}>
      <div
        className="collapsible-section-header"
        role="button"
        tabIndex={0}
        aria-expanded={!collapsed}
        aria-controls={bodyId}
        onClick={onToggle}
        onKeyDown={handleKeyDown}
        title={collapsed ? tooltipExpand : tooltipCollapse}
      >
        <span className="collapsible-section-chevron" aria-hidden="true">▼</span>
        <h3 className="collapsible-section-title">{title}</h3>
        {trailing ? <span className="collapsible-section-trailing">{trailing}</span> : null}
      </div>
      {!collapsed && (
        <div id={bodyId} className="collapsible-section-body">
          {children}
        </div>
      )}
    </div>
  );
}
