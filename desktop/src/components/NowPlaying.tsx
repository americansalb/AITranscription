import { useState, useEffect } from "react";
import type { QueueItem } from "../lib/queueTypes";

interface NowPlayingProps {
  item: QueueItem;
  isPlaying: boolean;
  isPaused: boolean;
}

export function NowPlaying({ item, isPlaying, isPaused }: NowPlayingProps) {
  const [elapsedTime, setElapsedTime] = useState(0);

  // Update elapsed time every 100ms while playing
  useEffect(() => {
    if (!isPlaying || isPaused) return;

    const startTime = item.startedAt || Date.now();
    const interval = setInterval(() => {
      setElapsedTime(Date.now() - startTime);
    }, 100);

    return () => clearInterval(interval);
  }, [isPlaying, isPaused, item.startedAt]);

  // Format time as MM:SS
  const formatTime = (ms: number): string => {
    const seconds = Math.floor(ms / 1000);
    const mins = Math.floor(seconds / 60);
    const secs = seconds % 60;
    return `${mins}:${secs.toString().padStart(2, "0")}`;
  };

  // Estimate duration based on text length (rough approximation: 150 words per minute)
  const estimatedDuration = item.durationMs || (item.text.split(/\s+/).length / 150) * 60 * 1000;
  const progress = Math.min((elapsedTime / estimatedDuration) * 100, 100);

  return (
    <div className="now-playing">
      <div className="now-playing-header">
        <span className={`speaker-icon ${isPlaying ? "playing" : ""}`}>
          {isPaused ? "⏸" : "🔊"}
        </span>
        <span>{isPaused ? "Paused" : "Now Playing"}</span>
      </div>

      <div className="now-playing-text">{item.text}</div>

      <div className="now-playing-progress">
        <div className="progress-bar">
          <div
            className="progress-bar-fill"
            style={{ width: `${progress}%` }}
          />
        </div>
        <span className="progress-time">
          {formatTime(elapsedTime)} / {formatTime(estimatedDuration)}
        </span>
      </div>

      <div className="now-playing-session">
        Session: {item.sessionId}
      </div>
    </div>
  );
}

export default NowPlaying;
