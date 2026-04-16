import type { DiscussionState, BoardMessage } from "../lib/collabTypes";
import { getRoleColor } from "../utils/roleColors";

interface PipelineStepperProps {
  discussionState: DiscussionState;
  messages?: BoardMessage[];
  /** When true, only shows the stepper node bar — hides content cards and status bar */
  compact?: boolean;
}

/** Truncate text to maxLen chars with ellipsis */
function truncate(text: string, maxLen: number): string {
  if (text.length <= maxLen) return text;
  return text.slice(0, maxLen).trimEnd() + "...";
}

/**
 * PipelineStepper — visual pipeline progress with stage content cards.
 *
 * Top: horizontal node bar (circles + connectors)
 * Middle: stage content cards showing what each completed agent said
 * Bottom: status bar with round counter + pipeline mode badge
 */
export function PipelineStepper({ discussionState, messages = [], compact = false }: PipelineStepperProps) {
  const pipelineOrder = discussionState.pipeline_order;
  if (!pipelineOrder || pipelineOrder.length === 0) return null;

  const stage = discussionState.pipeline_stage ?? 0;
  const isPipelineDone = discussionState.phase === "pipeline_complete";
  const currentRound = discussionState.current_round ?? 1;
  const pipelineMode = discussionState.pipeline_mode ?? "discussion";
  const outputs = discussionState.pipeline_outputs ?? [];

  // Build a map of stage outputs to their message content
  const outputMap = new Map<number, { agent: string; body: string; messageId: number }>();
  for (const output of outputs) {
    const msg = messages.find(m => m.id === output.message_id);
    if (msg) {
      outputMap.set(output.stage, {
        agent: output.agent,
        body: msg.body,
        messageId: output.message_id,
      });
    }
  }

  return (
    <div className="pipeline-stepper">
      {/* Pipeline mode badge + round counter (hidden in compact mode — DiscussionPanel header shows this) */}
      {!compact && (
        <div className="pipeline-status-bar">
          <span className={`pipeline-mode-badge pipeline-mode-${pipelineMode}`}>
            {pipelineMode === "action" ? "Action" : "Discussion"}
          </span>
          <span className="pipeline-stepper-counter">
            {isPipelineDone
              ? `Complete · Round ${currentRound + 1}`
              : `Stage ${stage + 1} of ${pipelineOrder.length} · Round ${currentRound + 1}`}
          </span>
        </div>
      )}

      {/* Horizontal stepper bar */}
      <div
        className="pipeline-stepper-bar"
        role="list"
        aria-label={`Pipeline: stage ${Math.min(stage + 1, pipelineOrder.length)} of ${pipelineOrder.length}${isPipelineDone ? " (complete)" : ""}`}
      >
        {pipelineOrder.map((agentId, idx) => {
          const [role] = agentId.split(":");
          const isComplete = idx < stage || isPipelineDone;
          const isCurrent = idx === stage && !isPipelineDone;
          const roleColor = getRoleColor(role);

          return (
            <span key={agentId} style={{ display: "contents" }}>
              {idx > 0 && (
                <span
                  className={`pipeline-stepper-connector${isComplete || isCurrent ? " pipeline-stepper-connector-done" : ""}`}
                  aria-hidden="true"
                />
              )}
              <span
                className={`pipeline-stepper-node${isCurrent ? " pipeline-stepper-node-active" : ""}${isComplete ? " pipeline-stepper-node-done" : ""}${!isComplete && !isCurrent ? " pipeline-stepper-node-waiting" : ""}`}
                role="listitem"
                aria-current={isCurrent ? "step" : undefined}
                aria-label={`Stage ${idx + 1}: ${agentId}, ${isCurrent ? "current turn" : isComplete ? "completed" : "waiting"}`}
                title={`${agentId}${isCurrent ? " — current turn" : isComplete ? " — done" : " — waiting"}`}
              >
                <span
                  className="pipeline-stepper-circle"
                  style={{ borderColor: isCurrent ? roleColor : undefined }}
                >
                  {isComplete ? "✓" : isCurrent ? "●" : idx + 1}
                </span>
                <span
                  className="pipeline-stepper-label"
                  style={{ color: isCurrent ? roleColor : undefined }}
                >
                  {agentId}
                </span>
              </span>
            </span>
          );
        })}
      </div>

      {/* Stage content cards (hidden in compact mode — content lives in main message board) */}
      {!compact && (
        <div className="pipeline-stage-cards">
          {pipelineOrder.map((agentId, idx) => {
            const [role] = agentId.split(":");
            const isComplete = idx < stage || isPipelineDone;
            const isCurrent = idx === stage && !isPipelineDone;
            const roleColor = getRoleColor(role);
            const output = outputMap.get(idx);

            if (!isComplete && !isCurrent) return null; // Don't show future stages

            return (
              <div
                key={agentId}
                className={`pipeline-stage-card${isCurrent ? " pipeline-stage-card-active" : ""}${isComplete ? " pipeline-stage-card-done" : ""}`}
                style={{ borderLeftColor: roleColor }}
              >
                <div className="pipeline-stage-card-header">
                  <span className="pipeline-stage-card-agent" style={{ color: roleColor }}>
                    {agentId}
                  </span>
                  <span className="pipeline-stage-card-badge">
                    {isCurrent ? "In Progress" : `Stage ${idx + 1}`}
                  </span>
                </div>
                <div className="pipeline-stage-card-body">
                  {isCurrent && !output ? (
                    <span className="pipeline-stage-waiting">Waiting for response...</span>
                  ) : output ? (
                    truncate(output.body, 300)
                  ) : (
                    <span className="pipeline-stage-waiting">No output</span>
                  )}
                </div>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
