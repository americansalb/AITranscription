interface RecordButtonProps {
  isRecording: boolean;
  onClick: () => void;
  disabled?: boolean;
  hint?: string;
}

export function RecordButton({ isRecording, onClick, disabled, hint }: RecordButtonProps) {
  return (
    <div className="record-wrapper">
      <button
        className={`record-btn ${isRecording ? "recording" : ""}`}
        onClick={onClick}
        disabled={disabled}
        aria-label={isRecording ? "Stop recording" : "Start recording"}
      >
        <span className="record-icon" />
      </button>
      {hint && <span className="record-hint">{hint}</span>}
    </div>
  );
}
