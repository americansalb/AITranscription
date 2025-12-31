from anthropic import AsyncAnthropic

from app.core.config import settings


class PolishService:
    """Handles text cleanup and formatting via Claude Haiku."""

    def __init__(self):
        self._client: AsyncAnthropic | None = None

    @property
    def client(self) -> AsyncAnthropic:
        if self._client is None:
            if not settings.anthropic_api_key:
                raise ValueError("ANTHROPIC_API_KEY is not configured")
            self._client = AsyncAnthropic(api_key=settings.anthropic_api_key)
        return self._client

    def _build_system_prompt(
        self,
        context: str | None = None,
        custom_words: list[str] | None = None,
        formality: str = "neutral",
    ) -> str:
        """Build the system prompt for text polishing."""
        # Currently unused - polish is disabled by default
        return ""

    async def polish(
        self,
        raw_text: str,
        context: str | None = None,
        custom_words: list[str] | None = None,
        formality: str = "neutral",
    ) -> dict:
        """
        Polish raw transcription text.

        Currently disabled - just returns the raw text unchanged.
        The Whisper transcription is already accurate enough.

        Args:
            raw_text: Raw transcription from Whisper
            context: Optional context (unused for now)
            custom_words: List of custom vocabulary (unused for now)
            formality: 'casual', 'neutral', or 'formal' (unused for now)

        Returns:
            dict with 'text' (unchanged raw text) and 'usage' (zero tokens)
        """
        # Just pass through the raw text - no modification
        return {
            "text": raw_text.strip(),
            "usage": {"input_tokens": 0, "output_tokens": 0},
        }


# Singleton instance
polish_service = PolishService()
