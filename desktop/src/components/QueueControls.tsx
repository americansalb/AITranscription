import React from "react";
import * as queueStore from "../lib/queueStore";

interface QueueControlsProps {
  isPlaying: boolean;
  isPaused: boolean;
  autoPlay: boolean;
  volume: number;
}

export function QueueControls({
  isPlaying,
  isPaused: _isPaused,
  autoPlay,
  volume,
}: QueueControlsProps) {
  const handlePlayPause = () => {
    queueStore.togglePlayPause();
  };

  const handleSkipPrevious = () => {
    queueStore.skipPrevious();
  };

  const handleSkipNext = () => {
    queueStore.skipNext();
  };

  const handleVolumeChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    queueStore.setVolume(parseFloat(e.target.value));
  };

  const handleAutoPlayChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    queueStore.setAutoPlay(e.target.checked);
  };

  // Determine play/pause icon
  const getPlayPauseIcon = () => {
    if (isPlaying) return "⏸"; // Pause
    return "▶"; // Play
  };

  return (
    <div className="queue-controls">
      <div className="queue-controls-left">
        <button
          className="control-button"
          onClick={handleSkipPrevious}
          title="Previous"
        >
          ⏮
        </button>

        <button
          className="control-button play-pause"
          onClick={handlePlayPause}
          title={isPlaying ? "Pause" : "Play"}
        >
          {getPlayPauseIcon()}
        </button>

        <button
          className="control-button"
          onClick={handleSkipNext}
          title="Next"
        >
          ⏭
        </button>
      </div>

      <div className="queue-controls-right">
        <div className="volume-control">
          <span>🔊</span>
          <input
            type="range"
            className="volume-slider"
            min="0"
            max="1"
            step="0.1"
            value={volume}
            onChange={handleVolumeChange}
          />
        </div>

        <label className="autoplay-toggle">
          <input
            type="checkbox"
            checked={autoPlay}
            onChange={handleAutoPlayChange}
          />
          Auto-play
        </label>
      </div>
    </div>
  );
}

export default QueueControls;
