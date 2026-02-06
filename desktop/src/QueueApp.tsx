import { useEffect, useState } from "react";
import { QueueTab } from "./components/QueueTab";
import { initQueueStore, fetchAvailableVoices, getAvailableVoices, getDefaultVoice, saveDefaultVoice } from "./lib/queueStore";
import { getStoredVoiceEnabled, saveVoiceEnabled } from "./lib/voiceStream";
import "./styles/queue-panel.css";

export function QueueApp() {
  const [voiceEnabled, setVoiceEnabled] = useState(() => getStoredVoiceEnabled());
  const [voices, setVoices] = useState<{ voice_id: string; name: string }[]>([]);
  const [defaultVoice, setDefaultVoice] = useState(() => getDefaultVoice());

  // Initialize queue store and fetch voices on mount
  useEffect(() => {
    initQueueStore();
    fetchAvailableVoices().then(() => {
      const fetched = getAvailableVoices();
      setVoices(fetched);
      // Re-read saved default after voices load to stay in sync
      setDefaultVoice(getDefaultVoice());
    });
  }, []);

  // Listen for voice-settings-changed from other windows
  useEffect(() => {
    if (!window.__TAURI__) return;
    let unlisten: (() => void) | undefined;
    const setup = async () => {
      const { listen } = await import("@tauri-apps/api/event");
      unlisten = await listen<{ voiceEnabled: boolean }>("voice-settings-changed", (event) => {
        setVoiceEnabled(event.payload.voiceEnabled);
      });
    };
    setup();
    return () => { if (unlisten) unlisten(); };
  }, []);

  const handleVoiceToggle = async () => {
    const next = !voiceEnabled;
    setVoiceEnabled(next);
    saveVoiceEnabled(next);
    if (!next) {
      const { clearPending } = await import("./lib/queueStore");
      await clearPending();
    }
    if (window.__TAURI__) {
      try {
        const { emit } = await import("@tauri-apps/api/event");
        await emit("voice-settings-changed", { voiceEnabled: next });
      } catch {}
    }
  };

  return (
    <div className="queue-app">
      <div className="queue-panel-header">
        <div className="queue-panel-header-left">
          <span className="queue-panel-title">Voice Controls</span>
        </div>
        <div className="queue-panel-header-right">
          <button
            className={`queue-panel-voice-toggle ${voiceEnabled ? "enabled" : ""}`}
            onClick={handleVoiceToggle}
            title={voiceEnabled ? "Voice ON (click to disable)" : "Voice OFF (click to enable)"}
          >
            {voiceEnabled ? (
              <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <polygon points="11 5 6 9 2 9 2 15 6 15 11 19 11 5" />
                <path d="M15.54 8.46a5 5 0 0 1 0 7.07" />
                <path d="M19.07 4.93a10 10 0 0 1 0 14.14" />
              </svg>
            ) : (
              <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <polygon points="11 5 6 9 2 9 2 15 6 15 11 19 11 5" />
                <line x1="23" y1="9" x2="17" y2="15" />
                <line x1="17" y1="9" x2="23" y2="15" />
              </svg>
            )}
            <span>{voiceEnabled ? "ON" : "OFF"}</span>
          </button>
        </div>
      </div>
      {voices.length > 0 && (
        <div className="queue-app-voice-bar">
          <label className="queue-app-voice-label">Default Voice</label>
          <select
            className="queue-app-voice-select"
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
      )}
      <div className="queue-panel-body">
        <QueueTab />
      </div>
    </div>
  );
}
