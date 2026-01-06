# Claude Code Voice Integration - Implementation Plan

## Overview

Integrate Scribe with Claude Code so users **hear spoken explanations** of code changes in real-time.

```
Claude Code writes code → Hook fires → Scribe explains → User hears
```

---

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                      CLAUDE CODE                                 │
│                                                                  │
│  User: "Add authentication to the API"                          │
│  Claude: *writes code*                                          │
│                         ↓                                        │
│              PostToolUse Hook Fires                              │
└─────────────────────────────────────────────────────────────────┘
                          │
                          │ HTTP POST (JSON)
                          ▼
┌─────────────────────────────────────────────────────────────────┐
│                    SCRIBE BACKEND                                │
│                                                                  │
│  POST /api/v1/claude-event                                      │
│    │                                                            │
│    ├─→ ExplanationService (Haiku)                               │
│    │     "Explain this code change in spoken language"          │
│    │     Output: "I added a login function that validates..."   │
│    │                                                            │
│    └─→ TTSService (Groq Orpheus)                                │
│          Input: explanation text                                 │
│          Output: audio bytes (MP3)                              │
│                         │                                        │
└─────────────────────────────────────────────────────────────────┘
                          │
                          │ Server-Sent Events (SSE)
                          ▼
┌─────────────────────────────────────────────────────────────────┐
│                 SCRIBE DESKTOP APP                               │
│                                                                  │
│  SSE Client listening on /api/v1/voice-stream                   │
│    │                                                            │
│    └─→ Audio Player                                             │
│          Plays MP3 through speakers                             │
│                         │                                        │
└─────────────────────────────────────────────────────────────────┘
                          │
                          ▼
┌─────────────────────────────────────────────────────────────────┐
│                      USER HEARS                                  │
│                                                                  │
│  "I added a login function that checks email and password.      │
│   Invalid credentials return a 401 error. Want me to add        │
│   rate limiting?"                                                │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

---

## Components To Build

### 1. Claude Code Hook Script

**File:** `~/.claude/hooks/scribe-voice-notify.sh`

```bash
#!/bin/bash
# Claude Code PostToolUse hook that sends code changes to Scribe

# Read JSON payload from stdin
INPUT=$(cat)

# Extract fields
TOOL_NAME=$(echo "$INPUT" | jq -r '.tool_name // empty')
HOOK_EVENT=$(echo "$INPUT" | jq -r '.hook_event_name // empty')

# Only process Write/Edit operations from PostToolUse
if [[ "$HOOK_EVENT" == "PostToolUse" && ("$TOOL_NAME" == "Write" || "$TOOL_NAME" == "Edit") ]]; then
  # Send to Scribe backend (non-blocking)
  curl -s -X POST "http://localhost:8000/api/v1/claude-event" \
    -H "Content-Type: application/json" \
    -d "$INPUT" \
    --max-time 5 \
    > /dev/null 2>&1 &
fi

# Always exit 0 to not block Claude Code
exit 0
```

**Make executable:**
```bash
chmod +x ~/.claude/hooks/scribe-voice-notify.sh
```

---

### 2. Claude Code Settings Configuration

**File:** `~/.claude/settings.json` (merge with existing)

```json
{
  "hooks": {
    "PostToolUse": [
      {
        "matcher": "Write|Edit",
        "hooks": [
          {
            "type": "command",
            "command": "~/.claude/hooks/scribe-voice-notify.sh",
            "timeout": 10
          }
        ]
      }
    ]
  }
}
```

---

### 3. Backend: ExplanationService

**File:** `backend/app/services/explanation.py`

```python
"""
Service for generating spoken explanations of code changes using Claude Haiku.
"""
from anthropic import AsyncAnthropic
from app.core.config import settings


class ExplanationService:
    """Generates natural language explanations of code changes."""

    def __init__(self):
        if not settings.anthropic_api_key:
            raise ValueError("ANTHROPIC_API_KEY not configured")
        self.client = AsyncAnthropic(api_key=settings.anthropic_api_key)
        self.model = settings.haiku_model  # claude-3-5-haiku-20241022

    async def explain_code_change(
        self,
        file_path: str,
        content: str,
        operation: str = "Write",
        old_content: str | None = None,
        max_words: int = 40,
    ) -> str:
        """
        Generate a spoken explanation of a code change.

        Args:
            file_path: Path to the file that was changed
            content: The new content (or snippet for large files)
            operation: "Write" for new file, "Edit" for modification
            old_content: Previous content (for Edit operations)
            max_words: Target word count (Groq TTS has 200 char limit)

        Returns:
            Natural language explanation suitable for TTS
        """
        # Truncate content to avoid token limits
        content_preview = content[:2000] if len(content) > 2000 else content

        # Determine file type for context
        file_ext = file_path.split('.')[-1] if '.' in file_path else 'txt'

        system_prompt = f"""You are explaining code changes to a developer via voice.

CRITICAL RULES:
1. Keep response under {max_words} words (will be spoken aloud)
2. Be conversational, not technical documentation
3. Lead with WHAT you did, then briefly WHY
4. NO markdown, NO code snippets, NO bullet points
5. Use "I" to describe actions ("I added...", "I created...")
6. End with a brief follow-up question if appropriate
7. NO special characters that don't speak well

File type: {file_ext}
Operation: {"Created new file" if operation == "Write" else "Modified existing file"}"""

        user_prompt = f"""Explain this code change in spoken language:

File: {file_path}
{"New content:" if operation == "Write" else "Updated content:"}
```
{content_preview}
```

Remember: Under {max_words} words, conversational, no code in response."""

        try:
            response = await self.client.messages.create(
                model=self.model,
                max_tokens=150,  # ~40 words
                messages=[{"role": "user", "content": user_prompt}],
                system=system_prompt,
            )

            explanation = response.content[0].text.strip()
            return explanation

        except Exception as e:
            # Fallback to simple explanation
            return f"I updated {file_path.split('/')[-1]}."


# Singleton instance
explanation_service = ExplanationService()
```

---

### 4. Backend: TTSService (Groq Orpheus)

**File:** `backend/app/services/tts.py`

```python
"""
Text-to-Speech service using Groq's Orpheus model.
"""
import base64
from groq import AsyncGroq
from app.core.config import settings


class TTSService:
    """Converts text to speech using Groq Orpheus TTS."""

    def __init__(self):
        if not settings.groq_api_key:
            raise ValueError("GROQ_API_KEY not configured")
        self.client = AsyncGroq(api_key=settings.groq_api_key)
        self.model = "playai-tts"  # or "playai-tts-arabic" for Arabic
        self.voice = "Fritz-PlayAI"  # Default voice

    async def synthesize(
        self,
        text: str,
        voice: str | None = None,
    ) -> bytes | None:
        """
        Convert text to speech.

        Args:
            text: Text to synthesize (max ~200 chars for Groq)
            voice: Voice ID to use

        Returns:
            MP3 audio bytes, or None if failed
        """
        if not text:
            return None

        # Groq TTS has character limits - truncate if needed
        if len(text) > 200:
            # Find a good break point
            text = text[:197] + "..."

        try:
            response = await self.client.audio.speech.create(
                model=self.model,
                voice=voice or self.voice,
                input=text,
                response_format="mp3",
            )

            # Get audio bytes
            audio_bytes = response.read()
            return audio_bytes

        except Exception as e:
            print(f"TTS error: {e}")
            return None

    def audio_to_base64(self, audio_bytes: bytes) -> str:
        """Convert audio bytes to base64 string for JSON transport."""
        return base64.b64encode(audio_bytes).decode('utf-8')


# Singleton instance
tts_service = TTSService()
```

---

### 5. Backend: SSE Event Manager

**File:** `backend/app/services/events.py`

```python
"""
Server-Sent Events manager for real-time voice notifications.
"""
import asyncio
import json
from typing import AsyncGenerator
from dataclasses import dataclass, asdict
from datetime import datetime


@dataclass
class VoiceEvent:
    """Voice event to send to clients."""
    type: str  # "voice" | "status" | "error"
    audio_base64: str | None = None
    explanation: str | None = None
    file_path: str | None = None
    timestamp: str | None = None


class EventManager:
    """Manages SSE connections and broadcasts events."""

    def __init__(self):
        self._clients: list[asyncio.Queue] = []

    async def subscribe(self) -> AsyncGenerator[str, None]:
        """Subscribe to voice events. Returns an async generator of SSE data."""
        queue: asyncio.Queue = asyncio.Queue()
        self._clients.append(queue)

        try:
            # Send initial connection event
            yield self._format_sse(VoiceEvent(
                type="connected",
                timestamp=datetime.utcnow().isoformat()
            ))

            while True:
                event = await queue.get()
                yield self._format_sse(event)

        finally:
            self._clients.remove(queue)

    async def broadcast(self, event: VoiceEvent):
        """Send event to all connected clients."""
        for queue in self._clients:
            await queue.put(event)

    def _format_sse(self, event: VoiceEvent) -> str:
        """Format event as SSE data."""
        data = json.dumps(asdict(event))
        return f"data: {data}\n\n"

    @property
    def client_count(self) -> int:
        """Number of connected clients."""
        return len(self._clients)


# Singleton instance
event_manager = EventManager()
```

---

### 6. Backend: API Routes

**File:** `backend/app/api/routes.py` (add to existing)

```python
# Add these imports at top
from fastapi import BackgroundTasks
from fastapi.responses import StreamingResponse
from app.services.explanation import explanation_service
from app.services.tts import tts_service
from app.services.events import event_manager, VoiceEvent
from datetime import datetime


# Add these endpoints

@router.post("/claude-event")
async def handle_claude_event(
    event: dict,
    background_tasks: BackgroundTasks,
):
    """
    Receive events from Claude Code hooks.
    Generates explanation and TTS, broadcasts to connected clients.
    """
    tool_name = event.get("tool_name")
    tool_input = event.get("tool_input", {})
    hook_event = event.get("hook_event_name")

    # Only process code changes
    if tool_name not in ["Write", "Edit"]:
        return {"status": "ignored", "reason": "not a code change"}

    file_path = tool_input.get("file_path", "unknown")
    content = tool_input.get("content", "")

    # Process in background to not block Claude Code
    background_tasks.add_task(
        process_code_change,
        file_path=file_path,
        content=content,
        operation=tool_name,
    )

    return {"status": "processing"}


async def process_code_change(
    file_path: str,
    content: str,
    operation: str,
):
    """Background task to generate explanation and TTS."""
    try:
        # Generate explanation
        explanation = await explanation_service.explain_code_change(
            file_path=file_path,
            content=content,
            operation=operation,
        )

        # Generate TTS
        audio_bytes = await tts_service.synthesize(explanation)
        audio_base64 = tts_service.audio_to_base64(audio_bytes) if audio_bytes else None

        # Broadcast to connected clients
        await event_manager.broadcast(VoiceEvent(
            type="voice",
            audio_base64=audio_base64,
            explanation=explanation,
            file_path=file_path,
            timestamp=datetime.utcnow().isoformat(),
        ))

    except Exception as e:
        print(f"Error processing code change: {e}")
        await event_manager.broadcast(VoiceEvent(
            type="error",
            explanation=f"Failed to explain: {str(e)}",
            timestamp=datetime.utcnow().isoformat(),
        ))


@router.get("/voice-stream")
async def voice_stream():
    """
    SSE endpoint for real-time voice notifications.
    Desktop app connects here to receive audio events.
    """
    return StreamingResponse(
        event_manager.subscribe(),
        media_type="text/event-stream",
        headers={
            "Cache-Control": "no-cache",
            "Connection": "keep-alive",
            "X-Accel-Buffering": "no",  # Disable nginx buffering
        },
    )


@router.get("/voice-stream/status")
async def voice_stream_status():
    """Check SSE connection status."""
    return {
        "connected_clients": event_manager.client_count,
        "status": "active",
    }
```

---

### 7. Frontend: SSE Client & Audio Player

**File:** `desktop/src/lib/voiceStream.ts`

```typescript
/**
 * Voice stream client - connects to Scribe backend SSE endpoint
 * and plays audio explanations of Claude Code actions.
 */

interface VoiceEvent {
  type: 'voice' | 'status' | 'error' | 'connected';
  audio_base64?: string;
  explanation?: string;
  file_path?: string;
  timestamp?: string;
}

type VoiceEventHandler = (event: VoiceEvent) => void;

class VoiceStreamClient {
  private eventSource: EventSource | null = null;
  private handlers: VoiceEventHandler[] = [];
  private reconnectAttempts = 0;
  private maxReconnectAttempts = 5;
  private reconnectDelay = 1000;
  private currentAudio: HTMLAudioElement | null = null;
  private enabled = false;

  /**
   * Start listening to voice events from the backend.
   */
  connect(apiUrl: string): void {
    if (this.eventSource) {
      this.disconnect();
    }

    this.enabled = true;
    const url = `${apiUrl}/api/v1/voice-stream`;

    try {
      this.eventSource = new EventSource(url);

      this.eventSource.onopen = () => {
        console.log('[VoiceStream] Connected');
        this.reconnectAttempts = 0;
      };

      this.eventSource.onmessage = (event) => {
        try {
          const data: VoiceEvent = JSON.parse(event.data);
          this.handleEvent(data);
        } catch (e) {
          console.error('[VoiceStream] Parse error:', e);
        }
      };

      this.eventSource.onerror = () => {
        console.error('[VoiceStream] Connection error');
        this.handleDisconnect();
      };

    } catch (e) {
      console.error('[VoiceStream] Failed to connect:', e);
      this.handleDisconnect();
    }
  }

  /**
   * Stop listening to voice events.
   */
  disconnect(): void {
    this.enabled = false;
    if (this.eventSource) {
      this.eventSource.close();
      this.eventSource = null;
    }
    this.stopAudio();
  }

  /**
   * Register a handler for voice events.
   */
  onEvent(handler: VoiceEventHandler): () => void {
    this.handlers.push(handler);
    return () => {
      this.handlers = this.handlers.filter(h => h !== handler);
    };
  }

  /**
   * Stop any currently playing audio.
   */
  stopAudio(): void {
    if (this.currentAudio) {
      this.currentAudio.pause();
      this.currentAudio = null;
    }
  }

  private handleEvent(event: VoiceEvent): void {
    // Notify handlers
    this.handlers.forEach(handler => handler(event));

    // Play audio if present
    if (event.type === 'voice' && event.audio_base64) {
      this.playAudio(event.audio_base64);
    }
  }

  private async playAudio(base64Audio: string): Promise<void> {
    // Stop any playing audio first
    this.stopAudio();

    try {
      // Decode base64 to blob
      const binaryString = atob(base64Audio);
      const bytes = new Uint8Array(binaryString.length);
      for (let i = 0; i < binaryString.length; i++) {
        bytes[i] = binaryString.charCodeAt(i);
      }
      const blob = new Blob([bytes], { type: 'audio/mpeg' });
      const url = URL.createObjectURL(blob);

      // Create and play audio
      this.currentAudio = new Audio(url);
      this.currentAudio.onended = () => {
        URL.revokeObjectURL(url);
        this.currentAudio = null;
      };
      this.currentAudio.onerror = (e) => {
        console.error('[VoiceStream] Audio playback error:', e);
        URL.revokeObjectURL(url);
        this.currentAudio = null;
      };

      await this.currentAudio.play();

    } catch (e) {
      console.error('[VoiceStream] Failed to play audio:', e);
    }
  }

  private handleDisconnect(): void {
    if (!this.enabled) return;

    if (this.reconnectAttempts < this.maxReconnectAttempts) {
      this.reconnectAttempts++;
      const delay = this.reconnectDelay * this.reconnectAttempts;
      console.log(`[VoiceStream] Reconnecting in ${delay}ms...`);
      setTimeout(() => {
        if (this.enabled && this.eventSource?.url) {
          this.connect(this.eventSource.url.replace('/api/v1/voice-stream', ''));
        }
      }, delay);
    } else {
      console.error('[VoiceStream] Max reconnect attempts reached');
    }
  }

  /**
   * Check if currently connected.
   */
  get isConnected(): boolean {
    return this.eventSource?.readyState === EventSource.OPEN;
  }
}

// Singleton instance
export const voiceStream = new VoiceStreamClient();
```

---

### 8. Frontend: Settings Integration

**File:** `desktop/src/components/Settings.tsx` (add to Preferences)

Add these state variables and handlers:

```typescript
// Add to Preferences component

// State
const [voiceEnabled, setVoiceEnabled] = useState(() => getStoredVoiceEnabled());

// Handler
const handleVoiceEnabledChange = (enabled: boolean) => {
  setVoiceEnabled(enabled);
  saveVoiceEnabled(enabled);
  onVoiceEnabledChange?.(enabled);
};

// JSX - Add after noise cancellation toggle
<label className="toggle-setting">
  <span>Voice explanations (Claude Code)</span>
  <input
    type="checkbox"
    checked={voiceEnabled}
    onChange={(e) => handleVoiceEnabledChange(e.target.checked)}
  />
  <span className="toggle-switch" />
</label>
<p className="setting-hint">
  Hear spoken explanations when Claude Code writes or edits files
</p>
```

Add storage functions:

```typescript
// Add to Settings.tsx

export function getStoredVoiceEnabled(): boolean {
  try {
    return localStorage.getItem("scribe_voice_enabled") === "true";
  } catch {
    return false;
  }
}

export function saveVoiceEnabled(enabled: boolean): void {
  try {
    localStorage.setItem("scribe_voice_enabled", enabled ? "true" : "false");
  } catch {
    // Ignore storage errors
  }
}
```

---

### 9. Frontend: App Integration

**File:** `desktop/src/App.tsx` (add voice stream connection)

```typescript
// Add import
import { voiceStream } from './lib/voiceStream';
import { getStoredVoiceEnabled } from './components/Settings';

// In App component, add useEffect for voice stream
useEffect(() => {
  const voiceEnabled = getStoredVoiceEnabled();

  if (voiceEnabled) {
    const apiUrl = import.meta.env.VITE_API_URL || 'http://localhost:8000';
    voiceStream.connect(apiUrl);

    // Optional: Log events for debugging
    const unsubscribe = voiceStream.onEvent((event) => {
      if (event.type === 'voice') {
        console.log('[Voice]', event.explanation);
      }
    });

    return () => {
      unsubscribe();
      voiceStream.disconnect();
    };
  }
}, []);

// Add handler for settings change
const handleVoiceEnabledChange = useCallback((enabled: boolean) => {
  const apiUrl = import.meta.env.VITE_API_URL || 'http://localhost:8000';
  if (enabled) {
    voiceStream.connect(apiUrl);
  } else {
    voiceStream.disconnect();
  }
}, []);
```

---

## File Changes Summary

### New Files

```
~/.claude/hooks/scribe-voice-notify.sh      # Claude Code hook script
backend/app/services/explanation.py          # Haiku explanation service
backend/app/services/tts.py                  # Groq TTS service
backend/app/services/events.py               # SSE event manager
desktop/src/lib/voiceStream.ts               # Frontend SSE client
```

### Modified Files

```
~/.claude/settings.json                      # Add PostToolUse hook config
backend/app/api/routes.py                    # Add /claude-event and /voice-stream
backend/app/core/config.py                   # (if needed) Add TTS config
desktop/src/components/Settings.tsx          # Add voice toggle
desktop/src/App.tsx                          # Connect voice stream
```

---

## Implementation Order

### Phase 1: Backend Foundation
1. Create `explanation.py` service
2. Create `tts.py` service
3. Create `events.py` SSE manager
4. Add routes to `routes.py`
5. Test with curl

### Phase 2: Claude Code Hook
1. Create hook script
2. Update `~/.claude/settings.json`
3. Test hook fires correctly

### Phase 3: Frontend Integration
1. Create `voiceStream.ts`
2. Add settings toggle
3. Connect in App.tsx
4. Test end-to-end

### Phase 4: Polish
1. Error handling
2. Reconnection logic
3. Audio interruption (stop on new recording)
4. UI feedback

---

## Testing Plan

### Unit Tests
```bash
# Test explanation service
curl -X POST http://localhost:8000/api/v1/claude-event \
  -H "Content-Type: application/json" \
  -d '{
    "hook_event_name": "PostToolUse",
    "tool_name": "Write",
    "tool_input": {
      "file_path": "src/auth.ts",
      "content": "export function login(email: string, password: string) {\n  // Validate credentials\n  return true;\n}"
    }
  }'

# Test SSE stream
curl -N http://localhost:8000/api/v1/voice-stream
```

### Integration Test
1. Start Scribe backend
2. Open Scribe desktop app with voice enabled
3. Use Claude Code to write a file
4. Verify audio plays with explanation

---

## Error Handling

| Scenario | Handling |
|----------|----------|
| Hook script fails | Exit 0 anyway, don't block Claude |
| Backend unreachable | Hook fails silently, Claude continues |
| Haiku API error | Fallback to simple "I updated X" |
| TTS API error | Skip audio, log error |
| SSE disconnect | Auto-reconnect with backoff |
| Audio playback fails | Log error, continue |

**Principle:** Never block Claude Code. Voice is enhancement, not requirement.

---

## Latency Budget

| Step | Target | Max |
|------|--------|-----|
| Hook to backend | 50ms | 100ms |
| Haiku explanation | 500ms | 1000ms |
| Groq TTS | 300ms | 500ms |
| Audio start | 50ms | 100ms |
| **Total** | **~900ms** | **~1700ms** |

User should hear explanation within 1-2 seconds of Claude Code finishing.

---

## Security Considerations

1. **Hook script** runs with user permissions
2. **Backend** only accepts local connections (localhost)
3. **No auth** on `/claude-event` (local only)
4. **Content truncation** prevents token abuse
5. **Rate limiting** could be added if needed

---

## Future Enhancements

1. **Smart filtering**: Don't explain trivial changes (imports, formatting)
2. **Batch explanations**: Combine multiple rapid changes
3. **Voice selection**: Let users pick TTS voice
4. **Verbosity control**: Brief vs detailed explanations
5. **History**: Show recent explanations in UI
6. **Keyboard shortcut**: Replay last explanation

---

**Ready to implement. Start with Phase 1?**
