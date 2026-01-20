import { useEffect, useRef, useState, useCallback } from "react";

// Dynamic import for Tauri - will be undefined in browser
let tauriGlobalShortcut: typeof import("@tauri-apps/plugin-global-shortcut") | null = null;

// Try to load Tauri plugin
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
  const [permissionGranted, setPermissionGranted] = useState<boolean | null>(null);
  const keyDownRef = useRef(onKeyDown);
  const keyUpRef = useRef(onKeyUp);
  const currentHotkeyRef = useRef(hotkey);

  // Keep callback refs up to date
  keyDownRef.current = onKeyDown;
  keyUpRef.current = onKeyUp;
  currentHotkeyRef.current = hotkey;

  // Check if running on macOS
  const isMac = typeof navigator !== "undefined" && navigator.platform.toUpperCase().indexOf("MAC") >= 0;

  // Check accessibility permissions (macOS only)
  const checkPermissions = useCallback(async () => {
    // Only check on macOS when running in Tauri
    if (!isMac || !tauriGlobalShortcut) {
      setPermissionGranted(true);
      return true;
    }

    try {
      const { invoke } = await import("@tauri-apps/api/core");
      const granted = await invoke<boolean>("check_accessibility_permission");
      setPermissionGranted(granted);

      if (!granted) {
        console.error(
          "❌ Accessibility permission denied. Global hotkeys will not work.\n" +
          "Please enable Scribe in System Settings > Privacy & Security > Accessibility"
        );
      } else {
        console.log("✅ Accessibility permission granted");
      }

      return granted;
    } catch (error) {
      console.error("Failed to check accessibility permission:", error);
      setPermissionGranted(false);
      return false;
    }
  }, [isMac]);

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

        // Check permissions first (macOS only)
        const hasPermission = await checkPermissions();
        if (!hasPermission) {
          console.error(
            `❌ Cannot register hotkey ${hotkey} - accessibility permission denied.\n` +
            (isMac
              ? "Go to System Settings > Privacy & Security > Accessibility and enable Scribe."
              : "Permission check failed.")
          );
          isRegistering.current = false;
          return;
        }

        // Check if still mounted after permission check
        if (!mountedRef.current) {
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
          console.log(`✅ Global hotkey registered successfully: ${hotkey}`);
        } else {
          // Component unmounted during registration, clean up
          try {
            await tauriGlobalShortcut.unregister(hotkey);
          } catch (e) {
            // Ignore cleanup errors
          }
        }
      } catch (error) {
        console.error(`❌ Failed to register global hotkey: ${hotkey}`, error);

        // Platform-specific error guidance
        if (isMac) {
          console.error(
            "macOS: If hotkeys don't work, ensure Scribe has accessibility permission.\n" +
            "Go to System Settings > Privacy & Security > Accessibility"
          );
        }
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
  }, [hotkey, enabled, checkPermissions, isMac]);

  return { isRegistered: isRegistered.current, permissionGranted };
}

/**
 * Common hotkey combinations
 */
export const HOTKEYS = {
  PUSH_TO_TALK: "CommandOrControl+Shift+S",
  TOGGLE_RECORDING: "CommandOrControl+Shift+R",
  CANCEL_RECORDING: "Escape",
} as const;
