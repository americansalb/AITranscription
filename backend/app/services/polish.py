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
        prompt_parts = [
            "You are a transcription cleanup assistant. Your job is to transform raw speech-to-text output into clean, readable text.",
            "",
            "RULES:",
            "1. If the text is in a non-English language, translate it to English while preserving the meaning",
            "2. Remove filler words (um, uh, like, you know, basically, actually, sort of, kind of) unless they add meaning",
            "3. Fix grammar and punctuation while preserving the speaker's voice",
            "4. Add appropriate capitalization and sentence structure",
            "5. Remove false starts and self-corrections (keep only the intended word)",
            "6. NEVER add information that wasn't in the original",
            "7. NEVER change the core meaning or intent",
            "8. NEVER add commentary, explanations, or suggestions",
            "9. Return ONLY the cleaned text (or translation), nothing else",
        ]

        # Add formality guidance
        formality_guides = {
            "casual": "10. Keep the tone casual and conversational. Contractions are fine.",
            "neutral": "10. Use a balanced, professional but approachable tone.",
            "formal": "10. Use formal language. Avoid contractions. Suitable for business/academic contexts.",
        }
        prompt_parts.append(formality_guides.get(formality, formality_guides["neutral"]))

        # Add context awareness
        if context:
            prompt_parts.extend([
                "",
                f"CONTEXT: The user is writing in: {context}",
                "Adjust formatting appropriately for this context.",
            ])

        # Add custom dictionary
        if custom_words:
            prompt_parts.extend([
                "",
                "CUSTOM VOCABULARY (use exact spelling):",
                ", ".join(custom_words),
            ])

        return "\n".join(prompt_parts)

    async def polish(
        self,
        raw_text: str,
        context: str | None = None,
        custom_words: list[str] | None = None,
        formality: str = "neutral",
    ) -> dict:
        """
        Polish raw transcription text using Claude Haiku.

        Args:
            raw_text: Raw transcription from Whisper
            context: Optional context (e.g., 'email', 'slack', 'code comment', 'document')
            custom_words: List of custom vocabulary to preserve exactly
            formality: 'casual', 'neutral', or 'formal'

        Returns:
            dict with 'text' (polished text) and 'usage' (token counts)
        """
        if not raw_text.strip():
            return {"text": "", "usage": {"input_tokens": 0, "output_tokens": 0}}

        system_prompt = self._build_system_prompt(context, custom_words, formality)

        response = await self.client.messages.create(
            model=settings.haiku_model,
            max_tokens=4096,
            system=system_prompt,
            messages=[{"role": "user", "content": raw_text}],
        )

        polished_text = response.content[0].text if response.content else ""

        return {
            "text": polished_text,
            "usage": {
                "input_tokens": response.usage.input_tokens,
                "output_tokens": response.usage.output_tokens,
            },
        }


# Singleton instance
polish_service = PolishService()
