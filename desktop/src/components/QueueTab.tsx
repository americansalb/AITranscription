import React, { useEffect, useState } from "react";
import { useQueueStore } from "../lib/queueStore";
import * as queueStore from "../lib/queueStore";
import type { QueueItem } from "../lib/queueTypes";
import "../styles/queue.css";

// Format relative time
function formatTimeAgo(timestamp: number): string {
  const seconds = Math.floor((Date.now() - timestamp) / 1000);
  if (seconds < 5) return "now";
  if (seconds < 60) return `${seconds}s ago`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m ago`;
  if (seconds < 86400) return `${Math.floor(seconds / 3600)}h ago`;
  return `${Math.floor(seconds / 86400)}d ago`;
}

// Format time as MM:SS
function formatTime(ms: number): string {
  const seconds = Math.floor(ms / 1000);
  const mins = Math.floor(seconds / 60);
  const secs = seconds % 60;
  return `${mins}:${secs.toString().padStart(2, "0")}`;
}

// Now Playing Card Component
function NowPlayingCard({
  item,
  isPlaying,
  isPaused,
  volume,
}: {
  item: QueueItem;
  isPlaying: boolean;
  isPaused: boolean;
  volume: number;
}) {
  const [currentTime, setCurrentTime] = useState(0);
  const [duration, setDuration] = useState(0);

  // Update time from real audio element every 100ms while playing
  useEffect(() => {
    if (!isPlaying && !isPaused) return;

    const updateTime = () => {
      const info = queueStore.getAudioPlaybackInfo();
      setCurrentTime(info.currentTimeMs);
      if (info.durationMs > 0) {
        setDuration(info.durationMs);
      }
    };

    // Initial update
    updateTime();

    const interval = setInterval(updateTime, 100);
    return () => clearInterval(interval);
  }, [isPlaying, isPaused]);

  // Use real duration if available, otherwise estimate from text length
  const displayDuration = duration > 0
    ? duration
    : (item.durationMs || (item.text.split(/\s+/).length / 150) * 60 * 1000);
  const progress = displayDuration > 0 ? Math.min((currentTime / displayDuration) * 100, 100) : 0;

  const handlePlayPause = () => {
    queueStore.togglePlayPause();
  };

  const handleSkipPrevious = () => {
    queueStore.skipPrevious();
  };

  const handleSkipNext = () => {
    queueStore.skipNext();
  };

  const handleVolumeChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    queueStore.setVolume(parseFloat(e.target.value));
  };

  return (
    <div className="now-playing-card">
      <div className="now-playing-card-header">
        <div className="now-playing-label">
          <span className={`speaker-animated ${isPlaying && !isPaused ? "active" : ""}`}>
            <span></span><span></span><span></span>
          </span>
          <span className="label-text">{isPaused ? "PAUSED" : "NOW PLAYING"}</span>
        </div>
      </div>

      <div className="now-playing-card-content">
        <div className="now-playing-text-full">{item.text}</div>
      </div>

      <div className="now-playing-progress-bar">
        <div className="progress-track">
          <div
            className="progress-fill"
            style={{ width: `${progress}%` }}
          />
          <div
            className="progress-thumb"
            style={{ left: `${progress}%` }}
          />
        </div>
        <div className="progress-times">
          <span>{formatTime(currentTime)}</span>
          <span>{formatTime(displayDuration)}</span>
        </div>
      </div>

      <div className="now-playing-controls">
        <div className="playback-controls">
          <button
            className="control-btn"
            onClick={handleSkipPrevious}
            title="Previous"
          >
            <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor">
              <path d="M6 6h2v12H6V6zm3.5 6l8.5 6V6l-8.5 6z"/>
            </svg>
          </button>

          <button
            className="control-btn primary"
            onClick={handlePlayPause}
            title={isPlaying && !isPaused ? "Pause" : "Play"}
          >
            {isPlaying && !isPaused ? (
              <svg width="20" height="20" viewBox="0 0 24 24" fill="currentColor">
                <path d="M6 4h4v16H6V4zm8 0h4v16h-4V4z"/>
              </svg>
            ) : (
              <svg width="20" height="20" viewBox="0 0 24 24" fill="currentColor">
                <path d="M8 5v14l11-7L8 5z"/>
              </svg>
            )}
          </button>

          <button
            className="control-btn"
            onClick={handleSkipNext}
            title="Next"
          >
            <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor">
              <path d="M6 18l8.5-6L6 6v12zm8.5 0V6h2v12h-2z"/>
            </svg>
          </button>
        </div>

        <div className="secondary-controls">
          <div className="volume-control">
            <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor" className="volume-icon">
              <path d="M3 9v6h4l5 5V4L7 9H3zm13.5 3A4.5 4.5 0 0014 8.14v7.72c1.48-.73 2.5-2.25 2.5-3.86z"/>
            </svg>
            <input
              type="range"
              className="volume-slider"
              min="0"
              max="1"
              step="0.1"
              value={volume}
              onChange={handleVolumeChange}
            />
          </div>

        </div>
      </div>
    </div>
  );
}

// Empty Now Playing State
function EmptyNowPlaying() {
  return (
    <div className="now-playing-empty">
      <div className="empty-icon">
        <svg width="32" height="32" viewBox="0 0 24 24" fill="currentColor">
          <path d="M3 9v6h4l5 5V4L7 9H3zm13.5 3A4.5 4.5 0 0014 8.14v7.72c1.48-.73 2.5-2.25 2.5-3.86zM14 3.23v2.06c2.89.86 5 3.54 5 6.71s-2.11 5.85-5 6.71v2.06c4.01-.91 7-4.49 7-8.77s-2.99-7.86-7-8.77z"/>
        </svg>
      </div>
      <div className="empty-text">Queue empty - waiting for messages</div>
      <div className="empty-hint">Use the speaker toggle in the main app header to enable/disable voice</div>
    </div>
  );
}

// Drop indicator line component
function DropIndicator({ show }: { show: boolean }) {
  if (!show) return null;
  return <div className="drop-indicator" />;
}

// Up Next Item Component with mouse-based reordering
function UpNextItem({
  item,
  index,
  onPlay,
  onRemove,
  onStartDrag,
  isBeingDragged,
  showDropBefore,
  showDropAfter,
}: {
  item: QueueItem;
  index: number;
  onPlay: () => void;
  onRemove: () => void;
  onStartDrag: (index: number) => void;
  isBeingDragged: boolean;
  showDropBefore: boolean;
  showDropAfter: boolean;
}) {
  return (
    <>
      <DropIndicator show={showDropBefore} />
      <div
        className={`up-next-item ${isBeingDragged ? "dragging" : ""}`}
        data-index={index}
      >
      <div
        className="drag-handle"
        onMouseDown={(e) => {
          e.preventDefault();
          console.log("[UpNextItem] Mouse down on drag handle, index:", index);
          onStartDrag(index);
        }}
      >
        <span></span>
        <span></span>
        <span></span>
      </div>
      <div className="up-next-content">
        <div className="up-next-text">{item.text}</div>
        <div className="up-next-meta">
          <span className="meta-time">{formatTimeAgo(item.createdAt)}</span>
        </div>
      </div>
      <div className="up-next-actions">
        <button
          className="action-btn play-btn"
          onClick={onPlay}
          title="Play now"
        >
          <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor">
            <path d="M8 5v14l11-7L8 5z"/>
          </svg>
        </button>
        <button
          className="action-btn remove-btn"
          onClick={onRemove}
          title="Remove"
        >
          <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor">
            <path d="M19 6.41L17.59 5 12 10.59 6.41 5 5 6.41 10.59 12 5 17.59 6.41 19 12 13.41 17.59 19 19 17.59 13.41 12 19 6.41z"/>
          </svg>
        </button>
      </div>
    </div>
      <DropIndicator show={showDropAfter} />
    </>
  );
}

// History Item Component
function HistoryItem({
  item,
  onReplay,
}: {
  item: QueueItem;
  onReplay: () => void;
}) {
  const [showError, setShowError] = useState(false);
  const isCompleted = item.status === "completed";
  const isFailed = item.status === "failed";

  return (
    <div className={`history-item ${item.status}`}>
      <div className={`history-status ${item.status}`}>
        {isCompleted ? (
          <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor">
            <path d="M9 16.17L4.83 12l-1.42 1.41L9 19 21 7l-1.41-1.41L9 16.17z"/>
          </svg>
        ) : (
          <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor">
            <path d="M19 6.41L17.59 5 12 10.59 6.41 5 5 6.41 10.59 12 5 17.59 6.41 19 12 13.41 17.59 19 19 17.59 13.41 12 19 6.41z"/>
          </svg>
        )}
      </div>
      <div className="history-content">
        <div className="history-text">{item.text}</div>
        <div className="history-meta">
          {isCompleted && item.durationMs && (
            <span className="meta-duration">{(item.durationMs / 1000).toFixed(1)}s</span>
          )}
          {isFailed && (
            <button
              className="meta-error clickable"
              onClick={() => setShowError(!showError)}
              title="Click to show error details"
            >
              Error {showError ? "▲" : "▼"}
            </button>
          )}
          <span className="meta-time">
            {formatTimeAgo(item.completedAt || item.createdAt)}
          </span>
        </div>
        {isFailed && showError && (
          <div className="error-details">
            {item.errorMessage || "Unknown error"}
          </div>
        )}
      </div>
      <button
        className="action-btn replay-btn"
        onClick={onReplay}
        title="Replay"
      >
        <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor">
          <path d="M12 5V1L7 6l5 5V7c3.31 0 6 2.69 6 6s-2.69 6-6 6-6-2.69-6-6H4c0 4.42 3.58 8 8 8s8-3.58 8-8-3.58-8-8-8z"/>
        </svg>
      </button>
    </div>
  );
}

export function QueueTab() {
  const state = useQueueStore();
  const [, forceUpdate] = React.useState(0);
  const [historyCollapsed, setHistoryCollapsed] = useState(false);

  // Track if dragging to pause polling
  const isDraggingRef = React.useRef(false);

  // Poll for updates every 250ms (balance between responsiveness and performance)
  // NOTE: Audio playback is handled by the MAIN window, not this transcript window
  // Polling is paused while dragging to prevent choppy reordering
  useEffect(() => {
    const interval = setInterval(() => {
      // Skip polling while dragging to prevent choppy UI
      if (isDraggingRef.current) return;
      queueStore.loadItems();
      forceUpdate((n) => n + 1);
    }, 250);
    return () => clearInterval(interval);
  }, []);

  // Load items on mount
  useEffect(() => {
    queueStore.loadItems();
  }, []);

  // Get current playing item for display
  const playingItem = state.currentItem || state.items.find((i) => i.status === "playing");

  // Separate items by status
  const pendingItems = state.items
    .filter((i) => i.status === "pending")
    .sort((a, b) => a.position - b.position);

  const historyItems = state.items
    .filter((i) => i.status === "completed" || i.status === "failed")
    .sort((a, b) => (b.completedAt || b.createdAt) - (a.completedAt || a.createdAt));

  const handlePlay = (uuid: string) => {
    queueStore.playItem(uuid);
  };

  const handleRemove = (uuid: string) => {
    queueStore.removeItem(uuid);
  };

  const handleReplay = async (item: QueueItem) => {
    // Add the item back to the queue and play it
    await queueStore.addItem(item.text, item.sessionId);
  };

  const handleClearUpNext = async () => {
    for (const item of pendingItems) {
      await queueStore.removeItem(item.uuid);
    }
  };

  const handleClearHistory = () => {
    queueStore.clearCompleted();
  };

  // Mouse-based drag state
  const [draggingIndex, setDraggingIndex] = useState<number | null>(null);
  const [dropTargetIndex, setDropTargetIndex] = useState<number | null>(null);
  const listRef = React.useRef<HTMLDivElement>(null);

  // Use refs to avoid re-registering event listeners on every render
  const draggingIndexRef = React.useRef<number | null>(null);
  const dropTargetIndexRef = React.useRef<number | null>(null);
  const pendingItemsRef = React.useRef(pendingItems);

  // Keep refs in sync
  React.useEffect(() => {
    pendingItemsRef.current = pendingItems;
  }, [pendingItems]);

  // Start dragging
  const handleStartDrag = (index: number) => {
    console.log("[QueueTab] Starting drag from index:", index);
    isDraggingRef.current = true; // Pause polling
    setDraggingIndex(index);
    draggingIndexRef.current = index;
  };

  // Handle mouse move during drag - using refs to avoid dependency issues
  useEffect(() => {
    if (draggingIndex === null) return;

    let lastDropTarget: number | null = null;
    let rafId: number | null = null;

    const handleMouseMove = (e: MouseEvent) => {
      // Cancel any pending RAF to throttle updates
      if (rafId !== null) {
        cancelAnimationFrame(rafId);
      }

      rafId = requestAnimationFrame(() => {
        if (!listRef.current) return;

        // Find which item we're hovering over
        const items = listRef.current.querySelectorAll('.up-next-item');
        let newDropTarget: number | null = null;

        items.forEach((item, idx) => {
          const rect = item.getBoundingClientRect();
          const midPoint = rect.top + rect.height / 2;
          // Use midpoint for more stable drop positioning
          if (e.clientY >= rect.top && e.clientY <= rect.bottom) {
            newDropTarget = idx;
          }
        });

        if (newDropTarget !== lastDropTarget) {
          lastDropTarget = newDropTarget;
          dropTargetIndexRef.current = newDropTarget;
          setDropTargetIndex(newDropTarget);
        }
      });
    };

    const handleMouseUp = async () => {
      if (rafId !== null) {
        cancelAnimationFrame(rafId);
      }

      const currentDragging = draggingIndexRef.current;
      const currentDropTarget = dropTargetIndexRef.current;
      const items = pendingItemsRef.current;

      console.log("[QueueTab] Mouse up - dragging:", currentDragging, "dropTarget:", currentDropTarget);

      if (currentDragging !== null && currentDropTarget !== null && currentDragging !== currentDropTarget) {
        const sourceItem = items[currentDragging];
        const targetItem = items[currentDropTarget];

        if (sourceItem && targetItem) {
          console.log("[QueueTab] Reordering", sourceItem.uuid, "to position", targetItem.position);
          try {
            await queueStore.reorderItem(sourceItem.uuid, targetItem.position);
            console.log("[QueueTab] Reorder complete");
          } catch (err) {
            console.error("[QueueTab] Reorder failed:", err);
          }
        }
      }

      isDraggingRef.current = false; // Resume polling
      draggingIndexRef.current = null;
      dropTargetIndexRef.current = null;
      setDraggingIndex(null);
      setDropTargetIndex(null);
    };

    document.addEventListener('mousemove', handleMouseMove, { passive: true });
    document.addEventListener('mouseup', handleMouseUp);

    return () => {
      if (rafId !== null) {
        cancelAnimationFrame(rafId);
      }
      document.removeEventListener('mousemove', handleMouseMove);
      document.removeEventListener('mouseup', handleMouseUp);
    };
  }, [draggingIndex]); // Only depend on draggingIndex, not dropTargetIndex or pendingItems

  return (
    <div className="queue-tab-redesign">
      {/* Section 1: Now Playing */}
      <section className="queue-section now-playing-section">
        {playingItem ? (
          <NowPlayingCard
            item={playingItem}
            isPlaying={playingItem.status === "playing"}
            isPaused={state.isPaused}
            volume={state.volume}
          />
        ) : (
          <EmptyNowPlaying />
        )}
      </section>

      {/* Section 2: Up Next */}
      <section className="queue-section up-next-section">
        <div className="section-header">
          <div className="section-title">
            <span className="title-text">UP NEXT</span>
            {pendingItems.length > 0 && (
              <span className="section-count">{pendingItems.length}</span>
            )}
          </div>
          {pendingItems.length > 0 && (
            <button className="section-action" onClick={handleClearUpNext}>
              Clear All
            </button>
          )}
        </div>

        <div className="section-content">
          {pendingItems.length === 0 ? (
            <div className="section-empty">
              <span>No messages queued</span>
            </div>
          ) : (
            <div className="up-next-list" ref={listRef}>
              {pendingItems.map((item, index) => (
                <UpNextItem
                  key={item.uuid}
                  item={item}
                  index={index}
                  onPlay={() => handlePlay(item.uuid)}
                  onRemove={() => handleRemove(item.uuid)}
                  onStartDrag={handleStartDrag}
                  isBeingDragged={draggingIndex === index}
                  showDropBefore={dropTargetIndex === index && draggingIndex !== null && draggingIndex > index}
                  showDropAfter={dropTargetIndex === index && draggingIndex !== null && draggingIndex < index}
                />
              ))}
            </div>
          )}
        </div>
      </section>

      {/* Section 3: History (Collapsible) */}
      <section className={`queue-section history-section ${historyCollapsed ? "collapsed" : ""}`}>
        <div
          className="section-header clickable"
          onClick={() => setHistoryCollapsed(!historyCollapsed)}
        >
          <div className="section-title">
            <span className="title-text">HISTORY</span>
            {historyItems.length > 0 && (
              <span className="section-count">{historyItems.length}</span>
            )}
          </div>
          <div className="section-header-right">
            <button
              className="collapse-btn"
              title={historyCollapsed ? "Expand" : "Collapse"}
            >
              <svg
                width="16"
                height="16"
                viewBox="0 0 24 24"
                fill="currentColor"
                className={`collapse-icon ${historyCollapsed ? "collapsed" : ""}`}
              >
                <path d="M7.41 8.59L12 13.17l4.59-4.58L18 10l-6 6-6-6 1.41-1.41z"/>
              </svg>
            </button>
          </div>
        </div>

        {!historyCollapsed && (
          <>
            <div className="section-content">
              {historyItems.length === 0 ? (
                <div className="section-empty">
                  <span>No history yet</span>
                </div>
              ) : (
                <div className="history-list">
                  {historyItems.map((item) => (
                    <HistoryItem
                      key={item.uuid}
                      item={item}
                      onReplay={() => handleReplay(item)}
                    />
                  ))}
                </div>
              )}
            </div>

            {historyItems.length > 0 && (
              <div className="section-footer">
                <button className="clear-history-btn" onClick={handleClearHistory}>
                  Clear History
                </button>
              </div>
            )}
          </>
        )}
      </section>
    </div>
  );
}

export default QueueTab;
