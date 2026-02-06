/**
 * Rapid Message Batching - 1-second debounce window combines
 * rapid-fire short messages into one fluid utterance.
 * Critical messages bypass batching. Max 10 messages or 3000 chars per batch.
 */

import { classifyPriority, getStoredPriorityEnabled } from './priorityClassifier';

const MAX_BATCH_MESSAGES = 10;
const MAX_BATCH_CHARS = 3000;
const DEBOUNCE_MS = 1000;

interface PendingMessage {
  text: string;
  sessionId: string;
  timestamp: number;
}

type BatchCallback = (text: string, sessionId: string, batchCount: number) => void;

let pendingBatch: PendingMessage[] = [];
let debounceTimer: ReturnType<typeof setTimeout> | null = null;
let batchCallback: BatchCallback | null = null;

/**
 * Set the callback that receives batched messages.
 */
export function setBatchCallback(cb: BatchCallback): void {
  batchCallback = cb;
}

/**
 * Flush the current batch immediately.
 */
function flushBatch(): void {
  if (debounceTimer) {
    clearTimeout(debounceTimer);
    debounceTimer = null;
  }

  if (pendingBatch.length === 0) return;

  const batch = pendingBatch;
  pendingBatch = [];

  // Group by sessionId (most recent session wins for the combined message)
  const sessionId = batch[batch.length - 1].sessionId;
  const combinedText = batch.map(m => m.text).join('. ');
  const batchCount = batch.length;

  console.log(`[Batcher] Flushing batch of ${batchCount} messages (${combinedText.length} chars)`);
  batchCallback?.(combinedText, sessionId, batchCount);
}

/**
 * Add a message to the batcher. Critical messages bypass batching.
 * Returns true if the message was batched, false if it was sent immediately.
 */
export function batchMessage(text: string, sessionId: string): boolean {
  // Critical messages bypass batching if priority is enabled
  if (getStoredPriorityEnabled() && classifyPriority(text) === 'critical') {
    // Flush any pending batch first
    flushBatch();
    // Send critical message immediately
    batchCallback?.(text, sessionId, 1);
    return false;
  }

  pendingBatch.push({ text, sessionId, timestamp: Date.now() });

  // Check if batch is full
  const totalChars = pendingBatch.reduce((sum, m) => sum + m.text.length, 0);
  if (pendingBatch.length >= MAX_BATCH_MESSAGES || totalChars >= MAX_BATCH_CHARS) {
    flushBatch();
    return true;
  }

  // Reset debounce timer
  if (debounceTimer) {
    clearTimeout(debounceTimer);
  }
  debounceTimer = setTimeout(flushBatch, DEBOUNCE_MS);

  return true;
}

/**
 * Cancel all pending batches.
 */
export function clearBatch(): void {
  if (debounceTimer) {
    clearTimeout(debounceTimer);
    debounceTimer = null;
  }
  pendingBatch = [];
}
