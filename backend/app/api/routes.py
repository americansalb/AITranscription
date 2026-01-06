from fastapi import APIRouter, BackgroundTasks, Depends, File, Form, HTTPException, UploadFile
from fastapi.responses import StreamingResponse
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
from app.models.transcript import Transcript
from app.models.user import User
from app.services import polish_service, transcription_service
from app.services.explanation import explanation_service
from app.services.tts import tts_service
from app.services.events import event_manager, VoiceEvent
from datetime import datetime

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


@router.get("/debug/schema")
async def debug_schema(db: AsyncSession = Depends(get_db)):
    """Debug endpoint to check database schema."""
    from sqlalchemy import text

    try:
        # Check users table columns
        result = await db.execute(text(
            "SELECT column_name FROM information_schema.columns WHERE table_name = 'users' ORDER BY ordinal_position"
        ))
        user_columns = [row[0] for row in result.fetchall()]

        # Check if transcripts table exists
        result = await db.execute(text(
            "SELECT EXISTS (SELECT FROM information_schema.tables WHERE table_name = 'transcripts')"
        ))
        transcripts_exists = result.scalar()

        return {
            "users_columns": user_columns,
            "transcripts_table_exists": transcripts_exists,
            "status": "ok"
        }
    except Exception as e:
        return {"error": str(e), "status": "error"}


@router.post(
    "/transcribe",
    response_model=TranscribeResponse,
    responses={400: {"model": ErrorResponse}, 500: {"model": ErrorResponse}},
)
async def transcribe_audio(
    audio: UploadFile = File(..., description="Audio file to transcribe"),
    language: str | None = Form(default=None, description="Optional language code (e.g., 'en')"),
    model: str | None = Form(default=None, description="Whisper model: 'whisper-large-v3' or 'whisper-large-v3-turbo'"),
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
            model=model,
        )

        return TranscribeResponse(
            raw_text=result["text"],
            duration=result.get("duration"),
            language=result.get("language"),
            model_used=result.get("model_used"),
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
async def polish_text(request: PolishRequest):
    """
    Polish raw transcription text using Claude Haiku.

    Removes filler words, fixes grammar, and formats appropriately for context.
    """
    if not request.text.strip():
        raise HTTPException(status_code=400, detail="Text cannot be empty")

    try:
        result = await polish_service.polish(
            raw_text=request.text,
            context=request.context,
            custom_words=request.custom_words,
            formality=request.formality,
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
    model: str | None = Form(default=None, description="Whisper model: 'whisper-large-v3' or 'whisper-large-v3-turbo'"),
    db: AsyncSession = Depends(get_db),
    current_user: User | None = Depends(get_optional_user),
):
    """
    Combined endpoint: transcribe audio and polish the result.

    This is the main endpoint for the transcription pipeline.
    Provides both raw and polished text for comparison/debugging.
    Saves transcript to database if user is authenticated.
    """
    # First, transcribe
    transcribe_response = await transcribe_audio(audio=audio, language=language, model=model)

    if not transcribe_response.raw_text.strip():
        return TranscribeAndPolishResponse(
            raw_text="",
            polished_text="",
            duration=transcribe_response.duration,
            language=transcribe_response.language,
            usage={"input_tokens": 0, "output_tokens": 0},
        )

    # Then, polish
    polish_request = PolishRequest(
        text=transcribe_response.raw_text,
        context=context,
        formality=formality,
    )
    polish_response = await polish_text(polish_request)

    # Save transcript to database
    raw_text = transcribe_response.raw_text
    polished_text = polish_response.text
    duration = transcribe_response.duration or 0.0
    word_count = len(polished_text.split())
    char_count = len(polished_text)
    wpm = (word_count / duration * 60) if duration > 0 else 0.0

    transcript = Transcript(
        user_id=current_user.id if current_user else None,
        raw_text=raw_text,
        polished_text=polished_text,
        audio_duration_seconds=duration,
        language=transcribe_response.language,
        word_count=word_count,
        character_count=char_count,
        words_per_minute=round(wpm, 1),
        context=context,
        formality=formality,
    )
    db.add(transcript)

    # Update user stats if authenticated
    if current_user:
        current_user.total_transcriptions += 1
        current_user.total_words += word_count
        current_user.total_audio_seconds += int(duration)
        current_user.daily_transcriptions_used += 1

    await db.commit()

    return TranscribeAndPolishResponse(
        raw_text=transcribe_response.raw_text,
        polished_text=polish_response.text,
        duration=transcribe_response.duration,
        language=transcribe_response.language,
        usage={
            "input_tokens": polish_response.input_tokens,
            "output_tokens": polish_response.output_tokens,
        },
    )


# ============================================================================
# Voice Response Endpoints (Claude Code Integration)
# ============================================================================


@router.post("/claude-event")
async def handle_claude_event(
    event: dict,
    background_tasks: BackgroundTasks,
):
    """
    Receive events from Claude Code hooks.

    This endpoint is called by the Claude Code PostToolUse hook script
    when code is written or edited. It triggers voice explanation generation.
    """
    tool_name = event.get("tool_name", "")
    tool_input = event.get("tool_input", {})

    # Only process Write and Edit operations
    if tool_name not in ("Write", "Edit"):
        return {"status": "ignored", "reason": f"tool {tool_name} not handled"}

    file_path = tool_input.get("file_path", "unknown")
    content = tool_input.get("content", "") or tool_input.get("new_string", "")
    old_content = tool_input.get("old_string")

    if not content:
        return {"status": "ignored", "reason": "no content"}

    # Process in background to respond quickly to the hook
    background_tasks.add_task(
        _process_claude_event,
        tool_name=tool_name,
        file_path=file_path,
        content=content,
        old_content=old_content,
    )

    return {"status": "accepted", "file_path": file_path}


async def _process_claude_event(
    tool_name: str,
    file_path: str,
    content: str,
    old_content: str | None,
):
    """Background task to generate and broadcast voice explanation."""
    try:
        # Generate explanation using Haiku
        explanation = await explanation_service.explain_code_change(
            file_path=file_path,
            content=content,
            operation=tool_name,
            old_content=old_content,
        )

        if not explanation:
            return

        # Convert to speech using Groq TTS
        audio_bytes = await tts_service.synthesize(explanation)

        if audio_bytes:
            # Broadcast to all connected clients
            await event_manager.broadcast(
                VoiceEvent(
                    type="voice",
                    audio_base64=tts_service.audio_to_base64(audio_bytes),
                    explanation=explanation,
                    file_path=file_path,
                    timestamp=datetime.utcnow().isoformat(),
                )
            )
        else:
            # TTS failed, but still send the text explanation
            await event_manager.broadcast(
                VoiceEvent(
                    type="status",
                    explanation=explanation,
                    file_path=file_path,
                    timestamp=datetime.utcnow().isoformat(),
                )
            )

    except Exception as e:
        # Broadcast error to clients
        await event_manager.broadcast(
            VoiceEvent(
                type="error",
                explanation=f"Failed to generate explanation: {str(e)}",
                file_path=file_path,
                timestamp=datetime.utcnow().isoformat(),
            )
        )


@router.get("/voice-stream")
async def voice_stream():
    """
    Server-Sent Events endpoint for real-time voice notifications.

    Connect to this endpoint to receive voice explanations as they're generated.
    Events are JSON-formatted with types: 'connected', 'voice', 'status', 'error'.
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
    """
    Check voice stream status and connected client count.
    """
    return {
        "status": "active",
        "connected_clients": event_manager.client_count,
        "groq_configured": bool(settings.groq_api_key),
        "anthropic_configured": bool(settings.anthropic_api_key),
    }
