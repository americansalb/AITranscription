import { useEffect, useRef, useState } from "react";
import "../styles/visualization.css";
import { useProjectDir } from "../contexts/ProjectDirContext";
import type { ParsedProject, SessionBinding, BoardMessage } from "../lib/collabTypes";
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

interface Popup {
  id: number;
  text: string;
  tone: "earn" | "decay" | "loss" | "grant" | "passive" | "tune";
  born: number;
}

interface CurrencyEventRow {
  id: number;
  type?: string;
  seat?: string;
  amount?: number;
}

const POPUP_TTL_MS = 2000;
const POPUP_CAP_PER_SEAT = 5;

function classifyEvent(type: string | undefined, amount: number): { text: string; tone: Popup["tone"] } | null {
  if (!type) return null;
  const abs = Math.abs(amount);
  const sign = amount >= 0 ? "+" : "−";
  switch (type) {
    case "decay":
      return { text: `decay −${abs}c`, tone: "decay" };
    case "human_adjust":
      return amount >= 0
        ? { text: `+${abs}c grant`, tone: "grant" }
        : { text: `−${abs}c grant`, tone: "loss" };
    case "passive":
    case "interest":
      return amount === 0 ? null : { text: `${sign}${abs}c`, tone: "passive" };
    case "economy_tune":
      return { text: "tuned", tone: "tune" };
    default:
      if (amount === 0) return null;
      return amount > 0
        ? { text: `+${abs}c`, tone: "earn" }
        : { text: `−${abs}c`, tone: "loss" };
  }
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

  // B1c — currency event popups. Polls read_currency_events_stream every 1.5s
  // with a moving cursor. New rows render as floating popups over the
  // corresponding avatar card; popups expire after POPUP_TTL_MS. The first
  // call advances the cursor to the current tail so we don't dump the entire
  // historical ledger as popups on mount.
  const [popupsBySeat, setPopupsBySeat] = useState<Map<string, Popup[]>>(new Map());
  const cursorRef = useRef<number>(-1);
  useEffect(() => {
    if (!window.__TAURI__ || !projectDir) return;
    let cancelled = false;
    let interval: ReturnType<typeof setInterval> | null = null;

    const seedCursor = async () => {
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        const resp = await invoke<{ last_txn_id: number }>(
          "read_currency_events_stream",
          { dir: projectDir, sinceTxnId: 0 },
        );
        if (!cancelled) cursorRef.current = resp.last_txn_id ?? 0;
      } catch { cursorRef.current = 0; }
    };

    const poll = async () => {
      if (cursorRef.current < 0) return;
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        const resp = await invoke<{ rows: CurrencyEventRow[]; last_txn_id: number }>(
          "read_currency_events_stream",
          { dir: projectDir, sinceTxnId: cursorRef.current },
        );
        if (cancelled) return;
        if (resp.last_txn_id > cursorRef.current) cursorRef.current = resp.last_txn_id;
        if (!resp.rows || resp.rows.length === 0) return;
        const now = Date.now();
        setPopupsBySeat((prev) => {
          const next = new Map(prev);
          for (const r of resp.rows) {
            if (!r.seat) continue;
            const c = classifyEvent(r.type, r.amount ?? 0);
            if (!c) continue;
            const popup: Popup = { id: r.id, text: c.text, tone: c.tone, born: now };
            const existing = next.get(r.seat) ?? [];
            const capped = existing.length >= POPUP_CAP_PER_SEAT
              ? existing.slice(existing.length - POPUP_CAP_PER_SEAT + 1)
              : existing;
            next.set(r.seat, [...capped, popup]);
          }
          return next;
        });
      } catch { /* pre-events-stream binary — no popups */ }
    };

    (async () => {
      await seedCursor();
      if (!cancelled) {
        interval = setInterval(poll, 1500);
      }
    })();

    return () => {
      cancelled = true;
      if (interval) clearInterval(interval);
    };
  }, [projectDir]);

  // Cleanup expired popups every 500ms (separate from poll so popups age out
  // even when no new events fire).
  useEffect(() => {
    const interval = setInterval(() => {
      setPopupsBySeat((prev) => {
        const now = Date.now();
        const next = new Map<string, Popup[]>();
        let changed = false;
        for (const [seat, popups] of prev) {
          const fresh = popups.filter((p) => now - p.born < POPUP_TTL_MS);
          if (fresh.length !== popups.length) changed = true;
          if (fresh.length > 0) next.set(seat, fresh);
        }
        return changed ? next : prev;
      });
    }, 500);
    return () => clearInterval(interval);
  }, []);

  const activeSessions: SessionBinding[] = (project?.sessions ?? [])
    .filter((s) => s.status === "active" && !!s.role && s.role !== "human");

  const seatLabel = (s: SessionBinding) => `${s.role}:${s.instance ?? 0}`;
  const roleTitle = (slug: string) => project?.config.roles?.[slug]?.title ?? slug;

  const RECENT_MESSAGE_COUNT = 20;
  const recentMessages: BoardMessage[] = (project?.messages ?? []).slice(-RECENT_MESSAGE_COUNT);

  const sidePanelRef = useRef<HTMLDivElement>(null);
  const lastSeenIdRef = useRef<number>(0);
  useEffect(() => {
    const el = sidePanelRef.current;
    if (!el || recentMessages.length === 0) return;
    const newestId = recentMessages[recentMessages.length - 1].id;
    if (newestId === lastSeenIdRef.current) return;
    lastSeenIdRef.current = newestId;
    requestAnimationFrame(() => {
      el.scrollTop = el.scrollHeight;
    });
  }, [recentMessages]);

  const formatSender = (msg: BoardMessage): { name: string; color: string } => {
    if (msg.from === "human:0" || msg.from === "human") return { name: "human", color: "#f5c518" };
    if (msg.from === "system" || msg.from.startsWith("system")) return { name: "system", color: "#8899a6" };
    const slug = msg.from.split(":")[0];
    return { name: msg.from, color: getRoleColor(slug) };
  };

  const previewBody = (body: string, max = 110): string => {
    const stripped = body.replace(/\n+/g, " ").trim();
    return stripped.length > max ? `${stripped.slice(0, max - 1)}…` : stripped;
  };

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
                  {(popupsBySeat.get(label) ?? []).map((p, idx) => (
                    <span
                      key={p.id}
                      className={`viz-popup viz-popup-${p.tone}`}
                      style={{ animationDelay: `${idx * 80}ms` }}
                      aria-hidden="true"
                    >
                      {p.text}
                    </span>
                  ))}
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
        <aside className="viz-side-panel" aria-label="Recent team chat">
          <header className="viz-side-panel-header">
            Recent chat
            <span className="viz-side-panel-count">({recentMessages.length})</span>
          </header>
          <div className="viz-side-panel-body" ref={sidePanelRef}>
            {recentMessages.length === 0 ? (
              <p className="viz-side-panel-placeholder">
                No messages on the board yet.
              </p>
            ) : (
              <ul className="viz-chat-list">
                {recentMessages.map((msg) => {
                  const { name, color } = formatSender(msg);
                  return (
                    <li key={msg.id} className="viz-chat-row">
                      <span
                        className="viz-chat-sender"
                        style={{ color, borderLeftColor: color }}
                      >
                        {name}
                      </span>
                      {msg.subject && (
                        <span className="viz-chat-subject">{msg.subject}</span>
                      )}
                      <span className="viz-chat-body">{previewBody(msg.body)}</span>
                    </li>
                  );
                })}
              </ul>
            )}
          </div>
        </aside>
      </div>
    </div>
  );
}
