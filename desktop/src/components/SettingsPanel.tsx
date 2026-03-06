import { useCallback } from "react";
import { DownloadIcon } from "./Icons";

// Context icons for the dropdown
const CONTEXT_OPTIONS = [
  { value: "general", label: "General", icon: "🎯" },
  { value: "email", label: "Email", icon: "📧" },
  { value: "slack", label: "Slack / Chat", icon: "💬" },
  { value: "document", label: "Document", icon: "📄" },
  { value: "code", label: "Code Comment", icon: "💻" },
];

// Quick Mode Presets
export interface Preset {
  id: string;
  name: string;
  icon: string;
  context: string;
  formality: "casual" | "neutral" | "formal";
  description: string;
}

const PRESETS: Preset[] = [
  { id: "pro-email", name: "Professional Email", icon: "📧", context: "email", formality: "formal", description: "Formal business emails" },
  { id: "casual-slack", name: "Casual Chat", icon: "💬", context: "slack", formality: "casual", description: "Friendly messages" },
  { id: "meeting-notes", name: "Meeting Notes", icon: "📝", context: "document", formality: "neutral", description: "Clear documentation" },
  { id: "code-docs", name: "Code Comments", icon: "💻", context: "code", formality: "neutral", description: "Technical docs" },
];

export interface SettingsPanelProps {
  context: string;
  formality: "casual" | "neutral" | "formal";
  historyCount: number;
  onContextChange: (context: string) => void;
  onFormalityChange: (formality: "casual" | "neutral" | "formal") => void;
  onExportClick: () => void;
  showToast: (message: string, type: "info" | "success" | "warning" | "error") => void;
}

export function SettingsPanel({
  context,
  formality,
  historyCount,
  onContextChange,
  onFormalityChange,
  onExportClick,
  showToast,
}: SettingsPanelProps) {
  const handlePresetSelect = useCallback((preset: Preset) => {
    onContextChange(preset.context);
    onFormalityChange(preset.formality);
    showToast(`Applied "${preset.name}" preset`, "info");
  }, [onContextChange, onFormalityChange, showToast]);

  return (
    <>
      {/* Quick Mode Presets */}
      <div className="presets-section">
        <span className="presets-label">Quick Modes:</span>
        <div className="presets-list">
          {PRESETS.map((preset) => (
            <button
              key={preset.id}
              className={`preset-btn ${context === preset.context && formality === preset.formality ? "active" : ""}`}
              onClick={() => handlePresetSelect(preset)}
              title={preset.description}
            >
              <span className="preset-icon">{preset.icon}</span>
              <span className="preset-name">{preset.name}</span>
            </button>
          ))}
        </div>
      </div>

      <div className="context-selector">
        <label>Context:</label>
        <select value={context} onChange={(e) => onContextChange(e.target.value)}>
          {CONTEXT_OPTIONS.map((opt) => (
            <option key={opt.value} value={opt.value}>
              {opt.label}
            </option>
          ))}
        </select>

        <label>Tone:</label>
        <select
          value={formality}
          onChange={(e) =>
            onFormalityChange(e.target.value as "casual" | "neutral" | "formal")
          }
        >
          <option value="casual">Casual</option>
          <option value="neutral">Neutral</option>
          <option value="formal">Formal</option>
        </select>

        {historyCount > 0 && (
          <button
            className="export-history-btn"
            onClick={onExportClick}
            title="Export transcript history"
          >
            <DownloadIcon />
            Export ({historyCount})
          </button>
        )}
      </div>
    </>
  );
}
