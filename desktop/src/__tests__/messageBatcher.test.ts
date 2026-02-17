/**
 * Tests for messageBatcher — rapid message debounce and batching.
 *
 * Covers:
 *   - setBatchCallback: callback registration
 *   - batchMessage: debouncing, batch size cap, char cap, critical bypass
 *   - clearBatch: cancel pending batches
 *   - Timer-based flush after 1s debounce
 *   - Max 10 messages per batch
 *   - Max 3000 chars per batch
 *   - Critical messages bypass batching (when priority enabled)
 */
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

// Mock priorityClassifier before importing messageBatcher
vi.mock("../lib/priorityClassifier", () => ({
  classifyPriority: vi.fn().mockReturnValue("normal"),
  getStoredPriorityEnabled: vi.fn().mockReturnValue(false),
}));

import {
  setBatchCallback,
  batchMessage,
  clearBatch,
} from "../lib/messageBatcher";

import {
  classifyPriority,
  getStoredPriorityEnabled,
} from "../lib/priorityClassifier";

const mockClassify = vi.mocked(classifyPriority);
const mockPriorityEnabled = vi.mocked(getStoredPriorityEnabled);


describe("messageBatcher", () => {
  let callback: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    vi.useFakeTimers();
    callback = vi.fn();
    setBatchCallback(callback);
    clearBatch();
    mockClassify.mockReturnValue("normal");
    mockPriorityEnabled.mockReturnValue(false);
  });

  afterEach(() => {
    clearBatch();
    vi.useRealTimers();
  });


  // ===========================================================================
  // BASIC BATCHING
  // ===========================================================================

  describe("basic batching", () => {
    it("does not call callback immediately when batching", () => {
      batchMessage("Hello", "session-1");
      expect(callback).not.toHaveBeenCalled();
    });

    it("calls callback after 1s debounce", () => {
      batchMessage("Hello", "session-1");
      vi.advanceTimersByTime(1000);
      expect(callback).toHaveBeenCalledTimes(1);
      expect(callback).toHaveBeenCalledWith("Hello", "session-1", 1);
    });

    it("combines multiple messages with period separator", () => {
      batchMessage("First", "session-1");
      batchMessage("Second", "session-1");
      vi.advanceTimersByTime(1000);
      expect(callback).toHaveBeenCalledTimes(1);
      expect(callback).toHaveBeenCalledWith("First. Second", "session-1", 2);
    });

    it("uses last message session ID for the batch", () => {
      batchMessage("Hello", "session-1");
      batchMessage("World", "session-2");
      vi.advanceTimersByTime(1000);
      expect(callback).toHaveBeenCalledWith("Hello. World", "session-2", 2);
    });

    it("returns true indicating message was batched", () => {
      const result = batchMessage("Hello", "session-1");
      expect(result).toBe(true);
    });

    it("resets debounce timer on each new message", () => {
      batchMessage("First", "session-1");
      vi.advanceTimersByTime(800); // 800ms
      batchMessage("Second", "session-1");
      vi.advanceTimersByTime(800); // Total: 1600ms from first, 800ms from second
      expect(callback).not.toHaveBeenCalled();
      vi.advanceTimersByTime(200); // Now 1000ms from second
      expect(callback).toHaveBeenCalledTimes(1);
    });
  });


  // ===========================================================================
  // BATCH SIZE CAP (10 messages)
  // ===========================================================================

  describe("batch size cap", () => {
    it("flushes at 10 messages", () => {
      for (let i = 0; i < 10; i++) {
        batchMessage(`msg-${i}`, "session-1");
      }
      // Should flush immediately at 10, no timer needed
      expect(callback).toHaveBeenCalledTimes(1);
      expect(callback.mock.calls[0][2]).toBe(10); // batchCount = 10
    });

    it("does not flush at 9 messages", () => {
      for (let i = 0; i < 9; i++) {
        batchMessage(`msg-${i}`, "session-1");
      }
      expect(callback).not.toHaveBeenCalled();
    });

    it("starts new batch after flush from cap", () => {
      for (let i = 0; i < 10; i++) {
        batchMessage(`msg-${i}`, "session-1");
      }
      expect(callback).toHaveBeenCalledTimes(1);

      // Next message starts a new batch
      batchMessage("new-batch", "session-1");
      vi.advanceTimersByTime(1000);
      expect(callback).toHaveBeenCalledTimes(2);
      expect(callback.mock.calls[1][0]).toBe("new-batch");
    });
  });


  // ===========================================================================
  // BATCH CHAR CAP (3000 chars)
  // ===========================================================================

  describe("batch char cap", () => {
    it("flushes when total chars reach 3000", () => {
      // Each message is 1500 chars — 2 messages = 3000 chars
      batchMessage("x".repeat(1500), "session-1");
      batchMessage("y".repeat(1500), "session-1");
      expect(callback).toHaveBeenCalledTimes(1);
    });

    it("does not flush when total chars under 3000", () => {
      batchMessage("x".repeat(1499), "session-1");
      batchMessage("y".repeat(1499), "session-1");
      expect(callback).not.toHaveBeenCalled();
    });
  });


  // ===========================================================================
  // CRITICAL MESSAGE BYPASS
  // ===========================================================================

  describe("critical message bypass", () => {
    it("sends critical messages immediately when priority enabled", () => {
      mockPriorityEnabled.mockReturnValue(true);
      mockClassify.mockReturnValue("critical");

      const result = batchMessage("ERROR: build failed", "session-1");
      expect(result).toBe(false); // false = not batched, sent immediately
      expect(callback).toHaveBeenCalledTimes(1);
      expect(callback).toHaveBeenCalledWith("ERROR: build failed", "session-1", 1);
    });

    it("flushes pending batch before sending critical message", () => {
      mockPriorityEnabled.mockReturnValue(true);
      mockClassify.mockReturnValueOnce("normal"); // First message: normal

      batchMessage("Normal message", "session-1");
      expect(callback).not.toHaveBeenCalled();

      mockClassify.mockReturnValueOnce("critical"); // Second message: critical
      batchMessage("CRITICAL ERROR", "session-1");

      // Should have flushed the pending normal message first, then sent critical
      expect(callback).toHaveBeenCalledTimes(2);
      expect(callback.mock.calls[0][0]).toBe("Normal message"); // Flushed batch
      expect(callback.mock.calls[1][0]).toBe("CRITICAL ERROR"); // Immediate
    });

    it("does NOT bypass when priority is disabled", () => {
      mockPriorityEnabled.mockReturnValue(false);
      mockClassify.mockReturnValue("critical");

      const result = batchMessage("ERROR: build failed", "session-1");
      expect(result).toBe(true); // batched normally
      expect(callback).not.toHaveBeenCalled();
    });
  });


  // ===========================================================================
  // CLEAR BATCH
  // ===========================================================================

  describe("clearBatch", () => {
    it("cancels pending batch", () => {
      batchMessage("Hello", "session-1");
      clearBatch();
      vi.advanceTimersByTime(2000);
      expect(callback).not.toHaveBeenCalled();
    });

    it("is safe to call when no batch is pending", () => {
      expect(() => clearBatch()).not.toThrow();
    });

    it("allows new batches after clearing", () => {
      batchMessage("First", "session-1");
      clearBatch();

      batchMessage("Second", "session-1");
      vi.advanceTimersByTime(1000);
      expect(callback).toHaveBeenCalledTimes(1);
      expect(callback).toHaveBeenCalledWith("Second", "session-1", 1);
    });
  });


  // ===========================================================================
  // NO CALLBACK SET
  // ===========================================================================

  describe("no callback", () => {
    it("does not crash when no callback is set", () => {
      setBatchCallback(null as unknown as (text: string, sessionId: string, batchCount: number) => void);
      batchMessage("Hello", "session-1");
      expect(() => vi.advanceTimersByTime(1000)).not.toThrow();
    });
  });
});
