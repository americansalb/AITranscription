/**
 * Frontend keyboard shortcut handler for queue controls.
 * Works with Tauri global shortcuts for system-wide hotkeys,
 * plus in-window keydown fallback.
 */

import * as queueStore from './queueStore';
import { earcons } from './earcons';

/** Current playback speed rate */
let playbackSpeed = 1.0;

/** Get playback speed */
export function getPlaybackSpeed(): number {
  return playbackSpeed;
}

/** Adjust playback speed by delta (clamped 0.5-3.0) */
export function adjustSpeed(delta: number): void {
  playbackSpeed = Math.round(Math.max(0.5, Math.min(3.0, playbackSpeed + delta)) * 100) / 100;
  const audio = (queueStore as any).getCurrentAudio?.();
  if (audio) {
    audio.playbackRate = playbackSpeed;
  }
  earcons.speedChange();
  console.log(`[Shortcuts] Speed: ${playbackSpeed}x`);
}

/** Adjust volume by delta (clamped 0-1) */
export function adjustVolume(delta: number): void {
  const state = queueStore.getState();
  const newVol = Math.round(Math.max(0, Math.min(1, state.volume + delta)) * 100) / 100;
  queueStore.setVolume(newVol);
  earcons.volumeChange();
  console.log(`[Shortcuts] Volume: ${Math.round(newVol * 100)}%`);
}

/** Replay current playing item from the beginning */
export function replayCurrent(): void {
  const audio = (queueStore as any).getCurrentAudio?.();
  if (audio) {
    audio.currentTime = 0;
    earcons.replayCurrent();
    console.log('[Shortcuts] Replaying current');
  }
}

/** Replay last completed item */
export function replayLastCompleted(): void {
  earcons.replayCurrent();
  queueStore.skipPrevious();
  console.log('[Shortcuts] Replaying last completed');
}

/** Speak queue status aloud using browser TTS */
export function speakStatus(): void {
  earcons.statusAnnounce();
  const state = queueStore.getState();
  const pending = state.items.filter(i => i.status === 'pending').length;
  const current = state.currentItem;

  let statusText = '';
  if (current && state.isPlaying) {
    statusText = `Now playing: ${current.text.substring(0, 60)}. `;
  } else if (state.isPaused && current) {
    statusText = `Paused: ${current.text.substring(0, 60)}. `;
  } else {
    statusText = 'Queue is idle. ';
  }
  statusText += `${pending} items pending. Speed ${playbackSpeed}x. Volume ${Math.round(state.volume * 100)}%.`;

  if (window.speechSynthesis) {
    window.speechSynthesis.cancel();
    const utterance = new SpeechSynthesisUtterance(statusText);
    utterance.rate = 1.2;
    window.speechSynthesis.speak(utterance);
  }
  console.log('[Shortcuts] Status:', statusText);
}

/** Stop and clear entire queue */
export function stopAndClear(): void {
  earcons.queueCleared();
  // Stop current audio
  queueStore.skipNext(); // stops current
  queueStore.clearPending();
  console.log('[Shortcuts] Queue stopped and cleared');
}

/**
 * Handle a keyboard shortcut event.
 * Returns true if the event was handled.
 */
export function handleShortcutEvent(e: KeyboardEvent): boolean {
  const ctrl = e.ctrlKey || e.metaKey;
  const shift = e.shiftKey;

  // Space - Pause/Resume (only when not in input)
  if (e.code === 'Space' && !ctrl && !shift && !isInputFocused()) {
    e.preventDefault();
    earcons.pauseResume();
    queueStore.togglePlayPause();
    return true;
  }

  // Ctrl+Right or F4 - Skip to next
  if ((ctrl && !shift && e.code === 'ArrowRight') || e.code === 'F4') {
    e.preventDefault();
    earcons.skipNext();
    queueStore.skipNext();
    return true;
  }

  // Ctrl+Left - Replay current
  if (ctrl && !shift && e.code === 'ArrowLeft') {
    e.preventDefault();
    replayCurrent();
    return true;
  }

  // Ctrl+Up - Speed +0.25
  if (ctrl && !shift && e.code === 'ArrowUp') {
    e.preventDefault();
    adjustSpeed(0.25);
    return true;
  }

  // Ctrl+Down - Speed -0.25
  if (ctrl && !shift && e.code === 'ArrowDown') {
    e.preventDefault();
    adjustSpeed(-0.25);
    return true;
  }

  // Ctrl+Shift+Up - Volume +10%
  if (ctrl && shift && e.code === 'ArrowUp') {
    e.preventDefault();
    adjustVolume(0.1);
    return true;
  }

  // Ctrl+Shift+Down - Volume -10%
  if (ctrl && shift && e.code === 'ArrowDown') {
    e.preventDefault();
    adjustVolume(-0.1);
    return true;
  }

  // Ctrl+R - Replay last completed
  if (ctrl && !shift && e.code === 'KeyR') {
    e.preventDefault();
    replayLastCompleted();
    return true;
  }

  // Ctrl+S - Speak status
  if (ctrl && !shift && e.code === 'KeyS') {
    e.preventDefault();
    speakStatus();
    return true;
  }

  // Escape - Stop and clear
  if (e.code === 'Escape' && !ctrl && !shift) {
    stopAndClear();
    return true;
  }

  return false;
}

function isInputFocused(): boolean {
  const el = document.activeElement;
  if (!el) return false;
  const tag = el.tagName.toLowerCase();
  return tag === 'input' || tag === 'textarea' || tag === 'select' || (el as HTMLElement).isContentEditable;
}

/** Initialize keyboard shortcut listener (call once on app startup) */
export function initShortcutHandler(): () => void {
  const handler = (e: KeyboardEvent) => handleShortcutEvent(e);
  window.addEventListener('keydown', handler);
  console.log('[Shortcuts] Keyboard shortcut handler initialized');
  return () => window.removeEventListener('keydown', handler);
}
