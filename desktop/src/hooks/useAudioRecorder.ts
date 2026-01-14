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

      // Check if mediaDevices API is available
      if (!navigator.mediaDevices || !navigator.mediaDevices.getUserMedia) {
        throw new Error(
          "Microphone access not available. Please grant microphone permission in System Settings > Privacy & Security > Microphone."
        );
      }

      // Detect platform for appropriate constraints
      // macOS WebKit ignores some constraints, so we use simpler ones there
      const isMac = navigator.platform.includes("Mac");

      // Request microphone access with platform-appropriate constraints
      // Safari/WebKit ignores sampleRate, so don't set it on Mac
      const audioConstraints: MediaTrackConstraints = isMac
        ? {
            echoCancellation: true,
            noiseSuppression: true,
            // Don't specify sampleRate on Mac - WebKit ignores it and may cause issues
          }
        : {
            echoCancellation: true,
            noiseSuppression: true,
            sampleRate: 44100,
          };

      const stream = await navigator.mediaDevices.getUserMedia({
        audio: audioConstraints,
      });

      const audioTracks = stream.getAudioTracks();
      console.log("[AudioRecorder] Platform:", isMac ? "macOS" : "other");
      console.log("[AudioRecorder] Got stream with", audioTracks.length, "audio tracks");
      if (audioTracks.length > 0) {
        const settings = audioTracks[0].getSettings();
        console.log("[AudioRecorder] Track:", audioTracks[0].label, "enabled:", audioTracks[0].enabled, "muted:", audioTracks[0].muted);
        console.log("[AudioRecorder] Actual settings:", JSON.stringify(settings));
      }

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

      // Determine best supported format based on platform
      // macOS WebKit does NOT support WebM - must use MP4
      // Windows/Linux Chromium supports WebM (preferred for quality)
      let mimeType: string;

      // Check what's actually supported and log it
      const webmOpus = MediaRecorder.isTypeSupported("audio/webm;codecs=opus");
      const webm = MediaRecorder.isTypeSupported("audio/webm");
      const mp4 = MediaRecorder.isTypeSupported("audio/mp4");
      const aac = MediaRecorder.isTypeSupported("audio/aac");

      console.log("[AudioRecorder] Codec support - webm;opus:", webmOpus, "webm:", webm, "mp4:", mp4, "aac:", aac);

      if (webmOpus) {
        // Chromium-based (Windows WebView2, Linux WebKitGTK with GStreamer)
        mimeType = "audio/webm;codecs=opus";
      } else if (mp4) {
        // macOS WebKit - use MP4 (with AAC codec)
        mimeType = "audio/mp4";
      } else if (webm) {
        // Fallback to plain WebM
        mimeType = "audio/webm";
      } else {
        // Last resort - let browser pick
        mimeType = "";
        console.warn("[AudioRecorder] No preferred codec supported, using browser default");
      }

      console.log("[AudioRecorder] Using mimeType:", mimeType || "(browser default)");

      const recorderOptions: MediaRecorderOptions = mimeType ? { mimeType } : {};
      mediaRecorder.current = new MediaRecorder(stream, recorderOptions);
      audioChunks.current = [];
      startTime.current = Date.now();

      mediaRecorder.current.ondataavailable = (event) => {
        if (event.data.size > 0) {
          audioChunks.current.push(event.data);
        }
      };

      // Use longer timeslice on Mac for more reliable recording
      const timeslice = isMac ? 250 : 100;
      mediaRecorder.current.start(timeslice);
      console.log("[AudioRecorder] Started with timeslice:", timeslice, "ms");

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
        // Use the actual mimeType from the recorder, fallback to mp4 (works on both platforms)
        const mimeType = mediaRecorder.current?.mimeType || "audio/mp4";
        const audioBlob = new Blob(audioChunks.current, { type: mimeType });
        console.log("[AudioRecorder] Final blob mimeType:", mimeType);

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
