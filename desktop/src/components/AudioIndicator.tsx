import { useState, useRef, useCallback, useEffect } from "react";

interface AudioIndicatorProps {
  isRecording: boolean;
  isProcessing: boolean;
  audioLevel: number; // 0-1 normalized
  onCancel?: () => void;
}

// LocalStorage key for position
const POSITION_KEY = "vaak_audio_indicator_position";

interface Position {
  x: number;
  y: number;
}

function loadPosition(): Position | null {
  try {
    const stored = localStorage.getItem(POSITION_KEY);
    if (stored) {
      return JSON.parse(stored);
    }
  } catch (e) {
    console.warn("Failed to load audio indicator position:", e);
  }
  return null;
}

function savePosition(pos: Position): void {
  try {
    localStorage.setItem(POSITION_KEY, JSON.stringify(pos));
  } catch (e) {
    console.warn("Failed to save audio indicator position:", e);
  }
}

/**
 * Minimal floating audio indicator bar - shows recording status
 * with audio level visualization (like Wispr Flow)
 * Now draggable and with cancel button!
 */
export function AudioIndicator({ isRecording, isProcessing, audioLevel, onCancel }: AudioIndicatorProps) {
  const [position, setPosition] = useState<Position | null>(loadPosition);
  const [isDragging, setIsDragging] = useState(false);
  const dragRef = useRef<{ startX: number; startY: number; startPosX: number; startPosY: number } | null>(null);
  const indicatorRef = useRef<HTMLDivElement>(null);

  // Handle mouse down to start dragging
  const handleMouseDown = useCallback((e: React.MouseEvent) => {
    // Don't start drag if clicking on cancel button
    if ((e.target as HTMLElement).closest(".audio-indicator-cancel")) {
      return;
    }

    e.preventDefault();
    const rect = indicatorRef.current?.getBoundingClientRect();
    if (!rect) return;

    setIsDragging(true);
    dragRef.current = {
      startX: e.clientX,
      startY: e.clientY,
      startPosX: position?.x ?? rect.left,
      startPosY: position?.y ?? rect.top,
    };
  }, [position]);

  // Handle mouse move while dragging
  useEffect(() => {
    if (!isDragging) return;

    const handleMouseMove = (e: MouseEvent) => {
      if (!dragRef.current) return;

      const deltaX = e.clientX - dragRef.current.startX;
      const deltaY = e.clientY - dragRef.current.startY;

      const newX = dragRef.current.startPosX + deltaX;
      const newY = dragRef.current.startPosY + deltaY;

      // Constrain to viewport
      const maxX = window.innerWidth - 200;
      const maxY = window.innerHeight - 50;

      setPosition({
        x: Math.max(0, Math.min(newX, maxX)),
        y: Math.max(0, Math.min(newY, maxY)),
      });
    };

    const handleMouseUp = () => {
      setIsDragging(false);
      if (position) {
        savePosition(position);
      }
    };

    window.addEventListener("mousemove", handleMouseMove);
    window.addEventListener("mouseup", handleMouseUp);

    return () => {
      window.removeEventListener("mousemove", handleMouseMove);
      window.removeEventListener("mouseup", handleMouseUp);
    };
  }, [isDragging, position]);

  if (!isRecording && !isProcessing) return null;

  const style: React.CSSProperties = position
    ? {
        left: position.x,
        top: position.y,
        bottom: "auto",
        transform: "none",
      }
    : {};

  return (
    <div
      ref={indicatorRef}
      className={`audio-indicator ${isDragging ? "dragging" : ""}`}
      style={style}
      onMouseDown={handleMouseDown}
    >
      <div className="audio-indicator-content">
        {isProcessing ? (
          <>
            <div className="audio-indicator-spinner" />
            <span className="audio-indicator-text">Processing...</span>
          </>
        ) : (
          <>
            <div className="audio-indicator-dot" />
            <div className="audio-indicator-bars">
              {[...Array(5)].map((_, i) => (
                <div
                  key={i}
                  className="audio-indicator-bar"
                  style={{
                    height: `${Math.max(4, audioLevel * 20 * (0.5 + Math.random() * 0.5))}px`,
                    opacity: audioLevel > 0.05 ? 1 : 0.3,
                  }}
                />
              ))}
            </div>
            <span className="audio-indicator-text">Recording</span>
            {onCancel && (
              <button
                className="audio-indicator-cancel"
                onClick={(e) => {
                  e.stopPropagation();
                  onCancel();
                }}
                title="Cancel recording"
              >
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                  <line x1="18" y1="6" x2="6" y2="18" />
                  <line x1="6" y1="6" x2="18" y2="18" />
                </svg>
              </button>
            )}
          </>
        )}
      </div>
      <div className="audio-indicator-drag-hint">Drag to move</div>
    </div>
  );
}
