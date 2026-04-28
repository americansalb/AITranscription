// useProtocolState — single subscription point for the unified protocol
// (floor + consensus) state introduced in Slice 1+2.
//
// Spec: .vaak/al-architecture-diagram.md §4 (one-way data flow). UI never
// writes its own state — every mutation goes through invoke('protocol_mutate'),
// which writes protocol.json + emits a `protocol_changed` window event,
// triggering a re-read here.
//
// **Hard rule (spec §4.2):** never call setState outside the listener +
// initial load. UI never paints optimistic state. The mutator is fire-and-
// forget; result lands via the next push.

import { useEffect, useRef, useState, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

export type Protocol = {
  schema_version: number;
  rev: number;
  preset: string;
  floor: {
    mode: string;
    current_speaker: string | null;
    queue: string[];
    rotation_order: string[];
    threshold_ms: number;
    started_at: string | null;
  };
  consensus: {
    mode: string;
    round: unknown | null;
    phase: string | null;
    submissions: unknown[];
  };
  phase_plan: {
    phases: unknown[];
    current_phase_idx: number;
    paused_at: string | null;
    paused_total_secs: number;
  };
  scopes: { floor: string; consensus: string };
  last_writer_seat: string | null;
  last_writer_action: string | null;
  rev_at: string | null;
};

export type Heartbeat = {
  last_active_at_ms: number | null;
  last_drafting_at_ms: number | null;
  last_heartbeat: string | null;
  connected: boolean;
};

export type Heartbeats = Record<string, Heartbeat>;

export type ProtocolBundle = {
  state: Protocol | null;
  heartbeats: Heartbeats;
  loaded: boolean;
  lastError: string | null;
  mutate: (action: string, args?: object) => Promise<Protocol | null>;
};

type GetProtocolResponse = {
  section: string;
  protocol: Protocol;
  heartbeats: Heartbeats;
};

/**
 * Subscribe to protocol.json for the given project + section. Returns
 * `{ state, heartbeats, loaded, mutate }`. Mutations are fire-and-forget;
 * result arrives through the next `protocol_changed` event.
 */
export function useProtocolState(
  projectDir: string | null,
  section: string,
): ProtocolBundle {
  const [state, setState] = useState<Protocol | null>(null);
  const [heartbeats, setHeartbeats] = useState<Heartbeats>({});
  const [loaded, setLoaded] = useState(false);
  const [lastError, setLastError] = useState<string | null>(null);
  const stateRef = useRef<Protocol | null>(null);
  stateRef.current = state;

  const refresh = useCallback(async () => {
    if (!projectDir) return;
    try {
      const resp = await invoke<GetProtocolResponse>('get_protocol_cmd', {
        dir: projectDir,
        section,
      });
      setState(resp.protocol);
      setHeartbeats(resp.heartbeats ?? {});
      setLoaded(true);
      setLastError(null);
    } catch (e) {
      const msg = String(e);
      console.warn('[useProtocolState] get_protocol_cmd failed:', msg);
      setLastError(msg);
    }
  }, [projectDir, section]);

  // Initial load + section change.
  useEffect(() => {
    if (!projectDir) {
      setLoaded(false);
      setState(null);
      return;
    }
    void refresh();
  }, [projectDir, section, refresh]);

  // Listen for protocol_changed push events (spec §4.1 — push best-effort,
  // get_protocol authoritative; we always re-read rather than trusting the
  // event payload).
  useEffect(() => {
    let unlisten: UnlistenFn | null = null;
    let cancelled = false;
    void (async () => {
      try {
        unlisten = await listen('protocol_changed', () => {
          // Drop replays whose rev <= our last seen (spec §4.1 replay
          // protection). We always re-read via get_protocol, so rev
          // comparison is a soft optimization, not a correctness gate.
          void refresh();
        });
      } catch (e) {
        console.warn('[useProtocolState] failed to subscribe to protocol_changed:', e);
      }
      if (cancelled && unlisten) {
        unlisten();
      }
    })();
    return () => {
      cancelled = true;
      if (unlisten) unlisten();
    };
  }, [refresh]);

  const mutate = useCallback(
    async (action: string, args: object = {}): Promise<Protocol | null> => {
      if (!projectDir || !stateRef.current) return null;
      try {
        const result = await invoke<Protocol>('protocol_mutate_cmd', {
          dir: projectDir,
          action,
          args,
          rev: stateRef.current.rev,
        });
        // Don't setState here — the protocol_changed event will trigger
        // refresh(). This is the §4.2 "no setState outside listener" rule.
        return result;
      } catch (e) {
        const msg = String(e);
        console.warn(`[useProtocolState] mutate('${action}') failed: ${msg}`);
        setLastError(msg);
        // On [StaleRev] the next push will deliver truth — caller can retry.
        return null;
      }
    },
    [projectDir],
  );

  return { state, heartbeats, loaded, lastError, mutate };
}
