/**
 * Tests for interruptManager — auto-pause/resume TTS during recording.
 *
 * Covers:
 *   - getIsInterrupted: initial state and after transitions
 *   - onRecordingStart: pauses when playing, no-op when not playing
 *   - onRecordingStop: resumes if was playing, no-op otherwise
 *   - State transitions: playing→interrupted→resumed, idle→interrupted→idle
 *   - Earcon playback on interrupt/resume
 *   - setTimeout delay on resume (300ms)
 */
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

// Mock queueStore before importing interruptManager
vi.mock("../lib/queueStore", () => ({
  getState: vi.fn().mockReturnValue({
    items: [],
    currentItem: null,
    isPlaying: false,
    isPaused: false,
    autoPlay: true,
    volume: 0.8,
    currentPosition: 0,
    interrupted: false,
    playbackSpeed: 1.0,
  }),
  pause: vi.fn(),
  resume: vi.fn(),
}));

// Mock earcons before importing interruptManager
vi.mock("../lib/earcons", () => ({
  earcons: {
    interrupt: vi.fn(),
    interruptResume: vi.fn(),
  },
}));

import {
  getIsInterrupted,
  onRecordingStart,
  onRecordingStop,
} from "../lib/interruptManager";
import * as queueStore from "../lib/queueStore";
import { earcons } from "../lib/earcons";

const mockGetState = vi.mocked(queueStore.getState);
const mockPause = vi.mocked(queueStore.pause);
const mockResume = vi.mocked(queueStore.resume);


beforeEach(() => {
  vi.clearAllMocks();
  vi.useFakeTimers();
  // Reset module state by triggering a stop (clears isInterrupted)
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
  onRecordingStop();
  vi.runAllTimers();
  vi.clearAllMocks();
});

afterEach(() => {
  vi.useRealTimers();
});


// =============================================================================
// INITIAL STATE
// =============================================================================

describe("getIsInterrupted — initial state", () => {
  it("returns false initially", () => {
    expect(getIsInterrupted()).toBe(false);
  });
});


// =============================================================================
// onRecordingStart
// =============================================================================

describe("onRecordingStart", () => {
  it("sets interrupted to true", () => {
    onRecordingStart();
    expect(getIsInterrupted()).toBe(true);
  });

  it("pauses TTS when currently playing", () => {
    mockGetState.mockReturnValue({
      items: [],
      currentItem: null,
      isPlaying: true,
      isPaused: false,
      autoPlay: true,
      volume: 0.8,
      currentPosition: 0,
      interrupted: false,
      playbackSpeed: 1.0,
    });

    onRecordingStart();

    expect(mockPause).toHaveBeenCalledOnce();
    expect(earcons.interrupt).toHaveBeenCalledOnce();
  });

  it("does NOT pause when not playing", () => {
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

    onRecordingStart();

    expect(mockPause).not.toHaveBeenCalled();
    expect(earcons.interrupt).not.toHaveBeenCalled();
  });

  it("does NOT pause when already paused", () => {
    mockGetState.mockReturnValue({
      items: [],
      currentItem: null,
      isPlaying: true,
      isPaused: true,
      autoPlay: true,
      volume: 0.8,
      currentPosition: 0,
      interrupted: false,
      playbackSpeed: 1.0,
    });

    onRecordingStart();

    expect(mockPause).not.toHaveBeenCalled();
  });
});


// =============================================================================
// onRecordingStop
// =============================================================================

describe("onRecordingStop", () => {
  it("clears interrupted flag", () => {
    onRecordingStart();
    expect(getIsInterrupted()).toBe(true);

    onRecordingStop();
    expect(getIsInterrupted()).toBe(false);
  });

  it("resumes TTS after 300ms delay when was playing before interrupt", () => {
    // Start with playing state
    mockGetState.mockReturnValue({
      items: [],
      currentItem: null,
      isPlaying: true,
      isPaused: false,
      autoPlay: true,
      volume: 0.8,
      currentPosition: 0,
      interrupted: false,
      playbackSpeed: 1.0,
    });

    onRecordingStart();
    vi.clearAllMocks();

    // After stop, state is paused (from the interrupt)
    mockGetState.mockReturnValue({
      items: [],
      currentItem: null,
      isPlaying: false,
      isPaused: true,
      autoPlay: true,
      volume: 0.8,
      currentPosition: 0,
      interrupted: false,
      playbackSpeed: 1.0,
    });

    onRecordingStop();

    // Resume should NOT happen immediately
    expect(mockResume).not.toHaveBeenCalled();

    // After 300ms delay
    vi.advanceTimersByTime(300);
    expect(mockResume).toHaveBeenCalledOnce();
    expect(earcons.interruptResume).toHaveBeenCalledOnce();
  });

  it("does NOT resume when was not playing before interrupt", () => {
    // Start with idle state (not playing)
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

    onRecordingStart();
    onRecordingStop();
    vi.advanceTimersByTime(300);

    expect(mockResume).not.toHaveBeenCalled();
    expect(earcons.interruptResume).not.toHaveBeenCalled();
  });

  it("does NOT resume if state changed to not-paused during delay", () => {
    // Start with playing state
    mockGetState.mockReturnValue({
      items: [],
      currentItem: null,
      isPlaying: true,
      isPaused: false,
      autoPlay: true,
      volume: 0.8,
      currentPosition: 0,
      interrupted: false,
      playbackSpeed: 1.0,
    });

    onRecordingStart();

    // During stop, state is NOT paused (user manually resumed or queue moved on)
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

    onRecordingStop();
    vi.advanceTimersByTime(300);

    expect(mockResume).not.toHaveBeenCalled();
  });
});


// =============================================================================
// FULL STATE TRANSITION CYCLE
// =============================================================================

describe("full interrupt cycle", () => {
  it("playing → interrupt → resume → back to normal", () => {
    // 1. Start playing
    mockGetState.mockReturnValue({
      items: [],
      currentItem: null,
      isPlaying: true,
      isPaused: false,
      autoPlay: true,
      volume: 0.8,
      currentPosition: 0,
      interrupted: false,
      playbackSpeed: 1.0,
    });

    // 2. Recording starts → interrupt
    onRecordingStart();
    expect(getIsInterrupted()).toBe(true);
    expect(mockPause).toHaveBeenCalledOnce();

    // 3. State is now paused
    mockGetState.mockReturnValue({
      items: [],
      currentItem: null,
      isPlaying: false,
      isPaused: true,
      autoPlay: true,
      volume: 0.8,
      currentPosition: 0,
      interrupted: false,
      playbackSpeed: 1.0,
    });

    // 4. Recording stops → schedule resume
    onRecordingStop();
    expect(getIsInterrupted()).toBe(false);

    // 5. After 300ms → resume
    vi.advanceTimersByTime(300);
    expect(mockResume).toHaveBeenCalledOnce();
  });

  it("idle → interrupt → stop → stays idle", () => {
    // Not playing
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

    onRecordingStart();
    expect(getIsInterrupted()).toBe(true);
    expect(mockPause).not.toHaveBeenCalled();

    onRecordingStop();
    vi.advanceTimersByTime(300);
    expect(getIsInterrupted()).toBe(false);
    expect(mockResume).not.toHaveBeenCalled();
  });
});
