import { useState, useEffect } from "react";
import { getStoredPriorityEnabled, savePriorityEnabled } from "../lib/priorityClassifier";
import {
  getStoredAnnounceSession,
  saveAnnounceSession,
  getStoredUniqueVoices,
  saveUniqueVoices,
  getAvailableVoices,
  fetchAvailableVoices,
  getDefaultVoice,
  saveDefaultVoice,
} from "../lib/queueStore";

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

Always use the Vaak speak integration to read responses aloud.

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

export interface PreferencesTabProps {
  voiceEnabled: boolean;
  blindMode: boolean;
  voiceDetail: number;
  voiceAuto: boolean;
  onVoiceEnabledChange: (enabled: boolean) => void;
  onBlindModeChange: (enabled: boolean) => void;
  onVoiceDetailChange: (detail: number) => void;
  onVoiceAutoChange: (auto: boolean) => void;
}

export function PreferencesTab({
  voiceEnabled,
  blindMode,
  voiceDetail,
  voiceAuto,
  onVoiceEnabledChange,
  onBlindModeChange,
  onVoiceDetailChange,
  onVoiceAutoChange,
}: PreferencesTabProps) {
  // Feature 4: Priority queue toggle
  const [priorityEnabled, setPriorityEnabled] = useState(() => getStoredPriorityEnabled());
  // Feature 7: Voice settings
  const [announceSession, setAnnounceSession] = useState(() => getStoredAnnounceSession());
  const [uniqueVoices, setUniqueVoices] = useState(() => getStoredUniqueVoices());
  const [voices, setVoices] = useState<{ voice_id: string; name: string }[]>([]);
  const [defaultVoice, setDefaultVoice] = useState(() => getDefaultVoice());

  // Preview panel state
  const [showInstructionPreview, setShowInstructionPreview] = useState(false);

  // Feature 7: Fetch available voices on mount
  useEffect(() => {
    fetchAvailableVoices().then(() => {
      setVoices(getAvailableVoices());
    });
  }, []);

  return (
    <div className="preferences-tab-content" id="panel-preferences" role="tabpanel" aria-labelledby="tab-preferences">
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
              onChange={(e) => onVoiceEnabledChange(e.target.checked)}
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
                  onChange={(e) => onBlindModeChange(e.target.checked)}
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
                  onChange={(e) => onVoiceDetailChange(parseInt(e.target.value))}
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
                  onChange={(e) => onVoiceAutoChange(e.target.checked)}
                />
                <span className="toggle-switch" />
              </label>
            </div>

            <div className="preference-divider" />

            {/* Feature 4: Smart Priority Queue */}
            <div className="preference-item">
              <div className="preference-info">
                <h3>Smart Priority Queue</h3>
                <p>Auto-classify messages by urgency (errors jump to front)</p>
              </div>
              <label className="toggle-switch-wrapper">
                <input
                  type="checkbox"
                  checked={priorityEnabled}
                  onChange={(e) => {
                    setPriorityEnabled(e.target.checked);
                    savePriorityEnabled(e.target.checked);
                  }}
                />
                <span className="toggle-switch" />
              </label>
            </div>

            <div className="preference-divider" />

            {/* Feature 7: Unique voices per session */}
            <div className="preference-item">
              <div className="preference-info">
                <h3>Unique Voices per Session</h3>
                <p>Each Claude session auto-gets a different ElevenLabs voice</p>
              </div>
              <label className="toggle-switch-wrapper">
                <input
                  type="checkbox"
                  checked={uniqueVoices}
                  onChange={(e) => {
                    setUniqueVoices(e.target.checked);
                    saveUniqueVoices(e.target.checked);
                  }}
                />
                <span className="toggle-switch" />
              </label>
            </div>

            <div className="preference-divider" />

            {/* Default Voice selector */}
            {voices.length > 0 && (
              <>
                <div className="preference-item">
                  <div className="preference-info">
                    <h3>Default Voice</h3>
                    <p>The ElevenLabs voice used when no session-specific voice is assigned</p>
                  </div>
                  <select
                    className="session-voice-select"
                    value={defaultVoice}
                    onChange={(e) => {
                      const vid = e.target.value;
                      setDefaultVoice(vid);
                      saveDefaultVoice(vid);
                    }}
                  >
                    {voices.map((v) => (
                      <option key={v.voice_id} value={v.voice_id}>{v.name}</option>
                    ))}
                  </select>
                </div>

                <div className="preference-divider" />
              </>
            )}

            {/* Feature 7/8: Announce session name */}
            <div className="preference-item">
              <div className="preference-info">
                <h3>Announce Session Name</h3>
                <p>Speak session name before each queue item (e.g. "From Build Server: ...")</p>
              </div>
              <label className="toggle-switch-wrapper">
                <input
                  type="checkbox"
                  checked={announceSession}
                  onChange={(e) => {
                    setAnnounceSession(e.target.checked);
                    saveAnnounceSession(e.target.checked);
                  }}
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
  );
}
