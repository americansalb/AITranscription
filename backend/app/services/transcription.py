import asyncio
import io
import logging
import time

import httpx
from groq import AsyncGroq

from app.core.config import settings

logger = logging.getLogger(__name__)


class TranscriptionService:
    """Handles audio transcription via Groq's Whisper API."""

    def __init__(self):
        self._client: AsyncGroq | None = None

    @property
    def client(self) -> AsyncGroq:
        if self._client is None:
            if not settings.groq_api_key:
                raise ValueError("GROQ_API_KEY is not configured")
            self._client = AsyncGroq(
                api_key=settings.groq_api_key,
                timeout=httpx.Timeout(120.0, connect=10.0),
            )
        return self._client

    async def transcribe(
        self,
        audio_data: bytes,
        filename: str = "audio.wav",
        language: str | None = None,
    ) -> dict:
        """
        Transcribe audio data using Groq's Whisper model.

        Args:
            audio_data: Raw audio bytes (supports wav, mp3, m4a, webm, etc.)
            filename: Original filename with extension for format detection
            language: Optional ISO language code (e.g., 'en', 'es', 'fr')

        Returns:
            dict with 'text' (transcription) and 'duration' (audio length in seconds)
        """
        # Create a file-like object from bytes
        audio_file = io.BytesIO(audio_data)
        audio_file.name = filename

        # Build transcription parameters
        params = {
            "file": audio_file,
            "model": settings.whisper_model,
            "response_format": "verbose_json",  # Includes duration and segments
        }

        if language:
            params["language"] = language

        # Proportional timeout from configurable settings
        estimated_audio_secs = max(len(audio_data) / 16000, 1)
        timeout_secs = min(
            settings.groq_timeout_base + (estimated_audio_secs / 60) * settings.groq_timeout_per_min_audio,
            settings.timeout_ceiling,
        )

        start_time = time.monotonic()

        try:
            response = await asyncio.wait_for(
                self.client.audio.transcriptions.create(**params),
                timeout=timeout_secs,
            )
        except asyncio.TimeoutError:
            elapsed = time.monotonic() - start_time
            logger.error(
                '{"api": "groq", "audio_seconds": %.1f, "response_time_ms": %.0f, '
                '"status": "timeout", "timeout_limit_s": %.0f}',
                estimated_audio_secs,
                elapsed * 1000,
                timeout_secs,
            )
            raise TimeoutError(
                f"Transcription timed out after {int(timeout_secs)}s"
            )

        elapsed = time.monotonic() - start_time
        logger.info(
            '{"api": "groq", "audio_seconds": %.1f, "response_time_ms": %.0f, '
            '"status": "ok", "timeout_limit_s": %.0f}',
            estimated_audio_secs,
            elapsed * 1000,
            timeout_secs,
        )

        return {
            "text": response.text,
            "duration": getattr(response, "duration", None),
            "language": getattr(response, "language", language),
            "segments": getattr(response, "segments", None),
        }


# Singleton instance
transcription_service = TranscriptionService()
