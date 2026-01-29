import { useState, useEffect, useCallback } from "react";
import { getTranscriptHistory, TranscriptItem, isLoggedIn } from "../lib/api";

interface TranscriptHistoryProps {
  onClose: () => void;
  refreshTrigger?: number;
}

export function TranscriptHistory({ onClose, refreshTrigger }: TranscriptHistoryProps) {
  const [transcripts, setTranscripts] = useState<TranscriptItem[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [expandedTranscript, setExpandedTranscript] = useState<number | null>(null);
  const [filter, setFilter] = useState<"all" | "input" | "output">("all");

  const filteredTranscripts = transcripts.filter((t) => {
    if (filter === "all") return true;
    return t.transcript_type === filter;
  });

  const loadData = useCallback(async () => {
    if (!isLoggedIn()) {
      setError("Please log in to view history");
      setLoading(false);
      return;
    }

    setLoading(true);
    setError(null);

    try {
      const data = await getTranscriptHistory(0, 100);
      setTranscripts(data);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to load history");
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

  const formatDate = (dateString: string): string => {
    const date = new Date(dateString);
    const now = new Date();
    const diffMs = now.getTime() - date.getTime();
    const diffMins = Math.floor(diffMs / 60000);
    const diffHours = Math.floor(diffMs / 3600000);
    const diffDays = Math.floor(diffMs / 86400000);

    if (diffMins < 1) return "Just now";
    if (diffMins < 60) return `${diffMins}m ago`;
    if (diffHours < 24) return `${diffHours}h ago`;
    if (diffDays < 7) return `${diffDays}d ago`;

    return date.toLocaleDateString();
  };

  return (
    <div className="history-overlay" onClick={onClose}>
      <div className="history-panel" onClick={(e) => e.stopPropagation()}>
        <div className="history-header">
          <h2>Transcript History</h2>
          <div className="history-filters">
            <button
              className={`filter-btn ${filter === "all" ? "active" : ""}`}
              onClick={() => setFilter("all")}
            >
              All
            </button>
            <button
              className={`filter-btn ${filter === "input" ? "active" : ""}`}
              onClick={() => setFilter("input")}
            >
              🎤 Inputs
            </button>
            <button
              className={`filter-btn ${filter === "output" ? "active" : ""}`}
              onClick={() => setFilter("output")}
            >
              💬 Claude
            </button>
          </div>
          <button className="close-btn" onClick={onClose}>
            <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              <path d="M18 6L6 18M6 6l12 12" />
            </svg>
          </button>
        </div>

        <div className="history-content">
          {loading ? (
            <div className="history-loading">
              <div className="spinner" />
              <p>Loading history...</p>
            </div>
          ) : error ? (
            <div className="history-error">
              <p>{error}</p>
              <button onClick={loadData}>Retry</button>
            </div>
          ) : transcripts.length === 0 ? (
            <div className="empty-state">
              <p>No transcriptions yet</p>
              <p className="empty-hint">Start recording to see your history here</p>
            </div>
          ) : (
            <div className="transcript-list">
              {filteredTranscripts.map((t) => (
                <div
                  key={t.id}
                  className={`transcript-item ${t.transcript_type} ${expandedTranscript === t.id ? "expanded" : ""}`}
                >
                  <div
                    className="transcript-header"
                    onClick={() => setExpandedTranscript(expandedTranscript === t.id ? null : t.id)}
                  >
                    <div className="transcript-type-icon">
                      {t.transcript_type === "input" ? "🎤" : "💬"}
                    </div>
                    <div className="transcript-preview">
                      {t.polished_text.slice(0, 80)}
                      {t.polished_text.length > 80 ? "..." : ""}
                    </div>
                    <div className="transcript-meta">
                      <span className="transcript-time">{formatDate(t.created_at)}</span>
                      <span className="transcript-words">{t.word_count} words</span>
                      {t.transcript_type === "input" && t.words_per_minute > 0 && (
                        <span className="transcript-wpm">{t.words_per_minute} WPM</span>
                      )}
                    </div>
                  </div>
                  {expandedTranscript === t.id && (
                    <div className="transcript-expanded">
                      <div className="transcript-full-text">{t.polished_text}</div>
                      <div className="transcript-actions">
                        <button onClick={() => handleCopyTranscript(t.polished_text)}>
                          Copy
                        </button>
                        {t.context && (
                          <span className="transcript-context">{t.context}</span>
                        )}
                      </div>
                    </div>
                  )}
                </div>
              ))}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
