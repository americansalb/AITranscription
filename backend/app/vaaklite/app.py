"""Vaak Lite sub-app — mounted at /vaaklite on the main service."""

import logging
from pathlib import Path

from fastapi import FastAPI, File, Form, HTTPException, UploadFile
from fastapi.staticfiles import StaticFiles
from fastapi.responses import FileResponse
from pydantic import BaseModel

from app.vaaklite.transcription import transcription_service
from app.vaaklite.translation import translate, get_available_providers
from app.vaaklite import GROQ_API_KEY, WHISPER_MODEL

logger = logging.getLogger(__name__)

STATIC_DIR = Path(__file__).parent / "static"

vaaklite_app = FastAPI(title="Vaak Lite", docs_url=None, redoc_url=None)

ALLOWED_EXTENSIONS = {".wav", ".mp3", ".m4a", ".webm", ".ogg", ".flac", ".mp4"}


@vaaklite_app.get("/api/health")
async def health():
    providers = get_available_providers()
    return {
        "status": "healthy",
        "groq_configured": bool(GROQ_API_KEY),
        "whisper_model": WHISPER_MODEL,
        "translation_providers": [p["id"] for p in providers],
    }


@vaaklite_app.get("/api/providers")
async def list_providers():
    return {"providers": get_available_providers()}


@vaaklite_app.post("/api/transcribe")
async def transcribe_audio(
    audio: UploadFile = File(...),
    language: str | None = Form(default=None),
):
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


class TranslateRequest(BaseModel):
    text: str
    source_lang: str
    target_lang: str
    provider: str = "claude"


@vaaklite_app.post("/api/translate")
async def translate_text(req: TranslateRequest):
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


@vaaklite_app.post("/api/interpret")
async def interpret(
    audio: UploadFile = File(...),
    source_lang: str | None = Form(default=None),
    target_lang: str = Form(...),
    provider: str = Form(default="claude"),
):
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
    vaaklite_app.mount("/assets", StaticFiles(directory=STATIC_DIR / "assets"), name="vaaklite-assets")

    @vaaklite_app.get("/{full_path:path}")
    async def serve_spa(full_path: str):
        file_path = STATIC_DIR / full_path
        if file_path.is_file():
            return FileResponse(file_path)
        return FileResponse(STATIC_DIR / "index.html")
