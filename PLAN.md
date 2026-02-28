# Vaak Lite — Simplified Transcription App

## Overview

A stripped-down adaptation of Vaak that focuses purely on transcription with language/mode selection. Delivered as:

1. **Web service** — Standalone FastAPI backend with Groq Whisper
2. **iOS PWA** — React SPA built as a Progressive Web App, installable on iPhone via Safari "Add to Home Screen" for a native-like experience. No Xcode or App Store needed.

**What we keep:** Groq Whisper transcription, language selection, audio recording
**What we drop:** Auth, gamification, learning, screen reader, audience voting, roles, collaboration, polish, TTS, queues, analytics

---

## Architecture

```
vaak-lite/
├── backend/
│   ├── main.py                  # FastAPI app (~80 lines)
│   ├── config.py                # Settings from env vars
│   ├── transcription.py         # Groq Whisper service
│   └── requirements.txt         # Minimal deps: fastapi, uvicorn, groq, python-multipart
│
├── frontend/
│   ├── index.html
│   ├── package.json
│   ├── vite.config.ts
│   ├── tsconfig.json
│   ├── public/
│   │   ├── manifest.json        # PWA manifest (iOS home screen icon, name, theme)
│   │   └── icon-192.png         # App icon
│   ├── src/
│   │   ├── main.tsx             # React entry point
│   │   ├── App.tsx              # Main app component
│   │   ├── components/
│   │   │   ├── ModeSelector.tsx      # Mode picker: unidirectional, conversational, etc.
│   │   │   ├── LanguageSelector.tsx  # Source language dropdown
│   │   │   ├── TranscriptPanel.tsx   # Live transcript display
│   │   │   ├── RecordButton.tsx      # Record/stop with visual indicator
│   │   │   └── AudioVisualizer.tsx   # Simple waveform during recording
│   │   ├── hooks/
│   │   │   └── useAudioRecorder.ts   # Browser MediaRecorder hook
│   │   ├── lib/
│   │   │   ├── api.ts               # API client (transcribe endpoint)
│   │   │   └── languages.ts         # Language list (ISO codes + display names)
│   │   └── styles.css               # Single CSS file, mobile-first responsive
```

---

## Transcription Modes

### 1. Unidirectional (default)
- **One speaker, one language** → text
- Toggle recording on/off
- Single transcript panel
- Best for: dictation, note-taking, lectures

### 2. Conversational
- **Two speakers alternating**, possibly different languages
- Single audio stream, segments labeled Speaker A / Speaker B
- Uses Whisper's `verbose_json` response format with segments + timestamps
- Speaker turns detected by silence gaps (>1.5 seconds)
- Chat-style interleaved transcript display
- Best for: interviews, meetings, phone calls

### 3. Consecutive
- **Record → Stop → Review** cycle
- Speaker talks, user presses stop, transcript appears
- Each segment is a discrete numbered block
- Best for: interpretation, legal depositions

### 4. Simultaneous
- **Continuous real-time transcription**
- Audio chunked into 5-second windows, sent in parallel
- Results stream in and merge into a rolling transcript
- Best for: live events, real-time captioning

---

## PWA / iOS Details

- `manifest.json` declares the app name, icons, theme color, and `display: standalone`
- Meta tags in `index.html` for iOS: `apple-mobile-web-app-capable`, status bar style, touch icon
- Mobile-first CSS with large touch targets (especially the record button)
- Viewport locked to prevent zoom on input focus
- Audio recording via MediaRecorder API (supported in Safari 14.5+)

---

## API Surface

| Endpoint | Method | Purpose |
|----------|--------|---------|
| `/health` | GET | Health check |
| `/transcribe` | POST | Transcribe audio file → text, with segments |

Two endpoints total.
