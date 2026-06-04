// R2 — SeatChip state-mapping unit test (per #954 4-commit plan).
// Spec: .vaak/al-architecture-diagram.md §1.1 + .vaak/al-slice-3-4-design.md §4.
// Pure-function classification — no React render needed for this layer.

import { describe, expect, it } from 'vitest';
import { computeSeatChipState, describeSeatChipState } from '../SeatChip';
import type { Heartbeat, Protocol } from '../../../hooks/useProtocolState';

const baseProtocol: Protocol = {
  schema_version: 1,
  rev: 0,
  preset: 'Debate',
  floor: {
    mode: 'reactive',
    current_speaker: null,
    queue: [],
    rotation_order: [],
    threshold_ms: 60_000,
    started_at: null,
  },
  consensus: { mode: 'none', round: null, phase: null, submissions: [] },
  phase_plan: { phases: [], current_phase_idx: 0, paused_at: null, paused_total_secs: 0 },
  scopes: { floor: 'instance', consensus: 'role' },
  last_writer_seat: null,
  last_writer_action: null,
  rev_at: null,
};

const NOW = 1_700_000_000_000;

describe('computeSeatChipState — six visual states (R2)', () => {
  it('vacant: isVacant=true returns "vacant" regardless of heartbeat', () => {
    expect(
      computeSeatChipState('manager:0', baseProtocol, undefined, true, NOW),
    ).toBe('vacant');
  });

  it('disconnected: connected=false', () => {
    const hb: Heartbeat = { last_active_at_ms: NOW, last_drafting_at_ms: 0, last_heartbeat: null, connected: false };
    expect(
      computeSeatChipState('manager:0', baseProtocol, hb, false, NOW),
    ).toBe('disconnected');
  });

  it('disconnected: heartbeat undefined', () => {
    expect(
      computeSeatChipState('manager:0', baseProtocol, undefined, false, NOW),
    ).toBe('disconnected');
  });

  it('holding: seat IS current_speaker AND active <threshold', () => {
    const proto: Protocol = { ...baseProtocol, floor: { ...baseProtocol.floor, current_speaker: 'architect:0' } };
    const hb: Heartbeat = { last_active_at_ms: NOW - 5_000, last_drafting_at_ms: 0, last_heartbeat: null, connected: true };
    expect(
      computeSeatChipState('architect:0', proto, hb, false, NOW),
    ).toBe('holding');
  });

  it('stuck: seat IS current_speaker AND active >threshold', () => {
    const proto: Protocol = { ...baseProtocol, floor: { ...baseProtocol.floor, current_speaker: 'architect:0' } };
    const hb: Heartbeat = { last_active_at_ms: NOW - 90_000, last_drafting_at_ms: 0, last_heartbeat: null, connected: true };
    expect(
      computeSeatChipState('architect:0', proto, hb, false, NOW),
    ).toBe('stuck');
  });

  it('drafting: NOT current_speaker, last_drafting recent (within threshold×3)', () => {
    const hb: Heartbeat = { last_active_at_ms: 0, last_drafting_at_ms: NOW - 30_000, last_heartbeat: null, connected: true };
    expect(
      computeSeatChipState('developer:0', baseProtocol, hb, false, NOW),
    ).toBe('drafting');
  });

  it('online: connected, no draft activity, not current_speaker', () => {
    const hb: Heartbeat = { last_active_at_ms: NOW - 200_000, last_drafting_at_ms: 0, last_heartbeat: null, connected: true };
    expect(
      computeSeatChipState('developer:0', baseProtocol, hb, false, NOW),
    ).toBe('online');
  });

  it('hard cut: 59s active = holding, 61s active = stuck (no fade)', () => {
    const proto: Protocol = { ...baseProtocol, floor: { ...baseProtocol.floor, current_speaker: 'x:0' } };
    const justUnder: Heartbeat = { last_active_at_ms: NOW - 59_000, last_drafting_at_ms: 0, last_heartbeat: null, connected: true };
    const justOver: Heartbeat = { last_active_at_ms: NOW - 61_000, last_drafting_at_ms: 0, last_heartbeat: null, connected: true };
    expect(computeSeatChipState('x:0', proto, justUnder, false, NOW)).toBe('holding');
    expect(computeSeatChipState('x:0', proto, justOver, false, NOW)).toBe('stuck');
  });

  it('drafting trumps stuck for non-speakers (compose-while-stuck UX preserved)', () => {
    // Not current speaker; last_active is stale; last_drafting is fresh.
    // The "stuck" classification only applies to the current speaker.
    const hb: Heartbeat = { last_active_at_ms: NOW - 200_000, last_drafting_at_ms: NOW - 5_000, last_heartbeat: null, connected: true };
    expect(
      computeSeatChipState('developer:0', baseProtocol, hb, false, NOW),
    ).toBe('drafting');
  });
});

describe('describeSeatChipState — stuck label derives from threshold_ms', () => {
  it('uses the actual threshold, not a hardcoded 60s (90s preset)', () => {
    const proto: Protocol = { ...baseProtocol, floor: { ...baseProtocol.floor, threshold_ms: 90_000 } };
    expect(describeSeatChipState('stuck', proto)).toBe('Silent past 90s — anyone can grab');
  });

  it('uses 60s for the default threshold', () => {
    expect(describeSeatChipState('stuck', baseProtocol)).toBe('Silent past 60s — anyone can grab');
  });

  it('falls back to 60s (never "0s"/"NaNs") when threshold_ms is absent/zero', () => {
    // Guards the reviewer-flagged edge: a falsy threshold_ms must not render
    // "Silent past 0s" or "Silent past NaNs" — the lying-label bug one layer down.
    const zero: Protocol = { ...baseProtocol, floor: { ...baseProtocol.floor, threshold_ms: 0 } };
    expect(describeSeatChipState('stuck', zero)).toBe('Silent past 60s — anyone can grab');
    const missing: Protocol = { ...baseProtocol, floor: { ...baseProtocol.floor, threshold_ms: undefined as unknown as number } };
    expect(describeSeatChipState('stuck', missing)).toBe('Silent past 60s — anyone can grab');
  });

  it('non-stuck states are unaffected by threshold', () => {
    expect(describeSeatChipState('holding', baseProtocol)).toBe('Has the mic');
    expect(describeSeatChipState('online', baseProtocol)).toBe('Online · click to raise hand');
  });
});
