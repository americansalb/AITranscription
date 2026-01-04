import { useState, useEffect, useCallback } from "react";
import {
  login,
  signup,
  logout,
  getCurrentUser,
  getUserStats,
  getDetailedStats,
  isLoggedIn,
  UserResponse,
  UserStatsResponse,
  DetailedStatsResponse,
  ApiError,
} from "../lib/api";
import type { HistoryEntry } from "../App";
import { copyToClipboard } from "../lib/clipboard";

interface SettingsProps {
  onClose: () => void;
  refreshTrigger?: number;
  history?: HistoryEntry[];
  onClearHistory?: () => void;
  onHotkeyChange?: (hotkey: string) => void;
}

type SettingsTab = "account" | "stats" | "history" | "dictionary" | "preferences";

export function Settings({ onClose, refreshTrigger = 0, history = [], onClearHistory, onHotkeyChange }: SettingsProps) {
  const [activeTab, setActiveTab] = useState<SettingsTab>("account");
  const [user, setUser] = useState<UserResponse | null>(null);
  const [stats, setStats] = useState<UserStatsResponse | null>(null);
  const [loading, setLoading] = useState(true);

  // Fetch user and stats on mount and when refreshTrigger changes
  useEffect(() => {
    if (isLoggedIn()) {
      Promise.all([getCurrentUser(), getUserStats()])
        .then(([userData, statsData]) => {
          setUser(userData);
          setStats(statsData);
        })
        .catch(() => {
          setUser(null);
          setStats(null);
        })
        .finally(() => setLoading(false));
    } else {
      setLoading(false);
    }
  }, [refreshTrigger]);

  const handleLogout = useCallback(() => {
    logout();
    setUser(null);
  }, []);

  return (
    <div className="settings-overlay" onClick={onClose}>
      <div className="settings-modal" onClick={(e) => e.stopPropagation()}>
        <header className="settings-header">
          <h2>Settings</h2>
          <button className="close-btn" onClick={onClose}>
            <svg
              width="20"
              height="20"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              strokeWidth="2"
            >
              <line x1="18" y1="6" x2="6" y2="18" />
              <line x1="6" y1="6" x2="18" y2="18" />
            </svg>
          </button>
        </header>

        <nav className="settings-tabs">
          <button
            className={activeTab === "account" ? "active" : ""}
            onClick={() => setActiveTab("account")}
          >
            Account
          </button>
          <button
            className={activeTab === "stats" ? "active" : ""}
            onClick={() => setActiveTab("stats")}
          >
            Stats
          </button>
          <button
            className={activeTab === "history" ? "active" : ""}
            onClick={() => setActiveTab("history")}
          >
            History
          </button>
          <button
            className={activeTab === "dictionary" ? "active" : ""}
            onClick={() => setActiveTab("dictionary")}
          >
            Dictionary
          </button>
          <button
            className={activeTab === "preferences" ? "active" : ""}
            onClick={() => setActiveTab("preferences")}
          >
            Preferences
          </button>
        </nav>

        <div className="settings-content">
          {loading ? (
            <div className="loading">Loading...</div>
          ) : activeTab === "account" ? (
            user ? (
              <AccountInfo user={user} stats={stats} onLogout={handleLogout} />
            ) : (
              <AuthForm onSuccess={setUser} />
            )
          ) : activeTab === "stats" ? (
            user ? (
              <StatsPanel refreshTrigger={refreshTrigger} />
            ) : (
              <div className="auth-required">
                Please log in to view your statistics.
              </div>
            )
          ) : activeTab === "history" ? (
            <HistoryPanel history={history} onClear={onClearHistory} />
          ) : activeTab === "dictionary" ? (
            user ? (
              <DictionaryManager />
            ) : (
              <div className="auth-required">
                Please log in to manage your custom dictionary.
              </div>
            )
          ) : (
            <Preferences onHotkeyChange={onHotkeyChange} />
          )}
        </div>
      </div>
    </div>
  );
}

// Format seconds to human readable duration
function formatDuration(seconds: number): string {
  if (seconds < 60) return `${Math.round(seconds)}s`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m ${Math.round(seconds % 60)}s`;
  const hours = Math.floor(seconds / 3600);
  const mins = Math.floor((seconds % 3600) / 60);
  return `${hours}h ${mins}m`;
}

// Account info when logged in
function AccountInfo({
  user,
  stats,
  onLogout,
}: {
  user: UserResponse;
  stats: UserStatsResponse | null;
  onLogout: () => void;
}) {
  const tierLabels: Record<string, string> = {
    access: "Access (Accessibility)",
    standard: "Standard",
    enterprise: "Enterprise",
    developer: "Developer",
  };

  return (
    <div className="account-info">
      <div className="user-avatar">
        {user.full_name?.[0]?.toUpperCase() || user.email[0].toUpperCase()}
      </div>
      <h3>{user.full_name || "User"}</h3>
      <p className="email">{user.email}</p>
      <div className="tier-badge">{tierLabels[user.tier] || user.tier}</div>
      {user.accessibility_verified && (
        <div className="verified-badge">Accessibility Verified</div>
      )}

      {stats && (
        <div className="user-stats">
          <h4>Your Statistics</h4>
          <div className="stats-grid">
            <div className="stat-item">
              <span className="stat-value">{stats.total_transcriptions}</span>
              <span className="stat-label">Transcriptions</span>
            </div>
            <div className="stat-item">
              <span className="stat-value">{stats.total_words.toLocaleString()}</span>
              <span className="stat-label">Words</span>
            </div>
            <div className="stat-item">
              <span className="stat-value">{formatDuration(stats.total_audio_seconds)}</span>
              <span className="stat-label">Audio Time</span>
            </div>
            <div className="stat-item">
              <span className="stat-value">{stats.transcriptions_today}</span>
              <span className="stat-label">Today</span>
            </div>
          </div>
        </div>
      )}

      <button className="logout-btn" onClick={onLogout}>
        Log Out
      </button>
    </div>
  );
}

// Auth form for login/signup
function AuthForm({ onSuccess }: { onSuccess: (user: UserResponse) => void }) {
  const [mode, setMode] = useState<"login" | "signup">("login");
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [fullName, setFullName] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setError(null);
    setLoading(true);

    try {
      if (mode === "login") {
        await login(email, password);
      } else {
        await signup(email, password, fullName || undefined);
      }
      const user = await getCurrentUser();
      onSuccess(user);
    } catch (err) {
      const message =
        err instanceof ApiError ? err.detail || err.message : "An error occurred";
      setError(message);
    } finally {
      setLoading(false);
    }
  };

  return (
    <form className="auth-form" onSubmit={handleSubmit}>
      <h3>{mode === "login" ? "Log In" : "Create Account"}</h3>

      {error && <div className="form-error">{error}</div>}

      {mode === "signup" && (
        <div className="form-field">
          <label htmlFor="fullName">Full Name</label>
          <input
            id="fullName"
            type="text"
            value={fullName}
            onChange={(e) => setFullName(e.target.value)}
            placeholder="Your name"
          />
        </div>
      )}

      <div className="form-field">
        <label htmlFor="email">Email</label>
        <input
          id="email"
          type="email"
          value={email}
          onChange={(e) => setEmail(e.target.value)}
          placeholder="you@example.com"
          required
        />
      </div>

      <div className="form-field">
        <label htmlFor="password">Password</label>
        <input
          id="password"
          type="password"
          value={password}
          onChange={(e) => setPassword(e.target.value)}
          placeholder="••••••••"
          minLength={6}
          required
        />
      </div>

      <button type="submit" className="submit-btn" disabled={loading}>
        {loading ? "..." : mode === "login" ? "Log In" : "Create Account"}
      </button>

      <p className="auth-switch">
        {mode === "login" ? (
          <>
            Don't have an account?{" "}
            <button type="button" onClick={() => setMode("signup")}>
              Sign up
            </button>
          </>
        ) : (
          <>
            Already have an account?{" "}
            <button type="button" onClick={() => setMode("login")}>
              Log in
            </button>
          </>
        )}
      </p>
    </form>
  );
}

// Dictionary manager placeholder
function DictionaryManager() {
  const [words, setWords] = useState<string[]>([]);
  const [newWord, setNewWord] = useState("");

  return (
    <div className="dictionary-manager">
      <h3>Custom Dictionary</h3>
      <p className="hint">
        Add words and phrases that should be preserved exactly in transcriptions.
      </p>

      <div className="add-word-form">
        <input
          type="text"
          value={newWord}
          onChange={(e) => setNewWord(e.target.value)}
          placeholder="Add a word or phrase..."
        />
        <button
          onClick={() => {
            if (newWord.trim()) {
              setWords([...words, newWord.trim()]);
              setNewWord("");
            }
          }}
        >
          Add
        </button>
      </div>

      <ul className="word-list">
        {words.length === 0 ? (
          <li className="empty">No custom words yet</li>
        ) : (
          words.map((word, i) => (
            <li key={i}>
              <span>{word}</span>
              <button
                className="remove-btn"
                onClick={() => setWords(words.filter((_, j) => j !== i))}
              >
                ×
              </button>
            </li>
          ))
        )}
      </ul>
    </div>
  );
}

// Hotkey options for push-to-talk
const HOTKEY_OPTIONS = [
  { value: "Alt+D", label: "Alt+D", macLabel: "Option+D" },
  { value: "Alt+Space", label: "Alt+Space", macLabel: "Option+Space" },
  { value: "Alt+R", label: "Alt+R", macLabel: "Option+R" },
  { value: "CommandOrControl+Shift+Space", label: "Ctrl+Shift+Space", macLabel: "Cmd+Shift+Space" },
  { value: "CommandOrControl+Shift+D", label: "Ctrl+Shift+D", macLabel: "Cmd+Shift+D" },
  { value: "F9", label: "F9", macLabel: "F9" },
  { value: "F10", label: "F10", macLabel: "F10" },
];

// Get stored hotkey from localStorage
export function getStoredHotkey(): string {
  try {
    return localStorage.getItem("scribe_hotkey") || "Alt+D";
  } catch {
    return "Alt+D";
  }
}

// Save hotkey to localStorage
function saveHotkey(hotkey: string) {
  try {
    localStorage.setItem("scribe_hotkey", hotkey);
  } catch {
    // Ignore storage errors
  }
}

// App preferences
function Preferences({ onHotkeyChange }: { onHotkeyChange?: (hotkey: string) => void }) {
  const [autoPaste, setAutoPaste] = useState(true);
  const [playSound, setPlaySound] = useState(true);
  const [hotkey, setHotkey] = useState(() => getStoredHotkey());
  const isMac = navigator.platform.includes("Mac");

  const handleHotkeyChange = (newHotkey: string) => {
    setHotkey(newHotkey);
    saveHotkey(newHotkey);
    onHotkeyChange?.(newHotkey);
  };

  return (
    <div className="preferences">
      <h3>Preferences</h3>

      <label className="toggle-setting">
        <span>Auto-paste after transcription</span>
        <input
          type="checkbox"
          checked={autoPaste}
          onChange={(e) => setAutoPaste(e.target.checked)}
        />
        <span className="toggle-switch" />
      </label>

      <label className="toggle-setting">
        <span>Play sound when complete</span>
        <input
          type="checkbox"
          checked={playSound}
          onChange={(e) => setPlaySound(e.target.checked)}
        />
        <span className="toggle-switch" />
      </label>

      <div className="hotkey-setting">
        <span>Push-to-talk hotkey</span>
        <select
          value={hotkey}
          onChange={(e) => handleHotkeyChange(e.target.value)}
          className="hotkey-select"
        >
          {HOTKEY_OPTIONS.map((opt) => (
            <option key={opt.value} value={opt.value}>
              {isMac ? opt.macLabel : opt.label}
            </option>
          ))}
        </select>
      </div>

      <p className="hotkey-hint">
        Changes take effect immediately. Hold the hotkey to record, release to transcribe.
      </p>
    </div>
  );
}

// Beautiful Stats Panel
function StatsPanel({ refreshTrigger = 0 }: { refreshTrigger?: number }) {
  const [stats, setStats] = useState<DetailedStatsResponse | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  // Fetch stats on mount and whenever refreshTrigger changes
  useEffect(() => {
    setLoading(true);
    getDetailedStats()
      .then(setStats)
      .catch((err) => setError(err.message))
      .finally(() => setLoading(false));
  }, [refreshTrigger]);

  if (loading) {
    return <div className="loading">Loading statistics...</div>;
  }

  if (error) {
    return <div className="form-error">Failed to load stats: {error}</div>;
  }

  if (!stats) {
    return <div className="auth-required">No statistics available yet.</div>;
  }

  const contextLabels: Record<string, string> = {
    general: "General",
    email: "Email",
    slack: "Slack / Chat",
    document: "Document",
    code: "Code",
  };

  const contextColors: Record<string, string> = {
    general: "#6366f1",
    email: "#22c55e",
    slack: "#f59e0b",
    document: "#3b82f6",
    code: "#ec4899",
  };

  // Format time saved
  const formatTimeSaved = (minutes: number) => {
    if (minutes < 60) return `${Math.round(minutes)} min`;
    const hours = Math.floor(minutes / 60);
    const mins = Math.round(minutes % 60);
    return mins > 0 ? `${hours}h ${mins}m` : `${hours} hours`;
  };

  // Format date for display
  const formatDate = (dateStr: string) => {
    const date = new Date(dateStr);
    return date.toLocaleDateString("en-US", { weekday: "short", month: "short", day: "numeric" });
  };

  // Get day name from date string
  const getDayName = (dateStr: string) => {
    const date = new Date(dateStr);
    const today = new Date();
    const yesterday = new Date(today);
    yesterday.setDate(yesterday.getDate() - 1);

    if (date.toDateString() === today.toDateString()) return "Today";
    if (date.toDateString() === yesterday.toDateString()) return "Yesterday";
    return date.toLocaleDateString("en-US", { weekday: "short" });
  };

  // Calculate max for activity chart
  const maxWords = Math.max(...stats.daily_activity.map((d) => d.words), 1);

  return (
    <div className="stats-panel">
      {/* Time Saved Hero */}
      <div className="stats-hero">
        <div className="hero-icon">⚡</div>
        <div className="hero-content">
          <span className="hero-value">{formatTimeSaved(stats.estimated_time_saved_minutes)}</span>
          <span className="hero-label">Time Saved</span>
        </div>
        <div className="hero-subtitle">vs typing at 40 WPM</div>
      </div>

      {/* Quick Stats */}
      <div className="quick-stats">
        <div className="quick-stat">
          <span className="quick-value">{stats.total_transcriptions.toLocaleString()}</span>
          <span className="quick-label">Transcriptions</span>
        </div>
        <div className="quick-stat">
          <span className="quick-value">{stats.total_words.toLocaleString()}</span>
          <span className="quick-label">Words</span>
        </div>
        <div className="quick-stat">
          <span className="quick-value">{Math.round(stats.average_words_per_minute)}</span>
          <span className="quick-label">Avg WPM</span>
        </div>
        <div className="quick-stat">
          <span className="quick-value">{stats.current_streak_days}</span>
          <span className="quick-label">Day Streak 🔥</span>
        </div>
      </div>

      {/* Time Period Breakdown */}
      <div className="stats-section">
        <h4>Activity</h4>
        <div className="period-stats">
          <div className="period-row">
            <span className="period-label">Today</span>
            <span className="period-value">{stats.words_today.toLocaleString()} words</span>
            <span className="period-count">{stats.transcriptions_today} transcriptions</span>
          </div>
          <div className="period-row">
            <span className="period-label">This Week</span>
            <span className="period-value">{stats.words_this_week.toLocaleString()} words</span>
            <span className="period-count">{stats.transcriptions_this_week} transcriptions</span>
          </div>
          <div className="period-row">
            <span className="period-label">This Month</span>
            <span className="period-value">{stats.words_this_month.toLocaleString()} words</span>
            <span className="period-count">{stats.transcriptions_this_month} transcriptions</span>
          </div>
        </div>
      </div>

      {/* 7-Day Activity Chart */}
      <div className="stats-section">
        <h4>Last 7 Days</h4>
        <div className="activity-chart">
          {stats.daily_activity.map((day) => (
            <div key={day.date} className="activity-bar-container">
              <div
                className="activity-bar"
                style={{ height: `${(day.words / maxWords) * 100}%` }}
                title={`${day.words.toLocaleString()} words`}
              />
              <span className="activity-day">{getDayName(day.date)}</span>
              <span className="activity-words">{day.words > 0 ? day.words.toLocaleString() : "-"}</span>
            </div>
          ))}
        </div>
      </div>

      {/* Context Breakdown */}
      {stats.context_breakdown.length > 0 && (
        <div className="stats-section">
          <h4>Usage by Context</h4>
          <div className="context-breakdown">
            {stats.context_breakdown.map((ctx) => (
              <div key={ctx.context} className="context-item">
                <div className="context-header">
                  <span
                    className="context-dot"
                    style={{ background: contextColors[ctx.context] || "#666" }}
                  />
                  <span className="context-name">{contextLabels[ctx.context] || ctx.context}</span>
                  <span className="context-percentage">{ctx.percentage}%</span>
                </div>
                <div className="context-bar-bg">
                  <div
                    className="context-bar-fill"
                    style={{
                      width: `${ctx.percentage}%`,
                      background: contextColors[ctx.context] || "#666",
                    }}
                  />
                </div>
                <span className="context-words">{ctx.words.toLocaleString()} words</span>
              </div>
            ))}
          </div>
        </div>
      )}

      {/* Personal Records */}
      <div className="stats-section">
        <h4>Personal Records</h4>
        <div className="records-grid">
          <div className="record-item">
            <span className="record-icon">🏆</span>
            <span className="record-value">{stats.longest_streak_days} days</span>
            <span className="record-label">Longest Streak</span>
          </div>
          <div className="record-item">
            <span className="record-icon">📝</span>
            <span className="record-value">{stats.longest_transcription_words.toLocaleString()}</span>
            <span className="record-label">Longest Transcription</span>
          </div>
          {stats.most_productive_day && (
            <div className="record-item">
              <span className="record-icon">⭐</span>
              <span className="record-value">{stats.most_productive_day_words.toLocaleString()} words</span>
              <span className="record-label">Best Day ({formatDate(stats.most_productive_day)})</span>
            </div>
          )}
          <div className="record-item">
            <span className="record-icon">📊</span>
            <span className="record-value">{stats.average_transcriptions_per_day}</span>
            <span className="record-label">Avg/Day</span>
          </div>
        </div>
      </div>

      {/* Member Since */}
      <div className="member-since">
        Member since {new Date(stats.member_since).toLocaleDateString("en-US", { month: "long", year: "numeric" })}
      </div>
    </div>
  );
}

// History Panel
function HistoryPanel({
  history,
  onClear,
}: {
  history: HistoryEntry[];
  onClear?: () => void;
}) {
  const formatTime = (date: Date) => {
    const now = new Date();
    const diffMs = now.getTime() - date.getTime();
    const diffMins = Math.floor(diffMs / 60000);
    const diffHours = Math.floor(diffMs / 3600000);
    const diffDays = Math.floor(diffMs / 86400000);

    if (diffMins < 1) return "Just now";
    if (diffMins < 60) return `${diffMins}m ago`;
    if (diffHours < 24) return `${diffHours}h ago`;
    if (diffDays < 7) return `${diffDays}d ago`;
    return date.toLocaleDateString("en-US", { month: "short", day: "numeric" });
  };

  const formatDuration = (seconds: number | null) => {
    if (!seconds) return "";
    if (seconds < 60) return `${Math.round(seconds)}s`;
    return `${Math.floor(seconds / 60)}m ${Math.round(seconds % 60)}s`;
  };

  const handleCopy = async (text: string) => {
    await copyToClipboard(text);
  };

  if (history.length === 0) {
    return (
      <div className="history-panel">
        <div className="history-empty">
          <div className="history-empty-icon">📝</div>
          <div className="history-empty-text">
            No transcriptions yet. Start recording to see your history here.
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="history-panel">
      <div className="history-header">
        <h3>Recent Transcriptions</h3>
        {onClear && history.length > 0 && (
          <button className="clear-history-btn" onClick={onClear}>
            Clear All
          </button>
        )}
      </div>

      <div className="history-list">
        {history.map((entry) => (
          <div
            key={entry.id}
            className="history-item"
            onClick={() => handleCopy(entry.polishedText)}
            title="Click to copy"
          >
            <div className="history-item-header">
              <span className="history-time">{formatTime(entry.timestamp)}</span>
              <div className="history-meta">
                <span className="history-context">{entry.context}</span>
                {entry.duration && (
                  <span className="history-duration">{formatDuration(entry.duration)}</span>
                )}
              </div>
            </div>
            <div className="history-text">{entry.polishedText}</div>
          </div>
        ))}
      </div>
    </div>
  );
}
