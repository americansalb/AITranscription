import { useEffect, useState } from "react";

/**
 * Minimal floating recording indicator - WhisperFlow-inspired design.
 * Tiny pill with pulsing dot and 3 audio bars.
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

  return (
    <div className="mini-overlay">
      <div className={`mini-pill ${isProcessing ? "processing" : isRecording ? "recording" : "idle"}`}>
        {/* Pulsing dot */}
        <div className={`mini-dot ${isRecording ? "active" : ""}`} />

        {/* Audio bars */}
        <div className="mini-bars">
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
