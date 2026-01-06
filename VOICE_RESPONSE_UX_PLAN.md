# Voice Response Feature - UX-First Design

## Starting Over: What Problem Are We Actually Solving?

The problem isn't "add voice output." The problem is:

**"I want to stay in flow state while using AI. Reading responses breaks my flow."**

So the question becomes: **When does voice ADD value vs. when does it interrupt?**

---

## Thinking Through User Scenarios

### Scenario 1: Simple Transcription
```
User: *dictates* "Hey John, let's meet tomorrow at 3pm"
Current: Text appears, gets pasted
With voice: "I transcribed your message about meeting John"
```
**Is voice useful here?** Not really. User knows what they said. Voice is just noise.

### Scenario 2: Error Occurred
```
User: *dictates something*
Current: Error message appears (must READ it)
With voice: "I couldn't hear that clearly. Try speaking closer to the mic."
```
**Is voice useful here?** YES. User doesn't have to look away to understand the problem.

### Scenario 3: Code/Complex Operation
```
User: *asks AI to add authentication*
Current: Code appears (must READ to understand what changed)
With voice: "Added JWT authentication. Login route is public,
            everything under /api is protected. Want me to add refresh tokens?"
```
**Is voice useful here?** VERY YES. This is the unlock.

### Scenario 4: Ambiguity
```
User: *dictates* "Send the report to the team"
Current: Just transcribes literally
With voice: "Should I format this as an email or a Slack message?"
```
**Is voice useful here?** YES. Enables true conversation.

---

## The Insight: Voice Should Be Contextual, Not Constant

**Bad UX:** Voice response for everything (annoying, slow, unnecessary)

**Good UX:** Voice responds when it ADDS information the user doesn't already have

| Situation | Voice Response | Why |
|-----------|---------------|-----|
| Simple transcription | None or brief "Done" | User knows what they said |
| Error | Explain the error | User needs to know what went wrong |
| Complex action | Explain what happened | User can't see/understand the changes |
| Ambiguity | Ask clarifying question | Enables conversation |
| Code changes | Summarize the diff | User needs to understand impact |

---

## Proposed UX Model: "Smart Voice"

### Default Behavior (Out of Box)
- **Recording starts:** Subtle beep (current behavior)
- **Recording stops:** Subtle beep (current behavior)
- **Success:** Brief audio cue OR optional "Done" voice
- **Error:** Voice explains what went wrong
- **Complex action:** Voice summarizes what happened

### User Control (Settings)

```
┌─────────────────────────────────────────────────────────┐
│ Voice Feedback                                          │
│                                                         │
│ ○ Off (audio cues only)                                │
│ ● Smart (voice when helpful)              ← DEFAULT    │
│ ○ Always (voice confirms everything)                   │
│                                                         │
│ ┌─────────────────────────────────────────────────────┐│
│ │ Smart mode speaks when:                             ││
│ │ ☑ Something goes wrong                              ││
│ │ ☑ Code or complex content is generated              ││
│ │ ☑ Clarification is needed                           ││
│ │ ☐ Transcription completes (brief confirmation)      ││
│ └─────────────────────────────────────────────────────┘│
│                                                         │
│ Voice                                                   │
│ [Rachel ▾]  [▶ Preview]                                │
│                                                         │
│ Speaking speed                                          │
│ [━━━━━●━━━] Normal                                     │
│                                                         │
└─────────────────────────────────────────────────────────┘
```

### Why "Smart" as Default?
1. **New users** experience the magic immediately (errors are explained)
2. **Not annoying** because it doesn't speak on every action
3. **Discoverable** - users learn the feature exists naturally
4. **Easy to turn off** if unwanted

---

## Interaction Flow

### Flow 1: Simple Transcription (Smart Mode)
```
User speaks → Text transcribed → Paste happens → [Silence]

No voice needed. User knows what they said. Don't interrupt.
```

### Flow 2: Error Occurs (Smart Mode)
```
User speaks → Error detected → [Voice: "I couldn't understand
that. There was too much background noise. Try again in a
quieter spot."]

User doesn't have to read anything. They hear the problem
and can react while staying in flow.
```

### Flow 3: Code Generated (Smart Mode)
```
User: "Add a login form with validation"

→ Code written to editor
→ [Voice: "Created a login form with email and password fields.
   Added validation for email format and minimum password length.
   Invalid fields show red borders. Want me to add a forgot
   password link?"]

User understands what happened without reading the code.
Can respond verbally to continue the conversation.
```

### Flow 4: User Says "Explain That"
```
User just received a code change they don't understand.

User: "Explain that"

→ [Voice: "I added a useEffect hook. This runs code when the
   component first loads. It's fetching user data from the API
   and storing it in state. The empty brackets at the end mean
   it only runs once."]

On-demand explanation for when user needs more detail.
```

---

## Discoverability & Onboarding

### First-Time Experience
After first successful transcription:
```
┌─────────────────────────────────────────────────────────┐
│                                                         │
│  ✓ Transcription successful!                           │
│                                                         │
│  💡 Tip: Scribe can speak responses to you so you      │
│     don't have to read. Enable voice feedback in       │
│     settings, or say "explain that" anytime.           │
│                                                         │
│  [Got it]  [Enable Voice]                              │
│                                                         │
└─────────────────────────────────────────────────────────┘
```

**Not a blocking modal.** A subtle, dismissible tooltip that introduces the feature.

### Voice Commands (Discoverable Through Use)
- "Explain that" - Get voice explanation of last action
- "Read it back" - Hear the transcription
- "What did you do?" - Summarize recent actions
- "Stop" - Interrupt current voice playback

These are OPTIONAL. App works fully without them. But power users discover them.

---

## Audio Behavior

### Interruption Handling
```
Voice is playing...
User starts new recording → Voice stops immediately
User says "stop" → Voice stops
User presses Escape → Voice stops

Never fight for attention. User action always wins.
```

### Volume & Output
- Uses system audio output (headphones if connected)
- Respects system volume
- No separate volume control (unnecessary complexity)

### Latency Budget
```
Target: Voice starts within 1.5 seconds of action

Breakdown:
- Haiku explanation: 500-800ms
- TTS generation: 300-500ms
- Audio start: 50ms
- Buffer: 150ms
```

If TTS takes longer, show subtle "preparing response..." indicator.

---

## Groq TTS vs Eleven Labs

### Option 1: Groq "Playht TTS" / "Canopy Labs Orpheus"
**Pros:**
- Already have Groq API key
- Single vendor (simpler)
- Potentially lower latency (same infra as Whisper)
- Likely cheaper

**Cons:**
- Less proven for TTS quality
- Fewer voice options
- Less documentation/examples

### Option 2: Eleven Labs
**Pros:**
- Industry-leading voice quality
- Many voice options
- Well-documented API
- Streaming support

**Cons:**
- Second API key to manage
- Additional cost
- Another vendor relationship

### Recommendation: Start with Groq, Option to Upgrade

1. Implement with Groq TTS first (simpler, already integrated)
2. Build abstraction layer so TTS provider is swappable
3. Add Eleven Labs as "Premium Voice" option later if quality isn't sufficient

```python
# Abstraction
class TTSProvider(Protocol):
    async def synthesize(self, text: str) -> bytes: ...

class GroqTTS(TTSProvider): ...
class ElevenLabsTTS(TTSProvider): ...

# Config determines which one is used
tts = get_tts_provider(config.tts_provider)  # "groq" or "elevenlabs"
```

---

## Settings Persistence

### Stored in localStorage (Frontend)
```typescript
interface VoiceSettings {
  mode: 'off' | 'smart' | 'always';
  smartTriggers: {
    errors: boolean;
    codeChanges: boolean;
    clarifications: boolean;
    confirmations: boolean;
  };
  voice: string;        // Voice ID
  speed: number;        // 0.5 - 2.0, default 1.0
}

// Defaults
const DEFAULT_VOICE_SETTINGS: VoiceSettings = {
  mode: 'smart',
  smartTriggers: {
    errors: true,
    codeChanges: true,
    clarifications: true,
    confirmations: false,  // Off by default - would be too chatty
  },
  voice: 'default',
  speed: 1.0,
};
```

---

## Implementation Phases

### Phase 1: Foundation (Get It Working)
1. Research Groq TTS API (model name, parameters, response format)
2. Create TTSService with Groq backend
3. Create ExplanationService (Haiku generates spoken text)
4. Add `/api/v1/explain` endpoint
5. Frontend: Basic audio playback
6. Frontend: Simple on/off toggle in settings

**Deliverable:** Voice works. User can enable/disable it.

### Phase 2: Smart Mode (Make It Good)
1. Implement "smart" detection logic (when to speak)
2. Add the smart mode settings UI
3. Add voice preview in settings
4. Implement interruption handling (stop on new recording)
5. Add "explain that" voice command

**Deliverable:** Voice is contextually useful, not annoying.

### Phase 3: Polish (Make It Great)
1. Fine-tune Haiku prompts for natural speech
2. Add speaking speed control
3. Optimize latency (parallel requests, caching)
4. Add Eleven Labs as premium option (if Groq quality insufficient)
5. Add more voice commands
6. First-time user onboarding tooltip

**Deliverable:** Feature feels polished and professional.

---

## Open UX Questions

1. **What's the voice's "personality"?**
   - Professional and neutral?
   - Friendly and casual?
   - Should it have a name? ("Scribe" speaking?)

2. **Should voice have a visual indicator while speaking?**
   - Subtle animation on the floating indicator?
   - Nothing (pure audio)?

3. **How verbose should explanations be?**
   - User skill level affects this
   - But also: some users want detailed, some want brief
   - Add a "verbosity" setting? Or is that too much?

4. **Localization?**
   - English only for v1?
   - TTS in other languages later?

---

## Success Metrics (How Do We Know It's Good?)

1. **Adoption:** % of users who keep voice on after trying it
2. **Retention:** Do voice users use the app more?
3. **Interruption rate:** How often do users interrupt/skip voice?
   - High skip rate = voice is too long or unnecessary
4. **Error recovery:** Do users recover from errors faster with voice?
5. **Qualitative:** Does it feel like a conversation or an announcement?

---

## Next Steps

1. **Research Groq TTS API** - Find exact model names, parameters, pricing
2. **Create minimal prototype** - Just "voice says 'Done'" on success
3. **Test latency** - Is Groq TTS fast enough?
4. **Iterate on UX** - Based on how it feels in practice

---

## Decision Points Needed

Before implementing, please confirm:

1. **Start with Groq TTS?** (vs. Eleven Labs first)
2. **"Smart" as default mode?** (vs. off by default)
3. **Phase 1 scope OK?** (Basic toggle, not full smart mode)
4. **Voice personality preference?** (Professional/neutral vs. friendly/casual)

---

*This plan prioritizes UX decisions over implementation details.
The best feature is one users actually want to use.*
