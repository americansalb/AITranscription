import { useState, useCallback, useEffect, useRef } from "react";
// Force reload to pick up hotkey hook changes
import { useUnifiedAudioRecorder } from "./hooks/useUnifiedAudioRecorder";
import { useGlobalHotkey } from "./hooks/useGlobalHotkey";
import { transcribeAndPolish, transcribe, polish, checkHealth, ApiError, isLoggedIn, submitFeedback, getApiBaseUrl, getAuthToken, getUserStats } from "./lib/api";
import { injectText, setTrayRecordingState, updateOverlayState } from "./lib/clipboard";
import { isMacOS, formatHotkeyForDisplay as formatHotkeyDisplay } from "./lib/platform";
import { trimHistory, enforceStorageCap } from "./lib/storageManager";
import { Settings, getStoredHotkey } from "./components/Settings";
import { AudioIndicator } from "./components/AudioIndicator";
import { StatsPanel } from "./components/StatsPanel";
import { TranscriptHistory } from "./components/TranscriptHistory";
import { ClaudeOutputsViewer } from "./components/ClaudeOutputsViewer";
import { KeyboardShortcutsModal } from "./components/KeyboardShortcutsModal";
import { ComparisonView } from "./components/ComparisonView";
import { ExportModal } from "./components/ExportModal";
import { LearningDashboard } from "./components/LearningDashboard";
import { useToast } from "./components/Toast";
import { Confetti } from "./components/Confetti";
import { playStartSound, playStopSound, playSuccessSound, playErrorSound } from "./lib/sounds";
import { initSpeakListener } from "./lib/speak";
import { getStoredVoiceEnabled, saveVoiceEnabled } from "./lib/voiceStream";
import { onRecordingStart, onRecordingStop } from "./lib/interruptManager";
import { QueueSlidePanel } from "./components/QueueSlidePanel";
import {
  VolumeOnIcon, VolumeOffIcon, BookIcon, BarChartIcon, ClockIcon,
  ChatBubbleIcon, MonitorIcon, SunIcon, GearIcon, CompareIcon,
  RefreshIcon, EditIcon, CopyIcon, CheckIcon, XIcon, DownloadIcon,
} from "./components/Icons";

// One-time migration from old "scribe_*" localStorage keys to "vaak_*"
function migrateLocalStorageKeys() {
  if (localStorage.getItem("vaak_keys_migrated")) return;
  const keyMap: [string, string][] = [
    ["scribe_token", "vaak_token"],
    ["scribe_context", "vaak_context"],
    ["scribe_formality", "vaak_formality"],
    ["scribe_history", "vaak_history"],
    ["scribe_sound_enabled", "vaak_sound_enabled"],
    ["scribe_overlay_position", "vaak_overlay_position"],
    ["scribe_speak_sessions", "vaak_speak_sessions"],
    ["scribe_voice_enabled", "vaak_voice_enabled"],
    ["scribe_blind_mode", "vaak_blind_mode"],
    ["scribe_voice_mode", "vaak_voice_mode"],
    ["scribe_voice_detail", "vaak_voice_detail"],
    ["scribe_voice_auto", "vaak_voice_auto"],
    ["scribe_hotkey", "vaak_hotkey"],
    ["scribe_whisper_model", "vaak_whisper_model"],
    ["scribe_noise_cancellation", "vaak_noise_cancellation"],
    ["scribe_audio_indicator_position", "vaak_audio_indicator_position"],
    ["scribe_queue_autoplay", "vaak_queue_autoplay"],
  ];
  for (const [oldKey, newKey] of keyMap) {
    const val = localStorage.getItem(oldKey);
    if (val !== null && localStorage.getItem(newKey) === null) {
      localStorage.setItem(newKey, val);
    }
  }
  localStorage.setItem("vaak_keys_migrated", "1");
}
migrateLocalStorageKeys();

// Error types for actionable messages
interface ActionableError {
  message: string;
  action?: {
    label: string;
    onClick: () => void;
  };
}

// Transcript history entry
export interface TranscriptEntry {
  id: string;
  rawText: string;
  polishedText: string;
  context: string;
  formality: string;
  timestamp: number;
  confidence?: number;
  duration?: number | null;
}

// Settings expects Date for timestamp - create alias with Date type
export interface HistoryEntry {
  id: string;
  rawText: string;
  polishedText: string;
  context: string;
  formality: string;
  timestamp: Date;
  confidence?: number;
  duration?: number | null;
}

// Context icons for the dropdown
const CONTEXT_OPTIONS = [
  { value: "general", label: "General", icon: "🎯" },
  { value: "email", label: "Email", icon: "📧" },
  { value: "slack", label: "Slack / Chat", icon: "💬" },
  { value: "document", label: "Document", icon: "📄" },
  { value: "code", label: "Code Comment", icon: "💻" },
];

// Quick Mode Presets
interface Preset {
  id: string;
  name: string;
  icon: string;
  context: string;
  formality: "casual" | "neutral" | "formal";
  description: string;
}

const PRESETS: Preset[] = [
  { id: "pro-email", name: "Professional Email", icon: "📧", context: "email", formality: "formal", description: "Formal business emails" },
  { id: "casual-slack", name: "Casual Chat", icon: "💬", context: "slack", formality: "casual", description: "Friendly messages" },
  { id: "meeting-notes", name: "Meeting Notes", icon: "📝", context: "document", formality: "neutral", description: "Clear documentation" },
  { id: "code-docs", name: "Code Comments", icon: "💻", context: "code", formality: "neutral", description: "Technical docs" },
];

// Processing steps for progress indicator
type ProcessingStep = "recording" | "transcribing" | "polishing" | "done";
const PROCESSING_STEPS: { key: ProcessingStep; label: string }[] = [
  { key: "recording", label: "Recording" },
  { key: "transcribing", label: "Transcribing" },
  { key: "polishing", label: "Polishing" },
  { key: "done", label: "Done" },
];

// LocalStorage keys
const STORAGE_KEYS = {
  CONTEXT: "vaak_context",
  FORMALITY: "vaak_formality",
  HISTORY: "vaak_history",
  SOUND_ENABLED: "vaak_sound_enabled",
  POLISH_ENABLED: "vaak_polish_enabled",
};

export function getPolishEnabled(): boolean {
  const stored = localStorage.getItem(STORAGE_KEYS.POLISH_ENABLED);
  return stored === null ? true : stored === "true";
}

export function savePolishEnabled(enabled: boolean): void {
  localStorage.setItem(STORAGE_KEYS.POLISH_ENABLED, String(enabled));
}

// Tauri window APIs for overlay (Tauri 2.0)
declare global {
  interface Window {
    __TAURI__?: {
      event: {
        emit: (event: string, payload: unknown) => Promise<void>;
      };
      webviewWindow: {
        WebviewWindow: {
          getByLabel: (label: string) => Promise<{
            show: () => Promise<void>;
            hide: () => Promise<void>;
            setPosition: (position: { type: string; x: number; y: number }) => Promise<void>;
          } | null>;
        };
        getCurrentWebviewWindow: () => {
          show: () => Promise<void>;
          hide: () => Promise<void>;
        };
      };
      window: {
        currentMonitor: () => Promise<{ size: { width: number; height: number } } | null>;
      };
    };
  }
}

type ProcessingStatus = "idle" | "recording" | "processing" | "success" | "error";

function formatDuration(seconds: number): string {
  const mins = Math.floor(seconds / 60);
  const secs = seconds % 60;
  return `${mins}:${secs.toString().padStart(2, "0")}`;
}

// Load persisted setting from localStorage
function loadSetting<T>(key: string, defaultValue: T): T {
  try {
    const stored = localStorage.getItem(key);
    if (stored !== null) {
      return JSON.parse(stored) as T;
    }
  } catch (e) {
    console.warn(`Failed to load setting ${key}:`, e);
  }
  return defaultValue;
}

// Save setting to localStorage
function saveSetting<T>(key: string, value: T): void {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch (e) {
    console.warn(`Failed to save setting ${key}:`, e);
  }
}

function ScreenReaderButton() {
  return (
    <button
      className="screen-reader-btn"
      title="Screen Reader Settings"
      aria-label="Open screen reader settings"
      onClick={async () => {
        try {
          const { invoke } = await import("@tauri-apps/api/core");
          await invoke("toggle_screen_reader_window");
        } catch (err) {
          console.error("Failed to open screen reader window:", err);
        }
      }}
    >
      <MonitorIcon />
      <span className="screen-reader-btn-label">Screen</span>
    </button>
  );
}

function App() {
  const recorder = useUnifiedAudioRecorder();
  const { showToast } = useToast();
  const [status, setStatus] = useState<ProcessingStatus>("idle");
  const [result, setResult] = useState<string>("");
  const [rawText, setRawText] = useState<string>("");
  const [error, setError] = useState<ActionableError | null>(null);
  const [copied, setCopied] = useState(false);
  const [context, setContext] = useState<string>(() => loadSetting(STORAGE_KEYS.CONTEXT, "general"));
  const [formality, setFormality] = useState<"casual" | "neutral" | "formal">(() => loadSetting(STORAGE_KEYS.FORMALITY, "neutral"));
  const [showRaw, setShowRaw] = useState(false);
  const [backendReady, setBackendReady] = useState<boolean | null>(null);
  const [showSettings, setShowSettings] = useState(false);
  const [showStats, setShowStats] = useState(false);
  const [showHistory, setShowHistory] = useState(false);
  const [showClaudeOutputs, setShowClaudeOutputs] = useState(false);
  const [showShortcuts, setShowShortcuts] = useState(false);
  const [transcriptionCount, setTranscriptionCount] = useState(0); // Increments after each successful transcription
  const [showConfetti, setShowConfetti] = useState(false);
  const [previousTranscriptionCount, setPreviousTranscriptionCount] = useState(0);
  const [soundEnabled, setSoundEnabled] = useState<boolean>(() => loadSetting(STORAGE_KEYS.SOUND_ENABLED, true));
  const [currentHotkey, setCurrentHotkey] = useState<string>(() => getStoredHotkey());
  const [voiceEnabled, setVoiceEnabled] = useState<boolean>(() => getStoredVoiceEnabled());
  const [showQueuePanel, setShowQueuePanel] = useState(false);

  // Handle hotkey change from Settings
  const handleHotkeyChange = useCallback((newHotkey: string) => {
    setCurrentHotkey(newHotkey);
  }, []);

  // Handle voice toggle - save preference and update CLAUDE.md
  const handleVoiceToggle = useCallback(async () => {
    const newEnabled = !voiceEnabled;
    setVoiceEnabled(newEnabled);
    saveVoiceEnabled(newEnabled);

    // If disabling voice, clear pending items from queue
    if (!newEnabled) {
      const { clearPending } = await import("./lib/queueStore");
      await clearPending();
    }

    // Update CLAUDE.md to reflect the new setting
    if (window.__TAURI__) {
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        const { emit } = await import("@tauri-apps/api/event");
        const { getStoredBlindMode, getStoredVoiceDetail } = await import("./lib/voiceStream");
        await invoke("update_claude_md", {
          enabled: newEnabled,
          blindMode: getStoredBlindMode(),
          detail: getStoredVoiceDetail(),
        });

        // Emit event to sync other windows
        await emit("voice-settings-changed", {
          voiceEnabled: newEnabled,
          blindMode: getStoredBlindMode(),
          detail: getStoredVoiceDetail()
        });

        showToast(newEnabled ? "Vaak Speak enabled" : "Vaak Speak disabled", "info");
      } catch (err) {
        console.error("[App] Failed to update CLAUDE.md:", err);
      }
    }
  }, [voiceEnabled, showToast]);

  // Listen for voice settings changes from other windows
  useEffect(() => {
    if (!window.__TAURI__) return;

    let unlisten: (() => void) | undefined;

    const setupListener = async () => {
      try {
        const { listen } = await import("@tauri-apps/api/event");
        unlisten = await listen<{ voiceEnabled: boolean; blindMode: boolean; detail: number }>(
          "voice-settings-changed",
          (event) => {
            console.log("[App] Received voice-settings-changed event:", event.payload);
            setVoiceEnabled(event.payload.voiceEnabled);
          }
        );
      } catch (err) {
        console.error("[App] Failed to setup voice settings listener:", err);
      }
    };

    setupListener();

    return () => {
      if (unlisten) unlisten();
    };
  }, []);

  // New state for editing and progress
  const [isEditing, setIsEditing] = useState(false);
  const [editedText, setEditedText] = useState("");
  const [processingStep, setProcessingStep] = useState<ProcessingStep>("recording");
  const editTextareaRef = useRef<HTMLTextAreaElement>(null);

  // Week 3: New state for comparison, regenerate, history, confidence
  const [showComparison, setShowComparison] = useState(false);
  const [showExport, setShowExport] = useState(false);
  const [showLearning, setShowLearning] = useState(false);
  const [isRegenerating, setIsRegenerating] = useState(false);
  const [confidence, setConfidence] = useState<number | null>(null);

  // Learning system: track original text before editing for feedback
  const [originalPolishedText, setOriginalPolishedText] = useState<string>("");
  const [transcriptHistory, setTranscriptHistory] = useState<TranscriptEntry[]>(() => {
    try {
      const stored = localStorage.getItem(STORAGE_KEYS.HISTORY);
      return stored ? JSON.parse(stored) : [];
    } catch {
      return [];
    }
  });

  // Save history to localStorage when it changes
  useEffect(() => {
    try {
      // Keep only last 50 entries with capped text sizes (enforces 5MB global limit)
      const trimmed = trimHistory(transcriptHistory, 50);
      localStorage.setItem(STORAGE_KEYS.HISTORY, JSON.stringify(trimmed));
      enforceStorageCap();
    } catch (e) {
      console.warn("Failed to save history:", e);
    }
  }, [transcriptHistory]);

  // Add entry to history
  const addToHistory = useCallback((entry: Omit<TranscriptEntry, "id" | "timestamp">) => {
    const newEntry: TranscriptEntry = {
      ...entry,
      id: `${Date.now()}-${Math.random().toString(36).substr(2, 9)}`,
      timestamp: Date.now(),
    };
    setTranscriptHistory(prev => [newEntry, ...prev]);
  }, []);

  // Load user's actual transcription count on startup to prevent false milestone celebrations
  useEffect(() => {
    const loadUserStats = async () => {
      if (isLoggedIn()) {
        try {
          const stats = await getUserStats();
          // Initialize both counts to the real total so we don't trigger old milestones
          setTranscriptionCount(stats.total_transcriptions);
          setPreviousTranscriptionCount(stats.total_transcriptions);
        } catch (e) {
          console.warn("Failed to load user stats:", e);
        }
      }
    };
    loadUserStats();
  }, []);

  // Check for achievement milestones
  useEffect(() => {
    const milestones = [1, 10, 25, 50, 100, 250, 500, 1000];
    if (transcriptionCount > previousTranscriptionCount && milestones.includes(transcriptionCount)) {
      setShowConfetti(true);
      showToast(`Achievement unlocked! ${transcriptionCount} transcriptions!`, "success");
    }
    setPreviousTranscriptionCount(transcriptionCount);
  }, [transcriptionCount, previousTranscriptionCount, showToast]);

  // Refs for push-to-talk state management
  const isProcessingRef = useRef(false);
  const contextRef = useRef(context);
  const formalityRef = useRef(formality);

  // Keep refs in sync with state
  useEffect(() => {
    contextRef.current = context;
    formalityRef.current = formality;
  }, [context, formality]);

  // Persist context and formality to localStorage
  useEffect(() => {
    saveSetting(STORAGE_KEYS.CONTEXT, context);
  }, [context]);

  useEffect(() => {
    saveSetting(STORAGE_KEYS.FORMALITY, formality);
  }, [formality]);

  useEffect(() => {
    saveSetting(STORAGE_KEYS.SOUND_ENABLED, soundEnabled);
  }, [soundEnabled]);

  // Keyboard shortcut: ? to open shortcuts modal
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      // Don't trigger if typing in an input/textarea
      if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) {
        return;
      }
      if (e.key === "?" || (e.shiftKey && e.key === "/")) {
        e.preventDefault();
        setShowShortcuts(true);
      }
      // Escape to close modals
      if (e.key === "Escape") {
        if (showShortcuts) setShowShortcuts(false);
        if (isEditing) {
          setIsEditing(false);
          setEditedText("");
        }
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [showShortcuts, isEditing]);

  // Focus textarea when entering edit mode
  useEffect(() => {
    if (isEditing && editTextareaRef.current) {
      editTextareaRef.current.focus();
      editTextareaRef.current.select();
    }
  }, [isEditing]);

  // Update tray icon state
  useEffect(() => {
    setTrayRecordingState(recorder.isRecording);
  }, [recorder.isRecording]);

  // Feature 3: Interrupt-on-Record - auto-pause TTS when recording
  useEffect(() => {
    if (recorder.isRecording) {
      onRecordingStart();
    } else {
      onRecordingStop();
    }
  }, [recorder.isRecording]);

  // Update overlay state (overlay is always visible, just expands/collapses)
  useEffect(() => {
    updateOverlayState({
      isRecording: recorder.isRecording,
      isProcessing: status === "processing",
      duration: recorder.duration,
      audioLevel: recorder.audioLevel || 0,
    });
  }, [recorder.isRecording, recorder.duration, recorder.audioLevel, status]);

  // Cancel recording handler
  const handleCancelRecording = useCallback(async () => {
    if (recorder.isRecording) {
      await recorder.stopRecording(); // Stop but discard
      setStatus("idle");
      setError(null);
      showToast("Recording cancelled", "info");
    }
  }, [recorder, showToast]);

  // Guard against double-firing from native hook + Tauri plugin
  const isStartingRecordingRef = useRef(false);

  // Push-to-talk: start recording on key down
  const handleHotkeyDown = useCallback(async () => {
    if (recorder.isRecording || isProcessingRef.current || backendReady === false || isStartingRecordingRef.current) return;
    isStartingRecordingRef.current = true;

    setError(null);
    setResult("");
    setRawText("");

    try {
      await recorder.startRecording();
      setStatus("recording");
      if (soundEnabled) playStartSound(); // Audio feedback
    } catch (err) {
      const message = err instanceof Error ? err.message : "Failed to start recording";

      // Create actionable error for microphone issues
      if (message.toLowerCase().includes("microphone") || message.toLowerCase().includes("permission") || message.toLowerCase().includes("not found")) {
        setError({
          message: "Microphone access denied or not found",
          action: {
            label: "Grant Permission",
            onClick: () => {
              // Try to request microphone permission again
              navigator.mediaDevices.getUserMedia({ audio: true })
                .then(() => {
                  setError(null);
                  showToast("Microphone access granted!", "success");
                })
                .catch(() => {
                  showToast("Please enable microphone in system settings", "error");
                });
            },
          },
        });
      } else {
        setError({ message });
      }
      setStatus("error");
      if (soundEnabled) playErrorSound();
    } finally {
      isStartingRecordingRef.current = false;
    }
  }, [recorder, backendReady, showToast, soundEnabled]);

  // Shared transcription processing logic — used by both hotkey and click handlers
  const processRecording = useCallback(async (audioBlob: Blob, ctx: string, fmt: "casual" | "neutral" | "formal") => {
    setProcessingStep("transcribing");

    let rawText: string;
    let polishedText: string;

    let transcriptSaved = false;
    if (getPolishEnabled()) {
      const response = await transcribeAndPolish(audioBlob, {
        context: ctx === "general" ? undefined : ctx,
        formality: fmt,
      });
      setProcessingStep("polishing");
      rawText = response.raw_text;
      polishedText = response.polished_text;
      transcriptSaved = response.saved;
    } else {
      const response = await transcribe(audioBlob);
      rawText = response.raw_text;
      polishedText = response.raw_text;
      transcriptSaved = false;
    }

    setRawText(rawText);
    setResult(polishedText);

    addToHistory({
      rawText,
      polishedText,
      context: ctx,
      formality: fmt,
    });

    // Auto-inject FIRST before any UI feedback (toast/sound might activate Vaak on Mac)
    let pasteWarning = "";
    if (polishedText) {
      const injectResult = await injectText(polishedText);
      if (!injectResult.success) {
        throw new Error(injectResult.message);
      }
      if (!injectResult.pasted) {
        pasteWarning = injectResult.message;
      }
    }

    // Done
    setProcessingStep("done");
    setStatus("success");
    if (soundEnabled) playSuccessSound();

    // Single consolidated toast
    if (pasteWarning) {
      showToast(pasteWarning, "warning");
    } else if (!transcriptSaved && getPolishEnabled()) {
      showToast("Transcription complete (not saved — log in to save)", "warning");
    } else {
      showToast("Transcription complete!", "success");
    }

    setTranscriptionCount((c) => c + 1);
  }, [showToast, soundEnabled, addToHistory]);

  // Shared error handling for transcription failures
  const handleTranscriptionError = useCallback((err: unknown) => {
    const message =
      err instanceof ApiError
        ? err.detail || err.message
        : err instanceof Error
          ? err.message
          : "An error occurred";

    if (message.toLowerCase().includes("network") || message.toLowerCase().includes("connect") || message.toLowerCase().includes("fetch")) {
      setError({
        message: "Cannot connect to server",
        action: {
          label: "Retry",
          onClick: () => {
            setError(null);
            checkHealth().then((health) => {
              if (health.groq_configured && health.anthropic_configured) {
                showToast("Connection restored!", "success");
                setBackendReady(true);
              }
            }).catch(() => {
              showToast("Still unable to connect", "error");
            });
          },
        },
      });
    } else {
      setError({ message });
    }
    setProcessingStep("recording");
    setStatus("error");
    if (soundEnabled) playErrorSound();
    showToast(message, "error");
  }, [showToast, soundEnabled]);

  // Push-to-talk: stop recording and process on key up
  const handleHotkeyUp = useCallback(async () => {
    if (!recorder.isRecording || isProcessingRef.current) return;

    isProcessingRef.current = true;
    setStatus("processing");
    setProcessingStep("transcribing");
    setError(null);
    if (soundEnabled) playStopSound();

    try {
      const audioBlob = await recorder.stopRecording();
      if (!audioBlob) {
        throw new Error("No audio recorded");
      }
      await processRecording(audioBlob, contextRef.current, formalityRef.current);
    } catch (err) {
      handleTranscriptionError(err);
    } finally {
      isProcessingRef.current = false;
    }
  }, [recorder, soundEnabled, processRecording, handleTranscriptionError]);

  // Register global hotkey for push-to-talk (uses dynamic hotkey from settings)
  const { isRegistered: hotkeyRegistered } = useGlobalHotkey({
    hotkey: currentHotkey,
    onKeyDown: handleHotkeyDown,
    onKeyUp: handleHotkeyUp,
    enabled: backendReady !== false,
  });

  // Debug: log hotkey status
  useEffect(() => {
    console.log("Hotkey registered status:", hotkeyRegistered);
  }, [hotkeyRegistered]);

  // Check backend health on mount
  useEffect(() => {
    const performHealthCheck = () => {
      checkHealth()
        .then((health) => {
          setBackendReady(health.groq_configured && health.anthropic_configured);
          if (health.groq_configured && health.anthropic_configured) {
            setError(null);
          } else if (!health.groq_configured) {
            setError({
              message: "Backend: Groq API key not configured",
              action: {
                label: "Open Settings",
                onClick: () => setShowSettings(true),
              },
            });
          } else if (!health.anthropic_configured) {
            setError({
              message: "Backend: Anthropic API key not configured",
              action: {
                label: "Open Settings",
                onClick: () => setShowSettings(true),
              },
            });
          }
        })
        .catch(() => {
          setBackendReady(false);
          setError({
            message: "Cannot connect to backend. Is it running?",
            action: {
              label: "Retry",
              onClick: performHealthCheck,
            },
          });
        });
    };
    performHealthCheck();
  }, []);

  // Initialize speak listener for Claude Code integration
  useEffect(() => {
    let unlisten: (() => void) | undefined;

    initSpeakListener()
      .then((unlistenFn) => {
        unlisten = unlistenFn;
        console.log("[App] Speak listener initialized");
      })
      .catch((err) => {
        console.error("[App] Failed to initialize speak listener:", err);
      });

    return () => {
      if (unlisten) {
        unlisten();
      }
    };
  }, []);

  // Initialize project path and CLAUDE.md based on voice preference
  useEffect(() => {
    const initClaudeMd = async () => {
      if (!window.__TAURI__) return;

      try {
        const { invoke } = await import("@tauri-apps/api/core");
        const { getStoredVoiceEnabled, getStoredBlindMode, getStoredVoiceDetail } = await import("./lib/voiceStream");

        // First, set the project path so CLAUDE.md is written to the right location
        try {
          const { resourceDir } = await import("@tauri-apps/api/path");
          const resDir = await resourceDir();
          // Go up from resources to find project root
          const projectPath = resDir.replace(/[\\/]desktop[\\/]src-tauri[\\/]target[\\/].*$/, "");

          if (projectPath && projectPath !== resDir) {
            await invoke("set_project_path", { path: projectPath });
            console.log("[App] Set project path to:", projectPath);
          } else {
            console.warn("[App] Could not detect project root from resourceDir:", resDir);
          }
        } catch (pathErr) {
          console.warn("[App] Could not resolve project path:", pathErr);
        }

        // Now initialize CLAUDE.md with current settings
        const enabled = getStoredVoiceEnabled();
        const blindMode = getStoredBlindMode();
        const detail = getStoredVoiceDetail();

        await invoke("update_claude_md", { enabled, blindMode, detail });
        console.log(`[App] CLAUDE.md initialized: enabled=${enabled}, blindMode=${blindMode}, detail=${detail}`);
      } catch (err) {
        console.error("[App] Failed to initialize CLAUDE.md:", err);
      }
    };

    initClaudeMd();
  }, []);

  // Control overlay window visibility
  // Skip overlay on macOS - showing windows activates the app and breaks paste
  useEffect(() => {
    const updateOverlay = async () => {
      if (!window.__TAURI__) return;
      // Don't show overlay on macOS - it activates the app and breaks keyboard simulation
      if (isMacOS()) return;

      try {
        const { invoke } = await import("@tauri-apps/api/core");
        const { emit } = await import("@tauri-apps/api/event");

        const isActive = recorder.isRecording || status === "processing";

        // Show or hide the floating overlay window (Windows/Linux only)
        if (isActive) {
          await invoke("show_recording_overlay");
        } else {
          await invoke("hide_recording_overlay");
        }

        // Send state to overlay window
        await emit("overlay-update", {
          isRecording: recorder.isRecording,
          isProcessing: status === "processing",
          duration: recorder.duration || 0,
          audioLevel: recorder.audioLevel || 0,
        });
      } catch (err) {
        console.error("Overlay update failed:", err);
      }
    };

    updateOverlay();
  }, [recorder.isRecording, recorder.duration, recorder.audioLevel, status]);

  const handleRecordClick = useCallback(async () => {
    if (recorder.isRecording) {
      // Guard: prevent concurrent processing (matches hotkey handler)
      if (isProcessingRef.current) return;
      isProcessingRef.current = true;
      // Stop recording and process — uses shared processRecording
      setStatus("processing");
      setProcessingStep("transcribing");
      setError(null);
      if (soundEnabled) playStopSound();

      try {
        const audioBlob = await recorder.stopRecording();
        if (!audioBlob) {
          throw new Error("No audio recorded");
        }
        await processRecording(audioBlob, context, formality);
      } catch (err) {
        handleTranscriptionError(err);
      } finally {
        isProcessingRef.current = false;
      }
    } else {
      // Start recording
      setError(null);
      setResult("");
      setRawText("");
      setProcessingStep("recording");

      try {
        await recorder.startRecording();
        setStatus("recording");
      } catch (err) {
        const message =
          err instanceof Error ? err.message : "Failed to start recording";

        if (message.toLowerCase().includes("microphone") || message.toLowerCase().includes("permission")) {
          setError({
            message: "Microphone access denied or not found",
            action: {
              label: "Grant Permission",
              onClick: () => {
                navigator.mediaDevices.getUserMedia({ audio: true })
                  .then(() => {
                    setError(null);
                    showToast("Microphone access granted!", "success");
                  })
                  .catch(() => {
                    showToast("Please enable microphone in system settings", "error");
                  });
              },
            },
          });
        } else {
          setError({ message });
        }
        setStatus("error");
      }
    }
  }, [recorder, context, formality, showToast, soundEnabled, processRecording, handleTranscriptionError]);

  // Edit mode handlers
  const handleStartEditing = useCallback(() => {
    const textToEdit = showRaw ? rawText : result;
    setEditedText(textToEdit);
    // Store the original text before editing for learning feedback
    setOriginalPolishedText(textToEdit);
    setIsEditing(true);
  }, [showRaw, rawText, result]);

  const handleSaveEdit = useCallback(async () => {
    if (showRaw) {
      setRawText(editedText);
    } else {
      setResult(editedText);
      // Also copy to clipboard and inject
      await navigator.clipboard.writeText(editedText);
      const injectResult = await injectText(editedText);

      // Submit feedback to learning system if text changed and user is logged in
      let feedbackLearned = false;
      if (isLoggedIn() && originalPolishedText && editedText !== originalPolishedText) {
        try {
          const response = await submitFeedback(originalPolishedText, editedText);
          feedbackLearned = response.success;
        } catch (err) {
          console.warn("Failed to submit learning feedback:", err);
        }
      }

      // Single consolidated toast for the save action
      if (!injectResult.success) {
        showToast(injectResult.message, "error");
      } else if (!injectResult.pasted) {
        showToast(injectResult.message, "warning");
      } else if (feedbackLearned) {
        showToast("Saved & correction learned!", "success");
      }
    }
    setIsEditing(false);
    setEditedText("");
    setOriginalPolishedText("");
  }, [showRaw, editedText, originalPolishedText, showToast]);

  const handleCancelEdit = useCallback(() => {
    setIsEditing(false);
    setEditedText("");
  }, []);

  // Apply a preset
  const handlePresetSelect = useCallback((preset: Preset) => {
    setContext(preset.context);
    setFormality(preset.formality);
    showToast(`Applied "${preset.name}" preset`, "info");
  }, [showToast]);

  // Regenerate with different settings
  const handleRegenerate = useCallback(async (newContext?: string, newFormality?: "casual" | "neutral" | "formal") => {
    if (!rawText) {
      showToast("No text to regenerate", "warning");
      return;
    }

    setIsRegenerating(true);
    const targetContext = newContext || context;
    const targetFormality = newFormality || formality;

    try {
      const response = await polish(rawText, {
        context: targetContext === "general" ? undefined : targetContext,
        formality: targetFormality,
      });

      setResult(response.text);

      // Update history with new version
      addToHistory({
        rawText,
        polishedText: response.text,
        context: targetContext,
        formality: targetFormality,
      });

      // Auto-inject the new text — single toast for the whole operation
      const injectResult = await injectText(response.text);
      if (!injectResult.success) {
        showToast(injectResult.message, "error");
      } else if (!injectResult.pasted) {
        showToast(injectResult.message, "warning");
      } else {
        showToast("Text regenerated and pasted!", "success");
      }
    } catch (err) {
      const message = err instanceof ApiError ? err.detail || err.message : "Failed to regenerate";
      showToast(message, "error");
    } finally {
      setIsRegenerating(false);
    }
  }, [rawText, context, formality, confidence, showToast, addToHistory]);

  const handleCopy = useCallback(async () => {
    if (result) {
      await navigator.clipboard.writeText(result);
      setCopied(true);
      showToast("Copied to clipboard!", "success");
      setTimeout(() => setCopied(false), 2000);
    }
  }, [result, showToast]);

  // Clear results handler
  const handleClear = useCallback(() => {
    setResult("");
    setRawText("");
    setConfidence(null);
    setStatus("idle");
    showToast("Cleared", "info");
  }, [showToast]);

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
        <h1>Vaak</h1>
        <div className="header-actions">
          {/* Voice Queue Panel Toggle */}
          <button
            className={`voice-toggle-btn ${voiceEnabled ? "enabled" : "disabled"}`}
            title="Voice Queue (click to open)"
            aria-label={voiceEnabled ? "Voice queue enabled, click to open panel" : "Voice queue disabled, click to open panel"}
            onClick={() => setShowQueuePanel(!showQueuePanel)}
          >
            {voiceEnabled ? <VolumeOnIcon /> : <VolumeOffIcon />}
          </button>
          {isLoggedIn() && (
            <button className="learning-btn" title="Learning Dashboard" aria-label="Open learning dashboard" onClick={() => setShowLearning(true)}>
              <BookIcon />
            </button>
          )}
          {isLoggedIn() && (
            <button className="stats-btn" title="Statistics" aria-label="Open statistics panel" onClick={() => setShowStats(true)}>
              <BarChartIcon />
            </button>
          )}
          {isLoggedIn() && (
            <button className="history-btn" title="History" aria-label="Open transcript history" onClick={() => setShowHistory(true)}>
              <ClockIcon />
            </button>
          )}
          {isLoggedIn() && (
            <button className="claude-outputs-btn" title="Claude Conversations" aria-label="Open Claude conversations viewer" onClick={() => setShowClaudeOutputs(true)}>
              <ChatBubbleIcon />
            </button>
          )}
          <ScreenReaderButton />
          <button
            className="claude-integration-btn"
            title="Claude Integration"
            aria-label="Open Claude integration panel"
            onClick={async () => {
              const { invoke } = await import("@tauri-apps/api/core");
              await invoke("toggle_transcript_window");
            }}
          >
            <SunIcon />
            <span className="claude-btn-label">Claude</span>
          </button>
          <button className="settings-btn" title="Settings" aria-label="Open settings" onClick={() => setShowSettings(true)}>
            <GearIcon />
          </button>
        </div>
      </header>

      {error && (
        <div className="error-message">
          <span>{error.message}</span>
          {error.action && (
            <button className="error-action-btn" onClick={error.action.onClick}>
              {error.action.label}
            </button>
          )}
        </div>
      )}

      <div className={`recording-section ${recorder.isRecording ? 'is-recording' : ''}`}>
        {/* Sound Toggle */}
        <button
          className={`sound-toggle ${!soundEnabled ? "muted" : ""}`}
          onClick={() => setSoundEnabled(!soundEnabled)}
          title={soundEnabled ? "Mute sounds" : "Unmute sounds"}
          aria-label={soundEnabled ? "Mute sounds" : "Unmute sounds"}
        >
          {soundEnabled ? (
            <VolumeOnIcon size={16} />
          ) : (
            <VolumeOffIcon size={16} />
          )}
        </button>

        <button
          className={`record-btn ${recorder.isRecording ? "recording" : ""}`}
          onClick={handleRecordClick}
          disabled={status === "processing" || backendReady === false}
          title={recorder.isRecording ? "Stop recording" : "Start recording"}
          aria-label={recorder.isRecording ? "Stop recording" : "Start recording"}
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

        {status === "processing" ? (
          <div className="progress-steps">
            {PROCESSING_STEPS.map((step, index) => {
              const stepIndex = PROCESSING_STEPS.findIndex(s => s.key === processingStep);
              const isActive = step.key === processingStep;
              const isComplete = index < stepIndex || processingStep === "done";
              const isCurrent = isActive && processingStep !== "done";

              return (
                <div key={step.key} className={`progress-step ${isComplete ? "complete" : ""} ${isCurrent ? "current" : ""}`}>
                  <div className="progress-step-indicator">
                    {isComplete ? (
                      <CheckIcon size={12} strokeWidth={3} />
                    ) : (
                      <span>{index + 1}</span>
                    )}
                  </div>
                  <span className="progress-step-label">{step.label}</span>
                </div>
              );
            })}
          </div>
        ) : (
          <div className="status">
            <span
              className={`status-dot ${status === "recording" ? "recording" : status === "success" ? "success" : ""}`}
            />
            {getStatusText()}
          </div>
        )}

        <p className="record-hint">
          Click to {recorder.isRecording ? "stop" : "start"} • Hold{" "}
          <span className="hotkey">{formatHotkeyDisplay(currentHotkey)}</span> for push-to-talk
          <span className={`hotkey-status ${hotkeyRegistered ? "active" : "inactive"}`}>
            {hotkeyRegistered ? "Ready" : "Restart app to enable"}
          </span>
          {" "} • Press <span className="hotkey">?</span> for shortcuts
        </p>
      </div>

      {/* Quick Mode Presets */}
      <div className="presets-section">
        <span className="presets-label">Quick Modes:</span>
        <div className="presets-list">
          {PRESETS.map((preset) => (
            <button
              key={preset.id}
              className={`preset-btn ${context === preset.context && formality === preset.formality ? "active" : ""}`}
              onClick={() => handlePresetSelect(preset)}
              title={preset.description}
            >
              <span className="preset-icon">{preset.icon}</span>
              <span className="preset-name">{preset.name}</span>
            </button>
          ))}
        </div>
      </div>

      <div className="context-selector">
        <label>Context:</label>
        <select value={context} onChange={(e) => setContext(e.target.value)}>
          {CONTEXT_OPTIONS.map((opt) => (
            <option key={opt.value} value={opt.value}>
              {opt.label}
            </option>
          ))}
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

        {transcriptHistory.length > 0 && (
          <button
            className="export-history-btn"
            onClick={() => setShowExport(true)}
            title="Export transcript history"
          >
            <DownloadIcon />
            Export ({transcriptHistory.length})
          </button>
        )}
      </div>

      <div className="result-section">
        <div className="result-header">
          <div className="result-title-row">
            <h2>
              {showRaw ? "Raw Transcription" : "Polished Text"}
              {rawText && (
                <button
                  onClick={() => setShowRaw(!showRaw)}
                  className="toggle-view-btn"
                >
                  Show {showRaw ? "polished" : "raw"}
                </button>
              )}
            </h2>
            {/* Confidence badge removed — previous implementation used fabricated scores */}
          </div>
          <div className="result-actions">
            {rawText && result && !isEditing && (
              <button
                className="compare-btn"
                onClick={() => setShowComparison(true)}
                title="Compare raw vs polished text"
              >
                <CompareIcon />
                Compare
              </button>
            )}
            {rawText && !isEditing && (
              <button
                className={`regenerate-btn ${isRegenerating ? "loading" : ""}`}
                onClick={() => handleRegenerate()}
                disabled={isRegenerating}
                title="Re-polish with current settings"
              >
                {isRegenerating ? (
                  <div className="btn-spinner" />
                ) : (
                  <RefreshIcon />
                )}
                Regenerate
              </button>
            )}
            {(result || rawText) && !isEditing && (
              <button
                className="edit-btn"
                onClick={handleStartEditing}
                title="Edit the text manually"
              >
                <EditIcon />
                Edit
              </button>
            )}
            {result && !isEditing && (
              <button
                className={`copy-btn ${copied ? "copied" : ""}`}
                onClick={handleCopy}
                title="Copy to clipboard"
              >
                {copied ? (
                  <>
                    <CheckIcon />
                    Copied
                  </>
                ) : (
                  <>
                    <CopyIcon />
                    Copy
                  </>
                )}
              </button>
            )}
            {(result || rawText) && !isEditing && (
              <button
                className="clear-btn"
                onClick={handleClear}
                title="Clear the current result"
              >
                <XIcon />
                Clear
              </button>
            )}
          </div>
        </div>
        {isEditing ? (
          <div className="result-edit-container">
            <textarea
              ref={editTextareaRef}
              className="result-edit-textarea"
              value={editedText}
              onChange={(e) => setEditedText(e.target.value)}
              placeholder="Edit your text..."
            />
            <div className="edit-stats">
              <span>{editedText.length} characters</span>
              <span>{editedText.trim() ? editedText.trim().split(/\s+/).length : 0} words</span>
            </div>
            <div className="result-edit-actions">
              <button className="save-edit-btn" onClick={handleSaveEdit}>
                <CheckIcon />
                Save & Copy
              </button>
              <button className="cancel-edit-btn" onClick={handleCancelEdit}>
                Cancel
              </button>
            </div>
          </div>
        ) : (
          <div className={`result-text ${!result && !rawText ? "empty" : ""}`}>
            {showRaw ? rawText : result || (
              <div className="welcome-state" role="status">
                <div className="welcome-icon" aria-hidden="true">🎙️</div>
                <div className="welcome-title">Ready to transcribe</div>
                <div className="welcome-hint">
                  Click the record button or hold <span className="hotkey">{formatHotkeyDisplay(currentHotkey)}</span> to start speaking.
                  <br />Your words will be transcribed and polished automatically.
                </div>
              </div>
            )}
          </div>
        )}
      </div>

      {showSettings && <Settings
        onClose={() => setShowSettings(false)}
        onHotkeyChange={handleHotkeyChange}
        refreshTrigger={transcriptionCount}
        onViewStats={() => {
          setShowSettings(false);
          setShowStats(true);
        }}
      />}
      {showStats && <StatsPanel onClose={() => setShowStats(false)} refreshTrigger={transcriptionCount} />}
      {showHistory && <TranscriptHistory onClose={() => setShowHistory(false)} refreshTrigger={transcriptionCount} />}
      {showClaudeOutputs && <ClaudeOutputsViewer onClose={() => setShowClaudeOutputs(false)} refreshTrigger={transcriptionCount} />}
      {showShortcuts && <KeyboardShortcutsModal onClose={() => setShowShortcuts(false)} />}
      {showLearning && (
        <>
          <div className="dashboard-overlay" onClick={() => setShowLearning(false)} />
          <LearningDashboard
            apiUrl={getApiBaseUrl()}
            token={getAuthToken() || ""}
            onClose={() => setShowLearning(false)}
          />
        </>
      )}
      {showComparison && rawText && result && (
        <ComparisonView
          rawText={rawText}
          polishedText={result}
          onClose={() => setShowComparison(false)}
        />
      )}
      {showExport && (
        <ExportModal
          history={transcriptHistory}
          onClose={() => setShowExport(false)}
        />
      )}

      {/* Floating audio indicator for push-to-talk */}
      <AudioIndicator
        isRecording={recorder.isRecording}
        isProcessing={status === "processing"}
        audioLevel={recorder.audioLevel}
        onCancel={handleCancelRecording}
      />

      {/* Confetti for achievement celebrations */}
      <Confetti isActive={showConfetti} onComplete={() => setShowConfetti(false)} />

      {/* Queue Slide-Out Panel */}
      <QueueSlidePanel
        isOpen={showQueuePanel}
        onClose={() => setShowQueuePanel(false)}
        voiceEnabled={voiceEnabled}
        onVoiceToggle={handleVoiceToggle}
      />
    </div>
  );
}

export default App;
