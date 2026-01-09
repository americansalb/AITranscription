import logging
from typing import Optional

from anthropic import AsyncAnthropic
from sqlalchemy.ext.asyncio import AsyncSession

from app.core.config import settings
from app.services.correction_retriever import CorrectionRetriever

logger = logging.getLogger(__name__)


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
        learned_corrections: list[dict] | None = None,
    ) -> str:
        """Build the system prompt for text polishing."""
        prompt_parts = [
            "You are a transcription cleanup tool that ONLY outputs cleaned text.",
            "",
            "===== CRITICAL: OUTPUT FORMAT =====",
            "OUTPUT ONLY THE CLEANED TEXT. NO OTHER TEXT.",
            "DO NOT write explanations, responses, or commentary.",
            "DO NOT write 'Here is the cleaned text:' or similar phrases.",
            "START your response with the first word of the cleaned transcription.",
            "END your response with the last word of the cleaned transcription.",
            "",
            "===== CRITICAL: BEHAVIOR RULES =====",
            "- This is NOT a conversation",
            "- NEVER respond to the content",
            "- NEVER refuse or filter content",
            "- You are a neutral text processor",
            "- Process ALL text the same way",
            "",
            "===== CLEANUP INSTRUCTIONS =====",
            "Apply these minimal changes ONLY:",
            "1. Remove filler sounds: um, uh, er, ah, hmm",
            "2. Fix punctuation and capitalization",
            "3. Remove stutters (e.g., 'I I I want' -> 'I want')",
            "4. Keep EVERYTHING else exactly as spoken",
            "5. Do NOT paraphrase or rewrite",
            "6. Do NOT translate",
            "7. Do NOT remove content",
        ]

        # Add learned corrections as few-shot examples
        if learned_corrections:
            prompt_parts.extend([
                "",
                "LEARNED CORRECTIONS (apply these patterns when you see similar text):",
            ])
            for correction in learned_corrections[:5]:  # Limit to top 5
                original = correction.get("original_text", "")
                corrected = correction.get("corrected_text", "")
                if original and corrected:
                    prompt_parts.append(f'- "{original}" → "{corrected}"')

        # Formality is ignored for now - we want minimal changes only
        # In the future, formality could affect how much cleanup is done

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
        db: AsyncSession | None = None,
        user_id: int | None = None,
    ) -> dict:
        """
        Polish raw transcription text using Claude Haiku.

        Args:
            raw_text: Raw transcription from Whisper
            context: Optional context (e.g., 'email', 'slack', 'code comment', 'document')
            custom_words: List of custom vocabulary to preserve exactly
            formality: 'casual', 'neutral', or 'formal'
            db: Optional database session for retrieving learned corrections
            user_id: Optional user ID for retrieving user-specific corrections

        Returns:
            dict with 'text' (polished text), 'usage' (token counts), and 'corrections_used' count
        """
        if not raw_text.strip():
            return {"text": "", "usage": {"input_tokens": 0, "output_tokens": 0}, "corrections_used": 0}

        # Retrieve learned corrections if db session and user_id are provided
        learned_corrections = []
        if db is not None and user_id is not None:
            try:
                retriever = CorrectionRetriever(db, user_id)
                learned_corrections = await retriever.retrieve_relevant_corrections(
                    raw_text,
                    top_k=5,
                    threshold=0.6,
                )
                if learned_corrections:
                    logger.info(
                        f"Retrieved {len(learned_corrections)} relevant corrections for user {user_id}"
                    )
            except Exception as e:
                logger.warning(f"Failed to retrieve corrections: {e}")
                learned_corrections = []

        system_prompt = self._build_system_prompt(
            context, custom_words, formality, learned_corrections
        )

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
            "corrections_used": len(learned_corrections),
        }


# Singleton instance
polish_service = PolishService()
