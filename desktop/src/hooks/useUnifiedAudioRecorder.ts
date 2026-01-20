import { useAudioRecorder, UseAudioRecorderReturn } from "./useAudioRecorder";
import { useNativeAudioRecorder } from "./useNativeAudioRecorder";

/**
 * Unified audio recorder that uses native Rust recording in Tauri desktop app
 * and falls back to browser MediaRecorder for web.
 *
 * Native recording provides consistent high-quality WAV audio on all platforms
 * (Mac, Windows, Linux) whereas browser recording has codec limitations
 * (WebKit on Mac doesn't support WebM/Opus).
 */
export function useUnifiedAudioRecorder(): UseAudioRecorderReturn {
  const isTauri = typeof window !== "undefined" && "__TAURI__" in window;

  // Native recorder for desktop (Tauri)
  const nativeRecorder = useNativeAudioRecorder();

  // Browser recorder for web fallback
  const browserRecorder = useAudioRecorder();

  // Return native recorder in Tauri environment
  if (isTauri) {
    return {
      ...nativeRecorder,
      // Native recorder doesn't support pause/resume yet, but provide stubs
      pauseRecording: () => {
        console.warn("[UnifiedAudioRecorder] Pause not supported in native mode");
      },
      resumeRecording: () => {
        console.warn("[UnifiedAudioRecorder] Resume not supported in native mode");
      },
    };
  }

  // Fallback to browser recorder for web
  return browserRecorder;
}
