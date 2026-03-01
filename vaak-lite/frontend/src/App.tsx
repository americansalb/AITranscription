import { useState, useCallback, useRef, useEffect } from "react";
import { SettingsPanel, DEFAULT_SETTINGS, type InterpretationSettings } from "./components/SettingsPanel";
import { InterpretationView, type InterpretationEntry, type EntryStatus } from "./components/InterpretationView";
import { RecordButton } from "./components/RecordButton";
import { AudioVisualizer } from "./components/AudioVisualizer";
import { useAudioRecorder } from "./hooks/useAudioRecorder";
import { useSpeechSynthesis } from "./hooks/useSpeechSynthesis";
import { interpret, transcribe, translateText, getProviders, type ProviderInfo } from "./lib/api";

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
  // Track the seq of the latest transcription result we accepted
  const lastAcceptedSeq = useRef(-1);
  // Debounced translation: abort stale LLM calls, timer for debounce
  const translateAbort = useRef<AbortController | null>(null);
  const translateTimer = useRef<number>(0);
  // Latest source text for the current simultaneous entry (for debounced translation)
  const latestSourceText = useRef("");
  const latestSourceLang = useRef("");
  const recorder = useAudioRecorder();
  const tts = useSpeechSynthesis();

  // TTS: track which entries have been spoken and debounce timer
  const lastSpokenEntryId = useRef(0);
  const ttsDebounceTimer = useRef<number>(0);
  const lastTranslatedText = useRef("");

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

  // TTS: auto-read translations after silence delay
  useEffect(() => {
    if (!settings.ttsEnabled || settings.mode !== "interpret") return;
    if (ttsDebounceTimer.current) clearTimeout(ttsDebounceTimer.current);

    // Find the latest entry with translated text
    const latestWithText = [...entries].reverse().find(
      (e) => e.translatedText && e.id > lastSpokenEntryId.current,
    );
    if (!latestWithText) return;

    // If the text changed, reset the debounce timer
    if (latestWithText.translatedText !== lastTranslatedText.current) {
      lastTranslatedText.current = latestWithText.translatedText;
    }

    // Wait for silence delay, then speak
    ttsDebounceTimer.current = window.setTimeout(() => {
      if (!settings.ttsEnabled) return;
      const textToSpeak = latestWithText.translatedText;
      if (!textToSpeak) return;

      // Find the selected voice
      const targetVoices = tts.voicesForLang(settings.targetLang);
      const selectedVoice = settings.ttsVoice
        ? targetVoices.find((v) => v.voiceURI === settings.ttsVoice) || null
        : targetVoices[0] || null;

      tts.speak(textToSpeak, selectedVoice, settings.ttsRate);
      lastSpokenEntryId.current = latestWithText.id;
    }, settings.ttsSilenceDelay * 1000);

    return () => {
      if (ttsDebounceTimer.current) clearTimeout(ttsDebounceTimer.current);
    };
  }, [entries, settings.ttsEnabled, settings.ttsSilenceDelay, settings.ttsRate, settings.ttsVoice, settings.targetLang, settings.mode, tts]);

  // Auto-stop TTS when recording starts (don't talk over the mic)
  useEffect(() => {
    if (recorder.isRecording && tts.isSpeaking) {
      tts.stop();
    }
  }, [recorder.isRecording, tts]);

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
  // Transcription (Whisper) runs on every chunk without throttling — it's
  // fast and each chunk's rolling window supersedes the previous one.
  // Translation (LLM) is debounced: it only fires 2 seconds after the
  // source text stops changing, and stale translations are aborted.

  /** Fire a debounced translation for the current source text. */
  const scheduleTranslation = useCallback(
    (entryId: number) => {
      if (translateTimer.current) clearTimeout(translateTimer.current);
      translateTimer.current = window.setTimeout(async () => {
        const text = latestSourceText.current;
        const srcLang = latestSourceLang.current;
        if (!text.trim() || settings.mode !== "interpret") return;

        // Abort any previous in-flight translation
        if (translateAbort.current) translateAbort.current.abort();
        const controller = new AbortController();
        translateAbort.current = controller;

        try {
          const result = await translateText(
            text, srcLang, settings.targetLang, settings.provider, controller.signal,
          );
          setEntries((prev) =>
            prev.map((e) =>
              e.id === entryId
                ? { ...e, translatedText: result.translated_text, provider: result.provider }
                : e,
            ),
          );
        } catch (err) {
          if (err instanceof DOMException && err.name === "AbortError") return;
          console.warn("Translation failed:", err);
        }
      }, 2000);
    },
    [settings],
  );

  const processSimultaneousChunk = useCallback(
    async (blob: Blob, seq: number, speaker?: "A" | "B") => {
      const isTranslation = settings.mode === "interpret";

      // Create the entry on first chunk only
      if (simultaneousEntryId.current === null) {
        const entryId = nextId.current++;
        simultaneousEntryId.current = entryId;
        lastAcceptedSeq.current = -1;
        latestSourceText.current = "";
        latestSourceLang.current = "";
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
        // Always transcribe — Whisper is fast, don't throttle it
        const lang = settings.sourceLang !== "auto" ? settings.sourceLang : undefined;
        const result = await transcribe(blob, lang);
        if (!result.text.trim()) return;

        // Ignore out-of-order responses
        if (seq <= lastAcceptedSeq.current) return;
        lastAcceptedSeq.current = seq;

        // Update source text immediately
        latestSourceText.current = result.text;
        latestSourceLang.current = result.language || settings.sourceLang;
        setEntries((prev) =>
          prev.map((e) =>
            e.id === currentEntryId
              ? {
                  ...e,
                  sourceText: result.text,
                  sourceLang: result.language || settings.sourceLang,
                  duration: result.duration,
                  pending: false,
                  status: "in_progress",
                  seq,
                }
              : e,
          ),
        );

        // Schedule translation (debounced — only fires when text stabilizes)
        if (isTranslation) {
          scheduleTranslation(currentEntryId);
        }
      } catch (err) {
        console.warn("Simultaneous chunk failed:", err);
      }
    },
    [settings, scheduleTranslation],
  );

  // Finalize the in-progress simultaneous entry
  const finalizeSimultaneous = useCallback(
    async () => {
      // Cancel pending translation timer
      if (translateTimer.current) {
        clearTimeout(translateTimer.current);
        translateTimer.current = 0;
      }

      const entryId = simultaneousEntryId.current;
      if (entryId === null) return;

      // If in interpret mode, fire one final translation with the latest text
      if (settings.mode === "interpret" && latestSourceText.current.trim()) {
        if (translateAbort.current) translateAbort.current.abort();
        const controller = new AbortController();
        translateAbort.current = controller;
        try {
          const result = await translateText(
            latestSourceText.current,
            latestSourceLang.current,
            settings.targetLang,
            settings.provider,
            controller.signal,
          );
          setEntries((prev) =>
            prev.map((e) =>
              e.id === entryId
                ? { ...e, translatedText: result.translated_text, provider: result.provider, status: "complete" as EntryStatus }
                : e,
            ),
          );
        } catch {
          // Even if translation fails, still mark entry complete
          setEntries((prev) =>
            prev.map((e) =>
              e.id === entryId ? { ...e, status: "complete" as EntryStatus } : e,
            ),
          );
        }
      } else {
        setEntries((prev) =>
          prev.map((e) =>
            e.id === entryId ? { ...e, status: "complete" as EntryStatus } : e,
          ),
        );
      }

      simultaneousEntryId.current = null;
      lastAcceptedSeq.current = -1;
      latestSourceText.current = "";
      latestSourceLang.current = "";
    },
    [settings],
  );

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
      lastAcceptedSeq.current = -1;
      latestSourceText.current = "";
      latestSourceLang.current = "";
      if (translateAbort.current) {
        translateAbort.current.abort();
        translateAbort.current = null;
      }
      if (translateTimer.current) {
        clearTimeout(translateTimer.current);
        translateTimer.current = 0;
      }
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
          ttsVoices={tts.voicesForLang(settings.targetLang)}
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

      {tts.isSpeaking && (
        <div className="tts-bar">
          <span className="tts-label">{tts.isPaused ? "Paused" : "Speaking..."}</span>
          <button
            className="tts-btn"
            onClick={() => tts.isPaused ? tts.resume() : tts.pause()}
            aria-label={tts.isPaused ? "Resume speech" : "Pause speech"}
          >
            {tts.isPaused ? "Resume" : "Pause"}
          </button>
          <button
            className="tts-btn tts-stop"
            onClick={() => tts.stop()}
            aria-label="Stop speech"
          >
            Stop
          </button>
        </div>
      )}

      <footer className="app-footer">
        {recorder.isRecording && <span className="stat">{formatDuration(recorder.duration)}</span>}
        {entries.length > 0 && <span className="stat">{entries.length} segments</span>}
      </footer>

      {recorder.error && <div className="error-toast">{recorder.error}</div>}
      {statusMsg && <div className="status-toast">{statusMsg}</div>}
    </div>
  );
}
