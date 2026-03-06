import {
  VolumeOnIcon, VolumeOffIcon, CheckIcon,
} from "./Icons";
import { isMacOS, formatHotkeyForDisplay as formatHotkeyDisplay } from "../lib/platform";

// Processing steps for progress indicator
export type ProcessingStep = "recording" | "transcribing" | "polishing" | "done";

const PROCESSING_STEPS: { key: ProcessingStep; label: string }[] = [
  { key: "recording", label: "Recording" },
  { key: "transcribing", label: "Transcribing" },
  { key: "polishing", label: "Polishing" },
  { key: "done", label: "Done" },
];

function formatDuration(seconds: number): string {
  const mins = Math.floor(seconds / 60);
  const secs = seconds % 60;
  return `${mins}:${secs.toString().padStart(2, "0")}`;
}

export type ProcessingStatus = "idle" | "recording" | "processing" | "success" | "error";

export interface RecordingControlsProps {
  isRecording: boolean;
  status: ProcessingStatus;
  duration: number;
  backendReady: boolean | null;
  soundEnabled: boolean;
  processingStep: ProcessingStep;
  processingElapsed: number;
  hotkeyRegistered: boolean;
  currentHotkey: string;
  onRecordClick: () => void;
  onCancelProcessing: () => void;
  onSoundToggle: () => void;
  showToast: (message: string, type: "info" | "success" | "warning" | "error") => void;
}

export function RecordingControls({
  isRecording,
  status,
  duration,
  backendReady,
  soundEnabled,
  processingStep,
  processingElapsed,
  hotkeyRegistered,
  currentHotkey,
  onRecordClick,
  onCancelProcessing,
  onSoundToggle,
  showToast,
}: RecordingControlsProps) {
  const getStatusText = () => {
    switch (status) {
      case "recording":
        return `Recording... ${formatDuration(duration)}`;
      case "processing":
        return "Processing...";
      case "success":
        return "Ready";
      case "error":
        return "Error";
      default:
        return "Ready to record";
    }
  };

  return (
    <>
      {/* macOS in-app recording indicator (replaces floating overlay which steals focus) */}
      {isMacOS() && (isRecording || status === "processing") && (
        <div className="macos-recording-banner" role="alert" aria-live="assertive">
          <span className="macos-rec-dot" />
          <span className="macos-rec-label">
            {status === "processing" ? "Processing..." : `Recording ${Math.floor((duration || 0) / 60)}:${String(Math.floor((duration || 0) % 60)).padStart(2, "0")}`}
          </span>
          {isRecording && (
            <button className="macos-rec-stop" onClick={onRecordClick} title="Stop recording">
              Stop
            </button>
          )}
        </div>
      )}

      <div className={`recording-section ${isRecording ? 'is-recording' : ''}`}>
        {/* Sound Toggle */}
        <button
          className={`sound-toggle ${!soundEnabled ? "muted" : ""}`}
          onClick={onSoundToggle}
          title={soundEnabled ? "Mute sounds" : "Unmute sounds"}
          aria-label={soundEnabled ? "Mute sounds" : "Unmute sounds"}
        >
          {soundEnabled ? (
            <VolumeOnIcon size={16} />
          ) : (
            <VolumeOffIcon size={16} />
          )}
        </button>

        <button
          className={`record-btn ${isRecording ? "recording" : ""}`}
          onClick={onRecordClick}
          disabled={status === "processing" || backendReady === false}
          title={isRecording ? "Stop recording" : "Start recording"}
          aria-label={isRecording ? "Stop recording" : "Start recording"}
        >
          {status === "processing" ? (
            <div className="spinner" />
          ) : isRecording ? (
            <svg viewBox="0 0 24 24" fill="currentColor">
              <rect x="6" y="6" width="12" height="12" rx="2" />
            </svg>
          ) : (
            <svg viewBox="0 0 24 24" fill="currentColor">
              <circle cx="12" cy="12" r="6" />
            </svg>
          )}
        </button>

        {status === "processing" ? (
          <div className="progress-steps">
            {PROCESSING_STEPS.map((step, index) => {
              const stepIndex = PROCESSING_STEPS.findIndex(s => s.key === processingStep);
              const isActive = step.key === processingStep;
              const isComplete = index < stepIndex || processingStep === "done";
              const isCurrent = isActive && processingStep !== "done";

              return (
                <div key={step.key} className={`progress-step ${isComplete ? "complete" : ""} ${isCurrent ? "current" : ""}`}>
                  <div className="progress-step-indicator">
                    {isComplete ? (
                      <CheckIcon size={12} strokeWidth={3} />
                    ) : (
                      <span>{index + 1}</span>
                    )}
                  </div>
                  <span className="progress-step-label">
                    {step.label}
                    {isCurrent && processingElapsed > 0 && ` (${processingElapsed}s)`}
                  </span>
                </div>
              );
            })}
            <button
              className="cancel-processing-btn"
              onClick={() => {
                onCancelProcessing();
                showToast("Transcription cancelled", "warning");
              }}
              title="Cancel transcription"
              aria-label="Cancel transcription"
            >
              Cancel
            </button>
          </div>
        ) : (
          <div className="status">
            <span
              className={`status-dot ${status === "recording" ? "recording" : status === "success" ? "success" : ""}`}
            />
            {getStatusText()}
          </div>
        )}

        <p className="record-hint">
          Click to {isRecording ? "stop" : "start"} • Hold{" "}
          <span className="hotkey">{formatHotkeyDisplay(currentHotkey)}</span> for push-to-talk
          <span className={`hotkey-status ${hotkeyRegistered ? "active" : "inactive"}`}>
            {hotkeyRegistered ? "Ready" : "Restart app to enable"}
          </span>
          {" "} • Press <span className="hotkey">?</span> for shortcuts
        </p>
      </div>
    </>
  );
}
