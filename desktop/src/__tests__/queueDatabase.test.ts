/**
 * Tests for queueDatabase — Tauri invoke wrappers with snake_case → camelCase conversion.
 *
 * Covers:
 *   - addQueueItem: invoke call + snake→camel conversion
 *   - getQueueItems: array mapping + optional filters
 *   - updateQueueItemStatus: invoke parameters
 *   - reorderQueueItem: invoke parameters
 *   - removeQueueItem: invoke parameters
 *   - clearCompletedItems: return value passthrough
 *   - getPendingCount: return value passthrough
 *   - getNextPendingItem: null handling + conversion
 *   - snake_case → camelCase field mapping (via convertToCamelCase)
 */
import { describe, it, expect, vi, beforeEach } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import {
  addQueueItem,
  getQueueItems,
  updateQueueItemStatus,
  reorderQueueItem,
  removeQueueItem,
  clearCompletedItems,
  getPendingCount,
  getNextPendingItem,
} from "../lib/queueDatabase";

const mockInvoke = vi.mocked(invoke);


// =============================================================================
// FIXTURE: snake_case item as returned by Rust backend
// =============================================================================

function makeSnakeCaseItem(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    id: 1,
    uuid: "abc-123",
    session_id: "sess-001",
    text: "Hello world",
    status: "pending",
    position: 0,
    created_at: 1700000000,
    started_at: undefined,
    completed_at: undefined,
    duration_ms: undefined,
    error_message: undefined,
    ...overrides,
  };
}


beforeEach(() => {
  mockInvoke.mockReset();
});


// =============================================================================
// snake_case → camelCase CONVERSION (tested indirectly via addQueueItem)
// =============================================================================

describe("snake_case → camelCase conversion", () => {
  it("maps all snake_case fields to camelCase", async () => {
    const snakeItem = makeSnakeCaseItem({
      id: 42,
      uuid: "test-uuid",
      session_id: "my-session",
      text: "Test text",
      status: "playing",
      position: 3,
      created_at: 1700000001,
      started_at: 1700000002,
      completed_at: 1700000003,
      duration_ms: 5000,
      error_message: "Something failed",
    });
    mockInvoke.mockResolvedValueOnce(snakeItem);

    const result = await addQueueItem("Test text", "my-session");

    expect(result.id).toBe(42);
    expect(result.uuid).toBe("test-uuid");
    expect(result.sessionId).toBe("my-session");
    expect(result.text).toBe("Test text");
    expect(result.status).toBe("playing");
    expect(result.position).toBe(3);
    expect(result.createdAt).toBe(1700000001);
    expect(result.startedAt).toBe(1700000002);
    expect(result.completedAt).toBe(1700000003);
    expect(result.durationMs).toBe(5000);
    expect(result.errorMessage).toBe("Something failed");
  });

  it("handles undefined optional fields", async () => {
    mockInvoke.mockResolvedValueOnce(makeSnakeCaseItem());

    const result = await addQueueItem("Hello", "sess-001");

    expect(result.startedAt).toBeUndefined();
    expect(result.completedAt).toBeUndefined();
    expect(result.durationMs).toBeUndefined();
    expect(result.errorMessage).toBeUndefined();
  });
});


// =============================================================================
// addQueueItem
// =============================================================================

describe("addQueueItem", () => {
  it("invokes 'add_queue_item' with text and sessionId", async () => {
    mockInvoke.mockResolvedValueOnce(makeSnakeCaseItem());

    await addQueueItem("Hello world", "sess-001");

    expect(mockInvoke).toHaveBeenCalledWith("add_queue_item", {
      text: "Hello world",
      sessionId: "sess-001",
    });
  });

  it("returns converted QueueItem", async () => {
    mockInvoke.mockResolvedValueOnce(makeSnakeCaseItem({ uuid: "new-item" }));

    const result = await addQueueItem("Test", "sess");
    expect(result.uuid).toBe("new-item");
    expect(result.sessionId).toBe("sess-001");
  });
});


// =============================================================================
// getQueueItems
// =============================================================================

describe("getQueueItems", () => {
  it("invokes 'get_queue_items' with no filters when called with no args", async () => {
    mockInvoke.mockResolvedValueOnce([]);

    await getQueueItems();

    expect(mockInvoke).toHaveBeenCalledWith("get_queue_items", {
      status: undefined,
      sessionId: undefined,
      limit: undefined,
    });
  });

  it("passes filter options to invoke", async () => {
    mockInvoke.mockResolvedValueOnce([]);

    await getQueueItems({ status: "pending", sessionId: "sess-1", limit: 10 });

    expect(mockInvoke).toHaveBeenCalledWith("get_queue_items", {
      status: "pending",
      sessionId: "sess-1",
      limit: 10,
    });
  });

  it("converts each item in the returned array", async () => {
    mockInvoke.mockResolvedValueOnce([
      makeSnakeCaseItem({ uuid: "item-1", session_id: "s1" }),
      makeSnakeCaseItem({ uuid: "item-2", session_id: "s2" }),
    ]);

    const results = await getQueueItems();

    expect(results).toHaveLength(2);
    expect(results[0].uuid).toBe("item-1");
    expect(results[0].sessionId).toBe("s1");
    expect(results[1].uuid).toBe("item-2");
    expect(results[1].sessionId).toBe("s2");
  });

  it("returns empty array when no items", async () => {
    mockInvoke.mockResolvedValueOnce([]);

    const results = await getQueueItems();
    expect(results).toEqual([]);
  });
});


// =============================================================================
// updateQueueItemStatus
// =============================================================================

describe("updateQueueItemStatus", () => {
  it("invokes 'update_queue_item_status' with required params", async () => {
    mockInvoke.mockResolvedValueOnce(undefined);

    await updateQueueItemStatus("uuid-1", "completed");

    expect(mockInvoke).toHaveBeenCalledWith("update_queue_item_status", {
      uuid: "uuid-1",
      status: "completed",
      durationMs: undefined,
      errorMessage: undefined,
    });
  });

  it("passes optional durationMs and errorMessage", async () => {
    mockInvoke.mockResolvedValueOnce(undefined);

    await updateQueueItemStatus("uuid-1", "failed", 3000, "Playback error");

    expect(mockInvoke).toHaveBeenCalledWith("update_queue_item_status", {
      uuid: "uuid-1",
      status: "failed",
      durationMs: 3000,
      errorMessage: "Playback error",
    });
  });
});


// =============================================================================
// reorderQueueItem
// =============================================================================

describe("reorderQueueItem", () => {
  it("invokes 'reorder_queue_item' with uuid and newPosition", async () => {
    mockInvoke.mockResolvedValueOnce(undefined);

    await reorderQueueItem("uuid-1", 5);

    expect(mockInvoke).toHaveBeenCalledWith("reorder_queue_item", {
      uuid: "uuid-1",
      newPosition: 5,
    });
  });
});


// =============================================================================
// removeQueueItem
// =============================================================================

describe("removeQueueItem", () => {
  it("invokes 'remove_queue_item' with uuid", async () => {
    mockInvoke.mockResolvedValueOnce(undefined);

    await removeQueueItem("uuid-1");

    expect(mockInvoke).toHaveBeenCalledWith("remove_queue_item", {
      uuid: "uuid-1",
    });
  });
});


// =============================================================================
// clearCompletedItems
// =============================================================================

describe("clearCompletedItems", () => {
  it("invokes 'clear_completed_items' and returns count", async () => {
    mockInvoke.mockResolvedValueOnce(7);

    const count = await clearCompletedItems();

    expect(mockInvoke).toHaveBeenCalledWith("clear_completed_items", {
      olderThanDays: undefined,
    });
    expect(count).toBe(7);
  });

  it("passes olderThanDays when provided", async () => {
    mockInvoke.mockResolvedValueOnce(3);

    await clearCompletedItems(30);

    expect(mockInvoke).toHaveBeenCalledWith("clear_completed_items", {
      olderThanDays: 30,
    });
  });
});


// =============================================================================
// getPendingCount
// =============================================================================

describe("getPendingCount", () => {
  it("invokes 'get_pending_count' and returns number", async () => {
    mockInvoke.mockResolvedValueOnce(42);

    const count = await getPendingCount();

    expect(mockInvoke).toHaveBeenCalledWith("get_pending_count");
    expect(count).toBe(42);
  });

  it("returns 0 when no pending items", async () => {
    mockInvoke.mockResolvedValueOnce(0);

    const count = await getPendingCount();
    expect(count).toBe(0);
  });
});


// =============================================================================
// getNextPendingItem
// =============================================================================

describe("getNextPendingItem", () => {
  it("returns converted QueueItem when item exists", async () => {
    mockInvoke.mockResolvedValueOnce(makeSnakeCaseItem({ uuid: "next-item" }));

    const result = await getNextPendingItem();

    expect(result).not.toBeNull();
    expect(result!.uuid).toBe("next-item");
    expect(result!.sessionId).toBe("sess-001");
  });

  it("returns null when no pending items", async () => {
    mockInvoke.mockResolvedValueOnce(null);

    const result = await getNextPendingItem();
    expect(result).toBeNull();
  });
});
