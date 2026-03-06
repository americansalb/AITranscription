import { useState, useCallback, useEffect, useRef } from "react";
import {
  renameSession,
  changeSessionColor,
  clearSessionMessages,
  deleteSession,
  isSessionActive,
  getRelativeTime,
  formatTime,
  SESSION_COLORS,
  type Session,
} from "../lib/sessionManager";
import {
  saveVoiceAssignment,
  getVoiceAssignments,
  getAvailableVoices,
  fetchAvailableVoices,
} from "../lib/queueStore";

export interface SessionsPanelProps {
  sessions: Session[];
  onSessionsChange: (sessions: Session[]) => void;
}

export function SessionsPanel({ sessions, onSessionsChange }: SessionsPanelProps) {
  const [selectedSessionId, setSelectedSessionId] = useState<string | null>(null);
  const [editingSessionId, setEditingSessionId] = useState<string | null>(null);
  const [editingName, setEditingName] = useState("");
  const [colorPickerSessionId, setColorPickerSessionId] = useState<string | null>(null);
  const [filter, setFilter] = useState<"all" | "active">("all");
  const [searchQuery, setSearchQuery] = useState("");
  const [messageSort, setMessageSort] = useState<"newest" | "oldest">("newest");
  const [voiceAssignments, setVoiceAssignments] = useState<Record<string, string>>(() => getVoiceAssignments());
  const [voices, setVoices] = useState<{ voice_id: string; name: string }[]>([]);
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const messagesTopRef = useRef<HTMLDivElement>(null);

  // Fetch available voices on mount
  useEffect(() => {
    fetchAvailableVoices().then(() => {
      setVoices(getAvailableVoices());
    });
  }, []);

  // Auto-scroll when selected session changes
  useEffect(() => {
    if (messageSort === "newest" && messagesTopRef.current) {
      messagesTopRef.current.scrollIntoView({ behavior: "smooth" });
    } else if (messageSort === "oldest" && messagesEndRef.current) {
      messagesEndRef.current.scrollIntoView({ behavior: "smooth" });
    }
  }, [selectedSessionId, sessions, messageSort]);

  const handleRenameSession = useCallback(
    (sessionId: string) => {
      const session = sessions.find((s) => s.id === sessionId);
      if (session) {
        setEditingSessionId(sessionId);
        setEditingName(session.name);
      }
    },
    [sessions]
  );

  const handleSaveRename = useCallback(() => {
    if (editingSessionId && editingName.trim()) {
      onSessionsChange(renameSession(sessions, editingSessionId, editingName.trim()));
      setEditingSessionId(null);
      setEditingName("");
    }
  }, [sessions, editingSessionId, editingName, onSessionsChange]);

  const handleChangeColor = useCallback(
    (sessionId: string, newColor: string, closeAfter: boolean = false) => {
      onSessionsChange(changeSessionColor(sessions, sessionId, newColor));
      if (closeAfter) {
        setColorPickerSessionId(null);
      }
    },
    [sessions, onSessionsChange]
  );

  const handleClearSession = useCallback(
    (sessionId: string) => {
      if (confirm("Clear all messages from this session?")) {
        onSessionsChange(clearSessionMessages(sessions, sessionId));
      }
    },
    [sessions, onSessionsChange]
  );

  const handleDeleteSession = useCallback(
    (sessionId: string) => {
      if (confirm("Delete this session permanently?")) {
        onSessionsChange(deleteSession(sessions, sessionId));
        if (selectedSessionId === sessionId) {
          setSelectedSessionId(null);
        }
      }
    },
    [sessions, selectedSessionId, onSessionsChange]
  );

  const handleCopyMessage = useCallback((text: string) => {
    navigator.clipboard.writeText(text);
  }, []);

  const handleCopyAllMessages = useCallback(
    (sessionId: string) => {
      const session = sessions.find((s) => s.id === sessionId);
      if (session) {
        const text = session.messages
          .slice()
          .reverse()
          .map((m) => m.text)
          .join("\n\n");
        navigator.clipboard.writeText(text);
      }
    },
    [sessions]
  );

  const filteredSessions = sessions
    .filter((session) => {
      if (filter === "active") {
        if (!isSessionActive(session, 5 * 60 * 1000)) return false;
      }
      if (searchQuery) {
        const query = searchQuery.toLowerCase();
        return (
          session.name.toLowerCase().includes(query) ||
          session.messages.some((m) => m.text.toLowerCase().includes(query))
        );
      }
      return true;
    })
    .sort((a, b) => b.lastActivity - a.lastActivity);

  const selectedSession = selectedSessionId
    ? sessions.find((s) => s.id === selectedSessionId)
    : null;

  return (
    <div className="sessions-content" id="panel-sessions" role="tabpanel" aria-labelledby="tab-sessions">
      {/* Sidebar - Session List */}
      <div className="transcript-sidebar">
        <div className="transcript-header">
          <div>
            <h1>Claude Sessions</h1>
            <div className="transcript-stats">
              <span>{sessions.length} sessions</span>
              <span>
                {sessions.reduce((sum, s) => sum + s.messages.length, 0)} messages
              </span>
            </div>
          </div>
          <button
            className="transcript-btn transcript-btn-danger"
            onClick={() => {
              if (confirm("Clear all sessions and messages? This cannot be undone.")) {
                localStorage.removeItem("vaak_speak_sessions");
                onSessionsChange([]);
                setSelectedSessionId(null);
              }
            }}
            title="Clear all sessions"
          >
            Clear All
          </button>
        </div>

        {/* Filters */}
        <div className="transcript-filters">
          <div className="filter-tabs">
            <button
              className={filter === "all" ? "active" : ""}
              onClick={() => setFilter("all")}
            >
              All
            </button>
            <button
              className={filter === "active" ? "active" : ""}
              onClick={() => setFilter("active")}
            >
              Active
            </button>
          </div>
          <input
            type="text"
            placeholder="Search sessions..."
            className="filter-search"
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
          />
        </div>

        {/* Session List */}
        <div className="session-list">
          {filteredSessions.length === 0 ? (
            <div className="session-list-empty">
              <div className="empty-icon">💬</div>
              <div className="empty-text">No sessions yet</div>
              <div className="empty-hint">
                Speak messages will appear here automatically
              </div>
            </div>
          ) : (
            filteredSessions.map((session) => (
              <div
                key={session.id}
                className={`session-item ${
                  selectedSessionId === session.id ? "selected" : ""
                }`}
                onClick={() => setSelectedSessionId(session.id)}
              >
                <div
                  className="session-color"
                  style={{ backgroundColor: session.color }}
                />
                <div className="session-info">
                  {editingSessionId === session.id ? (
                    <input
                      type="text"
                      className="session-name-edit"
                      value={editingName}
                      onChange={(e) => setEditingName(e.target.value)}
                      onBlur={handleSaveRename}
                      onKeyDown={(e) => {
                        if (e.key === "Enter") handleSaveRename();
                        if (e.key === "Escape") setEditingSessionId(null);
                      }}
                      onClick={(e) => e.stopPropagation()}
                      autoFocus
                    />
                  ) : (
                    <div className="session-name">{session.name}</div>
                  )}
                  <div className="session-meta">
                    <span className="session-count">
                      {session.messages.length} messages
                    </span>
                    <span className="session-time">
                      {getRelativeTime(session.lastActivity)}
                    </span>
                  </div>
                </div>
              </div>
            ))
          )}
        </div>
      </div>

      {/* Main Panel - Messages */}
      <div className="transcript-main">
        {selectedSession ? (
          <>
            <div className="transcript-main-header">
              <div className="transcript-main-title">
                <div
                  className="session-color-large"
                  style={{ backgroundColor: selectedSession.color }}
                />
                <div>
                  <h2>{selectedSession.name}</h2>
                  <div className="transcript-main-subtitle">
                    {selectedSession.messages.length} messages
                  </div>
                </div>
              </div>
              <div className="transcript-main-actions">
                <button
                  className={`transcript-btn${messageSort === "newest" ? " active" : ""}`}
                  onClick={() => setMessageSort(messageSort === "newest" ? "oldest" : "newest")}
                  title={`Sort: ${messageSort === "newest" ? "Newest first" : "Oldest first"}`}
                >
                  {messageSort === "newest" ? "⬆️ Newest" : "⬇️ Oldest"}
                </button>
                <button
                  className="transcript-btn"
                  onClick={() => handleRenameSession(selectedSession.id)}
                  title="Rename session"
                >
                  ✏️ Rename
                </button>
                <div className="color-picker-container">
                  <button
                    className="transcript-btn"
                    onClick={() => setColorPickerSessionId(
                      colorPickerSessionId === selectedSession.id ? null : selectedSession.id
                    )}
                    title="Change color"
                  >
                    🎨 Color
                  </button>
                  {colorPickerSessionId === selectedSession.id && (
                    <div className="color-picker-dropdown" onClick={(e) => e.stopPropagation()}>
                      <div className="color-picker-presets">
                        {SESSION_COLORS.map((color) => (
                          <button
                            key={color}
                            className={`color-picker-option ${selectedSession.color === color ? 'selected' : ''}`}
                            style={{ backgroundColor: color }}
                            onClick={() => handleChangeColor(selectedSession.id, color, true)}
                            title={color}
                          />
                        ))}
                      </div>
                      <div className="color-picker-custom">
                        <label className="color-picker-custom-label">
                          Custom:
                          <input
                            type="color"
                            value={selectedSession.color}
                            onChange={(e) => handleChangeColor(selectedSession.id, e.target.value, false)}
                            className="color-picker-input"
                          />
                        </label>
                      </div>
                    </div>
                  )}
                </div>
                {/* Feature 7: Voice selector per session */}
                {voices.length > 0 && (
                  <select
                    className="session-voice-select"
                    value={voiceAssignments[selectedSession.id] || ''}
                    onChange={(e) => {
                      const vid = e.target.value;
                      saveVoiceAssignment(selectedSession.id, vid);
                      setVoiceAssignments({ ...voiceAssignments, [selectedSession.id]: vid });
                    }}
                    title="Assign voice"
                  >
                    <option value="">Default Voice</option>
                    {voices.map((v) => (
                      <option key={v.voice_id} value={v.voice_id}>{v.name}</option>
                    ))}
                  </select>
                )}
                <button
                  className="transcript-btn"
                  onClick={() => handleCopyAllMessages(selectedSession.id)}
                  title="Copy all messages"
                >
                  📋 Copy All
                </button>
                <button
                  className="transcript-btn"
                  onClick={() => handleClearSession(selectedSession.id)}
                  title="Clear messages"
                >
                  🗑️ Clear
                </button>
                <button
                  className="transcript-btn transcript-btn-danger"
                  onClick={() => handleDeleteSession(selectedSession.id)}
                  title="Delete session"
                >
                  ❌ Delete
                </button>
              </div>
            </div>

            <div className="transcript-messages">
              {selectedSession.messages.length === 0 ? (
                <div className="transcript-messages-empty">
                  <div className="empty-icon">💬</div>
                  <div className="empty-text">No messages in this session</div>
                </div>
              ) : (
                <>
                  <div ref={messagesTopRef} />
                  {(messageSort === "newest" ? [...selectedSession.messages].reverse() : selectedSession.messages).map((message) => (
                    <div key={message.id} className="transcript-message">
                      <div className="transcript-message-header">
                        <span className="transcript-message-time">
                          {formatTime(message.timestamp)}
                        </span>
                        <button
                          className="transcript-message-copy"
                          onClick={() => handleCopyMessage(message.text)}
                          title="Copy message"
                        >
                          📋
                        </button>
                      </div>
                      <div className="transcript-message-text">
                        {message.text}
                      </div>
                    </div>
                  ))}
                  <div ref={messagesEndRef} />
                </>
              )}
            </div>
          </>
        ) : (
          <div className="transcript-main-empty">
            <div className="empty-icon">👈</div>
            <div className="empty-text">Select a session to view messages</div>
            <div className="empty-hint">
              Sessions are created automatically when Claude speaks
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
