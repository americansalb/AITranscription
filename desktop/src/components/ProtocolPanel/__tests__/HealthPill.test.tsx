// HealthPill unit tests — Slice 9 follow-on per dev #1047 self-closure.
// Covers the 4-layer roll-up render + click-to-expand + per-layer
// detail rendering. Mocks invoke('get_resilience_status').

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
import { HealthPill } from '../HealthPill';

const STATUS_GREEN = {
  roll_up: 'green',
  pillars_ok: 4,
  layer1: { ok: true, label: 'Agents responding', detail: '3 of 3 agents have heartbeated recently' },
  layer2: { ok: true, label: 'Auto-recovery watchdog', detail: 'Running — will restart hung agents automatically' },
  layer3: { ok: true, label: 'Activity heartbeats', detail: 'Installed — every agent action keeps the seat alive' },
  layer4: { ok: true, label: 'Visual indicators', detail: 'This panel is rendering' },
};

const STATUS_BAD = {
  roll_up: 'bad',
  pillars_ok: 2,
  layer1: { ok: false, label: 'Agents responding', detail: '0 of 0 agents have heartbeated recently' },
  layer2: { ok: false, label: 'Auto-recovery watchdog', detail: 'Not running — auto-recovery disabled' },
  layer3: { ok: true, label: 'Activity heartbeats', detail: 'Installed' },
  layer4: { ok: true, label: 'Visual indicators', detail: 'This panel is rendering' },
};

beforeEach(() => {
  (invoke as ReturnType<typeof vi.fn>).mockReset();
});

describe('HealthPill — Slice 9 resilience-stack JOIN UI (spec §12.4)', () => {
  it('renders loading skeleton until invoke resolves', () => {
    (invoke as ReturnType<typeof vi.fn>).mockImplementation(() => new Promise(() => {}));
    render(<HealthPill projectDir="/tmp/x" />);
    expect(screen.getByText(/Stack…/)).toBeInTheDocument();
  });

  it('renders 🟢 Stack OK when all 4 pillars healthy', async () => {
    (invoke as ReturnType<typeof vi.fn>).mockResolvedValue(STATUS_GREEN);
    render(<HealthPill projectDir="/tmp/x" />);
    await waitFor(() => {
      expect(screen.getByRole('button', { name: /All systems running/ })).toBeInTheDocument();
    });
  });

  it('renders 🔴 Stack degraded when only 2 pillars OK', async () => {
    (invoke as ReturnType<typeof vi.fn>).mockResolvedValue(STATUS_BAD);
    render(<HealthPill projectDir="/tmp/x" />);
    await waitFor(() => {
      expect(screen.getByRole('button', { name: /Something's wrong/ })).toBeInTheDocument();
    });
  });

  it('clicking pill expands per-layer detail', async () => {
    (invoke as ReturnType<typeof vi.fn>).mockResolvedValue(STATUS_BAD);
    render(<HealthPill projectDir="/tmp/x" />);
    await waitFor(() => screen.getByRole('button', { name: /Something's wrong/ }));
    fireEvent.click(screen.getByRole('button', { name: /Something's wrong/ }));
    await waitFor(() => {
      expect(screen.getByText(/Auto-recovery watchdog/)).toBeInTheDocument();
      expect(screen.getByText(/Not running — auto-recovery disabled/)).toBeInTheDocument();
      expect(screen.getByText(/Installed/)).toBeInTheDocument();
    });
  });

  it('expanded detail shows ✓ for ok layers and ✗ for failing layers', async () => {
    (invoke as ReturnType<typeof vi.fn>).mockResolvedValue(STATUS_BAD);
    render(<HealthPill projectDir="/tmp/x" />);
    await waitFor(() => screen.getByRole('button', { name: /Something's wrong/ }));
    fireEvent.click(screen.getByRole('button', { name: /Something's wrong/ }));
    await waitFor(() => {
      // Layer1 + Layer2 fail in STATUS_BAD; Layer3 + Layer4 pass.
      const fails = screen.getAllByText('✗');
      const oks = screen.getAllByText('✓');
      expect(fails.length).toBe(2);
      expect(oks.length).toBe(2);
    });
  });

  it('does not invoke get_resilience_status when projectDir is null', async () => {
    render(<HealthPill projectDir={null} />);
    // Wait briefly to ensure no async invoke fires.
    await new Promise((resolve) => setTimeout(resolve, 50));
    const calls = (invoke as ReturnType<typeof vi.fn>).mock.calls.filter(
      (c) => c[0] === 'get_resilience_status',
    );
    expect(calls.length).toBe(0);
  });
});
