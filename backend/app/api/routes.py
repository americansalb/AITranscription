from typing import Optional

from fastapi import APIRouter, Depends, File, Form, HTTPException, UploadFile
from sqlalchemy.ext.asyncio import AsyncSession

from app.api.auth import get_optional_user
from app.api.schemas import (
    ErrorResponse,
    HealthResponse,
    PolishRequest,
    PolishResponse,
    TranscribeAndPolishResponse,
    TranscribeResponse,
)
from app.core.config import settings
from app.core.database import get_db
from app.models.user import User
from app.services import polish_service, transcription_service
from app.services import elevenlabs_tts

router = APIRouter()


@router.get("/health", response_model=HealthResponse)
async def health_check():
    """Check API health and configuration status."""
    return HealthResponse(
        status="healthy",
        version="0.1.0",
        groq_configured=bool(settings.groq_api_key),
        anthropic_configured=bool(settings.anthropic_api_key),
    )


@router.post(
    "/transcribe",
    response_model=TranscribeResponse,
    responses={400: {"model": ErrorResponse}, 500: {"model": ErrorResponse}},
)
async def transcribe_audio(
    audio: UploadFile = File(..., description="Audio file to transcribe"),
    language: str | None = Form(default=None, description="Optional language code (e.g., 'en')"),
):
    """
    Transcribe audio using Groq's Whisper API.

    Accepts audio files in various formats: wav, mp3, m4a, webm, ogg, flac.
    Returns raw transcription text.
    """
    # Validate file type
    allowed_types = {
        "audio/wav",
        "audio/wave",
        "audio/x-wav",
        "audio/mpeg",
        "audio/mp3",
        "audio/mp4",
        "audio/m4a",
        "audio/x-m4a",
        "audio/webm",
        "audio/ogg",
        "audio/flac",
    }

    content_type = audio.content_type or ""
    if content_type and content_type not in allowed_types:
        # Also check by extension as fallback
        filename = audio.filename or ""
        valid_extensions = {".wav", ".mp3", ".m4a", ".webm", ".ogg", ".flac"}
        if not any(filename.lower().endswith(ext) for ext in valid_extensions):
            raise HTTPException(
                status_code=400,
                detail=f"Unsupported audio format: {content_type}. Use wav, mp3, m4a, webm, ogg, or flac.",
            )

    try:
        audio_data = await audio.read()

        # Check file size (max 25MB - Groq's limit)
        if len(audio_data) > 25 * 1024 * 1024:
            raise HTTPException(status_code=400, detail="Audio file too large. Maximum size is 25MB.")

        result = await transcription_service.transcribe(
            audio_data=audio_data,
            filename=audio.filename or "audio.wav",
            language=language,
        )

        return TranscribeResponse(
            raw_text=result["text"],
            duration=result.get("duration"),
            language=result.get("language"),
        )

    except ValueError as e:
        raise HTTPException(status_code=500, detail=str(e))
    except Exception as e:
        raise HTTPException(status_code=500, detail=f"Transcription failed: {str(e)}")


@router.post(
    "/polish",
    response_model=PolishResponse,
    responses={400: {"model": ErrorResponse}, 500: {"model": ErrorResponse}},
)
async def polish_text(
    request: PolishRequest,
    db: AsyncSession = Depends(get_db),
    user: User | None = Depends(get_optional_user),
):
    """
    Polish raw transcription text using Claude Haiku.

    Removes filler words, fixes grammar, and formats appropriately for context.
    If authenticated, uses learned corrections from the user's history.
    """
    if not request.text.strip():
        raise HTTPException(status_code=400, detail="Text cannot be empty")

    try:
        result = await polish_service.polish(
            raw_text=request.text,
            context=request.context,
            custom_words=request.custom_words,
            formality=request.formality,
            db=db if user else None,
            user_id=user.id if user else None,
        )

        return PolishResponse(
            text=result["text"],
            input_tokens=result["usage"]["input_tokens"],
            output_tokens=result["usage"]["output_tokens"],
        )

    except ValueError as e:
        raise HTTPException(status_code=500, detail=str(e))
    except Exception as e:
        raise HTTPException(status_code=500, detail=f"Polish failed: {str(e)}")


@router.post(
    "/transcribe-and-polish",
    response_model=TranscribeAndPolishResponse,
    responses={400: {"model": ErrorResponse}, 500: {"model": ErrorResponse}},
)
async def transcribe_and_polish(
    audio: UploadFile = File(..., description="Audio file to transcribe and polish"),
    language: str | None = Form(default=None, description="Optional language code"),
    context: str | None = Form(default=None, description="Context like 'email', 'slack'"),
    formality: str = Form(default="neutral", description="'casual', 'neutral', or 'formal'"),
    db: AsyncSession = Depends(get_db),
    user: User | None = Depends(get_optional_user),
):
    """
    Combined endpoint: transcribe audio and polish the result.

    This is the main endpoint for the transcription pipeline.
    Provides both raw and polished text for comparison/debugging.
    If authenticated, uses learned corrections from the user's history.
    """
    # First, transcribe
    transcribe_response = await transcribe_audio(audio=audio, language=language)

    if not transcribe_response.raw_text.strip():
        return TranscribeAndPolishResponse(
            raw_text="",
            polished_text="",
            duration=transcribe_response.duration,
            language=transcribe_response.language,
            usage={"input_tokens": 0, "output_tokens": 0},
        )

    # Then, polish (with learned corrections if authenticated)
    try:
        result = await polish_service.polish(
            raw_text=transcribe_response.raw_text,
            context=context,
            formality=formality,
            db=db if user else None,
            user_id=user.id if user else None,
        )

        return TranscribeAndPolishResponse(
            raw_text=transcribe_response.raw_text,
            polished_text=result["text"],
            duration=transcribe_response.duration,
            language=transcribe_response.language,
            usage={
                "input_tokens": result["usage"]["input_tokens"],
                "output_tokens": result["usage"]["output_tokens"],
            },
        )
    except Exception as e:
        raise HTTPException(status_code=500, detail=f"Polish failed: {str(e)}")


@router.post(
    "/tts",
    responses={400: {"model": ErrorResponse}, 500: {"model": ErrorResponse}},
)
async def text_to_speech(
    text: str = Form(..., description="Text to convert to speech"),
    user: User | None = Depends(get_optional_user),
):
    """
    Convert text to speech using Groq TTS (Orpheus model).

    Used for Claude Code speak integration - when Claude wants to speak responses aloud.
    Returns MP3 audio data.
    """
    if not text.strip():
        raise HTTPException(status_code=400, detail="Text cannot be empty")

    # Limit text length to prevent abuse
    if len(text) > 500:
        text = text[:500]

    try:
        from fastapi.responses import Response

        audio_bytes = await elevenlabs_tts.synthesize(text)

        if not audio_bytes:
            raise HTTPException(status_code=500, detail="ElevenLabs TTS generation failed")

        return Response(
            content=audio_bytes,
            media_type="audio/mpeg",
            headers={
                "Content-Disposition": "inline; filename=speech.mp3",
                "Cache-Control": "no-cache",
            },
        )

    except ValueError as e:
        raise HTTPException(status_code=500, detail=str(e))
    except Exception as e:
        raise HTTPException(status_code=500, detail=f"TTS failed: {str(e)}")
