import { useState } from "react";
import type { DiscussionState, BoardMessage } from "../lib/collabTypes";
import { getTerminationStrategy, getAutomationLevel, getAudienceConfig } from "../lib/collabTypes";
// pr-pipeline-discussionpanel-dedupe PR-7: PipelineStepper import removed;
// pipeline visualization moved to SequenceSessionCard. Component file kept
// for now to avoid breaking any out-of-tree references; will be deleted in
// a future cleanup PR after a grep window confirms no callers.
import { OxfordView } from "./OxfordView";
import { DelphiView } from "./DelphiView";
import { RedTeamView } from "./RedTeamView";
import { ModeratorPicker, type ModeratorPickerCandidate } from "./ModeratorPicker";
import { getRoleColor, getModeColor } from "../utils/roleColors";
import "../styles/discussion.css";

function getPhaseLabel(state: DiscussionState, closingRound: boolean): string {
  if (closingRound) return "Aggregating...";
  const { phase, mode } = state;
  if (mode === "pipeline") {
    if (phase === "pipeline_active") {
      const stage = (state.pipeline_stage ?? 0) + 1;
      const total = state.pipeline_order?.length ?? "?";
      return `Stage ${stage}/${total}`;
    }
    if (phase === "pipeline_complete") return "Complete";
  }
  switch (phase) {
    case "submitting": return "Submitting";
    case "aggregating": return "Aggregating";
    case "reviewing": return "Reviewing";
    case "paused": return "Paused";
    case "complete": return "Complete";
    case "oxford_opening": return "Opening Statements";
    case "oxford_rebuttal": return "Rebuttals";
    case "oxford_closing": return "Closing Statements";
    case "oxford_vote": return "Voting";
    case "oxford_declaration": return "Declaration";
    case "red_team_attacking": return "Attacking";
    case "red_team_defending": return "Defending";
    case "red_team_assessment": return "Assessment";
    case "continuous_active": return "Active";
    default: return phase || "";
  }
}

/**
 * Convert the current session's participant list into the shape the
 * ModeratorPicker component consumes. Participants are stored as
 * "role:instance" strings in discussion.json; the picker expects the
 * split-out role + instance for a clean invoke("set_session_moderator").
 */
function buildModeratorCandidates(state: DiscussionState): ModeratorPickerCandidate[] {
  const participants = state.participants ?? [];
  return participants
    .map((id) => {
      const [role, instanceStr] = id.split(":");
      const instance = Number.parseInt(instanceStr ?? "0", 10);
      if (!role || Number.isNaN(instance)) return null;
      return { id, role, instance };
    })
    .filter((c): c is ModeratorPickerCandidate => c !== null);
}

function getTerminationLabel(state: DiscussionState): string | null {
  const strategy = getTerminationStrategy(state.settings);
  switch (strategy.type) {
    case "fixed_rounds": return `${strategy.rounds} round${strategy.rounds !== 1 ? "s" : ""}`;
    case "consensus": return `Consensus ≥${Math.round(strategy.threshold * 100)}%`;
    case "moderator_call": return "Moderator decides";
    case "time_bound": return `${strategy.minutes}min`;
    case "unlimited": return "Unlimited";
    default: return null;
  }
}

interface DiscussionPanelProps {
  discussionState: DiscussionState;
  /** Board messages for format-specific views (Oxford needs them for speech cards) */
  messages?: BoardMessage[];
  /** Whether a round-close operation is in progress */
  closingRound: boolean;
  /** Continuous mode timeout value in seconds */
  continuousTimeout: number;
  /** Whether the auto-moderator ethereal agent is running */
  autoModActive: boolean;
  /** Callback: close the current round */
  onCloseRound: () => void;
  /** Callback: end the entire discussion */
  onEndDiscussion: () => void;
  /** Callback: change continuous timeout */
  onSetContinuousTimeout: (seconds: number) => void;
  /** Callback: pause/resume the discussion */
  onTogglePause?: () => void;
  /** Callback: update max_rounds mid-discussion */
  onSetMaxRounds?: (rounds: number | null) => void;
  /** Callback: change the moderator. Parent wires to invoke("set_session_moderator").
   *  When omitted, the Change affordance is hidden (read-only moderator display). */
  onSetModerator?: (role: string, instance: number) => void | Promise<void>;
}

/**
 * DiscussionPanel — self-contained discussion status and control panel.
 *
 * Renders format-specific views based on the discussion mode:
 * - Pipeline: PipelineStepper + turn info
 * - Delphi: Submission tracker + round history
 * - Oxford: Phase indicator (future: split view)
 * - Red Team: Severity summary (future: attack/defense columns)
 * - Continuous: Timeout controls + micro-round list
 *
 * This component is the first extraction step from the CollabTab monolith.
 * Future phases will add richer format-specific sub-views.
 */
export function DiscussionPanel({
  discussionState,
  messages = [],
  closingRound,
  continuousTimeout,
  autoModActive,
  onCloseRound,
  onEndDiscussion,
  onSetContinuousTimeout,
  onTogglePause,
  onSetMaxRounds,
  onSetModerator,
}: DiscussionPanelProps) {
  const [modPickerOpen, setModPickerOpen] = useState(false);
  if (!discussionState.active) return null;

  const modeColor = getModeColor(discussionState.mode);
  // Mode-aware label. Falls back to "Session" (mode-neutral) rather than
  // "Discussion" when the mode string is missing — the user clearly asked
  // for the UI to stop calling everything "discussion" when it's running
  // as a pipeline / delphi / etc. (human msg 511).
  const modeLabel = discussionState.mode
    ? discussionState.mode.charAt(0).toUpperCase() + discussionState.mode.slice(1)
    : "Session";
  const phaseLabel = getPhaseLabel(discussionState, closingRound);
  const terminationLabel = getTerminationLabel(discussionState);
  // automationLevel reserved for Phase 2 moderator controls
  void getAutomationLevel(discussionState.settings);
  const audienceConfig = getAudienceConfig(discussionState.settings);

  return (
    <div
      className="discussion-panel"
      role="region"
      aria-label="Active session"
      aria-live="polite"
    >
      {/* Header: Mode badge + Phase + Round + Controls */}
      <div className="discussion-panel-header">
        <div className="discussion-panel-header-left">
          {/* Mode badge */}
          <span
            className="discussion-panel-mode-badge"
            style={{ background: `${modeColor}20`, color: modeColor, borderColor: `${modeColor}66` }}
          >
            {modeLabel}
          </span>

          {/* pr-pipeline-discussionpanel-dedupe (PR-7, 2026-04-19): pipeline
              sub-mode badge + pipeline turn-info badge removed. Pipeline status
              is now rendered exclusively via SequenceSessionCard
              (SequenceBanner + QueueVisualization + auto-advance indicator)
              per architect msg 1034 + tech-leader msg 1054. Avoids the 2-UI
              overlap the human reported in msg 1023. */}

          {/* Phase badge */}
          <span className={`discussion-panel-phase-badge${discussionState.paused_at ? " discussion-panel-paused" : ""}`}>
            {discussionState.paused_at ? "Paused" : phaseLabel}
          </span>

          {/* Round counter (non-pipeline modes only — pipeline shows holder
              + queue via SequenceSessionCard now). */}
          {discussionState.mode !== "pipeline" && (
            <span className="discussion-panel-round-info">
              Round {(discussionState.current_round ?? 0) + 1}
              {discussionState.settings?.max_rounds && discussionState.settings.max_rounds < 900 ? ` / ${discussionState.settings.max_rounds}` : ""}
            </span>
          )}

          {/* Termination strategy indicator */}
          {terminationLabel && (
            <span className="discussion-panel-termination" title={`Termination: ${terminationLabel}`}>
              {terminationLabel}
            </span>
          )}

          {/* Moderator info (+ change affordance when the parent wires onSetModerator) */}
          {discussionState.moderator && (
            <span className="discussion-panel-moderator" style={{ position: "relative" }}>
              Mod:{" "}
              <span style={{ color: getRoleColor(discussionState.moderator.split(":")[0]) }}>
                {discussionState.moderator}
              </span>
              {autoModActive && (
                <span className="discussion-panel-auto-mod" title="Automated Session Moderator is active">
                  Auto-Mod
                </span>
              )}
              {onSetModerator && (
                <>
                  <button
                    type="button"
                    className="discussion-panel-mod-change"
                    onClick={() => setModPickerOpen((prev) => !prev)}
                    aria-haspopup="menu"
                    aria-expanded={modPickerOpen}
                    title="Designate a different moderator"
                  >
                    Change
                  </button>
                  <ModeratorPicker
                    open={modPickerOpen}
                    onClose={() => setModPickerOpen(false)}
                    candidates={buildModeratorCandidates(discussionState)}
                    currentModeratorId={discussionState.moderator ?? null}
                    onSelect={async (role, instance) => {
                      await onSetModerator(role, instance);
                    }}
                  />
                </>
              )}
            </span>
          )}

          {/* Missing moderator warning (+ designate affordance when writable) */}
          {!discussionState.moderator && (
            <span className="discussion-panel-no-mod-warning" style={{ position: "relative" }}>
              Unmoderated
              {onSetModerator && (
                <>
                  <button
                    type="button"
                    className="discussion-panel-mod-change"
                    onClick={() => setModPickerOpen((prev) => !prev)}
                    aria-haspopup="menu"
                    aria-expanded={modPickerOpen}
                    title="Designate a moderator"
                  >
                    Designate
                  </button>
                  <ModeratorPicker
                    open={modPickerOpen}
                    onClose={() => setModPickerOpen(false)}
                    candidates={buildModeratorCandidates(discussionState)}
                    currentModeratorId={null}
                    onSelect={async (role, instance) => {
                      await onSetModerator(role, instance);
                    }}
                  />
                </>
              )}
            </span>
          )}

          {/* Continuous mode timeout selector */}
          {discussionState.mode === "continuous" && (
            <select
              className="discussion-panel-timeout-select"
              value={continuousTimeout}
              onChange={(e) => onSetContinuousTimeout(Number(e.target.value))}
              aria-label="Auto-close timeout"
            >
              <option value={30}>30s</option>
              <option value={60}>60s</option>
              <option value={120}>2m</option>
              <option value={300}>5m</option>
            </select>
          )}
        </div>

        {/* Action buttons */}
        <div className="discussion-panel-actions">
          {discussionState.phase === "submitting" && discussionState.mode !== "continuous" && (
            <button
              className="discussion-panel-btn"
              onClick={onCloseRound}
              disabled={closingRound}
            >
              {closingRound ? "Closing..." : "Close Round"}
            </button>
          )}
          {onTogglePause && (
            <button
              className={`discussion-panel-btn${discussionState.paused_at ? " discussion-panel-btn-resume" : ""}`}
              onClick={onTogglePause}
              title={discussionState.paused_at ? "Resume discussion" : "Pause after current speaker"}
            >
              {discussionState.paused_at ? "Resume" : "Pause"}
            </button>
          )}
          <button
            className="discussion-panel-btn discussion-panel-btn-end"
            onClick={onEndDiscussion}
          >
            End
          </button>
          {onSetMaxRounds && discussionState.settings?.max_rounds != null && (
            <select
              className="discussion-panel-rounds-select"
              value={discussionState.settings.max_rounds >= 900 ? "unlimited" : String(discussionState.settings.max_rounds)}
              onChange={(e) => {
                const val = e.target.value;
                onSetMaxRounds(val === "unlimited" ? null : parseInt(val));
              }}
              title="Change round limit"
              aria-label="Round limit"
            >
              <option value="3">3 rounds</option>
              <option value="5">5 rounds</option>
              <option value="10">10 rounds</option>
              <option value="20">20 rounds</option>
              <option value="unlimited">Unlimited</option>
            </select>
          )}
        </div>
      </div>

      {/* Topic */}
      {discussionState.topic && (
        <div className="discussion-panel-topic" title={discussionState.topic}>
          {discussionState.topic}
        </div>
      )}

      {/* Format-specific views.
          pr-pipeline-discussionpanel-dedupe (PR-7, 2026-04-19): PipelineStepper
          rendering removed. Pipeline mode's stage-by-stage visualization lives
          in SequenceSessionCard's QueueVisualization now (renders the same
          pipeline_order data as a horizontal chip strip with done/current/
          upcoming states). Eliminates the 2-UI overlap the human reported in
          msg 1023. */}
      {discussionState.mode === "oxford" && discussionState.oxford_teams && (
        <OxfordView discussionState={discussionState} messages={messages} />
      )}
      {discussionState.mode === "delphi" && (
        <DelphiView discussionState={discussionState} />
      )}
      {(discussionState.mode as string) === "red_team" && discussionState.attack_chains && (
        <RedTeamView discussionState={discussionState} />
      )}

      {/* Delphi submission tracker */}
      {discussionState.phase === "submitting" && discussionState.rounds.length > 0 && (() => {
        const currentRound = discussionState.rounds[discussionState.rounds.length - 1];
        const submittedBy = new Set((currentRound?.submissions || []).map(s => s.from));
        const eligible = (discussionState.participants || []).filter(p => p !== discussionState.moderator);
        if (eligible.length === 0) return null;
        return (
          <div className="discussion-panel-submissions" aria-label={`${submittedBy.size} of ${eligible.length} submitted`}>
            {eligible.map(pid => {
              const [role] = pid.split(":");
              const didSubmit = submittedBy.has(pid);
              return (
                <span
                  key={pid}
                  className={`discussion-panel-participant${didSubmit ? " dp-submitted" : ""}`}
                  title={`${pid}${didSubmit ? " — submitted" : " — waiting"}`}
                >
                  <span className="dp-check" aria-hidden="true">{didSubmit ? "✓" : "•"}</span>
                  <span style={{ color: getRoleColor(role) }}>{pid}</span>
                </span>
              );
            })}
          </div>
        );
      })()}

      {/* Round history mini-bar */}
      {discussionState.rounds.length > 1 && (
        <div className="discussion-panel-rounds-bar" aria-label="Round history">
          {discussionState.rounds.map((round, i) => (
            <span
              key={i}
              className={`dp-round-pip${round.closed_at ? " dp-round-closed" : " dp-round-open"}`}
              title={`Round ${round.number}: ${round.closed_at ? "closed" : "open"} — ${round.submissions?.length || 0} submissions`}
            />
          ))}
        </div>
      )}

      {/* Audience bar */}
      {audienceConfig.enabled && (
        <div className="discussion-panel-audience-bar">
          <span className="discussion-panel-audience-gate">
            {audienceConfig.gate.charAt(0).toUpperCase() + audienceConfig.gate.slice(1)}
          </span>
          <span className="discussion-panel-audience-size">
            {audienceConfig.size} persona{audienceConfig.size !== 1 ? "s" : ""}
          </span>
        </div>
      )}
    </div>
  );
}
