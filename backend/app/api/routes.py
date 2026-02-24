import json
import logging
import re
from typing import Optional

logger = logging.getLogger(__name__)

from fastapi import APIRouter, Depends, File, Form, HTTPException, UploadFile
from fastapi.responses import StreamingResponse
from sqlalchemy import update
from sqlalchemy.ext.asyncio import AsyncSession

from app.api.auth import get_current_user
from app.api.schemas import (
    ComputerUseRequest,
    ComputerUseResponse,
    DescribeScreenRequest,
    DescribeScreenResponse,
    ErrorResponse,
    HealthResponse,
    PolishRequest,
    PolishResponse,
    ScreenReaderChatRequest,
    ScreenReaderChatResponse,
    TranscribeAndPolishResponse,
    TranscribeBase64Request,
    TranscribeResponse,
)
from app.core.config import settings
from app.core.database import get_db
from app.models.transcript import Transcript
from app.models.user import User
from app.services import polish_service, transcription_service
from app.services import elevenlabs_tts
from app.services.gamification import GamificationService, AchievementService

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
    current_user: User = Depends(get_current_user),
):
    """
    Transcribe audio using Groq's Whisper API.

    Accepts audio files in various formats: wav, mp3, m4a, webm, ogg, flac.
    Returns raw transcription text. Requires authentication.
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
        logger.error("Transcription ValueError: %s", e)
        raise HTTPException(status_code=400, detail="Transcription failed: invalid input")
    except TimeoutError as e:
        logger.error("Transcription timed out: %s", e)
        raise HTTPException(status_code=504, detail="Transcription timed out — try a shorter recording")
    except Exception as e:
        logger.error("Transcription failed: %s: %s", type(e).__name__, e)
        raise HTTPException(status_code=502, detail="Transcription service unavailable")


@router.post(
    "/polish",
    response_model=PolishResponse,
    responses={400: {"model": ErrorResponse}, 500: {"model": ErrorResponse}},
)
async def polish_text(
    request: PolishRequest,
    db: AsyncSession = Depends(get_db),
    user: User = Depends(get_current_user),
):
    """
    Polish raw transcription text using Claude Haiku.

    Removes filler words, fixes grammar, and formats appropriately for context.
    Uses learned corrections from the user's history. Requires authentication.
    """
    if not request.text.strip():
        raise HTTPException(status_code=400, detail="Text cannot be empty")

    try:
        result = await polish_service.polish(
            raw_text=request.text,
            context=request.context,
            custom_words=request.custom_words,
            formality=request.formality,
            db=db,
            user_id=user.id,
        )

        return PolishResponse(
            text=result["text"],
            input_tokens=result["usage"]["input_tokens"],
            output_tokens=result["usage"]["output_tokens"],
        )

    except ValueError as e:
        logger.warning("Polish ValueError, returning raw text: %s", e)
        return PolishResponse(text=request.text, input_tokens=0, output_tokens=0)
    except Exception as e:
        logger.warning("Polish failed, returning raw text: %s: %s", type(e).__name__, e)
        return PolishResponse(text=request.text, input_tokens=0, output_tokens=0)


@router.post("/polish-stream")
async def polish_text_stream(
    request: PolishRequest,
    db: AsyncSession = Depends(get_db),
    user: User = Depends(get_current_user),
):
    """
    Stream polished text using Server-Sent Events.

    This endpoint provides progressive polish results as they arrive from Claude.
    Falls back to batch /polish endpoint if streaming fails.
    Requires authentication.

    Returns SSE stream with events:
    - correction_info: Number of learned corrections used
    - chunk: Text chunks as they arrive
    - done: Final event with usage statistics
    - error: Error information if streaming fails
    """
    if not request.text.strip():
        raise HTTPException(status_code=400, detail="Text cannot be empty")

    async def event_generator():
        """Generate Server-Sent Events from polish stream."""
        try:
            async for event in polish_service.polish_stream(
                raw_text=request.text,
                context=request.context,
                custom_words=request.custom_words,
                formality=request.formality,
                db=db,
                user_id=user.id,
            ):
                # Format as SSE: "data: {json}\n\n"
                yield f"data: {json.dumps(event)}\n\n"

        except Exception as e:
            logger.error("SSE stream error: %s: %s", type(e).__name__, e)
            error_event = {"type": "error", "data": {"message": "Processing failed"}}
            yield f"data: {json.dumps(error_event)}\n\n"

    return StreamingResponse(
        event_generator(),
        media_type="text/event-stream",
        headers={
            "Cache-Control": "no-cache",
            "X-Accel-Buffering": "no",  # Disable nginx buffering
        },
    )


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
    user: User = Depends(get_current_user),
):
    """
    Combined endpoint: transcribe audio and polish the result.

    This is the main endpoint for the transcription pipeline.
    Provides both raw and polished text for comparison/debugging.
    Uses learned corrections from the user's history. Requires authentication.
    """
    # First, transcribe
    transcribe_response = await transcribe_audio(audio=audio, language=language, current_user=user)

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
            db=db,
            user_id=user.id,
        )

        # Save transcript to database (user is always authenticated now)
        if db:
            # Calculate metrics — use regex for accurate word boundaries (H8 fix)
            word_count = len(re.findall(r'\b\w+\b', result["text"]))
            character_count = len(result["text"])
            duration = transcribe_response.duration or 0
            # Cap WPM at 300 to prevent inflated values from short recordings (C7 fix)
            raw_wpm = (word_count / (duration / 60)) if duration > 0 else 0
            words_per_minute = min(raw_wpm, 300.0) if duration >= 5 else 0

            # Create transcript record
            transcript = Transcript(
                user_id=user.id,
                raw_text=transcribe_response.raw_text,
                polished_text=result["text"],
                audio_duration_seconds=duration,
                language=transcribe_response.language,
                word_count=word_count,
                character_count=character_count,
                words_per_minute=words_per_minute,
                context=context,
                formality=formality,
                transcript_type="input",
            )
            db.add(transcript)

            # Update user statistics atomically to prevent race conditions (H5 fix)
            # H7 fix: Use float for audio_seconds to avoid truncation
            from app.models.user import User as UserModel
            await db.execute(
                update(UserModel)
                .where(UserModel.id == user.id)
                .values(
                    total_transcriptions=UserModel.total_transcriptions + 1,
                    total_words=UserModel.total_words + word_count,
                    total_audio_seconds=UserModel.total_audio_seconds + round(duration),
                    total_polish_tokens=UserModel.total_polish_tokens + result["usage"]["input_tokens"] + result["usage"]["output_tokens"],
                )
            )

            await db.flush()

            # Award XP for transcription
            try:
                gamification_service = GamificationService(db)
                await gamification_service.award_transcription_xp(
                    user_id=user.id,
                    word_count=word_count,
                    transcript_id=transcript.id,
                )

                # Check for new achievements (async, non-blocking on error)
                achievement_service = AchievementService(db)
                await achievement_service.check_achievements(user.id)
            except Exception as gam_error:
                # Log but don't fail the transcription
                import logging
                logging.getLogger(__name__).warning(f"Gamification error: {gam_error}")

            await db.commit()

        return TranscribeAndPolishResponse(
            raw_text=transcribe_response.raw_text,
            polished_text=result["text"],
            duration=transcribe_response.duration,
            language=transcribe_response.language,
            usage={
                "input_tokens": result["usage"]["input_tokens"],
                "output_tokens": result["usage"]["output_tokens"],
            },
            saved=bool(user),
        )
    except Exception as e:
        logger.warning("Polish failed, returning raw text: %s: %s", type(e).__name__, e)
        # Graceful degradation: return raw text as polished text instead of 500
        return TranscribeAndPolishResponse(
            raw_text=transcribe_response.raw_text,
            polished_text=transcribe_response.raw_text,
            duration=transcribe_response.duration,
            language=transcribe_response.language,
            usage={"input_tokens": 0, "output_tokens": 0},
        )


@router.post(
    "/tts",
    responses={400: {"model": ErrorResponse}, 500: {"model": ErrorResponse}},
)
async def text_to_speech(
    text: str = Form(..., description="Text to convert to speech"),
    session_id: str | None = Form(default=None, description="Claude Code session identifier"),
    voice_id: str | None = Form(default=None, description="ElevenLabs voice ID for per-session voice"),
    user: User = Depends(get_current_user),
    db: AsyncSession = Depends(get_db),
):
    """
    Convert text to speech using ElevenLabs API.

    Used for Claude Code speak integration - when Claude wants to speak responses aloud.
    Returns MP3 audio data. Requires authentication.
    """
    if not text.strip():
        raise HTTPException(status_code=400, detail="Text cannot be empty")

    # Limit text length to prevent API cost abuse (5000 chars ≈ ~1000 words)
    if len(text) > 5000:
        raise HTTPException(status_code=400, detail="Text too long. Maximum 5000 characters.")

    full_text = text

    try:
        from fastapi.responses import Response

        audio_bytes = await elevenlabs_tts.synthesize(text, voice_id=voice_id)

        if not audio_bytes:
            raise HTTPException(status_code=502, detail="TTS service returned empty audio")

        # Save as output transcript (user is always authenticated now)
        if db:
            try:
                word_count = len(full_text.split())
                character_count = len(full_text)

                transcript = Transcript(
                    user_id=user.id,
                    raw_text=full_text,
                    polished_text=full_text,
                    audio_duration_seconds=0.0,  # TTS doesn't have input audio duration
                    language="en",
                    word_count=word_count,
                    character_count=character_count,
                    words_per_minute=0,
                    context="claude_output",
                    formality="neutral",
                    transcript_type="output",
                    session_id=session_id,
                )
                db.add(transcript)
                await db.commit()
            except Exception as save_error:
                logger.error("Failed to save transcript: %s: %s", type(save_error).__name__, save_error)
                await db.rollback()
                # Continue anyway - don't fail the TTS request just because we couldn't save

        return Response(
            content=audio_bytes,
            media_type="audio/mpeg",
            headers={
                "Content-Disposition": "inline; filename=speech.mp3",
                "Cache-Control": "no-cache",
            },
        )

    except ValueError as e:
        logger.error("TTS ValueError: %s", e)
        raise HTTPException(status_code=400, detail="TTS failed: invalid input")
    except Exception as e:
        logger.error("TTS failed: %s: %s", type(e).__name__, e, exc_info=True)
        raise HTTPException(status_code=502, detail="TTS service unavailable")


@router.post(
    "/tts/stream",
    responses={400: {"model": ErrorResponse}, 500: {"model": ErrorResponse}},
)
async def text_to_speech_stream(
    text: str = Form(..., description="Text to convert to speech"),
    session_id: str | None = Form(default=None, description="Claude Code session identifier"),
    voice_id: str | None = Form(default=None, description="ElevenLabs voice ID"),
    user: User = Depends(get_current_user),
):
    """
    Stream text-to-speech audio using ElevenLabs streaming API.

    Returns chunked audio data for lower latency playback (~200ms to first audio).
    Requires authentication.
    """
    if not text.strip():
        raise HTTPException(status_code=400, detail="Text cannot be empty")

    if len(text) > 5000:
        raise HTTPException(status_code=400, detail="Text too long. Maximum 5000 characters.")

    async def audio_stream():
        async for chunk in elevenlabs_tts.synthesize_stream(text, voice_id=voice_id):
            yield chunk

    return StreamingResponse(
        audio_stream(),
        media_type="audio/mpeg",
        headers={
            "Content-Disposition": "inline; filename=speech.mp3",
            "Cache-Control": "no-cache",
            "Transfer-Encoding": "chunked",
        },
    )


@router.post(
    "/describe-screen",
    response_model=DescribeScreenResponse,
    responses={500: {"model": ErrorResponse}},
)
async def describe_screen(request: DescribeScreenRequest, current_user: User = Depends(get_current_user)):
    """Describe a screenshot using Claude Vision API. Requires authentication."""
    if not settings.anthropic_api_key:
        raise HTTPException(status_code=503, detail="Anthropic API key not configured")

    try:
        from app.services.screen_reader import screen_reader_service

        result = await screen_reader_service.describe(
            image_base64=request.image_base64,
            blind_mode=request.blind_mode,
            detail=request.detail,
            model=request.model,
            focus=request.focus,
            uia_tree=request.uia_tree,
        )
        return DescribeScreenResponse(**result)
    except Exception as e:
        logger.error("Screen description failed: %s: %s", type(e).__name__, e)
        raise HTTPException(status_code=502, detail="Screen description service failed")


@router.post(
    "/transcribe-base64",
    response_model=TranscribeResponse,
    responses={400: {"model": ErrorResponse}, 500: {"model": ErrorResponse}},
)
async def transcribe_audio_base64(request: TranscribeBase64Request, current_user: User = Depends(get_current_user)):
    """
    Transcribe base64-encoded audio using Groq's Whisper API.

    Accepts JSON with base64-encoded WAV audio instead of multipart form upload.
    Used by the Rust desktop client for push-to-talk screen reader questions.
    Requires authentication.
    """
    import base64

    try:
        audio_data = base64.b64decode(request.audio_base64)
    except Exception:
        raise HTTPException(status_code=400, detail="Invalid base64 audio data")

    if len(audio_data) > 25 * 1024 * 1024:
        raise HTTPException(status_code=400, detail="Audio file too large. Maximum size is 25MB.")

    try:
        result = await transcription_service.transcribe(
            audio_data=audio_data,
            filename="recording.wav",
            language=request.language,
        )

        return TranscribeResponse(
            raw_text=result["text"],
            duration=result.get("duration"),
            language=result.get("language"),
        )
    except Exception as e:
        logger.error("SR transcription failed: %s: %s", type(e).__name__, e)
        raise HTTPException(status_code=502, detail="Transcription service unavailable")


@router.post(
    "/screen-reader-chat",
    response_model=ScreenReaderChatResponse,
    responses={500: {"model": ErrorResponse}},
)
async def screen_reader_chat(request: ScreenReaderChatRequest, current_user: User = Depends(get_current_user)):
    """
    Multi-turn conversation about a screenshot.

    The image is included in the first user message. Follow-up questions
    reference the same screenshot through conversation context.
    Requires authentication.
    """
    if not settings.anthropic_api_key:
        raise HTTPException(status_code=503, detail="Anthropic API key not configured")

    if not request.messages:
        raise HTTPException(status_code=400, detail="Messages cannot be empty")

    try:
        from app.services.screen_reader import screen_reader_service

        result = await screen_reader_service.chat(
            image_base64=request.image_base64,
            messages=request.messages,
            blind_mode=request.blind_mode,
            detail=request.detail,
            model=request.model,
            focus=request.focus,
            uia_tree=request.uia_tree,
        )
        return ScreenReaderChatResponse(**result)
    except Exception as e:
        logger.error("Screen reader chat failed: %s: %s", type(e).__name__, e)
        raise HTTPException(status_code=502, detail="Screen reader chat service failed")


@router.post(
    "/computer-use",
    response_model=ComputerUseResponse,
    responses={500: {"model": ErrorResponse}},
)
async def computer_use(request: ComputerUseRequest, current_user: User = Depends(get_current_user)):
    """
    Single-turn computer use call. Rust drives the tool_use loop,
    calling this endpoint repeatedly until stop_reason is 'end_turn'.
    Requires authentication.
    """
    if not settings.anthropic_api_key:
        raise HTTPException(status_code=503, detail="Anthropic API key not configured")

    if not request.messages:
        raise HTTPException(status_code=400, detail="Messages cannot be empty")

    try:
        from app.services.screen_reader import screen_reader_service
        import logging
        logger = logging.getLogger(__name__)

        # Convert pydantic models to dicts for the service
        messages = [{"role": m.role, "content": m.content} for m in request.messages]
        logger.warning(f"[ComputerUse] messages count={len(messages)}, display={request.display_width}x{request.display_height}")

        result = await screen_reader_service.computer_use(
            messages=messages,
            display_width=request.display_width,
            display_height=request.display_height,
            model=request.model,
            uia_tree=request.uia_tree,
        )
        logger.warning(f"[ComputerUse] success: stop_reason={result.get('stop_reason')}, blocks={len(result.get('content', []))}")
        return ComputerUseResponse(**result)
    except Exception as e:
        logger.error("Computer use failed: %s: %s", type(e).__name__, e, exc_info=True)
        raise HTTPException(status_code=502, detail="Computer use service failed")


@router.get("/voices")
async def get_voices(current_user: User = Depends(get_current_user)):
    """
    Get available ElevenLabs voices.

    Returns a list of voice objects with voice_id and name.
    Used for per-session voice assignment. Requires authentication.
    """
    voices = await elevenlabs_tts.get_available_voices()
    return {"voices": voices}
