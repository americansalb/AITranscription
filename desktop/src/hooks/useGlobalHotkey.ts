import { useEffect, useRef, useCallback, useState } from "react";

// Dynamic import for Tauri - will be undefined in browser
let tauriGlobalShortcut: typeof import("@tauri-apps/plugin-global-shortcut") | null = null;

// Try to load Tauri global shortcut plugin
async function loadTauriPlugin() {
  try {
    if (typeof window !== "undefined" && "__TAURI__" in window) {
      tauriGlobalShortcut = await import("@tauri-apps/plugin-global-shortcut");
      return true;
    }
  } catch {
    // Not running in Tauri environment
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
  const [registrationError, setRegistrationError] = useState<string | null>(null);
  const [registered, setRegistered] = useState(false);
  const isRegistered = useRef(false);
  const isTauriLoaded = useRef(false);
  const keyDownRef = useRef(onKeyDown);
  const keyUpRef = useRef(onKeyUp);

  // Keep callback refs up to date
  keyDownRef.current = onKeyDown;
  keyUpRef.current = onKeyUp;

  const register = useCallback(async () => {
    if (!enabled || isRegistered.current) return;

    // Load Tauri plugin if not already loaded
    if (!isTauriLoaded.current) {
      isTauriLoaded.current = await loadTauriPlugin();
    }

    if (!tauriGlobalShortcut) {
      setRegistrationError("Global hotkeys not available (not running in Tauri)");
      return;
    }

    try {
      await tauriGlobalShortcut.register(hotkey, (event) => {
        if (event.state === "Pressed") {
          keyDownRef.current?.();
        } else if (event.state === "Released") {
          keyUpRef.current?.();
        }
      });
      isRegistered.current = true;
      setRegistered(true);
      setRegistrationError(null);
      console.log(`Global hotkey registered: ${hotkey}`);
    } catch (error) {
      const errorMsg = error instanceof Error ? error.message : String(error);
      setRegistrationError(`Hotkey failed: ${errorMsg}`);
      console.error(`Failed to register global hotkey: ${hotkey}`, error);
    }
  }, [hotkey, enabled]);

  const unregister = useCallback(async () => {
    if (!isRegistered.current || !tauriGlobalShortcut) return;

    try {
      await tauriGlobalShortcut.unregister(hotkey);
      isRegistered.current = false;
      setRegistered(false);
      console.log(`Global hotkey unregistered: ${hotkey}`);
    } catch (error) {
      console.error(`Failed to unregister global hotkey: ${hotkey}`, error);
    }
  }, [hotkey]);

  useEffect(() => {
    register();
    return () => {
      unregister();
    };
  }, [register, unregister]);

  return { isRegistered: registered, error: registrationError };
}

/**
 * Common hotkey combinations
 *
 * Push-to-talk: Ctrl+Space (hold to record, release to transcribe)
 * - Intuitive: "speak into space"
 * - Easy to hold with one hand
 * - Note: May conflict with input method switching for CJK users
 */
export const HOTKEYS = {
  PUSH_TO_TALK: "Control+Space",
  TOGGLE_RECORDING: "CommandOrControl+Shift+R",
  CANCEL_RECORDING: "Escape",
} as const;
