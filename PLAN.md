# Vaak Lite — Live Interpretation & Translation

## What It Is

A live interpretation and translation tool. Audio goes in one language, translated text comes out in another. Deployed as a web service (API + PWA frontend), installable on iOS via Safari.

## Pipeline

```
Audio → Groq Whisper (transcription) → LLM (translation) → Translated text
```

## Translation Mode Settings

**Direction:**
- Unidirectional — one language in, another out (A→B)
- Bidirectional — two speakers, two languages, bridging both ways (A↔B)

**Timing:**
- Consecutive — waits for a pause before interpreting
- Simultaneous — interprets in real-time, never compromising quality for speed

**Trigger (consecutive only):**
- Auto — interprets after a natural pause of X seconds (adjustable 0.5–5s)
- Manual — waits for user to click "done"

**LLM Provider (selectable):**
- Claude (Anthropic)
- GPT (OpenAI)
- Groq (Llama)
- Gemini (Google)

## API Endpoints

| Endpoint | Method | Purpose |
|----------|--------|---------|
| `/health` | GET | Health check + available providers |
| `/providers` | GET | List configured LLM providers |
| `/transcribe` | POST | Whisper transcription only |
| `/translate` | POST | LLM translation only |
| `/interpret` | POST | Full pipeline: transcribe + translate |

## Deployment

Render blueprint at `vaak-lite/render.yaml` deploys both services. Set API keys in the Render dashboard and it's live.
