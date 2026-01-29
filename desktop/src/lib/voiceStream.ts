/**
 * Voice stream client - connects to Scribe backend SSE endpoint
 * and plays audio explanations of Claude Code actions.
 */

export interface VoiceEvent {
  type: 'voice' | 'status' | 'error' | 'connected' | 'speak';
  audio_base64?: string;
  explanation?: string;
  file_path?: string;
  timestamp?: string;
}

export type VoiceEventHandler = (event: VoiceEvent) => void;

class VoiceStreamClient {
  private eventSource: EventSource | null = null;
  private handlers: VoiceEventHandler[] = [];
  private reconnectAttempts = 0;
  private maxReconnectAttempts = 5;
  private reconnectDelay = 1000;
  private currentAudio: HTMLAudioElement | null = null;
  private enabled = false;
  private apiUrl = '';

  /**
   * Start listening to voice events from the backend.
   */
  connect(apiUrl: string): void {
    if (this.eventSource) {
      this.disconnect();
    }

    this.enabled = true;
    this.apiUrl = apiUrl;
    const url = `${apiUrl}/api/v1/voice-stream`;

    try {
      this.eventSource = new EventSource(url);

      this.eventSource.onopen = () => {
        console.log('[VoiceStream] Connected');
        this.reconnectAttempts = 0;
      };

      this.eventSource.onmessage = (event) => {
        try {
          const data: VoiceEvent = JSON.parse(event.data);
          this.handleEvent(data);
        } catch (e) {
          console.error('[VoiceStream] Parse error:', e);
        }
      };

      this.eventSource.onerror = () => {
        console.error('[VoiceStream] Connection error');
        this.handleDisconnect();
      };

    } catch (e) {
      console.error('[VoiceStream] Failed to connect:', e);
      this.handleDisconnect();
    }
  }

  /**
   * Stop listening to voice events.
   */
  disconnect(): void {
    this.enabled = false;
    if (this.eventSource) {
      this.eventSource.close();
      this.eventSource = null;
    }
    this.stopAudio();
  }

  /**
   * Register a handler for voice events.
   * Returns an unsubscribe function.
   */
  onEvent(handler: VoiceEventHandler): () => void {
    this.handlers.push(handler);
    return () => {
      this.handlers = this.handlers.filter(h => h !== handler);
    };
  }

  /**
   * Stop any currently playing audio.
   */
  stopAudio(): void {
    if (this.currentAudio) {
      this.currentAudio.pause();
      this.currentAudio = null;
    }
  }

  private handleEvent(event: VoiceEvent): void {
    // Notify handlers
    this.handlers.forEach(handler => handler(event));

    // Play audio if present
    if (event.type === 'voice' && event.audio_base64) {
      this.playAudio(event.audio_base64);
    }

    // Handle speak events using browser TTS
    if (event.type === 'speak' && event.explanation) {
      this.speakText(event.explanation);
    }
  }

  private speakText(text: string): void {
    if (!window.speechSynthesis) {
      console.warn('[VoiceStream] SpeechSynthesis not available');
      return;
    }

    // Cancel any ongoing speech
    window.speechSynthesis.cancel();

    const utterance = new SpeechSynthesisUtterance(text);
    utterance.rate = 1.1;
    utterance.pitch = 1.0;
    utterance.volume = 1.0;

    // Try to use a good voice
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
    console.log('[VoiceStream] Speaking:', text);
  }

  private async playAudio(base64Audio: string): Promise<void> {
    // Stop any playing audio first
    this.stopAudio();

    try {
      // Decode base64 to blob
      const binaryString = atob(base64Audio);
      const bytes = new Uint8Array(binaryString.length);
      for (let i = 0; i < binaryString.length; i++) {
        bytes[i] = binaryString.charCodeAt(i);
      }
      const blob = new Blob([bytes], { type: 'audio/mpeg' });
      const url = URL.createObjectURL(blob);

      // Create and play audio
      this.currentAudio = new Audio(url);
      this.currentAudio.onended = () => {
        URL.revokeObjectURL(url);
        this.currentAudio = null;
      };
      this.currentAudio.onerror = (e) => {
        console.error('[VoiceStream] Audio playback error:', e);
        URL.revokeObjectURL(url);
        this.currentAudio = null;
      };

      await this.currentAudio.play();

    } catch (e) {
      console.error('[VoiceStream] Failed to play audio:', e);
    }
  }

  private handleDisconnect(): void {
    if (this.eventSource) {
      this.eventSource.close();
      this.eventSource = null;
    }

    if (!this.enabled) return;

    if (this.reconnectAttempts < this.maxReconnectAttempts) {
      this.reconnectAttempts++;
      const delay = this.reconnectDelay * this.reconnectAttempts;
      console.log(`[VoiceStream] Reconnecting in ${delay}ms (attempt ${this.reconnectAttempts}/${this.maxReconnectAttempts})...`);
      setTimeout(() => {
        if (this.enabled) {
          this.connect(this.apiUrl);
        }
      }, delay);
    } else {
      console.error('[VoiceStream] Max reconnect attempts reached');
    }
  }

  /**
   * Check if currently connected.
   */
  get isConnected(): boolean {
    return this.eventSource?.readyState === EventSource.OPEN;
  }
}

// Singleton instance
export const voiceStream = new VoiceStreamClient();


// Storage helpers for voice settings
const VOICE_ENABLED_KEY = 'scribe_voice_enabled';

export function getStoredVoiceEnabled(): boolean {
  try {
    return localStorage.getItem(VOICE_ENABLED_KEY) === 'true';
  } catch {
    return false;
  }
}

export function saveVoiceEnabled(enabled: boolean): void {
  try {
    localStorage.setItem(VOICE_ENABLED_KEY, enabled ? 'true' : 'false');
  } catch {
    // Ignore storage errors
  }
}

// Blind mode storage (replaces old VoiceMode)
const BLIND_MODE_KEY = 'scribe_blind_mode';

// Legacy type for backwards compatibility during transition
export type VoiceMode = 'summary' | 'developer' | 'blind';

export function getStoredBlindMode(): boolean {
  try {
    const stored = localStorage.getItem(BLIND_MODE_KEY);
    if (stored !== null) {
      return stored === 'true';
    }
    // Migration from old voice mode
    const oldMode = localStorage.getItem('scribe_voice_mode');
    return oldMode === 'blind';
  } catch {
    return false;
  }
}

export function saveBlindMode(enabled: boolean): void {
  try {
    localStorage.setItem(BLIND_MODE_KEY, enabled ? 'true' : 'false');
  } catch {
    // Ignore storage errors
  }
}

// Legacy functions for backwards compatibility
export function getStoredVoiceMode(): VoiceMode {
  return getStoredBlindMode() ? 'blind' : 'summary';
}

export function saveVoiceMode(mode: VoiceMode): void {
  saveBlindMode(mode === 'blind');
}

// Voice detail level storage
const VOICE_DETAIL_KEY = 'scribe_voice_detail';

export function getStoredVoiceDetail(): number {
  try {
    const detail = localStorage.getItem(VOICE_DETAIL_KEY);
    const parsed = parseInt(detail || '3', 10);
    return parsed >= 1 && parsed <= 5 ? parsed : 3;
  } catch {
    return 3; // Default to balanced
  }
}

export function saveVoiceDetail(detail: number): void {
  try {
    if (detail >= 1 && detail <= 5) {
      localStorage.setItem(VOICE_DETAIL_KEY, detail.toString());
    }
  } catch {
    // Ignore storage errors
  }
}

// Voice trigger mode storage
const VOICE_AUTO_KEY = 'scribe_voice_auto';

export function getStoredVoiceAuto(): boolean {
  try {
    const auto = localStorage.getItem(VOICE_AUTO_KEY);
    return auto !== 'false'; // Default to true (automatic)
  } catch {
    return true;
  }
}

export function saveVoiceAuto(auto: boolean): void {
  try {
    localStorage.setItem(VOICE_AUTO_KEY, auto ? 'true' : 'false');
  } catch {
    // Ignore storage errors
  }
}
