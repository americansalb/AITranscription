// PhasePlanEditor — minimal modal for set_phase_plan (spec §7).
// Slice 9 remainder per architect #1027.
//
// MVP shape: textarea for the phases JSON array + Set button calling
// `mutate('set_phase_plan', { phases })`. Full visual editor (per-phase
// outcome predicate dropdown, duration spinner, etc.) is a follow-on;
// this gets the path live.

import { useState } from 'react';
import type { Protocol } from '../../hooks/useProtocolState';

export type PhasePlanEditorProps = {
  protocol: Protocol;
  mutate: (action: string, args?: object) => Promise<unknown>;
  onClose: () => void;
};

const SAMPLE_PLAN = [
  {
    preset: 'Debate',
    duration_secs: 3600,
    outcome: { kind: 'file_nonempty', target: '.vaak/notes.md' },
  },
  {
    preset: 'Brainstorm',
    duration_secs: 1800,
    outcome: { kind: 'manual' },
  },
];

export function PhasePlanEditor({ protocol, mutate, onClose }: PhasePlanEditorProps) {
  const initial = JSON.stringify(
    protocol.phase_plan.phases.length > 0 ? protocol.phase_plan.phases : SAMPLE_PLAN,
    null,
    2,
  );
  const [body, setBody] = useState(initial);
  const [error, setError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);

  const onSet = async () => {
    setError(null);
    let phases: unknown;
    try {
      phases = JSON.parse(body);
    } catch (e) {
      setError(`JSON parse error: ${String(e)}`);
      return;
    }
    if (!Array.isArray(phases)) {
      setError('phases must be a JSON array of phase objects');
      return;
    }
    setSubmitting(true);
    try {
      // protocol_mutate_cmd in main.rs rejects set_phase_plan with [InvalidAction]
      // (per spec §10 plan-author tier — UI side routes through MCP authoritative
      // path). For MVP we surface the routing error so user knows to call the
      // MCP tool directly. Future: route this through a plan-author Tauri cmd.
      await mutate('set_phase_plan', { phases });
      onClose();
    } catch (e) {
      setError(String(e));
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <div
      role="dialog"
      aria-label="Phase plan editor"
      style={{
        position: 'fixed',
        inset: 0,
        background: 'rgba(0,0,0,0.4)',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        zIndex: 1000,
      }}
      onClick={(e) => { if (e.target === e.currentTarget) onClose(); }}
    >
      <div style={{
        background: '#fff',
        borderRadius: 12,
        padding: 24,
        width: '90%',
        maxWidth: 720,
        maxHeight: '80vh',
        display: 'flex',
        flexDirection: 'column',
        gap: 12,
      }}>
        <h3 style={{ margin: 0 }}>Phase plan editor</h3>
        <div style={{ fontSize: '0.85rem', color: '#5b6478' }}>
          JSON array of phases. Each phase needs <code>preset</code> and{' '}
          <code>outcome</code> ({'{'}<code>kind</code>: file_nonempty | timer | manual | vote_quorum{'}'}). Optional <code>duration_secs</code>, <code>extension_secs</code>.
        </div>
        <textarea
          value={body}
          onChange={(e) => setBody(e.target.value)}
          rows={16}
          style={{
            fontFamily: 'monospace',
            fontSize: '0.85rem',
            border: '1px solid #e3e6ec',
            borderRadius: 6,
            padding: 10,
            resize: 'vertical',
            minHeight: 200,
          }}
          aria-label="Phases JSON"
        />
        {error && (
          <div style={{ background: '#fef2f2', color: '#ef4444', padding: '8px 12px', borderRadius: 4, fontSize: '0.85rem' }}>
            {error}
          </div>
        )}
        <div style={{ display: 'flex', gap: 8, justifyContent: 'flex-end' }}>
          <button type="button" onClick={onClose} style={{ padding: '6px 14px', borderRadius: 6, border: '1px solid #e3e6ec', background: '#fafbfc' }}>
            Cancel
          </button>
          <button
            type="button"
            onClick={() => { void onSet(); }}
            disabled={submitting}
            style={{ padding: '6px 14px', borderRadius: 6, border: 'none', background: '#4f46e5', color: 'white' }}
          >
            {submitting ? 'Setting…' : 'Set plan'}
          </button>
        </div>
      </div>
    </div>
  );
}
