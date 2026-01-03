import { useMemo } from "react";

interface AudioVisualizerProps {
  isRecording: boolean;
  audioLevel: number; // 0-1
}

/**
 * A small audio level visualizer widget that shows audio input levels
 * during recording as animated bars.
 */
export function AudioVisualizer({ isRecording, audioLevel }: AudioVisualizerProps) {
  // Generate bar heights based on audio level
  const bars = useMemo(() => {
    const numBars = 16;
    const result: number[] = [];

    for (let i = 0; i < numBars; i++) {
      // Create a wave pattern based on position and audio level
      const centerDistance = Math.abs(i - numBars / 2) / (numBars / 2);
      const baseHeight = 1 - centerDistance * 0.6;
      const height = isRecording
        ? Math.max(0.15, baseHeight * audioLevel + Math.random() * 0.1 * audioLevel)
        : 0.15;
      result.push(height);
    }

    return result;
  }, [isRecording, audioLevel]);

  return (
    <div className="audio-visualizer" aria-label="Audio level indicator">
      <div className="visualizer-bars">
        {bars.map((height, i) => (
          <div
            key={i}
            className={`visualizer-bar ${isRecording ? "active" : ""}`}
            style={{
              height: `${height * 100}%`,
              animationDelay: `${i * 30}ms`,
            }}
          />
        ))}
      </div>
    </div>
  );
}
