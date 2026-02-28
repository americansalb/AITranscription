import { useEffect, useRef } from "react";
import { langName } from "../lib/languages";

export interface InterpretationEntry {
  id: number;
  sourceText: string;
  translatedText: string;
  sourceLang: string;
  targetLang: string;
  duration: number | null;
  provider: string;
  timestamp: Date;
  /** Which speaker in bidirectional mode. */
  speaker?: "A" | "B";
  /** Is this still being processed? */
  pending?: boolean;
}

interface InterpretationViewProps {
  entries: InterpretationEntry[];
  bidirectional: boolean;
  isRecording: boolean;
}

function formatTime(seconds: number): string {
  const m = Math.floor(seconds / 60);
  const s = Math.floor(seconds % 60);
  return `${m}:${s.toString().padStart(2, "0")}`;
}

export function InterpretationView({ entries, bidirectional, isRecording }: InterpretationViewProps) {
  const bottomRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [entries]);

  if (entries.length === 0) {
    return (
      <div className="interpretation-view empty">
        <p className="placeholder">
          {isRecording ? "Listening..." : "Press the button to start interpreting"}
        </p>
      </div>
    );
  }

  return (
    <div className="interpretation-view">
      {entries.map((entry) => (
        <div
          key={entry.id}
          className={`interp-entry ${bidirectional ? `speaker-${entry.speaker || "A"}` : ""} ${entry.pending ? "pending" : ""}`}
        >
          {bidirectional && entry.speaker && (
            <span className="speaker-badge">Speaker {entry.speaker}</span>
          )}

          <div className="interp-source">
            <span className="lang-badge">{langName(entry.sourceLang)}</span>
            <p>{entry.sourceText || (entry.pending ? "Transcribing..." : "")}</p>
          </div>

          <div className="interp-arrow">&#8595;</div>

          <div className="interp-translation">
            <span className="lang-badge target">{langName(entry.targetLang)}</span>
            <p>{entry.translatedText || (entry.pending ? "Translating..." : "")}</p>
          </div>

          <div className="interp-meta">
            {entry.duration != null && <span>{formatTime(entry.duration)}</span>}
            {entry.provider && !entry.pending && <span>{entry.provider}</span>}
          </div>
        </div>
      ))}
      <div ref={bottomRef} />
    </div>
  );
}
