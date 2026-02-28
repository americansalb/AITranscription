# Plan: Fix Simultaneous Mode + Add Read-Aloud TTS

## Part 1: Fix Simultaneous Mode Reliability

**Root cause**: The `simultaneousProcessing` boolean guard is a dead lock — if a Whisper+LLM request hangs or takes >5s, *every subsequent chunk is silently dropped*. The user sees nothing, thinks it stopped.

### Changes:

**A. Add timeout + abort to the processing guard** (`App.tsx`)
- Replace the simple boolean guard with an AbortController-based approach
- Each new chunk **cancels** the previous in-flight request if it hasn't returned yet (via AbortController.abort())
- This means the most recent chunk always wins — no more "stuck on old request" deadlocks
- Set a 15-second hard timeout on fetch calls in `api.ts` so requests can't hang forever

**B. Add AbortSignal support to `api.ts`**
- Add an optional `signal?: AbortSignal` parameter to `interpret()` and `transcribe()`
- Pass it through to `fetch()` so in-flight requests can be cancelled from the frontend

**C. Remove the length-based guard** (`App.tsx`)
- The `result.source_text.length >= lastSimultaneousText.current.length` check causes stale results to be rejected even when they're correct (Whisper can revise text to be shorter but more accurate)
- Replace with: always accept the latest result from the latest chunk (seq-based, not length-based)

---

## Part 2: Add Read-Aloud TTS

**Vision**: A toggleable read-aloud mode. When enabled, completed translations are spoken aloud using browser Speech Synthesis. Triggered after configurable silence (no new text). User can pause/interrupt. Voice is selectable.

### New files:

**A. `hooks/useSpeechSynthesis.ts`** — Custom React hook wrapping the Web Speech API

State it exposes:
- `voices: SpeechSynthesisVoice[]` — available voices (loaded async via `voiceschanged` event)
- `isSpeaking: boolean` — currently speaking
- `isPaused: boolean` — paused mid-utterance

Methods:
- `speak(text: string, voice: SpeechSynthesisVoice, rate?: number)` — queue and speak text
- `pause()` — pause current utterance
- `resume()` — resume paused utterance
- `stop()` — cancel all queued speech immediately
- Cleanup: cancels speech on unmount

Internally:
- Listens for `voiceschanged` event to populate voice list
- Filters voices to only show voices matching the target language
- Handles the known Chrome bug where `speechSynthesis.speaking` gets stuck (workaround: cancel + re-queue after 14 seconds of silence)

**B. Settings additions** (`SettingsPanel.tsx` + `InterpretationSettings` type)

New settings fields:
- `ttsEnabled: boolean` — master toggle for read-aloud (default: false)
- `ttsVoice: string` — voice URI (default: "" = system default)
- `ttsSilenceDelay: number` — seconds of "no new text" before speaking (default: 2.0s, range 0.5–5.0)
- `ttsRate: number` — speech rate (default: 1.0, range 0.5–2.0)

New UI in settings panel (below the LLM Provider row, only shown when mode = "interpret"):
- "Read Aloud" toggle button row (on/off)
- When enabled, shows:
  - Voice dropdown (populated from `useSpeechSynthesis` voices, filtered to target language)
  - Silence delay slider (same style as existing pause threshold slider)
  - Speed slider (0.5x–2.0x)

**C. TTS integration in `App.tsx`**

Logic:
1. Track `lastSpokenEntryId` ref — which entry we last spoke
2. Track `ttsSilenceTimer` ref — a timer that fires after `ttsSilenceDelay` seconds of no text changes
3. When an entry transitions to status `"complete"` (or when `translatedText` stops changing for `ttsSilenceDelay` seconds):
   - If TTS is enabled and the entry hasn't been spoken yet
   - Call `speak(entry.translatedText, selectedVoice)`
   - Mark entry as spoken (via the ref)
4. For simultaneous mode: watch for text stabilization (no change for N seconds), then speak the delta (new text since last spoken position)

**D. Pause/interrupt controls**

In the UI, when TTS is active and speaking:
- Show a small "Speaking..." indicator near the controls area (bottom of screen, above the record button)
- A pause/resume toggle button
- A stop button to cancel speech entirely
- If the user starts recording while TTS is speaking, auto-stop the speech (don't talk over the microphone)

**E. CSS** (`styles.css`)
- Style the TTS settings row (toggle, voice dropdown, sliders)
- Style the speaking indicator bar (subtle bar at the bottom with pause/stop controls)

---

## Implementation Order

1. Fix simultaneous mode (Part 1A–C) — ~3 files changed
2. Create `useSpeechSynthesis` hook (Part 2A) — 1 new file
3. Add TTS settings to `SettingsPanel` (Part 2B) — 1 file changed
4. Wire TTS into `App.tsx` (Part 2C–D) — 1 file changed
5. Add CSS for TTS controls (Part 2E) — 1 file changed
6. Build and push

## Files touched
- `vaak-lite/frontend/src/lib/api.ts` — add AbortSignal + timeout
- `vaak-lite/frontend/src/App.tsx` — fix simultaneous guard, add TTS wiring
- `vaak-lite/frontend/src/hooks/useSpeechSynthesis.ts` — **new file**
- `vaak-lite/frontend/src/components/SettingsPanel.tsx` — TTS settings
- `vaak-lite/frontend/src/styles.css` — TTS styling
- `backend/app/vaaklite/static/` — rebuilt frontend
