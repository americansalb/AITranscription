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
 * Minimize the Scribe window to return focus to the target app
 *
 * We use minimize instead of hide because:
 * - On Windows, minimize properly returns focus to the previous app
 * - The window is still accessible via taskbar
 * - show() after hide() steals focus on Windows
 */
async function minimizeWindow(): Promise<void> {
  if (!tauriWindow) {
    await loadTauriWindow();
  }
  if (tauriWindow) {
    try {
      const win = tauriWindow.getCurrentWindow();
      await win.minimize();
    } catch (error) {
      console.error("Failed to minimize window:", error);
    }
  }
}

/**
 * Inject text into the active application using clipboard + paste simulation
 *
 * IMPORTANT: This function minimizes the Scribe window and does NOT bring it
 * back automatically. This prevents focus stealing which was causing text to
 * end up in Scribe instead of the target app.
 *
 * The user can click the tray icon to bring the window back.
 */
export async function injectText(text: string): Promise<boolean> {
  const timestamp = Date.now();
  console.log(`[injectText:${timestamp}] Called with text length:`, text.length);

  // First, copy to clipboard and verify
  const copied = await copyToClipboard(text);
  console.log(`[injectText:${timestamp}] Clipboard copy result:`, copied);
  if (!copied) return false;

  // Delay to ensure clipboard is fully written (antivirus scanning, etc.)
  console.log(`[injectText:${timestamp}] Waiting for clipboard to settle...`);
  await new Promise((resolve) => setTimeout(resolve, 100));

  // Try to simulate paste via Tauri command
  if (!tauriCore) {
    await loadTauriCore();
  }

  if (tauriCore) {
    try {
      // Minimize window - this works better than hide() on Windows
      // because it properly returns focus to the previous app
      console.log(`[injectText:${timestamp}] Minimizing window...`);
      await minimizeWindow();

      // Give the OS time to switch focus back to the target app
      // Increased from 150ms to 250ms - Windows needs more time for reliable focus switch
      console.log(`[injectText:${timestamp}] Waiting for focus switch...`);
      await new Promise((resolve) => setTimeout(resolve, 250));

      console.log(`[injectText:${timestamp}] Invoking simulate_paste...`);
      await tauriCore.invoke("simulate_paste");
      console.log(`[injectText:${timestamp}] Paste command sent`);

      // Wait a bit to let the paste complete before returning
      await new Promise((resolve) => setTimeout(resolve, 50));
      console.log(`[injectText:${timestamp}] Done`);

      // DO NOT show/restore the window here!
      // On Windows, win.show() ALWAYS steals focus, which is the main regression.
      // The window stays minimized. User can click tray icon if needed.

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
      // Minimize window to return focus to the target app
      await minimizeWindow();

      // Give the OS time to switch focus
      await new Promise((resolve) => setTimeout(resolve, 150));

      // Now paste into the target app
      await tauriCore.invoke("simulate_paste");

      // DO NOT show/restore the window - it steals focus on Windows
      // Window stays minimized, user can click tray to restore

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
