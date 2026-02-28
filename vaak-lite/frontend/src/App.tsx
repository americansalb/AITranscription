import { useState, useCallback, useRef } from "react";
import { ModeSelector, type TranscriptionMode } from "./components/ModeSelector";
import { LanguageSelector } from "./components/LanguageSelector";
import { RecordButton } from "./components/RecordButton";
import { AudioVisualizer } from "./components/AudioVisualizer";
import { TranscriptPanel, type TranscriptEntry } from "./components/TranscriptPanel";
import { useAudioRecorder } from "./hooks/useAudioRecorder";
import { transcribe } from "./lib/api";

export default function App() {
  const [mode, setMode] = useState<TranscriptionMode>("unidirectional");
  const [language, setLanguage] = useState("auto");
  const [entries, setEntries] = useState<TranscriptEntry[]>([]);
  const [processing, setProcessing] = useState(false);
  const [statusMsg, setStatusMsg] = useState<string | null>(null);
  const nextId = useRef(1);
  const segmentCount = useRef(0);
  const speakerToggle = useRef<"A" | "B">("A");

  const recorder = useAudioRecorder();

  // ── Helpers ────────────────────────────────────────────────

  const addEntry = useCallback(
    (
      text: string,
      result: { language: string | null; duration: number | null; segments: any[] },
      extra: Partial<TranscriptEntry> = {}
    ) => {
      const entry: TranscriptEntry = {
        id: nextId.current++,
        text,
        language: result.language,
        duration: result.duration,
        segments: result.segments ?? [],
        timestamp: new Date(),
        ...extra,
      };
      setEntries((prev) => [...prev, entry]);
    },
    []
  );

  const showStatus = useCallback((msg: string, durationMs = 3000) => {
    setStatusMsg(msg);
    setTimeout(() => setStatusMsg(null), durationMs);
  }, []);

  // ── Mode-specific recording logic ─────────────────────────

  const handleUnidirectional = useCallback(async () => {
    if (recorder.isRecording) {
      setProcessing(true);
      try {
        const blob = await recorder.stop();
        const result = await transcribe(blob, language);
        if (result.text.trim()) {
          addEntry(result.text, result);
        } else {
          showStatus("No speech detected");
        }
      } catch (err) {
        showStatus(err instanceof Error ? err.message : "Transcription failed");
      } finally {
        setProcessing(false);
      }
    } else {
      await recorder.start();
    }
  }, [recorder, language, addEntry, showStatus]);

  const handleConversational = useCallback(async () => {
    if (recorder.isRecording) {
      setProcessing(true);
      try {
        const blob = await recorder.stop();
        const result = await transcribe(blob, language);

        if (!result.text.trim()) {
          showStatus("No speech detected");
          setProcessing(false);
          return;
        }

        // Use Whisper segments to detect speaker turns via silence gaps
        if (result.segments && result.segments.length > 1) {
          let currentSpeaker = speakerToggle.current;
          let currentGroup: string[] = [];
          let groupStart = result.segments[0]?.start ?? 0;

          for (let i = 0; i < result.segments.length; i++) {
            const seg = result.segments[i];
            const prevEnd = i > 0 ? result.segments[i - 1].end : seg.start;
            const gap = seg.start - prevEnd;

            if (gap > 1.5 && currentGroup.length > 0) {
              // Speaker turn: flush current group
              addEntry(currentGroup.join(" ").trim(), result, {
                speaker: currentSpeaker,
                duration: prevEnd - groupStart,
              });
              currentSpeaker = currentSpeaker === "A" ? "B" : "A";
              currentGroup = [];
              groupStart = seg.start;
            }
            currentGroup.push(seg.text.trim());
          }

          // Flush remaining
          if (currentGroup.length > 0) {
            addEntry(currentGroup.join(" ").trim(), result, {
              speaker: currentSpeaker,
            });
            speakerToggle.current = currentSpeaker === "A" ? "B" : "A";
          }
        } else {
          // Single segment — assign to current speaker and toggle
          addEntry(result.text, result, { speaker: speakerToggle.current });
          speakerToggle.current = speakerToggle.current === "A" ? "B" : "A";
        }
      } catch (err) {
        showStatus(err instanceof Error ? err.message : "Transcription failed");
      } finally {
        setProcessing(false);
      }
    } else {
      await recorder.start();
    }
  }, [recorder, language, addEntry, showStatus]);

  const handleConsecutive = useCallback(async () => {
    if (recorder.isRecording) {
      setProcessing(true);
      segmentCount.current += 1;
      try {
        const blob = await recorder.stop();
        const result = await transcribe(blob, language);
        if (result.text.trim()) {
          addEntry(result.text, result, { segmentNumber: segmentCount.current });
        } else {
          showStatus("No speech detected in this segment");
          segmentCount.current -= 1;
        }
      } catch (err) {
        showStatus(err instanceof Error ? err.message : "Transcription failed");
        segmentCount.current -= 1;
      } finally {
        setProcessing(false);
      }
    } else {
      await recorder.start();
    }
  }, [recorder, language, addEntry, showStatus]);

  const handleSimultaneous = useCallback(async () => {
    if (recorder.isRecording) {
      setProcessing(true);
      try {
        await recorder.stop();
      } catch {
        // ignore
      }
      // Mark all partials as confirmed
      setEntries((prev) => prev.map((e) => (e.partial ? { ...e, partial: false } : e)));
      setProcessing(false);
    } else {
      await recorder.startChunked(5000, async (chunk, seq) => {
        try {
          // Add a partial placeholder
          const placeholderId = nextId.current++;
          setEntries((prev) => [
            ...prev,
            {
              id: placeholderId,
              text: "...",
              language: null,
              duration: null,
              segments: [],
              timestamp: new Date(),
              partial: true,
            },
          ]);

          const result = await transcribe(chunk, language);

          // Replace placeholder with actual result
          setEntries((prev) =>
            prev.map((e) =>
              e.id === placeholderId
                ? {
                    ...e,
                    text: result.text,
                    language: result.language,
                    duration: result.duration,
                    segments: result.segments,
                    partial: false,
                  }
                : e
            )
          );
        } catch {
          // Remove failed placeholder
          setEntries((prev) => prev.filter((e) => !e.partial || e.text !== "..."));
        }
      });
    }
  }, [recorder, language]);

  // ── Dispatch to mode handler ──────────────────────────────

  const handleRecord = useCallback(() => {
    switch (mode) {
      case "unidirectional":
        return handleUnidirectional();
      case "conversational":
        return handleConversational();
      case "consecutive":
        return handleConsecutive();
      case "simultaneous":
        return handleSimultaneous();
    }
  }, [mode, handleUnidirectional, handleConversational, handleConsecutive, handleSimultaneous]);

  // ── Mode change resets state ──────────────────────────────

  const handleModeChange = useCallback(
    (newMode: TranscriptionMode) => {
      if (recorder.isRecording) return; // Don't switch while recording
      setMode(newMode);
      setEntries([]);
      segmentCount.current = 0;
      speakerToggle.current = "A";
    },
    [recorder.isRecording]
  );

  // ── Hints per mode ────────────────────────────────────────

  const recordHint = recorder.isRecording
    ? mode === "consecutive"
      ? "Tap to end this segment"
      : "Tap to stop"
    : mode === "consecutive"
      ? "Tap to record a segment"
      : mode === "simultaneous"
        ? "Tap to start live transcription"
        : "Tap to start";

  // ── Word count ────────────────────────────────────────────

  const totalWords = entries.reduce(
    (sum, e) => sum + (e.text === "..." ? 0 : e.text.split(/\s+/).filter(Boolean).length),
    0
  );

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
      </header>

      <ModeSelector
        mode={mode}
        onChange={handleModeChange}
        disabled={recorder.isRecording || processing}
      />

      <LanguageSelector
        value={language}
        onChange={setLanguage}
        disabled={recorder.isRecording}
      />

      <TranscriptPanel entries={entries} mode={mode} isRecording={recorder.isRecording} />

      <div className="controls">
        <AudioVisualizer analyser={recorder.analyser} isRecording={recorder.isRecording} />
        <RecordButton
          isRecording={recorder.isRecording}
          onClick={handleRecord}
          disabled={processing}
          hint={processing ? "Processing..." : recordHint}
        />
      </div>

      <footer className="app-footer">
        {recorder.isRecording && (
          <span className="stat">{formatDuration(recorder.duration)}</span>
        )}
        {totalWords > 0 && <span className="stat">{totalWords} words</span>}
        {entries.length > 0 && entries[entries.length - 1].language && (
          <span className="stat">{entries[entries.length - 1].language}</span>
        )}
      </footer>

      {recorder.error && <div className="error-toast">{recorder.error}</div>}
      {statusMsg && <div className="status-toast">{statusMsg}</div>}
    </div>
  );
}
