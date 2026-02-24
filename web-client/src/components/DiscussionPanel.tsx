/**
 * DiscussionPanel — UI for structured discussions (Delphi, Oxford, Continuous).
 * Matches desktop parity: start discussion, view rounds, submit responses, see aggregates.
 */

import { useState } from "react";
import { useUIStore } from "../lib/stores";
import * as api from "../lib/api";
import type { DiscussionResponse } from "../lib/api";

interface DiscussionPanelProps {
  projectId: string;
  discussion: DiscussionResponse | null;
  onRefresh: () => void;
}

const MODE_INFO: Record<string, { label: string; desc: string; icon: string }> = {
  delphi: { label: "Delphi", desc: "Blind submissions, anonymized aggregation, iterative rounds", icon: "\uD83C\uDFDB\uFE0F" },
  oxford: { label: "Oxford", desc: "Structured for/against debate with audience voting", icon: "\u2694\uFE0F" },
  red_team: { label: "Red Team", desc: "Adversarial challenge of proposals and assumptions", icon: "\uD83D\uDEE1\uFE0F" },
  continuous: { label: "Continuous Review", desc: "Auto-triggered micro-rounds, silence = consent", icon: "\uD83D\uDD04" },
};

export function DiscussionPanel({ projectId, discussion, onRefresh }: DiscussionPanelProps) {
  const addToast = useUIStore((s) => s.addToast);
  const [showStart, setShowStart] = useState(false);
  const [mode, setMode] = useState<"delphi" | "oxford" | "red_team" | "continuous">("delphi");
  const [topic, setTopic] = useState("");
  const [starting, setStarting] = useState(false);
  const [submitting, setSubmitting] = useState(false);
  const [submitText, setSubmitText] = useState("");
  const [actionLoading, setActionLoading] = useState(false);

  const handleStart = async () => {
    if (!topic.trim()) return;
    setStarting(true);
    try {
      await api.startDiscussion(projectId, mode, topic.trim());
      addToast(`${MODE_INFO[mode].label} discussion started`, "success");
      setShowStart(false);
      setTopic("");
      onRefresh();
    } catch (e) {
      addToast(e instanceof api.ApiError ? e.userMessage : "Failed to start discussion", "error");
    } finally {
      setStarting(false);
    }
  };

  const handleSubmit = async () => {
    if (!discussion || !submitText.trim()) return;
    setSubmitting(true);
    try {
      await api.submitToRound(projectId, discussion.id, submitText.trim());
      addToast("Submitted", "success");
      setSubmitText("");
      onRefresh();
    } catch (e) {
      addToast(e instanceof api.ApiError ? e.userMessage : "Failed to submit", "error");
    } finally {
      setSubmitting(false);
    }
  };

  const handleAction = async (action: "close-round" | "open-round" | "end") => {
    if (!discussion) return;
    setActionLoading(true);
    try {
      if (action === "close-round") await api.closeRound(projectId, discussion.id);
      else if (action === "open-round") await api.openNextRound(projectId, discussion.id);
      else await api.endDiscussion(projectId, discussion.id);
      addToast(action === "end" ? "Discussion ended" : `Round ${action === "close-round" ? "closed" : "opened"}`, "success");
      onRefresh();
    } catch (e) {
      addToast(e instanceof api.ApiError ? e.userMessage : "Action failed", "error");
    } finally {
      setActionLoading(false);
    }
  };

  // No active discussion — show start button
  if (!discussion) {
    return (
      <div style={{ marginBottom: "var(--space-4)" }}>
        <div style={{
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          marginBottom: "var(--space-3)",
        }}>
          <h3 style={{ fontSize: "var(--text-sm)", fontWeight: "var(--weight-semibold)", color: "var(--text-secondary)" }}>
            Discussions
          </h3>
          <button className="btn btn-secondary" style={{ fontSize: "var(--text-xs)" }} onClick={() => setShowStart(true)}>
            Start Discussion
          </button>
        </div>

        {showStart && (
          <div className="card" style={{ padding: "var(--space-3)" }}>
            <div className="field" style={{ marginBottom: "var(--space-3)" }}>
              <label className="field-label" htmlFor="disc-mode">Format</label>
              <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: "var(--space-2)" }}>
                {Object.entries(MODE_INFO).map(([key, info]) => (
                  <button
                    key={key}
                    className={`card ${mode === key ? "" : "card-hover"}`}
                    onClick={() => setMode(key as typeof mode)}
                    style={{
                      cursor: "pointer",
                      textAlign: "left",
                      padding: "var(--space-2)",
                      borderColor: mode === key ? "var(--accent)" : "var(--border)",
                      background: mode === key ? "var(--accent-muted)" : undefined,
                    }}
                    aria-pressed={mode === key}
                  >
                    <div style={{ fontSize: "var(--text-sm)" }}>{info.icon} {info.label}</div>
                    <div style={{ fontSize: "var(--text-xs)", color: "var(--text-muted)", marginTop: "2px" }}>{info.desc}</div>
                  </button>
                ))}
              </div>
            </div>

            <div className="field" style={{ marginBottom: "var(--space-3)" }}>
              <label className="field-label" htmlFor="disc-topic">Topic</label>
              <input
                id="disc-topic"
                className="input"
                value={topic}
                onChange={(e) => setTopic(e.target.value)}
                placeholder="What should the team discuss?"
              />
            </div>

            <div style={{ display: "flex", gap: "var(--space-2)" }}>
              <button className="btn btn-primary" onClick={handleStart} disabled={!topic.trim() || starting}>
                {starting ? "Starting..." : "Start"}
              </button>
              <button className="btn btn-ghost" onClick={() => setShowStart(false)}>Cancel</button>
            </div>
          </div>
        )}
      </div>
    );
  }

  // Active discussion
  const info = MODE_INFO[discussion.mode] || MODE_INFO.delphi;
  const isSubmitting = discussion.phase === "submitting";
  const latestRound = discussion.rounds[discussion.rounds.length - 1];

  return (
    <div className="card" style={{ marginBottom: "var(--space-4)", borderColor: "var(--accent)" }}>
      <div style={{
        display: "flex",
        alignItems: "center",
        justifyContent: "space-between",
        marginBottom: "var(--space-2)",
      }}>
        <div style={{ display: "flex", alignItems: "center", gap: "var(--space-2)" }}>
          <span>{info.icon}</span>
          <span style={{ fontWeight: "var(--weight-semibold)", fontSize: "var(--text-sm)" }}>
            {info.label} Discussion
          </span>
          <span className="badge badge-accent">{discussion.phase}</span>
          <span style={{ fontSize: "var(--text-xs)", color: "var(--text-muted)" }}>
            Round {discussion.current_round}
          </span>
        </div>
        <button
          className="btn btn-ghost"
          style={{ fontSize: "var(--text-xs)", color: "var(--error)" }}
          onClick={() => handleAction("end")}
          disabled={actionLoading}
          aria-label="End discussion"
        >
          End
        </button>
      </div>

      <div style={{ fontSize: "var(--text-sm)", color: "var(--text-secondary)", marginBottom: "var(--space-3)" }}>
        {discussion.topic}
      </div>

      {/* Round info */}
      {latestRound && (
        <div style={{
          fontSize: "var(--text-xs)",
          color: "var(--text-muted)",
          marginBottom: "var(--space-2)",
          padding: "var(--space-2)",
          background: "var(--bg-tertiary)",
          borderRadius: "var(--radius-sm)",
        }}>
          Round {latestRound.number}: {latestRound.submission_count} submissions
          {latestRound.aggregate && (
            <div style={{ marginTop: "var(--space-1)", color: "var(--text-secondary)", whiteSpace: "pre-wrap" }}>
              {typeof latestRound.aggregate === "string"
                ? latestRound.aggregate
                : JSON.stringify(latestRound.aggregate, null, 2)}
            </div>
          )}
        </div>
      )}

      {/* Continuous timeout selector */}
      {discussion.mode === "continuous" && (
        <div style={{ display: "flex", alignItems: "center", gap: "var(--space-2)", marginBottom: "var(--space-2)" }}>
          <label style={{ fontSize: "var(--text-xs)", color: "var(--text-muted)" }}>Timeout:</label>
          <select
            className="input"
            style={{ fontSize: "var(--text-xs)", padding: "2px var(--space-2)", width: 80 }}
            value={discussion.auto_close_timeout_seconds}
            onChange={async (e) => {
              try {
                await api.setDiscussionTimeout(projectId, discussion.id, parseInt(e.target.value));
                onRefresh();
              } catch (err) {
                addToast("Failed to set timeout", "error");
              }
            }}
            aria-label="Auto-close timeout"
          >
            <option value="30">30s</option>
            <option value="60">60s</option>
            <option value="120">2m</option>
            <option value="300">5m</option>
          </select>
        </div>
      )}

      {/* Submit response + close round (if submitting phase) */}
      {isSubmitting && (
        <>
          <div style={{ display: "flex", gap: "var(--space-2)" }}>
            <input
              className="input"
              value={submitText}
              onChange={(e) => setSubmitText(e.target.value)}
              placeholder="Your response..."
              onKeyDown={(e) => { if (e.key === "Enter") handleSubmit(); }}
              aria-label="Discussion response"
            />
            <button className="btn btn-primary" onClick={handleSubmit} disabled={!submitText.trim() || submitting}>
              {submitting ? "..." : "Submit"}
            </button>
          </div>
          {discussion.mode !== "continuous" && (
            <div style={{ marginTop: "var(--space-2)" }}>
              <button className="btn btn-ghost" style={{ fontSize: "var(--text-xs)" }} onClick={() => handleAction("close-round")} disabled={actionLoading}>
                Close Round
              </button>
            </div>
          )}
        </>
      )}

      {/* Open next round (reviewing/preparing phase) */}
      {(discussion.phase === "reviewing" || discussion.phase === "preparing") && (
        <div style={{ display: "flex", gap: "var(--space-2)", marginTop: "var(--space-2)" }}>
          <button className="btn btn-secondary" onClick={() => handleAction("open-round")} disabled={actionLoading}>
            Open Round {discussion.current_round + 1}
          </button>
        </div>
      )}
    </div>
  );
}
