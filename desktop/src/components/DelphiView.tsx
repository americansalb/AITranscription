import type { DiscussionState } from "../lib/collabTypes";
import { getRoleColor } from "../utils/roleColors";

interface DelphiViewProps {
  discussionState: DiscussionState;
}

/**
 * DelphiView — Blind submission + reveal layout for Delphi-format discussions.
 *
 * Submission phase: Shows participant avatars with submitted/waiting status. NO content visible.
 * Reveal phase: Grid of anonymized submission cards with convergence meter.
 */
export function DelphiView({ discussionState }: DelphiViewProps) {
  const rounds = discussionState.rounds || [];
  const currentRound = rounds.length > 0 ? rounds[rounds.length - 1] : null;
  const phase = discussionState.phase;
  const isSubmitting = phase === "submitting";
  const isRevealing = phase === "aggregating" || phase === "reviewing";
  const participants = (discussionState.participants || []).filter(p => p !== discussionState.moderator);

  // Convergence: percentage of rounds where submissions showed agreement
  // Simple heuristic: ratio of submissions in current round vs participants
  const submittedSet = new Set((currentRound?.submissions || []).map(s => s.from));
  const submittedCount = submittedSet.size;
  const totalParticipants = participants.length;

  // Compute convergence history from past rounds (simplified: submission completion rate)
  const convergenceHistory = rounds
    .filter(r => r.closed_at)
    .map(r => {
      const subs = r.submissions?.length || 0;
      const total = participants.length || 1;
      return Math.round((subs / total) * 100);
    });

  return (
    <div className="delphi-view">
      {isSubmitting && currentRound && (
        <div className="delphi-submission-phase">
          <div className="delphi-phase-label">
            <span className="delphi-phase-badge delphi-phase-submitting">SUBMITTING</span>
            <span className="delphi-submission-count">{submittedCount} of {totalParticipants} submitted</span>
          </div>

          {/* Participant avatars */}
          <div className="delphi-avatars">
            {participants.map(pid => {
              const [role] = pid.split(":");
              const didSubmit = submittedSet.has(pid);
              const initial = (role.charAt(0) || "?").toUpperCase();
              return (
                <div
                  key={pid}
                  className={`delphi-avatar${didSubmit ? " delphi-avatar-submitted" : ""}`}
                  title={`${pid}${didSubmit ? " — submitted" : " — waiting"}`}
                >
                  <div
                    className="delphi-avatar-circle"
                    style={{ borderColor: didSubmit ? "#22c55e" : getRoleColor(role) }}
                  >
                    {didSubmit ? "✓" : initial}
                  </div>
                  <span className="delphi-avatar-label">{role}</span>
                </div>
              );
            })}
          </div>

          <div className="delphi-blind-notice">Content hidden until all participants submit</div>
        </div>
      )}

      {isRevealing && currentRound && (
        <div className="delphi-reveal-phase">
          <div className="delphi-phase-label">
            <span className="delphi-phase-badge delphi-phase-reveal">REVEAL</span>
          </div>

          {/* Anonymized submission cards */}
          <div className="delphi-submission-grid">
            {(currentRound.submissions || []).map((sub, idx) => (
              <div
                key={sub.message_id}
                className="delphi-submission-card"
                style={{ animationDelay: `${idx * 100}ms` }}
              >
                <div className="delphi-submission-header">Submission #{idx + 1}</div>
                <div className="delphi-submission-body">
                  {/* Content would be loaded from the message board by message_id */}
                  Submission from anonymous participant (message #{sub.message_id})
                </div>
              </div>
            ))}
          </div>
        </div>
      )}

      {/* Convergence meter */}
      {convergenceHistory.length > 0 && (
        <div className="delphi-convergence">
          <div className="delphi-convergence-label">
            Convergence: {convergenceHistory[convergenceHistory.length - 1]}%
          </div>
          <div className="delphi-convergence-track">
            <div
              className="delphi-convergence-fill"
              style={{
                width: `${convergenceHistory[convergenceHistory.length - 1]}%`,
                background: convergenceHistory[convergenceHistory.length - 1] >= 90 ? "#22c55e"
                  : convergenceHistory[convergenceHistory.length - 1] >= 70 ? "#818cf8"
                  : convergenceHistory[convergenceHistory.length - 1] >= 40 ? "#f59e0b"
                  : "#ef4444",
              }}
            />
          </div>
          {/* Round history dots */}
          <div className="delphi-convergence-history">
            {convergenceHistory.map((pct, i) => (
              <span
                key={i}
                className="delphi-convergence-dot"
                style={{
                  background: pct >= 90 ? "#22c55e" : pct >= 70 ? "#818cf8" : pct >= 40 ? "#f59e0b" : "#ef4444",
                }}
                title={`Round ${i + 1}: ${pct}%`}
              />
            ))}
          </div>
        </div>
      )}
    </div>
  );
}
