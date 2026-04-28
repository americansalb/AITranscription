// Slice 9 Gap B — PhaseRow React unit tests.
// Spec: .vaak/al-architecture-diagram.md §1 + §7.
// COVERAGE_GAPS.md Gap B closer per dev-challenger #980 finding 3.
//
// PhaseRow is exported indirectly via ProtocolPanel; we test it via the
// parent surface with seeded protocol state, asserting button behavior +
// disabled states + pause/resume label switching.

import { describe, expect, it, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import '@testing-library/jest-dom/vitest';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}));
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(async () => () => {}),
}));

import { invoke } from '@tauri-apps/api/core';
import { ProtocolPanel } from '../ProtocolPanel';

function fixtureWith(phasePatch: Record<string, unknown>) {
  return {
    section: 'default',
    protocol: {
      schema_version: 1,
      rev: 5,
      preset: 'Debate',
      floor: { mode: 'reactive', current_speaker: null, queue: [], rotation_order: [], threshold_ms: 60_000, started_at: null },
      consensus: { mode: 'none', round: null, phase: null, submissions: [] },
      phase_plan: {
        phases: [
          { preset: 'Debate', duration_secs: 0, extension_secs: 0, outcome: { kind: 'manual' }, started_at: '2026-04-28T00:00:00Z', ended_at: null },
          { preset: 'Brainstorm', duration_secs: 0, extension_secs: 0, outcome: { kind: 'manual' }, started_at: null, ended_at: null },
        ],
        current_phase_idx: 0,
        paused_at: null,
        paused_total_secs: 0,
        ...phasePatch,
      },
      scopes: { floor: 'instance', consensus: 'role' },
      last_writer_seat: null,
      last_writer_action: null,
      rev_at: null,
    },
    heartbeats: {},
  };
}

beforeEach(() => {
  (invoke as ReturnType<typeof vi.fn>).mockReset();
});

describe('PhaseRow — Slice 9 Gap B (R6 phase controls)', () => {
  it('renders Phase 1 of 2 with progress bar', async () => {
    (invoke as ReturnType<typeof vi.fn>).mockResolvedValue(fixtureWith({}));
    render(<ProtocolPanel projectDir="/x" section="default" selfSeat={null} rosterRoles={[]} defaultExpanded={true} />);
    await waitFor(() => {
      expect(screen.getByText(/Phase 1 of 2/)).toBeInTheDocument();
    });
  });

  it('pause button shows "pause" when paused_at is null', async () => {
    (invoke as ReturnType<typeof vi.fn>).mockResolvedValue(fixtureWith({ paused_at: null }));
    render(<ProtocolPanel projectDir="/x" section="default" selfSeat={null} rosterRoles={[]} defaultExpanded={true} />);
    await waitFor(() => {
      expect(screen.getByRole('button', { name: /pause/i })).toBeInTheDocument();
    });
  });

  it('pause button shows "resume" + ⏸ paused indicator when paused_at is set', async () => {
    (invoke as ReturnType<typeof vi.fn>).mockResolvedValue(
      fixtureWith({ paused_at: '2026-04-28T20:00:00Z' }),
    );
    render(<ProtocolPanel projectDir="/x" section="default" selfSeat={null} rosterRoles={[]} defaultExpanded={true} />);
    await waitFor(() => {
      expect(screen.getByRole('button', { name: /resume/i })).toBeInTheDocument();
      expect(screen.getByText(/paused/i)).toBeInTheDocument();
    });
  });

  it('clicking pause invokes protocol_mutate_cmd with action=pause_plan', async () => {
    (invoke as ReturnType<typeof vi.fn>).mockResolvedValue(fixtureWith({}));
    render(<ProtocolPanel projectDir="/x" section="default" selfSeat={null} rosterRoles={[]} defaultExpanded={true} />);
    await waitFor(() => screen.getByRole('button', { name: /pause/i }));
    fireEvent.click(screen.getByRole('button', { name: /pause/i }));
    await waitFor(() => {
      const mutateCalls = (invoke as ReturnType<typeof vi.fn>).mock.calls.filter(
        (c) => c[0] === 'protocol_mutate_cmd',
      );
      expect(mutateCalls.length).toBeGreaterThan(0);
      expect(mutateCalls[0][1]).toMatchObject({ action: 'pause_plan' });
    });
  });

  it('clicking skip invokes protocol_mutate_cmd with action=advance_phase', async () => {
    (invoke as ReturnType<typeof vi.fn>).mockResolvedValue(fixtureWith({}));
    render(<ProtocolPanel projectDir="/x" section="default" selfSeat={null} rosterRoles={[]} defaultExpanded={true} />);
    await waitFor(() => screen.getByRole('button', { name: /skip/i }));
    fireEvent.click(screen.getByRole('button', { name: /skip/i }));
    await waitFor(() => {
      const mutateCalls = (invoke as ReturnType<typeof vi.fn>).mock.calls.filter(
        (c) => c[0] === 'protocol_mutate_cmd',
      );
      expect(mutateCalls.length).toBeGreaterThan(0);
      expect(mutateCalls[0][1]).toMatchObject({ action: 'advance_phase' });
    });
  });

  it('clicking +15m invokes protocol_mutate_cmd with action=extend_phase, args.secs=900', async () => {
    (invoke as ReturnType<typeof vi.fn>).mockResolvedValue(fixtureWith({}));
    render(<ProtocolPanel projectDir="/x" section="default" selfSeat={null} rosterRoles={[]} defaultExpanded={true} />);
    await waitFor(() => screen.getByRole('button', { name: /\+15m/i }));
    fireEvent.click(screen.getByRole('button', { name: /\+15m/i }));
    await waitFor(() => {
      const mutateCalls = (invoke as ReturnType<typeof vi.fn>).mock.calls.filter(
        (c) => c[0] === 'protocol_mutate_cmd',
      );
      expect(mutateCalls.length).toBeGreaterThan(0);
      expect(mutateCalls[0][1]).toMatchObject({ action: 'extend_phase', args: { secs: 900 } });
    });
  });

  it('renders complete pill + disabled buttons when current_phase_idx >= phases.length', async () => {
    (invoke as ReturnType<typeof vi.fn>).mockResolvedValue(fixtureWith({ current_phase_idx: 2 }));
    render(<ProtocolPanel projectDir="/x" section="default" selfSeat={null} rosterRoles={[]} defaultExpanded={true} />);
    await waitFor(() => {
      expect(screen.getByText(/complete/i)).toBeInTheDocument();
      expect(screen.getByRole('button', { name: /pause/i })).toBeDisabled();
      expect(screen.getByRole('button', { name: /skip/i })).toBeDisabled();
      expect(screen.getByRole('button', { name: /\+15m/i })).toBeDisabled();
    });
  });

  it('empty phase plan renders the "set one" empty-state message', async () => {
    (invoke as ReturnType<typeof vi.fn>).mockResolvedValue(
      fixtureWith({ phases: [], current_phase_idx: 0 }),
    );
    render(<ProtocolPanel projectDir="/x" section="default" selfSeat={null} rosterRoles={[]} defaultExpanded={true} />);
    await waitFor(() => {
      expect(screen.getByText(/No phase plan/i)).toBeInTheDocument();
    });
  });
});
