import { QueueTab } from "./QueueTab";
import "../styles/queue-panel.css";

interface QueueSlidePanelProps {
  isOpen: boolean;
  onClose: () => void;
  voiceEnabled: boolean;
  onVoiceToggle: () => void;
}

async function handlePopOut(onClose: () => void) {
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    await invoke("toggle_queue_window");
    onClose();
  } catch (e) {
    console.error("Failed to pop out queue window:", e);
  }
}

export function QueueSlidePanel({ isOpen, onClose, voiceEnabled, onVoiceToggle }: QueueSlidePanelProps) {
  return (
    <>
      <div
        className={`queue-panel-backdrop ${isOpen ? "open" : ""}`}
        onClick={onClose}
      />
      <div className={`queue-slide-panel ${isOpen ? "open" : ""}`}>
        <div className="queue-panel-header">
          <div className="queue-panel-header-left">
            <span className="queue-panel-title">Voice Controls</span>
          </div>
          <div className="queue-panel-header-right">
            <button
              className={`queue-panel-voice-toggle ${voiceEnabled ? "enabled" : ""}`}
              onClick={onVoiceToggle}
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
            <button
              className="queue-panel-popout-btn"
              onClick={() => handlePopOut(onClose)}
              title="Pop out to separate window"
            >
              <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6" />
                <polyline points="15 3 21 3 21 9" />
                <line x1="10" y1="14" x2="21" y2="3" />
              </svg>
            </button>
            <button className="queue-panel-close-btn" onClick={onClose} title="Close panel">
              <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <line x1="18" y1="6" x2="6" y2="18" />
                <line x1="6" y1="6" x2="18" y2="18" />
              </svg>
            </button>
          </div>
        </div>
        <div className="queue-panel-body">
          <QueueTab />
        </div>
      </div>
    </>
  );
}
