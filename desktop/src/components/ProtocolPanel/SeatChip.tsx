// SeatChip — pure component rendering one of six visual states from
// (seat, protocol, heartbeats). State table per al-architecture-diagram.md §1.1.
//
// Hard cuts at threshold boundaries (spec §5.3) — no fade. Reduced-motion
// gates pulse animations via CSS @media (prefers-reduced-motion: reduce).

import { useMemo } from 'react';
import type { Protocol, Heartbeat } from '../../hooks/useProtocolState';

export type SeatChipState =
  | 'holding'
  | 'stuck'
  | 'drafting'
  | 'online'
  | 'disconnected'
  | 'vacant';

export type SeatChipProps = {
  seatLabel: string; // "role:N"
  protocol: Protocol;
  heartbeat: Heartbeat | undefined;
  isVacant: boolean; // role exists in roster, no seat joined
  isSelf: boolean;
  now: number; // for freshness — refreshed by parent's 1Hz tick
  onClick?: () => void;
};

export function computeSeatChipState(
  seatLabel: string,
  protocol: Protocol,
  heartbeat: Heartbeat | undefined,
  isVacant: boolean,
  now: number,
): SeatChipState {
  if (isVacant) return 'vacant';
  if (!heartbeat || !heartbeat.connected) return 'disconnected';

  const isCurrent = protocol.floor.current_speaker === seatLabel;
  const lastActive = heartbeat.last_active_at_ms ?? 0;
  const lastDrafting = heartbeat.last_drafting_at_ms ?? 0;
  const threshold = protocol.floor.threshold_ms || 60_000;
  const activeAge = now - lastActive;
  const draftingAge = now - lastDrafting;

  if (isCurrent) {
    // holding vs stuck: <10s solid pulse vs >threshold amber
    return activeAge > threshold ? 'stuck' : 'holding';
  }

  // drafting: keep_alive within threshold×3 window (some grace beyond the gate)
  if (draftingAge < threshold * 3) return 'drafting';

  return 'online';
}

const STATE_ICON: Record<SeatChipState, string> = {
  holding: '🎙',
  stuck: '⚠',
  drafting: '✎',
  online: '●',
  disconnected: '⊘',
  vacant: '·',
};

const STATE_DESCRIPTION: Record<SeatChipState, string> = {
  holding: 'Has the mic',
  stuck: 'Silent past 60s — anyone can grab',
  drafting: 'Composing',
  online: 'Online · click to raise hand',
  disconnected: 'Disconnected',
  vacant: 'Vacant',
};

export function SeatChip({
  seatLabel,
  protocol,
  heartbeat,
  isVacant,
  isSelf,
  now,
  onClick,
}: SeatChipProps) {
  const chipState = useMemo(
    () => computeSeatChipState(seatLabel, protocol, heartbeat, isVacant, now),
    [seatLabel, protocol, heartbeat, isVacant, now],
  );

  const lastActive = heartbeat?.last_active_at_ms ?? 0;
  const ageSecs = lastActive > 0 ? Math.max(0, Math.floor((now - lastActive) / 1000)) : null;
  const ariaLabel =
    `${seatLabel}, ${STATE_DESCRIPTION[chipState]}` +
    (ageSecs !== null ? `, last active ${ageSecs}s ago` : '');

  const inQueue = protocol.floor.queue.includes(seatLabel);

  return (
    <button
      type="button"
      className={`seat-chip seat-chip--${chipState}${isSelf ? ' seat-chip--self' : ''}${inQueue ? ' seat-chip--queued' : ''}`}
      aria-label={ariaLabel}
      aria-busy={chipState === 'holding' || chipState === 'drafting'}
      onClick={onClick}
      disabled={!onClick}
    >
      <span className="seat-chip__icon" aria-hidden="true">
        {inQueue ? '🙋' : STATE_ICON[chipState]}
      </span>
      <span className="seat-chip__col">
        <span className="seat-chip__label">
          {seatLabel}
          {isSelf ? ' (you)' : ''}
        </span>
        <span className="seat-chip__state">
          {inQueue ? `In queue · #${protocol.floor.queue.indexOf(seatLabel) + 1}` : STATE_DESCRIPTION[chipState]}
        </span>
      </span>
    </button>
  );
}
