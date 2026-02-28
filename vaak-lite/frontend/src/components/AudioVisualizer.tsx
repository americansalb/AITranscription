import { useEffect, useRef } from "react";

interface AudioVisualizerProps {
  analyser: AnalyserNode | null;
  isRecording: boolean;
}

export function AudioVisualizer({ analyser, isRecording }: AudioVisualizerProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const rafRef = useRef<number>(0);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas || !analyser || !isRecording) {
      // Clear canvas when not recording
      if (canvas) {
        const ctx = canvas.getContext("2d");
        if (ctx) {
          ctx.clearRect(0, 0, canvas.width, canvas.height);
        }
      }
      return;
    }

    const ctx = canvas.getContext("2d")!;
    const bufLen = analyser.frequencyBinCount;
    const data = new Uint8Array(bufLen);

    const draw = () => {
      rafRef.current = requestAnimationFrame(draw);
      analyser.getByteFrequencyData(data);

      const w = canvas.width;
      const h = canvas.height;
      ctx.clearRect(0, 0, w, h);

      const barCount = 32;
      const barWidth = w / barCount - 2;
      const step = Math.floor(bufLen / barCount);

      for (let i = 0; i < barCount; i++) {
        const val = data[i * step] / 255;
        const barH = Math.max(2, val * h);
        const x = i * (barWidth + 2);
        const y = (h - barH) / 2;

        ctx.fillStyle = `hsl(210, 80%, ${50 + val * 30}%)`;
        ctx.beginPath();
        ctx.roundRect(x, y, barWidth, barH, 2);
        ctx.fill();
      }
    };

    draw();

    return () => {
      cancelAnimationFrame(rafRef.current);
    };
  }, [analyser, isRecording]);

  return (
    <canvas
      ref={canvasRef}
      className="audio-visualizer"
      width={320}
      height={48}
      aria-hidden="true"
    />
  );
}
