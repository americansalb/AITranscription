import { useEffect, useRef, useState } from "react";
import { useModalA11y } from "../hooks/useModalA11y";

export type AssemblySetupSubmit = {
  mic_passing_mode: "rotation" | "hand_raise" | "moderator";
  moderator: string | null;
  preset: string;
};

export function AssemblySetupModal(props: {
  open: boolean;
  projectDir: string;
  activeSeats: string[];
  currentMicMode?: "rotation" | "hand_raise" | "moderator";
  currentModerator?: string | null;
  currentPreset?: string;
  onClose: () => void;
  onStarted?: (config: AssemblySetupSubmit) => void;
}) {
  const {
    open,
    projectDir,
    activeSeats,
    currentMicMode,
    currentModerator,
    currentPreset,
    onClose,
    onStarted,
  } = props;

  const [micMode, setMicMode] = useState<"rotation" | "hand_raise" | "moderator">(
    currentMicMode ?? "rotation",
  );
  const [moderator, setModerator] = useState<string>(currentModerator ?? "");
  const [preset, setPreset] = useState<string>(currentPreset ?? "Assembly Line");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const dialogRef = useRef<HTMLDivElement>(null);
  const firstFieldRef = useRef<HTMLSelectElement>(null);

  useEffect(() => {
    if (open) {
      setMicMode(currentMicMode ?? "rotation");
      setModerator(currentModerator ?? "");
      setPreset(currentPreset ?? "Assembly Line");
      setError(null);
      setBusy(false);
      const t = setTimeout(() => firstFieldRef.current?.focus(), 0);
      return () => clearTimeout(t);
    }
  }, [open, currentMicMode, currentModerator, currentPreset]);

  useModalA11y({
    open,
    onClose,
    containerRef: dialogRef,
    closeAllowed: () => !busy,
  });

  if (!open) return null;

  const availableSeats = ["human:0", ...activeSeats];
  const moderatorRequired = micMode === "moderator";
  const moderatorMissing = moderatorRequired && !moderator;
  const valid = !moderatorMissing;

  const submit = async () => {
    if (!valid || busy) return;
    setBusy(true);
    setError(null);
    try {
      const { invoke } = await import("@tauri-apps/api/core");

      // Persist customization via protocol_mutate_cmd. Each setting is its own
      // mutate call so the server can reject one without losing others. The
      // rev is read fresh because we don't take a rev prop — the modal mutates
      // against current server state.
      const protocol = await invoke<{ rev: number }>("get_protocol_cmd", {
        dir: projectDir,
      }).catch(() => null);

      if (protocol) {
        await invoke("protocol_mutate_cmd", {
          dir: projectDir,
          action: "set_mic_passing",
          args: { mode: micMode },
          rev: protocol.rev,
        }).catch((e) => console.warn("[AssemblySetup] set_mic_passing:", e));

        if (micMode === "moderator" && moderator) {
          const p2 = await invoke<{ rev: number }>("get_protocol_cmd", {
            dir: projectDir,
          }).catch(() => null);
          if (p2) {
            await invoke("protocol_mutate_cmd", {
              dir: projectDir,
              action: "set_moderator",
              args: { seat: moderator },
              rev: p2.rev,
            }).catch((e) => console.warn("[AssemblySetup] set_moderator:", e));
          }
        }

        if (preset && preset !== currentPreset) {
          const p3 = await invoke<{ rev: number }>("get_protocol_cmd", {
            dir: projectDir,
          }).catch(() => null);
          if (p3) {
            await invoke("protocol_mutate_cmd", {
              dir: projectDir,
              action: "set_preset",
              args: { preset },
              rev: p3.rev,
            }).catch((e) => console.warn("[AssemblySetup] set_preset:", e));
          }
        }
      }

      // Flip assembly ON via the legacy set_assembly_state cmd. The post-
      // Phase-0 backend (SHA-13.4 commit 21ab8bc) re-seeds rotation_order
      // and stamps a fresh started_at on every enable call.
      await invoke<{
        active: boolean;
        current_speaker: string | null;
        rotation_order: string[];
      }>("set_assembly_state", { dir: projectDir, action: "enable" });

      if (onStarted) {
        onStarted({ mic_passing_mode: micMode, moderator: moderator || null, preset });
      }
      onClose();
    } catch (e: any) {
      setError(String(e?.message ?? e));
      setBusy(false);
    }
  };

  return (
    <div className="asm-backdrop" onClick={() => { if (!busy) onClose(); }}>
      <div
        ref={dialogRef}
        className="asm-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="asm-title"
        onClick={(e) => e.stopPropagation()}
      >
        <h3 id="asm-title" className="asm-title">Start Assembly Line</h3>
        <p className="asm-subtitle">
          One-speaker-at-a-time mic control. Choose how the mic passes between seats, then activate.
        </p>

        {error && <div className="asm-error">{error}</div>}

        <label className="asm-field">
          <span className="asm-field-label">Mic passing mode</span>
          <select
            ref={firstFieldRef}
            className="asm-select"
            value={micMode}
            onChange={(e) => setMicMode(e.target.value as typeof micMode)}
          >
            <option value="rotation">Rotation — round-robin through active seats</option>
            <option value="hand_raise">Hand-raise — seats request the mic</option>
            <option value="moderator">Moderator picks next speaker</option>
          </select>
        </label>

        <label className="asm-field">
          <span className="asm-field-label">
            Moderator {moderatorRequired ? <span className="asm-required">(required)</span> : <span className="asm-hint">(only used in moderator mode)</span>}
          </span>
          <select
            className="asm-select"
            value={moderator}
            onChange={(e) => setModerator(e.target.value)}
            disabled={!moderatorRequired}
            aria-invalid={moderatorMissing || undefined}
          >
            <option value="">— pick moderator —</option>
            {availableSeats.map((s) => (
              <option key={s} value={s}>{s}</option>
            ))}
          </select>
        </label>

        <label className="asm-field">
          <span className="asm-field-label">Preset</span>
          <select
            className="asm-select"
            value={preset}
            onChange={(e) => setPreset(e.target.value)}
          >
            <option value="Assembly Line">Assembly Line</option>
            <option value="Default chat">Default chat</option>
          </select>
        </label>

        <div className="asm-summary" aria-live="polite">
          <strong>Setup:</strong>{" "}
          mic={micMode}
          {micMode === "moderator" && (moderator ? `, moderator=${moderator}` : <em>, no moderator</em>)}
          , preset={preset}
        </div>

        <div className="asm-actions">
          <button type="button" className="asm-btn asm-btn-cancel" onClick={onClose} disabled={busy}>
            Cancel
          </button>
          <button
            type="button"
            className="asm-btn asm-btn-submit"
            onClick={submit}
            disabled={!valid || busy}
          >
            {busy ? "Activating…" : "Activate Assembly Line"}
          </button>
        </div>
      </div>
    </div>
  );
}
