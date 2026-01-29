import { useState, useEffect, useCallback } from "react";
import {
  LineChart,
  Line,
  BarChart,
  Bar,
  PieChart,
  Pie,
  Cell,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  Legend,
  ResponsiveContainer,
  Area,
  AreaChart,
} from "recharts";
import {
  getDetailedStats,
  getUserStats,
  getTranscriptHistory,
  DetailedStatsResponse,
  UserStats,
  TranscriptItem,
  isLoggedIn,
} from "../lib/api";

interface AnalyticsDashboardProps {
  onClose: () => void;
  refreshTrigger?: number;
}

const COLORS = {
  primary: "#6366f1",
  success: "#22c55e",
  warning: "#f59e0b",
  error: "#ef4444",
  info: "#3b82f6",
  purple: "#a855f7",
  pink: "#ec4899",
  cyan: "#06b6d4",
  teal: "#14b8a6",
};

const PIE_COLORS = [
  COLORS.primary,
  COLORS.success,
  COLORS.warning,
  COLORS.info,
  COLORS.purple,
  COLORS.pink,
  COLORS.cyan,
  COLORS.teal,
];

export function AnalyticsDashboard({ onClose, refreshTrigger }: AnalyticsDashboardProps) {
  const [detailedStats, setDetailedStats] = useState<DetailedStatsResponse | null>(null);
  const [basicStats, setBasicStats] = useState<UserStats | null>(null);
  const [_transcripts, _setTranscripts] = useState<TranscriptItem[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [activeTab, setActiveTab] = useState<"overview" | "activity" | "insights">("overview");
  const [_expandedTranscript, _setExpandedTranscript] = useState<number | null>(null);

  const loadData = useCallback(async () => {
    if (!isLoggedIn()) {
      setError("Please log in to view analytics");
      setLoading(false);
      return;
    }

    setLoading(true);
    setError(null);

    try {
      const [detailed, basic, transcriptsData] = await Promise.all([
        getDetailedStats(),
        getUserStats(),
        getTranscriptHistory(0, 100),
      ]);
      setDetailedStats(detailed);
      setBasicStats(basic);
      _setTranscripts(transcriptsData);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to load analytics");
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    loadData();
  }, [loadData, refreshTrigger]);

  const formatDuration = (seconds: number): string => {
    const hours = Math.floor(seconds / 3600);
    const mins = Math.floor((seconds % 3600) / 60);
    const secs = Math.floor(seconds % 60);
    if (hours > 0) return `${hours}h ${mins}m`;
    if (mins > 0) return `${mins}m ${secs}s`;
    return `${secs}s`;
  };

  const formatTimeSaved = (seconds: number): string => {
    if (seconds < 60) return `${seconds}s`;
    if (seconds < 3600) {
      const mins = Math.floor(seconds / 60);
      const secs = seconds % 60;
      return secs > 0 ? `${mins}m ${secs}s` : `${mins}m`;
    }
    const hours = Math.floor(seconds / 3600);
    const mins = Math.floor((seconds % 3600) / 60);
    return mins > 0 ? `${hours}h ${mins}m` : `${hours}h`;
  };

  const renderGrowthIndicator = (value: number) => {
    if (value === 0) return <span className="growth-neutral">—</span>;
    const isPositive = value > 0;
    return (
      <span className={`growth-indicator ${isPositive ? "positive" : "negative"}`}>
        {isPositive ? "↑" : "↓"} {Math.abs(value).toFixed(1)}%
      </span>
    );
  };

  if (loading) {
    return (
      <div className="analytics-overlay" onClick={onClose}>
        <div className="analytics-dashboard" onClick={(e) => e.stopPropagation()}>
          <div className="analytics-loading">
            <div className="spinner" />
            <p>Loading analytics...</p>
          </div>
        </div>
      </div>
    );
  }

  if (error || !detailedStats || !basicStats) {
    return (
      <div className="analytics-overlay" onClick={onClose}>
        <div className="analytics-dashboard" onClick={(e) => e.stopPropagation()}>
          <div className="analytics-error">
            <p>{error || "Failed to load analytics"}</p>
            <button onClick={loadData}>Retry</button>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="analytics-overlay" onClick={onClose}>
      <div className="analytics-dashboard" onClick={(e) => e.stopPropagation()}>
        <div className="analytics-header">
          <h2>Analytics Dashboard</h2>
          <button className="close-btn" onClick={onClose}>
            <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              <path d="M18 6L6 18M6 6l12 12" />
            </svg>
          </button>
        </div>

        <div className="analytics-tabs">
          <button
            className={`analytics-tab ${activeTab === "overview" ? "active" : ""}`}
            onClick={() => setActiveTab("overview")}
          >
            Overview
          </button>
          <button
            className={`analytics-tab ${activeTab === "activity" ? "active" : ""}`}
            onClick={() => setActiveTab("activity")}
          >
            Activity
          </button>
          <button
            className={`analytics-tab ${activeTab === "insights" ? "active" : ""}`}
            onClick={() => setActiveTab("insights")}
          >
            Insights
          </button>
        </div>

        <div className="analytics-content">
          {activeTab === "overview" && (
            <div className="overview-tab">
              <div className="metrics-grid">
                <div className="metric-card highlight">
                  <div className="metric-icon">⚡</div>
                  <div className="metric-content">
                    <div className="metric-value">{formatTimeSaved(basicStats.time_saved_seconds)}</div>
                    <div className="metric-label">Time Saved</div>
                    <div className="metric-subtext">vs typing at {basicStats.typing_wpm} WPM</div>
                  </div>
                </div>

                <div className="metric-card">
                  <div className="metric-icon">📝</div>
                  <div className="metric-content">
                    <div className="metric-value">{basicStats.total_transcriptions.toLocaleString()}</div>
                    <div className="metric-label">Total Transcriptions</div>
                    {renderGrowthIndicator(detailedStats.growth.transcriptions_wow_change)}
                  </div>
                </div>

                <div className="metric-card">
                  <div className="metric-icon">💬</div>
                  <div className="metric-content">
                    <div className="metric-value">{basicStats.total_words.toLocaleString()}</div>
                    <div className="metric-label">Total Words</div>
                    {renderGrowthIndicator(detailedStats.growth.words_wow_change)}
                  </div>
                </div>

                <div className="metric-card">
                  <div className="metric-icon">🎤</div>
                  <div className="metric-content">
                    <div className="metric-value">{formatDuration(basicStats.total_audio_seconds)}</div>
                    <div className="metric-label">Audio Time</div>
                    <div className="metric-subtext">{(basicStats.total_audio_seconds / 60).toFixed(1)} min total</div>
                  </div>
                </div>

                <div className="metric-card">
                  <div className="metric-icon">🚀</div>
                  <div className="metric-content">
                    <div className="metric-value">{basicStats.average_words_per_minute}</div>
                    <div className="metric-label">Avg Speaking WPM</div>
                    <div className="metric-subtext">{detailedStats.productivity.efficiency_score.toFixed(1)} words/sec audio</div>
                  </div>
                </div>

                <div className="metric-card">
                  <div className="metric-icon">🔥</div>
                  <div className="metric-content">
                    <div className="metric-value">{detailedStats.current_streak_days}</div>
                    <div className="metric-label">Current Streak</div>
                    <div className="metric-subtext">Longest: {detailedStats.longest_streak_days} days</div>
                  </div>
                </div>
              </div>

              <div className="chart-section">
                <h3>Activity Trend (Last 30 Days)</h3>
                <ResponsiveContainer width="100%" height={250}>
                  <AreaChart data={detailedStats.daily_activity}>
                    <defs>
                      <linearGradient id="colorWords" x1="0" y1="0" x2="0" y2="1">
                        <stop offset="5%" stopColor={COLORS.primary} stopOpacity={0.3}/>
                        <stop offset="95%" stopColor={COLORS.primary} stopOpacity={0}/>
                      </linearGradient>
                    </defs>
                    <CartesianGrid strokeDasharray="3 3" stroke="#333" />
                    <XAxis
                      dataKey="date"
                      tickFormatter={(value) => new Date(value).toLocaleDateString(undefined, { month: 'short', day: 'numeric' })}
                      stroke="#888"
                    />
                    <YAxis stroke="#888" />
                    <Tooltip
                      contentStyle={{ backgroundColor: '#1a1a1a', border: '1px solid #333' }}
                      labelFormatter={(value) => new Date(value).toLocaleDateString()}
                    />
                    <Area
                      type="monotone"
                      dataKey="words"
                      stroke={COLORS.primary}
                      fillOpacity={1}
                      fill="url(#colorWords)"
                    />
                  </AreaChart>
                </ResponsiveContainer>
              </div>

              <div className="stats-comparison">
                <h3>This Week vs Last Week</h3>
                <div className="comparison-grid">
                  <div className="comparison-item">
                    <div className="comparison-label">Words</div>
                    <div className="comparison-values">
                      <span className="current">{detailedStats.growth.last_week_words.toLocaleString()}</span>
                      <span className="vs">vs</span>
                      <span className="previous">{detailedStats.growth.prev_week_words.toLocaleString()}</span>
                    </div>
                    {renderGrowthIndicator(detailedStats.growth.words_wow_change)}
                  </div>

                  <div className="comparison-item">
                    <div className="comparison-label">Transcriptions</div>
                    <div className="comparison-values">
                      <span className="current">{detailedStats.transcriptions_this_week}</span>
                      <span className="vs">vs</span>
                      <span className="previous">{Math.max(0, detailedStats.transcriptions_this_week - (detailedStats.growth.transcriptions_wow_change / 100 * detailedStats.transcriptions_this_week))}</span>
                    </div>
                    {renderGrowthIndicator(detailedStats.growth.transcriptions_wow_change)}
                  </div>
                </div>
              </div>
            </div>
          )}

          {activeTab === "activity" && (
            <div className="activity-tab">
              <div className="chart-section">
                <h3>Monthly Trends (Last 12 Months)</h3>
                <ResponsiveContainer width="100%" height={300}>
                  <BarChart data={detailedStats.monthly_trends}>
                    <CartesianGrid strokeDasharray="3 3" stroke="#333" />
                    <XAxis dataKey="month_label" stroke="#888" />
                    <YAxis stroke="#888" />
                    <Tooltip
                      contentStyle={{ backgroundColor: '#1a1a1a', border: '1px solid #333' }}
                    />
                    <Legend />
                    <Bar dataKey="words" fill={COLORS.primary} name="Words" />
                    <Bar dataKey="transcriptions" fill={COLORS.success} name="Transcriptions" />
                  </BarChart>
                </ResponsiveContainer>
              </div>

              <div className="chart-section">
                <h3>Activity by Hour of Day</h3>
                <ResponsiveContainer width="100%" height={250}>
                  <LineChart data={detailedStats.hourly_activity}>
                    <CartesianGrid strokeDasharray="3 3" stroke="#333" />
                    <XAxis
                      dataKey="hour"
                      tickFormatter={(value) => `${value}:00`}
                      stroke="#888"
                    />
                    <YAxis stroke="#888" />
                    <Tooltip
                      contentStyle={{ backgroundColor: '#1a1a1a', border: '1px solid #333' }}
                      labelFormatter={(value) => `Hour: ${value}:00`}
                    />
                    <Legend />
                    <Line type="monotone" dataKey="transcriptions" stroke={COLORS.success} strokeWidth={2} name="Transcriptions" />
                    <Line type="monotone" dataKey="words" stroke={COLORS.primary} strokeWidth={2} name="Words" />
                  </LineChart>
                </ResponsiveContainer>
              </div>

              <div className="chart-section">
                <h3>Activity by Day of Week</h3>
                <ResponsiveContainer width="100%" height={250}>
                  <BarChart data={detailedStats.day_of_week_breakdown}>
                    <CartesianGrid strokeDasharray="3 3" stroke="#333" />
                    <XAxis dataKey="day" stroke="#888" />
                    <YAxis stroke="#888" />
                    <Tooltip
                      contentStyle={{ backgroundColor: '#1a1a1a', border: '1px solid #333' }}
                    />
                    <Legend />
                    <Bar dataKey="transcriptions" fill={COLORS.info} name="Transcriptions" />
                    <Bar dataKey="words" fill={COLORS.purple} name="Words" />
                  </BarChart>
                </ResponsiveContainer>
              </div>

              <div className="best-times">
                <h3>Peak Performance</h3>
                <div className="peak-stats">
                  <div className="peak-item">
                    <div className="peak-label">Most Productive Hour</div>
                    <div className="peak-value">{detailedStats.productivity.peak_hour_label}</div>
                  </div>
                  <div className="peak-item">
                    <div className="peak-label">Most Productive Day</div>
                    <div className="peak-value">{detailedStats.productivity.peak_day}</div>
                  </div>
                  <div className="peak-item">
                    <div className="peak-label">Busiest Week Ever</div>
                    <div className="peak-value">
                      {detailedStats.productivity.busiest_week_ever || "N/A"}
                      <span className="peak-subtext">{detailedStats.productivity.busiest_week_words} words</span>
                    </div>
                  </div>
                </div>
              </div>
            </div>
          )}

          {activeTab === "insights" && (
            <div className="insights-tab">
              <div className="insights-row">
                <div className="chart-section">
                  <h3>Context Breakdown</h3>
                  <ResponsiveContainer width="100%" height={300}>
                    <PieChart>
                      <Pie
                        data={detailedStats.context_breakdown}
                        cx="50%"
                        cy="50%"
                        labelLine={false}
                        label={(entry: any) => `${entry.context}: ${entry.percentage}%`}
                        outerRadius={80}
                        fill="#8884d8"
                        dataKey="count"
                      >
                        {detailedStats.context_breakdown.map((_entry, index) => (
                          <Cell key={`cell-${index}`} fill={PIE_COLORS[index % PIE_COLORS.length]} />
                        ))}
                      </Pie>
                      <Tooltip contentStyle={{ backgroundColor: '#1a1a1a', border: '1px solid #333' }} />
                    </PieChart>
                  </ResponsiveContainer>
                </div>

                <div className="chart-section">
                  <h3>Formality Breakdown</h3>
                  <ResponsiveContainer width="100%" height={300}>
                    <PieChart>
                      <Pie
                        data={detailedStats.formality_breakdown}
                        cx="50%"
                        cy="50%"
                        labelLine={false}
                        label={(entry: any) => `${entry.formality}: ${entry.percentage}%`}
                        outerRadius={80}
                        fill="#8884d8"
                        dataKey="count"
                      >
                        {detailedStats.formality_breakdown.map((_entry, index) => (
                          <Cell key={`cell-${index}`} fill={PIE_COLORS[index % PIE_COLORS.length]} />
                        ))}
                      </Pie>
                      <Tooltip contentStyle={{ backgroundColor: '#1a1a1a', border: '1px solid #333' }} />
                    </PieChart>
                  </ResponsiveContainer>
                </div>
              </div>

              <div className="chart-section">
                <h3>Transcription Length Distribution</h3>
                <ResponsiveContainer width="100%" height={250}>
                  <BarChart data={detailedStats.word_length_distribution}>
                    <CartesianGrid strokeDasharray="3 3" stroke="#333" />
                    <XAxis dataKey="range_label" stroke="#888" />
                    <YAxis stroke="#888" />
                    <Tooltip
                      contentStyle={{ backgroundColor: '#1a1a1a', border: '1px solid #333' }}
                    />
                    <Bar dataKey="count" fill={COLORS.teal} name="Transcriptions" />
                  </BarChart>
                </ResponsiveContainer>
              </div>

              <div className="insights-metrics">
                <h3>Detailed Metrics</h3>
                <div className="metrics-list">
                  <div className="metrics-item">
                    <span className="metrics-label">Average Session Words</span>
                    <span className="metrics-value">{detailedStats.productivity.avg_session_words.toFixed(0)}</span>
                  </div>
                  <div className="metrics-item">
                    <span className="metrics-label">Average Session Duration</span>
                    <span className="metrics-value">{formatDuration(detailedStats.productivity.avg_session_duration_seconds)}</span>
                  </div>
                  <div className="metrics-item">
                    <span className="metrics-label">Total Characters</span>
                    <span className="metrics-value">{detailedStats.total_characters.toLocaleString()}</span>
                  </div>
                  <div className="metrics-item">
                    <span className="metrics-label">Speech Efficiency</span>
                    <span className="metrics-value">{detailedStats.productivity.efficiency_score.toFixed(2)} words/sec</span>
                  </div>
                </div>
              </div>
            </div>
          )}

        </div>
      </div>
    </div>
  );
}
