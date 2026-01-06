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
  const isTauriLoaded = useRef(false);
  const keyDownRef = useRef(onKeyDown);
  const keyUpRef = useRef(onKeyUp);
  const registeredHotkeyRef = useRef<string | null>(null);

  // Keep callback refs up to date
  keyDownRef.current = onKeyDown;
  keyUpRef.current = onKeyUp;

  const unregisterHotkey = useCallback(async (hotkeyToUnregister: string) => {
    if (!tauriGlobalShortcut || !hotkeyToUnregister) return;

    try {
      await tauriGlobalShortcut.unregister(hotkeyToUnregister);
      console.log(`Global hotkey unregistered: ${hotkeyToUnregister}`);
    } catch (error) {
      console.error(`Failed to unregister global hotkey: ${hotkeyToUnregister}`, error);
    }
  }, []);

  const registerHotkey = useCallback(async (hotkeyToRegister: string) => {
    if (!tauriGlobalShortcut) return false;

    try {
      await tauriGlobalShortcut.register(hotkeyToRegister, (event) => {
        if (event.state === "Pressed") {
          keyDownRef.current?.();
        } else if (event.state === "Released") {
          keyUpRef.current?.();
        }
      });
      console.log(`Global hotkey registered: ${hotkeyToRegister}`);
      return true;
    } catch (error) {
      const errorMsg = error instanceof Error ? error.message : String(error);
      setRegistrationError(`Hotkey failed: ${errorMsg}`);
      console.error(`Failed to register global hotkey: ${hotkeyToRegister}`, error);
      return false;
    }
  }, []);

  useEffect(() => {
    let mounted = true;

    const setupHotkey = async () => {
      if (!enabled) {
        // Unregister if disabled
        if (registeredHotkeyRef.current) {
          await unregisterHotkey(registeredHotkeyRef.current);
          registeredHotkeyRef.current = null;
          setRegistered(false);
        }
        return;
      }

      // Load Tauri plugin if not already loaded
      if (!isTauriLoaded.current) {
        isTauriLoaded.current = await loadTauriPlugin();
      }

      if (!tauriGlobalShortcut) {
        setRegistrationError("Global hotkeys not available (not running in Tauri)");
        return;
      }

      // If hotkey changed, unregister the old one first
      if (registeredHotkeyRef.current && registeredHotkeyRef.current !== hotkey) {
        await unregisterHotkey(registeredHotkeyRef.current);
        registeredHotkeyRef.current = null;
        setRegistered(false);
      }

      // Register new hotkey if not already registered
      if (registeredHotkeyRef.current !== hotkey) {
        const success = await registerHotkey(hotkey);
        if (mounted && success) {
          registeredHotkeyRef.current = hotkey;
          setRegistered(true);
          setRegistrationError(null);
        }
      }
    };

    setupHotkey();

    return () => {
      mounted = false;
      // Cleanup on unmount
      if (registeredHotkeyRef.current) {
        unregisterHotkey(registeredHotkeyRef.current);
        registeredHotkeyRef.current = null;
      }
    };
  }, [hotkey, enabled, registerHotkey, unregisterHotkey]);

  return { isRegistered: registered, error: registrationError };
}

/**
 * Common hotkey combinations
 *
 * Push-to-talk: Alt+D (Option+D on Mac)
 * - "D" for Dictate - easy to remember
 * - Feels like a dedicated tool, not a system shortcut
 * - One-handed: thumb on Alt/Option, finger on D
 */
export const HOTKEYS = {
  PUSH_TO_TALK: "Alt+D",
  TOGGLE_RECORDING: "CommandOrControl+Shift+R",
  CANCEL_RECORDING: "Escape",
} as const;
