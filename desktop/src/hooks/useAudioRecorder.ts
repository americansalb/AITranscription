import { useState, useRef, useCallback } from "react";

export interface AudioRecorderState {
  isRecording: boolean;
  isPaused: boolean;
  duration: number;
  error: string | null;
  audioLevel: number; // 0-1 normalized audio level
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
    audioLevel: 0,
  });

  const mediaRecorder = useRef<MediaRecorder | null>(null);
  const audioChunks = useRef<Blob[]>([]);
  const startTime = useRef<number>(0);
  const durationInterval = useRef<number | null>(null);
  const audioContext = useRef<AudioContext | null>(null);
  const analyser = useRef<AnalyserNode | null>(null);
  const audioLevelInterval = useRef<number | null>(null);

  const clearDurationInterval = useCallback(() => {
    if (durationInterval.current) {
      clearInterval(durationInterval.current);
      durationInterval.current = null;
    }
  }, []);

  const clearAudioLevelInterval = useCallback(() => {
    if (audioLevelInterval.current) {
      clearInterval(audioLevelInterval.current);
      audioLevelInterval.current = null;
    }
  }, []);

  const cleanupAudioContext = useCallback(() => {
    clearAudioLevelInterval();
    if (audioContext.current) {
      audioContext.current.close();
      audioContext.current = null;
    }
    analyser.current = null;
    setState((s) => ({ ...s, audioLevel: 0 }));
  }, [clearAudioLevelInterval]);

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

      // Set up audio level analyzer
      audioContext.current = new AudioContext();
      analyser.current = audioContext.current.createAnalyser();
      analyser.current.fftSize = 256;
      const source = audioContext.current.createMediaStreamSource(stream);
      source.connect(analyser.current);

      // Start monitoring audio level
      const dataArray = new Uint8Array(analyser.current.frequencyBinCount);
      audioLevelInterval.current = window.setInterval(() => {
        if (analyser.current) {
          analyser.current.getByteFrequencyData(dataArray);
          // Calculate average level (0-255) and normalize to 0-1
          const average = dataArray.reduce((a, b) => a + b, 0) / dataArray.length;
          const normalized = Math.min(1, average / 128); // Normalize with some headroom
          setState((s) => ({ ...s, audioLevel: normalized }));
        }
      }, 50); // Update 20 times per second for smooth animation

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
      cleanupAudioContext();

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
  }, [clearDurationInterval, cleanupAudioContext]);

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
    cleanupAudioContext();

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
      audioLevel: 0,
    });
  }, [clearDurationInterval, cleanupAudioContext]);

  return {
    ...state,
    startRecording,
    stopRecording,
    pauseRecording,
    resumeRecording,
    cancelRecording,
  };
}
