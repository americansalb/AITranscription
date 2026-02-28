import { useState, useCallback, useRef, useEffect } from "react";
import { SettingsPanel, DEFAULT_SETTINGS, type InterpretationSettings } from "./components/SettingsPanel";
import { InterpretationView, type InterpretationEntry, type EntryStatus } from "./components/InterpretationView";
import { RecordButton } from "./components/RecordButton";
import { AudioVisualizer } from "./components/AudioVisualizer";
import { useAudioRecorder } from "./hooks/useAudioRecorder";
import { interpret, transcribe, getProviders, type ProviderInfo } from "./lib/api";

export default function App() {
  const [settings, setSettings] = useState<InterpretationSettings>(DEFAULT_SETTINGS);
  const [entries, setEntries] = useState<InterpretationEntry[]>([]);
  const [providers, setProviders] = useState<ProviderInfo[]>([]);
  const [processing, setProcessing] = useState(false);
  const [statusMsg, setStatusMsg] = useState<string | null>(null);
  const [showSettings, setShowSettings] = useState(true);

  const nextId = useRef(1);
  const speakerRef = useRef<"A" | "B">("A");
  // Track the in-progress entry for simultaneous mode
  const simultaneousEntryId = useRef<number | null>(null);
  const lastSimultaneousText = useRef("");
  const recorder = useAudioRecorder();

  // Fetch available providers on mount
  useEffect(() => {
    getProviders().then((p) => {
      setProviders(p);
      if (p.length > 0 && !p.find((x) => x.id === settings.provider)) {
        setSettings((s) => ({ ...s, provider: p[0].id }));
      }
    });
  }, []);

  // Reset speaker when switching away from bidirectional
  useEffect(() => {
    if (settings.direction !== "bidirectional") {
      speakerRef.current = "A";
    }
  }, [settings.direction]);

  const showStatus = useCallback((msg: string, ms = 3000) => {
    setStatusMsg(msg);
    setTimeout(() => setStatusMsg(null), ms);
  }, []);

  // ── Process a blob (consecutive mode — one blob per segment) ──

  const processBlob = useCallback(
    async (blob: Blob, speaker?: "A" | "B", status: EntryStatus = "complete") => {
      const entryId = nextId.current++;
      const isTranslation = settings.mode === "interpret";
      const pendingEntry: InterpretationEntry = {
        id: entryId,
        sourceText: "",
        translatedText: "",
        sourceLang: settings.sourceLang,
        targetLang: isTranslation ? settings.targetLang : settings.sourceLang,
        duration: null,
        provider: isTranslation ? settings.provider : "",
        timestamp: new Date(),
        speaker,
        pending: true,
        status: "pending",
      };

      setEntries((prev) => [...prev, pendingEntry]);
      setProcessing(true);

      try {
        if (isTranslation) {
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
                    status,
                  }
                : e,
            ),
          );
        } else {
          const lang = settings.sourceLang !== "auto" ? settings.sourceLang : undefined;
          const result = await transcribe(blob, lang);

          if (!result.text.trim()) {
            setEntries((prev) => prev.filter((e) => e.id !== entryId));
            showStatus("No speech detected");
            return;
          }

          setEntries((prev) =>
            prev.map((e) =>
              e.id === entryId
                ? {
                    ...e,
                    sourceText: result.text,
                    translatedText: "",
                    sourceLang: result.language || settings.sourceLang,
                    duration: result.duration,
                    provider: "",
                    pending: false,
                    status,
                  }
                : e,
            ),
          );
        }
      } catch (err) {
        setEntries((prev) => prev.filter((e) => e.id !== entryId));
        showStatus(err instanceof Error ? err.message : "Processing failed");
      } finally {
        setProcessing(false);
      }
    },
    [settings, showStatus],
  );

  // ── Process a blob for simultaneous mode ────────────────
  // Updates the single in-progress entry instead of creating new ones.
  // Each chunk contains the full accumulated audio, so each Whisper result
  // supersedes the previous one. The text is "in_progress" until recording stops.

  const processSimultaneousChunk = useCallback(
    async (blob: Blob, seq: number, speaker?: "A" | "B") => {
      const isTranslation = settings.mode === "interpret";

      // Create in-progress entry on first chunk
      if (simultaneousEntryId.current === null) {
        const entryId = nextId.current++;
        simultaneousEntryId.current = entryId;
        lastSimultaneousText.current = "";
        const entry: InterpretationEntry = {
          id: entryId,
          sourceText: "",
          translatedText: "",
          sourceLang: settings.sourceLang,
          targetLang: isTranslation ? settings.targetLang : settings.sourceLang,
          duration: null,
          provider: isTranslation ? settings.provider : "",
          timestamp: new Date(),
          speaker,
          pending: true,
          status: "in_progress",
          seq: 0,
        };
        setEntries((prev) => [...prev, entry]);
      }

      const currentEntryId = simultaneousEntryId.current;

      try {
        if (isTranslation) {
          const result = await interpret(blob, settings.targetLang, settings.provider, settings.sourceLang);
          if (!result.source_text.trim()) return;

          // Only update if this result has more text (later chunk = more audio = more complete)
          if (result.source_text.length >= lastSimultaneousText.current.length) {
            lastSimultaneousText.current = result.source_text;
            setEntries((prev) =>
              prev.map((e) =>
                e.id === currentEntryId
                  ? {
                      ...e,
                      sourceText: result.source_text,
                      translatedText: result.translated_text,
                      sourceLang: result.source_lang,
                      targetLang: result.target_lang,
                      duration: result.duration,
                      provider: result.provider,
                      pending: false,
                      status: "in_progress",
                      seq,
                    }
                  : e,
              ),
            );
          }
        } else {
          const lang = settings.sourceLang !== "auto" ? settings.sourceLang : undefined;
          const result = await transcribe(blob, lang);
          if (!result.text.trim()) return;

          if (result.text.length >= lastSimultaneousText.current.length) {
            lastSimultaneousText.current = result.text;
            setEntries((prev) =>
              prev.map((e) =>
                e.id === currentEntryId
                  ? {
                      ...e,
                      sourceText: result.text,
                      translatedText: "",
                      sourceLang: result.language || settings.sourceLang,
                      duration: result.duration,
                      provider: "",
                      pending: false,
                      status: "in_progress",
                      seq,
                    }
                  : e,
              ),
            );
          }
        }
      } catch (err) {
        // Don't remove the entry on chunk failure — next chunk may succeed
        console.warn("Simultaneous chunk failed:", err);
      }
    },
    [settings],
  );

  // Finalize the in-progress simultaneous entry
  const finalizeSimultaneous = useCallback(() => {
    const entryId = simultaneousEntryId.current;
    if (entryId !== null) {
      setEntries((prev) =>
        prev.map((e) =>
          e.id === entryId ? { ...e, status: "complete" as EntryStatus } : e,
        ),
      );
      simultaneousEntryId.current = null;
      lastSimultaneousText.current = "";
    }
  }, []);

  // ── Recording handlers per mode combination ──────────────

  const useBidirectional = settings.direction === "bidirectional" && settings.mode === "interpret";

  const handleRecord = useCallback(async () => {
    if (recorder.isRecording) {
      try {
        const blob = await recorder.stop();

        if (settings.timing === "simultaneous") {
          // Send the final full blob for one last transcription, then finalize
          const speaker = useBidirectional ? speakerRef.current : undefined;
          await processSimultaneousChunk(blob, 999, speaker);
          finalizeSimultaneous();
        } else {
          const speaker = useBidirectional ? speakerRef.current : undefined;
          await processBlob(blob, speaker, "complete");
        }

        if (useBidirectional) {
          speakerRef.current = speakerRef.current === "A" ? "B" : "A";
        }
      } catch {
        // recorder.stop() can throw if already stopped
        finalizeSimultaneous();
      }
      return;
    }

    if (settings.timing === "consecutive" && settings.trigger === "auto") {
      await recorder.startWithSilenceDetection(
        settings.silenceThreshold * 1000,
        async () => {
          try {
            const blob = await recorder.stop();
            const speaker = useBidirectional ? speakerRef.current : undefined;
            await processBlob(blob, speaker, "complete");
            if (useBidirectional) {
              speakerRef.current = speakerRef.current === "A" ? "B" : "A";
            }
          } catch {
            // ignore
          }
        },
      );
    } else if (settings.timing === "simultaneous") {
      simultaneousEntryId.current = null;
      lastSimultaneousText.current = "";
      await recorder.startChunked(5000, async (chunk, seq) => {
        const speaker = useBidirectional ? speakerRef.current : undefined;
        processSimultaneousChunk(chunk, seq, speaker);
      });
    } else {
      // Manual consecutive
      await recorder.start();
    }
  }, [recorder, settings, processBlob, processSimultaneousChunk, finalizeSimultaneous, useBidirectional]);

  // ── UI hints ──────────────────────────────────────────────

  const modeLabel = settings.mode === "interpret" ? "interpret" : "transcribe";

  const recordHint = (() => {
    if (processing && !recorder.isRecording) return "Processing...";
    if (recorder.isRecording) {
      if (settings.timing === "consecutive" && settings.trigger === "auto") {
        return `Listening... auto-stops after ${settings.silenceThreshold}s silence`;
      }
      if (settings.timing === "simultaneous") {
        return settings.mode === "interpret" ? "Live interpreting..." : "Live transcribing...";
      }
      return `Tap to stop and ${modeLabel}`;
    }
    if (useBidirectional) {
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
        <div className="header-actions">
          {entries.length > 0 && (
            <button
              className="clear-btn"
              onClick={() => { setEntries([]); nextId.current = 1; speakerRef.current = "A"; simultaneousEntryId.current = null; }}
              disabled={recorder.isRecording}
              aria-label="Clear transcript"
            >
              Clear
            </button>
          )}
          <button
            className="settings-toggle"
            onClick={() => setShowSettings((v) => !v)}
            aria-label={showSettings ? "Hide settings" : "Show settings"}
          >
            {showSettings ? "Hide Settings" : "Settings"}
          </button>
        </div>
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
        bidirectional={useBidirectional}
        transcribeOnly={settings.mode === "transcribe"}
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
