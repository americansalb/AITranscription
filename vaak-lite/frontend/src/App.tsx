import { useState, useCallback, useRef, useEffect } from "react";
import { SettingsPanel, DEFAULT_SETTINGS, type InterpretationSettings } from "./components/SettingsPanel";
import { InterpretationView, type InterpretationEntry } from "./components/InterpretationView";
import { RecordButton } from "./components/RecordButton";
import { AudioVisualizer } from "./components/AudioVisualizer";
import { useAudioRecorder } from "./hooks/useAudioRecorder";
import { interpret, getProviders, type ProviderInfo } from "./lib/api";

export default function App() {
  const [settings, setSettings] = useState<InterpretationSettings>(DEFAULT_SETTINGS);
  const [entries, setEntries] = useState<InterpretationEntry[]>([]);
  const [providers, setProviders] = useState<ProviderInfo[]>([]);
  const [processing, setProcessing] = useState(false);
  const [statusMsg, setStatusMsg] = useState<string | null>(null);
  const [showSettings, setShowSettings] = useState(true);

  const nextId = useRef(1);
  const speakerRef = useRef<"A" | "B">("A");
  const recorder = useAudioRecorder();

  // Fetch available providers on mount
  useEffect(() => {
    getProviders().then((p) => {
      setProviders(p);
      // Default to first available provider
      if (p.length > 0 && !p.find((x) => x.id === settings.provider)) {
        setSettings((s) => ({ ...s, provider: p[0].id }));
      }
    });
  }, []);

  const showStatus = useCallback((msg: string, ms = 3000) => {
    setStatusMsg(msg);
    setTimeout(() => setStatusMsg(null), ms);
  }, []);

  // ── Process a recorded blob through the interpretation pipeline ──

  const processBlob = useCallback(
    async (blob: Blob, speaker?: "A" | "B") => {
      const entryId = nextId.current++;
      const pendingEntry: InterpretationEntry = {
        id: entryId,
        sourceText: "",
        translatedText: "",
        sourceLang: settings.sourceLang,
        targetLang: settings.targetLang,
        duration: null,
        provider: settings.provider,
        timestamp: new Date(),
        speaker,
        pending: true,
      };

      setEntries((prev) => [...prev, pendingEntry]);
      setProcessing(true);

      try {
        const result = await interpret(blob, settings.targetLang, settings.provider, settings.sourceLang);

        if (!result.source_text.trim()) {
          setEntries((prev) => prev.filter((e) => e.id !== entryId));
          showStatus("No speech detected");
          return;
        }

        setEntries((prev) =>
          prev.map((e) =>
            e.id === entryId
              ? {
                  ...e,
                  sourceText: result.source_text,
                  translatedText: result.translated_text,
                  sourceLang: result.source_lang,
                  targetLang: result.target_lang,
                  duration: result.duration,
                  provider: result.provider,
                  pending: false,
                }
              : e,
          ),
        );
      } catch (err) {
        setEntries((prev) => prev.filter((e) => e.id !== entryId));
        showStatus(err instanceof Error ? err.message : "Interpretation failed");
      } finally {
        setProcessing(false);
      }
    },
    [settings, showStatus],
  );

  // ── Recording handlers per mode combination ──────────────

  const handleRecord = useCallback(async () => {
    if (recorder.isRecording) {
      // Stop recording
      try {
        const blob = await recorder.stop();
        const speaker = settings.direction === "bidirectional" ? speakerRef.current : undefined;
        await processBlob(blob, speaker);

        // Toggle speaker for bidirectional
        if (settings.direction === "bidirectional") {
          speakerRef.current = speakerRef.current === "A" ? "B" : "A";
        }
      } catch {
        // recorder.stop() can throw if already stopped (e.g. auto-consecutive)
      }
      return;
    }

    // Start recording based on mode
    if (settings.timing === "consecutive" && settings.trigger === "auto") {
      // Auto-consecutive: start with silence detection
      await recorder.startWithSilenceDetection(
        settings.silenceThreshold * 1000,
        async () => {
          // Silence detected — stop and process
          try {
            const blob = await recorder.stop();
            const speaker = settings.direction === "bidirectional" ? speakerRef.current : undefined;
            await processBlob(blob, speaker);

            if (settings.direction === "bidirectional") {
              speakerRef.current = speakerRef.current === "A" ? "B" : "A";
            }
          } catch {
            // ignore
          }
        },
      );
    } else if (settings.timing === "simultaneous") {
      // Simultaneous: chunk every 5 seconds, process each chunk
      await recorder.startChunked(5000, async (chunk, _seq) => {
        const speaker = settings.direction === "bidirectional" ? speakerRef.current : undefined;
        // Process in background — don't await to keep recording flowing
        processBlob(chunk, speaker);
      });
    } else {
      // Manual consecutive: simple start, user clicks stop
      await recorder.start();
    }
  }, [recorder, settings, processBlob]);

  // ── UI hints ──────────────────────────────────────────────

  const recordHint = (() => {
    if (processing && !recorder.isRecording) return "Processing...";
    if (recorder.isRecording) {
      if (settings.timing === "consecutive" && settings.trigger === "auto") {
        return `Listening... auto-stops after ${settings.silenceThreshold}s silence`;
      }
      if (settings.timing === "simultaneous") return "Live interpreting...";
      return "Tap to stop and interpret";
    }
    if (settings.direction === "bidirectional") {
      return `Tap to record Speaker ${speakerRef.current}`;
    }
    return "Tap to start";
  })();

  const formatDuration = (sec: number) => {
    const m = Math.floor(sec / 60);
    const s = Math.floor(sec % 60);
    return `${m.toString().padStart(2, "0")}:${s.toString().padStart(2, "0")}`;
  };

  // ── Render ────────────────────────────────────────────────

  return (
    <div className="app">
      <header className="app-header">
        <h1>Vaak Lite</h1>
        <button
          className="settings-toggle"
          onClick={() => setShowSettings((v) => !v)}
          aria-label={showSettings ? "Hide settings" : "Show settings"}
        >
          {showSettings ? "Hide Settings" : "Settings"}
        </button>
      </header>

      {showSettings && (
        <SettingsPanel
          settings={settings}
          onChange={setSettings}
          availableProviders={providers}
          disabled={recorder.isRecording}
        />
      )}

      <InterpretationView
        entries={entries}
        bidirectional={settings.direction === "bidirectional"}
        isRecording={recorder.isRecording}
      />

      <div className="controls">
        <AudioVisualizer analyser={recorder.analyser} isRecording={recorder.isRecording} />
        <RecordButton
          isRecording={recorder.isRecording}
          onClick={handleRecord}
          disabled={processing && !recorder.isRecording}
          hint={recordHint}
        />
      </div>

      <footer className="app-footer">
        {recorder.isRecording && <span className="stat">{formatDuration(recorder.duration)}</span>}
        {entries.length > 0 && <span className="stat">{entries.length} segments</span>}
      </footer>

      {recorder.error && <div className="error-toast">{recorder.error}</div>}
      {statusMsg && <div className="status-toast">{statusMsg}</div>}
    </div>
  );
}
