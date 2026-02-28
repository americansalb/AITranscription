import { useCallback, useRef, useState } from "react";

export interface UseAudioRecorderReturn {
  isRecording: boolean;
  /** Start recording. Returns once the recorder is active. */
  start: () => Promise<void>;
  /** Stop recording and return the captured audio blob. */
  stop: () => Promise<Blob>;
  /**
   * Start chunked recording for simultaneous mode.
   * Fires `onChunk` every `intervalMs` with the latest audio chunk.
   */
  startChunked: (intervalMs: number, onChunk: (blob: Blob, seq: number) => void) => Promise<void>;
  /**
   * Start recording with silence detection for auto-consecutive mode.
   * Fires `onSilence` when silence exceeds `silenceMs`.
   * Call `stop()` after onSilence to get the blob.
   */
  startWithSilenceDetection: (silenceMs: number, onSilence: () => void) => Promise<void>;
  /** Current audio analyser node (for visualizer). */
  analyser: AnalyserNode | null;
  /** Duration of current recording in seconds. */
  duration: number;
  error: string | null;
}

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
  const silenceTimerRef = useRef<number>(0);

  const getMimeType = useCallback((): string => {
    if (typeof MediaRecorder === "undefined") return "";
    if (MediaRecorder.isTypeSupported("audio/webm;codecs=opus")) return "audio/webm;codecs=opus";
    if (MediaRecorder.isTypeSupported("audio/webm")) return "audio/webm";
    if (MediaRecorder.isTypeSupported("audio/mp4")) return "audio/mp4";
    return "";
  }, []);

  const cleanup = useCallback(() => {
    if (timerRef.current) { clearInterval(timerRef.current); timerRef.current = 0; }
    if (silenceTimerRef.current) { clearInterval(silenceTimerRef.current); silenceTimerRef.current = 0; }
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

  // ── Standard start / stop ────────────────────────────

  const start = useCallback(async () => {
    setError(null);
    try {
      const stream = await initStream();
      const mime = getMimeType();
      const recorder = new MediaRecorder(stream, mime ? { mimeType: mime } : undefined);
      mediaRecorderRef.current = recorder;
      chunksRef.current = [];

      recorder.ondataavailable = (e) => { if (e.data.size > 0) chunksRef.current.push(e.data); };
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
      setError(err instanceof Error ? err.message : "Microphone access denied");
      cleanup();
    }
  }, [initStream, getMimeType, cleanup, startDurationTimer]);

  const stop = useCallback((): Promise<Blob> => {
    return new Promise((resolve, reject) => {
      const recorder = mediaRecorderRef.current;
      if (!recorder || recorder.state === "inactive") { reject(new Error("Not recording")); return; }
      resolveStopRef.current = resolve;
      recorder.stop();
    });
  }, []);

  // ── Chunked mode (simultaneous) ─────────────────────
  // Accumulates all chunks and sends the *full* recording so far each interval.
  // This gives Whisper full context so sentences aren't cut mid-word.

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
          }
        };
        recorder.onstop = () => {
          const blob = new Blob(chunksRef.current, { type: recorder.mimeType });
          resolveStopRef.current?.(blob);
          resolveStopRef.current = null;
          cleanup();
        };

        // Use timeslice to get periodic data, but send accumulated audio
        recorder.start(intervalMs);

        // Periodically send the full accumulated audio for context
        const chunkInterval = window.setInterval(() => {
          if (chunksRef.current.length > 0) {
            const fullBlob = new Blob(chunksRef.current, { type: recorder.mimeType });
            onChunk(fullBlob, seq++);
          }
        }, intervalMs + 200); // slight offset so ondataavailable fires first

        // Store interval for cleanup
        const origCleanup = recorder.onstop;
        recorder.onstop = () => {
          clearInterval(chunkInterval);
          const blob = new Blob(chunksRef.current, { type: recorder.mimeType });
          resolveStopRef.current?.(blob);
          resolveStopRef.current = null;
          cleanup();
        };

        setIsRecording(true);
        startDurationTimer();
      } catch (err) {
        setError(err instanceof Error ? err.message : "Microphone access denied");
        cleanup();
      }
    },
    [initStream, getMimeType, cleanup, startDurationTimer],
  );

  // ── Silence detection (auto-consecutive) ────────────

  const startWithSilenceDetection = useCallback(
    async (silenceMs: number, onSilence: () => void) => {
      setError(null);
      try {
        const stream = await initStream();
        const mime = getMimeType();
        const recorder = new MediaRecorder(stream, mime ? { mimeType: mime } : undefined);
        mediaRecorderRef.current = recorder;
        chunksRef.current = [];

        recorder.ondataavailable = (e) => { if (e.data.size > 0) chunksRef.current.push(e.data); };
        recorder.onstop = () => {
          const blob = new Blob(chunksRef.current, { type: recorder.mimeType });
          resolveStopRef.current?.(blob);
          resolveStopRef.current = null;
          cleanup();
        };

        recorder.start();
        setIsRecording(true);
        startDurationTimer();

        // Monitor audio level for silence — reuse the analyser from initStream
        const analyserNode = audioCtxRef.current ? (() => {
          const a = audioCtxRef.current!.createAnalyser();
          a.fftSize = 256;
          // Connect from the existing source (already created in initStream)
          const src = audioCtxRef.current!.createMediaStreamSource(stream);
          src.connect(a);
          return a;
        })() : null;

        if (analyserNode) {
          const data = new Uint8Array(analyserNode.frequencyBinCount);
          let silenceStart = 0;
          let fired = false;

          silenceTimerRef.current = window.setInterval(() => {
            if (fired) return;
            analyserNode.getByteFrequencyData(data);
            const avg = data.reduce((a, b) => a + b, 0) / data.length;

            if (avg < 5) {
              // Silence
              if (!silenceStart) silenceStart = Date.now();
              if (Date.now() - silenceStart >= silenceMs) {
                fired = true;
                onSilence();
              }
            } else {
              silenceStart = 0;
            }
          }, 100);
        }
      } catch (err) {
        setError(err instanceof Error ? err.message : "Microphone access denied");
        cleanup();
      }
    },
    [initStream, getMimeType, cleanup, startDurationTimer],
  );

  return { isRecording, start, stop, startChunked, startWithSilenceDetection, analyser, duration, error };
}
