import { useState, useEffect, useCallback } from "react";
import { getTranscriptHistory, TranscriptItem, isLoggedIn } from "../lib/api";

interface ClaudeOutputsViewerProps {
  onClose: () => void;
  refreshTrigger?: number;
}

interface ConversationGroup {
  id: string;
  sessionId: string | null;
  sessionLabel: string;
  startTime: Date;
  endTime: Date;
  outputs: TranscriptItem[];
}

export function ClaudeOutputsViewer({ onClose, refreshTrigger }: ClaudeOutputsViewerProps) {
  const [transcripts, setTranscripts] = useState<TranscriptItem[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [expandedGroup, setExpandedGroup] = useState<string | null>(null);
  const [expandedTranscript, setExpandedTranscript] = useState<number | null>(null);

  // Group outputs by session_id (each terminal gets its own session)
  const conversationGroups: ConversationGroup[] = (() => {
    const outputTranscripts = transcripts
      .filter((t) => t.transcript_type === "output")
      .sort((a, b) => new Date(b.created_at).getTime() - new Date(a.created_at).getTime());

    // Group by session_id
    const sessionMap = new Map<string, TranscriptItem[]>();

    for (const transcript of outputTranscripts) {
      const sessionKey = transcript.session_id || "unknown";
      if (!sessionMap.has(sessionKey)) {
        sessionMap.set(sessionKey, []);
      }
      sessionMap.get(sessionKey)!.push(transcript);
    }

    // Convert map to array of ConversationGroup objects
    const groups: ConversationGroup[] = [];
    let sessionIndex = 0;

    for (const [sessionId, outputs] of sessionMap.entries()) {
      const times = outputs.map((o) => new Date(o.created_at).getTime());
      const startTime = new Date(Math.min(...times));
      const endTime = new Date(Math.max(...times));

      // Generate a readable session label
      let sessionLabel: string;
      if (sessionId === "unknown") {
        sessionLabel = "Unknown Session";
      } else {
        // Extract hostname and terminal ID from session_id (format: hostname-ppid)
        const parts = sessionId.split("-");
        if (parts.length >= 2) {
          const hostname = parts[0];
          const terminalId = parts.slice(1).join("-");
          sessionLabel = `${hostname} • Terminal ${terminalId}`;
        } else {
          sessionLabel = `Session ${sessionIndex + 1}`;
        }
      }

      groups.push({
        id: `session-${sessionId}`,
        sessionId: sessionId === "unknown" ? null : sessionId,
        sessionLabel,
        startTime,
        endTime,
        outputs,
      });

      sessionIndex++;
    }

    // Sort by most recent activity
    return groups.sort((a, b) => b.endTime.getTime() - a.endTime.getTime());
  })();

  const loadData = useCallback(async () => {
    if (!isLoggedIn()) {
      setError("Please log in to view Claude outputs");
      setLoading(false);
      return;
    }

    setLoading(true);
    setError(null);

    try {
      const data = await getTranscriptHistory(0, 100);
      setTranscripts(data);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to load outputs");
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    loadData();
  }, [loadData, refreshTrigger]);

  const handleCopyTranscript = async (text: string) => {
    await navigator.clipboard.writeText(text);
  };

  const handleCopyConversation = async (group: ConversationGroup) => {
    const text = group.outputs
      .map((t, i) => `${i + 1}. ${t.polished_text}`)
      .join("\n\n");
    await navigator.clipboard.writeText(text);
  };

  const formatDate = (dateString: string): string => {
    const date = new Date(dateString);
    return date.toLocaleString("en-US", {
      month: "short",
      day: "numeric",
      hour: "numeric",
      minute: "2-digit",
      hour12: true,
    });
  };

  const formatDuration = (start: Date, end: Date): string => {
    const diff = end.getTime() - start.getTime();
    const minutes = Math.floor(diff / (60 * 1000));
    if (minutes < 1) return "< 1 min";
    if (minutes === 1) return "1 min";
    if (minutes < 60) return `${minutes} mins`;
    const hours = Math.floor(minutes / 60);
    const remainingMins = minutes % 60;
    if (remainingMins === 0) return `${hours}h`;
    return `${hours}h ${remainingMins}m`;
  };

  return (
    <div className="history-overlay" onClick={onClose}>
      <div className="claude-outputs-panel" onClick={(e) => e.stopPropagation()}>
        <div className="history-header">
          <h2>💬 Claude Conversations</h2>
          <button className="close-btn" onClick={onClose}>
            ×
          </button>
        </div>

        {loading && <div className="loading">Loading conversations...</div>}
        {error && <div className="error-message">{error}</div>}

        {!loading && !error && conversationGroups.length === 0 && (
          <div className="empty-state">
            <p>No Claude conversations yet</p>
            <p className="hint">Claude's spoken responses will appear here</p>
          </div>
        )}

        {!loading && !error && conversationGroups.length > 0 && (
          <div className="conversations-list">
            {conversationGroups.map((group) => (
              <div key={group.id} className="conversation-group">
                <div
                  className="conversation-header"
                  onClick={() =>
                    setExpandedGroup(expandedGroup === group.id ? null : group.id)
                  }
                >
                  <div className="conversation-info">
                    <span className="conversation-title">
                      {group.sessionLabel}
                    </span>
                    <span className="conversation-time">
                      Last active: {formatDate(group.endTime.toISOString())}
                    </span>
                    <span className="conversation-stats">
                      {group.outputs.length} message{group.outputs.length !== 1 ? "s" : ""}
                      {" • "}
                      {formatDuration(group.startTime, group.endTime)}
                    </span>
                  </div>
                  <div className="conversation-actions">
                    <button
                      className="action-btn"
                      onClick={(e) => {
                        e.stopPropagation();
                        handleCopyConversation(group);
                      }}
                      title="Copy entire conversation"
                    >
                      📋
                    </button>
                    <span className="expand-icon">
                      {expandedGroup === group.id ? "▼" : "▶"}
                    </span>
                  </div>
                </div>

                {expandedGroup === group.id && (
                  <div className="conversation-messages">
                    {group.outputs.map((transcript, index) => (
                      <div key={transcript.id} className="output-message">
                        <div className="message-header">
                          <span className="message-number">#{index + 1}</span>
                          <span className="message-time">
                            {formatDate(transcript.created_at)}
                          </span>
                          <button
                            className="action-btn"
                            onClick={() => handleCopyTranscript(transcript.polished_text)}
                            title="Copy message"
                          >
                            📋
                          </button>
                        </div>
                        <div
                          className={`message-text ${
                            expandedTranscript === transcript.id ? "expanded" : ""
                          }`}
                          onClick={() =>
                            setExpandedTranscript(
                              expandedTranscript === transcript.id ? null : transcript.id
                            )
                          }
                        >
                          {transcript.polished_text}
                        </div>
                        <div className="message-meta">
                          {transcript.word_count} words
                        </div>
                      </div>
                    ))}
                  </div>
                )}
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
