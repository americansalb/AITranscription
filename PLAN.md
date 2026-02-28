# Vaak Lite — Simplified Transcription App

## Overview

A stripped-down adaptation of Vaak that focuses purely on transcription with language/mode selection. Web-first (React SPA + FastAPI backend) with an optional Tauri desktop wrapper for native audio.

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
│   ├── src/
│   │   ├── main.tsx             # React entry point
│   │   ├── App.tsx              # Main app component (~300 lines)
│   │   ├── components/
│   │   │   ├── ModeSelector.tsx      # Mode picker: unidirectional, conversational, etc.
│   │   │   ├── LanguageSelector.tsx  # Source/target language dropdowns
│   │   │   ├── TranscriptPanel.tsx   # Live transcript display
│   │   │   ├── RecordButton.tsx      # Record/stop with visual indicator
│   │   │   └── AudioVisualizer.tsx   # Simple waveform during recording
│   │   ├── hooks/
│   │   │   └── useAudioRecorder.ts   # Browser MediaRecorder hook
│   │   ├── lib/
│   │   │   ├── api.ts               # API client (transcribe endpoint)
│   │   │   └── languages.ts         # Language list (ISO codes + display names)
│   │   └── styles.css               # Single CSS file, clean minimal UI
│   └── src-tauri/                    # Optional Tauri wrapper (Phase 2)
│       ├── Cargo.toml
│       ├── tauri.conf.json
│       └── src/
│           ├── main.rs
│           └── audio.rs             # Native CPAL audio recording
```

---

## Transcription Modes

### 1. Unidirectional (default)
- **One speaker, one language** → text
- Simple push-to-talk or toggle recording
- Single transcript panel
- Best for: dictation, note-taking, lectures

### 2. Conversational
- **Two speakers alternating**, possibly different languages
- Single audio stream, but segments are labeled Speaker A / Speaker B
- Uses Whisper's `verbose_json` response format with segments + timestamps
- We detect speaker turns by silence gaps (>1.5 seconds) between segments
- Two-column or interleaved transcript display
- Best for: interviews, meetings, phone calls

### 3. Consecutive
- **Record → Stop → Review** cycle
- Speaker talks, user presses stop, transcript appears
- Then the next segment begins
- Each segment is a discrete block in the transcript
- Pairs naturally with interpretation (future: translate each block)
- Best for: diplomatic interpretation, legal depositions

### 4. Simultaneous
- **Continuous real-time transcription**
- Audio is chunked into overlapping 5-second windows, sent in parallel
- Results stream in and merge into a rolling transcript
- Uses a sliding window approach: record 5s, send, continue recording
- Overlap of 0.5s prevents word-boundary cuts
- Best for: live events, real-time captioning

---

## Implementation Steps

### Step 1: Backend — Minimal FastAPI Server
Create `vaak-lite/backend/` with:

- **`config.py`**: Load `GROQ_API_KEY` from env, set `WHISPER_MODEL`, `PORT`
- **`transcription.py`**: TranscriptionService class (reuse from existing, ~60 lines)
  - `transcribe(audio_data, filename, language)` → `{text, duration, language, segments}`
- **`main.py`**: FastAPI app with:
  - `GET /health` — health check
  - `POST /transcribe` — accept audio file + language, return transcription
  - `POST /transcribe-stream` — for simultaneous mode: accept chunked audio, return partial results via SSE
  - CORS configured for localhost dev + any origin for production

### Step 2: Frontend — React SPA Core
Create `vaak-lite/frontend/` with Vite + React + TypeScript:

- **`App.tsx`**: Main layout
  - Top bar: Mode selector (4 modes as tabs/pills) + Language selector
  - Center: Transcript panel (scrollable, auto-scroll to bottom)
  - Bottom: Record button + audio visualizer
  - Mode-specific behavior controlled by state

- **`ModeSelector.tsx`**: Four mode options with icons and descriptions
  - Unidirectional, Conversational, Consecutive, Simultaneous
  - Selected mode changes recording behavior and transcript layout

- **`LanguageSelector.tsx`**: Dropdown with ~30 popular languages
  - Whisper supported languages (en, es, fr, de, zh, ja, ko, ar, hi, pt, ru, etc.)
  - "Auto-detect" as default option
  - In conversational mode: show two language selectors (Speaker A / Speaker B)

- **`RecordButton.tsx`**: Large, centered button
  - Unidirectional: toggle on/off
  - Conversational: toggle on/off (continuous)
  - Consecutive: press to start segment, press to stop segment
  - Simultaneous: toggle on/off (auto-chunking behind the scenes)

- **`TranscriptPanel.tsx`**: Display transcribed text
  - Unidirectional: single flowing text
  - Conversational: labeled segments (Speaker A / Speaker B) with timestamps
  - Consecutive: discrete numbered blocks with timestamps
  - Simultaneous: rolling text with live partial results (grayed) and confirmed results

- **`AudioVisualizer.tsx`**: Simple canvas-based waveform using Web Audio API's AnalyserNode

- **`useAudioRecorder.ts`**: Hook wrapping MediaRecorder
  - Start/stop recording
  - Return audio blob on stop
  - For simultaneous mode: emit chunks every 5 seconds while recording continues
  - Audio format: WebM/Opus (browser default) or WAV

- **`api.ts`**: Thin API client
  - `transcribe(blob, language?)` → `{text, duration, language, segments}`
  - `transcribeStream(blob, language?)` → EventSource for SSE
  - Base URL from env var `VITE_API_URL`

- **`languages.ts`**: Static list of Whisper-supported languages
  ```ts
  export const LANGUAGES = [
    { code: "auto", name: "Auto-detect" },
    { code: "en", name: "English" },
    { code: "es", name: "Spanish" },
    { code: "fr", name: "French" },
    // ... ~30 languages
  ];
  ```

- **`styles.css`**: Clean, minimal, dark-theme CSS
  - Mobile-responsive
  - Large touch targets for the record button
  - Clear visual hierarchy

### Step 3: Simultaneous Mode — Chunked Streaming
This is the most complex mode. Implementation details:

- **Frontend**: `useAudioRecorder` emits 5-second chunks via `ondataavailable`
  - Set `MediaRecorder` timeslice to 5000ms
  - Each chunk is sent immediately to `/transcribe`
  - Results are appended to transcript in order (using sequence numbers)
  - Overlap: the last 0.5s of each chunk is included in the next chunk's audio context

- **Backend**: No special streaming endpoint needed initially
  - Each chunk is a regular `/transcribe` call
  - Frontend manages ordering and deduplication
  - Future optimization: WebSocket for lower latency

### Step 4: Conversational Mode — Speaker Detection
- Use Whisper's segment timestamps from `verbose_json`
- Detect speaker turns by silence gaps between segments
- Label alternating segments as Speaker A / Speaker B
- Simple heuristic: gap > 1.5s = new speaker
- Display with alternating colors/alignment (like a chat)

### Step 5: Tauri Desktop Wrapper (Phase 2)
- Add `src-tauri/` to the frontend directory
- Reuse audio.rs from existing Vaak for native CPAL recording
- Unified recorder hook: prefer native when in Tauri, fall back to browser
- Tauri config: single window, no system tray, minimal permissions
- Build targets: macOS (.dmg), Windows (.msi), Linux (.AppImage)

---

## UI Mockup (Text)

```
┌──────────────────────────────────────────────────────┐
│  Vaak Lite                                           │
│                                                      │
│  ┌──────────┬──────────────┬─────────────┬─────────┐│
│  │⚡Unidir. │💬 Conversa.  │📋 Consec.  │🔴 Simul.││
│  └──────────┴──────────────┴─────────────┴─────────┘│
│                                                      │
│  Language: [ Auto-detect        ▾ ]                  │
│                                                      │
│  ┌──────────────────────────────────────────────────┐│
│  │                                                  ││
│  │  "Hello, welcome to the meeting. Today we're     ││
│  │   going to discuss the quarterly results and     ││
│  │   the roadmap for next quarter."                 ││
│  │                                                  ││
│  │  [00:15] "The revenue numbers look strong        ││
│  │   across all regions..."                         ││
│  │                                                  ││
│  │                                                  ││
│  └──────────────────────────────────────────────────┘│
│                                                      │
│              ┌─────────────────┐                     │
│              │   ● Recording   │                     │
│              │   ▁▃▅▇▅▃▁▃▅▇   │                     │
│              └─────────────────┘                     │
│                                                      │
│  Duration: 00:32  │  Words: 47  │  Language: en      │
└──────────────────────────────────────────────────────┘
```

---

## File Count & Complexity

| Component | Files | Est. Lines |
|-----------|-------|-----------|
| Backend | 4 | ~200 |
| Frontend components | 5 | ~600 |
| Frontend hooks/lib | 3 | ~250 |
| Config/build | 5 | ~100 |
| Styles | 1 | ~300 |
| **Total** | **18** | **~1,450** |

Compare to existing Vaak: ~100+ files, ~46,000 lines. This is a **97% reduction**.

---

## API Surface

| Endpoint | Method | Purpose |
|----------|--------|---------|
| `/health` | GET | Health check |
| `/transcribe` | POST | Transcribe audio file → text |

That's it. Two endpoints vs. the existing app's 30+.

---

## Dependencies

### Backend
```
fastapi>=0.109.0
uvicorn[standard]>=0.27.0
groq>=0.4.0
python-multipart>=0.0.9
```

4 packages vs. existing 15+.

### Frontend
```
react, react-dom
vite, typescript
@types/react
```

3 core packages vs. existing 20+.
