import { useEffect, useRef, useCallback } from "react";

// Dynamic import for Tauri - will be undefined in browser
let tauriGlobalShortcut: typeof import("@tauri-apps/plugin-global-shortcut") | null = null;

// Window globals to persist state across HMR reloads
declare global {
  interface Window {
    __HOTKEY_REGISTERED__?: string | null; // Currently registered hotkey
    __HOTKEY_CALLBACKS__?: {
      onKeyDown?: () => void;
      onKeyUp?: () => void;
    };
  }
}

// Initialize window globals
if (typeof window !== "undefined") {
  if (!window.__HOTKEY_CALLBACKS__) {
    window.__HOTKEY_CALLBACKS__ = {};
  }
}

// Try to load Tauri global shortcut plugin with retry
async function loadTauriPlugin(retries = 3): Promise<boolean> {
  for (let attempt = 1; attempt <= retries; attempt++) {
    try {
      if (typeof window !== "undefined" && "__TAURI__" in window) {
        console.log(`[Hotkey] Tauri detected (attempt ${attempt}), loading global shortcut plugin...`);
        tauriGlobalShortcut = await import("@tauri-apps/plugin-global-shortcut");
        console.log("[Hotkey] Global shortcut plugin loaded successfully");
        return true;
      } else {
        console.log(`[Hotkey] Tauri not detected in window (attempt ${attempt}/${retries})`);
        if (attempt < retries) {
          // Wait before retrying - Tauri might not be ready yet
          await new Promise(resolve => setTimeout(resolve, 500));
        }
      }
    } catch (err) {
      console.error(`[Hotkey] Failed to load Tauri global shortcut plugin (attempt ${attempt}):`, err);
      if (attempt < retries) {
        await new Promise(resolve => setTimeout(resolve, 500));
      }
    }
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
  const isTauriLoaded = useRef(false);
  const isRegistering = useRef(false);
  const mountedRef = useRef(true);

  // Update window callbacks so HMR gets the latest functions
  useEffect(() => {
    if (window.__HOTKEY_CALLBACKS__) {
      window.__HOTKEY_CALLBACKS__.onKeyDown = onKeyDown;
      window.__HOTKEY_CALLBACKS__.onKeyUp = onKeyUp;
    }
  }, [onKeyDown, onKeyUp]);

  // Core registration function - extracted so it can be called on visibility change
  const registerHotkey = useCallback(async (forceReregister = false) => {
    if (!enabled || isRegistering.current) return;

    // Check if already registered with this hotkey (survives HMR)
    // But verify it's actually registered with Tauri
    if (window.__HOTKEY_REGISTERED__ === hotkey && tauriGlobalShortcut && !forceReregister) {
      try {
        const isRegistered = await tauriGlobalShortcut.isRegistered(hotkey);
        if (isRegistered) {
          console.log(`[Hotkey] Already registered (verified): ${hotkey}`);
          return;
        }
        console.log(`[Hotkey] Window says registered but Tauri says no - re-registering: ${hotkey}`);
        window.__HOTKEY_REGISTERED__ = null;
      } catch {
        // isRegistered might not be available, continue with registration
      }
    }

    isRegistering.current = true;

    try {
      // Load Tauri plugin if not already loaded
      if (!isTauriLoaded.current && !tauriGlobalShortcut) {
        isTauriLoaded.current = await loadTauriPlugin();
      }

      if (!mountedRef.current) {
        isRegistering.current = false;
        return;
      }

      if (!tauriGlobalShortcut) {
        console.log("[Hotkey] Global hotkeys not available (not running in Tauri)");
        isRegistering.current = false;
        return;
      }

      // If a different hotkey was registered, unregister it first
      if (window.__HOTKEY_REGISTERED__ && window.__HOTKEY_REGISTERED__ !== hotkey) {
        console.log(`[Hotkey] Unregistering old hotkey: ${window.__HOTKEY_REGISTERED__}`);
        try {
          await tauriGlobalShortcut.unregister(window.__HOTKEY_REGISTERED__);
        } catch (e) {
          // Ignore errors when unregistering
        }
        window.__HOTKEY_REGISTERED__ = null;
      }

      // For force re-register, unregister current first
      if (forceReregister && window.__HOTKEY_REGISTERED__ === hotkey) {
        console.log(`[Hotkey] Force re-registering: ${hotkey}`);
        try {
          await tauriGlobalShortcut.unregister(hotkey);
        } catch (e) {
          // Ignore errors when unregistering
        }
        window.__HOTKEY_REGISTERED__ = null;
      }

      console.log(`[Hotkey] Attempting to register: ${hotkey}`);
      await tauriGlobalShortcut.register(hotkey, (event) => {
        // Use window callbacks to get latest functions (survives HMR)
        const callbacks = window.__HOTKEY_CALLBACKS__;
        if (!callbacks) return;

        console.log(`[Hotkey] Event received:`, event.state);
        if (event.state === "Pressed") {
          console.log("[Hotkey] Key PRESSED - starting recording");
          callbacks.onKeyDown?.();
        } else if (event.state === "Released") {
          console.log("[Hotkey] Key RELEASED - stopping recording");
          callbacks.onKeyUp?.();
        }
      });

      if (mountedRef.current) {
        window.__HOTKEY_REGISTERED__ = hotkey;
        console.log(`[Hotkey] Registered successfully: ${hotkey}`);
      } else {
        try {
          await tauriGlobalShortcut.unregister(hotkey);
        } catch (e) {
          // Ignore cleanup errors
        }
      }
    } catch (error) {
      console.error(`[Hotkey] Failed to register: ${hotkey}`, error);
    } finally {
      isRegistering.current = false;
    }
  }, [hotkey, enabled]);

  useEffect(() => {
    mountedRef.current = true;

    registerHotkey();

    return () => {
      mountedRef.current = false;
      // DON'T unregister on cleanup - let it survive HMR
      // The registration persists via window.__HOTKEY_REGISTERED__
      // Only unregister when hotkey changes (handled above)
    };
  }, [registerHotkey]);

  // Re-verify and re-register hotkey when window gains focus
  // This fixes issues on Windows multi-monitor setups where hotkeys
  // can stop working after clicking to another monitor
  useEffect(() => {
    if (!enabled) return;

    let unlistenTauriFocus: (() => void) | undefined;

    const handleVisibilityChange = () => {
      if (document.visibilityState === "visible") {
        console.log("[Hotkey] Window became visible, verifying registration...");
        // Small delay to let the window fully activate
        setTimeout(() => {
          registerHotkey(false);
        }, 100);
      }
    };

    const handleFocus = () => {
      console.log("[Hotkey] Window gained focus, verifying registration...");
      // Verify registration when window gains focus
      setTimeout(() => {
        registerHotkey(false);
      }, 100);
    };

    // Also listen for window focus events (more reliable on Windows)
    const handleWindowFocus = async () => {
      if (!tauriGlobalShortcut || !hotkey) return;

      try {
        const isRegistered = await tauriGlobalShortcut.isRegistered(hotkey);
        if (!isRegistered) {
          console.log("[Hotkey] Hotkey not registered after focus change, re-registering...");
          registerHotkey(true);
        }
      } catch {
        // If check fails, try to re-register anyway
        registerHotkey(true);
      }
    };

    document.addEventListener("visibilitychange", handleVisibilityChange);
    window.addEventListener("focus", handleFocus);

    // Additional: Check hotkey periodically when app has focus
    // This catches cases where the OS might have unregistered our hotkey
    let focusCheckInterval: number | undefined;

    const startFocusCheck = () => {
      if (focusCheckInterval) return;
      focusCheckInterval = window.setInterval(() => {
        if (document.hasFocus()) {
          handleWindowFocus();
        }
      }, 5000); // Check every 5 seconds when focused
    };

    const stopFocusCheck = () => {
      if (focusCheckInterval) {
        clearInterval(focusCheckInterval);
        focusCheckInterval = undefined;
      }
    };

    // Start checking when window has focus
    if (document.hasFocus()) {
      startFocusCheck();
    }

    window.addEventListener("focus", startFocusCheck);
    window.addEventListener("blur", stopFocusCheck);

    // Listen for Tauri window focus events (more reliable on Windows)
    const setupTauriListeners = async () => {
      if (typeof window !== "undefined" && "__TAURI__" in window) {
        try {
          const { getCurrentWebviewWindow } = await import("@tauri-apps/api/webviewWindow");
          const currentWindow = getCurrentWebviewWindow();

          unlistenTauriFocus = await currentWindow.onFocusChanged(({ payload: focused }) => {
            if (focused) {
              console.log("[Hotkey] Tauri window focused, verifying registration...");
              setTimeout(() => {
                registerHotkey(false);
              }, 150);
            }
          });
          console.log("[Hotkey] Tauri focus listener registered");
        } catch (err) {
          console.log("[Hotkey] Could not setup Tauri focus listener:", err);
        }
      }
    };

    setupTauriListeners();

    return () => {
      document.removeEventListener("visibilitychange", handleVisibilityChange);
      window.removeEventListener("focus", handleFocus);
      window.removeEventListener("focus", startFocusCheck);
      window.removeEventListener("blur", stopFocusCheck);
      stopFocusCheck();
      if (unlistenTauriFocus) {
        unlistenTauriFocus();
      }
    };
  }, [hotkey, enabled, registerHotkey]);

  return {
    isRegistered: window.__HOTKEY_REGISTERED__ === hotkey,
    reregister: () => registerHotkey(true),
  };
}

/**
 * Common hotkey combinations
 */
export const HOTKEYS = {
  PUSH_TO_TALK: "CommandOrControl+Shift+S",
  TOGGLE_RECORDING: "CommandOrControl+Shift+R",
  CANCEL_RECORDING: "Escape",
} as const;
