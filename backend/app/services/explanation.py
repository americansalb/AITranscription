"""
Service for generating spoken explanations of code changes using Claude Haiku.
"""
from anthropic import AsyncAnthropic

from app.core.config import settings


class ExplanationService:
    """Generates natural language explanations of code changes for TTS."""

    def __init__(self):
        self._client: AsyncAnthropic | None = None

    @property
    def client(self) -> AsyncAnthropic:
        if self._client is None:
            if not settings.anthropic_api_key:
                raise ValueError("ANTHROPIC_API_KEY is not configured")
            self._client = AsyncAnthropic(api_key=settings.anthropic_api_key)
        return self._client

    async def explain_code_change(
        self,
        file_path: str,
        content: str,
        operation: str = "Write",
        old_content: str | None = None,
        max_words: int = 40,
    ) -> str:
        """
        Generate a spoken explanation of a code change.

        Args:
            file_path: Path to the file that was changed
            content: The new content (or snippet for large files)
            operation: "Write" for new file, "Edit" for modification
            old_content: Previous content (for Edit operations)
            max_words: Target word count (~40 words = ~15 sec speech, fits 200 char TTS limit)

        Returns:
            Natural language explanation suitable for TTS
        """
        # Truncate content to avoid token limits
        content_preview = content[:2000] if len(content) > 2000 else content

        # Determine file type for context
        file_ext = file_path.split(".")[-1] if "." in file_path else "txt"
        file_name = file_path.split("/")[-1] if "/" in file_path else file_path

        system_prompt = f"""You are explaining code changes to a developer via voice.

CRITICAL RULES:
1. Response MUST be under {max_words} words (will be spoken aloud)
2. Be conversational, like talking to a colleague
3. Lead with WHAT you did, then briefly WHY if relevant
4. NO markdown, NO code snippets, NO bullet points, NO special characters
5. Use "I" to describe actions ("I added...", "I created...")
6. Can end with a brief question if appropriate
7. Plain spoken English only

File type: {file_ext}
Operation: {"Created new file" if operation == "Write" else "Modified existing file"}"""

        user_prompt = f"""Explain this code change in spoken language:

File: {file_name}
{"New content:" if operation == "Write" else "Updated content:"}
{content_preview}

Remember: Under {max_words} words, conversational, no code in response."""

        try:
            response = await self.client.messages.create(
                model=settings.haiku_model,
                max_tokens=100,
                messages=[{"role": "user", "content": user_prompt}],
                system=system_prompt,
            )

            explanation = response.content[0].text.strip()

            # Clean up any markdown or special chars that slipped through
            explanation = explanation.replace("*", "").replace("`", "").replace("#", "")

            return explanation

        except Exception as e:
            # Fallback to simple explanation
            print(f"Explanation generation error: {e}")
            return f"I updated {file_name}."


# Singleton instance
explanation_service = ExplanationService()
