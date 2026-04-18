import { getRoleColor } from "../utils/roleColors";
import type { SequenceTurnState } from "./SequenceBanner";

/**
 * Horizontal scroller beneath the SequenceBanner. Shows the full turn queue
 * as role-colored chips: completed (dimmed + strikethrough), current (pulse +
 * outline), upcoming (normal). One glance, everyone knows where they sit.
 *
 * PR-B per manager msg 396 / ux-engineer msg 382 plan. Reads the same
 * SequenceTurnState prop shape the banner uses — single source of truth in
 * the parent's sequence-state subscription.
 */

interface Props {
  turn: SequenceTurnState | null | undefined;
}

type ChipState = "done" | "current" | "upcoming";

interface QueueChip {
  id: string;
  role: string;
  instance: string;
  state: ChipState;
  position: number;
}

function splitRoleInstance(id: string): { role: string; instance: string } {
  const idx = id.indexOf(":");
  if (idx === -1) return { role: id, instance: "0" };
  return { role: id.slice(0, idx), instance: id.slice(idx + 1) };
}

export default function QueueVisualization({ turn }: Props) {
  if (!turn) return null;

  const completed = turn.queue_completed ?? [];
  const remaining = turn.queue_remaining ?? [];
  const current = turn.current_holder;

  const chips: QueueChip[] = [];
  let position = 0;

  for (const id of completed) {
    const { role, instance } = splitRoleInstance(id);
    chips.push({ id, role, instance, state: "done", position: ++position });
  }
  if (current) {
    const { role, instance } = splitRoleInstance(current);
    chips.push({ id: current, role, instance, state: "current", position: ++position });
  }
  for (const id of remaining) {
    const { role, instance } = splitRoleInstance(id);
    chips.push({ id, role, instance, state: "upcoming", position: ++position });
  }

  if (chips.length === 0) return null;

  return (
    <div
      className="queue-visualization"
      role="list"
      aria-label="Turn queue — completed, current, upcoming"
    >
      {chips.map((chip, i) => {
        const color = getRoleColor(chip.role);
        const isLast = i === chips.length - 1;
        return (
          <div key={`${chip.id}-${chip.position}`} className="queue-chip-wrapper">
            <div
              className={`queue-chip queue-chip-${chip.state}`}
              role="listitem"
              aria-label={`${chip.id} — ${chip.state === "done" ? "completed" : chip.state === "current" ? "on turn now" : `upcoming, position ${chip.position}`}`}
              style={
                chip.state === "current"
                  ? { borderColor: color, boxShadow: `0 0 0 1px ${color}` }
                  : undefined
              }
            >
              <span className="queue-chip-dot" style={{ background: color }} />
              <span className="queue-chip-label">
                <span className="queue-chip-role">{chip.role}</span>
                <span className="queue-chip-instance">:{chip.instance}</span>
              </span>
              {chip.state === "current" && (
                <span className="queue-chip-now-badge" style={{ color }}>
                  NOW
                </span>
              )}
            </div>
            {!isLast && (
              <span className="queue-chip-separator" aria-hidden="true">
                →
              </span>
            )}
          </div>
        );
      })}
    </div>
  );
}
