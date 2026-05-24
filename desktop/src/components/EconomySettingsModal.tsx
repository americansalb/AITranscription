import { useEffect, useRef, useState } from "react";
import { useModalA11y } from "../hooks/useModalA11y";

/**
 * Economy Settings page (modal). Per human msg 657: "every hardcoded economic
 * constant from collab.rs into .vaak/economy.json. Build a settings page in
 * the UI where I can adjust all values live."
 *
 * Reads current settings via read_economy_settings_cmd on open; submits edits
 * via write_economy_settings_cmd. Backend emits one audit ledger row per
 * changed field. Changes take effect on the next process_tick (no rebuild)
 * because read_economy_settings re-reads disk every call.
 */

type Settings = Record<string, number>;

const GROUPS: Array<{ title: string; fields: Array<{ key: string; label: string; hint?: string }> }> = [
  {
    title: "Base",
    fields: [
      { key: "starting_balance_copper", label: "Starting balance (copper)", hint: "1 silver = 100c, 1 gold = 10000c. New seats land here." },
      { key: "deficit_cap_copper", label: "Deficit cap (copper)", hint: "Negative; crossing this trips timed_out." },
      { key: "passive_per_tick_copper", label: "Passive per tick", hint: "Per active seat per mic-advance." },
    ],
  },
  {
    title: "Earn tier",
    fields: [
      { key: "pass_earn_copper", label: "PASS earn" },
      { key: "speak_earn_copper", label: "SPEAK earn" },
      { key: "edit_earn_copper", label: "EDIT earn (base)" },
      { key: "test_earn_copper", label: "TEST earn" },
      { key: "edit_line_bonus_threshold", label: "EDIT line bonus threshold", hint: "+1c per line beyond this." },
    ],
  },
  {
    title: "Escrow ticks",
    fields: [
      { key: "pass_escrow_ticks", label: "PASS ticks" },
      { key: "speak_escrow_ticks", label: "SPEAK ticks" },
      { key: "edit_escrow_ticks", label: "EDIT ticks" },
      { key: "test_escrow_ticks", label: "TEST ticks" },
    ],
  },
  {
    title: "Interest",
    fields: [
      { key: "interest_min_held_copper", label: "Min held for interest" },
      { key: "interest_per_10_copper_held", label: "Interest per 10c held per tick", hint: "0 = no interest. Was 1 pre-fix." },
    ],
  },
  {
    title: "Classifier",
    fields: [
      { key: "pass_body_len_threshold", label: "PASS body length threshold (legacy)" },
    ],
  },
  {
    title: "Disputes",
    fields: [
      { key: "objection_cost_copper", label: "Objection cost" },
      { key: "dispute_speech_cost_copper", label: "Dispute speech cost" },
      { key: "dispute_edit_cost_copper", label: "Dispute edit cost" },
      { key: "judge_cost_per_party", label: "Judge cost per party" },
      { key: "judge_auto_invoke_threshold", label: "Judge auto-invoke pool size" },
      { key: "system_dispute_cost", label: "System dispute cost" },
      { key: "system_dispute_reward", label: "System dispute reward (correct)" },
      { key: "system_dispute_penalty", label: "System dispute penalty (incorrect)" },
      { key: "system_dispute_ban_turns", label: "System dispute ban turns" },
      { key: "clawback_percent", label: "Clawback %" },
    ],
  },
  {
    title: "Penalty hooks",
    fields: [
      { key: "retro_pass_penalty_copper", label: "Retro-Pass penalty" },
      { key: "retro_pass_scan_window_turns", label: "Retro-Pass scan window (turns)" },
      { key: "coliability_test_penalty_copper", label: "Co-liability test penalty" },
    ],
  },
  {
    title: "Bounty",
    fields: [
      { key: "bounty_claim_stake_percent", label: "Claim stake %" },
      { key: "bounty_abandon_loss_percent", label: "Abandon loss %" },
      { key: "bounty_reject_loss_percent", label: "Reject loss %" },
      { key: "bounty_objection_clawback_percent", label: "Objection clawback %" },
    ],
  },
  {
    title: "Decay tax",
    fields: [
      { key: "decay_copper_pct_per_turn_tenths", label: "Copper decay % per turn (tenths)", hint: "10 = 1.0%" },
      { key: "decay_silver_pct_per_turn_tenths", label: "Silver decay % per turn (tenths)", hint: "5 = 0.5%" },
      { key: "decay_floor_copper", label: "Decay floor (copper)", hint: "Balance below this is not taxed." },
    ],
  },
];

export function EconomySettingsModal(props: {
  open: boolean;
  projectDir: string;
  onClose: () => void;
}) {
  const { open, projectDir, onClose } = props;
  const [settings, setSettings] = useState<Settings | null>(null);
  const [original, setOriginal] = useState<Settings | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [submitMsg, setSubmitMsg] = useState<string | null>(null);
  const dialogRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) {
      setSettings(null);
      setOriginal(null);
      setError(null);
      setSubmitMsg(null);
      setBusy(false);
      return;
    }
    (async () => {
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        const s = await invoke<Settings>("read_economy_settings_cmd", { dir: projectDir });
        setSettings({ ...s });
        setOriginal({ ...s });
      } catch (e: any) {
        setError(String(e?.message ?? e));
      }
    })();
  }, [open, projectDir]);

  useModalA11y({
    open,
    onClose,
    containerRef: dialogRef,
    closeAllowed: () => !busy,
  });

  if (!open) return null;

  const changedKeys = settings && original
    ? Object.keys(settings).filter((k) => settings[k] !== original[k])
    : [];

  const submit = async () => {
    if (!settings || busy) return;
    setBusy(true);
    setError(null);
    setSubmitMsg(null);
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      const result = await invoke<{ fields_changed: number; changes: Array<{ field: string; from: any; to: any }> }>(
        "write_economy_settings_cmd",
        { dir: projectDir, settings },
      );
      setSubmitMsg(
        result.fields_changed === 0
          ? "No changes — nothing to write."
          : `Saved ${result.fields_changed} change${result.fields_changed === 1 ? "" : "s"}. Effect next tick.`,
      );
      setOriginal({ ...settings });
    } catch (e: any) {
      setError(String(e?.message ?? e));
    } finally {
      setBusy(false);
    }
  };

  const reset = () => {
    if (original) setSettings({ ...original });
    setSubmitMsg(null);
    setError(null);
  };

  return (
    <div className="esm-backdrop" onClick={() => { if (!busy) onClose(); }}>
      <div
        ref={dialogRef}
        className="esm-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="esm-title"
        onClick={(e) => e.stopPropagation()}
      >
        <header className="esm-header">
          <h3 id="esm-title">Economy Settings</h3>
          <span className="esm-subtitle">
            Tune live. Changes write to <code>.vaak/economy.json</code> and take effect next tick. Each change writes an audit ledger row.
          </span>
        </header>

        {error && <div className="esm-error">{error}</div>}
        {submitMsg && <div className="esm-success">{submitMsg}</div>}

        {!settings && !error && <div className="esm-loading">Loading current settings…</div>}

        {settings && (
          <div className="esm-body">
            {GROUPS.map((group) => (
              <section key={group.title} className="esm-group">
                <h4 className="esm-group-title">{group.title}</h4>
                {group.fields.map((f) => {
                  const isChanged = original ? settings[f.key] !== original[f.key] : false;
                  return (
                    <label key={f.key} className={`esm-field${isChanged ? " esm-field-changed" : ""}`}>
                      <span className="esm-field-label">
                        {f.label}
                        {isChanged && original && (
                          <span className="esm-field-diff">
                            (was {original[f.key]})
                          </span>
                        )}
                      </span>
                      <input
                        className="esm-input"
                        type="number"
                        step={1}
                        value={settings[f.key] ?? 0}
                        onChange={(e) => {
                          const v = e.target.value === "" ? 0 : parseInt(e.target.value, 10);
                          if (!Number.isNaN(v)) {
                            setSettings({ ...settings, [f.key]: v });
                          }
                        }}
                      />
                      {f.hint && <span className="esm-hint">{f.hint}</span>}
                    </label>
                  );
                })}
              </section>
            ))}
          </div>
        )}

        <footer className="esm-actions">
          <span className="esm-changed-count">
            {changedKeys.length === 0
              ? "No unsaved changes"
              : `${changedKeys.length} unsaved change${changedKeys.length === 1 ? "" : "s"}`}
          </span>
          <div className="esm-actions-btns">
            <button
              type="button"
              className="esm-btn esm-btn-cancel"
              onClick={onClose}
              disabled={busy}
            >Close</button>
            <button
              type="button"
              className="esm-btn esm-btn-reset"
              onClick={reset}
              disabled={busy || changedKeys.length === 0}
            >Reset</button>
            <button
              type="button"
              className="esm-btn esm-btn-submit"
              onClick={submit}
              disabled={busy || changedKeys.length === 0}
            >{busy ? "Saving…" : "Save"}</button>
          </div>
        </footer>
      </div>
    </div>
  );
}
