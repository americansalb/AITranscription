export type TranscriptionMode = "unidirectional" | "conversational" | "consecutive" | "simultaneous";

interface ModeSelectorProps {
  mode: TranscriptionMode;
  onChange: (mode: TranscriptionMode) => void;
  disabled?: boolean;
}

const MODES: { id: TranscriptionMode; label: string; desc: string }[] = [
  { id: "unidirectional", label: "Unidirectional", desc: "One speaker, continuous" },
  { id: "conversational", label: "Conversational", desc: "Two speakers alternating" },
  { id: "consecutive", label: "Consecutive", desc: "Segment by segment" },
  { id: "simultaneous", label: "Simultaneous", desc: "Real-time streaming" },
];

export function ModeSelector({ mode, onChange, disabled }: ModeSelectorProps) {
  return (
    <div className="mode-selector" role="radiogroup" aria-label="Transcription mode">
      {MODES.map((m) => (
        <button
          key={m.id}
          className={`mode-btn ${mode === m.id ? "active" : ""}`}
          onClick={() => onChange(m.id)}
          disabled={disabled}
          role="radio"
          aria-checked={mode === m.id}
          title={m.desc}
        >
          <span className="mode-label">{m.label}</span>
          <span className="mode-desc">{m.desc}</span>
        </button>
      ))}
    </div>
  );
}
