import type { DiscussionState, BoardMessage } from "../lib/collabTypes";
import { getRoleColor } from "../utils/roleColors";

interface OxfordViewProps {
  discussionState: DiscussionState;
  messages: BoardMessage[];
}

/**
 * OxfordView — Split-screen debate layout for Oxford-format discussions.
 *
 * Left column (blue): Proposition team arguments
 * Right column (red): Opposition team arguments
 * Bottom bar: Vote tally (visible only during vote phase)
 */
export function OxfordView({ discussionState, messages }: OxfordViewProps) {
  const teams = discussionState.oxford_teams;
  const votes = discussionState.oxford_votes;
  const motion = discussionState.oxford_motion || discussionState.topic;
  const phase = discussionState.phase;

  // Filter discussion messages (submissions/broadcasts from participants)
  const discussionMsgs = messages.filter(
    m => (m.type === "submission" || m.type === "broadcast") &&
         m.metadata?.discussion_action !== "start"
  );

  // Split messages by team
  const propMembers = new Set(teams?.proposition || []);
  const oppMembers = new Set(teams?.opposition || []);
  const propMessages = discussionMsgs.filter(m => propMembers.has(m.from));
  const oppMessages = discussionMsgs.filter(m => oppMembers.has(m.from));

  const isVotePhase = phase === "oxford_vote" || phase === "oxford_declaration";
  const totalVotes = (votes?.for_count || 0) + (votes?.against_count || 0) + (votes?.abstain_count || 0);
  const forPct = totalVotes > 0 ? (votes!.for_count / totalVotes) * 100 : 0;
  const againstPct = totalVotes > 0 ? (votes!.against_count / totalVotes) * 100 : 0;

  return (
    <div className="oxford-view">
      {/* Motion banner */}
      {motion && (
        <div className="oxford-motion-banner">
          <span className="oxford-motion-label">Motion</span>
          <span className="oxford-motion-text">{motion}</span>
        </div>
      )}

      {/* Two-column layout */}
      <div className="oxford-columns">
        {/* Proposition (left, blue) */}
        <div className="oxford-column oxford-column-prop">
          <div className="oxford-column-header oxford-prop-header">PROPOSITION</div>
          <div className="oxford-column-members">
            {(teams?.proposition || []).map(pid => {
              const [role] = pid.split(":");
              return <span key={pid} className="oxford-member" style={{ color: getRoleColor(role) }}>{pid}</span>;
            })}
          </div>
          <div className="oxford-speeches">
            {propMessages.map(msg => (
              <div key={msg.id} className="oxford-speech-card oxford-speech-prop">
                <div className="oxford-speech-speaker" style={{ color: getRoleColor(msg.from.split(":")[0]) }}>
                  {msg.from}
                </div>
                <div className="oxford-speech-body">{msg.body}</div>
              </div>
            ))}
            {propMessages.length === 0 && (
              <div className="oxford-empty">Awaiting proposition arguments...</div>
            )}
          </div>
        </div>

        {/* Opposition (right, red) */}
        <div className="oxford-column oxford-column-opp">
          <div className="oxford-column-header oxford-opp-header">OPPOSITION</div>
          <div className="oxford-column-members">
            {(teams?.opposition || []).map(pid => {
              const [role] = pid.split(":");
              return <span key={pid} className="oxford-member" style={{ color: getRoleColor(role) }}>{pid}</span>;
            })}
          </div>
          <div className="oxford-speeches">
            {oppMessages.map(msg => (
              <div key={msg.id} className="oxford-speech-card oxford-speech-opp">
                <div className="oxford-speech-speaker" style={{ color: getRoleColor(msg.from.split(":")[0]) }}>
                  {msg.from}
                </div>
                <div className="oxford-speech-body">{msg.body}</div>
              </div>
            ))}
            {oppMessages.length === 0 && (
              <div className="oxford-empty">Awaiting opposition arguments...</div>
            )}
          </div>
        </div>
      </div>

      {/* Vote tally bar */}
      {isVotePhase && votes && (
        <div className="oxford-vote-bar">
          <span className="oxford-vote-label oxford-vote-for">For: {votes.for_count}</span>
          <div className="oxford-vote-track">
            <div className="oxford-vote-fill oxford-vote-fill-for" style={{ width: `${forPct}%` }} />
            <div className="oxford-vote-fill oxford-vote-fill-against" style={{ width: `${againstPct}%` }} />
          </div>
          <span className="oxford-vote-label oxford-vote-against">Against: {votes.against_count}</span>
        </div>
      )}
    </div>
  );
}
