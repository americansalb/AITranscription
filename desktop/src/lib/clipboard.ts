/**
 * Clipboard and text injection utilities
 *
 * For text injection into active applications, we use the clipboard + paste approach:
 * 1. Save current clipboard content
 * 2. Copy our text to clipboard
 * 3. Simulate Ctrl+V / Cmd+V to paste
 * 4. Restore original clipboard content
 *
 * This is the most reliable cross-platform approach for universal dictation.
 */

import { isMacOS, getPasteShortcut } from "./platform";

// For backward compatibility
const pasteShortcut = getPasteShortcut();

// Dynamic import for Tauri clipboard plugin
let tauriClipboard: typeof import("@tauri-apps/plugin-clipboard-manager") | null = null;

async function loadTauriClipboard() {
  try {
    if (typeof window !== "undefined" && "__TAURI__" in window) {
      tauriClipboard = await import("@tauri-apps/plugin-clipboard-manager");
      return true;
    }
  } catch {
    // Not running in Tauri
  }
  return false;
}

/**
 * Copy text to clipboard (works in both browser and Tauri)
 */
export async function copyToClipboard(text: string): Promise<boolean> {
  // Try Tauri first
  if (!tauriClipboard) {
    await loadTauriClipboard();
  }

  if (tauriClipboard) {
    try {
      await tauriClipboard.writeText(text);
      return true;
    } catch (error) {
      console.error("Tauri clipboard write failed:", error);
    }
  }

  // Fallback to browser API
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch (error) {
    console.error("Browser clipboard write failed:", error);
    return false;
  }
}

/**
 * Read text from clipboard
 */
export async function readFromClipboard(): Promise<string | null> {
  // Try Tauri first
  if (!tauriClipboard) {
    await loadTauriClipboard();
  }

  if (tauriClipboard) {
    try {
      return await tauriClipboard.readText();
    } catch (error) {
      console.error("Tauri clipboard read failed:", error);
    }
  }

  // Fallback to browser API
  try {
    return await navigator.clipboard.readText();
  } catch (error) {
    console.error("Browser clipboard read failed:", error);
    return null;
  }
}

// Dynamic import for Tauri core API
let tauriCore: typeof import("@tauri-apps/api/core") | null = null;
let tauriWindow: typeof import("@tauri-apps/api/window") | null = null;

async function loadTauriCore() {
  try {
    if (typeof window !== "undefined" && "__TAURI__" in window) {
      tauriCore = await import("@tauri-apps/api/core");
      return true;
    }
  } catch {
    // Not running in Tauri
  }
  return false;
}

async function loadTauriWindow() {
  try {
    if (typeof window !== "undefined" && "__TAURI__" in window) {
      tauriWindow = await import("@tauri-apps/api/window");
      return true;
    }
  } catch {
    // Not running in Tauri
  }
  return false;
}

/**
 * Hide the Scribe window to return focus to the previous app
 */
async function hideWindow(): Promise<boolean> {
  if (!tauriWindow) {
    await loadTauriWindow();
  }
  if (tauriWindow) {
    try {
      const win = tauriWindow.getCurrentWindow();
      await win.hide();
      // Small delay to let OS switch focus
      await new Promise((resolve) => setTimeout(resolve, 150));
      return true;
    } catch (error) {
      console.error("Failed to hide window:", error);
    }
  }
  return false;
}

/**
 * Show the Scribe window again
 */
async function showWindow(): Promise<void> {
  if (!tauriWindow) {
    await loadTauriWindow();
  }
  if (tauriWindow) {
    try {
      const win = tauriWindow.getCurrentWindow();
      await win.show();
    } catch (error) {
      console.error("Failed to show window:", error);
    }
  }
}

/**
 * Inject text into the active application using clipboard + paste simulation.
 * Just copies to clipboard and simulates paste - no window manipulation.
 */
export async function injectText(text: string): Promise<boolean> {
  // First, copy to clipboard
  const copied = await copyToClipboard(text);
  if (!copied) return false;

  // Try to simulate paste via Tauri command
  if (!tauriCore) {
    await loadTauriCore();
  }

  if (tauriCore) {
    try {
      // Simulate Cmd+V / Ctrl+V - pastes into whatever app has focus
      await tauriCore.invoke("simulate_paste");
      return true;
    } catch (error) {
      console.error("Auto-paste failed:", error);
      // Clipboard still has the text, user can paste manually
      return true;
    }
  }

  // In browser mode, just copy to clipboard
  return true;
}

/**
 * Type text directly using keyboard simulation (alternative to paste)
 */
export async function typeText(text: string): Promise<boolean> {
  if (!tauriCore) {
    await loadTauriCore();
  }

  if (tauriCore) {
    try {
      await tauriCore.invoke("type_text", { text });
      return true;
    } catch (error) {
      console.error("Type text failed:", error);
      return false;
    }
  }

  return false;
}

/**
 * Auto-paste mode: copies text and automatically pastes if possible
 */
export interface InjectionResult {
  success: boolean;
  method: "clipboard" | "auto-paste" | "type";
  message: string;
}

export async function injectTextWithFeedback(text: string): Promise<InjectionResult> {
  // First copy to clipboard
  const copied = await copyToClipboard(text);
  if (!copied) {
    return {
      success: false,
      method: "clipboard",
      message: "Failed to copy to clipboard",
    };
  }

  // Try auto-paste via Tauri
  if (!tauriCore) {
    await loadTauriCore();
  }

  if (tauriCore) {
    try {
      // Hide window to return focus to previous app
      await hideWindow();

      await tauriCore.invoke("simulate_paste");

      // Show window again after a short delay
      setTimeout(() => showWindow(), 500);
      return {
        success: true,
        method: "auto-paste",
        message: "Text pasted into active application",
      };
    } catch (error) {
      console.error("Auto-paste failed:", error);
      // Show window again if paste failed
      showWindow();
      return {
        success: true,
        method: "clipboard",
        message: `Copied to clipboard. Press ${pasteShortcut} to paste.`,
      };
    }
  }

  return {
    success: true,
    method: "clipboard",
    message: `Copied to clipboard. Press ${pasteShortcut} to paste.`,
  };
}

/**
 * Update tray icon to show recording state
 */
export async function setTrayRecordingState(recording: boolean): Promise<void> {
  if (!tauriCore) {
    await loadTauriCore();
  }

  if (tauriCore) {
    try {
      await tauriCore.invoke("set_recording_state", { recording });
    } catch (error) {
      console.error("Failed to set tray recording state:", error);
    }
  }
}

/**
 * Update the floating overlay window state
 */
export interface OverlayState {
  isRecording: boolean;
  isProcessing: boolean;
  duration: number;
  audioLevel: number;
}

let tauriEvent: typeof import("@tauri-apps/api/event") | null = null;

async function loadTauriEvent() {
  try {
    if (typeof window !== "undefined" && "__TAURI__" in window) {
      tauriEvent = await import("@tauri-apps/api/event");
      return true;
    }
  } catch {
    // Not running in Tauri
  }
  return false;
}

export async function updateOverlayState(state: OverlayState): Promise<void> {
  if (!tauriEvent) {
    await loadTauriEvent();
  }

  if (tauriEvent) {
    try {
      await tauriEvent.emit("overlay-update", state);
    } catch (error) {
      console.error("Failed to update overlay state:", error);
    }
  }
}
