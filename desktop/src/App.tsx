import { useState, useCallback, useEffect, useRef } from "react";
import { useAudioRecorder } from "./hooks/useAudioRecorder";
import { useGlobalHotkey, HOTKEYS } from "./hooks/useGlobalHotkey";
import { transcribeAndPolish, checkHealth, ApiError } from "./lib/api";
import { injectText, setTrayRecordingState } from "./lib/clipboard";
import { Settings } from "./components/Settings";

type ProcessingStatus = "idle" | "recording" | "processing" | "success" | "error";

function formatDuration(seconds: number): string {
  const mins = Math.floor(seconds / 60);
  const secs = seconds % 60;
  return `${mins}:${secs.toString().padStart(2, "0")}`;
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

  // Refs for push-to-talk state management
  const isProcessingRef = useRef(false);
  const contextRef = useRef(context);
  const formalityRef = useRef(formality);

  // Keep refs in sync with state
  useEffect(() => {
    contextRef.current = context;
    formalityRef.current = formality;
  }, [context, formality]);

  // Update tray icon when recording state changes
  useEffect(() => {
    setTrayRecordingState(recorder.isRecording);
  }, [recorder.isRecording]);

  // Push-to-talk: start recording on key down
  const handleHotkeyDown = useCallback(async () => {
    if (recorder.isRecording || isProcessingRef.current || backendReady === false) return;

    setError(null);
    setResult("");
    setRawText("");

    try {
      await recorder.startRecording();
      setStatus("recording");
    } catch (err) {
      const message = err instanceof Error ? err.message : "Failed to start recording";
      setError(message);
      setStatus("error");
    }
  }, [recorder, backendReady]);

  // Push-to-talk: stop recording and process on key up
  const handleHotkeyUp = useCallback(async () => {
    if (!recorder.isRecording || isProcessingRef.current) return;

    isProcessingRef.current = true;
    setStatus("processing");
    setError(null);

    try {
      const audioBlob = await recorder.stopRecording();
      if (!audioBlob) {
        throw new Error("No audio recorded");
      }

      const response = await transcribeAndPolish(audioBlob, {
        context: contextRef.current === "general" ? undefined : contextRef.current,
        formality: formalityRef.current,
      });

      setRawText(response.raw_text);
      setResult(response.polished_text);
      setStatus("success");

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
    } finally {
      isProcessingRef.current = false;
    }
  }, [recorder]);

  // Register global hotkey for push-to-talk
  const { error: hotkeyError } = useGlobalHotkey({
    hotkey: HOTKEYS.PUSH_TO_TALK,
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

  const handleRecordClick = useCallback(async () => {
    if (recorder.isRecording) {
      // Stop recording and process
      setStatus("processing");
      setError(null);

      try {
        const audioBlob = await recorder.stopRecording();
        if (!audioBlob) {
          throw new Error("No audio recorded");
        }

        const response = await transcribeAndPolish(audioBlob, {
          context: context === "general" ? undefined : context,
          formality,
        });

        setRawText(response.raw_text);
        setResult(response.polished_text);
        setStatus("success");
      } catch (err) {
        const message =
          err instanceof ApiError
            ? err.detail || err.message
            : err instanceof Error
              ? err.message
              : "An error occurred";
        setError(message);
        setStatus("error");
      }
    } else {
      // Start recording
      setError(null);
      setResult("");
      setRawText("");

      try {
        await recorder.startRecording();
        setStatus("recording");
      } catch (err) {
        const message =
          err instanceof Error ? err.message : "Failed to start recording";
        setError(message);
        setStatus("error");
      }
    }
  }, [recorder, context, formality]);

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

        <p className="record-hint">
          Click to {recorder.isRecording ? "stop" : "start"} • Hold{" "}
          <span className="hotkey">{navigator.platform.includes("Mac") ? "Cmd" : "Ctrl"}+Shift+A</span> for push-to-talk
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

      {showSettings && <Settings onClose={() => setShowSettings(false)} />}
    </div>
  );
}

export default App;
