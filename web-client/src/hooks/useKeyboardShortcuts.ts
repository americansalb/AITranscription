/**
 * Global keyboard shortcuts for the web client.
 * Matches common patterns: Cmd/Ctrl+K for search, Escape to close modals, etc.
 */

import { useEffect } from "react";

interface Shortcut {
  key: string;
  ctrl?: boolean;
  shift?: boolean;
  handler: () => void;
  /** Description for help overlay */
  description: string;
}

export function useKeyboardShortcuts(shortcuts: Shortcut[]) {
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      // Don't trigger shortcuts when typing in inputs
      const target = e.target as HTMLElement;
      if (target.tagName === "INPUT" || target.tagName === "TEXTAREA" || target.tagName === "SELECT") {
        // Only allow Escape in inputs
        if (e.key !== "Escape") return;
      }

      for (const shortcut of shortcuts) {
        const ctrlMatch = shortcut.ctrl ? (e.ctrlKey || e.metaKey) : true;
        const shiftMatch = shortcut.shift ? e.shiftKey : !e.shiftKey;

        if (e.key === shortcut.key && ctrlMatch && shiftMatch) {
          e.preventDefault();
          shortcut.handler();
          return;
        }
      }
    };

    document.addEventListener("keydown", handleKeyDown);
    return () => document.removeEventListener("keydown", handleKeyDown);
  }, [shortcuts]);
}

/** Predefined shortcut keys for documentation */
export const SHORTCUT_DESCRIPTIONS = [
  { keys: "Ctrl+/", description: "Toggle keyboard shortcuts help" },
  { keys: "Ctrl+N", description: "New message (focus compose)" },
  { keys: "Escape", description: "Close modal / cancel" },
  { keys: "Ctrl+Enter", description: "Send message" },
];
