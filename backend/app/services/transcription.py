import io
from groq import AsyncGroq

from app.core.config import settings


# Available Groq Whisper models
WHISPER_MODELS = {
    "whisper-large-v3": "Higher accuracy, slower",
    "whisper-large-v3-turbo": "Faster, more cost-effective",
}


class TranscriptionService:
    """Handles audio transcription via Groq's Whisper API."""

    def __init__(self):
        self._client: AsyncGroq | None = None

    @property
    def client(self) -> AsyncGroq:
        if self._client is None:
            if not settings.groq_api_key:
                raise ValueError("GROQ_API_KEY is not configured")
            self._client = AsyncGroq(api_key=settings.groq_api_key)
        return self._client

    async def transcribe(
        self,
        audio_data: bytes,
        filename: str = "audio.wav",
        language: str | None = None,
        model: str | None = None,
    ) -> dict:
        """
        Transcribe audio data using Groq's Whisper model.

        Args:
            audio_data: Raw audio bytes (supports wav, mp3, m4a, webm, etc.)
            filename: Original filename with extension for format detection
            language: Optional ISO language code (e.g., 'en', 'es', 'fr')
            model: Whisper model to use ('whisper-large-v3' or 'whisper-large-v3-turbo')

        Returns:
            dict with 'text' (transcription) and 'duration' (audio length in seconds)
        """
        # Create a file-like object from bytes
        audio_file = io.BytesIO(audio_data)
        audio_file.name = filename

        # Use specified model or fall back to default
        whisper_model = model if model in WHISPER_MODELS else settings.whisper_model

        # Build transcription parameters
        params = {
            "file": audio_file,
            "model": whisper_model,
            "response_format": "verbose_json",  # Includes duration and segments
        }

        if language:
            params["language"] = language

        # Call Groq Whisper API
        response = await self.client.audio.transcriptions.create(**params)

        return {
            "text": response.text,
            "duration": getattr(response, "duration", None),
            "language": getattr(response, "language", language),
            "segments": getattr(response, "segments", None),
            "model_used": whisper_model,
        }


# Singleton instance
transcription_service = TranscriptionService()
