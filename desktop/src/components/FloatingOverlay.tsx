import { useEffect, useState, useRef, useCallback } from "react";

const POSITION_STORAGE_KEY = "scribe_overlay_position";

interface Position {
  x: number;
  y: number;
}

function loadSavedPosition(): Position | null {
  try {
    const saved = localStorage.getItem(POSITION_STORAGE_KEY);
    if (saved) {
      return JSON.parse(saved);
    }
  } catch {
    // Ignore parse errors
  }
  return null;
}

function savePosition(pos: Position): void {
  try {
    localStorage.setItem(POSITION_STORAGE_KEY, JSON.stringify(pos));
  } catch {
    // Ignore storage errors
  }
}

/**
 * Minimal floating recording indicator - Wispr Flow-inspired design.
 * Shows a clean, modern pill when recording/processing.
 * Draggable with position memory.
 */
export function FloatingOverlay() {
  const [isRecording, setIsRecording] = useState(false);
  const [isProcessing, setIsProcessing] = useState(false);
  const [audioLevel, setAudioLevel] = useState(0);
  const [duration, setDuration] = useState(0);
  const [isDragging, setIsDragging] = useState(false);
  const dragStartRef = useRef<{ x: number; y: number } | null>(null);
  const windowPosRef = useRef<Position | null>(null);

  // Restore saved position on mount, or position at top-right by default
  useEffect(() => {
    const initPosition = async () => {
      if (typeof window !== "undefined" && "__TAURI__" in window) {
        try {
          const { getCurrentWindow, availableMonitors, PhysicalPosition } = await import("@tauri-apps/api/window");
          const appWindow = getCurrentWindow();

          const savedPos = loadSavedPosition();
          if (savedPos) {
            // Restore saved position
            await appWindow.setPosition(new PhysicalPosition(savedPos.x, savedPos.y));
          } else {
            // Position at top-right by default
            const monitors = await availableMonitors();
            if (monitors.length > 0) {
              const primary = monitors[0];
              const screenWidth = primary.size.width;
              const windowWidth = 140;
              const margin = 16;
              const topOffset = 50; // Below where title bar/X button would be
              const x = screenWidth - windowWidth - margin;
              const y = topOffset;
              await appWindow.setPosition(new PhysicalPosition(x, y));
              savePosition({ x, y });
            }
          }
        } catch (e) {
          console.error("Failed to init overlay position:", e);
        }
      }
    };
    initPosition();
  }, []);

  // Listen for Tauri events from main window
  useEffect(() => {
    let unlisten: (() => void) | undefined;

    const setupListener = async () => {
      if (typeof window !== "undefined" && "__TAURI__" in window) {
        const { listen } = await import("@tauri-apps/api/event");
        unlisten = await listen<{
          isRecording: boolean;
          isProcessing: boolean;
          duration: number;
          audioLevel: number;
        }>("recording-state", (event) => {
          setIsRecording(event.payload.isRecording);
          setIsProcessing(event.payload.isProcessing);
          setAudioLevel(event.payload.audioLevel);
          setDuration(event.payload.duration);
        });
      }
    };

    setupListener();
    return () => {
      if (unlisten) unlisten();
    };
  }, []);

  // Handle drag start
  const handleMouseDown = useCallback(async (e: React.MouseEvent) => {
    if (typeof window !== "undefined" && "__TAURI__" in window) {
      try {
        const { getCurrentWindow } = await import("@tauri-apps/api/window");
        const appWindow = getCurrentWindow();
        const pos = await appWindow.outerPosition();
        windowPosRef.current = { x: pos.x, y: pos.y };
        dragStartRef.current = { x: e.screenX, y: e.screenY };
        setIsDragging(true);
      } catch (e) {
        console.error("Failed to start drag:", e);
      }
    }
  }, []);

  // Handle drag
  useEffect(() => {
    if (!isDragging) return;

    const handleMouseMove = async (e: MouseEvent) => {
      if (!dragStartRef.current || !windowPosRef.current) return;

      const deltaX = e.screenX - dragStartRef.current.x;
      const deltaY = e.screenY - dragStartRef.current.y;
      const newX = windowPosRef.current.x + deltaX;
      const newY = windowPosRef.current.y + deltaY;

      if (typeof window !== "undefined" && "__TAURI__" in window) {
        try {
          const { getCurrentWindow, PhysicalPosition } = await import("@tauri-apps/api/window");
          const appWindow = getCurrentWindow();
          await appWindow.setPosition(new PhysicalPosition(newX, newY));
        } catch {
          // Ignore position errors during drag
        }
      }
    };

    const handleMouseUp = async () => {
      setIsDragging(false);
      dragStartRef.current = null;

      // Save final position
      if (typeof window !== "undefined" && "__TAURI__" in window) {
        try {
          const { getCurrentWindow } = await import("@tauri-apps/api/window");
          const appWindow = getCurrentWindow();
          const pos = await appWindow.outerPosition();
          savePosition({ x: pos.x, y: pos.y });
        } catch {
          // Ignore save errors
        }
      }
    };

    document.addEventListener("mousemove", handleMouseMove);
    document.addEventListener("mouseup", handleMouseUp);

    return () => {
      document.removeEventListener("mousemove", handleMouseMove);
      document.removeEventListener("mouseup", handleMouseUp);
    };
  }, [isDragging]);

  // Format duration as m:ss
  const formatDuration = (secs: number) => {
    const m = Math.floor(secs / 60);
    const s = secs % 60;
    return `${m}:${s.toString().padStart(2, "0")}`;
  };

  // Generate 4 bar heights based on audio level for visualizer
  const bars = [
    Math.max(0.2, audioLevel * 0.6 + Math.sin(Date.now() / 200) * 0.1),
    Math.max(0.3, audioLevel * 0.9 + Math.sin(Date.now() / 150 + 1) * 0.1),
    Math.max(0.3, audioLevel + Math.sin(Date.now() / 180 + 2) * 0.1),
    Math.max(0.2, audioLevel * 0.7 + Math.sin(Date.now() / 160 + 3) * 0.1),
  ];

  // Determine state
  const isActive = isRecording || isProcessing;

  return (
    <div
      className={`floating-overlay-container ${isDragging ? "dragging" : ""}`}
      onMouseDown={handleMouseDown}
    >
      <div className={`floating-pill ${isRecording ? "recording" : ""} ${isProcessing ? "processing" : ""}`}>
        {/* Recording indicator dot */}
        <div className="floating-dot" />

        {/* Audio visualizer bars */}
        {isRecording && (
          <div className="floating-bars">
            {bars.map((h, i) => (
              <div
                key={i}
                className="floating-bar"
                style={{ height: `${Math.min(h, 1) * 100}%` }}
              />
            ))}
          </div>
        )}

        {/* Processing spinner */}
        {isProcessing && (
          <div className="floating-spinner" />
        )}

        {/* Duration text */}
        {isActive && (
          <span className="floating-text">
            {isProcessing ? "..." : formatDuration(duration)}
          </span>
        )}
      </div>
    </div>
  );
}
