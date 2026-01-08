/**
 * Speech synthesis for Claude Code integration.
 * Uses ElevenLabs via the Scribe backend API for high-quality voice.
 */

import { listen } from "@tauri-apps/api/event";

const API_URL = import.meta.env.VITE_API_URL || "https://scribe-api-yk09.onrender.com";

// Audio queue for sequential playback
let audioQueue: string[] = [];
let isPlaying = false;
let currentAudio: HTMLAudioElement | null = null;

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
  const text = audioQueue.shift()!;

  try {
    const token = getAuthToken();
    if (!token) {
      console.warn("[Speak] No auth token, falling back to browser TTS");
      fallbackSpeak(text);
      isPlaying = false;
      playNext();
      return;
    }

    // Call the Scribe TTS endpoint
    const formData = new FormData();
    formData.append("text", text);

    const response = await fetch(`${API_URL}/api/v1/tts`, {
      method: "POST",
      headers: {
        "Authorization": `Bearer ${token}`,
      },
      body: formData,
    });

    if (!response.ok) {
      console.warn("[Speak] TTS API failed, falling back to browser TTS");
      fallbackSpeak(text);
      isPlaying = false;
      playNext();
      return;
    }

    // Get audio blob and play it
    const audioBlob = await response.blob();
    const audioUrl = URL.createObjectURL(audioBlob);

    currentAudio = new Audio(audioUrl);
    currentAudio.onended = () => {
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

    await currentAudio.play();
  } catch (error) {
    console.warn("[Speak] Error:", error);
    fallbackSpeak(text);
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
export function speak(text: string): void {
  if (!text) return;

  audioQueue.push(text);
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
 */
export async function initSpeakListener(): Promise<() => void> {
  // Preload voices for fallback
  if (window.speechSynthesis) {
    window.speechSynthesis.getVoices();
  }

  const unlisten = await listen<string>("speak", (event) => {
    console.log("[Speak] Received:", event.payload);
    speak(event.payload);
  });

  console.log("[Speak] Listener initialized (ElevenLabs)");
  return unlisten;
}
