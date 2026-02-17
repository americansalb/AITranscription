/**
 * Tests for platform detection utilities (T2-3 fix).
 *
 * Covers:
 *   - Platform detection: mac, windows, linux, unknown
 *   - isMacOS / isWindows / isLinux helpers
 *   - Modifier key names: Cmd vs Ctrl
 *   - Alt key names: Option vs Alt
 *   - Hotkey formatting for display
 *   - Paste shortcut per platform
 *   - Edge case: missing navigator
 */
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

// We need to re-import after mocking navigator, so use dynamic imports
// and reset modules between tests.

describe("platform detection", () => {
  const originalNavigator = globalThis.navigator;

  afterEach(() => {
    Object.defineProperty(globalThis, "navigator", {
      value: originalNavigator,
      configurable: true,
    });
    vi.resetModules();
  });

  function mockPlatform(platform: string) {
    Object.defineProperty(globalThis, "navigator", {
      value: { platform },
      configurable: true,
    });
  }

  it("detects macOS from navigator.platform", async () => {
    mockPlatform("MacIntel");
    const { getPlatform, isMacOS } = await import("../lib/platform");
    expect(getPlatform()).toBe("mac");
    expect(isMacOS()).toBe(true);
  });

  it("detects Windows from navigator.platform", async () => {
    mockPlatform("Win32");
    const { getPlatform, isWindows } = await import("../lib/platform");
    expect(getPlatform()).toBe("windows");
    expect(isWindows()).toBe(true);
  });

  it("detects Linux from navigator.platform", async () => {
    mockPlatform("Linux x86_64");
    const { getPlatform, isLinux } = await import("../lib/platform");
    expect(getPlatform()).toBe("linux");
    expect(isLinux()).toBe(true);
  });

  it("returns unknown for unrecognized platform", async () => {
    mockPlatform("FreeBSD");
    const { getPlatform } = await import("../lib/platform");
    expect(getPlatform()).toBe("unknown");
  });

  it("returns unknown when navigator is undefined", async () => {
    Object.defineProperty(globalThis, "navigator", {
      value: undefined,
      configurable: true,
    });
    const { getPlatform } = await import("../lib/platform");
    expect(getPlatform()).toBe("unknown");
  });
});


describe("modifier key names", () => {
  const originalNavigator = globalThis.navigator;

  afterEach(() => {
    Object.defineProperty(globalThis, "navigator", {
      value: originalNavigator,
      configurable: true,
    });
    vi.resetModules();
  });

  it("returns Cmd on macOS", async () => {
    Object.defineProperty(globalThis, "navigator", {
      value: { platform: "MacIntel" },
      configurable: true,
    });
    const { getModifierKeyName } = await import("../lib/platform");
    expect(getModifierKeyName()).toBe("Cmd");
  });

  it("returns Ctrl on Windows", async () => {
    Object.defineProperty(globalThis, "navigator", {
      value: { platform: "Win32" },
      configurable: true,
    });
    const { getModifierKeyName } = await import("../lib/platform");
    expect(getModifierKeyName()).toBe("Ctrl");
  });

  it("returns Ctrl on Linux", async () => {
    Object.defineProperty(globalThis, "navigator", {
      value: { platform: "Linux x86_64" },
      configurable: true,
    });
    const { getModifierKeyName } = await import("../lib/platform");
    expect(getModifierKeyName()).toBe("Ctrl");
  });
});


describe("alt key names (T2-3)", () => {
  const originalNavigator = globalThis.navigator;

  afterEach(() => {
    Object.defineProperty(globalThis, "navigator", {
      value: originalNavigator,
      configurable: true,
    });
    vi.resetModules();
  });

  it("returns Option on macOS", async () => {
    Object.defineProperty(globalThis, "navigator", {
      value: { platform: "MacIntel" },
      configurable: true,
    });
    const { getAltKeyName } = await import("../lib/platform");
    expect(getAltKeyName()).toBe("Option");
  });

  it("returns Alt on Windows", async () => {
    Object.defineProperty(globalThis, "navigator", {
      value: { platform: "Win32" },
      configurable: true,
    });
    const { getAltKeyName } = await import("../lib/platform");
    expect(getAltKeyName()).toBe("Alt");
  });

  it("returns Alt on Linux", async () => {
    Object.defineProperty(globalThis, "navigator", {
      value: { platform: "Linux x86_64" },
      configurable: true,
    });
    const { getAltKeyName } = await import("../lib/platform");
    expect(getAltKeyName()).toBe("Alt");
  });
});


describe("formatHotkeyForDisplay", () => {
  const originalNavigator = globalThis.navigator;

  afterEach(() => {
    Object.defineProperty(globalThis, "navigator", {
      value: originalNavigator,
      configurable: true,
    });
    vi.resetModules();
  });

  it("converts CommandOrControl+Shift+D to Cmd+Shift+D on Mac", async () => {
    Object.defineProperty(globalThis, "navigator", {
      value: { platform: "MacIntel" },
      configurable: true,
    });
    const { formatHotkeyForDisplay } = await import("../lib/platform");
    expect(formatHotkeyForDisplay("CommandOrControl+Shift+D")).toBe("Cmd+Shift+D");
  });

  it("converts CommandOrControl+Shift+D to Ctrl+Shift+D on Windows", async () => {
    Object.defineProperty(globalThis, "navigator", {
      value: { platform: "Win32" },
      configurable: true,
    });
    const { formatHotkeyForDisplay } = await import("../lib/platform");
    expect(formatHotkeyForDisplay("CommandOrControl+Shift+D")).toBe("Ctrl+Shift+D");
  });

  it("converts Alt+R to Option+R on Mac", async () => {
    Object.defineProperty(globalThis, "navigator", {
      value: { platform: "MacIntel" },
      configurable: true,
    });
    const { formatHotkeyForDisplay } = await import("../lib/platform");
    expect(formatHotkeyForDisplay("Alt+R")).toBe("Option+R");
  });

  it("keeps Alt+R as Alt+R on Windows", async () => {
    Object.defineProperty(globalThis, "navigator", {
      value: { platform: "Win32" },
      configurable: true,
    });
    const { formatHotkeyForDisplay } = await import("../lib/platform");
    expect(formatHotkeyForDisplay("Alt+R")).toBe("Alt+R");
  });
});


describe("getPasteShortcut", () => {
  const originalNavigator = globalThis.navigator;

  afterEach(() => {
    Object.defineProperty(globalThis, "navigator", {
      value: originalNavigator,
      configurable: true,
    });
    vi.resetModules();
  });

  it("returns Cmd+V on Mac", async () => {
    Object.defineProperty(globalThis, "navigator", {
      value: { platform: "MacIntel" },
      configurable: true,
    });
    const { getPasteShortcut } = await import("../lib/platform");
    expect(getPasteShortcut()).toBe("Cmd+V");
  });

  it("returns Ctrl+V on Windows", async () => {
    Object.defineProperty(globalThis, "navigator", {
      value: { platform: "Win32" },
      configurable: true,
    });
    const { getPasteShortcut } = await import("../lib/platform");
    expect(getPasteShortcut()).toBe("Ctrl+V");
  });
});
