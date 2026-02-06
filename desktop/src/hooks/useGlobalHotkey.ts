import { useEffect, useRef, useCallback } from "react";

// Dynamic import for Tauri - will be undefined in browser
let tauriGlobalShortcut: typeof import("@tauri-apps/plugin-global-shortcut") | null = null;

// Window globals to persist state across HMR reloads
declare global {
  interface Window {
    __HOTKEY_REGISTERED__?: string | null;
    __HOTKEY_CALLBACKS__?: {
      onKeyDown?: () => void;
      onKeyUp?: () => void;
    };
  }
}

if (typeof window !== "undefined") {
  if (!window.__HOTKEY_CALLBACKS__) {
    window.__HOTKEY_CALLBACKS__ = {};
  }
}

async function loadTauriPlugin(retries = 3): Promise<boolean> {
  for (let attempt = 1; attempt <= retries; attempt++) {
    try {
      if (typeof window !== "undefined" && "__TAURI__" in window) {
        console.log(`[Hotkey] Loading Tauri plugin (attempt ${attempt})...`);
        tauriGlobalShortcut = await import("@tauri-apps/plugin-global-shortcut");
        console.log("[Hotkey] Plugin loaded");
        return true;
      }
      if (attempt < retries) await new Promise(r => setTimeout(r, 500));
    } catch (err) {
      console.error(`[Hotkey] Plugin load failed (attempt ${attempt}):`, err);
      if (attempt < retries) await new Promise(r => setTimeout(r, 500));
    }
  }
  return false;
}

export interface UseGlobalHotkeyOptions {
  hotkey: string;
  onKeyDown?: () => void;
  onKeyUp?: () => void;
  enabled?: boolean;
}

/**
 * Hook for registering global hotkeys via the Tauri global shortcut plugin.
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

  // Update window callbacks
  useEffect(() => {
    if (window.__HOTKEY_CALLBACKS__) {
      window.__HOTKEY_CALLBACKS__.onKeyDown = onKeyDown;
      window.__HOTKEY_CALLBACKS__.onKeyUp = onKeyUp;
    }
  }, [onKeyDown, onKeyUp]);

  // Tauri global shortcut registration
  const registerHotkey = useCallback(async (forceReregister = false) => {
    if (!enabled || isRegistering.current) return;

    if (window.__HOTKEY_REGISTERED__ === hotkey && tauriGlobalShortcut && !forceReregister) {
      try {
        const isRegistered = await tauriGlobalShortcut.isRegistered(hotkey);
        if (isRegistered) return;
        window.__HOTKEY_REGISTERED__ = null;
      } catch {}
    }

    isRegistering.current = true;
    try {
      if (!isTauriLoaded.current && !tauriGlobalShortcut) {
        isTauriLoaded.current = await loadTauriPlugin();
      }
      if (!mountedRef.current || !tauriGlobalShortcut) {
        isRegistering.current = false;
        return;
      }

      if (window.__HOTKEY_REGISTERED__ && window.__HOTKEY_REGISTERED__ !== hotkey) {
        try { await tauriGlobalShortcut.unregister(window.__HOTKEY_REGISTERED__); } catch {}
        window.__HOTKEY_REGISTERED__ = null;
      }
      if (forceReregister && window.__HOTKEY_REGISTERED__ === hotkey) {
        try { await tauriGlobalShortcut.unregister(hotkey); } catch {}
        window.__HOTKEY_REGISTERED__ = null;
      }

      console.log(`[Hotkey] Registering Tauri shortcut: ${hotkey}`);
      await tauriGlobalShortcut.register(hotkey, (event) => {
        const callbacks = window.__HOTKEY_CALLBACKS__;
        if (!callbacks) return;
        if (event.state === "Pressed") {
          console.log("[Hotkey] Tauri plugin: PRESSED");
          callbacks.onKeyDown?.();
        } else if (event.state === "Released") {
          console.log("[Hotkey] Tauri plugin: RELEASED");
          callbacks.onKeyUp?.();
        }
      });

      if (mountedRef.current) {
        window.__HOTKEY_REGISTERED__ = hotkey;
        console.log(`[Hotkey] Tauri shortcut registered: ${hotkey}`);
      } else {
        try { await tauriGlobalShortcut.unregister(hotkey); } catch {}
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
    return () => { mountedRef.current = false; };
  }, [registerHotkey]);

  // Re-verify on focus
  useEffect(() => {
    if (!enabled) return;

    const handleFocus = () => {
      setTimeout(() => registerHotkey(false), 100);
    };

    window.addEventListener("focus", handleFocus);
    return () => window.removeEventListener("focus", handleFocus);
  }, [hotkey, enabled, registerHotkey]);

  return {
    isRegistered: window.__HOTKEY_REGISTERED__ === hotkey,
    reregister: () => registerHotkey(true),
  };
}

export const HOTKEYS = {
  PUSH_TO_TALK: "CommandOrControl+Shift+S",
  TOGGLE_RECORDING: "CommandOrControl+Shift+R",
  CANCEL_RECORDING: "Escape",
} as const;
