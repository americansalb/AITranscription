import { useEffect, useRef } from "react";

interface RecordingOverlayProps {
  isRecording: boolean;
  isProcessing: boolean;
  duration: number;
}

/**
 * Floating recording indicator that provides visual feedback.
 *
 * This component:
 * 1. Shows a floating pill when recording/processing
 * 2. Uses system notifications as backup (visible even when window is minimized)
 * 3. Displays duration while recording
 */
export function RecordingOverlay({
  isRecording,
  isProcessing,
  duration,
}: RecordingOverlayProps) {
  const notificationRef = useRef<Notification | null>(null);

  // Format duration as mm:ss
  const formatDuration = (seconds: number): string => {
    const mins = Math.floor(seconds / 60);
    const secs = seconds % 60;
    return `${mins}:${secs.toString().padStart(2, "0")}`;
  };

  // Request notification permission on mount
  useEffect(() => {
    if ("Notification" in window && Notification.permission === "default") {
      Notification.requestPermission();
    }
  }, []);

  // Show system notification when recording starts (backup for when window is minimized)
  useEffect(() => {
    if (isRecording && "Notification" in window && Notification.permission === "granted") {
      // Close any existing notification
      if (notificationRef.current) {
        notificationRef.current.close();
      }

      // Show new notification
      notificationRef.current = new Notification("Scribe Recording", {
        body: "Recording in progress... Release hotkey to stop.",
        icon: "/icons/32x32.png",
        tag: "scribe-recording", // Prevents duplicate notifications
        silent: true, // Don't play system sound (we have our own)
      });
    } else if (!isRecording && !isProcessing && notificationRef.current) {
      notificationRef.current.close();
      notificationRef.current = null;
    }
  }, [isRecording, isProcessing]);

  // Don't render if not recording or processing
  if (!isRecording && !isProcessing) {
    return null;
  }

  return (
    <div className="recording-overlay">
      <div className={`recording-pill ${isProcessing ? "processing" : "recording"}`}>
        <div className="recording-indicator">
          {isProcessing ? (
            <div className="processing-spinner" />
          ) : (
            <div className="recording-dot" />
          )}
        </div>
        <span className="recording-text">
          {isProcessing ? "Processing..." : `Recording ${formatDuration(duration)}`}
        </span>
      </div>
    </div>
  );
}
