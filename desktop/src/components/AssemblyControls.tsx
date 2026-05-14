// AssemblyControls — two-controls UI surface for v1.1 (planning/execution
// phase + assembly on/off + mic-passing mode). Spec at
// .vaak/design-notes/two-controls-spec-2026-05-14.md.
//
// Renders only when commit-A-protocol fields are present (assembly_active !==
// undefined). Sections that haven't migrated render nothing here; existing
// ProtocolPanel continues to handle them.
//
// Affordances surfaced:
// - Mic-icon-button (Assembly ON/OFF) — distinct from phase pill per §39
// - Phase mode-pill (Planning slate-blue / Execution green) — distinct from
//   assembly button so the two controls don't read as a composite toggle
// - Mic-passing dropdown (rotation / hand_raise / moderator) — greyed when
//   assembly is off so users see the current setting but can't change it
// - Plan path text input (v1.1 fallback per project audit: no list_design_notes
//   Tauri command exists; spec §148's mtime-sorted dropdown is a follow-up)
// - ••• menu on plan link: revise_plan greyed for non-architect/manager/human
//   per §149 + commit A's RevisePlanForbidden gate (view-plan-file deferred —
//   no shell-opener plugin)
// - Destructive-confirm modal on open_planning per §63 (toast bullet dropped
//   per architect msg 1032 — modal is the only confirmation pattern)
// - Per-mechanism status strip below the controls (UI-arch msg 969 fold)
// - Combination map (2x2) with filled-accent active cell + hover tooltips

import { useState, useEffect, useRef } from 'react';
import type { Protocol } from '../hooks/useProtocolState';
import './AssemblyControls.css';

type Mutate = (action: string, args?: object) => Promise<Protocol | null>;

export type AssemblyControlsProps = {
  protocol: Protocol;
  mutate: Mutate;
  // Latest error from useProtocolState (translated via friendlyError). B.1 fold
  // per UX-eng msg 1155 FINDING — modal bodies surface server-side rejections
  // (PlanPathMissing, RevisePlanForbidden, SetModeratorForbidden, etc.) inline
  // so the user sees WHY their Accept/Revise/Discard click didn't take.
  lastError: string | null;
  selfRole: string | null; // current user's role slug (null = human view)
};

const REVISE_ALLOWED_ROLES = new Set(['architect', 'manager', 'human']);

export function AssemblyControls({ protocol, mutate, lastError, selfRole }: AssemblyControlsProps) {
  // B.2 back-compat default render (per architect msg 1298 + human msg 1296).
  // Pre-commit-A sections (5-12 et al.) lack the new floor fields. Original B.1
  // gate `if (assembly_active === undefined) return null` made the UI invisible
  // in every legacy section, blocking the human's live-click verification.
  // Resolution: render with nullish-coalesce defaults — assembly OFF, execution
  // phase, rotation mic. User opts in via the toggle; server-side state only
  // changes once they actually issue protocol_mutate.
  const assemblyActive = protocol.floor.assembly_active ?? false;
  const phase = protocol.floor.phase ?? 'execution';
  const micMode = protocol.floor.mic_passing_mode ?? 'rotation';
  const planPath = protocol.floor.plan_path ?? null;
  const moderator = protocol.floor.moderator ?? null;
  const handQueue = protocol.floor.hand_queue ?? [];
  const rotationOrder = protocol.floor.rotation_order ?? [];
  const currentSpeaker = protocol.floor.current_speaker;

  // Destructive-confirm modal state for open_planning (spec §63).
  const [confirmOpenPlanning, setConfirmOpenPlanning] = useState(false);
  // Plan path entry mode — null when not entering; "accept" or "revise" when entering.
  const [planEntryMode, setPlanEntryMode] = useState<'accept' | 'revise' | null>(null);
  const [planPathInput, setPlanPathInput] = useState('');
  const [revisionNote, setRevisionNote] = useState('');
  // ••• menu dropdown open state.
  const [planMenuOpen, setPlanMenuOpen] = useState(false);
  // Baseline-snapshot pattern for surfacing modal errors (B.1 fold per UX-eng
  // msg 1155 FINDING). When a modal opens we snapshot the current `lastError`
  // — any future `lastError` that differs from the baseline is an error that
  // landed AFTER the modal opened (i.e., from a failed mutate on this attempt).
  // Avoids stale-closure issues that would come with an async-read approach,
  // and avoids needing a separate setModalError state.
  const [errorBaseline, setErrorBaseline] = useState<string | null>(null);
  const displayError = lastError !== errorBaseline ? lastError : null;
  // Refs for default-focus targets on each modal (Nit 1 per UX-eng msg 1155).
  const confirmCancelRef = useRef<HTMLButtonElement | null>(null);
  const planInputRef = useRef<HTMLInputElement | null>(null);

  // Nit 2 (UX-eng msg 1155): tighten the phase-toggle authority gate. Original
  // `selfRole === null` worked for CollabTab's human view but would let any
  // AI agent with null role lookup also toggle. Explicit human-only allows the
  // current usage AND blocks future AI-bound contexts where AssemblyControls
  // might be rendered.
  const canRevise = selfRole !== null && REVISE_ALLOWED_ROLES.has(selfRole);
  const canTogglePhase = selfRole === null || selfRole === 'human';

  // ESC-to-close for both modals (Nit 1 per UX-eng msg 1155). One global
  // keydown handler dispatches to whichever modal is currently open.
  useEffect(() => {
    if (!confirmOpenPlanning && !planEntryMode) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== 'Escape') return;
      if (confirmOpenPlanning) {
        setConfirmOpenPlanning(false);
      } else if (planEntryMode) {
        setPlanEntryMode(null);
      }
    };
    document.addEventListener('keydown', onKey);
    return () => document.removeEventListener('keydown', onKey);
  }, [confirmOpenPlanning, planEntryMode]);

  // Default-focus on open: Cancel for destructive-confirm (safer default per
  // destructive-action UX convention), input field for plan-entry.
  useEffect(() => {
    if (confirmOpenPlanning) {
      confirmCancelRef.current?.focus();
    } else if (planEntryMode) {
      planInputRef.current?.focus();
    }
  }, [confirmOpenPlanning, planEntryMode]);

  // FINDING (UX-eng msg 1155): server-side rejections (PlanPathMissing,
  // RevisePlanForbidden, SetModeratorForbidden, etc.) used to fire-and-forget
  // — modal closed, nothing happened, user confused. Now await mutate(),
  // catch error from useProtocolState.friendlyError, surface inline in modal
  // body. Modal stays open on error so the user can correct and retry.
  const handleAssemblyToggle = () => {
    void mutate('set_assembly', { active: !assemblyActive });
  };

  const handlePhaseToggle = () => {
    if (!canTogglePhase) return;
    // Snapshot lastError baseline so we only show errors that land AFTER this
    // modal opens (B.1 baseline-snapshot pattern).
    setErrorBaseline(lastError);
    if (phase === 'planning') {
      // planning → execution requires a plan_path; open the accept-plan entry.
      setPlanEntryMode('accept');
      setPlanPathInput('');
    } else {
      // execution → planning is destructive (clears plan_hash); confirm-modal per §63.
      setConfirmOpenPlanning(true);
    }
  };

  // Open revise-plan entry from the ••• menu — snapshot the baseline too.
  const openReviseModal = () => {
    setErrorBaseline(lastError);
    setPlanEntryMode('revise');
    setPlanPathInput(planPath ?? '');
    setRevisionNote('');
    setPlanMenuOpen(false);
  };

  const confirmOpenPlanningSubmit = async () => {
    const result = await mutate('open_planning', {});
    // Only close on success. On failure (result === null), the hook has
    // updated lastError; displayError will surface it on next render.
    if (result !== null) {
      setConfirmOpenPlanning(false);
    }
  };

  const submitPlanPath = async () => {
    const path = planPathInput.trim();
    if (!path) return;
    let result: Protocol | null = null;
    if (planEntryMode === 'accept') {
      result = await mutate('accept_plan', { plan_path: path });
    } else if (planEntryMode === 'revise') {
      result = await mutate('revise_plan', { plan_path: path, revision_note: revisionNote.trim() });
    }
    // Only close on success. On failure, modal stays open and displayError
    // surfaces the friendly translation of the server-side error variant.
    if (result !== null) {
      setPlanEntryMode(null);
      setPlanPathInput('');
      setRevisionNote('');
    }
  };

  const handleMicModeChange = (e: React.ChangeEvent<HTMLSelectElement>) => {
    const mode = e.target.value;
    if (mode !== micMode) {
      void mutate('set_mic_passing', { mode });
    }
  };

  // Per-mechanism status strip second line (UI-arch msg 969 fold).
  const renderStatusLine = () => {
    if (micMode === 'rotation') {
      if (rotationOrder.length === 0) {
        return <span className="assembly-status-empty">No rotation set</span>;
      }
      const currentIdx = currentSpeaker ? rotationOrder.indexOf(currentSpeaker) : -1;
      const next = rotationOrder
        .slice(currentIdx + 1)
        .concat(rotationOrder.slice(0, Math.max(0, currentIdx)))
        .slice(0, 3);
      return (
        <>
          <span className="assembly-status-label">Next:</span>{' '}
          {next.length === 0
            ? <span className="assembly-status-empty">—</span>
            : next.map((seat, i) => (
                <span key={seat}>
                  {i > 0 && <span className="assembly-status-arrow"> → </span>}
                  <span className="assembly-status-seat">{seat}</span>
                </span>
              ))}
        </>
      );
    }
    if (micMode === 'hand_raise') {
      return (
        <>
          <span className="assembly-status-label">Queue:</span>{' '}
          {handQueue.length === 0
            ? <span className="assembly-status-empty">empty</span>
            : handQueue.map((seat, i) => (
                <span key={seat}>
                  {i > 0 && <span className="assembly-status-sep">, </span>}
                  <span className="assembly-status-seat">{seat}</span>
                </span>
              ))}{' '}
          <button
            type="button"
            className="assembly-status-action"
            onClick={() => { void mutate('raise_hand', {}); }}
            disabled={selfRole === null}
            title={selfRole === null ? 'Only seats can raise hand (human is observer)' : 'Add yourself to the queue'}
          >
            Raise hand
          </button>
        </>
      );
    }
    if (micMode === 'moderator') {
      return (
        <>
          <span className="assembly-status-label">Moderator:</span>{' '}
          <span className="assembly-status-seat">{moderator ?? 'unset'}</span>
          {selfRole !== null && moderator === `${selfRole}:0` && (
            <span className="assembly-status-hint"> (you pick next speaker via grant_mic)</span>
          )}
        </>
      );
    }
    return null;
  };

  // 2x2 combination map cell highlight per UI-arch msg 986 minor 3 fold.
  const activeCell = `${assemblyActive ? 'on' : 'off'}-${phase}`;
  const cellMeta: Record<string, { label: string; tooltip: string }> = {
    'on-planning': { label: 'Orderly', tooltip: 'Sequential planning. Mic-gated sends. Commits blocked.' },
    'off-planning': { label: 'Brainstorm', tooltip: 'Free planning. Anyone speaks. Commits blocked.' },
    'on-execution': { label: 'Sequential', tooltip: 'Mic-gated execution. One author at a time. Commits allowed under plan scope.' },
    'off-execution': { label: 'Parallel', tooltip: 'Free execution. Multiple authors at once. Commits allowed under plan scope.' },
  };

  return (
    <section className="assembly-controls" role="region" aria-label="Assembly and phase controls">
      {/* Row 1: distinct widgets — mic-icon-button + phase pill */}
      <div className="assembly-controls-row">
        <button
          type="button"
          className={`assembly-mic-btn${assemblyActive ? ' is-on' : ' is-off'}`}
          onClick={handleAssemblyToggle}
          aria-pressed={assemblyActive}
          title={assemblyActive ? 'Turn Assembly Line off (free-send mode)' : 'Turn Assembly Line on (mic-gated mode)'}
        >
          <span aria-hidden="true" className="assembly-mic-icon">🎙</span>
          <span className="assembly-mic-label">ASSEMBLY: {assemblyActive ? 'ON' : 'OFF'}</span>
        </button>

        <button
          type="button"
          className={`assembly-phase-pill is-${phase}${canTogglePhase ? '' : ' is-disabled'}`}
          onClick={handlePhaseToggle}
          disabled={!canTogglePhase}
          title={
            !canTogglePhase
              ? 'Phase change is human-only (per phase_change_human_only)'
              : phase === 'planning'
                ? 'Click to accept a plan and switch to EXECUTION'
                : 'Click to open planning (discards current plan)'
          }
        >
          <span className="assembly-phase-marker" aria-hidden="true">
            {phase === 'planning' ? '✎' : '▷'}
          </span>
          <span className="assembly-phase-label">{phase === 'planning' ? 'PLANNING' : 'EXECUTING'}</span>
        </button>
      </div>

      {/* Row 2: mic-passing dropdown (greyed when assembly off) + plan link */}
      <div className="assembly-controls-row">
        <label className={`assembly-mic-mode${assemblyActive ? '' : ' is-disabled'}`}>
          <span className="assembly-mic-mode-label">Mic passing:</span>
          <select
            value={micMode}
            onChange={handleMicModeChange}
            disabled={!assemblyActive}
            title={assemblyActive ? 'Choose how the mic passes between seats' : 'Mic-passing mode is only meaningful when Assembly is ON'}
          >
            <option value="rotation">Rotation</option>
            <option value="hand_raise">Hand raise</option>
            <option value="moderator">Moderator</option>
          </select>
        </label>

        {phase === 'execution' && planPath && (
          <div className="assembly-plan-link">
            <span className="assembly-plan-label">Plan:</span>
            <span className="assembly-plan-path" title={planPath}>{planPath.replace(/^.*\//, '')}</span>
            <div className="assembly-plan-menu-wrap">
              <button
                type="button"
                className="assembly-plan-menu-btn"
                onClick={() => setPlanMenuOpen(!planMenuOpen)}
                aria-haspopup="menu"
                aria-expanded={planMenuOpen}
                title="Plan actions"
              >
                •••
              </button>
              {planMenuOpen && (
                <div className="assembly-plan-menu" role="menu">
                  <button
                    type="button"
                    role="menuitem"
                    className="assembly-plan-menu-item"
                    onClick={openReviseModal}
                    disabled={!canRevise}
                    title={
                      canRevise
                        ? 'Revise the current plan (re-hashes the file)'
                        : 'Revise is gated to architect, manager, or human roles'
                    }
                  >
                    Revise plan…
                  </button>
                  <button
                    type="button"
                    role="menuitem"
                    className="assembly-plan-menu-item is-disabled"
                    disabled
                    title="View-plan-file deferred — no shell-opener plugin in v1.1"
                  >
                    View plan file (v1.2)
                  </button>
                </div>
              )}
            </div>
          </div>
        )}
      </div>

      {/* Row 3: per-mechanism status strip — UI-arch msg 969 fold */}
      {assemblyActive && (
        <div className="assembly-status-strip">
          <span className="assembly-status-currently">
            <span className="assembly-status-label">Currently:</span>{' '}
            <span className="assembly-status-seat">{currentSpeaker ?? 'idle'}</span>
          </span>
          <span className="assembly-status-separator">·</span>
          <span className="assembly-status-second">{renderStatusLine()}</span>
        </div>
      )}

      {/* Row 4: 2x2 combination map — filled-accent active cell per UI-arch msg 986 minor 3 */}
      <div className="assembly-combo-map" role="table" aria-label="Combination map">
        <div className="assembly-combo-header" role="row">
          <div role="columnheader" className="assembly-combo-cell is-header"></div>
          <div role="columnheader" className="assembly-combo-cell is-header">Planning</div>
          <div role="columnheader" className="assembly-combo-cell is-header">Execution</div>
        </div>
        <div className="assembly-combo-row" role="row">
          <div role="rowheader" className="assembly-combo-cell is-rowheader">Asm ON</div>
          <ComboCell active={activeCell === 'on-planning'} meta={cellMeta['on-planning']} />
          <ComboCell active={activeCell === 'on-execution'} meta={cellMeta['on-execution']} />
        </div>
        <div className="assembly-combo-row" role="row">
          <div role="rowheader" className="assembly-combo-cell is-rowheader">Asm OFF</div>
          <ComboCell active={activeCell === 'off-planning'} meta={cellMeta['off-planning']} />
          <ComboCell active={activeCell === 'off-execution'} meta={cellMeta['off-execution']} />
        </div>
      </div>

      {/* Destructive-confirm modal for open_planning per spec §63 */}
      {confirmOpenPlanning && (
        <div className="assembly-modal-backdrop" onClick={() => setConfirmOpenPlanning(false)}>
          <div className="assembly-modal" role="dialog" aria-modal="true" onClick={(e) => e.stopPropagation()}>
            <div className="assembly-modal-title">Discard plan and open planning?</div>
            <div className="assembly-modal-body">
              {planPath ? (
                <>
                  Current plan: <code>{planPath}</code>
                  <br />Clearing this allows replanning and will re-block commits until a new plan is accepted.
                </>
              ) : (
                'Switch to planning phase. Commits will be blocked until a new plan is accepted.'
              )}
            </div>
            {displayError && (
              <div className="assembly-modal-error" role="alert">
                {displayError}
              </div>
            )}
            <div className="assembly-modal-actions">
              <button
                type="button"
                className="assembly-modal-cancel"
                onClick={() => setConfirmOpenPlanning(false)}
                ref={confirmCancelRef}
              >
                Cancel
              </button>
              <button type="button" className="assembly-modal-confirm" onClick={confirmOpenPlanningSubmit}>
                Discard plan
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Plan path entry modal (accept_plan or revise_plan) */}
      {planEntryMode && (
        <div className="assembly-modal-backdrop" onClick={() => setPlanEntryMode(null)}>
          <div className="assembly-modal" role="dialog" aria-modal="true" onClick={(e) => e.stopPropagation()}>
            <div className="assembly-modal-title">
              {planEntryMode === 'accept' ? 'Accept plan' : 'Revise plan'}
            </div>
            <div className="assembly-modal-body">
              <label className="assembly-modal-field">
                <span>Plan path (under <code>.vaak/design-notes/</code>):</span>
                <input
                  ref={planInputRef}
                  type="text"
                  className="assembly-modal-input"
                  value={planPathInput}
                  onChange={(e) => setPlanPathInput(e.target.value)}
                  placeholder=".vaak/design-notes/your-plan-2026-XX-XX.md"
                />
              </label>
              {planEntryMode === 'revise' && (
                <label className="assembly-modal-field">
                  <span>Revision note (optional):</span>
                  <input
                    type="text"
                    className="assembly-modal-input"
                    value={revisionNote}
                    onChange={(e) => setRevisionNote(e.target.value)}
                    placeholder="scope addition: foo.rs"
                  />
                </label>
              )}
              <div className="assembly-modal-hint">
                Plan file must include a <code>&lt;!-- scope: path1 path2 --&gt;</code> block declaring files in scope.
                Use <code>&lt;!-- scope: * --&gt;</code> for an unrestricted plan.
              </div>
            </div>
            {displayError && (
              <div className="assembly-modal-error" role="alert">
                {displayError}
              </div>
            )}
            <div className="assembly-modal-actions">
              <button type="button" className="assembly-modal-cancel" onClick={() => setPlanEntryMode(null)}>
                Cancel
              </button>
              <button
                type="button"
                className="assembly-modal-confirm"
                onClick={submitPlanPath}
                disabled={!planPathInput.trim()}
              >
                {planEntryMode === 'accept' ? 'Accept plan' : 'Revise plan'}
              </button>
            </div>
          </div>
        </div>
      )}
    </section>
  );
}

function ComboCell({ active, meta }: { active: boolean; meta: { label: string; tooltip: string } }) {
  return (
    <div
      role="cell"
      className={`assembly-combo-cell${active ? ' is-active' : ''}`}
      title={meta.tooltip}
    >
      {meta.label}
    </div>
  );
}
