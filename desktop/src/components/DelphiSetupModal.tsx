import { useEffect, useRef, useState } from "react";
import { useModalA11y } from "../hooks/useModalA11y";
import { DELPHI_DEFAULTS, type DelphiConvergenceMode } from "../lib/collabTypes";

export function DelphiSetupModal(props: {
  open: boolean;
  projectDir: string;
  activeSeats: string[];
  onClose: () => void;
  onStarted?: (discussion: {
    discussion_id: number;
    moderator: string;
    topic: string;
    participants: string[];
    max_rounds: number;
  }) => void;
}) {
  const { open, projectDir, activeSeats, onClose, onStarted } = props;
  const [moderator, setModerator] = useState<string>("");
  const [participants, setParticipants] = useState<Set<string>>(new Set());
  const [audience, setAudience] = useState<Set<string>>(new Set());
  const [topic, setTopic] = useState<string>("");
  const [maxRounds, setMaxRounds] = useState<string>(String(DELPHI_DEFAULTS.MAX_ROUNDS));
  const [convergenceCriterion, setConvergenceCriterion] = useState<DelphiConvergenceMode>("moderator");
  const [reward, setReward] = useState<string>(String(DELPHI_DEFAULTS.CONVERGENCE_REWARD_COPPER));
  const [rewardDenom, setRewardDenom] = useState<"copper" | "silver" | "gold">("copper");
  const [advancedOpen, setAdvancedOpen] = useState(false);
  const [submissionSoftFloorSecs, setSubmissionSoftFloorSecs] = useState<string>(String(DELPHI_DEFAULTS.SUBMISSION_SOFT_FLOOR_SECS));
  const [submissionHardFloorSecs, setSubmissionHardFloorSecs] = useState<string>(String(DELPHI_DEFAULTS.SUBMISSION_HARD_FLOOR_SECS));
  const [reviewFloorSecs, setReviewFloorSecs] = useState<string>(String(DELPHI_DEFAULTS.REVIEW_FLOOR_SECS));
  const [blindGateStrict, setBlindGateStrict] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const dialogRef = useRef<HTMLDivElement>(null);
  const topicRef = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    if (open) {
      setModerator("");
      setParticipants(new Set());
      setAudience(new Set());
      setTopic("");
      setMaxRounds(String(DELPHI_DEFAULTS.MAX_ROUNDS));
      setConvergenceCriterion("moderator");
      setReward(String(DELPHI_DEFAULTS.CONVERGENCE_REWARD_COPPER));
      setRewardDenom("copper");
      setAdvancedOpen(false);
      setSubmissionSoftFloorSecs(String(DELPHI_DEFAULTS.SUBMISSION_SOFT_FLOOR_SECS));
      setSubmissionHardFloorSecs(String(DELPHI_DEFAULTS.SUBMISSION_HARD_FLOOR_SECS));
      setReviewFloorSecs(String(DELPHI_DEFAULTS.REVIEW_FLOOR_SECS));
      setBlindGateStrict(true);
      setError(null);
      setBusy(false);
      const t = setTimeout(() => topicRef.current?.focus(), 0);
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

  const maxRoundsParsed = parseInt(maxRounds, 10);
  const maxRoundsValid = Number.isInteger(maxRoundsParsed) && maxRoundsParsed >= 1 && maxRoundsParsed <= 20;

  const softParsed = parseInt(submissionSoftFloorSecs, 10);
  const hardParsed = parseInt(submissionHardFloorSecs, 10);
  const reviewParsed = parseInt(reviewFloorSecs, 10);
  const softValid = Number.isInteger(softParsed) && softParsed >= 30 && softParsed <= 1800;
  const hardValid = Number.isInteger(hardParsed) && hardParsed >= 30 && hardParsed <= 3600 && hardParsed >= softParsed;
  const reviewValid = Number.isInteger(reviewParsed) && reviewParsed >= 30 && reviewParsed <= 3600;

  const valid =
    moderator !== "" &&
    participants.size >= DELPHI_DEFAULTS.MIN_PARTICIPANTS &&
    topic.trim().length > 0 &&
    rewardValid &&
    maxRoundsValid &&
    softValid &&
    hardValid &&
    reviewValid;

  const submit = async () => {
    if (!valid || busy) return;
    setBusy(true);
    setError(null);
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      const result = await invoke<{
        discussion_id: number;
        moderator: string;
        topic: string;
        participants: string[];
        max_rounds: number;
      }>("delphi_initiate_cmd", {
        dir: projectDir,
        moderator,
        topic: topic.trim(),
        participants: Array.from(participants),
        audience: Array.from(audience),
        max_rounds: maxRoundsParsed,
        convergence_criterion: convergenceCriterion,
        convergence_reward_copper: rewardCopper,
        submission_soft_floor_secs: softParsed,
        submission_hard_floor_secs: hardParsed,
        review_floor_secs: reviewParsed,
        blind_gate_strict: blindGateStrict,
      });
      console.log("[delphi_initiate]", result);
      if (onStarted && result && typeof result.discussion_id === "number") {
        onStarted({
          discussion_id: result.discussion_id,
          moderator: result.moderator,
          topic: result.topic,
          participants: result.participants,
          max_rounds: result.max_rounds,
        });
      }
      onClose();
    } catch (e: any) {
      setError(String(e?.message ?? e));
      setBusy(false);
    }
  };

  const renderRolePicker = (
    label: string,
    helpText: string,
    set: Set<string>,
    setSet: (s: Set<string>) => void,
  ) => (
    <div className="dsm-role-block">
      <label className="dsm-role-label">
        {label} <span className="dsm-count">({set.size})</span>
        <span className="dsm-hint"> {helpText}</span>
      </label>
      <div className="dsm-seat-grid">
        {availableSeats.map((seat) => {
          const checked = set.has(seat);
          const blockedByOther = !checked &&
            (seat === moderator || (set !== participants && participants.has(seat)) || (set !== audience && audience.has(seat)));
          if (blockedByOther) return null;
          return (
            <label key={seat} className={`dsm-seat-pill${checked ? " dsm-seat-pill-active" : ""}`}>
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
    <div className="dsm-backdrop" onClick={() => { if (!busy) onClose(); }}>
      <div
        ref={dialogRef}
        className="dsm-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="dsm-title"
        onClick={(e) => e.stopPropagation()}
      >
        <h3 id="dsm-title" className="dsm-title">Start a Delphi discussion</h3>
        <p className="dsm-subtitle">
          Blind submissions per round, anonymized aggregation, optional audience vote. Designed to reduce groupthink — participants don't see each other's submissions until the moderator reveals.
        </p>

        {error && <div className="dsm-error">{error}</div>}

        <label className="dsm-field">
          <span className="dsm-field-label">Topic</span>
          <textarea
            ref={topicRef}
            className="dsm-textarea"
            value={topic}
            onChange={(e) => setTopic(e.target.value)}
            placeholder="What's the question? (e.g. 'Which Q3 feature should we prioritize?')"
            rows={2}
          />
        </label>

        <div className="dsm-field">
          <span className="dsm-field-label">Moderator (1)</span>
          <select
            className="dsm-select"
            value={moderator}
            onChange={(e) => setModerator(e.target.value)}
          >
            <option value="">— pick moderator —</option>
            {availableSeats
              .filter((s) => !participants.has(s) && !audience.has(s))
              .map((s) => (
                <option key={s} value={s}>{s}</option>
              ))}
          </select>
        </div>

        {renderRolePicker("Participants", `Submit blind each round (min ${DELPHI_DEFAULTS.MIN_PARTICIPANTS})`, participants, setParticipants)}
        {renderRolePicker("Audience", "Optional — observes + asks questions during reviewing phase", audience, setAudience)}

        <div className="dsm-field-row">
          <label className="dsm-field dsm-field-half">
            <span className="dsm-field-label">Max rounds <span className="dsm-hint">(1–20)</span></span>
            <input
              className="dsm-input"
              type="number"
              min={1}
              max={20}
              step={1}
              value={maxRounds}
              onChange={(e) => setMaxRounds(e.target.value)}
              aria-label="maximum number of rounds"
            />
          </label>
          <div className="dsm-field dsm-field-half">
            <span className="dsm-field-label">Convergence end</span>
            <div className="dsm-denoms" role="radiogroup" aria-label="convergence criterion">
              {(["moderator", "max_rounds", "hybrid"] as const).map((c) => (
                <label key={c} className={`dsm-denom-pill${convergenceCriterion === c ? " dsm-denom-pill-active" : ""}`}>
                  <input
                    type="radio"
                    name="dsm-convergence-criterion"
                    value={c}
                    checked={convergenceCriterion === c}
                    onChange={() => setConvergenceCriterion(c)}
                  />
                  <span className="dsm-denom-label">{c === "max_rounds" ? "max" : c}</span>
                </label>
              ))}
            </div>
          </div>
        </div>

        <label className="dsm-field">
          <span className="dsm-field-label">
            Convergence reward
            <span className="dsm-hint"> Pool-funded (delphi_pool). 0 = no reward channel.</span>
          </span>
          <div className="dsm-reward-row">
            <div className="dsm-denoms" role="radiogroup" aria-label="reward denomination">
              {(["copper", "silver", "gold"] as const).map((d) => (
                <label key={d} className={`dsm-denom-pill${rewardDenom === d ? " dsm-denom-pill-active" : ""}`}>
                  <input
                    type="radio"
                    name="dsm-reward-denom"
                    value={d}
                    checked={rewardDenom === d}
                    onChange={() => setRewardDenom(d)}
                  />
                  <span className={`coin-icon coin-icon-${d}`} aria-hidden="true" />
                  <span className="dsm-denom-label">{d}</span>
                </label>
              ))}
            </div>
            <input
              className="dsm-input"
              type="number"
              min={0}
              step={1}
              value={reward}
              onChange={(e) => setReward(e.target.value)}
              aria-label={`reward amount in ${rewardDenom}`}
            />
          </div>
          {rewardValid && (
            <span className="dsm-reward-preview">
              {rewardCopper === 0
                ? "No reward (participants get 0c on convergence)."
                : `Pool: ${rewardCopper.toLocaleString()} copper, split among converging participants.`}
            </span>
          )}
        </label>

        <details
          className="dsm-advanced"
          open={advancedOpen}
          onToggle={(e) => setAdvancedOpen((e.target as HTMLDetailsElement).open)}
        >
          <summary className="dsm-advanced-summary">Advanced — timing + blind gate</summary>
          <div className="dsm-advanced-body">
            <div className="dsm-field-row">
              <label className="dsm-field dsm-field-half">
                <span className="dsm-field-label">Submission soft floor <span className="dsm-hint">(s, 30–1800)</span></span>
                <input
                  className="dsm-input"
                  type="number"
                  min={30}
                  max={1800}
                  step={10}
                  value={submissionSoftFloorSecs}
                  onChange={(e) => setSubmissionSoftFloorSecs(e.target.value)}
                  aria-label="submission soft floor in seconds"
                />
                <span className="dsm-hint">After this, phase advances if quorum (≥½ participants) submitted</span>
              </label>
              <label className="dsm-field dsm-field-half">
                <span className="dsm-field-label">Submission hard floor <span className="dsm-hint">(s, ≥ soft)</span></span>
                <input
                  className="dsm-input"
                  type="number"
                  min={30}
                  max={3600}
                  step={10}
                  value={submissionHardFloorSecs}
                  onChange={(e) => setSubmissionHardFloorSecs(e.target.value)}
                  aria-label="submission hard floor in seconds"
                />
                <span className="dsm-hint">Forced close even without quorum; non-submitters timeout</span>
              </label>
            </div>
            <label className="dsm-field">
              <span className="dsm-field-label">Review floor <span className="dsm-hint">(s, 30–3600)</span></span>
              <input
                className="dsm-input"
                type="number"
                min={30}
                max={3600}
                step={10}
                value={reviewFloorSecs}
                onChange={(e) => setReviewFloorSecs(e.target.value)}
                aria-label="review phase floor in seconds"
              />
              <span className="dsm-hint">Min time aggregate stays visible before moderator can advance/end</span>
            </label>
            <label className="dsm-toggle-field">
              <input
                type="checkbox"
                checked={blindGateStrict}
                onChange={(e) => setBlindGateStrict(e.target.checked)}
              />
              <span className="dsm-toggle-label">Strict blind gate</span>
              <span className="dsm-hint">When on, even DMs to moderator from participants are blocked during submitting (default: on for stronger anonymity)</span>
            </label>
          </div>
        </details>

        <div className="dsm-summary" aria-live="polite">
          <strong>Lineup:</strong>{" "}
          {moderator ? `1 moderator (${moderator})` : <em>no moderator</em>}
          {", "}
          {participants.size > 0 ? `${participants.size} participants` : <em>no participants</em>}
          {", "}
          {audience.size > 0 ? `${audience.size} audience` : "no audience"}
          {", "}
          {maxRoundsValid ? `${maxRoundsParsed} max rounds` : <em>invalid round count</em>}
          {", "}
          {convergenceCriterion === "max_rounds" ? "auto-end at max" : convergenceCriterion === "hybrid" ? "moderator or max-rounds" : "moderator ends"}
        </div>

        <div className="dsm-actions">
          <button type="button" className="dsm-btn dsm-btn-cancel" onClick={onClose} disabled={busy}>Cancel</button>
          <button
            type="button"
            className="dsm-btn dsm-btn-submit"
            onClick={submit}
            disabled={!valid || busy}
          >{busy ? "Starting…" : "Start discussion"}</button>
        </div>
      </div>
    </div>
  );
}
