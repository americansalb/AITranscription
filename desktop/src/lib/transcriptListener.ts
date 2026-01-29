/**
 * Global transcript listener singleton
 * This module initializes ONCE per window and is independent of React component lifecycle
 */

import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { SpeakMessage } from "./sessionManager";

type SessionUpdateCallback = (message: SpeakMessage) => void;
type HeartbeatCallback = (sessionId: string, timestamp: number) => void;

export interface HeartbeatPayload {
  session_id: string;
  timestamp: number;
}

class TranscriptListener {
  private static instance: TranscriptListener | null = null;
  // Unlisten functions stored for potential future cleanup
  private unlistenSpeak: UnlistenFn | null = null;
  private unlistenHeartbeat: UnlistenFn | null = null;
  private initialized = false;
  private callback: SessionUpdateCallback | null = null;
  private heartbeatCallback: HeartbeatCallback | null = null;
  private recentMessages = new Set<string>();

  private constructor() {
    console.log("[TranscriptListener] Constructor called");
  }

  static getInstance(): TranscriptListener {
    if (!TranscriptListener.instance) {
      console.log("[TranscriptListener] Creating singleton instance");
      TranscriptListener.instance = new TranscriptListener();
    }
    return TranscriptListener.instance;
  }

  async initialize(callback: SessionUpdateCallback, heartbeatCallback?: HeartbeatCallback): Promise<void> {
    if (this.initialized) {
      console.log("[TranscriptListener] Already initialized, updating callbacks only");
      this.callback = callback;
      if (heartbeatCallback) this.heartbeatCallback = heartbeatCallback;
      return;
    }

    console.log("[TranscriptListener] Initializing for the first time");
    this.callback = callback;
    if (heartbeatCallback) this.heartbeatCallback = heartbeatCallback;
    this.initialized = true;

    // Listen for speak events
    this.unlistenSpeak = await listen<SpeakMessage>("speak", (event) => {
      const message = event.payload;

      // Create unique message ID for deduplication
      const messageId = `${message.timestamp}-${message.session_id}-${message.text.substring(0, 100)}`;

      // Check if we've already processed this message recently
      if (this.recentMessages.has(messageId)) {
        console.log(`[TranscriptListener] DUPLICATE DETECTED - Skipping: "${message.text.substring(0, 50)}..."`);
        return;
      }

      // Add to recent messages
      this.recentMessages.add(messageId);

      // Clean up after 5 seconds
      setTimeout(() => {
        this.recentMessages.delete(messageId);
      }, 5000);

      console.log(`[TranscriptListener] Message received - Session: ${message.session_id}, Text: "${message.text.substring(0, 100)}..."`);

      // Call the callback with the message
      if (this.callback) {
        this.callback(message);
      }
    });

    // Listen for heartbeat events
    this.unlistenHeartbeat = await listen<HeartbeatPayload>("heartbeat", (event) => {
      const { session_id, timestamp } = event.payload;
      console.log(`[TranscriptListener] Heartbeat received - Session: ${session_id}`);

      if (this.heartbeatCallback) {
        this.heartbeatCallback(session_id, timestamp);
      }
    });

    console.log("[TranscriptListener] Listeners registered successfully (speak + heartbeat)");
  }

  updateCallback(callback: SessionUpdateCallback): void {
    console.log("[TranscriptListener] Callback updated");
    this.callback = callback;
  }

  updateHeartbeatCallback(callback: HeartbeatCallback): void {
    console.log("[TranscriptListener] Heartbeat callback updated");
    this.heartbeatCallback = callback;
  }

  cleanup(): void {
    if (this.unlistenSpeak) {
      console.log("[TranscriptListener] Cleaning up speak listener");
      this.unlistenSpeak();
      this.unlistenSpeak = null;
    }
    if (this.unlistenHeartbeat) {
      console.log("[TranscriptListener] Cleaning up heartbeat listener");
      this.unlistenHeartbeat();
      this.unlistenHeartbeat = null;
    }
    this.initialized = false;
  }
}

export const transcriptListener = TranscriptListener.getInstance();
