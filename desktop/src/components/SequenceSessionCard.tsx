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
 */

interface Props {
  turn: SequenceTurnState | null | undefined;
  projectDir: string | null;
  availableRoleInstances: ModeratorSequencePanelRosterEntry[];
}

export default function SequenceSessionCard({
  turn,
  projectDir,
  availableRoleInstances,
}: Props) {
  if (!turn) return null;

  return (
    <div className="sequence-session-card" aria-label="Active turn-sequence session">
      <SequenceBanner turn={turn} selfRoleInstance={null} />
      <QueueVisualization turn={turn} />
      <HumanSequenceOverrideBar turn={turn} projectDir={projectDir} />
      <PendingTurnRequests turn={turn} projectDir={projectDir} />
      <ModeratorSequencePanel
        turn={turn}
        projectDir={projectDir}
        availableRoleInstances={availableRoleInstances}
      />
    </div>
  );
}
