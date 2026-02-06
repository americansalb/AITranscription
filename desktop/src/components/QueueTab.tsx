import React, { useEffect, useState } from "react";
import { useQueueStore } from "../lib/queueStore";
import * as queueStore from "../lib/queueStore";
import type { QueueItem } from "../lib/queueTypes";
import { queueSync } from "../lib/queueSync";
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

// Session color badge component (Feature 8)
function SessionBadge({ name, color }: { name?: string; color?: string }) {
  if (!name && !color) return null;
  return (
    <span className="session-badge" style={{ borderColor: color || '#3b82f6' }}>
      {color && <span className="session-badge-dot" style={{ backgroundColor: color }} />}
      <span className="session-badge-name">{name || 'Unknown'}</span>
    </span>
  );
}

// Batch count indicator (Feature 5)
function BatchIndicator({ count }: { count?: number }) {
  if (!count || count <= 1) return null;
  return (
    <span className="batch-indicator" title={`${count} messages combined`}>
      {count} msgs
    </span>
  );
}

// Priority badge (Feature 4)
function PriorityBadge({ priority }: { priority?: string }) {
  if (!priority || priority === 'normal') return null;
  return (
    <span className={`priority-badge priority-${priority}`}>
      {priority === 'critical' ? '!' : priority}
    </span>
  );
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

  // Listen for playback info from main window via Tauri events
  useEffect(() => {
    let cleanup: (() => void) | null = null;
    import("@tauri-apps/api/event").then(({ listen }) => {
      listen<{ currentTimeMs: number; durationMs: number }>("queue-playback-info", (event) => {
        setCurrentTime(event.payload.currentTimeMs);
        if (event.payload.durationMs > 0) {
          setDuration(event.payload.durationMs);
        }
      }).then((unlisten) => {
        cleanup = unlisten;
      });
    });
    return () => { cleanup?.(); };
  }, []);

  // Also poll local audio info (for main window) as fallback
  useEffect(() => {
    if (!isPlaying && !isPaused) return;

    const updateTime = () => {
      const info = queueStore.getAudioPlaybackInfo();
      if (info.currentTimeMs > 0 || info.durationMs > 0) {
        setCurrentTime(info.currentTimeMs);
        if (info.durationMs > 0) {
          setDuration(info.durationMs);
        }
      }
    };

    updateTime();
    const interval = setInterval(updateTime, 250);
    return () => clearInterval(interval);
  }, [isPlaying, isPaused]);

  const displayDuration = duration > 0
    ? duration
    : (item.durationMs || (item.text.split(/\s+/).length / 150) * 60 * 1000);
  const progress = displayDuration > 0 ? Math.min((currentTime / displayDuration) * 100, 100) : 0;

  return (
    <div className="now-playing-card">
      <div className="now-playing-card-header">
        <div className="now-playing-label">
          <span className={`speaker-animated ${isPlaying && !isPaused ? "active" : ""}`}>
            <span></span><span></span><span></span>
          </span>
          <span className="label-text">{isPaused ? "PAUSED" : "NOW PLAYING"}</span>
        </div>
        {/* Feature 8: Session identity in now playing */}
        <SessionBadge name={item.sessionName} color={item.sessionColor} />
      </div>

      <div className="now-playing-card-content">
        <div className="now-playing-text-full">{item.text}</div>
        <div className="now-playing-meta-row">
          <BatchIndicator count={item.batchCount} />
          <PriorityBadge priority={item.priority} />
        </div>
      </div>

      <div className="now-playing-progress-bar">
        <div
          className="progress-track"
          role="slider"
          aria-label="Seek audio position"
          aria-valuemin={0}
          aria-valuemax={Math.round(displayDuration)}
          aria-valuenow={Math.round(currentTime)}
          tabIndex={0}
          onClick={(e) => {
            const rect = e.currentTarget.getBoundingClientRect();
            const fraction = Math.max(0, Math.min(1, (e.clientX - rect.left) / rect.width));
            const seekMs = fraction * displayDuration;
            queueStore.seek(seekMs);
            setCurrentTime(seekMs);
          }}
          onKeyDown={(e) => {
            const step = displayDuration * 0.05; // 5% per key press
            if (e.key === "ArrowRight") {
              const newTime = Math.min(currentTime + step, displayDuration);
              queueStore.seek(newTime);
              setCurrentTime(newTime);
            } else if (e.key === "ArrowLeft") {
              const newTime = Math.max(currentTime - step, 0);
              queueStore.seek(newTime);
              setCurrentTime(newTime);
            }
          }}
          style={{ cursor: "pointer" }}
        >
          <div className="progress-fill" style={{ width: `${progress}%` }} />
          <div className="progress-thumb" style={{ left: `${progress}%` }} />
        </div>
        <div className="progress-times">
          <span>{formatTime(currentTime)}</span>
          <span>{formatTime(displayDuration)}</span>
        </div>
      </div>

      <div className="now-playing-controls">
        <div className="playback-controls">
          <button className="control-btn" onClick={() => queueStore.skipPrevious()} title="Previous">
            <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor">
              <path d="M6 6h2v12H6V6zm3.5 6l8.5 6V6l-8.5 6z"/>
            </svg>
          </button>

          <button className="control-btn primary" onClick={() => queueStore.togglePlayPause()} title={isPlaying && !isPaused ? "Pause" : "Play"}>
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

          <button className="control-btn" onClick={() => queueStore.skipNext()} title="Next">
            <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor">
              <path d="M6 18l8.5-6L6 6v12zm8.5 0V6h2v12h-2z"/>
            </svg>
          </button>

          <button className="control-btn" onClick={() => queueStore.stopPlayback()} title="Stop">
            <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor">
              <rect x="6" y="6" width="12" height="12"/>
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
              onChange={(e) => queueStore.setVolume(parseFloat(e.target.value))}
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

// Up Next Item Component
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
      <div className={`up-next-item ${isBeingDragged ? "dragging" : ""}`} data-index={index}>
        <div
          className="drag-handle"
          onMouseDown={(e) => {
            e.preventDefault();
            onStartDrag(index);
          }}
        >
          <span></span><span></span><span></span>
        </div>
        <div className="up-next-content">
          <div className="up-next-text">{item.text}</div>
          <div className="up-next-meta">
            {/* Feature 8: Session badge */}
            <SessionBadge name={item.sessionName} color={item.sessionColor} />
            <BatchIndicator count={item.batchCount} />
            <PriorityBadge priority={item.priority} />
            <span className="meta-time">{formatTimeAgo(item.createdAt)}</span>
          </div>
        </div>
        <div className="up-next-actions">
          <button className="action-btn play-btn" onClick={onPlay} title="Play now">
            <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor">
              <path d="M8 5v14l11-7L8 5z"/>
            </svg>
          </button>
          <button className="action-btn remove-btn" onClick={onRemove} title="Remove">
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
          {/* Feature 8: Session badge in history */}
          <SessionBadge name={item.sessionName} color={item.sessionColor} />
          {isCompleted && item.durationMs && (
            <span className="meta-duration">{(item.durationMs / 1000).toFixed(1)}s</span>
          )}
          {isFailed && (
            <button
              className="meta-error clickable"
              onClick={() => setShowError(!showError)}
              title="Click to show error details"
            >
              Error {showError ? "\u25B2" : "\u25BC"}
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
      <button className="action-btn replay-btn" onClick={onReplay} title="Replay">
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
  const isDraggingRef = React.useRef(false);

  // Feature 6: Use BroadcastChannel sync instead of 250ms polling.
  // Only do a periodic load as fallback, much less frequently.
  useEffect(() => {
    // Initial load
    queueStore.loadItems();

    // Listen for cross-window updates (instant)
    const unsubSync = queueSync.onRemoteUpdate(() => {
      if (!isDraggingRef.current) {
        queueStore.loadItems();
        forceUpdate((n) => n + 1);
      }
    });

    // Fallback polling at 2s (much less frequent, only for safety)
    const interval = setInterval(() => {
      if (isDraggingRef.current) return;
      queueStore.loadItems();
      forceUpdate((n) => n + 1);
    }, 2000);

    return () => {
      clearInterval(interval);
      unsubSync();
    };
  }, []);

  const playingItem = state.currentItem || state.items.find((i) => i.status === "playing");

  const pendingItems = state.items
    .filter((i) => i.status === "pending")
    .sort((a, b) => a.position - b.position);

  const historyItems = state.items
    .filter((i) => i.status === "completed" || i.status === "failed")
    .sort((a, b) => (b.completedAt || b.createdAt) - (a.completedAt || a.createdAt));

  const handlePlay = (uuid: string) => queueStore.playItem(uuid);
  const handleRemove = (uuid: string) => queueStore.removeItem(uuid);

  const handleReplay = async (item: QueueItem) => {
    await queueStore.addItem(item.text, item.sessionId);
  };

  const handleClearUpNext = async () => {
    for (const item of pendingItems) {
      await queueStore.removeItem(item.uuid);
    }
  };

  const handleClearHistory = () => queueStore.clearCompleted();

  // Mouse-based drag state
  const [draggingIndex, setDraggingIndex] = useState<number | null>(null);
  const [dropTargetIndex, setDropTargetIndex] = useState<number | null>(null);
  const listRef = React.useRef<HTMLDivElement>(null);
  const draggingIndexRef = React.useRef<number | null>(null);
  const dropTargetIndexRef = React.useRef<number | null>(null);
  const pendingItemsRef = React.useRef(pendingItems);

  React.useEffect(() => {
    pendingItemsRef.current = pendingItems;
  }, [pendingItems]);

  const handleStartDrag = (index: number) => {
    isDraggingRef.current = true;
    setDraggingIndex(index);
    draggingIndexRef.current = index;
  };

  useEffect(() => {
    if (draggingIndex === null) return;

    let lastDropTarget: number | null = null;
    let rafId: number | null = null;

    const handleMouseMove = (e: MouseEvent) => {
      if (rafId !== null) cancelAnimationFrame(rafId);
      rafId = requestAnimationFrame(() => {
        if (!listRef.current) return;
        const items = listRef.current.querySelectorAll('.up-next-item');
        let newDropTarget: number | null = null;
        items.forEach((item, idx) => {
          const rect = item.getBoundingClientRect();
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
      if (rafId !== null) cancelAnimationFrame(rafId);

      const currentDragging = draggingIndexRef.current;
      const currentDropTarget = dropTargetIndexRef.current;
      const items = pendingItemsRef.current;

      if (currentDragging !== null && currentDropTarget !== null && currentDragging !== currentDropTarget) {
        const sourceItem = items[currentDragging];
        const targetItem = items[currentDropTarget];
        if (sourceItem && targetItem) {
          try {
            await queueStore.reorderItem(sourceItem.uuid, targetItem.position);
          } catch (err) {
            console.error("[QueueTab] Reorder failed:", err);
          }
        }
      }

      isDraggingRef.current = false;
      draggingIndexRef.current = null;
      dropTargetIndexRef.current = null;
      setDraggingIndex(null);
      setDropTargetIndex(null);
    };

    document.addEventListener('mousemove', handleMouseMove, { passive: true });
    document.addEventListener('mouseup', handleMouseUp);

    return () => {
      if (rafId !== null) cancelAnimationFrame(rafId);
      document.removeEventListener('mousemove', handleMouseMove);
      document.removeEventListener('mouseup', handleMouseUp);
    };
  }, [draggingIndex]);

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

      {/* Section 3: History */}
      <section className={`queue-section history-section ${historyCollapsed ? "collapsed" : ""}`}>
        <div className="section-header clickable" onClick={() => setHistoryCollapsed(!historyCollapsed)}>
          <div className="section-title">
            <span className="title-text">HISTORY</span>
            {historyItems.length > 0 && (
              <span className="section-count">{historyItems.length}</span>
            )}
          </div>
          <div className="section-header-right">
            <button className="collapse-btn" title={historyCollapsed ? "Expand" : "Collapse"}>
              <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor" className={`collapse-icon ${historyCollapsed ? "collapsed" : ""}`}>
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
