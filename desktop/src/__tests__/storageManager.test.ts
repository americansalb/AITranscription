/**
 * Tests for storageManager — localStorage cap enforcement utilities.
 *
 * Covers:
 *   - getStorageUsageBytes: byte counting (UTF-16 × 2)
 *   - truncateText: string truncation with ellipsis
 *   - trimHistory: entry cap + text field trimming
 *   - trimVoiceAssignments: key count cap (insertion order)
 *   - enforceStorageCap: full storage budget enforcement
 */
import { describe, it, expect, beforeEach } from "vitest";
import {
  getStorageUsageBytes,
  truncateText,
  trimHistory,
  trimVoiceAssignments,
  enforceStorageCap,
} from "../lib/storageManager";


// =============================================================================
// getStorageUsageBytes
// =============================================================================

describe("getStorageUsageBytes", () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it("returns 0 for empty localStorage", () => {
    expect(getStorageUsageBytes()).toBe(0);
  });

  it("counts key + value bytes (UTF-16: 2 bytes per char)", () => {
    localStorage.setItem("abc", "def"); // key=3 + value=3 = 6 chars × 2 = 12 bytes
    expect(getStorageUsageBytes()).toBe(12);
  });

  it("sums across multiple keys", () => {
    localStorage.setItem("a", "1");   // 1+1 = 2 chars × 2 = 4
    localStorage.setItem("bb", "22"); // 2+2 = 4 chars × 2 = 8
    expect(getStorageUsageBytes()).toBe(12);
  });

  it("handles empty values", () => {
    localStorage.setItem("key", "");  // 3+0 = 3 chars × 2 = 6
    expect(getStorageUsageBytes()).toBe(6);
  });

  it("handles large values", () => {
    const bigValue = "x".repeat(10000);
    localStorage.setItem("k", bigValue); // 1+10000 = 10001 chars × 2 = 20002
    expect(getStorageUsageBytes()).toBe(20002);
  });
});


// =============================================================================
// truncateText
// =============================================================================

describe("truncateText", () => {
  it("returns original text when under max length", () => {
    expect(truncateText("Hello world")).toBe("Hello world");
  });

  it("returns original text when exactly at max length", () => {
    const text = "a".repeat(2000);
    expect(truncateText(text)).toBe(text);
  });

  it("truncates and appends ellipsis when over default max (2000)", () => {
    const text = "a".repeat(2500);
    const result = truncateText(text);
    expect(result.length).toBe(2000);
    expect(result.endsWith("...")).toBe(true);
    // First 1997 chars are 'a', then '...'
    expect(result.slice(0, 1997)).toBe("a".repeat(1997));
  });

  it("respects custom max length", () => {
    const text = "Hello, World!"; // 13 chars
    const result = truncateText(text, 10);
    expect(result.length).toBe(10);
    expect(result).toBe("Hello, ..."); // 7 chars + "..."
  });

  it("handles empty string", () => {
    expect(truncateText("")).toBe("");
  });

  it("handles single character", () => {
    expect(truncateText("x")).toBe("x");
  });

  it("handles custom maxLen exactly at text length", () => {
    expect(truncateText("abc", 3)).toBe("abc");
  });
});


// =============================================================================
// trimHistory
// =============================================================================

describe("trimHistory", () => {
  it("returns all entries when under default max (50)", () => {
    const entries = Array.from({ length: 10 }, (_, i) => ({
      rawText: `Text ${i}`,
      polishedText: `Polished ${i}`,
      timestamp: Date.now() - i * 1000,
    }));
    const result = trimHistory(entries);
    expect(result.length).toBe(10);
  });

  it("trims to max entries, keeping most recent", () => {
    const now = Date.now();
    const entries = Array.from({ length: 100 }, (_, i) => ({
      rawText: `Text ${i}`,
      timestamp: now - i * 1000, // i=0 is most recent
    }));
    const result = trimHistory(entries);
    expect(result.length).toBe(50);
    // Most recent should be kept (timestamp closest to now)
    expect(result[0].rawText).toBe("Text 0");
  });

  it("respects custom max entries", () => {
    const entries = Array.from({ length: 20 }, (_, i) => ({
      rawText: `Text ${i}`,
      timestamp: Date.now() - i * 1000,
    }));
    const result = trimHistory(entries, 5);
    expect(result.length).toBe(5);
  });

  it("sorts by timestamp descending (newest first)", () => {
    const now = Date.now();
    const entries = [
      { rawText: "old", timestamp: now - 10000 },
      { rawText: "newest", timestamp: now },
      { rawText: "middle", timestamp: now - 5000 },
    ];
    const result = trimHistory(entries);
    expect(result[0].rawText).toBe("newest");
    expect(result[1].rawText).toBe("middle");
    expect(result[2].rawText).toBe("old");
  });

  it("truncates rawText fields that exceed MAX_TEXT_LENGTH", () => {
    const longText = "a".repeat(3000);
    const entries = [{ rawText: longText, timestamp: Date.now() }];
    const result = trimHistory(entries);
    expect(result[0].rawText!.length).toBe(2000);
    expect(result[0].rawText!.endsWith("...")).toBe(true);
  });

  it("truncates polishedText fields that exceed MAX_TEXT_LENGTH", () => {
    const longText = "b".repeat(3000);
    const entries = [{ polishedText: longText, timestamp: Date.now() }];
    const result = trimHistory(entries);
    expect(result[0].polishedText!.length).toBe(2000);
    expect(result[0].polishedText!.endsWith("...")).toBe(true);
  });

  it("leaves rawText undefined when input has undefined rawText", () => {
    const entries = [{ timestamp: Date.now() }];
    const result = trimHistory(entries);
    expect(result[0].rawText).toBeUndefined();
  });

  it("handles empty array", () => {
    expect(trimHistory([])).toEqual([]);
  });

  it("handles entries with no timestamp", () => {
    const entries = [{ rawText: "no-ts" }, { rawText: "also-no-ts" }];
    const result = trimHistory(entries);
    expect(result.length).toBe(2);
  });
});


// =============================================================================
// trimVoiceAssignments
// =============================================================================

describe("trimVoiceAssignments", () => {
  it("returns all assignments when under default max (50)", () => {
    const assignments: Record<string, string> = {};
    for (let i = 0; i < 30; i++) {
      assignments[`session-${i}`] = `voice-${i}`;
    }
    const result = trimVoiceAssignments(assignments);
    expect(Object.keys(result).length).toBe(30);
  });

  it("trims to max entries when over limit", () => {
    const assignments: Record<string, string> = {};
    for (let i = 0; i < 100; i++) {
      assignments[`session-${i}`] = `voice-${i}`;
    }
    const result = trimVoiceAssignments(assignments);
    expect(Object.keys(result).length).toBe(50);
  });

  it("keeps the LAST N entries by insertion order", () => {
    const assignments: Record<string, string> = {};
    for (let i = 0; i < 10; i++) {
      assignments[`session-${i}`] = `voice-${i}`;
    }
    const result = trimVoiceAssignments(assignments, 3);
    // Should keep session-7, session-8, session-9 (last 3)
    expect(Object.keys(result)).toEqual(["session-7", "session-8", "session-9"]);
    expect(result["session-7"]).toBe("voice-7");
  });

  it("returns same object when exactly at max", () => {
    const assignments: Record<string, string> = {
      "s1": "v1",
      "s2": "v2",
    };
    const result = trimVoiceAssignments(assignments, 2);
    expect(result).toEqual(assignments);
  });

  it("handles empty object", () => {
    expect(trimVoiceAssignments({})).toEqual({});
  });

  it("respects custom max entries", () => {
    const assignments: Record<string, string> = {};
    for (let i = 0; i < 25; i++) {
      assignments[`s-${i}`] = `v-${i}`;
    }
    const result = trimVoiceAssignments(assignments, 10);
    expect(Object.keys(result).length).toBe(10);
  });
});


// =============================================================================
// enforceStorageCap
// =============================================================================

describe("enforceStorageCap", () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it("does nothing when storage is under 5MB cap", () => {
    localStorage.setItem("some_key", "small value");
    enforceStorageCap();
    expect(localStorage.getItem("some_key")).toBe("small value");
  });

  it("prunes history when over cap", () => {
    // Create a large history to push over 5MB
    const bigEntries = Array.from({ length: 50 }, (_, i) => ({
      rawText: "x".repeat(50000), // 50KB each = 2.5MB for the history alone
      polishedText: "y".repeat(50000),
      timestamp: Date.now() - i * 1000,
    }));
    localStorage.setItem("vaak_history", JSON.stringify(bigEntries));

    // Add more data to push past 5MB
    localStorage.setItem("filler", "z".repeat(500000));

    enforceStorageCap();

    const pruned = JSON.parse(localStorage.getItem("vaak_history") || "[]");
    expect(pruned.length).toBeLessThanOrEqual(10);
  });

  it("prunes sessions when over cap", () => {
    const sessions = Array.from({ length: 20 }, (_, i) => ({
      id: `sess-${i}`,
      name: `Claude #${i + 1}`,
      messages: Array.from({ length: 200 }, (_, j) => ({
        id: `msg-${j}`,
        text: "m".repeat(1000),
        timestamp: Date.now(),
      })),
      lastActivity: Date.now() - i * 1000,
    }));
    localStorage.setItem("vaak_speak_sessions", JSON.stringify(sessions));

    // Add filler to push past 5MB
    localStorage.setItem("filler", "z".repeat(500000));

    enforceStorageCap();

    const pruned = JSON.parse(localStorage.getItem("vaak_speak_sessions") || "[]");
    expect(pruned.length).toBeLessThanOrEqual(10);
    // Messages should be trimmed to 100 each
    if (pruned.length > 0) {
      expect(pruned[0].messages.length).toBeLessThanOrEqual(100);
    }
  });

  it("prunes voice assignments when over cap", () => {
    const assignments: Record<string, string> = {};
    for (let i = 0; i < 100; i++) {
      assignments[`session-${i}`] = `voice-${"x".repeat(1000)}`;
    }
    localStorage.setItem("vaak_voice_assignments", JSON.stringify(assignments));
    // Add filler to push well past 5MB (each char = 2 bytes in UTF-16)
    localStorage.setItem("filler", "z".repeat(2700000));

    enforceStorageCap();

    const prunedRaw = localStorage.getItem("vaak_voice_assignments");
    if (prunedRaw) {
      const pruned = JSON.parse(prunedRaw);
      expect(Object.keys(pruned).length).toBeLessThanOrEqual(20);
    }
  });

  it("handles corrupted history gracefully", () => {
    localStorage.setItem("vaak_history", "not-valid-json{{{");
    // Push over cap
    localStorage.setItem("filler", "z".repeat(3000000));

    enforceStorageCap();

    // Corrupt key should be removed
    expect(localStorage.getItem("vaak_history")).toBeNull();
  });

  it("handles corrupted sessions gracefully", () => {
    localStorage.setItem("vaak_speak_sessions", "corrupt-data");
    localStorage.setItem("filler", "z".repeat(3000000));

    enforceStorageCap();

    expect(localStorage.getItem("vaak_speak_sessions")).toBeNull();
  });
});
