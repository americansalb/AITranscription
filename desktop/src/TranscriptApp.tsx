import { useState, useEffect, useCallback } from "react";
import {
  loadSessions,
  saveSessions,
  getOrCreateSession,
  addMessageToSession,
  updateSessionHeartbeat,
  createSessionFromHeartbeat,
  type Session,
  type SpeakMessage,
} from "./lib/sessionManager";
import { transcriptListener } from "./lib/transcriptListener";
import { CollabTab } from "./components/CollabTab";
import { PreferencesTab } from "./components/PreferencesTab";
import { SessionsPanel } from "./components/SessionsPanel";
import {
  getStoredVoiceEnabled,
  saveVoiceEnabled,
  getStoredBlindMode,
  saveBlindMode,
  getStoredVoiceDetail,
  saveVoiceDetail,
  getStoredVoiceAuto,
  saveVoiceAuto,
} from "./lib/voiceStream";
import {
  updateSessionCache,
  getVoiceAssignments,
} from "./lib/queueStore";

// Tab type for navigation
type TabType = "preferences" | "sessions" | "collab";


export function TranscriptApp() {
  const [sessions, setSessions] = useState<Session[]>(() => loadSessions());
  const [activeTab, setActiveTab] = useState<TabType>("preferences");

  // Voice settings state (kept here for CLAUDE.md sync and session cache)
  const [voiceEnabled, setVoiceEnabled] = useState(() => getStoredVoiceEnabled());
  const [blindMode, setBlindMode] = useState(() => getStoredBlindMode());
  const [voiceDetail, setVoiceDetail] = useState(() => getStoredVoiceDetail());
  const [voiceAuto, setVoiceAuto] = useState(() => getStoredVoiceAuto());
  const [voiceAssignments] = useState<Record<string, string>>(() => getVoiceAssignments());

  // Voice settings handlers
  const handleVoiceEnabledChange = useCallback(async (enabled: boolean) => {
    setVoiceEnabled(enabled);
    saveVoiceEnabled(enabled);

    // If disabling voice, clear pending items from queue
    if (!enabled) {
      const { clearPending } = await import("./lib/queueStore");
      await clearPending();
    }

    // Update CLAUDE.md file
    if (window.__TAURI__) {
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        const { emit } = await import("@tauri-apps/api/event");
        await invoke("update_claude_md", { enabled, blindMode, detail: voiceDetail });

        // Emit event to sync other windows
        await emit("voice-settings-changed", {
          voiceEnabled: enabled,
          blindMode,
          detail: voiceDetail
        });
      } catch (e) {
        console.error("Failed to update CLAUDE.md:", e);
      }
    }
  }, [blindMode, voiceDetail]);

  const handleBlindModeChange = useCallback(async (enabled: boolean) => {
    setBlindMode(enabled);
    saveBlindMode(enabled);

    if (window.__TAURI__) {
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        const { emit } = await import("@tauri-apps/api/event");
        await invoke("save_voice_settings_cmd", { enabled: voiceEnabled, blindMode: enabled, detail: voiceDetail });
        await invoke("update_claude_md", { enabled: voiceEnabled, blindMode: enabled, detail: voiceDetail });

        // Emit event to sync other windows
        await emit("voice-settings-changed", {
          voiceEnabled,
          blindMode: enabled,
          detail: voiceDetail
        });
      } catch (e) {
        console.error("Failed to save voice settings:", e);
      }
    }
  }, [voiceEnabled, voiceDetail]);

  const handleVoiceDetailChange = useCallback(async (detail: number) => {
    setVoiceDetail(detail);
    saveVoiceDetail(detail);

    if (window.__TAURI__) {
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        const { emit } = await import("@tauri-apps/api/event");
        await invoke("save_voice_settings_cmd", { enabled: voiceEnabled, blindMode, detail });
        await invoke("update_claude_md", { enabled: voiceEnabled, blindMode, detail });

        // Emit event to sync other windows
        await emit("voice-settings-changed", {
          voiceEnabled,
          blindMode,
          detail
        });
      } catch (e) {
        console.error("Failed to save voice settings:", e);
      }
    }
  }, [voiceEnabled, blindMode]);

  const handleVoiceAutoChange = useCallback((auto: boolean) => {
    setVoiceAuto(auto);
    saveVoiceAuto(auto);
  }, []);

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
            console.log("[TranscriptApp] Received voice-settings-changed event:", event.payload);
            setVoiceEnabled(event.payload.voiceEnabled);
            setBlindMode(event.payload.blindMode);
            setVoiceDetail(event.payload.detail);
          }
        );
      } catch (err) {
        console.error("[TranscriptApp] Failed to setup voice settings listener:", err);
      }
    };

    setupListener();

    return () => {
      if (unlisten) unlisten();
    };
  }, []);

  // Save sessions whenever they change + sync session cache for queue (Feature 8)
  useEffect(() => {
    saveSessions(sessions);
    // Update queueStore session cache so queue items get session name/color
    updateSessionCache(sessions.map(s => ({
      id: s.id,
      name: s.name,
      color: s.color,
      voiceId: voiceAssignments[s.id],
    })));
    // Sync session names to backend so CollabTab (main window) can access them
    if (sessions.length > 0) {
      fetch("http://127.0.0.1:7865/sessions/names", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ sessions: sessions.map(s => ({ id: s.id, name: s.name })) }),
      }).catch((e) => console.debug("[TranscriptApp] Session sync skipped (backend not running):", e));
    }
  }, [sessions, voiceAssignments]);

  // Initialize project path and sync CLAUDE.md on mount
  useEffect(() => {
    const initProjectPath = async () => {
      if (!window.__TAURI__) return;

      try {
        const { invoke } = await import("@tauri-apps/api/core");
        // @tauri-apps/api/path is imported below where needed

        // Get the project directory - this is the AITranscription directory
        // We need to go up from the app data dir to find the project root
        // For development, use the current working directory approach
        try {
          // Try to get the resource directory first (where the app is running from)
          const { resourceDir } = await import("@tauri-apps/api/path");
          const resDir = await resourceDir();
          // Go up from resources to find project root
          // In dev: resources is in target/debug or target/release
          // We want the AITranscription folder which contains desktop/
          const projectPath = resDir.replace(/[\\/]desktop[\\/]src-tauri[\\/]target[\\/].*$/, "");

          if (projectPath && projectPath !== resDir) {
            await invoke("set_project_path", { path: projectPath });
            console.log("[TranscriptApp] Set project path to:", projectPath);
          } else {
            console.warn("[TranscriptApp] Could not detect project root from resourceDir:", resDir);
          }
        } catch (pathErr) {
          console.warn("[TranscriptApp] Could not resolve project path:", pathErr);
        }

        // After setting project path, sync CLAUDE.md with current settings
        await invoke("update_claude_md", {
          enabled: voiceEnabled,
          blindMode,
          detail: voiceDetail
        });
        console.log("[TranscriptApp] Synced CLAUDE.md with current settings");

      } catch (err) {
        console.error("[TranscriptApp] Failed to initialize project path:", err);
      }
    };

    initProjectPath();
  }, []); // Run once on mount

  // Listen for speak events from backend using singleton listener
  // IMPORTANT: This window does NOT play audio - only updates UI
  // Audio playback happens in main window only
  useEffect(() => {
    console.log("[TranscriptApp] Setting up listener callback");

    // Define the callback for processing messages
    const handleMessage = (message: SpeakMessage) => {
      setSessions((currentSessions) => {
        console.log(`[TranscriptApp] Processing message. Current sessions: ${currentSessions.length}`);

        const { session, isNew } = getOrCreateSession(message, currentSessions);
        const updatedSession = addMessageToSession(session, message);

        if (isNew) {
          // DEFENSIVE CHECK: Verify this session ID doesn't already exist
          const duplicate = currentSessions.find(s => s.id === updatedSession.id);
          if (duplicate) {
            console.error(`[TranscriptApp] CRITICAL BUG: Tried to create duplicate session ${updatedSession.id}. Updating existing instead.`);
            return currentSessions.map((s) =>
              s.id === updatedSession.id ? updatedSession : s
            );
          }

          // New session - add to list (with heartbeat set)
          const newSessions = [{ ...updatedSession, lastHeartbeat: Date.now() }, ...currentSessions];
          console.log(`[TranscriptApp] NEW session created: ${updatedSession.name} (${updatedSession.id}). Total: ${newSessions.length}`);

          return newSessions;
        } else {
          // Update existing session (also update heartbeat on message)
          console.log(`[TranscriptApp] UPDATED existing session: ${updatedSession.name} (${updatedSession.id})`);
          return currentSessions.map((s) =>
            s.id === session.id ? { ...updatedSession, lastHeartbeat: Date.now() } : s
          );
        }
      });
    };

    // Define the callback for processing heartbeats
    const handleHeartbeat = (sessionId: string, timestamp: number) => {
      setSessions((currentSessions) => {
        const existingSession = currentSessions.find(s => s.id === sessionId);
        if (existingSession) {
          console.log(`[TranscriptApp] Heartbeat for existing session: ${existingSession.name}`);
          return updateSessionHeartbeat(currentSessions, sessionId, timestamp);
        } else {
          // Create a new session from heartbeat so it appears in the sessions list
          const newSession = createSessionFromHeartbeat(sessionId, currentSessions, timestamp);
          console.log(`[TranscriptApp] New session created from heartbeat: ${newSession.name} (${sessionId})`);
          return [newSession, ...currentSessions];
        }
      });
    };

    // Initialize or update the singleton listener
    transcriptListener.initialize(handleMessage, handleHeartbeat);

    // Cleanup: only update callback to null, don't destroy listener
    return () => {
      console.log("[TranscriptApp] Component unmounting, clearing callback");
      transcriptListener.updateCallback(() => {});
    };
  }, []); // Empty dependency array - only run once on mount

  return (
    <div className="transcript-app">
      {/* Main Tab Navigation */}
      <div className="main-tab-nav" role="tablist" aria-label="Main navigation">
        <button
          className={`main-tab ${activeTab === "preferences" ? "active" : ""}`}
          onClick={() => setActiveTab("preferences")}
          role="tab"
          aria-selected={activeTab === "preferences"}
          aria-controls="panel-preferences"
          tabIndex={activeTab === "preferences" ? 0 : -1}
          onKeyDown={(e) => {
            if (e.key === "ArrowRight") { e.preventDefault(); setActiveTab("sessions"); }
            if (e.key === "ArrowLeft") { e.preventDefault(); setActiveTab("collab"); }
          }}
        >
          Preferences
        </button>
        <button
          className={`main-tab ${activeTab === "sessions" ? "active" : ""}`}
          onClick={() => setActiveTab("sessions")}
          role="tab"
          aria-selected={activeTab === "sessions"}
          aria-controls="panel-sessions"
          tabIndex={activeTab === "sessions" ? 0 : -1}
          onKeyDown={(e) => {
            if (e.key === "ArrowRight") { e.preventDefault(); setActiveTab("collab"); }
            if (e.key === "ArrowLeft") { e.preventDefault(); setActiveTab("preferences"); }
          }}
        >
          Sessions
        </button>
        <button
          className={`main-tab ${activeTab === "collab" ? "active" : ""}`}
          onClick={() => setActiveTab("collab")}
          role="tab"
          aria-selected={activeTab === "collab"}
          aria-controls="panel-collab"
          tabIndex={activeTab === "collab" ? 0 : -1}
          onKeyDown={(e) => {
            if (e.key === "ArrowRight") { e.preventDefault(); setActiveTab("preferences"); }
            if (e.key === "ArrowLeft") { e.preventDefault(); setActiveTab("sessions"); }
          }}
        >
          Collab
        </button>
      </div>

      {/* Preferences Tab */}
      {activeTab === "preferences" && (
        <PreferencesTab
          voiceEnabled={voiceEnabled}
          blindMode={blindMode}
          voiceDetail={voiceDetail}
          voiceAuto={voiceAuto}
          onVoiceEnabledChange={handleVoiceEnabledChange}
          onBlindModeChange={handleBlindModeChange}
          onVoiceDetailChange={handleVoiceDetailChange}
          onVoiceAutoChange={handleVoiceAutoChange}
        />
      )}

      {/* Collab Tab — always mounted so event listeners stay active */}
      <div id="panel-collab" role="tabpanel" aria-labelledby="tab-collab" style={{ display: activeTab === "collab" ? "flex" : "none", flex: 1, minHeight: 0 }}>
        <CollabTab />
      </div>

      {/* Sessions Tab */}
      {activeTab === "sessions" && (
        <SessionsPanel sessions={sessions} onSessionsChange={setSessions} />
      )}
    </div>
  );
}
