// R1 — ProtocolPanel render integration test.
// Spec: .vaak/al-slice-3-4-design.md §7 R1 + #954 vote-3 gate (AssemblyBanner
// deletion gates on R1 passing).
//
// Mocks @tauri-apps/api/core invoke + @tauri-apps/api/event listen so the
// hook resolves without a real Tauri runtime. Asserts the panel renders the
// expected diagram regions (phase row / preset / mic line / queue / roster).

import { describe, expect, it, vi, beforeEach } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import '@testing-library/jest-dom/vitest';

// Mock the Tauri APIs the hook uses.
vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}));
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(async () => () => {}),
}));

import { invoke } from '@tauri-apps/api/core';
import { ProtocolPanel } from '../ProtocolPanel';

const FIXTURE_RESPONSE = {
  section: 'default',
  protocol: {
    schema_version: 1,
    rev: 12,
    preset: 'Debate',
    floor: {
      mode: 'reactive',
      current_speaker: 'architect:0',
      queue: ['developer:0', 'tester:0'],
      rotation_order: [],
      threshold_ms: 60_000,
      started_at: '2026-04-28T00:00:00Z',
    },
    consensus: { mode: 'none', round: null, phase: null, submissions: [] },
    phase_plan: { phases: [], current_phase_idx: 0, paused_at: null, paused_total_secs: 0 },
    scopes: { floor: 'instance', consensus: 'role' },
    last_writer_seat: 'architect:0',
    last_writer_action: 'transfer_mic',
    rev_at: '2026-04-28T20:00:00Z',
  },
  heartbeats: {
    'architect:0': {
      last_active_at_ms: Date.now(),
      last_drafting_at_ms: 0,
      last_heartbeat: null,
      connected: true,
    },
    'developer:0': {
      last_active_at_ms: Date.now() - 30_000,
      last_drafting_at_ms: 0,
      last_heartbeat: null,
      connected: true,
    },
    'tester:0': {
      last_active_at_ms: Date.now() - 30_000,
      last_drafting_at_ms: 0,
      last_heartbeat: null,
      connected: true,
    },
  },
};

beforeEach(() => {
  (invoke as ReturnType<typeof vi.fn>).mockReset();
  (invoke as ReturnType<typeof vi.fn>).mockResolvedValue(FIXTURE_RESPONSE);
});

describe('ProtocolPanel — R1 render integration (#954 vote-3 gate)', () => {
  it('renders skeleton until get_protocol_cmd resolves', async () => {
    (invoke as ReturnType<typeof vi.fn>).mockImplementation(() => new Promise(() => {})); // never resolves
    render(
      <ProtocolPanel
        projectDir="/tmp/x"
        section="default"
        selfSeat={null}
        rosterRoles={['architect', 'developer', 'tester']}
        defaultExpanded={true}
      />,
    );
    expect(screen.getByText(/Loading protocol/i)).toBeInTheDocument();
  });

  it('renders the protocol panel ARIA region after fixture loads', async () => {
    render(
      <ProtocolPanel
        projectDir="/tmp/x"
        section="default"
        selfSeat={null}
        rosterRoles={['architect', 'developer', 'tester']}
        defaultExpanded={true}
      />,
    );
    await waitFor(() => {
      expect(
        screen.getByRole('region', { name: /Protocol panel for section default/i }),
      ).toBeInTheDocument();
    });
  });

  it('renders preset name in CompactMicLine after fixture loads', async () => {
    render(
      <ProtocolPanel
        projectDir="/tmp/x"
        section="default"
        selfSeat={null}
        rosterRoles={['architect', 'developer', 'tester']}
        defaultExpanded={true}
      />,
    );
    await waitFor(() => {
      // Compact header shows "· Debate" after the speaker label.
      // Floor mode + consensus mode are no longer surfaced inline post-#1100.
      expect(screen.getByText(/Debate/)).toBeInTheDocument();
    });
  });

  it('renders the current speaker (architect:0)', async () => {
    render(
      <ProtocolPanel
        projectDir="/tmp/x"
        section="default"
        selfSeat={null}
        rosterRoles={['architect', 'developer', 'tester']}
        defaultExpanded={true}
      />,
    );
    await waitFor(() => {
      // Multiple matches OK — once in MicLine, once in Roster.
      expect(screen.getAllByText(/architect:0/).length).toBeGreaterThan(0);
    });
  });

  it('renders the queue strip with 2 items', async () => {
    render(
      <ProtocolPanel
        projectDir="/tmp/x"
        section="default"
        selfSeat={null}
        rosterRoles={['architect', 'developer', 'tester']}
        defaultExpanded={true}
      />,
    );
    await waitFor(() => {
      expect(screen.getByText(/Queue \(2\)/)).toBeInTheDocument();
    });
  });

  // 'renders Roster section header' DELETED per human #1100 — ProtocolPanel
  // no longer renders the Roster grid (CollabTab owns the roster cards).
});
