import { useState, useEffect, useCallback, useRef } from "react";
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
import { getStoredVoiceEnabled, saveVoiceEnabled } from "../lib/voiceStream";

interface SettingsProps {
  onClose: () => void;
  refreshTrigger?: number;
  history?: HistoryEntry[];
  onClearHistory?: () => void;
  onHotkeyChange?: (hotkey: string) => void;
  onModelChange?: (model: string) => void;
  onNoiseCancellationChange?: (enabled: boolean) => void;
  onVoiceEnabledChange?: (enabled: boolean) => void;
}

type SettingsTab = "account" | "stats" | "history" | "dictionary" | "preferences";

export function Settings({ onClose, refreshTrigger = 0, history = [], onClearHistory, onHotkeyChange, onModelChange, onNoiseCancellationChange, onVoiceEnabledChange }: SettingsProps) {
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
            <Preferences onHotkeyChange={onHotkeyChange} onModelChange={onModelChange} onNoiseCancellationChange={onNoiseCancellationChange} onVoiceEnabledChange={onVoiceEnabledChange} />
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

// Convert keyboard event to Tauri hotkey format
function keyEventToHotkey(e: KeyboardEvent): string | null {
  // Don't capture if only modifier keys pressed
  if (["Control", "Alt", "Shift", "Meta"].includes(e.key)) {
    return null;
  }

  const parts: string[] = [];

  // Build modifier string (Tauri format)
  if (e.ctrlKey || e.metaKey) {
    parts.push("CommandOrControl");
  }
  if (e.altKey) {
    parts.push("Alt");
  }
  if (e.shiftKey) {
    parts.push("Shift");
  }

  // Map key to Tauri format
  let key = e.key;
  if (key === " ") key = "Space";
  else if (key.length === 1) key = key.toUpperCase();
  else if (key.startsWith("Arrow")) key = key; // ArrowUp, etc
  else if (key.startsWith("F") && !isNaN(parseInt(key.slice(1)))) key = key; // F1-F12

  parts.push(key);

  return parts.join("+");
}

// Format hotkey for display
function formatHotkeyForDisplay(hotkey: string): string {
  const isMac = navigator.platform.includes("Mac");
  return hotkey
    .replace("CommandOrControl", isMac ? "Cmd" : "Ctrl")
    .replace("Alt", isMac ? "Option" : "Alt");
}

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
// Whisper model options for developer settings
const WHISPER_MODEL_OPTIONS = [
  { value: "whisper-large-v3-turbo", label: "Turbo (Fast)", description: "Faster, more cost-effective" },
  { value: "whisper-large-v3", label: "Large V3 (Accurate)", description: "Higher accuracy, slower" },
];

// Get stored whisper model from localStorage
export function getStoredWhisperModel(): string {
  try {
    return localStorage.getItem("scribe_whisper_model") || "whisper-large-v3-turbo";
  } catch {
    return "whisper-large-v3-turbo";
  }
}

// Save whisper model to localStorage
export function saveWhisperModel(model: string): void {
  try {
    localStorage.setItem("scribe_whisper_model", model);
  } catch {
    // Ignore storage errors
  }
}

// Get stored noise cancellation preference
export function getStoredNoiseCancellation(): boolean {
  try {
    return localStorage.getItem("scribe_noise_cancellation") === "true";
  } catch {
    return false;
  }
}

// Save noise cancellation preference
export function saveNoiseCancellation(enabled: boolean): void {
  try {
    localStorage.setItem("scribe_noise_cancellation", enabled ? "true" : "false");
  } catch {
    // Ignore storage errors
  }
}

interface PreferencesProps {
  onHotkeyChange?: (hotkey: string) => void;
  onModelChange?: (model: string) => void;
  onNoiseCancellationChange?: (enabled: boolean) => void;
  onVoiceEnabledChange?: (enabled: boolean) => void;
}

function Preferences({ onHotkeyChange, onModelChange, onNoiseCancellationChange, onVoiceEnabledChange }: PreferencesProps) {
  const [autoPaste, setAutoPaste] = useState(true);
  const [playSound, setPlaySound] = useState(true);
  const [hotkey, setHotkey] = useState(() => getStoredHotkey());
  const [whisperModel, setWhisperModel] = useState(() => getStoredWhisperModel());
  const [noiseCancellation, setNoiseCancellation] = useState(() => getStoredNoiseCancellation());
  const [voiceEnabled, setVoiceEnabled] = useState(() => getStoredVoiceEnabled());
  const [showDevSettings, setShowDevSettings] = useState(false);
  const [isRecordingHotkey, setIsRecordingHotkey] = useState(false);
  const hotkeyInputRef = useRef<HTMLInputElement>(null);

  const handleHotkeyChange = (newHotkey: string) => {
    setHotkey(newHotkey);
    saveHotkey(newHotkey);
    onHotkeyChange?.(newHotkey);
  };

  const handleModelChange = (newModel: string) => {
    setWhisperModel(newModel);
    saveWhisperModel(newModel);
    onModelChange?.(newModel);
  };

  const handleNoiseCancellationChange = (enabled: boolean) => {
    setNoiseCancellation(enabled);
    saveNoiseCancellation(enabled);
    onNoiseCancellationChange?.(enabled);
  };

  const handleVoiceEnabledChange = (enabled: boolean) => {
    setVoiceEnabled(enabled);
    saveVoiceEnabled(enabled);
    onVoiceEnabledChange?.(enabled);
  };

  // Handle keydown for custom hotkey recording
  const handleHotkeyKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    e.preventDefault();
    e.stopPropagation();

    // Convert the event to a hotkey string
    const newHotkey = keyEventToHotkey(e.nativeEvent);
    if (newHotkey) {
      handleHotkeyChange(newHotkey);
      setIsRecordingHotkey(false);
      hotkeyInputRef.current?.blur();
    }
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

      <label className="toggle-setting">
        <span>Noise cancellation</span>
        <input
          type="checkbox"
          checked={noiseCancellation}
          onChange={(e) => handleNoiseCancellationChange(e.target.checked)}
        />
        <span className="toggle-switch" />
      </label>
      <p className="setting-hint">Reduce background noise before transcription (experimental)</p>

      <label className="toggle-setting">
        <span>Voice explanations (Claude Code)</span>
        <input
          type="checkbox"
          checked={voiceEnabled}
          onChange={(e) => handleVoiceEnabledChange(e.target.checked)}
        />
        <span className="toggle-switch" />
      </label>
      <p className="setting-hint">Hear spoken explanations when Claude Code writes or edits files</p>

      {/* Claude Code Setup Instructions */}
      <div className="claude-code-setup">
        <p className="setup-title">Claude Code Setup</p>
        <p className="setting-hint">
          To enable voice announcements, add this to your Claude Code settings:
        </p>
        <ol className="setup-steps">
          <li>Install: <code>cd mcp-speak && pip install -e .</code></li>
          <li>Copy the config below</li>
          <li>Paste into <code>~/.claude/settings.json</code></li>
        </ol>
        <div className="config-copy">
          <code className="config-snippet">
            {`"mcpServers": { "scribe": { "command": "scribe-speak" } }`}
          </code>
          <button
            className="copy-config-btn"
            onClick={() => {
              const config = `{
  "mcpServers": {
    "scribe": {
      "command": "scribe-speak"
    }
  }
}`;
              navigator.clipboard.writeText(config);
            }}
          >
            Copy Config
          </button>
        </div>
      </div>

      <div className="hotkey-setting">
        <span>Push-to-talk hotkey</span>
        <div className="hotkey-recorder">
          <input
            ref={hotkeyInputRef}
            type="text"
            readOnly
            value={isRecordingHotkey ? "Press any key combo..." : formatHotkeyForDisplay(hotkey)}
            onFocus={() => setIsRecordingHotkey(true)}
            onBlur={() => setIsRecordingHotkey(false)}
            onKeyDown={handleHotkeyKeyDown}
            className={`hotkey-input ${isRecordingHotkey ? "recording" : ""}`}
            placeholder="Click to set hotkey"
          />
          {!isRecordingHotkey && (
            <button
              className="hotkey-clear-btn"
              onClick={() => handleHotkeyChange("Alt+D")}
              title="Reset to default"
            >
              Reset
            </button>
          )}
        </div>
      </div>

      <p className="hotkey-hint">
        Click the field and press any key combination. Hold the hotkey to record, release to transcribe.
      </p>

      {/* Developer Settings - Click to expand */}
      <div className="dev-settings-section">
        <button
          className="dev-settings-toggle"
          onClick={() => setShowDevSettings(!showDevSettings)}
        >
          <span>Developer Settings</span>
          <svg
            width="16"
            height="16"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2"
            style={{ transform: showDevSettings ? "rotate(180deg)" : "rotate(0deg)", transition: "transform 0.2s" }}
          >
            <polyline points="6 9 12 15 18 9" />
          </svg>
        </button>

        {showDevSettings && (
          <div className="dev-settings-content">
            <div className="model-setting">
              <span>Whisper Model</span>
              <select
                value={whisperModel}
                onChange={(e) => handleModelChange(e.target.value)}
                className="model-select"
              >
                {WHISPER_MODEL_OPTIONS.map((opt) => (
                  <option key={opt.value} value={opt.value}>
                    {opt.label}
                  </option>
                ))}
              </select>
            </div>
            <p className="setting-hint">
              {WHISPER_MODEL_OPTIONS.find(m => m.value === whisperModel)?.description}
            </p>
          </div>
        )}
      </div>
    </div>
  );
}

// Extremely In-Depth Stats Panel
function StatsPanel({ refreshTrigger = 0 }: { refreshTrigger?: number }) {
  const [stats, setStats] = useState<DetailedStatsResponse | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [activeSection, setActiveSection] = useState<string>("overview");

  useEffect(() => {
    setLoading(true);
    getDetailedStats()
      .then(setStats)
      .catch((err) => setError(err.message))
      .finally(() => setLoading(false));
  }, [refreshTrigger]);

  if (loading) return <div className="loading">Loading statistics...</div>;
  if (error) return <div className="form-error">Failed to load stats: {error}</div>;
  if (!stats) return <div className="auth-required">No statistics available yet.</div>;

  const contextLabels: Record<string, string> = {
    general: "General", email: "Email", slack: "Slack / Chat", document: "Document", code: "Code",
  };
  const contextColors: Record<string, string> = {
    general: "#6366f1", email: "#22c55e", slack: "#f59e0b", document: "#3b82f6", code: "#ec4899",
  };
  const formalityLabels: Record<string, string> = {
    casual: "Casual", neutral: "Neutral", formal: "Formal",
  };
  const formalityColors: Record<string, string> = {
    casual: "#22c55e", neutral: "#6366f1", formal: "#8b5cf6",
  };

  const formatTimeSaved = (minutes: number) => {
    if (minutes < 60) return `${Math.round(minutes)} min`;
    const hours = Math.floor(minutes / 60);
    const mins = Math.round(minutes % 60);
    return mins > 0 ? `${hours}h ${mins}m` : `${hours} hours`;
  };

  const formatDate = (dateStr: string) => {
    const date = new Date(dateStr);
    return date.toLocaleDateString("en-US", { weekday: "short", month: "short", day: "numeric" });
  };

  const getDayName = (dateStr: string) => {
    const date = new Date(dateStr);
    const today = new Date();
    const yesterday = new Date(today);
    yesterday.setDate(yesterday.getDate() - 1);
    if (date.toDateString() === today.toDateString()) return "Today";
    if (date.toDateString() === yesterday.toDateString()) return "Yest";
    return date.toLocaleDateString("en-US", { weekday: "short" });
  };

  const formatGrowth = (value: number) => {
    if (value === 0) return "—";
    const sign = value > 0 ? "+" : "";
    return `${sign}${value.toFixed(1)}%`;
  };

  const formatDuration = (seconds: number) => {
    if (seconds < 60) return `${Math.round(seconds)}s`;
    const mins = Math.floor(seconds / 60);
    const secs = Math.round(seconds % 60);
    return `${mins}m ${secs}s`;
  };

  const maxDailyWords = Math.max(...stats.daily_activity.map((d) => d.words), 1);
  const maxHourlyWords = Math.max(...stats.hourly_activity.map((h) => h.words), 1);
  const maxMonthlyWords = Math.max(...stats.monthly_trends.map((m) => m.words), 1);
  const maxDowWords = Math.max(...stats.day_of_week_breakdown.map((d) => d.words), 1);

  const sections = [
    { id: "overview", label: "Overview" },
    { id: "activity", label: "Activity" },
    { id: "patterns", label: "Patterns" },
    { id: "growth", label: "Growth" },
    { id: "achievements", label: "Achievements" },
    { id: "records", label: "Records" },
  ];

  return (
    <div className="stats-panel-deep">
      {/* Section Tabs */}
      <div className="stats-tabs">
        {sections.map((s) => (
          <button
            key={s.id}
            className={activeSection === s.id ? "active" : ""}
            onClick={() => setActiveSection(s.id)}
          >
            {s.label}
          </button>
        ))}
      </div>

      {/* OVERVIEW SECTION */}
      {activeSection === "overview" && (
        <>
          {/* Time Saved Hero */}
          <div className="stats-hero">
            <div className="hero-icon">⚡</div>
            <div className="hero-content">
              <span className="hero-value">{formatTimeSaved(stats.estimated_time_saved_minutes)}</span>
              <span className="hero-label">Time Saved</span>
            </div>
            <div className="hero-subtitle">vs typing at 40 WPM</div>
          </div>

          {/* Quick Stats Grid */}
          <div className="quick-stats-extended">
            <div className="quick-stat">
              <span className="quick-value">{stats.total_transcriptions.toLocaleString()}</span>
              <span className="quick-label">Transcriptions</span>
            </div>
            <div className="quick-stat">
              <span className="quick-value">{stats.total_words.toLocaleString()}</span>
              <span className="quick-label">Words</span>
            </div>
            <div className="quick-stat">
              <span className="quick-value">{stats.total_characters.toLocaleString()}</span>
              <span className="quick-label">Characters</span>
            </div>
            <div className="quick-stat">
              <span className="quick-value">{formatDuration(stats.total_audio_seconds)}</span>
              <span className="quick-label">Audio Time</span>
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

          {/* Productivity Insights */}
          <div className="stats-section">
            <h4>Productivity Insights</h4>
            <div className="insights-grid">
              <div className="insight-item">
                <span className="insight-icon">🕐</span>
                <span className="insight-label">Peak Hour</span>
                <span className="insight-value">{stats.productivity.peak_hour_label}</span>
              </div>
              <div className="insight-item">
                <span className="insight-icon">📅</span>
                <span className="insight-label">Peak Day</span>
                <span className="insight-value">{stats.productivity.peak_day}</span>
              </div>
              <div className="insight-item">
                <span className="insight-icon">📊</span>
                <span className="insight-label">Efficiency</span>
                <span className="insight-value">{stats.productivity.efficiency_score} WPM</span>
              </div>
              <div className="insight-item">
                <span className="insight-icon">📝</span>
                <span className="insight-label">Avg Session</span>
                <span className="insight-value">{Math.round(stats.productivity.avg_session_words)} words</span>
              </div>
            </div>
          </div>

          {/* Member Info */}
          <div className="member-info-grid">
            <div className="member-stat">
              <span className="member-label">Member Since</span>
              <span className="member-value">{new Date(stats.member_since).toLocaleDateString("en-US", { month: "long", day: "numeric", year: "numeric" })}</span>
            </div>
            <div className="member-stat">
              <span className="member-label">Days as Member</span>
              <span className="member-value">{stats.days_as_member}</span>
            </div>
            <div className="member-stat">
              <span className="member-label">Active Days</span>
              <span className="member-value">{stats.total_active_days}</span>
            </div>
            <div className="member-stat">
              <span className="member-label">Activity Rate</span>
              <span className="member-value">{((stats.total_active_days / stats.days_as_member) * 100).toFixed(1)}%</span>
            </div>
          </div>
        </>
      )}

      {/* ACTIVITY SECTION */}
      {activeSection === "activity" && (
        <>
          {/* Time Period Breakdown */}
          <div className="stats-section">
            <h4>Activity Summary</h4>
            <div className="period-stats-extended">
              <div className="period-row-extended">
                <span className="period-label">Today</span>
                <div className="period-data">
                  <span className="period-words">{stats.words_today.toLocaleString()} words</span>
                  <span className="period-count">{stats.transcriptions_today} transcriptions</span>
                </div>
              </div>
              <div className="period-row-extended">
                <span className="period-label">This Week</span>
                <div className="period-data">
                  <span className="period-words">{stats.words_this_week.toLocaleString()} words</span>
                  <span className="period-count">{stats.transcriptions_this_week} transcriptions</span>
                </div>
              </div>
              <div className="period-row-extended">
                <span className="period-label">This Month</span>
                <div className="period-data">
                  <span className="period-words">{stats.words_this_month.toLocaleString()} words</span>
                  <span className="period-count">{stats.transcriptions_this_month} transcriptions</span>
                </div>
              </div>
            </div>
          </div>

          {/* 7-Day Activity Chart */}
          <div className="stats-section">
            <h4>Last 7 Days</h4>
            <div className="activity-chart">
              {stats.daily_activity.map((day) => (
                <div key={day.date} className="activity-bar-container">
                  <div className="activity-bar" style={{ height: `${(day.words / maxDailyWords) * 100}%` }} title={`${day.words.toLocaleString()} words`} />
                  <span className="activity-day">{getDayName(day.date)}</span>
                  <span className="activity-words">{day.words > 0 ? day.words.toLocaleString() : "-"}</span>
                </div>
              ))}
            </div>
          </div>

          {/* Monthly Trends */}
          <div className="stats-section">
            <h4>Monthly Trends (Last 12 Months)</h4>
            <div className="monthly-chart">
              {stats.monthly_trends.map((month) => (
                <div key={month.month} className="monthly-bar-container">
                  <div className="monthly-bar" style={{ height: `${(month.words / maxMonthlyWords) * 100}%` }} title={`${month.words.toLocaleString()} words`} />
                  <span className="monthly-label">{month.month_label}</span>
                  <span className="monthly-words">{month.words > 0 ? (month.words > 999 ? `${(month.words / 1000).toFixed(1)}k` : month.words) : "-"}</span>
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
                      <span className="context-dot" style={{ background: contextColors[ctx.context] || "#666" }} />
                      <span className="context-name">{contextLabels[ctx.context] || ctx.context}</span>
                      <span className="context-percentage">{ctx.percentage}%</span>
                    </div>
                    <div className="context-bar-bg">
                      <div className="context-bar-fill" style={{ width: `${ctx.percentage}%`, background: contextColors[ctx.context] || "#666" }} />
                    </div>
                    <div className="context-meta">
                      <span>{ctx.count} transcriptions</span>
                      <span>{ctx.words.toLocaleString()} words</span>
                    </div>
                  </div>
                ))}
              </div>
            </div>
          )}

          {/* Formality Breakdown */}
          {stats.formality_breakdown.length > 0 && (
            <div className="stats-section">
              <h4>Usage by Formality</h4>
              <div className="context-breakdown">
                {stats.formality_breakdown.map((f) => (
                  <div key={f.formality} className="context-item">
                    <div className="context-header">
                      <span className="context-dot" style={{ background: formalityColors[f.formality] || "#666" }} />
                      <span className="context-name">{formalityLabels[f.formality] || f.formality}</span>
                      <span className="context-percentage">{f.percentage}%</span>
                    </div>
                    <div className="context-bar-bg">
                      <div className="context-bar-fill" style={{ width: `${f.percentage}%`, background: formalityColors[f.formality] || "#666" }} />
                    </div>
                    <div className="context-meta">
                      <span>{f.count} transcriptions</span>
                      <span>{f.words.toLocaleString()} words</span>
                    </div>
                  </div>
                ))}
              </div>
            </div>
          )}
        </>
      )}

      {/* PATTERNS SECTION */}
      {activeSection === "patterns" && (
        <>
          {/* Hourly Heatmap */}
          <div className="stats-section">
            <h4>Activity by Hour of Day</h4>
            <div className="hourly-heatmap">
              {stats.hourly_activity.map((h) => (
                <div key={h.hour} className="hourly-cell" style={{ opacity: 0.2 + (h.words / maxHourlyWords) * 0.8 }} title={`${h.hour}:00 - ${h.words.toLocaleString()} words, ${h.transcriptions} transcriptions`}>
                  <span className="hour-label">{h.hour}</span>
                </div>
              ))}
            </div>
            <div className="heatmap-legend">
              <span>Less active</span>
              <div className="legend-gradient" />
              <span>More active</span>
            </div>
          </div>

          {/* Day of Week Breakdown */}
          <div className="stats-section">
            <h4>Activity by Day of Week</h4>
            <div className="dow-chart">
              {stats.day_of_week_breakdown.map((d) => (
                <div key={d.day} className="dow-bar-container">
                  <div className="dow-bar" style={{ height: `${(d.words / maxDowWords) * 100}%` }} />
                  <span className="dow-label">{d.day.substring(0, 3)}</span>
                  <span className="dow-words">{d.words.toLocaleString()}</span>
                  <span className="dow-percent">{d.percentage}%</span>
                </div>
              ))}
            </div>
          </div>

          {/* Word Length Distribution */}
          <div className="stats-section">
            <h4>Transcription Length Distribution</h4>
            <div className="length-distribution">
              {stats.word_length_distribution.map((d) => (
                <div key={d.range_label} className="length-item">
                  <span className="length-label">{d.range_label} words</span>
                  <div className="length-bar-bg">
                    <div className="length-bar-fill" style={{ width: `${d.percentage}%` }} />
                  </div>
                  <span className="length-count">{d.count} ({d.percentage}%)</span>
                </div>
              ))}
            </div>
          </div>

          {/* Averages */}
          <div className="stats-section">
            <h4>Averages</h4>
            <div className="averages-grid">
              <div className="average-item">
                <span className="average-value">{stats.average_words_per_transcription.toFixed(1)}</span>
                <span className="average-label">Words per Transcription</span>
              </div>
              <div className="average-item">
                <span className="average-value">{stats.average_words_per_minute.toFixed(1)}</span>
                <span className="average-label">Words per Minute</span>
              </div>
              <div className="average-item">
                <span className="average-value">{stats.average_transcriptions_per_day.toFixed(2)}</span>
                <span className="average-label">Transcriptions per Day</span>
              </div>
              <div className="average-item">
                <span className="average-value">{formatDuration(stats.average_audio_duration_seconds)}</span>
                <span className="average-label">Avg Audio Duration</span>
              </div>
            </div>
          </div>
        </>
      )}

      {/* GROWTH SECTION */}
      {activeSection === "growth" && (
        <>
          {/* Growth Metrics */}
          <div className="stats-section">
            <h4>Week over Week</h4>
            <div className="growth-comparison">
              <div className="growth-item">
                <span className="growth-label">Words</span>
                <div className="growth-values">
                  <span className="growth-current">{stats.growth.last_week_words.toLocaleString()}</span>
                  <span className="growth-arrow">←</span>
                  <span className="growth-previous">{stats.growth.prev_week_words.toLocaleString()}</span>
                </div>
                <span className={`growth-change ${stats.growth.words_wow_change >= 0 ? "positive" : "negative"}`}>
                  {formatGrowth(stats.growth.words_wow_change)}
                </span>
              </div>
              <div className="growth-item">
                <span className="growth-label">Transcriptions</span>
                <div className="growth-values">
                  <span className="growth-current">{stats.transcriptions_this_week}</span>
                  <span className="growth-arrow">←</span>
                  <span className="growth-previous">{Math.round(stats.growth.prev_week_words / (stats.average_words_per_transcription || 1))}</span>
                </div>
                <span className={`growth-change ${stats.growth.transcriptions_wow_change >= 0 ? "positive" : "negative"}`}>
                  {formatGrowth(stats.growth.transcriptions_wow_change)}
                </span>
              </div>
            </div>
          </div>

          <div className="stats-section">
            <h4>Month over Month</h4>
            <div className="growth-comparison">
              <div className="growth-item">
                <span className="growth-label">Words</span>
                <div className="growth-values">
                  <span className="growth-current">{stats.growth.last_month_words.toLocaleString()}</span>
                  <span className="growth-arrow">←</span>
                  <span className="growth-previous">{stats.growth.prev_month_words.toLocaleString()}</span>
                </div>
                <span className={`growth-change ${stats.growth.words_mom_change >= 0 ? "positive" : "negative"}`}>
                  {formatGrowth(stats.growth.words_mom_change)}
                </span>
              </div>
              <div className="growth-item">
                <span className="growth-label">Transcriptions</span>
                <div className="growth-values">
                  <span className="growth-current">{stats.transcriptions_this_month}</span>
                  <span className="growth-arrow">←</span>
                  <span className="growth-previous">{Math.round(stats.growth.prev_month_words / (stats.average_words_per_transcription || 1))}</span>
                </div>
                <span className={`growth-change ${stats.growth.transcriptions_mom_change >= 0 ? "positive" : "negative"}`}>
                  {formatGrowth(stats.growth.transcriptions_mom_change)}
                </span>
              </div>
            </div>
          </div>

          {/* Streak Information */}
          <div className="stats-section">
            <h4>Streaks</h4>
            <div className="streaks-display">
              <div className="streak-item current">
                <span className="streak-icon">🔥</span>
                <span className="streak-value">{stats.current_streak_days}</span>
                <span className="streak-label">Current Streak</span>
              </div>
              <div className="streak-item best">
                <span className="streak-icon">🏆</span>
                <span className="streak-value">{stats.longest_streak_days}</span>
                <span className="streak-label">Longest Streak</span>
              </div>
            </div>
          </div>

          {/* Busiest Week */}
          {stats.productivity.busiest_week_ever && (
            <div className="stats-section">
              <h4>Busiest Week Ever</h4>
              <div className="busiest-week">
                <span className="busiest-week-label">{stats.productivity.busiest_week_ever}</span>
                <span className="busiest-week-value">{stats.productivity.busiest_week_words.toLocaleString()} words</span>
              </div>
            </div>
          )}
        </>
      )}

      {/* ACHIEVEMENTS SECTION */}
      {activeSection === "achievements" && (
        <>
          <div className="achievements-grid">
            {stats.achievements.map((a) => (
              <div key={a.id} className={`achievement-card ${a.earned ? "earned" : "locked"}`}>
                <span className="achievement-icon">{a.icon}</span>
                <span className="achievement-name">{a.name}</span>
                <span className="achievement-desc">{a.description}</span>
                {!a.earned && a.target && a.current !== null && (
                  <div className="achievement-progress">
                    <div className="achievement-bar-bg">
                      <div className="achievement-bar-fill" style={{ width: `${a.progress}%` }} />
                    </div>
                    <span className="achievement-progress-text">{a.current?.toLocaleString()} / {a.target.toLocaleString()}</span>
                  </div>
                )}
                {a.earned && <span className="achievement-earned">Earned!</span>}
              </div>
            ))}
          </div>
        </>
      )}

      {/* RECORDS SECTION */}
      {activeSection === "records" && (
        <>
          <div className="stats-section">
            <h4>Personal Records</h4>
            <div className="records-extended">
              <div className="record-card">
                <span className="record-icon">🏆</span>
                <span className="record-title">Longest Streak</span>
                <span className="record-value">{stats.longest_streak_days} days</span>
              </div>
              <div className="record-card">
                <span className="record-icon">📝</span>
                <span className="record-title">Longest Transcription</span>
                <span className="record-value">{stats.longest_transcription_words.toLocaleString()} words</span>
              </div>
              <div className="record-card">
                <span className="record-icon">📄</span>
                <span className="record-title">Shortest Transcription</span>
                <span className="record-value">{stats.shortest_transcription_words} words</span>
              </div>
              <div className="record-card">
                <span className="record-icon">⚡</span>
                <span className="record-title">Fastest WPM</span>
                <span className="record-value">{stats.fastest_wpm.toFixed(1)} WPM</span>
              </div>
              <div className="record-card">
                <span className="record-icon">🐢</span>
                <span className="record-title">Slowest WPM</span>
                <span className="record-value">{stats.slowest_wpm.toFixed(1)} WPM</span>
              </div>
              {stats.most_productive_day && (
                <div className="record-card">
                  <span className="record-icon">⭐</span>
                  <span className="record-title">Most Productive Day</span>
                  <span className="record-value">{stats.most_productive_day_words.toLocaleString()} words</span>
                  <span className="record-date">{formatDate(stats.most_productive_day)}</span>
                </div>
              )}
            </div>
          </div>

          {/* Totals Summary */}
          <div className="stats-section">
            <h4>Lifetime Totals</h4>
            <div className="totals-grid">
              <div className="total-item">
                <span className="total-label">Transcriptions</span>
                <span className="total-value">{stats.total_transcriptions.toLocaleString()}</span>
              </div>
              <div className="total-item">
                <span className="total-label">Words</span>
                <span className="total-value">{stats.total_words.toLocaleString()}</span>
              </div>
              <div className="total-item">
                <span className="total-label">Characters</span>
                <span className="total-value">{stats.total_characters.toLocaleString()}</span>
              </div>
              <div className="total-item">
                <span className="total-label">Audio Recorded</span>
                <span className="total-value">{formatDuration(stats.total_audio_seconds)}</span>
              </div>
              <div className="total-item">
                <span className="total-label">Time Saved</span>
                <span className="total-value">{formatTimeSaved(stats.estimated_time_saved_minutes)}</span>
              </div>
              <div className="total-item">
                <span className="total-label">Active Days</span>
                <span className="total-value">{stats.total_active_days}</span>
              </div>
            </div>
          </div>
        </>
      )}
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
