/**
 * Interrupt Manager - auto-pause TTS when recording starts (F3),
 * auto-resume when recording stops. Prevents auto-advance during interruption.
 */

import * as queueStore from './queueStore';
import { earcons } from './earcons';

let isInterrupted = false;
let wasPlayingBeforeInterrupt = false;

/** Whether the queue is currently interrupted by recording */
export function getIsInterrupted(): boolean {
  return isInterrupted;
}

/**
 * Called when recording starts (F3 pressed).
 * Pauses TTS playback if currently playing.
 */
export function onRecordingStart(): void {
  const state = queueStore.getState();

  if (state.isPlaying && !state.isPaused) {
    wasPlayingBeforeInterrupt = true;
    queueStore.pause();
    earcons.interrupt();
    console.log('[InterruptManager] Paused TTS for recording');
  } else {
    wasPlayingBeforeInterrupt = false;
  }

  isInterrupted = true;
}

/**
 * Called when recording stops.
 * Resumes TTS playback if it was playing before interruption.
 */
export function onRecordingStop(): void {
  isInterrupted = false;

  if (wasPlayingBeforeInterrupt) {
    wasPlayingBeforeInterrupt = false;
    // Small delay to let recording processing finish
    setTimeout(() => {
      const state = queueStore.getState();
      if (state.isPaused) {
        queueStore.resume();
        earcons.interruptResume();
        console.log('[InterruptManager] Resumed TTS after recording');
      }
    }, 300);
  }
}
