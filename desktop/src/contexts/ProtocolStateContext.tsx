// ProtocolStateContext — single source of truth for protocol-bundle state
// across the CollabTab subtree.
//
// Origin: msg 5450 redesign Commit 1 / architect spec df90be3 / evil-arch
// F-EA-MSG5450-3 (msg 5458). The redesign moves runtime assembly state
// (mic-holder, rotation order, moderator) ONTO Team Roster cards as
// overlay badges + sort order. Today the protocol state is read at
// CollabTab via `useProtocolState(projectDir, section)` and prop-drilled
// where needed (AssemblyControls + ProtocolPanel as direct children).
//
// Roster-card decoration in Commit 2 would extend prop-drilling deep into
// the cards loop. That's brittle: two readers (card-decoration + the
// existing AssemblyControls inside the Discussion Mode band) could drift
// if either drills its own copy of state. Same class-of-bug pattern
// ProjectDirContext closed for the project_dir value (per F-EA-CTR-A in
// pre-req 8162d3f). Extract the bundle once at the provider boundary;
// every descendant reads through the hook.
//
// Scope: this context wraps the existing `useProtocolState` hook's
// ProtocolBundle (state + heartbeats + loaded + lastError + mutate).
// No new state, no new persistence — pure subscription consolidation.

import { createContext, useContext, type ReactNode } from "react";
import { useProtocolState, type ProtocolBundle } from "../hooks/useProtocolState";

const ProtocolStateContext = createContext<ProtocolBundle | null>(null);

interface ProtocolStateProviderProps {
  projectDir: string | null;
  section: string;
  children: ReactNode;
}

export function ProtocolStateProvider({
  projectDir,
  section,
  children,
}: ProtocolStateProviderProps) {
  // One subscription site for the (projectDir, section) pair. Children
  // that consume via `useProtocolStateContext()` share the same bundle
  // — no duplicate `get_protocol_cmd` invocations, no duplicate
  // `protocol_changed` listener subscriptions, no drift between readers.
  const bundle = useProtocolState(projectDir, section);
  return (
    <ProtocolStateContext.Provider value={bundle}>
      {children}
    </ProtocolStateContext.Provider>
  );
}

/**
 * Throw-if-no-provider pattern (matches `useProjectDir` per pre-req
 * 8162d3f F-EA-CTR-C TypeScript-strict guidance). Missing provider is a
 * structural bug, not a runtime fallback case — surface it loudly so
 * callers know to mount the provider rather than silently render
 * uninitialized protocol state.
 */
export function useProtocolStateContext(): ProtocolBundle {
  const ctx = useContext(ProtocolStateContext);
  if (!ctx) {
    throw new Error(
      "useProtocolStateContext must be called inside a ProtocolStateProvider. " +
        "Mount the provider above any component that reads or mutates protocol state.",
    );
  }
  return ctx;
}
