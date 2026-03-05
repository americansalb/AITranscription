"""Vaak Lite — Live interpretation and translation API."""

import logging
from pathlib import Path

from fastapi import FastAPI, File, Form, HTTPException, UploadFile
from fastapi.middleware.cors import CORSMiddleware
from fastapi.staticfiles import StaticFiles
from fastapi.responses import FileResponse
from pydantic import BaseModel

import config
from transcription import transcription_service
from translation import translate, get_available_providers

logging.basicConfig(level=logging.INFO)
logger = logging.getLogger(__name__)

STATIC_DIR = Path(__file__).parent / "static"

app = FastAPI(
    title="Vaak Lite",
    description="Live interpretation and translation API — Whisper transcription + multi-LLM translation",
    version="0.1.0",
    docs_url="/docs" if config.DEBUG else None,
)

app.add_middleware(
    CORSMiddleware,
    allow_origins=["*"],
    allow_methods=["GET", "POST", "OPTIONS"],
    allow_headers=["Content-Type"],
)


# ── Health & Config ──────────────────────────────────────

@app.get("/health")
async def health():
    providers = get_available_providers()
    return {
        "status": "healthy",
        "groq_configured": bool(config.GROQ_API_KEY),
        "whisper_model": config.WHISPER_MODEL,
        "translation_providers": [p["id"] for p in providers],
    }


@app.get("/providers")
async def list_providers():
    """List available translation LLM providers and their models."""
    return {"providers": get_available_providers()}


# ── Transcribe Only ──────────────────────────────────────

ALLOWED_EXTENSIONS = {".wav", ".mp3", ".m4a", ".webm", ".ogg", ".flac", ".mp4"}


@app.post("/transcribe")
async def transcribe_audio(
    audio: UploadFile = File(...),
    language: str | None = Form(default=None),
):
    """Transcribe audio → text in the source language."""
    filename = audio.filename or "audio.wav"
    ext = "." + filename.rsplit(".", 1)[-1].lower() if "." in filename else ""
    if ext and ext not in ALLOWED_EXTENSIONS:
        raise HTTPException(status_code=400, detail=f"Unsupported format: {ext}")

    audio_data = await audio.read()
    if len(audio_data) > 25 * 1024 * 1024:
        raise HTTPException(status_code=400, detail="Audio too large (max 25 MB)")
    if not audio_data:
        raise HTTPException(status_code=400, detail="Empty audio file")

    try:
        return await transcription_service.transcribe(
            audio_data=audio_data, filename=filename, language=language,
        )
    except ValueError as e:
        raise HTTPException(status_code=500, detail=str(e))
    except Exception as e:
        logger.error("Transcription failed: %s: %s", type(e).__name__, e)
        raise HTTPException(status_code=500, detail="Transcription failed")


# ── Translate Only ───────────────────────────────────────

class TranslateRequest(BaseModel):
    text: str
    source_lang: str
    target_lang: str
    provider: str = "claude"


@app.post("/translate")
async def translate_text(req: TranslateRequest):
    """Translate text from source language to target language using the selected LLM."""
    if not req.text.strip():
        raise HTTPException(status_code=400, detail="Text cannot be empty")

    try:
        return await translate(
            text=req.text,
            source_lang=req.source_lang,
            target_lang=req.target_lang,
            provider=req.provider,
        )
    except ValueError as e:
        raise HTTPException(status_code=400, detail=str(e))
    except Exception as e:
        logger.error("Translation failed: %s: %s", type(e).__name__, e)
        raise HTTPException(status_code=500, detail="Translation failed")


# ── Transcribe + Translate (single call) ─────────────────

@app.post("/interpret")
async def interpret(
    audio: UploadFile = File(...),
    source_lang: str | None = Form(default=None),
    target_lang: str = Form(...),
    provider: str = Form(default="claude"),
):
    """Full interpretation pipeline: transcribe audio then translate.

    This is the main endpoint for live interpretation.
    """
    filename = audio.filename or "audio.wav"
    ext = "." + filename.rsplit(".", 1)[-1].lower() if "." in filename else ""
    if ext and ext not in ALLOWED_EXTENSIONS:
        raise HTTPException(status_code=400, detail=f"Unsupported format: {ext}")

    audio_data = await audio.read()
    if len(audio_data) > 25 * 1024 * 1024:
        raise HTTPException(status_code=400, detail="Audio too large (max 25 MB)")
    if not audio_data:
        raise HTTPException(status_code=400, detail="Empty audio file")

    # Step 1: Transcribe
    try:
        transcription = await transcription_service.transcribe(
            audio_data=audio_data, filename=filename, language=source_lang,
        )
    except Exception as e:
        logger.error("Transcription failed: %s: %s", type(e).__name__, e)
        raise HTTPException(status_code=500, detail="Transcription failed")

    if not transcription["text"].strip():
        return {
            "source_text": "",
            "translated_text": "",
            "source_lang": transcription.get("language", source_lang),
            "target_lang": target_lang,
            "duration": transcription.get("duration"),
            "segments": transcription.get("segments", []),
            "provider": provider,
            "model": "",
        }

    # Step 2: Translate
    detected_lang = transcription.get("language") or source_lang or "auto"
    try:
        translation = await translate(
            text=transcription["text"],
            source_lang=detected_lang,
            target_lang=target_lang,
            provider=provider,
        )
    except ValueError as e:
        raise HTTPException(status_code=400, detail=str(e))
    except Exception as e:
        logger.error("Translation failed: %s: %s", type(e).__name__, e)
        raise HTTPException(status_code=500, detail="Translation failed")

    return {
        "source_text": transcription["text"],
        "translated_text": translation["translated_text"],
        "source_lang": detected_lang,
        "target_lang": target_lang,
        "duration": transcription.get("duration"),
        "segments": transcription.get("segments", []),
        "provider": translation["provider"],
        "model": translation["model"],
    }


# ── Serve Frontend ───────────────────────────────────────

if STATIC_DIR.exists():
    # Serve static assets (JS, CSS, images, etc.)
    app.mount("/assets", StaticFiles(directory=STATIC_DIR / "assets"), name="assets")

    @app.get("/{full_path:path}")
    async def serve_spa(full_path: str):
        """Serve the frontend SPA. Any non-API path returns index.html."""
        file_path = STATIC_DIR / full_path
        if file_path.is_file():
            return FileResponse(file_path)
        return FileResponse(STATIC_DIR / "index.html")


if __name__ == "__main__":
    import uvicorn
    uvicorn.run("main:app", host="0.0.0.0", port=config.PORT, reload=config.DEBUG)
