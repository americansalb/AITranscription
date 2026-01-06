# Voice Response Feature - Implementation Plan

## Overview

Add spoken AI responses to Scribe, so users HEAR explanations instead of reading them. This transforms the app from "transcription tool" to "voice-first AI assistant."

---

## Current Architecture (What Exists)

```
USER SPEAKS
    ↓
[Frontend] useAudioRecorder captures audio blob
    ↓
[Frontend] POST /api/v1/transcribe-and-polish (lib/api.ts)
    ↓
[Backend] TranscriptionService → Groq Whisper (transcription.py)
    ↓
[Backend] PolishService → Claude Haiku (polish.py) [CURRENTLY DISABLED]
    ↓
[Backend] Response: {raw_text, polished_text, duration, usage}
    ↓
[Frontend] Display text + inject via clipboard/paste
    ↓
USER READS (this is the bottleneck we're solving)
```

---

## Proposed Architecture (What We're Adding)

```
USER SPEAKS
    ↓
[Existing Flow - unchanged]
    ↓
[Backend] Response: {raw_text, polished_text, ...}
    ↓
[NEW] If voice_response enabled:
    ↓
[Backend] ExplanationService → Claude Haiku
    │   Input: polished_text + context
    │   Output: spoken_explanation (conversational summary)
    ↓
[Backend] TTSService → Eleven Labs API
    │   Input: spoken_explanation text
    │   Output: audio bytes (mp3)
    ↓
[Frontend] Audio playback via Web Audio API
    ↓
USER HEARS (problem solved)
```

---

## Components To Build

### 1. Backend: ExplanationService (NEW)

**File:** `backend/app/services/explanation.py`

**Purpose:** Generate conversational, spoken-friendly explanations of text/code changes.

**Input:**
```python
{
    "content": str,           # The text/code to explain
    "content_type": str,      # "text" | "code" | "diff"
    "context": str,           # "email" | "code" | "general" etc.
    "skill_level": str,       # "beginner" | "intermediate" | "expert"
    "max_words": int          # Target ~50 words = ~15 seconds speech
}
```

**Output:**
```python
{
    "explanation": str,       # Conversational explanation for TTS
    "tokens_used": int
}
```

**Key Design Decisions:**
- Uses same Haiku model as polish service (`claude-3-5-haiku-20241022`)
- Prompt optimized for SPOKEN output (no markdown, no bullet points)
- Short by default (~50 words = ~15 seconds of speech)
- Adapts complexity to skill_level

**System Prompt Structure:**
```
You are explaining what just happened to a user via voice.

Rules:
1. Be conversational - this will be spoken aloud
2. Lead with WHAT happened in plain language
3. Keep it under {max_words} words
4. No markdown, no code snippets, no bullet points
5. Use "I" to describe actions ("I added...", "I fixed...")
6. End with a brief question or next step if appropriate
7. Complexity level: {skill_level}

Content to explain:
{content}

Context: {context}
```

---

### 2. Backend: TTSService (NEW)

**File:** `backend/app/services/tts.py`

**Purpose:** Convert text to speech using Eleven Labs API.

**Dependencies:**
```
elevenlabs  # Official Python SDK
```

**Input:**
```python
{
    "text": str,              # Text to speak
    "voice_id": str,          # Eleven Labs voice ID (default: configurable)
    "model_id": str,          # "eleven_turbo_v2_5" (fast) or "eleven_multilingual_v2"
    "stability": float,       # 0.0-1.0, default 0.5
    "similarity_boost": float # 0.0-1.0, default 0.75
}
```

**Output:**
```python
{
    "audio": bytes,           # MP3 audio data
    "duration_ms": int        # Estimated duration
}
```

**Key Design Decisions:**
- Use `eleven_turbo_v2_5` model for lowest latency (~300ms)
- Stream audio if possible (Eleven Labs supports streaming)
- Default voice: "Rachel" or configurable per user
- Cache common phrases? (probably not needed initially)

**Error Handling:**
- API key missing → Return None, log warning
- Rate limit hit → Return None with error message
- Network timeout → Retry once, then fail gracefully

---

### 3. Backend: New API Endpoint

**Option A: Extend existing endpoint**

Modify `/api/v1/transcribe-and-polish` to optionally return audio:

```python
# Request adds:
voice_response: bool = False
skill_level: str = "intermediate"

# Response adds:
voice_explanation: str | None      # The text that was spoken
voice_audio_base64: str | None     # Base64-encoded MP3
voice_duration_ms: int | None
```

**Pros:** Single request, simpler frontend
**Cons:** Larger response, couples features, slower response time

---

**Option B: Separate endpoint (RECOMMENDED)**

New endpoint: `POST /api/v1/explain`

```python
# Request:
{
    "content": str,           # Text/code to explain
    "content_type": str,      # "text" | "code" | "diff"
    "context": str,           # "email" | "code" | "general"
    "skill_level": str,       # "beginner" | "intermediate" | "expert"
    "return_audio": bool      # If true, include TTS audio
}

# Response:
{
    "explanation": str,       # The explanation text
    "audio_base64": str | None,  # Base64-encoded MP3 (if return_audio=true)
    "duration_ms": int | None,
    "tokens_used": int
}
```

**Pros:**
- Decoupled - can use explanation without TTS, or get explanation first then TTS
- Can be called independently (for code diffs, any content)
- Doesn't slow down main transcription flow
- Easier to test/debug

**Cons:**
- Two requests from frontend (can be parallel or sequential)

---

### 4. Backend: Configuration

**File:** `backend/app/core/config.py`

Add:
```python
# Eleven Labs
elevenlabs_api_key: str = os.getenv("ELEVENLABS_API_KEY", "")
elevenlabs_voice_id: str = os.getenv("ELEVENLABS_VOICE_ID", "Rachel")
elevenlabs_model: str = "eleven_turbo_v2_5"

# Explanation defaults
default_skill_level: str = "intermediate"
explanation_max_words: int = 50
```

**File:** `backend/.env`

Add:
```
ELEVENLABS_API_KEY=your_key_here
ELEVENLABS_VOICE_ID=Rachel  # or specific voice ID
```

---

### 5. Frontend: Audio Playback

**File:** `desktop/src/lib/audio.ts` (NEW)

```typescript
// Play base64-encoded MP3 audio
export async function playAudioResponse(base64Audio: string): Promise<void> {
  const audioData = Uint8Array.from(atob(base64Audio), c => c.charCodeAt(0));
  const audioBlob = new Blob([audioData], { type: 'audio/mpeg' });
  const audioUrl = URL.createObjectURL(audioBlob);

  const audio = new Audio(audioUrl);

  return new Promise((resolve, reject) => {
    audio.onended = () => {
      URL.revokeObjectURL(audioUrl);
      resolve();
    };
    audio.onerror = reject;
    audio.play();
  });
}

// Stop any playing audio (for interruption)
let currentAudio: HTMLAudioElement | null = null;

export function stopAudioResponse(): void {
  if (currentAudio) {
    currentAudio.pause();
    currentAudio = null;
  }
}
```

---

### 6. Frontend: API Integration

**File:** `desktop/src/lib/api.ts`

Add new function:
```typescript
export interface ExplainRequest {
  content: string;
  content_type: 'text' | 'code' | 'diff';
  context: string;
  skill_level: 'beginner' | 'intermediate' | 'expert';
  return_audio: boolean;
}

export interface ExplainResponse {
  explanation: string;
  audio_base64: string | null;
  duration_ms: number | null;
  tokens_used: number;
}

export async function explainContent(request: ExplainRequest): Promise<ExplainResponse> {
  const response = await fetch(`${API_URL}/api/v1/explain`, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      ...getAuthHeader(),
    },
    body: JSON.stringify(request),
  });

  if (!response.ok) {
    throw new ApiError(response.status, await response.text());
  }

  return response.json();
}
```

---

### 7. Frontend: Settings

**File:** `desktop/src/components/Settings.tsx`

Add to Preferences:
```typescript
// New state
const [voiceResponseEnabled, setVoiceResponseEnabled] = useState(() =>
  getStoredVoiceResponse()
);
const [skillLevel, setSkillLevel] = useState(() =>
  getStoredSkillLevel()
);

// New UI elements
<label className="toggle-setting">
  <span>Voice responses</span>
  <input
    type="checkbox"
    checked={voiceResponseEnabled}
    onChange={(e) => handleVoiceResponseChange(e.target.checked)}
  />
  <span className="toggle-switch" />
</label>
<p className="setting-hint">Hear explanations instead of reading them</p>

<div className="skill-level-setting">
  <span>Explanation detail level</span>
  <select value={skillLevel} onChange={...}>
    <option value="beginner">Simple (non-technical)</option>
    <option value="intermediate">Standard</option>
    <option value="expert">Technical</option>
  </select>
</div>
```

---

### 8. Frontend: Integration in App.tsx

**Modified flow after transcription:**

```typescript
// After successful transcription
const response = await transcribeAndPolish(audioBlob, options);
setResult(response.polished_text);

// NEW: If voice response enabled, get explanation and play it
if (voiceResponseEnabled) {
  try {
    const explanation = await explainContent({
      content: response.polished_text,
      content_type: 'text',
      context: context,
      skill_level: skillLevel,
      return_audio: true,
    });

    if (explanation.audio_base64) {
      await playAudioResponse(explanation.audio_base64);
    }
  } catch (error) {
    // Voice response is optional - don't fail the whole flow
    console.warn('Voice response failed:', error);
  }
}

// Continue with paste injection
await injectText(response.polished_text);
```

---

## Data Flow Diagram

```
┌─────────────────────────────────────────────────────────────────────┐
│                         USER SPEAKS                                  │
└─────────────────────────────────────────────────────────────────────┘
                                │
                                ▼
┌─────────────────────────────────────────────────────────────────────┐
│  FRONTEND: App.tsx                                                   │
│  ├─ Capture audio via useAudioRecorder                              │
│  ├─ POST /api/v1/transcribe-and-polish                              │
│  └─ Receive: {raw_text, polished_text}                              │
└─────────────────────────────────────────────────────────────────────┘
                                │
                    ┌───────────┴───────────┐
                    │                       │
                    ▼                       ▼
┌──────────────────────────┐    ┌──────────────────────────────────┐
│  INJECT TEXT (parallel)   │    │  VOICE RESPONSE (parallel)        │
│  ├─ Copy to clipboard     │    │  POST /api/v1/explain              │
│  └─ Simulate Ctrl+V       │    │  ├─ content: polished_text        │
└──────────────────────────┘    │  ├─ return_audio: true             │
                                 │  └─ skill_level: user_pref         │
                                 └──────────────────────────────────┘
                                                  │
                                                  ▼
                                 ┌──────────────────────────────────┐
                                 │  BACKEND: /api/v1/explain         │
                                 │                                    │
                                 │  1. ExplanationService             │
                                 │     └─ Haiku generates explanation │
                                 │                                    │
                                 │  2. TTSService (if return_audio)   │
                                 │     └─ Eleven Labs → MP3 bytes     │
                                 │                                    │
                                 │  Response: {                       │
                                 │    explanation: str,               │
                                 │    audio_base64: str,              │
                                 │    duration_ms: int                │
                                 │  }                                 │
                                 └──────────────────────────────────┘
                                                  │
                                                  ▼
                                 ┌──────────────────────────────────┐
                                 │  FRONTEND: playAudioResponse()    │
                                 │  ├─ Decode base64 → Blob          │
                                 │  ├─ Create Audio element          │
                                 │  └─ Play through speakers         │
                                 └──────────────────────────────────┘
                                                  │
                                                  ▼
┌─────────────────────────────────────────────────────────────────────┐
│                         USER HEARS                                   │
│  "I transcribed your message about the meeting tomorrow.            │
│   It's ready to paste. Anything else?"                              │
└─────────────────────────────────────────────────────────────────────┘
```

---

## File Changes Summary

### New Files
```
backend/app/services/explanation.py    # Haiku explanation generation
backend/app/services/tts.py            # Eleven Labs TTS
desktop/src/lib/audio.ts               # Audio playback utilities
```

### Modified Files
```
backend/app/api/routes.py              # Add /api/v1/explain endpoint
backend/app/core/config.py             # Add Eleven Labs config
backend/requirements.txt               # Add elevenlabs package
backend/.env.example                   # Add ELEVENLABS_API_KEY

desktop/src/lib/api.ts                 # Add explainContent function
desktop/src/components/Settings.tsx    # Add voice response toggle
desktop/src/App.tsx                    # Integrate voice response flow
```

---

## Error Handling Strategy

| Scenario | Handling |
|----------|----------|
| Eleven Labs API key missing | Skip TTS, return explanation text only |
| Eleven Labs rate limit | Return explanation text only, log warning |
| Eleven Labs timeout | Return explanation text only after 5s timeout |
| Haiku explanation fails | Skip voice response entirely, continue normal flow |
| Audio playback fails | Log error, continue (user still has text) |
| User interrupts (new recording) | Stop current audio playback |

**Key principle:** Voice response is an ENHANCEMENT. If it fails, the core transcription flow must still work.

---

## Latency Considerations

| Step | Expected Latency |
|------|-----------------|
| Haiku explanation | ~500-800ms |
| Eleven Labs TTS | ~300-500ms (turbo model) |
| Audio decode + play start | ~50ms |
| **Total additional latency** | **~1-1.5 seconds** |

**Optimization options:**
1. Start TTS call while text is being pasted (parallel)
2. Use Eleven Labs streaming API (start playing before full audio received)
3. Cache common explanations (probably not worth complexity)

---

## Testing Plan

### Unit Tests
- [ ] ExplanationService generates valid explanations
- [ ] TTSService handles API errors gracefully
- [ ] Audio playback works with various MP3 sizes

### Integration Tests
- [ ] Full flow: transcribe → explain → TTS → playback
- [ ] Voice response disabled doesn't call explain endpoint
- [ ] Errors in voice response don't break main flow

### Manual Tests
- [ ] Voice sounds natural, not robotic
- [ ] Explanation makes sense for different content types
- [ ] Settings persist across sessions
- [ ] Audio can be interrupted by new recording

---

## Open Questions (Need Your Input)

1. **Should voice response be ON by default for new users?**
   - Pro: Showcases the feature
   - Con: Might be unexpected/jarring

2. **Should we offer multiple Eleven Labs voices?**
   - Could be a premium feature
   - Adds complexity

3. **For code explanations specifically, should we detect the language?**
   - "I added a React component" vs "I added a function"
   - More context-aware

4. **Should explanation play BEFORE or AFTER paste?**
   - Before: User knows what's coming
   - After: Text appears faster

5. **Do you have an Eleven Labs API key to use?**
   - Need to add to backend/.env
   - Different tiers have different rate limits

---

## Implementation Order

1. **Phase 1: Backend foundation**
   - Create ExplanationService
   - Create TTSService
   - Add /api/v1/explain endpoint
   - Test with curl/Postman

2. **Phase 2: Frontend integration**
   - Add audio playback utility
   - Add API function
   - Add settings toggle
   - Integrate into App.tsx

3. **Phase 3: Polish**
   - Tune Haiku prompt for natural speech
   - Adjust timing/flow
   - Error handling edge cases
   - User testing

---

## Risks & Mitigations

| Risk | Mitigation |
|------|------------|
| Eleven Labs cost per request | Track usage, add limits, make feature toggleable |
| Latency feels slow | Parallel execution, streaming, optimize prompts |
| Explanations sound robotic | Tune Haiku prompt for conversational output |
| Feature creep | Stick to MVP, iterate based on feedback |
| Breaking existing flow | Voice is additive only, all existing paths unchanged |

---

**Ready for your review. What questions do you have before we start implementation?**
