import { useEffect, useRef, useState } from "react";
import { useModalA11y } from "../hooks/useModalA11y";

export type ContinuousSetupSubmit = {
  silence_timeout_seconds: number;
};

// Review window default duration. Per human msg 2599 ("60 seconds is too
// short for review window"), default raised from 60s to 5m. Range covers
// tight-feedback (1m) through proper-review (1h).
const TIMEOUT_OPTIONS: Array<{ value: number; label: string }> = [
  { value: 60, label: "1 minute — tight feedback (tiny change)" },
  { value: 300, label: "5 minutes — default (quick async review)" },
  { value: 900, label: "15 minutes — moderate" },
  { value: 1800, label: "30 minutes — relaxed" },
  { value: 3600, label: "1 hour — proper review (large change)" },
];

export function ContinuousSetupModal(props: {
  open: boolean;
  projectDir: string;
  activeSeats: string[];
  currentTimeoutSeconds?: number;
  onClose: () => void;
  onStarted?: (config: ContinuousSetupSubmit) => void;
}) {
  const {
    open,
    projectDir,
    activeSeats,
    currentTimeoutSeconds,
    onClose,
    onStarted,
  } = props;

  const defaultTimeout = currentTimeoutSeconds ?? 300;
  const [timeout, setTimeoutValue] = useState<number>(defaultTimeout);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const dialogRef = useRef<HTMLDivElement>(null);
  const firstFieldRef = useRef<HTMLSelectElement>(null);

  useEffect(() => {
    if (open) {
      setTimeoutValue(currentTimeoutSeconds ?? 300);
      setError(null);
      setBusy(false);
      const t = setTimeout(() => firstFieldRef.current?.focus(), 0);
      return () => clearTimeout(t);
    }
  }, [open, currentTimeoutSeconds]);

  useModalA11y({
    open,
    onClose,
    containerRef: dialogRef,
    closeAllowed: () => !busy,
  });

  if (!open) return null;

  const submit = async () => {
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      const { invoke } = await import("@tauri-apps/api/core");

      // Persist the silence timeout if it changed.
      if (timeout !== currentTimeoutSeconds) {
        await invoke("set_continuous_timeout", {
          dir: projectDir,
          timeoutSeconds: timeout,
        }).catch((e) => console.warn("[ContinuousSetup] set_continuous_timeout:", e));
      }

      // Default participants = all active seats (matches the existing
      // handleStartDiscussion shortcut at CollabTab.tsx:3046).
      const participants = activeSeats;

      // Default moderator: first moderator seat / first manager / first
      // participant / fallback to human:0.
      const modSeat = participants.find((s) => s.startsWith("moderator:"));
      const mgrSeat = participants.find((s) => s.startsWith("manager:"));
      const moderator = modSeat ?? mgrSeat ?? participants[0] ?? "human:0";

      await invoke("start_discussion", {
        dir: projectDir,
        mode: "continuous",
        topic: "Continuous review — auto-triggered micro-rounds",
        moderator,
        participants,
      });

      if (onStarted) {
        onStarted({ silence_timeout_seconds: timeout });
      }
      onClose();
    } catch (e: any) {
      setError(String(e?.message ?? e));
      setBusy(false);
    }
  };

  return (
    <div className="csm-backdrop" onClick={() => { if (!busy) onClose(); }}>
      <div
        ref={dialogRef}
        className="csm-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="csm-title"
        onClick={(e) => e.stopPropagation()}
      >
        <h3 id="csm-title" className="csm-title">Start Continuous Review</h3>
        <p className="csm-subtitle">
          Free-form work + a review window on every commit. Builder names ≥2 reviewers (if available); named reviewers respond APPROVE / BLOCK / COMMENT. Silence past the timer = APPROVE.
        </p>

        {error && <div className="csm-error">{error}</div>}

        <label className="csm-field">
          <span className="csm-field-label">Review window duration</span>
          <select
            ref={firstFieldRef}
            className="csm-select"
            value={timeout}
            onChange={(e) => setTimeoutValue(Number(e.target.value))}
          >
            {TIMEOUT_OPTIONS.map((opt) => (
              <option key={opt.value} value={opt.value}>{opt.label}</option>
            ))}
          </select>
        </label>

        <div className="csm-summary" aria-live="polite">
          <strong>Setup:</strong> silence_timeout={timeout}s · participants={activeSeats.length} active seat{activeSeats.length === 1 ? "" : "s"}
        </div>

        <div className="csm-actions">
          <button type="button" className="csm-btn csm-btn-cancel" onClick={onClose} disabled={busy}>
            Cancel
          </button>
          <button
            type="button"
            className="csm-btn csm-btn-submit"
            onClick={submit}
            disabled={busy}
          >
            {busy ? "Starting…" : "Start Continuous Review"}
          </button>
        </div>
      </div>
    </div>
  );
}
