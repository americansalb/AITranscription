import { useEffect, useState } from "react";

/**
 * Minimal floating recording indicator - WhisperFlow-inspired design.
 * Always visible: small line when idle, expands to pill with audio bars when recording.
 */
export function FloatingOverlay() {
  const [isRecording, setIsRecording] = useState(false);
  const [isProcessing, setIsProcessing] = useState(false);
  const [audioLevel, setAudioLevel] = useState(0);

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
        });
      }
    };

    setupListener();
    return () => {
      if (unlisten) unlisten();
    };
  }, []);

  // Generate 3 bar heights based on audio level
  const bars = [
    Math.max(0.3, audioLevel * 0.7 + Math.random() * 0.1),
    Math.max(0.4, audioLevel + Math.random() * 0.1),
    Math.max(0.3, audioLevel * 0.8 + Math.random() * 0.1),
  ];

  // Determine state for CSS class
  const state = isProcessing ? "processing" : isRecording ? "recording" : "idle";
  const isExpanded = isRecording || isProcessing;

  return (
    <div className="mini-overlay">
      <div className={`mini-pill ${state} ${isExpanded ? "expanded" : "collapsed"}`}>
        {/* Pulsing dot - always visible */}
        <div className={`mini-dot ${isRecording ? "active" : ""}`} />

        {/* Audio bars - only visible when expanded */}
        <div className={`mini-bars ${isExpanded ? "visible" : ""}`}>
          {bars.map((h, i) => (
            <div
              key={i}
              className="mini-bar"
              style={{ height: `${h * 100}%` }}
            />
          ))}
        </div>
      </div>
    </div>
  );
}
