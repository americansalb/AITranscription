/**
 * Storage manager to prevent unbounded localStorage growth.
 * Enforces a global size cap and prunes oldest entries when exceeded.
 */

const MAX_STORAGE_BYTES = 5 * 1024 * 1024; // 5MB total localStorage budget
const MAX_HISTORY_ENTRIES = 50;
const MAX_TEXT_LENGTH = 2000; // Max chars per text field in history entries
const MAX_VOICE_ASSIGNMENTS = 50; // Max cached voice assignments

/** Estimate total localStorage usage in bytes. */
export function getStorageUsageBytes(): number {
  let total = 0;
  for (let i = 0; i < localStorage.length; i++) {
    const key = localStorage.key(i);
    if (key) {
      // Each char is ~2 bytes in JS (UTF-16), keys + values
      total += (key.length + (localStorage.getItem(key)?.length || 0)) * 2;
    }
  }
  return total;
}

/** Truncate a string to max length, appending ellipsis if truncated. */
export function truncateText(text: string, maxLen: number = MAX_TEXT_LENGTH): string {
  if (text.length <= maxLen) return text;
  return text.slice(0, maxLen - 3) + "...";
}

/**
 * Trim history array to max entries and cap text field sizes.
 * Returns the trimmed array (does not write to localStorage).
 */
export function trimHistory<T extends { rawText?: string; polishedText?: string; timestamp?: number }>(
  entries: T[],
  maxEntries: number = MAX_HISTORY_ENTRIES,
): T[] {
  // Sort by timestamp descending (newest first), then trim
  const sorted = [...entries].sort((a, b) => (b.timestamp || 0) - (a.timestamp || 0));
  const trimmed = sorted.slice(0, maxEntries);

  // Cap text field sizes
  return trimmed.map((entry) => ({
    ...entry,
    rawText: entry.rawText ? truncateText(entry.rawText) : entry.rawText,
    polishedText: entry.polishedText ? truncateText(entry.polishedText) : entry.polishedText,
  }));
}

/**
 * Clean up voice assignments object — keep only the most recent N entries.
 * Assignments are { [sessionId]: voiceId } — no timestamp, so we just cap count.
 */
export function trimVoiceAssignments(
  assignments: Record<string, string>,
  maxEntries: number = MAX_VOICE_ASSIGNMENTS,
): Record<string, string> {
  const keys = Object.keys(assignments);
  if (keys.length <= maxEntries) return assignments;

  // Keep last N keys (insertion order in modern JS)
  const kept = keys.slice(-maxEntries);
  const result: Record<string, string> = {};
  for (const key of kept) {
    result[key] = assignments[key];
  }
  return result;
}

/**
 * Check if localStorage is approaching the size cap.
 * If so, aggressively prune the history (largest key).
 */
export function enforceStorageCap(): void {
  const usage = getStorageUsageBytes();
  if (usage <= MAX_STORAGE_BYTES) return;

  // Prune history first (largest variable-size key)
  const historyKey = "vaak_history";
  const historyRaw = localStorage.getItem(historyKey);
  if (historyRaw) {
    try {
      const entries = JSON.parse(historyRaw);
      if (Array.isArray(entries) && entries.length > 10) {
        // Keep only 10 most recent when over cap
        const pruned = trimHistory(entries, 10);
        localStorage.setItem(historyKey, JSON.stringify(pruned));
      }
    } catch {
      // Corrupt data — clear it
      localStorage.removeItem(historyKey);
    }
  }

  // Prune sessions (second largest variable-size key)
  const sessionsKey = "vaak_speak_sessions";
  const sessionsRaw = localStorage.getItem(sessionsKey);
  if (sessionsRaw) {
    try {
      const sessions = JSON.parse(sessionsRaw);
      if (Array.isArray(sessions) && sessions.length > 10) {
        // Keep 10 most recent sessions by lastActivity, trim messages to 100 each
        const sorted = [...sessions].sort((a: { lastActivity?: number }, b: { lastActivity?: number }) =>
          (b.lastActivity || 0) - (a.lastActivity || 0));
        const pruned = sorted.slice(0, 10).map((s: { messages?: unknown[] }) => ({
          ...s,
          messages: Array.isArray(s.messages) ? s.messages.slice(0, 100) : []
        }));
        localStorage.setItem(sessionsKey, JSON.stringify(pruned));
      }
    } catch {
      localStorage.removeItem(sessionsKey);
    }
  }

  // Prune voice assignments
  for (const key of ["vaak_voice_assignments", "vaak_collab_voice_assignments"]) {
    const raw = localStorage.getItem(key);
    if (raw) {
      try {
        const assignments = JSON.parse(raw);
        if (typeof assignments === "object" && assignments !== null) {
          const trimmed = trimVoiceAssignments(assignments, 20);
          localStorage.setItem(key, JSON.stringify(trimmed));
        }
      } catch {
        localStorage.removeItem(key);
      }
    }
  }
}
