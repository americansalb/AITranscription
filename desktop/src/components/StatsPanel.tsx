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
  AchievementItem,
  AchievementsResponse,
  DetailedStatsResponse,
  GamificationProgress,
  AchievementListResponse,
  LeaderboardResponse,
  isLoggedIn,
} from "../lib/api";
import { AchievementCard } from "./gamification/AchievementCard";
import { XPBar } from "./gamification/XPBar";
import {
  getWordEquivalent,
  getTimeSavedEquivalent,
  getAudioEquivalent,
} from "../lib/statsEquivalents";
import { generateInsights, Insight } from "../lib/insightGenerator";
import {
  AnimatedCounter,
  TrendIndicator,
  TrendBadge,
  StreakFlame,
  TodayProgress,
  HeatMap,
  InsightCard,
  HourlyActivityChart,
  DayOfWeekChart,
  MonthlyTrendChart,
  ContextDonutChart,
  WordLengthChart,
} from "./stats";
import { Confetti } from "./Confetti";

interface StatsPanelProps {
  onClose: () => void;
  refreshTrigger?: number;
}

type TabType = "story" | "insights" | "history" | "achievements";

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

// Icon mapping for achievements
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

const defaultIcon = <svg width="24" height="24" viewBox="0 0 24 24" fill="currentColor"><path d="M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2zm-2 15l-5-5 1.41-1.41L10 14.17l7.59-7.59L19 8l-9 9z"/></svg>;

function getAchievementIcon(iconName: string): JSX.Element | string {
  // Check if iconName is an emoji (backend sends emojis like "🚀", "📝", etc.)
  // Emojis typically have code points > 127 or are multi-char sequences
  if (iconName && (iconName.codePointAt(0) || 0) > 127) {
    return iconName; // Return emoji directly
  }
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

export function StatsPanel({ onClose, refreshTrigger }: StatsPanelProps) {
  const [stats, setStats] = useState<UserStats | null>(null);
  const [detailedStats, setDetailedStats] = useState<DetailedStatsResponse | null>(null);
  const [transcripts, setTranscripts] = useState<TranscriptItem[]>([]);
  const [achievements, setAchievements] = useState<AchievementsResponse | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [activeTab, setActiveTab] = useState<TabType>("story");
  const [expandedTranscript, setExpandedTranscript] = useState<number | null>(null);
  const [editingWpm, setEditingWpm] = useState(false);
  const [wpmValue, setWpmValue] = useState("");
  const [expandedCategories, setExpandedCategories] = useState<Set<string>>(new Set(['Words', 'Streak', 'Transcriptions']));
  const [showConfetti, setShowConfetti] = useState(false);
  const [prevUnlockedCount, setPrevUnlockedCount] = useState<number | null>(null);

  // Gamification state
  const [gamificationProgress, setGamificationProgress] = useState<GamificationProgress | null>(null);
  const [gamificationAchievements, setGamificationAchievements] = useState<AchievementListResponse | null>(null);
  const [achievementPage, setAchievementPage] = useState(1); // 1-indexed for API
  const [achievementCategory, setAchievementCategory] = useState<string>("all");
  const [achievementRarity, setAchievementRarity] = useState<string>("all");
  const [showUnlockedOnly, setShowUnlockedOnly] = useState(false);
  const [loadingAchievements, setLoadingAchievements] = useState(false);
  const [leaderboard, setLeaderboard] = useState<LeaderboardResponse | null>(null);
  const [leaderboardMetric, setLeaderboardMetric] = useState<"lifetime_xp" | "achievements" | "words">("lifetime_xp");

  const loadData = useCallback(async () => {
    if (!isLoggedIn()) {
      setError("Please log in to view statistics");
      setLoading(false);
      return;
    }

    setLoading(true);
    setError(null);

    try {
      const [statsData, transcriptsData, achievementsData, detailedData, gamificationData] = await Promise.all([
        getUserStats(),
        getTranscriptHistory(0, 100),
        getUserAchievements(),
        getDetailedStats(),
        getGamificationProgress().catch(() => null), // Graceful fallback if not available
      ]);
      setStats(statsData);
      setTranscripts(transcriptsData);
      setDetailedStats(detailedData);
      setGamificationProgress(gamificationData);

      // Check for new achievement unlocks
      if (prevUnlockedCount !== null && achievementsData.total_unlocked > prevUnlockedCount) {
        setShowConfetti(true);
      }
      setPrevUnlockedCount(achievementsData.total_unlocked);
      setAchievements(achievementsData);

      // Load initial achievements page (1-indexed)
      await loadAchievementsPage(1, achievementCategory, achievementRarity, showUnlockedOnly);

      // Load leaderboard
      try {
        const leaderboardData = await getLeaderboard(leaderboardMetric, 50, true);
        setLeaderboard(leaderboardData);
      } catch (err) {
        console.error("Failed to load leaderboard:", err);
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to load data");
    } finally {
      setLoading(false);
    }
  }, [prevUnlockedCount, achievementCategory, achievementRarity, showUnlockedOnly]);

  // Load achievements with pagination and filters
  const loadAchievementsPage = useCallback(async (
    page: number,
    category: string,
    rarity: string,
    unlockedOnly: boolean
  ) => {
    setLoadingAchievements(true);
    try {
      const response = await getGamificationAchievements({
        page_size: 50,
        page: page,
        category: category === "all" ? undefined : category as any,
        rarity: rarity === "all" ? undefined : rarity as any,
        unlocked_only: unlockedOnly ? true : undefined,
      });
      setGamificationAchievements(response);
      setAchievementPage(page);
    } catch (err) {
      console.error("Failed to load achievements:", err);
    } finally {
      setLoadingAchievements(false);
    }
  }, []);

  useEffect(() => {
    loadData();
  }, [loadData, refreshTrigger]);

  // Reload achievements when filters change
  useEffect(() => {
    if (!loading && gamificationProgress) {
      loadAchievementsPage(1, achievementCategory, achievementRarity, showUnlockedOnly);
    }
  }, [achievementCategory, achievementRarity, showUnlockedOnly, loading, gamificationProgress, loadAchievementsPage]);

  // Reload leaderboard when metric changes
  useEffect(() => {
    if (!loading) {
      getLeaderboard(leaderboardMetric, 50, true)
        .then(setLeaderboard)
        .catch(err => console.error("Failed to load leaderboard:", err));
    }
  }, [leaderboardMetric, loading]);

  // Generate narrative insights
  const narrativeInsights = useMemo((): Insight[] => {
    if (!stats) return [];
    return generateInsights(stats, detailedStats, achievements?.achievements);
  }, [stats, detailedStats, achievements]);

  // Compute heatmap data
  const heatmapData = useMemo(() => computeHeatmapData(transcripts), [transcripts]);

  // Get equivalents
  const wordEquivalent = stats ? getWordEquivalent(stats.total_words) : null;
  const timeSavedEquivalent = stats ? getTimeSavedEquivalent(stats.time_saved_seconds) : null;
  const audioMinutes = stats ? stats.total_audio_seconds / 60 : 0;
  const audioEquivalent = stats ? getAudioEquivalent(audioMinutes) : null;

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

  const toggleCategory = (category: string) => {
    setExpandedCategories(prev => {
      const next = new Set(prev);
      if (next.has(category)) {
        next.delete(category);
      } else {
        next.add(category);
      }
      return next;
    });
  };

  // Group achievements by category
  const achievementsByCategory = useMemo(() => {
    if (!achievements) return new Map<string, AchievementItem[]>();
    const map = new Map<string, AchievementItem[]>();
    achievements.achievements.forEach(a => {
      const cat = a.category.charAt(0).toUpperCase() + a.category.slice(1);
      if (!map.has(cat)) map.set(cat, []);
      map.get(cat)!.push(a);
    });
    return map;
  }, [achievements]);

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
            className={`stats-tab ${activeTab === "story" ? "active" : ""}`}
            onClick={() => setActiveTab("story")}
          >
            Your Story
          </button>
          <button
            className={`stats-tab ${activeTab === "insights" ? "active" : ""}`}
            onClick={() => setActiveTab("insights")}
          >
            Insights
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
          ) : activeTab === "story" && stats ? (
            <div className="story-container">
              {/* Hero Stats Section */}
              <div className="hero-stats-section">
                {/* Primary Hero: Time Saved */}
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

                {/* Secondary Heroes */}
                <div className="hero-secondary-grid">
                  <div className="hero-secondary">
                    <div className="hero-icon">{wordEquivalent?.icon || '📚'}</div>
                    <div className="hero-value">
                      <AnimatedCounter value={stats.total_words} />
                    </div>
                    <div className="hero-label">Total Words</div>
                    {wordEquivalent && (
                      <div className="hero-equivalent">{wordEquivalent.description}</div>
                    )}
                  </div>

                  <div className="hero-secondary">
                    <div className="hero-icon">{audioEquivalent?.icon || '🎤'}</div>
                    <div className="hero-value">
                      {Math.floor(audioMinutes / 60) > 0 && (
                        <AnimatedCounter value={Math.floor(audioMinutes / 60)} suffix="h " />
                      )}
                      <AnimatedCounter value={Math.round(audioMinutes % 60)} suffix="m" />
                    </div>
                    <div className="hero-label">Audio Time</div>
                    {audioEquivalent && (
                      <div className="hero-equivalent">{audioEquivalent.description}</div>
                    )}
                  </div>
                </div>
              </div>

              {/* Streak Display */}
              {detailedStats && (
                <StreakFlame
                  currentStreak={detailedStats.current_streak_days}
                  bestStreak={detailedStats.longest_streak_days}
                  hasActivityToday={stats.transcriptions_today > 0}
                />
              )}

              {/* Today's Progress */}
              <TodayProgress
                wordsToday={stats.words_today}
                wordsGoal={2000}
                transcriptionsToday={stats.transcriptions_today}
                transcriptionsGoal={10}
                minutesActive={Math.round(stats.time_saved_today_seconds / 60)}
                minutesGoal={30}
              />

              {/* Quick Stats Grid */}
              <div className="quick-stats-grid">
                <div className="quick-stat">
                  <div className="quick-stat-value">
                    <AnimatedCounter value={stats.words_today} />
                  </div>
                  <div className="quick-stat-label">Words today</div>
                </div>
                <div className="quick-stat">
                  <div className="quick-stat-value">
                    <AnimatedCounter value={stats.transcriptions_today} />
                  </div>
                  <div className="quick-stat-label">Transcriptions today</div>
                </div>
                <div className="quick-stat">
                  <div className="quick-stat-value">
                    <AnimatedCounter value={stats.total_transcriptions} />
                  </div>
                  <div className="quick-stat-label">Total transcriptions</div>
                </div>
                <div className="quick-stat">
                  <div className="quick-stat-value">{Math.round(stats.average_words_per_minute)}</div>
                  <div className="quick-stat-label">Avg WPM</div>
                </div>
              </div>

              {/* Week Comparison */}
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
          ) : activeTab === "insights" ? (
            <div className="insights-container">
              {/* Narrative Insight Cards */}
              {narrativeInsights.length > 0 ? (
                <div className="insights-tab">
                  <div className="insights-header">
                    <h2>Your Story</h2>
                    <p className="insights-subtitle">Discover what your data says about you</p>
                  </div>
                  <div className="insights-cards">
                    {narrativeInsights.map((insight, index) => (
                      <InsightCard key={insight.id} insight={insight} index={index} />
                    ))}
                  </div>
                </div>
              ) : (
                <div className="insights-empty">
                  <div className="empty-icon">📊</div>
                  <h3>Your Story is Being Written</h3>
                  <p>Keep transcribing to unlock personalized insights!</p>
                </div>
              )}

              {/* Activity Heatmap */}
              {heatmapData.length > 0 && (
                <div className="heatmap-section">
                  <h3 className="section-title">Activity Overview</h3>
                  <HeatMap data={heatmapData} weeks={12} />
                </div>
              )}
            </div>
          ) : activeTab === "history" ? (
            <div className="transcript-list">
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
                  <div className="achievement-summary">
                    <span className="unlocked-count">
                      {gamificationProgress.achievements.unlocked} / {gamificationProgress.achievements.total} Achievements
                    </span>
                  </div>
                </div>
              )}

              {/* Your Ranking */}
              {leaderboard?.user_rank && (
                <div className="your-ranking-section">
                  <h3 className="section-title">Your Ranking</h3>
                  <div className="ranking-card">
                    <div className="ranking-position">
                      <span className="rank-number">#{leaderboard.user_rank.rank}</span>
                      <span className="rank-total">of {leaderboard.user_rank.total_users} users</span>
                    </div>
                    <div className="ranking-details">
                      <div className="ranking-metric">
                        <span className="metric-label">
                          {leaderboard.user_rank.metric === "lifetime_xp" ? "Lifetime XP" :
                           leaderboard.user_rank.metric === "achievements" ? "Achievements" : "Total Words"}
                        </span>
                        <span className="metric-value">{leaderboard.user_rank.value.toLocaleString()}</span>
                      </div>
                      <div className="ranking-percentile">
                        Top {(100 - leaderboard.user_rank.percentile).toFixed(1)}%
                      </div>
                    </div>
                  </div>
                </div>
              )}

              {/* Leaderboard */}
              <div className="leaderboard-section">
                <div className="leaderboard-header">
                  <h3 className="section-title">Leaderboard</h3>
                  <select
                    className="leaderboard-metric-select"
                    value={leaderboardMetric}
                    onChange={(e) => setLeaderboardMetric(e.target.value as any)}
                  >
                    <option value="lifetime_xp">XP</option>
                    <option value="achievements">Achievements</option>
                    <option value="words">Words</option>
                  </select>
                </div>
                {leaderboard && leaderboard.leaderboard.length > 0 ? (
                  <div className="leaderboard-list">
                    {leaderboard.leaderboard.slice(0, 10).map((entry, index) => (
                      <div
                        key={entry.user_id}
                        className={`leaderboard-entry ${entry.user_id === leaderboard.user_rank?.user_id ? 'is-you' : ''}`}
                      >
                        <div className="leaderboard-rank">
                          {index === 0 ? '🥇' : index === 1 ? '🥈' : index === 2 ? '🥉' : `#${entry.rank}`}
                        </div>
                        <div className="leaderboard-user">
                          <span className="leaderboard-name">{entry.display_name}</span>
                          <span className="leaderboard-level">Lv.{entry.level} {entry.tier}</span>
                        </div>
                        <div className="leaderboard-value">
                          {leaderboardMetric === "lifetime_xp" ? `${(entry.lifetime_xp || 0).toLocaleString()} XP` :
                           leaderboardMetric === "achievements" ? `${entry.achievements} unlocked` :
                           `${(entry.total_words || 0).toLocaleString()} words`}
                        </div>
                      </div>
                    ))}
                  </div>
                ) : (
                  <div className="empty-state small">
                    <p>No leaderboard data yet</p>
                  </div>
                )}
              </div>

              {/* Filters */}
              <div className="achievement-filters">
                <div className="filter-group">
                  <label>Category:</label>
                  <select
                    value={achievementCategory}
                    onChange={(e) => setAchievementCategory(e.target.value)}
                  >
                    <option value="all">All Categories</option>
                    <option value="volume">Volume</option>
                    <option value="streak">Streak</option>
                    <option value="speed">Speed</option>
                    <option value="context">Context</option>
                    <option value="formality">Formality</option>
                    <option value="learning">Learning</option>
                    <option value="temporal">Temporal</option>
                    <option value="records">Records</option>
                    <option value="combo">Combinations</option>
                    <option value="special">Special</option>
                  </select>
                </div>
                <div className="filter-group">
                  <label>Rarity:</label>
                  <select
                    value={achievementRarity}
                    onChange={(e) => setAchievementRarity(e.target.value)}
                  >
                    <option value="all">All Rarities</option>
                    <option value="common">Common</option>
                    <option value="rare">Rare</option>
                    <option value="epic">Epic</option>
                    <option value="legendary">Legendary</option>
                  </select>
                </div>
                <div className="filter-group">
                  <label className="checkbox-label">
                    <input
                      type="checkbox"
                      checked={showUnlockedOnly}
                      onChange={(e) => setShowUnlockedOnly(e.target.checked)}
                    />
                    Unlocked only
                  </label>
                </div>
              </div>

              {/* Achievements Grid */}
              {loadingAchievements ? (
                <div className="achievements-loading">
                  <div className="spinner" />
                  <p>Loading achievements...</p>
                </div>
              ) : gamificationAchievements && gamificationAchievements.achievements.length > 0 ? (
                <>
                  {/* Category summary */}
                  <div className="achievement-category-summary">
                    <span className="summary-text">
                      Showing {gamificationAchievements.achievements.length} of {gamificationAchievements.total} achievements
                      {achievementCategory !== "all" && ` in ${achievementCategory}`}
                      {achievementRarity !== "all" && ` (${achievementRarity})`}
                    </span>
                  </div>

                  <div className="achievements-grid gamification">
                    {gamificationAchievements.achievements.map((achievement) => (
                      <AchievementCard
                        key={achievement.id}
                        achievement={achievement}
                        compact={false}
                      />
                    ))}
                  </div>

                  {/* Pagination - 1-indexed */}
                  {gamificationAchievements.total_pages > 1 && (
                    <div className="achievement-pagination">
                      <button
                        className="pagination-btn"
                        onClick={() => loadAchievementsPage(
                          achievementPage - 1,
                          achievementCategory,
                          achievementRarity,
                          showUnlockedOnly
                        )}
                        disabled={achievementPage <= 1}
                      >
                        ← Prev
                      </button>
                      <span className="pagination-info">
                        Page {achievementPage} of {gamificationAchievements.total_pages}
                      </span>
                      <button
                        className="pagination-btn"
                        onClick={() => loadAchievementsPage(
                          achievementPage + 1,
                          achievementCategory,
                          achievementRarity,
                          showUnlockedOnly
                        )}
                        disabled={achievementPage >= gamificationAchievements.total_pages}
                      >
                        Next →
                      </button>
                    </div>
                  )}
                </>
              ) : (
                <div className="empty-state">
                  <p>No achievements found</p>
                  <p className="empty-hint">Try changing the filters above</p>
                </div>
              )}
            </div>
          ) : null}
        </div>
      </div>
    </div>
  );
}
