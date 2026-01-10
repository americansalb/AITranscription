import { useState, useEffect } from "react";

interface OverlayState {
  isRecording: boolean;
  isProcessing: boolean;
  duration: number;
}

/**
 * Minimal overlay window that shows recording status.
 * This runs in a separate Tauri window that stays on top.
 */
export function OverlayApp() {
  const [state, setState] = useState<OverlayState>({
    isRecording: false,
    isProcessing: false,
    duration: 0,
  });

  useEffect(() => {
    // Listen for events from main window
    const setupListener = async () => {
      if (window.__TAURI__) {
        try {
          const { listen } = await import("@tauri-apps/api/event");

          const unlisten = await listen<OverlayState>("overlay-update", (event) => {
            setState(event.payload);
          });

          return () => {
            unlisten();
          };
        } catch (e) {
          console.error("Failed to setup overlay listener:", e);
        }
      }
    };

    setupListener();
  }, []);

  const formatDuration = (seconds: number): string => {
    const mins = Math.floor(seconds / 60);
    const secs = seconds % 60;
    return `${mins}:${secs.toString().padStart(2, "0")}`;
  };

  // Always show the overlay pill (it's in its own window)
  return (
    <div className="overlay-window">
      <div className={`overlay-pill ${state.isRecording ? "recording" : state.isProcessing ? "processing" : "idle"}`}>
        {state.isRecording ? (
          <>
            <div className="overlay-dot recording" />
            <span className="overlay-text">REC {formatDuration(state.duration)}</span>
          </>
        ) : state.isProcessing ? (
          <>
            <div className="overlay-spinner" />
            <span className="overlay-text">Processing...</span>
          </>
        ) : (
          <>
            <div className="overlay-dot idle" />
            <span className="overlay-text">Ready</span>
          </>
        )}
      </div>
    </div>
  );
}
