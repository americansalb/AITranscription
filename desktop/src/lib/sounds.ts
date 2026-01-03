/**
 * Audio feedback for recording events
 *
 * Uses Web Audio API to generate simple tones - no external files needed.
 * Sounds are subtle and professional, similar to Wispr Flow.
 */

let audioContext: AudioContext | null = null;

function getAudioContext(): AudioContext {
  if (!audioContext) {
    audioContext = new AudioContext();
  }
  return audioContext;
}

/**
 * Play a simple tone
 */
function playTone(
  frequency: number,
  duration: number,
  volume: number = 0.3,
  type: OscillatorType = "sine"
): void {
  try {
    const ctx = getAudioContext();

    // Resume context if suspended (required by browsers)
    if (ctx.state === "suspended") {
      ctx.resume();
    }

    const oscillator = ctx.createOscillator();
    const gainNode = ctx.createGain();

    oscillator.connect(gainNode);
    gainNode.connect(ctx.destination);

    oscillator.type = type;
    oscillator.frequency.setValueAtTime(frequency, ctx.currentTime);

    // Smooth fade in/out to avoid clicks
    gainNode.gain.setValueAtTime(0, ctx.currentTime);
    gainNode.gain.linearRampToValueAtTime(volume, ctx.currentTime + 0.01);
    gainNode.gain.linearRampToValueAtTime(0, ctx.currentTime + duration);

    oscillator.start(ctx.currentTime);
    oscillator.stop(ctx.currentTime + duration);
  } catch (error) {
    console.error("Failed to play sound:", error);
  }
}

/**
 * Sound when recording starts - a quick ascending "boop"
 */
export function playStartSound(): void {
  // Two quick ascending tones
  playTone(440, 0.08, 0.25); // A4
  setTimeout(() => playTone(880, 0.1, 0.2), 60); // A5
}

/**
 * Sound when recording stops - a quick descending "boop"
 */
export function playStopSound(): void {
  // Two quick descending tones
  playTone(660, 0.08, 0.25); // E5
  setTimeout(() => playTone(440, 0.1, 0.2), 60); // A4
}

/**
 * Sound when transcription succeeds - a pleasant chime
 */
export function playSuccessSound(): void {
  // Pleasant chord
  playTone(523, 0.15, 0.15); // C5
  setTimeout(() => playTone(659, 0.15, 0.15), 50); // E5
  setTimeout(() => playTone(784, 0.2, 0.12), 100); // G5
}

/**
 * Sound when an error occurs - a subtle low tone
 */
export function playErrorSound(): void {
  playTone(220, 0.2, 0.2); // A3
  setTimeout(() => playTone(196, 0.25, 0.15), 100); // G3
}
