"""Provider dispatch.

Independence comes from the reviewer not sharing a context with the writer. The
strongest form of that is a different model from a different vendor, so every
caller is addressed by provider name and every provider gets the identical
brief.

Anthropic is the reference implementation and the only hard dependency. Other
providers are optional and are imported only when asked for. Groq hosts
open-weight models from several families behind an OpenAI-shaped interface, so
one Groq key gives a council a second family. Adding a provider is a single
function plus a line in the registry.
"""

from __future__ import annotations

import json
import os
from typing import Callable, TypeVar

from pydantic import BaseModel

T = TypeVar("T", bound=BaseModel)

ANTHROPIC_MODEL = os.environ.get("CREW_ANTHROPIC_MODEL", "claude-opus-5")
OPENAI_MODEL = os.environ.get("CREW_OPENAI_MODEL", "gpt-5")
GROQ_MODEL = os.environ.get("CREW_GROQ_MODEL", "openai/gpt-oss-120b")
GROQ_BASE_URL = "https://api.groq.com/openai/v1"

MAX_TOKENS = 16000

KEY_VARIABLES = {
    "anthropic": "ANTHROPIC_API_KEY",
    "openai": "OPENAI_API_KEY",
    "groq": "GROQ_API_KEY",
}


class ProviderError(RuntimeError):
    """The provider could not be reached, or would not answer."""


def _call_anthropic(system: str, user: str, output_model: type[T], model: str | None = None) -> T:
    try:
        import anthropic
    except ImportError as exc:  # pragma: no cover - import guard
        raise ProviderError("The anthropic package is not installed.") from exc

    client = anthropic.Anthropic()
    response = client.messages.parse(
        model=model or ANTHROPIC_MODEL,
        max_tokens=MAX_TOKENS,
        system=system,
        messages=[{"role": "user", "content": user}],
        output_format=output_model,
    )

    # Safety classifiers can decline with HTTP 200 and no usable content, so
    # check before reading the parsed output.
    if getattr(response, "stop_reason", None) == "refusal":
        details = getattr(response, "stop_details", None)
        category = getattr(details, "category", None) or "unspecified"
        raise ProviderError(f"Anthropic declined this request, category {category}.")

    parsed = response.parsed_output
    if parsed is None:
        raise ProviderError("Anthropic returned no parsed output.")
    return parsed


def _openai_shaped(client, model: str, system: str, user: str, output_model: type[T], name: str) -> T:
    schema = json.dumps(output_model.model_json_schema(), indent=2)
    completion = client.chat.completions.create(
        model=model,
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
        raise ProviderError(f"{name} returned an empty response.")
    return output_model.model_validate_json(content)


def _openai_client(**kwargs):
    try:
        from openai import OpenAI
    except ImportError as exc:
        raise ProviderError(
            "The openai package is not installed. Install it to use OpenAI or Groq."
        ) from exc
    return OpenAI(**kwargs)


def _call_openai(system: str, user: str, output_model: type[T], model: str | None = None) -> T:
    return _openai_shaped(_openai_client(), model or OPENAI_MODEL, system, user, output_model, "OpenAI")


def _call_groq(system: str, user: str, output_model: type[T], model: str | None = None) -> T:
    key = os.environ.get("GROQ_API_KEY", "")
    if not key:
        raise ProviderError("GROQ_API_KEY is not set.")
    client = _openai_client(api_key=key, base_url=GROQ_BASE_URL)
    return _openai_shaped(client, model or GROQ_MODEL, system, user, output_model, "Groq")


Caller = Callable[[str, str, type[BaseModel], "str | None"], BaseModel]

REGISTRY: dict[str, Caller] = {
    "anthropic": _call_anthropic,
    "openai": _call_openai,
    "groq": _call_groq,
}

DEFAULT_MODELS = {
    "anthropic": ANTHROPIC_MODEL,
    "openai": OPENAI_MODEL,
    "groq": GROQ_MODEL,
}

DEFAULT_PROVIDER = "anthropic"


def available() -> list[str]:
    return sorted(REGISTRY)


def has_key(provider: str) -> bool:
    """Whether the environment holds a key for this provider."""
    return bool(os.environ.get(KEY_VARIABLES.get(provider, ""), ""))


def default_model(provider: str) -> str:
    return DEFAULT_MODELS.get(provider, "")


def ask(provider: str, system: str, user: str, output_model: type[T], model: str | None = None) -> T:
    """Send one brief to one provider and return its structured answer."""
    caller = REGISTRY.get(provider)
    if caller is None:
        raise ProviderError(
            f"Unknown provider {provider!r}. Available: {', '.join(available())}."
        )
    return caller(system, user, output_model, model)  # type: ignore[return-value]
