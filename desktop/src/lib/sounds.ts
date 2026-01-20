/**
 * Sound feedback utilities for transcription events
 * Uses Web Audio API to generate tones (no external audio files needed)
 */

let audioContext: AudioContext | null = null;

/**
 * Get or create the AudioContext, and ensure it's resumed.
 * Modern browsers suspend AudioContext until user interaction,
 * so we must resume it before playing sounds.
 */
async function getAudioContext(): Promise<AudioContext> {
  if (!audioContext) {
    audioContext = new AudioContext();
  }

  // Resume if suspended (common on Mac/Safari when triggered from global hotkey)
  if (audioContext.state === "suspended") {
    try {
      await audioContext.resume();
    } catch (e) {
      console.error("[Sounds] Failed to resume AudioContext:", e);
    }
  }

  return audioContext;
}

/**
 * Play a tone with the given frequency and duration
 */
async function playTone(frequency: number, duration: number, volume = 0.3): Promise<void> {
  try {
    const ctx = await getAudioContext();

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
    console.error("[Sounds] Failed to play tone:", error);
  }
}

/**
 * Play the "recording started" sound - ascending tone
 */
export async function playStartSound(): Promise<void> {
  try {
    const ctx = await getAudioContext();
    const now = ctx.currentTime;

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
    console.error("[Sounds] Failed to play start sound:", error);
  }
}

/**
 * Play the "recording stopped / processing" sound - descending tone
 */
export async function playStopSound(): Promise<void> {
  try {
    const ctx = await getAudioContext();
    const now = ctx.currentTime;

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
    console.error("[Sounds] Failed to play stop sound:", error);
  }
}

/**
 * Play the "success" sound - pleasant chord
 */
export async function playSuccessSound(): Promise<void> {
  try {
    const ctx = await getAudioContext();
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
    console.error("[Sounds] Failed to play success sound:", error);
  }
}

/**
 * Play the "error" sound - dissonant tone
 */
export async function playErrorSound(): Promise<void> {
  await playTone(200, 0.2, 0.25);
}
