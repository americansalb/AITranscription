import "../styles/visualization.css";

/**
 * Visualization Tab — Phase B v1 shell (B1a). Per architect spec at
 * .vaak/design-notes/2026-05-24-phase-b-visualization-tab-spec.md.
 *
 * B1a (this commit): tab shell only — header, empty canvas placeholder, side
 * panel skeleton. No data binding, no rendering. Lands the navigation surface
 * so subsequent commits (B1b roster grid, B1c currency popups, B1d side-panel
 * chat) can ship as small independently-reviewable diffs.
 *
 * Rendering-tech decision (Pixi.js vs Canvas2D-vanilla) is deferred to B1b
 * where the choice actually fires — architect msg 755 leans Pixi.js per spec
 * §11.1; ui-architect:0 msg 766 leans Canvas2D-vanilla for v1 YAGNI. B1a
 * imposes neither.
 */

export function VisualizationTab() {
  return (
    <div className="viz-tab">
      <header className="viz-tab-header">
        <span className="viz-tab-mode-badge">Default Roster</span>
        <span className="viz-tab-subtitle">Bird's-eye view of the team — Phase B v1 shell</span>
      </header>
      <div className="viz-tab-body">
        <div className="viz-canvas-placeholder" role="img" aria-label="Visualization canvas — content lands in B1b">
          <span className="viz-canvas-placeholder-text">
            Roster grid renders here (B1b)
          </span>
        </div>
        <aside className="viz-side-panel" aria-label="Active chat and currency events">
          <header className="viz-side-panel-header">Side panel</header>
          <div className="viz-side-panel-body">
            <p className="viz-side-panel-placeholder">
              Active chat scroll + recent currency events (B1d).
            </p>
          </div>
        </aside>
      </div>
    </div>
  );
}
