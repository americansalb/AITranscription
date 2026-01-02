import { useState, useEffect, useCallback } from "react";
import {
  login,
  signup,
  logout,
  getCurrentUser,
  getUserStats,
  isLoggedIn,
  UserResponse,
  UserStatsResponse,
  ApiError,
} from "../lib/api";
import { HOTKEYS } from "../hooks/useGlobalHotkey";

interface SettingsProps {
  onClose: () => void;
}

type SettingsTab = "account" | "dictionary" | "preferences";

export function Settings({ onClose }: SettingsProps) {
  const [activeTab, setActiveTab] = useState<SettingsTab>("account");
  const [user, setUser] = useState<UserResponse | null>(null);
  const [stats, setStats] = useState<UserStatsResponse | null>(null);
  const [loading, setLoading] = useState(true);

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
  }, []);

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
          ) : activeTab === "dictionary" ? (
            user ? (
              <DictionaryManager />
            ) : (
              <div className="auth-required">
                Please log in to manage your custom dictionary.
              </div>
            )
          ) : (
            <Preferences />
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

// App preferences
function Preferences() {
  const [autoPaste, setAutoPaste] = useState(true);
  const [playSound, setPlaySound] = useState(true);

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
        <kbd>{navigator.platform.includes("Mac") ? "Option" : "Alt"}+D</kbd>
      </div>
    </div>
  );
}
