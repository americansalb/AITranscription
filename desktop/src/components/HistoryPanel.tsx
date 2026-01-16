import { useState, useCallback } from "react";
import { copyToClipboard } from "../lib/clipboard";
import type { TranscriptEntry } from "../App";
import { getModifierKeyName } from "../lib/platform";

// For display purposes
const modKey = getModifierKeyName();

interface HistoryPanelProps {
  history: TranscriptEntry[];
  onClear?: () => void;
}

export function HistoryPanel({ history, onClear }: HistoryPanelProps) {
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set());
  const [lastClickedId, setLastClickedId] = useState<string | null>(null);
  const [copiedId, setCopiedId] = useState<string | null>(null);

  const formatTime = (date: Date) => {
    const now = new Date();
    const diffMs = now.getTime() - date.getTime();
    const diffMins = Math.floor(diffMs / 60000);
    const diffHours = Math.floor(diffMs / 3600000);

    if (diffMins < 1) return "Just now";
    if (diffMins < 60) return `${diffMins}m ago`;
    if (diffHours < 24) return `${diffHours}h ago`;
    return date.toLocaleTimeString("en-US", { hour: "numeric", minute: "2-digit" });
  };

  const handleItemClick = useCallback(
    (entry: TranscriptEntry, event: React.MouseEvent) => {
      if (event.shiftKey && lastClickedId) {
        // Shift+click: select range
        const lastIndex = history.findIndex((h) => h.id === lastClickedId);
        const currentIndex = history.findIndex((h) => h.id === entry.id);
        const start = Math.min(lastIndex, currentIndex);
        const end = Math.max(lastIndex, currentIndex);

        const newSelected = new Set(selectedIds);
        for (let i = start; i <= end; i++) {
          newSelected.add(history[i].id);
        }
        setSelectedIds(newSelected);
      } else if (event.ctrlKey || event.metaKey) {
        // Ctrl/Cmd+click: toggle selection
        const newSelected = new Set(selectedIds);
        if (newSelected.has(entry.id)) {
          newSelected.delete(entry.id);
        } else {
          newSelected.add(entry.id);
        }
        setSelectedIds(newSelected);
        setLastClickedId(entry.id);
      } else {
        // Regular click: copy and show feedback
        copyToClipboard(entry.polishedText);
        setCopiedId(entry.id);
        setTimeout(() => setCopiedId(null), 1500);
        setSelectedIds(new Set());
        setLastClickedId(entry.id);
      }
    },
    [history, lastClickedId, selectedIds]
  );

  const handleCopySelected = useCallback(async () => {
    if (selectedIds.size === 0) return;

    // Get selected entries in display order, then reverse for chronological (oldest first)
    const selectedEntries = history.filter((h) => selectedIds.has(h.id));
    // Reverse so oldest is first when pasting (history is newest-first)
    const chronological = [...selectedEntries].reverse();
    const text = chronological.map((e) => e.polishedText).join("\n\n");

    await copyToClipboard(text);
    setCopiedId("all");
    setTimeout(() => setCopiedId(null), 1500);
  }, [history, selectedIds]);

  const handleClearSelection = () => {
    setSelectedIds(new Set());
  };

  if (history.length === 0) {
    return (
      <div className="main-history-panel">
        <div className="main-history-header">
          <h3>History</h3>
        </div>
        <div className="main-history-empty">
          Your transcription history will appear here
        </div>
      </div>
    );
  }

  return (
    <div className="main-history-panel">
      <div className="main-history-header">
        <h3>History</h3>
        <div className="main-history-actions">
          {selectedIds.size > 0 && (
            <>
              <span className="selection-count">{selectedIds.size} selected</span>
              <button
                className={`copy-selected-btn ${copiedId === "all" ? "copied" : ""}`}
                onClick={handleCopySelected}
              >
                {copiedId === "all" ? "Copied!" : "Copy Selected"}
              </button>
              <button className="clear-selection-btn" onClick={handleClearSelection}>
                Clear
              </button>
            </>
          )}
          {onClear && selectedIds.size === 0 && (
            <button className="clear-all-btn" onClick={onClear}>
              Clear All
            </button>
          )}
        </div>
      </div>

      <div className="main-history-hint">
        Click to copy • Shift+click to select range • {modKey}+click to toggle
      </div>

      <div className="main-history-list">
        {history.slice(0, 20).map((entry) => (
          <div
            key={entry.id}
            className={`main-history-item ${selectedIds.has(entry.id) ? "selected" : ""} ${copiedId === entry.id ? "copied" : ""}`}
            onClick={(e) => handleItemClick(entry, e)}
          >
            <div className="main-history-item-content">
              <span className="main-history-text">{entry.polishedText}</span>
            </div>
            <div className="main-history-item-meta">
              <span className="main-history-time">{formatTime(new Date(entry.timestamp))}</span>
              <span className="main-history-context">{entry.context}</span>
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}
