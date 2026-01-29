import React from "react";
import type { QueueItem as QueueItemType } from "../lib/queueTypes";

interface QueueItemProps {
  item: QueueItemType;
  onDragStart: (e: React.DragEvent, uuid: string) => void;
  onDragOver: (e: React.DragEvent) => void;
  onDrop: (e: React.DragEvent, targetUuid: string) => void;
  onPlay?: (uuid: string) => void;
}

// Format relative time
function formatTimeAgo(timestamp: number): string {
  const seconds = Math.floor((Date.now() - timestamp) / 1000);

  if (seconds < 60) return "just now";
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m ago`;
  if (seconds < 86400) return `${Math.floor(seconds / 3600)}h ago`;
  return `${Math.floor(seconds / 86400)}d ago`;
}

// Get status icon
function getStatusIcon(status: string): string {
  switch (status) {
    case "pending":
      return "⏱"; // Clock
    case "playing":
      return "🔊"; // Speaker
    case "paused":
      return "⏸"; // Pause
    case "completed":
      return "✓"; // Checkmark
    case "failed":
      return "✗"; // X
    default:
      return "⏱";
  }
}

export function QueueItemComponent({
  item,
  onDragStart,
  onDragOver,
  onDrop,
  onPlay,
}: QueueItemProps) {
  const handleDragStart = (e: React.DragEvent) => {
    e.currentTarget.classList.add("dragging");
    onDragStart(e, item.uuid);
  };

  const handleDragEnd = (e: React.DragEvent) => {
    e.currentTarget.classList.remove("dragging");
  };

  const handleDragOver = (e: React.DragEvent) => {
    e.preventDefault();
    e.dataTransfer.dropEffect = "move";
    onDragOver(e);
  };

  const handleDrop = (e: React.DragEvent) => {
    e.preventDefault();
    onDrop(e, item.uuid);
  };

  const handleDoubleClick = () => {
    if (onPlay && item.status === "pending") {
      onPlay(item.uuid);
    }
  };

  return (
    <div
      className={`queue-item ${item.status}`}
      draggable={item.status === "pending"}
      onDragStart={handleDragStart}
      onDragEnd={handleDragEnd}
      onDragOver={handleDragOver}
      onDrop={handleDrop}
      onDoubleClick={handleDoubleClick}
    >
      {item.status === "pending" && (
        <div className="queue-item-drag-handle">
          <span />
          <span />
          <span />
        </div>
      )}

      <div className="queue-item-content">
        <div className="queue-item-text" title={item.text}>
          {item.text}
        </div>
        <div className="queue-item-meta">
          <span className="queue-item-session">
            {item.sessionId.substring(0, 12)}...
          </span>
          <span className="queue-item-time">
            {formatTimeAgo(item.createdAt)}
          </span>
        </div>
      </div>

      <div className={`queue-item-status ${item.status}`}>
        {getStatusIcon(item.status)}
      </div>
    </div>
  );
}

export default QueueItemComponent;
