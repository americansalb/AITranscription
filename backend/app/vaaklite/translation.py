"""Multi-LLM translation service for Vaak Lite.

Supports Claude, GPT, Groq (Llama), and Gemini.
"""

import logging
from typing import Literal

from app.vaaklite import (
    ANTHROPIC_API_KEY, ANTHROPIC_MODEL,
    OPENAI_API_KEY, OPENAI_MODEL,
    GROQ_API_KEY, GROQ_LLAMA_MODEL,
    GOOGLE_API_KEY, GOOGLE_MODEL,
)

logger = logging.getLogger(__name__)

Provider = Literal["claude", "gpt", "groq", "gemini"]

AVAILABLE_PROVIDERS: dict[Provider, str] = {}


def _check_providers():
    global AVAILABLE_PROVIDERS
    AVAILABLE_PROVIDERS = {}
    if ANTHROPIC_API_KEY:
        AVAILABLE_PROVIDERS["claude"] = ANTHROPIC_MODEL
    if OPENAI_API_KEY:
        AVAILABLE_PROVIDERS["gpt"] = OPENAI_MODEL
    if GROQ_API_KEY:
        AVAILABLE_PROVIDERS["groq"] = GROQ_LLAMA_MODEL
    if GOOGLE_API_KEY:
        AVAILABLE_PROVIDERS["gemini"] = GOOGLE_MODEL


_check_providers()


SYSTEM_PROMPT = """You are a professional interpreter providing live translation.

RULES:
1. Translate the source text faithfully into the target language.
2. Preserve the speaker's tone, register, and intent.
3. Do NOT add commentary, notes, or explanations.
4. Do NOT censor or modify the content in any way.
5. If a word or phrase has no direct equivalent, use the closest natural expression in the target language.
6. Preserve proper nouns, technical terms, and brand names as-is unless they have an established translation.
7. Output ONLY the translated text. Nothing else."""


def _build_user_prompt(text: str, source_lang: str, target_lang: str) -> str:
    return f"Translate from {source_lang} to {target_lang}:\n\n{text}"


async def translate_claude(text: str, source_lang: str, target_lang: str) -> str:
    from anthropic import AsyncAnthropic
    client = AsyncAnthropic(api_key=ANTHROPIC_API_KEY)
    response = await client.messages.create(
        model=ANTHROPIC_MODEL,
        max_tokens=4096,
        system=SYSTEM_PROMPT,
        messages=[{"role": "user", "content": _build_user_prompt(text, source_lang, target_lang)}],
    )
    return response.content[0].text if response.content else ""


async def translate_gpt(text: str, source_lang: str, target_lang: str) -> str:
    from openai import AsyncOpenAI
    client = AsyncOpenAI(api_key=OPENAI_API_KEY)
    response = await client.chat.completions.create(
        model=OPENAI_MODEL,
        messages=[
            {"role": "system", "content": SYSTEM_PROMPT},
            {"role": "user", "content": _build_user_prompt(text, source_lang, target_lang)},
        ],
        max_tokens=4096,
    )
    return response.choices[0].message.content or ""


async def translate_groq(text: str, source_lang: str, target_lang: str) -> str:
    from groq import AsyncGroq
    client = AsyncGroq(api_key=GROQ_API_KEY)
    response = await client.chat.completions.create(
        model=GROQ_LLAMA_MODEL,
        messages=[
            {"role": "system", "content": SYSTEM_PROMPT},
            {"role": "user", "content": _build_user_prompt(text, source_lang, target_lang)},
        ],
        max_tokens=4096,
    )
    return response.choices[0].message.content or ""


async def translate_gemini(text: str, source_lang: str, target_lang: str) -> str:
    from google import genai
    client = genai.Client(api_key=GOOGLE_API_KEY)
    response = await client.aio.models.generate_content(
        model=GOOGLE_MODEL,
        contents=_build_user_prompt(text, source_lang, target_lang),
        config=genai.types.GenerateContentConfig(
            system_instruction=SYSTEM_PROMPT,
            max_output_tokens=4096,
        ),
    )
    return response.text or ""


_TRANSLATORS = {
    "claude": translate_claude,
    "gpt": translate_gpt,
    "groq": translate_groq,
    "gemini": translate_gemini,
}


async def translate(
    text: str,
    source_lang: str,
    target_lang: str,
    provider: Provider = "claude",
) -> dict:
    if not text.strip():
        return {"translated_text": "", "provider": provider, "model": ""}

    if provider not in _TRANSLATORS:
        raise ValueError(f"Unknown provider: {provider}")

    _check_providers()
    if provider not in AVAILABLE_PROVIDERS:
        raise ValueError(f"Provider '{provider}' is not configured (missing API key)")

    translator = _TRANSLATORS[provider]
    translated = await translator(text, source_lang, target_lang)

    return {
        "translated_text": translated.strip(),
        "provider": provider,
        "model": AVAILABLE_PROVIDERS[provider],
    }


def get_available_providers() -> list[dict]:
    _check_providers()
    return [{"id": pid, "model": model} for pid, model in AVAILABLE_PROVIDERS.items()]
