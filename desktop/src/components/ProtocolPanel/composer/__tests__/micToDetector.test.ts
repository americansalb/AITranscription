// R5 — composer mic_to detector test set.
// Spec: .vaak/al-slice-3-4-design.md §5 + §9 vote 5 ratified at #954.
// 18 cases ratified by team: 12 (architect #949) + 2 (dev #950)
//                          + 1 (evil-arch #952) + 3 (dev-chall #953).
//
// Hard rule under test: regex is HINT only. `metadata.mic_to` lands ONLY when
// the composer's confirm UI is clicked — the detector is the classifier; the
// click is the gate. These tests cover classification semantics, NOT whether
// metadata is set (that's the UI's responsibility).

import { describe, expect, it } from 'vitest';
import { detectMicTo, type SeatRef } from '../micToDetector';

const seats = (
  ...specs: Array<[string, number, boolean]>
): SeatRef[] => specs.map(([role, instance, connected]) => ({ role, instance, connected }));

describe('detectMicTo — R5 18 cases (ratified #954)', () => {
  // 1. Explicit role:instance
  it('1. explicit role:0 → resolved exact', () => {
    const c = detectMicTo(
      'Mic to architect:0 for review',
      seats(['architect', 0, true], ['developer', 0, true]),
      null,
      null,
    );
    expect(c?.classification).toBe('resolved');
    expect(c?.resolved).toBe('architect:0');
  });

  // 2. Bare role + one active instance
  it('2. bare role with one active instance → resolved', () => {
    const c = detectMicTo(
      'Mic to architect — your turn',
      seats(['architect', 0, true]),
      null,
      null,
    );
    expect(c?.classification).toBe('resolved');
    expect(c?.resolved).toBe('architect:0');
  });

  // 3. Bare role + multiple active instances → ambiguous
  it('3. bare role with multiple active instances → ambiguous', () => {
    const c = detectMicTo(
      'Mic to dev',
      seats(['dev', 0, true], ['dev', 1, true]),
      null,
      null,
    );
    expect(c?.classification).toBe('ambiguous');
    expect(c?.resolved).toBeNull();
    expect(c?.candidates).toEqual(['dev:0', 'dev:1']);
  });

  // 4. Vacant role (not in roster at all) → vacant
  it('4. vacant role → vacant fallthrough', () => {
    const c = detectMicTo(
      'Mic to manager',
      seats(['architect', 0, true]),
      null,
      null,
    );
    expect(c?.classification).toBe('vacant');
    expect(c?.resolved).toBeNull();
  });

  // 5. Non-role word → null (no regex match required)
  it('5. body without "Mic to ..." → null', () => {
    const c = detectMicTo(
      'I think the architect should review this',
      seats(['architect', 0, true]),
      null,
      null,
    );
    expect(c).toBeNull();
  });

  // 6. Case insensitivity
  it('6. case insensitive — "MIC TO" / "mic to" / "Mic To"', () => {
    expect(detectMicTo('MIC TO architect', seats(['architect', 0, true]), null, null)?.resolved).toBe('architect:0');
    expect(detectMicTo('mic to architect', seats(['architect', 0, true]), null, null)?.resolved).toBe('architect:0');
    expect(detectMicTo('Mic To architect', seats(['architect', 0, true]), null, null)?.resolved).toBe('architect:0');
  });

  // 7. No match returns null
  it('7. no match → null', () => {
    expect(detectMicTo('hello world', [], null, null)).toBeNull();
  });

  // 8. Trailing punctuation does not pollute the role match
  it('8. trailing comma not captured as part of role', () => {
    const c = detectMicTo(
      'Mic to architect, please',
      seats(['architect', 0, true]),
      null,
      null,
    );
    expect(c?.rawRole).toBe('architect');
    expect(c?.resolved).toBe('architect:0');
  });

  // 9. False positive — `architect's approach` — regex matches `architect`
  // but classification is still "resolved"; metadata gating is the UI's
  // job (not the detector's).
  it('9. body "Mic to architect\'s approach" — classifies as resolved (UI gates via confirm-click)', () => {
    const c = detectMicTo(
      "Mic to architect's approach",
      seats(['architect', 0, true]),
      null,
      null,
    );
    // Detector must classify; UI must NOT auto-write metadata without click.
    expect(c).not.toBeNull();
    expect(c?.classification).toBe('resolved');
    // The defense lives in the click-to-confirm UI, not here. This test
    // documents the contract: detector outputs classification only.
  });

  // 10. Multiple "Mic to ..." in one body — first-match-wins (dev-chall #953
  // ratified at #954).
  it('10. multiple "Mic to ..." — first-match-wins', () => {
    const c = detectMicTo(
      'Mic to architect now, then Mic to developer later',
      seats(['architect', 0, true], ['developer', 0, true]),
      null,
      null,
    );
    expect(c?.rawRole).toBe('architect');
    expect(c?.resolved).toBe('architect:0');
  });

  // 11. Bare role with NO active instances (all disconnected) →
  // disconnected (NOT vacant — evil-arch #952).
  it('11. bare role all-disconnected → disconnected hint', () => {
    const c = detectMicTo(
      'Mic to dev',
      seats(['dev', 0, false], ['dev', 1, false]),
      null,
      null,
    );
    expect(c?.classification).toBe('disconnected');
    expect(c?.resolved).toBeNull();
    expect(c?.candidates).toEqual(['dev:0', 'dev:1']);
  });

  // 12. Self-target — resolved seat == selfSeat == current_speaker → no-op
  it('12. self-target → self_target classification', () => {
    const c = detectMicTo(
      'Mic to architect',
      seats(['architect', 0, true]),
      'architect:0',
      'architect:0',
    );
    expect(c?.classification).toBe('self_target');
    expect(c?.resolved).toBe('architect:0');
  });

  // 13. Self-target only fires when selfSeat == current_speaker
  // (mentioning own role when not speaker is a normal resolved case).
  it('13. self mention when not current_speaker → resolved (not self_target)', () => {
    const c = detectMicTo(
      'Mic to developer',
      seats(['architect', 0, true], ['developer', 0, true]),
      'developer:0',
      'architect:0', // architect speaking, dev mentioning self
    );
    expect(c?.classification).toBe('resolved');
  });

  // 14. Explicit role:N where N doesn't exist → vacant
  it('14. explicit instance not present → vacant', () => {
    const c = detectMicTo(
      'Mic to dev:5',
      seats(['dev', 0, true], ['dev', 1, true]),
      null,
      null,
    );
    expect(c?.classification).toBe('vacant');
    expect(c?.rawInstance).toBe(5);
  });

  // 15. Explicit role:N where N exists but disconnected
  it('15. explicit role:0 disconnected → disconnected', () => {
    const c = detectMicTo(
      'Mic to architect:0',
      seats(['architect', 0, false]),
      null,
      null,
    );
    expect(c?.classification).toBe('disconnected');
    expect(c?.resolved).toBeNull();
    expect(c?.candidates).toEqual(['architect:0']);
  });

  // 16. Hyphenated role slugs (project_join allows them)
  it('16. hyphenated role slug like "evil-architect"', () => {
    const c = detectMicTo(
      'Mic to evil-architect',
      seats(['evil-architect', 0, true]),
      null,
      null,
    );
    expect(c?.rawRole).toBe('evil-architect');
    expect(c?.resolved).toBe('evil-architect:0');
  });

  // 17. Body starts with "Mic to ..." — anchor at line start
  it('17. body starts with "Mic to architect"', () => {
    const c = detectMicTo(
      'Mic to architect — handing off',
      seats(['architect', 0, true]),
      null,
      null,
    );
    expect(c?.resolved).toBe('architect:0');
  });

  // 18. Whitespace before "Mic to" — multiline body
  it('18. whitespace + "Mic to" mid-paragraph', () => {
    const c = detectMicTo(
      'I am done.\nMic to architect for the next phase.',
      seats(['architect', 0, true]),
      null,
      null,
    );
    expect(c?.resolved).toBe('architect:0');
  });
});
