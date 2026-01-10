import { useState, useEffect, useCallback, useRef } from "react";
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
import { getStoredVoiceEnabled, saveVoiceEnabled } from "../lib/voiceStream";

interface SettingsProps {
  onClose: () => void;
  refreshTrigger?: number;
  onHotkeyChange?: (hotkey: string) => void;
  onModelChange?: (model: string) => void;
  onNoiseCancellationChange?: (enabled: boolean) => void;
  onVoiceEnabledChange?: (enabled: boolean) => void;
}

type SettingsTab = "account" | "dictionary" | "preferences";

export function Settings({ onClose, refreshTrigger = 0, onHotkeyChange, onModelChange, onNoiseCancellationChange, onVoiceEnabledChange }: SettingsProps) {
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

  const handleVoiceEnabledChange = async (enabled: boolean) => {
    setVoiceEnabled(enabled);
    saveVoiceEnabled(enabled);
    onVoiceEnabledChange?.(enabled);

    // Update CLAUDE.md in home directory to enable/disable voice instructions
    if (window.__TAURI__) {
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        // When enabled, use "summary" mode (speaks explanations, not code)
        // When disabled, clear the file so Claude Code doesn't try to speak
        await invoke("update_claude_md", { mode: enabled ? "summary" : "disabled" });
      } catch (e) {
        console.error("Failed to update CLAUDE.md:", e);
      }
    }
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

