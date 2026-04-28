// MicToHint — inline composer hint UI for the mic_to detector.
// Spec: §4.3 + §5 of al-architecture-diagram.md.
// Three hint variants matching detector classification (dev-chall #953):
//   - resolved: "Detected: ROLE — click to confirm"
//   - ambiguous: "Multiple candidates — pick one"
//   - vacant: "ROLE is vacant — fall through to queue head"
//   - disconnected: "ROLE is offline — fall through with note"
//   - self_target: "self-address is no-op — composer warns"
//
// Hard rule: nothing happens without an explicit confirm click.

import { useState } from 'react';
import type { MicToCandidate } from './micToDetector';

export type MicToHintProps = {
  candidate: MicToCandidate;
  /** Called when user confirms a specific seat. Receives the resolved label. */
  onConfirm: (seatLabel: string) => void;
  /** Called when user dismisses the hint (X button). */
  onDismiss: () => void;
};

export function MicToHint({ candidate, onConfirm, onDismiss }: MicToHintProps) {
  const [pickerOpen, setPickerOpen] = useState(false);

  if (candidate.classification === 'vacant') {
    return (
      <div className="mic-to-hint mic-to-hint--vacant" role="note">
        <span>
          <b>{candidate.rawRole}{candidate.rawInstance !== null ? `:${candidate.rawInstance}` : ''}</b>{' '}
          is vacant — message will fall through to queue head if any.
        </span>
        <button type="button" onClick={onDismiss} aria-label="Dismiss hint">×</button>
      </div>
    );
  }

  if (candidate.classification === 'disconnected') {
    return (
      <div className="mic-to-hint mic-to-hint--disconnected" role="note">
        <span>
          <b>{candidate.rawRole}</b> is offline ({candidate.candidates.length}{' '}
          disconnected). Message will fall through to queue head.
        </span>
        <button type="button" onClick={onDismiss} aria-label="Dismiss hint">×</button>
      </div>
    );
  }

  if (candidate.classification === 'self_target') {
    return (
      <div className="mic-to-hint mic-to-hint--self" role="note">
        <span>
          You are addressing yourself — no mic transfer (current speaker stays).
        </span>
        <button type="button" onClick={onDismiss} aria-label="Dismiss hint">×</button>
      </div>
    );
  }

  if (candidate.classification === 'ambiguous' && pickerOpen) {
    return (
      <div className="mic-to-hint mic-to-hint--ambiguous" role="note">
        <span>Pick one:</span>
        {candidate.candidates.map((seat) => (
          <button
            key={seat}
            type="button"
            className="mic-to-hint__pick"
            onClick={() => onConfirm(seat)}
          >
            {seat}
          </button>
        ))}
        <button type="button" onClick={onDismiss} aria-label="Cancel">×</button>
      </div>
    );
  }

  if (candidate.classification === 'ambiguous') {
    return (
      <div className="mic-to-hint mic-to-hint--ambiguous" role="note">
        <span>
          💡 <b>{candidate.rawRole}</b> has {candidate.candidates.length} active —{' '}
          <button type="button" className="mic-to-hint__confirm" onClick={() => setPickerOpen(true)}>
            pick one ▸
          </button>
        </span>
        <button type="button" onClick={onDismiss} aria-label="Dismiss hint">×</button>
      </div>
    );
  }

  // resolved
  if (candidate.resolved) {
    return (
      <div className="mic-to-hint mic-to-hint--resolved" role="note">
        <span>
          💡 Detected: <b>{candidate.resolved}</b> —{' '}
          <button
            type="button"
            className="mic-to-hint__confirm"
            onClick={() => onConfirm(candidate.resolved!)}
          >
            click to confirm ▸
          </button>
        </span>
        <button type="button" onClick={onDismiss} aria-label="Dismiss hint">×</button>
      </div>
    );
  }

  return null;
}
