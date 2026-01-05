/**
 * Clipboard and text injection utilities
 *
 * For text injection into active applications, we use the clipboard + paste approach:
 * 1. Copy our text to clipboard
 * 2. Hide Scribe window to return focus to target app
 * 3. Simulate Ctrl+V / Cmd+V to paste
 * 4. Keep window hidden - user can click tray to reopen
 *
 * This is the most reliable cross-platform approach for universal dictation.
 */

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
      // Verify the write by reading back (helps catch async issues)
      const readBack = await tauriClipboard.readText();
      if (readBack !== text) {
        console.warn("[clipboard] Write verification failed, retrying...");
        await tauriClipboard.writeText(text);
      }
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

/**
 * Inject text into the active application using clipboard + paste simulation
 *
 * Simply copies text to clipboard and simulates Ctrl+V / Cmd+V.
 * The text goes wherever the cursor/focus already is - no window manipulation.
 */
export async function injectText(text: string): Promise<boolean> {
  const timestamp = Date.now();
  console.log(`[injectText:${timestamp}] Called with text length:`, text.length);

  // First, copy to clipboard and verify
  const copied = await copyToClipboard(text);
  console.log(`[injectText:${timestamp}] Clipboard copy result:`, copied);
  if (!copied) return false;

  // Short delay to ensure clipboard is fully written
  await new Promise((resolve) => setTimeout(resolve, 50));

  // Simulate paste via Tauri command
  if (!tauriCore) {
    await loadTauriCore();
  }

  if (tauriCore) {
    try {
      console.log(`[injectText:${timestamp}] Invoking simulate_paste...`);
      await tauriCore.invoke("simulate_paste");
      console.log(`[injectText:${timestamp}] Paste complete`);
      return true;
    } catch (error) {
      console.error(`[injectText:${timestamp}] Auto-paste failed:`, error);
      // Clipboard still has the text, user can paste manually
      return true;
    }
  } else {
    console.log(`[injectText:${timestamp}] tauriCore not available`);
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

  // Small delay to ensure clipboard is fully written
  await new Promise((resolve) => setTimeout(resolve, 50));

  // Try auto-paste via Tauri
  if (!tauriCore) {
    await loadTauriCore();
  }

  if (tauriCore) {
    try {
      // Paste directly - cursor/focus is already where user wants it
      await tauriCore.invoke("simulate_paste");

      return {
        success: true,
        method: "auto-paste",
        message: "Text pasted into active application",
      };
    } catch (error) {
      console.error("Auto-paste failed:", error);
      const pasteKey = navigator.platform.includes("Mac") ? "Cmd+V" : "Ctrl+V";
      return {
        success: true,
        method: "clipboard",
        message: `Copied to clipboard. Press ${pasteKey} to paste.`,
      };
    }
  }

  const pasteKey = navigator.platform.includes("Mac") ? "Cmd+V" : "Ctrl+V";
  return {
    success: true,
    method: "clipboard",
    message: `Copied to clipboard. Press ${pasteKey} to paste.`,
  };
}

/**
 * Update tray icon recording state
 *
 * This ONLY updates the tray icon - doesn't touch the window.
 * Window stays wherever it is. User can access via tray when they want.
 */
export async function setTrayRecordingState(recording: boolean): Promise<void> {
  if (!tauriCore) {
    await loadTauriCore();
  }

  if (tauriCore) {
    try {
      await tauriCore.invoke("set_recording_state", { recording });
    } catch (error) {
      console.error("Failed to update tray state:", error);
    }
  }
}

/**
 * Show the floating recording indicator overlay window
 */
export async function showRecordingOverlay(): Promise<void> {
  if (!tauriCore) {
    await loadTauriCore();
  }

  if (tauriCore) {
    try {
      await tauriCore.invoke("show_recording_overlay");
    } catch (error) {
      console.error("Failed to show recording overlay:", error);
    }
  }
}

/**
 * Hide the floating recording indicator overlay window
 */
export async function hideRecordingOverlay(): Promise<void> {
  if (!tauriCore) {
    await loadTauriCore();
  }

  if (tauriCore) {
    try {
      await tauriCore.invoke("hide_recording_overlay");
    } catch (error) {
      console.error("Failed to hide recording overlay:", error);
    }
  }
}

/**
 * Send recording state to the overlay window via Tauri events
 */
export async function updateOverlayState(state: {
  isRecording: boolean;
  isProcessing: boolean;
  duration: number;
  audioLevel: number;
}): Promise<void> {
  if (typeof window !== "undefined" && "__TAURI__" in window) {
    try {
      const { emit } = await import("@tauri-apps/api/event");
      await emit("recording-state", state);
    } catch (error) {
      console.error("Failed to update overlay state:", error);
    }
  }
}
