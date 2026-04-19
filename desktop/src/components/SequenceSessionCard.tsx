import SequenceBanner, { type SequenceTurnState } from "./SequenceBanner";
import QueueVisualization from "./QueueVisualization";
import HumanSequenceOverrideBar from "./HumanSequenceOverrideBar";
import PendingTurnRequests from "./PendingTurnRequests";
import ModeratorSequencePanel, {
  type ModeratorSequencePanelRosterEntry,
} from "./ModeratorSequencePanel";

/**
 * Single unified visual zone for an active sequential-turn session. Wraps
 * SequenceBanner / QueueVisualization / HumanSequenceOverrideBar /
 * PendingTurnRequests / ModeratorSequencePanel into one card so the Collab
 * tab stops looking like a stack of 5 unrelated containers.
 *
 * Returns null when no sequence is active — the whole zone disappears from
 * the page, not just each individual child.
 *
 * Per human msg 602 ("clean UI right now confusing af") + manager msg 614
 * cleanup directive.
 *
 * pr-pipeline-sequence-cleanup PR-5 narrative note (2026-04-19): "Pipeline"
 * in the user-facing UI is implemented as a sequence-mode preset. When the
 * QuickLaunchBar Pipeline pill or the gear-icon Start Session modal launches
 * a pipeline, the Tauri start_discussion command writes mode="pipeline" to
 * discussion.json with auto_advance behavior controlled by
 * pipeline_ack_timeout_secs (default 300s, non-destructive notification).
 * The visual surface (this card + its children) is identical to sequence's
 * surface; the isPipelineMode prop only changes which Tauri command the
 * override-bar buttons dispatch to. There is no separate "sequence" feature
 * in the UI anymore (PR-4 deleted Start Sequence button + StartSequenceModal).
 */

interface Props {
  turn: SequenceTurnState | null | undefined;
  projectDir: string | null;
  availableRoleInstances: ModeratorSequencePanelRosterEntry[];
  /**
   * pr-pipeline-unified-controls (PR-3a, 2026-04-19): when the active discussion
   * is in pipeline mode, render the read-only visualization (banner + queue) but
   * hide the interactive children (HumanSequenceOverrideBar, PendingTurnRequests,
   * ModeratorSequencePanel) — those buttons invoke active_sequence-specific Tauri
   * commands that don't operate on pipeline state. PR-3b will rewire them per
   * architect msg 974's Option A. Defaults to false (sequence mode behavior).
   */
  isPipelineMode?: boolean;
}

export default function SequenceSessionCard({
  turn,
  projectDir,
  availableRoleInstances,
  isPipelineMode = false,
}: Props) {
  if (!turn) return null;

  return (
    <div className="sequence-session-card" aria-label="Active turn-sequence session">
      <SequenceBanner turn={turn} selfRoleInstance={null} />
      {/* PR-5: auto-advance UX safety indicator. Only visible in pipeline mode
          where stall behavior matters most — communicates whether silence will
          eventually skip the holder. Pipeline default is 300s non-destructive
          notification (per PR-2 PIPELINE_ACK_TIMEOUT_DEFAULT bump from 30s →
          300s). Sequence mode doesn't use auto-advance, so the indicator is
          omitted. */}
      {isPipelineMode && (
        <div
          className="sequence-auto-advance-indicator"
          role="status"
          aria-label="Auto-advance behavior"
          style={{
            padding: "4px 10px",
            fontSize: "11px",
            color: "#8899a6",
            background: "rgba(136, 153, 166, 0.08)",
            borderRadius: "4px",
            margin: "4px 0",
            display: "flex",
            alignItems: "center",
            gap: "6px",
          }}
        >
          <span style={{ color: "#1da1f2", fontWeight: 600 }}>Auto-advance:</span>
          <span>ON — silent holder advances after 300s (non-destructive notification first).</span>
        </div>
      )}
      <QueueVisualization turn={turn} />
      {/* PR-3b: HumanSequenceOverrideBar renders in BOTH sequence and pipeline
          modes. The mode prop routes button clicks to the right backend. */}
      <HumanSequenceOverrideBar turn={turn} projectDir={projectDir} isPipelineMode={isPipelineMode} />
      {/* PR-3b deferred: PendingTurnRequests + ModeratorSequencePanel still
          hidden in pipeline mode. PendingTurnRequests' backends are stubs;
          ModeratorSequencePanel needs ~6 pipeline equivalents (pause/resume/
          assign/reorder/insert/remove) which exceed the 250 LOC envelope.
          Future PR can wire those when the human asks for them. */}
      {!isPipelineMode && (
        <>
          <PendingTurnRequests turn={turn} projectDir={projectDir} />
          <ModeratorSequencePanel
            turn={turn}
            projectDir={projectDir}
            availableRoleInstances={availableRoleInstances}
          />
        </>
      )}
    </div>
  );
}
