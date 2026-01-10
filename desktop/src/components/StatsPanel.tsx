import { useState, useEffect, useCallback } from "react";
import {
  getUserStats,
  getTranscriptHistory,
  getUserAchievements,
  updateTypingWpm,
  UserStats,
  TranscriptItem,
  AchievementItem,
  AchievementsResponse,
  isLoggedIn,
} from "../lib/api";

interface StatsPanelProps {
  onClose: () => void;
  refreshTrigger?: number; // Increment to trigger data refresh
}

function formatDuration(seconds: number): string {
  const hours = Math.floor(seconds / 3600);
  const mins = Math.floor((seconds % 3600) / 60);
  const secs = Math.floor(seconds % 60);

  if (hours > 0) {
    return `${hours}h ${mins}m`;
  }
  if (mins > 0) {
    return `${mins}m ${secs}s`;
  }
  return `${secs}s`;
}

function formatDate(dateString: string): string {
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
}

// Icon mapping for achievements (simple SVG icons)
const achievementIcons: Record<string, JSX.Element> = {
  mic: <svg width="24" height="24" viewBox="0 0 24 24" fill="currentColor"><path d="M12 14c1.66 0 3-1.34 3-3V5c0-1.66-1.34-3-3-3S9 3.34 9 5v6c0 1.66 1.34 3 3 3zm-1-9c0-.55.45-1 1-1s1 .45 1 1v6c0 .55-.45 1-1 1s-1-.45-1-1V5z"/><path d="M17 11c0 2.76-2.24 5-5 5s-5-2.24-5-5H5c0 3.53 2.61 6.43 6 6.92V21h2v-3.08c3.39-.49 6-3.39 6-6.92h-2z"/></svg>,
  trending_up: <svg width="24" height="24" viewBox="0 0 24 24" fill="currentColor"><path d="M16 6l2.29 2.29-4.88 4.88-4-4L2 16.59 3.41 18l6-6 4 4 6.3-6.29L22 12V6h-6z"/></svg>,
  star: <svg width="24" height="24" viewBox="0 0 24 24" fill="currentColor"><path d="M12 17.27L18.18 21l-1.64-7.03L22 9.24l-7.19-.61L12 2 9.19 8.63 2 9.24l5.46 4.73L5.82 21 12 17.27z"/></svg>,
  bolt: <svg width="24" height="24" viewBox="0 0 24 24" fill="currentColor"><path d="M11 21h-1l1-7H7.5c-.58 0-.57-.32-.38-.66.19-.34.05-.08.07-.12C8.48 10.94 10.42 7.54 13 3h1l-1 7h3.5c.49 0 .56.33.47.51l-.07.15C12.96 17.55 11 21 11 21z"/></svg>,
  workspace_premium: <svg width="24" height="24" viewBox="0 0 24 24" fill="currentColor"><path d="M9.68 13.69L12 11.93l2.31 1.76-.88-2.85L15.75 9h-2.84L12 6.19 11.09 9H8.25l2.31 1.84-.88 2.85zM20 10c0-4.42-3.58-8-8-8s-8 3.58-8 8c0 2.03.76 3.87 2 5.28V23l6-2 6 2v-7.72c1.24-1.41 2-3.25 2-5.28zm-8-6c3.31 0 6 2.69 6 6s-2.69 6-6 6-6-2.69-6-6 2.69-6 6-6z"/></svg>,
  emoji_events: <svg width="24" height="24" viewBox="0 0 24 24" fill="currentColor"><path d="M19 5h-2V3H7v2H5c-1.1 0-2 .9-2 2v1c0 2.55 1.92 4.63 4.39 4.94.63 1.5 1.98 2.63 3.61 2.96V19H7v2h10v-2h-4v-3.1c1.63-.33 2.98-1.46 3.61-2.96C19.08 12.63 21 10.55 21 8V7c0-1.1-.9-2-2-2zM5 8V7h2v3.82C5.84 10.4 5 9.3 5 8zm14 0c0 1.3-.84 2.4-2 2.82V7h2v1z"/></svg>,
  edit_note: <svg width="24" height="24" viewBox="0 0 24 24" fill="currentColor"><path d="M3 10h11v2H3v-2zm0-2h11V6H3v2zm0 8h7v-2H3v2zm15.01-3.13l.71-.71c.39-.39 1.02-.39 1.41 0l.71.71c.39.39.39 1.02 0 1.41l-.71.71-2.12-2.12zm-.71.71l-5.3 5.3V21h2.12l5.3-5.3-2.12-2.12z"/></svg>,
  description: <svg width="24" height="24" viewBox="0 0 24 24" fill="currentColor"><path d="M14 2H6c-1.1 0-1.99.9-1.99 2L4 20c0 1.1.89 2 1.99 2H18c1.1 0 2-.9 2-2V8l-6-6zm2 16H8v-2h8v2zm0-4H8v-2h8v2zm-3-5V3.5L18.5 9H13z"/></svg>,
  auto_stories: <svg width="24" height="24" viewBox="0 0 24 24" fill="currentColor"><path d="M19 1l-5 5v11l5-4.5V1zM1 6v14.65c0 .25.25.5.5.5.1 0 .15-.05.25-.05C3.1 20.45 5.05 20 6.5 20c1.95 0 4.05.4 5.5 1.5V6c-1.45-1.1-3.55-1.5-5.5-1.5S2.45 4.9 1 6zm22 13.5V6c-.6-.45-1.25-.75-2-1v13.5c-1.1-.35-2.3-.5-3.5-.5-1.7 0-4.15.65-5.5 1.5v2c1.35-.85 3.8-1.5 5.5-1.5 1.65 0 3.35.3 4.75 1.05.1.05.15.05.25.05.25 0 .5-.25.5-.5z"/></svg>,
  military_tech: <svg width="24" height="24" viewBox="0 0 24 24" fill="currentColor"><path d="M17 10.43V2H7v8.43c0 .35.18.68.49.86l4.18 2.51-.99 2.34-3.41.29 2.59 2.24L9.07 22 12 20.23 14.93 22l-.79-3.33 2.59-2.24-3.41-.29-.99-2.34 4.18-2.51c.31-.18.49-.51.49-.86zm-4 1.8l-1 .6-1-.6V3h2v9.23z"/></svg>,
  diamond: <svg width="24" height="24" viewBox="0 0 24 24" fill="currentColor"><path d="M19 3H5L2 9l10 12L22 9l-3-6zm-9.62 8L12 16.88 14.62 11H9.38zm5.69-2L17.26 5h-2.65l-1.3 4h2.06zM10.69 9l-1.3-4H6.74l2.19 4h1.76zM12 5.12L13.17 9h-2.34L12 5.12z"/></svg>,
  timer: <svg width="24" height="24" viewBox="0 0 24 24" fill="currentColor"><path d="M15 1H9v2h6V1zm-4 13h2V8h-2v6zm8.03-6.61l1.42-1.42c-.43-.51-.9-.99-1.41-1.41l-1.42 1.42C16.07 4.74 14.12 4 12 4c-4.97 0-9 4.03-9 9s4.02 9 9 9 9-4.03 9-9c0-2.12-.74-4.07-1.97-5.61zM12 20c-3.87 0-7-3.13-7-7s3.13-7 7-7 7 3.13 7 7-3.13 7-7 7z"/></svg>,
  schedule: <svg width="24" height="24" viewBox="0 0 24 24" fill="currentColor"><path d="M11.99 2C6.47 2 2 6.48 2 12s4.47 10 9.99 10C17.52 22 22 17.52 22 12S17.52 2 11.99 2zM12 20c-4.42 0-8-3.58-8-8s3.58-8 8-8 8 3.58 8 8-3.58 8-8 8zm.5-13H11v6l5.25 3.15.75-1.23-4.5-2.67V7z"/></svg>,
  hourglass_bottom: <svg width="24" height="24" viewBox="0 0 24 24" fill="currentColor"><path d="M18 22l-.01-6L14 12l3.99-4.01L18 2H6v6l4 4-4 3.99V22h12zM8 7.5V4h8v3.5l-4 4-4-4z"/></svg>,
  hourglass_full: <svg width="24" height="24" viewBox="0 0 24 24" fill="currentColor"><path d="M6 2v6h.01L6 8.01 10 12l-4 4 .01.01H6V22h12v-5.99h-.01L18 16l-4-4 4-3.99-.01-.01H18V2H6z"/></svg>,
  today: <svg width="24" height="24" viewBox="0 0 24 24" fill="currentColor"><path d="M19 3h-1V1h-2v2H8V1H6v2H5c-1.11 0-1.99.9-1.99 2L3 19c0 1.1.89 2 2 2h14c1.1 0 2-.9 2-2V5c0-1.1-.9-2-2-2zm0 16H5V8h14v11zM9 10H7v2h2v-2zm4 0h-2v2h2v-2zm4 0h-2v2h2v-2z"/></svg>,
  event_available: <svg width="24" height="24" viewBox="0 0 24 24" fill="currentColor"><path d="M16.53 11.06L15.47 10l-4.88 4.88-2.12-2.12-1.06 1.06L10.59 17l5.94-5.94zM19 3h-1V1h-2v2H8V1H6v2H5c-1.11 0-1.99.9-1.99 2L3 19c0 1.1.89 2 2 2h14c1.1 0 2-.9 2-2V5c0-1.1-.9-2-2-2zm0 16H5V8h14v11z"/></svg>,
  local_fire_department: <svg width="24" height="24" viewBox="0 0 24 24" fill="currentColor"><path d="M19.48 12.35c-1.57-4.08-7.16-4.3-5.81-10.23.1-.44-.37-.78-.75-.55C9.29 3.71 6.68 8 8.87 13.62c.18.46-.36.89-.75.59-1.81-1.37-2-3.34-1.84-4.75.06-.52-.62-.77-.91-.34C4.69 10.16 4 11.84 4 14.37c.38 5.6 5.11 7.32 6.81 7.54 2.43.31 5.06-.14 6.95-1.87 2.08-1.93 2.84-5.01 1.72-7.69z"/></svg>,
};

// Default icon for unknown types
const defaultIcon = <svg width="24" height="24" viewBox="0 0 24 24" fill="currentColor"><path d="M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2zm-2 15l-5-5 1.41-1.41L10 14.17l7.59-7.59L19 8l-9 9z"/></svg>;

function getAchievementIcon(iconName: string): JSX.Element {
  return achievementIcons[iconName] || defaultIcon;
}

function formatThreshold(achievement: AchievementItem): string {
  if (achievement.category === "audio") {
    const seconds = achievement.threshold;
    if (seconds >= 3600) return `${seconds / 3600}h`;
    if (seconds >= 60) return `${seconds / 60}m`;
    return `${seconds}s`;
  }
  return achievement.threshold.toLocaleString();
}

function formatCurrentValue(achievement: AchievementItem): string {
  if (achievement.category === "audio") {
    const seconds = achievement.current_value;
    if (seconds >= 3600) return `${(seconds / 3600).toFixed(1)}h`;
    if (seconds >= 60) return `${Math.floor(seconds / 60)}m`;
    return `${seconds}s`;
  }
  return achievement.current_value.toLocaleString();
}

// Format time saved in a readable way
function formatTimeSaved(seconds: number): string {
  if (seconds < 60) return `${seconds}s`;
  if (seconds < 3600) {
    const mins = Math.floor(seconds / 60);
    const secs = seconds % 60;
    return secs > 0 ? `${mins}m ${secs}s` : `${mins}m`;
  }
  const hours = Math.floor(seconds / 3600);
  const mins = Math.floor((seconds % 3600) / 60);
  return mins > 0 ? `${hours}h ${mins}m` : `${hours}h`;
}

export function StatsPanel({ onClose, refreshTrigger }: StatsPanelProps) {
  const [stats, setStats] = useState<UserStats | null>(null);
  const [transcripts, setTranscripts] = useState<TranscriptItem[]>([]);
  const [achievements, setAchievements] = useState<AchievementsResponse | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [activeTab, setActiveTab] = useState<"stats" | "history" | "achievements">("stats");
  const [expandedTranscript, setExpandedTranscript] = useState<number | null>(null);
  const [editingWpm, setEditingWpm] = useState(false);
  const [wpmValue, setWpmValue] = useState("");

  const loadData = useCallback(async () => {
    if (!isLoggedIn()) {
      setError("Please log in to view statistics");
      setLoading(false);
      return;
    }

    setLoading(true);
    setError(null);

    try {
      const [statsData, transcriptsData, achievementsData] = await Promise.all([
        getUserStats(),
        getTranscriptHistory(0, 500), // Load up to 500 entries
        getUserAchievements(),
      ]);
      setStats(statsData);
      setTranscripts(transcriptsData);
      setAchievements(achievementsData);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to load data");
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

  const handleWpmEdit = () => {
    setWpmValue(stats?.typing_wpm?.toString() || "40");
    setEditingWpm(true);
  };

  const handleWpmSave = async () => {
    const newWpm = parseInt(wpmValue, 10);
    if (isNaN(newWpm) || newWpm < 1 || newWpm > 200) {
      return;
    }
    try {
      await updateTypingWpm(newWpm);
      setEditingWpm(false);
      loadData(); // Refresh to get new time saved calculations
    } catch (err) {
      console.error("Failed to update WPM:", err);
    }
  };

  const handleWpmCancel = () => {
    setEditingWpm(false);
    setWpmValue("");
  };

  return (
    <div className="stats-overlay" onClick={onClose}>
      <div className="stats-panel" onClick={(e) => e.stopPropagation()}>
        <div className="stats-header">
          <h2>Statistics</h2>
          <button className="close-btn" onClick={onClose}>
            <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              <path d="M18 6L6 18M6 6l12 12" />
            </svg>
          </button>
        </div>

        <div className="stats-tabs">
          <button
            className={`stats-tab ${activeTab === "stats" ? "active" : ""}`}
            onClick={() => setActiveTab("stats")}
          >
            Overview
          </button>
          <button
            className={`stats-tab ${activeTab === "history" ? "active" : ""}`}
            onClick={() => setActiveTab("history")}
          >
            History
          </button>
          <button
            className={`stats-tab ${activeTab === "achievements" ? "active" : ""}`}
            onClick={() => setActiveTab("achievements")}
          >
            Achievements
            {achievements && (
              <span className="achievement-badge">
                {achievements.total_unlocked}/{achievements.total_achievements}
              </span>
            )}
          </button>
        </div>

        <div className="stats-content">
          {loading ? (
            <div className="stats-loading">
              <div className="spinner" />
              <p>Loading...</p>
            </div>
          ) : error ? (
            <div className="stats-error">
              <p>{error}</p>
              <button onClick={loadData}>Retry</button>
            </div>
          ) : activeTab === "stats" && stats ? (
            <div className="stats-grid">
              <div className="stat-card primary time-saved">
                <div className="stat-value">{formatTimeSaved(stats.time_saved_seconds)}</div>
                <div className="stat-label">Time Saved</div>
              </div>
              <div className="stat-card">
                <div className="stat-value">{stats.total_transcriptions}</div>
                <div className="stat-label">Total Transcriptions</div>
              </div>
              <div className="stat-card">
                <div className="stat-value">{stats.total_words.toLocaleString()}</div>
                <div className="stat-label">Total Words</div>
              </div>
              <div className="stat-card">
                <div className="stat-value">{formatDuration(stats.total_audio_seconds)}</div>
                <div className="stat-label">Audio Transcribed</div>
              </div>
              <div className="stat-card accent">
                <div className="stat-value">{stats.average_words_per_minute}</div>
                <div className="stat-label">Avg Speaking WPM</div>
              </div>

              <div className="stat-divider">
                <span>Today</span>
              </div>

              <div className="stat-card">
                <div className="stat-value">{formatTimeSaved(stats.time_saved_today_seconds)}</div>
                <div className="stat-label">Time Saved Today</div>
              </div>
              <div className="stat-card">
                <div className="stat-value">{stats.transcriptions_today}</div>
                <div className="stat-label">Transcriptions Today</div>
              </div>
              <div className="stat-card">
                <div className="stat-value">{stats.words_today.toLocaleString()}</div>
                <div className="stat-label">Words Today</div>
              </div>

              <div className="stat-divider">
                <span>Your Typing Speed</span>
              </div>

              <div className="stat-card wpm-setting" style={{ gridColumn: "span 2" }}>
                {editingWpm ? (
                  <div className="wpm-edit">
                    <input
                      type="number"
                      value={wpmValue}
                      onChange={(e) => setWpmValue(e.target.value)}
                      min="1"
                      max="200"
                      autoFocus
                    />
                    <span className="wpm-unit">WPM</span>
                    <div className="wpm-actions">
                      <button onClick={handleWpmSave} className="save-btn">Save</button>
                      <button onClick={handleWpmCancel} className="cancel-btn">Cancel</button>
                    </div>
                  </div>
                ) : (
                  <div className="wpm-display" onClick={handleWpmEdit}>
                    <div className="stat-value">{stats.typing_wpm} WPM</div>
                    <div className="stat-label">Your typing speed (click to edit)</div>
                    <div className="wpm-hint">
                      Used to calculate how much time you save by speaking instead of typing
                    </div>
                  </div>
                )}
              </div>
            </div>
          ) : activeTab === "history" ? (
            <div className="transcript-list">
              {transcripts.length === 0 ? (
                <div className="empty-state">
                  <p>No transcriptions yet</p>
                  <p className="empty-hint">Start recording to see your history here</p>
                </div>
              ) : (
                transcripts.map((t) => (
                  <div
                    key={t.id}
                    className={`transcript-item ${expandedTranscript === t.id ? "expanded" : ""}`}
                  >
                    <div
                      className="transcript-header"
                      onClick={() => setExpandedTranscript(expandedTranscript === t.id ? null : t.id)}
                    >
                      <div className="transcript-preview">
                        {t.polished_text.slice(0, 80)}
                        {t.polished_text.length > 80 ? "..." : ""}
                      </div>
                      <div className="transcript-meta">
                        <span className="transcript-time">{formatDate(t.created_at)}</span>
                        <span className="transcript-words">{t.word_count} words</span>
                        <span className="transcript-wpm">{t.words_per_minute} WPM</span>
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
                ))
              )}
            </div>
          ) : activeTab === "achievements" && achievements ? (
            <div className="achievements-container">
              <div className="achievements-summary">
                <div className="achievements-progress-ring">
                  <svg viewBox="0 0 100 100">
                    <circle
                      className="progress-bg"
                      cx="50"
                      cy="50"
                      r="45"
                      fill="none"
                      strokeWidth="8"
                    />
                    <circle
                      className="progress-fill"
                      cx="50"
                      cy="50"
                      r="45"
                      fill="none"
                      strokeWidth="8"
                      strokeDasharray={`${(achievements.total_unlocked / achievements.total_achievements) * 283} 283`}
                      transform="rotate(-90 50 50)"
                    />
                  </svg>
                  <div className="progress-text">
                    <span className="progress-count">{achievements.total_unlocked}</span>
                    <span className="progress-total">/ {achievements.total_achievements}</span>
                  </div>
                </div>
                <p className="achievements-label">Achievements Unlocked</p>
              </div>

              <div className="achievements-list">
                {/* Unlocked achievements first */}
                {achievements.achievements
                  .filter((a: AchievementItem) => a.unlocked)
                  .map((achievement: AchievementItem) => (
                    <div key={achievement.id} className="achievement-item unlocked">
                      <div className="achievement-icon">
                        {getAchievementIcon(achievement.icon)}
                      </div>
                      <div className="achievement-info">
                        <div className="achievement-name">{achievement.name}</div>
                        <div className="achievement-description">{achievement.description}</div>
                      </div>
                      <div className="achievement-status">
                        <svg width="20" height="20" viewBox="0 0 24 24" fill="currentColor">
                          <path d="M9 16.17L4.83 12l-1.42 1.41L9 19 21 7l-1.41-1.41L9 16.17z"/>
                        </svg>
                      </div>
                    </div>
                  ))}

                {/* In-progress achievements */}
                {achievements.achievements
                  .filter((a: AchievementItem) => !a.unlocked)
                  .sort((a: AchievementItem, b: AchievementItem) => b.progress - a.progress) // Show closest to completion first
                  .map((achievement: AchievementItem) => (
                    <div key={achievement.id} className="achievement-item locked">
                      <div className="achievement-icon">
                        {getAchievementIcon(achievement.icon)}
                      </div>
                      <div className="achievement-info">
                        <div className="achievement-name">{achievement.name}</div>
                        <div className="achievement-description">{achievement.description}</div>
                        <div className="achievement-progress">
                          <div className="progress-bar">
                            <div
                              className="progress-fill"
                              style={{ width: `${achievement.progress * 100}%` }}
                            />
                          </div>
                          <span className="progress-label">
                            {formatCurrentValue(achievement)} / {formatThreshold(achievement)}
                          </span>
                        </div>
                      </div>
                    </div>
                  ))}
              </div>
            </div>
          ) : null}
        </div>
      </div>
    </div>
  );
}
