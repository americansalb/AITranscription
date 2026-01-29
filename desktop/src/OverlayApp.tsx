import { useState, useEffect, useRef, useCallback } from "react";

interface OverlayState {
  isRecording: boolean;
  isProcessing: boolean;
  duration: number;
  audioLevel: number;
}

/**
 * Beautiful floating recording indicator overlay.
 * Runs in a separate Tauri window (160x48, transparent, always-on-top).
 * Shows audio-reactive visualizer bars when recording, spinner when processing.
 * Draggable to reposition, saves position across sessions.
 */
export function OverlayApp() {
  const [state, setState] = useState<OverlayState>({
    isRecording: false,
    isProcessing: false,
    duration: 0,
    audioLevel: 0,
  });

  // Animation frame for smooth bar updates
  const animFrameRef = useRef<number>(0);
  const [animBars, setAnimBars] = useState([0.15, 0.2, 0.25, 0.2, 0.15]);

  // Drag support
  const [isDragging, setIsDragging] = useState(false);
  const dragStartRef = useRef<{ x: number; y: number } | null>(null);
  const windowPosRef = useRef<{ x: number; y: number } | null>(null);

  // Listen for events from main window
  useEffect(() => {
    let unlisten: (() => void) | undefined;

    const setupListener = async () => {
      if (window.__TAURI__) {
        try {
          const { listen } = await import("@tauri-apps/api/event");
          unlisten = await listen<OverlayState>("overlay-update", (event) => {
            setState(event.payload);
          });
        } catch (e) {
          console.error("Failed to setup overlay listener:", e);
        }
      }
    };

    setupListener();
    return () => {
      if (unlisten) unlisten();
    };
  }, []);

  // Animate bars smoothly
  useEffect(() => {
    if (!state.isRecording) {
      setAnimBars([0.15, 0.2, 0.25, 0.2, 0.15]);
      return;
    }

    let running = true;
    const animate = () => {
      if (!running) return;
      const t = Date.now();
      const level = state.audioLevel;
      setAnimBars([
        Math.max(0.1, level * 0.5 + Math.sin(t / 220) * 0.12),
        Math.max(0.15, level * 0.8 + Math.sin(t / 180 + 1.2) * 0.1),
        Math.max(0.2, level * 1.0 + Math.sin(t / 150 + 2.4) * 0.08),
        Math.max(0.15, level * 0.75 + Math.sin(t / 190 + 3.6) * 0.1),
        Math.max(0.1, level * 0.45 + Math.sin(t / 210 + 4.8) * 0.12),
      ]);
      animFrameRef.current = requestAnimationFrame(animate);
    };
    animFrameRef.current = requestAnimationFrame(animate);

    return () => {
      running = false;
      cancelAnimationFrame(animFrameRef.current);
    };
  }, [state.isRecording, state.audioLevel]);

  // Drag handlers
  const handleMouseDown = useCallback(async (e: React.MouseEvent) => {
    if (window.__TAURI__) {
      try {
        const { getCurrentWindow } = await import("@tauri-apps/api/window");
        const appWindow = getCurrentWindow();
        const pos = await appWindow.outerPosition();
        windowPosRef.current = { x: pos.x, y: pos.y };
        dragStartRef.current = { x: e.screenX, y: e.screenY };
        setIsDragging(true);
      } catch {
        // ignore
      }
    }
  }, []);

  useEffect(() => {
    if (!isDragging) return;

    const handleMouseMove = async (e: MouseEvent) => {
      if (!dragStartRef.current || !windowPosRef.current) return;
      const dx = e.screenX - dragStartRef.current.x;
      const dy = e.screenY - dragStartRef.current.y;
      try {
        const { getCurrentWindow, PhysicalPosition } = await import("@tauri-apps/api/window");
        await getCurrentWindow().setPosition(
          new PhysicalPosition(windowPosRef.current.x + dx, windowPosRef.current.y + dy)
        );
      } catch {
        // ignore
      }
    };

    const handleMouseUp = async () => {
      setIsDragging(false);
      dragStartRef.current = null;
      try {
        const { getCurrentWindow } = await import("@tauri-apps/api/window");
        const pos = await getCurrentWindow().outerPosition();
        localStorage.setItem(
          "vaak_overlay_position",
          JSON.stringify({ x: pos.x, y: pos.y })
        );
      } catch {
        // ignore
      }
    };

    document.addEventListener("mousemove", handleMouseMove);
    document.addEventListener("mouseup", handleMouseUp);
    return () => {
      document.removeEventListener("mousemove", handleMouseMove);
      document.removeEventListener("mouseup", handleMouseUp);
    };
  }, [isDragging]);

  // Force transparent background on html/body/#root for this window
  useEffect(() => {
    document.documentElement.style.background = "transparent";
    document.body.style.background = "transparent";
    const root = document.getElementById("root");
    if (root) root.style.background = "transparent";
  }, []);

  // Restore position on mount
  useEffect(() => {
    const restore = async () => {
      if (!window.__TAURI__) return;
      try {
        const { getCurrentWindow, PhysicalPosition, availableMonitors } =
          await import("@tauri-apps/api/window");
        const saved = localStorage.getItem("vaak_overlay_position");
        if (saved) {
          const { x, y } = JSON.parse(saved);
          await getCurrentWindow().setPosition(new PhysicalPosition(x, y));
        } else {
          const monitors = await availableMonitors();
          if (monitors.length > 0) {
            const x = Math.round((monitors[0].size.width - 160) / 2);
            await getCurrentWindow().setPosition(new PhysicalPosition(x, 8));
          }
        }
      } catch {
        // ignore
      }
    };
    restore();
  }, []);

  const formatDuration = (secs: number) => {
    const m = Math.floor(secs / 60);
    const s = secs % 60;
    return `${m}:${s.toString().padStart(2, "0")}`;
  };

  const isActive = state.isRecording || state.isProcessing;

  return (
    <div
      className="overlay-root"
      onMouseDown={handleMouseDown}
      style={{ cursor: isDragging ? "grabbing" : "grab" }}
    >
      <div className={`overlay-capsule ${state.isRecording ? "rec" : ""} ${state.isProcessing ? "proc" : ""} ${isActive ? "active" : "idle"}`}>
        {/* Recording: dot + bars + duration */}
        {state.isRecording && (
          <>
            <div className="overlay-rec-dot" />
            <div className="overlay-bars">
              {animBars.map((h, i) => (
                <div
                  key={i}
                  className="overlay-bar"
                  style={{ height: `${Math.min(h, 1) * 100}%` }}
                />
              ))}
            </div>
            <span className="overlay-label">{formatDuration(state.duration)}</span>
          </>
        )}

        {/* Processing: spinner + text */}
        {state.isProcessing && (
          <>
            <div className="overlay-proc-spinner" />
            <span className="overlay-label">Processing</span>
          </>
        )}

        {/* Idle: subtle ready dot */}
        {!isActive && (
          <>
            <div className="overlay-idle-dot" />
            <span className="overlay-label idle-label">Vaak</span>
          </>
        )}
      </div>
    </div>
  );
}
