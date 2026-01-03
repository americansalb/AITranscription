import { useMemo } from "react";

interface AudioVisualizerProps {
  isRecording: boolean;
  audioLevel: number; // 0-1
}

/**
 * A compact, subtle audio level indicator that appears during recording.
 * Small enough to not distract, visible enough to confirm audio input.
 */
export function AudioVisualizer({ isRecording, audioLevel }: AudioVisualizerProps) {
  // Only show when recording
  if (!isRecording) return null;

  // Generate bar heights based on audio level - just 5 compact bars
  const bars = useMemo(() => {
    const numBars = 5;
    const result: number[] = [];

    for (let i = 0; i < numBars; i++) {
      // Create a wave pattern based on position and audio level
      const centerDistance = Math.abs(i - numBars / 2) / (numBars / 2);
      const baseHeight = 1 - centerDistance * 0.4;
      const height = Math.max(0.2, baseHeight * audioLevel + Math.random() * 0.15 * audioLevel);
      result.push(height);
    }

    return result;
  }, [audioLevel]);

  return (
    <div className="audio-visualizer" aria-label="Audio level indicator">
      <div className="visualizer-bars">
        {bars.map((height, i) => (
          <div
            key={i}
            className="visualizer-bar active"
            style={{
              height: `${height * 100}%`,
            }}
          />
        ))}
      </div>
    </div>
  );
}
