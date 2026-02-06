/**
 * Earcon playback utility for audio feedback on queue actions.
 * Uses Web Audio API to generate short synthesized tones.
 */

const audioCtx = new (window.AudioContext || (window as any).webkitAudioContext)();

function playTone(frequency: number, durationMs: number, type: OscillatorType = 'sine', volume = 0.15) {
  const osc = audioCtx.createOscillator();
  const gain = audioCtx.createGain();
  osc.type = type;
  osc.frequency.setValueAtTime(frequency, audioCtx.currentTime);
  gain.gain.setValueAtTime(volume, audioCtx.currentTime);
  gain.gain.exponentialRampToValueAtTime(0.001, audioCtx.currentTime + durationMs / 1000);
  osc.connect(gain);
  gain.connect(audioCtx.destination);
  osc.start();
  osc.stop(audioCtx.currentTime + durationMs / 1000);
}

function playChord(frequencies: number[], durationMs: number, type: OscillatorType = 'sine', volume = 0.1) {
  frequencies.forEach(f => playTone(f, durationMs, type, volume));
}

export const earcons = {
  /** Pause/resume toggle */
  pauseResume() {
    playTone(880, 80, 'sine', 0.12);
  },

  /** Skip to next item */
  skipNext() {
    playTone(660, 60, 'sine', 0.1);
    setTimeout(() => playTone(880, 60, 'sine', 0.1), 70);
  },

  /** Replay current item */
  replayCurrent() {
    playTone(880, 60, 'sine', 0.1);
    setTimeout(() => playTone(660, 60, 'sine', 0.1), 70);
  },

  /** Speed change */
  speedChange() {
    playTone(1200, 40, 'triangle', 0.08);
  },

  /** Volume change */
  volumeChange() {
    playTone(440, 50, 'sine', 0.08);
  },

  /** Queue cleared / stopped */
  queueCleared() {
    playTone(440, 100, 'sine', 0.1);
    setTimeout(() => playTone(330, 150, 'sine', 0.1), 110);
  },

  /** Status announcement */
  statusAnnounce() {
    playChord([523, 659], 80, 'sine', 0.08);
  },

  /** Critical priority item arrived */
  criticalAlert() {
    playChord([880, 1100], 120, 'square', 0.12);
    setTimeout(() => playChord([880, 1100], 120, 'square', 0.12), 150);
  },

  /** Interrupt (recording started) */
  interrupt() {
    playTone(600, 60, 'triangle', 0.1);
  },

  /** Resume from interrupt */
  interruptResume() {
    playTone(800, 60, 'triangle', 0.1);
  },

  /** Screen reader capture started */
  screenReaderStart() {
    playTone(523, 80, 'sine', 0.15);
    setTimeout(() => playTone(784, 80, 'sine', 0.15), 90);
  },

  /** Screen reader description received */
  screenReaderDone() {
    playTone(784, 60, 'sine', 0.12);
    setTimeout(() => playTone(1047, 80, 'sine', 0.12), 70);
  },

  /** Screen reader failed */
  screenReaderError() {
    playTone(330, 120, 'square', 0.12);
    setTimeout(() => playTone(220, 150, 'square', 0.12), 130);
  },

  /** Screen reader ask started (Alt+A recording) */
  screenReaderAskStart() {
    playTone(440, 60, 'sine', 0.12);
    setTimeout(() => playTone(660, 60, 'sine', 0.12), 70);
  },

  /** Screen reader ask answered */
  screenReaderAskDone() {
    playTone(660, 60, 'sine', 0.12);
    setTimeout(() => playTone(880, 80, 'sine', 0.12), 70);
  },

  /** Screen reader ask failed */
  screenReaderAskError() {
    playTone(300, 100, 'square', 0.12);
    setTimeout(() => playTone(200, 130, 'square', 0.12), 110);
  },
};
