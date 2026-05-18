// ProjectDirContext — single in-memory source of truth for the
// `vaak_collab_project_dir` localStorage key, owning both the read and
// write paths for that one shared value.
//
// Origin (architect msg 5249 + evil-architect msg 5246 F-EA-CTR-A):
// Path B (commit 2fe16e8) closed the divergent-READER class for this key
// via the shared `persistedState.ts` helper. The CollabTab restructure
// spec (architect msg 5238 §Change C) would dual-mount RolesTab — once as
// the standalone Tauri tab, once embedded inside CollabTab's Team Section
// "Manage Roles" tab. Two simultaneously-mounted components reading + writing
// the same localStorage key opens a divergent-WRITER bug: localStorage does
// not broadcast `storage` events to same-process listeners, so one mount's
// write is invisible to the other until the next render trigger.
//
// This context closes that future-class-of-bug by:
//   - One useState instance (single in-memory value)
//   - One writer function (the context's setProjectDir) — both mounts call
//     this; both observe the updated state synchronously via React's
//     normal re-render cycle
//   - localStorage I/O routed through `persistedState.ts` helpers so the
//     encoding stays symmetric with the rest of the app
//
// Scope boundary (per ui-architect msg 5257 F-UIA-CTR-6): this context
// exposes ONLY persistence-bound state (`projectDir` + `setProjectDir`).
// Component-local UI state — scroll position, search filter, expanded
// tree node set, focus state — stays in `useState` inside each consumer
// mount. Mixing UI state into this context would re-render every consumer
// on every UI tweak, which is exactly the cascade F-EA-CTR-B warned
// against and which `useMemo` on the value object exists to prevent.
//
// Re-render cascade mitigation (per evil-architect msg 5265 F-EA-CTR-B +
// architect msg 5269 acceptance): the context value object is wrapped in
// `useMemo` and the setter is wrapped in `useCallback` with empty deps.
// Result: the value reference is stable across re-renders unless
// `projectDir` actually changes. Consumers that subscribe via the hook
// re-render only when `projectDir` changes; consumers that don't
// subscribe (never call `useProjectDir()`) do not re-render at all.
// This is the same pattern Dan Abramov documents for shared-state
// contexts; sufficient for v1 given project_dir changes rarely (once
// per session when the user switches projects). If a future shared
// context exposes a value that mutates more often, split into
// read/write contexts then.

import { createContext, useCallback, useContext, useMemo, useState, type ReactNode } from "react";
import { isString, loadJSON, saveJSON } from "../lib/persistedState";

const PROJECT_DIR_STORAGE_KEY = "vaak_collab_project_dir";

interface ProjectDirContextValue {
  projectDir: string;
  setProjectDir: (dir: string) => void;
}

const ProjectDirContext = createContext<ProjectDirContextValue | null>(null);

interface ProjectDirProviderProps {
  children: ReactNode;
}

export function ProjectDirProvider({ children }: ProjectDirProviderProps) {
  const [projectDir, setProjectDirState] = useState<string>(() =>
    loadJSON<string>(PROJECT_DIR_STORAGE_KEY, "", isString),
  );

  // Empty-string semantics match the prior CollabTab inline `persistDir`:
  // a non-empty dir is JSON-encoded via `saveJSON`; an empty-string call
  // clears the key entirely so the next reader gets the fallback rather
  // than an empty-string match. The shared helper doesn't expose a remove
  // path (no other call site needs it), so the localStorage.removeItem
  // call stays inline here.
  const setProjectDir = useCallback((dir: string) => {
    setProjectDirState(dir);
    if (dir) {
      saveJSON(PROJECT_DIR_STORAGE_KEY, dir);
    } else {
      try {
        localStorage.removeItem(PROJECT_DIR_STORAGE_KEY);
      } catch {
        /* ignore — same fail-open posture as persistedState.saveJSON */
      }
    }
  }, []);

  const value = useMemo<ProjectDirContextValue>(
    () => ({ projectDir, setProjectDir }),
    [projectDir, setProjectDir],
  );

  return <ProjectDirContext.Provider value={value}>{children}</ProjectDirContext.Provider>;
}

/**
 * Throw-if-no-provider pattern (per evil-architect msg 5265 F-EA-CTR-C):
 * the default context value would otherwise need to be either `null` (and
 * every consumer would null-check) or a fallback no-op pair (and a missing
 * provider would silently fail to persist). Throwing surfaces the missing
 * provider at first call rather than silently breaking persistence.
 */
export function useProjectDir(): ProjectDirContextValue {
  const ctx = useContext(ProjectDirContext);
  if (!ctx) {
    throw new Error(
      "useProjectDir must be called inside a ProjectDirProvider. " +
        "Mount the provider above any component that reads or writes the project directory.",
    );
  }
  return ctx;
}
