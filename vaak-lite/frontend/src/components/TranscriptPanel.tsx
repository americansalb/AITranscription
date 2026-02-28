import { useEffect, useRef } from "react";
import type { TranscriptionMode } from "./ModeSelector";
import type { Segment } from "../lib/api";

export interface TranscriptEntry {
  id: number;
  text: string;
  language: string | null;
  duration: number | null;
  segments: Segment[];
  timestamp: Date;
  /** For consecutive mode: the segment number. */
  segmentNumber?: number;
  /** For conversational mode: which speaker. */
  speaker?: "A" | "B";
  /** For simultaneous mode: is this a partial (in-flight) result? */
  partial?: boolean;
}

interface TranscriptPanelProps {
  entries: TranscriptEntry[];
  mode: TranscriptionMode;
  isRecording: boolean;
}

function formatTime(seconds: number): string {
  const m = Math.floor(seconds / 60);
  const s = Math.floor(seconds % 60);
  return `${m}:${s.toString().padStart(2, "0")}`;
}

export function TranscriptPanel({ entries, mode, isRecording }: TranscriptPanelProps) {
  const bottomRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [entries]);

  if (entries.length === 0) {
    return (
      <div className="transcript-panel empty">
        <p className="placeholder">
          {isRecording
            ? "Listening..."
            : mode === "consecutive"
              ? "Press the button to record a segment"
              : "Press the button to start transcribing"}
        </p>
      </div>
    );
  }

  return (
    <div className="transcript-panel">
      {mode === "conversational"
        ? entries.map((e) => (
            <div key={e.id} className={`transcript-entry conversation speaker-${e.speaker || "A"}`}>
              <span className="speaker-label">Speaker {e.speaker || "A"}</span>
              <p>{e.text}</p>
              {e.duration != null && (
                <span className="entry-meta">{formatTime(e.duration)}</span>
              )}
            </div>
          ))
        : mode === "consecutive"
          ? entries.map((e) => (
              <div key={e.id} className="transcript-entry consecutive">
                <div className="segment-header">
                  <span className="segment-num">Segment {e.segmentNumber ?? e.id}</span>
                  {e.duration != null && (
                    <span className="entry-meta">{formatTime(e.duration)}</span>
                  )}
                  {e.language && <span className="entry-lang">{e.language}</span>}
                </div>
                <p>{e.text}</p>
              </div>
            ))
          : mode === "simultaneous"
            ? entries.map((e) => (
                <span key={e.id} className={`sim-chunk ${e.partial ? "partial" : "confirmed"}`}>
                  {e.text}{" "}
                </span>
              ))
            : /* unidirectional */
              entries.map((e) => (
                <div key={e.id} className="transcript-entry uni">
                  <p>{e.text}</p>
                  <div className="entry-footer">
                    {e.duration != null && (
                      <span className="entry-meta">{formatTime(e.duration)}</span>
                    )}
                    {e.language && <span className="entry-lang">{e.language}</span>}
                  </div>
                </div>
              ))}
      <div ref={bottomRef} />
    </div>
  );
}
