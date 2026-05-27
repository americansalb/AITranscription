import { useEffect, useRef, useState } from "react";
import { useModalA11y } from "../hooks/useModalA11y";

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
  const [maxRounds, setMaxRounds] = useState<string>("3");
  const [timeoutMinutes, setTimeoutMinutes] = useState<string>("15");
  const [reward, setReward] = useState<string>("0");
  const [rewardDenom, setRewardDenom] = useState<"copper" | "silver" | "gold">("silver");
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
      setMaxRounds("3");
      setTimeoutMinutes("15");
      setReward("0");
      setRewardDenom("silver");
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

  const timeoutParsed = parseInt(timeoutMinutes, 10);
  const timeoutValid = Number.isInteger(timeoutParsed) && timeoutParsed >= 1 && timeoutParsed <= 120;

  const valid =
    moderator !== "" &&
    participants.size >= 2 &&
    topic.trim().length > 0 &&
    rewardValid &&
    maxRoundsValid &&
    timeoutValid;

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
        maxRounds: maxRoundsParsed,
        timeoutMinutes: timeoutParsed,
        winningRewardCopper: rewardCopper,
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

        {renderRolePicker("Participants", "Submit blind each round (min 2)", participants, setParticipants)}
        {renderRolePicker("Audience", "Optional — votes after final round", audience, setAudience)}

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
          <label className="dsm-field dsm-field-half">
            <span className="dsm-field-label">Timeout / round <span className="dsm-hint">(min)</span></span>
            <input
              className="dsm-input"
              type="number"
              min={1}
              max={120}
              step={1}
              value={timeoutMinutes}
              onChange={(e) => setTimeoutMinutes(e.target.value)}
              aria-label="round timeout in minutes"
            />
          </label>
        </div>

        <label className="dsm-field">
          <span className="dsm-field-label">
            Convergence reward
            <span className="dsm-hint"> Pool-funded. 0 = no reward.</span>
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

        <div className="dsm-summary" aria-live="polite">
          <strong>Lineup:</strong>{" "}
          {moderator ? `1 moderator (${moderator})` : <em>no moderator</em>}
          {", "}
          {participants.size > 0 ? `${participants.size} participants` : <em>no participants</em>}
          {", "}
          {audience.size > 0 ? `${audience.size} audience` : "no audience"}
          {", "}
          {maxRoundsValid ? `${maxRoundsParsed} max rounds` : <em>invalid round count</em>}
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
