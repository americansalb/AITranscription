import { useEffect, useRef, useState } from "react";
import { getRoleColor } from "../utils/roleColors";

/**
 * Modal for human-initiated `start_sequence`. Mounts when opened from the
 * "Start Sequence" button in CollabTab header. Collects topic + optional goal +
 * ordered participant list, then invokes the existing MCP-style discussion_control
 * command via Tauri. Hidden when closed.
 *
 * Human-actor UX per feedback_dont_overgate_moderator_ux: no reason required,
 * single-button confirm via Start.
 */

export interface StartSequenceCandidate {
  id: string;
  title: string;
}

interface Props {
  open: boolean;
  onClose: () => void;
  projectDir: string | null;
  candidates: StartSequenceCandidate[];
}

export default function StartSequenceModal({ open, onClose, projectDir, candidates }: Props) {
  const [topic, setTopic] = useState("");
  const [goal, setGoal] = useState("");
  const [selected, setSelected] = useState<string[]>([]);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const topicRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (open) {
      setTopic("");
      setGoal("");
      setSelected([]);
      setSubmitting(false);
      setError(null);
      requestAnimationFrame(() => topicRef.current?.focus());
    }
  }, [open]);

  if (!open) return null;

  const toggleParticipant = (id: string) => {
    setSelected((prev) =>
      prev.includes(id) ? prev.filter((p) => p !== id) : [...prev, id],
    );
  };

  const moveUp = (index: number) => {
    if (index <= 0) return;
    setSelected((prev) => {
      const next = [...prev];
      [next[index - 1], next[index]] = [next[index], next[index - 1]];
      return next;
    });
  };

  const moveDown = (index: number) => {
    setSelected((prev) => {
      if (index >= prev.length - 1) return prev;
      const next = [...prev];
      [next[index], next[index + 1]] = [next[index + 1], next[index]];
      return next;
    });
  };

  const canSubmit = topic.trim().length > 0 && selected.length > 0 && !submitting;

  const handleSubmit = async () => {
    if (!canSubmit || !projectDir || !window.__TAURI__) return;
    setError(null);
    setSubmitting(true);
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      await invoke("discussion_control", {
        dir: projectDir,
        action: "start_sequence",
        topic: topic.trim(),
        goal: goal.trim() || null,
        participants: selected,
      });
      onClose();
    } catch (e) {
      const msg = typeof e === "string" ? e : (e as Error).message ?? String(e);
      setError(msg);
      setSubmitting(false);
    }
  };

  const handleKey = (e: React.KeyboardEvent<HTMLDivElement>) => {
    if (e.key === "Escape") {
      e.preventDefault();
      onClose();
    }
  };

  const unselected = candidates.filter((c) => !selected.includes(c.id));

  return (
    <div
      className="start-sequence-modal-backdrop"
      role="dialog"
      aria-modal="true"
      aria-labelledby="start-sequence-modal-title"
      onClick={onClose}
      onKeyDown={handleKey}
    >
      <div
        className="start-sequence-modal"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="start-sequence-modal-header">
          <h3 id="start-sequence-modal-title" className="start-sequence-modal-title">
            Start Sequence
          </h3>
          <button
            type="button"
            className="start-sequence-modal-close"
            onClick={onClose}
            aria-label="Close"
          >
            ×
          </button>
        </div>

        <div className="start-sequence-modal-body">
          <label className="start-sequence-field">
            <span className="start-sequence-label">Topic</span>
            <input
              ref={topicRef}
              className="start-sequence-input"
              type="text"
              value={topic}
              onChange={(e) => setTopic(e.target.value)}
              placeholder="What is this sequence about?"
              disabled={submitting}
            />
          </label>

          <label className="start-sequence-field">
            <span className="start-sequence-label">
              Goal <span className="start-sequence-label-hint">(optional)</span>
            </span>
            <textarea
              className="start-sequence-textarea"
              value={goal}
              onChange={(e) => setGoal(e.target.value)}
              placeholder="What outcome are you aiming for?"
              rows={2}
              disabled={submitting}
            />
          </label>

          <div className="start-sequence-field">
            <span className="start-sequence-label">
              Participants <span className="start-sequence-label-hint">(in order of turn)</span>
            </span>
            {selected.length > 0 && (
              <ul className="start-sequence-selected" role="list">
                {selected.map((id, index) => {
                  const candidate = candidates.find((c) => c.id === id);
                  const [role] = id.split(":");
                  return (
                    <li key={id} className="start-sequence-selected-item">
                      <span className="start-sequence-pos">#{index + 1}</span>
                      <span
                        className="start-sequence-dot"
                        style={{ background: getRoleColor(role) }}
                      />
                      <span className="start-sequence-selected-label">
                        <span className="start-sequence-selected-title">
                          {candidate?.title ?? role}
                        </span>
                        <span className="start-sequence-selected-id">{id}</span>
                      </span>
                      <span className="start-sequence-item-actions">
                        <button
                          type="button"
                          className="start-sequence-icon-btn"
                          onClick={() => moveUp(index)}
                          disabled={submitting || index === 0}
                          aria-label={`Move ${id} up`}
                          title="Move up"
                        >
                          ↑
                        </button>
                        <button
                          type="button"
                          className="start-sequence-icon-btn"
                          onClick={() => moveDown(index)}
                          disabled={submitting || index === selected.length - 1}
                          aria-label={`Move ${id} down`}
                          title="Move down"
                        >
                          ↓
                        </button>
                        <button
                          type="button"
                          className="start-sequence-icon-btn start-sequence-icon-danger"
                          onClick={() => toggleParticipant(id)}
                          disabled={submitting}
                          aria-label={`Remove ${id}`}
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
            {unselected.length > 0 ? (
              <div className="start-sequence-available">
                <div className="start-sequence-available-title">Add participant</div>
                <div className="start-sequence-available-list" role="list">
                  {unselected.map((c) => {
                    const [role] = c.id.split(":");
                    return (
                      <button
                        key={c.id}
                        type="button"
                        className="start-sequence-available-chip"
                        onClick={() => toggleParticipant(c.id)}
                        disabled={submitting}
                      >
                        <span
                          className="start-sequence-dot"
                          style={{ background: getRoleColor(role) }}
                        />
                        <span className="start-sequence-available-title-text">
                          {c.title}
                        </span>
                        <span className="start-sequence-available-id">{c.id}</span>
                      </button>
                    );
                  })}
                </div>
              </div>
            ) : (
              selected.length === 0 && (
                <div className="start-sequence-empty">
                  No active roles available. Launch at least one team member first.
                </div>
              )
            )}
          </div>

          {error && (
            <div className="start-sequence-error" role="alert">
              {error}
            </div>
          )}
        </div>

        <div className="start-sequence-modal-footer">
          <button
            type="button"
            className="start-sequence-btn-subtle"
            onClick={onClose}
            disabled={submitting}
          >
            Cancel
          </button>
          <button
            type="button"
            className="start-sequence-btn-primary"
            onClick={handleSubmit}
            disabled={!canSubmit}
            aria-label={`Start sequence with ${selected.length} participant${selected.length === 1 ? "" : "s"}`}
          >
            {submitting ? "Starting…" : `Start (${selected.length})`}
          </button>
        </div>
      </div>
    </div>
  );
}
