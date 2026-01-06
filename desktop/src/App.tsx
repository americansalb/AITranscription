import { useState, useCallback, useEffect, useRef } from "react";
import { useAudioRecorder } from "./hooks/useAudioRecorder";
import { useGlobalHotkey } from "./hooks/useGlobalHotkey";
import { transcribeAndPolish, checkHealth, ApiError } from "./lib/api";
import { injectText, setTrayRecordingState, showRecordingOverlay, hideRecordingOverlay, updateOverlayState } from "./lib/clipboard";
import { playStartSound, playStopSound, playSuccessSound, playErrorSound } from "./lib/sounds";
import { Settings, getStoredHotkey, getStoredWhisperModel, getStoredNoiseCancellation } from "./components/Settings";
import { RecordingOverlay } from "./components/RecordingOverlay";
import { AudioVisualizer } from "./components/AudioVisualizer";
import { HistoryPanel } from "./components/HistoryPanel";
import { voiceStream, getStoredVoiceEnabled } from "./lib/voiceStream";
import { initSpeakListener, stop as stopSpeaking } from "./lib/speak";

type ProcessingStatus = "idle" | "recording" | "processing" | "success" | "error";

// History entry interface
export interface HistoryEntry {
  id: string;
  timestamp: Date;
  rawText: string;
  polishedText: string;
  context: string;
  formality: string;
  duration: number | null;
}

// Load history from localStorage
function loadHistory(): HistoryEntry[] {
  try {
    const stored = localStorage.getItem("scribe_history");
    if (stored) {
      const parsed = JSON.parse(stored);
      return parsed.map((entry: HistoryEntry) => ({
        ...entry,
        timestamp: new Date(entry.timestamp),
      }));
    }
  } catch {
    // Ignore parse errors
  }
  return [];
}

// Save history to localStorage
function saveHistory(history: HistoryEntry[]) {
  try {
    // Keep only last 100 entries to avoid storage bloat
    const toSave = history.slice(0, 100);
    localStorage.setItem("scribe_history", JSON.stringify(toSave));
  } catch {
    // Ignore storage errors
  }
}

function formatDuration(seconds: number): string {
  const mins = Math.floor(seconds / 60);
  const secs = seconds % 60;
  return `${mins}:${secs.toString().padStart(2, "0")}`;
}

// Format hotkey for display (handle platform differences)
function formatHotkeyDisplay(hotkey: string): string {
  const isMac = navigator.platform.includes("Mac");
  return hotkey
    .replace("Alt+", isMac ? "Option+" : "Alt+")
    .replace("CommandOrControl+", isMac ? "Cmd+" : "Ctrl+");
}

function App() {
  const recorder = useAudioRecorder();
  const [status, setStatus] = useState<ProcessingStatus>("idle");
  const [result, setResult] = useState<string>("");
  const [rawText, setRawText] = useState<string>("");
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const [context, setContext] = useState<string>("general");
  const [formality, setFormality] = useState<"casual" | "neutral" | "formal">("neutral");
  const [showRaw, setShowRaw] = useState(false);
  const [backendReady, setBackendReady] = useState<boolean | null>(null);
  const [showSettings, setShowSettings] = useState(false);
  const [statsRefreshTrigger, setStatsRefreshTrigger] = useState(0);
  const [history, setHistory] = useState<HistoryEntry[]>(() => loadHistory());
  const [hotkey, setHotkey] = useState(() => getStoredHotkey());
  const [whisperModel, setWhisperModel] = useState(() => getStoredWhisperModel());
  // eslint-disable-next-line @typescript-eslint/no-unused-vars
  const [_noiseCancellation, setNoiseCancellation] = useState(() => getStoredNoiseCancellation());
  const [voiceEnabled, setVoiceEnabled] = useState(() => getStoredVoiceEnabled());

  // Refs for push-to-talk state management
  const isProcessingRef = useRef(false);
  const contextRef = useRef(context);
  const formalityRef = useRef(formality);
  const whisperModelRef = useRef(whisperModel);

  // Keep refs in sync with state
  useEffect(() => {
    contextRef.current = context;
    formalityRef.current = formality;
    whisperModelRef.current = whisperModel;
  }, [context, formality, whisperModel]);

  // Persist history to localStorage when it changes
  useEffect(() => {
    saveHistory(history);
  }, [history]);

  // Add entry to history
  const addToHistory = useCallback((entry: Omit<HistoryEntry, "id">) => {
    const newEntry: HistoryEntry = {
      ...entry,
      id: `${Date.now()}-${Math.random().toString(36).substr(2, 9)}`,
    };
    setHistory((prev) => [newEntry, ...prev]);
  }, []);

  // Update tray icon when recording state changes
  useEffect(() => {
    setTrayRecordingState(recorder.isRecording);
  }, [recorder.isRecording]);

  // Show/hide floating overlay when recording starts/stops
  useEffect(() => {
    if (recorder.isRecording || status === "processing") {
      showRecordingOverlay();
    } else {
      hideRecordingOverlay();
    }
  }, [recorder.isRecording, status]);

  // Update overlay with audio level and duration
  useEffect(() => {
    if (recorder.isRecording || status === "processing") {
      updateOverlayState({
        isRecording: recorder.isRecording,
        isProcessing: status === "processing",
        duration: recorder.duration,
        audioLevel: recorder.audioLevel || 0,
      });
    }
  }, [recorder.isRecording, recorder.duration, recorder.audioLevel, status]);

  // Push-to-talk: start recording on key down
  const handleHotkeyDown = useCallback(async () => {
    if (recorder.isRecording || isProcessingRef.current || backendReady === false) return;

    // Stop any playing voice/speech so it doesn't interfere with recording
    voiceStream.stopAudio();
    stopSpeaking();

    setError(null);
    setResult("");
    setRawText("");

    try {
      await recorder.startRecording();
      setStatus("recording");
      playStartSound();
    } catch (err) {
      const message = err instanceof Error ? err.message : "Failed to start recording";
      setError(message);
      setStatus("error");
      playErrorSound();
    }
  }, [recorder, backendReady]);

  // Push-to-talk: stop recording and process on key up
  const handleHotkeyUp = useCallback(async () => {
    if (!recorder.isRecording || isProcessingRef.current) return;

    isProcessingRef.current = true;
    setStatus("processing");
    setError(null);
    playStopSound();

    try {
      const audioBlob = await recorder.stopRecording();
      if (!audioBlob) {
        throw new Error("No audio recorded");
      }

      const response = await transcribeAndPolish(audioBlob, {
        language: "en",  // Default to English to avoid wrong language detection
        context: contextRef.current === "general" ? undefined : contextRef.current,
        formality: formalityRef.current,
        model: whisperModelRef.current as "whisper-large-v3" | "whisper-large-v3-turbo",
      });

      setRawText(response.raw_text);
      setResult(response.polished_text);
      setStatus("success");
      playSuccessSound();

      // Add to history
      addToHistory({
        timestamp: new Date(),
        rawText: response.raw_text,
        polishedText: response.polished_text,
        context: contextRef.current,
        formality: formalityRef.current,
        duration: response.duration,
      });

      // Trigger stats refresh so dashboard updates in real-time
      setStatsRefreshTrigger((prev) => prev + 1);

      // Auto-inject the polished text into the active application
      if (response.polished_text) {
        await injectText(response.polished_text);
      }
    } catch (err) {
      const message =
        err instanceof ApiError
          ? err.detail || err.message
          : err instanceof Error
            ? err.message
            : "An error occurred";
      setError(message);
      setStatus("error");
      playErrorSound();
    } finally {
      isProcessingRef.current = false;
    }
  }, [recorder, addToHistory]);

  // Register global hotkey for push-to-talk
  const { error: hotkeyError } = useGlobalHotkey({
    hotkey,
    onKeyDown: handleHotkeyDown,
    onKeyUp: handleHotkeyUp,
    enabled: backendReady !== false,
  });

  // Check backend health on mount
  useEffect(() => {
    checkHealth()
      .then((health) => {
        setBackendReady(health.groq_configured && health.anthropic_configured);
        if (!health.groq_configured) {
          setError("Backend: Groq API key not configured");
        } else if (!health.anthropic_configured) {
          setError("Backend: Anthropic API key not configured");
        }
      })
      .catch(() => {
        setBackendReady(false);
        setError("Cannot connect to backend. Is it running?");
      });
  }, []);

  // Initialize speak listener for Claude Code integration
  useEffect(() => {
    let unlisten: (() => void) | undefined;

    initSpeakListener().then((unlistenFn) => {
      unlisten = unlistenFn;
    });

    return () => {
      if (unlisten) unlisten();
      stopSpeaking();
    };
  }, []);

  // Connect/disconnect voice stream based on voiceEnabled setting
  useEffect(() => {
    const apiUrl = import.meta.env.VITE_API_URL || 'https://scribe-api-yk09.onrender.com';

    if (voiceEnabled) {
      voiceStream.connect(apiUrl);

      // Log voice events for debugging
      const unsubscribe = voiceStream.onEvent((event) => {
        if (event.type === 'voice' && event.explanation) {
          console.log('[Voice]', event.explanation);
        } else if (event.type === 'error') {
          console.error('[Voice Error]', event.explanation);
        }
      });

      return () => {
        unsubscribe();
        voiceStream.disconnect();
      };
    } else {
      voiceStream.disconnect();
    }
  }, [voiceEnabled]);

  // Handler for voice enabled toggle
  const handleVoiceEnabledChange = useCallback((enabled: boolean) => {
    setVoiceEnabled(enabled);
  }, []);

  const handleRecordClick = useCallback(async () => {
    if (recorder.isRecording) {
      // Stop recording and process
      setStatus("processing");
      setError(null);
      playStopSound();

      try {
        const audioBlob = await recorder.stopRecording();
        if (!audioBlob) {
          throw new Error("No audio recorded");
        }

        const response = await transcribeAndPolish(audioBlob, {
          language: "en",  // Default to English
          context: context === "general" ? undefined : context,
          formality,
          model: whisperModel as "whisper-large-v3" | "whisper-large-v3-turbo",
        });

        setRawText(response.raw_text);
        setResult(response.polished_text);
        setStatus("success");
        playSuccessSound();

        // Add to history
        addToHistory({
          timestamp: new Date(),
          rawText: response.raw_text,
          polishedText: response.polished_text,
          context,
          formality,
          duration: response.duration,
        });

        // Trigger stats refresh so dashboard updates in real-time
        setStatsRefreshTrigger((prev) => prev + 1);
      } catch (err) {
        const message =
          err instanceof ApiError
            ? err.detail || err.message
            : err instanceof Error
              ? err.message
              : "An error occurred";
        setError(message);
        setStatus("error");
        playErrorSound();
      }
    } else {
      // Start recording
      // Stop any playing voice/speech so it doesn't interfere
      voiceStream.stopAudio();
      stopSpeaking();

      setError(null);
      setResult("");
      setRawText("");

      try {
        await recorder.startRecording();
        setStatus("recording");
        playStartSound();
      } catch (err) {
        const message =
          err instanceof Error ? err.message : "Failed to start recording";
        setError(message);
        setStatus("error");
        playErrorSound();
      }
    }
  }, [recorder, context, formality, addToHistory]);

  const handleCopy = useCallback(async () => {
    if (result) {
      await navigator.clipboard.writeText(result);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    }
  }, [result]);

  const getStatusText = () => {
    switch (status) {
      case "recording":
        return `Recording... ${formatDuration(recorder.duration)}`;
      case "processing":
        return "Processing...";
      case "success":
        return "Ready";
      case "error":
        return "Error";
      default:
        return "Ready to record";
    }
  };

  return (
    <div className="app">
      <header className="header">
        <h1>Scribe</h1>
        <button className="settings-btn" title="Settings" onClick={() => setShowSettings(true)}>
          <svg
            width="20"
            height="20"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2"
            strokeLinecap="round"
            strokeLinejoin="round"
          >
            <circle cx="12" cy="12" r="3" />
            <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-2 2 2 2 0 0 1-2-2v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1-2-2 2 2 0 0 1 2-2h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 2-2 2 2 0 0 1 2 2v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 2 2 2 2 0 0 1-2 2h-.09a1.65 1.65 0 0 0-1.51 1z" />
          </svg>
        </button>
      </header>

      {error && <div className="error-message">{error}</div>}
      {hotkeyError && <div className="error-message">{hotkeyError}</div>}

      <div className="recording-section">
        <button
          className={`record-btn ${recorder.isRecording ? "recording" : ""}`}
          onClick={handleRecordClick}
          disabled={status === "processing" || backendReady === false}
        >
          {status === "processing" ? (
            <div className="spinner" />
          ) : recorder.isRecording ? (
            <svg viewBox="0 0 24 24" fill="currentColor">
              <rect x="6" y="6" width="12" height="12" rx="2" />
            </svg>
          ) : (
            <svg viewBox="0 0 24 24" fill="currentColor">
              <circle cx="12" cy="12" r="6" />
            </svg>
          )}
        </button>

        <div className="status">
          <span
            className={`status-dot ${status === "recording" ? "recording" : status === "processing" ? "processing" : status === "success" ? "success" : ""}`}
          />
          {getStatusText()}
        </div>

        {/* Audio level visualizer */}
        <AudioVisualizer
          isRecording={recorder.isRecording}
          audioLevel={recorder.audioLevel || 0}
        />

        <p className="record-hint">
          Click to {recorder.isRecording ? "stop" : "start"} • Hold{" "}
          <span className="hotkey">{formatHotkeyDisplay(hotkey)}</span> for push-to-talk
        </p>
      </div>

      <div className="context-selector">
        <label>Context:</label>
        <select value={context} onChange={(e) => setContext(e.target.value)}>
          <option value="general">General</option>
          <option value="email">Email</option>
          <option value="slack">Slack / Chat</option>
          <option value="document">Document</option>
          <option value="code">Code Comment</option>
        </select>

        <label>Tone:</label>
        <select
          value={formality}
          onChange={(e) =>
            setFormality(e.target.value as "casual" | "neutral" | "formal")
          }
        >
          <option value="casual">Casual</option>
          <option value="neutral">Neutral</option>
          <option value="formal">Formal</option>
        </select>
      </div>

      <div className="result-section">
        <div className="result-header">
          <h2>
            {showRaw ? "Raw Transcription" : "Polished Text"}
            {rawText && (
              <button
                onClick={() => setShowRaw(!showRaw)}
                style={{
                  marginLeft: 8,
                  background: "none",
                  border: "none",
                  color: "var(--text-muted)",
                  cursor: "pointer",
                  fontSize: 12,
                  textDecoration: "underline",
                }}
              >
                Show {showRaw ? "polished" : "raw"}
              </button>
            )}
          </h2>
          {result && (
            <button
              className={`copy-btn ${copied ? "copied" : ""}`}
              onClick={handleCopy}
            >
              {copied ? (
                <>
                  <svg
                    width="14"
                    height="14"
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    strokeWidth="2"
                  >
                    <polyline points="20 6 9 17 4 12" />
                  </svg>
                  Copied
                </>
              ) : (
                <>
                  <svg
                    width="14"
                    height="14"
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    strokeWidth="2"
                  >
                    <rect x="9" y="9" width="13" height="13" rx="2" ry="2" />
                    <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" />
                  </svg>
                  Copy
                </>
              )}
            </button>
          )}
        </div>
        <div className={`result-text ${!result && !rawText ? "empty" : ""}`}>
          {showRaw ? rawText : result || "Your transcription will appear here..."}
        </div>
      </div>

      {/* History panel with click-to-copy and multi-select */}
      <HistoryPanel history={history} onClear={() => setHistory([])} />

      {showSettings && (
        <Settings
          onClose={() => setShowSettings(false)}
          refreshTrigger={statsRefreshTrigger}
          history={history}
          onClearHistory={() => setHistory([])}
          onHotkeyChange={setHotkey}
          onModelChange={setWhisperModel}
          onNoiseCancellationChange={setNoiseCancellation}
          onVoiceEnabledChange={handleVoiceEnabledChange}
        />
      )}

      {/* Floating recording indicator - visible even when window is minimized */}
      <RecordingOverlay
        isRecording={recorder.isRecording}
        isProcessing={status === "processing"}
        duration={recorder.duration}
      />
    </div>
  );
}

export default App;
