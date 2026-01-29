import { invoke } from "@tauri-apps/api/core";
import type { QueueItem, QueueItemStatus } from "./queueTypes";

// Convert snake_case from Rust to camelCase for TypeScript
function convertToCamelCase(item: Record<string, unknown>): QueueItem {
  return {
    id: item.id as number,
    uuid: item.uuid as string,
    sessionId: item.session_id as string,
    text: item.text as string,
    status: item.status as QueueItemStatus,
    position: item.position as number,
    createdAt: item.created_at as number,
    startedAt: item.started_at as number | undefined,
    completedAt: item.completed_at as number | undefined,
    durationMs: item.duration_ms as number | undefined,
    errorMessage: item.error_message as string | undefined,
  };
}

// Add a new item to the queue
export async function addQueueItem(text: string, sessionId: string): Promise<QueueItem> {
  const result = await invoke<Record<string, unknown>>("add_queue_item", {
    text,
    sessionId,
  });
  return convertToCamelCase(result);
}

// Get queue items with optional filtering
export async function getQueueItems(options?: {
  status?: QueueItemStatus;
  sessionId?: string;
  limit?: number;
}): Promise<QueueItem[]> {
  const results = await invoke<Record<string, unknown>[]>("get_queue_items", {
    status: options?.status,
    sessionId: options?.sessionId,
    limit: options?.limit,
  });
  return results.map(convertToCamelCase);
}

// Update queue item status
export async function updateQueueItemStatus(
  uuid: string,
  status: QueueItemStatus,
  durationMs?: number,
  errorMessage?: string
): Promise<void> {
  await invoke("update_queue_item_status", {
    uuid,
    status,
    durationMs,
    errorMessage,
  });
}

// Reorder a queue item
export async function reorderQueueItem(uuid: string, newPosition: number): Promise<void> {
  console.log("[queueDatabase] reorderQueueItem called:", uuid, newPosition);
  await invoke("reorder_queue_item", { uuid, newPosition });
  console.log("[queueDatabase] reorderQueueItem completed");
}

// Remove a queue item
export async function removeQueueItem(uuid: string): Promise<void> {
  await invoke("remove_queue_item", { uuid });
}

// Clear completed items
export async function clearCompletedItems(olderThanDays?: number): Promise<number> {
  return invoke<number>("clear_completed_items", { olderThanDays });
}

// Get pending count
export async function getPendingCount(): Promise<number> {
  return invoke<number>("get_pending_count");
}

// Get next pending item
export async function getNextPendingItem(): Promise<QueueItem | null> {
  const result = await invoke<Record<string, unknown> | null>("get_next_pending_item");
  return result ? convertToCamelCase(result) : null;
}
