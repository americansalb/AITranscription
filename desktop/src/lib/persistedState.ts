/**
 * persistedState — shared localStorage helper for typed-JSON values with a
 * required runtime type guard. Path B (per architect msg 5183 + 5194
 * sequencing + msg 5201 naming directive) closes the
 * F-EA-LAYOUT-LOCALSTORAGE-CLASS forward-flag evil-arch raised in msg 5043
 * + 5123 + the naming concern from msg 5200.
 *
 * The original name `projectDirStorage.ts` traced back to msg 5033 when the
 * symptom surfaced first as a project_dir-specific bug. Architect msg 5201
 * renamed pre-ship: the actual semantic is generic typed-JSON localStorage,
 * not project-dir-specific. Future readers seeing `persistedState` should
 * import from here rather than create parallel raw-localStorage call sites.
 *
 * Before this module: every panel-collapse state and every shared key had to
 * implement the JSON.stringify-on-write / JSON.parse-on-read symmetric pattern
 * inline. The original divergent-reader bug human:0 hit in msg 5029 (RolesTab
 * reading raw localStorage that CollabTab wrote JSON.stringify-wrapped) was
 * the direct symptom; the architectural close demands a single source of
 * truth so future readers can't accidentally drift.
 *
 * The Path A inline pattern that this module replaces:
 *   - 4796f5f RolesTab.tsx fix
 *   - 1c5678d Team Roster collapse
 *   - c115441 DecisionPanel collapse
 *   - 795db42 Active Claims collapse sister-fix
 *
 * All four use byte-identical try/catch + JSON.stringify/parse wrapping; this
 * module consolidates them.
 *
 * Why a runtime type-guard parameter on load:
 *   localStorage is a side-channel any extension or earlier app version can
 *   write to. A key documented as "boolean" might contain a string from an
 *   older bundle. Trusting the parsed value blindly is the same divergent-
 *   reader class we're closing. The is-valid callback lets each call site
 *   declare its expected shape and fall through to a default on mismatch.
 *
 * Why JSON.parse on every read (not just on the first call):
 *   localStorage doesn't notify on cross-tab writes. Future enhancements
 *   could subscribe to the `storage` event, but for now a re-read on the
 *   next render-trigger is sufficient (matches the existing useState lazy
 *   initializer pattern across the codebase).
 */

/**
 * Read a JSON-wrapped value from localStorage. Returns `fallback` if:
 *   - the key is absent (`getItem` returns null)
 *   - the stored value isn't valid JSON
 *   - the parsed value fails the `isValid` type guard
 *   - the synchronous read throws (e.g. localStorage disabled)
 *
 * Pair with `saveJSON` so writes and reads use the same encoding.
 */
export function loadJSON<T>(
  key: string,
  fallback: T,
  isValid: (value: unknown) => value is T,
): T {
  try {
    const stored = localStorage.getItem(key);
    if (stored === null) return fallback;
    const parsed: unknown = JSON.parse(stored);
    return isValid(parsed) ? parsed : fallback;
  } catch {
    return fallback;
  }
}

/**
 * Persist a value to localStorage using the JSON encoding `loadJSON` expects.
 * Silently no-ops on quota exceeded / localStorage disabled — same fail-open
 * posture every existing call site uses.
 */
export function saveJSON<T>(key: string, value: T): void {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {
    /* ignore — same fail-open posture as the Path A inline pattern */
  }
}

/** Type guard for booleans. Most collapse-state keys use this. */
export const isBoolean = (v: unknown): v is boolean => typeof v === "boolean";

/** Type guard for strings. Used for `vaak_collab_project_dir`. */
export const isString = (v: unknown): v is string => typeof v === "string";
