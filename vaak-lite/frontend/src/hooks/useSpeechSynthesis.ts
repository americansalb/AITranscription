import { useCallback, useEffect, useRef, useState } from "react";

export interface UseSpeechSynthesisReturn {
  /** All available voices on this device. */
  voices: SpeechSynthesisVoice[];
  /** Voices filtered to a specific language code (e.g. "en", "es"). */
  voicesForLang: (lang: string) => SpeechSynthesisVoice[];
  /** Whether the synth is currently speaking. */
  isSpeaking: boolean;
  /** Whether speech is paused. */
  isPaused: boolean;
  /** Speak text. Cancels any current speech first. */
  speak: (text: string, voice?: SpeechSynthesisVoice | null, rate?: number) => void;
  /** Pause current speech. */
  pause: () => void;
  /** Resume paused speech. */
  resume: () => void;
  /** Cancel all speech immediately. */
  stop: () => void;
  /** Whether the browser supports speech synthesis. */
  supported: boolean;
}

export function useSpeechSynthesis(): UseSpeechSynthesisReturn {
  const [voices, setVoices] = useState<SpeechSynthesisVoice[]>([]);
  const [isSpeaking, setIsSpeaking] = useState(false);
  const [isPaused, setIsPaused] = useState(false);
  const supported = typeof window !== "undefined" && "speechSynthesis" in window;

  // Chrome has a bug where long utterances stop firing events after ~15s.
  // Workaround: periodically call pause/resume to keep it alive.
  const keepAliveRef = useRef<number>(0);

  // Load voices (async on some browsers)
  useEffect(() => {
    if (!supported) return;
    const synth = window.speechSynthesis;

    const loadVoices = () => {
      const v = synth.getVoices();
      if (v.length > 0) setVoices(v);
    };

    loadVoices();
    synth.addEventListener("voiceschanged", loadVoices);
    return () => synth.removeEventListener("voiceschanged", loadVoices);
  }, [supported]);

  // Cleanup on unmount
  useEffect(() => {
    return () => {
      if (supported) window.speechSynthesis.cancel();
      if (keepAliveRef.current) clearInterval(keepAliveRef.current);
    };
  }, [supported]);

  const voicesForLang = useCallback(
    (lang: string) => {
      const code = lang.toLowerCase().slice(0, 2);
      return voices.filter((v) => v.lang.toLowerCase().startsWith(code));
    },
    [voices],
  );

  const speak = useCallback(
    (text: string, voice?: SpeechSynthesisVoice | null, rate = 1.0) => {
      if (!supported || !text.trim()) return;
      const synth = window.speechSynthesis;

      // Cancel any current speech
      synth.cancel();
      if (keepAliveRef.current) {
        clearInterval(keepAliveRef.current);
        keepAliveRef.current = 0;
      }

      const utterance = new SpeechSynthesisUtterance(text);
      if (voice) utterance.voice = voice;
      utterance.rate = rate;

      utterance.onstart = () => {
        setIsSpeaking(true);
        setIsPaused(false);
      };
      utterance.onend = () => {
        setIsSpeaking(false);
        setIsPaused(false);
        if (keepAliveRef.current) {
          clearInterval(keepAliveRef.current);
          keepAliveRef.current = 0;
        }
      };
      utterance.onerror = () => {
        setIsSpeaking(false);
        setIsPaused(false);
        if (keepAliveRef.current) {
          clearInterval(keepAliveRef.current);
          keepAliveRef.current = 0;
        }
      };
      utterance.onpause = () => setIsPaused(true);
      utterance.onresume = () => setIsPaused(false);

      synth.speak(utterance);

      // Chrome keep-alive: pause+resume every 10s to prevent the synth
      // from silently stopping on long texts.
      keepAliveRef.current = window.setInterval(() => {
        if (synth.speaking && !synth.paused) {
          synth.pause();
          synth.resume();
        }
      }, 10000);
    },
    [supported],
  );

  const pause = useCallback(() => {
    if (supported) window.speechSynthesis.pause();
  }, [supported]);

  const resume = useCallback(() => {
    if (supported) window.speechSynthesis.resume();
  }, [supported]);

  const stop = useCallback(() => {
    if (!supported) return;
    window.speechSynthesis.cancel();
    setIsSpeaking(false);
    setIsPaused(false);
    if (keepAliveRef.current) {
      clearInterval(keepAliveRef.current);
      keepAliveRef.current = 0;
    }
  }, [supported]);

  return { voices, voicesForLang, isSpeaking, isPaused, speak, pause, resume, stop, supported };
}
