"""
Text-to-Speech service using Groq's TTS models.
"""
import base64

from groq import AsyncGroq

from app.core.config import settings


# Available TTS voices
TTS_VOICES = {
    "Fritz-PlayAI": "Male, natural American English",
    "Arista-PlayAI": "Female, natural American English",
}


class TTSService:
    """Converts text to speech using Groq TTS API."""

    def __init__(self):
        self._client: AsyncGroq | None = None

    @property
    def client(self) -> AsyncGroq:
        if self._client is None:
            if not settings.groq_api_key:
                raise ValueError("GROQ_API_KEY is not configured")
            self._client = AsyncGroq(api_key=settings.groq_api_key)
        return self._client

    async def synthesize(
        self,
        text: str,
        voice: str = "Fritz-PlayAI",
        response_format: str = "mp3",
    ) -> bytes | None:
        """
        Convert text to speech.

        Args:
            text: Text to synthesize (recommended under 200 chars)
            voice: Voice ID to use
            response_format: Audio format (mp3, wav, flac, ogg)

        Returns:
            Audio bytes, or None if failed
        """
        if not text or not text.strip():
            return None

        # Groq TTS works best with shorter text
        # Truncate to ~200 chars at a sentence boundary if possible
        if len(text) > 200:
            # Try to find a good break point
            truncated = text[:200]
            last_period = truncated.rfind(".")
            last_question = truncated.rfind("?")
            last_exclaim = truncated.rfind("!")

            break_point = max(last_period, last_question, last_exclaim)
            if break_point > 100:
                text = text[: break_point + 1]
            else:
                text = truncated.rsplit(" ", 1)[0] + "..."

        try:
            response = await self.client.audio.speech.create(
                model="playai-tts",
                voice=voice,
                input=text,
                response_format=response_format,
            )

            # Read the audio bytes from the response
            audio_bytes = response.read()
            return audio_bytes

        except Exception as e:
            print(f"TTS error: {e}")
            return None

    def audio_to_base64(self, audio_bytes: bytes) -> str:
        """Convert audio bytes to base64 string for JSON transport."""
        return base64.b64encode(audio_bytes).decode("utf-8")


# Singleton instance
tts_service = TTSService()
