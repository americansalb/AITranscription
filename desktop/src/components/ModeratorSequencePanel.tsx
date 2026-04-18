import { useState } from "react";
import { getRoleColor } from "../utils/roleColors";
import type { SequenceTurnState } from "./SequenceBanner";

/**
 * Collapsible control surface for manipulating an active sequence.
 * Visible to the human (CollabTab is the human's view) per manager msg 398;
 * moderator-side operator UI lives in the hook-injected prompt layer, so this
 * panel is the HUMAN operator's sequence-manipulation interface.
 *
 * Controls:
 *  - Per-queue-item: move-up, move-down, remove. Keyboard-accessible
 *    (buttons with aria-labels), no drag-and-drop complexity.
 *  - Insert-role dropdown at bottom — picks from the project roster.
 *  - Pause/resume toggle for the whole sequence.
 *  - Skip current — force-advance without waiting for end_of_turn tag.
 *
 * Hidden when no active sequence. Collapsed by default when sequence IS
 * active so the banner + queue stay the primary surface; user expands to
 * manipulate.
 */

export interface ModeratorSequencePanelRosterEntry {
  id: string;
  title: string;
}

interface Props {
  turn: SequenceTurnState | null | undefined;
  projectDir: string | null;
  availableRoleInstances: ModeratorSequencePanelRosterEntry[];
}

type BusyAction = null | "pause" | "resume" | "skip" | "reorder" | "insert" | "remove";

function splitRoleInstance(id: string): { role: string; instance: string } {
  const idx = id.indexOf(":");
  if (idx === -1) return { role: id, instance: "0" };
  return { role: id.slice(0, idx), instance: id.slice(idx + 1) };
}

export default function ModeratorSequencePanel({ turn, projectDir, availableRoleInstances }: Props) {
  const [expanded, setExpanded] = useState(false);
  const [insertChoice, setInsertChoice] = useState("");
  const [busy, setBusy] = useState<BusyAction>(null);

  if (!turn || !projectDir) return null;

  const remaining = turn.queue_remaining ?? [];
  const paused = turn.paused_for_human === true;

  const invokeCommand = async (command: string, args: Record<string, unknown>, action: BusyAction) => {
    if (!window.__TAURI__) return;
    setBusy(action);
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      await invoke(command, { projectDir, ...args });
    } catch (e) {
      console.error(`[ModeratorSequencePanel] ${command} failed:`, e);
    } finally {
      setBusy(null);
    }
  };

  const moveUp = (index: number) => {
    if (index <= 0) return;
    const next = [...remaining];
    [next[index - 1], next[index]] = [next[index], next[index - 1]];
    void invokeCommand("reorder_queue", { newOrder: next }, "reorder");
  };

  const moveDown = (index: number) => {
    if (index >= remaining.length - 1) return;
    const next = [...remaining];
    [next[index], next[index + 1]] = [next[index + 1], next[index]];
    void invokeCommand("reorder_queue", { newOrder: next }, "reorder");
  };

  const removeAt = (id: string) => {
    void invokeCommand("remove_role_from_queue", { roleInstance: id }, "remove");
  };

  const insertAtEnd = () => {
    if (!insertChoice) return;
    void invokeCommand(
      "insert_role_in_queue",
      { roleInstance: insertChoice, position: remaining.length },
      "insert",
    );
    setInsertChoice("");
  };

  const togglePause = () => {
    if (paused) {
      void invokeCommand("resume_sequence", {}, "resume");
    } else {
      void invokeCommand("pause_sequence", {}, "pause");
    }
  };

  const skipCurrent = () => {
    void invokeCommand("skip_current_turn", {}, "skip");
  };

  const insertCandidates = availableRoleInstances.filter(
    (entry) =>
      !remaining.includes(entry.id) &&
      entry.id !== turn.current_holder &&
      !(turn.queue_completed ?? []).includes(entry.id),
  );

  return (
    <div className="sequence-mod-panel">
      <button
        type="button"
        className="sequence-mod-panel-toggle"
        onClick={() => setExpanded(!expanded)}
        aria-expanded={expanded}
        aria-controls="sequence-mod-panel-body"
      >
        <span className="sequence-mod-panel-chevron" aria-hidden="true">
          {expanded ? "▼" : "▶"}
        </span>
        <span>Moderator controls</span>
        <span className="sequence-mod-panel-summary">
          {remaining.length} upcoming{paused ? " · paused" : ""}
        </span>
      </button>
      {expanded && (
        <div id="sequence-mod-panel-body" className="sequence-mod-panel-body">
          <div className="sequence-mod-section-controls">
            <button
              type="button"
              className="sequence-mod-action-btn"
              onClick={togglePause}
              disabled={busy !== null}
              aria-label={paused ? "Resume the sequence" : "Pause the sequence"}
            >
              {paused ? "Resume" : "Pause"}
            </button>
            <button
              type="button"
              className="sequence-mod-action-btn sequence-mod-action-warn"
              onClick={skipCurrent}
              disabled={busy !== null || !turn.current_holder}
              aria-label={`Force-advance past the current turn-holder ${turn.current_holder ?? ""}`}
            >
              Skip current
            </button>
          </div>

          <div className="sequence-mod-section">
            <div className="sequence-mod-section-title">Upcoming</div>
            {remaining.length === 0 ? (
              <div className="sequence-mod-empty">Queue is empty.</div>
            ) : (
              <ul className="sequence-mod-queue-list" role="list">
                {remaining.map((id, index) => {
                  const { role, instance } = splitRoleInstance(id);
                  const color = getRoleColor(role);
                  return (
                    <li key={id} className="sequence-mod-queue-item">
                      <span className="sequence-mod-pos">#{index + 1}</span>
                      <span className="sequence-mod-dot" style={{ background: color }} />
                      <span className="sequence-mod-label">
                        <span className="sequence-mod-role">{role}</span>
                        <span className="sequence-mod-instance">:{instance}</span>
                      </span>
                      <span className="sequence-mod-item-controls">
                        <button
                          type="button"
                          className="sequence-mod-icon-btn"
                          onClick={() => moveUp(index)}
                          disabled={busy !== null || index === 0}
                          aria-label={`Move ${id} up`}
                          title="Move up"
                        >
                          ↑
                        </button>
                        <button
                          type="button"
                          className="sequence-mod-icon-btn"
                          onClick={() => moveDown(index)}
                          disabled={busy !== null || index === remaining.length - 1}
                          aria-label={`Move ${id} down`}
                          title="Move down"
                        >
                          ↓
                        </button>
                        <button
                          type="button"
                          className="sequence-mod-icon-btn sequence-mod-icon-danger"
                          onClick={() => removeAt(id)}
                          disabled={busy !== null}
                          aria-label={`Remove ${id} from queue`}
                          title="Remove"
                        >
                          ×
                        </button>
                      </span>
                    </li>
                  );
                })}
              </ul>
            )}
          </div>

          <div className="sequence-mod-section sequence-mod-insert-section">
            <div className="sequence-mod-section-title">Add role to queue</div>
            <div className="sequence-mod-insert-row">
              <select
                className="sequence-mod-select"
                value={insertChoice}
                onChange={(e) => setInsertChoice(e.target.value)}
                disabled={busy !== null || insertCandidates.length === 0}
                aria-label="Role to insert at end of queue"
              >
                <option value="">
                  {insertCandidates.length === 0 ? "No eligible roles" : "Select a role…"}
                </option>
                {insertCandidates.map((entry) => (
                  <option key={entry.id} value={entry.id}>
                    {entry.title} ({entry.id})
                  </option>
                ))}
              </select>
              <button
                type="button"
                className="sequence-mod-action-btn"
                onClick={insertAtEnd}
                disabled={busy !== null || !insertChoice}
              >
                Add to end
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
