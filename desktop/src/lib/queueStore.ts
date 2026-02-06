import { useState, useEffect } from "react";
import type { QueueItem, QueueState, QueueItemStatus } from "./queueTypes";
import * as db from "./queueDatabase";
import { getStoredVoiceEnabled } from "./voiceStream";

// Check if this is the main window (has audio playback) or a secondary window
const hash = window.location.hash;
const isMainWindow = !hash.includes("/transcript") && !hash.includes("/queue");

// Send playback command to main window via Tauri event
async function sendPlaybackCommand(command: string, args?: Record<string, unknown>): Promise<void> {
  try {
    const { emit } = await import("@tauri-apps/api/event");
    await emit("queue-playback-command", { command, ...args });
    log(`Sent playback command to main window: ${command}`);
  } catch (e) {
    logError("Failed to send playback command:", e);
  }
}

// Default state
const defaultState: QueueState = {
  items: [],
  currentItem: null,
  isPlaying: false,
  isPaused: false,
  autoPlay: true,
  volume: 1.0,
  currentPosition: 0,
  interrupted: false,
  playbackSpeed: 1.0,
};

// Store listeners
type Listener = () => void;
const listeners = new Set<Listener>();

// Global state
let state: QueueState = { ...defaultState };
let currentAudio: HTMLAudioElement | null = null;
let audioStartTime: number = 0;

// Real-time audio position tracking
let audioDurationMs: number = 0;
let audioCurrentTimeMs: number = 0;

// Shared playback info (updated by main window, read by all windows)
let sharedPlaybackInfo = { currentTimeMs: 0, durationMs: 0 };

// Get real audio playback info
export function getAudioPlaybackInfo(): { currentTimeMs: number; durationMs: number } {
  if (currentAudio) {
    // Always return currentTime if audio exists
    const currentTimeMs = currentAudio.currentTime * 1000;
    // Duration might be NaN before metadata loads - use tracked value as fallback
    const durationMs = !isNaN(currentAudio.duration)
      ? currentAudio.duration * 1000
      : audioDurationMs;
    return { currentTimeMs, durationMs };
  }
  // Non-main windows use shared info from broadcasts
  if (!isMainWindow) {
    return sharedPlaybackInfo;
  }
  return {
    currentTimeMs: audioCurrentTimeMs,
    durationMs: audioDurationMs,
  };
}

// Seek to a position in the current audio (in milliseconds)
export function seek(positionMs: number): void {
  log(`seek(${positionMs}ms) called`);
  if (!isMainWindow) {
    sendPlaybackCommand("seek", { positionMs });
    return;
  }
  if (currentAudio && !isNaN(currentAudio.duration)) {
    const clampedMs = Math.max(0, Math.min(positionMs, currentAudio.duration * 1000));
    currentAudio.currentTime = clampedMs / 1000;
    audioCurrentTimeMs = clampedMs;
    log(`Seeked to ${clampedMs}ms`);
  }
}

// Mutex to prevent race conditions when starting playback
let isStartingPlayback = false;

// Logging helper
function log(message: string, ...args: unknown[]): void {
  console.log(`[QueueStore] ${message}`, ...args);
}

function logError(message: string, ...args: unknown[]): void {
  console.error(`[QueueStore] ${message}`, ...args);
}

// Get API URL from environment
function getApiUrl(): string {
  return (import.meta.env?.VITE_API_URL as string) || "http://127.0.0.1:19836";
}

// Notify all listeners of state change
function notify(): void {
  log(`Notifying ${listeners.size} listeners of state change`);
  listeners.forEach((listener) => listener());
}

// Broadcast state to other windows via Tauri events
async function broadcastState(): Promise<void> {
  if (!isMainWindow) return; // Only main window broadcasts
  try {
    const { emit } = await import("@tauri-apps/api/event");
    // Send serializable subset of state (no functions/audio elements)
    await emit("queue-state-sync", {
      isPlaying: state.isPlaying,
      isPaused: state.isPaused,
      autoPlay: state.autoPlay,
      volume: state.volume,
      currentPosition: state.currentPosition,
      interrupted: state.interrupted,
      playbackSpeed: state.playbackSpeed,
      currentItem: state.currentItem,
      items: state.items,
    });
  } catch {
    // Ignore broadcast errors
  }
}

// Broadcast playback position to other windows
let lastBroadcastTime = 0;
async function broadcastPlaybackInfo(): Promise<void> {
  if (!isMainWindow) return;
  // Throttle to every 200ms to avoid flooding
  const now = Date.now();
  if (now - lastBroadcastTime < 200) return;
  lastBroadcastTime = now;
  try {
    const { emit } = await import("@tauri-apps/api/event");
    const info = getAudioPlaybackInfo();
    await emit("queue-playback-info", info);
  } catch {
    // Ignore
  }
}

// Update state and notify
function setState(partial: Partial<QueueState>): void {
  state = { ...state, ...partial };
  notify();
  broadcastState();
}

// Get current state (immutable)
export function getState(): Readonly<QueueState> {
  return state;
}

// Subscribe to state changes
export function subscribe(listener: Listener): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

// Load items from database
export async function loadItems(): Promise<void> {
  try {
    const items = await db.getQueueItems({});

    // Count by status for debugging
    const pending = items.filter(i => i.status === "pending").length;
    const playing = items.filter(i => i.status === "playing").length;
    const completed = items.filter(i => i.status === "completed").length;
    const failed = items.filter(i => i.status === "failed").length;

    // Only log if something changed
    if (items.length !== state.items.length || pending > 0 || playing > 0) {
      log(`Loaded ${items.length} items: ${pending} pending, ${playing} playing, ${completed} completed, ${failed} failed`);
    }

    setState({ items: enrichWithSessionInfo(items) });
  } catch (error) {
    logError("Failed to load items:", error);
  }
}

// Auto-start timer for delayed playback
let autoStartTimer: ReturnType<typeof setTimeout> | null = null;

// Add item to queue (called when speak event arrives)
export async function addItem(text: string, sessionId: string, existingItem?: QueueItem, _batchCount?: number, voiceIdOverride?: string): Promise<QueueItem | null> {
  log(`Adding item: "${text.substring(0, 50)}..." (session: ${sessionId})`);
  log(`Current state: isPlaying=${state.isPlaying}, isPaused=${state.isPaused}, isStartingPlayback=${isStartingPlayback}, autoPlay=${state.autoPlay}, items=${state.items.length}`);

  try {
    // If we already have the item from the backend event, use it
    const item = existingItem || await db.addQueueItem(text, sessionId);
    // Apply voice override if provided (e.g. screen reader voice)
    if (voiceIdOverride) {
      item.voiceId = voiceIdOverride;
    }
    log(`Item created with uuid: ${item.uuid}, position: ${item.position}`);

    // Enrich with session info
    if (item.sessionId.startsWith("screen-reader")) {
      // Auto-label screen reader items (Alt+R describe / Alt+A ask)
      item.sessionName = item.sessionId.startsWith("screen-reader-ask")
        ? "Screen Reader (Ask)"
        : item.sessionId.startsWith("screen-reader-err")
          ? "Screen Reader (Error)"
          : "Screen Reader";
      item.sessionColor = "#6366f1"; // Indigo to distinguish from Claude sessions
    } else {
      const cached = sessionCache[item.sessionId];
      if (cached) {
        item.sessionName = cached.name;
        item.sessionColor = cached.color;
      }
    }

    // Add to local state
    const newItems = [...state.items, item];
    setState({ items: newItems });
    log(`Queue now has ${newItems.length} items`);

    // Auto-start playback if enabled and nothing is playing
    log(`Auto-start check: autoPlay=${state.autoPlay}, isPlaying=${state.isPlaying}, isPaused=${state.isPaused}, isStartingPlayback=${isStartingPlayback}`);
    if (state.autoPlay && !state.isPlaying && !state.isPaused && !isStartingPlayback) {
      // Clear any existing timer
      if (autoStartTimer) {
        clearTimeout(autoStartTimer);
      }
      // Start playback after short delay (allows UI to show "Up Next" briefly)
      log("Auto-start conditions met, starting 500ms timer...");
      autoStartTimer = setTimeout(() => {
        autoStartTimer = null;
        log("Auto-start timer fired, calling playNext()");
        playNext();
      }, 500);
    } else {
      log("Auto-start conditions NOT met, skipping auto-play");
    }

    return item;
  } catch (error) {
    logError("Failed to add item:", error);
    return null;
  }
}

// Remove item from queue
export async function removeItem(uuid: string): Promise<void> {
  log(`Removing item ${uuid}`);
  try {
    await db.removeQueueItem(uuid);
    const newItems = state.items.filter((item) => item.uuid !== uuid);
    setState({ items: newItems });
    log(`Item ${uuid} removed, ${newItems.length} items remaining`);
  } catch (error) {
    logError(`Failed to remove item ${uuid}:`, error);
  }
}

// Reorder item in queue
export async function reorderItem(uuid: string, newPosition: number): Promise<void> {
  log(`Reordering item ${uuid} to position ${newPosition}`);
  try {
    await db.reorderQueueItem(uuid, newPosition);
    await loadItems(); // Reload to get correct positions
    log(`Item ${uuid} reordered successfully`);
  } catch (error) {
    logError(`Failed to reorder item ${uuid}:`, error);
  }
}

// Play the next pending item
export async function playNext(): Promise<void> {
  log("playNext() called");

  // Check if voice is enabled - this is the master control
  const voiceEnabled = getStoredVoiceEnabled();
  log(`playNext() voice enabled check: ${voiceEnabled}`);
  if (!voiceEnabled) {
    log("playNext() aborted - voice is disabled");
    return;
  }

  // Check if already playing or starting playback (mutex)
  if (isStartingPlayback) {
    log("playNext() aborted - already starting playback (mutex)");
    return;
  }
  if (state.isPlaying) {
    log("playNext() aborted - already playing");
    return;
  }

  // Set mutex immediately to prevent race conditions
  isStartingPlayback = true;
  log("Mutex acquired - isStartingPlayback = true");

  // Find next pending item
  const pendingItems = state.items
    .filter((item) => item.status === "pending")
    .sort((a, b) => a.position - b.position);

  log(`Found ${pendingItems.length} pending items`);

  if (pendingItems.length === 0) {
    log("No pending items - stopping playback");
    isStartingPlayback = false;
    setState({ isPlaying: false, currentItem: null });
    return;
  }

  const nextItem = pendingItems[0];
  log(`Playing next item: ${nextItem.uuid} - "${nextItem.text.substring(0, 50)}..."`);

  try {
    await playItem(nextItem.uuid);
  } finally {
    // Mutex is released in playItem after audio starts or fails
  }
}

// Play a specific item
export async function playItem(uuid: string): Promise<void> {
  log(`playItem(${uuid}) called`);
  pauseRequested = false; // Reset any stale pause request

  // Check if voice is enabled - this is the master control
  if (!getStoredVoiceEnabled()) {
    log("playItem() aborted - voice is disabled");
    isStartingPlayback = false;
    return;
  }

  const item = state.items.find((i) => i.uuid === uuid);
  if (!item) {
    log(`Item ${uuid} not found in state`);
    isStartingPlayback = false;
    return;
  }

  try {
    // Update status to playing
    log(`Updating item ${uuid} status to "playing" in database`);
    await db.updateQueueItemStatus(uuid, "playing");

    // Update local state - set isPlaying BEFORE async TTS call
    const updatedItems = state.items.map((i) =>
      i.uuid === uuid ? { ...i, status: "playing" as QueueItemStatus } : i
    );
    setState({
      items: updatedItems,
      currentItem: { ...item, status: "playing" },
      isPlaying: true,
      isPaused: false,
      currentPosition: 0,
    });
    log(`State updated: isPlaying=true, currentItem=${uuid}`);

    // Call TTS API
    const formData = new FormData();
    formData.append("text", item.text);
    formData.append("session_id", item.sessionId);

    // Pass voice_id if assigned (check cache, then localStorage fallback, then default)
    const DEFAULT_VOICE_ID = getDefaultVoice();
    // For screen reader sessions, ALWAYS use the SR voice from settings
    let srVoice: string | undefined;
    if (item.sessionId.startsWith("screen-reader")) {
      try {
        const stored = localStorage.getItem("vaak-sr-settings");
        if (stored) {
          const parsed = JSON.parse(stored);
          srVoice = parsed.voice_id;
        }
      } catch {}
    }
    // For SR sessions: item.voiceId (from Rust emit) > localStorage SR voice > default
    // For non-SR sessions: item.voiceId > session cache > voice assignments > default
    const voiceId = srVoice || item.voiceId || sessionCache[item.sessionId]?.voiceId || getVoiceAssignments()[item.sessionId] || DEFAULT_VOICE_ID;
    log(`[playItem] voiceId resolved: ${voiceId} (item.voiceId=${item.voiceId}, srVoice=${srVoice}, sessionId=${item.sessionId})`);
    formData.append("voice_id", voiceId);

    const apiUrl = getApiUrl();
    log(`Calling TTS API at ${apiUrl}/api/v1/tts`);
    const response = await fetch(`${apiUrl}/api/v1/tts`, {
      method: "POST",
      body: formData,
    });

    if (!response.ok) {
      // Try to get error details from response
      let errorDetail = "";
      try {
        const errorText = await response.text();
        errorDetail = errorText.substring(0, 200); // Limit error message length
      } catch {
        errorDetail = "Could not read error response";
      }
      throw new Error(`TTS API failed (${response.status}): ${errorDetail}`);
    }

    log(`TTS API response received, creating audio blob`);
    const blob = await response.blob();

    // Check if blob is valid
    if (!blob || blob.size === 0) {
      throw new Error("TTS API returned empty audio data");
    }
    log(`Audio blob size: ${blob.size} bytes, type: ${blob.type}`);
    const audioUrl = URL.createObjectURL(blob);

    // Stop any existing audio
    if (currentAudio) {
      log("Stopping existing audio");
      currentAudio.pause();
      currentAudio.src = "";
    }

    // Create and play audio
    log(`Creating Audio element for ${uuid}`);
    currentAudio = new Audio(audioUrl);
    currentAudio.volume = state.volume;
    currentAudio.muted = false; // Explicitly ensure not muted
    audioStartTime = Date.now();
    audioDurationMs = 0; // Reset duration
    audioCurrentTimeMs = 0; // Reset current time

    // Debug audio element properties
    log(`Audio element created - volume: ${currentAudio.volume}, muted: ${currentAudio.muted}, paused: ${currentAudio.paused}`);

    // Add canplay handler to verify audio is ready
    currentAudio.oncanplay = () => {
      log(`Audio canplay event fired for ${uuid} - readyState: ${currentAudio?.readyState}, duration: ${currentAudio?.duration}`);
    };

    // Capture duration when metadata loads
    currentAudio.onloadedmetadata = () => {
      if (currentAudio && !isNaN(currentAudio.duration)) {
        audioDurationMs = currentAudio.duration * 1000;
        log(`Audio metadata loaded, duration: ${audioDurationMs}ms`);
      }
    };

    // Track time updates for more accurate progress + broadcast to other windows
    currentAudio.ontimeupdate = () => {
      if (currentAudio) {
        audioCurrentTimeMs = currentAudio.currentTime * 1000;
        // Broadcast position to other windows
        broadcastPlaybackInfo();
      }
    };

    currentAudio.onended = async () => {
      const duration = Date.now() - audioStartTime;
      log(`Audio ended for ${uuid}, duration: ${duration}ms`);
      await onItemComplete(uuid, duration);
      URL.revokeObjectURL(audioUrl);

      // Auto-play next if enabled
      if (state.autoPlay) {
        log("Auto-play enabled, calling playNext()");
        playNext();
      }
    };

    currentAudio.onerror = async () => {
      // Get detailed error information from the audio element
      const audioError = currentAudio?.error;
      let errorMessage = "Audio playback failed";

      if (audioError) {
        const errorCodes: Record<number, string> = {
          1: "MEDIA_ERR_ABORTED - Playback aborted by user",
          2: "MEDIA_ERR_NETWORK - Network error while loading audio",
          3: "MEDIA_ERR_DECODE - Audio decoding failed (corrupted or unsupported format)",
          4: "MEDIA_ERR_SRC_NOT_SUPPORTED - Audio format not supported",
        };
        errorMessage = errorCodes[audioError.code] || `Unknown audio error (code: ${audioError.code})`;
        if (audioError.message) {
          errorMessage += ` - ${audioError.message}`;
        }
      }

      logError(`Audio error for ${uuid}: ${errorMessage}`);
      await onItemFailed(uuid, errorMessage);
      URL.revokeObjectURL(audioUrl);

      // Try next item
      if (state.autoPlay) {
        playNext();
      }
    };

    log(`Starting audio playback for ${uuid}`);
    log(`Pre-play state: volume=${currentAudio.volume}, muted=${currentAudio.muted}, paused=${currentAudio.paused}, readyState=${currentAudio.readyState}`);

    try {
      await currentAudio.play();
      log(`Audio playback started successfully for ${uuid}`);
      isStartingPlayback = false;
      log("Mutex released - playback confirmed started");
      log(`Post-play state: volume=${currentAudio.volume}, muted=${currentAudio.muted}, paused=${currentAudio.paused}, currentTime=${currentAudio.currentTime}`);

      // Check if pause was requested during TTS fetch
      if (pauseRequested) {
        log("Pause was requested during TTS fetch — pausing now");
        pauseRequested = false;
        currentAudio.pause();
        setState({ isPaused: true, isPlaying: false, currentPosition: 0 });
      }
    } catch (playError) {
      logError(`Play() promise rejected for ${uuid}:`, playError);
      throw playError;
    }
  } catch (error) {
    logError(`Failed to play item ${uuid}:`, error);
    isStartingPlayback = false; // Ensure mutex is released on error
    await onItemFailed(uuid, String(error));

    // Try next item
    if (state.autoPlay) {
      playNext();
    }
  }
}

// Track which items have been finalized to prevent double-processing
const finalizedItems = new Set<string>();

// Mark item as completed
async function onItemComplete(uuid: string, durationMs: number): Promise<void> {
  // Prevent double-processing (e.g., if both onended and onerror fire)
  if (finalizedItems.has(uuid)) {
    log(`Item ${uuid} already finalized, skipping onItemComplete`);
    return;
  }
  finalizedItems.add(uuid);

  log(`*** COMPLETING ITEM *** uuid=${uuid}, duration=${durationMs}ms`);
  try {
    log(`Calling db.updateQueueItemStatus for ${uuid}...`);
    await db.updateQueueItemStatus(uuid, "completed", durationMs);
    log(`Database updated successfully for ${uuid}`);

    const updatedItems = state.items.map((i) =>
      i.uuid === uuid
        ? { ...i, status: "completed" as QueueItemStatus, durationMs }
        : i
    );
    setState({
      items: updatedItems,
      currentItem: null,
      isPlaying: false,
    });
    log(`Item ${uuid} marked as completed in local state, isPlaying=false`);

    // Force a reload to ensure we have the latest from database
    log(`Triggering immediate loadItems after completion...`);
    await loadItems();
    log(`Post-completion loadItems done`);
  } catch (error) {
    logError(`Failed to mark item ${uuid} complete:`, error);
  }
}

// Mark item as failed
async function onItemFailed(uuid: string, errorMessage: string): Promise<void> {
  // Prevent double-processing (e.g., if both onended and onerror fire)
  if (finalizedItems.has(uuid)) {
    log(`Item ${uuid} already finalized, skipping onItemFailed`);
    return;
  }
  finalizedItems.add(uuid);

  log(`Marking item ${uuid} as failed: ${errorMessage}`);
  try {
    await db.updateQueueItemStatus(uuid, "failed", undefined, errorMessage);

    const updatedItems = state.items.map((i) =>
      i.uuid === uuid
        ? { ...i, status: "failed" as QueueItemStatus, errorMessage }
        : i
    );
    setState({
      items: updatedItems,
      currentItem: null,
      isPlaying: false,
    });
    log(`Item ${uuid} marked as failed, isPlaying=false`);
  } catch (error) {
    logError(`Failed to mark item ${uuid} failed:`, error);
  }
}

// Flag to handle pause requested during TTS fetch (before audio element exists)
let pauseRequested = false;

// Pause playback
export function pause(): void {
  log("pause() called");
  if (!isMainWindow) {
    setState({ isPaused: true });
    sendPlaybackCommand("pause");
    return;
  }
  if (currentAudio && state.isPlaying) {
    currentAudio.pause();
    const position = (Date.now() - audioStartTime);
    setState({ isPaused: true, isPlaying: false, currentPosition: position });
    pauseRequested = false;
    log(`Paused at position ${position}ms`);
  } else if (state.isPlaying && !currentAudio) {
    // Audio is "playing" state but TTS fetch hasn't completed yet — defer pause
    log("pause() - audio not ready yet, setting pauseRequested flag");
    pauseRequested = true;
    setState({ isPaused: true, isPlaying: false });
  } else {
    log("pause() - no audio playing");
  }
}

// Resume playback
export function resume(): void {
  log("resume() called");
  if (!isMainWindow) {
    setState({ isPaused: false });
    sendPlaybackCommand("resume");
    return;
  }
  pauseRequested = false; // Clear any pending pause request
  if (currentAudio && state.isPaused) {
    currentAudio.play();
    audioStartTime = Date.now() - state.currentPosition;
    setState({ isPaused: false, isPlaying: true });
    log("Resumed playback");
  } else {
    log("resume() - no paused audio");
  }
}

// Toggle play/pause
export function togglePlayPause(): void {
  log(`togglePlayPause() - isPlaying=${state.isPlaying}, isPaused=${state.isPaused}`);
  if (!isMainWindow) {
    // Toggle local isPaused state immediately for UI responsiveness
    const hasPlayingItem = state.items.some(i => i.status === "playing");
    if (state.isPaused) {
      setState({ isPaused: false });
    } else if (hasPlayingItem) {
      setState({ isPaused: true });
    }
    sendPlaybackCommand("togglePlayPause");
    return;
  }
  if (state.isPlaying) {
    pause();
  } else if (state.isPaused) {
    resume();
  } else {
    playNext();
  }
}

// Stop playback entirely (don't advance to next item)
export function stopPlayback(): void {
  log("stopPlayback() called");
  if (!isMainWindow) {
    setState({ isPaused: false });
    sendPlaybackCommand("stopPlayback");
    return;
  }

  // Finalize current item BEFORE clearing audio to prevent onended/onerror race
  if (state.currentItem) {
    finalizedItems.add(state.currentItem.uuid);
  }

  if (currentAudio) {
    currentAudio.pause();
    currentAudio.src = "";
    currentAudio = null;
  }

  if (state.currentItem) {
    // Mark current as completed (not pending — pending causes auto-replay)
    log(`Marking current item ${state.currentItem.uuid} as completed (stopped)`);
    db.updateQueueItemStatus(state.currentItem.uuid, "completed").catch(logError);

    const updatedItems = state.items.map((i) =>
      i.uuid === state.currentItem!.uuid
        ? { ...i, status: "completed" as QueueItemStatus }
        : i
    );
    setState({ items: updatedItems, currentItem: null, isPlaying: false, isPaused: false });
  } else {
    setState({ currentItem: null, isPlaying: false, isPaused: false });
  }

  isStartingPlayback = false;
  pauseRequested = false;
  log("Playback stopped");
}

// Skip to next item
export function skipNext(): void {
  log("skipNext() called");
  if (!isMainWindow) {
    sendPlaybackCommand("skipNext");
    return;
  }

  // Finalize current item BEFORE clearing audio to prevent onended/onerror race
  if (state.currentItem) {
    finalizedItems.add(state.currentItem.uuid);
  }

  if (currentAudio) {
    log("Stopping current audio");
    currentAudio.pause();
    currentAudio.src = "";
    currentAudio = null;
  }

  if (state.currentItem) {
    // Mark current as completed
    log(`Marking current item ${state.currentItem.uuid} as completed`);
    db.updateQueueItemStatus(state.currentItem.uuid, "completed").catch(logError);

    const updatedItems = state.items.map((i) =>
      i.uuid === state.currentItem!.uuid
        ? { ...i, status: "completed" as QueueItemStatus }
        : i
    );
    setState({ items: updatedItems, currentItem: null, isPlaying: false, isPaused: false });
  }

  playNext();
}

// Skip to previous item (replay last completed)
export function skipPrevious(): void {
  log("skipPrevious() called");
  if (!isMainWindow) {
    sendPlaybackCommand("skipPrevious");
    return;
  }

  // Finalize current item BEFORE clearing audio to prevent onended/onerror race
  if (state.currentItem) {
    finalizedItems.add(state.currentItem.uuid);
  }

  const completedItems = state.items
    .filter((item) => item.status === "completed")
    .sort((a, b) => (b.completedAt || 0) - (a.completedAt || 0));

  log(`Found ${completedItems.length} completed items`);

  if (completedItems.length > 0) {
    // Stop current playback
    if (currentAudio) {
      log("Stopping current audio");
      currentAudio.pause();
      currentAudio.src = "";
      currentAudio = null;
    }

    // Reset the last completed item to pending and play it
    const lastItem = completedItems[0];
    log(`Replaying last completed item: ${lastItem.uuid}`);
    db.updateQueueItemStatus(lastItem.uuid, "pending").then(() => {
      const updatedItems = state.items.map((i) =>
        i.uuid === lastItem.uuid ? { ...i, status: "pending" as QueueItemStatus } : i
      );
      setState({ items: updatedItems });
      playItem(lastItem.uuid);
    }).catch(logError);
  } else {
    log("No completed items to replay");
  }
}

// Set volume
export function setVolume(volume: number): void {
  const clampedVolume = Math.max(0, Math.min(1, volume));
  setState({ volume: clampedVolume });
  if (currentAudio) {
    currentAudio.volume = clampedVolume;
  }
}

// Set auto-play (persisted to localStorage)
export function setAutoPlay(enabled: boolean): void {
  setState({ autoPlay: enabled });
  try {
    localStorage.setItem("vaak_queue_autoplay", enabled ? "true" : "false");
    log(`AutoPlay set to ${enabled} and saved`);
  } catch (e) {
    // Ignore storage errors
  }
}

// Load autoPlay from localStorage
function loadAutoPlay(): boolean {
  try {
    const stored = localStorage.getItem("vaak_queue_autoplay");
    return stored !== "false"; // Default to true
  } catch {
    return true;
  }
}

// Clear completed items
export async function clearCompleted(olderThanDays?: number): Promise<void> {
  try {
    await db.clearCompletedItems(olderThanDays);
    await loadItems();
  } catch (error) {
    console.error("[QueueStore] Failed to clear completed:", error);
  }
}

// Clear all pending items (called when voice is disabled)
export async function clearPending(): Promise<void> {
  log("Clearing all pending items (voice disabled)");
  try {
    const pendingItems = state.items.filter((i) => i.status === "pending");
    for (const item of pendingItems) {
      await db.removeQueueItem(item.uuid);
    }
    await loadItems();
    log(`Cleared ${pendingItems.length} pending items`);
  } catch (error) {
    console.error("[QueueStore] Failed to clear pending:", error);
  }
}

// Reset any stuck "playing" items back to pending or remove them
export async function resetStuckItems(): Promise<void> {
  log("Resetting stuck playing items");
  try {
    const playingItems = state.items.filter((i) => i.status === "playing");
    for (const item of playingItems) {
      // Mark as failed since it got stuck
      await db.updateQueueItemStatus(item.uuid, "failed", undefined, "Playback interrupted");
    }
    // Also reset local state
    if (currentAudio) {
      currentAudio.pause();
      currentAudio = null;
    }
    setState({ currentItem: null, isPlaying: false, isPaused: false });
    isStartingPlayback = false;
    await loadItems();
    log(`Reset ${playingItems.length} stuck items`);
  } catch (error) {
    console.error("[QueueStore] Failed to reset stuck items:", error);
  }
}

// React hook for using the queue store
export function useQueueStore(): QueueState {
  // Use a state object that gets replaced on every update
  const [currentState, setCurrentState] = useState<QueueState>(() => ({ ...state }));

  useEffect(() => {
    // Update immediately in case state changed before subscription
    setCurrentState({ ...state });

    const unsubscribe = subscribe(() => {
      // Create new object to trigger React re-render
      setCurrentState({ ...state });
    });
    return unsubscribe;
  }, []);

  return currentState;
}

// Initialize the store - call once on app startup
export async function initQueueStore(): Promise<void> {
  log("Initializing queue store...");
  // Load persisted autoPlay setting
  const autoPlay = loadAutoPlay();
  setState({ autoPlay });
  await loadItems();
  // Reset any items stuck in "playing" state from previous session
  await resetStuckItems();
  log(`Queue store initialized with ${state.items.length} items, autoPlay=${state.autoPlay}`);

  // Listen for playback info broadcasts from main window (for progress bar)
  if (!isMainWindow) {
    try {
      const { listen } = await import("@tauri-apps/api/event");
      await listen<{ currentTimeMs: number; durationMs: number }>("queue-playback-info", (event) => {
        sharedPlaybackInfo = event.payload;
      });
      log("Playback info listener registered for transcript window");
    } catch (e) {
      logError("Failed to register playback info listener:", e);
    }
  }

  // Listen for playback commands from other windows (transcript window)
  if (isMainWindow) {
    try {
      const { listen } = await import("@tauri-apps/api/event");
      await listen<{ command: string }>("queue-playback-command", (event) => {
        const { command } = event.payload;
        log(`Received playback command from other window: ${command}`);
        switch (command) {
          case "pause": pause(); break;
          case "resume": resume(); break;
          case "togglePlayPause": togglePlayPause(); break;
          case "skipNext": skipNext(); break;
          case "skipPrevious": skipPrevious(); break;
          case "stopPlayback": stopPlayback(); break;
          case "seek": seek((event.payload as unknown as { positionMs: number }).positionMs); break;
          default: log(`Unknown playback command: ${command}`);
        }
      });
      log("Playback command listener registered for cross-window control");
    } catch (e) {
      logError("Failed to register playback command listener:", e);
    }
  }
}

// ==================== Feature 7: Unique voices per session ====================

const UNIQUE_VOICES_KEY = 'vaak_unique_voices';

export function getStoredUniqueVoices(): boolean {
  try {
    return localStorage.getItem(UNIQUE_VOICES_KEY) === 'true';
  } catch { return false; }
}

export function saveUniqueVoices(enabled: boolean): void {
  try {
    localStorage.setItem(UNIQUE_VOICES_KEY, enabled ? 'true' : 'false');
  } catch {}
}

// Default voice (fallback when no session-specific voice is set)
const DEFAULT_VOICE_KEY = 'vaak_default_voice';
const HARDCODED_DEFAULT_VOICE = 'TlLCuK5N2ARR6OHBwD53'; // AALB

export function getDefaultVoice(): string {
  try {
    return localStorage.getItem(DEFAULT_VOICE_KEY) || HARDCODED_DEFAULT_VOICE;
  } catch { return HARDCODED_DEFAULT_VOICE; }
}

export function saveDefaultVoice(voiceId: string): void {
  try {
    localStorage.setItem(DEFAULT_VOICE_KEY, voiceId);
  } catch {}
}

// Voice assignments per session
const VOICE_ASSIGNMENTS_KEY = 'vaak_voice_assignments';

export function getVoiceAssignments(): Record<string, string> {
  try {
    const stored = localStorage.getItem(VOICE_ASSIGNMENTS_KEY);
    return stored ? JSON.parse(stored) : {};
  } catch { return {}; }
}

export function saveVoiceAssignment(sessionId: string, voiceId: string): void {
  try {
    const assignments = getVoiceAssignments();
    assignments[sessionId] = voiceId;
    localStorage.setItem(VOICE_ASSIGNMENTS_KEY, JSON.stringify(assignments));
  } catch {}
}

// Available voices cache
let availableVoices: { voice_id: string; name: string }[] = [];

export function getAvailableVoices(): { voice_id: string; name: string }[] {
  return availableVoices;
}

export async function fetchAvailableVoices(): Promise<void> {
  try {
    const apiUrl = getApiUrl();
    const response = await fetch(`${apiUrl}/api/v1/voices`);
    if (response.ok) {
      const data = await response.json();
      availableVoices = data.voices || [];
      log(`Fetched ${availableVoices.length} available voices`);
    }
  } catch (error) {
    logError("Failed to fetch voices:", error);
  }
}

// ==================== Feature 8: Session identity cache ====================

interface SessionCacheEntry {
  name: string;
  color: string;
  voiceId?: string;
}

const sessionCache: Record<string, SessionCacheEntry> = {};

// Enrich queue items with session name/color from cache
function enrichWithSessionInfo(items: QueueItem[]): QueueItem[] {
  return items.map(item => {
    if (item.sessionId.startsWith("screen-reader")) {
      const sessionName = item.sessionId.startsWith("screen-reader-ask")
        ? "Screen Reader (Ask)"
        : item.sessionId.startsWith("screen-reader-err")
          ? "Screen Reader (Error)"
          : "Screen Reader";
      return { ...item, sessionName, sessionColor: "#6366f1" };
    }
    const cached = sessionCache[item.sessionId];
    if (cached && (!item.sessionName || !item.sessionColor)) {
      return { ...item, sessionName: cached.name, sessionColor: cached.color };
    }
    return item;
  });
}

export function updateSessionCache(sessions: { id: string; name: string; color: string; voiceId?: string }[]): void {
  for (const s of sessions) {
    sessionCache[s.id] = { name: s.name, color: s.color, voiceId: s.voiceId };
  }
  // Re-enrich current items with updated session info
  if (state.items.length > 0) {
    setState({ items: enrichWithSessionInfo(state.items) });
  }
}

// ==================== Feature 8: Announce session toggle ====================

const ANNOUNCE_SESSION_KEY = 'vaak_announce_session';

export function getStoredAnnounceSession(): boolean {
  try {
    return localStorage.getItem(ANNOUNCE_SESSION_KEY) === 'true';
  } catch { return false; }
}

export function saveAnnounceSession(enabled: boolean): void {
  try {
    localStorage.setItem(ANNOUNCE_SESSION_KEY, enabled ? 'true' : 'false');
  } catch {}
}
