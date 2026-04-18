import { useEffect, useState } from "react";
import { getRoleColor } from "../utils/roleColors";

/**
 * Shown at the top of CollabTab whenever a sequential-turn session is active.
 * One-glance summary: topic, who holds the turn, how many stages are left,
 * elapsed time. Every agent should be able to look at this banner and
 * answer "am I next? how soon?" without scanning the board.
 *
 * Per manager:0 msgs 377 + 388 + tech-leader:0 msg 378 ANCHOR 9:
 * turn state lives in discussion.json's `turn` block. Parent (CollabTab)
 * passes the block as `turn` prop; banner renders nothing when null/undefined.
 * Backend emits `sequence-state-changed` Tauri events on every mutation;
 * subscription to that event lives in CollabTab (single source of truth),
 * not here — banner is presentational.
 */

export interface SequenceTurnRequest {
  requester: string;
  reason?: string;
  requested_at: string;
}

export interface SequenceTurnState {
  current_holder: string | null;
  queue_remaining: string[];
  queue_completed?: string[];
  started_at: string;
  turn_started_at: string;
  initiator: string;
  topic: string;
  paused_for_human?: boolean;
  pending_requests?: SequenceTurnRequest[];
}

interface Props {
  turn: SequenceTurnState | null | undefined;
  selfRoleInstance: string | null;
}

function formatElapsed(ms: number): string {
  if (ms < 0) return "0s";
  const totalSeconds = Math.floor(ms / 1000);
  if (totalSeconds < 60) return `${totalSeconds}s`;
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  if (minutes < 60) return `${minutes}m ${seconds}s`;
  const hours = Math.floor(minutes / 60);
  return `${hours}h ${minutes % 60}m`;
}

function parseRoleInstance(id: string | null): { role: string; instance: string } | null {
  if (!id) return null;
  const idx = id.indexOf(":");
  if (idx === -1) return { role: id, instance: "0" };
  return { role: id.slice(0, idx), instance: id.slice(idx + 1) };
}

export default function SequenceBanner({ turn, selfRoleInstance }: Props) {
  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    if (!turn) return;
    const interval = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(interval);
  }, [turn]);

  if (!turn) return null;

  const holder = parseRoleInstance(turn.current_holder);
  const holderColor = holder ? getRoleColor(holder.role) : "#888";
  const completedCount = turn.queue_completed?.length ?? 0;
  const remainingCount = turn.queue_remaining?.length ?? 0;
  const totalStages = completedCount + (turn.current_holder ? 1 : 0) + remainingCount;
  const currentStage = completedCount + (turn.current_holder ? 1 : 0);

  const turnStartMs = Date.parse(turn.turn_started_at);
  const sessionStartMs = Date.parse(turn.started_at);
  const turnElapsed = Number.isFinite(turnStartMs) ? formatElapsed(now - turnStartMs) : "—";
  const sessionElapsed = Number.isFinite(sessionStartMs) ? formatElapsed(now - sessionStartMs) : "—";

  const isSelfTurn =
    selfRoleInstance != null && turn.current_holder === selfRoleInstance;

  const selfPositionInRemaining =
    selfRoleInstance != null && turn.queue_remaining
      ? turn.queue_remaining.indexOf(selfRoleInstance)
      : -1;
  const selfPositionHint =
    isSelfTurn
      ? "YOUR TURN"
      : selfPositionInRemaining >= 0
        ? `You are ${ordinal(selfPositionInRemaining + 1)} in queue`
        : null;

  return (
    <div
      className={`sequence-banner${turn.paused_for_human ? " sequence-banner-paused" : ""}${isSelfTurn ? " sequence-banner-self-turn" : ""}`}
      role="region"
      aria-label={`Sequential turn session — ${turn.topic}`}
      style={{ borderLeftColor: holderColor }}
    >
      <div className="sequence-banner-row">
        <div className="sequence-banner-topic">
          <span className="sequence-banner-label">Session:</span>
          <span className="sequence-banner-topic-text">{turn.topic}</span>
        </div>
        <div className="sequence-banner-stage">
          Stage {currentStage} of {totalStages}
        </div>
      </div>
      <div className="sequence-banner-row">
        <div className="sequence-banner-holder">
          <span className="sequence-banner-label">On turn:</span>
          <span
            className="sequence-banner-role-badge"
            style={{ background: holderColor }}
          >
            {turn.current_holder ?? "—"}
          </span>
          <span className="sequence-banner-turn-elapsed" title="How long this turn has been held">
            {turnElapsed}
          </span>
        </div>
        {selfPositionHint && (
          <div
            className={`sequence-banner-self-hint${isSelfTurn ? " sequence-banner-self-hint-now" : ""}`}
          >
            {selfPositionHint}
          </div>
        )}
      </div>
      {turn.paused_for_human && (
        <div className="sequence-banner-pause-note">
          Paused — human is speaking. Sequence will resume after.
        </div>
      )}
      <div className="sequence-banner-meta">
        <span title="Total session time">Session: {sessionElapsed}</span>
        <span className="sequence-banner-dot">·</span>
        <span title="Who started this session">Started by {turn.initiator}</span>
        {remainingCount > 0 && (
          <>
            <span className="sequence-banner-dot">·</span>
            <span>{remainingCount} after current</span>
          </>
        )}
        {completedCount > 0 && (
          <>
            <span className="sequence-banner-dot">·</span>
            <span>{completedCount} done</span>
          </>
        )}
      </div>
    </div>
  );
}

function ordinal(n: number): string {
  const s = ["th", "st", "nd", "rd"];
  const v = n % 100;
  return `${n}${s[(v - 20) % 10] ?? s[v] ?? s[0]}`;
}
