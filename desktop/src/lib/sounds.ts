/**
 * Sound feedback utilities for transcription events
 * Uses Web Audio API to generate tones (no external audio files needed)
 */

let audioContext: AudioContext | null = null;

function getAudioContext(): AudioContext {
  if (!audioContext) {
    audioContext = new AudioContext();
  }
  return audioContext;
}

/**
 * Play a tone with the given frequency and duration
 */
function playTone(frequency: number, duration: number, volume = 0.3): void {
  try {
    const ctx = getAudioContext();

    const oscillator = ctx.createOscillator();
    const gainNode = ctx.createGain();

    oscillator.connect(gainNode);
    gainNode.connect(ctx.destination);

    oscillator.frequency.value = frequency;
    oscillator.type = "sine";

    // Fade in/out to avoid clicks
    const now = ctx.currentTime;
    gainNode.gain.setValueAtTime(0, now);
    gainNode.gain.linearRampToValueAtTime(volume, now + 0.01);
    gainNode.gain.linearRampToValueAtTime(0, now + duration);

    oscillator.start(now);
    oscillator.stop(now + duration);
  } catch (error) {
    console.error("Failed to play sound:", error);
  }
}

/**
 * Play the "recording started" sound - ascending tone
 */
export function playStartSound(): void {
  const ctx = getAudioContext();
  const now = ctx.currentTime;

  try {
    // Two quick ascending tones
    const osc1 = ctx.createOscillator();
    const osc2 = ctx.createOscillator();
    const gain = ctx.createGain();

    osc1.connect(gain);
    osc2.connect(gain);
    gain.connect(ctx.destination);

    osc1.frequency.value = 440; // A4
    osc2.frequency.value = 554; // C#5
    osc1.type = "sine";
    osc2.type = "sine";

    gain.gain.setValueAtTime(0, now);
    gain.gain.linearRampToValueAtTime(0.2, now + 0.02);
    gain.gain.linearRampToValueAtTime(0, now + 0.15);

    osc1.start(now);
    osc1.stop(now + 0.08);

    osc2.start(now + 0.08);
    osc2.stop(now + 0.15);
  } catch (error) {
    console.error("Failed to play start sound:", error);
  }
}

/**
 * Play the "recording stopped / processing" sound - descending tone
 */
export function playStopSound(): void {
  const ctx = getAudioContext();
  const now = ctx.currentTime;

  try {
    // Two quick descending tones
    const osc1 = ctx.createOscillator();
    const osc2 = ctx.createOscillator();
    const gain = ctx.createGain();

    osc1.connect(gain);
    osc2.connect(gain);
    gain.connect(ctx.destination);

    osc1.frequency.value = 554; // C#5
    osc2.frequency.value = 440; // A4
    osc1.type = "sine";
    osc2.type = "sine";

    gain.gain.setValueAtTime(0, now);
    gain.gain.linearRampToValueAtTime(0.2, now + 0.02);
    gain.gain.linearRampToValueAtTime(0, now + 0.15);

    osc1.start(now);
    osc1.stop(now + 0.08);

    osc2.start(now + 0.08);
    osc2.stop(now + 0.15);
  } catch (error) {
    console.error("Failed to play stop sound:", error);
  }
}

/**
 * Play the "success" sound - pleasant chord
 */
export function playSuccessSound(): void {
  try {
    const ctx = getAudioContext();
    const now = ctx.currentTime;

    // Play a major chord
    const frequencies = [523, 659, 784]; // C5, E5, G5
    const gain = ctx.createGain();
    gain.connect(ctx.destination);

    gain.gain.setValueAtTime(0, now);
    gain.gain.linearRampToValueAtTime(0.15, now + 0.02);
    gain.gain.linearRampToValueAtTime(0, now + 0.3);

    frequencies.forEach(freq => {
      const osc = ctx.createOscillator();
      osc.connect(gain);
      osc.frequency.value = freq;
      osc.type = "sine";
      osc.start(now);
      osc.stop(now + 0.3);
    });
  } catch (error) {
    console.error("Failed to play success sound:", error);
  }
}

/**
 * Play the "error" sound - dissonant tone
 */
export function playErrorSound(): void {
  playTone(200, 0.2, 0.25);
}
