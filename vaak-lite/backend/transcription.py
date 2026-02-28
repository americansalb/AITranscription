"""Groq Whisper transcription service."""

import logging
from groq import AsyncGroq
import config

logger = logging.getLogger(__name__)


class TranscriptionService:
    """Transcribes audio using Groq's Whisper API."""

    def __init__(self):
        self._client: AsyncGroq | None = None

    @property
    def client(self) -> AsyncGroq:
        if self._client is None:
            if not config.GROQ_API_KEY:
                raise ValueError("GROQ_API_KEY is not configured")
            self._client = AsyncGroq(api_key=config.GROQ_API_KEY)
        return self._client

    async def transcribe(
        self,
        audio_data: bytes,
        filename: str = "audio.wav",
        language: str | None = None,
    ) -> dict:
        """Transcribe audio bytes using Groq Whisper.

        Args:
            audio_data: Raw audio bytes.
            filename: Original filename (used for format detection).
            language: Optional ISO language code (e.g. 'en'). None = auto-detect.

        Returns:
            dict with keys: text, duration, language, segments
        """
        kwargs: dict = {
            "model": config.WHISPER_MODEL,
            "file": (filename, audio_data),
            "response_format": "verbose_json",
        }
        if language and language != "auto":
            kwargs["language"] = language

        response = await self.client.audio.transcriptions.create(**kwargs)

        segments = []
        if hasattr(response, "segments") and response.segments:
            for seg in response.segments:
                segments.append({
                    "start": seg.get("start", 0) if isinstance(seg, dict) else getattr(seg, "start", 0),
                    "end": seg.get("end", 0) if isinstance(seg, dict) else getattr(seg, "end", 0),
                    "text": seg.get("text", "") if isinstance(seg, dict) else getattr(seg, "text", ""),
                })

        return {
            "text": response.text,
            "duration": getattr(response, "duration", None),
            "language": getattr(response, "language", language),
            "segments": segments,
        }


transcription_service = TranscriptionService()
