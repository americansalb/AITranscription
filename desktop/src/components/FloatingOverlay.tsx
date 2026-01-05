import { useEffect, useState, useMemo } from "react";

/**
 * Floating audio indicator window - shown while recording even when main app is minimized.
 * This renders in a separate transparent, always-on-top window.
 *
 * Beautiful, non-intrusive design inspired by WhisperFlow but better.
 */
export function FloatingOverlay() {
  const [isRecording, setIsRecording] = useState(true);
  const [isProcessing, setIsProcessing] = useState(false);
  const [duration, setDuration] = useState(0);
  const [audioLevel, setAudioLevel] = useState(0.5);

  // Listen for messages from the main window
  useEffect(() => {
    const handleMessage = (event: MessageEvent) => {
      if (event.data.type === "recording-state") {
        setIsRecording(event.data.isRecording);
        setIsProcessing(event.data.isProcessing);
        setDuration(event.data.duration || 0);
        setAudioLevel(event.data.audioLevel || 0);
      }
    };

    window.addEventListener("message", handleMessage);
    return () => window.removeEventListener("message", handleMessage);
  }, []);

  // Also listen for Tauri events
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
          setDuration(event.payload.duration);
          setAudioLevel(event.payload.audioLevel);
        });
      }
    };

    setupListener();
    return () => {
      if (unlisten) unlisten();
    };
  }, []);

  // Simulate audio level animation when recording
  useEffect(() => {
    if (!isRecording) return;

    const interval = setInterval(() => {
      // Gentle pulsing animation
      setAudioLevel((prev) => {
        const delta = (Math.random() - 0.5) * 0.3;
        return Math.max(0.2, Math.min(1, prev + delta));
      });
    }, 100);

    return () => clearInterval(interval);
  }, [isRecording]);

  // Duration timer
  useEffect(() => {
    if (!isRecording) return;

    const interval = setInterval(() => {
      setDuration((d) => d + 1);
    }, 1000);

    return () => clearInterval(interval);
  }, [isRecording]);

  // Format duration as mm:ss
  const formatDuration = (seconds: number): string => {
    const mins = Math.floor(seconds / 60);
    const secs = seconds % 60;
    return `${mins}:${secs.toString().padStart(2, "0")}`;
  };

  // Generate beautiful waveform bars
  const bars = useMemo(() => {
    const numBars = 9;
    const result: number[] = [];

    for (let i = 0; i < numBars; i++) {
      const centerDistance = Math.abs(i - numBars / 2) / (numBars / 2);
      const baseHeight = 1 - centerDistance * 0.5;
      const height = Math.max(0.15, baseHeight * audioLevel + Math.random() * 0.2 * audioLevel);
      result.push(height);
    }

    return result;
  }, [audioLevel]);

  return (
    <div className="floating-overlay-container">
      <div className={`floating-overlay ${isProcessing ? "processing" : "recording"}`}>
        {/* Audio waveform visualization */}
        <div className="floating-waveform">
          {bars.map((height, i) => (
            <div
              key={i}
              className="floating-bar"
              style={{
                height: `${height * 100}%`,
                animationDelay: `${i * 50}ms`,
              }}
            />
          ))}
        </div>

        {/* Status indicator */}
        <div className="floating-status">
          {isProcessing ? (
            <>
              <div className="floating-spinner" />
              <span>Processing...</span>
            </>
          ) : (
            <>
              <div className="floating-dot" />
              <span>{formatDuration(duration)}</span>
            </>
          )}
        </div>
      </div>
    </div>
  );
}
