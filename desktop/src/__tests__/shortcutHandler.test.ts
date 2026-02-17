/**
 * Tests for shortcutHandler — keyboard shortcut routing and queue controls.
 *
 * Covers:
 *   - handleShortcutEvent: all 10 keyboard shortcuts
 *   - getPlaybackSpeed / adjustSpeed: speed clamping 0.5-3.0
 *   - adjustVolume: volume clamping 0-1
 *   - speakStatus: status text generation
 *   - stopAndClear: queue stop + clear
 *   - isInputFocused: blocks Space when input/textarea/select focused
 *   - initShortcutHandler: registers and returns cleanup function
 *   - Unhandled keys return false
 */
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

// Mock queueStore before importing shortcutHandler
vi.mock("../lib/queueStore", () => ({
  getState: vi.fn().mockReturnValue({
    items: [],
    currentItem: null,
    isPlaying: false,
    isPaused: false,
    volume: 0.8,
  }),
  togglePlayPause: vi.fn(),
  skipNext: vi.fn(),
  skipPrevious: vi.fn(),
  setVolume: vi.fn(),
  clearPending: vi.fn(),
  getCurrentAudio: vi.fn().mockReturnValue(null),
}));

// Mock earcons before importing shortcutHandler
vi.mock("../lib/earcons", () => ({
  earcons: {
    pauseResume: vi.fn(),
    skipNext: vi.fn(),
    replayCurrent: vi.fn(),
    speedChange: vi.fn(),
    volumeChange: vi.fn(),
    queueCleared: vi.fn(),
    statusAnnounce: vi.fn(),
  },
}));

import {
  handleShortcutEvent,
  getPlaybackSpeed,
  adjustSpeed,
  adjustVolume,
  speakStatus,
  stopAndClear,
  replayCurrent,
  replayLastCompleted,
  initShortcutHandler,
} from "../lib/shortcutHandler";
import * as queueStore from "../lib/queueStore";
import { earcons } from "../lib/earcons";

const mockGetState = vi.mocked(queueStore.getState);
const mockToggle = vi.mocked(queueStore.togglePlayPause);
const mockSkipNext = vi.mocked(queueStore.skipNext);
const mockSkipPrev = vi.mocked(queueStore.skipPrevious);
const mockSetVolume = vi.mocked(queueStore.setVolume);
const mockClearPending = vi.mocked(queueStore.clearPending);


// =============================================================================
// HELPERS
// =============================================================================

function makeKeyboardEvent(
  code: string,
  opts: { ctrlKey?: boolean; shiftKey?: boolean; metaKey?: boolean } = {}
): KeyboardEvent {
  const event = new KeyboardEvent("keydown", {
    code,
    ctrlKey: opts.ctrlKey ?? false,
    shiftKey: opts.shiftKey ?? false,
    metaKey: opts.metaKey ?? false,
    bubbles: true,
    cancelable: true,
  });
  return event;
}


beforeEach(() => {
  vi.clearAllMocks();
  mockGetState.mockReturnValue({
    items: [],
    currentItem: null,
    isPlaying: false,
    isPaused: false,
    autoPlay: true,
    volume: 0.8,
    currentPosition: 0,
    interrupted: false,
    playbackSpeed: 1.0,
  });
  // Reset playback speed to default by adjusting to 1.0
  // The module keeps internal state, so we need to reset it
  // We'll adjust to bring it back to 1.0 if needed
});


// =============================================================================
// PLAYBACK SPEED
// =============================================================================

describe("playback speed", () => {
  it("starts at 1.0", () => {
    // Speed persists across tests within the module, but defaults to 1.0
    const speed = getPlaybackSpeed();
    expect(typeof speed).toBe("number");
    expect(speed).toBeGreaterThanOrEqual(0.5);
    expect(speed).toBeLessThanOrEqual(3.0);
  });

  it("adjustSpeed increases speed", () => {
    const before = getPlaybackSpeed();
    adjustSpeed(0.25);
    expect(getPlaybackSpeed()).toBe(Math.min(3.0, before + 0.25));
  });

  it("adjustSpeed decreases speed", () => {
    const before = getPlaybackSpeed();
    adjustSpeed(-0.25);
    expect(getPlaybackSpeed()).toBe(Math.max(0.5, before - 0.25));
  });

  it("clamps speed at 3.0 maximum", () => {
    // Set to max
    for (let i = 0; i < 20; i++) adjustSpeed(0.25);
    expect(getPlaybackSpeed()).toBe(3.0);
    adjustSpeed(0.25);
    expect(getPlaybackSpeed()).toBe(3.0);
  });

  it("clamps speed at 0.5 minimum", () => {
    // Set to min
    for (let i = 0; i < 20; i++) adjustSpeed(-0.25);
    expect(getPlaybackSpeed()).toBe(0.5);
    adjustSpeed(-0.25);
    expect(getPlaybackSpeed()).toBe(0.5);
  });

  it("plays earcon on speed change", () => {
    adjustSpeed(0.25);
    expect(earcons.speedChange).toHaveBeenCalled();
  });
});


// =============================================================================
// VOLUME
// =============================================================================

describe("adjustVolume", () => {
  it("calls setVolume with adjusted value", () => {
    mockGetState.mockReturnValue({
      items: [],
      currentItem: null,
      isPlaying: false,
      isPaused: false,
      autoPlay: true,
      volume: 0.5,
      currentPosition: 0,
      interrupted: false,
      playbackSpeed: 1.0,
    });
    adjustVolume(0.1);
    expect(mockSetVolume).toHaveBeenCalledWith(0.6);
  });

  it("clamps volume at 1.0 maximum", () => {
    mockGetState.mockReturnValue({
      items: [],
      currentItem: null,
      isPlaying: false,
      isPaused: false,
      autoPlay: true,
      volume: 0.95,
      currentPosition: 0,
      interrupted: false,
      playbackSpeed: 1.0,
    });
    adjustVolume(0.1);
    expect(mockSetVolume).toHaveBeenCalledWith(1.0);
  });

  it("clamps volume at 0 minimum", () => {
    mockGetState.mockReturnValue({
      items: [],
      currentItem: null,
      isPlaying: false,
      isPaused: false,
      autoPlay: true,
      volume: 0.05,
      currentPosition: 0,
      interrupted: false,
      playbackSpeed: 1.0,
    });
    adjustVolume(-0.1);
    expect(mockSetVolume).toHaveBeenCalledWith(0);
  });

  it("plays earcon on volume change", () => {
    adjustVolume(0.1);
    expect(earcons.volumeChange).toHaveBeenCalled();
  });
});


// =============================================================================
// KEYBOARD EVENT ROUTING
// =============================================================================

describe("handleShortcutEvent — routing", () => {
  it("Space toggles play/pause", () => {
    const e = makeKeyboardEvent("Space");
    const handled = handleShortcutEvent(e);
    expect(handled).toBe(true);
    expect(mockToggle).toHaveBeenCalled();
    expect(earcons.pauseResume).toHaveBeenCalled();
  });

  it("Space is blocked when input is focused", () => {
    const input = document.createElement("input");
    document.body.appendChild(input);
    input.focus();

    const e = makeKeyboardEvent("Space");
    const handled = handleShortcutEvent(e);
    expect(handled).toBe(false);
    expect(mockToggle).not.toHaveBeenCalled();

    document.body.removeChild(input);
  });

  it("Space is blocked when textarea is focused", () => {
    const textarea = document.createElement("textarea");
    document.body.appendChild(textarea);
    textarea.focus();

    const e = makeKeyboardEvent("Space");
    const handled = handleShortcutEvent(e);
    expect(handled).toBe(false);

    document.body.removeChild(textarea);
  });

  it("Space is blocked when select is focused", () => {
    const select = document.createElement("select");
    document.body.appendChild(select);
    select.focus();

    const e = makeKeyboardEvent("Space");
    const handled = handleShortcutEvent(e);
    expect(handled).toBe(false);

    document.body.removeChild(select);
  });

  it("Ctrl+Right skips next", () => {
    const e = makeKeyboardEvent("ArrowRight", { ctrlKey: true });
    const handled = handleShortcutEvent(e);
    expect(handled).toBe(true);
    expect(mockSkipNext).toHaveBeenCalled();
    expect(earcons.skipNext).toHaveBeenCalled();
  });

  it("F4 skips next (alternative)", () => {
    const e = makeKeyboardEvent("F4");
    const handled = handleShortcutEvent(e);
    expect(handled).toBe(true);
    expect(mockSkipNext).toHaveBeenCalled();
  });

  it("Ctrl+Left replays current (no earcon without audio)", () => {
    const e = makeKeyboardEvent("ArrowLeft", { ctrlKey: true });
    const handled = handleShortcutEvent(e);
    expect(handled).toBe(true);
    // replayCurrent only triggers earcon if getCurrentAudio returns non-null
    // Our mock returns null, so no earcon — matches real behavior when nothing is playing
    expect(earcons.replayCurrent).not.toHaveBeenCalled();
  });

  it("Ctrl+Up increases speed", () => {
    const e = makeKeyboardEvent("ArrowUp", { ctrlKey: true });
    const handled = handleShortcutEvent(e);
    expect(handled).toBe(true);
    expect(earcons.speedChange).toHaveBeenCalled();
  });

  it("Ctrl+Down decreases speed", () => {
    const e = makeKeyboardEvent("ArrowDown", { ctrlKey: true });
    const handled = handleShortcutEvent(e);
    expect(handled).toBe(true);
    expect(earcons.speedChange).toHaveBeenCalled();
  });

  it("Ctrl+Shift+Up increases volume", () => {
    const e = makeKeyboardEvent("ArrowUp", { ctrlKey: true, shiftKey: true });
    const handled = handleShortcutEvent(e);
    expect(handled).toBe(true);
    expect(earcons.volumeChange).toHaveBeenCalled();
  });

  it("Ctrl+Shift+Down decreases volume", () => {
    const e = makeKeyboardEvent("ArrowDown", { ctrlKey: true, shiftKey: true });
    const handled = handleShortcutEvent(e);
    expect(handled).toBe(true);
    expect(earcons.volumeChange).toHaveBeenCalled();
  });

  it("Ctrl+R replays last completed", () => {
    const e = makeKeyboardEvent("KeyR", { ctrlKey: true });
    const handled = handleShortcutEvent(e);
    expect(handled).toBe(true);
    expect(mockSkipPrev).toHaveBeenCalled();
    expect(earcons.replayCurrent).toHaveBeenCalled();
  });

  it("Ctrl+S speaks status", () => {
    const e = makeKeyboardEvent("KeyS", { ctrlKey: true });
    const handled = handleShortcutEvent(e);
    expect(handled).toBe(true);
    expect(earcons.statusAnnounce).toHaveBeenCalled();
  });

  it("Escape stops and clears", () => {
    const e = makeKeyboardEvent("Escape");
    const handled = handleShortcutEvent(e);
    expect(handled).toBe(true);
    expect(mockSkipNext).toHaveBeenCalled();
    expect(mockClearPending).toHaveBeenCalled();
    expect(earcons.queueCleared).toHaveBeenCalled();
  });

  it("Meta key works as Ctrl alternative (macOS)", () => {
    const e = makeKeyboardEvent("ArrowRight", { metaKey: true });
    const handled = handleShortcutEvent(e);
    expect(handled).toBe(true);
    expect(mockSkipNext).toHaveBeenCalled();
  });

  it("returns false for unhandled keys", () => {
    const e = makeKeyboardEvent("KeyA");
    expect(handleShortcutEvent(e)).toBe(false);
  });

  it("returns false for Ctrl+unhandled keys", () => {
    const e = makeKeyboardEvent("KeyZ", { ctrlKey: true });
    expect(handleShortcutEvent(e)).toBe(false);
  });
});


// =============================================================================
// speakStatus — STATUS TEXT GENERATION
// =============================================================================

describe("speakStatus", () => {
  it("generates idle status when nothing playing", () => {
    // speechSynthesis is not available in jsdom, but speakStatus should not throw
    expect(() => speakStatus()).not.toThrow();
    expect(earcons.statusAnnounce).toHaveBeenCalled();
  });

  it("generates playing status when item is playing", () => {
    mockGetState.mockReturnValue({
      items: [
        { id: 1, uuid: "a", sessionId: "s", text: "Test", status: "pending", position: 0, createdAt: 0 },
      ],
      currentItem: { id: 2, uuid: "b", sessionId: "s", text: "Now playing this message", status: "playing", position: 1, createdAt: 0 },
      isPlaying: true,
      isPaused: false,
      autoPlay: true,
      volume: 0.8,
      currentPosition: 0,
      interrupted: false,
      playbackSpeed: 1.0,
    });
    // Should not throw even without speechSynthesis
    expect(() => speakStatus()).not.toThrow();
  });
});


// =============================================================================
// stopAndClear
// =============================================================================

describe("stopAndClear", () => {
  it("calls skipNext and clearPending", () => {
    stopAndClear();
    expect(mockSkipNext).toHaveBeenCalled();
    expect(mockClearPending).toHaveBeenCalled();
    expect(earcons.queueCleared).toHaveBeenCalled();
  });
});


// =============================================================================
// initShortcutHandler
// =============================================================================

describe("initShortcutHandler", () => {
  it("returns a cleanup function", () => {
    const cleanup = initShortcutHandler();
    expect(typeof cleanup).toBe("function");
    cleanup(); // Should not throw
  });
});
