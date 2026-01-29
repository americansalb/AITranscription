import { useState, useEffect, useCallback, useRef } from "react";
import {
  loadSessions,
  saveSessions,
  getOrCreateSession,
  addMessageToSession,
  renameSession,
  changeSessionColor,
  updateSessionHeartbeat,
  isSessionActive,
  clearSessionMessages,
  deleteSession,
  getRelativeTime,
  formatTime,
  SESSION_COLORS,
  type Session,
  type SpeakMessage,
} from "./lib/sessionManager";
import { transcriptListener } from "./lib/transcriptListener";
import { QueueTab } from "./components/QueueTab";
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

// Tab type for navigation
type TabType = "preferences" | "sessions" | "queue";

// Detail level labels
const DETAIL_LABELS = ['Summary', '', 'Balanced', '', 'Developer'];

// Structured instruction data for preview rendering
interface InstructionPreviewData {
  detailLevel: {
    name: string;
    position: string;
    description: string;
    example: string;
  };
  mode: {
    name: string;
    description: string;
  };
  rules: {
    always: string[];
    never: string[];
  };
  allLevels: { level: number; name: string; brief: string }[];
}

function getInstructionPreviewData(blindMode: boolean, detail: number): InstructionPreviewData {
  const detailLevels: Record<number, { name: string; position: string; description: string; example: string }> = {
    1: {
      name: "Summary",
      position: "MINIMUM detail (1 of 5)",
      description: "Be as brief as humanly possible. One short sentence max. No technical terms. A child should understand it.",
      example: "I updated the login page."
    },
    2: {
      name: "Brief",
      position: "LOW detail (2 of 5)",
      description: "Keep it to 1-2 simple sentences. Mention what changed and why, nothing more.",
      example: "I fixed the login button. It wasn't responding to clicks because of a missing event handler."
    },
    3: {
      name: "Balanced",
      position: "MEDIUM detail (3 of 5)",
      description: "Include the file name, what you changed, and why. A few sentences is fine. Balance clarity with brevity.",
      example: "I modified LoginForm.tsx to fix the submit button. The onClick handler was missing, so I added one that calls the authentication API when clicked."
    },
    4: {
      name: "Detailed",
      position: "HIGH detail (4 of 5)",
      description: "Be thorough. Include file names, line numbers, technical details, and explain the implications of your changes.",
      example: "I modified LoginForm.tsx at line 45. The submit button had no click handler, causing the form to not submit. I added an async onClick handler that validates the form fields, calls the /api/auth/login endpoint, and redirects to the dashboard on success."
    },
    5: {
      name: "Developer",
      position: "MAXIMUM detail (5 of 5)",
      description: "Give a comprehensive technical breakdown. Mention every file you touched, explain architecture decisions, cover edge cases, and describe implementation specifics.",
      example: "I made changes to three files to fix the authentication flow. In LoginForm.tsx, I added form validation using Zod at line 23, an async submit handler at line 45 with error handling. In api/auth.ts, I added retry logic. In types/auth.ts, I added the LoginResponse interface."
    }
  };

  const currentDetail = detailLevels[detail] || detailLevels[3];

  const mode = blindMode ? {
    name: "Screen Reader Mode",
    description: "The user cannot see the screen. Describe ALL visual information: where things are positioned, colors, spacing, layout structure, and spatial relationships between elements."
  } : {
    name: "Standard Mode",
    description: "The user can see the screen. Focus on explaining what you did and why, without describing visual layouts."
  };

  const rules = blindMode ? {
    always: [
      "Say the full file path when you modify a file",
      "Describe where UI elements are positioned (top-right, centered, below the header)",
      "Mention colors, sizes, and spacing when relevant",
      "Explain the visual hierarchy and structure of code",
      "Describe what's above, below, and beside changed elements"
    ],
    never: [
      "Read code syntax character by character",
      "Assume the user can see anything on screen",
      "Skip describing the location of changes",
      "Use vague terms like 'here' or 'this' without context"
    ]
  } : {
    always: [
      "Say the file name when you modify a file",
      "Explain what you changed and why",
      "Mention if you created new files or functions",
      "Summarize the purpose of bug fixes"
    ],
    never: [
      "Read entire code blocks out loud",
      "Spell out syntax like brackets and semicolons",
      "Describe visual layouts in detail (user can see)",
      "Give lengthy explanations for simple changes"
    ]
  };

  const allLevels = [
    { level: 1, name: "Summary", brief: "One sentence only" },
    { level: 2, name: "Brief", brief: "1-2 sentences" },
    { level: 3, name: "Balanced", brief: "File names + why" },
    { level: 4, name: "Detailed", brief: "Technical details" },
    { level: 5, name: "Developer", brief: "Full breakdown" },
  ];

  return { detailLevel: currentDetail, mode, rules, allLevels };
}

// Generate the ACTUAL raw CLAUDE.md content (must match Rust generate_voice_template)
function generateRawClaudeMd(blindMode: boolean, detail: number): string {
  const baseInstruction = `# Claude Code Instructions

## Voice Output

Always use the Scribe speak integration to read responses aloud.

**CRITICAL: Use the MCP \`/speak\` tool - NOT curl**

The \`/speak\` tool is available through the MCP server. It automatically manages stable session IDs based on your terminal process. Simply call the speak tool using MCP:

The session ID is handled automatically - all messages from this terminal will be grouped together in the same conversation.

**Session Management:**
- Each terminal window gets a unique session ID automatically (based on process ID)
- All Claude instances in the same terminal share the same session
- You don't need to track or pass session IDs manually
- NEVER use curl to call the speak endpoint directly

**How it works:**
- Session ID format: \`{hostname}-{parent_process_id}\`
- Same terminal = Same parent PID = Same session
- Different terminal = Different parent PID = Different session
`;

  const detailDescriptions: Record<number, string> = {
    1: "This is the MINIMUM detail. Be as brief as humanly possible. One short sentence max. No technical terms. A child should understand it.",
    2: "This is LOW detail. Keep it to 1-2 simple sentences. Mention what changed and why, nothing more.",
    3: "This is MEDIUM detail. Include the file name, what you changed, and why. A few sentences is fine. Balance clarity with brevity.",
    4: "This is HIGH detail. Be thorough. Include file names, line numbers, technical details, and explain the implications of your changes.",
    5: "This is MAXIMUM detail. Give a comprehensive technical breakdown. Mention every file you touched, explain your architecture decisions, cover edge cases, and describe implementation specifics. Developers want the full picture.",
  };

  const detailScale = `
## Detail Level: ${detail} out of 5

THE FULL SCALE (so you understand the range):
- Level 1 (Minimum): One sentence only. "I updated the login page."
- Level 2: 1-2 sentences. "I fixed the login button - the click handler was missing."
- Level 3 (Middle): Mention file names and explain why. "I modified LoginForm.tsx to fix the submit button by adding the missing onClick handler."
- Level 4: Include line numbers, technical details, and implications.
- Level 5 (Maximum): Full technical breakdown with architecture decisions, edge cases, all files touched, and implementation specifics.

YOU ARE AT LEVEL ${detail}: ${detailDescriptions[detail] || detailDescriptions[3]}
`;

  const modeInstructions = blindMode ? `
${detailScale}
## Mode: Screen Reader

The user CANNOT see the screen. You MUST describe all visual information.

### ALWAYS do these things:
- Say the full file path when you modify a file
- Describe where UI elements are positioned (top-right, centered, below the header)
- Mention colors, sizes, and spacing when relevant
- Explain the visual hierarchy and structure of code
- Describe what's above, below, and beside changed elements

### NEVER do these things:
- Read code syntax character by character
- Assume the user can see anything on screen
- Skip describing the location of changes
- Use vague terms like "here" or "this" without context
` : `
${detailScale}
## Mode: Standard

The user can see the screen. Focus on explaining what you did and why.

### ALWAYS do these things:
- Say the file name when you modify a file
- Explain what you changed and why
- Mention if you created new files or functions
- Summarize the purpose of bug fixes

### NEVER do these things:
- Read entire code blocks out loud
- Spell out syntax like brackets and semicolons
- Describe visual layouts in detail (user can see)
- Give lengthy explanations for simple changes
`;

  return baseInstruction + modeInstructions;
}

// Preview component for nice rendering
function InstructionPreview({ blindMode, detail }: { blindMode: boolean; detail: number }) {
  const [showRaw, setShowRaw] = useState(false);
  const data = getInstructionPreviewData(blindMode, detail);

  return (
    <div className="instruction-preview-formatted">
      {/* Toggle between formatted and raw view */}
      <div className="preview-view-toggle">
        <button
          className={`preview-toggle-btn ${!showRaw ? 'active' : ''}`}
          onClick={() => setShowRaw(false)}
        >
          Formatted
        </button>
        <button
          className={`preview-toggle-btn ${showRaw ? 'active' : ''}`}
          onClick={() => setShowRaw(true)}
        >
          Raw CLAUDE.md
        </button>
      </div>

      {showRaw ? (
        /* Raw CLAUDE.md content */
        <div className="preview-raw-content">
          <div className="preview-raw-note">
            This is the exact text written to CLAUDE.md that Claude reads:
          </div>
          <pre className="preview-raw-text">{generateRawClaudeMd(blindMode, detail)}</pre>
        </div>
      ) : (
        /* Formatted view */
        <>
          <div className="preview-section">
            <div className="preview-section-header">
              <span className="preview-icon">🎯</span>
              <span className="preview-section-title">Current Mode</span>
            </div>
            <div className="preview-mode-card">
              <div className="preview-mode-name">{data.mode.name}</div>
              <div className="preview-mode-desc">{data.mode.description}</div>
            </div>
          </div>

          <div className="preview-section">
            <div className="preview-section-header">
              <span className="preview-icon">📊</span>
              <span className="preview-section-title">Detail Level: {data.detailLevel.position}</span>
            </div>

            {/* Visual scale showing all levels */}
            <div className="preview-scale">
              {data.allLevels.map((lvl) => (
                <div
                  key={lvl.level}
                  className={`preview-scale-item ${lvl.level === detail ? 'active' : ''}`}
                >
                  <div className="preview-scale-number">{lvl.level}</div>
                  <div className="preview-scale-name">{lvl.name}</div>
                  <div className="preview-scale-brief">{lvl.brief}</div>
                </div>
              ))}
            </div>

            <div className="preview-detail-card">
              <div className="preview-detail-label">Claude is told:</div>
              <div className="preview-detail-desc">"{data.detailLevel.description}"</div>
              <div className="preview-example">
                <div className="preview-example-label">Example response at this level:</div>
                <div className="preview-example-text">"{data.detailLevel.example}"</div>
              </div>
            </div>
          </div>

          <div className="preview-section">
            <div className="preview-section-header">
              <span className="preview-icon">✅</span>
              <span className="preview-section-title">Claude Will Always</span>
            </div>
            <ul className="preview-rules-list preview-rules-always">
              {data.rules.always.map((rule, i) => (
                <li key={i}>{rule}</li>
              ))}
            </ul>
          </div>

          <div className="preview-section">
            <div className="preview-section-header">
              <span className="preview-icon">🚫</span>
              <span className="preview-section-title">Claude Will Never</span>
            </div>
            <ul className="preview-rules-list preview-rules-never">
              {data.rules.never.map((rule, i) => (
                <li key={i}>{rule}</li>
              ))}
            </ul>
          </div>
        </>
      )}
    </div>
  );
}

export function TranscriptApp() {
  const [sessions, setSessions] = useState<Session[]>(() => loadSessions());
  const [selectedSessionId, setSelectedSessionId] = useState<string | null>(null);
  const [editingSessionId, setEditingSessionId] = useState<string | null>(null);
  const [editingName, setEditingName] = useState("");
  const [colorPickerSessionId, setColorPickerSessionId] = useState<string | null>(null);
  const [filter, setFilter] = useState<"all" | "active">("all");
  const [searchQuery, setSearchQuery] = useState("");
  const [activeTab, setActiveTab] = useState<TabType>("preferences");
  const messagesEndRef = useRef<HTMLDivElement>(null);

  // Voice settings state
  const [voiceEnabled, setVoiceEnabled] = useState(() => getStoredVoiceEnabled());
  const [blindMode, setBlindMode] = useState(() => getStoredBlindMode());
  const [voiceDetail, setVoiceDetail] = useState(() => getStoredVoiceDetail());
  const [voiceAuto, setVoiceAuto] = useState(() => getStoredVoiceAuto());

  // Preview panel state
  const [showInstructionPreview, setShowInstructionPreview] = useState(false);

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
        await invoke("save_voice_settings_cmd", { blindMode: enabled, detail: voiceDetail });
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
        await invoke("save_voice_settings_cmd", { blindMode, detail });
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
  });

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

  // Save sessions whenever they change
  useEffect(() => {
    saveSessions(sessions);
  }, [sessions]);

  // Initialize project path and sync CLAUDE.md on mount
  useEffect(() => {
    const initProjectPath = async () => {
      if (!window.__TAURI__) return;

      try {
        const { invoke } = await import("@tauri-apps/api/core");
        const { appDataDir, resolve } = await import("@tauri-apps/api/path");

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
            // Fallback: use a known path for this specific project
            const knownPath = "C:\\Users\\18479\\Desktop\\LOCAL APP TESTING\\AITranscription";
            await invoke("set_project_path", { path: knownPath });
            console.log("[TranscriptApp] Set project path to known path:", knownPath);
          }
        } catch (pathErr) {
          // Fallback to a known path if path resolution fails
          const knownPath = "C:\\Users\\18479\\Desktop\\LOCAL APP TESTING\\AITranscription";
          await invoke("set_project_path", { path: knownPath });
          console.log("[TranscriptApp] Set project path to fallback:", knownPath);
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

          // Auto-select new session if nothing selected
          if (!selectedSessionId) {
            setSelectedSessionId(updatedSession.id);
          }
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
          // Create a placeholder session for heartbeat from unknown session
          console.log(`[TranscriptApp] Heartbeat from new session: ${sessionId}`);
          const sessionNumber = currentSessions.length + 1;
          const newSession: Session = {
            id: sessionId,
            name: `Claude #${sessionNumber}`,
            color: SESSION_COLORS[(sessionNumber - 1) % SESSION_COLORS.length],
            messages: [],
            createdAt: timestamp,
            lastActivity: timestamp,
            lastHeartbeat: timestamp,
            isAutoNamed: true,
          };
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

  // Auto-scroll to latest message when selected session changes
  useEffect(() => {
    if (messagesEndRef.current) {
      messagesEndRef.current.scrollIntoView({ behavior: "smooth" });
    }
  }, [selectedSessionId, sessions]);

  const handleRenameSession = useCallback(
    (sessionId: string) => {
      const session = sessions.find((s) => s.id === sessionId);
      if (session) {
        setEditingSessionId(sessionId);
        setEditingName(session.name);
      }
    },
    [sessions]
  );

  const handleSaveRename = useCallback(() => {
    if (editingSessionId && editingName.trim()) {
      setSessions(renameSession(sessions, editingSessionId, editingName.trim()));
      setEditingSessionId(null);
      setEditingName("");
    }
  }, [sessions, editingSessionId, editingName]);

  const handleChangeColor = useCallback(
    (sessionId: string, newColor: string, closeAfter: boolean = false) => {
      setSessions(changeSessionColor(sessions, sessionId, newColor));
      if (closeAfter) {
        setColorPickerSessionId(null);
      }
    },
    [sessions]
  );

  const handleClearSession = useCallback(
    (sessionId: string) => {
      if (confirm("Clear all messages from this session?")) {
        setSessions(clearSessionMessages(sessions, sessionId));
      }
    },
    [sessions]
  );

  const handleDeleteSession = useCallback(
    (sessionId: string) => {
      if (confirm("Delete this session permanently?")) {
        setSessions(deleteSession(sessions, sessionId));
        if (selectedSessionId === sessionId) {
          setSelectedSessionId(null);
        }
      }
    },
    [sessions, selectedSessionId]
  );

  const handleCopyMessage = useCallback((text: string) => {
    navigator.clipboard.writeText(text);
  }, []);

  const handleCopyAllMessages = useCallback(
    (sessionId: string) => {
      const session = sessions.find((s) => s.id === sessionId);
      if (session) {
        const text = session.messages
          .slice()
          .reverse()
          .map((m) => m.text)
          .join("\n\n");
        navigator.clipboard.writeText(text);
      }
    },
    [sessions]
  );

  const filteredSessions = sessions
    .filter((session) => {
      // Active filter: sessions with heartbeat in the last 5 minutes
      if (filter === "active") {
        if (!isSessionActive(session, 5 * 60 * 1000)) return false;
      }
      if (searchQuery) {
        const query = searchQuery.toLowerCase();
        return (
          session.name.toLowerCase().includes(query) ||
          session.messages.some((m) => m.text.toLowerCase().includes(query))
        );
      }
      return true;
    })
    // Sort by most recent activity first
    .sort((a, b) => b.lastActivity - a.lastActivity);

  const selectedSession = selectedSessionId
    ? sessions.find((s) => s.id === selectedSessionId)
    : null;

  return (
    <div className="transcript-app">
      {/* Main Tab Navigation */}
      <div className="main-tab-nav">
        <button
          className={`main-tab ${activeTab === "preferences" ? "active" : ""}`}
          onClick={() => setActiveTab("preferences")}
        >
          Preferences
        </button>
        <button
          className={`main-tab ${activeTab === "sessions" ? "active" : ""}`}
          onClick={() => setActiveTab("sessions")}
        >
          Sessions
        </button>
        <button
          className={`main-tab ${activeTab === "queue" ? "active" : ""}`}
          onClick={() => setActiveTab("queue")}
        >
          Queue
        </button>
      </div>

      {/* Preferences Tab */}
      {activeTab === "preferences" && (
        <div className="preferences-tab-content">
          <div className="preferences-header">
            <h2>Claude Integration Settings</h2>
            <p className="preferences-subtitle">Configure how Claude Code speaks to you</p>
          </div>

          <div className="preferences-card">
            <div className="preference-item">
              <div className="preference-info">
                <h3>Voice Output</h3>
                <p>Hear spoken explanations when Claude Code makes changes <span style={{color: '#8899a6', fontSize: '11px'}}>(synced with main app toggle)</span></p>
              </div>
              <label className="toggle-switch-wrapper">
                <input
                  type="checkbox"
                  checked={voiceEnabled}
                  onChange={(e) => handleVoiceEnabledChange(e.target.checked)}
                />
                <span className="toggle-switch" />
              </label>
            </div>

            {voiceEnabled && (
              <>
                <div className="preference-divider" />

                <div className="preference-item">
                  <div className="preference-info">
                    <h3>Screen Reader Mode</h3>
                    <p>Detailed descriptions of visual layouts, positioning, and spatial relationships</p>
                  </div>
                  <label className="toggle-switch-wrapper">
                    <input
                      type="checkbox"
                      checked={blindMode}
                      onChange={(e) => handleBlindModeChange(e.target.checked)}
                    />
                    <span className="toggle-switch" />
                  </label>
                </div>

                <div className="preference-divider" />

                <div className="preference-item vertical">
                  <div className="preference-info">
                    <h3>Detail Level</h3>
                    <p>How much information Claude provides in voice responses</p>
                  </div>
                  <div className="detail-slider-wrapper">
                    <input
                      type="range"
                      min="1"
                      max="5"
                      step="1"
                      value={voiceDetail}
                      onChange={(e) => handleVoiceDetailChange(parseInt(e.target.value))}
                      className="detail-slider"
                    />
                    <div className="slider-labels">
                      <span className={voiceDetail === 1 ? "active" : ""}>Summary</span>
                      <span className={voiceDetail === 3 ? "active" : ""}>Balanced</span>
                      <span className={voiceDetail === 5 ? "active" : ""}>Developer</span>
                    </div>
                    <div className="current-level">
                      Current: <strong>{DETAIL_LABELS[voiceDetail - 1] || `Level ${voiceDetail}`}</strong>
                    </div>
                  </div>
                </div>

                <div className="preference-divider" />

                <div className="preference-item">
                  <div className="preference-info">
                    <h3>Automatic Announcements</h3>
                    <p>Speak automatically when Claude makes changes</p>
                  </div>
                  <label className="toggle-switch-wrapper">
                    <input
                      type="checkbox"
                      checked={voiceAuto}
                      onChange={(e) => handleVoiceAutoChange(e.target.checked)}
                    />
                    <span className="toggle-switch" />
                  </label>
                </div>
              </>
            )}
          </div>

          {/* Instruction Preview Panel */}
          {voiceEnabled && (
            <div className="instruction-preview-section">
              <button
                className={`instruction-preview-toggle ${showInstructionPreview ? "expanded" : ""}`}
                onClick={() => setShowInstructionPreview(!showInstructionPreview)}
              >
                <span className="toggle-icon">{showInstructionPreview ? "▼" : "▶"}</span>
                <span className="toggle-text">Preview Claude Instructions</span>
                <span className="toggle-hint">See what Claude will do</span>
              </button>
              {showInstructionPreview && (
                <div className="instruction-preview-content">
                  <InstructionPreview blindMode={blindMode} detail={voiceDetail} />
                </div>
              )}
            </div>
          )}

          <div className="preferences-footer">
            <p>These settings sync with your CLAUDE.md file for consistent behavior across sessions.</p>
          </div>
        </div>
      )}

      {/* Queue Tab */}
      {activeTab === "queue" && <QueueTab />}

      {/* Sessions Tab */}
      {activeTab === "sessions" && (
        <div className="sessions-content">
          {/* Sidebar - Session List */}
          <div className="transcript-sidebar">
            <div className="transcript-header">
              <div>
                <h1>Claude Sessions</h1>
                <div className="transcript-stats">
                  <span>{sessions.length} sessions</span>
                  <span>
                    {sessions.reduce((sum, s) => sum + s.messages.length, 0)} messages
                  </span>
                </div>
              </div>
              <button
                className="transcript-btn transcript-btn-danger"
                onClick={() => {
                  if (confirm("Clear all sessions and messages? This cannot be undone.")) {
                    localStorage.removeItem("scribe_speak_sessions");
                    setSessions([]);
                    setSelectedSessionId(null);
                  }
                }}
                title="Clear all sessions"
              >
                Clear All
              </button>
            </div>

            {/* Filters */}
            <div className="transcript-filters">
              <div className="filter-tabs">
                <button
                  className={filter === "all" ? "active" : ""}
                  onClick={() => setFilter("all")}
                >
                  All
                </button>
                <button
                  className={filter === "active" ? "active" : ""}
                  onClick={() => setFilter("active")}
                >
                  Active
                </button>
              </div>
          <input
            type="text"
            placeholder="Search sessions..."
            className="filter-search"
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
          />
        </div>

        {/* Session List */}
        <div className="session-list">
          {filteredSessions.length === 0 ? (
            <div className="session-list-empty">
              <div className="empty-icon">💬</div>
              <div className="empty-text">No sessions yet</div>
              <div className="empty-hint">
                Speak messages will appear here automatically
              </div>
            </div>
          ) : (
            filteredSessions.map((session) => (
              <div
                key={session.id}
                className={`session-item ${
                  selectedSessionId === session.id ? "selected" : ""
                }`}
                onClick={() => setSelectedSessionId(session.id)}
              >
                <div
                  className="session-color"
                  style={{ backgroundColor: session.color }}
                />
                <div className="session-info">
                  {editingSessionId === session.id ? (
                    <input
                      type="text"
                      className="session-name-edit"
                      value={editingName}
                      onChange={(e) => setEditingName(e.target.value)}
                      onBlur={handleSaveRename}
                      onKeyDown={(e) => {
                        if (e.key === "Enter") handleSaveRename();
                        if (e.key === "Escape") setEditingSessionId(null);
                      }}
                      onClick={(e) => e.stopPropagation()}
                      autoFocus
                    />
                  ) : (
                    <div className="session-name">{session.name}</div>
                  )}
                  <div className="session-meta">
                    <span className="session-count">
                      {session.messages.length} messages
                    </span>
                    <span className="session-time">
                      {getRelativeTime(session.lastActivity)}
                    </span>
                  </div>
                </div>
              </div>
            ))
          )}
        </div>
      </div>

      {/* Main Panel - Messages */}
      <div className="transcript-main">
        {selectedSession ? (
          <>
            <div className="transcript-main-header">
              <div className="transcript-main-title">
                <div
                  className="session-color-large"
                  style={{ backgroundColor: selectedSession.color }}
                />
                <div>
                  <h2>{selectedSession.name}</h2>
                  <div className="transcript-main-subtitle">
                    {selectedSession.messages.length} messages
                  </div>
                </div>
              </div>
              <div className="transcript-main-actions">
                <button
                  className="transcript-btn"
                  onClick={() => handleRenameSession(selectedSession.id)}
                  title="Rename session"
                >
                  ✏️ Rename
                </button>
                <div className="color-picker-container">
                  <button
                    className="transcript-btn"
                    onClick={() => setColorPickerSessionId(
                      colorPickerSessionId === selectedSession.id ? null : selectedSession.id
                    )}
                    title="Change color"
                  >
                    🎨 Color
                  </button>
                  {colorPickerSessionId === selectedSession.id && (
                    <div className="color-picker-dropdown" onClick={(e) => e.stopPropagation()}>
                      <div className="color-picker-presets">
                        {SESSION_COLORS.map((color) => (
                          <button
                            key={color}
                            className={`color-picker-option ${selectedSession.color === color ? 'selected' : ''}`}
                            style={{ backgroundColor: color }}
                            onClick={() => handleChangeColor(selectedSession.id, color, true)}
                            title={color}
                          />
                        ))}
                      </div>
                      <div className="color-picker-custom">
                        <label className="color-picker-custom-label">
                          Custom:
                          <input
                            type="color"
                            value={selectedSession.color}
                            onChange={(e) => handleChangeColor(selectedSession.id, e.target.value, false)}
                            className="color-picker-input"
                          />
                        </label>
                      </div>
                    </div>
                  )}
                </div>
                <button
                  className="transcript-btn"
                  onClick={() => handleCopyAllMessages(selectedSession.id)}
                  title="Copy all messages"
                >
                  📋 Copy All
                </button>
                <button
                  className="transcript-btn"
                  onClick={() => handleClearSession(selectedSession.id)}
                  title="Clear messages"
                >
                  🗑️ Clear
                </button>
                <button
                  className="transcript-btn transcript-btn-danger"
                  onClick={() => handleDeleteSession(selectedSession.id)}
                  title="Delete session"
                >
                  ❌ Delete
                </button>
              </div>
            </div>

            <div className="transcript-messages">
              {selectedSession.messages.length === 0 ? (
                <div className="transcript-messages-empty">
                  <div className="empty-icon">💬</div>
                  <div className="empty-text">No messages in this session</div>
                </div>
              ) : (
                <>
                  {[...selectedSession.messages].reverse().map((message) => (
                    <div key={message.id} className="transcript-message">
                      <div className="transcript-message-header">
                        <span className="transcript-message-time">
                          {formatTime(message.timestamp)}
                        </span>
                        <button
                          className="transcript-message-copy"
                          onClick={() => handleCopyMessage(message.text)}
                          title="Copy message"
                        >
                          📋
                        </button>
                      </div>
                      <div className="transcript-message-text">
                        {message.text}
                      </div>
                    </div>
                  ))}
                  <div ref={messagesEndRef} />
                </>
              )}
            </div>
          </>
        ) : (
          <div className="transcript-main-empty">
            <div className="empty-icon">👈</div>
            <div className="empty-text">Select a session to view messages</div>
            <div className="empty-hint">
              Sessions are created automatically when Claude speaks
            </div>
          </div>
          )}
        </div>
        </div>
      )}
    </div>
  );
}
