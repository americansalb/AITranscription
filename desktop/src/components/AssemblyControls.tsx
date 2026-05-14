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
import { invoke } from '@tauri-apps/api/core';
import type { Protocol } from '../hooks/useProtocolState';
import './AssemblyControls.css';

type Mutate = (action: string, args?: object) => Promise<Protocol | null>;

// B.4 Item 7 (per UX-eng spec §104-136 follow-up + human msg 1447 Q1/Q2):
// active-seats response shape from main.rs's list_active_seats_cmd at
// commit 7abef44 (developer:0). The `label` field is the canonical
// "role:instance" seat id used by set_moderator's args.seat.
type ActiveSeat = {
  role: string;
  instance: number;
  label: string;
  last_heartbeat: string;
};
type ActiveSeatsResponse = { seats: ActiveSeat[] };

export type AssemblyControlsProps = {
  protocol: Protocol;
  mutate: Mutate;
  // Latest error from useProtocolState (translated via friendlyError). B.1 fold
  // per UX-eng msg 1155 FINDING — modal bodies surface server-side rejections
  // (PlanPathMissing, RevisePlanForbidden, SetModeratorForbidden, etc.) inline
  // so the user sees WHY their Accept/Revise/Discard click didn't take.
  lastError: string | null;
  selfRole: string | null; // current user's role slug (null = human view)
  projectDir: string | null; // for the list_active_seats_cmd IPC (B.4)
};

const REVISE_ALLOWED_ROLES = new Set(['architect', 'manager', 'human']);

export function AssemblyControls({ protocol, mutate, lastError, selfRole, projectDir }: AssemblyControlsProps) {
  // B.2 back-compat default render (per architect msg 1298 + human msg 1296).
  // Pre-commit-A sections (5-12 et al.) lack the new floor fields. Original B.1
  // gate `if (assembly_active === undefined) return null` made the UI invisible
  // in every legacy section, blocking the human's live-click verification.
  // Resolution: render with nullish-coalesce defaults — assembly OFF, execution
  // phase, rotation mic. User opts in via the toggle; server-side state only
  // changes once they actually issue protocol_mutate.
  //
  // B.3.1 backfill-read (per architect msg 1399 layer i + human msg 1397):
  // when the legacy assembly-line button at CollabTab.tsx:2710 flips preset to
  // "Assembly Line" without writing the new assembly_active field, this reader
  // backfills from preset so the new AssemblyControls UI agrees with the
  // legacy one. Eliminates the dual-UI de-sync the human flagged.
  const assemblyActive = protocol.floor.assembly_active ?? (protocol.preset === 'Assembly Line');
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

  // Item 5 — moderator fast-flip row. Renders when the current viewer IS the
  // moderator (per UX-eng msg 1587 split-by-surface design + UI-arch msg 1589
  // per-actor render variants). Self-seat-label derived from selfRole; human
  // view (selfRole === null) maps to "human:0" canonical seat.
  const selfSeatLabel = selfRole === null ? 'human:0' : `${selfRole}:0`;
  const isSelfModerator = micMode === 'moderator' && moderator !== null && moderator === selfSeatLabel;

  // Per the spec line 70-103, moderator-driven phase flips skip the modal
  // — the explicit moderator-only surface IS the intent declaration.
  const handleFastFlipPlanning = () => {
    void mutate('open_planning', {});
  };
  const handleFastFlipExecuting = () => {
    // For the fast-flip path, we don't open the plan-entry modal. Server
    // requires plan_path for accept_plan — moderator must use the regular
    // phase pill to enter a path on first flip TO execution. Fast-flip TO
    // execution only works mid-back-and-forth when a plan_path is already
    // bound on the section. If plan_path is null, surface a transient hint.
    if (!planPath) {
      setMicModeHint('No plan accepted yet. Use the phase pill to enter a plan path first; fast-flip is for back-and-forth after the initial accept.');
      return;
    }
    void mutate('accept_plan', { plan_path: planPath });
  };

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

  // Item 6 (UX-eng msg 1449 per human msg 1447 + architect msg 1464): when
  // user picks "Moderator picks next" without a moderator assigned, watchdog
  // would auto-promote back to rotation silently with no UI feedback. Short-
  // circuit at the change handler — show inline notice next to the dropdown,
  // don't fire the mutate (avoids rejection round-trip and the silent revert
  // confusion that human msg 1447 hit).
  const [micModeHint, setMicModeHint] = useState<string | null>(null);

  // B.4 Item 7 — moderator-picker dropdown wired to list_active_seats_cmd
  // (developer:0 commit 7abef44). Fetched on mount + when assembly turns on.
  // Dropdown renders only when selfRole === null (human view) AND
  // assemblyActive — picking a moderator on a disabled assembly is moot.
  const [activeSeats, setActiveSeats] = useState<ActiveSeat[]>([]);
  useEffect(() => {
    if (!projectDir || !assemblyActive) return;
    let cancelled = false;
    void (async () => {
      try {
        const resp = await invoke<ActiveSeatsResponse>('list_active_seats_cmd', { dir: projectDir });
        if (!cancelled) setActiveSeats(resp.seats);
      } catch (e) {
        console.warn('[AssemblyControls] list_active_seats_cmd failed:', e);
      }
    })();
    return () => { cancelled = true; };
  }, [projectDir, assemblyActive, currentSpeaker]);

  const handleModeratorChange = (e: React.ChangeEvent<HTMLSelectElement>) => {
    const seat = e.target.value;
    if (!seat || seat === moderator) return;
    void mutate('set_moderator', { seat });
  };
  const handleMicModeChange = (e: React.ChangeEvent<HTMLSelectElement>) => {
    const mode = e.target.value;
    if (mode === micMode) return;
    if (mode === 'moderator' && !moderator) {
      setMicModeHint('"Moderator picks next" requires a moderator. Set one first.');
      // Revert the select to its prior value so the dropdown reflects actual state.
      e.target.value = micMode;
      return;
    }
    setMicModeHint(null);
    void mutate('set_mic_passing', { mode });
  };

  // Item 2 of moderator-authority bundle (per UX-eng msg 1603 inline spec text
  // + UI-arch msg 1605 endorsement): filter the moderator out of any rendered
  // rotation/queue list when moderator mode is active. Mirrors the server-side
  // rotation-advance filter in 9ae070d's next_assembly_speaker. Without this,
  // the status strip would say "Moderator: ux-eng:0 (out of rotation)" AND
  // "Next: developer:0 → ux-eng:0 → tester:0" — contradictory.
  const isModeratorExempt = (seat: string) =>
    micMode === 'moderator' && moderator !== null && seat === moderator;

  // Per-mechanism status strip second line (UI-arch msg 969 fold).
  const renderStatusLine = () => {
    if (micMode === 'rotation') {
      if (rotationOrder.length === 0) {
        return <span className="assembly-status-empty">No rotation set</span>;
      }
      const visibleOrder = rotationOrder.filter((seat) => !isModeratorExempt(seat));
      const currentIdx = currentSpeaker ? visibleOrder.indexOf(currentSpeaker) : -1;
      const next = visibleOrder
        .slice(currentIdx + 1)
        .concat(visibleOrder.slice(0, Math.max(0, currentIdx)))
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
      // Defensive filter (UX-eng msg 1603): moderator shouldn't end up in
      // hand_queue, but cheap insurance if they ever do.
      const visibleQueue = handQueue.filter((seat) => !isModeratorExempt(seat));
      return (
        <>
          <span className="assembly-status-label">Queue:</span>{' '}
          {visibleQueue.length === 0
            ? <span className="assembly-status-empty">empty</span>
            : visibleQueue.map((seat, i) => (
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

  // B.3 Item 4 (option c per UX-eng spec §86-102): combination map REMOVED
  // entirely. Plain-language labels on the two toggle controls now
  // communicate state directly; the 2x2 matrix was teaching-not-operational
  // and the human msg 1303 confirmed it added cognitive load without
  // capability. Negative space replaces it; assembly button + phase pill
  // labels carry the state-meaning.

  // B.3 Item 5 (per UX-eng spec §104-136 + human msg 1421): embed
  // current_speaker in the assembly button label so "who has the floor"
  // reads as the primary state on the primary control. Three button-text
  // states:
  //   assembly OFF                          → "Assembly Off — anyone can speak"
  //   assembly ON + speaker                 → "Assembly On — <seat> has floor"
  //   assembly ON + no speaker (idle)       → "Assembly On — waiting for next speaker"
  const assemblyButtonText = !assemblyActive
    ? 'Assembly Off — anyone can speak'
    : currentSpeaker
      ? `Assembly On — ${currentSpeaker} has floor`
      : 'Assembly On — waiting for next speaker';

  // B.3 Item 3 (per UX-eng spec §63-84): plain-language phase labels.
  // Drops all-caps, adds action-oriented subtitle hint after the primary
  // state. Tooltip on hover surfaces the full hint when narrow.
  const phaseButtonText = phase === 'planning'
    ? 'Planning — discuss, design, decide'
    : 'Executing — code, commit, ship';

  return (
    <section className="assembly-controls" role="region" aria-label="Assembly and phase controls">
      {/* B.3 Item 2 (per UX-eng spec §43-61): single horizontal row.
          Was 4 vertical rows (~120px); now one flex-wrap row (~36px).
          Mic-button + phase-pill + mic-mode (compact) + plan-link all inline. */}
      <div className="assembly-controls-row">
        <button
          type="button"
          className={`assembly-mic-btn${assemblyActive ? ' is-on' : ' is-off'}`}
          onClick={handleAssemblyToggle}
          aria-pressed={assemblyActive}
          title={assemblyActive ? 'Click to turn Assembly Line off (everyone can speak freely)' : 'Click to turn Assembly Line on (one speaker at a time)'}
        >
          <span aria-hidden="true" className="assembly-mic-icon">🎙</span>
          <span className="assembly-mic-label">{assemblyButtonText}</span>
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
                ? 'Click to accept a plan and switch to Executing'
                : 'Click to open Planning (discards current plan)'
          }
        >
          <span className="assembly-phase-marker" aria-hidden="true">
            {phase === 'planning' ? '✎' : '▷'}
          </span>
          <span className="assembly-phase-label">{phaseButtonText}</span>
          {canTogglePhase && <span className="assembly-phase-chevron" aria-hidden="true">▾</span>}
        </button>

        <label className={`assembly-mic-mode${assemblyActive ? '' : ' is-disabled'}`}>
          <span className="assembly-mic-mode-label">Mic:</span>
          <select
            value={micMode}
            onChange={handleMicModeChange}
            disabled={!assemblyActive}
            title={assemblyActive ? 'Choose how the mic passes between seats' : 'Mic-passing mode is only meaningful when Assembly is ON'}
          >
            <option value="rotation">Round-robin</option>
            <option value="hand_raise">Hand-raise</option>
            <option value="moderator">Moderator</option>
          </select>
        </label>

        {/* B.4 Item 7 — moderator-picker dropdown. Only renders for the human
            view (selfRole === null) since set_moderator is human-only gated
            at apply_set_moderator per A.2.1's [SetModeratorForbidden] gate.
            Renders when assembly is ON so the human can stage a moderator
            BEFORE switching mic-mode to Moderator (Item 6's short-circuit
            requires this). */}
        {assemblyActive && selfRole === null && (
          <label className="assembly-moderator-picker">
            <span className="assembly-moderator-label">Moderator:</span>
            <select
              value={moderator ?? ''}
              onChange={handleModeratorChange}
              title="Pick a moderator. Required before switching mic-passing mode to Moderator."
            >
              <option value="">— none —</option>
              {activeSeats.map((seat) => (
                <option key={seat.label} value={seat.label}>{seat.label}</option>
              ))}
            </select>
          </label>
        )}

        {micModeHint && (
          <span className="assembly-mic-mode-hint" role="alert">
            {micModeHint}
          </span>
        )}

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

      {/* Moderator fast-flip row — moderator-authority Item 5 per UX-eng
          msg 1587 split-by-surface + UI-arch msg 1589 per-actor render. Only
          the moderator seat sees this; provides no-modal phase-flip for the
          "back and forth quite a bit" use case per human msg 1576. Spec
          line 70-103 of moderator-authority-spec-2026-05-14.md. */}
      {assemblyActive && isSelfModerator && (
        <div className="assembly-moderator-fast-flip">
          <span className="assembly-fast-flip-label">As moderator:</span>
          <button
            type="button"
            className="assembly-fast-flip-btn"
            onClick={handleFastFlipPlanning}
            disabled={phase === 'planning'}
            title="Flip to Planning (no modal — moderator fast-flip)"
          >
            → Planning
          </button>
          <button
            type="button"
            className="assembly-fast-flip-btn"
            onClick={handleFastFlipExecuting}
            disabled={phase === 'execution'}
            title="Flip to Executing (no modal — moderator fast-flip; requires existing plan_path)"
          >
            → Executing
          </button>
        </div>
      )}

      {/* Per-mechanism status strip — UI-arch msg 969 fold. "Currently:"
          segment dropped in B.3 Item 5 (per UX-eng spec §131): the current
          speaker is now embedded in the assembly button's label, making the
          strip's "Currently:" redundant. The second-line "what's next" info
          (rotation order, hand-raise queue, moderator) is still useful. */}
      {assemblyActive && (
        <div className="assembly-status-strip">
          {renderStatusLine()}
        </div>
      )}

      {/* Combination map REMOVED in B.3 Item 4 per UX-eng spec §86-102
          option (c). Plain-language labels on the assembly button + phase
          pill now carry the state-meaning directly. */}

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

