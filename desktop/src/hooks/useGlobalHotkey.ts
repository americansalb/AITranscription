import { useEffect, useRef } from "react";

// Dynamic import for Tauri - will be undefined in browser
let tauriGlobalShortcut: typeof import("@tauri-apps/plugin-global-shortcut") | null = null;

// Try to load Tauri global shortcut plugin
async function loadTauriPlugin() {
  try {
    if (typeof window !== "undefined" && "__TAURI__" in window) {
      console.log("Tauri detected, loading global shortcut plugin...");
      tauriGlobalShortcut = await import("@tauri-apps/plugin-global-shortcut");
      console.log("Global shortcut plugin loaded successfully");
      return true;
    } else {
      console.log("Tauri not detected in window");
    }
  } catch (err) {
    console.error("Failed to load Tauri global shortcut plugin:", err);
  }
  return false;
}

export interface UseGlobalHotkeyOptions {
  /** Hotkey combination (e.g., "CommandOrControl+Shift+Space") */
  hotkey: string;
  /** Callback when key is pressed */
  onKeyDown?: () => void;
  /** Callback when key is released (for push-to-talk) */
  onKeyUp?: () => void;
  /** Whether the hotkey is enabled */
  enabled?: boolean;
}

/**
 * Hook for registering global hotkeys via Tauri.
 * Falls back gracefully when not running in Tauri.
 */
export function useGlobalHotkey({
  hotkey,
  onKeyDown,
  onKeyUp,
  enabled = true,
}: UseGlobalHotkeyOptions) {
  const isRegistered = useRef(false);
  const isTauriLoaded = useRef(false);
  const isRegistering = useRef(false); // Prevent concurrent registration attempts
  const mountedRef = useRef(true);
  const keyDownRef = useRef(onKeyDown);
  const keyUpRef = useRef(onKeyUp);
  const currentHotkeyRef = useRef(hotkey);

  // Keep callback refs up to date
  keyDownRef.current = onKeyDown;
  keyUpRef.current = onKeyUp;
  currentHotkeyRef.current = hotkey;

  useEffect(() => {
    mountedRef.current = true;

    const register = async () => {
      if (!enabled || isRegistered.current || isRegistering.current) return;

      isRegistering.current = true;

      try {
        // Load Tauri plugin if not already loaded
        if (!isTauriLoaded.current) {
          isTauriLoaded.current = await loadTauriPlugin();
        }

        // Check if still mounted after async load
        if (!mountedRef.current) {
          isRegistering.current = false;
          return;
        }

        if (!tauriGlobalShortcut) {
          console.log("Global hotkeys not available (not running in Tauri)");
          isRegistering.current = false;
          return;
        }

        console.log(`Attempting to register hotkey: ${hotkey}`);
        await tauriGlobalShortcut.register(hotkey, (event) => {
          // Check if still mounted before calling callbacks
          if (!mountedRef.current) return;

          console.log(`Hotkey event received:`, event);
          if (event.state === "Pressed") {
            console.log("Key PRESSED - starting recording");
            keyDownRef.current?.();
          } else if (event.state === "Released") {
            console.log("Key RELEASED - stopping recording");
            keyUpRef.current?.();
          }
        });

        // Check if still mounted after registration
        if (mountedRef.current) {
          isRegistered.current = true;
          console.log(`Global hotkey registered successfully: ${hotkey}`);
        } else {
          // Component unmounted during registration, clean up
          try {
            await tauriGlobalShortcut.unregister(hotkey);
          } catch (e) {
            // Ignore cleanup errors
          }
        }
      } catch (error) {
        console.error(`Failed to register global hotkey: ${hotkey}`, error);
      } finally {
        isRegistering.current = false;
      }
    };

    register();

    return () => {
      mountedRef.current = false;

      // Unregister on cleanup
      const unregister = async () => {
        if (!isRegistered.current || !tauriGlobalShortcut) return;

        try {
          await tauriGlobalShortcut.unregister(currentHotkeyRef.current);
          isRegistered.current = false;
          console.log(`Global hotkey unregistered: ${currentHotkeyRef.current}`);
        } catch (error) {
          console.error(`Failed to unregister global hotkey: ${currentHotkeyRef.current}`, error);
        }
      };

      unregister();
    };
  }, [hotkey, enabled]);

  return { isRegistered: isRegistered.current };
}

/**
 * Common hotkey combinations
 */
export const HOTKEYS = {
  PUSH_TO_TALK: "CommandOrControl+Shift+S",
  TOGGLE_RECORDING: "CommandOrControl+Shift+R",
  CANCEL_RECORDING: "Escape",
} as const;
