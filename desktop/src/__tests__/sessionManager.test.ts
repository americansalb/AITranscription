/**
 * Tests for sessionManager — T1-1 localStorage cap enforcement.
 *
 * Covers:
 *   - Session cap at MAX_SESSIONS (20)
 *   - Pruning by lastActivity (most recent kept)
 *   - Message trimming (max 500 per session)
 *   - Load/save round-trip
 *   - Session numbering: sequential, gap-filling
 *   - getOrCreateSession: find existing vs create new
 *   - Session CRUD: rename, changeColor, delete, clearMessages
 *   - Heartbeat tracking and isSessionActive
 *   - Relative time formatting
 */
import { describe, it, expect, beforeEach } from "vitest";
import {
  loadSessions,
  saveSessions,
  getOrCreateSession,
  createSessionFromHeartbeat,
  addMessageToSession,
  renameSession,
  changeSessionColor,
  deleteSession,
  clearSessionMessages,
  updateSessionHeartbeat,
  isSessionActive,
  getRelativeTime,
  SESSION_COLORS,
  type Session,
  type SpeakMessage,
} from "../lib/sessionManager";


// Helper to create a mock session
function makeSession(overrides: Partial<Session> = {}): Session {
  return {
    id: `session-${Math.random().toString(36).slice(2, 8)}`,
    name: "Claude #1",
    color: "#3B82F6",
    messages: [],
    createdAt: Date.now(),
    lastActivity: Date.now(),
    ...overrides,
  };
}

// Helper to create N sessions with staggered lastActivity
function makeSessions(count: number): Session[] {
  const now = Date.now();
  return Array.from({ length: count }, (_, i) =>
    makeSession({
      id: `session-${i}`,
      name: `Claude #${i + 1}`,
      color: SESSION_COLORS[i % SESSION_COLORS.length],
      lastActivity: now - (count - i) * 1000, // Older sessions have earlier lastActivity
    })
  );
}


// =============================================================================
// SESSION CAP (T1-1 core fix)
// =============================================================================

describe("session cap enforcement (T1-1)", () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it("keeps at most 20 sessions when saving", () => {
    const sessions = makeSessions(25);
    saveSessions(sessions);

    const loaded = loadSessions();
    expect(loaded.length).toBe(20);
  });

  it("preserves the 20 most recent sessions by lastActivity", () => {
    const sessions = makeSessions(25);
    saveSessions(sessions);

    const loaded = loadSessions();
    // Sessions 5-24 (0-indexed) should survive — they have higher lastActivity
    const loadedIds = loaded.map(s => s.id);
    for (let i = 5; i < 25; i++) {
      expect(loadedIds).toContain(`session-${i}`);
    }
    // Sessions 0-4 should be pruned (oldest)
    for (let i = 0; i < 5; i++) {
      expect(loadedIds).not.toContain(`session-${i}`);
    }
  });

  it("does not prune when at or below the cap", () => {
    const sessions = makeSessions(20);
    saveSessions(sessions);

    const loaded = loadSessions();
    expect(loaded.length).toBe(20);
  });

  it("handles exactly 1 session", () => {
    const sessions = [makeSession()];
    saveSessions(sessions);

    const loaded = loadSessions();
    expect(loaded.length).toBe(1);
  });

  it("handles empty sessions array", () => {
    saveSessions([]);

    const loaded = loadSessions();
    expect(loaded.length).toBe(0);
  });
});


// =============================================================================
// MESSAGE TRIMMING
// =============================================================================

describe("message trimming per session", () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it("trims messages to 500 per session on save", () => {
    const messages = Array.from({ length: 600 }, (_, i) => ({
      id: `msg-${i}`,
      text: `Message ${i}`,
      timestamp: Date.now() - i * 1000,
    }));

    const session = makeSession({ messages });
    saveSessions([session]);

    const loaded = loadSessions();
    expect(loaded[0].messages.length).toBe(500);
  });

  it("preserves messages when under 500", () => {
    const messages = Array.from({ length: 100 }, (_, i) => ({
      id: `msg-${i}`,
      text: `Message ${i}`,
      timestamp: Date.now(),
    }));

    const session = makeSession({ messages });
    saveSessions([session]);

    const loaded = loadSessions();
    expect(loaded[0].messages.length).toBe(100);
  });
});


// =============================================================================
// LOAD / SAVE ROUND-TRIP
// =============================================================================

describe("load and save", () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it("loadSessions returns empty array when no data", () => {
    expect(loadSessions()).toEqual([]);
  });

  it("round-trips sessions through localStorage", () => {
    const sessions = [
      makeSession({ id: "s1", name: "Claude #1" }),
      makeSession({ id: "s2", name: "Claude #2" }),
    ];
    saveSessions(sessions);

    const loaded = loadSessions();
    expect(loaded.length).toBe(2);
    expect(loaded.find(s => s.id === "s1")?.name).toBe("Claude #1");
    expect(loaded.find(s => s.id === "s2")?.name).toBe("Claude #2");
  });

  it("handles corrupted localStorage gracefully", () => {
    localStorage.setItem("vaak_speak_sessions", "not-valid-json{{{");
    expect(loadSessions()).toEqual([]);
  });
});


// =============================================================================
// SESSION NUMBERING
// =============================================================================

describe("session numbering", () => {
  it("first session gets number 1", () => {
    const message: SpeakMessage = {
      text: "Hello",
      session_id: "new-session",
      timestamp: Date.now(),
    };
    const { session } = getOrCreateSession(message, []);
    expect(session.name).toBe("Claude #1");
  });

  it("second session gets number 2", () => {
    const existing = [makeSession({ name: "Claude #1" })];
    const message: SpeakMessage = {
      text: "Hello",
      session_id: "new-session",
      timestamp: Date.now(),
    };
    const { session } = getOrCreateSession(message, existing);
    expect(session.name).toBe("Claude #2");
  });

  it("fills gaps in numbering", () => {
    const existing = [
      makeSession({ name: "Claude #1" }),
      makeSession({ name: "Claude #3" }),
    ];
    const message: SpeakMessage = {
      text: "Hello",
      session_id: "new-session",
      timestamp: Date.now(),
    };
    const { session } = getOrCreateSession(message, existing);
    expect(session.name).toBe("Claude #2");
  });
});


// =============================================================================
// GET OR CREATE SESSION
// =============================================================================

describe("getOrCreateSession", () => {
  it("finds existing session by id", () => {
    const existing = [makeSession({ id: "existing-id", name: "Claude #1" })];
    const message: SpeakMessage = {
      text: "Hello",
      session_id: "existing-id",
      timestamp: Date.now(),
    };
    const { session, isNew } = getOrCreateSession(message, existing);
    expect(isNew).toBe(false);
    expect(session.id).toBe("existing-id");
    expect(session.name).toBe("Claude #1");
  });

  it("creates new session when id not found", () => {
    const existing = [makeSession({ id: "other-id" })];
    const message: SpeakMessage = {
      text: "Hello",
      session_id: "brand-new-id",
      timestamp: Date.now(),
    };
    const { session, isNew } = getOrCreateSession(message, existing);
    expect(isNew).toBe(true);
    expect(session.id).toBe("brand-new-id");
  });

  it("assigns a color from SESSION_COLORS", () => {
    const message: SpeakMessage = {
      text: "Hello",
      session_id: "new",
      timestamp: Date.now(),
    };
    const { session } = getOrCreateSession(message, []);
    expect(SESSION_COLORS).toContain(session.color);
  });
});


// =============================================================================
// SESSION CRUD
// =============================================================================

describe("session mutations", () => {
  it("renameSession changes name and sets isAutoNamed false", () => {
    const sessions = [makeSession({ id: "s1", name: "Claude #1", isAutoNamed: true })];
    const updated = renameSession(sessions, "s1", "My Custom Name");
    expect(updated[0].name).toBe("My Custom Name");
    expect(updated[0].isAutoNamed).toBe(false);
  });

  it("changeSessionColor updates color", () => {
    const sessions = [makeSession({ id: "s1", color: "#3B82F6" })];
    const updated = changeSessionColor(sessions, "s1", "#EF4444");
    expect(updated[0].color).toBe("#EF4444");
  });

  it("deleteSession removes the session", () => {
    const sessions = [
      makeSession({ id: "s1" }),
      makeSession({ id: "s2" }),
    ];
    const updated = deleteSession(sessions, "s1");
    expect(updated.length).toBe(1);
    expect(updated[0].id).toBe("s2");
  });

  it("clearSessionMessages empties messages array", () => {
    const sessions = [makeSession({
      id: "s1",
      messages: [{ id: "m1", text: "Hello", timestamp: Date.now() }],
    })];
    const updated = clearSessionMessages(sessions, "s1");
    expect(updated[0].messages).toEqual([]);
  });

  it("addMessageToSession prepends message", () => {
    const session = makeSession({ id: "s1", messages: [] });
    const message: SpeakMessage = {
      text: "New message",
      session_id: "s1",
      timestamp: Date.now(),
    };
    const updated = addMessageToSession(session, message);
    expect(updated.messages.length).toBe(1);
    expect(updated.messages[0].text).toBe("New message");
  });
});


// =============================================================================
// HEARTBEAT & ACTIVITY
// =============================================================================

describe("heartbeat tracking", () => {
  it("updateSessionHeartbeat sets lastHeartbeat", () => {
    const sessions = [makeSession({ id: "s1" })];
    const ts = Date.now();
    const updated = updateSessionHeartbeat(sessions, "s1", ts);
    expect(updated[0].lastHeartbeat).toBe(ts);
  });

  it("isSessionActive returns true for recent heartbeat", () => {
    const session = makeSession({ lastHeartbeat: Date.now() - 1000 });
    expect(isSessionActive(session)).toBe(true);
  });

  it("isSessionActive returns false for old heartbeat", () => {
    const session = makeSession({ lastHeartbeat: Date.now() - 10 * 60 * 1000 });
    expect(isSessionActive(session)).toBe(false);
  });

  it("isSessionActive returns false with no heartbeat", () => {
    const session = makeSession({ lastHeartbeat: undefined });
    expect(isSessionActive(session)).toBe(false);
  });

  it("isSessionActive respects custom threshold", () => {
    const session = makeSession({ lastHeartbeat: Date.now() - 3000 });
    expect(isSessionActive(session, 5000)).toBe(true);
    expect(isSessionActive(session, 2000)).toBe(false);
  });
});


// =============================================================================
// RELATIVE TIME
// =============================================================================

describe("getRelativeTime", () => {
  it("returns 'Just now' for recent timestamps", () => {
    expect(getRelativeTime(Date.now())).toBe("Just now");
  });

  it("returns minutes for timestamps within an hour", () => {
    const fiveMinAgo = Date.now() - 5 * 60 * 1000;
    expect(getRelativeTime(fiveMinAgo)).toBe("5m ago");
  });

  it("returns hours for timestamps within a day", () => {
    const threeHoursAgo = Date.now() - 3 * 3600 * 1000;
    expect(getRelativeTime(threeHoursAgo)).toBe("3h ago");
  });

  it("returns days for older timestamps", () => {
    const twoDaysAgo = Date.now() - 2 * 86400 * 1000;
    expect(getRelativeTime(twoDaysAgo)).toBe("2d ago");
  });
});


// =============================================================================
// CREATE FROM HEARTBEAT
// =============================================================================

describe("createSessionFromHeartbeat", () => {
  it("creates a session with heartbeat timestamp", () => {
    const ts = Date.now();
    const session = createSessionFromHeartbeat("hb-session", [], ts);
    expect(session.id).toBe("hb-session");
    expect(session.lastHeartbeat).toBe(ts);
    expect(session.createdAt).toBe(ts);
    expect(session.name).toBe("Claude #1");
  });
});
