"""Multi-LLM translation service for Vaak Lite.

Supports Claude (Opus & Sonnet), GPT, Groq (Llama), and Gemini.
"""

import logging
from functools import partial

from app.vaaklite import (
    ANTHROPIC_API_KEY, CLAUDE_OPUS_MODEL, CLAUDE_SONNET_MODEL,
    OPENAI_API_KEY, OPENAI_MODEL,
    GROQ_API_KEY, GROQ_LLAMA_MODEL,
    GOOGLE_API_KEY, GOOGLE_MODEL,
)

logger = logging.getLogger(__name__)

# Provider ID -> model ID mapping (populated dynamically)
AVAILABLE_PROVIDERS: dict[str, str] = {}


def _check_providers():
    global AVAILABLE_PROVIDERS
    AVAILABLE_PROVIDERS = {}
    if ANTHROPIC_API_KEY:
        AVAILABLE_PROVIDERS["claude-opus"] = CLAUDE_OPUS_MODEL
        AVAILABLE_PROVIDERS["claude-sonnet"] = CLAUDE_SONNET_MODEL
    if OPENAI_API_KEY:
        AVAILABLE_PROVIDERS["gpt"] = OPENAI_MODEL
    if GROQ_API_KEY:
        AVAILABLE_PROVIDERS["groq"] = GROQ_LLAMA_MODEL
    if GOOGLE_API_KEY:
        AVAILABLE_PROVIDERS["gemini"] = GOOGLE_MODEL


_check_providers()


SYSTEM_PROMPT = """You are a professional interpreter and editor providing polished translations.

RULES:
1. Translate the source text into the target language.
2. Clean up the text for readability — spoken language is messy, your output should read like clean written prose.
3. Remove filler words (um, uh, like, you know), false starts, repetitions, and verbal stumbles.
4. Break the text into short, clear paragraphs. Put a blank line between each paragraph.
5. Each distinct thought or topic shift should start a new paragraph.
6. Preserve the speaker's meaning, tone, and intent — but improve the structure and flow.
7. If a word or phrase has no direct equivalent, use the closest natural expression in the target language.
8. Preserve proper nouns, technical terms, and brand names as-is unless they have an established translation.
9. Do NOT add commentary, notes, explanations, or speaker labels.
10. Output ONLY the translated, reorganized text. Nothing else."""


def _build_user_prompt(text: str, source_lang: str, target_lang: str, previous_translation: str = "") -> str:
    prompt = f"Translate from {source_lang} to {target_lang}:\n\n{text}"
    if previous_translation:
        prompt += (
            "\n\n---\n"
            "CONTINUITY CONTEXT:\n"
            "Below is your previous translation of an earlier, shorter version of this same audio. "
            "Keep the existing text stable — extend and refine, do not rearrange what was already correct. "
            "Only update where the new audio adds or changes content.\n\n"
            f"Previous translation:\n{previous_translation}"
        )
    return prompt


async def translate_claude(text: str, source_lang: str, target_lang: str, *, model: str, previous_translation: str = "") -> str:
    from anthropic import AsyncAnthropic
    client = AsyncAnthropic(api_key=ANTHROPIC_API_KEY)
    response = await client.messages.create(
        model=model,
        max_tokens=4096,
        system=SYSTEM_PROMPT,
        messages=[{"role": "user", "content": _build_user_prompt(text, source_lang, target_lang, previous_translation)}],
    )
    return response.content[0].text if response.content else ""


async def translate_gpt(text: str, source_lang: str, target_lang: str, *, previous_translation: str = "") -> str:
    from openai import AsyncOpenAI
    client = AsyncOpenAI(api_key=OPENAI_API_KEY)
    response = await client.chat.completions.create(
        model=OPENAI_MODEL,
        messages=[
            {"role": "system", "content": SYSTEM_PROMPT},
            {"role": "user", "content": _build_user_prompt(text, source_lang, target_lang, previous_translation)},
        ],
        max_tokens=4096,
    )
    return response.choices[0].message.content or ""


async def translate_groq(text: str, source_lang: str, target_lang: str, *, previous_translation: str = "") -> str:
    from groq import AsyncGroq
    client = AsyncGroq(api_key=GROQ_API_KEY)
    response = await client.chat.completions.create(
        model=GROQ_LLAMA_MODEL,
        messages=[
            {"role": "system", "content": SYSTEM_PROMPT},
            {"role": "user", "content": _build_user_prompt(text, source_lang, target_lang, previous_translation)},
        ],
        max_tokens=4096,
    )
    return response.choices[0].message.content or ""


async def translate_gemini(text: str, source_lang: str, target_lang: str, *, previous_translation: str = "") -> str:
    from google import genai
    client = genai.Client(api_key=GOOGLE_API_KEY)
    response = await client.aio.models.generate_content(
        model=GOOGLE_MODEL,
        contents=_build_user_prompt(text, source_lang, target_lang, previous_translation),
        config=genai.types.GenerateContentConfig(
            system_instruction=SYSTEM_PROMPT,
            max_output_tokens=4096,
        ),
    )
    return response.text or ""


_TRANSLATORS: dict[str, object] = {
    "claude-opus": partial(translate_claude, model=CLAUDE_OPUS_MODEL),
    "claude-sonnet": partial(translate_claude, model=CLAUDE_SONNET_MODEL),
    "gpt": translate_gpt,
    "groq": translate_groq,
    "gemini": translate_gemini,
}


async def translate(
    text: str,
    source_lang: str,
    target_lang: str,
    provider: str = "claude-opus",
    previous_translation: str = "",
) -> dict:
    if not text.strip():
        return {"translated_text": "", "provider": provider, "model": ""}

    if provider not in _TRANSLATORS:
        raise ValueError(f"Unknown provider: {provider}")

    _check_providers()
    if provider not in AVAILABLE_PROVIDERS:
        raise ValueError(f"Provider '{provider}' is not configured (missing API key)")

    translator = _TRANSLATORS[provider]
    translated = await translator(text, source_lang, target_lang, previous_translation=previous_translation)

    return {
        "translated_text": translated.strip(),
        "provider": provider,
        "model": AVAILABLE_PROVIDERS[provider],
    }


def get_available_providers() -> list[dict]:
    _check_providers()
    return [{"id": pid, "model": model} for pid, model in AVAILABLE_PROVIDERS.items()]
