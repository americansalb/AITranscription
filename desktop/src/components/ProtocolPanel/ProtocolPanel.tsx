// ProtocolPanel — section-pinned UI surface that replaces AssemblyBanner
// + scattered roster status dots. Spec §1.1.
//
// One-way data flow (spec §4): we read from useProtocolState, dispatch via
// its mutate(). NEVER setState locally. Refresh comes through the
// `protocol_changed` event listener inside the hook.
//
// 1Hz freshness ticker re-renders SeatChip children with the current
// `now`. Per spec §4.2: freshness is recomputed from Date.now() each tick,
// no `x += dt` accumulation (memory: timer-accumulator-hidden-tab drift).

import { useEffect, useMemo, useState } from 'react';
import { useProtocolState } from '../../hooks/useProtocolState';
import type { Heartbeats, Protocol } from '../../hooks/useProtocolState';
import { SeatChip } from './SeatChip';
import { PhasePlanEditor } from './PhasePlanEditor';
import { HealthPill } from './HealthPill';
import './ProtocolPanel.css';

export type ProtocolPanelProps = {
  projectDir: string | null;
  section: string;
  selfSeat: string | null; // "role:N" of the current viewer (null if human)
  rosterRoles: string[]; // role slugs from project config (legacy — CollabTab owns roster post-#1100)
  defaultExpanded?: boolean; // human #1100: panel collapsed by default; tests + power users may force-expand
};

export function ProtocolPanel({
  projectDir,
  section,
  selfSeat,
  rosterRoles: _rosterRoles,
  defaultExpanded = false,
}: ProtocolPanelProps) {
  // _rosterRoles is intentionally unused after the human #1100 collapse —
  // CollabTab owns the roster surface; ProtocolPanel surfaces only mic +
  // phase + queue (collapsed by default unless `defaultExpanded` set).
  const { state, heartbeats, loaded, lastError, mutate } = useProtocolState(
    projectDir,
    section,
  );

  // 1Hz freshness ticker. Recomputes `now` each tick — never accumulates.
  // Hidden-tab throttling is OK because the next visible tick recomputes
  // from Date.now() (memory entry on timer drift).
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const id = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(id);
  }, []);

  // Slice 9: phase plan editor modal toggle.
  const [editorOpen, setEditorOpen] = useState(false);

  // Human #1100: panel was 80% of screen — collapsed-by-default. User
  // expands via the chevron to see queue/floor mode/health detail.
  const [expanded, setExpanded] = useState(defaultExpanded);

  // ARIA-live announcement of mic transitions. We announce when
  // current_speaker changes — polite (not assertive) per spec §5.2.
  const [announcement, setAnnouncement] = useState<string>('');
  const [prevSpeaker, setPrevSpeaker] = useState<string | null>(null);
  useEffect(() => {
    if (!state) return;
    const cs = state.floor.current_speaker;
    if (cs !== prevSpeaker) {
      if (cs) setAnnouncement(`${cs} now has the mic`);
      else setAnnouncement('Floor cleared — first send claims');
      setPrevSpeaker(cs);
    }
  }, [state, prevSpeaker]);

  if (!loaded || !state) {
    return (
      <section
        className="protocol-panel"
        role="region"
        aria-label={`Protocol panel for section ${section}`}
      >
        <div className="protocol-panel__skeleton">
          {lastError ? `Protocol panel error: ${lastError}` : 'Loading protocol…'}
        </div>
      </section>
    );
  }

  return (
    <section
      className="protocol-panel"
      role="region"
      aria-label={`Protocol panel for section ${section}`}
    >
      <div aria-live="polite" className="visually-hidden" style={{ position: 'absolute', clip: 'rect(0 0 0 0)', clipPath: 'inset(50%)', height: 1, overflow: 'hidden', whiteSpace: 'nowrap', width: 1 }}>
        {announcement}
      </div>

      {/* Compact header (always visible): chevron + mic line + health pill. */}
      <div className="protocol-panel__row" style={{ alignItems: 'center', padding: '6px 0' }}>
        <button
          type="button"
          onClick={() => setExpanded(!expanded)}
          aria-expanded={expanded}
          title="Toggle protocol detail"
          style={{ background: 'transparent', border: 'none', padding: '0 6px', cursor: 'pointer' }}
        >
          {expanded ? '▼' : '▶'}
        </button>
        <CompactMicLine
          protocol={state}
          heartbeats={heartbeats}
          selfSeat={selfSeat}
          now={now}
          mutate={mutate}
        />
        <HealthPill projectDir={projectDir} />
      </div>
      {/* Expandable detail. Roster + SymbolKey REMOVED per human #1100 —
          CollabTab already renders the roster cards with full context. */}
      {expanded && (
        <>
          <PhaseRow protocol={state} mutate={mutate} onEdit={() => setEditorOpen(true)} />
          <ConsensusRow protocol={state} mutate={mutate} />
          <QueueStrip queue={state.floor.queue} />
        </>
      )}
      {editorOpen && (
        <PhasePlanEditor
          protocol={state}
          mutate={mutate}
          onClose={() => setEditorOpen(false)}
        />
      )}
    </section>
  );
}

function PhaseRow({
  protocol,
  mutate,
  onEdit,
}: {
  protocol: Protocol;
  mutate: (action: string, args?: object) => Promise<unknown>;
  onEdit?: () => void;
}) {
  const phases = protocol.phase_plan.phases;
  const idx = protocol.phase_plan.current_phase_idx;
  const total = phases.length;
  const isPaused = protocol.phase_plan.paused_at !== null;

  if (total === 0) {
    return (
      <div className="protocol-panel__row protocol-panel__phase">
        <span style={{ color: '#5b6478', fontStyle: 'italic' }}>
          No phase plan.
        </span>
        {onEdit && (
          <button type="button" className="protocol-panel__pill" onClick={onEdit}>
            ✎ Edit plan
          </button>
        )}
      </div>
    );
  }
  const pct = total > 0 ? Math.round(((idx + 1) / total) * 100) : 0;
  const atEnd = idx >= total;

  return (
    <div className="protocol-panel__row protocol-panel__phase">
      <span>
        <strong>Phase {Math.min(idx + 1, total)} of {total}</strong>
        {isPaused && <span style={{ color: '#f59e0b', marginLeft: 8 }}>⏸ paused</span>}
        {atEnd && <span style={{ color: '#10b981', marginLeft: 8 }}>✓ complete</span>}
      </span>
      <div className="protocol-panel__progress" aria-label={`${pct}% complete`}>
        <div className="protocol-panel__progress-fill" style={{ width: `${pct}%` }} />
      </div>
      <button
        type="button"
        className="protocol-panel__pill"
        onClick={() => { void mutate(isPaused ? 'resume_plan' : 'pause_plan', {}); }}
        disabled={atEnd}
        title={isPaused ? 'Resume the timer' : 'Pause the timer (floor + consensus stay live)'}
      >
        {isPaused ? '▶ resume' : '⏸ pause'}
      </button>
      <button
        type="button"
        className="protocol-panel__pill"
        onClick={() => { void mutate('advance_phase', {}); }}
        disabled={atEnd}
        title="Force-advance to next phase regardless of outcome predicate"
      >
        ⏭ skip
      </button>
      <button
        type="button"
        className="protocol-panel__pill"
        onClick={() => { void mutate('extend_phase', { secs: 900 }); }}
        disabled={atEnd}
        title="Add 15 minutes to current phase duration"
      >
        ⏲ +15m
      </button>
      {onEdit && (
        <button
          type="button"
          className="protocol-panel__pill"
          onClick={onEdit}
          title="Edit phase plan"
        >
          ✎
        </button>
      )}
    </div>
  );
}

// @ts-expect-error TS6133: retained for follow-on detail-view enrichment
function PresetRow({ protocol }: { protocol: Protocol }) {
  return (
    <div className="protocol-panel__row">
      <span><strong>Preset:</strong> {protocol.preset}</span>
      <span><strong>Floor:</strong> {protocol.floor.mode}</span>
      <span><strong>Consensus:</strong> {protocol.consensus.mode}</span>
    </div>
  );
}

/// ConsensusRow — Slice 9 follow-on (closes legacy discussion-status-panel
/// gap per spec §1.1 + human #1062). Renders when a consensus round is
/// active: topic + phase pill + close-round button. Replaces the
/// `discussion-status-panel` JSX block in CollabTab.
function ConsensusRow({
  protocol,
  mutate,
}: {
  protocol: Protocol;
  mutate: (action: string, args?: object) => Promise<unknown>;
}) {
  const round = protocol.consensus.round as
    | { topic?: string; opened_at?: string; opened_by?: string }
    | null;
  const phase = protocol.consensus.phase;

  // Only render when there's an active round to display.
  if (!round || !phase || phase === "closed") return null;

  const phaseLabel =
    phase === "submitting" ? "Submitting" :
    phase === "reviewing" ? "Reviewing" :
    phase;

  return (
    <div className="protocol-panel__row protocol-consensus-row">
      <span style={{ fontSize: '1.05rem' }}>📊</span>
      <span style={{ fontWeight: 600 }}>{round.topic || 'Round in progress'}</span>
      <span className="protocol-panel__pill protocol-consensus-phase">{phaseLabel}</span>
      {round.opened_by && (
        <span className="protocol-mic-preset">opener: {round.opened_by}</span>
      )}
      {phase === "submitting" && (
        <button
          type="button"
          className="protocol-panel__pill protocol-consensus-close-btn"
          onClick={() => { void mutate('close_round', {}); }}
          title="Close this consensus round"
        >
          Close round
        </button>
      )}
    </div>
  );
}

/// CompactMicLine — single-line replacement for MicLine + PresetRow.
/// Designed for the always-visible header row (human #1100: panel was
/// 80% of screen). Shows just: 🎙 [speaker] · [preset] · Yield/[empty].
function CompactMicLine({
  protocol,
  heartbeats,
  selfSeat,
  now,
  mutate,
}: {
  protocol: Protocol;
  heartbeats: Heartbeats;
  selfSeat: string | null;
  now: number;
  mutate: (action: string, args?: object) => Promise<unknown>;
}) {
  const speaker = protocol.floor.current_speaker;
  const isSelfSpeaker = selfSeat !== null && speaker === selfSeat;
  const hb = speaker ? heartbeats[speaker] : undefined;
  const ageSecs = hb && hb.last_active_at_ms ? Math.max(0, Math.floor((now - hb.last_active_at_ms) / 1000)) : null;

  // ui-architect 2026-04-29 (human #1269): rotation strip used to be a
  // 0.78rem inline-flex inside the mic line — invisible at default sizes.
  // Restructured as two stacked rows: (1) the small mic/state line on top,
  // (2) a dedicated horizontal pill row underneath that only renders when
  // AL is on. Each seat gets a real pill (not text). Three named states:
  // current speaker (filled), next-up (outlined), idle (muted).
  const isAssemblyLine = protocol.floor.mode === 'round-robin';
  const rotation = protocol.floor.rotation_order;
  const speakerIdx = speaker ? rotation.indexOf(speaker) : -1;
  const nextUp = isAssemblyLine && rotation.length > 0 && speakerIdx >= 0
    ? rotation[(speakerIdx + 1) % rotation.length]
    : null;

  return (
    <div className="protocol-mic-stack">
      {/* Top line: AL state + speaker + age + Yield (if self) */}
      <div className="protocol-mic-line">
        {isAssemblyLine && (
          <span className="protocol-al-badge" aria-label="Assembly line is on">
            ASSEMBLY LINE
          </span>
        )}
        <span aria-hidden="true" className="protocol-mic-icon">🎙</span>
        <span className="protocol-mic-speaker">
          {speaker || <span className="protocol-mic-idle">idle</span>}
        </span>
        {ageSecs !== null && (
          <span className="protocol-mic-age">{ageSecs}s</span>
        )}
        {!isAssemblyLine && (
          <span className="protocol-mic-preset">· {protocol.preset}</span>
        )}
        {isSelfSpeaker && (
          <button
            type="button"
            className="protocol-mic-yield-btn"
            onClick={() => { void mutate('yield', {}); }}
          >
            Yield
          </button>
        )}
      </div>
      {/* Second line (AL only): horizontal pill row, one pill per seat. */}
      {isAssemblyLine && rotation.length > 0 && (
        <div
          className="protocol-al-rotation"
          role="list"
          aria-label="Assembly line rotation order"
        >
          {rotation.map((seat, i) => {
            const isCurrent = seat === speaker;
            const isNext = nextUp === seat;
            const stateClass = isCurrent
              ? 'is-current'
              : isNext
                ? 'is-next'
                : 'is-idle';
            return (
              <span key={seat} className="protocol-al-rotation-item" role="listitem">
                {i > 0 && <span className="protocol-al-arrow" aria-hidden="true">→</span>}
                <span className={`protocol-al-seat-pill ${stateClass}`}>
                  {seat}
                </span>
              </span>
            );
          })}
        </div>
      )}
    </div>
  );
}

// @ts-expect-error TS6133: replaced by CompactMicLine; retained for detail view
function MicLine({
  protocol,
  heartbeats,
  selfSeat,
  now,
  mutate,
}: {
  protocol: Protocol;
  heartbeats: Heartbeats;
  selfSeat: string | null;
  now: number;
  mutate: (action: string, args?: object) => Promise<unknown>;
}) {
  const speaker = protocol.floor.current_speaker;
  const isSelfSpeaker = selfSeat !== null && speaker === selfSeat;

  if (!speaker) {
    return (
      <div className="protocol-panel__row protocol-panel__mic-line">
        <span className="protocol-panel__mic-icon" aria-hidden="true">🎙</span>
        <span style={{ color: '#5b6478', fontStyle: 'italic' }}>
          No current speaker — first send claims the floor.
        </span>
      </div>
    );
  }

  const hb = heartbeats[speaker];
  const ageSecs = hb && hb.last_active_at_ms ? Math.max(0, Math.floor((now - hb.last_active_at_ms) / 1000)) : null;

  return (
    <div className="protocol-panel__row protocol-panel__mic-line">
      <span className="protocol-panel__mic-icon" aria-hidden="true">🎙</span>
      <span className="protocol-panel__speaker">{speaker}</span>
      {ageSecs !== null && (
        <span style={{ color: '#5b6478', fontSize: '0.9rem' }}>
          active {ageSecs}s ago
        </span>
      )}
      {isSelfSpeaker && (
        <button
          type="button"
          className="protocol-panel__pill"
          onClick={() => { void mutate('yield', {}); }}
          style={{ marginLeft: 'auto', background: '#4f46e5', color: 'white', borderColor: '#4f46e5' }}
        >
          Yield mic
        </button>
      )}
    </div>
  );
}

function QueueStrip({ queue }: { queue: string[] }) {
  if (queue.length === 0) return null;
  return (
    <div className="protocol-panel__row">
      <span>🙋 Queue ({queue.length}):</span>
      <div className="protocol-panel__queue">
        {queue.map((seat, i) => (
          <span key={seat} className="protocol-panel__queue-item">
            {i + 1}. {seat}
          </span>
        ))}
      </div>
    </div>
  );
}

// @ts-expect-error TS6133: REMOVED per human #1100 — CollabTab owns the roster
function Roster({
  protocol,
  heartbeats,
  rosterRoles,
  selfSeat,
  now,
  mutate,
}: {
  protocol: Protocol;
  heartbeats: Heartbeats;
  rosterRoles: string[];
  selfSeat: string | null;
  now: number;
  mutate: (action: string, args?: object) => Promise<unknown>;
}) {
  // Collect all seats — those with heartbeats AND vacant roster roles.
  const seatLabels = useMemo(() => {
    const labels = new Set<string>();
    Object.keys(heartbeats).forEach((k) => labels.add(k));
    rosterRoles.forEach((role) => {
      if (!Array.from(labels).some((l) => l.startsWith(`${role}:`))) {
        labels.add(`${role}:0`); // synthetic vacant seat at instance 0
      }
    });
    return Array.from(labels).sort();
  }, [heartbeats, rosterRoles]);

  return (
    <>
      <div className="protocol-panel__section-header">Roster</div>
      <div className="protocol-panel__row">
        <div className="protocol-panel__roster">
          {seatLabels.map((seat) => {
            const hb = heartbeats[seat];
            const isVacant = !hb;
            const isSelf = selfSeat === seat;
            const onClick = isSelf
              ? () => { void mutate('toggle_queue', {}); }
              : undefined;
            return (
              <SeatChip
                key={seat}
                seatLabel={seat}
                protocol={protocol}
                heartbeat={hb}
                isVacant={isVacant}
                isSelf={isSelf}
                now={now}
                onClick={onClick}
              />
            );
          })}
        </div>
      </div>
    </>
  );
}

// @ts-expect-error TS6133: REMOVED per human #1100 — legend belongs near the roster, not in panel
function SymbolKey() {
  return (
    <div className="protocol-panel__symbol-key">
      <div><b style={{ color: '#4f46e5' }}>🎙 Has the mic</b><br />Currently speaking.</div>
      <div><b style={{ color: '#f59e0b' }}>⚠ Silent past 60s</b><br />Anyone can grab the mic.</div>
      <div><b style={{ color: '#3b82f6' }}>✎ Composing</b><br />Drafting — don't skip.</div>
      <div><b>● Online</b><br />Connected, idle.</div>
      <div><b style={{ color: '#94a3b8' }}>⊘ Disconnected</b><br />Offline.</div>
      <div><b style={{ color: '#94a3b8' }}>· Vacant</b><br />No seat joined.</div>
    </div>
  );
}
