import { useEffect, useState } from "react";
import type { Seat } from "./types";
import { loadSeatsOnce } from "./seatsLoader";
import "../../styles/collaborate-v2.css";

// Single source of truth for the flow list shown in P1 orientation copy.
// Changing a row here updates both the roadmap list and any future picker.
// Makes path-(b) "skeleton in UI, spec frozen" cheap to reverse if v0.5
// reshuffles (per dev-challenger:0 id 404 + evil-architect:0 id 396 risk log).
type FlowStub = {
  name: string;
  phase: string;
  deferred?: boolean;
  ariaHint: string;
};

const FLOW_STUBS: FlowStub[] = [
  { name: "Assembly Line", phase: "P3a", ariaHint: "ships in P3a" },
  { name: "Round Robin", phase: "P3a", ariaHint: "ships in P3a" },
  { name: "Oxford", phase: "P5", ariaHint: "ships in P5" },
  { name: "Delphi", phase: "P5", ariaHint: "ships in P5" },
  { name: "Debate", phase: "post-v0.5", deferred: true, ariaHint: "deferred to post-v0.5, mechanics not yet specified" },
  { name: "Open", phase: "P1 (default)", ariaHint: "default in P1" },
];

// Collaborate v2 — P1 scope per COLLABORATE_V2_SPEC.html §18 + §20.
// Shell + static roster only. No wire, no gating, no live updates.
// Everything else (mic / flow / claims / decisions / admin drawer / roles modal)
// lands in P3a / P3b / P3c / P5 per the phase-mark on each §A.2.x surface.

export function CollaborateV2App() {
  const [seats, setSeats] = useState<Seat[]>([]);
  const [loaded, setLoaded] = useState(false);
  const [projectDir, setProjectDir] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      const dir = await resolveProjectDir();
      if (cancelled) return;
      setProjectDir(dir);
      if (dir) {
        await updateWindowTitle(dir);
        const rows = await loadSeatsOnce(dir);
        if (!cancelled) setSeats(rows);
      }
      if (!cancelled) setLoaded(true);
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  return (
    <div className="cv2-root">
      <Header projectDir={projectDir} />
      <div className="cv2-body">
        <TeamSidebar seats={seats} loaded={loaded} projectDir={projectDir} />
        <MessageFeed projectDir={projectDir} />
        <RightSidebarPlaceholder />
      </div>
      <BottomBar />
    </div>
  );
}

function Header({ projectDir }: { projectDir: string | null }) {
  const projectName = projectDir ? projectDir.split(/[\\/]/).filter(Boolean).pop() : null;
  return (
    <header className="cv2-header" role="banner">
      <div className="cv2-header-title">
        <div className="cv2-header-title-main">Collaborate v2 <span className="cv2-phase-pill">P1 · shell</span></div>
        <div className="cv2-header-title-sub">
          {projectName ? (
            <>Project: <code>{projectName}</code> · <span className="cv2-muted">{projectDir}</span></>
          ) : (
            <span className="cv2-muted">No project active — open a project in the main Vaak window first</span>
          )}
        </div>
      </div>
      <div className="cv2-header-controls" aria-label="Header controls — most actions ship in later phases">
        <span className="cv2-flow-pill" aria-label="Current flow">Open · 0 seats active</span>
        <span className="cv2-work-pill" aria-label="Work mode">Planning only</span>
        <button className="cv2-header-btn" disabled aria-label="Pass mic — not wired yet, ships with the mic mechanism in phase three-a">Pass mic ▾</button>
        <button className="cv2-header-btn" disabled aria-label="Designate a moderator — not wired yet, ships in phase five alongside the admin drawer">Moderator: —</button>
        <button className="cv2-header-btn" disabled aria-label="Open the roles modal — not wired yet, ships in phase five">👥 Roles…</button>
        <button className="cv2-header-btn" disabled aria-label="Open the admin drawer — not wired yet, ships in phase five">⚙ Settings</button>
      </div>
    </header>
  );
}

function TeamSidebar({ seats, loaded, projectDir }: { seats: Seat[]; loaded: boolean; projectDir: string | null }) {
  return (
    <aside className="cv2-team-sidebar" aria-label="Team roster">
      <h2 className="cv2-section-label">Team {seats.length > 0 ? `· ${seats.length} seats` : ""}</h2>
      {!loaded ? (
        <div className="cv2-empty-roster">Loading…</div>
      ) : !projectDir ? (
        <div className="cv2-empty-roster">No project active.</div>
      ) : seats.length === 0 ? (
        <div className="cv2-empty-roster">
          <p>No seats yet.</p>
          <p>Roster reads <code>.vaak/v2/seats.json</code> — it's empty until P2 wires the launcher to create v2 seats.</p>
        </div>
      ) : (
        <ul className="cv2-seat-list">
          {seats.map((seat) => (
            <li key={`${seat.role}-${seat.instance}`} className="cv2-seat">
              <div className="cv2-seat-name">{seat.role}:{seat.instance}</div>
              <span className={`cv2-pill cv2-pill-${pillClass(seat)}`}>{pillLabel(seat)}</span>
            </li>
          ))}
        </ul>
      )}
    </aside>
  );
}

function MessageFeed({ projectDir }: { projectDir: string | null }) {
  return (
    <main className="cv2-feed" aria-label="Message feed">
      <h2 className="cv2-section-label">Message feed</h2>
      <div className="cv2-feed-orient">
        <h3 className="cv2-feed-h2">This is the P1 shell.</h3>
        <p>You clicked <em>Collaborate ✨</em> and a standalone window opened — that's the P1 deliverable per §18 Phase Plan in <code>COLLABORATE_V2_SPEC.html</code>.</p>
        <h3 className="cv2-feed-subhead">Ships today</h3>
        <p>The shell shows what ships today:</p>
        <ul>
          <li><strong>Three-column layout</strong> — team on the left, feed in the middle, decisions panel on the right.</li>
          <li><strong>Project awareness</strong> — {projectDir ? <>reading <code>{projectDir}</code>.</> : <>no project active yet.</>}</li>
          <li><strong>Static roster</strong> — reads <code>.vaak/v2/seats.json</code> once on mount.</li>
          <li><strong>Disabled header controls</strong> — flow pill, work toggle, pass-mic, roles, settings are visible as stubs.</li>
        </ul>
        <h3 className="cv2-feed-subhead">Deferred to later phases</h3>
        <p>What's deliberately NOT here yet (deferred to later phases):</p>
        <ul>
          <li><strong>P2</strong> — launcher v2 with <code>agent_id</code> minting, terminal spawning, reconnect logic.</li>
          <li><strong>P3a</strong> — Assembly Line + Round Robin flow, speaker token, <code>project_send_v2</code> gate, messaging.</li>
          <li><strong>P3b</strong> — Work-Allowed toggle live, claim mechanism (edit-mode wire-enforced).</li>
          <li><strong>P3c</strong> — Decisions Panel actually getting cards from the board.</li>
          <li><strong>P5</strong> — Admin drawer (⚙), roles modal (👥), active-claims modal (full view).</li>
          <li><strong>post-v0.5</strong> — Debate flow (structured debate with real mechanics: motion, stances, rebuttal, rounds — architect owns the rule set, not drafted yet).</li>
        </ul>
        <p className="cv2-muted">Per Ground Rule 8 of v0.4, none of those ship until this shell lands and gets real-use feedback.</p>
        <h3 className="cv2-feed-subhead">Flow picker options</h3>
        <p id="cv2-flow-list-label">Flows planned for the Flow picker (still disabled in P1):</p>
        <dl className="cv2-flow-stubs" aria-labelledby="cv2-flow-list-label">
          {FLOW_STUBS.map((flow) => (
            <div key={flow.name} className="cv2-flow-stub" role="listitem">
              <dt>{flow.name}</dt>
              <dd aria-label={flow.ariaHint}>
                <span className={`cv2-phase-tag${flow.deferred ? " cv2-phase-tag-deferred" : ""}`}>{flow.phase}</span>
              </dd>
            </div>
          ))}
        </dl>
      </div>
    </main>
  );
}

function RightSidebarPlaceholder() {
  return (
    <aside className="cv2-right-sidebar" aria-label="At-a-glance panels">
      <section className="cv2-right-panel" aria-label="Decisions panel (ships in P3c)">
        <h2 className="cv2-section-label">Needs your decision <span className="cv2-phase-tag">P3c</span></h2>
        <div className="cv2-right-placeholder">
          Multiple-choice cards appear here when agents need your call — claim conflicts, orphaned mic, new-seat approvals, recusal replacements.
        </div>
      </section>

      <section className="cv2-right-panel" aria-label="Active claims quick view (ships in P3b)">
        <h2 className="cv2-section-label">Active claims <span className="cv2-phase-tag">P3b</span></h2>
        <div className="cv2-right-placeholder">
          File claims (who's reading or editing what) surface here. Click through for the full claims modal.
        </div>
      </section>

      <section className="cv2-right-panel" aria-label="Moderator panel (ships in P5)">
        <h2 className="cv2-section-label">Moderator <span className="cv2-phase-tag cv2-phase-tag-deferred">P5</span></h2>
        <div className="cv2-right-placeholder">
          Moderator controls (timebox, grant mic, yank, pause) appear here when you're the designated moderator.
        </div>
      </section>

      <section className="cv2-right-panel" aria-label="Round stats (ships in P4)">
        <h2 className="cv2-section-label">Round so far <span className="cv2-phase-tag cv2-phase-tag-deferred">P4</span></h2>
        <div className="cv2-right-placeholder">
          Live stats: messages sent, mic holds, avg hold time, longest hold.
        </div>
      </section>

      <button
        type="button"
        className="cv2-override-ribbon"
        disabled
        aria-label="Human override — not wired yet, ships with the mic mechanism in phase three-a"
      >
        <span className="cv2-override-ribbon-main">⚡ Human override available · 1-click</span>
        <span className="cv2-muted cv2-tiny">(Ships in P3a alongside the mic mechanism.)</span>
      </button>
    </aside>
  );
}

function BottomBar() {
  return (
    <footer className="cv2-bottom-bar" role="contentinfo">
      <div className="cv2-input-disabled" aria-label="Message input — not wired yet, ships with the mic mechanism in phase three-a">
        Sending messages ships in P3a. This shell is read-only.
      </div>
      <button className="cv2-send-disabled" disabled>Send</button>
    </footer>
  );
}

// Read pills per §8.2 state / transport_bound pair-read. P1 only shows the
// visible flavor; the pair-read becomes material once the heartbeat reaper
// ships in P2.
function pillClass(seat: Seat): "active" | "standby" | "disconnected" {
  if (seat.state === "active") return "active";
  if (seat.state === "standby") return "standby";
  return "disconnected";
}

function pillLabel(seat: Seat): string {
  if (seat.state === "active") return "Active";
  if (seat.state === "standby") return seat.transport_bound ? "Reconnecting" : "Held for reclaim";
  return "Disconnected";
}

// Resolve the active project directory: query string → localStorage → main-window
// Tauri command. In this window (opened via `toggle_collaborate_v2_window`), no
// query param is passed, so the backend `get_project_path` is the canonical
// source. This is the same function the main window uses to remember what's
// currently open; P2+ may replace it with a v2-scoped picker.
async function resolveProjectDir(): Promise<string | null> {
  const params = new URLSearchParams(window.location.search);
  const fromQuery = params.get("projectDir");
  if (fromQuery) return fromQuery;
  try {
    const cached = localStorage.getItem("collab:projectDir");
    if (cached) return cached;
  } catch {
    // localStorage can throw in restricted contexts; fall through.
  }
  if (!window.__TAURI__) return null;
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    const result = await invoke<string | null>("get_project_path");
    return result ?? null;
  } catch {
    return null;
  }
}

async function updateWindowTitle(projectDir: string): Promise<void> {
  if (!window.__TAURI__) return;
  try {
    const { getCurrentWindow } = await import("@tauri-apps/api/window");
    const w = getCurrentWindow();
    const name = projectDir.split(/[\\/]/).filter(Boolean).pop() ?? projectDir;
    await w.setTitle(`Collaborate — ${name}`);
  } catch {
    // Non-fatal; title just stays as the tauri.conf.json default.
  }
}
