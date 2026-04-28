// Composer mic_to detector — pure regex+roster classification.
//
// Spec: .vaak/al-architecture-diagram.md §4.3 + §8 (multi-instance).
// Design: .vaak/al-slice-3-4-design.md §5 + §9 vote 5 (18 R5 cases).
//
// Hard rule (spec §4.3): regex is HINT only — `metadata.mic_to` is the
// authoritative field, set ONLY when the user clicks confirm. Detector
// produces a candidate; the composer hint UI renders + the user decides.
//
// First-match-wins on multiple `Mic to ...` in one body (dev-chall #953,
// ratified in tech-leader #954): we use a non-global regex and consume the
// first occurrence only. Matches the single `metadata.mic_to` field shape.

export type SeatRef = {
  role: string;
  instance: number;
  connected: boolean;
};

export type MicToClassification =
  | 'resolved' // unambiguous → one active seat selected
  | 'ambiguous' // bare role + multiple active instances → user must pick
  | 'vacant' // role doesn't exist OR has no active/disconnected instances
  | 'disconnected' // role's instance(s) exist but `connected=false`
  | 'self_target'; // resolved seat equals selfSeat AND equals current_speaker

export type MicToCandidate = {
  raw: string; // e.g. "Mic to architect"
  rawRole: string; // e.g. "architect"
  rawInstance: number | null; // e.g. 0 if "architect:0", null if bare
  resolved: string | null; // e.g. "architect:0" or null when ambiguous/vacant
  classification: MicToClassification;
  // For ambiguous: list of candidate seats the user must disambiguate.
  candidates: string[];
};

const MIC_TO_REGEX = /(?:^|\s)mic\s+to\s+([a-z][a-z0-9-]*)(?::(\d+))?/i;

/**
 * Detect a `Mic to ROLE[:N]` mention in the body. Returns null if no match.
 * Otherwise classifies the candidate using the active roster + the caller's
 * own seat. Pure function — no I/O.
 */
export function detectMicTo(
  body: string,
  seats: SeatRef[],
  selfSeat: string | null,
  currentSpeaker: string | null,
): MicToCandidate | null {
  const m = MIC_TO_REGEX.exec(body);
  if (!m) return null;

  const raw = m[0].trim();
  const rawRole = m[1].toLowerCase();
  const rawInstance = m[2] !== undefined ? Number.parseInt(m[2], 10) : null;

  const sameRole = seats.filter((s) => s.role === rawRole);
  const activeSameRole = sameRole.filter((s) => s.connected);

  // Explicit role:instance — exact match wins.
  if (rawInstance !== null) {
    const exact = sameRole.find((s) => s.instance === rawInstance);
    if (exact) {
      const resolved = `${exact.role}:${exact.instance}`;
      if (!exact.connected) {
        return {
          raw,
          rawRole,
          rawInstance,
          resolved: null,
          classification: 'disconnected',
          candidates: [resolved],
        };
      }
      const isSelfTarget =
        selfSeat !== null && resolved === selfSeat && currentSpeaker === selfSeat;
      return {
        raw,
        rawRole,
        rawInstance,
        resolved,
        classification: isSelfTarget ? 'self_target' : 'resolved',
        candidates: [resolved],
      };
    }
    // role:N requested but that instance doesn't exist at all → vacant.
    return {
      raw,
      rawRole,
      rawInstance,
      resolved: null,
      classification: 'vacant',
      candidates: [],
    };
  }

  // Bare role.
  if (activeSameRole.length === 0) {
    if (sameRole.length > 0) {
      // All instances disconnected.
      return {
        raw,
        rawRole,
        rawInstance,
        resolved: null,
        classification: 'disconnected',
        candidates: sameRole.map((s) => `${s.role}:${s.instance}`),
      };
    }
    // No instances at all (or role doesn't exist) → vacant.
    return {
      raw,
      rawRole,
      rawInstance,
      resolved: null,
      classification: 'vacant',
      candidates: [],
    };
  }

  if (activeSameRole.length === 1) {
    const only = activeSameRole[0];
    const resolved = `${only.role}:${only.instance}`;
    const isSelfTarget =
      selfSeat !== null && resolved === selfSeat && currentSpeaker === selfSeat;
    return {
      raw,
      rawRole,
      rawInstance,
      resolved,
      classification: isSelfTarget ? 'self_target' : 'resolved',
      candidates: [resolved],
    };
  }

  // Multiple active instances → user must pick.
  return {
    raw,
    rawRole,
    rawInstance,
    resolved: null,
    classification: 'ambiguous',
    candidates: activeSameRole.map((s) => `${s.role}:${s.instance}`),
  };
}
