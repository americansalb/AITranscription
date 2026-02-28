"""Vaak Lite — Minimal transcription API."""

import logging

from fastapi import FastAPI, File, Form, HTTPException, UploadFile
from fastapi.middleware.cors import CORSMiddleware

import config
from transcription import transcription_service

logging.basicConfig(level=logging.INFO)
logger = logging.getLogger(__name__)

app = FastAPI(
    title="Vaak Lite",
    description="Minimal transcription API powered by Groq Whisper",
    version="0.1.0",
    docs_url="/docs" if config.DEBUG else None,
)

app.add_middleware(
    CORSMiddleware,
    allow_origins=["*"],
    allow_methods=["GET", "POST", "OPTIONS"],
    allow_headers=["Content-Type"],
)


@app.get("/health")
async def health():
    return {
        "status": "healthy",
        "groq_configured": bool(config.GROQ_API_KEY),
        "model": config.WHISPER_MODEL,
    }


ALLOWED_EXTENSIONS = {".wav", ".mp3", ".m4a", ".webm", ".ogg", ".flac", ".mp4"}


@app.post("/transcribe")
async def transcribe_audio(
    audio: UploadFile = File(..., description="Audio file to transcribe"),
    language: str | None = Form(default=None, description="Language code (e.g. 'en') or null for auto-detect"),
):
    """Transcribe an audio file. Returns text, duration, detected language, and segments."""
    filename = audio.filename or "audio.wav"
    ext = "." + filename.rsplit(".", 1)[-1].lower() if "." in filename else ""
    if ext and ext not in ALLOWED_EXTENSIONS:
        raise HTTPException(status_code=400, detail=f"Unsupported format: {ext}")

    audio_data = await audio.read()
    if len(audio_data) > 25 * 1024 * 1024:
        raise HTTPException(status_code=400, detail="Audio file too large (max 25 MB)")

    if not audio_data:
        raise HTTPException(status_code=400, detail="Empty audio file")

    try:
        result = await transcription_service.transcribe(
            audio_data=audio_data,
            filename=filename,
            language=language,
        )
        return result
    except ValueError as e:
        logger.error("Transcription config error: %s", e)
        raise HTTPException(status_code=500, detail=str(e))
    except Exception as e:
        logger.error("Transcription failed: %s: %s", type(e).__name__, e)
        raise HTTPException(status_code=500, detail="Transcription failed")


if __name__ == "__main__":
    import uvicorn
    uvicorn.run("main:app", host="0.0.0.0", port=config.PORT, reload=config.DEBUG)
