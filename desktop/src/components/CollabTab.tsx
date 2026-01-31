import { useEffect, useState } from "react";
import type { CollabState } from "../lib/collabTypes";
import "../styles/collab.css";

export function CollabTab() {
  const [collabs, setCollabs] = useState<CollabState[]>([]);
  const [selectedCollab, setSelectedCollab] = useState<string | null>(null);

  // Listen for collab events from backend
  useEffect(() => {
    if (!window.__TAURI__) return;

    let unlistenMessage: (() => void) | undefined;
    let unlistenUpdate: (() => void) | undefined;

    const setup = async () => {
      try {
        const { listen } = await import("@tauri-apps/api/event");

        unlistenMessage = await listen<{ collaboration: CollabState }>(
          "collab-message",
          (event) => {
            const updated = event.payload.collaboration;
            setCollabs((prev) => {
              const idx = prev.findIndex(
                (c) => c.collab_id === updated.collab_id
              );
              if (idx >= 0) {
                const next = [...prev];
                next[idx] = updated;
                return next;
              }
              return [...prev, updated];
            });
          }
        );

        unlistenUpdate = await listen<{ collaborations: CollabState[] }>(
          "collab-update",
          (event) => {
            setCollabs(event.payload.collaborations);
          }
        );
      } catch (err) {
        console.error("[CollabTab] Failed to setup listeners:", err);
      }
    };

    setup();

    return () => {
      unlistenMessage?.();
      unlistenUpdate?.();
    };
  }, []);

  const active = selectedCollab
    ? collabs.find((c) => c.collab_id === selectedCollab)
    : collabs[0] || null;

  const endCollaboration = async () => {
    if (!active) return;
    // Could call a Tauri command; for now just clear local state
    setCollabs((prev) =>
      prev.filter((c) => c.collab_id !== active.collab_id)
    );
    setSelectedCollab(null);
  };

  if (collabs.length === 0) {
    return (
      <div className="collab-tab">
        <div className="collab-empty">
          <div className="collab-empty-icon">🤝</div>
          <div className="collab-empty-title">No Active Collaborations</div>
          <div className="collab-empty-hint">
            Start a collaboration by telling two Claude Code sessions to join
            with <code>collab_join</code>. Use the same project directory for
            both.
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="collab-tab">
      {/* Status Banner */}
      <div className="collab-banner">
        <div className="collab-banner-status">
          <span className="collab-status-dot active" />
          <span>Active Collaboration</span>
        </div>
        {active && (
          <div className="collab-banner-info">
            <span className="collab-id">ID: {active.collab_id}</span>
            <span className="collab-participants">
              {active.participants.map((p) => (
                <span
                  key={p.session_id}
                  className={`collab-role collab-role-${p.role.toLowerCase()}`}
                >
                  {p.role}
                </span>
              ))}
            </span>
          </div>
        )}
        <button className="collab-end-btn" onClick={endCollaboration}>
          End Collaboration
        </button>
      </div>

      {/* File path */}
      {active && (
        <div className="collab-filepath">
          <span className="collab-filepath-label">File:</span>
          <code>{active.project_dir}/.vaak/collab.md</code>
        </div>
      )}

      {/* If multiple collabs, show selector */}
      {collabs.length > 1 && (
        <div className="collab-selector">
          {collabs.map((c) => (
            <button
              key={c.collab_id}
              className={`collab-selector-btn ${
                active?.collab_id === c.collab_id ? "active" : ""
              }`}
              onClick={() => setSelectedCollab(c.collab_id)}
            >
              {c.collab_id}
            </button>
          ))}
        </div>
      )}

      {/* Message Feed */}
      <div className="collab-messages">
        {active && active.messages.length === 0 ? (
          <div className="collab-messages-empty">
            Waiting for messages...
          </div>
        ) : (
          active?.messages.map((msg) => (
            <div
              key={msg.number}
              className={`collab-msg collab-msg-${msg.role.toLowerCase()}`}
            >
              <div className="collab-msg-header">
                <span className="collab-msg-number">#{msg.number}</span>
                <span
                  className={`collab-msg-role collab-role-${msg.role.toLowerCase()}`}
                >
                  {msg.role}
                </span>
                <span className="collab-msg-time">{msg.timestamp}</span>
              </div>
              <div className="collab-msg-text">{msg.text}</div>
            </div>
          ))
        )}
      </div>
    </div>
  );
}
