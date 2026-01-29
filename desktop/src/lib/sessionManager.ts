/**
 * Session management for tracking Claude Code instances
 * Each Claude instance maintains its own session_id across messages
 */

export interface SpeakMessage {
  text: string;
  session_id: string;
  timestamp: number;
}

export interface SessionMessage {
  id: string;
  text: string;
  timestamp: number;
}

export interface Session {
  id: string; // session_id from Claude
  name: string; // "Claude #1", "Claude #2", or user-renamed
  color: string; // Visual identifier
  messages: SessionMessage[];
  createdAt: number;
  lastActivity: number;
  lastHeartbeat?: number; // Last heartbeat timestamp - indicates session is actively running
  isAutoNamed?: boolean; // Track if name was auto-generated (can be updated) or manually set
}

// Storage keys
const SESSIONS_KEY = "vaak_speak_sessions";

// Predefined colors for visual distinction (8 colors for 8 Claude instances)
export const SESSION_COLORS = [
  "#3B82F6", // Blue
  "#10B981", // Green
  "#F59E0B", // Amber
  "#EF4444", // Red
  "#8B5CF6", // Purple
  "#EC4899", // Pink
  "#14B8A6", // Teal
  "#F97316", // Orange
];

/**
 * Get session color by index
 */
function getSessionColor(index: number): string {
  return SESSION_COLORS[index % SESSION_COLORS.length];
}

/**
 * Load all sessions from localStorage
 */
export function loadSessions(): Session[] {
  try {
    const stored = localStorage.getItem(SESSIONS_KEY);
    if (stored) {
      const sessions = JSON.parse(stored) as Session[];
      console.log(`[SessionManager] Loaded ${sessions.length} sessions from localStorage:`, sessions.map(s => ({ id: s.id, name: s.name })));
      return sessions;
    }
  } catch (e) {
    console.error("[SessionManager] Failed to load sessions:", e);
  }
  console.log("[SessionManager] No sessions in localStorage, starting fresh");
  return [];
}

/**
 * Save sessions to localStorage
 */
export function saveSessions(sessions: Session[]): void {
  try {
    // Keep only last 500 messages per session to avoid storage bloat
    const trimmed = sessions.map(session => ({
      ...session,
      messages: session.messages.slice(0, 500)
    }));
    localStorage.setItem(SESSIONS_KEY, JSON.stringify(trimmed));
    console.log(`[SessionManager] Saved ${sessions.length} sessions to localStorage`);
  } catch (e) {
    console.error("[SessionManager] Failed to save sessions:", e);
  }
}

/**
 * Get the next available session number based on current sessions
 * Numbers are assigned sequentially starting from 1
 */
function getNextSessionNumber(existingSessions: Session[]): number {
  if (existingSessions.length === 0) return 1;

  // Find the highest number currently in use
  const usedNumbers = existingSessions
    .map(s => {
      const match = s.name.match(/Claude #(\d+)/);
      return match ? parseInt(match[1], 10) : 0;
    })
    .filter(n => n > 0);

  if (usedNumbers.length === 0) return 1;

  // Find the first gap in the sequence, or return the next number
  const sorted = usedNumbers.sort((a, b) => a - b);
  for (let i = 1; i <= sorted.length + 1; i++) {
    if (!sorted.includes(i)) {
      return i;
    }
  }

  return sorted.length + 1;
}

/**
 * Get or create session for a message
 */
export function getOrCreateSession(
  message: SpeakMessage,
  existingSessions: Session[]
): { session: Session; isNew: boolean } {
  const now = Date.now();

  console.log(`[SessionManager] Looking for session: ${message.session_id}`);
  console.log(`[SessionManager] Existing sessions:`, existingSessions.map(s => ({ id: s.id, name: s.name })));

  // Try to find existing session by session_id
  const existing = existingSessions.find(s => s.id === message.session_id);
  if (existing) {
    console.log(`[SessionManager] FOUND existing session: ${existing.name} (${existing.id})`);
    return { session: existing, isNew: false };
  }

  // Create new session with a sequential number
  const sessionNumber = getNextSessionNumber(existingSessions);
  const newSession: Session = {
    id: message.session_id,
    name: `Claude #${sessionNumber}`,
    color: getSessionColor(sessionNumber - 1),
    messages: [],
    createdAt: now,
    lastActivity: now,
    isAutoNamed: false, // Static names, not auto-generated
  };

  console.log(`[SessionManager] CREATING new session: ${newSession.name} (${newSession.id})`);
  return { session: newSession, isNew: true };
}


/**
 * Add message to session
 */
export function addMessageToSession(
  session: Session,
  message: SpeakMessage
): Session {
  const newMessage: SessionMessage = {
    id: `msg-${Date.now()}-${Math.random().toString(36).substr(2, 9)}`,
    text: message.text,
    timestamp: message.timestamp,
  };

  return {
    ...session,
    messages: [newMessage, ...session.messages],
    lastActivity: Date.now(),
  };
}

/**
 * Rename a session
 */
export function renameSession(
  sessions: Session[],
  sessionId: string,
  newName: string
): Session[] {
  return sessions.map(s =>
    s.id === sessionId
      ? { ...s, name: newName, isAutoNamed: false } // Mark as manually named
      : s
  );
}

/**
 * Change a session's color
 */
export function changeSessionColor(
  sessions: Session[],
  sessionId: string,
  newColor: string
): Session[] {
  return sessions.map(s =>
    s.id === sessionId
      ? { ...s, color: newColor }
      : s
  );
}

/**
 * Update session heartbeat timestamp
 */
export function updateSessionHeartbeat(
  sessions: Session[],
  sessionId: string,
  timestamp: number
): Session[] {
  return sessions.map(s =>
    s.id === sessionId
      ? { ...s, lastHeartbeat: timestamp }
      : s
  );
}

/**
 * Check if a session is active (has heartbeat within threshold)
 */
export function isSessionActive(session: Session, thresholdMs: number = 5 * 60 * 1000): boolean {
  if (!session.lastHeartbeat) return false;
  return (Date.now() - session.lastHeartbeat) < thresholdMs;
}

/**
 * Clear messages from a session
 */
export function clearSessionMessages(
  sessions: Session[],
  sessionId: string
): Session[] {
  return sessions.map(s =>
    s.id === sessionId ? { ...s, messages: [] } : s
  );
}

/**
 * Delete a session
 */
export function deleteSession(
  sessions: Session[],
  sessionId: string
): Session[] {
  return sessions.filter(s => s.id !== sessionId);
}

/**
 * Get relative time string
 */
export function getRelativeTime(timestamp: number): string {
  const now = Date.now();
  const diff = now - timestamp;
  const minutes = Math.floor(diff / 60000);
  const hours = Math.floor(diff / 3600000);
  const days = Math.floor(diff / 86400000);

  if (minutes < 1) return "Just now";
  if (minutes < 60) return `${minutes}m ago`;
  if (hours < 24) return `${hours}h ago`;
  return `${days}d ago`;
}

/**
 * Format time for display
 */
export function formatTime(timestamp: number): string {
  const date = new Date(timestamp);
  return date.toLocaleTimeString("en-US", {
    hour: "numeric",
    minute: "2-digit",
    second: "2-digit",
  });
}
