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
  rosterRoles: string[]; // role slugs from project config (for vacancy detection)
};

export function ProtocolPanel({
  projectDir,
  section,
  selfSeat,
  rosterRoles,
}: ProtocolPanelProps) {
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

      <div style={{ display: 'flex', alignItems: 'flex-start', gap: 8 }}>
        <div style={{ flex: 1 }}>
          <PhaseRow protocol={state} mutate={mutate} onEdit={() => setEditorOpen(true)} />
        </div>
        <HealthPill projectDir={projectDir} />
      </div>
      {editorOpen && (
        <PhasePlanEditor
          protocol={state}
          mutate={mutate}
          onClose={() => setEditorOpen(false)}
        />
      )}
      <PresetRow protocol={state} />
      <ConsensusRow protocol={state} mutate={mutate} />
      <MicLine
        protocol={state}
        heartbeats={heartbeats}
        selfSeat={selfSeat}
        now={now}
        mutate={mutate}
      />
      <QueueStrip queue={state.floor.queue} />
      <Roster
        protocol={state}
        heartbeats={heartbeats}
        rosterRoles={rosterRoles}
        selfSeat={selfSeat}
        now={now}
        mutate={mutate}
      />
      <SymbolKey />
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
    <div className="protocol-panel__row" style={{ background: '#eef2ff', borderRadius: 6, padding: '8px 12px' }}>
      <span style={{ fontSize: '1.05rem' }}>📊</span>
      <span style={{ fontWeight: 600 }}>{round.topic || 'Round in progress'}</span>
      <span className="protocol-panel__pill" style={{ background: '#fff' }}>{phaseLabel}</span>
      {round.opened_by && (
        <span style={{ color: '#5b6478', fontSize: '0.85rem' }}>opener: {round.opened_by}</span>
      )}
      {phase === "submitting" && (
        <button
          type="button"
          className="protocol-panel__pill"
          style={{ marginLeft: 'auto', background: '#4f46e5', color: 'white', borderColor: '#4f46e5' }}
          onClick={() => { void mutate('close_round', {}); }}
          title="Close this consensus round"
        >
          Close round
        </button>
      )}
    </div>
  );
}

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
