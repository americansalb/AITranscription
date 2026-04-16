import type { DiscussionState } from "../lib/collabTypes";

const SEVERITY_COLORS: Record<string, { bg: string; color: string }> = {
  critical: { bg: "rgba(239, 68, 68, 0.25)", color: "#fca5a5" },
  high: { bg: "rgba(249, 115, 22, 0.25)", color: "#fdba74" },
  medium: { bg: "rgba(245, 158, 11, 0.25)", color: "#fde68a" },
  low: { bg: "rgba(156, 163, 175, 0.25)", color: "#d1d5db" },
};

const STATUS_ICONS: Record<string, string> = {
  addressed: "✓",
  partially_addressed: "◐",
  unaddressed: "⊘",
};

interface RedTeamViewProps {
  discussionState: DiscussionState;
}

/**
 * RedTeamView — Attack/defense chain layout for Red Team discussions.
 *
 * Left column (red): Attack probes with severity badges
 * Right column (green): Defense responses linked to attacks
 * Bottom bar: Summary counts
 */
export function RedTeamView({ discussionState }: RedTeamViewProps) {
  const attacks = discussionState.attack_chains || [];
  const severitySummary = discussionState.severity_summary || {};
  const unaddressedCount = discussionState.unaddressed_count ?? attacks.filter(a => a.status === "unaddressed").length;
  const addressedCount = attacks.filter(a => a.status === "addressed").length;
  const partialCount = attacks.filter(a => a.status === "partially_addressed").length;

  return (
    <div className="redteam-view">
      {/* Severity summary bar */}
      <div className="redteam-summary-bar">
        {Object.entries(severitySummary).map(([sev, count]) => (
          <span key={sev} className="redteam-summary-badge" style={{
            background: SEVERITY_COLORS[sev]?.bg || "rgba(156, 163, 175, 0.25)",
            color: SEVERITY_COLORS[sev]?.color || "#d1d5db",
          }}>
            {sev.toUpperCase()}: {count as number}
          </span>
        ))}
        <span className="redteam-summary-unaddressed">
          Unaddressed: {unaddressedCount} of {attacks.length}
        </span>
      </div>

      {/* Two-column layout */}
      <div className="redteam-columns">
        {/* Attacks (left, red) */}
        <div className="redteam-column redteam-column-attack">
          <div className="redteam-column-header redteam-attack-header">
            ATTACKS
            <span className="redteam-count-badge">{attacks.length} probes</span>
          </div>
          {attacks.map((chain, idx) => {
            const sevColor = SEVERITY_COLORS[chain.severity] || SEVERITY_COLORS.low;
            const isUnaddressed = chain.status === "unaddressed";
            return (
              <div
                key={idx}
                className={`redteam-attack-card${isUnaddressed ? " redteam-attack-unaddressed" : ""}${chain.status === "addressed" ? " redteam-attack-addressed" : ""}`}
              >
                <span className="redteam-severity-badge" style={{ background: sevColor.bg, color: sevColor.color }}>
                  {chain.severity.toUpperCase()}
                </span>
                <div className="redteam-attack-id">Attack #{chain.attack_message_id}</div>
              </div>
            );
          })}
          {attacks.length === 0 && (
            <div className="redteam-empty">No attacks submitted yet...</div>
          )}
        </div>

        {/* Defenses (right, green) */}
        <div className="redteam-column redteam-column-defense">
          <div className="redteam-column-header redteam-defense-header">
            DEFENSES
            <span className="redteam-count-badge">{addressedCount + partialCount} responses</span>
          </div>
          {attacks.filter(a => a.defense_message_id != null).map((chain, idx) => {
            const statusIcon = STATUS_ICONS[chain.status] || "?";
            return (
              <div key={idx} className={`redteam-defense-card redteam-defense-${chain.status}`}>
                <span className="redteam-status-icon">{statusIcon}</span>
                <div className="redteam-defense-id">
                  Defense #{chain.defense_message_id} → Attack #{chain.attack_message_id}
                </div>
                <span className="redteam-status-label">{chain.status.replace("_", " ")}</span>
              </div>
            );
          })}
          {attacks.filter(a => a.defense_message_id != null).length === 0 && (
            <div className="redteam-empty">No defenses submitted yet...</div>
          )}
        </div>
      </div>

      {/* Bottom summary */}
      <div className="redteam-bottom-summary">
        <span>{attacks.length} attacks</span>
        <span className="redteam-sep">|</span>
        <span style={{ color: "#4ade80" }}>{addressedCount} addressed</span>
        <span className="redteam-sep">|</span>
        <span style={{ color: "#fbbf24" }}>{partialCount} partial</span>
        <span className="redteam-sep">|</span>
        <span style={{ color: "#f87171" }}>{unaddressedCount} unaddressed</span>
      </div>
    </div>
  );
}
