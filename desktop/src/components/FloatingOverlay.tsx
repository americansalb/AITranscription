import { useEffect, useState } from "react";

/**
 * Minimal floating recording indicator - Wispr Flow-inspired design.
 * Shows a clean, modern pill when recording/processing.
 */
export function FloatingOverlay() {
  const [isRecording, setIsRecording] = useState(false);
  const [isProcessing, setIsProcessing] = useState(false);
  const [audioLevel, setAudioLevel] = useState(0);
  const [duration, setDuration] = useState(0);

  // Listen for Tauri events from main window
  useEffect(() => {
    let unlisten: (() => void) | undefined;

    const setupListener = async () => {
      if (typeof window !== "undefined" && "__TAURI__" in window) {
        const { listen } = await import("@tauri-apps/api/event");
        unlisten = await listen<{
          isRecording: boolean;
          isProcessing: boolean;
          duration: number;
          audioLevel: number;
        }>("recording-state", (event) => {
          setIsRecording(event.payload.isRecording);
          setIsProcessing(event.payload.isProcessing);
          setAudioLevel(event.payload.audioLevel);
          setDuration(event.payload.duration);
        });
      }
    };

    setupListener();
    return () => {
      if (unlisten) unlisten();
    };
  }, []);

  // Format duration as m:ss
  const formatDuration = (secs: number) => {
    const m = Math.floor(secs / 60);
    const s = secs % 60;
    return `${m}:${s.toString().padStart(2, "0")}`;
  };

  // Generate 4 bar heights based on audio level for visualizer
  const bars = [
    Math.max(0.2, audioLevel * 0.6 + Math.sin(Date.now() / 200) * 0.1),
    Math.max(0.3, audioLevel * 0.9 + Math.sin(Date.now() / 150 + 1) * 0.1),
    Math.max(0.3, audioLevel + Math.sin(Date.now() / 180 + 2) * 0.1),
    Math.max(0.2, audioLevel * 0.7 + Math.sin(Date.now() / 160 + 3) * 0.1),
  ];

  // Determine state
  const isActive = isRecording || isProcessing;

  return (
    <div className="floating-overlay-container">
      <div className={`floating-pill ${isRecording ? "recording" : ""} ${isProcessing ? "processing" : ""}`}>
        {/* Recording indicator dot */}
        <div className="floating-dot" />

        {/* Audio visualizer bars */}
        {isRecording && (
          <div className="floating-bars">
            {bars.map((h, i) => (
              <div
                key={i}
                className="floating-bar"
                style={{ height: `${Math.min(h, 1) * 100}%` }}
              />
            ))}
          </div>
        )}

        {/* Processing spinner */}
        {isProcessing && (
          <div className="floating-spinner" />
        )}

        {/* Duration text */}
        {isActive && (
          <span className="floating-text">
            {isProcessing ? "..." : formatDuration(duration)}
          </span>
        )}
      </div>
    </div>
  );
}
