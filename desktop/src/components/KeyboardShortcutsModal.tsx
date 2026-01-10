import { useEffect } from "react";

// Detect if running on macOS
const isMac = typeof navigator !== "undefined" && navigator.platform.toUpperCase().indexOf("MAC") >= 0;
const modKey = isMac ? "Cmd" : "Ctrl";

interface KeyboardShortcutsModalProps {
  onClose: () => void;
}

const SHORTCUTS = [
  {
    category: "Recording",
    items: [
      { keys: [modKey, "Shift", "S"], description: "Push-to-talk (hold to record)" },
      { keys: ["Esc"], description: "Cancel recording" },
    ],
  },
  {
    category: "Results",
    items: [
      { keys: [modKey, "C"], description: "Copy result to clipboard" },
      { keys: [modKey, "A"], description: "Select all text" },
    ],
  },
  {
    category: "Navigation",
    items: [
      { keys: ["?"], description: "Show this help" },
      { keys: ["Esc"], description: "Close modals / Cancel editing" },
    ],
  },
];

export function KeyboardShortcutsModal({ onClose }: KeyboardShortcutsModalProps) {
  // Close on Escape
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        onClose();
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [onClose]);

  // Close on backdrop click
  const handleBackdropClick = (e: React.MouseEvent) => {
    if (e.target === e.currentTarget) {
      onClose();
    }
  };

  return (
    <div className="shortcuts-overlay" onClick={handleBackdropClick}>
      <div className="shortcuts-modal">
        <div className="shortcuts-header">
          <h2>Keyboard Shortcuts</h2>
          <button className="close-btn" onClick={onClose}>
            <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              <line x1="18" y1="6" x2="6" y2="18" />
              <line x1="6" y1="6" x2="18" y2="18" />
            </svg>
          </button>
        </div>
        <div className="shortcuts-content">
          {SHORTCUTS.map((section) => (
            <div key={section.category} className="shortcuts-section">
              <h3>{section.category}</h3>
              <div className="shortcuts-list">
                {section.items.map((shortcut, index) => (
                  <div key={index} className="shortcut-item">
                    <div className="shortcut-keys">
                      {shortcut.keys.map((key, keyIndex) => (
                        <span key={keyIndex}>
                          <kbd>{key}</kbd>
                          {keyIndex < shortcut.keys.length - 1 && <span className="key-separator">+</span>}
                        </span>
                      ))}
                    </div>
                    <span className="shortcut-description">{shortcut.description}</span>
                  </div>
                ))}
              </div>
            </div>
          ))}
        </div>
        <div className="shortcuts-footer">
          <p>Press <kbd>Esc</kbd> to close</p>
        </div>
      </div>
    </div>
  );
}
