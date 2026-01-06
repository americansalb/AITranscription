/**
 * Speech synthesis for Claude Code integration.
 * Listens for speak events from the Tauri backend and uses the browser's
 * SpeechSynthesis API to speak the text aloud.
 */

import { listen } from "@tauri-apps/api/event";

let currentUtterance: SpeechSynthesisUtterance | null = null;

/**
 * Speak text using the browser's SpeechSynthesis API.
 */
export function speak(text: string): void {
  if (!text || !window.speechSynthesis) {
    console.warn("[Speak] SpeechSynthesis not available");
    return;
  }

  // Cancel any ongoing speech
  stop();

  const utterance = new SpeechSynthesisUtterance(text);
  utterance.rate = 1.1; // Slightly faster
  utterance.pitch = 1.0;
  utterance.volume = 1.0;

  // Try to use a good voice if available
  const voices = window.speechSynthesis.getVoices();
  const preferredVoice = voices.find(
    (v) =>
      v.name.includes("Samantha") || // macOS
      v.name.includes("Microsoft Zira") || // Windows
      v.name.includes("Google") || // Chrome
      v.lang.startsWith("en")
  );
  if (preferredVoice) {
    utterance.voice = preferredVoice;
  }

  currentUtterance = utterance;
  window.speechSynthesis.speak(utterance);

  utterance.onend = () => {
    currentUtterance = null;
  };
}

/**
 * Stop any ongoing speech.
 */
export function stop(): void {
  if (window.speechSynthesis) {
    window.speechSynthesis.cancel();
  }
  currentUtterance = null;
}

/**
 * Initialize the speak listener.
 * Call this once when the app starts.
 */
export async function initSpeakListener(): Promise<() => void> {
  // Preload voices (some browsers need this)
  if (window.speechSynthesis) {
    window.speechSynthesis.getVoices();
  }

  const unlisten = await listen<string>("speak", (event) => {
    console.log("[Speak] Received:", event.payload);
    speak(event.payload);
  });

  console.log("[Speak] Listener initialized");
  return unlisten;
}
