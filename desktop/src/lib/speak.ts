/**
 * Speech synthesis for Claude Code integration.
 * Uses ElevenLabs via the Scribe backend API for high-quality voice.
 * Now integrated with the queue store for persistent queue management.
 */

import { listen } from "@tauri-apps/api/event";
import * as queueStore from "./queueStore";
import type { SpeakEventPayload, QueueItem as QueueStoreItem } from "./queueTypes";
import { getStoredVoiceEnabled } from "./voiceStream";

const API_URL = import.meta.env.VITE_API_URL || "https://scribe-api-yk09.onrender.com";

// Legacy audio queue for backward compatibility (will be phased out)
interface LegacyQueueItem {
  text: string;
  session_id?: string;
}
let audioQueue: LegacyQueueItem[] = [];
let isPlaying = false;
let currentAudio: HTMLAudioElement | null = null;

// Flag to use new queue store vs legacy queue
const USE_QUEUE_STORE = true;

// Prevent duplicate listener registration in React StrictMode
// Use window object to ensure TRULY global singleton across hot reloads
declare global {
  interface Window {
    __SPEAK_LISTENER_INITIALIZED__?: boolean;
    __SPEAK_GLOBAL_UNLISTEN__?: (() => void) | null;
    __SPEAK_RECENT_MESSAGES__?: Set<string>;
  }
}

// State is tracked via window object to persist across hot reloads
// window.__SPEAK_LISTENER_INITIALIZED__ and window.__SPEAK_GLOBAL_UNLISTEN__

// Track recent messages to prevent duplicates (safety net)
if (!window.__SPEAK_RECENT_MESSAGES__) {
  window.__SPEAK_RECENT_MESSAGES__ = new Set<string>();
}
const recentMessages = window.__SPEAK_RECENT_MESSAGES__;

/**
 * Get the auth token from localStorage.
 */
function getAuthToken(): string | null {
  return localStorage.getItem("scribe_token");
}

/**
 * Play the next audio in the queue.
 */
async function playNext(): Promise<void> {
  if (isPlaying || audioQueue.length === 0) return;

  isPlaying = true;
  const item = audioQueue.shift()!;

  try {
    const token = getAuthToken();

    // Call the Scribe TTS endpoint
    const formData = new FormData();
    formData.append("text", item.text);
    if (item.session_id) {
      formData.append("session_id", item.session_id);
    }

    const headers: Record<string, string> = {};
    if (token) {
      headers["Authorization"] = `Bearer ${token}`;
    }

    const response = await fetch(`${API_URL}/api/v1/tts`, {
      method: "POST",
      headers,
      body: formData,
    });

    if (!response.ok) {
      console.warn("[Speak] TTS API failed, falling back to browser TTS");
      fallbackSpeak(item.text);
      isPlaying = false;
      playNext();
      return;
    }

    // Get audio blob and play it
    const audioBlob = await response.blob();
    const audioUrl = URL.createObjectURL(audioBlob);

    currentAudio = new Audio(audioUrl);
    currentAudio.onended = () => {
      console.log("[Speak] Audio playback ended");
      URL.revokeObjectURL(audioUrl);
      currentAudio = null;
      isPlaying = false;
      playNext();
    };
    currentAudio.onerror = () => {
      console.warn("[Speak] Audio playback failed");
      URL.revokeObjectURL(audioUrl);
      currentAudio = null;
      isPlaying = false;
      playNext();
    };

    console.log(`[Speak] Starting audio playback: "${item.text.substring(0, 50)}..."`);
    await currentAudio.play();
  } catch (error) {
    console.warn("[Speak] Error:", error);
    fallbackSpeak(item.text);
    isPlaying = false;
    playNext();
  }
}

/**
 * Fallback to browser's SpeechSynthesis API.
 */
function fallbackSpeak(text: string): void {
  if (!window.speechSynthesis) return;

  window.speechSynthesis.cancel();
  const utterance = new SpeechSynthesisUtterance(text);
  utterance.rate = 1.1;
  utterance.pitch = 1.0;
  utterance.volume = 1.0;

  const voices = window.speechSynthesis.getVoices();
  const preferredVoice = voices.find(
    (v) =>
      v.name.includes("Samantha") ||
      v.name.includes("Microsoft Zira") ||
      v.name.includes("Google") ||
      v.lang.startsWith("en")
  );
  if (preferredVoice) {
    utterance.voice = preferredVoice;
  }

  window.speechSynthesis.speak(utterance);
}

/**
 * Speak text using ElevenLabs via Scribe API.
 */
export function speak(text: string, session_id?: string): void {
  if (!text) return;

  console.log(`[Speak] speak() called - Queue before: ${audioQueue.length}, Text: "${text.substring(0, 50)}..."`);
  audioQueue.push({ text, session_id });
  playNext();
}

/**
 * Stop any ongoing speech.
 */
export function stop(): void {
  audioQueue = [];

  if (currentAudio) {
    currentAudio.pause();
    currentAudio = null;
  }

  if (window.speechSynthesis) {
    window.speechSynthesis.cancel();
  }

  isPlaying = false;
}

/**
 * Initialize the speak listener.
 * Call this once when the app starts.
 * Protected against React StrictMode double-initialization.
 */
export async function initSpeakListener(): Promise<() => void> {
  console.log("[Speak] initSpeakListener() called");

  // Check window global to prevent duplicate initialization across hot reloads
  if (window.__SPEAK_LISTENER_INITIALIZED__) {
    console.log("[Speak] Listener already initialized (window check), skipping duplicate");
    return () => {}; // Return no-op cleanup function
  }

  console.log("[Speak] First-time initialization starting...");

  // If there's an old listener somehow, clean it up first
  if (window.__SPEAK_GLOBAL_UNLISTEN__) {
    console.log("[Speak] Cleaning up old listener before creating new one");
    window.__SPEAK_GLOBAL_UNLISTEN__();
    window.__SPEAK_GLOBAL_UNLISTEN__ = null;
  }

  // Mark as initialized on window object (persists across hot reloads)
  window.__SPEAK_LISTENER_INITIALIZED__ = true;

  // Preload voices for fallback
  if (window.speechSynthesis) {
    window.speechSynthesis.getVoices();
  }

  // Initialize the queue store
  if (USE_QUEUE_STORE) {
    await queueStore.initQueueStore();
  }

  interface SpeakPayload {
    text: string;
    session_id: string;
    timestamp: number;
    queue_item?: QueueStoreItem;
  }

  const unlisten = await listen<SpeakPayload>("speak", (event) => {
    const payload = event.payload;

    console.log(`[Speak] *** EVENT RECEIVED *** Text: "${payload.text.substring(0, 50)}..."`);

    // Check if voice is enabled - skip if disabled
    const voiceEnabled = getStoredVoiceEnabled();
    console.log(`[Speak] Voice enabled check: ${voiceEnabled}`);
    if (!voiceEnabled) {
      console.log(`[Speak] Voice disabled - skipping message: "${payload.text.substring(0, 50)}..."`);
      return;
    }

    // Create unique message ID for deduplication
    const messageId = `${payload.timestamp}-${payload.session_id}-${payload.text.substring(0, 100)}`;

    // Check if we've already processed this message recently
    if (recentMessages.has(messageId)) {
      console.log(`[Speak] DUPLICATE DETECTED - Skipping message: "${payload.text.substring(0, 50)}..."`);
      return;
    }

    // Add to recent messages
    recentMessages.add(messageId);

    // Clean up old messages after 5 seconds to prevent memory leak
    setTimeout(() => {
      recentMessages.delete(messageId);
    }, 5000);

    console.log(`[Speak] Tauri event received - Session: ${payload.session_id}, Text: "${payload.text.substring(0, 50)}..."`);

    // Use queue store for playback if enabled
    console.log(`[Speak] USE_QUEUE_STORE: ${USE_QUEUE_STORE}`);
    if (USE_QUEUE_STORE) {
      // Add to queue store - it will handle playback
      console.log(`[Speak] Calling queueStore.addItem...`);
      queueStore.addItem(payload.text, payload.session_id, payload.queue_item)
        .then((item) => {
          console.log(`[Speak] Successfully added to queue:`, item?.uuid);
        })
        .catch((error) => {
          console.error("[Speak] Failed to add to queue store:", error);
          // Fallback to legacy speak
          speak(payload.text);
        });
    } else {
      // Legacy behavior
      speak(payload.text);
    }

    // Emit full payload for transcript window (no need to include session_id in speech)
    // This allows the transcript window to track the session
    if (payload.session_id) {
      // Dispatch custom event for transcript tracking
      const customEvent = new CustomEvent("speak-message", {
        detail: payload
      });
      window.dispatchEvent(customEvent);
    }
  });

  // Store unlisten in window to persist across hot reloads
  window.__SPEAK_GLOBAL_UNLISTEN__ = unlisten;

  console.log("[Speak] Listener initialized (ElevenLabs) - stored in window");

  // Return cleanup function
  return () => {
    console.log("[Speak] Cleanup called - unlistening");
    if (window.__SPEAK_GLOBAL_UNLISTEN__) {
      window.__SPEAK_GLOBAL_UNLISTEN__();
      window.__SPEAK_GLOBAL_UNLISTEN__ = null;
    }
    // Reset the flag so HMR can re-initialize properly
    window.__SPEAK_LISTENER_INITIALIZED__ = false;
    console.log("[Speak] Reset initialized flag for HMR");
  };
}

// Re-export queue store functions for UI access
export {
  queueStore,
  type QueueStoreItem as QueueItem,
  type SpeakEventPayload,
};
