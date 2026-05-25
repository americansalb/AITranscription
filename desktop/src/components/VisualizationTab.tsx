import { useEffect, useState } from "react";
import "../styles/visualization.css";
import { useProjectDir } from "../contexts/ProjectDirContext";
import type { ParsedProject, SessionBinding } from "../lib/collabTypes";
import { getRoleColor } from "../utils/roleColors";

/**
 * Visualization Tab — Phase B v1 (B1a shell + B1b roster grid). Per architect
 * spec .vaak/design-notes/2026-05-24-phase-b-visualization-tab-spec.md.
 *
 * B1b lands the default-roster layout: active seats render as labeled cards
 * in a wrap-flow grid. Each card shows name + role title + activity pip + mic
 * indicator + gold/silver/copper balance pill. Subscribes to the same
 * "project-update" Tauri event the CollabTab uses, polls get_assembly_state
 * for the mic-holder, polls get_currency_balances_cmd for the balance pills.
 *
 * Rendering tech: HTML divs (NOT canvas). Architect msg 769 conceded Pixi.js
 * → Canvas2D for v1; ui-architect:0 msg 766 further reasoned that v1 (static
 * grid + text) doesn't even need canvas — HTML divs are accessible by default
 * and zero-cost. Canvas/Pixi.js comes in v2 when Assembly Line rotation
 * animation makes the case.
 */

interface BalanceInfo {
  balance_copper: number;
  escrow_held_copper: number;
  timed_out: boolean;
  initialized: boolean;
  display: { gold: number; silver: number; copper: number };
}

interface AssemblyState {
  active: boolean;
  current_speaker: string | null;
  rotation_order: string[];
}

export function VisualizationTab() {
  const { projectDir } = useProjectDir();
  const [project, setProject] = useState<ParsedProject | null>(null);
  const [balances, setBalances] = useState<Map<string, BalanceInfo>>(new Map());
  const [assembly, setAssembly] = useState<AssemblyState>({ active: false, current_speaker: null, rotation_order: [] });

  useEffect(() => {
    if (!window.__TAURI__ || !projectDir) return;
    let cancelled = false;
    (async () => {
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        const result = await invoke<ParsedProject | null>("watch_project_dir", { dir: projectDir });
        if (!cancelled && result) setProject(result);
      } catch { /* pre-fix binary or no project — degrade silently */ }
    })();

    let unlistenUpdate: (() => void) | undefined;
    (async () => {
      try {
        const { listen } = await import("@tauri-apps/api/event");
        unlistenUpdate = await listen<ParsedProject>("project-update", (e) => {
          if (!cancelled) setProject(e.payload);
        });
      } catch { /* no event API available */ }
    })();

    return () => {
      cancelled = true;
      if (unlistenUpdate) unlistenUpdate();
    };
  }, [projectDir]);

  useEffect(() => {
    if (!window.__TAURI__ || !projectDir) return;
    let cancelled = false;
    const fetchBalances = async () => {
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        const resp = await invoke<{ seats: Array<BalanceInfo & { label: string }> }>(
          "get_currency_balances_cmd",
          { dir: projectDir },
        );
        if (cancelled) return;
        const next = new Map<string, BalanceInfo>();
        for (const s of resp.seats || []) {
          next.set(s.label, {
            balance_copper: s.balance_copper,
            escrow_held_copper: s.escrow_held_copper,
            timed_out: s.timed_out,
            initialized: s.initialized,
            display: s.display,
          });
        }
        setBalances(next);
      } catch { /* pre-currency binary — render without pills */ }
    };
    fetchBalances();
    const interval = setInterval(fetchBalances, 5000);
    return () => { cancelled = true; clearInterval(interval); };
  }, [projectDir]);

  useEffect(() => {
    if (!window.__TAURI__ || !projectDir) return;
    let cancelled = false;
    const poll = async () => {
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        const state = await invoke<AssemblyState>("get_assembly_state", { dir: projectDir });
        if (!cancelled) setAssembly(state);
      } catch { /* pre-assembly binary — no mic ring */ }
    };
    poll();
    const interval = setInterval(poll, 1000);
    return () => { cancelled = true; clearInterval(interval); };
  }, [projectDir]);

  const activeSessions: SessionBinding[] = (project?.sessions ?? [])
    .filter((s) => s.status === "active" && !!s.role && s.role !== "human");

  const seatLabel = (s: SessionBinding) => `${s.role}:${s.instance ?? 0}`;
  const roleTitle = (slug: string) => project?.config.roles?.[slug]?.title ?? slug;

  return (
    <div className="viz-tab">
      <header className="viz-tab-header">
        <span className="viz-tab-mode-badge">Default Roster</span>
        <span className="viz-tab-subtitle">
          {activeSessions.length === 0 ? "No active team members" : `${activeSessions.length} active team member${activeSessions.length === 1 ? "" : "s"}`}
        </span>
      </header>
      <div className="viz-tab-body">
        <div className="viz-canvas" role="region" aria-label="Team roster — bird's-eye view">
          {activeSessions.length === 0 && (
            <div className="viz-canvas-empty">
              <span className="viz-canvas-empty-text">No team members are currently active.</span>
            </div>
          )}
          <div className="viz-roster-grid">
            {activeSessions.map((s) => {
              const label = seatLabel(s);
              const bal = balances.get(label);
              const color = getRoleColor(s.role);
              const isMicHolder = assembly.active && assembly.current_speaker === label;
              const activity = s.activity ?? "idle";
              return (
                <div
                  key={label}
                  className={`viz-avatar-card${isMicHolder ? " viz-avatar-card-mic" : ""}`}
                  style={{ borderColor: `${color}55`, background: `linear-gradient(180deg, ${color}11, transparent)` }}
                  aria-label={`${roleTitle(s.role)} ${s.instance}, ${activity}${isMicHolder ? ", holding the mic" : ""}`}
                >
                  <div className="viz-avatar-puck" style={{ background: color, boxShadow: isMicHolder ? `0 0 0 3px ${color}55, 0 0 12px ${color}aa` : `0 0 0 2px ${color}33` }}>
                    <span className="viz-avatar-puck-initial" aria-hidden="true">
                      {(roleTitle(s.role)[0] || s.role[0] || "?").toUpperCase()}
                    </span>
                  </div>
                  <div className="viz-avatar-meta">
                    <span className="viz-avatar-role" title={label}>{roleTitle(s.role)}</span>
                    <span className="viz-avatar-instance">#{s.instance}</span>
                  </div>
                  <div className={`viz-avatar-pip viz-avatar-pip-${activity}`} title={activity} aria-hidden="true" />
                  {isMicHolder && (
                    <span className="viz-avatar-mic-flag" aria-hidden="true">🎤</span>
                  )}
                  {bal && bal.initialized && (
                    <div className="viz-avatar-balance" aria-label={`balance ${bal.balance_copper} copper`}>
                      {bal.display.gold > 0 && (
                        <span className="viz-avatar-coin">
                          <span className="coin-icon coin-icon-gold" aria-hidden="true" />
                          {bal.display.gold}
                        </span>
                      )}
                      {(bal.display.gold > 0 || bal.display.silver > 0) && (
                        <span className="viz-avatar-coin">
                          <span className="coin-icon coin-icon-silver" aria-hidden="true" />
                          {bal.display.silver}
                        </span>
                      )}
                      <span className="viz-avatar-coin">
                        <span className="coin-icon coin-icon-copper" aria-hidden="true" />
                        {bal.display.copper}
                      </span>
                    </div>
                  )}
                </div>
              );
            })}
          </div>
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
