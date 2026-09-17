"""Provider dispatch.

Independence comes from the reviewer not sharing a context with the writer. The
strongest form of that is a different model from a different vendor, so the
reviewer is addressed by provider name and every provider gets the identical
brief.

Anthropic is the reference implementation and the only hard dependency. Other
providers are optional and are imported only when asked for. Adding one is a
single function plus a line in the registry.
"""

from __future__ import annotations

import json
import os
from typing import Callable, TypeVar

from pydantic import BaseModel

T = TypeVar("T", bound=BaseModel)

ANTHROPIC_MODEL = os.environ.get("CREW_ANTHROPIC_MODEL", "claude-opus-5")
OPENAI_MODEL = os.environ.get("CREW_OPENAI_MODEL", "gpt-5")

MAX_TOKENS = 16000


class ProviderError(RuntimeError):
    """The provider could not be reached, or would not answer."""


def _call_anthropic(system: str, user: str, output_model: type[T]) -> T:
    try:
        import anthropic
    except ImportError as exc:  # pragma: no cover - import guard
        raise ProviderError("The anthropic package is not installed.") from exc

    client = anthropic.Anthropic()
    response = client.messages.parse(
        model=ANTHROPIC_MODEL,
        max_tokens=MAX_TOKENS,
        system=system,
        messages=[{"role": "user", "content": user}],
        output_format=output_model,
    )

    # Safety classifiers can decline with HTTP 200 and no usable content, so
    # check before reading the parsed output. Server-side fallbacks are an
    # option here if a declined review should silently route to another model.
    if getattr(response, "stop_reason", None) == "refusal":
        details = getattr(response, "stop_details", None)
        category = getattr(details, "category", None) or "unspecified"
        raise ProviderError(f"Anthropic declined this review, category {category}.")

    parsed = response.parsed_output
    if parsed is None:
        raise ProviderError("Anthropic returned no parsed output.")
    return parsed


def _call_openai(system: str, user: str, output_model: type[T]) -> T:
    try:
        from openai import OpenAI
    except ImportError as exc:
        raise ProviderError(
            "The openai package is not installed. Install it to review across vendors."
        ) from exc

    schema = json.dumps(output_model.model_json_schema(), indent=2)
    client = OpenAI()
    completion = client.chat.completions.create(
        model=OPENAI_MODEL,
        response_format={"type": "json_object"},
        messages=[
            {
                "role": "system",
                "content": f"{system}\n\nReply with JSON matching this schema:\n{schema}",
            },
            {"role": "user", "content": user},
        ],
    )
    content = completion.choices[0].message.content
    if not content:
        raise ProviderError("OpenAI returned an empty response.")
    return output_model.model_validate_json(content)


Caller = Callable[[str, str, type[BaseModel]], BaseModel]

REGISTRY: dict[str, Caller] = {
    "anthropic": _call_anthropic,
    "openai": _call_openai,
}

DEFAULT_PROVIDER = "anthropic"


def available() -> list[str]:
    return sorted(REGISTRY)


def ask(provider: str, system: str, user: str, output_model: type[T]) -> T:
    """Send one brief to one provider and return its structured answer."""
    caller = REGISTRY.get(provider)
    if caller is None:
        raise ProviderError(
            f"Unknown provider {provider!r}. Available: {', '.join(available())}."
        )
    return caller(system, user, output_model)  # type: ignore[return-value]
