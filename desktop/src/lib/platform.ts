/**
 * Centralized platform detection utilities
 *
 * Use these functions instead of directly checking navigator.platform
 * to ensure consistent behavior across the codebase.
 */

export type Platform = "mac" | "windows" | "linux" | "unknown";

/**
 * Get the current platform
 */
export function getPlatform(): Platform {
  if (typeof navigator === "undefined") return "unknown";
  const platform = navigator.platform.toUpperCase();
  if (platform.includes("MAC")) return "mac";
  if (platform.includes("WIN")) return "windows";
  if (platform.includes("LINUX")) return "linux";
  return "unknown";
}

/**
 * Check if running on macOS
 */
export function isMacOS(): boolean {
  return getPlatform() === "mac";
}

/**
 * Check if running on Windows
 */
export function isWindows(): boolean {
  return getPlatform() === "windows";
}

/**
 * Check if running on Linux
 */
export function isLinux(): boolean {
  return getPlatform() === "linux";
}

/**
 * Get the appropriate modifier key name for display
 * Mac: "Cmd", Windows/Linux: "Ctrl"
 */
export function getModifierKeyName(): string {
  return isMacOS() ? "Cmd" : "Ctrl";
}

/**
 * Get the appropriate alt key name for display
 * Mac: "Option", Windows/Linux: "Alt"
 */
export function getAltKeyName(): string {
  return isMacOS() ? "Option" : "Alt";
}

/**
 * Format a hotkey string for display on the current platform
 * Converts "CommandOrControl" to "Cmd" or "Ctrl" and "Alt" to "Option" on Mac
 */
export function formatHotkeyForDisplay(hotkey: string): string {
  return hotkey
    .replace("CommandOrControl", getModifierKeyName())
    .replace("Alt", getAltKeyName());
}

/**
 * Get the paste keyboard shortcut for the current platform
 */
export function getPasteShortcut(): string {
  return isMacOS() ? "Cmd+V" : "Ctrl+V";
}
