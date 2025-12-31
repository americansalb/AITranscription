import { useState, useRef, useCallback } from "react";

export interface AudioRecorderState {
  isRecording: boolean;
  isPaused: boolean;
  duration: number;
  error: string | null;
}

export interface UseAudioRecorderReturn extends AudioRecorderState {
  startRecording: () => Promise<void>;
  stopRecording: () => Promise<Blob | null>;
  pauseRecording: () => void;
  resumeRecording: () => void;
  cancelRecording: () => void;
}

export function useAudioRecorder(): UseAudioRecorderReturn {
  const [state, setState] = useState<AudioRecorderState>({
    isRecording: false,
    isPaused: false,
    duration: 0,
    error: null,
  });

  const mediaRecorder = useRef<MediaRecorder | null>(null);
  const audioChunks = useRef<Blob[]>([]);
  const startTime = useRef<number>(0);
  const durationInterval = useRef<number | null>(null);

  const clearDurationInterval = useCallback(() => {
    if (durationInterval.current) {
      clearInterval(durationInterval.current);
      durationInterval.current = null;
    }
  }, []);

  const startRecording = useCallback(async () => {
    try {
      setState((s) => ({ ...s, error: null }));

      // Request microphone access
      const stream = await navigator.mediaDevices.getUserMedia({
        audio: {
          echoCancellation: true,
          noiseSuppression: true,
          sampleRate: 44100,
        },
      });

      // Determine best supported format
      const mimeType = MediaRecorder.isTypeSupported("audio/webm;codecs=opus")
        ? "audio/webm;codecs=opus"
        : MediaRecorder.isTypeSupported("audio/webm")
          ? "audio/webm"
          : "audio/mp4";

      mediaRecorder.current = new MediaRecorder(stream, { mimeType });
      audioChunks.current = [];
      startTime.current = Date.now();

      mediaRecorder.current.ondataavailable = (event) => {
        if (event.data.size > 0) {
          audioChunks.current.push(event.data);
        }
      };

      mediaRecorder.current.start(100); // Collect data every 100ms

      // Update duration every 100ms
      durationInterval.current = window.setInterval(() => {
        setState((s) => ({
          ...s,
          duration: Math.floor((Date.now() - startTime.current) / 1000),
        }));
      }, 100);

      setState((s) => ({
        ...s,
        isRecording: true,
        isPaused: false,
        duration: 0,
      }));
    } catch (err) {
      const message =
        err instanceof Error ? err.message : "Failed to start recording";
      setState((s) => ({ ...s, error: message }));
      throw err;
    }
  }, []);

  const stopRecording = useCallback(async (): Promise<Blob | null> => {
    return new Promise((resolve) => {
      if (!mediaRecorder.current || mediaRecorder.current.state === "inactive") {
        resolve(null);
        return;
      }

      clearDurationInterval();

      mediaRecorder.current.onstop = () => {
        const mimeType = mediaRecorder.current?.mimeType || "audio/webm";
        const audioBlob = new Blob(audioChunks.current, { type: mimeType });

        // Stop all tracks
        mediaRecorder.current?.stream.getTracks().forEach((track) => track.stop());

        setState((s) => ({
          ...s,
          isRecording: false,
          isPaused: false,
        }));

        resolve(audioBlob);
      };

      mediaRecorder.current.stop();
    });
  }, [clearDurationInterval]);

  const pauseRecording = useCallback(() => {
    if (mediaRecorder.current && mediaRecorder.current.state === "recording") {
      mediaRecorder.current.pause();
      clearDurationInterval();
      setState((s) => ({ ...s, isPaused: true }));
    }
  }, [clearDurationInterval]);

  const resumeRecording = useCallback(() => {
    if (mediaRecorder.current && mediaRecorder.current.state === "paused") {
      mediaRecorder.current.resume();

      // Resume duration counting
      const pausedDuration = state.duration;
      startTime.current = Date.now() - pausedDuration * 1000;
      durationInterval.current = window.setInterval(() => {
        setState((s) => ({
          ...s,
          duration: Math.floor((Date.now() - startTime.current) / 1000),
        }));
      }, 100);

      setState((s) => ({ ...s, isPaused: false }));
    }
  }, [state.duration]);

  const cancelRecording = useCallback(() => {
    clearDurationInterval();

    if (mediaRecorder.current) {
      mediaRecorder.current.stream.getTracks().forEach((track) => track.stop());
      mediaRecorder.current = null;
    }

    audioChunks.current = [];
    setState({
      isRecording: false,
      isPaused: false,
      duration: 0,
      error: null,
    });
  }, [clearDurationInterval]);

  return {
    ...state,
    startRecording,
    stopRecording,
    pauseRecording,
    resumeRecording,
    cancelRecording,
  };
}
