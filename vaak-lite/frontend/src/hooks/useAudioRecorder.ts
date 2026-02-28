import { useCallback, useRef, useState } from "react";

export interface UseAudioRecorderReturn {
  /** Whether the recorder is currently capturing audio. */
  isRecording: boolean;
  /** Start recording. Resolves once the recorder is active. */
  start: () => Promise<void>;
  /** Stop recording and return the captured audio blob. */
  stop: () => Promise<Blob>;
  /**
   * Enable chunked mode: the `onChunk` callback fires every `intervalMs`
   * with the latest audio chunk while recording continues.
   */
  startChunked: (intervalMs: number, onChunk: (blob: Blob, seq: number) => void) => Promise<void>;
  /** Current audio analyser node (for visualizer). */
  analyser: AnalyserNode | null;
  /** Duration of current recording in seconds (updated every ~250ms). */
  duration: number;
  /** Any error that occurred. */
  error: string | null;
}

/**
 * Hook wrapping the browser MediaRecorder API.
 *
 * Supports two usage patterns:
 * 1. `start()` → `stop()` → returns full Blob  (unidirectional / consecutive)
 * 2. `startChunked(5000, cb)` → `stop()`        (simultaneous mode)
 */
export function useAudioRecorder(): UseAudioRecorderReturn {
  const [isRecording, setIsRecording] = useState(false);
  const [analyser, setAnalyser] = useState<AnalyserNode | null>(null);
  const [duration, setDuration] = useState(0);
  const [error, setError] = useState<string | null>(null);

  const mediaRecorderRef = useRef<MediaRecorder | null>(null);
  const streamRef = useRef<MediaStream | null>(null);
  const audioCtxRef = useRef<AudioContext | null>(null);
  const chunksRef = useRef<Blob[]>([]);
  const timerRef = useRef<number>(0);
  const startTimeRef = useRef<number>(0);
  const resolveStopRef = useRef<((blob: Blob) => void) | null>(null);

  /** Pick a supported MIME type. Safari prefers mp4, others prefer webm. */
  const getMimeType = useCallback((): string => {
    if (MediaRecorder.isTypeSupported("audio/webm;codecs=opus")) return "audio/webm;codecs=opus";
    if (MediaRecorder.isTypeSupported("audio/webm")) return "audio/webm";
    if (MediaRecorder.isTypeSupported("audio/mp4")) return "audio/mp4";
    return "";
  }, []);

  const cleanup = useCallback(() => {
    if (timerRef.current) {
      clearInterval(timerRef.current);
      timerRef.current = 0;
    }
    streamRef.current?.getTracks().forEach((t) => t.stop());
    streamRef.current = null;
    audioCtxRef.current?.close();
    audioCtxRef.current = null;
    setAnalyser(null);
    setIsRecording(false);
    setDuration(0);
  }, []);

  const initStream = useCallback(async () => {
    const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
    streamRef.current = stream;

    // Create analyser for visualizer
    const ctx = new AudioContext();
    audioCtxRef.current = ctx;
    const source = ctx.createMediaStreamSource(stream);
    const node = ctx.createAnalyser();
    node.fftSize = 256;
    source.connect(node);
    setAnalyser(node);

    return stream;
  }, []);

  const startDurationTimer = useCallback(() => {
    startTimeRef.current = Date.now();
    timerRef.current = window.setInterval(() => {
      setDuration((Date.now() - startTimeRef.current) / 1000);
    }, 250);
  }, []);

  // ── Standard start/stop ──────────────────────────────────────

  const start = useCallback(async () => {
    setError(null);
    try {
      const stream = await initStream();
      const mime = getMimeType();
      const recorder = new MediaRecorder(stream, mime ? { mimeType: mime } : undefined);
      mediaRecorderRef.current = recorder;
      chunksRef.current = [];

      recorder.ondataavailable = (e) => {
        if (e.data.size > 0) chunksRef.current.push(e.data);
      };

      recorder.onstop = () => {
        const blob = new Blob(chunksRef.current, { type: recorder.mimeType });
        resolveStopRef.current?.(blob);
        resolveStopRef.current = null;
        cleanup();
      };

      recorder.start();
      setIsRecording(true);
      startDurationTimer();
    } catch (err) {
      const msg = err instanceof Error ? err.message : "Microphone access denied";
      setError(msg);
      cleanup();
    }
  }, [initStream, getMimeType, cleanup, startDurationTimer]);

  const stop = useCallback((): Promise<Blob> => {
    return new Promise((resolve, reject) => {
      const recorder = mediaRecorderRef.current;
      if (!recorder || recorder.state === "inactive") {
        reject(new Error("Not recording"));
        return;
      }
      resolveStopRef.current = resolve;
      recorder.stop();
    });
  }, []);

  // ── Chunked mode (simultaneous) ─────────────────────────────

  const startChunked = useCallback(
    async (intervalMs: number, onChunk: (blob: Blob, seq: number) => void) => {
      setError(null);
      try {
        const stream = await initStream();
        const mime = getMimeType();
        const recorder = new MediaRecorder(stream, mime ? { mimeType: mime } : undefined);
        mediaRecorderRef.current = recorder;
        chunksRef.current = [];

        let seq = 0;

        recorder.ondataavailable = (e) => {
          if (e.data.size > 0) {
            chunksRef.current.push(e.data);
            onChunk(e.data, seq++);
          }
        };

        recorder.onstop = () => {
          // Emit any remaining data as a final chunk
          if (chunksRef.current.length > 0) {
            const finalBlob = new Blob(chunksRef.current, { type: recorder.mimeType });
            resolveStopRef.current?.(finalBlob);
          }
          resolveStopRef.current = null;
          cleanup();
        };

        recorder.start(intervalMs);
        setIsRecording(true);
        startDurationTimer();
      } catch (err) {
        const msg = err instanceof Error ? err.message : "Microphone access denied";
        setError(msg);
        cleanup();
      }
    },
    [initStream, getMimeType, cleanup, startDurationTimer]
  );

  return { isRecording, start, stop, startChunked, analyser, duration, error };
}
