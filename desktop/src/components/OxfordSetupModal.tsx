import { useEffect, useRef, useState } from "react";
import { useModalA11y } from "../hooks/useModalA11y";

/**
 * Oxford-style debate setup modal (Phase A v1 UI per human msg 706 "where is
 * debate?"). Lets the human pick moderator + side_a/side_b debaters + premise
 * + audience + optional reward, then calls oxford_initiate_cmd Tauri command
 * to launch the debate.
 *
 * Designed minimal — `<select>` for moderator, multi-checkbox grids for sides
 * and audience, text inputs for premise and reward. Architect's bigger setup
 * wizard (per Phase A spec §3.1) can replace this later.
 */

export function OxfordSetupModal(props: {
  open: boolean;
  projectDir: string;
  activeSeats: string[]; // labels like "manager:0", "architect:0"
  onClose: () => void;
}) {
  const { open, projectDir, activeSeats, onClose } = props;
  const [moderator, setModerator] = useState<string>("");
  const [sideA, setSideA] = useState<Set<string>>(new Set());
  const [sideB, setSideB] = useState<Set<string>>(new Set());
  const [audience, setAudience] = useState<Set<string>>(new Set());
  const [premise, setPremise] = useState<string>("");
  const [reward, setReward] = useState<string>("5"); // value in the picked denomination
  const [rewardDenom, setRewardDenom] = useState<"copper" | "silver" | "gold">("silver");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const dialogRef = useRef<HTMLDivElement>(null);
  const premiseRef = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    if (open) {
      setModerator("");
      setSideA(new Set());
      setSideB(new Set());
      setAudience(new Set());
      setPremise("");
      setReward("5");
      setRewardDenom("silver");
      setError(null);
      setBusy(false);
      const t = setTimeout(() => premiseRef.current?.focus(), 0);
      return () => clearTimeout(t);
    }
  }, [open]);

  useModalA11y({
    open,
    onClose,
    containerRef: dialogRef,
    closeAllowed: () => !busy,
  });

  if (!open) return null;

  // Available seats for each role (mutual exclusion):
  // - moderator: any active seat not in sides or audience
  // - sideA: any active seat not moderator, not in sideB, not in audience
  // - sideB: any active seat not moderator, not in sideA, not in audience
  // - audience: any active seat not moderator, not in sides; human:0 always available
  const availableSeats = ["human:0", ...activeSeats];

  const toggle = (set: Set<string>, setSet: (s: Set<string>) => void, seat: string) => {
    const next = new Set(set);
    if (next.has(seat)) next.delete(seat);
    else next.add(seat);
    setSet(next);
  };

  const rewardMultiplier = rewardDenom === "gold" ? 10_000 : rewardDenom === "silver" ? 100 : 1;
  const rewardParsed = parseInt(reward, 10);
  const rewardValid = Number.isInteger(rewardParsed) && rewardParsed >= 0;
  const rewardCopper = rewardValid ? rewardParsed * rewardMultiplier : 0;

  const valid =
    moderator !== "" &&
    sideA.size >= 1 &&
    sideB.size >= 1 &&
    premise.trim().length > 0 &&
    rewardValid;

  const submit = async () => {
    if (!valid || busy) return;
    setBusy(true);
    setError(null);
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      const result = await invoke("oxford_initiate_cmd", {
        dir: projectDir,
        moderator,
        sideA: Array.from(sideA),
        sideB: Array.from(sideB),
        premise: premise.trim(),
        audience: Array.from(audience),
        winningSideRewardCopper: rewardCopper,
      });
      console.log("[oxford_initiate]", result);
      onClose();
    } catch (e: any) {
      setError(String(e?.message ?? e));
      setBusy(false);
    }
  };

  const renderRolePicker = (
    label: string,
    set: Set<string>,
    setSet: (s: Set<string>) => void,
  ) => (
    <div className="osm-role-block">
      <label className="osm-role-label">{label} <span className="osm-count">({set.size})</span></label>
      <div className="osm-seat-grid">
        {availableSeats.map((seat) => {
          const checked = set.has(seat);
          const blockedByOther = !checked &&
            (seat === moderator || sideA.has(seat) || sideB.has(seat) || audience.has(seat));
          if (blockedByOther) return null;
          return (
            <label key={seat} className={`osm-seat-pill${checked ? " osm-seat-pill-active" : ""}`}>
              <input
                type="checkbox"
                checked={checked}
                onChange={() => toggle(set, setSet, seat)}
              />
              {seat}
            </label>
          );
        })}
      </div>
    </div>
  );

  return (
    <div className="osm-backdrop" onClick={() => { if (!busy) onClose(); }}>
      <div
        ref={dialogRef}
        className="osm-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="osm-title"
        onClick={(e) => e.stopPropagation()}
      >
        <h3 id="osm-title" className="osm-title">Start an Oxford-style debate</h3>
        <p className="osm-subtitle">
          Designate a moderator + two sides + audience. Audience votes at the end; strict-majority winner splits the pool reward.
        </p>

        {error && <div className="osm-error">{error}</div>}

        <label className="osm-field">
          <span className="osm-field-label">Premise</span>
          <textarea
            ref={premiseRef}
            className="osm-textarea"
            value={premise}
            onChange={(e) => setPremise(e.target.value)}
            placeholder="What's being debated? (e.g. 'Vaak should adopt strict TypeScript')"
            rows={2}
          />
        </label>

        <div className="osm-field">
          <span className="osm-field-label">Moderator (1)</span>
          <select
            className="osm-select"
            value={moderator}
            onChange={(e) => setModerator(e.target.value)}
          >
            <option value="">— pick moderator —</option>
            {availableSeats
              .filter((s) => !sideA.has(s) && !sideB.has(s) && !audience.has(s))
              .map((s) => (
                <option key={s} value={s}>{s}</option>
              ))}
          </select>
        </div>

        {renderRolePicker("Side A", sideA, setSideA)}
        {renderRolePicker("Side B", sideB, setSideB)}
        {renderRolePicker("Audience", audience, setAudience)}

        <label className="osm-field">
          <span className="osm-field-label">
            Winning-side reward
            <span className="osm-hint"> Pool-funded. 0 = no reward.</span>
          </span>
          <div className="osm-reward-row">
            <div className="osm-denoms" role="radiogroup" aria-label="reward denomination">
              {(["copper", "silver", "gold"] as const).map((d) => (
                <label key={d} className={`osm-denom-pill${rewardDenom === d ? " osm-denom-pill-active" : ""}`}>
                  <input
                    type="radio"
                    name="osm-reward-denom"
                    value={d}
                    checked={rewardDenom === d}
                    onChange={() => setRewardDenom(d)}
                  />
                  <span className={`coin-icon coin-icon-${d}`} aria-hidden="true" />
                  <span className="osm-denom-label">{d}</span>
                </label>
              ))}
            </div>
            <input
              className="osm-input"
              type="number"
              min={0}
              step={1}
              value={reward}
              onChange={(e) => setReward(e.target.value)}
              aria-label={`reward amount in ${rewardDenom}`}
            />
          </div>
          {rewardValid && (
            <span className="osm-reward-preview">
              {rewardCopper === 0
                ? "No reward (winners get 0c)."
                : `Winners split ${rewardCopper.toLocaleString()} copper total.`}
            </span>
          )}
        </label>

        <div className="osm-summary" aria-live="polite">
          <strong>Lineup:</strong>{" "}
          {moderator ? `1 moderator (${moderator})` : <em>no moderator</em>}
          {", "}
          {sideA.size > 0 ? `${sideA.size} side A` : <em>no side A</em>}
          {", "}
          {sideB.size > 0 ? `${sideB.size} side B` : <em>no side B</em>}
          {", "}
          {audience.size > 0 ? `${audience.size} audience` : "no audience"}
        </div>

        <div className="osm-actions">
          <button type="button" className="osm-btn osm-btn-cancel" onClick={onClose} disabled={busy}>Cancel</button>
          <button
            type="button"
            className="osm-btn osm-btn-submit"
            onClick={submit}
            disabled={!valid || busy}
          >{busy ? "Starting…" : "Start debate"}</button>
        </div>
      </div>
    </div>
  );
}
