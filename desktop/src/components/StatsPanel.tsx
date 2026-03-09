/**
 * StatsPanel — Rebuilt statistics experience.
 *
 * Two tabs:
 *   1. Overview: hero time-saved card, core metrics grid, today section,
 *      weekly comparison, speech patterns, personal records, typing-speed setting.
 *   2. History: activity heatmap + scrollable transcript list.
 *
 * All data comes from three endpoints:
 *   GET /auth/stats          → UserStats (basic numbers)
 *   GET /auth/stats/detailed → DetailedStatsResponse (breakdowns, streaks, records)
 *   GET /auth/transcripts    → TranscriptItem[]
 */

import { useState, useEffect, useCallback, useMemo } from "react";
import {
  getUserStats,
  getTranscriptHistory,
  getDetailedStats,
  updateTypingWpm,
  UserStats,
  TranscriptItem,
  DetailedStatsResponse,
  isLoggedIn,
} from "../lib/api";
import {
  getTimeSavedEquivalent,
  getWordEquivalent,
  getAudioEquivalent,
} from "../lib/statsEquivalents";
import { AnimatedCounter, HeatMap } from "./stats";

/* ── props ─────────────────────────────────────────────────── */

interface StatsPanelProps {
  onClose: () => void;
  refreshTrigger?: number;
}

type TabType = "overview" | "history";

/* ── helpers ───────────────────────────────────────────────── */

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

function computeHeatmapData(transcripts: TranscriptItem[]) {
  const byDate: Record<string, { words: number; count: number }> = {};
  transcripts.forEach((t) => {
    const dateStr = new Date(t.created_at).toISOString().split("T")[0];
    if (!byDate[dateStr]) byDate[dateStr] = { words: 0, count: 0 };
    byDate[dateStr].words += t.word_count;
    byDate[dateStr].count += 1;
  });
  return Object.entries(byDate).map(([date, d]) => ({
    date,
    words: d.words,
    count: d.count,
  }));
}

function pctChange(a: number, b: number) {
  if (b === 0) return a > 0 ? 100 : 0;
  return Math.round(((a - b) / b) * 100);
}

/* ── component ─────────────────────────────────────────────── */

export function StatsPanel({ onClose, refreshTrigger }: StatsPanelProps) {
  const [stats, setStats] = useState<UserStats | null>(null);
  const [detailed, setDetailed] = useState<DetailedStatsResponse | null>(null);
  const [transcripts, setTranscripts] = useState<TranscriptItem[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [activeTab, setActiveTab] = useState<TabType>("overview");
  const [expandedTranscript, setExpandedTranscript] = useState<number | null>(null);
  const [editingWpm, setEditingWpm] = useState(false);
  const [wpmValue, setWpmValue] = useState("");

  /* ── data fetching ────────────────────────────────────────── */

  const loadData = useCallback(async () => {
    if (!isLoggedIn()) {
      setError("Please log in to view statistics");
      setLoading(false);
      return;
    }
    setLoading(true);
    setError(null);

    const [statsRes, detailedRes, histRes] = await Promise.allSettled([
      getUserStats(),
      getDetailedStats(),
      getTranscriptHistory(0, 200),
    ]);

    if (statsRes.status === "fulfilled") setStats(statsRes.value);
    if (detailedRes.status === "fulfilled") setDetailed(detailedRes.value);
    if (histRes.status === "fulfilled") setTranscripts(histRes.value);

    if (
      statsRes.status === "rejected" &&
      detailedRes.status === "rejected"
    ) {
      setError("Failed to load statistics. Please try again.");
    }
    setLoading(false);
  }, []);

  useEffect(() => {
    loadData();
  }, [loadData, refreshTrigger]);

  /* ── derived values ───────────────────────────────────────── */

  const heatmapData = useMemo(() => computeHeatmapData(transcripts), [transcripts]);

  const timeSavedEq = stats ? getTimeSavedEquivalent(stats.time_saved_seconds) : null;
  const wordEq = stats ? getWordEquivalent(stats.total_words) : null;
  const audioMinutes = stats ? stats.total_audio_seconds / 60 : 0;
  const audioEq = audioMinutes > 0 ? getAudioEquivalent(audioMinutes) : null;

  const weekChange = detailed
    ? pctChange(detailed.words_this_week, detailed.growth?.prev_week_words || 0)
    : 0;

  /* ── wpm editing ──────────────────────────────────────────── */

  const handleWpmEdit = () => {
    setWpmValue(stats?.typing_wpm?.toString() || "40");
    setEditingWpm(true);
  };
  const handleWpmSave = async () => {
    const v = parseInt(wpmValue, 10);
    if (isNaN(v) || v < 1 || v > 200) return;
    try {
      await updateTypingWpm(v);
      setEditingWpm(false);
      loadData();
    } catch {
      /* keep editing open */
    }
  };

  /* ── peak hour / day helper ────────────────────────────────── */

  const peakHourLabel = detailed?.productivity?.peak_hour_label || "--";
  const peakDay = detailed?.productivity?.peak_day || "--";

  /* ── render ───────────────────────────────────────────────── */

  return (
    <div className="stats-overlay" onClick={onClose}>
      <div className="stats-panel stats-panel-wide" onClick={(e) => e.stopPropagation()}>
        {/* Header */}
        <div className="stats-header">
          <h2>Your Statistics</h2>
          <button className="close-btn" onClick={onClose} aria-label="Close statistics panel">
            <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              <path d="M18 6L6 18M6 6l12 12" />
            </svg>
          </button>
        </div>

        {/* Tabs */}
        <div className="stats-tabs" role="tablist">
          <button
            role="tab"
            aria-selected={activeTab === "overview"}
            className={`stats-tab ${activeTab === "overview" ? "active" : ""}`}
            onClick={() => setActiveTab("overview")}
          >
            Overview
          </button>
          <button
            role="tab"
            aria-selected={activeTab === "history"}
            className={`stats-tab ${activeTab === "history" ? "active" : ""}`}
            onClick={() => setActiveTab("history")}
          >
            History
          </button>
        </div>

        {/* Body */}
        <div className="stats-content">
          {loading ? (
            <div className="stats-loading">
              <div className="spinner" />
              <p>Loading statistics...</p>
            </div>
          ) : error ? (
            <div className="stats-error">
              <p>{error}</p>
              <button onClick={loadData}>Retry</button>
            </div>
          ) : activeTab === "overview" && stats ? (
            <div className="story-container">

              {/* ── 1. Hero: Time Saved ───────────────────────── */}
              <div className="hero-stats-section">
                <div className="hero-primary">
                  <div className="hero-icon">{timeSavedEq?.icon || "⚡"}</div>
                  <div className="hero-value">
                    <AnimatedCounter value={Math.floor(stats.time_saved_seconds / 3600)} suffix="h " />
                    <AnimatedCounter value={Math.floor((stats.time_saved_seconds % 3600) / 60)} suffix="m" />
                  </div>
                  <div className="hero-label">Time Saved</div>
                  <div className="hero-today-sub">Today: {Math.floor(stats.time_saved_today_seconds / 60)}m</div>
                  {timeSavedEq && (
                    <div className="hero-equivalent">
                      That's enough time for {timeSavedEq.description}
                    </div>
                  )}
                  {weekChange !== 0 && (
                    <div className={`hero-trend ${weekChange > 0 ? "trend-up" : "trend-down"}`}>
                      {weekChange > 0 ? "+" : ""}{weekChange}% vs last week
                    </div>
                  )}
                </div>
              </div>

              {/* ── 2. Core Stats Grid ────────────────────────── */}
              <div className="core-stats-grid">
                <div className="core-stat-card">
                  <div className="core-stat-value"><AnimatedCounter value={stats.total_transcriptions} /></div>
                  <div className="core-stat-label">Transcriptions</div>
                </div>
                <div className="core-stat-card">
                  <div className="core-stat-value"><AnimatedCounter value={stats.total_words} /></div>
                  <div className="core-stat-label">Words Spoken</div>
                  {wordEq && <div className="core-stat-equiv">{wordEq.icon} {wordEq.description}</div>}
                </div>
                <div className="core-stat-card">
                  <div className="core-stat-value">
                    {Math.floor(audioMinutes / 60) > 0 && (
                      <><AnimatedCounter value={Math.floor(audioMinutes / 60)} suffix="h " /></>
                    )}
                    <AnimatedCounter value={Math.round(audioMinutes % 60)} suffix="m" />
                  </div>
                  <div className="core-stat-label">Audio Recorded</div>
                  {audioEq && <div className="core-stat-equiv">{audioEq.icon} {audioEq.description}</div>}
                </div>
                <div className="core-stat-card">
                  <div className="core-stat-value">{Math.round(stats.average_words_per_minute)}</div>
                  <div className="core-stat-label">Avg Speaking WPM</div>
                </div>
                <div className="core-stat-card">
                  <div className="core-stat-value">
                    {stats.total_transcriptions > 0
                      ? Math.round(stats.total_words / stats.total_transcriptions)
                      : 0}
                  </div>
                  <div className="core-stat-label">Avg Words / Entry</div>
                </div>
                <div className="core-stat-card streak-card">
                  <div className="core-stat-value">
                    <span className="streak-flame-icon">🔥</span>
                    {detailed?.current_streak_days ?? 0}
                  </div>
                  <div className="core-stat-label">Day Streak</div>
                  {detailed && detailed.longest_streak_days > 0 && (
                    <div className="core-stat-equiv">Best: {detailed.longest_streak_days}d</div>
                  )}
                </div>
              </div>

              {/* ── 3. Today ──────────────────────────────────── */}
              <div className="today-section">
                <h3 className="section-title">Today</h3>
                <div className="today-stats-row">
                  <div className="today-stat">
                    <div className="today-stat-value">{Math.floor(stats.time_saved_today_seconds / 60)}m</div>
                    <div className="today-stat-label">Time Saved</div>
                  </div>
                  <div className="today-stat">
                    <div className="today-stat-value"><AnimatedCounter value={stats.words_today} /></div>
                    <div className="today-stat-label">Words</div>
                  </div>
                  <div className="today-stat">
                    <div className="today-stat-value"><AnimatedCounter value={stats.transcriptions_today} /></div>
                    <div className="today-stat-label">Entries</div>
                  </div>
                </div>
              </div>

              {/* ── 4. This Week vs Last Week ─────────────────── */}
              {detailed && (
                <div className="week-comparison-section">
                  <h3 className="section-title">Weekly Comparison</h3>
                  <div className="week-comparison-cards">
                    <div className="week-card current">
                      <div className="week-card-label">This week</div>
                      <div className="week-card-value">
                        <AnimatedCounter value={detailed.words_this_week} />
                      </div>
                      <div className="week-card-unit">words</div>
                      {weekChange !== 0 && (
                        <div className={`week-card-change ${weekChange > 0 ? "up" : "down"}`}>
                          {weekChange > 0 ? "↑" : "↓"} {Math.abs(weekChange)}%
                        </div>
                      )}
                    </div>
                    <div className="week-card previous">
                      <div className="week-card-label">Last week</div>
                      <div className="week-card-value">
                        {(detailed.growth?.prev_week_words || 0).toLocaleString()}
                      </div>
                      <div className="week-card-unit">words</div>
                    </div>
                  </div>
                </div>
              )}

              {/* ── 5. Speech Patterns ────────────────────────── */}
              {detailed && (
                <div className="speech-patterns-section">
                  <h3 className="section-title">Speech Patterns</h3>
                  <div className="patterns-grid">
                    <div className="pattern-card">
                      <div className="pattern-icon">⏰</div>
                      <div className="pattern-label">Peak Hour</div>
                      <div className="pattern-value">{peakHourLabel}</div>
                    </div>
                    <div className="pattern-card">
                      <div className="pattern-icon">📅</div>
                      <div className="pattern-label">Peak Day</div>
                      <div className="pattern-value">{peakDay}</div>
                    </div>
                    <div className="pattern-card">
                      <div className="pattern-icon">⚡</div>
                      <div className="pattern-label">Efficiency</div>
                      <div className="pattern-value">
                        {detailed.productivity.efficiency_score.toFixed(1)} w/s
                      </div>
                    </div>
                    <div className="pattern-card">
                      <div className="pattern-icon">📊</div>
                      <div className="pattern-label">Active Days</div>
                      <div className="pattern-value">{detailed.total_active_days}</div>
                    </div>
                  </div>
                </div>
              )}

              {/* ── 6. Personal Records ───────────────────────── */}
              {detailed && (
                <div className="personal-records-section">
                  <h3 className="section-title">Personal Records</h3>
                  <div className="personal-records-grid">
                    {detailed.most_productive_day_words > 0 && (
                      <div className="record-card">
                        <div className="record-card-icon">🏆</div>
                        <div className="record-card-content">
                          <div className="record-card-label">Best Day</div>
                          <div className="record-card-value">
                            {detailed.most_productive_day_words.toLocaleString()} words
                          </div>
                          {detailed.most_productive_day && (
                            <div className="record-card-date">
                              {new Date(detailed.most_productive_day).toLocaleDateString("en-US", { month: "short", day: "numeric" })}
                            </div>
                          )}
                        </div>
                      </div>
                    )}
                    {detailed.fastest_wpm > 0 && (
                      <div className="record-card">
                        <div className="record-card-icon">⚡</div>
                        <div className="record-card-content">
                          <div className="record-card-label">Fastest Speed</div>
                          <div className="record-card-value">{Math.round(detailed.fastest_wpm)} WPM</div>
                        </div>
                      </div>
                    )}
                    {detailed.longest_transcription_words > 0 && (
                      <div className="record-card">
                        <div className="record-card-icon">📝</div>
                        <div className="record-card-content">
                          <div className="record-card-label">Longest Entry</div>
                          <div className="record-card-value">
                            {detailed.longest_transcription_words.toLocaleString()} words
                          </div>
                        </div>
                      </div>
                    )}
                    {detailed.longest_streak_days > 0 && (
                      <div className="record-card">
                        <div className="record-card-icon">🔥</div>
                        <div className="record-card-content">
                          <div className="record-card-label">Longest Streak</div>
                          <div className="record-card-value">{detailed.longest_streak_days} days</div>
                        </div>
                      </div>
                    )}
                  </div>
                </div>
              )}

              {/* ── 7. Typing Speed Setting ───────────────────── */}
              <div className="typing-speed-section">
                <h3 className="section-title">Your Typing Speed</h3>
                <div className="stat-card wpm-setting">
                  {editingWpm ? (
                    <div className="wpm-edit">
                      <input
                        type="number"
                        value={wpmValue}
                        onChange={(e) => setWpmValue(e.target.value)}
                        min="1"
                        max="200"
                        autoFocus
                        aria-label="Typing speed in words per minute"
                      />
                      <span className="wpm-unit">WPM</span>
                      <div className="wpm-actions">
                        <button onClick={handleWpmSave} className="save-btn">Save</button>
                        <button onClick={() => setEditingWpm(false)} className="cancel-btn">Cancel</button>
                      </div>
                    </div>
                  ) : (
                    <div className="wpm-display" onClick={handleWpmEdit}>
                      <div className="stat-value">{stats.typing_wpm} WPM</div>
                      <div className="stat-label">Click to edit</div>
                      <div className="wpm-hint">
                        Used to calculate how much time you save by speaking instead of typing
                      </div>
                    </div>
                  )}
                </div>
              </div>

              {/* ── 8. Member Since ───────────────────────────── */}
              {detailed && (
                <div className="member-since-section">
                  <div className="member-since-text">
                    Member for {detailed.days_as_member} day{detailed.days_as_member !== 1 ? "s" : ""} &middot; since{" "}
                    {new Date(detailed.member_since).toLocaleDateString("en-US", {
                      month: "long",
                      day: "numeric",
                      year: "numeric",
                    })}
                  </div>
                </div>
              )}
            </div>
          ) : activeTab === "history" ? (
            <div className="transcript-list">
              {/* Activity Heatmap */}
              {heatmapData.length > 0 && (
                <div className="heatmap-section">
                  <h3 className="section-title">Activity Overview</h3>
                  <HeatMap data={heatmapData} weeks={12} />
                </div>
              )}
              {transcripts.length === 0 ? (
                <div className="empty-state">
                  <p>No transcriptions yet</p>
                  <p className="empty-hint">Start recording to see your history here</p>
                </div>
              ) : (
                transcripts.slice(0, 100).map((t) => (
                  <div
                    key={t.id}
                    className={`transcript-item ${expandedTranscript === t.id ? "expanded" : ""}`}
                  >
                    <div
                      className="transcript-header"
                      onClick={() =>
                        setExpandedTranscript(expandedTranscript === t.id ? null : t.id)
                      }
                    >
                      <div className="transcript-preview">
                        {t.polished_text.slice(0, 80)}
                        {t.polished_text.length > 80 ? "..." : ""}
                      </div>
                      <div className="transcript-meta">
                        <span className="transcript-time">{formatDate(t.created_at)}</span>
                        <span className="transcript-words">{t.word_count} words</span>
                        {t.words_per_minute > 0 && (
                          <span className="transcript-wpm">{Math.round(t.words_per_minute)} WPM</span>
                        )}
                      </div>
                    </div>
                    {expandedTranscript === t.id && (
                      <div className="transcript-expanded">
                        <div className="transcript-full-text">{t.polished_text}</div>
                        <div className="transcript-actions">
                          <button
                            onClick={() => navigator.clipboard.writeText(t.polished_text)}
                          >
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
          ) : null}
        </div>
      </div>
    </div>
  );
}
