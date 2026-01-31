// Stats panel - restored overview layout
import { useState, useEffect, useCallback, useMemo } from "react";
import {
  getUserStats,
  getTranscriptHistory,
  getUserAchievements,
  getDetailedStats,
  updateTypingWpm,
  getGamificationProgress,
  getGamificationAchievements,
  getLeaderboard,
  UserStats,
  TranscriptItem,
  AchievementsResponse,
  DetailedStatsResponse,
  GamificationProgress,
  GamificationAchievement,
  LeaderboardResponse,
  isLoggedIn,
} from "../lib/api";
import { AchievementCard } from "./gamification/AchievementCard";
import { XPBar } from "./gamification/XPBar";
import {
  getTimeSavedEquivalent,
} from "../lib/statsEquivalents";
// insightGenerator removed with Insights tab
import {
  AnimatedCounter,
  TrendIndicator,
  TrendBadge,
  HeatMap,
} from "./stats";
import { Confetti } from "./Confetti";

interface StatsPanelProps {
  onClose: () => void;
  refreshTrigger?: number;
}

type TabType = "overview" | "history" | "achievements";

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

// Compute heatmap data from transcripts
function computeHeatmapData(transcripts: TranscriptItem[]) {
  const byDate: Record<string, { words: number; count: number }> = {};

  transcripts.forEach(t => {
    const date = new Date(t.created_at);
    const dateStr = date.toISOString().split('T')[0];
    if (!byDate[dateStr]) {
      byDate[dateStr] = { words: 0, count: 0 };
    }
    byDate[dateStr].words += t.word_count;
    byDate[dateStr].count += 1;
  });

  return Object.entries(byDate).map(([date, data]) => ({
    date,
    words: data.words,
    count: data.count,
  }));
}

// Calculate week-over-week change
function getWeekOverWeekChange(thisWeek: number, lastWeek: number) {
  if (lastWeek === 0) {
    return { percentage: 0, direction: 'same' as const };
  }
  const change = ((thisWeek - lastWeek) / lastWeek) * 100;
  const rounded = Math.round(Math.abs(change));
  if (change > 5) return { percentage: rounded, direction: 'up' as const };
  if (change < -5) return { percentage: rounded, direction: 'down' as const };
  return { percentage: 0, direction: 'same' as const };
}

const CATEGORIES: { value: string; label: string }[] = [
  { value: "all", label: "All Categories" },
  { value: "volume", label: "Volume" },
  { value: "streak", label: "Streak" },
  { value: "speed", label: "Speed" },
  { value: "context", label: "Context" },
  { value: "formality", label: "Formality" },
  { value: "learning", label: "Learning" },
  { value: "temporal", label: "Temporal" },
  { value: "records", label: "Records" },
  { value: "combo", label: "Combo" },
  { value: "special", label: "Special" },
];

const RARITIES: { value: string; label: string }[] = [
  { value: "all", label: "All Rarities" },
  { value: "common", label: "Common" },
  { value: "rare", label: "Rare" },
  { value: "epic", label: "Epic" },
  { value: "legendary", label: "Legendary" },
];

const SORT_OPTIONS: { value: string; label: string }[] = [
  { value: "impressive", label: "Most Impressive" },
  { value: "closest", label: "Closest to Done" },
  { value: "recent", label: "Unlock Date" },
  { value: "rarity", label: "Rarity" },
  { value: "name", label: "Name A-Z" },
];

const RARITY_ORDER: Record<string, number> = { legendary: 0, epic: 1, rare: 2, common: 3 };

const PAGE_SIZE = 24;

export function StatsPanel({ onClose, refreshTrigger }: StatsPanelProps) {
  const [stats, setStats] = useState<UserStats | null>(null);
  const [detailedStats, setDetailedStats] = useState<DetailedStatsResponse | null>(null);
  const [transcripts, setTranscripts] = useState<TranscriptItem[]>([]);
  const [, setAchievements] = useState<AchievementsResponse | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [activeTab, setActiveTab] = useState<TabType>("overview");
  const [expandedTranscript, setExpandedTranscript] = useState<number | null>(null);
  const [editingWpm, setEditingWpm] = useState(false);
  const [wpmValue, setWpmValue] = useState("");
  const [showConfetti, setShowConfetti] = useState(false);
  const [prevUnlockedCount, setPrevUnlockedCount] = useState<number | null>(null);

  // Gamification state
  const [gamificationProgress, setGamificationProgress] = useState<GamificationProgress | null>(null);
  const [allAchievements, setAllAchievements] = useState<GamificationAchievement[]>([]);
  const [achievementPage, setAchievementPage] = useState(1);
  const [achievementCategory, setAchievementCategory] = useState<string>("all");
  const [achievementRarity, setAchievementRarity] = useState<string>("all");
  const [showUnlockedOnly, setShowUnlockedOnly] = useState(false);
  const [achievementSearch, setAchievementSearch] = useState("");
  const [achievementSort, setAchievementSort] = useState("impressive");
  const [loadingAchievements, setLoadingAchievements] = useState(false);
  const [, setLeaderboard] = useState<LeaderboardResponse | null>(null);
  const [leaderboardMetric] = useState<"lifetime_xp" | "achievements" | "words">("lifetime_xp");

  const loadData = useCallback(async () => {
    if (!isLoggedIn()) {
      setError("Please log in to view statistics");
      setLoading(false);
      return;
    }

    setLoading(true);
    setError(null);

    try {
      const [statsResult, transcriptsResult, achievementsResult, detailedResult, gamificationResult] = await Promise.allSettled([
        getUserStats(),
        getTranscriptHistory(0, 100),
        getUserAchievements(),
        getDetailedStats(),
        getGamificationProgress(),
      ]);

      if (statsResult.status === "fulfilled") setStats(statsResult.value);
      else console.error("Failed to load stats:", statsResult.reason);

      if (transcriptsResult.status === "fulfilled") setTranscripts(transcriptsResult.value);
      else console.error("Failed to load transcripts:", transcriptsResult.reason);

      if (detailedResult.status === "fulfilled") setDetailedStats(detailedResult.value);
      else console.error("Failed to load detailed stats:", detailedResult.reason);

      if (gamificationResult.status === "fulfilled") setGamificationProgress(gamificationResult.value);
      else console.error("Failed to load gamification:", gamificationResult.reason);

      if (achievementsResult.status === "fulfilled") {
        const achievementsData = achievementsResult.value;
        if (prevUnlockedCount !== null && achievementsData.total_unlocked > prevUnlockedCount) {
          setShowConfetti(true);
        }
        setPrevUnlockedCount(achievementsData.total_unlocked);
        setAchievements(achievementsData);
      } else {
        console.error("Failed to load achievements:", achievementsResult.reason);
      }

      // Load leaderboard
      try {
        const leaderboardData = await getLeaderboard(leaderboardMetric, 50, true);
        setLeaderboard(leaderboardData);
      } catch (err) {
        console.error("Failed to load leaderboard:", err);
      }

      // Only show error if ALL main data sources failed
      if (statsResult.status === "rejected" && transcriptsResult.status === "rejected" && detailedResult.status === "rejected") {
        setError("Failed to load statistics data");
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to load data");
    } finally {
      setLoading(false);
    }
  }, [prevUnlockedCount]);

  useEffect(() => {
    loadData();
  }, [loadData, refreshTrigger]);

  // Load all achievements once on mount (separate from refreshTrigger)
  useEffect(() => {
    if (!isLoggedIn()) return;
    setLoadingAchievements(true);
    (async () => {
      try {
        const all: GamificationAchievement[] = [];
        let page = 1;
        const batchSize = 100;
        let totalPages = 1;
        while (page <= totalPages) {
          const achResponse = await getGamificationAchievements({ page_size: batchSize, page });
          all.push(...achResponse.achievements);
          totalPages = achResponse.total_pages;
          page++;
        }
        setAllAchievements(all);
      } catch (err) {
        console.error("Failed to load gamification achievements:", err);
      } finally {
        setLoadingAchievements(false);
      }
    })();
  }, []);

  // Reset to page 1 when filters change
  useEffect(() => {
    setAchievementPage(1);
  }, [achievementCategory, achievementRarity, showUnlockedOnly, achievementSearch, achievementSort]);

  // Reload leaderboard when metric changes
  useEffect(() => {
    if (!loading) {
      getLeaderboard(leaderboardMetric, 50, true)
        .then(setLeaderboard)
        .catch(err => console.error("Failed to load leaderboard:", err));
    }
  }, [leaderboardMetric, loading]);

  // Compute heatmap data
  const heatmapData = useMemo(() => computeHeatmapData(transcripts), [transcripts]);

  // Get equivalents
  const timeSavedEquivalent = stats ? getTimeSavedEquivalent(stats.time_saved_seconds) : null;
  const audioMinutes = stats ? stats.total_audio_seconds / 60 : 0;

  // Week over week change (prev_week_words is the week before the current week)
  const wordsChange = detailedStats
    ? getWeekOverWeekChange(detailedStats.words_this_week, detailedStats.growth?.prev_week_words || 0)
    : { percentage: 0, direction: 'same' as const };

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
      loadData();
    } catch (err) {
      console.error("Failed to update WPM:", err);
    }
  };

  const handleWpmCancel = () => {
    setEditingWpm(false);
    setWpmValue("");
  };

  // Client-side filter + sort ALL achievements
  const filteredAchievements = useMemo(() => {
    let list = [...allAchievements];

    // Category filter
    if (achievementCategory !== "all") {
      list = list.filter(a => a.category === achievementCategory);
    }

    // Rarity filter
    if (achievementRarity !== "all") {
      list = list.filter(a => a.rarity === achievementRarity);
    }

    // Unlocked only
    if (showUnlockedOnly) {
      list = list.filter(a => a.is_unlocked);
    }

    // Search filter
    if (achievementSearch.trim()) {
      const q = achievementSearch.toLowerCase();
      list = list.filter(a =>
        a.name.toLowerCase().includes(q) || a.description.toLowerCase().includes(q)
      );
    }

    // Sort
    switch (achievementSort) {
      case "impressive":
        list.sort((a, b) => {
          // Unlocked first
          if (a.is_unlocked && !b.is_unlocked) return -1;
          if (!a.is_unlocked && b.is_unlocked) return 1;
          // By rarity (legendary > epic > rare > common), then threshold desc
          const rarityDiff = (RARITY_ORDER[a.rarity] ?? 9) - (RARITY_ORDER[b.rarity] ?? 9);
          if (rarityDiff !== 0) return rarityDiff;
          return b.threshold - a.threshold;
        });
        break;
      case "recent":
        list.sort((a, b) => {
          if (a.is_unlocked && !b.is_unlocked) return -1;
          if (!a.is_unlocked && b.is_unlocked) return 1;
          if (a.is_unlocked && b.is_unlocked) {
            const aDate = a.unlocked_at ? new Date(a.unlocked_at).getTime() : 0;
            const bDate = b.unlocked_at ? new Date(b.unlocked_at).getTime() : 0;
            if (bDate !== aDate) return bDate - aDate;
            // Tiebreak: higher threshold = harder = more impressive first
            return b.threshold - a.threshold;
          }
          return b.progress - a.progress;
        });
        break;
      case "closest":
        list.sort((a, b) => {
          if (!a.is_unlocked && !b.is_unlocked) return b.progress - a.progress;
          if (a.is_unlocked && b.is_unlocked) return 0;
          return a.is_unlocked ? 1 : -1;
        });
        break;
      case "rarity":
        list.sort((a, b) => (RARITY_ORDER[a.rarity] ?? 9) - (RARITY_ORDER[b.rarity] ?? 9));
        break;
      case "name":
        list.sort((a, b) => a.name.localeCompare(b.name));
        break;
    }

    return list;
  }, [allAchievements, achievementCategory, achievementRarity, showUnlockedOnly, achievementSearch, achievementSort]);

  // Client-side pagination
  const totalPages = Math.max(1, Math.ceil(filteredAchievements.length / PAGE_SIZE));
  const pagedAchievements = useMemo(() => {
    const start = (achievementPage - 1) * PAGE_SIZE;
    return filteredAchievements.slice(start, start + PAGE_SIZE);
  }, [filteredAchievements, achievementPage]);

  const pageNumbers = useMemo(() => {
    const pages: number[] = [];
    const maxVisible = 5;
    let start = Math.max(1, achievementPage - Math.floor(maxVisible / 2));
    let end = Math.min(totalPages, start + maxVisible - 1);
    if (end - start + 1 < maxVisible) {
      start = Math.max(1, end - maxVisible + 1);
    }
    for (let i = start; i <= end; i++) pages.push(i);
    return pages;
  }, [achievementPage, totalPages]);

  const goToPage = (page: number) => {
    if (page < 1 || page > totalPages) return;
    setAchievementPage(page);
  };

  return (
    <div className="stats-overlay" onClick={onClose}>
      <div className="stats-panel stats-panel-wide" onClick={(e) => e.stopPropagation()}>
        {/* Confetti for new achievements */}
        <Confetti isActive={showConfetti} onComplete={() => setShowConfetti(false)} />

        <div className="stats-header">
          <h2>Your Statistics</h2>
          <button className="close-btn" onClick={onClose}>
            <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              <path d="M18 6L6 18M6 6l12 12" />
            </svg>
          </button>
        </div>

        <div className="stats-tabs">
          <button
            className={`stats-tab ${activeTab === "overview" ? "active" : ""}`}
            onClick={() => setActiveTab("overview")}
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
            {gamificationProgress && (
              <span className="achievement-badge">
                {gamificationProgress.achievements.unlocked}/{gamificationProgress.achievements.total}
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
          ) : activeTab === "overview" && stats ? (
            <div className="story-container">
              {/* 1. Primary Card: Time Saved */}
              <div className="hero-stats-section">
                <div className="hero-primary">
                  <div className="hero-icon">{timeSavedEquivalent?.icon || '⚡'}</div>
                  <div className="hero-value">
                    <AnimatedCounter
                      value={Math.floor(stats.time_saved_seconds / 3600)}
                      suffix="h "
                    />
                    <AnimatedCounter
                      value={Math.floor((stats.time_saved_seconds % 3600) / 60)}
                      suffix="m"
                    />
                  </div>
                  <div className="hero-label">Time Saved</div>
                  <div className="hero-today-sub">Today: {Math.floor(stats.time_saved_today_seconds / 60)}m</div>
                  {timeSavedEquivalent && (
                    <div className="hero-equivalent">
                      "That's enough time for {timeSavedEquivalent.description}"
                    </div>
                  )}
                  {detailedStats && wordsChange.direction !== 'same' && (
                    <div className="hero-trend">
                      <TrendBadge
                        thisWeek={detailedStats.words_this_week}
                        lastWeek={detailedStats.growth?.prev_week_words || 0}
                        label="vs last week"
                      />
                    </div>
                  )}
                </div>
              </div>

              {/* 2. Core Stats Grid (3x2) */}
              <div className="core-stats-grid">
                <div className="core-stat-card">
                  <div className="core-stat-value"><AnimatedCounter value={stats.total_transcriptions} /></div>
                  <div className="core-stat-label">Total Transcriptions</div>
                </div>
                <div className="core-stat-card">
                  <div className="core-stat-value"><AnimatedCounter value={stats.total_words} /></div>
                  <div className="core-stat-label">Total Words</div>
                </div>
                <div className="core-stat-card">
                  <div className="core-stat-value">
                    {Math.floor(audioMinutes / 60) > 0 && (
                      <><AnimatedCounter value={Math.floor(audioMinutes / 60)} suffix="h " /></>
                    )}
                    <AnimatedCounter value={Math.round(audioMinutes % 60)} suffix="m" />
                  </div>
                  <div className="core-stat-label">Audio Transcribed</div>
                </div>
                <div className="core-stat-card">
                  <div className="core-stat-value">{Math.round(stats.average_words_per_minute)}</div>
                  <div className="core-stat-label">Avg WPM</div>
                </div>
                <div className="core-stat-card">
                  <div className="core-stat-value">
                    {stats.total_transcriptions > 0 ? Math.round(stats.total_words / stats.total_transcriptions) : 0}
                  </div>
                  <div className="core-stat-label">Avg Words/Transcription</div>
                </div>
                <div className="core-stat-card streak-card">
                  <div className="core-stat-value">
                    <span className="streak-flame-icon">🔥</span>
                    {detailedStats?.current_streak_days ?? 0}
                  </div>
                  <div className="core-stat-label">Current Streak</div>
                </div>
              </div>

              {/* 3. Today Section */}
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
                    <div className="today-stat-label">Transcriptions</div>
                  </div>
                </div>
              </div>

              {/* 4. Week Comparison */}
              {detailedStats && (
                <div className="week-comparison-section">
                  <h3 className="section-title">This Week</h3>
                  <div className="week-comparison-cards">
                    <div className="week-card current">
                      <div className="week-card-label">This week</div>
                      <div className="week-card-value">
                        <AnimatedCounter value={detailedStats.words_this_week} />
                      </div>
                      <div className="week-card-unit">words</div>
                      {wordsChange.direction !== 'same' && (
                        <div className={`week-card-change ${wordsChange.direction}`}>
                          <TrendIndicator
                            value={detailedStats.words_this_week}
                            previousValue={detailedStats.growth?.prev_week_words || 0}
                            size="md"
                          />
                        </div>
                      )}
                    </div>
                    <div className="week-card previous">
                      <div className="week-card-label">Last week</div>
                      <div className="week-card-value">{(detailedStats.growth?.prev_week_words || 0).toLocaleString()}</div>
                      <div className="week-card-unit">words</div>
                    </div>
                  </div>
                </div>
              )}

              {/* Personal Records */}
              {detailedStats && (
                <div className="personal-records-section">
                  <h3 className="section-title">
                    <span className="section-icon">🏅</span>
                    Personal Records
                  </h3>
                  <div className="personal-records-grid">
                    {detailedStats.most_productive_day_words > 0 && (
                      <div className="record-card">
                        <div className="record-card-icon">🏆</div>
                        <div className="record-card-content">
                          <div className="record-card-label">Most Words in a Day</div>
                          <div className="record-card-value">{detailedStats.most_productive_day_words.toLocaleString()} words</div>
                          {detailedStats.most_productive_day && (
                            <div className="record-card-date">
                              {new Date(detailedStats.most_productive_day).toLocaleDateString('en-US', { month: 'short', day: 'numeric' })}
                            </div>
                          )}
                        </div>
                      </div>
                    )}
                    {detailedStats.fastest_wpm > 0 && (
                      <div className="record-card">
                        <div className="record-card-icon">⚡</div>
                        <div className="record-card-content">
                          <div className="record-card-label">Fastest Speaking Speed</div>
                          <div className="record-card-value">{Math.round(detailedStats.fastest_wpm)} WPM</div>
                        </div>
                      </div>
                    )}
                    {detailedStats.longest_transcription_words > 0 && (
                      <div className="record-card">
                        <div className="record-card-icon">📝</div>
                        <div className="record-card-content">
                          <div className="record-card-label">Longest Transcription</div>
                          <div className="record-card-value">{detailedStats.longest_transcription_words.toLocaleString()} words</div>
                        </div>
                      </div>
                    )}
                    {detailedStats.longest_streak_days > 0 && (
                      <div className="record-card">
                        <div className="record-card-icon">🔥</div>
                        <div className="record-card-content">
                          <div className="record-card-label">Longest Streak</div>
                          <div className="record-card-value">{detailedStats.longest_streak_days} days</div>
                        </div>
                      </div>
                    )}
                  </div>
                </div>
              )}

              {/* Typing Speed Setting */}
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
                      <div className="stat-label">Click to edit</div>
                      <div className="wpm-hint">
                        Used to calculate how much time you save by speaking instead of typing
                      </div>
                    </div>
                  )}
                </div>
              </div>
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
                transcripts.slice(0, 50).map((t) => (
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
          ) : activeTab === "achievements" ? (
            <div className="achievements-container">
              {/* XP Progress Bar */}
              {gamificationProgress && (
                <div className="gamification-header">
                  <XPBar
                    currentXP={gamificationProgress.current_xp}
                    level={gamificationProgress.current_level}
                    xpToNextLevel={gamificationProgress.xp_to_next_level}
                    tier={gamificationProgress.prestige_tier}
                    lifetimeXP={gamificationProgress.lifetime_xp}
                  />
                </div>
              )}

              {/* Filter Bar */}
              <div className="achievement-filters">
                <div className="ach-filter-row">
                  <div className="ach-search-wrap">
                    <input
                      type="text"
                      className="ach-search-input"
                      placeholder="Search achievements..."
                      value={achievementSearch}
                      onChange={(e) => setAchievementSearch(e.target.value)}
                    />
                  </div>
                  <div className="filter-group">
                    <select
                      value={achievementCategory}
                      onChange={(e) => setAchievementCategory(e.target.value)}
                    >
                      {CATEGORIES.map(c => (
                        <option key={c.value} value={c.value}>{c.label}</option>
                      ))}
                    </select>
                  </div>
                  <div className="filter-group">
                    <select
                      value={achievementRarity}
                      onChange={(e) => setAchievementRarity(e.target.value)}
                    >
                      {RARITIES.map(r => (
                        <option key={r.value} value={r.value}>{r.label}</option>
                      ))}
                    </select>
                  </div>
                  <div className="filter-group">
                    <select
                      value={achievementSort}
                      onChange={(e) => setAchievementSort(e.target.value)}
                    >
                      {SORT_OPTIONS.map(s => (
                        <option key={s.value} value={s.value}>{s.label}</option>
                      ))}
                    </select>
                  </div>
                </div>
                <div className="ach-filter-meta">
                  <label className="checkbox-label">
                    <input
                      type="checkbox"
                      checked={showUnlockedOnly}
                      onChange={(e) => setShowUnlockedOnly(e.target.checked)}
                    />
                    <span>Unlocked only</span>
                  </label>
                  <span className="ach-total-count">
                    {filteredAchievements.length} result{filteredAchievements.length !== 1 ? "s" : ""}
                    {gamificationProgress
                      ? ` (${gamificationProgress.achievements.unlocked} unlocked)`
                      : ""}
                  </span>
                </div>
              </div>

              {/* Achievement Grid */}
              {loadingAchievements ? (
                <div className="achievements-loading">
                  <div className="spinner" />
                  <p>Loading achievements...</p>
                </div>
              ) : pagedAchievements.length === 0 ? (
                <div className="empty-state">
                  <p>No achievements found</p>
                  <p className="empty-hint">Try adjusting your filters</p>
                </div>
              ) : (
                <div className="achievements-card-grid">
                  {pagedAchievements.map(a => (
                    <AchievementCard key={a.id} achievement={a} />
                  ))}
                </div>
              )}

              {/* Pagination */}
              {totalPages > 1 && (
                <div className="ach-pagination">
                  <button
                    className="pagination-btn"
                    disabled={achievementPage <= 1}
                    onClick={() => goToPage(achievementPage - 1)}
                  >
                    Prev
                  </button>
                  {pageNumbers.map(p => (
                    <button
                      key={p}
                      className={`pagination-btn pagination-num ${p === achievementPage ? "pagination-active" : ""}`}
                      onClick={() => goToPage(p)}
                    >
                      {p}
                    </button>
                  ))}
                  <button
                    className="pagination-btn"
                    disabled={achievementPage >= totalPages}
                    onClick={() => goToPage(achievementPage + 1)}
                  >
                    Next
                  </button>
                </div>
              )}
            </div>
          ) : null}
        </div>
      </div>
    </div>
  );
}
