import { useState } from "react";
import type { DiscussionMode } from "../lib/collabTypes";
import { getModeColor } from "../utils/roleColors";

/** Discussion formats available in the quick-launch bar */
const FORMATS: Array<{ mode: DiscussionMode; label: string; desc: string }> = [
  { mode: "delphi", label: "Delphi", desc: "Blind consensus" },
  { mode: "oxford", label: "Oxford", desc: "Structured debate" },
  { mode: "pipeline", label: "Pipeline", desc: "Sequential turns" },
  { mode: "red_team" as DiscussionMode, label: "Red Team", desc: "Attack/defend" },
  { mode: "continuous", label: "Continuous", desc: "Ambient review" },
];

interface QuickLaunchBarProps {
  /** Whether a discussion is currently active (hides the bar) */
  discussionActive: boolean;
  /** Whether the launch is in progress */
  launching: boolean;
  /** Callback to start a discussion with the given format and topic */
  onLaunch: (format: string, topic: string) => void;
  /** Callback to open the full advanced settings dialog */
  onOpenAdvanced: () => void;
}

/**
 * QuickLaunchBar — inline discussion launcher that replaces the modal for common cases.
 *
 * Layout: [Topic input] [Format pills] [Go] [⚙]
 *
 * The Go button starts a discussion immediately with all active participants.
 * The ⚙ button opens the full advanced settings dialog for participant selection,
 * pipeline ordering, audience config, etc.
 *
 * Hidden when a discussion is already active.
 */
export function QuickLaunchBar({ discussionActive, launching, onLaunch, onOpenAdvanced }: QuickLaunchBarProps) {
  const [selectedFormat, setSelectedFormat] = useState<string>("pipeline");
  const [topic, setTopic] = useState("");

  if (discussionActive) return null;

  const canLaunch = (topic.trim() !== "" || selectedFormat === "continuous") && !launching;

  const handleGo = () => {
    if (!canLaunch) return;
    onLaunch(selectedFormat, topic.trim());
    setTopic("");
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Enter" && canLaunch) {
      e.preventDefault();
      handleGo();
    }
  };

  return (
    <div className="quick-launch-bar" role="form" aria-label="Start a discussion">
      {/* Topic input */}
      <input
        className="quick-launch-topic"
        type="text"
        placeholder="What should we discuss?"
        value={topic}
        onChange={(e) => setTopic(e.target.value)}
        onKeyDown={handleKeyDown}
        disabled={launching}
        aria-label="Discussion topic"
      />

      {/* Format pills */}
      <div className="quick-launch-formats" role="radiogroup" aria-label="Discussion format">
        {FORMATS.map(({ mode, label }) => {
          const isSelected = selectedFormat === mode;
          const color = getModeColor(mode);
          return (
            <button
              key={mode}
              className={`quick-launch-pill${isSelected ? " quick-launch-pill-active" : ""}`}
              style={isSelected ? {
                background: `${color}20`,
                color: color,
                borderColor: `${color}66`,
              } : undefined}
              onClick={() => setSelectedFormat(mode)}
              role="radio"
              aria-checked={isSelected}
              aria-label={`${label} format`}
              title={FORMATS.find(f => f.mode === mode)?.desc}
              disabled={launching}
            >
              {label}
            </button>
          );
        })}
      </div>

      {/* Go button */}
      <button
        className="quick-launch-go"
        onClick={handleGo}
        disabled={!canLaunch}
        aria-label="Start discussion"
      >
        {launching ? (
          <span className="quick-launch-spinner" aria-hidden="true" />
        ) : (
          "Go"
        )}
      </button>

      {/* Advanced settings toggle */}
      <button
        className="quick-launch-advanced"
        onClick={onOpenAdvanced}
        aria-label="Advanced discussion settings"
        title="Advanced settings (participants, ordering, audience)"
        disabled={launching}
      >
        ⚙
      </button>
    </div>
  );
}
