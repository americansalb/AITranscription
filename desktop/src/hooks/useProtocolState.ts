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
    assembly_active?: boolean;
    phase?: 'planning' | 'execution';
    mic_passing_mode?: 'rotation' | 'hand_raise' | 'moderator';
    moderator?: string | null;
    hand_queue?: string[];
    plan_path?: string | null;
    plan_hash?: string | null;
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
 * Translate raw protocol_mutate error envelopes into plain-English
 * messages for the UI (per dev-chall #1135 + memory #27 jargon sweep).
 * Raw `[Code]` form is preserved in console.warn so the team retains
 * debug context.
 */
function friendlyError(raw: string): string {
  if (raw.includes('[StaleRev]')) {
    return 'Someone else updated the panel state — the system is catching up. Try again in a moment.';
  }
  if (raw.includes('[MissingRev]')) {
    return 'Internal error: panel state revision missing. Please reload.';
  }
  if (raw.includes('[NotPermitted]')) {
    return 'You can\'t do that right now. Common causes: someone else has the mic, or you\'re trying to take an action only the speaker can do.';
  }
  if (raw.includes('[StuckGateNotPassed]')) {
    return 'The current speaker is still active. The mic only frees up after they\'ve been silent for a minute.';
  }
  if (raw.includes('[SeatNotFound]')) {
    return 'That seat isn\'t in the active roster.';
  }
  if (raw.includes('[InvalidArgs]')) {
    return 'Internal error: bad arguments. Check the console for details.';
  }
  if (raw.includes('[InvalidAction]')) {
    return 'That action isn\'t available right now.';
  }
  if (raw.includes('[Slice5Unimplemented]') || raw.includes('[Slice6Unimplemented]')) {
    return 'That feature isn\'t fully wired yet.';
  }
  if (raw.includes('[InternalError]')) {
    return 'Something failed inside the system. Check the console.';
  }
  if (raw.includes('[RevisePlanForbidden]')) {
    return 'Only architect, manager, or human can revise an accepted plan.';
  }
  if (raw.includes('[PlanPathOutsideDesignNotes]')) {
    return 'Plan files must live under .vaak/design-notes/.';
  }
  if (raw.includes('[PlanPathNotMarkdown]')) {
    return 'Plan files must be .md (markdown).';
  }
  if (raw.includes('[PlanPathMissing]')) {
    return 'That plan file doesn\'t exist or isn\'t readable.';
  }
  if (raw.includes('[PlanScopeBlockMissing]')) {
    return 'Plan file needs a `<!-- scope: path1 path2 -->` block (use `<!-- scope: * -->` for unrestricted).';
  }
  if (raw.includes('[UnknownMicMechanism]')) {
    return 'That mic-passing mode isn\'t recognized.';
  }
  return raw;
}

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

  // Ref bridges closure freshness into the one-time listener below. Without
  // this, the listener's useEffect depended on [refresh] — which recreates
  // when projectDir or section changes — causing unlisten → re-listen. Any
  // `protocol_changed` event emitted during that async re-bind gap was
  // dropped, leaving the UI deaf to mutations that landed exactly when the
  // hook was re-subscribing (e.g. set_moderator / set_mic_passing fired
  // right after a section switch). Toggling assembly forced a NEW emit AFTER
  // the listener had stabilized, which is why the human's workaround
  // (toggle off/on) appeared to "fix" stale UI. Class: listener re-bind race.
  const refreshRef = useRef(refresh);
  refreshRef.current = refresh;

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
  // event payload). ONE-TIME bind: deps intentionally empty so the listener
  // is never unsubscribed/rebound during the hook's lifetime. refreshRef
  // above carries the latest refresh closure into the listener.
  useEffect(() => {
    let unlisten: UnlistenFn | null = null;
    let cancelled = false;
    void (async () => {
      try {
        unlisten = await listen('protocol_changed', () => {
          void refreshRef.current();
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
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

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
        // Translate raw [Code] error envelopes into plain-English messages
        // before surfacing to the UI (memory #27 + dev-chall #1135 jargon
        // sweep). Keep the raw msg in the console for debugging.
        const friendly = friendlyError(msg);
        console.warn(`[useProtocolState] mutate('${action}') failed: ${msg} → '${friendly}'`);
        setLastError(friendly);
        // [StaleRev] recovery (evil-arch #952 RATIFIED in #954): re-invoke
        // get_protocol DIRECTLY rather than waiting for the best-effort
        // protocol_changed push. If the push event was dropped (Tauri IPC
        // hiccup, hidden tab, OS suspend mid-emit), the local rev would
        // stay stale forever and every subsequent mutate would StaleRev-
        // loop with no UI signal. Belt-and-suspenders with the listener.
        if (msg.includes('[StaleRev]')) {
          void refresh();
        }
        return null;
      }
    },
    [projectDir, refresh],
  );

  return { state, heartbeats, loaded, lastError, mutate };
}
