import { useState, useEffect, useRef } from "react";
import {
  getStoredSRModel,
  saveSRModel,
  getStoredSRDetail,
  saveSRDetail,
  getStoredSRFocus,
  saveSRFocus,
  getStoredSRHotkey,
  saveSRHotkey,
  getStoredSRVoiceId,
  saveSRVoiceId,
} from "./lib/voiceStream";
import { keyEventToHotkey } from "./components/Settings";
import { formatHotkeyForDisplay, isWindows, isMacOS, getModifierKeyName, getAltKeyName } from "./lib/platform";
import "./styles/screen-reader.css";

const SR_DETAIL_LABELS = ["Brief", "Concise", "Balanced", "Thorough", "Exhaustive"];

export function ScreenReaderApp() {
  const [enabled, setEnabled] = useState(() => localStorage.getItem("vaak_sr_enabled") !== "false");
  const [srModel, setSRModel] = useState(() => getStoredSRModel());
  const [srDetail, setSRDetail] = useState(() => getStoredSRDetail());
  const [srFocus, setSRFocus] = useState(() => getStoredSRFocus());
  const [srHotkey, setSRHotkey] = useState(() => getStoredSRHotkey());
  const [srVoiceId, setSRVoiceId] = useState(() => getStoredSRVoiceId());
  const [voices, setVoices] = useState<{ voice_id: string; name: string }[]>([]);
  const [isRecordingHotkey, setIsRecordingHotkey] = useState(false);
  const [uiaEnabled, setUiaEnabled] = useState(() => localStorage.getItem("vaak_sr_uia") !== "false");
  const [focusTrackingEnabled, setFocusTrackingEnabled] = useState(() => localStorage.getItem("vaak_sr_focus_tracking") === "true");
  const [backendStatus, setBackendStatus] = useState<"connected" | "disconnected">("disconnected");
  const [lastReadTime, setLastReadTime] = useState<string>(() => localStorage.getItem("vaak_sr_last_read") || "Never");
  const hotkeyRef = useRef<HTMLInputElement>(null);

  const syncToRust = async (model: string, detail: number, focus: string, hotkey: string, voice?: string) => {
    if (window.__TAURI__) {
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        await invoke("save_screen_reader_settings", { model, detail, focus, hotkey, voice });
        // Also save to localStorage so queueStore can use the right voice for SR sessions
        if (voice) {
          localStorage.setItem("vaak-sr-settings", JSON.stringify({ voice_id: voice }));
        }
      } catch (e) {
        console.error("Failed to save SR settings:", e);
      }
    }
  };

  // Fetch voices
  useEffect(() => {
    const apiUrl = import.meta.env.VITE_API_URL || "http://127.0.0.1:19836";
    fetch(`${apiUrl}/api/v1/voices`)
      .then((r) => (r.ok ? r.json() : null))
      .then((data) => { if (data?.voices) setVoices(data.voices); })
      .catch(() => {});
  }, []);

  // Check backend status
  useEffect(() => {
    const apiUrl = import.meta.env.VITE_API_URL || "http://127.0.0.1:19836";
    const check = () => {
      fetch(`${apiUrl}/api/v1/health`)
        .then((r) => setBackendStatus(r.ok ? "connected" : "disconnected"))
        .catch(() => setBackendStatus("disconnected"));
    };
    check();
    const interval = setInterval(check, 10000);
    return () => clearInterval(interval);
  }, []);

  // Listen for screen-reader-done events to update last read time
  useEffect(() => {
    if (!window.__TAURI__) return;
    let unlisten: (() => void) | undefined;
    const setup = async () => {
      const { listen } = await import("@tauri-apps/api/event");
      unlisten = await listen("screen-reader-done", () => {
        const now = new Date().toLocaleTimeString();
        setLastReadTime(now);
        localStorage.setItem("vaak_sr_last_read", now);
      });
    };
    setup();
    return () => { if (unlisten) unlisten(); };
  }, []);

  const handleToggle = () => {
    const next = !enabled;
    setEnabled(next);
    localStorage.setItem("vaak_sr_enabled", String(next));
  };

  const handleModelChange = (v: string) => { setSRModel(v); saveSRModel(v); syncToRust(v, srDetail, srFocus, srHotkey, srVoiceId); };
  const handleDetailChange = (v: number) => { setSRDetail(v); saveSRDetail(v); syncToRust(srModel, v, srFocus, srHotkey, srVoiceId); };
  const handleFocusChange = (v: string) => { setSRFocus(v); saveSRFocus(v); syncToRust(srModel, srDetail, v, srHotkey, srVoiceId); };
  const handleHotkeyChange = (v: string) => { setSRHotkey(v); saveSRHotkey(v); syncToRust(srModel, srDetail, srFocus, v, srVoiceId); };
  const handleVoiceChange = (v: string) => { setSRVoiceId(v); saveSRVoiceId(v); syncToRust(srModel, srDetail, srFocus, srHotkey, v); };

  const handleVoicePreview = async () => {
    try {
      const apiUrl = import.meta.env.VITE_API_URL || "http://127.0.0.1:19836";
      const resp = await fetch(`${apiUrl}/api/v1/speak`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ text: "Screen reader is working.", voice_id: srVoiceId }),
      });
      if (!resp.ok) console.error("Preview failed");
    } catch (e) {
      console.error("Voice preview error:", e);
    }
  };

  return (
    <div className="screen-reader-app">
      <div className="sr-header">
        <div>
          <h1>Screen Reader</h1>
          <div className="sr-subtitle">Configure how Vaak describes your screen</div>
        </div>
      </div>

      <div className="sr-content">
          {/* General Settings */}
          <div className="sr-section">
            <h2>
              <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83-2.83l.06-.06A1.65 1.65 0 0 0 4.68 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 2.83-2.83l.06.06A1.65 1.65 0 0 0 9 4.68a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 2.83l-.06.06A1.65 1.65 0 0 0 19.4 9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"/></svg>
              General
            </h2>
            <div className="sr-row">
              <div>
                <div className="sr-row-label">Enable Screen Reader</div>
                <div className="sr-row-desc">{formatHotkeyForDisplay("Alt+R")} captures screen, {formatHotkeyForDisplay("Alt+A")} for follow-up questions</div>
              </div>
              <button className={`sr-toggle ${enabled ? "active" : ""}`} onClick={handleToggle} />
            </div>
            <div className="sr-row">
              <div>
                <div className="sr-row-label">Hotkey</div>
                <div className="sr-row-desc">Requires app restart to take effect</div>
              </div>
              <div className="sr-hotkey-container">
                <input
                  ref={hotkeyRef}
                  type="text"
                  readOnly
                  value={isRecordingHotkey ? "Press keys..." : formatHotkeyForDisplay(srHotkey)}
                  className={`sr-hotkey-input ${isRecordingHotkey ? "recording" : ""}`}
                  onFocus={() => setIsRecordingHotkey(true)}
                  onBlur={() => setIsRecordingHotkey(false)}
                  onKeyDown={(e) => {
                    e.preventDefault();
                    const h = keyEventToHotkey(e.nativeEvent);
                    if (h) { handleHotkeyChange(h); setIsRecordingHotkey(false); hotkeyRef.current?.blur(); }
                  }}
                />
                <button className="sr-reset-btn" onClick={() => handleHotkeyChange("Alt+R")}>Reset</button>
              </div>
            </div>
          </div>

          {/* Accessibility Settings */}
          <div className="sr-section">
            <h2>
              <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M12 2a3 3 0 0 0-3 3v4a3 3 0 0 0 6 0V5a3 3 0 0 0-3-3z"/><path d="M19 10v2a7 7 0 0 1-14 0v-2"/></svg>
              Accessibility
            </h2>
            <div className="sr-row">
              <div>
                <div className="sr-row-label">UI Automation Tree{!isWindows() && " (Windows only)"}</div>
                <div className="sr-row-desc">Send element names, types, and coordinates alongside screenshots for precise interaction</div>
              </div>
              <button
                className={`sr-toggle ${uiaEnabled ? "active" : ""}`}
                disabled={!isWindows()}
                title={!isWindows() ? "Requires Windows UI Automation API" : undefined}
                onClick={() => {
                  if (!isWindows()) return;
                  const next = !uiaEnabled;
                  setUiaEnabled(next);
                  localStorage.setItem("vaak_sr_uia", String(next));
                }}
                style={!isWindows() ? { opacity: 0.4, cursor: "not-allowed" } : undefined}
              />
            </div>
            <div className="sr-row">
              <div>
                <div className="sr-row-label">Focus Tracking{!isWindows() && " (Windows only)"}</div>
                <div className="sr-row-desc">Automatically announce focused elements as you Tab through the UI (no API calls)</div>
              </div>
              <button
                className={`sr-toggle ${focusTrackingEnabled ? "active" : ""}`}
                disabled={!isWindows()}
                title={!isWindows() ? "Requires Windows UI Automation API" : undefined}
                onClick={async () => {
                  if (!isWindows()) return;
                  const next = !focusTrackingEnabled;
                  setFocusTrackingEnabled(next);
                  localStorage.setItem("vaak_sr_focus_tracking", String(next));
                  if (window.__TAURI__) {
                    try {
                      const { invoke } = await import("@tauri-apps/api/core");
                      await invoke("set_focus_tracking", { enabled: next });
                    } catch (e) {
                      console.error("Failed to toggle focus tracking:", e);
                    }
                  }
                }}
                style={!isWindows() ? { opacity: 0.4, cursor: "not-allowed" } : undefined}
              />
            </div>
            {isMacOS() && (
              <div className="sr-macos-guidance">
                <div className="sr-row-label">macOS Accessibility</div>
                <div className="sr-row-desc">
                  UI Automation and Focus Tracking require Windows APIs. On macOS, Vaak captures your screen to describe what's visible. For the best experience:
                </div>
                <ul className="sr-macos-tips">
                  <li>Enable VoiceOver ({getModifierKeyName()}+F5) for native macOS screen reading alongside Vaak</li>
                  <li>Grant Accessibility permission: System Settings &gt; Privacy &amp; Security &gt; Accessibility &gt; Vaak</li>
                  <li>Grant Screen Recording permission: System Settings &gt; Privacy &amp; Security &gt; Screen Recording &gt; Vaak</li>
                  <li>Use the <code>say</code> command in Terminal for quick text-to-speech testing</li>
                </ul>
              </div>
            )}
          </div>

          {/* Vision Settings */}
          <div className="sr-section">
            <h2>
              <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z"/><circle cx="12" cy="12" r="3"/></svg>
              Vision
            </h2>
            <div className="sr-row">
              <div className="sr-row-label">Model</div>
              <select className="sr-select" value={srModel} onChange={(e) => handleModelChange(e.target.value)}>
                <option value="claude-3-5-haiku-20241022">Haiku (fast/cheap)</option>
                <option value="claude-sonnet-4-20250514">Sonnet (balanced)</option>
                <option value="claude-opus-4-20250514">Opus (best)</option>
              </select>
            </div>
            <div className="sr-row">
              <div className="sr-row-label">Detail Level</div>
              <div className="sr-slider-container">
                <div className="sr-slider-value">{SR_DETAIL_LABELS[srDetail - 1]}</div>
                <input
                  type="range" min="1" max="5" step="1" value={srDetail}
                  className="sr-slider"
                  onChange={(e) => handleDetailChange(parseInt(e.target.value))}
                />
              </div>
            </div>
            <div className="sr-row">
              <div className="sr-row-label">Focus Mode</div>
              <select className="sr-select" value={srFocus} onChange={(e) => handleFocusChange(e.target.value)}>
                <option value="general">General</option>
                <option value="errors">Errors</option>
                <option value="code">Code</option>
                <option value="text">Text</option>
              </select>
            </div>
          </div>

          {/* Voice Settings */}
          <div className="sr-section">
            <h2>
              <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><polygon points="11 5 6 9 2 9 2 15 6 15 11 19 11 5"/><path d="M15.54 8.46a5 5 0 0 1 0 7.07"/><path d="M19.07 4.93a10 10 0 0 1 0 14.14"/></svg>
              Voice
            </h2>
            <div className="sr-row">
              <div className="sr-row-label">Voice</div>
              <div className="sr-voice-row">
                <select className="sr-select" value={srVoiceId} onChange={(e) => handleVoiceChange(e.target.value)}>
                  {voices.length > 0 ? (
                    voices.map((v) => <option key={v.voice_id} value={v.voice_id}>{v.name}</option>)
                  ) : (
                    <option value={srVoiceId}>Ravi (default)</option>
                  )}
                </select>
                <button className="sr-preview-btn" onClick={handleVoicePreview}>Test</button>
              </div>
            </div>
          </div>

          {/* App Profiles */}
          <div className="sr-section">
            <h2>
              <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><rect x="2" y="3" width="20" height="14" rx="2" ry="2"/><line x1="8" y1="21" x2="16" y2="21"/><line x1="12" y1="17" x2="12" y2="21"/></svg>
              App Profiles
            </h2>
            <div className="sr-row">
              <div>
                <div className="sr-row-label">Built-in Profiles</div>
                <div className="sr-row-desc">Chrome, Edge, Word, Excel, Outlook, VS Code, Notepad, File Explorer</div>
              </div>
              <div className="sr-status-value" style={{ color: "#22c55e" }}>Active</div>
            </div>
            <div className="sr-row">
              <div>
                <div className="sr-row-label">How it works</div>
                <div className="sr-row-desc">When a known app is focused, its keyboard shortcuts are automatically injected into the AI prompt for more reliable navigation</div>
              </div>
            </div>
          </div>

          {/* Status */}
          <div className="sr-section">
            <h2>
              <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><circle cx="12" cy="12" r="10"/><polyline points="12 6 12 12 16 14"/></svg>
              Status
            </h2>
            <div className="sr-status-grid">
              <div className="sr-status-item">
                <div className="sr-status-label">Capture</div>
                <div className="sr-status-value">{formatHotkeyForDisplay(srHotkey)}</div>
              </div>
              <div className="sr-status-item">
                <div className="sr-status-label">Ask</div>
                <div className="sr-status-value">{formatHotkeyForDisplay("Alt+A")}</div>
              </div>
              <div className="sr-status-item">
                <div className="sr-status-label">Last Read</div>
                <div className="sr-status-value">{lastReadTime}</div>
              </div>
              <div className="sr-status-item">
                <div className="sr-status-label">Backend</div>
                <div className={`sr-status-value ${backendStatus}`}>
                  {backendStatus === "connected" ? "Connected" : "Disconnected"}
                </div>
              </div>
              <div className="sr-status-item">
                <div className="sr-status-label">Model</div>
                <div className="sr-status-value">{srModel.includes("haiku") ? "Haiku" : srModel.includes("sonnet") ? "Sonnet" : "Opus"}</div>
              </div>
            </div>
          </div>
        </div>
    </div>
  );
}
