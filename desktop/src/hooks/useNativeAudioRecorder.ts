import { useState, useRef, useCallback, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, UnlistenFn } from "@tauri-apps/api/event";

interface AudioData {
  audio_base64: string;
  mime_type: string;
  duration_secs: number;
  sample_rate: number;
}

interface AudioLevelEvent {
  level: number;
}

export interface NativeAudioRecorderState {
  isRecording: boolean;
  isPaused: boolean;
  duration: number;
  error: string | null;
  audioLevel: number;
}

export interface UseNativeAudioRecorderReturn extends NativeAudioRecorderState {
  startRecording: () => Promise<void>;
  stopRecording: () => Promise<Blob | null>;
  cancelRecording: () => void;
}

/**
 * Native audio recorder using Rust/cpal backend
 * Provides consistent high-quality audio on all platforms (Mac, Windows, Linux)
 * Uses WAV format for maximum compatibility with Whisper
 */
export function useNativeAudioRecorder(): UseNativeAudioRecorderReturn {
  const [state, setState] = useState<NativeAudioRecorderState>({
    isRecording: false,
    isPaused: false,
    duration: 0,
    error: null,
    audioLevel: 0,
  });

  const startTime = useRef<number>(0);
  const durationInterval = useRef<number | null>(null);
  const unlistenRef = useRef<UnlistenFn | null>(null);

  // Cleanup function
  const cleanup = useCallback(() => {
    if (durationInterval.current) {
      clearInterval(durationInterval.current);
      durationInterval.current = null;
    }
    if (unlistenRef.current) {
      unlistenRef.current();
      unlistenRef.current = null;
    }
  }, []);

  // Listen for audio level events from Rust backend
  useEffect(() => {
    let mounted = true;

    const setupListener = async () => {
      if (!("__TAURI__" in window)) return;

      try {
        const unlisten = await listen<AudioLevelEvent>(
          "audio_level",
          (event) => {
            if (mounted) {
              setState((s) => ({ ...s, audioLevel: event.payload.level }));
            }
          }
        );

        // Only keep listener if still mounted, otherwise cleanup immediately
        if (mounted) {
          unlistenRef.current = unlisten;
        } else {
          unlisten();
        }
      } catch (err) {
        console.error("[NativeAudioRecorder] Failed to setup audio level listener:", err);
      }
    };

    setupListener();

    return () => {
      mounted = false;
      cleanup();
    };
  }, [cleanup]);

  const startRecording = useCallback(async () => {
    try {
      setState((s) => ({ ...s, error: null }));

      // Check if running in Tauri
      if (!("__TAURI__" in window)) {
        throw new Error("Native recording only available in desktop app");
      }

      console.log("[NativeAudioRecorder] Starting native recording...");

      // Start native recording
      await invoke("start_recording");

      startTime.current = Date.now();

      // Update duration every 100ms
      durationInterval.current = window.setInterval(() => {
        setState((s) => ({
          ...s,
          duration: Math.floor((Date.now() - startTime.current) / 1000),
        }));
      }, 100);

      setState((s) => ({
        ...s,
        isRecording: true,
        isPaused: false,
        duration: 0,
      }));

      console.log("[NativeAudioRecorder] Recording started");
    } catch (err) {
      const message =
        err instanceof Error ? err.message : "Failed to start recording";
      console.error("[NativeAudioRecorder] Start error:", message);
      setState((s) => ({ ...s, error: message }));
      throw err;
    }
  }, []);

  const stopRecording = useCallback(async (): Promise<Blob | null> => {
    cleanup();

    try {
      console.log("[NativeAudioRecorder] Stopping recording...");

      // Stop native recording and get audio data
      const audioData = await invoke<AudioData>("stop_recording");

      // Validate audio data before processing
      if (!audioData || typeof audioData.audio_base64 !== "string" || !audioData.audio_base64) {
        throw new Error("Invalid audio data received from native recorder");
      }
      if (!audioData.mime_type) {
        throw new Error("Missing MIME type in audio data");
      }
      // Validate duration - must be positive and reasonable
      if (typeof audioData.duration_secs !== "number" || audioData.duration_secs <= 0) {
        throw new Error("Recording has no audio (0 second duration)");
      }
      if (audioData.duration_secs < 0.3) {
        throw new Error("Recording too short (less than 0.3 seconds)");
      }

      // Convert base64 to Blob
      let binaryString: string;
      try {
        binaryString = atob(audioData.audio_base64);
      } catch (decodeError) {
        throw new Error("Failed to decode audio data: invalid base64");
      }

      const bytes = new Uint8Array(binaryString.length);
      for (let i = 0; i < binaryString.length; i++) {
        bytes[i] = binaryString.charCodeAt(i);
      }
      const blob = new Blob([bytes], { type: audioData.mime_type });

      console.log(
        "[NativeAudioRecorder] Recording stopped:",
        blob.size,
        "bytes,",
        audioData.duration_secs.toFixed(2),
        "seconds,",
        audioData.sample_rate,
        "Hz"
      );

      setState((s) => ({
        ...s,
        isRecording: false,
        isPaused: false,
        audioLevel: 0,
      }));

      return blob;
    } catch (err) {
      const message =
        err instanceof Error ? err.message : "Failed to stop recording";
      console.error("[NativeAudioRecorder] Stop error:", message);
      setState((s) => ({ ...s, error: message, isRecording: false }));
      return null;
    }
  }, [cleanup]);

  const cancelRecording = useCallback(() => {
    cleanup();

    console.log("[NativeAudioRecorder] Cancelling recording...");
    invoke("cancel_recording").catch(console.error);

    setState({
      isRecording: false,
      isPaused: false,
      duration: 0,
      error: null,
      audioLevel: 0,
    });
  }, [cleanup]);

  return {
    ...state,
    startRecording,
    stopRecording,
    cancelRecording,
  };
}
