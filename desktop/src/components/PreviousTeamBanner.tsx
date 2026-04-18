import { useEffect, useRef, useState } from "react";

/**
 * Shown at the top of CollabTab when a prior session's team still lives
 * in .vaak/spawned.json but those processes are no longer alive. Gives the
 * human an explicit "Relaunch Previous Team" action so the app never spawns
 * without a click — the absolute rule per human msg 20 / pipeline 2026-04-18.
 *
 * Hidden when: no projectDir, manifest empty, or every entry's PID is alive
 * (nothing to relaunch). Claude CLI missing keeps the button disabled with a
 * tooltip rather than hiding the banner, so the user isn't silently blocked.
 *
 * Double-click guard: `relaunch_spawned` returns immediately with a queued
 * count while spawns proceed on a background thread with a 2s stagger
 * (tester:1 msg 168). The button stays disabled for roughly the full stagger
 * window plus buffer to prevent a rapid second click from re-queuing
 * already-in-flight roles and double-spawning them.
 */

export interface PreviousTeamEntry {
  role: string;
  instance: number;
  pid: number;
  spawned_at: string;
  alive: boolean;
}

interface Props {
  projectDir: string | null;
  claudeInstalled: boolean | null;
  /**
   * Parent wraps this in its existing `spawnConsented` confirm-modal flow
   * (CollabTab.tsx ~1806). Called only when the user clicks "Relaunch Previous
   * Team". Parent decides whether to prompt or proceed, then calls `execute`
   * to actually fire the invoke. `execute` resolves with the `queued` count
   * returned by the Rust `relaunch_spawned` command so the banner can compute
   * the correct disable window.
   */
  onRequestLaunch: (count: number, execute: () => Promise<number>) => void;
}

const POLL_INTERVAL_MS = 5000;
export const STAGGER_MS = 2000;
export const POST_RELAUNCH_BUFFER_MS = 1500;
export const debounceWindowMs = (queuedCount: number) =>
  queuedCount * STAGGER_MS + POST_RELAUNCH_BUFFER_MS;

export default function PreviousTeamBanner({
  projectDir,
  claudeInstalled,
  onRequestLaunch,
}: Props) {
  const [entries, setEntries] = useState<PreviousTeamEntry[]>([]);
  const [relaunching, setRelaunching] = useState(false);
  const [queuedCount, setQueuedCount] = useState(0);
  const [dismissing, setDismissing] = useState(false);
  const reenableTimer = useRef<number | null>(null);

  const deadEntries = entries.filter((e) => !e.alive);
  const deadCount = deadEntries.length;

  useEffect(() => {
    if (!projectDir || !window.__TAURI__) {
      setEntries([]);
      return;
    }
    let cancelled = false;
    const fetchManifest = async () => {
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        const list = await invoke<PreviousTeamEntry[]>("list_spawned_manifest", {
          projectDir,
        });
        if (!cancelled) setEntries(Array.isArray(list) ? list : []);
      } catch (e) {
        if (!cancelled) {
          console.error("[PreviousTeamBanner] list_spawned_manifest failed:", e);
          setEntries([]);
        }
      }
    };
    fetchManifest();
    const interval = setInterval(fetchManifest, POLL_INTERVAL_MS);
    return () => {
      cancelled = true;
      clearInterval(interval);
    };
  }, [projectDir]);

  useEffect(() => {
    return () => {
      if (reenableTimer.current !== null) {
        clearTimeout(reenableTimer.current);
        reenableTimer.current = null;
      }
    };
  }, []);

  const runRelaunch = async (): Promise<number> => {
    if (!projectDir || !window.__TAURI__) return 0;
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      const queued = await invoke<number>("relaunch_spawned", { projectDir });
      const safeQueued = typeof queued === "number" && queued >= 0 ? queued : 0;
      setQueuedCount(safeQueued);
      if (safeQueued > 0) {
        setRelaunching(true);
        const disableWindowMs = debounceWindowMs(safeQueued);
        if (reenableTimer.current !== null) clearTimeout(reenableTimer.current);
        reenableTimer.current = window.setTimeout(() => {
          setRelaunching(false);
          setQueuedCount(0);
          reenableTimer.current = null;
        }, disableWindowMs);
      }
      return safeQueued;
    } catch (e) {
      console.error("[PreviousTeamBanner] relaunch_spawned failed:", e);
      setRelaunching(false);
      setQueuedCount(0);
      return 0;
    }
  };

  const handleRelaunchClick = () => {
    if (!projectDir || relaunching || dismissing || deadCount === 0) return;
    onRequestLaunch(deadCount, runRelaunch);
  };

  const handleDismiss = async () => {
    if (!projectDir || !window.__TAURI__ || dismissing || relaunching) return;
    setDismissing(true);
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      await invoke("discard_spawned_manifest", { projectDir });
      setEntries([]);
    } catch (e) {
      console.error("[PreviousTeamBanner] discard_spawned_manifest failed:", e);
    } finally {
      setDismissing(false);
    }
  };

  if (!projectDir || deadCount === 0) return null;

  const buttonLabel = (() => {
    if (claudeInstalled === false) return "Claude CLI Not Found";
    if (relaunching && queuedCount > 0) return `Relaunching ${queuedCount}...`;
    if (relaunching) return "Relaunching...";
    return `Relaunch Previous Team (${deadCount})`;
  })();

  const buttonAriaLabel =
    claudeInstalled === false
      ? "Claude CLI not installed"
      : `Relaunch ${deadCount} role${deadCount === 1 ? "" : "s"} from previous session`;

  return (
    <div
      className="previous-team-banner"
      role="region"
      aria-label="Previous session detected"
    >
      <div className="previous-team-banner-title">Previous session</div>
      <div className="previous-team-banner-body">
        {deadCount} role{deadCount === 1 ? "" : "s"} {deadCount === 1 ? "was" : "were"} active last session and {deadCount === 1 ? "is" : "are"} not currently running. Click to bring {deadCount === 1 ? "it" : "them"} back.
      </div>
      <div className="previous-team-banner-actions">
        <button
          type="button"
          className="launch-team-btn previous-team-relaunch-btn"
          onClick={handleRelaunchClick}
          disabled={relaunching || claudeInstalled === false || dismissing}
          aria-label={buttonAriaLabel}
          title={
            claudeInstalled === false
              ? "Claude CLI not found — install with: npm i -g @anthropic-ai/claude-code"
              : undefined
          }
        >
          {relaunching && <span className="launch-team-spinner" />}
          {buttonLabel}
        </button>
        <button
          type="button"
          className="previous-team-dismiss-btn"
          onClick={handleDismiss}
          disabled={relaunching || dismissing}
          aria-label="Dismiss — clear the previous session record"
        >
          {dismissing ? "Dismissing..." : "Dismiss"}
        </button>
      </div>
    </div>
  );
}
